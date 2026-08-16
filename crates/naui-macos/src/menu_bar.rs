//! アプリのメインメニュー (画面上端のメニューバー)。
//!
//! macOS では、⌘V などの編集ショートカットは**メインメニューのキー等価**として
//! 配送される。`NSApplication` にメニューが無いと、`NSTextField` に
//! フォーカスがあっても ⌘C / ⌘V / ⌘A が何も起こさない。
//!
//! ここで作るのは、その配送経路を用意するためだけの最小限のメニュー。
//! 項目のターゲットは nil にしてあるので、AppKit がレスポンダチェーンを
//! たどって、いま編集中のコントロールへ `paste:` などを届ける。
//! **コピーや貼り付けを実装しているのは AppKit 自身**で、naui は何もしない。

use objc2::rc::Retained;
use objc2::runtime::Sel;
use objc2::{sel, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSApplication, NSMenu, NSMenuItem};
use objc2_foundation::NSString;

/// メインメニューをまだ持っていなければ用意する。
///
/// 何度呼んでも 1 度しか組み立てない。アプリが自分でメニューを作っている
/// 場合 (ネイティブへの脱出口を使った場合) は、それを尊重して何もしない。
pub(crate) fn install(mtm: MainThreadMarker, app_name: &str) {
    let app = NSApplication::sharedApplication(mtm);
    if app.mainMenu().is_some() {
        return;
    }

    let main = NSMenu::new(mtm);
    main.addItem(&submenu(
        mtm,
        app_name,
        &[(&format!("{app_name} を終了"), sel!(terminate:), "q")],
    ));
    // 大文字の "Z" は ⇧⌘Z (シフトを含む) を意味する。AppKit の決まり。
    main.addItem(&submenu(
        mtm,
        "編集",
        &[
            ("取り消す", sel!(undo:), "z"),
            ("やり直す", sel!(redo:), "Z"),
            ("カット", sel!(cut:), "x"),
            ("コピー", sel!(copy:), "c"),
            ("ペースト", sel!(paste:), "v"),
            ("すべてを選択", sel!(selectAll:), "a"),
        ],
    ));
    app.setMainMenu(Some(&main));
}

/// 見出しと項目からサブメニューを 1 つ作る。
fn submenu(
    mtm: MainThreadMarker,
    title: &str,
    items: &[(&str, Sel, &str)],
) -> Retained<NSMenuItem> {
    let holder = NSMenuItem::new(mtm);
    let menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str(title));
    for (label, action, key) in items {
        // ターゲットを指定しないと、AppKit がレスポンダチェーンをたどって
        // 「いまその操作ができる」オブジェクトへ送ってくれる。
        unsafe {
            menu.addItemWithTitle_action_keyEquivalent(
                &NSString::from_str(label),
                Some(*action),
                &NSString::from_str(key),
            )
        };
    }
    holder.setSubmenu(Some(&menu));
    holder
}
