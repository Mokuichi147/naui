//! Popup (context) menus (AppKit).
//!
//! The real object is an `NSMenu`, with `NSMenuItem`s as items. Separators use
//! `NSMenuItem::separatorItem` as is.
//!
//! [`PopupMenu::attach`]ing it to a widget makes it that view's `menu`. **AppKit
//! itself shows it on right-click (secondary button)**; naui does neither
//! positioning nor showing. Only when you want to show it from code does
//! [`PopupMenu::open_at`] call `popUpMenuPositioningItem:atLocation:inView:`.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use naui_core::PopupItem;
use objc2::rc::Retained;
use objc2::{sel, MainThreadMarker};
use objc2_app_kit::{NSMenu, NSMenuItem};
use objc2_foundation::{NSPoint, NSString};

use crate::trampoline::{ActionTarget, SelectHandler};
use crate::widgets::Widget;

struct PopupMenuInner {
    native: Retained<NSMenu>,
    /// A trampoline per item. `NSMenuItem` targets are weak, so they are held.
    targets: RefCell<Vec<Retained<ActionTarget>>>,
    handler: SelectHandler,
    /// The number of items, separators included.
    count: Cell<usize>,
    /// The handles of the widgets it is attached to. Kept alive along with their views.
    attached: RefCell<Vec<Box<dyn Widget>>>,
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
            handler: SelectHandler::default(),
            count: Cell::new(0),
            attached: RefCell::new(Vec::new()),
        }))
    }

    /// Rebuilds the items. The previous items are removed.
    ///
    /// The index is the position in the sequence including separators.
    pub fn set_items(&self, items: &[PopupItem]) {
        let mtm = MainThreadMarker::from(&*self.0.native);
        self.0.native.removeAllItems();
        self.0.targets.borrow_mut().clear();

        let mut targets = Vec::with_capacity(items.len());
        for (index, item) in items.iter().enumerate() {
            if item.is_separator() {
                self.0.native.addItem(&NSMenuItem::separatorItem(mtm));
                continue;
            }
            let native_item = NSMenuItem::new(mtm);
            native_item.setTitle(&NSString::from_str(&item.label));
            native_item.setEnabled(item.enabled);
            // ハンドルを強く持つとトランポリンとの間で循環するため、弱参照にする。
            let target = ActionTarget::new(mtm, {
                let weak = Rc::downgrade(&self.0);
                move || {
                    let Some(inner) = weak.upgrade() else {
                        return;
                    };
                    inner.handler.emit(index);
                }
            });
            unsafe {
                native_item.setTarget(Some(&target));
                native_item.setAction(Some(sel!(invoke:)));
            }
            self.0.native.addItem(&native_item);
            targets.push(target);
        }
        *self.0.targets.borrow_mut() = targets;
        self.0.count.set(items.len());
    }

    /// The number of items, separators included.
    pub fn len(&self) -> usize {
        self.0.count.get()
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

    /// Chooses an item through the same path as a user's choice (for tests and
    /// automation).
    ///
    /// Separators and items that cannot be chosen are ignored.
    pub fn select(&self, index: usize) {
        if index >= self.len() {
            return;
        }
        let Some(item) = self.0.native.itemAtIndex(index as isize) else {
            return;
        };
        if item.isSeparatorItem() || !item.isEnabled() {
            return;
        }
        self.0.native.performActionForItemAtIndex(index as isize);
    }

    /// Called with the index when an item is chosen.
    pub fn on_select(&self, f: impl FnMut(usize) + 'static) {
        self.0.handler.set(f);
    }

    /// The real AppKit menu. Public as a backend-specific escape hatch.
    pub fn native_menu(&self) -> Retained<NSMenu> {
        self.0.native.clone()
    }
}
