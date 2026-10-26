//! Navigation widgets.
//!
//! Tabs, nav bars, docks, menus and pagination all share the same structure: "a
//! sequence of items + the one currently selected". GTK4 has no controls that
//! express these directly, so a common foundation called [`ItemBar`]
//! (`GtkToggleButton`s laid out in a `GtkBox`) is built, with the visual
//! differences given by CSS classes and layout direction.
//!
//! Only `Tabs` holds content widgets, so it uses `GtkNotebook`. Breadcrumbs are not
//! buttons but `GtkLabel`s with links (see [`Breadcrumbs`]).

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
    /// The CSS classes put on the buttons.
    css: &'static [&'static str],
}

/// A foundation that lays out items horizontally / vertically and holds a state
/// with exactly one selected.
#[derive(Clone)]
pub(crate) struct ItemBar(Rc<BarInner>);

impl ItemBar {
    fn new(orientation: gtk::Orientation, homogeneous: bool, css: &'static [&'static str]) -> Self {
        let native = gtk::Box::new(orientation, 0);
        native.set_homogeneous(homogeneous);
        Self(Rc::new(BarInner {
            native,
            buttons: RefCell::new(Vec::new()),
            items: RefCell::new(Vec::new()),
            selected: Cell::new(None),
            on_select: Notifier::default(),
            css,
        }))
    }

    fn native(&self) -> &gtk::Box {
        &self.0.native
    }

    /// Rebuilds the items. Indices change meaning, so the selection is cleared.
    fn set_items(&self, items: &[NavItem]) {
        while let Some(child) = self.0.native.first_child() {
            self.0.native.remove(&child);
        }
        self.0.buttons.borrow_mut().clear();
        self.0.selected.set(None);

        for (index, item) in items.iter().enumerate() {
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

    /// Matches the buttons' pressed state to the selection.
    fn show(&self, selected: Option<usize>) {
        self.0.selected.set(selected);
        for (index, button) in self.0.buttons.borrow().iter().enumerate() {
            // `set_active` は `clicked` を出さないので、通知の心配は要らない。
            button.set_active(Some(index) == selected);
        }
    }

    /// Whether the item can be selected.
    fn is_selectable(&self, index: usize) -> bool {
        self.0
            .items
            .borrow()
            .get(index)
            .is_some_and(|item| item.enabled)
    }

    /// Changes the selection without notifying.
    fn set_selected(&self, index: usize) {
        if self.is_selectable(index) {
            self.show(Some(index));
        }
    }

    /// Changes the selection through the same path as a user's choice (notifies).
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

/// Adds the API common to navigation widgets with items.
macro_rules! impl_item_bar {
    ($t:ty) => {
        impl $t {
            /// Rebuilds the items. Indices change meaning, so the selection is cleared.
            pub fn set_items(&self, items: &[NavItem]) {
                self.0.bar.set_items(items);
            }

            pub fn len(&self) -> usize {
                self.0.bar.len()
            }

            pub fn is_empty(&self) -> bool {
                self.len() == 0
            }

            pub fn selected(&self) -> Option<usize> {
                self.0.bar.selected()
            }

            /// Changes the selection without notifying.
            pub fn set_selected(&self, index: usize) {
                self.0.bar.set_selected(index);
            }

            /// Changes the selection through the same path as a user's choice (notifies).
            pub fn select(&self, index: usize) {
                self.0.bar.select(index);
            }

            /// Called with the index when an item is chosen.
            pub fn on_select(&self, f: impl FnMut(usize) + 'static) {
                self.0.bar.on_select(f);
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

/// Horizontal navigation placed at the top of the screen.
///
/// A heading (`GtkLabel`) and items (a row of `GtkToggleButton`s) are laid out in a
/// `GtkBox`.
#[derive(Clone)]
pub struct Navbar(Rc<NavbarInner>);
impl_widget!(Navbar);
impl_item_bar!(Navbar);

impl Navbar {
    pub(crate) fn new(title: &str) -> Self {
        let native = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        let label = gtk::Label::new(Some(title));
        label.add_css_class("title-4");
        let bar = ItemBar::new(gtk::Orientation::Horizontal, false, &["flat"]);
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

    /// The heading at the left end.
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

/// Horizontal navigation placed at the bottom of the screen (equal widths).
///
/// **Placement is the app's job.** Put it last in a vertical stack and give the
/// element before it [`Length::Fill`](naui_core::Length::Fill) or a `Spacer` to push
/// it to the bottom.
#[derive(Clone)]
pub struct Dock(Rc<DockInner>);
impl_widget!(Dock);
impl_item_bar!(Dock);

impl Dock {
    pub(crate) fn new() -> Self {
        let bar = ItemBar::new(gtk::Orientation::Horizontal, true, &["flat"]);
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

/// A vertical list of navigation items.
///
/// As in GNOME's sidebars, borderless buttons with a selected state are stacked
/// vertically. **It is not the popup shown on right-click** (that is
/// [`PopupMenu`]).
///
/// [`PopupMenu`]: crate::PopupMenu
#[derive(Clone)]
pub struct Menu(Rc<MenuInner>);
impl_widget!(Menu);
impl_item_bar!(Menu);

impl Menu {
    pub(crate) fn new() -> Self {
        let bar = ItemBar::new(gtk::Orientation::Vertical, false, &["flat"]);
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
    /// A label per item (separators not included).
    labels: RefCell<Vec<gtk::Label>>,
    items: RefCell<Vec<NavItem>>,
    selected: Cell<Option<usize>>,
    on_select: Notifier<usize>,
}

/// Breadcrumbs. Lays out the current position in a hierarchy from left to right.
///
/// GTK4 has no equivalent control, so the links (`<a href>`) supported by
/// `GtkLabel` markup are laid out with separators (`›`) between them. Only the
/// current position is plain text, not a link (as with the Web's `<a>` and macOS's
/// `NSPathControl`, it is a row of text without borders). It is not bold because
/// the text width would change on every reselection and shift the items after it.
/// Buttons are not used because libadwaita's padding (`padding: 5px 10px`) would
/// be added to every item and spread them out.
#[derive(Clone)]
pub struct Breadcrumbs(Rc<BreadcrumbsInner>);
impl_widget!(Breadcrumbs);

impl Breadcrumbs {
    pub(crate) fn new() -> Self {
        let native = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        let bin = SizeBin::wrap(&native);
        Self(Rc::new(BreadcrumbsInner {
            native,
            bin,
            labels: RefCell::new(Vec::new()),
            items: RefCell::new(Vec::new()),
            selected: Cell::new(None),
            on_select: Notifier::default(),
        }))
    }

    /// Rebuilds the hierarchy. **The last item is the current position.**
    pub fn set_items(&self, items: &[NavItem]) {
        let native = &self.0.native;
        while let Some(child) = native.first_child() {
            native.remove(&child);
        }
        let mut labels = Vec::with_capacity(items.len());
        for index in 0..items.len() {
            if index > 0 {
                let separator = gtk::Label::new(Some("›"));
                separator.add_css_class("dim-label");
                native.append(&separator);
            }
            let label = gtk::Label::new(None);
            without_context_menu(&label);
            let weak: Weak<BreadcrumbsInner> = Rc::downgrade(&self.0);
            label.connect_activate_link(move |label, _| {
                // 選び直しは次の周回へ回す。`GtkLabel` は押下を見ている最中で、
                // ここでリンクの無い文字にすると (`show` も、アプリの
                // `on_select` から呼ばれる `set_items` も必ずそうする)、
                // ラベルが押下を見ているジェスチャーごと捨てられ、戻り先の
                // 消えた GTK4 側が落ちるため。
                let weak = weak.clone();
                let label = label.downgrade();
                glib::idle_add_local_once(move || {
                    let (Some(inner), Some(label)) = (weak.upgrade(), label.upgrade()) else {
                        return;
                    };
                    let breadcrumbs = Breadcrumbs(inner);
                    // 待っている間に階層が作り直されていたら、もう別の場所。
                    if breadcrumbs.0.labels.borrow().get(index) == Some(&label) {
                        breadcrumbs.select(index);
                    }
                });
                // 既定の処理 (URI をブラウザで開く) には渡さない。
                glib::Propagation::Stop
            });
            native.append(&label);
            labels.push(label);
        }
        *self.0.labels.borrow_mut() = labels;
        *self.0.items.borrow_mut() = items.to_vec();
        self.show(None);
        if let Some(last) = items.len().checked_sub(1) {
            self.set_selected(last);
        }
    }

    pub fn len(&self) -> usize {
        self.0.labels.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn selected(&self) -> Option<usize> {
        self.0.selected.get()
    }

    /// Changes the selection without notifying.
    pub fn set_selected(&self, index: usize) {
        if self.is_selectable(index) {
            self.show(Some(index));
        }
    }

    /// Changes the selection through the same path as a user's choice (notifies).
    pub fn select(&self, index: usize) {
        if self.is_selectable(index) {
            self.show(Some(index));
            self.0.on_select.emit(index);
        }
    }

    /// Called with the index when an item is chosen.
    pub fn on_select(&self, f: impl FnMut(usize) + 'static) {
        self.0.on_select.set(f);
    }

    fn is_selectable(&self, index: usize) -> bool {
        self.0
            .items
            .borrow()
            .get(index)
            .is_some_and(|item| item.enabled)
    }

    /// Makes the selected item plain text and the other selectable items links.
    ///
    /// The markup is rewritten every time, so the "visited" color that clicked links
    /// get is also cleared here.
    fn show(&self, selected: Option<usize>) {
        self.0.selected.set(selected);
        let items = self.0.items.borrow();
        for (index, (label, item)) in self.0.labels.borrow().iter().zip(items.iter()).enumerate() {
            let text = glib::markup_escape_text(&item.label);
            if item.enabled && Some(index) != selected {
                // href は外へ見せない (右クリックのメニューを出さない) ので、番号で足りる。
                label.set_markup(&format!("<a href=\"{index}\">{text}</a>"));
            } else {
                label.set_markup(&text);
            }
            label.set_sensitive(item.enabled);
        }
    }
}

/// Keeps the label's right-click menu from appearing.
///
/// The menu of a `GtkLabel` with links offers only "Open Link" and "Copy Link
/// Address"; the former does the same as clicking, and the latter would copy the
/// item number stored in place of a URI ([`NavItem`] has no URI). The menu can be
/// opened from two places, so both are stopped.
///
/// - Keyboard (Shift+F10 / the Menu key): the `menu.popup` action is disabled.
///   `GtkLabel` never re-enables this action by itself.
/// - Right-click: the label's gesture opens it directly without going through the
///   action, so a capture-phase gesture takes it first.
fn without_context_menu(label: &gtk::Label) {
    label.action_set_enabled("menu.popup", false);
    let secondary = gtk::GestureClick::new();
    secondary.set_button(gtk::gdk::BUTTON_SECONDARY);
    secondary.set_propagation_phase(gtk::PropagationPhase::Capture);
    secondary.connect_pressed(|gesture, _, _, _| {
        gesture.set_state(gtk::EventSequenceState::Claimed);
    });
    label.add_controller(secondary);
}

// ------------------------------------------------------------- Pagination

struct PaginationInner {
    native: gtk::Box,
    bin: SizeBin,
    bar: ItemBar,
    previous: gtk::Button,
    next: gtk::Button,
}

/// Pagination.
///
/// GTK4 has no equivalent control, so it is built from previous/next buttons and
/// toggle buttons for the page numbers laid out in a row.
#[derive(Clone)]
pub struct Pagination(Rc<PaginationInner>);
impl_widget!(Pagination);

impl Pagination {
    pub(crate) fn new(page_count: usize) -> Self {
        let native = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        let previous = gtk::Button::from_icon_name("go-previous-symbolic");
        let next = gtk::Button::from_icon_name("go-next-symbolic");
        let bar = ItemBar::new(gtk::Orientation::Horizontal, false, &["flat"]);
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

    /// Resets the number of pages. The first page becomes selected.
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

    /// The page currently open (starting at 0).
    pub fn page(&self) -> usize {
        self.0.bar.selected().unwrap_or(0)
    }

    /// Changes the page without notifying.
    pub fn set_page(&self, page: usize) {
        self.0.bar.set_selected(page);
        self.update_arrows();
    }

    /// Changes the page through the same path as a user's click (notifies).
    pub fn select(&self, page: usize) {
        self.0.bar.select(page);
        self.update_arrows();
    }

    /// Goes to the previous page (notifies). Does nothing on the first page.
    pub fn go_previous(&self) {
        let page = self.page();
        if page > 0 {
            self.select(page - 1);
        }
    }

    /// Goes to the next page (notifies). Does nothing on the last page.
    pub fn go_next(&self) {
        let page = self.page();
        if page + 1 < self.page_count() {
            self.select(page + 1);
        }
    }

    /// Disables the arrow in a direction once the end has been reached.
    fn update_arrows(&self) {
        let page = self.page();
        let count = self.page_count();
        self.0.previous.set_sensitive(page > 0);
        self.0.next.set_sensitive(page + 1 < count);
    }

    /// Called with the page when the page changes.
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

/// Tabs (`GtkNotebook`). Holds each tab's content widget.
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

    /// Adds one tab. `child` becomes that tab's content.
    pub fn add_tab(&self, label: &str, child: &dyn Widget) {
        let bin = child.size_bin();
        bin.fill_parent();
        self.0
            .native
            .append_page(&bin, Some(&gtk::Label::new(Some(label))));
        self.0.children.borrow_mut().push(child.boxed_clone());
    }

    /// Removes one tab. Does nothing if out of range.
    ///
    /// When the selected tab is removed, the platform selects a nearby tab instead.
    /// Like [`set_selected`](Tabs::set_selected), this move is not notified.
    pub fn remove_tab(&self, index: usize) {
        if index >= self.len() {
            return;
        }
        let selected = self.selected();
        let bin = self.0.children.borrow()[index].size_bin();
        crate::interaction::clear_focus(&bin);
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

    /// Removes every tab.
    pub fn clear(&self) {
        let bins: Vec<_> = self
            .0
            .children
            .borrow()
            .iter()
            .map(|child| child.size_bin())
            .collect();
        for bin in bins {
            crate::interaction::clear_focus(&bin);
        }
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

    /// Changes the selection without notifying.
    pub fn set_selected(&self, index: usize) {
        if index < self.len() {
            without_signal(&self.0.native, &self.0.handler, || {
                self.0.native.set_current_page(Some(index as u32));
            });
        }
    }

    /// Changes the selection through the same path as a user's choice (notifies).
    pub fn select(&self, index: usize) {
        if index < self.len() {
            // 同じタブを選び直したときは `switch-page` が出ないため、
            // シグナルは止めて、ここで 1 回だけ通知する。
            self.set_selected(index);
            self.0.on_select.emit(index);
        }
    }

    /// Called with the index when the tab changes.
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

/// A link (`GtkLinkButton`).
///
/// If `href` is not empty, clicking it opens it in the browser (the desktop's
/// default handler).
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

    /// Called on every click. If there is an `href`, it is opened as well.
    pub fn on_click(&self, mut f: impl FnMut() + 'static) {
        self.0.on_click.set(move |()| f());
    }
}
