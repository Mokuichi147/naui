//! ウィンドウに取り付けるツールバーとメニューバー。
//!
//! どちらもレイアウトではなくウィンドウに取り付ける。ツールバーは macOS では
//! NSToolbar、Linux では AdwHeaderBar としてタイトルバーに出る。メニューバーは
//! macOS では画面上端のメニューバー (NSApplication.mainMenu)、ほかの 3 環境では
//! タイトルバーの下に敷かれる帯になる。
//!
//! 同じ操作はツールバーとメニューのどちらから選んでも同じ処理へ届け、
//! 押せる・押せないも両方で合わせる。実際のアプリと同じ作りにしておく。
//!
//! 「View」メニューには、ふつうのアプリの表示メニューにあるもの (区分を並べた
//! サブメニュー、印の付く項目、ファンクションキーで動く項目) を並べる。

use std::rc::Rc;

use naui::{
    Dialog, MenuBar, MenuItem, MenuShortcut, MenuSpec, Result, Toolbar, ToolbarIcon, ToolbarItem,
    Ui, Window, WindowState,
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

impl Command {
    /// 保存するもの (新規か開くで作ったもの) が要る操作か。
    /// 要るものは、それができるまで押せなくしておく。
    fn needs_document(self) -> bool {
        matches!(self, Command::Save | Command::SaveAs)
    }
}

/// ツールバーの並び。区切りは `None` で表す。
///
/// 通知は区切りも数えた位置で届くので、並びをそのまま引けるようにしておく。
const TOOLBAR: [Option<(ToolbarIcon, &str, Command)>; 4] = [
    Some((ToolbarIcon::New, "New", Command::New)),
    Some((ToolbarIcon::Open, "Open", Command::Open)),
    None,
    Some((ToolbarIcon::Save, "Save", Command::Save)),
];

/// 「File」メニューの並び。区切りは `None` で表す。
const FILE_MENU: [Option<(&str, Command)>; 5] = [
    Some(("New", Command::New)),
    Some(("Open", Command::Open)),
    None,
    Some(("Save", Command::Save)),
    Some(("Save As…", Command::SaveAs)),
];

/// メニューの位置。
const FILE: usize = 0;
const VIEW: usize = 1;
const HELP: usize = 2;

/// 「View」メニューの中の位置。区分は先頭のサブメニューにまとめてある。
const SECTIONS: usize = 0;
const SHOW_TOOLBAR: usize = 2;
const FULL_SCREEN: usize = 3;

/// ツールバーとメニューバーを作って `window` へ取り付ける。
///
/// 「View」メニューには `sections` の名前が並び、選ぶと `go` へその位置を渡す。
pub(crate) fn attach(
    ui: &Ui,
    window: &Window,
    sections: &[&str],
    go: Rc<dyn Fn(usize)>,
    notice: &Notice,
) -> Result<()> {
    let toolbar = ui.toolbar()?;
    let menu_bar = ui.menu_bar()?;

    // 保存できるものができるまで、保存の 2 項目はツールバーとメニューのどちらでも
    // 押せなくしておく。
    toolbar.set_items(&toolbar_items());
    menu_bar.set_menus(&menus(sections));
    let run = Rc::new({
        let toolbar = toolbar.clone();
        let menu_bar = menu_bar.clone();
        let notice = notice.clone();
        move |command: Command| match command {
            Command::New | Command::Open => {
                notice.show(if matches!(command, Command::New) {
                    "Created a new document"
                } else {
                    "Opened a document"
                });
                enable_document_commands(&toolbar, &menu_bar);
            }
            Command::Save => notice.show("Saved"),
            Command::SaveAs => notice.show("Saved as a new file"),
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
        // 通知はメニューバーが持つので、ウィンドウは弱参照で捕まえる。
        let window = window.downgrade();
        let menu_bar_handle = menu_bar.clone();
        move |path| match *path {
            [FILE, item] => {
                if let Some(Some((_, command))) = FILE_MENU.get(item) {
                    run(*command);
                }
            }
            [VIEW, SECTIONS, section] => go(section),
            // 印は通知の前に反転しているので、いまの印に合わせる。
            [VIEW, SHOW_TOOLBAR] => {
                let Some(window) = window.upgrade() else {
                    return;
                };
                if menu_bar_handle.is_item_checked(path) {
                    window.set_toolbar(&toolbar);
                } else {
                    window.clear_toolbar();
                }
            }
            [VIEW, FULL_SCREEN] => {
                if let Some(window) = window.upgrade() {
                    window.set_state(if window.state() == WindowState::Fullscreen {
                        WindowState::Normal
                    } else {
                        WindowState::Fullscreen
                    });
                }
            }
            [HELP, _] => about.open(),
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
                ToolbarItem::new(*icon, *label).enabled(!command.needs_document())
            }
            None => ToolbarItem::separator(),
        })
        .collect()
}

/// メニューバーの中身。
///
/// 文字のショートカットは**主修飾キー + 英数字 1 文字**で指定する。主修飾キーは
/// macOS だけ ⌘ で、Windows・Linux・Web では Ctrl になる。ファンクションキーは
/// 単独でも使える。
fn menus(sections: &[&str]) -> Vec<MenuSpec> {
    let file = FILE_MENU.iter().map(|entry| match entry {
        Some((label, command)) => {
            let shortcut = match command {
                Command::New => MenuShortcut::new('n'),
                Command::Open => MenuShortcut::new('o'),
                Command::Save => MenuShortcut::new('s'),
                Command::SaveAs => MenuShortcut::new('s').shift(true),
            };
            MenuItem::new(*label)
                .shortcut(shortcut)
                .enabled(!command.needs_document())
        }
        None => MenuItem::separator(),
    });
    let view = [
        MenuItem::submenu("Sections", sections.iter().copied()),
        MenuItem::separator(),
        MenuItem::new("Show Toolbar").checked(true),
        MenuItem::new("Full Screen").shortcut(MenuShortcut::function(11)),
    ];
    vec![
        MenuSpec::new("File", file),
        MenuSpec::new("View", view),
        MenuSpec::new("Help", ["About naui"]),
    ]
}

/// 保存するものが要る項目を、ツールバーとメニューの両方で押せるようにする。
fn enable_document_commands(toolbar: &Toolbar, menu_bar: &MenuBar) {
    for (index, entry) in TOOLBAR.iter().enumerate() {
        if let Some((_, _, command)) = entry {
            if command.needs_document() {
                toolbar.set_item_enabled(index, true);
            }
        }
    }
    for (index, entry) in FILE_MENU.iter().enumerate() {
        if let Some((_, command)) = entry {
            if command.needs_document() {
                menu_bar.set_item_enabled(&[FILE, index], true);
            }
        }
    }
}

fn about_dialog(ui: &Ui) -> Result<Dialog> {
    let about = ui.dialog("About naui")?;
    about.set_message(
        "naui is a UI library that drives each platform's native widgets (and the browser's) from one Rust API.",
    );
    Ok(about)
}
