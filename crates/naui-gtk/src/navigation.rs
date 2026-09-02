//! ナビゲーション系のウィジェット。
//!
//! タブ・ナビバー・ドック・メニュー・パンくず・ページ送りは、どれも
//! 「項目の並び + いま選ばれているもの」という同じ構造を持つ。GTK4 には
//! これらをそのまま表すコントロールが無いため、[`ItemBar`] という共通の
//! 土台 (`GtkToggleButton` を `GtkBox` に並べたもの) を作り、
//! 見た目の違いは CSS クラスと並ぶ向きで付けている。
//!
//! `Tabs` だけは中身のウィジェットを持つので `GtkNotebook` を使う。

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use gtk::glib;
use gtk::prelude::*;
use naui_core::NavItem;

use crate::bin::SizeBin;
use crate::callback::Notifier;
use crate::widgets::{impl_widget, without_signal, Widget};

// ---------------------------------------------------------------- ItemBar

struct BarInner {
    native: gtk::Box,
    buttons: RefCell<Vec<gtk::ToggleButton>>,
    items: RefCell<Vec<NavItem>>,
    selected: Cell<Option<usize>>,
    on_select: Notifier<usize>,
    /// 項目の間に挟む区切り (パンくずの `›`)。
    separator: Option<&'static str>,
    /// ボタンに付ける CSS クラス。
    css: &'static [&'static str],
}

/// 項目を横 / 縦に並べ、1 つだけ選ばれている状態を持つ土台。
#[derive(Clone)]
pub(crate) struct ItemBar(Rc<BarInner>);

impl ItemBar {
    fn new(
        orientation: gtk::Orientation,
        homogeneous: bool,
        separator: Option<&'static str>,
        css: &'static [&'static str],
    ) -> Self {
        let native = gtk::Box::new(orientation, if separator.is_some() { 2 } else { 0 });
        native.set_homogeneous(homogeneous);
        Self(Rc::new(BarInner {
            native,
            buttons: RefCell::new(Vec::new()),
            items: RefCell::new(Vec::new()),
            selected: Cell::new(None),
            on_select: Notifier::default(),
            separator,
            css,
        }))
    }

    fn native(&self) -> &gtk::Box {
        &self.0.native
    }

    /// 項目を作り直す。インデックスの意味が変わるため、選択は外れる。
    fn set_items(&self, items: &[NavItem]) {
        while let Some(child) = self.0.native.first_child() {
            self.0.native.remove(&child);
        }
        self.0.buttons.borrow_mut().clear();
        self.0.selected.set(None);

        for (index, item) in items.iter().enumerate() {
            if index > 0 {
                if let Some(separator) = self.0.separator {
                    let label = gtk::Label::new(Some(separator));
                    label.add_css_class("dim-label");
                    self.0.native.append(&label);
                }
            }
            let button = gtk::ToggleButton::with_label(&item.label);
            button.set_sensitive(item.enabled);
            for class in self.0.css {
                button.add_css_class(class);
            }
            let weak: Weak<BarInner> = Rc::downgrade(&self.0);
            button.connect_clicked(move |_| {
                if let Some(inner) = weak.upgrade() {
                    ItemBar(inner).select(index);
                }
            });
            self.0.native.append(&button);
            self.0.buttons.borrow_mut().push(button);
        }
        self.0.items.borrow_mut().clear();
        self.0.items.borrow_mut().extend_from_slice(items);
        self.show(None);
    }

    fn len(&self) -> usize {
        self.0.buttons.borrow().len()
    }

    fn selected(&self) -> Option<usize> {
        self.0.selected.get()
    }

    /// ボタンの押し込み状態を選択に合わせる。
    fn show(&self, selected: Option<usize>) {
        self.0.selected.set(selected);
        for (index, button) in self.0.buttons.borrow().iter().enumerate() {
            // `set_active` は `clicked` を出さないので、通知の心配は要らない。
            button.set_active(Some(index) == selected);
        }
    }

    /// 選べる項目か。
    fn is_selectable(&self, index: usize) -> bool {
        self.0
            .items
            .borrow()
            .get(index)
            .is_some_and(|item| item.enabled)
    }

    /// 通知せずに選択を変える。
    fn set_selected(&self, index: usize) {
        if self.is_selectable(index) {
            self.show(Some(index));
        }
    }

    /// ユーザーが選んだのと同じ経路で選択を変える (通知あり)。
    fn select(&self, index: usize) {
        if self.is_selectable(index) {
            self.show(Some(index));
            self.0.on_select.emit(index);
        } else {
            // 選べない項目を押されても、見た目だけは元に戻す。
            self.show(self.0.selected.get());
        }
    }

    fn on_select(&self, f: impl FnMut(usize) + 'static) {
        self.0.on_select.set(f);
    }
}

