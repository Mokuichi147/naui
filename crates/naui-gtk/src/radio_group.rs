//! A radio group that lays out options and lets the user choose exactly one (a
//! group of `GtkCheckButton`s).
//!
//! GTK4 removed `GtkRadioButton`; radio buttons are `GtkCheckButton`s grouped with
//! `gtk_check_button_set_group`. Laying the grouped buttons out in a `GtkBox` keeps
//! them from mixing with other radio groups.

use std::cell::RefCell;
use std::rc::Rc;

use gtk::glib;
use gtk::prelude::*;
use naui_core::Orientation;

use crate::bin::SizeBin;
use crate::callback::Notifier;
use crate::widgets::{impl_widget, Widget};

struct RadioGroupInner {
    native: gtk::Box,
    bin: SizeBin,
    buttons: RefCell<Vec<gtk::CheckButton>>,
    /// The `toggled` subscriptions, in the same order as the buttons. Blocked for
    /// changes from code.
    handlers: RefCell<Vec<glib::SignalHandlerId>>,
    on_select: Notifier<usize>,
}

/// A radio group that lays out options and lets the user choose exactly one.
///
/// Right after creation and right after [`set_items`](Self::set_items), nothing is
/// selected.
#[derive(Clone)]
pub struct RadioGroup(Rc<RadioGroupInner>);
impl_widget!(RadioGroup);

impl RadioGroup {
    pub(crate) fn new() -> Self {
        let native = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let bin = SizeBin::wrap(&native);
        Self(Rc::new(RadioGroupInner {
            native,
            bin,
            buttons: RefCell::new(Vec::new()),
            handlers: RefCell::new(Vec::new()),
            on_select: Notifier::default(),
        }))
    }

    /// Rebuilds the options and clears the selection. No selection notification is
    /// sent.
    pub fn set_items<S: AsRef<str>>(&self, items: &[S]) {
        for button in self.0.buttons.borrow_mut().drain(..) {
            // グループから外してから捨てないと、GTK4 が残りの組を触りにいく。
            button.set_group(gtk::CheckButton::NONE);
            self.0.native.remove(&button);
        }
        self.0.handlers.borrow_mut().clear();

        let mut buttons: Vec<gtk::CheckButton> = Vec::with_capacity(items.len());
        for item in items {
            let button = gtk::CheckButton::with_label(item.as_ref());
            crate::indicator::watch(&button);
            if let Some(first) = buttons.first() {
                button.set_group(Some(first));
            }
            self.0.native.append(&button);
            buttons.push(button);
        }

        // 購読はボタンをすべて並べ終えてからつなぐ。組み立ての最中に出る
        // `toggled` を通知として拾わないため。
        let handlers = buttons
            .iter()
            .enumerate()
            .map(|(index, button)| {
                let weak = Rc::downgrade(&self.0);
                button.connect_toggled(move |button| {
                    // 組の中では「外れた側」でも `toggled` が出る。点いたほうだけ通知する。
                    if !button.is_active() {
                        return;
                    }
                    if let Some(inner) = weak.upgrade() {
                        inner.on_select.emit(index);
                    }
                })
            })
            .collect();

        *self.0.buttons.borrow_mut() = buttons;
        *self.0.handlers.borrow_mut() = handlers;
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
            .position(|button| button.is_active())
    }

    /// Selects an in-range option without notifying. Does nothing if out of range.
    pub fn set_selected(&self, index: usize) {
        if index < self.len() {
            self.without_signals(|buttons| buttons[index].set_active(true));
        }
    }

    /// Clears the selection without notifying.
    pub fn clear_selection(&self) {
        self.without_signals(|buttons| {
            for button in buttons {
                button.set_active(false);
            }
        });
    }

    /// Selects an in-range option and notifies, as if the user had chosen it.
    pub fn select(&self, index: usize) {
        if index < self.len() {
            self.set_selected(index);
            self.0.on_select.emit(index);
        }
    }

    /// The direction the options are laid out in. Vertical by default.
    pub fn set_orientation(&self, orientation: Orientation) {
        self.0.native.set_orientation(if orientation.is_vertical() {
            gtk::Orientation::Vertical
        } else {
            gtk::Orientation::Horizontal
        });
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.0.native.set_sensitive(enabled);
    }

    /// Called with the index when an option is chosen. Setting it again removes the
    /// previous callback.
    pub fn on_select(&self, f: impl FnMut(usize) + 'static) {
        self.0.on_select.set(f);
    }

    /// The grouped `GtkCheckButton`s. Public as a backend-specific escape hatch.
    pub fn native_buttons(&self) -> Vec<gtk::CheckButton> {
        self.0.buttons.borrow().clone()
    }

    /// Operates with `toggled` blocked on every button in the group.
    ///
    /// Within a group, turning one on turns another off, so blocking just one is not
    /// enough.
    fn without_signals<T>(&self, f: impl FnOnce(&[gtk::CheckButton]) -> T) -> T {
        let buttons = self.0.buttons.borrow();
        let handlers = self.0.handlers.borrow();
        for (button, handler) in buttons.iter().zip(handlers.iter()) {
            button.block_signal(handler);
        }
        let out = f(&buttons);
        for (button, handler) in buttons.iter().zip(handlers.iter()) {
            button.unblock_signal(handler);
        }
        out
    }
}
