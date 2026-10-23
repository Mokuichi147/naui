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
//! The headings go in the top row of the `AdwToolbarView` (below the header bar).
//! GNOME prefers hamburger menus to menu bars, but naui offers the same API on all
//! four platforms, so it uses GTK4's real menu bar as is.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::gio;
use gtk::prelude::*;
use naui_core::MenuSpec;

use crate::callback::ActivateNotifier;

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
    /// Actions looked up by heading and item index. `None` at separators.
    items: RefCell<Vec<Vec<Option<gio::SimpleAction>>>>,
    on_activate: ActivateNotifier,
    /// Whether the whole menu bar is enabled. ANDed with the per-item settings.
    enabled: Cell<bool>,
    /// The number of windows it is attached to. Accelerators are registered with the
    /// app only while this is 1 or more.
    attachments: Cell<usize>,
}

impl MenuBarInner {
    /// The action's name. Prefixed with the namespace when referenced from the `GMenu`.
    fn action_name(menu: usize, item: usize) -> String {
        format!("m{menu}i{item}")
    }

    /// The action's name with the namespace (in the form `naui-menubar-3.m0i1`).
    fn detailed_name(&self, menu: usize, item: usize) -> String {
        format!("{}.{}", self.group, Self::action_name(menu, item))
    }

    /// Registers the current items' shortcuts as the app's accelerators.
    fn register_accels(&self) {
        for (menu, spec) in self.menus.borrow().iter().enumerate() {
            for (item, entry) in spec.items.iter().enumerate() {
                if entry.is_separator() {
                    continue;
                }
                if let Some(shortcut) = entry.shortcut {
                    // 右端の表示も、押されたキーの受け取りも GTK4 が行う。
                    self.app.set_accels_for_action(
                        &self.detailed_name(menu, item),
                        &[&shortcut.accelerator()],
                    );
                }
            }
        }
    }

    /// Removes the registered accelerators one by one.
    fn clear_accels(&self) {
        for (menu, spec) in self.menus.borrow().iter().enumerate() {
            for (item, entry) in spec.items.iter().enumerate() {
                if entry.shortcut.is_some() {
                    self.app
                        .set_accels_for_action(&self.detailed_name(menu, item), &[]);
                }
            }
        }
    }
}

/// The OS's application menu, attached to the top of the window.
///
/// Not a [`Widget`](crate::Widget). Attach it with
/// [`Window::set_menu_bar`](crate::Window::set_menu_bar). Every time an item is
/// clicked, [`on_activate`](Self::on_activate) is called with its **(heading index,
/// item index)**. The item index is the position in the sequence including
/// separators; separators are never returned.
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
            items: RefCell::new(Vec::new()),
            on_activate: ActivateNotifier::default(),
            enabled: Cell::new(true),
            attachments: Cell::new(0),
        }))
    }

    /// Rebuilds the menus. The previous menus are removed.
    ///
    /// The heading index is the position in the sequence passed, and the item index is
    /// the position in the sequence including separators.
    pub fn set_menus(&self, menus: &[MenuSpec]) {
        self.0.clear_accels();
        for name in self.0.actions.list_actions() {
            self.0.actions.remove_action(&name);
        }

        let whole = self.0.enabled.get();
        let model = gio::Menu::new();
        let mut built = Vec::with_capacity(menus.len());
        for (menu_index, spec) in menus.iter().enumerate() {
            let submenu = gio::Menu::new();
            // 区切り線は「ここで節を切る」ことで表す。
            let mut section = gio::Menu::new();
            let mut actions = Vec::with_capacity(spec.items.len());
            for (item_index, item) in spec.items.iter().enumerate() {
                if item.is_separator() {
                    if section.n_items() > 0 {
                        submenu.append_section(None, &section);
                        section = gio::Menu::new();
                    }
                    actions.push(None);
                    continue;
                }

                let name = MenuBarInner::action_name(menu_index, item_index);
                let action = gio::SimpleAction::new(&name, None);
                action.set_enabled(item.enabled && whole);
                // ハンドルを強く持つとシグナルとの間で循環するため、弱参照にする。
                let weak = Rc::downgrade(&self.0);
                action.connect_activate(move |_, _| {
                    if let Some(inner) = weak.upgrade() {
                        inner.on_activate.emit(menu_index, item_index);
                    }
                });
                self.0.actions.add_action(&action);

                let detailed = self.0.detailed_name(menu_index, item_index);
                section.append(Some(&item.label), Some(&detailed));
                actions.push(Some(action));
            }
            if section.n_items() > 0 {
                submenu.append_section(None, &section);
            }
            model.append_submenu(Some(&spec.title), &submenu);
            built.push(actions);
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
    pub fn set_enabled(&self, enabled: bool) {
        self.0.enabled.set(enabled);
        self.apply_enabled();
    }

    /// Applies the per-item settings and the overall setting to the native side.
    ///
    /// Making headings unclickable only needs `GtkWidget`'s sensitive, but accelerators
    /// invoke actions directly, so the actions are disabled too.
    fn apply_enabled(&self) {
        let whole = self.0.enabled.get();
        self.0.native.set_sensitive(whole);
        let menus = self.0.menus.borrow();
        for (spec, actions) in menus.iter().zip(self.0.items.borrow().iter()) {
            for (entry, action) in spec.items.iter().zip(actions.iter()) {
                if let Some(action) = action {
                    action.set_enabled(entry.enabled && whole);
                }
            }
        }
    }

    /// Runs the item as if the user had chosen it.
    ///
    /// Does nothing for separators, disabled items or out of range.
    pub fn activate(&self, menu: usize, item: usize) {
        if self.is_item_enabled(menu, item) {
            self.0.on_activate.emit(menu, item);
        }
    }

    /// Called with the heading and item indices when an item is clicked. Setting it
    /// again removes the previous callback.
    pub fn on_activate(&self, f: impl FnMut(usize, usize) + 'static) {
        self.0.on_activate.set(f);
    }

    /// The corresponding `GtkPopoverMenuBar`. Public as a backend-specific escape
    /// hatch.
    pub fn native_menu_bar(&self) -> gtk::PopoverMenuBar {
        self.0.native.clone()
    }

    /// The `GSimpleAction` for the item. `None` for separators and out of range. Public
    /// as a backend-specific escape hatch.
    pub fn native_action(&self, menu: usize, item: usize) -> Option<gio::SimpleAction> {
        self.0.items.borrow().get(menu)?.get(item)?.clone()
    }

    /// Returns the item's action as a name with the namespace (in the form
    /// `naui-menubar-3.m0i1`). `None` for separators and out of range.
    ///
    /// It is the name passed to `gtk_widget_activate_action` or
    /// `gtk_application_set_accels_for_action`, and is public as a backend-specific
    /// escape hatch.
    pub fn native_action_name(&self, menu: usize, item: usize) -> Option<String> {
        self.native_action(menu, item)?;
        Some(self.0.detailed_name(menu, item))
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
