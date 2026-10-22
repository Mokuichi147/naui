use crate::sys;
use naui_core::{SidebarItem, SidebarSection};

/// ウィンドウの左に付けるサイドバー。
///
/// [`Window::set_sidebar`](crate::Window::set_sidebar) で取り付ける。
/// レイアウトには置かないので [`Widget`](crate::Widget) ではない。
#[derive(Clone)]
pub struct Sidebar(pub(crate) sys::Sidebar);

impl_backend!(Sidebar);

impl Sidebar {
    /// 項目をまとまりごとに並べる。呼ぶたびに置き換わる。
    ///
    /// 選ばれていた項目は、同じ通し番号がまだ選べるなら選ばれたまま残る
    /// (通知はしない)。
    #[inline]
    pub fn set_sections(&self, sections: &[SidebarSection]) {
        self.0.set_sections(sections)
    }

    /// 見出しの無いまとまり 1 つだけで並べる。
    #[inline]
    pub fn set_items(&self, items: &[SidebarItem]) {
        self.0.set_items(items)
    }

    /// まとまりをまたいだ項目の数。
    #[inline]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// 選ばれている項目の通し番号。
    #[inline]
    pub fn selected(&self) -> Option<usize> {
        self.0.selected()
    }

    /// 通知せずに選択を変える。範囲外・選べない項目は無視する。
    #[inline]
    pub fn set_selected(&self, index: usize) {
        self.0.set_selected(index)
    }

    /// 選択を外す (通知しない)。
    #[inline]
    pub fn clear_selection(&self) {
        self.0.clear_selection()
    }

    /// 利用者が選んだのと同じように選び、通知する。
    ///
    /// 範囲外・選べない項目は無視する。すでに選ばれている項目でも通知する。
    #[inline]
    pub fn select(&self, index: usize) {
        self.0.select(index)
    }

    /// 項目が選ばれたときの通知先。引数は通し番号。
    #[inline]
    pub fn on_select(&self, f: impl FnMut(usize) + 'static) {
        self.0.on_select(f)
    }

    /// サイドバーの幅 (論理ピクセル)。既定は [`DEFAULT_SIDEBAR_WIDTH`](crate::DEFAULT_SIDEBAR_WIDTH)。
    ///
    /// 利用者は仕切りをドラッグして幅を変えられる (下限は
    /// [`SIDEBAR_MIN_WIDTH`](crate::SIDEBAR_MIN_WIDTH))。変えたあとの幅は [`width`](Self::width) が返し、
    /// [`on_resize`](Self::on_resize) で届く。この呼び出しでは `on_resize` を
    /// 呼ばない。
    #[inline]
    pub fn set_width(&self, width: f64) {
        self.0.set_width(width)
    }

    /// サイドバーの幅。閉じていても開いたときの幅を返す。
    #[inline]
    pub fn width(&self) -> f64 {
        self.0.width()
    }

    /// 利用者が仕切りで幅を変えるたび、変えた後の幅で呼ばれる。
    #[inline]
    pub fn on_resize(&self, f: impl FnMut(f64) + 'static) {
        self.0.on_resize(f)
    }

    /// サイドバーを閉じる (`true`) か開く (`false`)。
    ///
    /// 閉じても項目と選択と幅は残る。[`on_collapse`](Self::on_collapse) は
    /// 呼ばない。
    #[inline]
    pub fn set_collapsed(&self, collapsed: bool) {
        self.0.set_collapsed(collapsed)
    }

    /// 利用者がサイドバーを開閉したときの通知先。引数は閉じたかどうか。
    ///
    /// サイドバーボタンの操作で呼ばれ、[`set_collapsed`](Self::set_collapsed)
    /// では呼ばれない。
    #[inline]
    pub fn on_collapse(&self, f: impl FnMut(bool) + 'static) {
        self.0.on_collapse(f)
    }

    /// サイドバーが閉じているかどうか。
    ///
    /// 見るのは区画自身の表示の指定 (`get_visible`)。`is_visible` は親まで
    /// 含めて見えているかを返すので、ウィンドウを出す前は偽になってしまう。
    #[inline]
    pub fn is_collapsed(&self) -> bool {
        self.0.is_collapsed()
    }
}
