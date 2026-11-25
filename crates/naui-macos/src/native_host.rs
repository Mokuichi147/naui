//! A place for an `NSView` made outside naui (an `NSView` subclass of naui's own).
//!
//! The widget's view is a plain `NSView` of naui's, and the hosted view goes inside
//! it, pinned to its four edges with Auto Layout. So naui's size settings, the
//! container's constraints and visibility all apply to naui's view, and the hosted
//! view only follows it. Its own constraints and intrinsic size still pass through
//! the pins: a view that knows its size (a control) makes the host that size, and
//! one that does not (a view a GPU surface draws into) gets the size naui's layout
//! gives.
//!
//! The size is reported from `setFrameSize:`, the scale (`backingScaleFactor`) from
//! `viewDidChangeBackingProperties` and `viewDidMoveToWindow`, and joining or
//! leaving a window from `viewDidMoveToWindow`.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use naui_core::{HostEvents, HostSize, Sizing};
use objc2::rc::Retained;
use objc2::runtime::NSObjectProtocol;
use objc2::{define_class, msg_send, DefinedClass, MainThreadMarker, MainThreadOnly, Message};
use objc2_app_kit::{NSLayoutConstraint, NSScreen, NSView, NSViewNoIntrinsicMetric};
use objc2_foundation::{NSArray, NSSize};

use crate::widgets::Widget;

define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "NauiNativeHostView"]
    #[ivars = HostEvents]
    /// naui's view that the hosted view goes into. Reports its size, scale and
    /// window.
    struct HostView;

    unsafe impl NSObjectProtocol for HostView {}

    impl HostView {
        #[unsafe(method(setFrameSize:))]
        fn set_frame_size(&self, size: NSSize) {
            let _: () = unsafe { msg_send![super(self), setFrameSize: size] };
            self.report_size();
        }

        #[unsafe(method(viewDidChangeBackingProperties))]
        fn view_did_change_backing_properties(&self) {
            let _: () = unsafe { msg_send![super(self), viewDidChangeBackingProperties] };
            self.report_size();
        }

        #[unsafe(method(viewDidMoveToWindow))]
        fn view_did_move_to_window(&self) {
            let _: () = unsafe { msg_send![super(self), viewDidMoveToWindow] };
            self.ivars().report_attached(self.window().is_some());
            self.report_size();
        }
    }
);

impl HostView {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(HostEvents::default());
        unsafe { msg_send![super(this), init] }
    }

    /// The scale of the window it is in, or of the main display before it is in one.
    fn scale(&self) -> f64 {
        if let Some(window) = self.window() {
            return window.backingScaleFactor();
        }
        NSScreen::mainScreen(self.mtm()).map_or(1.0, |screen| screen.backingScaleFactor())
    }

    fn current_size(&self) -> HostSize {
        let size = self.frame().size;
        HostSize::new(size.width, size.height, self.scale())
    }

    fn report_size(&self) {
        self.ivars().report_size(self.current_size());
    }
}

/// The hosted view and the constraints pinning it to naui's view.
type Hosted = (Retained<NSView>, Vec<Retained<NSLayoutConstraint>>);

struct NativeHostInner {
    native: Retained<NSView>,
    /// The same object as `native`.
    view: Retained<HostView>,
    /// The hosted view, with the constraints pinning it.
    hosted: RefCell<Option<Hosted>>,
    /// Whether the app has set a size. Until it has, each axis follows the hosted
    /// view: `Auto`-like where the view has an intrinsic size, `Fill`-like where it
    /// has none (the same as `Canvas`).
    sized: Cell<bool>,
}

impl Drop for NativeHostInner {
    /// Lets go of the hosted view, taking it out of naui's view if it is still there.
    fn drop(&mut self) {
        if let Some((view, _)) = self.hosted.get_mut().take() {
            if crate::layout::is_placed_in(&view, &self.native) {
                view.removeFromSuperview();
            }
        }
    }
}

/// A place for an `NSView` made outside naui.
///
/// Put the view in with [`set_hosted_view`](Self::set_hosted_view). naui places,
/// sizes and shows naui's own view around it, and reports its size and scale
/// ([`on_resize`](Self::on_resize)) and when it joins or leaves a window
/// ([`on_attach`](Self::on_attach)); what the view draws is up to whoever made it.
#[derive(Clone)]
pub struct NativeHost(Rc<NativeHostInner>);

impl Widget for NativeHost {
    fn native_view(&self) -> Retained<NSView> {
        self.0.native.clone()
    }
    fn boxed_clone(&self) -> Box<dyn Widget> {
        Box::new(self.clone())
    }
}

crate::widgets::impl_interaction!(NativeHost);

impl NativeHost {
    pub(crate) fn new(mtm: MainThreadMarker) -> Self {
        let view = HostView::new(mtm);
        let native = view.clone().into_super();
        // 中身が無いうちは `Canvas` と同じく余りを受け取る側に回る。
        crate::layout::relax_hugging(&native);
        Self(Rc::new(NativeHostInner {
            native,
            view,
            hosted: RefCell::new(None),
            sized: Cell::new(false),
        }))
    }

