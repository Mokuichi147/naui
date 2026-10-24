//! File / folder picking (a WinUI 3 button + Windows's common dialog).
//!
//! WinUI 3 has no file picking control. A `Button` is placed, and clicking it opens
//! **`IFileOpenDialog` (the Common Item Dialog)**. It is the same dialog as File
//! Explorer's, and the list, search and Quick Access are all drawn by Windows.
//!
//! The Win32 dialog is used instead of `Windows.Storage.Pickers.FileOpenPicker`
//! (WinRT) because naui does not pull in the `Windows.Storage.Pickers` projection.
//! Both are the same Common Item Dialog in look and function, and it opens in
//! unpackaged execution just by passing an HWND.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use naui_core::{FileEntry, FileFilter, FilePickerMode, Result};
use naui_winui3::Microsoft::UI::Dispatching::{DispatcherQueue, DispatcherQueueHandler};
use naui_winui3::Microsoft::UI::Xaml::Controls::{Button as XamlButton, TextBlock};
use naui_winui3::Microsoft::UI::Xaml::{RoutedEventHandler, UIElement};
use windows::Win32::System::Com::{CoCreateInstance, CoTaskMemFree, CLSCTX_INPROC_SERVER};
use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
use windows::Win32::UI::Shell::{
    FileOpenDialog, IFileOpenDialog, FILEOPENDIALOGOPTIONS, FOS_ALLOWMULTISELECT,
    FOS_FORCEFILESYSTEM, FOS_PICKFOLDERS, SIGDN_FILESYSPATH,
};
use windows_core::{Interface, HSTRING, PCWSTR};

use crate::to_error;
use crate::ui_thread::{HandlerCell, UiThreadCell};
use crate::widgets::{impl_widget, Widget};

/// The dialog's settings and what was picked last.
///
/// The click delegate is required to be `Send + Sync`, so only this cell, not the
/// handle (`Rc`), is passed to the delegate.
#[derive(Default)]
struct PickerState {
    mode: FilePickerMode,
    filters: Vec<FileFilter>,
    selection: Vec<FileEntry>,
}

#[derive(Clone)]
struct SharedState(Arc<UiThreadCell<PickerState>>);

impl SharedState {
    fn new() -> Self {
        Self(Arc::new(UiThreadCell::new(PickerState::default())))
    }
}

/// The notification receiving the list of picked files.
type SelectionCallback = dyn FnMut(&[FileEntry]);

/// The handler notified when something is picked. The closure is taken out only for
/// the duration of the call, so setting it again from inside the notification does
/// not double-borrow.
#[derive(Clone)]
struct SelectionHandler(HandlerCell<SelectionCallback>);

impl SelectionHandler {
    fn new() -> Self {
        Self(Arc::new(UiThreadCell::new(None)))
    }

    fn set(&self, f: impl FnMut(&[FileEntry]) + 'static) {
        self.0.with_mut(|slot| *slot = Some(Box::new(f)));
    }

    fn emit(&self, entries: &[FileEntry]) {
        let Some(mut f) = self.0.with_mut(|slot| slot.take()) else {
            return;
        };
        f(entries);
        self.0.with_mut(|slot| {
            if slot.is_none() {
                *slot = Some(f);
            }
        });
    }
}

struct FilePickerInner {
    native: XamlButton,
    label: TextBlock,
    state: SharedState,
    handler: SelectionHandler,
    token: RefCell<Option<i64>>,
}

/// A button for picking files or folders (Button + IFileOpenDialog).
#[derive(Clone)]
pub struct FilePicker(Rc<FilePickerInner>);
impl_widget!(FilePicker, native);

impl FilePicker {
    pub(crate) fn new(text: &str) -> Result<Self> {
        let native = XamlButton::new().map_err(|e| to_error("Button の生成", e))?;
        let label = TextBlock::new().map_err(|e| to_error("Button ラベルの生成", e))?;
        label
            .SetText(&HSTRING::from(text))
            .map_err(|e| to_error("Button ラベルの設定", e))?;
        native
            .SetContent(&label)
            .map_err(|e| to_error("Button への内容設定", e))?;

        let this = Self(Rc::new(FilePickerInner {
            native,
            label,
            state: SharedState::new(),
            handler: SelectionHandler::new(),
            token: RefCell::new(None),
        }));
        this.install_click_handler();
        Ok(this)
    }

