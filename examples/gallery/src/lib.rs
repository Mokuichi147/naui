//! naui の UI を種別ごとに試すギャラリー。
//!
//! 表示されるコントロールはすべて OS (またはブラウザ) の実ウィジェット。
//! macOS なら NSButton / NSTextField、Web なら `<button>` / `<input>`。

mod basics;
mod dialog;
mod files;
mod input;
mod layout;
mod list;
mod media;
mod navigation;
mod tasks;

use naui::{
    FileEntry, GridCell, NavItem, Orientation, Padding, Result, ScrollPolicy, Settings, Sizing,
    Tabs, ToolbarIcon, ToolbarItem, Track, Ui, Widget,
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

const SECTIONS: [&str; 9] = [
    "基本",
    "入力",
    "一覧",
    "ナビゲーション",
    "レイアウト",
    "ファイル",
    "メディア",
    "ダイアログ",
    "非同期",
];

/// 共通の UI 構築。バックエンドによらず同じコードが動く。
pub fn build(ui: &Ui) -> Result<()> {
    let window = ui.window("naui UI gallery", 800.0, 860.0)?;
    let root = ui.grid()?;
    root.set_spacing(0.0, 10.0);
    root.set_padding(Padding::all(20.0));
    root.set_column_track(0, Track::FILL);
    root.set_row_track(0, Track::Auto);
    root.set_row_track(1, Track::FILL);

    // Windows の StackPanel は主軸方向の Fill に残りの高さを配らないため、
    // 固定部分だけを Stack にまとめ、タブは Grid の Fill 行へ直接置く。
    let header = ui.stack(Orientation::Vertical)?;
    header.set_spacing(10.0);
    // 見出しはウィンドウの幅いっぱいに広げる (中の文字は中央ぞろえ)。
    header.set_sizing(Sizing::fill_width());

    let crumbs = ui.breadcrumbs()?;
    crumbs.set_items(&NavItem::list(["naui gallery", "基本"]));
    // パンくずは画面全体の現在地を示すため、タイトルより先の左上へ置く。
    // 幅いっぱいの横 Stack に入れることで、ほかの要素の配置は変えずに左へ寄せる。
    let breadcrumb_row = ui.stack(Orientation::Horizontal)?;
    breadcrumb_row.set_sizing(Sizing::fill_width());
    breadcrumb_row.append(&crumbs);
    header.append(&breadcrumb_row);

    header.append(&ui.label("naui UI ギャラリー")?);
    header.append(&ui.label("UI の種別ごとに、特徴・状態・操作結果を確認できます。")?);

    // Toolbar はレイアウトではなくウィンドウに取り付ける。macOS では
    // NSToolbar、Linux では AdwHeaderBar としてタイトルバーに出る。
    // 項目はアイコンで並び、ラベルはツールチップと読み上げに使われる。
    let toolbar_status = ui.label("Toolbar: まだ押されていません")?;
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
    root.attach(&header, GridCell::new(0, 0));

    let tabs = ui.tabs()?;
    add_pane(ui, &tabs, "基本", &basics::build(ui, &window)?)?;
    add_pane(ui, &tabs, "入力", &input::build(ui)?)?;
    add_pane(ui, &tabs, "一覧", &list::build(ui)?)?;
    add_pane(ui, &tabs, "ナビゲーション", &navigation::build(ui)?)?;
    add_pane(ui, &tabs, "レイアウト", &layout::build(ui)?)?;
    add_pane(ui, &tabs, "ファイル", &files::build(ui)?)?;
    add_pane(ui, &tabs, "メディア", &media::build(ui)?)?;
    add_pane(ui, &tabs, "ダイアログ", &dialog::build(ui)?)?;
    add_pane(ui, &tabs, "非同期", &tasks::build(ui)?)?;
    tabs.set_sizing(Sizing::fill());
    root.attach(&tabs, GridCell::new(0, 1));

    tabs.on_select({
        let crumbs = crumbs.clone();
        move |index| {
            let Some(section) = SECTIONS.get(index) else {
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
