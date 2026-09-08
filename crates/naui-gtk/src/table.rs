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
use naui_core::{
    keeps_hidden_selection, keeps_row_window, row_window, Align, Result, RowWindow, SelectionMode,
    SortOrder, TableColumn, TableRow, ROW_WINDOW_OVERSCAN,
};

use crate::bin::SizeBin;
use crate::callback::{SelectionNotifier, SortNotifier};
use crate::list::ActivationHandler;
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
fn cell_label(text: &str, column: &TableColumn, uniform: bool) -> gtk::Label {
    let label = gtk::Label::new(Some(text));
    label.set_xalign(xalign(column.align));
    // 列より長い文字は折り返さず、末尾を省略する。
    label.set_ellipsize(pango::EllipsizeMode::End);
    // 行を絞っている表では、セルの文字の長さで列幅を決めない。決めてしまうと、
    // スクロールで組み立て直すたびに列の幅が動いてしまう。列の幅は見出しと
    // 余りの分け合いだけで決まればよいので、自然な幅を 1 文字ぶんに抑える。
    if uniform && column.width.is_none() {
        label.set_max_width_chars(1);
    }
    apply_column_width(&label, column);
    label
}

/// 列の幅の決め方を、セルの中身へ反映する。
fn apply_column_width(widget: &impl IsA<gtk::Widget>, column: &TableColumn) {
    let widget = widget.as_ref();
    match column.width {
        // 幅の指定がある列は、その幅のまま。余りは受け取らない。
        Some(width) => {
            widget.set_size_request(width as i32, -1);
            widget.set_hexpand(false);
        }
        // 指定が無い列だけで、余った幅を分け合う。
        None => widget.set_hexpand(true),
    }
}

/// セル 1 つ分の中身。
enum CellContent {
    Text(String),
    Widget(Box<dyn Widget>),
}

impl Clone for CellContent {
    fn clone(&self) -> Self {
        match self {
            Self::Text(text) => Self::Text(text.clone()),
            Self::Widget(content) => Self::Widget(content.boxed_clone()),
        }
    }
}

/// 表へ載せる 1 行の中身。
///
/// [`TableRow`] は文字列だけで済む表向けの簡便 API であり、セルにボタンや
/// チェックボックス、アイコンを置きたいときは `Grid` / `Stack` で中身を
/// 作ってこの型へ並べる。行は [`Table::set_row_builder`] から返す。
///
/// ```no_run
/// # use naui_gtk::{Table, TableCells};
/// # fn fill(table: &Table, cities: Vec<String>) {
/// table.set_row_builder(cities.len(), move |index| {
///     Ok(TableCells::new().text(&cities[index]).text("13,960,000"))
/// });
/// # }
/// ```
pub struct TableCells {
    cells: Vec<CellContent>,
    selectable: bool,
    /// 文字だけの行で `enabled` が `false` のとき。行ごと操作できなくする。
    dimmed: bool,
    activation: ActivationHandler,
}

impl Clone for TableCells {
    fn clone(&self) -> Self {
        Self {
            cells: self.cells.clone(),
            selectable: self.selectable,
            dimmed: self.dimmed,
            activation: self.activation.clone(),
        }
    }
}

impl Default for TableCells {
    fn default() -> Self {
        Self::new()
    }
}

impl TableCells {
    /// セルが 1 つも無い行を作る。
    pub fn new() -> Self {
        Self {
            cells: Vec::new(),
            selectable: true,
            dimmed: false,
            activation: ActivationHandler::default(),
        }
    }

    /// 文字のセルを 1 つ足す。揃えは列の指定に従う。
    pub fn text(mut self, text: impl Into<String>) -> Self {
        self.cells.push(CellContent::Text(text.into()));
        self
    }

    /// ウィジェットのセルを 1 つ足す。
    pub fn cell(mut self, content: &dyn Widget) -> Self {
        self.cells.push(CellContent::Widget(content.boxed_clone()));
        self
    }

    /// 行全体を選択できるようにするかどうか (既定はできる)。
    pub fn selectable(mut self, selectable: bool) -> Self {
        self.selectable = selectable;
        self
    }

    pub fn is_selectable(&self) -> bool {
        self.selectable
    }

