//! A radio group that lays out options and lets the user choose exactly one
//! (AppKit).
//!
//! Radio-type `NSButton`s are laid out in an `NSStackView`. AppKit treats "radio
//! buttons in the same superview with the same action" as one group, so giving the
//! group its own stack view keeps it from mixing with other radio groups.
//!
//! naui also writes the exclusive state explicitly. AppKit's automatic grouping
//! only works for user clicks, while
//! [`set_selected`](RadioGroup::set_selected) and
//! [`clear_selection`](RadioGroup::clear_selection) are taken care of by naui.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use naui_core::Orientation;
use objc2::rc::Retained;
use objc2::{sel, MainThreadMarker, Message};
use objc2_app_kit::{
    NSButton, NSControlStateValueOff, NSControlStateValueOn, NSLayoutAttribute, NSStackView,
    NSStackViewDistribution, NSUserInterfaceLayoutOrientation, NSView,
};
use objc2_foundation::NSString;

use crate::trampoline::{ActionTarget, SelectHandler};
use crate::widgets::{impl_widget, Widget};

struct RadioGroupInner {
    native: Retained<NSStackView>,
    buttons: RefCell<Vec<Retained<NSButton>>>,
    /// The target per button. AppKit targets are weak, so they are kept alive here.
    targets: RefCell<Vec<Retained<ActionTarget>>>,
    handler: SelectHandler,
    /// The setting from [`set_enabled`](RadioGroup::set_enabled). Remembered so the
    /// buttons recreated by `set_items` get the same state.
    enabled: Cell<bool>,
}

/// A radio group that lays out options and lets the user choose exactly one
/// (radio-type `NSButton`s).
///
/// Right after creation and right after [`set_items`](Self::set_items), nothing is
/// selected.
#[derive(Clone)]
pub struct RadioGroup(Rc<RadioGroupInner>);
impl_widget!(RadioGroup);

impl RadioGroup {
    pub(crate) fn new(mtm: MainThreadMarker) -> Self {
        let native = NSStackView::new(mtm);
        native.setDistribution(NSStackViewDistribution::GravityAreas);
        apply_orientation(&native, Orientation::Vertical);
        Self(Rc::new(RadioGroupInner {
            native,
            buttons: RefCell::new(Vec::new()),
            targets: RefCell::new(Vec::new()),
            handler: SelectHandler::default(),
            enabled: Cell::new(true),
        }))
    }

    /// Rebuilds the options and clears the selection. No selection notification is
    /// sent.
    pub fn set_items<S: AsRef<str>>(&self, items: &[S]) {
        let mtm = MainThreadMarker::from(&*self.0.native);
        for button in self.0.buttons.borrow_mut().drain(..) {
            self.0.native.removeArrangedSubview(&button);
            button.removeFromSuperview();
        }
        self.0.targets.borrow_mut().clear();

        let enabled = self.0.enabled.get();
        let mut buttons = Vec::with_capacity(items.len());
        let mut targets = Vec::with_capacity(items.len());
        for (index, item) in items.iter().enumerate() {
            let button = unsafe {
                NSButton::radioButtonWithTitle_target_action(
                    &NSString::from_str(item.as_ref()),
                    None,
                    None,
                    mtm,
                )
            };
            // ラジオボタンは作った直後にオンで来ることがあるため、明示的に外す。
            button.setState(NSControlStateValueOff);
            button.setEnabled(enabled);
            let target = ActionTarget::new(mtm, {
                let weak = Rc::downgrade(&self.0);
                move || {
                    let Some(inner) = weak.upgrade() else {
                        return;
                    };
                    let group = RadioGroup(inner);
                    // AppKit も排他にしてくれるが、naui 側の状態と必ず揃える。
                    group.write_selected(Some(index));
                    group.0.handler.emit(index);
                }
            });
            unsafe {
                button.setTarget(Some(&target));
                button.setAction(Some(sel!(invoke:)));
            }
            self.0.native.addArrangedSubview(&button);
            buttons.push(button);
            targets.push(target);
        }
        *self.0.buttons.borrow_mut() = buttons;
        *self.0.targets.borrow_mut() = targets;
    }

    /// The number of options.
    pub fn len(&self) -> usize {
        self.0.buttons.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The currently selected option. `None` if nothing is selected.
    pub fn selected(&self) -> Option<usize> {
        self.0
            .buttons
            .borrow()
            .iter()
            .position(|button| button.state() != NSControlStateValueOff)
    }

    /// Selects an in-range option without notifying. Does nothing if out of range.
    pub fn set_selected(&self, index: usize) {
        if index < self.len() {
            self.write_selected(Some(index));
        }
    }

    /// Clears the selection without notifying.
    pub fn clear_selection(&self) {
        self.write_selected(None);
    }

    /// Selects an in-range option and notifies, as if the user had chosen it.
    pub fn select(&self, index: usize) {
        if index < self.len() {
            self.write_selected(Some(index));
            self.0.handler.emit(index);
        }
    }

    /// The direction the options are laid out in. Vertical by default.
    pub fn set_orientation(&self, orientation: Orientation) {
        apply_orientation(&self.0.native, orientation);
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.0.enabled.set(enabled);
        for button in self.0.buttons.borrow().iter() {
            button.setEnabled(enabled);
        }
    }

    /// Called with the index when an option is chosen. Setting it again removes the
    /// previous callback.
    pub fn on_select(&self, f: impl FnMut(usize) + 'static) {
        self.0.handler.set(f);
    }

    /// The real AppKit control. Public as a backend-specific escape hatch.
    pub fn native_buttons(&self) -> Vec<Retained<NSButton>> {
        self.0.buttons.borrow().clone()
    }

    /// Writes the exclusive state. With `None`, all are off.
    fn write_selected(&self, index: Option<usize>) {
        for (position, button) in self.0.buttons.borrow().iter().enumerate() {
            button.setState(if Some(position) == index {
                NSControlStateValueOn
            } else {
                NSControlStateValueOff
            });
        }
    }
}

fn apply_orientation(native: &NSStackView, orientation: Orientation) {
    if orientation.is_vertical() {
        native.setOrientation(NSUserInterfaceLayoutOrientation::Vertical);
        // 縦に並べたラジオは、丸の位置を左端で揃える。
        native.setAlignment(NSLayoutAttribute::Leading);
    } else {
        native.setOrientation(NSUserInterfaceLayoutOrientation::Horizontal);
        native.setAlignment(NSLayoutAttribute::CenterY);
    }
}
