//! The menu bar (`GtkPopoverMenuBar` + `GMenu`).
//!
//! GTK4 menus are made of "models (`GMenu`) and actions (`GAction`)". The menu bar
//! is the same: headings are submenus of the `GMenu`, and items are represented by
//! **one `GSimpleAction` per item** (`GMenuItem` itself has no enabled state, so
//! whether it can be chosen is held by the action).
//!
//! Shortcuts are registered as the `GtkApplication`'s accelerators. Instead of
//! naui watching pressed keys, **GTK4 invokes the action**, so they work without
//! opening the menu, and GTK4 also renders the right edge of the items. There is
//! one accelerator table for the whole app, so they are registered **only while
//! attached to a window**.
//!
//! A submenu is `g_menu_append_submenu`, and whether it can be opened is held by
//! its `submenu-action` (a boolean action GTK4 turns on while it is open). A check
//! item is an action with a boolean state, which GTK4 draws as a check mark. naui
//! flips the state before notifying, the same as the other platforms.
//!
//! The headings go in the top row of the `AdwToolbarView` (below the header bar).
//! GNOME prefers hamburger menus to menu bars, but naui offers the same API on all
//! four platforms, so it uses GTK4's real menu bar as is.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use gtk::gio;
use gtk::prelude::*;
use naui_core::{MenuHandler, MenuItem, MenuSpec};

/// The prefix of the namespace that holds the menu's actions. A number per menu
/// bar follows it.
///
/// Accelerators are registered with the `GtkApplication` as **one table for the
/// whole app**, so sharing a namespace would let a later menu bar overwrite the
/// registrations of an earlier one. With a namespace per menu bar, even when the
/// same key is used in several windows, GTK4 invokes only the action in the focused
/// window.
const GROUP_PREFIX: &str = "naui-menubar-";

thread_local! {
    /// The number of the next menu bar to create. GTK4 can only be touched on the main
    /// thread.
    static NEXT_GROUP: Cell<u64> = const { Cell::new(0) };
}

struct MenuBarInner {
    native: gtk::PopoverMenuBar,
    app: adw::Application,
    /// The namespace of this menu bar's actions (`naui-menubar-<number>`).
    group: String,
    actions: gio::SimpleActionGroup,
    menus: RefCell<Vec<MenuSpec>>,
    /// Actions looked up by path (heading index first). Separators are not in it; a
    /// submenu heading has its `submenu-action`.
    items: RefCell<HashMap<Vec<usize>, gio::SimpleAction>>,
    on_activate: MenuHandler,
    /// Whether the whole menu bar is enabled. ANDed with the per-item settings.
    enabled: Cell<bool>,
    /// The number of windows it is attached to. Accelerators are registered with the
    /// app only while this is 1 or more.
    attachments: Cell<usize>,
}

impl MenuBarInner {
    /// The action's name. Prefixed with the namespace when referenced from the `GMenu`.
    fn action_name(path: &[usize]) -> String {
        action_name("m", path)
    }

    /// The action's name with the namespace (in the form `naui-menubar-3.m0-1`).
    fn detailed_name(&self, path: &[usize]) -> String {
        format!("{}.{}", self.group, Self::action_name(path))
    }

    /// Registers the current items' shortcuts as the app's accelerators.
    fn register_accels(&self) {
        MenuSpec::walk(&self.menus.borrow(), |path, entry| {
            if entry.is_separator() || entry.is_submenu() {
                return;
            }
            if let Some(shortcut) = entry.shortcut.filter(|s| s.is_valid()) {
                // 右端の表示も、押されたキーの受け取りも GTK4 が行う。
                self.app
                    .set_accels_for_action(&self.detailed_name(path), &[&shortcut.accelerator()]);
            }
        });
    }

    /// Removes the registered accelerators one by one.
    fn clear_accels(&self) {
        MenuSpec::walk(&self.menus.borrow(), |path, entry| {
            if entry.shortcut.is_some() {
                self.app
                    .set_accels_for_action(&self.detailed_name(path), &[]);
            }
        });
    }

