//! The sidebar (`GtkPaned` + `GtkListBox`).
//!
//! As in GNOME's Settings and Files, the window is split into left and right panes,
//! with the sidebar list in the left pane.
//!
//! | naui | GTK4 / libadwaita |
//! | --- | --- |
//! | The whole sidebar | `GtkPaned` (the window's content, the same as naui's `SplitView`) |
//! | Left pane | `AdwToolbarView` + `AdwHeaderBar` + `GtkScrolledWindow` |
//! | Item list | `GtkListBox` (`.navigation-sidebar`) |
//! | Item | `GtkListBoxRow` (`GtkImage` + `GtkLabel`) |
//! | Group heading | A `GtkLabel` in an unselectable row (`.heading` / `.dim-label`) |
//! | Between groups | A `GtkSeparator` in an unselectable row |
//! | Right pane | The window's `AdwToolbarView` (header bar + child) |
//!
//! libadwaita's `AdwOverlaySplitView` does not let the user change the width, so it
//! is not used. To let the width be changed with a divider as on the other three
//! platforms, the window is split with `GtkPaned`, GTK's standard movable divider.
//! Each pane having its own header bar follows GNOME conventions; inside a
//! `GtkPaned`, libadwaita does not decide which window buttons to show, so naui
//! switches them so they appear only on the side touching the window edge.
//!
//! It is collapsed and expanded with the sidebar button (a `GtkToggleButton` with
//! `sidebar-show-symbolic`). As on the other three platforms, it sits at the left
//! end of the sidebar's header bar while expanded, and at the left end of the
//! content's header bar while collapsed.
//!
//! libadwaita 1.9 has `AdwSidebar`, which handles the list too, but 1.5 (Ubuntu
//! 24.04), which naui targets, does not. As recommended up to 1.5, it is built from
//! a `GtkListBox` with `.navigation-sidebar`.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use gtk::glib;
use naui_core::{
    sidebar_item, sidebar_len, sidebar_rows, ui_strings, SidebarItem, SidebarRow, SidebarSection,
    DEFAULT_SIDEBAR_WIDTH, SIDEBAR_MIN_WIDTH,
};

use crate::callback::Notifier;
use crate::widgets::without_signal;

struct SidebarInner {
    paned: gtk::Paned,
    /// The left pane (header bar and list). Hidden when collapsed.
    pane: adw::ToolbarView,
    /// The left pane's header bar. The sidebar button comes here while expanded.
    header: adw::HeaderBar,
    /// The container placed at the left end of the content's header bar. The sidebar
    /// button comes here while collapsed. Hidden while empty.
    content_slot: gtk::Box,
    /// The header bar of the window it is attached to (the content side).
    content_header: RefCell<Option<adw::HeaderBar>>,
    list: gtk::ListBox,
    sections: RefCell<Vec<SidebarSection>>,
    /// The rows, in the same order as the items' running numbers.
    rows: RefCell<Vec<gtk::ListBoxRow>>,
    selected: Cell<Option<usize>>,
    on_select: Notifier<usize>,
    /// The `row-selected` subscription. Blocked only while selecting from code.
    handler: RefCell<Option<glib::SignalHandlerId>>,
    width: Cell<f64>,
    on_resize: Notifier<f64>,
    /// True while naui is positioning the divider. Position changes during this are not
    /// notified.
    applying: Cell<bool>,
    /// The sidebar button.
    toggle: gtk::ToggleButton,
    /// The last known collapsed state. Compared so that only the user's toggles are
    /// notified.
    collapsed: Cell<bool>,
    on_collapse: Notifier<bool>,
}

/// A sidebar attached to the left of the window.
///
/// Attach it with [`Window::set_sidebar`](crate::Window::set_sidebar). It is not
/// placed in a layout, so it is not a [`Widget`](crate::Widget).
#[derive(Clone)]
pub struct Sidebar(Rc<SidebarInner>);

