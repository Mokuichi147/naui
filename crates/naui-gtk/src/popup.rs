//! Popup (context) menus (`GtkPopoverMenu` + `GMenu`).
//!
//! GTK4 menus are made of "models (`GMenu`) and actions (`GAction`)". naui's items
//! are just "text, whether they can be chosen, separators", so **one
//! `GSimpleAction` is created per item**, and whether it can be chosen is expressed
//! by enabling or disabling the action (`GMenuItem` itself has no enabled state).

use std::cell::RefCell;
use std::rc::Rc;

use gtk::gio;
use gtk::prelude::*;
use naui_core::PopupItem;

use crate::callback::Notifier;
use crate::widgets::Widget;

/// The namespace that holds the menu's actions.
const GROUP: &str = "naui-popup";

struct PopupInner {
    native: gtk::PopoverMenu,
    actions: gio::SimpleActionGroup,
    items: RefCell<Vec<PopupItem>>,
    on_select: Notifier<usize>,
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
            on_select: Notifier::default(),
            attached: RefCell::new(Vec::new()),
        }))
    }

    /// The corresponding `GtkPopoverMenu`. Public as a backend-specific escape hatch.
    pub fn native_popover(&self) -> gtk::PopoverMenu {
        self.0.native.clone()
    }

    /// Rebuilds the items. Indices count **the sequence including separators**.
    pub fn set_items(&self, items: &[PopupItem]) {
        for name in self.0.actions.list_actions() {
            self.0.actions.remove_action(&name);
        }

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
            let name = format!("item{index}");
            let action = gio::SimpleAction::new(&name, None);
            action.set_enabled(item.enabled);
            {
                let weak = Rc::downgrade(&self.0);
                action.connect_activate(move |_, _| {
                    if let Some(inner) = weak.upgrade() {
                        inner.on_select.emit(index);
                    }
                });
            }
            self.0.actions.add_action(&action);
            section.append(Some(&item.label), Some(&format!("{GROUP}.{name}")));
        }
        if section.n_items() > 0 {
            menu.append_section(None, &section);
        }
        self.0.native.set_menu_model(Some(&menu));

        let mut stored = self.0.items.borrow_mut();
        stored.clear();
        stored.extend_from_slice(items);
    }

    /// The number of items, separators included.
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

    /// Notifies through the same path as a user's choice.
    ///
    /// Separators and items that cannot be chosen are ignored.
    pub fn select(&self, index: usize) {
        let selectable = self
            .0
            .items
            .borrow()
            .get(index)
            .is_some_and(|item| item.enabled && !item.is_separator());
        if selectable {
            self.0.on_select.emit(index);
        }
    }

    /// Called with the index when an item is chosen.
    pub fn on_select(&self, f: impl FnMut(usize) + 'static) {
        self.0.on_select.set(f);
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
