use naui::{Align, Length, NavItem, Orientation, Padding, Result, Sidebar, Sizing, Ui, Window};

use crate::parts::{self, Form, Notice};

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

    // ウィンドウの左に付いているのがサイドバー。レイアウトではなくウィンドウに
    // 取り付ける。外している間は「View」メニューから区分を移れる。開閉は
    // その環境のサイドバーボタンでも行え、幅は仕切りをドラッグして変えられる。
    parts::section(ui, &pane, "Sidebar")?;
    let attach = ui.checkbox("Attached")?;
    attach.set_checked(true);
    let collapse = ui.checkbox("Collapsed")?;
    // ウィンドウを狭めると畳まれ、広げると開き直す (既定で入っている)。
    let auto = ui.checkbox("Auto-collapse")?;
    auto.set_checked(sidebar.auto_collapse());
    auto.on_toggle({
        let sidebar = sidebar.clone();
        move |on| sidebar.set_auto_collapse(on)
    });
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
    // サイドバーボタン (その環境の標準のもの) やウィンドウの幅で開閉されたら、
    // チェックも合わせる。
    // collapse の通知が sidebar を捕まえているので、こちらは弱参照にする。
    sidebar.on_collapse({
        let collapse = collapse.downgrade();
        move |collapsed| {
            if let Some(collapse) = collapse.upgrade() {
                collapse.set_checked(collapsed);
            }
        }
    });
    // 仕切りで幅を変えると届く。
    sidebar.on_resize({
        let notice = notice.clone();
        move |value| notice.show(&format!("Sidebar width: {value:.0}"))
    });
    let sidebar_row = parts::row(ui)?;
    sidebar_row.set_spacing(16.0);
    sidebar_row.append(&attach);
    sidebar_row.append(&collapse);
    sidebar_row.append(&auto);
    pane.append(&sidebar_row);

    build_tabs(ui, &pane, notice)?;

    // 残りは名前とウィジェットの 2 列に並べる。どれも選ぶと通知が届く。
    parts::section(ui, &pane, "Navigation controls")?;
    let form = Form::new(ui, &pane)?;

    let navbar = ui.navbar("Mail")?;
    navbar.set_items(&NavItem::list(["Inbox", "Sent", "Drafts"]));
    navbar.set_selected(0);
    navbar.on_select({
        let notice = notice.clone();
        move |index| notice.show(&format!("Navbar: item {}", index + 1))
    });
    form.row(ui, "Navbar", &navbar)?;

    // 縦並びで選択状態を持つ。
    let menu = ui.menu()?;
    menu.set_items(&[
        NavItem::new("Profile"),
        NavItem::new("Security"),
        NavItem::new("Billing").enabled(false),
    ]);
    menu.set_selected(0);
    menu.on_select({
        let notice = notice.clone();
        move |index| notice.show(&format!("Menu: item {}", index + 1))
    });
    form.row(ui, "Menu", &menu)?;

    let breadcrumbs = ui.breadcrumbs()?;
    breadcrumbs.set_items(&NavItem::list(["Home", "Documents", "Reports"]));
    breadcrumbs.on_select({
        let notice = notice.clone();
        move |index| notice.show(&format!("Breadcrumbs: level {}", index + 1))
    });
    form.row(ui, "Breadcrumbs", &breadcrumbs)?;

    let pagination = ui.pagination(5)?;
    pagination.on_change({
        let notice = notice.clone();
        move |page| notice.show(&format!("Pagination: page {}", page + 1))
    });
    form.row(ui, "Pagination", &pagination)?;

    // 等幅の横並び。
    const DOCK: [&str; 3] = ["Home", "Search", "Library"];
    let dock = ui.dock()?;
    dock.set_items(&NavItem::list(DOCK));
    dock.set_sizing(Sizing::fill_width());
    dock.on_select({
        let notice = notice.clone();
        move |index| notice.show(&format!("Dock: {}", DOCK[index]))
    });
    form.row(ui, "Dock", &dock)?;

    // ブラウザまたは標準アプリで開く。
    form.row(
        ui,
        "Link",
        &ui.link("naui on GitHub", "https://github.com/mokuichi147/naui")?,
    )?;
    Ok(pane)
}

/// 見出しを並べ、選んだものの中身だけを出す `Tabs`。
///
/// タブごとに違う種類の中身を置き、中身にはどのウィジェットでも置けることを
/// 説明の文ではなく中身そのもので見せる。
fn build_tabs(ui: &Ui, pane: &naui::Stack, notice: &Notice) -> Result<()> {
    parts::section(ui, pane, "Tabs")?;
    const TITLES: [&str; 3] = ["Overview", "Options", "History"];

    let page = || -> Result<naui::Stack> {
        let page = ui.stack(Orientation::Vertical)?;
        page.set_spacing(8.0);
        page.set_padding(Padding::all(12.0));
        page.set_align(Align::Start);
        Ok(page)
    };

    let overview = page()?;
    let text = ui.label(
        "Weekly sync moves to Tuesday at 10:00. Slides go in the shared folder the day before.",
    )?;
    text.set_wrap(true);
    text.set_sizing(Sizing::fill_width());
    overview.append(&text);

    let options = page()?;
    options.append(&ui.checkbox("Send reminders")?);
    options.append(&ui.checkbox("Share with the team")?);

    let history = page()?;
    for entry in ["Oct 21 — Created", "Oct 22 — Renamed", "Oct 23 — Shared"] {
        history.append(&ui.label(entry)?);
    }

    let tabs = ui.tabs()?;
    for (title, content) in TITLES.iter().zip([&overview, &options, &history]) {
        tabs.add_tab(title, content);
    }
    // 中身の高さでは決まらないので、大きさを指定する。
    tabs.set_sizing(
        Sizing::new()
            .width(Length::Fill)
            .height(Length::Fixed(150.0)),
    );
    tabs.on_select({
        let notice = notice.clone();
        move |index| {
            if let Some(title) = TITLES.get(index) {
                notice.show(&format!("Tab: {title}"));
            }
        }
    });
    pane.append(&tabs);
    Ok(())
}
