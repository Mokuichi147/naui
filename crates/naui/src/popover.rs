use crate::sys;
use crate::Widget;
use naui_core::PopoverPosition;

/// A panel shown beside a widget, holding any widget as its content.
///
/// For settings panels and pickers opened from a button, where a modal dialog would
/// be too heavy. It closes when the user clicks outside it or presses Esc, and when
/// there is no room on the preferred side it moves to the opposite one; all of that
/// is the native popover's own behavior (`NSPopover`, `GtkPopover`, `Flyout`, the
/// HTML `popover` attribute).
///
/// It is not a widget laid out on screen, so it is not a [`Widget`]. See
/// [Popovers](crate::guide#popovers) for how to open one from a button.
#[derive(Clone)]
pub struct Popover(pub(crate) sys::Popover);

impl_backend!(Popover);

impl Popover {
    /// The content. Each call replaces it.
    ///
    /// The popover takes the content's natural size, so `Fill` in the content's
    /// [`set_sizing`](crate::Label::set_sizing) has nothing to fill. Give a fixed size
    /// or a minimum to make it larger.
    #[inline]
    pub fn set_child(&self, child: &dyn Widget) {
        self.0.set_child(child.as_backend())
    }

    /// The side it prefers to appear on. [`PopoverPosition::Bottom`] by default.
    ///
    /// Changing it while it is open moves it.
    #[inline]
    pub fn set_position(&self, position: PopoverPosition) {
        self.0.set_position(position)
    }

    #[inline]
    pub fn position(&self) -> PopoverPosition {
        self.0.position()
    }

    /// Shows it beside `widget`. If it is already open, it moves to `widget`.
    ///
    /// Does nothing while `widget` is not shown in a window.
    #[inline]
    pub fn open(&self, widget: &dyn Widget) {
        self.0.open(widget.as_backend())
    }

    /// Closes it. [`on_close`](Self::on_close) is not called. Does nothing if it is
    /// not open.
    #[inline]
    pub fn close(&self) {
        self.0.close()
    }

    #[inline]
    pub fn is_open(&self) -> bool {
        self.0.is_open()
    }

    /// Called when the user closes it (a click outside, Esc).
    ///
    /// Not called when it is closed with [`close`](Self::close). Setting it again
    /// replaces the previous closure.
    #[inline]
    pub fn on_close(&self, f: impl FnMut() + 'static) {
        self.0.on_close(f)
    }
}