    /// Flips a check item, then notifies. The same path for the user's choice and
    /// `activate`.
    fn choose(&self, path: &[usize]) {
        let flipped = {
            let mut menus = self.menus.borrow_mut();
            MenuSpec::item_at_mut(&mut menus, path)
                .filter(|item| item.is_checkable())
                .map(|item| {
                    let on = !item.checked.unwrap_or(false);
                    item.checked = Some(on);
                    on
                })
        };
        if let Some(on) = flipped {
            if let Some(action) = self.items.borrow().get(path) {
                action.set_state(&on.to_variant());
            }
        }
        self.on_activate.emit(path);
    }
}

/// The name of the action for the item at `path` (in the form `m0-2-1`).
pub(crate) fn action_name(prefix: &str, path: &[usize]) -> String {
    let parts: Vec<String> = path.iter().map(usize::to_string).collect();
    format!("{prefix}{}", parts.join("-"))
}

/// The action for a submenu's `submenu-action`. GTK4 turns its boolean state on
/// while the submenu is open, and does not open it while it is disabled.
pub(crate) fn submenu_action(name: &str, enabled: bool) -> gio::SimpleAction {
    let action = gio::SimpleAction::new_stateful(name, None, &false.to_variant());
    action.set_enabled(enabled);
    action
}

/// The action for an item that can be chosen. A check item has a boolean state, which
/// GTK4 draws as a check mark.
pub(crate) fn item_action(name: &str, checked: Option<bool>, enabled: bool) -> gio::SimpleAction {
    let action = match checked {
        Some(on) => gio::SimpleAction::new_stateful(name, None, &on.to_variant()),
        None => gio::SimpleAction::new(name, None),
    };
    action.set_enabled(enabled);
    action
}

/// A submenu item whose `submenu-action` is `action` (with the namespace).
pub(crate) fn submenu_item(label: &str, model: &gio::Menu, action: &str) -> gio::MenuItem {
    let item = gio::MenuItem::new_submenu(Some(label), model);
    item.set_attribute_value("submenu-action", Some(&action.to_variant()));
    item
}

/// The OS's application menu, attached to the top of the window.
///
/// Not a [`Widget`](crate::Widget). Attach it with
/// [`Window::set_menu_bar`](crate::Window::set_menu_bar). Items are identified by
/// **a path** (the heading index, then the item index at each level of submenus,
/// counting separators). Every time an item is clicked,
/// [`on_activate`](Self::on_activate) is called with its path. Separators and
/// submenu headings are never returned.
#[derive(Clone)]
pub struct MenuBar(Rc<MenuBarInner>);

impl MenuBar {
    pub(crate) fn new(app: &adw::Application) -> Self {
        let native = gtk::PopoverMenuBar::from_model(None::<&gio::Menu>);
        let actions = gio::SimpleActionGroup::new();
        let group = NEXT_GROUP.with(|next| {
            let id = next.get();
            next.set(id + 1);
            format!("{GROUP_PREFIX}{id}")
        });
        // 取り付ける前でもメニューから操作を引けるようにしておく。
        // 取り付け先のウィンドウへも同じ組を入れる (アクセラレータの解決は
        // フォーカスのあるウィジェットからたどるため)。
        native.insert_action_group(&group, Some(&actions));
        Self(Rc::new(MenuBarInner {
            native,
            app: app.clone(),
            group,
            actions,
            menus: RefCell::new(Vec::new()),
            items: RefCell::new(HashMap::new()),
            on_activate: MenuHandler::default(),
            enabled: Cell::new(true),
            attachments: Cell::new(0),
        }))
    }

