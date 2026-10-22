use crate::sys;
use naui_core::TreeItem;

/// 入れ子の項目を開閉できる一覧。自分でスクロールする。
///
/// 項目は根からの子インデックスの並び (パス) で指す。`[0, 2]` は
/// 「1 番目の根の 3 番目の子」で、空のパスは「選択なし」を表す。
///
/// 高さは中身から決まらないので、[`Tree::set_sizing`] で指定しておく。
#[derive(Clone)]
pub struct Tree(pub(crate) sys::Tree);

impl_widget!(Tree);

impl Tree {
    /// 項目を作り直す。パスの意味が変わるため、選択は外れる。
    ///
    /// 開閉は [`TreeItem::expanded`] のとおりに戻る。
    #[inline]
    pub fn set_items(&self, items: &[TreeItem]) {
        self.0.set_items(items)
    }

    /// 子孫まで数えた項目の総数。
    #[inline]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// 選ばれている項目のパス。何も選ばれていなければ `None`。
    #[inline]
    pub fn selected(&self) -> Option<Vec<usize>> {
        self.0.selected()
    }

    /// `path` の項目が見えるところまでスクロールする。選択は変わらない。
    ///
    /// 閉じた枝の中にあるときは、祖先を開いてから送る ([`set_selected`](Self::set_selected)
    /// と同じく、開閉は通知しない)。すでに見えていれば動かさない。
    /// 無いパスなら何もしない。
    #[inline]
    pub fn scroll_to_item(&self, path: &[usize]) {
        self.0.scroll_to_item(path)
    }

    /// 通知せずに 1 項目を選ぶ。
    ///
    /// 選べない項目や無いパスを渡すと、選択は外れる。閉じた枝の中にある
    /// 項目は、見えるように祖先を開いてから選ぶ。
    #[inline]
    pub fn set_selected(&self, path: &[usize]) {
        self.0.set_selected(path)
    }

    /// 通知せずに選択を外す。
    #[inline]
    pub fn clear_selection(&self) {
        self.0.clear_selection()
    }

    /// ユーザーが選んだのと同じ経路で 1 項目を選ぶ (通知あり)。
    #[inline]
    pub fn select(&self, path: &[usize]) {
        self.0.select(path)
    }

    /// 選択が変わったときに、選ばれている項目のパスで呼ばれる。
    ///
    /// 選択が外れたときは空のパスで呼ばれる。
    #[inline]
    pub fn on_select(&self, f: impl FnMut(&[usize]) + 'static) {
        self.0.on_select(f)
    }

    /// その項目が開いているかどうか。
    ///
    /// 閉じた枝の中にあって見えていない項目でも、開いた状態として覚えていれば
    /// `true` を返す (親を開き直すと、そのまま開いて出てくる)。
    #[inline]
    pub fn is_expanded(&self, path: &[usize]) -> bool {
        self.0.is_expanded(path)
    }

    /// 通知せずに開閉を変える。開くときは祖先もまとめて開く。
    #[inline]
    pub fn set_expanded(&self, path: &[usize], expanded: bool) {
        self.0.set_expanded(path, expanded)
    }

    /// ユーザーが開いたのと同じ経路で開く (通知あり)。
    #[inline]
    pub fn expand(&self, path: &[usize]) {
        self.0.expand(path)
    }

    /// ユーザーが閉じたのと同じ経路で閉じる (通知あり)。
    #[inline]
    pub fn collapse(&self, path: &[usize]) {
        self.0.collapse(path)
    }

    /// 通知せずにすべての枝を開く。
    #[inline]
    pub fn expand_all(&self) {
        self.0.expand_all()
    }

    /// 通知せずにすべての枝を閉じる。
    #[inline]
    pub fn collapse_all(&self) {
        self.0.collapse_all()
    }

    /// 開閉が変わったときに、その項目のパスと変わった後の状態で呼ばれる。
    #[inline]
    pub fn on_expand(&self, f: impl FnMut(&[usize], bool) + 'static) {
        self.0.on_expand(f)
    }
}
