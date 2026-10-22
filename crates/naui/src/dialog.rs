use crate::sys;
use crate::Widget;
use naui_core::{DialogButtons, DialogResponse};

/// モーダルダイアログ。
///
/// ウィジェットではないので、コンテナへは入れない (`Window` と同じ)。
#[derive(Clone)]
pub struct Dialog(pub(crate) sys::Dialog);

impl_backend!(Dialog);

impl Dialog {
    #[inline]
    pub fn set_title(&self, title: &str) {
        self.0.set_title(title)
    }

    #[inline]
    pub fn title(&self) -> String {
        self.0.title()
    }

    #[inline]
    pub fn set_message(&self, message: &str) {
        self.0.set_message(message)
    }

    #[inline]
    pub fn message(&self) -> String {
        self.0.message()
    }

    /// 本文の下に置くウィジェット。
    #[inline]
    pub fn set_child(&self, child: &dyn Widget) {
        self.0.set_child(child.as_backend())
    }

    /// 出すボタン。1 つも指定しなければ「OK」だけになる。
    #[inline]
    pub fn set_buttons(&self, buttons: DialogButtons) {
        self.0.set_buttons(buttons)
    }

    #[inline]
    pub fn buttons(&self) -> DialogButtons {
        self.0.buttons()
    }

    /// 閉じたときに、その理由で呼ばれる。
    #[inline]
    pub fn on_response(&self, f: impl FnMut(DialogResponse) + 'static) {
        self.0.on_response(f)
    }

    /// 開く。すでに開いていれば何もしない。
    #[inline]
    pub fn open(&self) {
        self.0.open()
    }

    /// 閉じる。開いていなければ何もしない。
    #[inline]
    pub fn close(&self) {
        self.0.close()
    }

    #[inline]
    pub fn is_open(&self) -> bool {
        self.0.is_open()
    }
}
