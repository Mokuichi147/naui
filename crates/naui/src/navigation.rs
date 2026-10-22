use crate::sys;
use crate::Widget;
use naui_core::NavItem;

/// パンくず。階層のいまいる場所を左から順に並べる。
///
/// GTK4 に相当するコントロールが無いため、`GtkLabel` のマークアップが持つ
/// リンク (`<a href>`) を区切り (`›`) で挟んで並べる。いまいる場所だけは
/// リンクにせず普通の文字にする (Web の `<a>` や macOS の `NSPathControl` と
/// 同じく、枠の無い文字の並びになる)。太字にしないのは、選び直すたびに
/// 文字幅が変わって後ろの項目がずれるため。ボタンにすると libadwaita の余白
/// (`padding: 5px 10px`) が項目ごとに付き、間延びするため使わない。
#[derive(Clone)]
pub struct Breadcrumbs(pub(crate) sys::Breadcrumbs);

impl_widget!(Breadcrumbs);

impl Breadcrumbs {
    /// 階層を作り直す。**末尾がいまいる場所**になる。
    #[inline]
    pub fn set_items(&self, items: &[NavItem]) {
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

    /// 通知せずに選択を変える。
    #[inline]
    pub fn set_selected(&self, index: usize) {
        self.0.set_selected(index)
    }

    /// ユーザーが選んだのと同じ経路で選択を変える (通知あり)。
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

/// 画面下部に置く横並びのナビゲーション (等幅)。
///
/// **配置はアプリの責務**。縦スタックの最後に置き、手前の要素に
/// [`Length::Fill`](naui_core::Length::Fill) か `Spacer` を使うと下端へ寄る。
#[derive(Clone)]
pub struct Dock(pub(crate) sys::Dock);

impl_widget!(Dock);

impl Dock {
    /// 項目を作り直す。インデックスの意味が変わるため、選択は外れる。
    #[inline]
    pub fn set_items(&self, items: &[NavItem]) {
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

    /// 通知せずに選択を変える。
    #[inline]
    pub fn set_selected(&self, index: usize) {
        self.0.set_selected(index)
    }

    /// ユーザーが選んだのと同じ経路で選択を変える (通知あり)。
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

/// リンク (`GtkLinkButton`)。
///
/// `href` が空でなければ、押したときにブラウザ (デスクトップの既定の
/// ハンドラ) で開く。
#[derive(Clone)]
pub struct Link(pub(crate) sys::Link);

impl_widget!(Link);

impl Link {
    #[inline]
    pub fn text(&self) -> String {
        self.0.text()
    }

    #[inline]
    pub fn set_text(&self, text: &str) {
        self.0.set_text(text)
    }

    #[inline]
    pub fn href(&self) -> String {
        self.0.href()
    }

    #[inline]
    pub fn set_href(&self, href: &str) {
        self.0.set_href(href)
    }

    #[inline]
    pub fn set_enabled(&self, enabled: bool) {
        self.0.set_enabled(enabled)
    }

    /// 押されるたびに呼ばれる。`href` があるときは、開く動作も併せて起きる。
    #[inline]
    pub fn on_click(&self, f: impl FnMut() + 'static) {
        self.0.on_click(f)
    }
}

/// 縦に並ぶナビゲーション一覧。
///
/// GNOME のサイドバーと同じく、選択状態を持つ枠なしボタンを縦に並べる。
/// **右クリックで出るポップアップではない** (それは [`PopupMenu`] のほう)。
///
/// [`PopupMenu`]: crate::PopupMenu
#[derive(Clone)]
pub struct Menu(pub(crate) sys::Menu);

impl_widget!(Menu);

impl Menu {
    /// 項目を作り直す。インデックスの意味が変わるため、選択は外れる。
    #[inline]
    pub fn set_items(&self, items: &[NavItem]) {
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

    /// 通知せずに選択を変える。
    #[inline]
    pub fn set_selected(&self, index: usize) {
        self.0.set_selected(index)
    }

    /// ユーザーが選んだのと同じ経路で選択を変える (通知あり)。
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

/// 画面上部に置く横並びのナビゲーション。
///
/// 見出し (`GtkLabel`) と項目 (`GtkToggleButton` の横並び) を `GtkBox` に並べる。
#[derive(Clone)]
pub struct Navbar(pub(crate) sys::Navbar);

impl_widget!(Navbar);

impl Navbar {
    /// 項目を作り直す。インデックスの意味が変わるため、選択は外れる。
    #[inline]
    pub fn set_items(&self, items: &[NavItem]) {
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

    /// 通知せずに選択を変える。
    #[inline]
    pub fn set_selected(&self, index: usize) {
        self.0.set_selected(index)
    }

    /// ユーザーが選んだのと同じ経路で選択を変える (通知あり)。
    #[inline]
    pub fn select(&self, index: usize) {
        self.0.select(index)
    }

    /// 項目が選ばれたときに、そのインデックスで呼ばれる。
    #[inline]
    pub fn on_select(&self, f: impl FnMut(usize) + 'static) {
        self.0.on_select(f)
    }

    /// 左端の見出し。
    #[inline]
    pub fn set_title(&self, title: &str) {
        self.0.set_title(title)
    }

    #[inline]
    pub fn title(&self) -> String {
        self.0.title()
    }
}

/// ページ送り。
///
/// GTK4 に相当するコントロールが無いため、前後のボタンとページ番号の
/// トグルボタンを横に並べて組み立てる。
#[derive(Clone)]
pub struct Pagination(pub(crate) sys::Pagination);

impl_widget!(Pagination);

impl Pagination {
    /// ページ数を決め直す。先頭のページが選ばれた状態になる。
    #[inline]
    pub fn set_page_count(&self, count: usize) {
        self.0.set_page_count(count)
    }

    #[inline]
    pub fn page_count(&self) -> usize {
        self.0.page_count()
    }

    /// いま開いているページ (0 始まり)。
    #[inline]
    pub fn page(&self) -> usize {
        self.0.page()
    }

    /// 通知せずにページを変える。
    #[inline]
    pub fn set_page(&self, page: usize) {
        self.0.set_page(page)
    }

    /// ユーザーが押したのと同じ経路でページを変える (通知あり)。
    #[inline]
    pub fn select(&self, page: usize) {
        self.0.select(page)
    }

    /// 1 つ前のページへ (通知あり)。先頭では何もしない。
    #[inline]
    pub fn go_previous(&self) {
        self.0.go_previous()
    }

    /// 1 つ後のページへ (通知あり)。末尾では何もしない。
    #[inline]
    pub fn go_next(&self) {
        self.0.go_next()
    }

    /// ページが変わったときに、そのページで呼ばれる。
    #[inline]
    pub fn on_change(&self, f: impl FnMut(usize) + 'static) {
        self.0.on_change(f)
    }
}

/// タブ (`GtkNotebook`)。中身のウィジェットごと持つ。
#[derive(Clone)]
pub struct Tabs(pub(crate) sys::Tabs);

impl_widget!(Tabs);

impl Tabs {
    /// タブを 1 枚追加する。`child` がそのタブの中身になる。
    #[inline]
    pub fn add_tab(&self, label: &str, child: &dyn Widget) {
        self.0.add_tab(label, child.as_backend())
    }

    /// タブを 1 枚外す。範囲外のときは何もしない。
    ///
    /// 選択中のタブを外したときは、環境が近くのタブを選び直す。
    /// この移動は [`set_selected`](Tabs::set_selected) と同じく通知しない。
    #[inline]
    pub fn remove_tab(&self, index: usize) {
        self.0.remove_tab(index)
    }

    /// タブをすべて外す。
    #[inline]
    pub fn clear(&self) {
        self.0.clear()
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

    /// 通知せずに選択を変える。
    #[inline]
    pub fn set_selected(&self, index: usize) {
        self.0.set_selected(index)
    }

    /// ユーザーが選んだのと同じ経路で選択を変える (通知あり)。
    #[inline]
    pub fn select(&self, index: usize) {
        self.0.select(index)
    }

    /// タブが切り替わったときに、そのインデックスで呼ばれる。
    #[inline]
    pub fn on_select(&self, f: impl FnMut(usize) + 'static) {
        self.0.on_select(f)
    }
}
