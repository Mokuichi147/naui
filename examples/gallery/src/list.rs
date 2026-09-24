use std::cell::RefCell;
use std::rc::Rc;

use naui::{
    Align, GridCell, Length, ListItem, ListRow, Orientation, PopupItem, Result, SelectionMode,
    Sizing, Track, TreeItem, Ui,
};

use crate::parts::{self, Notice};

/// List の補足表示、無効な行、単一・複数選択、コンテキストメニュー、
/// ListRow の任意内容の行と行クリック、Tree の開閉・選択。
pub(crate) fn build(ui: &Ui, notice: &Notice) -> Result<naui::Stack> {
    let pane = parts::pane(ui)?;

    parts::section(
        ui,
        &pane,
        "List",
        &["ListItem を set_items で並べます。補足表示の有無、選べない行、単一・複数選択を確認できます。"],
    )?;

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
    list.on_select({
        let notice = notice.clone();
        let detailed = detailed.clone();
        move |indices| {
            let labels: Vec<&str> = indices
                .iter()
                .filter_map(|&index| detailed.get(index).map(|item| item.label.as_str()))
                .collect();
            if labels.is_empty() {
                notice.show("選択: なし");
            } else {
                notice.show(&format!("選択: {}", labels.join(" / ")));
            }
        }
    });

    let (mode_row, mode) = parts::choice(ui, "選択方法", &["単一", "複数"])?;
    mode.on_select({
        let list = list.clone();
        move |index| {
            list.set_selection_mode(if index == 0 {
                SelectionMode::Single
            } else {
                SelectionMode::Multiple
            });
        }
    });
    pane.append(&mode_row);
    pane.append(&list);
    pane.append(&parts::note(
        ui,
        "一覧を右クリックすると PopupMenu が開きます。",
    )?);

    let popup = ui.popup_menu()?;
    popup.set_items(&[
        PopupItem::new("先頭を選択"),
        PopupItem::new("選択を解除"),
        PopupItem::separator(),
        PopupItem::new("無効なメニュー項目").enabled(false),
    ]);
    popup.on_select({
        let list = list.clone();
        move |index| match index {
            0 => list.select(0),
            1 => list.clear_selection(),
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
    let detail_toggle = ui.checkbox("補足を表示")?;
    detail_toggle.set_checked(true);
    detail_toggle.on_toggle({
        let list = list.clone();
        let detailed = detailed.clone();
        move |showing| list.set_items(if showing { &detailed } else { &plain })
    });
    actions.append(&select_example);
    actions.append(&detail_toggle);
    pane.append(&actions);

    build_composed_list(ui, &pane)?;
    build_dynamic_rows(ui, &pane)?;

    build_tree(ui, &pane, notice)?;
    Ok(pane)
}

/// 設定画面のような、先頭・本文・末尾を自由に組んだ行 (`ListRow`)。
fn build_composed_list(ui: &Ui, pane: &naui::Stack) -> Result<()> {
    parts::section(
        ui,
        pane,
        "ListRow",
        &["組み立てたウィジェットを set_rows で並べます。行を押すと on_activate でチェックが切り替わります。"],
    )?;

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
fn build_dynamic_rows(ui: &Ui, pane: &naui::Stack) -> Result<()> {
    parts::section(
        ui,
        pane,
        "後から作る行",
        &["Ui は clone できます。押されたところで行の中身を組み立て、set_rows へ渡しています。"],
    )?;

    let list = ui.list()?;
    list.set_sizing(Sizing::fill_width());
    // 行は積み上げていくので、並びはアプリ側で持つ。
    let rows: Rc<RefCell<Vec<ListRow>>> = Rc::new(RefCell::new(Vec::new()));

    let actions = ui.stack(Orientation::Horizontal)?;
    actions.set_spacing(8.0);
    let add = ui.button("行を足す")?;
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
                ui.label(&format!("あとから作った行 {index}")),
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

    let clear = ui.button("空にする")?;
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
    parts::section(
        ui,
        pane,
        "Tree",
        &["入れ子の項目の開閉と、選べない枝を確認できます。"],
    )?;

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

    tree.on_select({
        let notice = notice.clone();
        let items = items.clone();
        move |path| match TreeItem::at(&items, path) {
            Some(item) => notice.show(&format!("選択: {} {path:?}", item.label)),
            None => notice.show("選択: なし"),
        }
    });
    tree.on_expand({
        let notice = notice.clone();
        let items = items.clone();
        move |path, expanded| {
            let Some(item) = TreeItem::at(&items, path) else {
                return;
            };
            let state = if expanded { "開いた" } else { "閉じた" };
            notice.show(&format!("{} を{state}", item.label));
        }
    });
    pane.append(&tree);

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
