use crate::sys;
use crate::Widget;
use naui_core::PopupItem;

/// 右クリックで出るポップアップ (コンテキスト) メニュー。
///
/// 画面に並ぶウィジェットではないので [`Widget`] ではない。
#[derive(Clone)]
pub struct PopupMenu(pub(crate) sys::PopupMenu);

impl_backend!(PopupMenu);

impl PopupMenu {
    /// 項目を作り直す。インデックスは**区切り線を含めた並び**で数える。
    #[inline]
    pub fn set_items(&self, items: &[PopupItem]) {
        self.0.set_items(items)
    }

    /// 区切り線を含めた項目数。
    #[inline]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// 右クリックでこのメニューが出るようにする。いくつでも取り付けられる。
    #[inline]
    pub fn attach(&self, widget: &dyn Widget) {
        self.0.attach(widget.as_backend())
    }

    /// プログラムから出す。位置は `widget` の左上からの論理ピクセル。
    #[inline]
    pub fn open_at(&self, widget: &dyn Widget, x: f64, y: f64) {
        self.0.open_at(widget.as_backend(), x, y)
    }

    #[inline]
    pub fn close(&self) {
        self.0.close()
    }

    /// ユーザーが選んだのと同じ経路で通知する。
    ///
    /// 区切り線と選べない項目は無視する。
    #[inline]
    pub fn select(&self, index: usize) {
        self.0.select(index)
    }

    /// 項目が選ばれたときに、そのインデックスで呼ばれる。
    #[inline]
    pub fn on_select(&self, f: impl FnMut(usize) + 'static) {
        self.0.on_select(f)
    }
}
