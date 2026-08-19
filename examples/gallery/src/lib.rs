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

use naui::{FileEntry, NavItem, Orientation, Padding, Result, Settings, Sizing, Ui};

const SECTIONS: [&str; 8] = [
    "基本",
    "入力",
    "一覧",
    "ナビゲーション",
    "レイアウト",
    "ファイル",
    "メディア",
    "ダイアログ",
];

/// 共通の UI 構築。バックエンドによらず同じコードが動く。
pub fn build(ui: &Ui) -> Result<()> {
    let window = ui.window("naui UI gallery", 800.0, 860.0)?;
    let root = ui.stack(Orientation::Vertical)?;
    root.set_spacing(10.0);
    root.set_padding(Padding::all(20.0));

    let crumbs = ui.breadcrumbs()?;
    crumbs.set_items(&NavItem::list(["naui gallery", "基本"]));
    // パンくずは画面全体の現在地を示すため、タイトルより先の左上へ置く。
    // 幅いっぱいの横 Stack に入れることで、ほかの要素の配置は変えずに左へ寄せる。
    let breadcrumb_row = ui.stack(Orientation::Horizontal)?;
    breadcrumb_row.set_sizing(Sizing::fill_width());
    breadcrumb_row.append(&crumbs);
    root.append(&breadcrumb_row);

    root.append(&ui.label("naui UI ギャラリー")?);
    root.append(&ui.label("UI の種別ごとに、特徴・状態・操作結果を確認できます。")?);

    let tabs = ui.tabs()?;
    tabs.add_tab("基本", &basics::build(ui, &window)?);
    tabs.add_tab("入力", &input::build(ui)?);
    tabs.add_tab("一覧", &list::build(ui)?);
    tabs.add_tab("ナビゲーション", &navigation::build(ui)?);
    tabs.add_tab("レイアウト", &layout::build(ui)?);
    tabs.add_tab("ファイル", &files::build(ui)?);
    tabs.add_tab("メディア", &media::build(ui)?);
    tabs.add_tab("ダイアログ", &dialog::build(ui)?);
    tabs.set_sizing(Sizing::fill());
    root.append(&tabs);

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
