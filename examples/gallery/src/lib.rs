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

use std::cell::Cell;
use std::rc::Rc;

use naui::{
    GridCell, NavItem, Padding, Result, Scroll, ScrollPolicy, Settings, SidebarItem,
    SidebarSection, Sizing, ToolbarIcon, Track, Ui, Widget,
};

/// ギャラリーの区分。サイドバーと「表示」メニューの項目はこの順に並ぶので、
/// どちらの通知の通し番号も、そのまま区分の位置として使える。
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
    // 画面の中身は Grid の Fill 行へ直接置く。Windows の StackPanel は主軸方向の
    // Fill に残りの高さを配らないため、Stack へ入れると下まで伸びない。
    root.set_row_track(0, Track::Auto);
    root.set_row_track(1, Track::FILL);

    // パンくずは画面全体の現在地を示すため、どの区分でも左上に置く。
    // ギャラリーの題と説明は、最初の区分 (基本) の画面の先頭にだけ出す。
    let crumbs = ui.breadcrumbs()?;
    crumbs.set_items(&NavItem::list(["naui gallery", SECTIONS[0].0]));
    root.attach(&crumbs, GridCell::new(0, 0));

    // 操作の結果は、画面に Label を並べずトーストで知らせる。
    let notice = parts::Notice::new(ui)?;

    // Sidebar もウィンドウに取り付けるもの。選んだ区分の画面だけを下の行へ
    // 出す。取り外しと開閉は「ナビゲーション」の画面で試せる。
    let sidebar = ui.sidebar()?;
    sidebar.set_sections(&sidebar_sections());

    let (media, stop_media) = media::build(ui, &notice)?;

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
        media,
        dialog::build(ui, &notice)?,
        tasks::build(ui, &notice)?,
    ];
    let screens = panes
        .iter()
        .map(|pane| scrollable(ui, pane))
        .collect::<Result<Vec<_>>>()?;

    // 区分を移る。サイドバー・「表示」メニュー・パンくずのどこから選んでも
    // ここを通るので、ほかの 2 つの表示もここでそろえる。
    // サイドバーの `set_selected` は通知しないので、選び直しが回り続けない。
    let go: Rc<dyn Fn(usize)> = Rc::new({
        let root = root.clone();
        let crumbs = crumbs.clone();
        let sidebar = sidebar.clone();
        let current = Cell::new(None);
        move |index| {
            let (Some(screen), Some((title, _))) = (screens.get(index), SECTIONS.get(index)) else {
                return;
            };
            if current.replace(Some(index)) == Some(index) {
                return;
            }
            // 画面から外しても再生は止まらないので、区分を移るたびに止める。
            // 止まっているものを止めても何も起きない。
            stop_media();
            root.replace(screen, GridCell::new(0, 1));
            crumbs.set_items(&NavItem::list(["naui gallery", *title]));
            sidebar.set_selected(index);
        }
    });
    go(0);

    sidebar.on_select({
        let go = go.clone();
        move |index| go(index)
    });

    // パンくずの先頭を選ぶと概要 (先頭の区分) へ戻る。現在地側はそのままにする。
    crumbs.on_select({
        let go = go.clone();
        move |index| {
            if index == 0 {
                go(0);
            }
        }
    });

    // サイドバーを外している間も、「表示」メニューから区分を移れる。
    let titles = SECTIONS.map(|(title, _)| title);
    commands::attach(ui, &window, &titles, go, &notice)?;

    window.set_child(&root);
    window.set_sidebar(&sidebar);
    window.show();
    Ok(())
}

/// 区分の画面をスクロールに載せる。
///
/// ネイティブのウィンドウは、中身がはみ出しても勝手にはスクロールしない
/// (ページごと縦に伸びるのはブラウザだけ)。ギャラリーは 1 つの画面が縦に
/// 長いので、画面ごとにスクロールへ載せて下まで見られるようにする。
///
/// 横は `Never` にしてある。幅はウィンドウに合わせ、縦だけを送る。
fn scrollable(ui: &Ui, pane: &dyn Widget) -> Result<Scroll> {
    let scroll = ui.scroll()?;
    scroll.set_policy(ScrollPolicy::Never, ScrollPolicy::Auto);
    scroll.set_child(pane);
    // スクロールは中身から高さを決めないので、置かれた行いっぱいを指定する。
    scroll.set_sizing(Sizing::fill());
    Ok(scroll)
}

// ネイティブの `start()` と、Web のブラウザから呼ばれる入口を作る。
naui::entry!(Settings::new("naui UI gallery"), build);
