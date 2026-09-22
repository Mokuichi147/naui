//! メニューバー (画面上端のアプリケーションメニュー、AppKit の `NSMenu`)。
//!
//! macOS のメニューバーは**ウィンドウではなくアプリが持つ**
//! (`NSApplication.mainMenu`)。そのため [`MenuBar`] は
//! [`Widget`](crate::Widget) ではなく、
//! [`Window::set_menu_bar`](crate::Window::set_menu_bar) で取り付ける
//! ([`Toolbar`](crate::Toolbar) と同じ形にそろえてある)。見た目・開閉・
//! キーボード操作・ショートカットの配送はすべて AppKit が行う。
//!
//! ## 標準のメニュー
//!
//! ⌘V などの編集ショートカットは**メインメニューのキー等価**として配送される。
//! `NSApplication` にメニューが無いと、`NSTextField` にフォーカスがあっても
//! ⌘C / ⌘V / ⌘A が何も起こさない。そこで naui は
//!
//! - アプリ名のメニュー (先頭。macOS はここを必ずアプリメニューとして扱う)
//! - 「編集」メニュー (末尾)
//!
//! を自分で用意し、アプリが渡したメニューをその間へ並べる。項目のターゲットは
//! nil にしてあるので、AppKit がレスポンダチェーンをたどって、いま編集中の
//! コントロールへ `paste:` などを届ける。**コピーや貼り付けを実装しているのは
//! AppKit 自身**で、naui は何もしない。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use naui_core::MenuSpec;
use objc2::rc::Retained;
use objc2::runtime::Sel;
use objc2::{sel, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSApplication, NSEventModifierFlags, NSMenu, NSMenuItem};
use objc2_foundation::NSString;

use crate::trampoline::ActionTarget;

/// メインメニューをまだ持っていなければ用意する。
///
/// 何度呼んでも 1 度しか組み立てない。アプリが自分でメニューを作っている
/// 場合 (ネイティブへの脱出口を使った場合) は、それを尊重して何もしない。
pub(crate) fn install(mtm: MainThreadMarker, app_name: &str) {
    let app = NSApplication::sharedApplication(mtm);
    if app.mainMenu().is_some() {
        return;
    }
    app.setMainMenu(Some(&standard_menu(mtm, app_name)));
}

/// アプリメニューと編集メニューだけを持つ、naui の既定のメインメニュー。
fn standard_menu(mtm: MainThreadMarker, app_name: &str) -> Retained<NSMenu> {
    let main = NSMenu::new(mtm);
    main.addItem(&app_menu(mtm, app_name));
    main.addItem(&edit_menu(mtm));
    main
}

/// アプリ名のメニュー。macOS はメインメニューの先頭をアプリメニューとして扱う。
fn app_menu(mtm: MainThreadMarker, app_name: &str) -> Retained<NSMenuItem> {
    submenu(
        mtm,
        app_name,
        &[(&format!("{app_name} を終了"), sel!(terminate:), "q")],
    )
}

