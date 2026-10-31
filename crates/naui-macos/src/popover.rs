//! Popovers (AppKit's `NSPopover`).
//!
//! The content widget is put into a plain `NSView` with a margin around it, which
//! becomes the view of the popover's `contentViewController`. The popover's size
//! follows that view's constraints, so content that grows while it is shown (an
//! `Expander` being opened) grows the popover too.
//!
//! The arrow, the material, closing on a click outside or Esc
//! (`NSPopoverBehaviorTransient`) and moving to the opposite side when there is no
//! room are all `NSPopover`'s own. naui only chooses the preferred edge.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use naui_core::PopoverPosition;
use objc2::rc::Retained;
use objc2::runtime::{NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{define_class, msg_send, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSLayoutConstraint, NSPopover, NSPopoverBehavior, NSPopoverDelegate, NSView, NSViewController,
};
use objc2_foundation::{NSArray, NSNotification, NSRect, NSRectEdge};

use crate::trampoline::ValueHandler;
use crate::widgets::Widget;

/// The margin between the popover's edge and the content.
const PADDING: f64 = 12.0;

/// The width given to content that cannot decide its own width (a text field on its
/// own). About the same as `Dialog`'s.
const FALLBACK_WIDTH: f64 = 260.0;

define_class!(
    /// Receives `popoverDidClose:` and hands it to the closure.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "NauiPopoverObserver"]
    #[ivars = RefCell<Box<dyn FnMut()>>]
    struct PopoverObserver;

    unsafe impl NSObjectProtocol for PopoverObserver {}

    unsafe impl NSPopoverDelegate for PopoverObserver {
        #[unsafe(method(popoverDidClose:))]
        fn popover_did_close(&self, _notification: &NSNotification) {
            let mut f = self.ivars().borrow_mut();
            (f)();
        }
    }
);

impl PopoverObserver {
    fn new(mtm: MainThreadMarker, f: impl FnMut() + 'static) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(RefCell::new(Box::new(f) as Box<dyn FnMut()>));
        unsafe { msg_send![super(this), init] }
    }
}

struct PopoverInner {
    native: Retained<NSPopover>,
    /// The `contentViewController`'s view. The content goes inside it.
    container: Retained<NSView>,
    /// The content's handle. Held to keep it alive along with its trampolines.
    child: RefCell<Option<Box<dyn Widget>>>,
    /// The constraints placing the content inside the container.
    child_constraints: RefCell<Vec<Retained<NSLayoutConstraint>>>,
    /// The width given to content that cannot decide its own. Set on each open.
    fallback_width: RefCell<Option<Retained<NSLayoutConstraint>>>,
    /// The handle of the widget it is shown beside. `NSPopover` does not hold it.
    anchor: RefCell<Option<Box<dyn Widget>>>,
    position: Cell<PopoverPosition>,
    /// Whether it is open as far as naui knows. Cleared by `close` right away, and by
    /// `popoverDidClose:` when the user closed it.
    open: Cell<bool>,
    handler: ValueHandler<()>,
    /// `NSPopover`'s delegate is weak, so it is kept alive here.
    observer: RefCell<Option<Retained<PopoverObserver>>>,
}

/// A panel shown beside a widget (`NSPopover`).
///
/// It is not a widget laid out on screen, so it is not a [`Widget`]. [`crate::Ui`]
/// holds the popovers it creates, so dropping the return value does not stop
/// notifications.
#[derive(Clone)]
pub struct Popover(Rc<PopoverInner>);

impl Popover {
    pub(crate) fn new(mtm: MainThreadMarker) -> Self {
        let native = NSPopover::new(mtm);
        // 外側を押す・Esc で閉じる。メニューやほかのポップオーバーと同じ扱い。
        native.setBehavior(NSPopoverBehavior::Transient);
        let container = NSView::new(mtm);
        let controller = NSViewController::new(mtm);
        controller.setView(&container);
        native.setContentViewController(Some(&controller));

        let this = Self(Rc::new(PopoverInner {
            native,
            container,
            child: RefCell::new(None),
            child_constraints: RefCell::new(Vec::new()),
            fallback_width: RefCell::new(None),
            anchor: RefCell::new(None),
            position: Cell::new(PopoverPosition::default()),
            open: Cell::new(false),
            handler: ValueHandler::default(),
            observer: RefCell::new(None),
        }));

        let observer = PopoverObserver::new(mtm, {
            let weak = Rc::downgrade(&this.0);
            move || {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                // 閉じる動きの途中で開き直したときは、もう出ている。
                if inner.native.isShown() {
                    return;
                }
                inner.anchor.borrow_mut().take();
                // `close` が先に下ろしていれば、アプリが閉じたので知らせない。
                if inner.open.replace(false) {
                    inner.handler.emit(());
                }
            }
        });
        this.0
            .native
            .setDelegate(Some(ProtocolObject::from_ref(&*observer)));
        *this.0.observer.borrow_mut() = Some(observer);
        this
    }

    /// The content. Each call replaces it.
    pub fn set_child(&self, child: &dyn Widget) {
        let container = &self.0.container;
        if let Some(previous) = self.0.child.borrow_mut().take() {
            previous.native_view().removeFromSuperview();
        }
        let mut constraints = self.0.child_constraints.borrow_mut();
        if !constraints.is_empty() {
            NSLayoutConstraint::deactivateConstraints(&NSArray::from_retained_slice(&constraints));
            constraints.clear();
        }

        let view = child.native_view();
        crate::layout::prepare_child(&view);
        container.addSubview(&view);
        constraints.extend([
            view.leadingAnchor()
                .constraintEqualToAnchor_constant(&container.leadingAnchor(), PADDING),
            container
                .trailingAnchor()
                .constraintEqualToAnchor_constant(&view.trailingAnchor(), PADDING),
            view.topAnchor()
                .constraintEqualToAnchor_constant(&container.topAnchor(), PADDING),
            container
                .bottomAnchor()
                .constraintEqualToAnchor_constant(&view.bottomAnchor(), PADDING),
        ]);
        NSLayoutConstraint::activateConstraints(&NSArray::from_retained_slice(&constraints));
        drop(constraints);
        *self.0.child.borrow_mut() = Some(child.boxed_clone());
    }

    /// The side it prefers to appear on. Bottom by default.
    pub fn set_position(&self, position: PopoverPosition) {
        self.0.position.set(position);
        // 出ている間に変えたら、同じ場所で出し直して向きを変える。
        if self.0.open.get() {
            if let Some(anchor) = self.0.anchor.borrow().as_deref() {
                self.show_beside(&anchor.native_view());
            }
        }
    }

    pub fn position(&self) -> PopoverPosition {
        self.0.position.get()
    }

    /// Shows it beside `widget`. If it is already open, it moves to `widget`.
    ///
    /// Does nothing if `widget` is not in a window that is on screen (`NSPopover`
    /// needs somewhere to point at).
    pub fn open(&self, widget: &dyn Widget) {
        let view = widget.native_view();
        let Some(window) = view.window() else {
            return;
        };
        if !window.isVisible() {
            return;
        }
        self.apply_fallback_width();
        self.show_beside(&view);
        if self.0.native.isShown() {
            self.0.open.set(true);
            *self.0.anchor.borrow_mut() = Some(widget.boxed_clone());
        }
    }

    fn show_beside(&self, view: &NSView) {
        let edge = edge_for(self.0.position.get(), view.isFlipped());
        // 空の矩形は view の bounds と同じ扱い (NSPopover の約束)。
        self.0
            .native
            .showRelativeToRect_ofView_preferredEdge(NSRect::ZERO, view, edge);
    }

    /// Gives content that cannot decide its own width a width to show at.
    fn apply_fallback_width(&self) {
        if let Some(previous) = self.0.fallback_width.borrow_mut().take() {
            previous.setActive(false);
        }
        let Some(child) = self.0.child.borrow().as_ref().map(|c| c.native_view()) else {
            return;
        };
        if child.fittingSize().width > 0.0 {
            return;
        }
        let constraint = child
            .widthAnchor()
            .constraintGreaterThanOrEqualToConstant(FALLBACK_WIDTH);
        constraint.setActive(true);
        *self.0.fallback_width.borrow_mut() = Some(constraint);
    }

    /// Closes it. `on_close` is not called. Does nothing if it is not open.
    pub fn close(&self) {
        if !self.0.open.replace(false) {
            return;
        }
        self.0.anchor.borrow_mut().take();
        self.0.native.close();
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
        self.0.handler.set(move |()| f());
    }

    /// Closes it the way a click outside does, notifying `on_close` (for tests and
    /// automation).
    pub fn dismiss(&self) {
        if self.0.open.get() {
            unsafe { self.0.native.performClose(None) };
        }
    }

    /// The real `NSPopover`. Public as a backend-specific escape hatch.
    pub fn native_popover(&self) -> Retained<NSPopover> {
        self.0.native.clone()
    }
}

/// The edge of the anchor view to prefer. `NSPopover` reads it in the view's own
/// coordinates, so top and bottom swap for views that are not flipped.
fn edge_for(position: PopoverPosition, flipped: bool) -> NSRectEdge {
    match position {
        PopoverPosition::Left => NSRectEdge::MinX,
        PopoverPosition::Right => NSRectEdge::MaxX,
        PopoverPosition::Top if flipped => NSRectEdge::MinY,
        PopoverPosition::Top => NSRectEdge::MaxY,
        PopoverPosition::Bottom if flipped => NSRectEdge::MaxY,
        PopoverPosition::Bottom => NSRectEdge::MinY,
    }
}