/// 項目を持つナビゲーションに共通の API を生やす (`set_items` を除く)。
macro_rules! impl_item_bar_core {
    ($t:ty) => {
        impl $t {
            pub fn len(&self) -> usize {
                self.0.bar.len()
            }

            pub fn is_empty(&self) -> bool {
                self.len() == 0
            }

            pub fn selected(&self) -> Option<usize> {
                self.0.bar.selected()
            }

            /// 通知せずに選択を変える。
            pub fn set_selected(&self, index: usize) {
                self.0.bar.set_selected(index);
            }

            /// ユーザーが選んだのと同じ経路で選択を変える (通知あり)。
            pub fn select(&self, index: usize) {
                self.0.bar.select(index);
            }

            /// 項目が選ばれたときに、そのインデックスで呼ばれる。
            pub fn on_select(&self, f: impl FnMut(usize) + 'static) {
                self.0.bar.on_select(f);
            }
        }
    };
}

/// 項目を持つナビゲーションに共通の API を生やす。
macro_rules! impl_item_bar {
    ($t:ty) => {
        impl_item_bar_core!($t);

        impl $t {
            /// 項目を作り直す。インデックスの意味が変わるため、選択は外れる。
            pub fn set_items(&self, items: &[NavItem]) {
                self.0.bar.set_items(items);
            }
        }
    };
}

// ----------------------------------------------------------------- Navbar

struct NavbarInner {
    native: gtk::Box,
    bin: SizeBin,
    title: gtk::Label,
    bar: ItemBar,
}

/// 画面上部に置く横並びのナビゲーション。
///
/// 見出し (`GtkLabel`) と項目 (`GtkToggleButton` の横並び) を `GtkBox` に並べる。
#[derive(Clone)]
pub struct Navbar(Rc<NavbarInner>);
impl_widget!(Navbar);
impl_item_bar!(Navbar);

impl Navbar {
    pub(crate) fn new(title: &str) -> Self {
        let native = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        let label = gtk::Label::new(Some(title));
        label.add_css_class("title-4");
        let bar = ItemBar::new(gtk::Orientation::Horizontal, false, None, &["flat"]);
        bar.native().add_css_class("linked");
        native.append(&label);
        native.append(bar.native());
        let bin = SizeBin::wrap(&native);
        Self(Rc::new(NavbarInner {
            native,
            bin,
            title: label,
            bar,
        }))
    }

    /// 左端の見出し。
    pub fn set_title(&self, title: &str) {
        self.0.title.set_text(title);
    }

    pub fn title(&self) -> String {
        self.0.title.text().to_string()
    }
}

// ------------------------------------------------------------------- Dock

struct DockInner {
    native: gtk::Box,
    bin: SizeBin,
    bar: ItemBar,
}

/// 画面下部に置く横並びのナビゲーション (等幅)。
///
/// **配置はアプリの責務**。縦スタックの最後に置き、手前の要素に
/// [`Length::Fill`](naui_core::Length::Fill) か `Spacer` を使うと下端へ寄る。
#[derive(Clone)]
pub struct Dock(Rc<DockInner>);
impl_widget!(Dock);
impl_item_bar!(Dock);

impl Dock {
    pub(crate) fn new() -> Self {
        let bar = ItemBar::new(gtk::Orientation::Horizontal, true, None, &["flat"]);
        let native = bar.native().clone();
        native.add_css_class("toolbar");
        let bin = SizeBin::wrap(&native);
        Self(Rc::new(DockInner { native, bin, bar }))
    }
}

