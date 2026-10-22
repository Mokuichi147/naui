use crate::sys;
use naui_core::Orientation;

/// 選択肢を並べて 1 つだけ選ばせるラジオグループ。
///
/// 作った直後と [`set_items`](Self::set_items) の直後は、何も選ばれていない。
#[derive(Clone)]
pub struct RadioGroup(pub(crate) sys::RadioGroup);

impl_widget!(RadioGroup);

impl RadioGroup {
    /// 選択肢を作り直し、選択を外す。選択通知は発生しない。
    #[inline]
    pub fn set_items<S: AsRef<str>>(&self, items: &[S]) {
        self.0.set_items(items)
    }

    /// 選択肢の数。
    #[inline]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// 現在選ばれている選択肢。未選択なら `None`。
    #[inline]
    pub fn selected(&self) -> Option<usize> {
        self.0.selected()
    }

    /// 範囲内の選択肢を通知せずに選ぶ。範囲外なら何もしない。
    #[inline]
    pub fn set_selected(&self, index: usize) {
        self.0.set_selected(index)
    }

    /// 選択を通知せずに外す。
    #[inline]
    pub fn clear_selection(&self) {
        self.0.clear_selection()
    }

    /// ユーザーが選んだのと同じように、範囲内の選択肢を選んで通知する。
    #[inline]
    pub fn select(&self, index: usize) {
        self.0.select(index)
    }

    /// 選択肢の並ぶ向き。既定は縦。
    #[inline]
    pub fn set_orientation(&self, orientation: Orientation) {
        self.0.set_orientation(orientation)
    }

    #[inline]
    pub fn set_enabled(&self, enabled: bool) {
        self.0.set_enabled(enabled)
    }

    /// 選択肢が選ばれたときに、そのインデックスで呼ばれる。
    /// 設定し直すと以前のコールバックは外れる。
    #[inline]
    pub fn on_select(&self, f: impl FnMut(usize) + 'static) {
        self.0.on_select(f)
    }
}
