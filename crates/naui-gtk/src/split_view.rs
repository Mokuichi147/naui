//! A split view (`GtkPaned`).
//!
//! GTK4 has a control for the same job, so it is used as is. The divider handle,
//! following the drag, and the cursor shape are all done by `GtkPaned`.
//!
//! naui decides three things:
//!
//! - The direction. naui's `Orientation::Horizontal` means "panes side by side", so
//!   `GtkPaned` is `Horizontal` too (the same meaning as GTK4's orientation).
//! - Where leftover space goes. `resize-start-child` is off and `resize-end-child`
//!   is on, so the start side keeps its specified size and the end side takes the
//!   leftover space.
//! - The minimum sizes of both sides. Giving the `GtkPaned` children themselves a
//!   minimum would clash with the `size_request` the app specified with
//!   [`Sizing`](naui_core::Sizing), so each pane gets another container
//!   ([`SizeBin`]) wrapped around it, which holds the minimum.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::glib;
use gtk::prelude::*;
use naui_core::{clamp_split_position, Orientation, DEFAULT_SPLIT_POSITION};

use crate::bin::SizeBin;
use crate::callback::Notifier;
use crate::widgets::{impl_widget, without_signal, Widget};

struct SplitViewInner {
    native: gtk::Paned,
    bin: SizeBin,
    orientation: Orientation,
    /// The container for each pane. It holds the minimum size.
    start_pane: SizeBin,
    end_pane: SizeBin,
    /// Holds the pane's handle, keeping it alive along with its callbacks.
    start: RefCell<Option<Box<dyn Widget>>>,
    end: RefCell<Option<Box<dyn Widget>>>,
    position: Cell<f64>,
    min_start: Cell<f64>,
    min_end: Cell<f64>,
    on_resize: Notifier<f64>,
    handler: RefCell<Option<glib::SignalHandlerId>>,
}

/// A container that divides two panes with a movable divider (`GtkPaned`).
#[derive(Clone)]
pub struct SplitView(Rc<SplitViewInner>);
impl_widget!(SplitView);

impl SplitView {
    pub(crate) fn new(orientation: Orientation) -> Self {
        let native = gtk::Paned::new(if orientation.is_vertical() {
            gtk::Orientation::Vertical
        } else {
            gtk::Orientation::Horizontal
        });
        // 余りは end 側が受け取る (start は指定した大きさを保つ)。
        native.set_resize_start_child(false);
        native.set_resize_end_child(true);
        // 最小の大きさを効かせるため、どちらも最小より縮ませない。
        native.set_shrink_start_child(false);
        native.set_shrink_end_child(false);

        let start_pane = pane_container();
        let end_pane = pane_container();
        native.set_start_child(Some(&start_pane));
        native.set_end_child(Some(&end_pane));

        let bin = SizeBin::wrap(&native);
        let inner = Rc::new(SplitViewInner {
            native,
            bin,
            orientation,
            start_pane,
            end_pane,
            start: RefCell::new(None),
            end: RefCell::new(None),
            position: Cell::new(DEFAULT_SPLIT_POSITION),
            min_start: Cell::new(0.0),
            min_end: Cell::new(0.0),
            on_resize: Notifier::default(),
            handler: RefCell::new(None),
        });
        inner.native.set_position(to_px(DEFAULT_SPLIT_POSITION));

        let id = {
            let weak = Rc::downgrade(&inner);
            inner.native.connect_position_notify(move |native| {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                let position = f64::from(native.position());
                if (position - inner.position.get()).abs() < 1.0 {
                    return;
                }
                inner.position.set(position);
                inner.on_resize.emit(position);
            })
        };
        *inner.handler.borrow_mut() = Some(id);
        Self(inner)
    }

    /// The layout direction. With `Horizontal`, the panes are side by side (the divider
    /// is vertical).
    pub fn orientation(&self) -> Orientation {
        self.0.orientation
    }

