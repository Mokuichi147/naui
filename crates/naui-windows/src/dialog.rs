//! General-purpose dialogs (WinUI 3).
//!
//! WinUI 3's modal is the **`ContentDialog`** itself; the overlay, placement, button
//! row and cancelling with Esc are all done by WinUI. naui only passes it the title,
//! body text, content widget and button labels.
//!
//! naui's dialog has the shape "a title + body text + an optional widget + up to
//! three buttons with roles" in order to match this `ContentDialog`.
//!
//! ## Differences from other platforms
//!
//! - [`Dialog::open`] returns right away, and closing arrives through
//!   `on_response` (macOS's `NSAlert` does not return until it is closed).
//! - `ContentDialog` appears over a `XamlRoot`, so it cannot be shown **until a
//!   window has been shown**.
//! - Two dialogs cannot be shown on the same `XamlRoot` (a WinUI restriction).

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
    /// The box stacking the body text and the content widget vertically.
    content: StackPanel,
    message: TextBlock,
    child: RefCell<Option<Box<dyn Widget>>>,
    buttons: RefCell<DialogButtons>,
    on_response: RefCell<Slot<dyn FnMut(DialogResponse)>>,
    theme: Cell<Theme>,
    open: Cell<bool>,
    /// Whether it was closed with [`Dialog::close`]. A marker so that the reason for
    /// closing is not notified.
    closed_by_app: Cell<bool>,
}

/// A modal dialog (ContentDialog).
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

    /// Receives closing in one place.
    ///
    /// `ContentDialog`'s `Closed` is not in the [`naui_winui3`] projection (its event
    /// argument type is not generated). `Closing`, right before it closes, is, and the
    /// clicked button is known from its `Result`, so that one is used. It never cancels
    /// (`Cancel`), so it does not prevent closing.
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

    /// Cleanup and notification on close.
    fn finish(&self, result: ContentDialogResult) {
        self.0.open.set(false);
        if self.0.closed_by_app.replace(false) {
            return; // アプリ側から閉じた。通知はしない。
        }
        self.emit(response_for(result));
    }

    /// Notification. So that setting it again from inside the notification does not
    /// double-borrow, the closure is taken out only for the duration of the call.
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

    /// The body text shown below the title. Not shown if empty.
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

    /// The widget placed between the body text and the buttons. Each call replaces it.
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

    /// The buttons to show. By default there are none, in which case only "OK" is
    /// shown.
    pub fn set_buttons(&self, buttons: DialogButtons) {
        *self.0.buttons.borrow_mut() = buttons;
        let _ = self.apply_buttons();
    }

    pub fn buttons(&self) -> DialogButtons {
        self.0.buttons.borrow().clone()
    }

    /// Maps the buttons for each role onto the `ContentDialog`'s three slots.
    ///
    /// Cancel becomes the Close button. WinUI assigns Esc to it.
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

    /// Called with the reason when it closes. Setting it again removes the previous
    /// one.
    ///
    /// Not called when it is closed with [`Dialog::close`].
    pub fn on_response(&self, f: impl FnMut(DialogResponse) + 'static) {
        *self.0.on_response.borrow_mut() = Some(Box::new(f));
    }

    /// Shows the dialog. **It returns right away, and closing arrives through
    /// `on_response`.**
    ///
    /// `ContentDialog` appears over the window's `XamlRoot`, so **it cannot be shown
    /// before the window is shown**. Does nothing if it is already shown.
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

    /// Closes the dialog being shown. `on_response` is not called.
    pub fn close(&self) {
        if !self.0.open.get() {
            return;
        }
        self.0.closed_by_app.set(true);
        if self.0.native.Hide().is_err() {
            self.0.closed_by_app.set(false);
        }
    }

    /// Whether it is showing now.
    pub fn is_open(&self) -> bool {
        self.0.open.get()
    }

    /// Sets this dialog's color theme.
    pub(crate) fn set_theme(&self, theme: Theme) {
        self.0.theme.set(theme);
    }

    /// The real WinUI 3 dialog. A backend-specific escape hatch.
    pub fn native_dialog(&self) -> ContentDialog {
        self.0.native.clone()
    }
}

/// Maps the `ContentDialog`'s result back to the role of the clicked button.
///
/// The Close button, Esc and clicks outside all come back as `None`, so they are
/// all treated as cancel.
fn response_for(result: ContentDialogResult) -> DialogResponse {
    match result {
        ContentDialogResult::Primary => DialogResponse::Primary,
        ContentDialogResult::Secondary => DialogResponse::Secondary,
        _ => DialogResponse::Cancel,
    }
}
