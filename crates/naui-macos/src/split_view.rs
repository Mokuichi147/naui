//! A split view (`NSSplitView`).
//!
//! AppKit has a control for the same job, so it is used as is. Drawing the divider,
//! following the drag and the cursor shape (⇔) are all done by `NSSplitView`.
//!
//! naui decides only these three things, all passed as the delegate:
//!
//! - The direction (`setVertical`). naui's `Orientation::Horizontal` means "panes
//!   side by side" = a vertical divider, so it is `setVertical(true)`.
//! - How panes are sized when the size changes
//!   (`splitView:resizeSubviewsWithOldSize:`). The start side keeps its specified
//!   size, and the end side takes the leftover. `NSSplitView` distributes **by
//!   proportion** by default, so this replaces it with "keep the sidebar's width".
//! - The range the divider can move in (`splitView:constrainMinCoordinate:…` and
//!   `…constrainMaxCoordinate:…`).
//!
//! naui places the panes' frames directly, so the pane views **do not turn off**
//! `translatesAutoresizingMaskIntoConstraints` (unlike other containers). The
//! pane's content is laid out by Auto Layout within that frame.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use naui_core::{clamp_split_position, Orientation, DEFAULT_SPLIT_POSITION};
use objc2::rc::Retained;
use objc2::runtime::{NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{define_class, msg_send, DefinedClass, MainThreadMarker, MainThreadOnly, Message};
use objc2_app_kit::{
    NSAutoresizingMaskOptions, NSSplitView, NSSplitViewDelegate, NSSplitViewDividerStyle, NSView,
};
use objc2_foundation::{NSInteger, NSNotification, NSPoint, NSRect, NSSize};

use crate::trampoline::ValueHandler;
use crate::widgets::{impl_widget, Widget};

struct SplitViewInner {
    native: Retained<NSSplitView>,
    orientation: Orientation,
    /// The pane's handle. Held to keep it alive along with its trampolines.
    start: RefCell<Option<Box<dyn Widget>>>,
    end: RefCell<Option<Box<dyn Widget>>>,
    /// The divider position the app specified (the size of the start side).
    ///
    /// When it is too narrow to fit, it is **shown** clamped to the end, but this value
    /// is kept as is, so that it returns to the original position when it widens.
    position: Cell<f64>,
    min_start: Cell<f64>,
    min_end: Cell<f64>,
    handler: ValueHandler<f64>,
    /// The delegate is held weakly, so it is kept alive here.
    delegate: RefCell<Option<Retained<SplitObserver>>>,
}

/// A container that divides two panes with a movable divider (`NSSplitView`).
#[derive(Clone)]
pub struct SplitView(Rc<SplitViewInner>);
impl_widget!(SplitView);

impl SplitView {
    pub(crate) fn new(mtm: MainThreadMarker, orientation: Orientation) -> Self {
        let native = NSSplitView::new(mtm);
        // naui の Horizontal は「区画が横に並ぶ」= 仕切りは縦。
        native.setVertical(!orientation.is_vertical());
        // 2 区画の分割は、macOS の作法どおり細い仕切り。
        native.setDividerStyle(NSSplitViewDividerStyle::Thin);

        let this = Self(Rc::new(SplitViewInner {
            native,
            orientation,
            start: RefCell::new(None),
            end: RefCell::new(None),
            position: Cell::new(DEFAULT_SPLIT_POSITION),
            min_start: Cell::new(0.0),
            min_end: Cell::new(0.0),
            handler: ValueHandler::default(),
            delegate: RefCell::new(None),
        }));

        // 中継はハンドルと同じ寿命で持つ。通知の中から位置を変えても、
        // 実行中の中継そのものを解放しないようにするため。
        let delegate = SplitObserver::new(mtm, Rc::downgrade(&this.0));
        this.0
            .native
            .setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
        *this.0.delegate.borrow_mut() = Some(delegate);
        this
    }

    /// The layout direction. With `Horizontal`, the panes are side by side (the divider
    /// is vertical).
    pub fn orientation(&self) -> Orientation {
        self.0.orientation
    }

    /// The left (or top) pane. Each call replaces it.
    pub fn set_start(&self, child: &dyn Widget) {
        self.0.set_pane(0, child);
    }

    /// The right (or bottom) pane. Each call replaces it.
    pub fn set_end(&self, child: &dyn Widget) {
        self.0.set_pane(1, child);
    }

    /// The current divider position (size of the start side, logical pixels).
    pub fn position(&self) -> f64 {
        self.0.position.get()
    }

    /// Moves the divider. `on_resize` is not called.
    pub fn set_position(&self, position: f64) {
        self.0.position.set(self.0.clamp(position));
        self.0.relayout();
    }

    /// Moves the divider as if the user had dragged it. Calls `on_resize`.
    pub fn drag_to(&self, position: f64) {
        let clamped = self.0.clamp(position);
        self.0.position.set(clamped);
        self.0.relayout();
        self.0.handler.emit(clamped);
    }

    /// The minimum sizes of the two panes. Both default to 0.
    pub fn set_min_sizes(&self, start: f64, end: f64) {
        self.0.min_start.set(start.max(0.0));
        self.0.min_end.set(end.max(0.0));
        // いまの位置が範囲の外なら押し戻す (通知はしない)。
        self.0.position.set(self.0.clamp(self.0.position.get()));
        self.0.relayout();
    }

    /// Called with the new position every time the user moves the divider.
    pub fn on_resize(&self, f: impl FnMut(f64) + 'static) {
        self.0.handler.set(f);
    }

    /// The corresponding `NSSplitView`. Public as a backend-specific escape hatch.
    pub fn native_split_view(&self) -> Retained<NSSplitView> {
        self.0.native.clone()
    }
}

impl SplitViewInner {
    /// Replaces a pane. `index` 0 is start, 1 is end.
    fn set_pane(&self, index: usize, child: &dyn Widget) {
        let slot = if index == 0 { &self.start } else { &self.end };
        if let Some(previous) = slot.borrow_mut().take() {
            let view = previous.native_view();
            self.native.removeArrangedSubview(&view);
            view.removeFromSuperview();
        }

        let view = child.native_view();
        // 区画の frame は naui が置くので、Auto Layout には任せない。
        view.setTranslatesAutoresizingMaskIntoConstraints(true);
        view.setAutoresizingMask(NSAutoresizingMaskOptions::empty());
        self.native
            .insertArrangedSubview_atIndex(&view, index as NSInteger);
        *slot.borrow_mut() = Some(child.boxed_clone());
        self.relayout();
    }

    /// The size the two panes can share (excluding the divider). 0 if still 0.
    fn total(&self) -> f64 {
        let size = self.native.frame().size;
        let length = if self.orientation.is_vertical() {
            size.height
        } else {
            size.width
        };
        if length <= 0.0 {
            return 0.0;
        }
        (length - self.native.dividerThickness()).max(0.0)
    }

    /// Clamps the position to the current size and the minimum settings.
    fn clamp(&self, position: f64) -> f64 {
        clamp_split_position(
            position,
            self.total(),
            self.min_start.get(),
            self.min_end.get(),
        )
    }

    /// The size of the start side currently on screen.
    fn measured_position(&self) -> Option<f64> {
        let arranged = self.native.arrangedSubviews();
        if arranged.len() < 2 {
            return None;
        }
        let size = arranged.objectAtIndex(0).frame().size;
        Some(if self.orientation.is_vertical() {
            size.height
        } else {
            size.width
        })
    }

    fn relayout(&self) {
        self.native.adjustSubviews();
    }

    /// Places the panes' frames (the body of `splitView:resizeSubviewsWithOldSize:`).
    fn resize_subviews(&self) {
        let arranged = self.native.arrangedSubviews();
        if arranged.len() < 2 {
            return;
        }
        let size = self.native.frame().size;
        let thickness = self.native.dividerThickness();
        let position = self.clamp(self.position.get());
        let (start, end) = if self.orientation.is_vertical() {
            let rest = (size.height - thickness - position).max(0.0);
            (
                NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(size.width, position)),
                NSRect::new(
                    NSPoint::new(0.0, position + thickness),
                    NSSize::new(size.width, rest),
                ),
            )
        } else {
            let rest = (size.width - thickness - position).max(0.0);
            (
                NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(position, size.height)),
                NSRect::new(
                    NSPoint::new(position + thickness, 0.0),
                    NSSize::new(rest, size.height),
                ),
            )
        };
        arranged.objectAtIndex(0).setFrame(start);
        arranged.objectAtIndex(1).setFrame(end);
    }

    /// Handling when the `NSSplitView` has finished resizing the panes.
    ///
    /// It arrives both for the user's drags and when naui repositions it. **If they
    /// are placed exactly at the currently specified position, it was naui's doing**,
    /// so only otherwise is it taken as "the user moved it".
    fn did_resize(&self) {
        if self.total() <= 0.0 {
            return;
        }
        let Some(measured) = self.measured_position() else {
            return;
        };
        // 1 ピクセル未満のずれは AppKit の丸めなので拾わない。
        if (measured - self.clamp(self.position.get())).abs() < 1.0 {
            return;
        }
        self.position.set(measured);
        self.handler.emit(measured);
    }
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "NauiSplitObserver"]
    #[ivars = Weak<SplitViewInner>]
    /// The delegate responsible for sizing the panes, the movable range, and move
    /// notifications.
    struct SplitObserver;

    unsafe impl NSObjectProtocol for SplitObserver {}

    unsafe impl NSSplitViewDelegate for SplitObserver {
        #[unsafe(method(splitView:resizeSubviewsWithOldSize:))]
        fn resize_subviews(&self, _split_view: &NSSplitView, _old_size: NSSize) {
            if let Some(inner) = self.ivars().upgrade() {
                inner.resize_subviews();
            }
        }

        #[unsafe(method(splitView:constrainMinCoordinate:ofSubviewAt:))]
        fn constrain_min(
            &self,
            _split_view: &NSSplitView,
            proposed: f64,
            _divider_index: NSInteger,
        ) -> f64 {
            match self.ivars().upgrade() {
                Some(inner) => proposed.max(inner.min_start.get()),
                None => proposed,
            }
        }

        #[unsafe(method(splitView:constrainMaxCoordinate:ofSubviewAt:))]
        fn constrain_max(
            &self,
            _split_view: &NSSplitView,
            proposed: f64,
            _divider_index: NSInteger,
        ) -> f64 {
            let Some(inner) = self.ivars().upgrade() else {
                return proposed;
            };
            let total = inner.total();
            if total <= 0.0 {
                return proposed;
            }
            proposed.min((total - inner.min_end.get()).max(inner.min_start.get()))
        }

        #[unsafe(method(splitViewDidResizeSubviews:))]
        fn did_resize_subviews(&self, _notification: &NSNotification) {
            if let Some(inner) = self.ivars().upgrade() {
                inner.did_resize();
            }
        }
    }
);

impl SplitObserver {
    fn new(mtm: MainThreadMarker, inner: Weak<SplitViewInner>) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(inner);
        unsafe { msg_send![super(this), init] }
    }
}
