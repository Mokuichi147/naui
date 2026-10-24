//! 表 (`Table`)。列の幅と揃え、並べ替え、セルのウィジェット、大量の行。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use naui::{
    Align, Length, Result, SelectionMode, Sizing, SortOrder, TableCells, TableColumn, TableRow, Ui,
};

use crate::parts::{self, Notice};

/// 列の幅と揃え、見出しからの並べ替え、単一・複数選択、セルのウィジェット、
/// 10 万行の表。
pub(crate) fn build(ui: &Ui, notice: &Notice) -> Result<naui::Stack> {
    let pane = parts::pane(ui)?;
    build_table(ui, &pane, notice)?;
    build_table_cells(ui, &pane, notice)?;
    build_large_table(ui, &pane, notice)?;
    Ok(pane)
}

/// Table の列の幅と揃え、見出しからの並べ替え、選べない行、
/// 単一・複数選択、列の差し替え。
fn build_table(ui: &Ui, pane: &naui::Stack, notice: &Notice) -> Result<()> {
    parts::section(ui, pane, "Table")?;

    // 幅を指定しない列 (都市) だけが、余った幅を受け取って広がる。
    let wide = vec![
        TableColumn::new("City").sortable(true),
        TableColumn::new("Population")
            .width(120.0)
            .align(Align::End)
            .sortable(true),
        TableColumn::new("Area km²")
            .width(100.0)
            .align(Align::End)
            .sortable(true),
    ];
    let narrow = vec![
        TableColumn::new("City").sortable(true),
        TableColumn::new("Population")
            .width(120.0)
            .align(Align::End)
            .sortable(true),
    ];
    let rows = vec![
        TableRow::new(["Tokyo", "13,960,000", "2,194"]),
        TableRow::new(["Osaka", "8,838,000", "1,905"]),
        TableRow::new(["Nagoya", "2,332,000", "326"]),
        TableRow::new(["Counting…", "—", "—"]).enabled(false),
        TableRow::new(["Sapporo", "1,973,000", "1,121"]),
    ];

    let table = ui.table()?;
    table.set_columns(&wide);
    table.set_rows(&rows);
    table.set_sizing(
        Sizing::new()
            .width(Length::Fill)
            .height(Length::Fixed(200.0)),
    );

    // いま並んでいる行。見出しからの並べ替えでここが入れ替わる。
    let sorted = Rc::new(RefCell::new(rows.clone()));

    // 選択の通知は、いま並んでいる行 (`sorted`) から名前を引く。
    table.on_select({
        let notice = notice.clone();
        let sorted = sorted.clone();
        move |indices| {
            let rows = sorted.borrow();
            let names: Vec<&str> = indices
                .iter()
                .filter_map(|&index| rows.get(index).map(|row| row.cell(0)))
                .collect();
            if names.is_empty() {
                notice.show("Selected: none");
            } else {
                notice.show(&format!("Selected: {}", names.join(" / ")));
            }
        }
    });

    // 人口と面積は桁区切りを外し、数として比べる。
    // table 自身は弱参照で捕まえる (clone() で捕まえると解放されない)。
    table.on_sort({
        let table = table.downgrade();
        let sorted = sorted.clone();
        let notice = notice.clone();
        move |column, order| {
            let Some(table) = table.upgrade() else { return };
            let mut rows = sorted.borrow_mut();
            sort_rows(&mut rows, column, order);
            table.set_rows(&rows);
            notice.show(&describe_sort(column, order));
        }
    });

    // 表を操作するものは、表のすぐ上に 1 行でまとめる。
    let (mode_row, mode) = parts::choice(ui, "Selection", &["Single", "Multiple"])?;
    mode.on_select({
        let table = table.clone();
        move |index| {
            table.set_selection_mode(if index == 0 {
                SelectionMode::Single
            } else {
                SelectionMode::Multiple
            });
        }
    });
    let select_example = ui.button("Select 1 and 3")?;
    select_example.on_click({
        let table = table.clone();
        move || table.select_many(&[0, 2])
    });

    // 列を差し替えても、行の中身はそのまま残る。
    let column_toggle = ui.checkbox("Area column")?;
    column_toggle.set_checked(true);
    column_toggle.on_toggle({
        let table = table.clone();
        move |showing| table.set_columns(if showing { &wide } else { &narrow })
    });

    let actions = parts::row(ui)?;
    actions.append(&mode_row);
    actions.append(&select_example);
    actions.append(&column_toggle);
    pane.append(&actions);
    pane.append(&table);
    Ok(())
}

/// かかった時間を測る。
///
/// **Web では測らない。** `std::time::Instant` は wasm32-unknown-unknown に
/// 実装が無く、呼ぶとその場で panic する。
struct Stopwatch {
    #[cfg(not(target_arch = "wasm32"))]
    started: std::time::Instant,
}

impl Stopwatch {
    fn start() -> Self {
        Self {
            #[cfg(not(target_arch = "wasm32"))]
            started: std::time::Instant::now(),
        }
    }

