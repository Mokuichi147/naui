//! Popup (context) menus (WinUI 3's native `MenuFlyout`).
//!
//! | naui | WinUI 3 |
//! | --- | --- |
//! | `PopupMenu` | `MenuFlyout` |
//! | Item | `MenuFlyoutItem` |
//! | Separator | `MenuFlyoutSeparator` |
//!
//! Where it appears, its shadow, rounded corners, light dismiss, keyboard
//! navigation (arrows and Esc) and wrapping at screen edges are all owned by
//! `MenuFlyout`. There is nothing naui builds.
//!
//! Showing it on right-click is simply left to `UIElement.ContextFlyout`. WinUI
//! shows it for right-click, long-press and the context key alike.

use std::cell::RefCell;
use std::rc::{Rc, Weak};
use std::sync::Arc;

use naui_core::{PopupItem, Result};
use naui_winui3::Microsoft::UI::Xaml::Controls::{MenuFlyout, MenuFlyoutItem, MenuFlyoutSeparator};
use naui_winui3::Microsoft::UI::Xaml::RoutedEventHandler;
use windows::Foundation::Point;
use windows_core::HSTRING;

use crate::navigation::SelectHandler;
use crate::to_error;
use crate::ui_thread::UiThreadCell;
use crate::widgets::Widget;

struct PopupMenuInner {
    flyout: MenuFlyout,
    /// The `MenuFlyoutItem` per item. `None` at separator positions.
    items: RefCell<Vec<Option<MenuFlyoutItem>>>,
    /// The handles of the widgets it is attached to. Kept alive along with their
    /// callbacks.
    attached: RefCell<Vec<Box<dyn Widget>>>,
    handler: SelectHandler,
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
            attached: RefCell::new(Vec::new()),
            handler: SelectHandler::new(),
        })))
    }

    /// Rebuilds the items. The previous items are removed.
    ///
    /// The index is the position in the sequence including separators.
    pub fn set_items(&self, items: &[PopupItem]) {
        let Ok(children) = self.0.flyout.Items() else {
            return;
        };
        let _ = children.Clear();
        self.0.items.borrow_mut().clear();

        let mut built = Vec::with_capacity(items.len());
        for (index, item) in items.iter().enumerate() {
            if item.is_separator() {
                if let Ok(separator) = MenuFlyoutSeparator::new() {
                    let _ = children.Append(&separator);
                }
                built.push(None);
                continue;
            }
            match self.build_item(&item.label, item.enabled, index) {
                Ok(entry) => {
                    let _ = children.Append(&entry);
                    built.push(Some(entry));
                }
                Err(_) => built.push(None),
            }
        }
        *self.0.items.borrow_mut() = built;
    }

    fn build_item(&self, label: &str, enabled: bool, index: usize) -> Result<MenuFlyoutItem> {
        let entry = MenuFlyoutItem::new().map_err(|e| to_error("メニュー項目の生成", e))?;
        entry
            .SetText(&HSTRING::from(label))
            .map_err(|e| to_error("メニュー項目の文字設定", e))?;
        let _ = entry.SetIsEnabled(enabled);

        let weak = weak_cell(&self.0);
        let handler = RoutedEventHandler::new(move |_sender, _args| {
            if let Some(inner) = weak.try_with_mut(|weak| weak.upgrade()).flatten() {
                // 閉じるのは `MenuFlyout` が自分で行う。
                PopupMenu(inner).0.handler.emit(index);
            }
            Ok(())
        });
        entry
            .Click(&handler)
            .map_err(|e| to_error("メニュー項目の購読", e))?;
        Ok(entry)
    }

    /// The number of items, separators included.
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

    /// Chooses an item through the same path as a user's choice (for tests and
    /// automation).
    ///
    /// Separators and items that cannot be chosen are ignored.
    pub fn select(&self, index: usize) {
        let entry = self.0.items.borrow().get(index).cloned().flatten();
        let Some(entry) = entry else {
            return;
        };
        if !entry.IsEnabled().unwrap_or(false) {
            return;
        }
        self.close();
        self.0.handler.emit(index);
    }

    /// Called with the index when an item is chosen.
    pub fn on_select(&self, f: impl FnMut(usize) + 'static) {
        self.0.handler.set(f);
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
