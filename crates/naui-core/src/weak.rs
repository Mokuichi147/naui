//! Weak references to widgets.

use std::rc::Rc;

/// A weak reference for referring to a widget from a notification closure without
/// holding it strongly.
///
/// Returned by `downgrade()` on each backend's widgets. Notification closures
/// (`on_change` and so on) are held inside the widget, so capturing the widget
/// itself in the closure with `clone()` creates a cycle and it is never freed.
/// Capture this instead, and [`upgrade`](Self::upgrade) it when you use it.
///
/// ```ignore
/// let weak = input.downgrade();
/// input.on_focus_change(move |focused| {
///     if let Some(input) = weak.upgrade() {
///         if !focused {
///             input.set_text(input.text().trim());
///         }
///     }
/// });
/// ```
pub struct WeakWidget<W>(Rc<dyn Fn() -> Option<W>>);

impl<W> WeakWidget<W> {
    /// Created by backends. `upgrade` returns the widget if it still exists.
    #[doc(hidden)]
    pub fn new(upgrade: impl Fn() -> Option<W> + 'static) -> Self {
        Self(Rc::new(upgrade))
    }

    /// Returns a handle to the widget if it still exists.
    ///
    /// `None` once every handle has been dropped and the widget has been removed from
    /// its parent container and freed.
    pub fn upgrade(&self) -> Option<W> {
        (self.0)()
    }
}

impl<W> Clone for WeakWidget<W> {
    fn clone(&self) -> Self {
        Self(Rc::clone(&self.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone)]
    struct Probe(Rc<()>);

    #[test]
    fn upgrade_returns_none_after_drop() {
        let probe = Probe(Rc::new(()));
        let inner = Rc::downgrade(&probe.0);
        let weak = WeakWidget::new(move || inner.upgrade().map(Probe));
        let copy = weak.clone();
        assert!(weak.upgrade().is_some());
        // 弱参照そのものは中身を保持しない。
        assert_eq!(Rc::strong_count(&probe.0), 1);
        drop(probe);
        assert!(weak.upgrade().is_none());
        assert!(copy.upgrade().is_none());
    }
}