impl Sidebar {
    pub(crate) fn new() -> Self {
        let list = gtk::ListBox::new();
        list.add_css_class("navigation-sidebar");
        list.set_selection_mode(gtk::SelectionMode::Single);

        let scroll = gtk::ScrolledWindow::new();
        scroll.set_hscrollbar_policy(gtk::PolicyType::Never);
        scroll.set_vexpand(true);
        scroll.set_child(Some(&list));

        // 左の区画にもヘッダーバーを置く。タイトルは右の区画が出すので
        // こちらは出さない (「設定」と同じ並び)。
        let header = adw::HeaderBar::new();
        header.set_show_title(false);
        let pane = adw::ToolbarView::new();
        pane.add_top_bar(&header);
        pane.set_content(Some(&scroll));
        // 仕切りで狭められる下限。
        pane.set_size_request(SIDEBAR_MIN_WIDTH.round() as i32, -1);

        let paned = gtk::Paned::new(gtk::Orientation::Horizontal);
        paned.set_start_child(Some(&pane));
        // ウィンドウを広げた分は中身が受け取り、サイドバーは幅を保つ。
        paned.set_resize_start_child(false);
        paned.set_shrink_start_child(false);
        paned.set_resize_end_child(true);
        paned.set_shrink_end_child(false);

        let toggle = gtk::ToggleButton::new();
        toggle.set_icon_name("sidebar-show-symbolic");
        toggle.set_tooltip_text(Some(ui_strings().sidebar));
        // 押し込まれている = 開いている。区画の表示と双方向につなぐ。
        pane.bind_property("visible", &toggle, "active")
            .bidirectional()
            .sync_create()
            .build();
        header.pack_start(&toggle);
        let content_slot = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        content_slot.set_visible(false);

        let this = Self(Rc::new(SidebarInner {
            paned,
            pane,
            header,
            content_slot,
            content_header: RefCell::new(None),
            list,
            sections: RefCell::new(Vec::new()),
            rows: RefCell::new(Vec::new()),
            selected: Cell::new(None),
            on_select: Notifier::default(),
            handler: RefCell::new(None),
            width: Cell::new(DEFAULT_SIDEBAR_WIDTH),
            on_resize: Notifier::default(),
            applying: Cell::new(false),
            toggle,
            collapsed: Cell::new(false),
            on_collapse: Notifier::default(),
        }));
        this.apply_width();

        // 利用者がボタンで開閉したら通知する。`set_collapsed` は先に覚えて
        // おくので、ここでは食い違ったときだけが利用者の操作になる。
        let weak = Rc::downgrade(&this.0);
        this.0
            .pane
            .connect_notify_local(Some("visible"), move |_, _| {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                let sidebar = Sidebar(inner);
                sidebar.arrange_for_collapse();
                let collapsed = sidebar.is_collapsed();
                if sidebar.0.collapsed.replace(collapsed) != collapsed {
                    sidebar.0.on_collapse.emit(collapsed);
                }
            });

        // 利用者が仕切りを動かしたら通知する。
        let weak = Rc::downgrade(&this.0);
        this.0
            .paned
            .connect_notify_local(Some("position"), move |paned, _| {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                if inner.applying.get() || !inner.pane.get_visible() {
                    return;
                }
                let width = f64::from(paned.position());
                if width > 0.0 && (width - inner.width.replace(width)).abs() >= 0.5 {
                    inner.on_resize.emit(width);
                }
            });

        // ハンドルを強く持つと循環するので弱参照にする。
        let weak = Rc::downgrade(&this.0);
        let id = this.0.list.connect_row_selected(move |_, row| {
            let Some(inner) = weak.upgrade() else {
                return;
            };
            let index = row.and_then(|row| {
                inner
                    .rows
                    .borrow()
                    .iter()
                    .position(|candidate| candidate == row)
            });
            inner.selected.set(index);
            if let Some(index) = index {
                inner.on_select.emit(index);
            }
        });
        *this.0.handler.borrow_mut() = Some(id);
        this
    }

    /// Lays out items by group. Each call replaces them.
    ///
    /// The selected item stays selected (without notifying) if the same running number
    /// can still be selected.
    pub fn set_sections(&self, sections: &[SidebarSection]) {
        let keep = self
            .0
            .selected
            .get()
            .filter(|&index| sidebar_item(sections, index).is_some_and(|item| item.enabled));
        *self.0.sections.borrow_mut() = sections.to_vec();
        without_signal(&self.0.list, &self.0.handler, || {
            // `remove_all` は GTK 4.12 から。対象の 4.10 でも動くよう 1 つずつ外す。
            while let Some(child) = self.0.list.first_child() {
                self.0.list.remove(&child);
            }
            let mut rows = Vec::new();
            for row in sidebar_rows(sections) {
                match row {
                    SidebarRow::Gap => {
                        // 既定の `valign` (Fill) のままだと、線が行の高さ
                        // いっぱいに塗られて灰色の帯になる。
                        let line = gtk::Separator::new(gtk::Orientation::Horizontal);
                        line.set_valign(gtk::Align::Center);
                        self.0.list.append(&inert_row(&line));
                    }
                    SidebarRow::Header(title) => {
                        let label = gtk::Label::new(Some(title));
                        label.set_xalign(0.0);
                        label.add_css_class("heading");
                        label.add_css_class("dim-label");
                        self.0.list.append(&inert_row(&label));
                    }
                    SidebarRow::Item(_, item) => {
                        let row = item_row(item);
                        self.0.list.append(&row);
                        rows.push(row);
                    }
                }
            }
            *self.0.rows.borrow_mut() = rows;
        });
        self.0.selected.set(None);
        if let Some(index) = keep {
            self.set_selected(index);
        }
    }

