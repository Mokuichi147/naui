use std::cell::Cell;
use std::rc::Rc;

use std::cell::RefCell;

use naui::{
    Align, GridCell, Length, ListItem, ListRow, NavItem, Orientation, Padding, PopupItem, Result,
    SelectionMode, Sizing, SortOrder, TableColumn, TableRow, Track, TreeItem, Ui,
};

/// List の補足表示、無効な行、単一・複数選択、コンテキストメニュー、
/// Table の列と選択、Tree の開閉・選択。
pub(crate) fn build(ui: &Ui) -> Result<naui::Stack> {
    let pane = ui.stack(Orientation::Vertical)?;
    pane.set_spacing(12.0);
    pane.set_padding(Padding::all(12.0));

    pane.append(&ui.label("List")?);
    pane.append(&ui.label("補足表示の有無、選べない行、単一選択と複数選択を確認できます。")?);

    let detailed = vec![
        ListItem::new("項目 A").detail("補足テキスト A"),
        ListItem::new("項目 B").detail("補足テキスト B"),
        ListItem::new("項目 C").detail("補足テキスト C"),
        ListItem::new("無効な項目")
            .detail("この行は選択できません")
            .enabled(false),
        ListItem::new("項目 D"),
    ];
    let plain: Vec<ListItem> = detailed
        .iter()
        .map(|item| ListItem::new(&item.label).enabled(item.enabled))
        .collect();

    let list = ui.list()?;
    list.set_items(&detailed);
    list.set_sizing(
        Sizing::new()
            .width(Length::Fill)
            .height(Length::Fixed(180.0)),
    );
    let status = ui.label("選択: なし")?;
    list.on_select({
        let status = status.clone();
        let detailed = detailed.clone();
        move |indices| {
            let labels: Vec<&str> = indices
                .iter()
                .filter_map(|&index| detailed.get(index).map(|item| item.label.as_str()))
                .collect();
            if labels.is_empty() {
                status.set_text("選択: なし");
            } else {
                status.set_text(&format!("選択: {}", labels.join(" / ")));
            }
        }
    });

    let mode = ui.navbar("選択方法")?;
    mode.set_items(&NavItem::list(["単一", "複数"]));
    mode.set_selected(0);
    mode.on_select({
        let list = list.clone();
        let status = status.clone();
        move |index| {
            list.set_selection_mode(if index == 0 {
                SelectionMode::Single
            } else {
                SelectionMode::Multiple
            });
            status.set_text("選択: なし");
        }
    });
    pane.append(&mode);
    pane.append(&list);
    pane.append(&ui.label("一覧を右クリックすると PopupMenu が開きます。")?);
    pane.append(&status);

    let popup = ui.popup_menu()?;
    popup.set_items(&[
        PopupItem::new("先頭を選択"),
        PopupItem::new("選択を解除"),
        PopupItem::separator(),
        PopupItem::new("無効なメニュー項目").enabled(false),
    ]);
    popup.on_select({
        let list = list.clone();
        let status = status.clone();
        move |index| match index {
            0 => list.select(0),
            1 => {
                list.clear_selection();
                status.set_text("選択: なし");
            }
            _ => {}
        }
    });
    popup.attach(&list);

    let actions = ui.stack(Orientation::Horizontal)?;
    actions.set_spacing(8.0);
    let select_example = ui.button("選択例")?;
    select_example.on_click({
        let list = list.clone();
        move || list.select_many(&[0, 2])
    });
    let detail_toggle = ui.button("補足を隠す")?;
    let showing_detail = Rc::new(Cell::new(true));
    detail_toggle.on_click({
        let list = list.clone();
        let detailed = detailed.clone();
        let plain = plain.clone();
        let detail_toggle = detail_toggle.clone();
        let showing_detail = showing_detail.clone();
        let status = status.clone();
        move || {
            let next = !showing_detail.get();
            showing_detail.set(next);
            list.set_items(if next { &detailed } else { &plain });
            detail_toggle.set_text(if next {
                "補足を隠す"
            } else {
                "補足を表示"
            });
            status.set_text("選択: なし");
        }
    });
    actions.append(&select_example);
    actions.append(&detail_toggle);
    pane.append(&actions);

    build_composed_list(ui, &pane)?;

    build_table(ui, &pane)?;
    build_tree(ui, &pane)?;
    Ok(pane)
}

