//! 汎用ダイアログ (WinUI 3)。
//!
//! WinUI 3 のモーダルは **`ContentDialog`** そのもので、暗幕・配置・
//! ボタン列・Esc での取り消しはすべて WinUI が行う。naui はここへ
//! 見出し・本文・中身のウィジェット・ボタンの文字列を渡すだけ。
//!
//! naui のダイアログが「見出し + 本文 + 任意のウィジェット + 役割つきの
//! ボタン 3 つまで」という形なのは、この `ContentDialog` に合わせたため。
//!
//! ## 他の環境との違い
//!
//! - [`Dialog::open`] はすぐ戻り、閉じたことは `on_response` で届く
//!   (macOS の `NSAlert` は閉じるまで戻らない)。
//! - `ContentDialog` は `XamlRoot` の上に出るため、**ウィンドウを 1 つ
//!   表示してから**でないと出せない。
//! - 同じ `XamlRoot` に 2 つのダイアログは出せない (WinUI の制約)。

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use naui_core::{DialogButtons, DialogResponse, Result, Theme};
use naui_winui3::Microsoft::UI::Xaml::Controls::{
    ContentDialog, ContentDialogButton, ContentDialogClosingEventArgs, ContentDialogResult,
    Orientation as XamlOrientation, StackPanel, TextBlock,
};
use naui_winui3::Microsoft::UI::Xaml::{UIElement, Visibility};
use windows::Foundation::TypedEventHandler;
use windows_core::{Interface, HSTRING};

use crate::to_error;
use crate::ui_thread::UiThreadCell;
use crate::widgets::Widget;
use crate::window::{owner_xaml_root, set_theme_on_element};
use crate::Slot;

struct DialogInner {
    native: ContentDialog,
    title: TextBlock,
    /// 本文と中身のウィジェットを縦に積む箱。
    content: StackPanel,
    message: TextBlock,
    child: RefCell<Option<Box<dyn Widget>>>,
    buttons: RefCell<DialogButtons>,
    on_response: RefCell<Slot<dyn FnMut(DialogResponse)>>,
    theme: Cell<Theme>,
    open: Cell<bool>,
    /// [`Dialog::close`] で閉じたか。閉じた理由を通知しないための目印。
    closed_by_app: Cell<bool>,
}

/// モーダルダイアログ (ContentDialog)。
#[derive(Clone)]
pub struct Dialog(Rc<DialogInner>);

impl Dialog {
    pub(crate) fn new(title: &str, theme: Theme) -> Result<Self> {
        let native = ContentDialog::new().map_err(|e| to_error("ContentDialog の生成", e))?;
        let title_block = TextBlock::new().map_err(|e| to_error("ダイアログ見出しの生成", e))?;
        title_block
            .SetText(&HSTRING::from(title))
            .map_err(|e| to_error("ダイアログ見出しの設定", e))?;
        native
            .SetTitle(&title_block)
            .map_err(|e| to_error("ダイアログ見出しの適用", e))?;

        let content = StackPanel::new().map_err(|e| to_error("ダイアログ中身の生成", e))?;
        content
            .SetOrientation(XamlOrientation::Vertical)
            .map_err(|e| to_error("ダイアログ中身の向き設定", e))?;
        content
            .SetSpacing(12.0)
            .map_err(|e| to_error("ダイアログ中身の間隔設定", e))?;
        let message = TextBlock::new().map_err(|e| to_error("ダイアログ本文の生成", e))?;
        message
            .SetVisibility(Visibility::Collapsed)
            .map_err(|e| to_error("ダイアログ本文の表示設定", e))?;
        content
            .Children()
            .map_err(|e| to_error("ダイアログ中身の子取得", e))?
            .Append(&message)
            .map_err(|e| to_error("ダイアログ本文の配置", e))?;
        native
            .SetContent(&content)
            .map_err(|e| to_error("ダイアログ中身の適用", e))?;

        let this = Self(Rc::new(DialogInner {
            native,
            title: title_block,
            content,
            message,
            child: RefCell::new(None),
            buttons: RefCell::new(DialogButtons::new()),
            on_response: RefCell::new(None),
            theme: Cell::new(theme),
            open: Cell::new(false),
            closed_by_app: Cell::new(false),
        }));
        this.apply_buttons()?;
        this.install_closing_handler()?;
        Ok(this)
    }

