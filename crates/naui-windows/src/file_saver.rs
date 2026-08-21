//! ファイルの保存 (WinUI 3 のボタン + Windows の共通ダイアログ)。
//!
//! [`crate::FilePicker`] と対になるもので、`Button` を押すと
//! **`IFileSaveDialog` (Common Item Dialog)** を開く。エクスプローラーと
//! 同じダイアログで、フォルダーの移動・上書きの確認はすべて Windows が行う。
//! 選ばれた場所へ [`FileSaver::set_contents`] のバイト列を書くのは naui 側。

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use naui_core::{default_extension, with_default_extension, Error, FileEntry, FileFilter, Result};
use windows::Win32::Foundation::ERROR_CANCELLED;
use windows::Win32::System::Com::{CoCreateInstance, CoTaskMemFree, CLSCTX_INPROC_SERVER};
use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
use windows::Win32::UI::Shell::{
    FileSaveDialog, IFileSaveDialog, FILEOPENDIALOGOPTIONS, FOS_FORCEFILESYSTEM,
    FOS_OVERWRITEPROMPT, SIGDN_FILESYSPATH,
};
use windows_core::{Interface, HRESULT, HSTRING, PCWSTR};
use winui3::Microsoft::UI::Dispatching::{DispatcherQueue, DispatcherQueueHandler};
use winui3::Microsoft::UI::Xaml::Controls::{Button as XamlButton, TextBlock};
use winui3::Microsoft::UI::Xaml::{RoutedEventHandler, UIElement};

use crate::to_error;
use crate::ui_thread::UiThreadCell;
use crate::widgets::{impl_widget, Widget};

/// ダイアログの設定と、最後に書き出した先。
///
/// クリックのデリゲートは `Send + Sync` を要求されるため、
/// ハンドル (`Rc`) ではなくこのセルだけをデリゲートへ渡す。
#[derive(Default)]
struct SaverState {
    file_name: String,
    filters: Vec<FileFilter>,
    contents: Vec<u8>,
    destination: Option<FileEntry>,
}

#[derive(Clone)]
struct SharedState(Arc<UiThreadCell<SaverState>>);

impl SharedState {
    fn new() -> Self {
        Self(Arc::new(UiThreadCell::new(SaverState::default())))
    }
}

/// 差し替え可能なクロージャ 1 本。呼び出しの間だけ取り出すので、
/// 通知の中から設定し直しても二重借用にならない。
struct Handler<T: ?Sized>(Arc<UiThreadCell<Option<Box<dyn FnMut(&T)>>>>);

impl<T: ?Sized> Clone for Handler<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T: ?Sized> Handler<T> {
    fn new() -> Self {
        Self(Arc::new(UiThreadCell::new(None)))
    }

    fn set(&self, f: impl FnMut(&T) + 'static) {
        self.0.with_mut(|slot| *slot = Some(Box::new(f)));
    }

    fn emit(&self, value: &T) {
        let Some(mut f) = self.0.with_mut(|slot| slot.take()) else {
            return;
        };
        f(value);
        self.0.with_mut(|slot| {
            if slot.is_none() {
                *slot = Some(f);
            }
        });
    }
}

struct FileSaverInner {
    native: XamlButton,
    label: TextBlock,
    state: SharedState,
    on_save: Handler<FileEntry>,
    on_error: Handler<Error>,
    token: RefCell<Option<i64>>,
}

/// 内容をファイルへ書き出させるボタン (Button + IFileSaveDialog)。
#[derive(Clone)]
pub struct FileSaver(Rc<FileSaverInner>);
impl_widget!(FileSaver, native);

impl FileSaver {
    pub(crate) fn new(text: &str) -> Result<Self> {
        let native = XamlButton::new().map_err(|e| to_error("Button の生成", e))?;
        let label = TextBlock::new().map_err(|e| to_error("Button ラベルの生成", e))?;
        label
            .SetText(&HSTRING::from(text))
            .map_err(|e| to_error("Button ラベルの設定", e))?;
        native
            .SetContent(&label)
            .map_err(|e| to_error("Button への内容設定", e))?;

        let this = Self(Rc::new(FileSaverInner {
            native,
            label,
            state: SharedState::new(),
            on_save: Handler::new(),
            on_error: Handler::new(),
            token: RefCell::new(None),
        }));
        this.install_click_handler();
        Ok(this)
    }

