use crate::sys;
use crate::Widget;
use naui_core::Orientation;

/// A container that divides two panes with a movable divider (`GtkPaned`).
#[derive(Clone)]
pub struct SplitView(pub(crate) sys::SplitView);

impl_widget!(SplitView);

impl SplitView {
    /// The layout direction. With `Horizontal`, the panes are side by side (the divider
    /// is vertical).
    #[inline]
    pub fn orientation(&self) -> Orientation {
        self.0.orientation()
    }

    /// The left (or top) pane. Each call replaces it.
    #[inline]
    pub fn set_start(&self, child: &dyn Widget) {
        self.0.set_start(child.as_backend())
    }

    /// The right (or bottom) pane. Each call replaces it.
    #[inline]
    pub fn set_end(&self, child: &dyn Widget) {
        self.0.set_end(child.as_backend())
    }

    /// The current divider position (size of the start side, logical pixels).
    #[inline]
    pub fn position(&self) -> f64 {
        self.0.position()
    }

    /// Moves the divider. `on_resize` is not called.
    #[inline]
    pub fn set_position(&self, position: f64) {
        self.0.set_position(position)
    }

    /// Moves the divider as if the user had dragged it. Calls `on_resize`.
    #[inline]
    pub fn drag_to(&self, position: f64) {
        self.0.drag_to(position)
    }

    /// The minimum sizes of the two panes. Both default to 0.
    #[inline]
    pub fn set_min_sizes(&self, start: f64, end: f64) {
        self.0.set_min_sizes(start, end)
    }

    /// Called with the new position every time the user moves the divider.
    #[inline]
    pub fn on_resize(&self, f: impl FnMut(f64) + 'static) {
        self.0.on_resize(f)
    }
}
