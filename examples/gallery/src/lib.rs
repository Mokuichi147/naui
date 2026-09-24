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
    Align, GridCell, NavItem, Orientation, Padding, Result, ScrollPolicy, Settings, Sizing, Tabs,
    TextStyle, Track, Ui, Widget,
};

/// 共通の UI 構築。バックエンドによらず同じコードが動く。
pub fn build(ui: &Ui) -> Result<()> {
    let window = ui.window("naui UI gallery", 800.0, 860.0)?;
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

    let tabs = ui.tabs()?;
    let panes: [(&str, naui::Stack); 11] = [
        ("基本", basics::build(ui, &window, &notice)?),
        ("入力", input::build(ui, &notice)?),
        ("一覧", list::build(ui, &notice)?),
        ("表", table::build(ui, &notice)?),
        ("ナビゲーション", navigation::build(ui, &notice)?),
        ("レイアウト", layout::build(ui, &notice)?),
        ("描画", canvas::build(ui, &notice)?),
        ("ファイル", files::build(ui, &notice)?),
        ("メディア", media::build(ui, &notice)?),
        ("ダイアログ", dialog::build(ui, &notice)?),
        ("非同期", tasks::build(ui, &notice)?),
    ];
    for (title, pane) in &panes {
        add_pane(ui, &tabs, title, pane)?;
    }
    tabs.set_sizing(Sizing::fill());
    root.attach(&tabs, GridCell::new(0, 1));

    let sections = panes.map(|(title, _)| title);
    commands::attach(ui, &window, &tabs, &sections, &notice)?;
    tabs.on_select({
        let crumbs = crumbs.clone();
        move |index| {
            let Some(section) = sections.get(index) else {
                return;
            };
            crumbs.set_items(&NavItem::list(["naui gallery", *section]));
        }
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