// ------------------------------------------------------------------- Menu

struct MenuInner {
    native: gtk::Box,
    bin: SizeBin,
    bar: ItemBar,
}

/// 縦に並ぶナビゲーション一覧。
///
/// GNOME のサイドバーと同じく、選択状態を持つ枠なしボタンを縦に並べる。
/// **右クリックで出るポップアップではない** (それは [`PopupMenu`] のほう)。
///
/// [`PopupMenu`]: crate::PopupMenu
#[derive(Clone)]
pub struct Menu(Rc<MenuInner>);
impl_widget!(Menu);
impl_item_bar!(Menu);

impl Menu {
    pub(crate) fn new() -> Self {
        let bar = ItemBar::new(gtk::Orientation::Vertical, false, None, &["flat"]);
        let native = bar.native().clone();
        native.add_css_class("navigation-sidebar");
        let bin = SizeBin::wrap(&native);
        Self(Rc::new(MenuInner { native, bin, bar }))
    }
}

// ------------------------------------------------------------ Breadcrumbs

struct BreadcrumbsInner {
    native: gtk::Box,
    bin: SizeBin,
    bar: ItemBar,
}

/// パンくず。階層のいまいる場所を左から順に並べる。
///
/// GTK4 に相当するコントロールが無いため、区切り (`›`) を挟んだ
/// 枠なしボタンの横並びで組み立てる。
#[derive(Clone)]
pub struct Breadcrumbs(Rc<BreadcrumbsInner>);
impl_widget!(Breadcrumbs);
impl_item_bar_core!(Breadcrumbs);

impl Breadcrumbs {
    pub(crate) fn new() -> Self {
        let bar = ItemBar::new(gtk::Orientation::Horizontal, false, Some("›"), &["flat"]);
        let native = bar.native().clone();
        let bin = SizeBin::wrap(&native);
        Self(Rc::new(BreadcrumbsInner { native, bin, bar }))
    }

    /// 階層を作り直す。**末尾がいまいる場所**になる。
    pub fn set_items(&self, items: &[NavItem]) {
        self.0.bar.set_items(items);
        if !items.is_empty() {
            self.0.bar.set_selected(items.len() - 1);
        }
    }
}

// ------------------------------------------------------------- Pagination

struct PaginationInner {
    native: gtk::Box,
    bin: SizeBin,
    bar: ItemBar,
    previous: gtk::Button,
    next: gtk::Button,
}

/// ページ送り。
///
/// GTK4 に相当するコントロールが無いため、前後のボタンとページ番号の
/// トグルボタンを横に並べて組み立てる。
#[derive(Clone)]
pub struct Pagination(Rc<PaginationInner>);
impl_widget!(Pagination);

impl Pagination {
    pub(crate) fn new(page_count: usize) -> Self {
        let native = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        let previous = gtk::Button::from_icon_name("go-previous-symbolic");
        let next = gtk::Button::from_icon_name("go-next-symbolic");
        let bar = ItemBar::new(gtk::Orientation::Horizontal, false, None, &["flat"]);
        bar.native().add_css_class("linked");
        native.append(&previous);
        native.append(bar.native());
        native.append(&next);
        let bin = SizeBin::wrap(&native);

        let inner = Rc::new(PaginationInner {
            native,
            bin,
            bar,
            previous,
            next,
        });
        let pagination = Self(inner);
        pagination.set_page_count(page_count);
        {
            let weak = Rc::downgrade(&pagination.0);
            pagination.0.previous.connect_clicked(move |_| {
                if let Some(inner) = weak.upgrade() {
                    Pagination(inner).go_previous();
                }
            });
        }
        {
            let weak = Rc::downgrade(&pagination.0);
            pagination.0.next.connect_clicked(move |_| {
                if let Some(inner) = weak.upgrade() {
                    Pagination(inner).go_next();
                }
            });
        }
        pagination
    }

