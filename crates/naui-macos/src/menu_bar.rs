//! The menu bar (the application menu at the top of the screen, AppKit's
//! `NSMenu`).
//!
//! On macOS the menu bar **belongs to the app, not a window**
//! (`NSApplication.mainMenu`). So [`MenuBar`] is not a [`Widget`](crate::Widget);
//! it is attached with [`Window::set_menu_bar`](crate::Window::set_menu_bar) (the
//! same shape as [`Toolbar`](crate::Toolbar)). Its look, opening and closing,
//! keyboard navigation and shortcut dispatch are all done by AppKit.
//!
//! ## Standard menus
//!
//! Editing shortcuts such as ⌘V are dispatched as **key equivalents of the main
//! menu**. If `NSApplication` has no menu, ⌘C / ⌘V / ⌘A do nothing even when an
//! `NSTextField` has focus. So naui provides
//!
//! - the app-name menu (first; macOS always treats it as the app menu)
//! - the "Edit" menu (last)
//!
//! itself, and places the menus the app passes in between. Their titles follow the
//! OS's preferred languages ([`naui_core::ui_strings`]). The items' target is nil,
//! so AppKit walks the responder chain and delivers `paste:` and the like to the
//! control being edited. **Copy and paste are implemented by AppKit itself**;
//! naui does nothing.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use naui_core::{ui_strings, MenuSpec};
use objc2::rc::Retained;
use objc2::runtime::Sel;
use objc2::{sel, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSApplication, NSEventModifierFlags, NSMenu, NSMenuItem};
use objc2_foundation::NSString;

use crate::trampoline::ActionTarget;

/// Provides a main menu if there is none yet.
///
/// However many times it is called, it is built only once. If the app builds its
/// own menu (using the native escape hatches), that is respected and nothing is
/// done.
pub(crate) fn install(mtm: MainThreadMarker, app_name: &str) {
    let app = NSApplication::sharedApplication(mtm);
    if app.mainMenu().is_some() {
        return;
    }
    app.setMainMenu(Some(&standard_menu(mtm, app_name)));
}

/// naui's default main menu, with only the app menu and the Edit menu.
fn standard_menu(mtm: MainThreadMarker, app_name: &str) -> Retained<NSMenu> {
    let main = NSMenu::new(mtm);
    main.addItem(&app_menu(mtm, app_name));
    main.addItem(&edit_menu(mtm));
    main
}

/// The app-name menu. macOS treats the first item of the main menu as the app menu.
fn app_menu(mtm: MainThreadMarker, app_name: &str) -> Retained<NSMenuItem> {
    submenu(
        mtm,
        app_name,
        &[(&ui_strings().quit(app_name), sel!(terminate:), "q")],
    )
}

/// The standard Edit menu. Provides the dispatch path for ⌘C / ⌘V and the like.
fn edit_menu(mtm: MainThreadMarker) -> Retained<NSMenuItem> {
    let text = ui_strings();
    // 大文字の "Z" は ⇧⌘Z (シフトを含む) を意味する。AppKit の決まり。
    submenu(
        mtm,
        text.edit_menu,
        &[
            (text.undo, sel!(undo:), "z"),
            (text.redo, sel!(redo:), "Z"),
            (text.cut, sel!(cut:), "x"),
            (text.copy, sel!(copy:), "c"),
            (text.paste, sel!(paste:), "v"),
            (text.select_all, sel!(selectAll:), "a"),
        ],
    )
}

/// Creates one submenu from a heading and items.
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

/// The handler for the clicked item.
///
/// The closure is taken out only for the duration of the call, so rebuilding the
/// same menu bar from inside the notification does not double-borrow (the same
/// shape as the trampoline's `SelectHandler`, with two values).
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
    /// The menu passed to `NSApplication.mainMenu`. The app menu comes first and the
    /// standard Edit menu last.
    native: Retained<NSMenu>,
    app_name: String,
    menus: RefCell<Vec<MenuSpec>>,
    /// The `NSMenuItem` per heading (the holder of the submenu).
    holders: RefCell<Vec<Retained<NSMenuItem>>>,
    /// The `NSMenuItem`s looked up by heading and item index. `None` for separators.
    items: RefCell<Vec<Vec<Option<Retained<NSMenuItem>>>>>,
    /// The trampolines for clicks. `NSMenuItem` targets are weak, so they are held.
    targets: RefCell<Vec<Retained<ActionTarget>>>,
    handler: ActivateHandler,
    /// Whether the whole menu bar is enabled. ANDed with the per-item settings.
    enabled: Cell<bool>,
}