    /// 列数。
    pub fn len(&self) -> usize {
        self.cells.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    /// 行のセルや余白がクリックされたときに呼ぶ処理。
    ///
    /// ボタンや入力欄を直接押した場合は、そのコントロールがクリックを
    /// 受け取るので呼ばれない。
    pub fn on_activate(&self, f: impl FnMut() + 'static) {
        self.activation.set(f);
    }

    /// 文字だけの行から作る。`enabled` はそのまま「選べるか」になる。
    fn from_row(row: &TableRow) -> Self {
        Self {
            cells: row.cells.iter().cloned().map(CellContent::Text).collect(),
            selectable: row.enabled,
            dimmed: !row.enabled,
            activation: ActivationHandler::default(),
        }
    }

    fn content(&self, index: usize) -> Option<&CellContent> {
        self.cells.get(index)
    }
}

/// 行を組み立てるクロージャ。呼び出しの間だけ取り出す。
/// 行を組み立てるクロージャの置き場。
type RowBuildCell = Rc<RefCell<Option<Box<dyn FnMut(usize) -> Result<TableCells>>>>>;

/// 行を組み立てるクロージャ。
///
/// 呼び出しの間だけ取り出すので、組み立ての中から表を操作しても
/// 二重借用にならない。
#[derive(Clone, Default)]
struct RowBuilder(RowBuildCell);

impl RowBuilder {
    fn set(&self, f: impl FnMut(usize) -> Result<TableCells> + 'static) {
        *self.0.borrow_mut() = Some(Box::new(f));
    }

    fn clear(&self) {
        *self.0.borrow_mut() = None;
    }

    fn is_set(&self) -> bool {
        self.0.borrow().is_some()
    }

    fn build(&self, index: usize) -> Option<Result<TableCells>> {
        let mut f = self.0.borrow_mut().take()?;
        let cells = f(index);
        let mut slot = self.0.borrow_mut();
        if slot.is_none() {
            *slot = Some(f);
        }
        Some(cells)
    }
}

/// 行の出どころ。文字だけの行 ([`Table::set_rows`]) と、
/// 見えたときに組み立てる行 ([`Table::set_row_builder`]) の 2 通り。
#[derive(Default)]
struct RowsState {
    rows: RefCell<Vec<TableRow>>,
    builder: RowBuilder,
    count: Cell<usize>,
}

impl RowsState {
    fn len(&self) -> usize {
        self.count.get()
    }

    fn set_rows(&self, rows: &[TableRow]) {
        self.builder.clear();
        let mut stored = self.rows.borrow_mut();
        stored.clear();
        stored.extend_from_slice(rows);
        self.count.set(rows.len());
    }

    fn set_builder(&self, count: usize, f: impl FnMut(usize) -> Result<TableCells> + 'static) {
        self.rows.borrow_mut().clear();
        self.builder.set(f);
        self.count.set(count);
    }

    /// その行の中身。無ければ `None`。
    fn cells(&self, index: usize) -> Option<TableCells> {
        if index >= self.count.get() {
            return None;
        }
        if !self.builder.is_set() {
            return self.rows.borrow().get(index).map(TableCells::from_row);
        }
        // 組み立てに失敗した行は、列だけそろえた空の行にする。
        Some(self.builder.build(index)?.unwrap_or_default())
    }

