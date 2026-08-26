//! テーブル (`GtkListBox` の行を横並びにしたもの)。
//!
//! GTK4 の `GtkColumnView` は `GtkListItemFactory` と `GListModel` を組み合わせ、
//! 行のデータを GObject にして渡す作りになっている。naui のウィジェットは
//! 「値の並びを渡すと表示が変わる」形なので、[`crate::List`] と同じ
//! `GtkListBox` の上に組み立てている。
//!
//! | 部分 | 作り |
//! | --- | --- |
//! | 枠 | `GtkBox` (縦) に `frame` スタイルクラス |
//! | 見出し | `GtkBox` (横) + `GtkLabel`、下に `GtkSeparator` |
//! | 本体 | `GtkScrolledWindow` + `GtkListBox`。行は `GtkBox` (横) + `GtkLabel` |
//! | 列幅 | 列ごとの `GtkSizeGroup` に、見出しと全行のセルを入れてそろえる |
//!
//! 列の幅をドラッグで変えることはできない (`NSTableView` と違い、
//! `GtkListBox` にはそのための仕組みが無いため)。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::glib;
use gtk::pango;
use gtk::prelude::*;
use naui_core::{Align, SelectionMode, SortOrder, TableColumn, TableRow};

use crate::bin::SizeBin;
use crate::callback::{SelectionNotifier, SortNotifier};
use crate::widgets::{impl_widget, without_signal, Widget};

/// セルどうしの間隔。
const CELL_SPACING: i32 = 12;
/// 見出しと行の、左右の余白。
const SIDE_MARGIN: i32 = 10;
/// 見出しと行の、上下の余白。
const VERTICAL_MARGIN: i32 = 6;

/// GTK の文字揃えへ写す。`Fill` は文字に意味が無いので左と同じ扱い。
fn xalign(align: Align) -> f32 {
    match align {
        Align::Center => 0.5,
        Align::End => 1.0,
        Align::Start | Align::Fill => 0.0,
    }
}

/// 並べ替えの向きを表す文字。見出しの文字の後ろへ付ける。
fn sort_arrow(order: Option<SortOrder>) -> &'static str {
    match order {
        Some(SortOrder::Ascending) => " ▲",
        Some(SortOrder::Descending) => " ▼",
        None => "",
    }
}

/// セル 1 つ分のラベルを作る。列の幅の決め方もここで反映する。
fn cell_label(text: &str, column: &TableColumn, group: &gtk::SizeGroup) -> gtk::Label {
    let label = gtk::Label::new(Some(text));
    label.set_xalign(xalign(column.align));
    // 列より長い文字は折り返さず、末尾を省略する。
    label.set_ellipsize(pango::EllipsizeMode::End);
    match column.width {
        // 幅の指定がある列は、その幅のまま。余りは受け取らない。
        Some(width) => {
            label.set_size_request(width as i32, -1);
            label.set_hexpand(false);
        }
        // 指定が無い列だけで、余った幅を分け合う。
        None => label.set_hexpand(true),
    }
    // 見出しと全行の同じ列を 1 つの `GtkSizeGroup` に入れると、
    // どの行も同じ幅になる。
    group.add_widget(&label);
    label
}

struct TableInner {
    /// 外から見えるウィジェット。見出しと本体を縦に並べた入れ物。
    native: gtk::Box,
    /// 見出しの行。列を変えるたびに中身を作り直す。
    header: gtk::Box,
    list: gtk::ListBox,
    /// `GtkListBox` は自分でスクロールしないので、スクロール領域に載せる。
    _scroller: gtk::ScrolledWindow,
    bin: SizeBin,
    columns: RefCell<Vec<TableColumn>>,
    rows: RefCell<Vec<TableRow>>,
    /// 列ごとの `GtkSizeGroup`。組み立てのたびに作り直す。
    groups: RefCell<Vec<gtk::SizeGroup>>,
    /// 見出しに置いたラベル。並べ替えの指標の書き替えに使う。
    /// 押せる列ではボタンの中身になっている。
    header_labels: RefCell<Vec<gtk::Label>>,
    /// いまの並べ替え (列と向き)。
    sort: Cell<Option<(usize, SortOrder)>>,
    on_select: SelectionNotifier,
    on_sort: SortNotifier,
    handler: RefCell<Option<glib::SignalHandlerId>>,
}