/// 標準の編集メニュー。⌘C / ⌘V などの配送経路を用意するためのもの。
fn edit_menu(mtm: MainThreadMarker) -> Retained<NSMenuItem> {
    // 大文字の "Z" は ⇧⌘Z (シフトを含む) を意味する。AppKit の決まり。
    submenu(
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
    )
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

/// 押された項目の通知先。
///
/// 呼び出しの間だけクロージャを取り出すので、通知の中から同じメニューバーを
/// 組み替えても二重借用にならない (トランポリンの `SelectHandler` と同じ形を、
/// 2 つの値で行う)。
#[derive(Clone, Default)]
struct ActivateHandler(Rc<RefCell<Option<Box<dyn FnMut(usize, usize)>>>>);

impl ActivateHandler {
    fn set(&self, f: impl FnMut(usize, usize) + 'static) {
        *self.0.borrow_mut() = Some(Box::new(f));
    }

    fn emit(&self, menu: usize, item: usize) {
        let Some(mut f) = self.0.borrow_mut().take() else {
            return;
        };
        f(menu, item);
        let mut slot = self.0.borrow_mut();
        // 呼び出しの中で差し替えられていたら、新しいほうを残す。
        if slot.is_none() {
            *slot = Some(f);
        }
    }
}

struct MenuBarInner {
    mtm: MainThreadMarker,
    /// `NSApplication.mainMenu` へ渡すメニュー。
    /// 先頭はアプリメニュー、末尾は標準の編集メニュー。
    native: Retained<NSMenu>,
    app_name: String,
    menus: RefCell<Vec<MenuSpec>>,
    /// 見出しごとの `NSMenuItem` (submenu の持ち手)。
    holders: RefCell<Vec<Retained<NSMenuItem>>>,
    /// 見出しと項目のインデックスから引ける `NSMenuItem`。区切り線は `None`。
    items: RefCell<Vec<Vec<Option<Retained<NSMenuItem>>>>>,
    /// 押されたときのトランポリン。`NSMenuItem` の target は weak なので保持する。
    targets: RefCell<Vec<Retained<ActionTarget>>>,
    handler: ActivateHandler,
    /// メニューバー全体の有効・無効。項目ごとの指定と AND を取る。
    enabled: Cell<bool>,
}

/// 画面上端に出る、OS のアプリケーションメニュー (`NSMenu`)。
///
/// [`Widget`](crate::Widget) ではない。
/// [`Window::set_menu_bar`](crate::Window::set_menu_bar) で取り付ける。
/// 項目が押されるたびに、その **(見出しのインデックス, 項目のインデックス)**
/// で [`on_activate`](Self::on_activate) が呼ばれる。項目のインデックスは
/// 区切り線を含めた並びの位置で、区切り線が返ることはない。
///
/// macOS のメニューバーはアプリに 1 つなので、取り付けはウィンドウごとでは
/// なく**アプリ全体**に効く。
#[derive(Clone)]
pub struct MenuBar(Rc<MenuBarInner>);

impl MenuBar {
    pub(crate) fn new(mtm: MainThreadMarker, app_name: &str) -> Self {
        let native = NSMenu::new(mtm);
        // 見出しの有効・無効はアプリの指定をそのまま使う。既定では AppKit が
        // レスポンダチェーンを見て決めてしまう。
        native.setAutoenablesItems(false);
        let this = Self(Rc::new(MenuBarInner {
            mtm,
            native,
            app_name: app_name.to_string(),
            menus: RefCell::new(Vec::new()),
            holders: RefCell::new(Vec::new()),
            items: RefCell::new(Vec::new()),
            targets: RefCell::new(Vec::new()),
            handler: ActivateHandler::default(),
            enabled: Cell::new(true),
        }));
        this.rebuild();
        this
    }

    /// メニューを作り直す。以前のメニューは取り除かれる。
    ///
    /// 見出しのインデックスは渡した並びの位置、項目のインデックスは
    /// 区切り線を含めた並びの位置。
    pub fn set_menus(&self, menus: &[MenuSpec]) {
        let mut stored = self.0.menus.borrow_mut();
        stored.clear();
        stored.extend_from_slice(menus);
        drop(stored);
        self.rebuild();
    }

    /// `NSMenu` を組み直す。並びは アプリメニュー → アプリの見出し → 編集。
    fn rebuild(&self) {
        let mtm = self.0.mtm;
        let whole = self.0.enabled.get();
        self.0.native.removeAllItems();
        self.0.targets.borrow_mut().clear();

        self.0.native.addItem(&app_menu(mtm, &self.0.app_name));

        let menus = self.0.menus.borrow();
        let mut holders = Vec::with_capacity(menus.len());
        let mut built = Vec::with_capacity(menus.len());
        for (menu_index, spec) in menus.iter().enumerate() {
            let holder = NSMenuItem::new(mtm);
            holder.setTitle(&NSString::from_str(&spec.title));
            holder.setEnabled(whole);
            let menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str(&spec.title));
            // 項目の有効・無効もアプリの指定をそのまま使う。
            menu.setAutoenablesItems(false);

            let mut items = Vec::with_capacity(spec.items.len());
            for (item_index, entry) in spec.items.iter().enumerate() {
                if entry.is_separator() {
                    menu.addItem(&NSMenuItem::separatorItem(mtm));
                    items.push(None);
                    continue;
                }

                let native = NSMenuItem::new(mtm);
                native.setTitle(&NSString::from_str(&entry.label));
                native.setEnabled(entry.enabled && whole);
                if let Some(shortcut) = entry.shortcut {
                    // ⌘ は主修飾キー。⇧ と ⌥ は指定があるときだけ足す。
                    let mut mask = NSEventModifierFlags::Command;
                    if shortcut.shift {
                        mask |= NSEventModifierFlags::Shift;
                    }
                    if shortcut.alt {
                        mask |= NSEventModifierFlags::Option;
                    }
                    native.setKeyEquivalent(&NSString::from_str(&shortcut.key_equivalent()));
                    native.setKeyEquivalentModifierMask(mask);
                }

                // ハンドルを強く持つとトランポリンとの間で循環するため、
                // 弱参照にする。
                let target = ActionTarget::new(mtm, {
                    let weak = Rc::downgrade(&self.0);
                    move || {
                        if let Some(inner) = weak.upgrade() {
                            inner.handler.emit(menu_index, item_index);
                        }
                    }
                });
                unsafe {
                    native.setTarget(Some(&target));
                    native.setAction(Some(sel!(invoke:)));
                }
                self.0.targets.borrow_mut().push(target);

                menu.addItem(&native);
                items.push(Some(native));
            }

            holder.setSubmenu(Some(&menu));
            self.0.native.addItem(&holder);
            holders.push(holder);
            built.push(items);
        }
        drop(menus);

        // 編集メニューは末尾。⌘C / ⌘V の配送経路として必ず残す。
        self.0.native.addItem(&edit_menu(mtm));

        *self.0.holders.borrow_mut() = holders;
        *self.0.items.borrow_mut() = built;
        // すでに取り付けてあるなら、組み直した並びを映す。
        if self.is_installed() {
            self.install();
        }
    }

    /// 見出しの数。
    pub fn len(&self) -> usize {
        self.0.menus.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// 見出し 1 つが持つ、区切り線を含めた項目数。範囲外は `0`。
    pub fn menu_len(&self, menu: usize) -> usize {
        self.0
            .menus
            .borrow()
            .get(menu)
            .map_or(0, |spec| spec.items.len())
    }

    /// 項目 1 つの有効・無効を変える。区切り線と範囲外は何もしない。
    pub fn set_item_enabled(&self, menu: usize, item: usize, enabled: bool) {
        let mut menus = self.0.menus.borrow_mut();
        let Some(entry) = menus
            .get_mut(menu)
            .and_then(|spec| spec.items.get_mut(item))
        else {
            return;
        };
        if entry.is_separator() {
            return;
        }
        entry.enabled = enabled;
        drop(menus);
        self.apply_enabled();
    }

    /// いま押せる項目か。区切り線と範囲外は `false`。
    pub fn is_item_enabled(&self, menu: usize, item: usize) -> bool {
        self.0.enabled.get()
            && self
                .0
                .menus
                .borrow()
                .get(menu)
                .and_then(|spec| spec.items.get(item))
                .is_some_and(|entry| !entry.is_separator() && entry.enabled)
    }

    /// メニューバー全体の有効・無効を変える。項目ごとの指定は残る。
    ///
    /// naui が用意するアプリメニューと編集メニューは対象にしない
    /// (⌘Q と ⌘V を取り上げないため)。
    pub fn set_enabled(&self, enabled: bool) {
        self.0.enabled.set(enabled);
        self.apply_enabled();
    }

    /// 項目ごとの指定と全体の指定をネイティブへ反映する。
    fn apply_enabled(&self) {
        let whole = self.0.enabled.get();
        let menus = self.0.menus.borrow();
        let holders = self.0.holders.borrow();
        for (index, (spec, items)) in menus.iter().zip(self.0.items.borrow().iter()).enumerate() {
            if let Some(holder) = holders.get(index) {
                holder.setEnabled(whole);
            }
            for (entry, native) in spec.items.iter().zip(items.iter()) {
                if let Some(native) = native {
                    native.setEnabled(entry.enabled && whole);
                }
            }
        }
    }

    /// 利用者が選んだのと同じように項目を実行する。
    ///
    /// 区切り線・押せない項目・範囲外は何もしない。
    pub fn activate(&self, menu: usize, item: usize) {
        if self.is_item_enabled(menu, item) {
            self.0.handler.emit(menu, item);
        }
    }

    /// 項目が押されたときに、その見出しと項目のインデックスで呼ばれる。
    /// 設定し直すと以前のコールバックは外れる。
    pub fn on_activate(&self, f: impl FnMut(usize, usize) + 'static) {
        self.0.handler.set(f);
    }

    /// AppKit の実メニュー (`NSApplication.mainMenu` へ渡すもの)。
    /// バックエンド固有の脱出口として公開している。
    pub fn native_menu(&self) -> Retained<NSMenu> {
        self.0.native.clone()
    }

    /// 項目に対応する `NSMenuItem`。区切り線と範囲外は `None`。
    /// バックエンド固有の脱出口として公開している。
    pub fn native_item(&self, menu: usize, item: usize) -> Option<Retained<NSMenuItem>> {
        self.0.items.borrow().get(menu)?.get(item)?.clone()
    }

    /// アプリのメインメニューとして取り付ける。[`crate::Window`] だけが使う。
    pub(crate) fn install(&self) {
        NSApplication::sharedApplication(self.0.mtm).setMainMenu(Some(&self.0.native));
    }

    /// いまアプリのメインメニューになっているか。
    ///
    /// 同じ物かどうかを見るので、`isEqual:` ではなくポインタで突き合わせる。
    fn is_installed(&self) -> bool {
        let mine = Retained::as_ptr(&self.0.native);
        NSApplication::sharedApplication(self.0.mtm)
            .mainMenu()
            .is_some_and(|current| Retained::as_ptr(&current) == mine)
    }

    /// naui の既定のメニューへ戻す。[`crate::Window`] だけが使う。
    ///
    /// 何も付けないのではなく既定へ戻すのは、⌘C / ⌘V の配送経路を
    /// 絶やさないため。
    pub(crate) fn uninstall(&self) {
        if !self.is_installed() {
            return;
        }
        let standard = standard_menu(self.0.mtm, &self.0.app_name);
        NSApplication::sharedApplication(self.0.mtm).setMainMenu(Some(&standard));
    }
}
