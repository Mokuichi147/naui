//! The part of `NativeHost` shared by the backends: what it reports about the
//! hosted view (its size and the display scale) and when.

use std::cell::{Cell, RefCell};

/// The size of a `NativeHost` and the scale of the display it is shown on.
///
/// `width` and `height` are logical pixels, the same unit as everything else in
/// naui. `scale` is how many physical pixels make one logical pixel (2.0 on a
/// typical Retina or 200% display). A view that draws pixels itself (a GPU surface)
/// sizes its buffer with [`physical_size`](Self::physical_size).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HostSize {
    /// The width in logical pixels.
    pub width: f64,
    /// The height in logical pixels.
    pub height: f64,
    /// Physical pixels per logical pixel.
    pub scale: f64,
}

impl HostSize {
    /// Makes a size from its three parts.
    pub const fn new(width: f64, height: f64, scale: f64) -> Self {
        Self {
            width,
            height,
            scale,
        }
    }

    /// The size in physical pixels (width, height), each rounded to the nearest
    /// whole pixel. 0 when the logical size is 0 (before layout, for example).
    pub fn physical_size(&self) -> (u32, u32) {
        let pixels = |length: f64| {
            let value = (length * self.scale).round();
            if value.is_finite() && value > 0.0 {
                value.min(u32::MAX as f64) as u32
            } else {
                0
            }
        };
        (pixels(self.width), pixels(self.height))
    }
}

impl Default for HostSize {
    /// No size yet, at scale 1.
    fn default() -> Self {
        Self::new(0.0, 0.0, 1.0)
    }
}

type ResizeCallback = dyn FnMut(HostSize);
type AttachCallback = dyn FnMut(bool);

/// Holds a `NativeHost`'s `on_resize` and `on_attach` closures and what was last
/// reported, so each backend reports only changes.
///
/// The backends call [`report_size`](Self::report_size) and
/// [`report_attached`](Self::report_attached) from whatever native notification
/// they have; repeated values are dropped here. A closure is taken out while it is
/// being called (the same rule as `WheelHandler`), so it may set a new closure or
/// cause another report: a report arriving while the closure runs is remembered
/// but not delivered again.
#[doc(hidden)]
#[derive(Default)]
pub struct HostEvents {
    resize: RefCell<crate::Slot<ResizeCallback>>,
    attach: RefCell<crate::Slot<AttachCallback>>,
    /// The size last reported. `None` before the first report.
    size: Cell<Option<HostSize>>,
    /// Whether it was last reported as attached.
    attached: Cell<bool>,
}

impl HostEvents {
    /// Sets the closure called when the size or the scale changes.
    pub fn on_resize(&self, f: impl FnMut(HostSize) + 'static) {
        *self.resize.borrow_mut() = Some(Box::new(f));
    }

    /// Sets the closure called when the host joins (`true`) or leaves (`false`) a
    /// window.
    pub fn on_attach(&self, f: impl FnMut(bool) + 'static) {
        *self.attach.borrow_mut() = Some(Box::new(f));
    }

    /// The size last reported, if any.
    pub fn last_size(&self) -> Option<HostSize> {
        self.size.get()
    }

    /// Whether it was last reported as attached.
    pub fn is_attached(&self) -> bool {
        self.attached.get()
    }

    /// Reports the current size. Calls `on_resize` when it differs from the last
    /// report (the first report always counts, unless it is the zero size at
    /// scale 1, which is what a host reports before any layout).
    pub fn report_size(&self, size: HostSize) {
        let previous = self.size.replace(Some(size));
        let changed = match previous {
            Some(previous) => previous != size,
            None => size != HostSize::default(),
        };
        if changed {
            emit(&self.resize, size);
        }
    }

    /// Reports whether the host is in a window now. Calls `on_attach` when it
    /// differs from the last report (starting from "not attached").
    pub fn report_attached(&self, attached: bool) {
        if self.attached.replace(attached) != attached {
            emit(&self.attach, attached);
        }
    }
}