    /// The left (or top) pane. Each call replaces it.
    pub fn set_start(&self, child: &dyn Widget) {
        self.0.set_pane(true, child);
    }

    /// The right (or bottom) pane. Each call replaces it.
    pub fn set_end(&self, child: &dyn Widget) {
        self.0.set_pane(false, child);
    }

    /// The current divider position (size of the start side, logical pixels).
    pub fn position(&self) -> f64 {
        self.0.position.get()
    }

    /// Moves the divider. `on_resize` is not called.
    pub fn set_position(&self, position: f64) {
        let clamped = self.0.clamp(position);
        self.0.position.set(clamped);
        without_signal(&self.0.native, &self.0.handler, || {
            self.0.native.set_position(to_px(clamped));
        });
    }

    /// Moves the divider as if the user had dragged it. Calls `on_resize`.
    pub fn drag_to(&self, position: f64) {
        let clamped = self.0.clamp(position);
        self.set_position(clamped);
        self.0.on_resize.emit(clamped);
    }

    /// The minimum sizes of the two panes. Both default to 0.
    pub fn set_min_sizes(&self, start: f64, end: f64) {
        self.0.min_start.set(start.max(0.0));
        self.0.min_end.set(end.max(0.0));
        self.0.apply_min_sizes();
        // いまの位置が範囲の外なら押し戻す (通知はしない)。
        self.set_position(self.0.position.get());
    }

    /// Called with the new position every time the user moves the divider.
    pub fn on_resize(&self, f: impl FnMut(f64) + 'static) {
        self.0.on_resize.set(f);
    }

    /// The corresponding `GtkPaned`. Public as a backend-specific escape hatch.
    pub fn native_paned(&self) -> gtk::Paned {
        self.0.native.clone()
    }
}

impl SplitViewInner {
    fn set_pane(&self, is_start: bool, child: &dyn Widget) {
        let container = if is_start {
            &self.start_pane
        } else {
            &self.end_pane
        };
        while let Some(previous) = container.first_child() {
            previous.unparent();
        }
        let bin = child.size_bin();
        // 区画の中身は、配られた場所いっぱいに置く (`Scroll` と同じ扱い)。
        bin.fill_parent();
        bin.set_parent(container);

        let slot = if is_start { &self.start } else { &self.end };
        *slot.borrow_mut() = Some(child.boxed_clone());
    }

    /// The size the two panes can share (excluding the divider). 0 if still 0.
    ///
    /// CSS decides the thickness of a `GtkPaned`'s divider, so instead of subtracting,
    /// the sum of the sizes actually allotted to the panes is used.
    fn total(&self) -> f64 {
        let total = if self.orientation.is_vertical() {
            self.start_pane.height() + self.end_pane.height()
        } else {
            self.start_pane.width() + self.end_pane.width()
        };
        f64::from(total.max(0))
    }

    fn clamp(&self, position: f64) -> f64 {
        clamp_split_position(
            position,
            self.total(),
            self.min_start.get(),
            self.min_end.get(),
        )
    }

    /// Passes the minimum sizes as the `size_request` of the pane containers.
    fn apply_min_sizes(&self) {
        let (start, end) = (to_px(self.min_start.get()), to_px(self.min_end.get()));
        if self.orientation.is_vertical() {
            self.start_pane.set_size_request(-1, start);
            self.end_pane.set_size_request(-1, end);
        } else {
            self.start_pane.set_size_request(start, -1);
            self.end_pane.set_size_request(end, -1);
        }
    }
}

/// The container for one pane. Wrapped around it to hold the minimum size.
fn pane_container() -> SizeBin {
    let container: SizeBin = glib::Object::new();
    container.set_halign(gtk::Align::Fill);
    container.set_valign(gtk::Align::Fill);
    container.set_hexpand(true);
    container.set_vexpand(true);
    container
}

fn to_px(value: f64) -> i32 {
    value.round().clamp(0.0, f64::from(i32::MAX)) as i32
}
