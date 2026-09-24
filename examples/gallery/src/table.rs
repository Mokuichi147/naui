//! 表 (`Table`)。列の幅と揃え、並べ替え、セルのウィジェット、大量の行。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use naui::{
    Align, Length, Orientation, Result, SelectionMode, Sizing, SortOrder, TableCells, TableColumn,
    TableRow, Ui,
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
    parts::section(
        ui,
        pane,
        "Table",
        &["列見出しと、幅を指定した列・右寄せの列を確認できます。見出しを押すと並べ替わります。"],
    )?;

    // 幅を指定しない列 (都市) だけが、余った幅を受け取って広がる。
    let wide = vec![
        TableColumn::new("都市").sortable(true),
        TableColumn::new("人口")
            .width(120.0)
            .align(Align::End)
            .sortable(true),
        TableColumn::new("面積 km²")
            .width(100.0)
            .align(Align::End)
            .sortable(true),
    ];
    let narrow = vec![
        TableColumn::new("都市").sortable(true),
        TableColumn::new("人口")
            .width(120.0)
            .align(Align::End)
            .sortable(true),
    ];
    let rows = vec![
        TableRow::new(["東京", "13,960,000", "2,194"]),
        TableRow::new(["大阪", "8,838,000", "1,905"]),
        TableRow::new(["名古屋", "2,332,000", "326"]),
        TableRow::new(["集計中", "—", "—"]).enabled(false),
        TableRow::new(["札幌", "1,973,000", "1,121"]),
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
                notice.show("選択: なし");
            } else {
                notice.show(&format!("選択: {}", names.join(" / ")));
            }
        }
    });

    // 人口と面積は桁区切りを外し、数として比べる。
    table.on_sort({
        let table = table.clone();
        let sorted = sorted.clone();
        let notice = notice.clone();
        move |column, order| {
            let mut rows = sorted.borrow_mut();
            sort_rows(&mut rows, column, order);
            table.set_rows(&rows);
            notice.show(&describe_sort(column, order));
        }
    });

    let (mode_row, mode) = parts::choice(ui, "選択方法", &["単一", "複数"])?;
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
    pane.append(&mode_row);
    pane.append(&table);

    let actions = ui.stack(Orientation::Horizontal)?;
    actions.set_spacing(8.0);

    let select_example = ui.button("選択例")?;
    select_example.on_click({
        let table = table.clone();
        move || table.select_many(&[0, 2])
    });

    // 列を差し替えても、行の中身はそのまま残る。
    let column_toggle = ui.checkbox("面積の列を表示")?;
    column_toggle.set_checked(true);
    column_toggle.on_toggle({
        let table = table.clone();
        move |showing| table.set_columns(if showing { &wide } else { &narrow })
    });

    actions.append(&select_example);
    actions.append(&column_toggle);
    pane.append(&actions);
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
                row.cell(2) == "完了",
            )
        };
        let check = ui.checkbox("")?;
        check.set_checked(done);
        // 行ごとの状態はアプリのデータ側に持つ (行は作り直されるため)。
        check.on_toggle({
            let data = data.clone();
            move |checked| {
                if let Some(row) = data.borrow_mut().get_mut(index) {
                    let state = if checked { "完了" } else { "確認中" };
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

/// 「2 列目で並べ替え (昇順)」の形にする。
fn describe_sort(column: usize, order: SortOrder) -> String {
    let direction = match order {
        SortOrder::Ascending => "昇順",
        SortOrder::Descending => "降順",
    };
    format!("{} 列目で並べ替え ({direction})", column + 1)
}

/// 桁区切りを外して数として読む。数でなければ `None` (文字として比べる)。
fn number(cell: &str) -> Option<u64> {
    let digits: String = cell.chars().filter(|c| *c != ',').collect();
    digits.parse().ok()
}

/// セルにウィジェットを置く表 (`TableCells` + `set_row_builder`)。
fn build_table_cells(ui: &Ui, pane: &naui::Stack, notice: &Notice) -> Result<()> {
    parts::section(
        ui,
        pane,
        "TableCells",
        &[
            "set_row_builder に「行数」と「その行を組み立てる関数」を渡すと、セルにウィジェットを置けます。",
            "行のセルや余白を押すと on_activate が出ますが、中のボタンやチェックを押したときは出ません。",
        ],
    )?;

    let table = ui.table()?;
    table.set_columns(&[
        TableColumn::new("公開").width(60.0).align(Align::Center),
        TableColumn::new("名前"),
        TableColumn::new("操作").width(90.0).align(Align::End),
    ]);
    table.set_sizing(
        Sizing::new()
            .width(Length::Fill)
            .height(Length::Fixed(180.0)),
    );
    // セルにボタンを置くと、文字だけの行より高さが要る。
    table.set_row_height(44.0);

    const NAMES: [&str; 5] = ["設計メモ", "議事録", "見積り", "写真", "録音"];
    table.set_row_builder(NAMES.len(), {
        let ui = ui.clone();
        let notice = notice.clone();
        move |index| {
            let check = ui.checkbox("")?;
            let open = ui.button("開く")?;
            open.on_click({
                let notice = notice.clone();
                move || notice.show(&format!("{} を開きます", NAMES[index]))
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
fn build_large_table(ui: &Ui, pane: &naui::Stack, notice: &Notice) -> Result<()> {
    parts::section(
        ui,
        pane,
        "行が多い表",
        &[
            "10 万行を渡しても、組み立てるのは画面に出ている行だけです。",
            "スクロールバーの長さと位置は全行分のまま、選択もインデックスで覚えています。",
            "見出しを押すと並べ替わります。行を絞っていても、並べ替えの扱いは小さい表と同じです。",
            "行の作り方を「ウィジェット」にすると、10 万行のままセルにチェックボックスが入ります。",
        ],
    )?;

    const ROWS: usize = 100_000;
    let table = ui.table()?;
    table.set_columns(&[
        TableColumn::new("番号")
            .width(90.0)
            .align(Align::End)
            .sortable(true),
        TableColumn::new("名前").sortable(true),
        TableColumn::new("状態").width(90.0).sortable(true),
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
            Some(index) => notice.show(&format!("選択: {index} 行目")),
            None => notice.show("選択: なし"),
        }
    });

    // 行の作り方。文字だけの行 (`set_rows`) と、組み立てる行
    // (`set_row_builder` + `TableCells`) を切り替える。
    let widget_rows = Rc::new(Cell::new(false));

    // 10 万行でも、並べ替えは小さい表とまったく同じ形で書ける。
    // 番号は数として比べる (文字のままだと "10" が "2" より前へ来る)。
    table.on_sort({
        let ui = ui.clone();
        let table = table.clone();
        let rows = rows.clone();
        let widget_rows = widget_rows.clone();
        let notice = notice.clone();
        move |column, order| {
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
                "{} / {count} 行{}",
                describe_sort(column, order),
                stopwatch.took()
            ));
        }
    });

    let (mode_row, mode) = parts::choice(ui, "行の作り方", &["文字", "ウィジェット"])?;
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
                "{} の行にしました{}",
                if widget_rows.get() {
                    "ウィジェット"
                } else {
                    "文字"
                },
                stopwatch.took()
            ));
        }
    });
    pane.append(&mode_row);

    let actions = ui.stack(Orientation::Horizontal)?;
    actions.set_spacing(8.0);

    let fill = ui.button("10 万行を入れる")?;
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
                        format!("項目 {index}"),
                        if index % 3 == 0 {
                            "確認中"
                        } else {
                            "完了"
                        }
                        .to_owned(),
                    ])
                })
                .collect();
            show_rows(&ui, &table, &rows, widget_rows.get());
            notice.show(&format!("{ROWS} 行を入れました{}", stopwatch.took()));
        }
    });

    // 画面の外にある行も、インデックスで選べる。
    let select_far = ui.button("いちばん下の行を選ぶ")?;
    select_far.on_click({
        let table = table.clone();
        move || table.select_many(&[ROWS - 1])
    });

    let clear = ui.button("空にする")?;
    clear.on_click({
        let ui = ui.clone();
        let table = table.clone();
        let rows = rows.clone();
        let widget_rows = widget_rows.clone();
        move || {
            rows.borrow_mut().clear();
            show_rows(&ui, &table, &rows, widget_rows.get());
        }
    });

    actions.append(&fill);
    actions.append(&select_far);
    actions.append(&clear);
    pane.append(&table);
    pane.append(&actions);
    Ok(())
}
