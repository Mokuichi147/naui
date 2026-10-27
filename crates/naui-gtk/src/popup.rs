//! Popup (context) menus (`GtkPopoverMenu` + `GMenu`).
//!
//! GTK4 menus are made of "models (`GMenu`) and actions (`GAction`)". **One
//! `GSimpleAction` is created per item**, and whether it can be chosen is expressed
//! by enabling or disabling the action (`GMenuItem` itself has no enabled state).
//! Submenus and check marks are built the same way as [`crate::MenuBar`]'s.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use gtk::gio;
use gtk::prelude::*;
use naui_core::{MenuHandler, PopupItem};

use crate::menu_bar::{action_name, item_action, submenu_action, submenu_item};
use crate::widgets::Widget;

/// The namespace that holds the menu's actions.
const GROUP: &str = "naui-popup";

struct PopupInner {
    native: gtk::PopoverMenu,
    actions: gio::SimpleActionGroup,
    /// The items as passed, with the current check states.
    items: RefCell<Vec<PopupItem>>,
    /// Actions looked up by path. Separators are not in it.
    natives: RefCell<HashMap<Vec<usize>, gio::SimpleAction>>,
    on_select: MenuHandler,
    /// The widgets it is attached to, for catching right-clicks.
    attached: RefCell<Vec<gtk::Widget>>,
}

/// A popup (context) menu shown on right-click.
///
/// It is not a widget laid out on screen, so it is not a [`Widget`].
#[derive(Clone)]
pub struct PopupMenu(Rc<PopupInner>);

impl PopupMenu {
    pub(crate) fn new() -> Self {
        let native = gtk::PopoverMenu::from_model(None::<&gio::Menu>);
        native.set_has_arrow(false);
        native.set_halign(gtk::Align::Start);
        let actions = gio::SimpleActionGroup::new();
        native.insert_action_group(GROUP, Some(&actions));
        Self(Rc::new(PopupInner {
            native,
            actions,
            items: RefCell::new(Vec::new()),
            natives: RefCell::new(HashMap::new()),
            on_select: MenuHandler::default(),
            attached: RefCell::new(Vec::new()),
        }))
    }

    /// The corresponding `GtkPopoverMenu`. Public as a backend-specific escape hatch.
    pub fn native_popover(&self) -> gtk::PopoverMenu {
        self.0.native.clone()
    }

    /// Rebuilds the items. The previous items are removed.
    pub fn set_items(&self, items: &[PopupItem]) {
        for name in self.0.actions.list_actions() {
            self.0.actions.remove_action(&name);
        }
        let mut natives = HashMap::new();
        let menu = self.build_menu(items, &mut Vec::new(), true, &mut natives);
        self.0.native.set_menu_model(Some(&menu));
        *self.0.natives.borrow_mut() = natives;

        let mut stored = self.0.items.borrow_mut();
        stored.clear();
        stored.extend_from_slice(items);
    }

    /// Builds the `GMenu` for `items`, recursing into submenus. `enabled` is whether
    /// every submenu leading to it is enabled.
    fn build_menu(
        &self,
        items: &[PopupItem],
        path: &mut Vec<usize>,
        enabled: bool,
        natives: &mut HashMap<Vec<usize>, gio::SimpleAction>,
    ) -> gio::Menu {
        let menu = gio::Menu::new();
        let mut section = gio::Menu::new();
        for (index, item) in items.iter().enumerate() {
            if item.is_separator() {
                // 区切り線は「ここで節を切る」ことで表す。
                if section.n_items() > 0 {
                    menu.append_section(None, &section);
                    section = gio::Menu::new();
                }
                continue;
            }
            path.push(index);
            let name = action_name("item", path);
            let detailed = format!("{GROUP}.{name}");
            let here = item.enabled && enabled;
            if let Some(children) = &item.submenu {
                let action = submenu_action(&name, here);
                self.0.actions.add_action(&action);
                let model = self.build_menu(children, path, here, natives);
                section.append_item(&submenu_item(&item.label, &model, &detailed));
                natives.insert(path.clone(), action);
            } else {
                let action = item_action(&name, item.checked, here);
                let weak = Rc::downgrade(&self.0);
                let at = path.clone();
                action.connect_activate(move |_, _| {
                    if let Some(inner) = weak.upgrade() {
                        PopupMenu(inner).choose(&at);
                    }
                });
                self.0.actions.add_action(&action);
                section.append(Some(&item.label), Some(&detailed));
                natives.insert(path.clone(), action);
            }
            path.pop();
        }
        if section.n_items() > 0 {
            menu.append_section(None, &section);
        }
        menu
    }

