//! テーブル (DOM)。
//!
//! HTML の `<table>` をそのまま使う。列見出しは `<th scope="col">`、
//! 行は `<tr>`、セルは `<td>`。列の幅は `<colgroup>` の `<col>` に持たせ、
//! **幅の計算はブラウザのテーブルレイアウト**に任せる。
//!
//! 素の `<table>` には「行を選ぶ」仕組みが無いので、そこだけを naui が足す。
//!
//! | 部分 | 作り |
//! | --- | --- |
//! | 役割 | `role="grid"` + `aria-multiselectable` |
//! | 選択 | `<tr aria-selected>` と、システム色 (`SelectedItem` / `Highlight`) |
//! | 操作 | 行のクリック (⌘ / Ctrl / Shift)、矢印・Home / End・Space |
//!
//! 枠・選択・無効の色は、どれもブラウザが持つシステム色をそのまま使う。
//! naui は配色を決めない。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use naui_core::{
    keeps_row_window, row_window, Align, Result, RowWindow, SelectionMode, SortOrder, TableColumn,
    TableRow, ROW_WINDOW_OVERSCAN,
};
use wasm_bindgen::JsCast;
use web_sys::{Document, Element, HtmlElement, HtmlTableElement, KeyboardEvent, MouseEvent};

use crate::list::{control_was_targeted, ActivationHandler};
use crate::widgets::{create, impl_widget, Listener, Widget};

thread_local! {
    /// `aria-activedescendant` から行を指すための、表ごとの通し番号。
    static NEXT_ID: Cell<u32> = const { Cell::new(0) };
}

fn next_table_id() -> u32 {
    NEXT_ID.with(|n| {
        let id = n.get();
        n.set(id + 1);
        id
    })
}

fn style(element: &HtmlElement, property: &str, value: &str) {
    let _ = element.style().set_property(property, value);
}

/// CSS の文字揃えへ写す。`Fill` は文字に意味が無いので左と同じ扱い。
fn text_align(align: Align) -> &'static str {
    match align {
        Align::Center => "center",
        Align::End => "right",
        Align::Start | Align::Fill => "left",
    }
}

/// 選択が変わったことの通知先。
///
/// 単一選択でも複数選択でも同じ形にするため、選ばれている行を
/// 昇順の並びで渡す。呼び出しの間だけクロージャを取り出すので、
/// コールバックの中からテーブルを操作しても二重借用にならない。
#[derive(Clone, Default)]
struct SelectionHandler(Rc<RefCell<Option<Box<dyn FnMut(&[usize])>>>>);

impl SelectionHandler {
    fn set(&self, f: impl FnMut(&[usize]) + 'static) {
        *self.0.borrow_mut() = Some(Box::new(f));
    }

    fn emit(&self, indices: &[usize]) {
        let Some(mut f) = self.0.borrow_mut().take() else {
            return;
        };
        f(indices);
        let mut slot = self.0.borrow_mut();
        if slot.is_none() {
            *slot = Some(f);
        }
    }
}

/// 並べ替えが変わったことの通知先。
#[derive(Clone, Default)]
struct SortHandler(Rc<RefCell<Option<Box<dyn FnMut(usize, SortOrder)>>>>);

impl SortHandler {
    fn set(&self, f: impl FnMut(usize, SortOrder) + 'static) {
        *self.0.borrow_mut() = Some(Box::new(f));
    }

