//! Popup (context) menus (WinUI 3's native `MenuFlyout`).
//!
//! | naui | WinUI 3 |
//! | --- | --- |
//! | `PopupMenu` | `MenuFlyout` |
//! | Item | `MenuFlyoutItem` |
//! | Item with a check mark | `ToggleMenuFlyoutItem` |
//! | Submenu | `MenuFlyoutSubItem` |
//! | Separator | `MenuFlyoutSeparator` |
//!
//! Where it appears, its shadow, rounded corners, light dismiss, keyboard
//! navigation (arrows and Esc) and wrapping at screen edges are all owned by
//! `MenuFlyout`. There is nothing naui builds.
//!
//! Showing it on right-click is simply left to `UIElement.ContextFlyout`. WinUI
//! shows it for right-click, long-press and the context key alike.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::{Rc, Weak};
use std::sync::Arc;

use naui_core::{MenuHandler, MenuItem, PopupItem, Result};
use naui_winui3::Microsoft::UI::Xaml::Controls::MenuFlyout;
use naui_winui3::Microsoft::UI::Xaml::RoutedEventHandler;
use windows::Foundation::Point;

use crate::menu_bar::{build_entries, MenuEntry};
use crate::to_error;
use crate::ui_thread::UiThreadCell;
use crate::widgets::Widget;

struct PopupMenuInner {
    flyout: MenuFlyout,
    /// The items as passed, with the current check states.
    items: RefCell<Vec<PopupItem>>,
    /// The controls looked up by path. Separators are not in it.
    natives: RefCell<HashMap<Vec<usize>, MenuEntry>>,
    /// The handles of the widgets it is attached to. Kept alive along with their
    /// callbacks.
    attached: RefCell<Vec<Box<dyn Widget>>>,
    handler: MenuHandler,
}

impl PopupMenuInner {
    /// Flips a check item, then notifies. When the user clicks,
    /// `ToggleMenuFlyoutItem` has already flipped itself.
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
                native.set_checked(on);
            }
        }
        self.handler.emit(path);
    }
}

/// A popup (context) menu.
///
/// It is not a widget laid out on screen, so it is not a [`Widget`]. [`crate::Ui`]
/// holds the menus it creates, so dropping the return value does not remove it from
/// the widgets it is attached to.
#[derive(Clone)]
pub struct PopupMenu(Rc<PopupMenuInner>);

impl PopupMenu {
    pub(crate) fn new() -> Result<Self> {
        let flyout = MenuFlyout::new().map_err(|e| to_error("MenuFlyout の生成", e))?;
        Ok(Self(Rc::new(PopupMenuInner {
            flyout,
            items: RefCell::new(Vec::new()),
            natives: RefCell::new(HashMap::new()),
            attached: RefCell::new(Vec::new()),
            handler: MenuHandler::default(),
        })))
    }

    /// Rebuilds the items. The previous items are removed.
    pub fn set_items(&self, items: &[PopupItem]) {
        let Ok(children) = self.0.flyout.Items() else {
            return;
        };
        let _ = children.Clear();

        let weak = weak_cell(&self.0);
        let on_click = |path: Vec<usize>| {
            let weak = weak.clone();
            RoutedEventHandler::new(move |_sender, _args| {
                if let Some(inner) = weak.try_with_mut(|weak| weak.upgrade()).flatten() {
                    // 閉じるのは `MenuFlyout` が自分で行う。
                    inner.choose(&path);
                }
                Ok(())
            })
        };
        let mut natives = HashMap::new();
        build_entries(
            &children,
            &items
                .iter()
                .cloned()
                .map(MenuItem::from)
                .collect::<Vec<_>>(),
            &mut Vec::new(),
            true,
            &on_click,
            &mut natives,
        );
        *self.0.natives.borrow_mut() = natives;
        *self.0.items.borrow_mut() = items.to_vec();
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
    /// It is left to `ContextFlyout`, so WinUI decides whether it is invoked by
    /// right-click, long-press or the context key. The same menu may be attached to
    /// several widgets (it appears over the widget that invoked it).
    pub fn attach(&self, widget: &dyn Widget) {
        let element = widget.native_element();
        if element.SetContextFlyout(&self.0.flyout).is_ok() {
            self.0.attached.borrow_mut().push(widget.boxed_clone());
        }
    }

    /// Shows the menu from code. The position is in logical pixels **from the top
    /// left** of `widget` (y pointing down).
    pub fn open_at(&self, widget: &dyn Widget, x: f64, y: f64) {
        let element = widget.native_element();
        let point = Point {
            X: x as f32,
            Y: y as f32,
        };
        let _ = self.0.flyout.ShowAt2(&element, point);
    }

    /// Closes the menu being shown. Does nothing if none is shown.
    pub fn close(&self) {
        let _ = self.0.flyout.Hide();
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
            native.set_checked(checked);
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
        let enabled = self
            .0
            .natives
            .borrow()
            .get(path)
            .is_some_and(MenuEntry::is_enabled);
        if !enabled {
            return;
        }
        self.close();
        self.0.choose(path);
    }

    /// Called with the item's path when an item is chosen.
    pub fn on_select(&self, f: impl FnMut(&[usize]) + 'static) {
        self.0.handler.set(f);
    }

    /// The control for the item at `path` (submenu headings included). `None` for
    /// separators and out of range. Public as a backend-specific escape hatch.
    pub fn native_item(&self, path: &[usize]) -> Option<MenuEntry> {
        self.0.natives.borrow().get(path).cloned()
    }

    /// The menu itself. Public as a backend-specific escape hatch.
    pub fn native_flyout(&self) -> MenuFlyout {
        self.0.flyout.clone()
    }
}

/// WinRT delegates require `Send`, so it is wrapped in a UI-thread-only cell.
fn weak_cell(inner: &Rc<PopupMenuInner>) -> Arc<UiThreadCell<Weak<PopupMenuInner>>> {
    Arc::new(UiThreadCell::new(Rc::downgrade(inner)))
}