/// macOS の設定画面のような、先頭・本文・末尾を自由に組んだ行。
fn build_composed_list(ui: &Ui, pane: &naui::Stack) -> Result<()> {
    pane.append(&ui.label("任意内容の ListRow")?);
    pane.append(&ui.label(
        "Grid / Stack / Checkbox / Button などを行内容にし、行全体を選ばない設定項目も作れます。",
    )?);

    let mut rows = Vec::new();
    for (checked, title, detail, action) in [
        (true, "Wi-Fi", "メニューバーに表示", "オプション…"),
        (false, "Bluetooth", "近くのデバイスを管理", "詳細…"),
        (true, "バッテリー", "残量を表示", "設定…"),
    ] {
        let row = ui.grid()?;
        row.set_column_track(0, Track::Auto);
        row.set_column_track(1, Track::FILL);
        row.set_column_track(2, Track::Auto);
        row.set_spacing(10.0, 0.0);

        let check = ui.checkbox("")?;
        check.set_checked(checked);
        row.attach(&check, GridCell::new(0, 0));

        let text = ui.stack(Orientation::Vertical)?;
        text.set_align(Align::Start);
        text.set_spacing(2.0);
        text.append(&ui.label(title)?);
        text.append(&ui.label(detail)?);
        text.set_sizing(Sizing::fill_width());
        row.attach(&text, GridCell::new(1, 0));

        row.attach(&ui.button(action)?, GridCell::new(2, 0));
        row.set_sizing(Sizing::fill_width());
        let list_row = ListRow::new(&row).selectable(false);
        list_row.on_activate({
            let check = check.clone();
            move || check.click()
        });
        rows.push(list_row);
    }

    let settings = ui.list()?;
    settings.set_rows(&rows);
    settings.set_sizing(Sizing::fill_width());
    pane.append(&settings);
    Ok(())
}

/// Table の列の幅と揃え、見出しからの並べ替え、選べない行、
/// 単一・複数選択、列の差し替え。
fn build_table(ui: &Ui, pane: &naui::Stack) -> Result<()> {
    pane.append(&ui.label("Table")?);
    pane.append(&ui.label(
        "列見出しと、幅を指定した列・右寄せの列を確認できます。見出しを押すと並べ替わります。",
    )?);

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

    let status = ui.label("選択: なし")?;
    // いま並んでいる行。見出しからの並べ替えでここが入れ替わる。
    let sorted = Rc::new(RefCell::new(rows.clone()));

    // 選択の通知は、いま並んでいる行 (`sorted`) から名前を引く。
    table.on_select({
        let status = status.clone();
        let sorted = sorted.clone();
        move |indices| {
            let rows = sorted.borrow();
            let names: Vec<&str> = indices
                .iter()
                .filter_map(|&index| rows.get(index).map(|row| row.cell(0)))
                .collect();
            if names.is_empty() {
                status.set_text("選択: なし");
            } else {
                status.set_text(&format!("選択: {}", names.join(" / ")));
            }
        }
    });

    // 並べ替えるのはアプリの仕事。naui は「どの列を、どちら向きに」だけを渡す。
    // 人口と面積は数字なので、桁区切りを外してから数として比べる。
    table.on_sort({
        let table = table.clone();
        let sorted = sorted.clone();
        let status = status.clone();
        move |column, order| {
            let mut rows = sorted.borrow_mut();
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
            table.set_rows(&rows);
            status.set_text(&format!(
                "{} 列目で並べ替え ({}) / 選択: なし",
                column + 1,
                if order == SortOrder::Ascending {
                    "昇順"
                } else {
                    "降順"
                }
            ));
        }
    });

    // 選択の通知は、いま並んでいる行 (`sorted`) から名前を引く。
    table.on_select({
        let status = status.clone();
        let sorted = sorted.clone();
        move |indices| {
            let rows = sorted.borrow();
            let names: Vec<&str> = indices
                .iter()
                .filter_map(|&index| rows.get(index).map(|row| row.cell(0)))
                .collect();
            if names.is_empty() {
                status.set_text("選択: なし");
            } else {
                status.set_text(&format!("選択: {}", names.join(" / ")));
            }
        }
    });

    let mode = ui.navbar("選択方法")?;
    mode.set_items(&NavItem::list(["単一", "複数"]));
    mode.set_selected(0);
    mode.on_select({
        let table = table.clone();
        let status = status.clone();
        move |index| {
            table.set_selection_mode(if index == 0 {
                SelectionMode::Single
            } else {
                SelectionMode::Multiple
            });
            status.set_text("選択: なし");
        }
    });
    pane.append(&mode);
    pane.append(&table);
    pane.append(&status);

    let actions = ui.stack(Orientation::Horizontal)?;
    actions.set_spacing(8.0);

    let select_example = ui.button("選択例")?;
    select_example.on_click({
        let table = table.clone();
        move || table.select_many(&[0, 2])
    });

    // 列を差し替えても、行の中身はそのまま残る。
    let column_toggle = ui.button("面積を隠す")?;
    let showing_area = Rc::new(Cell::new(true));
    column_toggle.on_click({
        let table = table.clone();
        let column_toggle = column_toggle.clone();
        let showing_area = showing_area.clone();
        move || {
            let next = !showing_area.get();
            showing_area.set(next);
            table.set_columns(if next { &wide } else { &narrow });
            column_toggle.set_text(if next {
                "面積を隠す"
            } else {
                "面積を表示"
            });
        }
    });

    actions.append(&select_example);
    actions.append(&column_toggle);
    pane.append(&actions);
    Ok(())
}

