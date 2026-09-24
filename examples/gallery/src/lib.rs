//! naui の UI を種別ごとに試すギャラリー。
//!
//! 表示されるコントロールはすべて OS (またはブラウザ) の実ウィジェット。
//! macOS なら NSButton / NSTextField、Web なら `<button>` / `<input>`。

mod basics;
mod canvas;
mod dialog;
mod files;
mod input;
mod layout;
mod list;
mod media;
mod navigation;
mod parts;
mod tasks;

use naui::{
    Align, FileEntry, GridCell, MenuItem, MenuShortcut, MenuSpec, NavItem, Orientation, Padding,
    Result, ScrollPolicy, Settings, SidebarItem, SidebarSection, Sizing, Tabs, TextStyle,
    ToolbarIcon, ToolbarItem, Track, Ui, Widget,
};

/// ウィンドウに取り付けるツールバーの項目。区切りは空文字で埋める。
const COMMANDS: [&str; 4] = ["新規", "開く", "", "保存"];

/// 上の項目に対応するアイコン。
const COMMAND_ICONS: [ToolbarIcon; 4] = [
    ToolbarIcon::New,
    ToolbarIcon::Open,
    ToolbarIcon::Add,
    ToolbarIcon::Save,
];

const SECTIONS: [&str; 10] = [
    "基本",
    "入力",
    "一覧",
    "ナビゲーション",
    "レイアウト",
    "描画",
    "ファイル",
    "メディア",
    "ダイアログ",
    "非同期",
];

/// サイドバーの中身。並びは [`SECTIONS`] (タブの順) と同じにしてあるので、
/// 通知の通し番号をそのままタブの位置として使える。
///
/// アイコンは [`ToolbarIcon`] の中から近いものを選んでいる。
fn sidebar_sections() -> Vec<SidebarSection> {
    let item = |index: usize, icon| SidebarItem::new(SECTIONS[index]).icon(icon);
    vec![
        SidebarSection::untitled([
            item(0, ToolbarIcon::Info),
            item(1, ToolbarIcon::Edit),
            item(2, ToolbarIcon::Search),
            item(3, ToolbarIcon::Forward),
            item(4, ToolbarIcon::Copy),
        ]),
        SidebarSection::new(
            "そのほか",
            [
                item(5, ToolbarIcon::Cut),
                item(6, ToolbarIcon::Open),
                item(7, ToolbarIcon::Share),
                item(8, ToolbarIcon::New),
                item(9, ToolbarIcon::Refresh),
            ],
        ),
    ]
}

/// メニューバーの中身。
///
/// ショートカットは**主修飾キー + 英数字 1 文字**で指定する。主修飾キーは
/// macOS だけ ⌘ で、Windows・Linux・Web では Ctrl になる。
fn menus() -> Vec<MenuSpec> {
    vec![
        MenuSpec::new(
            "ファイル",
            [
                MenuItem::new("新規").shortcut(MenuShortcut::new('n')),
                MenuItem::new("開く").shortcut(MenuShortcut::new('o')),
                MenuItem::separator(),
                MenuItem::new("保存").shortcut(MenuShortcut::new('s')),
                MenuItem::new("別名で保存").shortcut(MenuShortcut::new('s').shift(true)),
            ],
        ),
        MenuSpec::new(
            "表示",
            [
                MenuItem::new("拡大"),
                MenuItem::new("縮小"),
                MenuItem::separator(),
                // 押せない項目は、その場ではできないことを表す。
                MenuItem::new("全画面").enabled(false),
            ],
        ),
        MenuSpec::new("ヘルプ", ["naui について"]),
    ]
}

