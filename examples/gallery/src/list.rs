use std::cell::{Cell, RefCell};
use std::rc::Rc;

use naui::{
    Align, GridCell, Length, ListItem, ListRow, Orientation, PopupItem, Result, SelectionMode,
    Sizing, Track, TreeItem, Ui,
};

use crate::parts::{self, Notice};

/// List の補足表示、無効な行、単一・複数選択、行の起動と行ごとのコンテキストメニュー、
/// ListRow の任意内容の行と行クリック、Tree の開閉・選択。
pub(crate) fn build(ui: &Ui, notice: &Notice) -> Result<naui::Stack> {
    let pane = parts::pane(ui)?;

    // ListItem を set_items で並べる。行の起動はダブルクリックか Enter、
    // 行を右クリックすると、その行についての PopupMenu が開く。
    parts::section(ui, &pane, "List")?;

    let detailed = vec![
        ListItem::new("Inbox").detail("3 unread"),
        ListItem::new("Drafts").detail("1 draft"),
        ListItem::new("Sent").detail("Updated today"),
        ListItem::new("Archive").detail("128 items").enabled(false),
        ListItem::new("Trash"),
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
    // GTK はダブルクリックの 2 回目でも同じ選択を通知する。そのまま出すと
    // 直前の「起動」のトーストを上書きしてしまうので、変わったときだけ出す。
    // 行の作り直しや select などは通知せずに選択を変えるので、そのたびに
    // 覚えた選択を忘れる (同じ行を選び直したときにトーストが出なくなる)。
    let last_selection: Rc<RefCell<Option<Vec<usize>>>> = Rc::new(RefCell::new(None));
    list.on_select({
        let notice = notice.clone();
        let detailed = detailed.clone();
        let last_selection = last_selection.clone();
        move |indices| {
            let mut last = last_selection.borrow_mut();
            if last.as_deref() == Some(indices) {
                return;
            }
            *last = Some(indices.to_vec());
            drop(last);
            let labels: Vec<&str> = indices
                .iter()
                .filter_map(|&index| detailed.get(index).map(|item| item.label.as_str()))
                .collect();
            if labels.is_empty() {
                notice.show("Selected: none");
            } else {
                notice.show(&format!("Selected: {}", labels.join(" / ")));
            }
        }
    });

    // 一覧を操作するものは、一覧のすぐ上に 1 行でまとめる。
    let (mode_row, mode) = parts::choice(ui, "Selection", &["Single", "Multiple"])?;
    mode.on_select({
        let list = list.clone();
        let last_selection = last_selection.clone();
        move |index| {
            list.set_selection_mode(if index == 0 {
                SelectionMode::Single
            } else {
                SelectionMode::Multiple
            });
            *last_selection.borrow_mut() = None;
        }
    });

    list.on_row_activate({
        let notice = notice.clone();
        let detailed = detailed.clone();
        move |index| {
            if let Some(item) = detailed.get(index) {
                notice.show(&format!("Activated: {}", item.label));
            }
        }
    });

    // 行ごとの文脈メニュー。右クリックされた行を覚えておき、
    // 項目の文言もその行に合わせてから出す。
    let popup = ui.popup_menu()?;
    let clicked: Rc<Cell<Option<usize>>> = Rc::new(Cell::new(None));
    // list の通知が popup を持つので、popup の通知の側は list を弱参照で捕まえる
    // (PopupMenu には downgrade() が無い)。
    popup.on_select({
        let list = list.downgrade();
        let notice = notice.clone();
        let detailed = detailed.clone();
        let clicked = clicked.clone();
        let last_selection = last_selection.clone();
        move |path| {
            let (Some(row), Some(list)) = (clicked.get(), list.upgrade()) else {
                return;
            };
            match *path {
                [0] => {
                    list.select(row);
                    *last_selection.borrow_mut() = None;
                }
                [1] => {
                    if let Some(item) = detailed.get(row) {
                        notice.show(&format!("Open: {}", item.label));
                    }
                }
                [2] => {
                    list.clear_selection();
                    *last_selection.borrow_mut() = None;
                }
                _ => {}
            }
        }
    });
    list.on_context_menu({
        let list = list.downgrade();
        let detailed = detailed.clone();
        move |index, at| {
            let (Some(item), Some(list)) = (detailed.get(index), list.upgrade()) else {
                return;
            };
            clicked.set(Some(index));
            popup.set_items(&[
                PopupItem::new(format!("Select “{}”", item.label)),
                PopupItem::new(format!("Open “{}”", item.label)),
                PopupItem::new("Clear selection"),
                PopupItem::separator(),
                PopupItem::new("Disabled item").enabled(false),
            ]);
            popup.open_at(&list, at.x, at.y);
        }
    });

    let select_example = ui.button("Select 1 and 3")?;
    select_example.on_click({
        let list = list.clone();
        let last_selection = last_selection.clone();
        move || {
            list.select_many(&[0, 2]);
            *last_selection.borrow_mut() = None;
        }
    });
    let detail_toggle = ui.checkbox("Details")?;
    detail_toggle.set_checked(true);
    detail_toggle.on_toggle({
        let list = list.clone();
        let detailed = detailed.clone();
        move |showing| {
            list.set_items(if showing { &detailed } else { &plain });
            *last_selection.borrow_mut() = None;
        }
    });
    let actions = parts::row(ui)?;
    actions.append(&mode_row);
    actions.append(&select_example);
    actions.append(&detail_toggle);
    pane.append(&actions);
    pane.append(&list);

    build_composed_list(ui, &pane)?;
    build_dynamic_rows(ui, &pane)?;

    build_tree(ui, &pane, notice)?;
    Ok(pane)
}

/// 設定画面のような、先頭・本文・末尾を自由に組んだ行 (`ListRow`)。
///
/// 組み立てたウィジェットを set_rows で並べる。行を押すと on_activate で
/// チェックが切り替わる。
fn build_composed_list(ui: &Ui, pane: &naui::Stack) -> Result<()> {
    parts::section(ui, pane, "ListRow")?;

    let mut rows = Vec::new();
    for (checked, title, detail, action) in [
        (true, "Wi-Fi", "Show in menu bar", "Options…"),
        (false, "Bluetooth", "Manage nearby devices", "Details…"),
        (true, "Battery", "Show percentage", "Settings…"),
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
        // 行のラベルや余白を押したときだけ呼ばれる。チェックボックスや
        // ボタンを直接押したときは、それぞれのコールバックだけが動く。
        list_row.on_activate({
            let check = check.clone();
            move || check.set_checked(!check.is_checked())
        });
        rows.push(list_row);
    }

    let settings = ui.list()?;
    settings.set_rows(&rows);
    settings.set_sizing(Sizing::fill_width());
    pane.append(&settings);
    Ok(())
}

/// コールバックの中で `Ui` を clone し、行を後から組み立てる。
///
/// 押されたところで行の中身を組み立て、set_rows へ渡す。
fn build_dynamic_rows(ui: &Ui, pane: &naui::Stack) -> Result<()> {
    parts::section(ui, pane, "Rows built later")?;

    let list = ui.list()?;
    list.set_sizing(Sizing::fill_width());
    // 行は積み上げていくので、並びはアプリ側で持つ。
    let rows: Rc<RefCell<Vec<ListRow>>> = Rc::new(RefCell::new(Vec::new()));

    let actions = parts::row(ui)?;
    let add = ui.button("Add row")?;
    add.on_click({
        // コールバックへ持ち込むのは clone した Ui。中身は同じ。
        let ui = ui.clone();
        let list = list.clone();
        let rows = rows.clone();
        move || {
            let index = rows.borrow().len() + 1;
            // ウィジェットを作る API は Result を返すので、ここで受ける。
            let (Ok(content), Ok(check), Ok(label)) = (
                ui.grid(),
                ui.checkbox(""),
                ui.label(&format!("Row {index}")),
            ) else {
                return;
            };
            content.set_column_track(0, Track::Auto);
            content.set_column_track(1, Track::FILL);
            content.set_spacing(10.0, 0.0);
            content.attach(&check, GridCell::new(0, 0));
            label.set_sizing(Sizing::fill_width());
            content.attach(&label, GridCell::new(1, 0));
            content.set_sizing(Sizing::fill_width());

            // 後から作った行でも、通知の付け方はふつうの行と同じ。
            let row = ListRow::new(&content).selectable(false);
            row.on_activate(move || check.set_checked(!check.is_checked()));
            rows.borrow_mut().push(row);
            list.set_rows(&rows.borrow());
        }
    });
    actions.append(&add);

    let clear = ui.button("Clear")?;
    clear.on_click({
        let list = list.clone();
        let rows = rows.clone();
        move || {
            rows.borrow_mut().clear();
            list.set_rows(&rows.borrow());
        }
    });
    actions.append(&clear);

    pane.append(&actions);
    pane.append(&list);
    Ok(())
}

/// Tree の入れ子・開閉・選べない枝・通知。
fn build_tree(ui: &Ui, pane: &naui::Stack, notice: &Notice) -> Result<()> {
    parts::section(ui, pane, "Tree")?;

    let items = vec![
        TreeItem::new("src").expanded(true).children([
            TreeItem::new("main.rs").detail("Entry point"),
            TreeItem::new("lib.rs"),
            TreeItem::new("ui").children([
                TreeItem::new("list.rs"),
                TreeItem::new("tree.rs").detail("This page"),
            ]),
        ]),
        TreeItem::new("docs").child(TreeItem::new("guide.md").detail("12 KB")),
        TreeItem::new("target")
            .enabled(false)
            .detail("Build output")
            .child(TreeItem::new("debug")),
    ];

    let tree = ui.tree()?;
    tree.set_items(&items);
    tree.set_sizing(
        Sizing::new()
            .width(Length::Fill)
            .height(Length::Fixed(200.0)),
    );

    tree.on_select({
        let notice = notice.clone();
        let items = items.clone();
        move |path| match TreeItem::at(&items, path) {
            Some(item) => notice.show(&format!("Selected: {} {path:?}", item.label)),
            None => notice.show("Selected: none"),
        }
    });
    tree.on_expand({
        let notice = notice.clone();
        let items = items.clone();
        move |path, expanded| {
            let Some(item) = TreeItem::at(&items, path) else {
                return;
            };
            let state = if expanded { "Expanded" } else { "Collapsed" };
            notice.show(&format!("{state} {}", item.label));
        }
    });

    let actions = parts::row(ui)?;
    let expand_all = ui.button("Expand all")?;
    expand_all.on_click({
        let tree = tree.clone();
        move || tree.expand_all()
    });
    let collapse_all = ui.button("Collapse all")?;
    collapse_all.on_click({
        let tree = tree.clone();
        move || tree.collapse_all()
    });
    // 閉じた枝の中でも、祖先ごと開いてから選ばれる。
    let select_deep = ui.button("Select tree.rs")?;
    select_deep.on_click({
        let tree = tree.clone();
        move || tree.select(&[0, 2, 1])
    });

    actions.append(&expand_all);
    actions.append(&collapse_all);
    actions.append(&select_deep);
    pane.append(&actions);
    pane.append(&tree);
    Ok(())
}