    /// 文字だけの行で、その行が選べるか。組み立てる行では `None`。
    fn text_row_selectable(&self, index: usize) -> Option<bool> {
        match self.builder.is_set() {
            true => None,
            false => Some(self.rows.borrow().get(index).is_some_and(|row| row.enabled)),
        }
    }
}

struct TableInner {
    /// 外から見えるウィジェット。見出しと本体を縦に並べた入れ物。
    native: gtk::Box,
    /// 見出しの行。列を変えるたびに中身を作り直す。
    header: gtk::Box,
    list: gtk::ListBox,
    /// `GtkListBox` は自分でスクロールしないので、スクロール領域に載せる。
    scroller: gtk::ScrolledWindow,
    /// 詰め物と一覧を縦に並べた入れ物。スクロール領域の中身。
    _content: gtk::Box,
    /// 窓の外にある行の分を埋める。スクロールバーの長さを保つ。
    top_spacer: gtk::Box,
    bottom_spacer: gtk::Box,
    bin: SizeBin,
    columns: RefCell<Vec<TableColumn>>,
    rows: Rc<RowsState>,
    /// いま組み立ててある行の範囲。行数が多いと画面の前後だけになる。
    window: Cell<RowWindow>,
    /// 行を組み立てている最中か。入れ子の作り直しを防ぐ。
    rebuilding: Cell<bool>,
    /// 組み立ててある行の中身。`window.start` から順に並ぶ。
    realized: RefCell<Vec<TableCells>>,
    /// 選ばれている行 (昇順)。**窓の外の行も入る**ので、
    /// `GtkListBox` ではなくここが正。
    selected: RefCell<Vec<usize>>,
    /// 1 行の高さ (論理ピクセル)。測った値か [`Table::set_row_height`] の指定。
    row_height: Cell<f64>,
    /// アプリが決めた行の高さ。無ければ組み立てた行から測る。
    fixed_row_height: Cell<Option<i32>>,
    /// 列ごとの `GtkSizeGroup`。組み立てのたびに作り直す。
    groups: RefCell<Vec<gtk::SizeGroup>>,
    /// 見出しに置いたラベル。並べ替えの指標の書き替えに使う。
    /// 押せる列ではボタンの中身になっている。
    header_labels: RefCell<Vec<gtk::Label>>,
    /// 列の幅をそろえる `GtkSizeGroup` へ入れる見出し側の相手。
    /// 押せる列ではボタン、そうでなければラベル。
    header_slots: RefCell<Vec<gtk::Widget>>,
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
        // 行が多い表では、画面に出ている分だけを `GtkListBox` へ入れ、
        // 残りの行が占めるはずの高さを上下の詰め物が持つ。詰め物を一覧の
        // **外**に置くのは、`GtkListBox` の行として入れるとキーボード操作や
        // 行のインデックスに混ざってしまうため。
        let top_spacer = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let bottom_spacer = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
        content.append(&top_spacer);
        content.append(&list);
        content.append(&bottom_spacer);
        scroller.set_child(Some(&content));

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
            scroller,
            _content: content,
            top_spacer,
            bottom_spacer,
            bin,
            columns: RefCell::new(Vec::new()),
            rows: Rc::new(RowsState::default()),
            window: Cell::new(RowWindow::default()),
            rebuilding: Cell::new(false),
            realized: RefCell::new(Vec::new()),
            selected: RefCell::new(Vec::new()),
            row_height: Cell::new(0.0),
            fixed_row_height: Cell::new(None),
            groups: RefCell::new(Vec::new()),
            header_labels: RefCell::new(Vec::new()),
            header_slots: RefCell::new(Vec::new()),
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
                    let selection = table.read_native_selection();
                    *table.0.selected.borrow_mut() = selection.clone();
                    table.0.on_select.emit(&selection);
                }
            })
        };
        *inner.handler.borrow_mut() = Some(id);

        let table = Self(inner);
        // 行のクリック (とキーボードの Enter / Space) は `row-activated` に出る。
        // セルの中のボタンや入力欄はそれ自身がクリックを受け取るので、
        // ここへは来ない。
        {
            let weak = Rc::downgrade(&table.0);
            table.0.list.connect_row_activated(move |_, row| {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                let Ok(offset) = usize::try_from(row.index()) else {
                    return;
                };
                // 通知の中から `set_rows` を呼べるように、借りたまま呼ばない。
                let activation = inner
                    .realized
                    .borrow()
                    .get(offset)
                    .map(|cells| cells.activation.clone());
                if let Some(activation) = activation {
                    activation.emit();
                }
            });
        }
        // スクロールに合わせて、組み立てる範囲を動かす。
        {
            let weak = Rc::downgrade(&table.0);
            let update = move |_: &gtk::Adjustment| {
                if let Some(inner) = weak.upgrade() {
                    Table(inner).update_window();
                }
            };
            let adjustment = table.0.scroller.vadjustment();
            adjustment.connect_value_changed(update.clone());
            // 表そのものの大きさが変わったときも引き直す。
            adjustment.connect_page_size_notify(move |adjustment| update(adjustment));
        }
        table
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
    ///
    /// 行数が多いときは、`GtkListBoxRow` を作るのも画面に出ている分だけに
    /// なる (残りは上下の詰め物が高さを持つので、スクロールバーの長さは
    /// 全行分のまま)。
    pub fn set_rows(&self, rows: &[TableRow]) {
        self.0.rows.set_rows(rows);
        self.reset_rows();
    }

    /// セルにウィジェットを置ける行を、**見えたときに組み立てる**形で渡す。
    ///
    /// `count` は行数で、`build` は 0 から `count - 1` のインデックスを受けて
    /// その行の中身 ([`TableCells`]) を返す。呼ばれるのは画面に出ている行
    /// (と、その少し前後) だけなので、行数が数十万になっても開くのは速い。
    ///
    /// ```no_run
    /// # use naui_gtk::{Table, TableCells};
    /// # fn fill(table: &Table, cities: Vec<String>) {
    /// table.set_row_builder(cities.len(), move |index| {
    ///     Ok(TableCells::new().text(&cities[index]))
    /// });
    /// # }
    /// ```
    pub fn set_row_builder(
        &self,
        count: usize,
        build: impl FnMut(usize) -> Result<TableCells> + 'static,
    ) {
        self.0.rows.set_builder(count, build);
        self.reset_rows();
    }

    /// 見えている行を組み立て直す。行数と選択はそのまま。
    pub fn refresh(&self) {
        let picked = self.selection();
        self.build_window();
        without_signal(&self.0.list, &self.0.handler, || self.show(&picked));
        // 中身が変わって行の高さが動いていれば、窓を引き直す。
        if self.measure_row_height() {
            self.update_window();
        }
    }

    /// 行の高さを論理ピクセルで決める。0 以下を渡すと、組み立てた行から測る。
    ///
    /// どの行も同じ高さになる。画面の外にある行の分は「行数 × この高さ」で
    /// 詰めるので、行の高さがそろっていないと、スクロールバーの長さが
    /// 実際と少しずれる。
    pub fn set_row_height(&self, height: f64) {
        self.0
            .fixed_row_height
            .set((height > 0.0).then_some(height as i32));
        // 指定を外したときは、組み立て直した行から測り直す。
        self.0.row_height.set(height.max(0.0));
        self.refresh();
    }

    /// 行の高さ (論理ピクセル)。まだ 1 行も組み立てていなければ 0。
    pub fn row_height(&self) -> f64 {
        self.0.row_height.get()
    }

    /// 行数。
    pub fn len(&self) -> usize {
        self.0.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// 選び方を変える。選択の意味が変わるため、選択は外れる。
    pub fn set_selection_mode(&self, mode: SelectionMode) {
        self.0.selected.borrow_mut().clear();
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
    ///
    /// 窓の外にある行も入る。`GtkListBox` が知っているのは組み立ててある行
    /// だけなので、覚えているほうを返す。
    pub fn selection(&self) -> Vec<usize> {
        self.0.selected.borrow().clone()
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
        without_signal(&self.0.list, &self.0.handler, || self.show(&[]));
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
        while let Some(child) = self.0.header.first_child() {
            self.0.header.remove(&child);
        }
        let mut header_labels = Vec::with_capacity(columns.len());
        let mut header_slots: Vec<gtk::Widget> = Vec::with_capacity(columns.len());
        for (index, column) in columns.iter().enumerate() {
            let label = cell_label(&column.title, column, false);
            // 見出しは、行の文字より小さく淡くする (`List` の補助と同じ扱い)。
            crate::list::apply_caption(&label);
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
                    header_slots.push(button.clone().upcast());
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
                false => {
                    header_slots.push(label.clone().upcast());
                    self.0.header.append(&label);
                }
            }
            header_labels.push(label);
        }
        *self.0.header_labels.borrow_mut() = header_labels;
        *self.0.header_slots.borrow_mut() = header_slots;
        // 見出しを作り直したので、前の列の `GtkSizeGroup` は捨てる
        // (新しい見出しはまだどの組にも入っていない)。
        self.0.groups.borrow_mut().clear();
        drop(columns);
        self.build_window();
        self.apply_sort();
    }

    // ------------------------------------------------------- 行を絞る窓

    /// 行を作り直して、窓を先頭へ戻す。選択も外れる。
    fn reset_rows(&self) {
        self.0.selected.borrow_mut().clear();
        self.0.window.set(self.compute_window(ROW_WINDOW_OVERSCAN));
        self.build_window();
        // 1 行目の高さが分かったら、その高さで窓を引き直す。
        if self.measure_row_height() {
            self.update_window();
        }
    }

    /// いまの窓の分だけ、`GtkListBox` の行を作り直す。
    fn build_window(&self) {
        // 組み立ての途中でアプリのコードが動くので、そこから呼ばれても
        // 二重に作り直さない。
        if self.0.rebuilding.replace(true) {
            return;
        }
        self.build_window_once();
        self.0.rebuilding.set(false);
    }

    fn build_window_once(&self) {
        let window = self.0.window.get();
        let columns = self.0.columns.borrow().clone();
        let uniform = !window.is_complete();
        let fixed_height = self.0.fixed_row_height.get();
        // 列ごとの `GtkSizeGroup` は組み立てのたびに作り直す。消えた行の
        // セルを持ったままだと、その幅が残ってしまうため。見出しは
        // 作り直さないので、古い組から外してから新しい組へ入れる。
        let slots = self.0.header_slots.borrow().clone();
        let groups: Vec<gtk::SizeGroup> = columns
            .iter()
            .map(|_| gtk::SizeGroup::new(gtk::SizeGroupMode::Horizontal))
            .collect();
        for (index, slot) in slots.iter().enumerate() {
            if let Some(group) = self.0.groups.borrow().get(index) {
                group.remove_widget(slot);
            }
            if let Some(group) = groups.get(index) {
                group.add_widget(slot);
            }
        }
        *self.0.groups.borrow_mut() = groups.clone();

        let mut realized = Vec::with_capacity(window.len());
        without_signal(&self.0.list, &self.0.handler, || {
            while let Some(row) = self.0.list.first_child() {
                self.0.list.remove(&row);
            }
            for index in window.indices() {
                let Some(cells) = self.0.rows.cells(index) else {
                    continue;
                };
                let row = build_row(&cells, &columns, &groups, uniform);
                if let Some(height) = fixed_height {
                    row.set_size_request(-1, height);
                }
                self.0.list.append(&row);
                realized.push(cells);
            }
            self.0.list.unselect_all();
        });
        *self.0.realized.borrow_mut() = realized;

        let height = self.0.row_height.get();
        set_spacer_height(&self.0.top_spacer, window.leading(height));
        set_spacer_height(&self.0.bottom_spacer, window.trailing(height));

        // 覚えている選択を、組み立て直した行へ写す。
        let picked = self.selection();
        without_signal(&self.0.list, &self.0.handler, || self.show(&picked));
    }

    /// いまのスクロール位置から、組み立てておく行の範囲を求める。
    fn compute_window(&self, overscan: usize) -> RowWindow {
        let adjustment = self.0.scroller.vadjustment();
        // 詰め物が窓の外の行と同じ高さを持つので、スクロール位置は
        // 「全行を作ったとき」と同じ座標になる。
        row_window(
            self.len(),
            adjustment.value(),
            adjustment.page_size(),
            self.0.row_height.get(),
            overscan,
        )
    }

    /// スクロールに合わせて、組み立てる範囲を動かす。
    ///
    /// 1 行スクロールするたびに作り直すのは重いので、余分に作ってある分
    /// ([`ROW_WINDOW_OVERSCAN`]) の半分までは、そのまま使う。
    fn update_window(&self) {
        let current = self.0.window.get();
        let needed = self.compute_window(ROW_WINDOW_OVERSCAN / 2);
        let next = self.compute_window(ROW_WINDOW_OVERSCAN);
        if next == current || keeps_row_window(current, needed, next) {
            return;
        }
        self.0.window.set(next);
        self.build_window();
        self.measure_row_height();
    }

    /// 組み立てた行から 1 行の高さを測る。変わったら `true`。
    ///
    /// 行の高さは中身から決まるので、作ってからでないと分からない。
    /// これが分かって初めて、窓の外にある行の分を正しく詰められる。
    fn measure_row_height(&self) -> bool {
        if self.0.fixed_row_height.get().is_some() {
            return false;
        }
        let Some(row) = self.0.list.row_at_index(0) else {
            return false;
        };
        let measured = row.measure(gtk::Orientation::Vertical, -1).1 as f64;
        if measured <= 0.0 || (self.0.row_height.get() - measured).abs() < 0.5 {
            return false;
        }
        self.0.row_height.set(measured);
        let window = self.0.window.get();
        set_spacer_height(&self.0.top_spacer, window.leading(measured));
        set_spacer_height(&self.0.bottom_spacer, window.trailing(measured));
        true
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
        self.selection_mode()
            .normalize_by(indices, |index| self.is_selectable(index))
    }

    /// その行を選べるか。
    ///
    /// 組み立てる行では、まだ作っていない行は「選べる」とみなす
    /// (作ってみないと分からないため)。
    fn is_selectable(&self, index: usize) -> bool {
        if index >= self.len() {
            return false;
        }
        if let Some(enabled) = self.0.rows.text_row_selectable(index) {
            return enabled;
        }
        let window = self.0.window.get();
        match window.contains(index) {
            true => self
                .0
                .realized
                .borrow()
                .get(index - window.start)
                .is_none_or(TableCells::is_selectable),
            false => true,
        }
    }

    /// 選択を覚えて、組み立ててある行へ写す。
    ///
    /// 窓の外の行は `GtkListBoxRow` が無いので写せない。窓が動いて
    /// 組み立て直したときに、また覚えているほうから写す。
    fn show(&self, picked: &[usize]) {
        *self.0.selected.borrow_mut() = picked.to_vec();
        let window = self.0.window.get();
        self.0.list.unselect_all();
        for &index in picked {
            if !window.contains(index) {
                continue;
            }
            let offset = (index - window.start) as i32;
            if let Some(row) = self.0.list.row_at_index(offset) {
                self.0.list.select_row(Some(&row));
            }
        }
    }

    /// ユーザーが変えた選択を読む。窓の外の行の扱いもここで決める。
    fn read_native_selection(&self) -> Vec<usize> {
        let window = self.0.window.get();
        let mut picked: Vec<usize> = self
            .0
            .list
            .selected_rows()
            .iter()
            .map(|row| window.start + row.index().max(0) as usize)
            .collect();
        picked.sort_unstable();
        if window.is_complete() {
            return picked;
        }
        // 組み立ててある行の選択しか届かないので、窓の外の選択を残すかどうかを
        // 変わり方から決める (`keeps_hidden_selection`)。
        let previous = self.0.selected.borrow().clone();
        let inside: Vec<usize> = previous
            .iter()
            .copied()
            .filter(|&index| window.contains(index))
            .collect();
        if !keeps_hidden_selection(&inside, &picked) {
            return picked;
        }
        let outside = previous.iter().copied().filter(|&i| !window.contains(i));
        picked.extend(outside);
        picked.sort_unstable();
        picked.dedup();
        picked
    }
}

