//! naui の UI を種別ごとに試すギャラリー。
//!
//! 表示されるコントロールはすべて OS (またはブラウザ) の実ウィジェット。
//! macOS なら NSButton / NSTextField、Web なら `<button>` / `<input>`。

mod basics;
mod canvas;
mod commands;
mod dialog;
mod files;
mod input;
mod layout;
mod list;
mod media;
mod navigation;
mod parts;
mod table;
mod tasks;

use naui::{
    Align, GridCell, NavItem, Orientation, Padding, Result, ScrollPolicy, Settings, SidebarItem,
    SidebarSection, Sizing, Tabs, TextStyle, ToolbarIcon, Track, Ui, Widget,
};

/// ギャラリーの区分。タブとサイドバーの項目はこの順に並ぶので、どちらの
/// 通知の通し番号も、そのままもう一方の位置として使える。
///
/// アイコンはサイドバーに出す。[`ToolbarIcon`] の中から近いものを選んでいる。
const SECTIONS: [(&str, ToolbarIcon); 11] = [
    ("基本", ToolbarIcon::Info),
    ("入力", ToolbarIcon::Edit),
    ("一覧", ToolbarIcon::Search),
    ("表", ToolbarIcon::Print),
    ("ナビゲーション", ToolbarIcon::Forward),
    ("レイアウト", ToolbarIcon::Copy),
    ("描画", ToolbarIcon::Cut),
    ("ファイル", ToolbarIcon::Open),
    ("メディア", ToolbarIcon::Share),
    ("ダイアログ", ToolbarIcon::New),
    ("非同期", ToolbarIcon::Refresh),
];

/// サイドバーで見出しなしの先頭にまとめる数。残りは「そのほか」へ入れる。
const PRIMARY_SECTIONS: usize = 6;

/// サイドバーの中身。見出しで区切っても通し番号は先頭から数える。
fn sidebar_sections() -> Vec<SidebarSection> {
    let items = SECTIONS.map(|(title, icon)| SidebarItem::new(title).icon(icon));
    let (primary, rest) = items.split_at(PRIMARY_SECTIONS);
    vec![
        SidebarSection::untitled(primary.to_vec()),
        SidebarSection::new("そのほか", rest.to_vec()),
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
        "UI の種別ごとに、特徴と状態を確認できます。操作の結果は画面の下端にトーストで出ます。",
    )?);

    // 操作の結果は、画面に Label を並べずトーストで知らせる。
    let notice = parts::Notice::new(ui)?;

    root.attach(&header, GridCell::new(0, 0));

    // Sidebar もウィンドウに取り付けるもの。起動時から付けておき、取り外しと
    // 開閉は「ナビゲーション」のタブで試せる。項目はタブと同じ並びなので、
    // 選択を互いに映し合う。
    let sidebar = ui.sidebar()?;
    sidebar.set_sections(&sidebar_sections());
    sidebar.set_selected(0);

    // 並びは SECTIONS と同じ。
    let panes = [
        basics::build(ui, &window, &notice)?,
        input::build(ui, &notice)?,
        list::build(ui, &notice)?,
        table::build(ui, &notice)?,
        navigation::build(ui, &window, &sidebar, &notice)?,
        layout::build(ui, &notice)?,
        canvas::build(ui, &notice)?,
        files::build(ui, &notice)?,
        media::build(ui, &notice)?,
        dialog::build(ui, &notice)?,
        tasks::build(ui, &notice)?,
    ];
    let tabs = ui.tabs()?;
    for ((title, _), pane) in SECTIONS.iter().zip(&panes) {
        add_pane(ui, &tabs, title, pane)?;
    }
    tabs.set_sizing(Sizing::fill());
    root.attach(&tabs, GridCell::new(0, 1));

    let titles = SECTIONS.map(|(title, _)| title);
    commands::attach(ui, &window, &tabs, &titles, &notice)?;
    tabs.on_select({
        let crumbs = crumbs.clone();
        let sidebar = sidebar.clone();
        move |index| {
            let Some(title) = titles.get(index) else {
                return;
            };
            crumbs.set_items(&NavItem::list(["naui gallery", *title]));
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

// ネイティブの `start()` と、Web のブラウザから呼ばれる入口を作る。
naui::entry!(Settings::new("naui UI gallery"), build);
