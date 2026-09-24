use naui::{Align, Length, NavItem, Orientation, Padding, Result, Sidebar, Sizing, Ui, Window};

use crate::parts::{self, Notice};

/// 各ナビゲーション UI の形と選択通知。
///
/// `sidebar` はギャラリーの区分を選ぶためのもので、起動時からウィンドウに
/// 付いている ([`crate::build`])。ここでは取り外しと開閉を試せるようにする。
pub(crate) fn build(
    ui: &Ui,
    window: &Window,
    sidebar: &Sidebar,
    notice: &Notice,
) -> Result<naui::Stack> {
    let pane = parts::pane(ui)?;

    parts::section(
        ui,
        &pane,
        "Sidebar",
        &[
            "ウィンドウの左に付いているのがサイドバーです。レイアウトではなくウィンドウに取り付けます。",
            "項目を選ぶと右側の画面が切り替わります。外している間は「表示」メニューから移れます。",
            "開閉はその環境のサイドバーボタンでも行えます。ボタンはウィンドウの上端の左 (ツールバーと同じ高さ) にあります。",
            "幅は仕切りをドラッグして変えられます。",
        ],
    )?;
    let attach = ui.checkbox("ウィンドウに付ける")?;
    attach.set_checked(true);
    let collapse = ui.checkbox("閉じる")?;
    attach.on_toggle({
        let window = window.clone();
        let sidebar = sidebar.clone();
        let collapse = collapse.clone();
        move |on| {
            if on {
                window.set_sidebar(&sidebar);
            } else {
                window.clear_sidebar();
            }
            // 付けていない間は開閉しても見えないので、押せなくする。
            collapse.set_enabled(on);
        }
    });
    collapse.on_toggle({
        let sidebar = sidebar.clone();
        move |on| sidebar.set_collapsed(on)
    });
    // サイドバーボタン (その環境の標準のもの) で開閉されたら、チェックも合わせる。
    sidebar.on_collapse({
        let collapse = collapse.clone();
        move |collapsed| collapse.set_checked(collapsed)
    });
    // 仕切りで幅を変えると届く。
    sidebar.on_resize({
        let notice = notice.clone();
        move |value| notice.show(&format!("Sidebar: 幅は {value:.0}"))
    });
    pane.append(&attach);
    pane.append(&collapse);

    parts::section(
        ui,
        &pane,
        "Tabs",
        &["見出しを並べ、選んだものの中身だけを出します。"],
    )?;
    const PAGES: [(&str, &str); 3] = [
        (
            "概要",
            "タブごとに中身を 1 つ持ちます。選び直すと前の中身は隠れます。",
        ),
        (
            "詳細",
            "中身には Stack や Grid など、どのウィジェットでも置けます。",
        ),
        ("履歴", "選び直すと on_select が届きます。"),
    ];
    let tabs = ui.tabs()?;
    for (title, body) in PAGES {
        let page = ui.stack(Orientation::Vertical)?;
        page.set_padding(Padding::all(12.0));
        page.set_align(Align::Start);
        let text = ui.label(body)?;
        text.set_wrap(true);
        text.set_sizing(Sizing::fill_width());
        page.append(&text);
        tabs.add_tab(title, &page);
    }
    // 中身の高さでは決まらないので、大きさを指定する。
    tabs.set_sizing(
        Sizing::new()
            .width(Length::Fill)
            .height(Length::Fixed(140.0)),
    );
    tabs.on_select({
        let notice = notice.clone();
        move |index| {
            if let Some((title, _)) = PAGES.get(index) {
                notice.show(&format!("Tabs: {title}"));
            }
        }
    });
    pane.append(&tabs);

    parts::section(ui, &pane, "Navbar", &["見出し付きの横並びナビゲーション。"])?;
    let navbar = ui.navbar("Navbar")?;
    navbar.set_items(&NavItem::list(["項目 A", "項目 B", "項目 C"]));
    navbar.set_selected(0);
    navbar.on_select({
        let notice = notice.clone();
        move |index| notice.show(&format!("Navbar: 項目 {}", index + 1))
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
        let notice = notice.clone();
        move |index| notice.show(&format!("Menu: 項目 {}", index + 1))
    });
    pane.append(&menu);

    parts::section(ui, &pane, "Breadcrumbs", &["階層と現在地を表示します。"])?;
    let breadcrumbs = ui.breadcrumbs()?;
    breadcrumbs.set_items(&NavItem::list(["階層 1", "階層 2", "現在地"]));
    breadcrumbs.on_select({
        let notice = notice.clone();
        move |index| notice.show(&format!("Breadcrumbs: {} 番目", index + 1))
    });
    pane.append(&breadcrumbs);

    parts::section(ui, &pane, "Pagination", &["ページ番号と前後移動。"])?;
    let pagination = ui.pagination(5)?;
    pagination.on_change({
        let notice = notice.clone();
        move |page| notice.show(&format!("Pagination: {} ページ", page + 1))
    });
    pane.append(&pagination);

    parts::section(ui, &pane, "Dock", &["等幅の横並びナビゲーション。"])?;
    let dock = ui.dock()?;
    dock.set_items(&NavItem::list(["左", "中央", "右"]));
    dock.set_sizing(Sizing::fill_width());
    dock.on_select({
        let notice = notice.clone();
        move |index| notice.show(&format!("Dock: {}", ["左", "中央", "右"][index]))
    });
    pane.append(&dock);

    parts::section(ui, &pane, "Link", &["ブラウザまたは標準アプリで開きます。"])?;
    pane.append(&ui.link("naui のリポジトリ", "https://github.com/mokuichi147/naui")?);
    Ok(pane)
}