    fn install_click_handler(&self) {
        let state = self.0.state.clone();
        let on_save = self.0.on_save.clone();
        let on_error = self.0.on_error.clone();
        let delegate = RoutedEventHandler::new(move |_sender, _args| {
            // Common Item Dialog の Show はモーダルで、Button.Click の中から
            // 直接呼ぶと、そのネストしたメッセージループが Click 中の WinUI と
            // 衝突することがある。ダイアログを開く処理自体を Click の戻り後へ移す
            // (`FilePicker` と同じ)。
            if let Ok(queue) = DispatcherQueue::GetForCurrentThread() {
                let state = state.clone();
                let on_save = on_save.clone();
                let on_error = on_error.clone();
                let operation = DispatcherQueueHandler::new(move || {
                    show_and_report(&state, &on_save, &on_error);
                    Ok(())
                });
                if queue.TryEnqueue(&operation).is_ok() {
                    return Ok(());
                }
            }
            show_and_report(&state, &on_save, &on_error);
            Ok(())
        });
        if let Ok(token) = self.0.native.Click(&delegate) {
            *self.0.token.borrow_mut() = Some(token);
        }
    }

    pub fn set_text(&self, text: &str) {
        let _ = self.0.label.SetText(&HSTRING::from(text));
    }

    pub fn set_enabled(&self, enabled: bool) {
        let _ = self.0.native.SetIsEnabled(enabled);
    }

    /// ダイアログに最初から入れておく名前。空なら Windows の既定に任せる。
    pub fn set_file_name(&self, name: &str) {
        self.0
            .state
            .0
            .with_mut(|state| state.file_name = name.to_string());
    }

    pub fn file_name(&self) -> String {
        self.0.state.0.with_mut(|state| state.file_name.clone())
    }

    /// 種類の絞り込み。先頭の拡張子が既定の拡張子になる。
    pub fn set_filters(&self, filters: &[FileFilter]) {
        self.0
            .state
            .0
            .with_mut(|state| state.filters = filters.to_vec());
    }

    /// 書き出す内容。保存のたびに、このバイト列がそのまま書かれる。
    pub fn set_contents(&self, contents: &[u8]) {
        self.0
            .state
            .0
            .with_mut(|state| state.contents = contents.to_vec());
    }

    /// 書き出す内容の大きさ (バイト数)。
    pub fn contents_len(&self) -> usize {
        self.0.state.0.with_mut(|state| state.contents.len())
    }

    /// 最後に書き出した先。まだ保存していなければ `None`。
    pub fn destination(&self) -> Option<FileEntry> {
        self.0.state.0.with_mut(|state| state.destination.clone())
    }

    /// 書き出しに成功したときに呼ばれる。取り消したときは呼ばれない。
    pub fn on_save(&self, f: impl FnMut(&FileEntry) + 'static) {
        self.0.on_save.set(f);
    }

    /// 書き出しに失敗したときに呼ばれる (書き込み権限が無い、など)。
    pub fn on_error(&self, f: impl FnMut(&Error) + 'static) {
        self.0.on_error.set(f);
    }

    /// ダイアログを出す。ボタンを押したときにも同じものが呼ばれる。
    ///
    /// Common Item Dialog はモーダルなので、閉じられるまで戻らない。
    pub fn open(&self) {
        show_and_report(&self.0.state, &self.0.on_save, &self.0.on_error);
    }
}

/// ダイアログの結果。取り消しは失敗と分ける。
enum Outcome {
    Chosen(PathBuf),
    Cancelled,
    Failed(Error),
}

/// ダイアログを出し、選ばれていれば書き出して通知する。
///
/// 通知の中からこのウィジェットを触れるよう、状態の書き込みを終えて
/// 借用を手放してから呼ぶ。
fn show_and_report(state: &SharedState, on_save: &Handler<FileEntry>, on_error: &Handler<Error>) {
    let (file_name, filters, contents) = state.0.with_mut(|state| {
        (
            state.file_name.clone(),
            state.filters.clone(),
            state.contents.clone(),
        )
    });
    let path = match show_dialog(&file_name, &filters) {
        Outcome::Chosen(path) => path,
        Outcome::Cancelled => return,
        Outcome::Failed(e) => return notify(on_error, e),
    };
    let entry = match write_contents(&path, &contents) {
        Ok(entry) => entry,
        Err(e) => return notify(on_error, e),
    };
    state
        .0
        .with_mut(|state| state.destination = Some(entry.clone()));
    notify(on_save, entry);
}