/// 列見出しを持つ表。自分でスクロールする。
///
/// 高さは中身から決まらないので、[`Table::set_sizing`] で指定しておく。
#[derive(Clone)]
pub struct Table(Rc<TableInner>);
impl_widget!(Table);

impl Table {
    pub(crate) fn new() -> Self {
        let list = gtk::ListBox::new();
        list.set_selection_mode(gtk::SelectionMode::Single);

        let scroller = gtk::ScrolledWindow::new();
        scroller.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        // 枠は外側の入れ物が持つので、こちらには付けない。
        scroller.set_has_frame(false);
        scroller.set_vexpand(true);
        scroller.set_child(Some(&list));

        let header = gtk::Box::new(gtk::Orientation::Horizontal, CELL_SPACING);
        header.set_margin_start(SIDE_MARGIN);
        header.set_margin_end(SIDE_MARGIN);
        header.set_margin_top(VERTICAL_MARGIN);
        header.set_margin_bottom(VERTICAL_MARGIN);

        let native = gtk::Box::new(gtk::Orientation::Vertical, 0);
        // 一覧の枠。`GtkScrolledWindow` の枠と同じ見た目になる標準クラス。
        native.add_css_class("frame");
        native.append(&header);
        native.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        native.append(&scroller);

        let bin = SizeBin::wrap(&native);
        let inner = Rc::new(TableInner {
            native,
            header,
            list,
            _scroller: scroller,
            bin,
            columns: RefCell::new(Vec::new()),
            rows: RefCell::new(Vec::new()),
            groups: RefCell::new(Vec::new()),
            header_labels: RefCell::new(Vec::new()),
            sort: Cell::new(None),
            on_select: SelectionNotifier::default(),
            on_sort: SortNotifier::default(),
            handler: RefCell::new(None),
        });
        // 選択の通知は常時つないでおき、プログラムから変えるときだけ止める。
        let id = {
            let weak = Rc::downgrade(&inner);
            inner.list.connect_selected_rows_changed(move |_| {
                if let Some(inner) = weak.upgrade() {
                    let table = Table(inner);
                    let selection = table.selection();
                    table.0.on_select.emit(&selection);
                }
            })
        };
        *inner.handler.borrow_mut() = Some(id);
        Self(inner)
    }

    /// 列を作り直す。行と選択はそのまま残り、セルの並べ直しだけが起きる。
    ///
    /// 並べ替えの指定も、その列がまだ並べ替えられるなら残る。
    pub fn set_columns(&self, columns: &[TableColumn]) {
        // 行を作り直すと選択も落ちるので、覚えて書き戻す。
        let picked = self.selection();
        let sort = self
            .0
            .sort
            .get()
            .filter(|&(column, _)| columns.get(column).is_some_and(|spec| spec.sortable));
        self.0.sort.set(sort);
        {
            let mut stored = self.0.columns.borrow_mut();
            stored.clear();
            stored.extend_from_slice(columns);
        }
        self.rebuild();
        without_signal(&self.0.list, &self.0.handler, || self.show(&picked));
    }

    /// 列数。
    pub fn column_count(&self) -> usize {
        self.0.columns.borrow().len()
    }

    /// 行を作り直す。インデックスの意味が変わるため、選択は外れる。
    pub fn set_rows(&self, rows: &[TableRow]) {
        {
            let mut stored = self.0.rows.borrow_mut();
            stored.clear();
            stored.extend_from_slice(rows);
        }
        self.rebuild();
    }

