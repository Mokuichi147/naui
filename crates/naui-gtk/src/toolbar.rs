//! The toolbar (a row of `GtkButton`s placed on the `AdwHeaderBar`).
//!
//! In GNOME the convention is to put frequently used actions in the `AdwHeaderBar`
//! at the top of the window, and GTK3's `GtkToolbar` was removed in GTK4. So naui's
//! toolbar is not a [`Widget`](crate::Widget); it is attached to the header bar
//! with [`Window::set_toolbar`](crate::Window::set_toolbar). It appears in the same
//! place as macOS's `NSToolbar`, which merges with the title bar.
//!
//! Icons are [`ToolbarIcon`](naui_core::ToolbarIcon) mapped to icon theme names
//! (freedesktop standard names), and `label` is used for tooltips and screen
//! readers. Separators are `GtkSeparator`s.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::prelude::*;
use naui_core::ToolbarItem;

use crate::callback::Notifier;

pub(crate) struct ToolbarInner {
    /// The container inserted into the header bar. Items are laid out inside it.
    native: gtk::Box,
    items: RefCell<Vec<ToolbarItem>>,
    /// In the same order as the items. `None` at separators.
    buttons: RefCell<Vec<Option<gtk::Button>>>,
    on_activate: Notifier<usize>,
    /// Whether the whole toolbar is enabled. ANDed with the per-item settings.
    enabled: Cell<bool>,
}

/// A row of frequently used actions attached to the top of the window.
///
/// Not a [`Widget`](crate::Widget). Attach it with
/// [`Window::set_toolbar`](crate::Window::set_toolbar). Unlike navigation, it
/// **has no selected item**; every click calls [`on_activate`](Self::on_activate)
/// with the index. The index is the position in the sequence including separators;
/// separators are never returned.
#[derive(Clone)]
pub struct Toolbar(Rc<ToolbarInner>);

impl Toolbar {
    pub(crate) fn new() -> Self {
        let native = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        Self(Rc::new(ToolbarInner {
            native,
            items: RefCell::new(Vec::new()),
            buttons: RefCell::new(Vec::new()),
            on_activate: Notifier::default(),
            enabled: Cell::new(true),
        }))
    }

    /// The container inserted into the header bar. Used only by [`crate::Window`].
    pub(crate) fn mount(&self) -> gtk::Box {
        self.0.native.clone()
    }

    /// Rebuilds the items. The previous items are removed.
    ///
    /// The index is the position in the sequence including separators.
    pub fn set_items(&self, items: &[ToolbarItem]) {
        while let Some(child) = self.0.native.first_child() {
            self.0.native.remove(&child);
        }
        self.0.buttons.borrow_mut().clear();

        let whole = self.0.enabled.get();
        let mut buttons = Vec::with_capacity(items.len());
        for (index, item) in items.iter().enumerate() {
            if item.is_separator() {
                let separator = gtk::Separator::new(gtk::Orientation::Vertical);
                self.0.native.append(&separator);
                buttons.push(None);
                continue;
            }

            // 見た目はアイコンテーマの絵。無い名前ならテーマの
            // 「画像がありません」の絵が出る (GTK4 が面倒を見る)。
            let button = gtk::Button::from_icon_name(item.icon.icon_name());
            // ヘッダーバーのボタンは枠なしが GNOME の既定。
            button.add_css_class("flat");
            button.set_tooltip_text(Some(&item.label));
            // アイコンだけでは読み上げに出ないので、名前を持たせる。
            button.update_property(&[gtk::accessible::Property::Label(&item.label)]);
            button.set_sensitive(item.enabled && whole);
            // ハンドルを強く持つとシグナルとの間で循環するため、弱参照にする。
            let weak = Rc::downgrade(&self.0);
            button.connect_clicked(move |_| {
                if let Some(inner) = weak.upgrade() {
                    inner.on_activate.emit(index);
                }
            });
            self.0.native.append(&button);
            buttons.push(Some(button));
        }

        *self.0.buttons.borrow_mut() = buttons;
        self.0.items.borrow_mut().clear();
        self.0.items.borrow_mut().extend_from_slice(items);
    }

    /// The number of items, separators included.
    pub fn len(&self) -> usize {
        self.0.items.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Enables or disables one item. Does nothing for separators or out of range.
    pub fn set_item_enabled(&self, index: usize, enabled: bool) {
        let mut items = self.0.items.borrow_mut();
        let Some(item) = items.get_mut(index) else {
            return;
        };
        if item.is_separator() {
            return;
        }
        item.enabled = enabled;
        drop(items);
        self.apply_enabled();
    }

    /// Whether the item can be clicked now. `false` for separators and out of range.
    pub fn is_item_enabled(&self, index: usize) -> bool {
        self.0.enabled.get()
            && self
                .0
                .items
                .borrow()
                .get(index)
                .is_some_and(|item| !item.is_separator() && item.enabled)
    }

    /// Enables or disables the whole toolbar. Per-item settings are kept.
    pub fn set_enabled(&self, enabled: bool) {
        self.0.enabled.set(enabled);
        self.apply_enabled();
    }

    /// Applies the per-item settings and the overall setting to the native side.
    fn apply_enabled(&self) {
        let whole = self.0.enabled.get();
        let items = self.0.items.borrow();
        for (button, item) in self.0.buttons.borrow().iter().zip(items.iter()) {
            if let Some(button) = button {
                button.set_sensitive(item.enabled && whole);
            }
        }
    }

    /// Runs the item as if the user had clicked it.
    ///
    /// Does nothing for separators, disabled items or out of range.
    pub fn activate(&self, index: usize) {
        if self.is_item_enabled(index) {
            self.0.on_activate.emit(index);
        }
    }

    /// Called with the index when an item is clicked. Setting it again removes the
    /// previous callback.
    pub fn on_activate(&self, f: impl FnMut(usize) + 'static) {
        self.0.on_activate.set(f);
    }

    /// The `GtkBox` laying out the items. Public as a backend-specific escape hatch.
    pub fn native_box(&self) -> gtk::Box {
        self.0.native.clone()
    }

    /// The GTK4 button for the item. `None` for separators and out of range. Public as
    /// a backend-specific escape hatch.
    pub fn native_button(&self, index: usize) -> Option<gtk::Button> {
        self.0.buttons.borrow().get(index)?.clone()
    }
}