    /// Lays out items as a single group without a heading.
    pub fn set_items(&self, items: &[SidebarItem]) {
        self.set_sections(&[SidebarSection::untitled(items.iter().cloned())]);
    }

    /// The number of items across groups.
    pub fn len(&self) -> usize {
        sidebar_len(&self.0.sections.borrow())
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The running number of the selected item.
    pub fn selected(&self) -> Option<usize> {
        self.0.selected.get()
    }

    /// Changes the selection without notifying. Out-of-range and unselectable items are
    /// ignored.
    pub fn set_selected(&self, index: usize) {
        let Some(row) = self.selectable_row(index) else {
            return;
        };
        without_signal(&self.0.list, &self.0.handler, || {
            self.0.list.select_row(Some(&row));
        });
        self.0.selected.set(Some(index));
    }

    /// Clears the selection (without notifying).
    pub fn clear_selection(&self) {
        without_signal(&self.0.list, &self.0.handler, || {
            self.0.list.unselect_all();
        });
        self.0.selected.set(None);
    }

    /// Selects as if the user had chosen it, and notifies.
    ///
    /// Out-of-range and unselectable items are ignored. Notifies even if the item is
    /// already selected.
    pub fn select(&self, index: usize) {
        if self.selectable_row(index).is_none() {
            return;
        }
        self.set_selected(index);
        self.0.on_select.emit(index);
    }

    /// The handler called when an item is chosen. The argument is the running number.
    pub fn on_select(&self, f: impl FnMut(usize) + 'static) {
        self.0.on_select.set(f);
    }

    /// The sidebar width (logical pixels). The default is [`DEFAULT_SIDEBAR_WIDTH`].
    ///
    /// The user can change the width by dragging the divider (the minimum is
    /// [`SIDEBAR_MIN_WIDTH`]). The width after a change is returned by
    /// [`width`](Self::width) and delivered through [`on_resize`](Self::on_resize).
    /// This call does not call `on_resize`.
    pub fn set_width(&self, width: f64) {
        if !width.is_finite() || width <= 0.0 {
            return;
        }
        self.0.width.set(width.max(SIDEBAR_MIN_WIDTH));
        self.apply_width();
    }

    /// The sidebar width. Even while collapsed, returns the width it has when expanded.
    pub fn width(&self) -> f64 {
        self.0.width.get()
    }

    /// Called with the new width every time the user changes it with the divider.
    pub fn on_resize(&self, f: impl FnMut(f64) + 'static) {
        self.0.on_resize.set(f);
    }

    fn apply_width(&self) {
        self.0.applying.set(true);
        self.0.paned.set_position(self.0.width.get().round() as i32);
        self.0.applying.set(false);
    }

    /// Collapses (`true`) or expands (`false`) the sidebar.
    ///
    /// Items, selection and width are kept while collapsed.
    /// [`on_collapse`](Self::on_collapse) is not called.
    pub fn set_collapsed(&self, collapsed: bool) {
        self.0.collapsed.set(collapsed);
        self.0.pane.set_visible(!collapsed);
        if !collapsed {
            // 隠している間に `GtkPaned` が位置を動かしていることがあるので戻す。
            self.apply_width();
        }
    }

    /// The handler called when the user collapses or expands the sidebar. The argument
    /// is whether it is now collapsed.
    ///
    /// Called when the sidebar button is used; not called by
    /// [`set_collapsed`](Self::set_collapsed).
    pub fn on_collapse(&self, f: impl FnMut(bool) + 'static) {
        self.0.on_collapse.set(f);
    }

    /// The sidebar button. A backend-specific escape hatch.
    pub fn native_toggle_button(&self) -> gtk::ToggleButton {
        self.0.toggle.clone()
    }

    /// Whether the sidebar is collapsed.
    ///
    /// This looks at the pane's own visibility setting (`get_visible`). `is_visible`
    /// reports whether it is visible including its parents, so it would be false before
    /// the window is shown.
    pub fn is_collapsed(&self) -> bool {
        !self.0.pane.get_visible()
    }

    /// The corresponding `GtkPaned`. A backend-specific escape hatch.
    pub fn native_paned(&self) -> gtk::Paned {
        self.0.paned.clone()
    }

    /// The left pane's header bar. A backend-specific escape hatch.
    pub fn native_header_bar(&self) -> adw::HeaderBar {
        self.0.header.clone()
    }

    /// The container placed at the left end of the content's header bar. Where the
    /// sidebar button goes while collapsed.
    pub(crate) fn content_slot(&self) -> gtk::Box {
        self.0.content_slot.clone()
    }

    /// Called when attached to or detached from a window.
    ///
    /// Remembers the content's header bar and adjusts which window buttons are shown.
    pub(crate) fn set_content_header(&self, header: Option<&adw::HeaderBar>) {
        if header.is_none() {
            // 外すときは、中身のヘッダーバーを元の姿 (両端にボタン) へ戻す。
            if let Some(old) = self.0.content_header.borrow_mut().take() {
                old.set_show_start_title_buttons(true);
            }
            return;
        }
        *self.0.content_header.borrow_mut() = header.cloned();
        self.arrange_for_collapse();
    }

    /// Adjusts where the sidebar button is and which window buttons are shown to match
    /// the collapsed state.
    ///
    /// While expanded, the sidebar pane touches the window's left edge, so the
    /// sidebar's header bar shows the left-side window buttons and the content's header
    /// bar shows the right-side ones. When collapsed, the content's header bar shows
    /// both.
    ///
    /// Moving the button is deferred to the next iteration. Reattaching the clicked
    /// button itself in the middle of its `toggled` would leave the press cleanup lost.
    fn arrange_for_collapse(&self) {
        let collapsed = self.is_collapsed();
        self.0.header.set_show_end_title_buttons(false);
        if let Some(content) = self.0.content_header.borrow().as_ref() {
            content.set_show_start_title_buttons(collapsed);
        }
        let weak = Rc::downgrade(&self.0);
        glib::idle_add_local_once(move || {
            if let Some(inner) = weak.upgrade() {
                Sidebar(inner).place_toggle();
            }
        });
    }

    /// Places the sidebar button at the top left of the sidebar when expanded, or at
    /// the top left of the content when collapsed.
    fn place_toggle(&self) {
        let toggle = &self.0.toggle;
        let collapsed = self.is_collapsed();
        let in_slot = toggle.parent().as_ref() == Some(self.0.content_slot.upcast_ref());
        if collapsed && !in_slot {
            if toggle.parent().is_some() {
                self.0.header.remove(toggle);
            }
            self.0.content_slot.append(toggle);
        } else if !collapsed && in_slot {
            self.0.content_slot.remove(toggle);
            self.0.header.pack_start(toggle);
        }
        self.0.content_slot.set_visible(collapsed);
    }

    /// The item list (`GtkListBox`). A backend-specific escape hatch.
    pub fn native_list_box(&self) -> gtk::ListBox {
        self.0.list.clone()
    }

    fn selectable_row(&self, index: usize) -> Option<gtk::ListBoxRow> {
        let enabled =
            sidebar_item(&self.0.sections.borrow(), index).is_some_and(|item| item.enabled);
        if !enabled {
            return None;
        }
        self.0.rows.borrow().get(index).cloned()
    }
}

/// A row that can be neither selected nor clicked (headings and separators).
fn inert_row(child: &impl IsA<gtk::Widget>) -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::new();
    row.set_selectable(false);
    row.set_activatable(false);
    row.set_can_focus(false);
    row.set_child(Some(child));
    row
}

/// One item row. The icon, if any, goes before the text.
fn item_row(item: &SidebarItem) -> gtk::ListBoxRow {
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    if let Some(icon) = item.icon {
        content.append(&gtk::Image::from_icon_name(icon.icon_name()));
    }
    let label = gtk::Label::new(Some(&item.label));
    label.set_xalign(0.0);
    label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    content.append(&label);

    let row = gtk::ListBoxRow::new();
    row.set_child(Some(&content));
    row.set_selectable(item.enabled);
    row.set_activatable(item.enabled);
    row.set_sensitive(item.enabled);
    row
}
