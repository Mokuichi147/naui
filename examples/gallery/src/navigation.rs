use naui::{NavItem, Result, Sizing, Ui};

use crate::parts;

/// 各ナビゲーション UI の形と選択通知。
pub(crate) fn build(ui: &Ui) -> Result<naui::Stack> {
    let pane = parts::pane(ui)?;

    parts::section(
        ui,
        &pane,
        "Tabs",
        &["Gallery 上部のタブが Tabs の例です。中身ごと切り替えます。"],
    )?;

    let status = parts::status(ui, "操作結果: なし")?;

    parts::section(ui, &pane, "Navbar", &["見出し付きの横並びナビゲーション。"])?;
    let navbar = ui.navbar("Navbar")?;
    navbar.set_items(&NavItem::list(["項目 A", "項目 B", "項目 C"]));
    navbar.set_selected(0);
    navbar.on_select({
        let status = status.clone();
        move |index| status.set_text(&format!("Navbar: 項目 {}", index + 1))
    });
    pane.append(&navbar);

    parts::section(
        ui,
        &pane,
        "Menu",
        &["縦並びで選択状態を持つナビゲーション。"],
    )?;
    let menu = ui.menu()?;
    menu.set_items(&[
        NavItem::new("項目 A"),
        NavItem::new("項目 B"),
        NavItem::new("無効な項目").enabled(false),
    ]);
    menu.set_selected(0);
    menu.on_select({
        let status = status.clone();
        move |index| status.set_text(&format!("Menu: 項目 {}", index + 1))
    });
    pane.append(&menu);

    parts::section(ui, &pane, "Breadcrumbs", &["階層と現在地を表示します。"])?;
    let breadcrumbs = ui.breadcrumbs()?;
    breadcrumbs.set_items(&NavItem::list(["階層 1", "階層 2", "現在地"]));
    breadcrumbs.on_select({
        let status = status.clone();
        move |index| status.set_text(&format!("Breadcrumbs: {} 番目", index + 1))
    });
    pane.append(&breadcrumbs);

    parts::section(ui, &pane, "Pagination", &["ページ番号と前後移動。"])?;
    let pagination = ui.pagination(5)?;
    pagination.on_change({
        let status = status.clone();
        move |page| status.set_text(&format!("Pagination: {} ページ", page + 1))
    });
    pane.append(&pagination);

    parts::section(ui, &pane, "Dock", &["等幅の横並びナビゲーション。"])?;
    let dock = ui.dock()?;
    dock.set_items(&NavItem::list(["左", "中央", "右"]));
    dock.set_sizing(Sizing::fill_width());
    dock.on_select({
        let status = status.clone();
        move |index| status.set_text(&format!("Dock: {}", ["左", "中央", "右"][index]))
    });
    pane.append(&dock);

    parts::section(ui, &pane, "Link", &["ブラウザまたは標準アプリで開きます。"])?;
    pane.append(&ui.link("naui のリポジトリ", "https://github.com/mokuichi147/naui")?);

    parts::section(
        ui,
        &pane,
        "操作結果",
        &["上のどれを選んでも、最後の操作をここへ出します。"],
    )?;
    pane.append(&status);
    Ok(pane)
}