/// Calls the closure in `slot` with `value`, taking it out while it runs. Nothing
/// happens when there is none, or when it is already running.
fn emit<T>(slot: &RefCell<crate::Slot<dyn FnMut(T)>>, value: T) {
    let Some(mut f) = slot.try_borrow_mut().ok().and_then(|mut slot| slot.take()) else {
        return;
    };
    f(value);
    let mut slot = slot.borrow_mut();
    if slot.is_none() {
        *slot = Some(f);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::rc::Rc;

    #[test]
    fn physical_size_rounds_and_clamps() {
        assert_eq!(HostSize::new(100.0, 50.5, 2.0).physical_size(), (200, 101));
        assert_eq!(HostSize::new(10.2, 10.0, 1.25).physical_size(), (13, 13));
        assert_eq!(HostSize::new(0.0, -3.0, 2.0).physical_size(), (0, 0));
        assert_eq!(HostSize::new(f64::NAN, 1.0, 1.0).physical_size(), (0, 1));
        assert_eq!(HostSize::default().physical_size(), (0, 0));
    }

    #[test]
    fn only_changes_are_reported() {
        let events = HostEvents::default();
        let seen = Rc::new(RefCell::new(Vec::new()));
        events.on_resize({
            let seen = seen.clone();
            move |size| seen.borrow_mut().push(size)
        });
        events.report_size(HostSize::default());
        assert!(
            seen.borrow().is_empty(),
            "the unlaid-out size is not a change"
        );
        events.report_size(HostSize::new(10.0, 20.0, 1.0));
        events.report_size(HostSize::new(10.0, 20.0, 1.0));
        events.report_size(HostSize::new(10.0, 20.0, 2.0));
        assert_eq!(
            *seen.borrow(),
            vec![
                HostSize::new(10.0, 20.0, 1.0),
                HostSize::new(10.0, 20.0, 2.0)
            ]
        );
        assert_eq!(events.last_size(), Some(HostSize::new(10.0, 20.0, 2.0)));
    }

    #[test]
    fn attach_reports_changes_from_detached() {
        let events = HostEvents::default();
        let seen = Rc::new(RefCell::new(Vec::new()));
        events.on_attach({
            let seen = seen.clone();
            move |attached| seen.borrow_mut().push(attached)
        });
        events.report_attached(false);
        events.report_attached(true);
        events.report_attached(true);
        events.report_attached(false);
        assert_eq!(*seen.borrow(), vec![true, false]);
        assert!(!events.is_attached());
    }

    #[test]
    fn a_report_from_inside_the_closure_is_not_delivered_again() {
        let events = Rc::new(HostEvents::default());
        let calls = Rc::new(Cell::new(0));
        events.on_resize({
            let events = Rc::downgrade(&events);
            let calls = calls.clone();
            move |size| {
                calls.set(calls.get() + 1);
                if let Some(events) = events.upgrade() {
                    events.report_size(HostSize::new(size.width + 1.0, size.height, 1.0));
                }
            }
        });
        events.report_size(HostSize::new(1.0, 1.0, 1.0));
        assert_eq!(calls.get(), 1);
        assert_eq!(events.last_size(), Some(HostSize::new(2.0, 1.0, 1.0)));
    }

    #[test]
    fn the_closure_may_replace_itself() {
        let events = Rc::new(HostEvents::default());
        let second = Rc::new(Cell::new(false));
        events.on_attach({
            let events = Rc::downgrade(&events);
            let second = second.clone();
            move |_| {
                let second = second.clone();
                if let Some(events) = events.upgrade() {
                    events.on_attach(move |_| second.set(true));
                }
            }
        });
        events.report_attached(true);
        events.report_attached(false);
        assert!(
            second.get(),
            "the closure set from inside replaced the first"
        );
    }
}
