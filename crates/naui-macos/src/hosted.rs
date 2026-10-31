//! A widget hosted in a plain view that is not a naui container.
//!
//! Popovers and sheets put their content into a view of their own, with a margin
//! around it, and size themselves from that view's constraints. Both need the same
//! handling: replacing the content, keeping its handle alive, and giving content
//! that cannot decide its own width (a lone text field) a width to show at.

use std::cell::RefCell;

use objc2::rc::Retained;
use objc2::MainThreadMarker;
use objc2_app_kit::{NSLayoutConstraint, NSView};
use objc2_foundation::NSArray;

use crate::widgets::Widget;

/// The width given to content that cannot decide its own width. About the same as
/// `Dialog`'s (`NSAlert`'s body text area).
pub(crate) const FALLBACK_WIDTH: f64 = 260.0;

/// The content of a popover or sheet, placed inside `container` with a margin.
pub(crate) struct HostedChild {
    container: Retained<NSView>,
    padding: f64,
    /// The content's handle. Held to keep it alive along with its trampolines.
    child: RefCell<Option<Box<dyn Widget>>>,
    /// The constraints placing the content inside the container.
    constraints: RefCell<Vec<Retained<NSLayoutConstraint>>>,
    /// The width given to content that cannot decide its own.
    fallback_width: RefCell<Option<Retained<NSLayoutConstraint>>>,
}

impl HostedChild {
    /// An empty `NSView` to hold the content, with `padding` around it.
    pub(crate) fn new(mtm: MainThreadMarker, padding: f64) -> Self {
        Self {
            container: NSView::new(mtm),
            padding,
            child: RefCell::new(None),
            constraints: RefCell::new(Vec::new()),
            fallback_width: RefCell::new(None),
        }
    }

    /// The view holding the content.
    pub(crate) fn container(&self) -> &Retained<NSView> {
        &self.container
    }

    /// Replaces the content.
    pub(crate) fn set(&self, child: &dyn Widget) {
        let container = &self.container;
        if let Some(previous) = self.child.borrow_mut().take() {
            previous.native_view().removeFromSuperview();
        }
        let mut constraints = self.constraints.borrow_mut();
        if !constraints.is_empty() {
            NSLayoutConstraint::deactivateConstraints(&NSArray::from_retained_slice(&constraints));
            constraints.clear();
        }

        let view = child.native_view();
        crate::layout::prepare_child(&view);
        container.addSubview(&view);
        let padding = self.padding;
        constraints.extend([
            view.leadingAnchor()
                .constraintEqualToAnchor_constant(&container.leadingAnchor(), padding),
            container
                .trailingAnchor()
                .constraintEqualToAnchor_constant(&view.trailingAnchor(), padding),
            view.topAnchor()
                .constraintEqualToAnchor_constant(&container.topAnchor(), padding),
            container
                .bottomAnchor()
                .constraintEqualToAnchor_constant(&view.bottomAnchor(), padding),
        ]);
        NSLayoutConstraint::activateConstraints(&NSArray::from_retained_slice(&constraints));
        drop(constraints);
        *self.child.borrow_mut() = Some(child.boxed_clone());
        // 出ている間に差し替えたときも、新しい中身で幅を決め直す。
        self.apply_fallback_width();
    }

    /// Gives content that cannot decide its own width a width to show at. Called
    /// again on each open, since the content may have changed since.
    pub(crate) fn apply_fallback_width(&self) {
        if let Some(previous) = self.fallback_width.borrow_mut().take() {
            previous.setActive(false);
        }
        let Some(child) = self.child.borrow().as_ref().map(|c| c.native_view()) else {
            return;
        };
        if child.fittingSize().width > 0.0 {
            return;
        }
        let constraint = child
            .widthAnchor()
            .constraintGreaterThanOrEqualToConstant(FALLBACK_WIDTH);
        constraint.setActive(true);
        *self.fallback_width.borrow_mut() = Some(constraint);
    }
}