    /// ページ数を決め直す。先頭のページが選ばれた状態になる。
    pub fn set_page_count(&self, count: usize) {
        let labels: Vec<NavItem> = (1..=count).map(|n| NavItem::new(n.to_string())).collect();
        self.0.bar.set_items(&labels);
        if count > 0 {
            self.0.bar.set_selected(0);
        }
        self.update_arrows();
    }

    pub fn page_count(&self) -> usize {
        self.0.bar.len()
    }

    /// いま開いているページ (0 始まり)。
    pub fn page(&self) -> usize {
        self.0.bar.selected().unwrap_or(0)
    }

    /// 通知せずにページを変える。
    pub fn set_page(&self, page: usize) {
        self.0.bar.set_selected(page);
        self.update_arrows();
    }

    /// ユーザーが押したのと同じ経路でページを変える (通知あり)。
    pub fn select(&self, page: usize) {
        self.0.bar.select(page);
        self.update_arrows();
    }

    /// 1 つ前のページへ (通知あり)。先頭では何もしない。
    pub fn go_previous(&self) {
        let page = self.page();
        if page > 0 {
            self.select(page - 1);
        }
    }

    /// 1 つ後のページへ (通知あり)。末尾では何もしない。
    pub fn go_next(&self) {
        let page = self.page();
        if page + 1 < self.page_count() {
            self.select(page + 1);
        }
    }

    /// 端まで来たら、その向きの矢印を押せなくする。
    fn update_arrows(&self) {
        let page = self.page();
        let count = self.page_count();
        self.0.previous.set_sensitive(page > 0);
        self.0.next.set_sensitive(page + 1 < count);
    }

    /// ページが変わったときに、そのページで呼ばれる。
    pub fn on_change(&self, f: impl FnMut(usize) + 'static) {
        self.0.bar.on_select(f);
    }
}

// ------------------------------------------------------------------- Tabs

struct TabsInner {
    native: gtk::Notebook,
    bin: SizeBin,
    children: RefCell<Vec<Box<dyn Widget>>>,
    on_select: Notifier<usize>,
    handler: RefCell<Option<glib::SignalHandlerId>>,
}

/// タブ (`GtkNotebook`)。中身のウィジェットごと持つ。
#[derive(Clone)]
pub struct Tabs(Rc<TabsInner>);
impl_widget!(Tabs);

impl Tabs {
    pub(crate) fn new() -> Self {
        let native = gtk::Notebook::new();
        // 既定の `GtkNotebook` は「全タブが横に並ぶ幅」を最小幅として申告する
        // ため、タブが増えるとウィンドウがそれ以下に縮められなくなる。
        // 収まらないときは矢印で送る形にして、最小幅をタブ 1 枚ぶんに保つ。
        native.set_scrollable(true);
        let bin = SizeBin::wrap(&native);
        let inner = Rc::new(TabsInner {
            native,
            bin,
            children: RefCell::new(Vec::new()),
            on_select: Notifier::default(),
            handler: RefCell::new(None),
        });
        // 切り替えの通知は常時つないでおき、`set_selected` の間だけ止める。
        let id = {
            let weak = Rc::downgrade(&inner);
            inner.native.connect_switch_page(move |_, _, page| {
                if let Some(inner) = weak.upgrade() {
                    inner.on_select.emit(page as usize);
                }
            })
        };
        *inner.handler.borrow_mut() = Some(id);
        Self(inner)
    }

    /// タブを 1 枚追加する。`child` がそのタブの中身になる。
    pub fn add_tab(&self, label: &str, child: &dyn Widget) {
        let bin = child.size_bin();
        bin.fill_parent();
        self.0
            .native
            .append_page(&bin, Some(&gtk::Label::new(Some(label))));
        self.0.children.borrow_mut().push(child.boxed_clone());
    }

