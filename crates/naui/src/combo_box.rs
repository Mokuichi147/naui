use crate::sys;

/// 1 項目を選ぶドロップダウン。
#[derive(Clone)]
pub struct ComboBox(pub(crate) sys::ComboBox);

impl_widget!(ComboBox);

impl ComboBox {
    /// 項目を作り直す。インデックスの意味が変わるため、選択は外れる。
    /// 通知は行わない。
    #[inline]
    pub fn set_items<S: AsRef<str>>(&self, items: &[S]) {
        self.0.set_items(items)
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    #[inline]
    pub fn selected(&self) -> Option<usize> {
        self.0.selected()
    }

    /// 範囲内なら、通知せずに選択を変える。
    #[inline]
    pub fn set_selected(&self, index: usize) {
        self.0.set_selected(index)
    }

    /// 通知せずに選択を外す。
    #[inline]
    pub fn clear_selection(&self) {
        self.0.clear_selection()
    }

    /// ユーザーが選んだのと同じ経路で項目を選び、1 回通知する。
    #[inline]
    pub fn select(&self, index: usize) {
        self.0.select(index)
    }

    #[inline]
    pub fn set_enabled(&self, enabled: bool) {
        self.0.set_enabled(enabled)
    }

    /// 選択が変わったときに、選ばれた項目のインデックスで呼ばれる。
    #[inline]
    pub fn on_select(&self, f: impl FnMut(usize) + 'static) {
        self.0.on_select(f)
    }
}
