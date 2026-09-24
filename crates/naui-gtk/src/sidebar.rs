//! サイドバー (`GtkPaned` + `GtkListBox`)。
//!
//! GNOME の「設定」やファイルと同じく、ウィンドウを左右の区画に分け、
//! 左の区画にサイドバーの一覧を置く。
//!
//! | naui | GTK4 / libadwaita |
//! | --- | --- |
//! | サイドバー全体 | `GtkPaned` (ウィンドウの中身。naui の `SplitView` と同じ) |
//! | 左の区画 | `AdwToolbarView` + `AdwHeaderBar` + `GtkScrolledWindow` |
//! | 項目の一覧 | `GtkListBox` (`.navigation-sidebar`) |
//! | 項目 | `GtkListBoxRow` (`GtkImage` + `GtkLabel`) |
//! | まとまりの見出し | 選べない行の `GtkLabel` (`.heading` / `.dim-label`) |
//! | まとまりの間 | 選べない行の `GtkSeparator` |
//! | 右の区画 | ウィンドウの `AdwToolbarView` (ヘッダーバー + 子) |
//!
//! libadwaita の `AdwOverlaySplitView` は幅を利用者が変えられないので使わない。
//! ほかの 3 環境と同じく仕切りで幅を変えられるよう、GTK 標準の動かせる仕切り
//! である `GtkPaned` で分ける。左右の区画がそれぞれヘッダーバーを持つのは
//! GNOME の作法どおりで、`GtkPaned` の中では libadwaita がウィンドウの
//! ボタンの出し分けをしないため、ウィンドウの端に接するほうにだけ出るよう
//! naui が切り替える。
//!
//! 開閉はサイドバーボタン (`sidebar-show-symbolic` の `GtkToggleButton`) で
//! 行う。ほかの 3 環境とそろえて、開いている間はサイドバーのヘッダーバーの
//! 左端、閉じている間は中身のヘッダーバーの左端に置く。
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
    DEFAULT_SIDEBAR_WIDTH, SIDEBAR_MIN_WIDTH,
};

use crate::callback::Notifier;
use crate::widgets::without_signal;

struct SidebarInner {
    paned: gtk::Paned,
    /// 左の区画 (ヘッダーバーと一覧)。閉じるとこれを隠す。
    pane: adw::ToolbarView,
    /// 左の区画のヘッダーバー。開いている間はサイドバーボタンがここに来る。
    header: adw::HeaderBar,
    /// 中身のヘッダーバーの左端に置く入れ物。閉じている間はサイドバー
    /// ボタンがここに来る。空の間は隠す。
    content_slot: gtk::Box,
    /// 取り付け先のウィンドウのヘッダーバー (中身の側)。
    content_header: RefCell<Option<adw::HeaderBar>>,
    list: gtk::ListBox,
    sections: RefCell<Vec<SidebarSection>>,
    /// 項目の通し番号と同じ並びの行。
    rows: RefCell<Vec<gtk::ListBoxRow>>,
    selected: Cell<Option<usize>>,
    on_select: Notifier<usize>,
    /// `row-selected` の購読。プログラムから選ぶ間だけ止める。
    handler: RefCell<Option<glib::SignalHandlerId>>,
    width: Cell<f64>,
    on_resize: Notifier<f64>,
    /// naui が仕切りを置いている間は真。この間の位置の変化は通知しない。
    applying: Cell<bool>,
    /// サイドバーボタン。
    toggle: gtk::ToggleButton,
    /// 最後に知っている開閉。利用者の開閉だけを通知するために比べる。
    collapsed: Cell<bool>,
    on_collapse: Notifier<bool>,
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
        toggle.set_tooltip_text(Some("サイドバー"));
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
    /// 利用者は仕切りをドラッグして幅を変えられる (下限は
    /// [`SIDEBAR_MIN_WIDTH`])。変えたあとの幅は [`width`](Self::width) が返し、
    /// [`on_resize`](Self::on_resize) で届く。この呼び出しでは `on_resize` を
    /// 呼ばない。
    pub fn set_width(&self, width: f64) {
        if !width.is_finite() || width <= 0.0 {
            return;
        }
        self.0.width.set(width.max(SIDEBAR_MIN_WIDTH));
        self.apply_width();
    }

    /// サイドバーの幅。閉じていても開いたときの幅を返す。
    pub fn width(&self) -> f64 {
        self.0.width.get()
    }

    /// 利用者が仕切りで幅を変えるたび、変えた後の幅で呼ばれる。
    pub fn on_resize(&self, f: impl FnMut(f64) + 'static) {
        self.0.on_resize.set(f);
    }

    fn apply_width(&self) {
        self.0.applying.set(true);
        self.0.paned.set_position(self.0.width.get().round() as i32);
        self.0.applying.set(false);
    }

    /// サイドバーを閉じる (`true`) か開く (`false`)。
    ///
    /// 閉じても項目と選択と幅は残る。[`on_collapse`](Self::on_collapse) は
    /// 呼ばない。
    pub fn set_collapsed(&self, collapsed: bool) {
        self.0.collapsed.set(collapsed);
        self.0.pane.set_visible(!collapsed);
        if !collapsed {
            // 隠している間に `GtkPaned` が位置を動かしていることがあるので戻す。
            self.apply_width();
        }
    }

    /// 利用者がサイドバーを開閉したときの通知先。引数は閉じたかどうか。
    ///
    /// サイドバーボタンの操作で呼ばれ、[`set_collapsed`](Self::set_collapsed)
    /// では呼ばれない。
    pub fn on_collapse(&self, f: impl FnMut(bool) + 'static) {
        self.0.on_collapse.set(f);
    }

    /// サイドバーボタン。バックエンド固有の脱出口。
    pub fn native_toggle_button(&self) -> gtk::ToggleButton {
        self.0.toggle.clone()
    }

    /// サイドバーが閉じているかどうか。
    ///
    /// 見るのは区画自身の表示の指定 (`get_visible`)。`is_visible` は親まで
    /// 含めて見えているかを返すので、ウィンドウを出す前は偽になってしまう。
    pub fn is_collapsed(&self) -> bool {
        !self.0.pane.get_visible()
    }

    /// 対応する `GtkPaned`。バックエンド固有の脱出口。
    pub fn native_paned(&self) -> gtk::Paned {
        self.0.paned.clone()
    }

    /// 左の区画のヘッダーバー。バックエンド固有の脱出口。
    pub fn native_header_bar(&self) -> adw::HeaderBar {
        self.0.header.clone()
    }

    /// 中身のヘッダーバーの左端に置く入れ物。閉じている間のサイドバー
    /// ボタンの置き場。
    pub(crate) fn content_slot(&self) -> gtk::Box {
        self.0.content_slot.clone()
    }

    /// ウィンドウへ取り付けたとき・外したときに呼ぶ。
    ///
    /// 中身のヘッダーバーを覚え、ウィンドウのボタンの出し分けを合わせる。
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

    /// 開閉に合わせて、サイドバーボタンの場所とウィンドウのボタンの出し分けを
    /// そろえる。
    ///
    /// 開いている間はサイドバーの区画がウィンドウの左端に接するので、左側の
    /// ウィンドウのボタンはサイドバーのヘッダーバーが、右側は中身のヘッダー
    /// バーが出す。閉じたら中身のヘッダーバーが両方を出す。
    ///
    /// ボタンの付け替えは次の周回へ回す。押されたボタンの `toggled` の最中に
    /// 自分自身を付け替えると、押下の後始末が迷子になるため。
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

    /// サイドバーボタンを、開いていればサイドバーの左上、閉じていれば中身の
    /// 左上へ置く。
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