    /// 行数。
    pub fn len(&self) -> usize {
        self.0.rows.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// 選び方を変える。選択の意味が変わるため、選択は外れる。
    pub fn set_selection_mode(&self, mode: SelectionMode) {
        let multiple = mode.is_multiple();
        self.0.list.set_selection_mode(if multiple {
            // `Multiple` は「⌘ / Ctrl や Shift を押しながら選ぶ」形。
            gtk::SelectionMode::Multiple
        } else {
            gtk::SelectionMode::Single
        });
        // `GtkListBox` は「1 クリックで確定」(既定) の間、クリックに付いている
        // Ctrl / Shift を読まない。複数選択ではこれを切る (`List` と同じ)。
        self.0.list.set_activate_on_single_click(!multiple);
        // GTK4 がモードの変更で選択を落とすとは限らないので、明示的に外す。
        without_signal(&self.0.list, &self.0.handler, || {
            self.0.list.unselect_all();
        });
    }

    pub fn selection_mode(&self) -> SelectionMode {
        match self.0.list.selection_mode() {
            gtk::SelectionMode::Multiple => SelectionMode::Multiple,
            _ => SelectionMode::Single,
        }
    }

    /// 選ばれている行のうち、いちばん上のもの。
    pub fn selected(&self) -> Option<usize> {
        self.selection().first().copied()
    }

    /// 選ばれている行 (昇順)。単一選択なら 0 件か 1 件。
    pub fn selection(&self) -> Vec<usize> {
        let mut indices: Vec<usize> = self
            .0
            .list
            .selected_rows()
            .iter()
            .map(|row| row.index().max(0) as usize)
            .collect();
        indices.sort_unstable();
        indices
    }

    /// 通知せずに 1 行だけを選ぶ。
    pub fn set_selected(&self, index: usize) {
        self.set_selection(&[index]);
    }

    /// 通知せずに選択を置き換える。
    ///
    /// 範囲外・選べない行・重複は取り除かれ、単一選択なら先頭の 1 件だけが残る
    /// ([`SelectionMode::normalize_by`])。
    pub fn set_selection(&self, indices: &[usize]) {
        let picked = self.normalize(indices);
        without_signal(&self.0.list, &self.0.handler, || self.show(&picked));
    }

    /// 通知せずに選択をすべて外す。
    pub fn clear_selection(&self) {
        without_signal(&self.0.list, &self.0.handler, || {
            self.0.list.unselect_all();
        });
    }

    /// ユーザーが選んだのと同じ経路で 1 行を選ぶ (通知あり)。
    pub fn select(&self, index: usize) {
        self.select_many(&[index]);
    }

    /// ユーザーが選んだのと同じ経路で選択を置き換える (通知あり)。
    pub fn select_many(&self, indices: &[usize]) {
        let picked = self.normalize(indices);
        without_signal(&self.0.list, &self.0.handler, || self.show(&picked));
        self.0.on_select.emit(&picked);
    }

    /// 選択が変わるたびに、選ばれている行のインデックスで呼ばれる。
    pub fn on_select(&self, f: impl FnMut(&[usize]) + 'static) {
        self.0.on_select.set(f);
    }

    /// 見出しが押されて並べ替えの指定が変わったときに、
    /// 押された列と向きで呼ばれる。
    ///
    /// [`TableColumn::sortable`] を立てた列だけが押せる。**行を並べ替えるのは
    /// アプリの仕事**で、通知を受けたら並べ替えた行を [`Table::set_rows`] で
    /// 渡し直す (`set_rows` は選択を外すので、必要なら選び直す)。
    pub fn on_sort(&self, f: impl FnMut(usize, SortOrder) + 'static) {
        self.0.on_sort.set(f);
    }

    /// いまの並べ替えの指定 (列と向き)。押されたことが無ければ `None`。
    pub fn sort(&self) -> Option<(usize, SortOrder)> {
        self.0.sort.get()
    }

    /// 通知せずに並べ替えの指定を置き換える。見出しの指標だけが変わる。
    ///
    /// 並べ替えられない列や範囲外の列を指すと、指定は外れる。
    pub fn set_sort(&self, sort: Option<(usize, SortOrder)>) {
        let sort = sort.filter(|&(column, _)| {
            self.0
                .columns
                .borrow()
                .get(column)
                .is_some_and(|spec| spec.sortable)
        });
        self.0.sort.set(sort);
        self.apply_sort();
    }

    /// 中身の `GtkListBox`。バックエンド固有の脱出口として公開している。
    pub fn native_list_box(&self) -> gtk::ListBox {
        self.0.list.clone()
    }

    // ------------------------------------------------------------ 組み立て

    /// 見出しと行を、いまの列と行から作り直す。
    ///
    /// 列ごとの `GtkSizeGroup` は毎回作り直す。古いセルを持ったままだと、
    /// 消えた行の幅が残ってしまうため。
    fn rebuild(&self) {
        let columns = self.0.columns.borrow();
        let rows = self.0.rows.borrow();
        let groups: Vec<gtk::SizeGroup> = columns
            .iter()
            .map(|_| gtk::SizeGroup::new(gtk::SizeGroupMode::Horizontal))
            .collect();

        while let Some(child) = self.0.header.first_child() {
            self.0.header.remove(&child);
        }
        let mut header_labels = Vec::with_capacity(columns.len());
        for (index, column) in columns.iter().enumerate() {
            let label = cell_label(&column.title, column, &groups[index]);
            // 見出しは、行の文字より小さく淡くする (`List` の補助と同じ扱い)。
            label.add_css_class("dim-label");
            label.add_css_class("caption");
            match column.sortable {
                // 並べ替えられる列は、見出しそのものをボタンにする。
                // 幅をそろえる `GtkSizeGroup` の相手もボタンへ移す。
                true => {
                    let button = gtk::Button::new();
                    // 地色と枠を出さない、見出し向けの標準クラス。
                    button.add_css_class("flat");
                    button.set_hexpand(label.hexpands());
                    // 幅の指定と `GtkSizeGroup` の相手は、外側のボタンへ移す。
                    // 列の幅は「見出しの外形」で決まってほしいため。
                    if column.width.is_some() {
                        button.set_size_request(label.width_request(), -1);
                        label.set_size_request(-1, -1);
                    }
                    button.set_child(Some(&label));
                    groups[index].remove_widget(&label);
                    groups[index].add_widget(&button);
                    button.connect_clicked({
                        let weak = Rc::downgrade(&self.0);
                        move |_| {
                            if let Some(inner) = weak.upgrade() {
                                Table(inner).on_header_activated(index);
                            }
                        }
                    });
                    self.0.header.append(&button);
                }
                false => self.0.header.append(&label),
            }
            header_labels.push(label);
        }
        *self.0.header_labels.borrow_mut() = header_labels;

        without_signal(&self.0.list, &self.0.handler, || {
            while let Some(row) = self.0.list.first_child() {
                self.0.list.remove(&row);
            }
            for row in rows.iter() {
                self.0.list.append(&build_row(row, &columns, &groups));
            }
            self.0.list.unselect_all();
        });
        *self.0.groups.borrow_mut() = groups;
        drop(rows);
        drop(columns);
        self.apply_sort();
    }

    /// 見出しが押されたとき。同じ列なら向きを反転し、違う列なら昇順から。
    fn on_header_activated(&self, index: usize) {
        let next = match self.0.sort.get() {
            Some((column, order)) if column == index => (index, order.reversed()),
            _ => (index, SortOrder::Ascending),
        };
        self.0.sort.set(Some(next));
        self.apply_sort();
        self.0.on_sort.emit(next.0, next.1);
    }

    /// 並べ替えの指定を見出しへ書く。
    fn apply_sort(&self) {
        let sort = self.0.sort.get();
        let columns = self.0.columns.borrow();
        for (index, label) in self.0.header_labels.borrow().iter().enumerate() {
            let order = sort.filter(|&(column, _)| column == index).map(|(_, o)| o);
            let title = columns.get(index).map(|c| c.title.as_str()).unwrap_or("");
            label.set_text(&format!("{title}{}", sort_arrow(order)));
        }
    }

    /// 指定された選択を、この表で意味を持つ形にそろえる。
    fn normalize(&self, indices: &[usize]) -> Vec<usize> {
        let rows = self.0.rows.borrow();
        self.selection_mode()
            .normalize_by(indices, |i| rows.get(i).is_some_and(|row| row.enabled))
    }

    /// 選択をネイティブへ写す。
    fn show(&self, picked: &[usize]) {
        self.0.list.unselect_all();
        for &index in picked {
            if let Some(row) = self.0.list.row_at_index(index as i32) {
                self.0.list.select_row(Some(&row));
            }
        }
    }
}

/// 1 行を組み立てる。セルは列と同じ順に並ぶ。
fn build_row(
    row: &TableRow,
    columns: &[TableColumn],
    groups: &[gtk::SizeGroup],
) -> gtk::ListBoxRow {
    let content = gtk::Box::new(gtk::Orientation::Horizontal, CELL_SPACING);
    content.set_margin_top(VERTICAL_MARGIN);
    content.set_margin_bottom(VERTICAL_MARGIN);
    content.set_margin_start(SIDE_MARGIN);
    content.set_margin_end(SIDE_MARGIN);

    for (index, column) in columns.iter().enumerate() {
        content.append(&cell_label(row.cell(index), column, &groups[index]));
    }

    let native = gtk::ListBoxRow::new();
    native.set_child(Some(&content));
    // 選べない行は、選択もクリックの反応もしない。
    native.set_selectable(row.enabled);
    native.set_activatable(row.enabled);
    native.set_sensitive(row.enabled);
    native
}
