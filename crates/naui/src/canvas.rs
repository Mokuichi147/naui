use crate::sys;
use naui_core::{EventResponse, Painter, PointerEvent};

/// A surface the app draws on itself (`GtkDrawingArea`).
///
/// Its size is not determined by its contents, so set it with `set_sizing` (the
/// same as `Scroll`).
#[derive(Clone)]
pub struct Canvas(pub(crate) sys::Canvas);

impl_widget!(Canvas);

impl Canvas {
    /// The closure that decides what to draw. Called with a fresh [`Painter`] every
    /// time it is redrawn.
    ///
    /// It is called after [`redraw`](Self::redraw), when the surface changes size (and
    /// when GTK asks for a redraw).
    #[inline]
    pub fn on_draw(&self, f: impl FnMut(&mut Painter) + 'static) {
        self.0.on_draw(f)
    }

    /// Requests a redraw. Nothing is drawn on the spot; `on_draw` is called at the next
    /// draw.
    #[inline]
    pub fn redraw(&self) {
        self.0.redraw()
    }

    /// Pointer (mouse, touch, pen) presses, moves and releases. Positions are logical
    /// pixels with the surface's top left as origin. Moves also arrive while nothing is
    /// pressed (hover). Presses and releases carry which button
    /// ([`PointerEvent::button`]).
    #[inline]
    pub fn on_pointer(&self, f: impl FnMut(PointerEvent) + 'static) {
        self.0.on_pointer(f)
    }

    /// The wheel (including two-finger scrolling on a trackpad). Amounts are logical
    /// pixels.
    ///
    /// Returning `Handled` keeps it from reaching the outer scroll view (and no
    /// momentum follows after the fingers lift). With `Continue`, the outer
    /// `GtkScrolledWindow` scrolls as usual.
    #[inline]
    pub fn on_wheel(&self, f: impl FnMut(PointerEvent) -> EventResponse + 'static) {
        self.0.on_wheel(f)
    }

    /// The current size of the surface (width, height). 0 if not laid out yet.
    #[inline]
    pub fn size(&self) -> (f64, f64) {
        self.0.size()
    }
}