/// 通知を次の UI tick へ送ってから呼ぶ。
///
/// `IFileSaveDialog::Show` は Button.Click の処理中にモーダルで動く。その直後に
/// 同期で WinUI を触ると再入状態になることがあるため、ダイアログと Click
/// イベントを完全に抜けてから呼び出す (`FilePicker` と同じ)。
fn notify<T: Send + 'static>(handler: &Handler<T>, value: T) {
    let Ok(queue) = DispatcherQueue::GetForCurrentThread() else {
        handler.emit(&value);
        return;
    };
    let handler = handler.clone();
    let _ = queue.TryEnqueue(&DispatcherQueueHandler::new(move || {
        handler.emit(&value);
        Ok(())
    }));
}

/// Common Item Dialog の保存を開いて、選ばれたパスを返す。
fn show_dialog(file_name: &str, filters: &[FileFilter]) -> Outcome {
    unsafe {
        let dialog: IFileSaveDialog =
            match CoCreateInstance(&FileSaveDialog, None, CLSCTX_INPROC_SERVER) {
                Ok(dialog) => dialog,
                Err(e) => return Outcome::Failed(to_error("保存ダイアログの生成", e)),
            };

        // 既定の指定を残したまま、必要なものだけ足す。
        let mut options = dialog.GetOptions().unwrap_or(FILEOPENDIALOGOPTIONS(0));
        options |= FOS_FORCEFILESYSTEM; // 実体のあるパスだけを返させる。
        options |= FOS_OVERWRITEPROMPT; // 上書きの確認は Windows に任せる。
        let _ = dialog.SetOptions(options);

        let name = with_default_extension(file_name, filters);
        if !name.is_empty() {
            let _ = dialog.SetFileName(&HSTRING::from(name.as_str()));
        }
        if let Some(extension) = default_extension(filters) {
            // 種類を切り替えたときに付け替わる拡張子。
            let _ = dialog.SetDefaultExtension(&HSTRING::from(extension));
        }

        // 種類欄。ダイアログが読む間、文字列を生かしておく必要がある。
        let buffers: Vec<(Vec<u16>, Vec<u16>)> = filters
            .iter()
            .filter(|f| !f.is_empty())
            .map(|filter| (wide(filter.label()), wide(&filter.glob_pattern())))
            .collect();
        if !buffers.is_empty() {
            let specs: Vec<COMDLG_FILTERSPEC> = buffers
                .iter()
                .map(|(name, spec)| COMDLG_FILTERSPEC {
                    pszName: PCWSTR(name.as_ptr()),
                    pszSpec: PCWSTR(spec.as_ptr()),
                })
                .collect();
            let _ = dialog.SetFileTypes(&specs);
        }

        // 取り消しも `Show` のエラーとして返る。
        if let Err(e) = dialog.Show(crate::window::owner_hwnd()) {
            if e.code() == HRESULT::from_win32(ERROR_CANCELLED.0) {
                return Outcome::Cancelled;
            }
            return Outcome::Failed(to_error("保存ダイアログの表示", e));
        }

        let item = match dialog.GetResult() {
            Ok(item) => item,
            Err(e) => return Outcome::Failed(to_error("保存先の取得", e)),
        };
        let Ok(raw) = item.GetDisplayName(SIGDN_FILESYSPATH) else {
            return Outcome::Failed(Error::new("保存先の取得", "パスを取れませんでした"));
        };
        let path = raw.to_string();
        CoTaskMemFree(Some(raw.0 as *const std::ffi::c_void));
        match path {
            Ok(path) => Outcome::Chosen(PathBuf::from(path)),
            Err(_) => Outcome::Failed(Error::new("保存先の取得", "パスを読めませんでした")),
        }
    }
}

/// 選ばれた場所へ書き出し、そこを指す [`FileEntry`] を返す。
fn write_contents(path: &Path, contents: &[u8]) -> Result<FileEntry> {
    std::fs::write(path, contents).map_err(|e| Error::new("ファイルの書き出し", e.to_string()))?;
    Ok(FileEntry::from_path(path))
}

/// 末尾に終端を付けた UTF-16 にする (COM へ渡す文字列の形)。
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}
