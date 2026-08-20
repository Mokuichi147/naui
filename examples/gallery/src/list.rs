use std::cell::Cell;
use std::rc::Rc;

use naui::{
    Length, ListItem, NavItem, Orientation, Padding, PopupItem, Result, SelectionMode, Sizing,
    TreeItem, Ui,
};

/// List の補足表示、無効な行、単一・複数選択、コンテキストメニューと、
/// Tree の開閉・選択。
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

    build_tree(ui, &pane)?;
    Ok(pane)
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