    fn install_click_handler(&self) {
        let state = self.0.state.clone();
        let handler = self.0.handler.clone();
        let delegate = RoutedEventHandler::new(move |_sender, _args| {
            // Common Item Dialog の Show はモーダルで、Button.Click の中から
            // 直接呼ぶと、そのネストしたメッセージループが Click 中の
            // WinUI/MediaPlayerElement と衝突することがある。ダイアログを
            // 開く処理自体を Click の戻り後へ移す。
            if let Ok(queue) = DispatcherQueue::GetForCurrentThread() {
                let state = state.clone();
                let handler = handler.clone();
                let operation = DispatcherQueueHandler::new(move || {
                    show_and_report(&state, &handler);
                    Ok(())
                });
                if queue.TryEnqueue(&operation).is_ok() {
                    return Ok(());
                }
            }
            show_and_report(&state, &handler);
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

    /// Decides what to let the user pick (one file by default).
    pub fn set_mode(&self, mode: FilePickerMode) {
        self.0.state.0.with_mut(|state| state.mode = mode);
    }

    pub fn mode(&self) -> FilePickerMode {
        self.0.state.0.with_mut(|state| state.mode)
    }

    /// Filters by extension. Ignored for [`FilePickerMode::Folder`].
    pub fn set_filters(&self, filters: &[FileFilter]) {
        self.0
            .state
            .0
            .with_mut(|state| state.filters = filters.to_vec());
    }

    /// What was picked last. Empty if nothing has been picked yet.
    pub fn selection(&self) -> Vec<FileEntry> {
        self.0.state.0.with_mut(|state| state.selection.clone())
    }

    /// Called when something is picked. Not called when the user cancels. Setting it
    /// again removes the previous one.
    pub fn on_select(&self, f: impl FnMut(&[FileEntry]) + 'static) {
        self.0.handler.set(f);
    }

    /// Shows the dialog. Clicking the button calls the same thing.
    ///
    /// The Common Item Dialog is modal, so this does not return until it is closed.
    pub fn open(&self) {
        show_and_report(&self.0.state, &self.0.handler);
    }
}

/// Shows the dialog and, if something was picked, records and notifies it.
///
/// The call is made after finishing writing `selection` and releasing the borrow, so
/// the notification can touch this widget.
fn show_and_report(state: &SharedState, handler: &SelectionHandler) {
    let (mode, filters) = state
        .0
        .with_mut(|state| (state.mode, state.filters.clone()));
    let Some(entries) = show_dialog(mode, &filters) else {
        return; // 取り消された、またはダイアログを出せなかった。
    };
    state.0.with_mut(|state| state.selection = entries.clone());

    // IFileOpenDialog.Show は Button.Click の処理中にモーダルに動く。
    // その直後に MediaPlayerElement の Source を同期設定すると、WinUI の
    // MediaPlayerPresenter が再入状態になり、動画・音声だけ stowed exception
    // (0xc000027b) でプロセスを終了させることがある。選択通知を次の UI tick
    // へ送って、ダイアログと Click イベントを完全に抜けてから呼び出す。
    let Ok(queue) = DispatcherQueue::GetForCurrentThread() else {
        handler.emit(&entries);
        return;
    };
    let handler = handler.clone();
    let _ = queue.TryEnqueue(&DispatcherQueueHandler::new(move || {
        handler.emit(&entries);
        Ok(())
    }));
}

/// Opens the Common Item Dialog and returns the chosen paths. `None` if cancelled.
fn show_dialog(mode: FilePickerMode, filters: &[FileFilter]) -> Option<Vec<FileEntry>> {
    unsafe {
        let dialog: IFileOpenDialog =
            CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;

        // 既定の指定を残したまま、必要なものだけ足す。
        let mut options = dialog.GetOptions().unwrap_or(FILEOPENDIALOGOPTIONS(0));
        options |= FOS_FORCEFILESYSTEM; // 実体のあるパスだけを返させる。
        if mode.is_folder() {
            options |= FOS_PICKFOLDERS;
        }
        if mode.allows_multiple() {
            options |= FOS_ALLOWMULTISELECT;
        }
        dialog.SetOptions(options).ok()?;

        // 種類欄。ダイアログが読む間、文字列を生かしておく必要がある。
        let mut buffers: Vec<(Vec<u16>, Vec<u16>)> = Vec::new();
        if !mode.is_folder() {
            for filter in filters.iter().filter(|f| !f.is_empty()) {
                buffers.push((wide(filter.label()), wide(&filter.glob_pattern())));
            }
        }
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
        dialog.Show(crate::window::owner_hwnd()).ok()?;

        let items = dialog.GetResults().ok()?;
        let count = items.GetCount().unwrap_or(0);
        let mut entries = Vec::with_capacity(count as usize);
        for index in 0..count {
            let Ok(item) = items.GetItemAt(index) else {
                continue;
            };
            let Ok(raw) = item.GetDisplayName(SIGDN_FILESYSPATH) else {
                continue;
            };
            if let Ok(path) = raw.to_string() {
                entries.push(FileEntry::from_path(path));
            }
            CoTaskMemFree(Some(raw.0 as *const std::ffi::c_void));
        }
        if entries.is_empty() {
            return None;
        }
        Some(entries)
    }
}

/// Converts to UTF-16 with a terminator at the end (the form of strings passed to
/// COM).
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}
