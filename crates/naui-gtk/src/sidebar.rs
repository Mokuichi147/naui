//! サイドバー (`AdwOverlaySplitView` + `GtkListBox`)。
//!
//! GNOME の「設定」やファイルと同じく、ウィンドウを左右の区画に分け、
//! 左の区画にサイドバーの一覧を置く。
//!
//! | naui | GTK4 / libadwaita |
//! | --- | --- |
//! | サイドバー全体 | `AdwOverlaySplitView` (ウィンドウの中身) |
//! | 左の区画 | `AdwToolbarView` + `AdwHeaderBar` + `GtkScrolledWindow` |
//! | 項目の一覧 | `GtkListBox` (`.navigation-sidebar`) |
//! | 項目 | `GtkListBoxRow` (`GtkImage` + `GtkLabel`) |
//! | まとまりの見出し | 選べない行の `GtkLabel` (`.heading` / `.dim-label`) |
//! | まとまりの間 | 選べない行の `GtkSeparator` |
//! | 右の区画 | ウィンドウの `AdwToolbarView` (ヘッダーバー + 子) |
//!
//! 左右の区画がそれぞれヘッダーバーを持つのが GNOME の作法で、閉じる・
//! 最小化のボタンはウィンドウの端に接するほうへ libadwaita が寄せる。
//!
//! libadwaita 1.9 には一覧まで引き受ける `AdwSidebar` があるが、naui が
//! 対象にしている 1.5 (Ubuntu 24.04) には無い。1.5 までの推奨どおり、
//! `.navigation-sidebar` を付けた `GtkListBox` で組む。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use gtk::glib;
use naui_core::{
    sidebar_item, sidebar_len, sidebar_rows, SidebarItem, SidebarRow, SidebarSection,
    DEFAULT_SIDEBAR_WIDTH,
};

use crate::callback::Notifier;
use crate::widgets::without_signal;

struct SidebarInner {
    split: adw::OverlaySplitView,
    list: gtk::ListBox,
    sections: RefCell<Vec<SidebarSection>>,
    /// 項目の通し番号と同じ並びの行。
    rows: RefCell<Vec<gtk::ListBoxRow>>,
    selected: Cell<Option<usize>>,
    on_select: Notifier<usize>,
    /// `row-selected` の購読。プログラムから選ぶ間だけ止める。
    handler: RefCell<Option<glib::SignalHandlerId>>,
    width: Cell<f64>,
}

/// ウィンドウの左に付けるサイドバー。
///
/// [`Window::set_sidebar`](crate::Window::set_sidebar) で取り付ける。
/// レイアウトには置かないので [`Widget`](crate::Widget) ではない。
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

        let split = adw::OverlaySplitView::new();
        split.set_sidebar(Some(&pane));
        // 幅が足りなくても自動では重ねない。開閉は `set_collapsed` で行う。
        split.set_collapsed(false);
        split.set_show_sidebar(true);

        let this = Self(Rc::new(SidebarInner {
            split,
            list,
            sections: RefCell::new(Vec::new()),
            rows: RefCell::new(Vec::new()),
            selected: Cell::new(None),
            on_select: Notifier::default(),
            handler: RefCell::new(None),
            width: Cell::new(DEFAULT_SIDEBAR_WIDTH),
        }));
        this.apply_width();

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

    /// 項目をまとまりごとに並べる。呼ぶたびに置き換わる。
    ///
    /// 選ばれていた項目は、同じ通し番号がまだ選べるなら選ばれたまま残る
    /// (通知はしない)。
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

    /// 見出しの無いまとまり 1 つだけで並べる。
    pub fn set_items(&self, items: &[SidebarItem]) {
        self.set_sections(&[SidebarSection::untitled(items.iter().cloned())]);
    }

    /// まとまりをまたいだ項目の数。
    pub fn len(&self) -> usize {
        sidebar_len(&self.0.sections.borrow())
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// 選ばれている項目の通し番号。
    pub fn selected(&self) -> Option<usize> {
        self.0.selected.get()
    }

    /// 通知せずに選択を変える。範囲外・選べない項目は無視する。
    pub fn set_selected(&self, index: usize) {
        let Some(row) = self.selectable_row(index) else {
            return;
        };
        without_signal(&self.0.list, &self.0.handler, || {
            self.0.list.select_row(Some(&row));
        });
        self.0.selected.set(Some(index));
    }

    /// 選択を外す (通知しない)。
    pub fn clear_selection(&self) {
        without_signal(&self.0.list, &self.0.handler, || {
            self.0.list.unselect_all();
        });
        self.0.selected.set(None);
    }

    /// 利用者が選んだのと同じように選び、通知する。
    ///
    /// 範囲外・選べない項目は無視する。すでに選ばれている項目でも通知する。
    pub fn select(&self, index: usize) {
        if self.selectable_row(index).is_none() {
            return;
        }
        self.set_selected(index);
        self.0.on_select.emit(index);
    }

    /// 項目が選ばれたときの通知先。引数は通し番号。
    pub fn on_select(&self, f: impl FnMut(usize) + 'static) {
        self.0.on_select.set(f);
    }

    /// サイドバーの幅 (論理ピクセル)。既定は [`DEFAULT_SIDEBAR_WIDTH`]。
    ///
    /// libadwaita の幅は「全体に対する割合を上下限で挟む」決め方なので、
    /// 上限と下限を同じ値にして固定する。
    pub fn set_width(&self, width: f64) {
        if !width.is_finite() || width <= 0.0 {
            return;
        }
        self.0.width.set(width);
        self.apply_width();
    }

    /// サイドバーの幅。閉じていても開いたときの幅を返す。
    pub fn width(&self) -> f64 {
        self.0.width.get()
    }

    fn apply_width(&self) {
        let width = self.0.width.get();
        // 下限が上限を超える瞬間を作らないよう、いったん下限を外してから置く。
        self.0.split.set_min_sidebar_width(0.0);
        self.0.split.set_max_sidebar_width(width);
        self.0.split.set_min_sidebar_width(width);
    }

    /// サイドバーを閉じる (`true`) か開く (`false`)。
    ///
    /// 閉じても項目と選択は残る。
    pub fn set_collapsed(&self, collapsed: bool) {
        self.0.split.set_show_sidebar(!collapsed);
    }

    /// サイドバーが閉じているかどうか。
    pub fn is_collapsed(&self) -> bool {
        !self.0.split.shows_sidebar()
    }

    /// 対応する `AdwOverlaySplitView`。バックエンド固有の脱出口。
    pub fn native_split_view(&self) -> adw::OverlaySplitView {
        self.0.split.clone()
    }

    /// 項目の一覧 (`GtkListBox`)。バックエンド固有の脱出口。
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

/// 選べも押せもしない行 (見出しと区切り)。
fn inert_row(child: &impl IsA<gtk::Widget>) -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::new();
    row.set_selectable(false);
    row.set_activatable(false);
    row.set_can_focus(false);
    row.set_child(Some(child));
    row
}

/// 項目 1 行。アイコンがあれば文字の前に置く。
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