    /// Rebuilds the menus. The previous menus are removed.
    pub fn set_menus(&self, menus: &[MenuSpec]) {
        self.0.clear_accels();
        for name in self.0.actions.list_actions() {
            self.0.actions.remove_action(&name);
        }

        let whole = self.0.enabled.get();
        let model = gio::Menu::new();
        let mut built = HashMap::new();
        for (menu_index, spec) in menus.iter().enumerate() {
            let submenu = self.build_menu(&spec.items, &mut vec![menu_index], whole, &mut built);
            model.append_submenu(Some(&spec.title), &submenu);
        }

        self.0.native.set_menu_model(Some(&model));
        *self.0.items.borrow_mut() = built;
        let mut stored = self.0.menus.borrow_mut();
        stored.clear();
        stored.extend_from_slice(menus);
        drop(stored);
        // ショートカットは、ウィンドウへ取り付けている間だけ登録する。
        if self.0.attachments.get() > 0 {
            self.0.register_accels();
        }
    }

    /// Builds the `GMenu` for `items`, recursing into submenus. `enabled` is whether
    /// everything leading to it is enabled.
    fn build_menu(
        &self,
        items: &[MenuItem],
        path: &mut Vec<usize>,
        enabled: bool,
        built: &mut HashMap<Vec<usize>, gio::SimpleAction>,
    ) -> gio::Menu {
        let menu = gio::Menu::new();
        // 区切り線は「ここで節を切る」ことで表す。
        let mut section = gio::Menu::new();
        for (index, item) in items.iter().enumerate() {
            if item.is_separator() {
                if section.n_items() > 0 {
                    menu.append_section(None, &section);
                    section = gio::Menu::new();
                }
                continue;
            }
            path.push(index);
            let name = MenuBarInner::action_name(path);
            let detailed = self.0.detailed_name(path);
            let here = item.enabled && enabled;
            if let Some(children) = &item.submenu {
                let action = submenu_action(&name, here);
                self.0.actions.add_action(&action);
                let model = self.build_menu(children, path, here, built);
                section.append_item(&submenu_item(&item.label, &model, &detailed));
                built.insert(path.clone(), action);
            } else {
                let action = item_action(&name, item.checked, here);
                // ハンドルを強く持つとシグナルとの間で循環するため、弱参照にする。
                let weak = Rc::downgrade(&self.0);
                let at = path.clone();
                action.connect_activate(move |_, _| {
                    if let Some(inner) = weak.upgrade() {
                        inner.choose(&at);
                    }
                });
                self.0.actions.add_action(&action);
                section.append(Some(&item.label), Some(&detailed));
                built.insert(path.clone(), action);
            }
            path.pop();
        }
        if section.n_items() > 0 {
            menu.append_section(None, &section);
        }
        menu
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

    /// Enables or disables the item at `path`. Does nothing for separators or out of
    /// range.
    pub fn set_item_enabled(&self, path: &[usize], enabled: bool) {
        let mut menus = self.0.menus.borrow_mut();
        let Some(entry) = MenuSpec::item_at_mut(&mut menus, path) else {
            return;
        };
        if entry.is_separator() {
            return;
        }
        entry.enabled = enabled;
        drop(menus);
        self.apply_enabled();
    }

    /// Whether the item at `path` can be clicked now. `false` for separators, submenu
    /// headings and out of range.
    pub fn is_item_enabled(&self, path: &[usize]) -> bool {
        self.0.enabled.get() && MenuSpec::can_choose(&self.0.menus.borrow(), path)
    }

    /// Turns the check mark of the item at `path` on or off, without notifying. Does
    /// nothing for items without a check mark.
    pub fn set_item_checked(&self, path: &[usize], checked: bool) {
        let mut menus = self.0.menus.borrow_mut();
        let Some(entry) = MenuSpec::item_at_mut(&mut menus, path) else {
            return;
        };
        if !entry.is_checkable() {
            return;
        }
        entry.checked = Some(checked);
        drop(menus);
        if let Some(action) = self.0.items.borrow().get(path) {
            action.set_state(&checked.to_variant());
        }
    }

    /// Whether the check mark of the item at `path` is on.
    pub fn is_item_checked(&self, path: &[usize]) -> bool {
        MenuSpec::item_at(&self.0.menus.borrow(), path)
            .is_some_and(|entry| entry.is_checkable() && entry.checked == Some(true))
    }

    /// Enables or disables the whole menu bar. Per-item settings are kept.
    pub fn set_enabled(&self, enabled: bool) {
        self.0.enabled.set(enabled);
        self.apply_enabled();
    }

    /// Applies the per-item settings and the overall setting to the native side.
    ///
    /// Making headings unclickable only needs `GtkWidget`'s sensitive, but accelerators
    /// invoke actions directly, so the actions are disabled too, including those
    /// inside a disabled submenu.
    fn apply_enabled(&self) {
        let whole = self.0.enabled.get();
        self.0.native.set_sensitive(whole);
        let menus = self.0.menus.borrow();
        let items = self.0.items.borrow();
        MenuSpec::walk(&menus, |path, _| {
            if let Some(action) = items.get(path) {
                action.set_enabled(whole && MenuSpec::enabled_at(&menus, path));
            }
        });
    }

    /// Runs the item at `path` as if the user had chosen it.
    ///
    /// Does nothing for separators, submenu headings, disabled items or out of range.
    pub fn activate(&self, path: &[usize]) {
        if self.is_item_enabled(path) {
            self.0.choose(path);
        }
    }

    /// Called with the item's path when an item is clicked. Setting it again removes
    /// the previous callback.
    pub fn on_activate(&self, f: impl FnMut(&[usize]) + 'static) {
        self.0.on_activate.set(f);
    }

    /// The corresponding `GtkPopoverMenuBar`. Public as a backend-specific escape
    /// hatch.
    pub fn native_menu_bar(&self) -> gtk::PopoverMenuBar {
        self.0.native.clone()
    }

    /// The `GSimpleAction` for the item at `path` (for a submenu heading, its
    /// `submenu-action`). `None` for separators and out of range. Public as a
    /// backend-specific escape hatch.
    pub fn native_action(&self, path: &[usize]) -> Option<gio::SimpleAction> {
        self.0.items.borrow().get(path).cloned()
    }

    /// Returns the item's action as a name with the namespace (in the form
    /// `naui-menubar-3.m0-1`). `None` for separators and out of range.
    ///
    /// It is the name passed to `gtk_widget_activate_action` or
    /// `gtk_application_set_accels_for_action`, and is public as a backend-specific
    /// escape hatch.
    pub fn native_action_name(&self, path: &[usize]) -> Option<String> {
        self.native_action(path)?;
        Some(self.0.detailed_name(path))
    }

    /// The widget inserted into the window's top row. Used only by [`crate::Window`].
    pub(crate) fn mount(&self) -> gtk::PopoverMenuBar {
        self.0.native.clone()
    }

    /// The action group added to the window it is attached to. Used only by
    /// [`crate::Window`].
    pub(crate) fn action_group(&self) -> gio::SimpleActionGroup {
        self.0.actions.clone()
    }

    /// Called when attached to a window. Used only by [`crate::Window`].
    ///
    /// On the first attachment, the shortcuts are registered as the app's
    /// accelerators.
    pub(crate) fn attach(&self) {
        let count = self.0.attachments.get();
        self.0.attachments.set(count + 1);
        if count == 0 {
            self.0.register_accels();
        }
    }

    /// Called when detached from a window. Used only by [`crate::Window`].
    ///
    /// Once it is attached to no window, the accelerator registrations are removed too,
    /// so that a detached menu bar's shortcuts do not linger in the app's table.
    pub(crate) fn detach(&self) {
        let count = self.0.attachments.get();
        if count == 0 {
            return;
        }
        self.0.attachments.set(count - 1);
        if count == 1 {
            self.0.clear_accels();
        }
    }

    /// The action namespace. Used only by [`crate::Window`].
    pub(crate) fn group(&self) -> String {
        self.0.group.clone()
    }
}

impl Drop for MenuBarInner {
    fn drop(&mut self) {
        // アプリへ登録したアクセラレータは、メニューバーが無くなっても残る。
        self.clear_accels();
    }
}
