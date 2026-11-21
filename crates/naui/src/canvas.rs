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
    /// Every event carries the modifier keys held at that moment
    /// ([`PointerEvent::modifiers`]).
    #[inline]
    pub fn on_pointer(&self, f: impl FnMut(PointerEvent) + 'static) {
        self.0.on_pointer(f)
    }

    /// The wheel (including two-finger scrolling on a trackpad). Amounts are logical
    /// pixels.
    ///
    /// The modifier keys held come with it ([`PointerEvent::modifiers`]), for
    /// example to zoom only while Ctrl or ⌘ is held.
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

    /// Delivers `event` as if the user had pressed, moved or released the pointer:
    /// [`on_pointer`](Self::on_pointer) is called with it as given. An event whose
    /// phase is [`Wheel`](naui_core::PointerPhase::Wheel) goes to
    /// [`on_wheel`](Self::on_wheel) instead, as with [`send_wheel`](Self::send_wheel).
    ///
    /// Build it with [`PointerEvent::new`] and its `button` and `modifiers`. Positions
    /// are logical pixels from the surface's top left, as `on_pointer` receives them;
    /// nothing checks that they are inside the surface.
    ///
    /// For tests and automation. No native event is made, so nothing else sees it
    /// (no outer view, no context menu). See
    /// [Testing apps](crate::guide#testing-apps).
    #[inline]
    pub fn send_pointer(&self, event: PointerEvent) {
        self.0.send_pointer(event)
    }

    /// Delivers `event` as if the user had turned the wheel:
    /// [`on_wheel`](Self::on_wheel) is called with it, and its answer is returned
    /// (`Continue` without `on_wheel`). Build it with [`PointerEvent::wheel`].
    ///
    /// For tests and automation. No native event is made, so nothing scrolls when
    /// the answer is `Continue`. See [Testing apps](crate::guide#testing-apps).
    #[inline]
    pub fn send_wheel(&self, event: PointerEvent) -> EventResponse {
        self.0.send_wheel(event)
    }
}