    /// 閉じることをまとめて受ける。
    ///
    /// `ContentDialog` の `Closed` は [`naui_winui3`] の投影に無い
    /// (イベント引数の型が生成されていない)。閉じる直前の `Closing` は
    /// あり、押されたボタンが `Result` で分かるので、こちらを使う。
    /// 取り消し (`Cancel`) はしないので、閉じるのを妨げない。
    fn install_closing_handler(&self) -> Result<()> {
        // WinRT のデリゲートは Send + Sync を要求するが、XAML のイベントは
        // 必ず UI スレッドで呼ばれる。UiThreadCell に載せて渡す。
        let weak = UiThreadCell::new(Rc::downgrade(&self.0));
        let handler = TypedEventHandler::<ContentDialog, ContentDialogClosingEventArgs>::new(
            move |_sender, args| {
                let result = args
                    .as_ref()
                    .and_then(|args| args.Result().ok())
                    .unwrap_or(ContentDialogResult::None);
                weak.with_mut(|weak: &mut Weak<DialogInner>| {
                    if let Some(inner) = weak.upgrade() {
                        Dialog(inner).finish(result);
                    }
                });
                Ok(())
            },
        );
        self.0
            .native
            .Closing(&handler)
            .map_err(|e| to_error("ダイアログの終了通知の購読", e))?;
        Ok(())
    }

    /// 閉じたときの後始末と通知。
    fn finish(&self, result: ContentDialogResult) {
        self.0.open.set(false);
        if self.0.closed_by_app.replace(false) {
            return; // アプリ側から閉じた。通知はしない。
        }
        self.emit(response_for(result));
    }

    /// 通知。通知の中から設定し直しても二重借用にならないよう、
    /// 呼び出しの間だけクロージャを取り出す。
    fn emit(&self, response: DialogResponse) {
        let Some(mut f) = self.0.on_response.borrow_mut().take() else {
            return;
        };
        f(response);
        let mut slot = self.0.on_response.borrow_mut();
        if slot.is_none() {
            *slot = Some(f);
        }
    }

    pub fn set_title(&self, title: &str) {
        let _ = self.0.title.SetText(&HSTRING::from(title));
    }

    pub fn title(&self) -> String {
        self.0
            .title
            .Text()
            .map(|s| s.to_string())
            .unwrap_or_default()
    }

    /// 見出しの下に出る本文。空にすると出ない。
    pub fn set_message(&self, message: &str) {
        let _ = self.0.message.SetText(&HSTRING::from(message));
        let _ = self.0.message.SetVisibility(if message.is_empty() {
            Visibility::Collapsed
        } else {
            Visibility::Visible
        });
    }

    pub fn message(&self) -> String {
        self.0
            .message
            .Text()
            .map(|s| s.to_string())
            .unwrap_or_default()
    }

    /// 本文とボタンの間に置くウィジェット。呼ぶたびに置き換わる。
    pub fn set_child(&self, child: &dyn Widget) {
        let Ok(children) = self.0.content.Children() else {
            return;
        };
        // 本文だけを残して、前の中身を外す。
        let _ = children.Clear();
        let _ = children.Append(&self.0.message);
        let element = child.native_element();
        if children.Append(&element).is_ok() {
            *self.0.child.borrow_mut() = Some(child.boxed_clone());
        }
    }

    /// 出すボタン。既定ではボタンを持たず、そのときは「OK」だけが出る。
    pub fn set_buttons(&self, buttons: DialogButtons) {
        *self.0.buttons.borrow_mut() = buttons;
        let _ = self.apply_buttons();
    }

    pub fn buttons(&self) -> DialogButtons {
        self.0.buttons.borrow().clone()
    }