    /// Sets the size. Each call discards the previous setting.
    ///
    /// The actual size is decided by Auto Layout, so what you pass here is only
    /// constraints such as "fix it" or "take the parent's leftover space". The hosted
    /// view's own constraints still apply on top of these.
    ///
    /// It can be called before or after putting the widget into a container.
    pub fn set_sizing(&self, sizing: Sizing) {
        self.0.sized.set(true);
        crate::layout::apply_sizing(&self.0.native, sizing);
    }

    /// Puts `view` in the host, replacing the view there before (which is taken out
    /// of the host and let go). `None` empties the host. Returns `true`.
    ///
    /// naui pins `view` to the host's four edges with Auto Layout, so it turns off
    /// the view's `translatesAutoresizingMaskIntoConstraints`. A view that is in
    /// another view is moved out of it, as `addSubview:` does. naui keeps a reference
    /// to `view` until it is replaced or the host is dropped; when either happens,
    /// the view is taken out of the host if it is still there.
    pub fn set_hosted_view(&self, view: Option<&NSView>) -> bool {
        let previous = self.0.hosted.borrow_mut().take();
        if let Some((previous, constraints)) = previous {
            if view.is_some_and(|view| std::ptr::eq(view, &*previous)) {
                *self.0.hosted.borrow_mut() = Some((previous, constraints));
                return true;
            }
            if crate::layout::is_placed_in(&previous, &self.0.native) {
                previous.removeFromSuperview();
            } else {
                NSLayoutConstraint::deactivateConstraints(&NSArray::from_retained_slice(
                    &constraints,
                ));
            }
        }
        let Some(view) = view else {
            self.follow_hosted(None);
            return true;
        };
        let host = &self.0.native;
        view.setTranslatesAutoresizingMaskIntoConstraints(false);
        host.addSubview(view);
        let constraints = vec![
            view.leadingAnchor()
                .constraintEqualToAnchor(&host.leadingAnchor()),
            view.trailingAnchor()
                .constraintEqualToAnchor(&host.trailingAnchor()),
            view.topAnchor().constraintEqualToAnchor(&host.topAnchor()),
            view.bottomAnchor()
                .constraintEqualToAnchor(&host.bottomAnchor()),
        ];
        NSLayoutConstraint::activateConstraints(&NSArray::from_retained_slice(&constraints));
        *self.0.hosted.borrow_mut() = Some((view.retain(), constraints));
        self.follow_hosted(Some(view));
        crate::layout::invalidate_ancestors(host);
        true
    }

    /// The view put in with [`set_hosted_view`](Self::set_hosted_view), if any.
    pub fn hosted_view(&self) -> Option<Retained<NSView>> {
        self.0
            .hosted
            .borrow()
            .as_ref()
            .map(|(view, _)| view.clone())
    }

    /// Until the app sets a size, makes each axis hug where the hosted view has an
    /// intrinsic size and take leftover space where it has none.
    fn follow_hosted(&self, view: Option<&NSView>) {
        if self.0.sized.get() {
            return;
        }
        let intrinsic = view.map(|view| view.intrinsicContentSize());
        let none = unsafe { NSViewNoIntrinsicMetric };
        for horizontal in [true, false] {
            let has_size = intrinsic.is_some_and(|size| {
                let value = if horizontal { size.width } else { size.height };
                value != none
            });
            crate::layout::set_axis_fill(&self.0.native, horizontal, !has_size);
        }
    }

    /// The current size (width, height) in points. 0 if not laid out yet.
    pub fn size(&self) -> (f64, f64) {
        let size = self.0.view.frame().size;
        (size.width, size.height)
    }

    /// The window's `backingScaleFactor`. Before the host is in a window, the main
    /// display's.
    pub fn scale_factor(&self) -> f64 {
        self.0.view.scale()
    }

    /// Called with the new size and scale when either changes (after `setFrameSize:`,
    /// `viewDidChangeBackingProperties` and `viewDidMoveToWindow`). It runs inside
    /// AppKit's layout pass.
    pub fn on_resize(&self, f: impl FnMut(HostSize) + 'static) {
        self.0.view.ivars().on_resize(f);
    }

    /// Called with `true` when the host joins a window's view tree and `false` when
    /// it leaves it (`viewDidMoveToWindow`). Joining happens when it is put into a
    /// container that is already in a window, or when its container is set as a
    /// window's content, whether or not the window is shown yet.
    pub fn on_attach(&self, f: impl FnMut(bool) + 'static) {
        self.0.view.ivars().on_attach(f);
    }

    /// Whether the host is in a window's view tree now.
    pub fn is_attached(&self) -> bool {
        self.0.view.window().is_some()
    }
}