/// 共通の UI 構築。バックエンドによらず同じコードが動く。
pub fn build(ui: &Ui) -> Result<()> {
    // 幅はサイドバー (既定 220) を付けても、中身が従来の 800 を保てる大きさ。
    let window = ui.window("naui UI gallery", 1020.0, 860.0)?;
    let root = ui.grid()?;
    root.set_spacing(0.0, 10.0);
    // 上だけ詰める。ツールバーの帯 (WinUI では 48px の CommandBar) が
    // すでにタイトルバーと中身を隔てているので、そこへさらに 20 足すと空きすぎる。
    root.set_padding(Padding {
        top: 8.0,
        right: 20.0,
        bottom: 20.0,
        left: 20.0,
    });
    root.set_column_track(0, Track::FILL);
    root.set_row_track(0, Track::Auto);
    root.set_row_track(1, Track::FILL);

    // Windows の StackPanel は主軸方向の Fill に残りの高さを配らないため、
    // 固定部分だけを Stack にまとめ、タブは Grid の Fill 行へ直接置く。
    let header = ui.stack(Orientation::Vertical)?;
    header.set_spacing(6.0);
    // 見出しはウィンドウの幅いっぱいに広げ、中身は左端でそろえる
    // (交差軸の既定は中央ぞろえ)。
    header.set_sizing(Sizing::fill_width());
    header.set_align(Align::Start);

    let crumbs = ui.breadcrumbs()?;
    crumbs.set_items(&NavItem::list(["naui gallery", "基本"]));
    // パンくずは画面全体の現在地を示すため、タイトルより先の左上へ置く。
    header.append(&crumbs);

    // 画面の顔になる見出しなので、本文より 1 段大きい段階を指定する。
    let title = ui.label("naui UI ギャラリー")?;
    title.set_style(TextStyle::Title);
    header.append(&title);
    header.append(&parts::note(
        ui,
        "UI の種別ごとに、特徴・状態・操作結果を確認できます。",
    )?);

    // Toolbar はレイアウトではなくウィンドウに取り付ける。macOS では
    // NSToolbar、Linux では AdwHeaderBar としてタイトルバーに出る。
    // 項目はアイコンで並び、ラベルはツールチップと読み上げに使われる。
    let toolbar_status = parts::status(ui, "Toolbar: まだ押されていません")?;
    let toolbar = ui.toolbar()?;
    toolbar.set_items(&[
        ToolbarItem::new(COMMAND_ICONS[0], COMMANDS[0]),
        ToolbarItem::new(COMMAND_ICONS[1], COMMANDS[1]),
        ToolbarItem::separator(),
        // 保存できるものがまだ無い状態から始める。
        ToolbarItem::new(COMMAND_ICONS[3], COMMANDS[3]).enabled(false),
    ]);
    toolbar.on_activate({
        let status = toolbar_status.clone();
        let toolbar = toolbar.clone();
        move |index| {
            status.set_text(&format!("Toolbar: {} を実行しました", COMMANDS[index]));
            // 新規・開くの後は保存できる。
            if index != 3 {
                toolbar.set_item_enabled(3, true);
            }
        }
    });
    window.set_toolbar(&toolbar);
    header.append(&toolbar_status);

    // MenuBar もレイアウトではなくウィンドウに取り付ける。macOS では画面
    // 上端のメニューバー (NSApplication.mainMenu)、ほかの 3 環境では
    // タイトルバーの下に敷かれる帯になる。
    // ショートカットの主修飾キーは macOS だけ ⌘ で、ほかは Ctrl。
    let menu_status = parts::status(ui, "MenuBar: まだ選ばれていません")?;
    let menu_bar = ui.menu_bar()?;
    let specs = menus();
    menu_bar.set_menus(&specs);
    menu_bar.on_activate({
        let status = menu_status.clone();
        // 通知はインデックスの組で来るので、渡した並びから名前を引く。
        move |menu, item| {
            let label = specs
                .get(menu)
                .and_then(|spec| spec.items.get(item))
                .map(|entry| entry.label.as_str())
                .unwrap_or_default();
            status.set_text(&format!("MenuBar: {label} を選びました"));
        }
    });
    window.set_menu_bar(&menu_bar);
    header.append(&menu_status);
    root.attach(&header, GridCell::new(0, 0));

    // Sidebar もウィンドウに取り付けるもの。起動時から付けておき、取り外しと
    // 開閉は「ナビゲーション」のタブで試せる。項目はタブと同じ並びなので、
    // 選択を互いに映し合う。
    let sidebar = ui.sidebar()?;
    sidebar.set_sections(&sidebar_sections());
    sidebar.set_selected(0);

    let tabs = ui.tabs()?;
    add_pane(ui, &tabs, "基本", &basics::build(ui, &window)?)?;
    add_pane(ui, &tabs, "入力", &input::build(ui)?)?;
    add_pane(ui, &tabs, "一覧", &list::build(ui)?)?;
    add_pane(
        ui,
        &tabs,
        "ナビゲーション",
        &navigation::build(ui, &window, &sidebar)?,
    )?;
    add_pane(ui, &tabs, "レイアウト", &layout::build(ui)?)?;
    add_pane(ui, &tabs, "描画", &canvas::build(ui)?)?;
    add_pane(ui, &tabs, "ファイル", &files::build(ui)?)?;
    add_pane(ui, &tabs, "メディア", &media::build(ui)?)?;
    add_pane(ui, &tabs, "ダイアログ", &dialog::build(ui)?)?;
    add_pane(ui, &tabs, "非同期", &tasks::build(ui)?)?;
    tabs.set_sizing(Sizing::fill());
    root.attach(&tabs, GridCell::new(0, 1));

    tabs.on_select({
        let crumbs = crumbs.clone();
        let sidebar = sidebar.clone();
        move |index| {
            let Some(section) = SECTIONS.get(index) else {
                return;
            };
            crumbs.set_items(&NavItem::list(["naui gallery", *section]));
            sidebar.set_selected(index);
        }
    });

    // サイドバーで選んだらタブを移す。タブの `select` は通知するので、
    // パンくずも上の `on_select` がそろえる。
    sidebar.on_select({
        let tabs = tabs.clone();
        move |index| tabs.select(index)
    });

    // パンくずの先頭を選ぶと概要へ戻る。現在地側はそのままにする。
    crumbs.on_select({
        let tabs = tabs.clone();
        move |index| {
            if index == 0 {
                tabs.select(0);
            }
        }
    });

    window.set_child(&root);
    window.set_sidebar(&sidebar);
    window.show();
    Ok(())
}