/// 桁区切りを外して数として読む。数でなければ `None` (文字として比べる)。
fn number(cell: &str) -> Option<u64> {
    let digits: String = cell.chars().filter(|c| *c != ',').collect();
    digits.parse().ok()
}

/// Tree の入れ子・開閉・選べない枝・通知。
fn build_tree(ui: &Ui, pane: &naui::Stack) -> Result<()> {
    pane.append(&ui.label("Tree")?);
    pane.append(&ui.label("入れ子の項目の開閉と、選べない枝を確認できます。")?);

    let items = vec![
        TreeItem::new("src").expanded(true).children([
            TreeItem::new("main.rs").detail("エントリーポイント"),
            TreeItem::new("lib.rs"),
            TreeItem::new("ui").children([
                TreeItem::new("list.rs"),
                TreeItem::new("tree.rs").detail("この画面"),
            ]),
        ]),
        TreeItem::new("docs").child(TreeItem::new("guide.md").detail("12 KB")),
        TreeItem::new("target")
            .enabled(false)
            .detail("この枝は中身ごと選べません")
            .child(TreeItem::new("debug")),
    ];

    let tree = ui.tree()?;
    tree.set_items(&items);
    tree.set_sizing(
        Sizing::new()
            .width(Length::Fill)
            .height(Length::Fixed(200.0)),
    );

    let status = ui.label("選択: なし")?;
    tree.on_select({
        let status = status.clone();
        let items = items.clone();
        move |path| {
            let label = TreeItem::at(&items, path).map(|item| item.label.clone());
            match label {
                Some(label) => status.set_text(&format!("選択: {label} {path:?}")),
                None => status.set_text("選択: なし"),
            }
        }
    });
    tree.on_expand({
        let status = status.clone();
        let items = items.clone();
        move |path, expanded| {
            let Some(item) = TreeItem::at(&items, path) else {
                return;
            };
            let state = if expanded { "開いた" } else { "閉じた" };
            status.set_text(&format!("{} を{state}", item.label));
        }
    });
    pane.append(&tree);
    pane.append(&status);

    let actions = ui.stack(Orientation::Horizontal)?;
    actions.set_spacing(8.0);

    let expand_all = ui.button("すべて開く")?;
    expand_all.on_click({
        let tree = tree.clone();
        move || tree.expand_all()
    });
    let collapse_all = ui.button("すべて閉じる")?;
    collapse_all.on_click({
        let tree = tree.clone();
        move || tree.collapse_all()
    });
    // 閉じた枝の中でも、祖先ごと開いてから選ばれる。
    let select_deep = ui.button("深い項目を選ぶ")?;
    select_deep.on_click({
        let tree = tree.clone();
        move || tree.select(&[0, 2, 1])
    });

    actions.append(&expand_all);
    actions.append(&collapse_all);
    actions.append(&select_deep);
    pane.append(&actions);
    Ok(())
}