    fn emit(&self, column: usize, order: SortOrder) {
        let Some(mut f) = self.0.borrow_mut().take() else {
            return;
        };
        f(column, order);
        let mut slot = self.0.borrow_mut();
        if slot.is_none() {
            *slot = Some(f);
        }
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
/// # use naui_web::{Table, TableCells};
/// # fn fill(table: &Table, cities: Vec<String>) {
/// table.set_row_builder(cities.len(), move |index| {
///     Ok(TableCells::new().text(&cities[index]).text("13,960,000"))
/// });
/// # }
/// ```
pub struct TableCells {
    cells: Vec<CellContent>,
    selectable: bool,
    /// 文字だけの行で `enabled` が `false` のとき。文字を淡くする。
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
    /// ボタン・入力欄などのコントロールを直接押した場合は呼ばれない。
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
        *self.rows.borrow_mut() = rows.to_vec();
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

/// 並べ替えの向きを表す文字。HTML に「並べ替え済み」の見た目は無いので、
/// 読み上げには `aria-sort`、目には矢印を出す。
fn sort_arrow(order: Option<SortOrder>) -> &'static str {
    match order {
        Some(SortOrder::Ascending) => " ▲",
        Some(SortOrder::Descending) => " ▼",
        None => "",
    }
}

struct TableInner {
    /// 外から見える枠。スクロールするのはここ。
    root: HtmlElement,
    document: Document,
    id: u32,
    table: HtmlElement,
    /// 列の幅を持つ `<colgroup>`。列を変えるたびに中身を作り直す。
    colgroup: HtmlElement,
    head: HtmlElement,
    body: HtmlElement,
    /// 見出しの `<th>`。並べ替えの指標はここへ書く。
    header_cells: RefCell<Vec<HtmlElement>>,
    /// 見出しの中に置いた `<button>`。押せる列にだけある。
    sort_buttons: RefCell<Vec<Option<HtmlElement>>>,
    /// 見出しのクリックの購読。列を作り直すたびに入れ替える。
    sort_listeners: RefCell<Vec<Listener>>,
    /// いまの並べ替え (列と向き)。
    sort: Cell<Option<(usize, SortOrder)>>,
    sort_handler: SortHandler,
    columns: RefCell<Vec<TableColumn>>,
    rows: Rc<RowsState>,
    /// いま組み立ててある行の範囲。行数が多いと画面の前後だけになる。
    window: Cell<RowWindow>,
    /// 組み立ててある行の中身。`row_elements` と同じ並び。
    realized: RefCell<Vec<TableCells>>,
    /// 1 行の高さ (論理ピクセル)。測った値か [`Table::set_row_height`] の指定。
    row_height: Cell<f64>,
    /// アプリが決めた行の高さ。無ければ組み立てた行から測る。
    fixed_row_height: Cell<Option<f64>>,
    /// 窓の外にある行の分を埋める `<tr>`。スクロールバーの長さを保つ。
    top_spacer: HtmlElement,
    bottom_spacer: HtmlElement,
    /// 組み立ててある行の `<tr>`。並びは `window.start` から。
    row_elements: RefCell<Vec<HtmlElement>>,
    mode: Cell<SelectionMode>,
    /// 選ばれている行 (昇順)。DOM ではなくここが正。
    selected: RefCell<Vec<usize>>,
    /// キーボードでいま指している行。
    active: Cell<Option<usize>>,
    /// Shift での範囲選択の起点。
    anchor: Cell<Option<usize>>,
    handler: SelectionHandler,
    /// 行ごとのクリックの購読。行を作り直すたびに入れ替える。
    row_listeners: RefCell<Vec<Listener>>,
    /// 表全体のキー操作の購読。`<table>` は作り直さないので 1 回だけ張る。
    _keys: RefCell<Option<Listener>>,
    /// スクロールの購読。窓を動かすのに使う。
    _scroll: RefCell<Option<Listener>>,
}

/// 列見出しを持つ表 (`<table>`)。
///
/// 高さは中身から決まるので、行数に関係なく固定したいときは
/// `set_sizing` で指定する。はみ出した分は枠の中でスクロールする。
#[derive(Clone)]
pub struct Table(Rc<TableInner>);
impl_widget!(Table, root);

impl Table {
    pub(crate) fn new(doc: &Document) -> Result<Self> {
        let root: HtmlElement = create(doc, "div")?.unchecked_into();
        style(&root, "overflow", "auto");
        style(&root, "min-height", "0");
        // 枠と地の色は、ブラウザが入力欄に使うシステム色に任せる。
        style(&root, "border", "1px solid");
        style(&root, "border-color", "ButtonBorder");
        style(&root, "background-color", "Field");
        style(&root, "color", "FieldText");
        // 行の `offsetTop` がこの要素を基準になるようにする
        // (`reveal_active` がスクロール位置を求めるのに使う)。
        style(&root, "position", "relative");

        let table: HtmlElement = create(doc, "table")?.unchecked_into();
        let _ = table.set_attribute("role", "grid");
        // キーボードで入れるようにする。中の行は `aria-activedescendant` で指す。
        let _ = table.set_attribute("tabindex", "0");
        style(&table, "width", "100%");
        // **枠は重ねない (`collapse` にしない)。** 重ねた枠はセルではなく表が
        // 描くので、見出しを `position: sticky` で留めても枠が付いてこず、
        // ブラウザによっては見出しの地色ごと描かれない (行が透けて見える)。
        // 区切り線はセルの枠として引き、間隔は 0 にして重ねたときと同じ細さに
        // する。
        style(&table, "border-collapse", "separate");
        style(&table, "border-spacing", "0");
        // 列の幅を `<col>` の指定どおりにする。指定の無い列は余りを分け合う。
        style(&table, "table-layout", "fixed");

        let colgroup: HtmlElement = create(doc, "colgroup")?.unchecked_into();
        let top_spacer = spacer_row(doc)?;
        let bottom_spacer = spacer_row(doc)?;
        let head: HtmlElement = create(doc, "thead")?.unchecked_into();
        let body: HtmlElement = create(doc, "tbody")?.unchecked_into();
        let _ = table.append_child(&colgroup);
        let _ = table.append_child(&head);
        let _ = table.append_child(&body);
        let _ = root.append_child(&table);

        let this = Self(Rc::new(TableInner {
            root,
            document: doc.clone(),
            id: next_table_id(),
            table,
            colgroup,
            head,
            body,
            header_cells: RefCell::new(Vec::new()),
            sort_buttons: RefCell::new(Vec::new()),
            sort_listeners: RefCell::new(Vec::new()),
            sort: Cell::new(None),
            sort_handler: SortHandler::default(),
            columns: RefCell::new(Vec::new()),
            rows: Rc::new(RowsState::default()),
            window: Cell::new(RowWindow::default()),
            realized: RefCell::new(Vec::new()),
            row_height: Cell::new(0.0),
            fixed_row_height: Cell::new(None),
            top_spacer,
            bottom_spacer,
            row_elements: RefCell::new(Vec::new()),
            mode: Cell::new(SelectionMode::Single),
            selected: RefCell::new(Vec::new()),
            active: Cell::new(None),
            anchor: Cell::new(None),
            handler: SelectionHandler::default(),
            row_listeners: RefCell::new(Vec::new()),
            _keys: RefCell::new(None),
            _scroll: RefCell::new(None),
        }));
        this.apply_mode();

        let keys = Listener::attach_event(this.0.table.as_ref(), "keydown", {
            let weak = Rc::downgrade(&this.0);
            move |event| {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                if let Some(key) = event.dyn_ref::<KeyboardEvent>() {
                    if Table(inner).on_key(key) {
                        event.prevent_default();
                    }
                }
            }
        })?;
        *this.0._keys.borrow_mut() = Some(keys);

        // 行が多い表では、スクロールに合わせて組み立てる範囲を動かす。
        let scroll = Listener::attach(this.0.root.as_ref(), "scroll", {
            let weak = Rc::downgrade(&this.0);
            move || {
                if let Some(inner) = weak.upgrade() {
                    Table(inner).update_window();
                }
            }
        })?;
        *this.0._scroll.borrow_mut() = Some(scroll);
        Ok(this)
    }

    /// 列を作り直す。行と選択はそのまま残り、セルの並べ直しだけが起きる。
    ///
    /// 並べ替えの指定も、その列がまだ並べ替えられるなら残る。
    pub fn set_columns(&self, columns: &[TableColumn]) {
        *self.0.columns.borrow_mut() = columns.to_vec();
        let _ = self.build_columns();
        // 見出しを作り直したので、指標を貼り直す (押せなくなった列なら外れる)。
        let sort = self
            .0
            .sort
            .get()
            .filter(|&(column, _)| columns.get(column).is_some_and(|spec| spec.sortable));
        self.0.sort.set(sort);
        self.apply_sort();
        // セルの数と揃えが変わるので、行も組み直す。
        let _ = self.build_window();
        let picked = self.selection();
        self.write_selection(&picked);
    }

    /// 列数。
    pub fn column_count(&self) -> usize {
        self.0.columns.borrow().len()
    }

    /// 行を作り直す。インデックスの意味が変わるため、選択は外れる。
    ///
    /// 行数が多いときは、`<tr>` を作るのも画面に出ている分だけになる
    /// (残りは上下の詰め物が高さを持つので、スクロールバーは全行分のまま)。
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
    /// # use naui_web::{Table, TableCells};
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
        let _ = self.build_window();
        let picked = self.selection();
        self.write_selection(&picked);
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
            .set((height > 0.0).then_some(height));
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
        self.0.mode.set(mode);
        self.apply_mode();
        self.write_selection(&[]);
    }

    pub fn selection_mode(&self) -> SelectionMode {
        self.0.mode.get()
    }

    /// 選ばれている行のうち、いちばん上のもの。
    pub fn selected(&self) -> Option<usize> {
        self.0.selected.borrow().first().copied()
    }

    /// 選ばれている行 (昇順)。単一選択なら 0 件か 1 件。
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
        self.write_selection(&picked);
    }

