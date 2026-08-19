use naui::{NavItem, Orientation, Padding, Result, Sizing, Ui};

/// 各ナビゲーション UI の形と選択通知。
pub(crate) fn build(ui: &Ui) -> Result<naui::Stack> {
    let pane = ui.stack(Orientation::Vertical)?;
    pane.set_spacing(12.0);
    pane.set_padding(Padding::all(12.0));

    pane.append(&ui.label("Tabs")?);
    pane.append(&ui.label("Gallery 上部のタブが Tabs の例です。中身ごと切り替えます。")?);

    let status = ui.label("操作結果: なし")?;

    pane.append(&ui.label("Navbar — 見出し付きの横並びナビゲーション")?);
    let navbar = ui.navbar("Navbar")?;
    navbar.set_items(&NavItem::list(["項目 A", "項目 B", "項目 C"]));
    navbar.set_selected(0);
    navbar.on_select({
        let status = status.clone();
        move |index| status.set_text(&format!("Navbar: 項目 {}", index + 1))
    });
    pane.append(&navbar);

    pane.append(&ui.label("Menu — 縦並びで選択状態を持つナビゲーション")?);
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

    pane.append(&ui.label("Breadcrumbs — 階層と現在地を表示")?);
    let breadcrumbs = ui.breadcrumbs()?;
    breadcrumbs.set_items(&NavItem::list(["階層 1", "階層 2", "現在地"]));
    breadcrumbs.on_select({
        let status = status.clone();
        move |index| status.set_text(&format!("Breadcrumbs: {} 番目", index + 1))
    });
    let breadcrumb_row = ui.stack(Orientation::Horizontal)?;
    breadcrumb_row.set_sizing(Sizing::fill_width());
    breadcrumb_row.append(&breadcrumbs);
    pane.append(&breadcrumb_row);

    pane.append(&ui.label("Pagination — ページ番号と前後移動")?);
    let pagination = ui.pagination(5)?;
    pagination.on_change({
        let status = status.clone();
        move |page| status.set_text(&format!("Pagination: {} ページ", page + 1))
    });
    pane.append(&pagination);

    pane.append(&ui.label("Dock — 等幅の横並びナビゲーション")?);
    let dock = ui.dock()?;
    dock.set_items(&NavItem::list(["左", "中央", "右"]));
    dock.set_sizing(Sizing::fill_width());
    dock.on_select({
        let status = status.clone();
        move |index| status.set_text(&format!("Dock: {}", ["左", "中央", "右"][index]))
    });
    pane.append(&dock);

    pane.append(&ui.link(
        "Link — ブラウザまたは標準アプリで開く",
        "https://github.com/mokuichi147/naui",
    )?);
    pane.append(&status);
    Ok(pane)
}
