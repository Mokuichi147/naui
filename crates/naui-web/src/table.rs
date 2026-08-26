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

use naui_core::{Align, Result, SelectionMode, SortOrder, TableColumn, TableRow};
use wasm_bindgen::JsCast;
use web_sys::{Document, Element, HtmlElement, HtmlTableElement, KeyboardEvent, MouseEvent};

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
    rows: RefCell<Vec<TableRow>>,
    /// 行の `<tr>`。選択の書き込みはここを通す。
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
        style(&table, "border-collapse", "collapse");
        // 列の幅を `<col>` の指定どおりにする。指定の無い列は余りを分け合う。
        style(&table, "table-layout", "fixed");

        let colgroup: HtmlElement = create(doc, "colgroup")?.unchecked_into();
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
            rows: RefCell::new(Vec::new()),
            row_elements: RefCell::new(Vec::new()),
            mode: Cell::new(SelectionMode::Single),
            selected: RefCell::new(Vec::new()),
            active: Cell::new(None),
            anchor: Cell::new(None),
            handler: SelectionHandler::default(),
            row_listeners: RefCell::new(Vec::new()),
            _keys: RefCell::new(None),
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
        let rows = self.0.rows.borrow().clone();
        let _ = self.build_rows(&rows);
        let picked = self.selection();
        self.write_selection(&picked);
    }

    /// 列数。
    pub fn column_count(&self) -> usize {
        self.0.columns.borrow().len()
    }

    /// 行を作り直す。インデックスの意味が変わるため、選択は外れる。
    pub fn set_rows(&self, rows: &[TableRow]) {
        *self.0.rows.borrow_mut() = rows.to_vec();
        self.0.selected.borrow_mut().clear();
        self.0.active.set(None);
        self.0.anchor.set(None);
        let _ = self.build_rows(rows);
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
            // スクロールしても見出しが残るようにする。
            style(&cell, "position", "sticky");
            style(&cell, "top", "0");
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

    /// `<tbody>` を、いまの列と行から作り直す。
    fn build_rows(&self, rows: &[TableRow]) -> Result<()> {
        clear(&self.0.body);
        self.0.row_listeners.borrow_mut().clear();
        let columns = self.0.columns.borrow();
        let mut elements = Vec::with_capacity(rows.len());
        let mut listeners = Vec::with_capacity(rows.len());

        for (index, row) in rows.iter().enumerate() {
            let element: HtmlElement = create(&self.0.document, "tr")?.unchecked_into();
            let _ = element.set_attribute("id", &self.row_id(index));
            let _ = element.set_attribute("aria-selected", "false");

            for (column_index, column) in columns.iter().enumerate() {
                let cell: HtmlElement = create(&self.0.document, "td")?.unchecked_into();
                cell.set_text_content(Some(row.cell(column_index)));
                style(&cell, "text-align", text_align(column.align));
                style(&cell, "padding", "4px 8px");
                // 行の区切りだけを引く。縦線まで引くと表としては強すぎる。
                style(&cell, "border-top", "1px solid");
                style(&cell, "border-color", "ButtonBorder");
                // 列より長い文字は、列の幅で切る。
                style(&cell, "overflow", "hidden");
                style(&cell, "text-overflow", "ellipsis");
                style(&cell, "white-space", "nowrap");
                let _ = element.append_child(&cell);
            }

            if row.enabled {
                listeners.push(Listener::attach_event(element.as_ref(), "click", {
                    let weak = Rc::downgrade(&self.0);
                    move |event| {
                        let Some(inner) = weak.upgrade() else {
                            return;
                        };
                        let mouse = event.dyn_ref::<MouseEvent>();
                        let toggle = mouse.is_some_and(|e| e.meta_key() || e.ctrl_key());
                        let extend = mouse.is_some_and(|e| e.shift_key());
                        Table(inner).on_row_activated(index, toggle, extend);
                    }
                })?);
            } else {
                let _ = element.set_attribute("aria-disabled", "true");
                // 無効な文字にブラウザが使うシステム色。
                style(&element, "color", "GrayText");
            }

            let _ = self.0.body.append_child(&element);
            elements.push(element);
        }

        *self.0.row_elements.borrow_mut() = elements;
        *self.0.row_listeners.borrow_mut() = listeners;
        Ok(())
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
        let rows = self.0.rows.borrow();
        self.0
            .mode
            .get()
            .normalize_by(indices, |i| rows.get(i).is_some_and(|row| row.enabled))
    }

    /// 選択を覚えて、そのまま行へ書き込む (通知は起きない)。
    fn write_selection(&self, indices: &[usize]) {
        *self.0.selected.borrow_mut() = indices.to_vec();
        for (index, element) in self.0.row_elements.borrow().iter().enumerate() {
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
        let rows = self.0.rows.borrow();
        let mut at = start;
        while at >= 0 && (at as usize) < rows.len() {
            if rows[at as usize].enabled {
                return Some(at as usize);
            }
            at += step;
        }
        None
    }

    /// `a` から `b` までの、選べる行の並び。
    fn range(&self, a: usize, b: usize) -> Vec<usize> {
        let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
        let rows = self.0.rows.borrow();
        (lo..=hi.min(rows.len().saturating_sub(1)))
            .filter(|&i| rows[i].enabled)
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
        let elements = self.0.row_elements.borrow();
        let Some(row) = elements.get(active) else {
            return;
        };
        let top = row.offset_top();
        let bottom = top + row.offset_height();
        let view_top = self.0.root.scroll_top();
        let view_bottom = view_top + self.0.root.client_height();
        if top < view_top {
            self.0.root.set_scroll_top(top);
        } else if bottom > view_bottom {
            self.0
                .root
                .set_scroll_top(bottom - self.0.root.client_height());
        }
    }
}

/// 要素の中身をすべて取り除く。
fn clear(element: &HtmlElement) {
    while let Some(child) = element.last_element_child() {
        let _ = element.remove_child(&child);
    }
}