    /// 「 (12ms)」の形にする。Web では空文字列。
    #[cfg(not(target_arch = "wasm32"))]
    fn took(&self) -> String {
        format!(" ({:?})", self.started.elapsed())
    }

    #[cfg(target_arch = "wasm32")]
    fn took(&self) -> String {
        String::new()
    }
}

/// いま持っている行を、選ばれている作り方で表へ渡す。
///
/// 文字だけの行は `set_rows`、ウィジェットの行は `set_row_builder` で渡す。
/// どちらも組み立てるのは画面に出ている分だけなので、行数が多くても同じ。
fn show_rows(ui: &Ui, table: &naui::Table, rows: &Rc<RefCell<Vec<TableRow>>>, widget_rows: bool) {
    if !widget_rows {
        table.set_rows(&rows.borrow());
        return;
    }
    let count = rows.borrow().len();
    let ui = ui.clone();
    let data = rows.clone();
    table.set_row_builder(count, move |index| {
        // ここはアプリのコードなので、データの借用はこの中で完結させる。
        let (number, name, done) = {
            let rows = data.borrow();
            let row = &rows[index];
            (
                row.cell(0).to_owned(),
                row.cell(1).to_owned(),
                row.cell(2) == "Done",
            )
        };
        let check = ui.checkbox("")?;
        check.set_checked(done);
        // 行ごとの状態はアプリのデータ側に持つ (行は作り直されるため)。
        check.on_toggle({
            let data = data.clone();
            move |checked| {
                if let Some(row) = data.borrow_mut().get_mut(index) {
                    let state = if checked { "Done" } else { "Review" };
                    row.cells[2] = state.to_owned();
                }
            }
        });
        // 行そのものも選べるままにする (チェックを押したときは選択は動かない)。
        Ok(TableCells::new().text(number).text(name).cell(&check))
    });
}

/// 並べ替える。並べ替えるのはアプリの仕事で、naui は「どの列を、どちら向きに」
/// だけを渡す。
///
/// 数として読める列は数で比べる (文字のままだと "10" が "2" より前へ来る)。
fn sort_rows(rows: &mut [TableRow], column: usize, order: SortOrder) {
    rows.sort_by(|a, b| {
        let ordering = match number(a.cell(column)).zip(number(b.cell(column))) {
            Some((a, b)) => a.cmp(&b),
            None => a.cell(column).cmp(b.cell(column)),
        };
        match order {
            SortOrder::Ascending => ordering,
            SortOrder::Descending => ordering.reverse(),
        }
    });
}

/// 「Sorted by column 2 (ascending)」の形にする。
fn describe_sort(column: usize, order: SortOrder) -> String {
    let direction = match order {
        SortOrder::Ascending => "ascending",
        SortOrder::Descending => "descending",
    };
    format!("Sorted by column {} ({direction})", column + 1)
}

/// 桁区切りを外して数として読む。数でなければ `None` (文字として比べる)。
fn number(cell: &str) -> Option<u64> {
    let digits: String = cell.chars().filter(|c| *c != ',').collect();
    digits.parse().ok()
}

/// セルにウィジェットを置く表 (`TableCells` + `set_row_builder`)。
///
/// set_row_builder に「行数」と「その行を組み立てる関数」を渡すと、セルに
/// ウィジェットを置ける。行のセルや余白を押すと on_activate が出るが、
/// 中のボタンやチェックを押したときは出ない。
fn build_table_cells(ui: &Ui, pane: &naui::Stack, notice: &Notice) -> Result<()> {
    parts::section(ui, pane, "TableCells")?;

    let table = ui.table()?;
    table.set_columns(&[
        TableColumn::new("Public").width(60.0).align(Align::Center),
        TableColumn::new("Name"),
        TableColumn::new("Action").width(90.0).align(Align::End),
    ]);
    table.set_sizing(
        Sizing::new()
            .width(Length::Fill)
            .height(Length::Fixed(180.0)),
    );
    // セルにボタンを置くと、文字だけの行より高さが要る。
    table.set_row_height(44.0);

    const NAMES: [&str; 5] = ["Design notes", "Minutes", "Estimate", "Photos", "Recording"];
    table.set_row_builder(NAMES.len(), {
        let ui = ui.clone();
        let notice = notice.clone();
        move |index| {
            let check = ui.checkbox("")?;
            let open = ui.button("Open")?;
            open.on_click({
                let notice = notice.clone();
                move || notice.show(&format!("Opening {}", NAMES[index]))
            });
            let cells = TableCells::new()
                .cell(&check)
                .text(NAMES[index])
                .cell(&open)
                // 行の中のコントロールだけを使う行にする。
                .selectable(false);
            // 行そのものを押したら、チェックを反転する。
            cells.on_activate(move || check.set_checked(!check.is_checked()));
            Ok(cells)
        }
    });

    pane.append(&table);
    Ok(())
}

