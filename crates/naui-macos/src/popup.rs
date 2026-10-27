//! Popup (context) menus (AppKit).
//!
//! The real object is an `NSMenu`, with `NSMenuItem`s as items. Separators use
//! `NSMenuItem::separatorItem` as is.
//!
//! [`PopupMenu::attach`]ing it to a widget makes it that view's `menu`. **AppKit
//! itself shows it on right-click (secondary button)**; naui does neither
//! positioning nor showing. Only when you want to show it from code does
//! [`PopupMenu::open_at`] call `popUpMenuPositioningItem:atLocation:inView:`.
//!
//! Submenus are `NSMenuItem.submenu` and check marks are `NSMenuItem.state`, the
//! same as [`crate::MenuBar`].

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use naui_core::{MenuHandler, PopupItem};
use objc2::rc::Retained;
use objc2::{sel, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSMenu, NSMenuItem};
use objc2_foundation::{NSPoint, NSString};

use crate::menu_bar::apply_checked;
use crate::trampoline::ActionTarget;
use crate::widgets::Widget;

struct PopupMenuInner {
    native: Retained<NSMenu>,
    /// A trampoline per item. `NSMenuItem` targets are weak, so they are held.
    targets: RefCell<Vec<Retained<ActionTarget>>>,
    handler: MenuHandler,
    /// The items as passed, with the current check states.
    items: RefCell<Vec<PopupItem>>,
    /// The `NSMenuItem`s looked up by path. Separators are not in it.
    natives: RefCell<HashMap<Vec<usize>, Retained<NSMenuItem>>>,
    /// The handles of the widgets it is attached to. Kept alive along with their views.
    attached: RefCell<Vec<Box<dyn Widget>>>,
}

impl PopupMenuInner {
    /// Flips a check item, then notifies.
    fn choose(&self, path: &[usize]) {
        let flipped = {
            let mut items = self.items.borrow_mut();
            PopupItem::at_mut(&mut items, path)
                .filter(|item| item.is_checkable())
                .map(|item| {
                    let on = !item.checked.unwrap_or(false);
                    item.checked = Some(on);
                    on
                })
        };
        if let Some(on) = flipped {
            if let Some(native) = self.natives.borrow().get(path) {
                apply_checked(native, on);
            }
        }
        self.handler.emit(path);
    }
}

/// A popup (context) menu (NSMenu).
///
/// It is not a widget laid out on screen, so it is not a [`Widget`]. [`crate::Ui`]
/// holds the menus it creates, so dropping the return value does not remove the
/// menu from the widgets it is attached to.
#[derive(Clone)]
pub struct PopupMenu(Rc<PopupMenuInner>);

impl PopupMenu {
    pub(crate) fn new(mtm: MainThreadMarker) -> Self {
        let native = NSMenu::new(mtm);
        // 既定では AppKit が「その操作ができるか」を自分で判断して項目を
        // 出し入れしてしまい、`setEnabled(false)` が無視される。
        native.setAutoenablesItems(false);
        Self(Rc::new(PopupMenuInner {
            native,
            targets: RefCell::new(Vec::new()),
            handler: MenuHandler::default(),
            items: RefCell::new(Vec::new()),
            natives: RefCell::new(HashMap::new()),
            attached: RefCell::new(Vec::new()),
        }))
    }

    /// Rebuilds the items. The previous items are removed.
    pub fn set_items(&self, items: &[PopupItem]) {
        self.0.native.removeAllItems();
        self.0.targets.borrow_mut().clear();
        let mut natives = HashMap::new();
        self.fill(&self.0.native, items, &mut Vec::new(), true, &mut natives);
        *self.0.natives.borrow_mut() = natives;
        *self.0.items.borrow_mut() = items.to_vec();
    }

