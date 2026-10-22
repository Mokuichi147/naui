use crate::sys;
use naui_core::MenuSpec;

/// ウィンドウの上端に付く、OS のアプリケーションメニュー。
///
/// [`Widget`](crate::Widget) ではない。
/// [`Window::set_menu_bar`](crate::Window::set_menu_bar) で取り付ける。
/// 項目が押されるたびに、その **(見出しのインデックス, 項目のインデックス)**
/// で [`on_activate`](Self::on_activate) が呼ばれる。項目のインデックスは
/// 区切り線を含めた並びの位置で、区切り線が返ることはない。
#[derive(Clone)]
pub struct MenuBar(pub(crate) sys::MenuBar);

impl_backend!(MenuBar);

impl MenuBar {
    /// メニューを作り直す。以前のメニューは取り除かれる。
    ///
    /// 見出しのインデックスは渡した並びの位置、項目のインデックスは
    /// 区切り線を含めた並びの位置。
    #[inline]
    pub fn set_menus(&self, menus: &[MenuSpec]) {
        self.0.set_menus(menus)
    }

    /// 見出しの数。
    #[inline]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// 見出し 1 つが持つ、区切り線を含めた項目数。範囲外は `0`。
    #[inline]
    pub fn menu_len(&self, menu: usize) -> usize {
        self.0.menu_len(menu)
    }

    /// 項目 1 つの有効・無効を変える。区切り線と範囲外は何もしない。
    #[inline]
    pub fn set_item_enabled(&self, menu: usize, item: usize, enabled: bool) {
        self.0.set_item_enabled(menu, item, enabled)
    }

    /// いま押せる項目か。区切り線と範囲外は `false`。
    #[inline]
    pub fn is_item_enabled(&self, menu: usize, item: usize) -> bool {
        self.0.is_item_enabled(menu, item)
    }

    /// メニューバー全体の有効・無効を変える。項目ごとの指定は残る。
    #[inline]
    pub fn set_enabled(&self, enabled: bool) {
        self.0.set_enabled(enabled)
    }

    /// 利用者が選んだのと同じように項目を実行する。
    ///
    /// 区切り線・押せない項目・範囲外は何もしない。
    #[inline]
    pub fn activate(&self, menu: usize, item: usize) {
        self.0.activate(menu, item)
    }

    /// 項目が押されたときに、その見出しと項目のインデックスで呼ばれる。
    /// 設定し直すと以前のコールバックは外れる。
    #[inline]
    pub fn on_activate(&self, f: impl FnMut(usize, usize) + 'static) {
        self.0.on_activate(f)
    }
}