    /// タブを 1 枚外す。範囲外のときは何もしない。
    ///
    /// 選択中のタブを外したときは、環境が近くのタブを選び直す。
    /// この移動は [`set_selected`](Tabs::set_selected) と同じく通知しない。
    pub fn remove_tab(&self, index: usize) {
        if index >= self.len() {
            return;
        }
        let selected = self.selected();
        without_signal(&self.0.native, &self.0.handler, || {
            self.0.native.remove_page(Some(index as u32));
        });
        self.0.children.borrow_mut().remove(index);
        // 選択の寄せ先は環境任せにせず、4 バックエンドで同じ形にそろえる。
        let left = self.len();
        let selected = match selected {
            _ if left == 0 => None,
            Some(current) if current == index => Some(index.min(left - 1)),
            Some(current) if current > index => Some(current - 1),
            other => other,
        };
        if let Some(selected) = selected {
            without_signal(&self.0.native, &self.0.handler, || {
                self.0.native.set_current_page(Some(selected as u32));
            });
        }
    }

    /// タブをすべて外す。
    pub fn clear(&self) {
        without_signal(&self.0.native, &self.0.handler, || {
            while self.0.native.n_pages() > 0 {
                self.0.native.remove_page(Some(0));
            }
        });
        self.0.children.borrow_mut().clear();
    }

    pub fn len(&self) -> usize {
        self.0.native.n_pages() as usize
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn selected(&self) -> Option<usize> {
        self.0.native.current_page().map(|page| page as usize)
    }

    /// 通知せずに選択を変える。
    pub fn set_selected(&self, index: usize) {
        if index < self.len() {
            without_signal(&self.0.native, &self.0.handler, || {
                self.0.native.set_current_page(Some(index as u32));
            });
        }
    }

    /// ユーザーが選んだのと同じ経路で選択を変える (通知あり)。
    pub fn select(&self, index: usize) {
        if index < self.len() {
            // 同じタブを選び直したときは `switch-page` が出ないため、
            // シグナルは止めて、ここで 1 回だけ通知する。
            self.set_selected(index);
            self.0.on_select.emit(index);
        }
    }

    /// タブが切り替わったときに、そのインデックスで呼ばれる。
    pub fn on_select(&self, f: impl FnMut(usize) + 'static) {
        self.0.on_select.set(f);
    }
}

// ------------------------------------------------------------------- Link

struct LinkInner {
    native: gtk::LinkButton,
    bin: SizeBin,
    on_click: Notifier<()>,
    handler: RefCell<Option<glib::SignalHandlerId>>,
}

/// リンク (`GtkLinkButton`)。
///
/// `href` が空でなければ、押したときにブラウザ (デスクトップの既定の
/// ハンドラ) で開く。
#[derive(Clone)]
pub struct Link(Rc<LinkInner>);
impl_widget!(Link);

impl Link {
    pub(crate) fn new(text: &str, href: &str) -> Self {
        let native = gtk::LinkButton::with_label(href, text);
        native.add_css_class("flat");
        let bin = SizeBin::wrap(&native);
        let inner = Rc::new(LinkInner {
            native,
            bin,
            on_click: Notifier::default(),
            handler: RefCell::new(None),
        });
        let id = {
            let weak = Rc::downgrade(&inner);
            inner.native.connect_activate_link(move |native| {
                let Some(inner) = weak.upgrade() else {
                    return glib::Propagation::Proceed;
                };
                inner.on_click.emit(());
                if native.uri().is_empty() {
                    // 行き先が無いときは、開こうとして失敗させない。
                    glib::Propagation::Stop
                } else {
                    glib::Propagation::Proceed
                }
            })
        };
        *inner.handler.borrow_mut() = Some(id);
        Self(inner)
    }

    pub fn text(&self) -> String {
        self.0
            .native
            .label()
            .map(|l| l.to_string())
            .unwrap_or_default()
    }

    pub fn set_text(&self, text: &str) {
        self.0.native.set_label(text);
    }

    pub fn href(&self) -> String {
        self.0.native.uri().to_string()
    }

    pub fn set_href(&self, href: &str) {
        self.0.native.set_uri(href);
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.0.native.set_sensitive(enabled);
    }

    /// 押されるたびに呼ばれる。`href` があるときは、開く動作も併せて起きる。
    pub fn on_click(&self, mut f: impl FnMut() + 'static) {
        self.0.on_click.set(move |()| f());
    }
}