    /// Adds `items` to `menu`, recursing into submenus. `enabled` is whether every
    /// submenu leading to it is enabled.
    fn fill(
        &self,
        menu: &NSMenu,
        items: &[PopupItem],
        path: &mut Vec<usize>,
        enabled: bool,
        natives: &mut HashMap<Vec<usize>, Retained<NSMenuItem>>,
    ) {
        let mtm = MainThreadMarker::from(menu);
        for (index, item) in items.iter().enumerate() {
            if item.is_separator() {
                menu.addItem(&NSMenuItem::separatorItem(mtm));
                continue;
            }
            path.push(index);
            let native_item = NSMenuItem::new(mtm);
            native_item.setTitle(&NSString::from_str(&item.label));
            native_item.setEnabled(item.enabled && enabled);
            if let Some(children) = &item.submenu {
                let submenu =
                    NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str(&item.label));
                submenu.setAutoenablesItems(false);
                self.fill(&submenu, children, path, item.enabled && enabled, natives);
                native_item.setSubmenu(Some(&submenu));
            } else {
                if let Some(checked) = item.checked {
                    apply_checked(&native_item, checked);
                }
                // ハンドルを強く持つとトランポリンとの間で循環するため、弱参照にする。
                let target = ActionTarget::new(mtm, {
                    let weak = Rc::downgrade(&self.0);
                    let path = path.clone();
                    move || {
                        if let Some(inner) = weak.upgrade() {
                            inner.choose(&path);
                        }
                    }
                });
                unsafe {
                    native_item.setTarget(Some(&target));
                    native_item.setAction(Some(sel!(invoke:)));
                }
                self.0.targets.borrow_mut().push(target);
            }
            menu.addItem(&native_item);
            natives.insert(path.clone(), native_item);
            path.pop();
        }
    }

    /// The number of top-level items, separators included.
    pub fn len(&self) -> usize {
        self.0.items.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Makes this menu appear on right-click on the widget.
    ///
    /// The same menu can be attached to several widgets.
    pub fn attach(&self, widget: &dyn Widget) {
        let view = widget.native_view();
        unsafe { view.setMenu(Some(&self.0.native)) };
        self.0.attached.borrow_mut().push(widget.boxed_clone());
    }

    /// Shows the menu from code. The position is in logical pixels **from the top
    /// left** of `widget` (y pointing down).
    ///
    /// AppKit runs the events while it is shown, so this call does not return until the
    /// menu closes.
    pub fn open_at(&self, widget: &dyn Widget, x: f64, y: f64) {
        let view = widget.native_view();
        // AppKit のビュー座標は既定で左下原点。naui は左上原点でそろえる。
        let point = if view.isFlipped() {
            NSPoint::new(x, y)
        } else {
            NSPoint::new(x, view.bounds().size.height - y)
        };
        self.0
            .native
            .popUpMenuPositioningItem_atLocation_inView(None, point, Some(&view));
    }

    /// Closes the menu being shown. Does nothing if none is shown.
    pub fn close(&self) {
        self.0.native.cancelTracking();
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
        if let Some(native) = self.0.natives.borrow().get(path) {
            apply_checked(native, checked);
        }
    }

    /// Whether the check mark of the item at `path` is on.
    pub fn is_item_checked(&self, path: &[usize]) -> bool {
        PopupItem::at(&self.0.items.borrow(), path)
            .is_some_and(|item| item.is_checkable() && item.checked == Some(true))
    }

    /// Chooses an item through the same path as a user's choice (for tests and
    /// automation).
    ///
    /// Separators, submenu headings and items that cannot be chosen are ignored.
    pub fn select(&self, path: &[usize]) {
        if !PopupItem::can_choose(&self.0.items.borrow(), path) {
            return;
        }
        // 項目を持つ NSMenu から、利用者が選んだときと同じ経路で動かす。
        // 区切り線も NSMenu の項目なので、経路の末尾がそのまま位置になる。
        let Some((&index, parent)) = path.split_last() else {
            return;
        };
        let menu = if parent.is_empty() {
            Some(self.0.native.clone())
        } else {
            self.0
                .natives
                .borrow()
                .get(parent)
                .and_then(|holder| holder.submenu())
        };
        if let Some(menu) = menu {
            menu.performActionForItemAtIndex(index as isize);
        }
    }

    /// Called with the item's path when an item is chosen.
    pub fn on_select(&self, f: impl FnMut(&[usize]) + 'static) {
        self.0.handler.set(f);
    }

    /// The `NSMenuItem` for the item at `path` (submenu headings included). `None` for
    /// separators and out of range. Public as a backend-specific escape hatch.
    pub fn native_item(&self, path: &[usize]) -> Option<Retained<NSMenuItem>> {
        self.0.natives.borrow().get(path).cloned()
    }

    /// The real AppKit menu. Public as a backend-specific escape hatch.
    pub fn native_menu(&self) -> Retained<NSMenu> {
        self.0.native.clone()
    }
}