/// タブの中身をスクロールに載せて貼る。
///
/// ネイティブのウィンドウは、中身がはみ出しても勝手にはスクロールしない
/// (ページごと縦に伸びるのはブラウザだけ)。ギャラリーは 1 つのタブが縦に
/// 長いので、タブごとにスクロールへ載せて下まで見られるようにする。
///
/// 横は `Never` にしてある。幅はウィンドウに合わせ、縦だけを送る。
fn add_pane(ui: &Ui, tabs: &Tabs, title: &str, pane: &dyn Widget) -> Result<()> {
    let scroll = ui.scroll()?;
    scroll.set_policy(ScrollPolicy::Never, ScrollPolicy::Auto);
    scroll.set_child(pane);
    // スクロールは中身から高さを決めないので、タブの領域いっぱいを指定する。
    scroll.set_sizing(Sizing::fill());
    tabs.add_tab(title, &scroll);
    Ok(())
}

/// 選ばれたファイルやフォルダーを、画面内の短いステータスとして表す。
pub(crate) fn describe_entries(entries: &[FileEntry]) -> String {
    match entries {
        [] => "選択されていません".to_string(),
        [entry] => match entry.path() {
            Some(path) => path.display().to_string(),
            None => format!("{} (この環境ではパス非公開)", entry.name()),
        },
        many => format!("{} 件: {} ほか", many.len(), many[0].name()),
    }
}

// ネイティブの `start()` と、Web のブラウザから呼ばれる入口を作る。
naui::entry!(Settings::new("naui UI gallery"), build);
