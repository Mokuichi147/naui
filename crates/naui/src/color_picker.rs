use crate::sys;
use naui_core::Color;

/// 色を選ばせるコントロール (`GtkColorDialogButton`)。
///
/// 作った直後の値は黒 ([`Color::BLACK`])。
#[derive(Clone)]
pub struct ColorPicker(pub(crate) sys::ColorPicker);

impl_widget!(ColorPicker);

impl ColorPicker {
    /// いま選ばれている色。
    #[inline]
    pub fn value(&self) -> Color {
        self.0.value()
    }

    /// プログラムから色を差し替える。`on_change` は呼ばれない。
    #[inline]
    pub fn set_value(&self, color: Color) {
        self.0.set_value(color)
    }

    /// 利用者が選んだのと同じ経路で色を決め、1 回通知する。
    #[inline]
    pub fn pick(&self, color: Color) {
        self.0.pick(color)
    }

    #[inline]
    pub fn set_enabled(&self, enabled: bool) {
        self.0.set_enabled(enabled)
    }

    /// 色が変わるたびに、変わった後の色で呼ばれる。
    #[inline]
    pub fn on_change(&self, f: impl FnMut(Color) + 'static) {
        self.0.on_change(f)
    }
}
