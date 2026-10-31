//! Popovers (`GtkPopover`).
//!
//! A GTK4 popover is a child of the widget it points at, so it is attached each
//! time [`Popover::open`] is called, and detached again once it has closed. While
//! attached, that widget is held: the parent does not keep its popover from
//! dangling, so a widget finalized first (its window closed) would leave the popover
//! pointing at freed memory.
//!
//! It is attached to the [`SizeBin`] wrapping the widget, not to the native widget
//! itself. A popover is laid out only when its parent calls `gtk_popover_present`
//! from its `size_allocate`, and not every widget does (`GtkScrolledWindow` does not,
//! so a popover on a `Scroll` collapsed to 0×0 once its content grew). `SizeBin`
//! does it, and has the same bounds as the widget.
//!
//! The arrow, closing on a click outside or Esc (`autohide`) and moving to the
//! opposite side when there is no room are all `GtkPopover`'s own.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::glib;
use gtk::prelude::*;
use naui_core::PopoverPosition;

use crate::bin::SizeBin;
use crate::callback::Notifier;
use crate::widgets::Widget;

struct PopoverInner {
    native: gtk::Popover,
    /// The widget it is attached to. Held while attached (see the module docs).
    anchor: RefCell<Option<SizeBin>>,
    /// The content's handle. Held to keep it alive along with its callbacks.
    child: RefCell<Option<Box<dyn Widget>>>,
    position: Cell<PopoverPosition>,
    /// Whether it is open as far as naui knows. Cleared by `close` before
    /// `popdown`, so the `closed` that follows is not passed on.
    open: Cell<bool>,
    on_close: Notifier<()>,
}

/// A panel shown beside a widget (`GtkPopover`).
///
/// It is not a widget laid out on screen, so it is not a [`Widget`].
#[derive(Clone)]
pub struct Popover(Rc<PopoverInner>);

impl Popover {
    pub(crate) fn new() -> Self {
        let native = gtk::Popover::new();
        native.set_position(gtk::PositionType::Bottom);
        let this = Self(Rc::new(PopoverInner {
            native,
            anchor: RefCell::new(None),
            child: RefCell::new(None),
            position: Cell::new(PopoverPosition::default()),
            open: Cell::new(false),
            on_close: Notifier::default(),
        }));
        let weak = Rc::downgrade(&this.0);
        this.0.native.connect_closed(move |_| {
            let Some(inner) = weak.upgrade() else {
                return;
            };
            if inner.open.replace(false) {
                inner.on_close.emit(());
            }
            // 閉じる途中で外すと GTK の後始末とぶつかるので、一巡してから外す。
            let weak = Rc::downgrade(&inner);
            glib::idle_add_local_once(move || {
                if let Some(inner) = weak.upgrade() {
                    if !inner.native.is_visible() {
                        inner.detach();
                    }
                }
            });
        });
        this
    }

    /// The content. Each call replaces it.
    pub fn set_child(&self, child: &dyn Widget) {
        let bin = child.size_bin();
        let previous = self.0.child.borrow().as_ref().map(|child| child.size_bin());
        if let Some(previous) = previous {
            if previous != bin {
                crate::interaction::clear_focus(&previous);
            }
        }
        bin.fill_parent();
        self.0.native.set_child(Some(&bin));
        *self.0.child.borrow_mut() = Some(child.boxed_clone());
    }

    /// The side it prefers to appear on. Bottom by default.
    pub fn set_position(&self, position: PopoverPosition) {
        self.0.position.set(position);
        self.0.native.set_position(position_type(position));
    }

    pub fn position(&self) -> PopoverPosition {
        self.0.position.get()
    }

    /// Shows it beside `widget`. If it is already open, it moves to `widget`.
    ///
    /// Does nothing while `widget` is not shown in a window (not mapped).
    pub fn open(&self, widget: &dyn Widget) {
        if !widget.native_widget().is_mapped() {
            return;
        }
        let target = widget.size_bin();
        let native = &self.0.native;
        if native.parent().as_ref() != Some(target.upcast_ref()) {
            if native.parent().is_some() {
                // 移すだけなので、閉じたことは知らせない。
                self.0.open.set(false);
                native.popdown();
                self.0.detach();
            }
            native.set_parent(&target);
            *self.0.anchor.borrow_mut() = Some(target);
        }
        native.popup();
        self.0.open.set(native.is_visible());
    }

    /// Closes it. `on_close` is not called. Does nothing if it is not open.
    pub fn close(&self) {
        if !self.0.open.replace(false) {
            return;
        }
        self.0.native.popdown();
    }

    /// Whether it is open.
    pub fn is_open(&self) -> bool {
        self.0.open.get()
    }

    /// Called when the user closes it (a click outside, Esc).
    ///
    /// Not called when it is closed with [`close`](Self::close). Setting it again
    /// replaces the previous closure.
    pub fn on_close(&self, f: impl FnMut() + 'static) {
        let mut f = f;
        self.0.on_close.set(move |()| f());
    }

    /// Closes it the way a click outside does, notifying `on_close` (for tests and
    /// automation).
    pub fn dismiss(&self) {
        if self.0.open.get() {
            self.0.native.popdown();
        }
    }

    /// The corresponding `GtkPopover`. Public as a backend-specific escape hatch.
    pub fn native_popover(&self) -> gtk::Popover {
        self.0.native.clone()
    }
}

impl PopoverInner {
    /// Detaches it from the widget it is attached to, and lets go of that widget.
    fn detach(&self) {
        if self.native.parent().is_some() {
            self.native.unparent();
        }
        self.anchor.borrow_mut().take();
    }
}

impl Drop for PopoverInner {
    fn drop(&mut self) {
        // GTK4 では、親を持ったまま壊すと警告が出る。
        self.detach();
    }
}

fn position_type(position: PopoverPosition) -> gtk::PositionType {
    match position {
        PopoverPosition::Top => gtk::PositionType::Top,
        PopoverPosition::Bottom => gtk::PositionType::Bottom,
        PopoverPosition::Left => gtk::PositionType::Left,
        PopoverPosition::Right => gtk::PositionType::Right,
    }
}