/// 行が多い表。画面に出ている行だけが組み立てられる。
///
/// 10 万行を渡しても、組み立てるのは画面に出ている行だけ。スクロールバーの
/// 長さと位置は全行分のまま、選択もインデックスで覚えている。行を絞って
/// いても、並べ替えの扱いは小さい表と同じ。行の作り方を「Widgets」にすると、
/// 10 万行のままセルにチェックボックスが入る。
fn build_large_table(ui: &Ui, pane: &naui::Stack, notice: &Notice) -> Result<()> {
    parts::section(ui, pane, "100,000 rows")?;

    const ROWS: usize = 100_000;
    let table = ui.table()?;
    table.set_columns(&[
        TableColumn::new("No.")
            .width(90.0)
            .align(Align::End)
            .sortable(true),
        TableColumn::new("Name").sortable(true),
        TableColumn::new("Status").width(90.0).sortable(true),
    ]);
    table.set_sizing(
        Sizing::new()
            .width(Length::Fill)
            .height(Length::Fixed(200.0)),
    );

    // いま並んでいる行はアプリが持つ。並べ替えるのもアプリの仕事。
    let rows: Rc<RefCell<Vec<TableRow>>> = Rc::new(RefCell::new(Vec::new()));

    table.on_select({
        let notice = notice.clone();
        move |indices| match indices.first() {
            Some(index) => notice.show(&format!("Selected: row {index}")),
            None => notice.show("Selected: none"),
        }
    });

    // 行の作り方。文字だけの行 (`set_rows`) と、組み立てる行
    // (`set_row_builder` + `TableCells`) を切り替える。
    let widget_rows = Rc::new(Cell::new(false));

    // 10 万行でも、並べ替えは小さい表とまったく同じ形で書ける。
    // 番号は数として比べる (文字のままだと "10" が "2" より前へ来る)。
    table.on_sort({
        let ui = ui.clone();
        let table = table.downgrade();
        let rows = rows.clone();
        let widget_rows = widget_rows.clone();
        let notice = notice.clone();
        move |column, order| {
            let Some(table) = table.upgrade() else { return };
            let stopwatch = Stopwatch::start();
            // **借用はここで返す。** 組み立てる行では、このあとの出し直しで
            // 同じデータを読むので、borrow_mut を持ったままだと落ちる。
            let count = {
                let mut rows = rows.borrow_mut();
                sort_rows(&mut rows, column, order);
                rows.len()
            };
            show_rows(&ui, &table, &rows, widget_rows.get());
            notice.show(&format!(
                "{} / {count} rows{}",
                describe_sort(column, order),
                stopwatch.took()
            ));
        }
    });

    let (mode_row, mode) = parts::choice(ui, "Rows", &["Text", "Widgets"])?;
    mode.on_select({
        let ui = ui.clone();
        let table = table.clone();
        let rows = rows.clone();
        let widget_rows = widget_rows.clone();
        let notice = notice.clone();
        move |index| {
            widget_rows.set(index == 1);
            let stopwatch = Stopwatch::start();
            show_rows(&ui, &table, &rows, widget_rows.get());
            notice.show(&format!(
                "Switched to {} rows{}",
                if widget_rows.get() { "widget" } else { "text" },
                stopwatch.took()
            ));
        }
    });
    let fill = ui.button("Fill 100,000 rows")?;
    fill.on_click({
        let ui = ui.clone();
        let table = table.clone();
        let notice = notice.clone();
        let rows = rows.clone();
        let widget_rows = widget_rows.clone();
        move || {
            let stopwatch = Stopwatch::start();
            *rows.borrow_mut() = (0..ROWS)
                .map(|index| {
                    TableRow::new([
                        index.to_string(),
                        format!("Item {index}"),
                        if index % 3 == 0 { "Review" } else { "Done" }.to_owned(),
                    ])
                })
                .collect();
            show_rows(&ui, &table, &rows, widget_rows.get());
            notice.show(&format!("Filled {ROWS} rows{}", stopwatch.took()));
        }
    });

    // 画面の外にある行も、インデックスで選べる。
    let select_far = ui.button("Select last row")?;
    select_far.on_click({
        let table = table.clone();
        move || table.select_many(&[ROWS - 1])
    });

    let clear = ui.button("Clear")?;
    clear.on_click({
        let ui = ui.clone();
        let table = table.clone();
        let rows = rows.clone();
        let widget_rows = widget_rows.clone();
        let notice = notice.clone();
        move || {
            rows.borrow_mut().clear();
            show_rows(&ui, &table, &rows, widget_rows.get());
            // 行を差し替えても選択の通知は出ないので、ここで知らせる。
            notice.show("Table cleared");
        }
    });

    let actions = parts::row(ui)?;
    actions.append(&mode_row);
    actions.append(&fill);
    actions.append(&select_far);
    actions.append(&clear);
    pane.append(&actions);
    pane.append(&table);
    Ok(())
}
