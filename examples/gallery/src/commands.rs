//! ウィンドウに取り付けるツールバーとメニューバー。
//!
//! どちらもレイアウトではなくウィンドウに取り付ける。ツールバーは macOS では
//! NSToolbar、Linux では AdwHeaderBar としてタイトルバーに出る。メニューバーは
//! macOS では画面上端のメニューバー (NSApplication.mainMenu)、ほかの 3 環境では
//! タイトルバーの下に敷かれる帯になる。
//!
//! 同じ操作はツールバーとメニューのどちらから選んでも同じ処理へ届け、
//! 押せる・押せないも両方で合わせる。実際のアプリと同じ作りにしておく。

use std::rc::Rc;

use naui::{
    Dialog, MenuBar, MenuItem, MenuShortcut, MenuSpec, Result, Toolbar, ToolbarIcon, ToolbarItem,
    Ui, Window,
};

use crate::parts::Notice;

/// ツールバーとメニューの両方から選べる操作。
#[derive(Clone, Copy)]
enum Command {
    New,
    Open,
    Save,
    SaveAs,
}

/// ツールバーの並び。区切りは `None` で表す。
///
/// 通知は区切りも数えた位置で届くので、並びをそのまま引けるようにしておく。
const TOOLBAR: [Option<(ToolbarIcon, &str, Command)>; 4] = [
    Some((ToolbarIcon::New, "新規", Command::New)),
    Some((ToolbarIcon::Open, "開く", Command::Open)),
    None,
    Some((ToolbarIcon::Save, "保存", Command::Save)),
];

/// 「ファイル」メニューの並び。区切りは `None` で表す。
const FILE_MENU: [Option<(&str, Command)>; 5] = [
    Some(("新規", Command::New)),
    Some(("開く", Command::Open)),
    None,
    Some(("保存", Command::Save)),
    Some(("別名で保存", Command::SaveAs)),
];

/// メニューの位置。
const FILE: usize = 0;
const VIEW: usize = 1;
const HELP: usize = 2;

/// ツールバーとメニューバーを作って `window` へ取り付ける。
///
/// 「表示」メニューには `sections` の名前が並び、選ぶと `go` へその位置を渡す。
pub(crate) fn attach(
    ui: &Ui,
    window: &Window,
    sections: &[&str],
    go: Rc<dyn Fn(usize)>,
    notice: &Notice,
) -> Result<()> {
    let toolbar = ui.toolbar()?;
    let menu_bar = ui.menu_bar()?;

    // 保存できるものができるまで、保存の項目はどちらも押せなくしておく。
    toolbar.set_items(&toolbar_items());
    menu_bar.set_menus(&menus(sections));
    let run = Rc::new({
        let toolbar = toolbar.clone();
        let menu_bar = menu_bar.clone();
        let notice = notice.clone();
        move |command: Command| match command {
            Command::New | Command::Open => {
                let label = if matches!(command, Command::New) {
                    "新規"
                } else {
                    "開く"
                };
                notice.show(&format!("{label} を実行しました"));
                set_save_enabled(&toolbar, &menu_bar, true);
            }
            Command::Save => notice.show("保存しました"),
            Command::SaveAs => notice.show("別名で保存しました"),
        }
    });

    // 項目はアイコンで並び、ラベルはツールチップと読み上げに使われる。
    toolbar.on_activate({
        let run = run.clone();
        move |index| {
            if let Some(Some((_, _, command))) = TOOLBAR.get(index) {
                run(*command);
            }
        }
    });
    window.set_toolbar(&toolbar);

    let about = about_dialog(ui)?;
    menu_bar.on_activate({
        let sections = sections.len();
        move |menu, item| match menu {
            FILE => {
                if let Some(Some((_, command))) = FILE_MENU.get(item) {
                    run(*command);
                }
            }
            // 「表示」の先頭から区分の名前が並んでいる。
            VIEW if item < sections => go(item),
            HELP => about.open(),
            _ => {}
        }
    });
    window.set_menu_bar(&menu_bar);
    Ok(())
}

fn toolbar_items() -> Vec<ToolbarItem> {
    TOOLBAR
        .iter()
        .map(|entry| match entry {
            Some((icon, label, command)) => {
                ToolbarItem::new(*icon, *label).enabled(!matches!(command, Command::Save))
            }
            None => ToolbarItem::separator(),
        })
        .collect()
}

/// メニューバーの中身。
///
/// ショートカットは**主修飾キー + 英数字 1 文字**で指定する。主修飾キーは
/// macOS だけ ⌘ で、Windows・Linux・Web では Ctrl になる。
fn menus(sections: &[&str]) -> Vec<MenuSpec> {
    let file = FILE_MENU.iter().map(|entry| match entry {
        Some((label, command)) => {
            let item = MenuItem::new(*label);
            match command {
                Command::New => item.shortcut(MenuShortcut::new('n')),
                Command::Open => item.shortcut(MenuShortcut::new('o')),
                Command::Save => item.shortcut(MenuShortcut::new('s')).enabled(false),
                Command::SaveAs => item.shortcut(MenuShortcut::new('s').shift(true)),
            }
        }
        None => MenuItem::separator(),
    });
    let view = sections
        .iter()
        .map(|section| MenuItem::new(*section))
        .chain([
            MenuItem::separator(),
            // 押せない項目は、その場ではできないことを表す。
            MenuItem::new("全画面").enabled(false),
        ]);
    vec![
        MenuSpec::new("ファイル", file),
        MenuSpec::new("表示", view),
        MenuSpec::new("ヘルプ", ["naui について"]),
    ]
}

/// 保存の項目を、ツールバーとメニューの両方で押せる・押せないにする。
fn set_save_enabled(toolbar: &Toolbar, menu_bar: &MenuBar, enabled: bool) {
    for (index, entry) in TOOLBAR.iter().enumerate() {
        if let Some((_, _, Command::Save)) = entry {
            toolbar.set_item_enabled(index, enabled);
        }
    }
    for (index, entry) in FILE_MENU.iter().enumerate() {
        if let Some((_, Command::Save)) = entry {
            menu_bar.set_item_enabled(FILE, index, enabled);
        }
    }
}

fn about_dialog(ui: &Ui) -> Result<Dialog> {
    let about = ui.dialog("naui について")?;
    about.set_message(
        "naui は、各 OS (とブラウザ) の実ウィジェットを同じ Rust の API で扱う UI ライブラリです。",
    );
    Ok(about)
}