    /// Flips a check item, then notifies.
    fn choose(&self, path: &[usize]) {
        let flipped = {
            let mut items = self.0.items.borrow_mut();
            PopupItem::at_mut(&mut items, path)
                .filter(|item| item.is_checkable())
                .map(|item| {
                    let on = !item.checked.unwrap_or(false);
                    item.checked = Some(on);
                    on
                })
        };
        if let Some(on) = flipped {
            if let Some(action) = self.0.natives.borrow().get(path) {
                action.set_state(&on.to_variant());
            }
        }
        self.0.on_select.emit(path);
    }

    /// The number of top-level items, separators included.
    pub fn len(&self) -> usize {
        self.0.items.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Makes this menu appear on right-click on the widget. It can be attached to any
    /// number of widgets.
    pub fn attach(&self, widget: &dyn Widget) {
        let target = widget.native_widget();
        let gesture = gtk::GestureClick::new();
        // 3 は副ボタン (右クリック)。
        gesture.set_button(gtk::gdk::BUTTON_SECONDARY);
        {
            let weak = Rc::downgrade(&self.0);
            let target = target.clone();
            gesture.connect_pressed(move |_, _, x, y| {
                if let Some(inner) = weak.upgrade() {
                    PopupMenu(inner).show_at(&target, x, y);
                }
            });
        }
        target.add_controller(gesture);
        self.0.attached.borrow_mut().push(target);
    }

    /// Shows it from code. The position is in logical pixels from the top left of
    /// `widget`.
    pub fn open_at(&self, widget: &dyn Widget, x: f64, y: f64) {
        self.show_at(&widget.native_widget(), x, y);
    }

    /// Shows it after switching what it is attached to.
    ///
    /// A GTK4 popover can have only one parent, so it is reattached to the current
    /// widget every time it is shown.
    fn show_at(&self, target: &gtk::Widget, x: f64, y: f64) {
        let native = &self.0.native;
        if native.parent().as_ref() != Some(target) {
            if native.parent().is_some() {
                native.unparent();
            }
            native.set_parent(target);
        }
        native.set_pointing_to(Some(&gtk::gdk::Rectangle::new(
            x.round() as i32,
            y.round() as i32,
            1,
            1,
        )));
        native.popup();
    }

    pub fn close(&self) {
        self.0.native.popdown();
    }

    /// Turns the check mark of the item at `path` on or off, without notifying. Does
    /// nothing for items without a check mark.
    pub fn set_item_checked(&self, path: &[usize], checked: bool) {
        let mut items = self.0.items.borrow_mut();
        let Some(item) = PopupItem::at_mut(&mut items, path) else {
            return;
        };
        if !item.is_checkable() {
            return;
        }
        item.checked = Some(checked);
        drop(items);
        if let Some(action) = self.0.natives.borrow().get(path) {
            action.set_state(&checked.to_variant());
        }
    }

    /// Whether the check mark of the item at `path` is on.
    pub fn is_item_checked(&self, path: &[usize]) -> bool {
        PopupItem::at(&self.0.items.borrow(), path)
            .is_some_and(|item| item.is_checkable() && item.checked == Some(true))
    }

    /// Notifies through the same path as a user's choice (a check item flips).
    ///
    /// Separators, submenu headings and items that cannot be chosen are ignored.
    pub fn select(&self, path: &[usize]) {
        let selectable = PopupItem::can_choose(&self.0.items.borrow(), path);
        if selectable {
            self.choose(path);
        }
    }

    /// Called with the item's path when an item is chosen.
    pub fn on_select(&self, f: impl FnMut(&[usize]) + 'static) {
        self.0.on_select.set(f);
    }

    /// The `GSimpleAction` for the item at `path` (for a submenu heading, its
    /// `submenu-action`). `None` for separators and out of range. Public as a
    /// backend-specific escape hatch.
    pub fn native_action(&self, path: &[usize]) -> Option<gio::SimpleAction> {
        self.0.natives.borrow().get(path).cloned()
    }
}

impl Drop for PopupInner {
    fn drop(&mut self) {
        // GTK4 では、親を持ったまま壊すと警告が出る。
        if self.native.parent().is_some() {
            self.native.unparent();
        }
    }
}
