//! A combo box for choosing one of the options (AppKit).
//!
//! On macOS, an `NSPopUpButton` holding only the options is used rather than the
//! editable `NSComboBox`. It goes back to having nothing selected right after its
//! items are rebuilt, giving the same state transitions as the other backends'
//! `<select>` / ComboBox.

use std::rc::Rc;

use objc2::rc::Retained;
use objc2::{sel, MainThreadMarker, MainThreadOnly, Message};
use objc2_app_kit::{NSPopUpButton, NSView};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};

use crate::trampoline::{ActionTarget, SelectHandler};
use crate::widgets::{impl_widget, Widget};

struct ComboBoxInner {
    native: Retained<NSPopUpButton>,
    handler: SelectHandler,
    /// AppKit targets are weak, so they are kept alive on the handle side.
    _target: Retained<ActionTarget>,
}

/// A drop-down for choosing one of the options (`NSPopUpButton`).
///
/// Right after creation and right after [`set_items`](Self::set_items), nothing is
/// selected.
#[derive(Clone)]
pub struct ComboBox(Rc<ComboBoxInner>);
impl_widget!(ComboBox);

impl ComboBox {
    pub(crate) fn new(mtm: MainThreadMarker) -> Self {
        let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(0.0, 0.0));
        let native =
            NSPopUpButton::initWithFrame_pullsDown(NSPopUpButton::alloc(mtm), frame, false);
        let handler = SelectHandler::default();
        let target = ActionTarget::new(mtm, {
            let native = native.clone();
            let handler = handler.clone();
            move || {
                let index = native.indexOfSelectedItem();
                if index >= 0 && index < native.numberOfItems() {
                    handler.emit(index as usize);
                }
            }
        });
        unsafe {
            native.setTarget(Some(&target));
            native.setAction(Some(sel!(invoke:)));
        }

        Self(Rc::new(ComboBoxInner {
            native,
            handler,
            _target: target,
        }))
    }

    /// Rebuilds the items and clears the selection. No selection notification is sent.
    pub fn set_items<S: AsRef<str>>(&self, items: &[S]) {
        self.0.native.removeAllItems();
        for item in items {
            self.0
                .native
                .addItemWithTitle(&NSString::from_str(item.as_ref()));
        }
        // NSPopUpButton は最初の項目を追加すると自動選択するので、明示的に外す。
        self.0.native.selectItem(None);
    }

    /// The number of items.
    pub fn len(&self) -> usize {
        self.0.native.numberOfItems() as usize
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The currently selected item. `None` if nothing is selected.
    pub fn selected(&self) -> Option<usize> {
        let index = self.0.native.indexOfSelectedItem();
        (index >= 0 && index < self.0.native.numberOfItems()).then_some(index as usize)
    }

    /// Selects an in-range item without notifying. Does nothing if out of range.
    pub fn set_selected(&self, index: usize) {
        if index < self.len() {
            self.0.native.selectItemAtIndex(index as isize);
        }
    }

    /// Clears the selection without notifying.
    pub fn clear_selection(&self) {
        self.0.native.selectItem(None);
    }

    /// Selects an in-range item and notifies, as if the user had chosen it.
    pub fn select(&self, index: usize) {
        if index < self.len() {
            self.0.native.selectItemAtIndex(index as isize);
            self.0.handler.emit(index);
        }
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.0.native.setEnabled(enabled);
    }

    /// Called with the index when an item is chosen. Setting it again removes the
    /// previous callback.
    pub fn on_select(&self, f: impl FnMut(usize) + 'static) {
        self.0.handler.set(f);
    }

    /// The real AppKit control. Public as a backend-specific escape hatch.
    pub fn native_combo_box(&self) -> Retained<NSPopUpButton> {
        self.0.native.clone()
    }
}
