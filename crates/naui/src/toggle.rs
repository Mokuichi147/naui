use crate::sys;

/// 入り切りを切り替えるスイッチ (`GtkSwitch`)。
#[derive(Clone)]
pub struct Toggle(pub(crate) sys::Toggle);

impl_widget!(Toggle);

impl Toggle {
    /// 入っているかどうか。
    #[inline]
    pub fn is_on(&self) -> bool {
        self.0.is_on()
    }

    /// プログラムから切り替える。`on_toggle` は呼ばれない。
    #[inline]
    pub fn set_on(&self, on: bool) {
        self.0.set_on(on)
    }

    #[inline]
    pub fn set_enabled(&self, enabled: bool) {
        self.0.set_enabled(enabled)
    }

    /// 利用者が切り替えるたびに、切り替えた後の状態で呼ばれる。
    #[inline]
    pub fn on_toggle(&self, f: impl FnMut(bool) + 'static) {
        self.0.on_toggle(f)
    }
}