/// 1 行を組み立てる。セルは列と同じ順に並ぶ。
fn build_row(
    cells: &TableCells,
    columns: &[TableColumn],
    groups: &[gtk::SizeGroup],
    uniform: bool,
) -> gtk::ListBoxRow {
    let content = gtk::Box::new(gtk::Orientation::Horizontal, CELL_SPACING);
    content.set_margin_top(VERTICAL_MARGIN);
    content.set_margin_bottom(VERTICAL_MARGIN);
    content.set_margin_start(SIDE_MARGIN);
    content.set_margin_end(SIDE_MARGIN);

    for (index, column) in columns.iter().enumerate() {
        match cells.content(index) {
            Some(CellContent::Widget(widget)) => {
                let bin = widget.size_bin();
                bin.fill_parent();
                apply_column_width(&bin, column);
                groups[index].add_widget(&bin);
                content.append(&bin);
            }
            // 列より短い行は、足りない分が空のセルになる。
            Some(CellContent::Text(text)) => {
                let label = cell_label(text, column, uniform);
                // 見出しと同じ列を 1 つの `GtkSizeGroup` に入れると、幅がそろう。
                groups[index].add_widget(&label);
                content.append(&label);
            }
            None => {
                let label = cell_label("", column, uniform);
                groups[index].add_widget(&label);
                content.append(&label);
            }
        }
    }

    let native = gtk::ListBoxRow::new();
    native.set_child(Some(&content));
    let selectable = cells.is_selectable();
    native.set_selectable(selectable);
    // 選べない行でも、クリックを受けたい行はある (行内のボタンだけを使う行)。
    native.set_activatable(selectable || cells.activation.is_some());
    // 文字だけの行で `enabled` が `false` のときは、行ごと操作できなくする。
    native.set_sensitive(!cells.dimmed);
    native
}

/// 窓の外にある行の分を、詰め物の高さとして持たせる。
fn set_spacer_height(spacer: &gtk::Box, height: f64) {
    let height = height.max(0.0).min(i32::MAX as f64) as i32;
    spacer.set_size_request(-1, height);
    spacer.set_visible(height > 0);
}
