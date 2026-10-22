use crate::sys;
use crate::Widget;

/// 見出しを押して中身を出し入れするコンテナ (`GtkExpander`)。
#[derive(Clone)]
pub struct Expander(pub(crate) sys::Expander);

impl_widget!(Expander);

impl Expander {
    /// 見出しの文字。
    #[inline]
    pub fn text(&self) -> String {
        self.0.text()
    }

    #[inline]
    pub fn set_text(&self, text: &str) {
        self.0.set_text(text)
    }

    /// 開いているかどうか。
    #[inline]
    pub fn is_expanded(&self) -> bool {
        self.0.is_expanded()
    }

    /// プログラムから開閉する。`on_toggle` は呼ばれない。
    #[inline]
    pub fn set_expanded(&self, expanded: bool) {
        self.0.set_expanded(expanded)
    }

    #[inline]
    pub fn set_enabled(&self, enabled: bool) {
        self.0.set_enabled(enabled)
    }

    /// 折りたたむ中身。呼ぶたびに置き換わる。
    #[inline]
    pub fn set_child(&self, child: &dyn Widget) {
        self.0.set_child(child.as_backend())
    }

    /// 利用者が開閉するたびに、変わった後の状態で呼ばれる。
    #[inline]
    pub fn on_toggle(&self, f: impl FnMut(bool) + 'static) {
        self.0.on_toggle(f)
    }
}