    /// 役割ごとのボタンを `ContentDialog` の 3 つの枠へ写す。
    ///
    /// 取り消しは Close ボタンになる。WinUI はここへ Esc を割り当てる。
    fn apply_buttons(&self) -> Result<()> {
        let buttons = self.0.buttons.borrow().resolved();
        let text = |response| HSTRING::from(buttons.label(response).unwrap_or(""));
        self.0
            .native
            .SetPrimaryButtonText(&text(DialogResponse::Primary))
            .map_err(|e| to_error("ダイアログの主ボタン設定", e))?;
        self.0
            .native
            .SetSecondaryButtonText(&text(DialogResponse::Secondary))
            .map_err(|e| to_error("ダイアログの副ボタン設定", e))?;
        self.0
            .native
            .SetCloseButtonText(&text(DialogResponse::Cancel))
            .map_err(|e| to_error("ダイアログの取り消しボタン設定", e))?;
        // 主となる操作があれば、それを既定のボタン (Enter) にする。
        let default = if buttons.label(DialogResponse::Primary).is_some() {
            ContentDialogButton::Primary
        } else {
            ContentDialogButton::Close
        };
        self.0
            .native
            .SetDefaultButton(default)
            .map_err(|e| to_error("ダイアログの既定ボタン設定", e))
    }

    /// 閉じたときに、閉じた理由で呼ばれる。設定し直すと以前のものは外れる。
    ///
    /// [`Dialog::close`] で閉じたときは呼ばれない。
    pub fn on_response(&self, f: impl FnMut(DialogResponse) + 'static) {
        *self.0.on_response.borrow_mut() = Some(Box::new(f));
    }

    /// ダイアログを出す。**すぐ戻り、閉じたことは `on_response` で届く。**
    ///
    /// `ContentDialog` はウィンドウの `XamlRoot` の上に出るため、
    /// **まだウィンドウを表示していないと出せない**。すでに出ているときは
    /// 何もしない。
    pub fn open(&self) {
        if self.0.open.get() {
            return;
        }
        if let Err(error) = self.show() {
            eprintln!("naui-windows: ダイアログを出せませんでした: {error}");
            return;
        }
        self.0.open.set(true);
        self.0.closed_by_app.set(false);
    }

    fn show(&self) -> Result<()> {
        let root = owner_xaml_root().ok_or_else(|| {
            naui_core::Error::new(
                "ダイアログの表示",
                "先に window.show() でウィンドウを表示してください",
            )
        })?;
        self.0
            .native
            .SetXamlRoot(&root)
            .map_err(|e| to_error("ダイアログの XamlRoot 設定", e))?;
        // ダイアログはウィンドウの中身とは別の層に出るため、
        // テーマを引き継がない。ここで同じものを指定しておく。
        let element = self
            .0
            .native
            .cast::<UIElement>()
            .map_err(|e| to_error("ダイアログ要素への変換", e))?;
        set_theme_on_element(&element, self.0.theme.get())?;
        // 戻り値の非同期操作は使わない。閉じたことは `Closing` で受ける。
        self.0
            .native
            .ShowAsync()
            .map_err(|e| to_error("ダイアログの表示", e))?;
        Ok(())
    }

    /// 出しているダイアログを閉じる。`on_response` は呼ばれない。
    pub fn close(&self) {
        if !self.0.open.get() {
            return;
        }
        self.0.closed_by_app.set(true);
        if self.0.native.Hide().is_err() {
            self.0.closed_by_app.set(false);
        }
    }

    /// いま出ているか。
    pub fn is_open(&self) -> bool {
        self.0.open.get()
    }

    /// このダイアログの配色テーマを指定する。
    pub(crate) fn set_theme(&self, theme: Theme) {
        self.0.theme.set(theme);
    }

    /// WinUI 3 の実ダイアログ。バックエンド固有の脱出口。
    pub fn native_dialog(&self) -> ContentDialog {
        self.0.native.clone()
    }
}

/// `ContentDialog` の結果を、押されたボタンの役割へ戻す。
///
/// Close ボタン・Esc・領域外の押しはどれも `None` で返るので、
/// まとめて取り消し扱いにする。
fn response_for(result: ContentDialogResult) -> DialogResponse {
    match result {
        ContentDialogResult::Primary => DialogResponse::Primary,
        ContentDialogResult::Secondary => DialogResponse::Secondary,
        _ => DialogResponse::Cancel,
    }
}