    /// 通知せずに選択をすべて外す。
    pub fn clear_selection(&self) {
        self.write_selection(&[]);
    }

    /// ユーザーが選んだのと同じ経路で 1 行を選ぶ (通知あり)。
    pub fn select(&self, index: usize) {
        self.select_many(&[index]);
    }

    /// ユーザーが選んだのと同じ経路で選択を置き換える (通知あり)。
    pub fn select_many(&self, indices: &[usize]) {
        self.set_selection(indices);
        // ブラウザはプログラムからの変更でイベントを出さないため、
        // ここで 1 回だけ通知する。
        let actual = self.selection();
        self.0.handler.emit(&actual);
    }

    /// 選択が変わったときに、選ばれている行 (昇順) で呼ばれる。
    ///
    /// 複数選択では 0 件で呼ばれることもある。
    pub fn on_select(&self, f: impl FnMut(&[usize]) + 'static) {
        self.0.handler.set(f);
    }

    /// 見出しが押されて並べ替えの指定が変わったときに、
    /// 押された列と向きで呼ばれる。
    ///
    /// [`TableColumn::sortable`] を立てた列だけが押せる。**行を並べ替えるのは
    /// アプリの仕事**で、通知を受けたら並べ替えた行を [`Table::set_rows`] で
    /// 渡し直す (`set_rows` は選択を外すので、必要なら選び直す)。
    pub fn on_sort(&self, f: impl FnMut(usize, SortOrder) + 'static) {
        self.0.sort_handler.set(f);
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

    /// 中身の `<table>`。バックエンド固有の脱出口として公開している。
    ///
    /// 枠 (スクロールする `<div>`) は [`Widget::native_element`] から取れる。
    pub fn native_table(&self) -> HtmlTableElement {
        self.0.table.clone().unchecked_into()
    }

    // ------------------------------------------------------------ 組み立て

    /// `<colgroup>` と `<thead>` を、いまの列の定義から作り直す。
    fn build_columns(&self) -> Result<()> {
        clear(&self.0.colgroup);
        clear(&self.0.head);
        let columns = self.0.columns.borrow();

        for column in columns.iter() {
            let col: HtmlElement = create(&self.0.document, "col")?.unchecked_into();
            // 幅の指定が無い列は、余りをブラウザが分け合わせる。
            if let Some(width) = column.width {
                style(&col, "width", &format!("{width}px"));
            }
            let _ = self.0.colgroup.append_child(&col);
        }

        let row: HtmlElement = create(&self.0.document, "tr")?.unchecked_into();
        let mut cells = Vec::with_capacity(columns.len());
        let mut buttons = Vec::with_capacity(columns.len());
        let mut listeners = Vec::new();
        for (index, column) in columns.iter().enumerate() {
            let cell: HtmlElement = create(&self.0.document, "th")?.unchecked_into();
            let _ = cell.set_attribute("scope", "col");
            style(&cell, "text-align", text_align(column.align));
            style(&cell, "padding", "4px 8px");
            // スクロールしても見出しが残るようにする。行の中のボタンや入力欄は
            // 自分で重なりの層を作るので、見出しにも層を与えて上に置く
            // (`z-index` が無いと、それらが見出しの上へ描かれてしまう)。
            style(&cell, "position", "sticky");
            style(&cell, "top", "0");
            style(&cell, "z-index", "1");
            style(&cell, "background-color", "Field");
            style(&cell, "border-bottom", "1px solid");
            style(&cell, "border-color", "ButtonBorder");

            // 並べ替えられる列は、見出しそのものを `<button>` にする。
            // キーボードでも押せて、読み上げにも「押せる」と伝わる。
            let button = match column.sortable {
                false => {
                    cell.set_text_content(Some(&column.title));
                    None
                }
                true => {
                    let button: HtmlElement = create(&self.0.document, "button")?.unchecked_into();
                    button.set_text_content(Some(&column.title));
                    // 見出しの中では、ボタンらしい枠や地色は出さない。
                    style(&button, "all", "unset");
                    style(&button, "display", "block");
                    style(&button, "width", "100%");
                    style(&button, "cursor", "pointer");
                    style(&button, "text-align", text_align(column.align));
                    listeners.push(Listener::attach(button.as_ref(), "click", {
                        let weak = Rc::downgrade(&self.0);
                        move || {
                            if let Some(inner) = weak.upgrade() {
                                Table(inner).on_header_activated(index);
                            }
                        }
                    })?);
                    let _ = cell.append_child(&button);
                    Some(button)
                }
            };

            let _ = row.append_child(&cell);
            cells.push(cell);
            buttons.push(button);
        }
        *self.0.header_cells.borrow_mut() = cells;
        *self.0.sort_buttons.borrow_mut() = buttons;
        *self.0.sort_listeners.borrow_mut() = listeners;

        self.0
            .head
            .append_child(&row)
            .map(|_| ())
            .map_err(|e| crate::to_error("テーブルの見出しの組み立て", e))
    }

    /// `<tbody>` を、いまの窓の分だけ作り直す。
    ///
    /// 窓の外にある行は `<tr>` を作らず、その分の高さを上下の詰め物が持つ。
    /// 表の高さもスクロールバーの長さも全行分のまま変わらない。
    fn build_window(&self) -> Result<()> {
        clear(&self.0.body);
        self.0.row_listeners.borrow_mut().clear();
        self.0.realized.borrow_mut().clear();
        // 組み立ての中でアプリのコードが動くので、列の借用は持ち越さない。
        let columns = self.0.columns.borrow().clone();
        let window = self.0.window.get();
        let height = self.0.row_height.get();

        // 読み上げには、窓ではなく表そのものの行数を伝える。
        let _ = self
            .0
            .table
            .set_attribute("aria-rowcount", &(window.count + 1).to_string());
        set_spacer_height(&self.0.top_spacer, window.leading(height), columns.len());
        set_spacer_height(
            &self.0.bottom_spacer,
            window.trailing(height),
            columns.len(),
        );
        let _ = self.0.body.append_child(&self.0.top_spacer);

        let mut elements = Vec::with_capacity(window.len());
        let mut listeners = Vec::with_capacity(window.len());
        let mut realized = Vec::with_capacity(window.len());

        for index in window.indices() {
            let Some(cells) = self.0.rows.cells(index) else {
                continue;
            };
            let element: HtmlElement = create(&self.0.document, "tr")?.unchecked_into();
            let _ = element.set_attribute("id", &self.row_id(index));
            let _ = element.set_attribute("aria-selected", "false");
            // 見出しの行が 1 行目なので、データの 1 行目は 2 になる。
            let _ = element.set_attribute("aria-rowindex", &(index + 2).to_string());
            if let Some(fixed) = self.0.fixed_row_height.get() {
                style(&element, "height", &format!("{fixed}px"));
            }

            for (column_index, column) in columns.iter().enumerate() {
                let cell: HtmlElement = create(&self.0.document, "td")?.unchecked_into();
                style(&cell, "text-align", text_align(column.align));
                style(&cell, "padding", "4px 8px");
                // 行の区切りだけを引く。縦線まで引くと表としては強すぎる。
                // 下側へ引くのは、見出しの枠と重なって 2 本にならないため
                // (枠を重ねない `separate` にしているため)。
                style(&cell, "border-bottom", "1px solid");
                style(&cell, "border-color", "ButtonBorder");
                match cells.content(column_index) {
                    Some(CellContent::Widget(content)) => {
                        let _ = cell.append_child(&content.native_element());
                    }
                    // 列より短い行は、足りない分が空のセルになる。
                    Some(CellContent::Text(text)) => {
                        cell.set_text_content(Some(text));
                        clip_text(&cell);
                    }
                    None => {
                        cell.set_text_content(Some(""));
                        clip_text(&cell);
                    }
                }
                let _ = element.append_child(&cell);
            }

            if cells.dimmed {
                let _ = element.set_attribute("aria-disabled", "true");
                // 無効な文字にブラウザが使うシステム色。
                style(&element, "color", "GrayText");
            } else {
                let selectable = cells.is_selectable();
                if !selectable {
                    let _ = element.set_attribute("aria-disabled", "true");
                }
                listeners.push(Listener::attach_event(element.as_ref(), "click", {
                    let weak = Rc::downgrade(&self.0);
                    let element = element.clone();
                    move |event| {
                        let Some(inner) = weak.upgrade() else {
                            return;
                        };
                        let table = Table(inner);
                        // セルの中のボタンや入力欄そのものを押したときは、
                        // 行の activation も選択も起こさない。押した先の
                        // コントロールだけが動く (ほかの環境では、その
                        // コントロールがクリックを受け取るので同じになる)。
                        if control_was_targeted(&element, &event) {
                            return;
                        }
                        table.activate_row(index);
                        if !selectable {
                            return;
                        }
                        let mouse = event.dyn_ref::<MouseEvent>();
                        let toggle = mouse.is_some_and(|e| e.meta_key() || e.ctrl_key());
                        let extend = mouse.is_some_and(|e| e.shift_key());
                        table.on_row_activated(index, toggle, extend);
                    }
                })?);
            }

            let _ = self.0.body.append_child(&element);
            elements.push(element);
            realized.push(cells);
        }

        let _ = self.0.body.append_child(&self.0.bottom_spacer);
        *self.0.row_elements.borrow_mut() = elements;
        *self.0.row_listeners.borrow_mut() = listeners;
        *self.0.realized.borrow_mut() = realized;
        self.paint_selection();
        Ok(())
    }

    /// その行の activation を出す。
    fn activate_row(&self, index: usize) {
        let window = self.0.window.get();
        let activation = self
            .0
            .realized
            .borrow()
            .get(index.wrapping_sub(window.start))
            .map(|cells| cells.activation.clone());
        if let Some(activation) = activation {
            activation.emit();
        }
    }

    // ------------------------------------------------------- 行を絞る窓

    /// 行を作り直して、窓を先頭へ戻す。選択も外れる。
    fn reset_rows(&self) {
        self.0.selected.borrow_mut().clear();
        self.0.active.set(None);
        self.0.anchor.set(None);
        self.0.window.set(self.compute_window(ROW_WINDOW_OVERSCAN));
        let _ = self.build_window();
        // 1 行目の高さが分かったら、その高さで窓を引き直す。
        if self.measure_row_height() {
            self.update_window();
        }
    }

    /// いまのスクロール位置から、組み立てておく行の範囲を求める。
    fn compute_window(&self, overscan: usize) -> RowWindow {
        let head = self.0.head.offset_height() as f64;
        let viewport = self.0.root.client_height() as f64 - head;
        row_window(
            self.len(),
            self.0.root.scroll_top() as f64,
            viewport,
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
        let _ = self.build_window();
        self.measure_row_height();
    }

    /// 組み立てた行から 1 行の高さを測る。変わったら `true`。
    ///
    /// 行の高さはブラウザが決めるので、作ってからでないと分からない。
    /// これが分かって初めて、窓の外にある行の分を正しく詰められる。
    fn measure_row_height(&self) -> bool {
        if self.0.fixed_row_height.get().is_some() {
            return false;
        }
        let measured = self
            .0
            .row_elements
            .borrow()
            .first()
            .map(|row| row.offset_height() as f64)
            .unwrap_or(0.0);
        if measured <= 0.0 || (self.0.row_height.get() - measured).abs() < 0.5 {
            return false;
        }
        self.0.row_height.set(measured);
        // 詰め物の高さは行の高さから決まるので、引き直す。
        let window = self.0.window.get();
        let columns = self.column_count();
        set_spacer_height(&self.0.top_spacer, window.leading(measured), columns);
        set_spacer_height(&self.0.bottom_spacer, window.trailing(measured), columns);
        true
    }

    /// その行の、表の中での上端 (論理ピクセル)。窓の外の行でも求まる。
    fn row_top(&self, index: usize) -> f64 {
        index as f64 * self.0.row_height.get()
    }

    /// 見出しが押されたとき。同じ列なら向きを反転し、違う列なら昇順から。
    fn on_header_activated(&self, index: usize) {
        let next = match self.0.sort.get() {
            Some((column, order)) if column == index => (index, order.reversed()),
            _ => (index, SortOrder::Ascending),
        };
        self.0.sort.set(Some(next));
        self.apply_sort();
        self.0.sort_handler.emit(next.0, next.1);
    }

    /// 並べ替えの指定を見出しへ書く。
    fn apply_sort(&self) {
        let sort = self.0.sort.get();
        let columns = self.0.columns.borrow();
        let cells = self.0.header_cells.borrow();
        let buttons = self.0.sort_buttons.borrow();
        for (index, cell) in cells.iter().enumerate() {
            let order = sort.filter(|&(column, _)| column == index).map(|(_, o)| o);
            let value = match order {
                Some(SortOrder::Ascending) => "ascending",
                Some(SortOrder::Descending) => "descending",
                // 押せる列は「まだ並べ替えていない」、押せない列は指定なし。
                None if columns.get(index).is_some_and(|spec| spec.sortable) => "none",
                None => {
                    let _ = cell.remove_attribute("aria-sort");
                    continue;
                }
            };
            let _ = cell.set_attribute("aria-sort", value);
            if let Some(Some(button)) = buttons.get(index) {
                let title = columns.get(index).map(|c| c.title.as_str()).unwrap_or("");
                button.set_text_content(Some(&format!("{title}{}", sort_arrow(order))));
            }
        }
    }

    fn row_id(&self, index: usize) -> String {
        format!("naui-table-{}-row-{index}", self.0.id)
    }

    /// 単一 / 複数の指定を DOM へ反映する。
    fn apply_mode(&self) {
        let multiple = self.0.mode.get().is_multiple();
        let _ = self.0.table.set_attribute(
            "aria-multiselectable",
            if multiple { "true" } else { "false" },
        );
    }

    // --------------------------------------------------------------- 選択

    /// 指定された選択を、この表で意味を持つ形にそろえる。
    fn normalize(&self, indices: &[usize]) -> Vec<usize> {
        self.0
            .mode
            .get()
            .normalize_by(indices, |i| self.is_selectable(i))
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

    /// 選択を覚えて、そのまま行へ書き込む (通知は起きない)。
    fn write_selection(&self, indices: &[usize]) {
        *self.0.selected.borrow_mut() = indices.to_vec();
        self.paint_selection();
    }

    /// 覚えている選択を、組み立ててある行へ書き込む。
    ///
    /// 窓の外にある行は `<tr>` が無いので、書けるのは見えている分だけ。
    /// 正は [`TableInner::selected`] なので、窓が動いて組み立て直されたら
    /// そのときにまた書かれる。
    fn paint_selection(&self) {
        let indices = self.0.selected.borrow().clone();
        let start = self.0.window.get().start;
        for (offset, element) in self.0.row_elements.borrow().iter().enumerate() {
            let index = start + offset;
            let picked = indices.contains(&index);
            let _ = element.set_attribute("aria-selected", if picked { "true" } else { "false" });
            if picked {
                // 選択の色はブラウザのシステム色をそのまま使う。
                // 新しい名前に対応していれば、後の指定が勝つ。
                style(element, "background-color", "Highlight");
                style(element, "background-color", "SelectedItem");
                style(element, "color", "HighlightText");
                style(element, "color", "SelectedItemText");
            } else {
                style(element, "background-color", "");
                style(element, "color", "");
                if element.has_attribute("aria-disabled") {
                    style(element, "color", "GrayText");
                }
            }
        }
    }

    /// 行が押されたとき。
    fn on_row_activated(&self, index: usize, toggle: bool, extend: bool) {
        let multiple = self.0.mode.get().is_multiple();
        let picked = if multiple && extend {
            let anchor = self.0.anchor.get().unwrap_or(index);
            self.range(anchor, index)
        } else if multiple && toggle {
            let mut picked = self.selection();
            match picked.iter().position(|&i| i == index) {
                Some(at) => {
                    picked.remove(at);
                }
                None => picked.push(index),
            }
            self.0.anchor.set(Some(index));
            picked
        } else {
            self.0.anchor.set(Some(index));
            vec![index]
        };
        self.0.active.set(Some(index));
        self.commit(&picked);
    }

    /// キー操作。処理したら `true` を返す (ブラウザの既定動作を止める)。
    fn on_key(&self, event: &KeyboardEvent) -> bool {
        let len = self.len();
        if len == 0 {
            return false;
        }
        let current = self.0.active.get().or_else(|| self.selected());
        let target = match event.key().as_str() {
            "ArrowDown" => self.step(current, 1),
            "ArrowUp" => self.step(current, -1),
            "Home" => self.first_enabled(0, 1),
            "End" => self.first_enabled(len as isize - 1, -1),
            // 複数選択では Space で、いま指している行を入れたり外したりする。
            " " if self.0.mode.get().is_multiple() => {
                let Some(index) = current else {
                    return false;
                };
                self.on_row_activated(index, true, false);
                return true;
            }
            _ => return false,
        };
        let Some(target) = target else {
            return false;
        };
        self.0.active.set(Some(target));
        if self.0.mode.get().is_multiple() && event.shift_key() {
            let anchor = self.0.anchor.get().unwrap_or(target);
            let picked = self.range(anchor, target);
            self.commit(&picked);
        } else {
            self.0.anchor.set(Some(target));
            self.commit(&[target]);
        }
        true
    }

    /// `from` から `step` の向きへ、次に選べる行を探す。
    fn step(&self, from: Option<usize>, step: isize) -> Option<usize> {
        let start = match from {
            Some(index) => index as isize + step,
            None if step > 0 => 0,
            None => self.len() as isize - 1,
        };
        self.first_enabled(start, step)
    }

    /// `start` から `step` の向きに進んで、最初に選べる行を返す。
    fn first_enabled(&self, start: isize, step: isize) -> Option<usize> {
        let len = self.len();
        let mut at = start;
        while at >= 0 && (at as usize) < len {
            if self.is_selectable(at as usize) {
                return Some(at as usize);
            }
            at += step;
        }
        None
    }

    /// `a` から `b` までの、選べる行の並び。
    fn range(&self, a: usize, b: usize) -> Vec<usize> {
        let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
        (lo..=hi.min(self.len().saturating_sub(1)))
            .filter(|&i| self.is_selectable(i))
            .collect()
    }

    /// ユーザー操作の結果を確定し、通知する。
    fn commit(&self, indices: &[usize]) {
        let picked = self.normalize(indices);
        self.write_selection(&picked);
        self.reveal_active();
        self.0.handler.emit(&picked);
    }

    /// キーボードで指している行を、スクロール領域の中へ入れる。
    fn reveal_active(&self) {
        let Some(active) = self.0.active.get() else {
            return;
        };
        let _ = self
            .0
            .table
            .set_attribute("aria-activedescendant", &self.row_id(active));
        // 見出しはスクロールしても残る (`position: sticky`) ので、
        // その分だけ画面の上側は行に使えない。
        let head = self.0.head.offset_height() as f64;
        let height = self.0.row_height.get().max(1.0);
        let top = self.row_top(active);
        let bottom = top + height;
        let view_top = self.0.root.scroll_top() as f64;
        let view_bottom = view_top + self.0.root.client_height() as f64 - head;
        if top < view_top {
            self.0.root.set_scroll_top(top as i32);
        } else if bottom > view_bottom {
            self.0
                .root
                .set_scroll_top((bottom - (self.0.root.client_height() as f64 - head)) as i32);
        }
        // スクロールの通知を待たずに、指した行を組み立てておく。
        self.update_window();
    }
}

/// 窓の外にある行の分を埋める `<tr>` を作る。
///
/// 高さだけを持ち、読み上げからは外す。表の高さが全行分のままになるので、
/// スクロールバーの長さも位置も、全行を作ったときと変わらない。
fn spacer_row(document: &Document) -> Result<HtmlElement> {
    let row: HtmlElement = create(document, "tr")?.unchecked_into();
    let _ = row.set_attribute("aria-hidden", "true");
    let _ = row.set_attribute("role", "presentation");
    let cell: HtmlElement = create(document, "td")?.unchecked_into();
    style(&cell, "padding", "0");
    style(&cell, "border", "0");
    let _ = row.append_child(&cell);
    Ok(row)
}

/// 詰め物の高さを書く。0 なら行そのものを畳む。
fn set_spacer_height(row: &HtmlElement, height: f64, columns: usize) {
    let hidden = height <= 0.0;
    style(row, "display", if hidden { "none" } else { "" });
    style(row, "height", &format!("{height}px"));
    if let Some(cell) = row.first_element_child() {
        let _ = cell.set_attribute("colspan", &columns.max(1).to_string());
        if let Some(cell) = cell.dyn_ref::<HtmlElement>() {
            style(cell, "height", &format!("{height}px"));
        }
    }
}

/// 列より長い文字を、列の幅で切る。
fn clip_text(cell: &HtmlElement) {
    style(cell, "overflow", "hidden");
    style(cell, "text-overflow", "ellipsis");
    style(cell, "white-space", "nowrap");
}

/// 要素の中身をすべて取り除く。
fn clear(element: &HtmlElement) {
    while let Some(child) = element.last_element_child() {
        let _ = element.remove_child(&child);
    }
}