/// The OS's application menu shown at the top of the screen (`NSMenu`).
///
/// Not a [`Widget`](crate::Widget). Attach it with
/// [`Window::set_menu_bar`](crate::Window::set_menu_bar). Every time an item is
/// clicked, [`on_activate`](Self::on_activate) is called with its **(heading index,
/// item index)**. The item index is the position in the sequence including
/// separators; separators are never returned.
///
/// macOS has one menu bar per app, so attaching it applies to **the whole app**,
/// not per window.
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

    /// Rebuilds the menus. The previous menus are removed.
    ///
    /// The heading index is the position in the sequence passed, and the item index is
    /// the position in the sequence including separators.
    pub fn set_menus(&self, menus: &[MenuSpec]) {
        let mut stored = self.0.menus.borrow_mut();
        stored.clear();
        stored.extend_from_slice(menus);
        drop(stored);
        self.rebuild();
    }

    /// Rebuilds the `NSMenu`. The order is app menu → the app's headings → Edit.
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

    /// The number of headings.
    pub fn len(&self) -> usize {
        self.0.menus.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The number of items under one heading, separators included. `0` if out of
    /// range.
    pub fn menu_len(&self, menu: usize) -> usize {
        self.0
            .menus
            .borrow()
            .get(menu)
            .map_or(0, |spec| spec.items.len())
    }

    /// Enables or disables one item. Does nothing for separators or out of range.
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

    /// Whether the item can be clicked now. `false` for separators and out of range.
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

    /// Enables or disables the whole menu bar. Per-item settings are kept.
    ///
    /// The app menu and Edit menu that naui provides are not affected (so that ⌘Q and
    /// ⌘V are not taken away).
    pub fn set_enabled(&self, enabled: bool) {
        self.0.enabled.set(enabled);
        self.apply_enabled();
    }

    /// Applies the per-item settings and the overall setting to the native side.
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

    /// Runs the item as if the user had chosen it.
    ///
    /// Does nothing for separators, disabled items or out of range.
    pub fn activate(&self, menu: usize, item: usize) {
        if self.is_item_enabled(menu, item) {
            self.0.handler.emit(menu, item);
        }
    }

    /// Called with the heading and item indices when an item is clicked. Setting it
    /// again removes the previous callback.
    pub fn on_activate(&self, f: impl FnMut(usize, usize) + 'static) {
        self.0.handler.set(f);
    }

    /// The real AppKit menu (the one passed to `NSApplication.mainMenu`). Public as a
    /// backend-specific escape hatch.
    pub fn native_menu(&self) -> Retained<NSMenu> {
        self.0.native.clone()
    }

    /// The `NSMenuItem` for the item. `None` for separators and out of range. Public as
    /// a backend-specific escape hatch.
    pub fn native_item(&self, menu: usize, item: usize) -> Option<Retained<NSMenuItem>> {
        self.0.items.borrow().get(menu)?.get(item)?.clone()
    }

    /// Installs it as the app's main menu. Used only by [`crate::Window`].
    pub(crate) fn install(&self) {
        NSApplication::sharedApplication(self.0.mtm).setMainMenu(Some(&self.0.native));
    }

    /// Whether it is the app's main menu now.
    ///
    /// This checks for the same object, so it compares pointers rather than using
    /// `isEqual:`.
    fn is_installed(&self) -> bool {
        let mine = Retained::as_ptr(&self.0.native);
        NSApplication::sharedApplication(self.0.mtm)
            .mainMenu()
            .is_some_and(|current| Retained::as_ptr(&current) == mine)
    }

    /// Goes back to naui's default menu. Used only by [`crate::Window`].
    ///
    /// It returns to the default rather than to no menu so that the dispatch path for
    /// ⌘C / ⌘V never disappears.
    pub(crate) fn uninstall(&self) {
        if !self.is_installed() {
            return;
        }
        let standard = standard_menu(self.0.mtm, &self.0.app_name);
        NSApplication::sharedApplication(self.0.mtm).setMainMenu(Some(&standard));
    }
}
