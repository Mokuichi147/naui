use crate::sys;
use naui_core::ToolbarItem;

/// ウィンドウの上端に付く、よく使う操作の並び。
///
/// [`Widget`](crate::Widget) ではない。
/// [`Window::set_toolbar`](crate::Window::set_toolbar) で取り付ける。
/// ナビゲーションと違い**選ばれている項目を持たず**、押されるたびに
/// そのインデックスで [`on_activate`](Self::on_activate) が呼ばれる。
/// インデックスは区切りを含めた並びの位置で、区切りが返ることはない。
#[derive(Clone)]
pub struct Toolbar(pub(crate) sys::Toolbar);

impl_backend!(Toolbar);

impl Toolbar {
    /// 項目を作り直す。以前の項目は取り除かれる。
    ///
    /// インデックスは区切りを含めた並びの位置。
    #[inline]
    pub fn set_items(&self, items: &[ToolbarItem]) {
        self.0.set_items(items)
    }

    /// 区切りを含めた項目数。
    #[inline]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// 項目 1 つの有効・無効を変える。区切りと範囲外は何もしない。
    #[inline]
    pub fn set_item_enabled(&self, index: usize, enabled: bool) {
        self.0.set_item_enabled(index, enabled)
    }

    /// いま押せる項目か。区切りと範囲外は `false`。
    #[inline]
    pub fn is_item_enabled(&self, index: usize) -> bool {
        self.0.is_item_enabled(index)
    }

    /// ツールバー全体の有効・無効を変える。項目ごとの指定は残る。
    #[inline]
    pub fn set_enabled(&self, enabled: bool) {
        self.0.set_enabled(enabled)
    }

    /// 利用者が押したのと同じように項目を実行する。
    ///
    /// 区切り・押せない項目・範囲外は何もしない。
    #[inline]
    pub fn activate(&self, index: usize) {
        self.0.activate(index)
    }

    /// 項目が押されたときに、そのインデックスで呼ばれる。
    /// 設定し直すと以前のコールバックは外れる。
    #[inline]
    pub fn on_activate(&self, f: impl FnMut(usize) + 'static) {
        self.0.on_activate(f)
    }
}
