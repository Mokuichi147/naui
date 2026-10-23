//! Shares the rules of `Scroll::on_scroll` across backends.
//!
//! "Call only when the position changed" and "don't miss a `scroll_to` from inside
//! the notification" are written once here, and backends just call
//! [`ScrollNotifier::notify_if_moved`] when they receive a native notification.

use std::cell::{Cell, RefCell};

use crate::layout::ScrollMetrics;

/// A cap so that a closure that keeps scrolling inside the notification cannot make
/// it loop forever.
const MAX_RENOTIFY: usize = 16;

type Handler = RefCell<Option<Box<dyn FnMut(ScrollMetrics)>>>;

/// The scroll position handler and the position last notified.
///
/// When `scroll_to` is called from inside the notification, some platforms deliver
/// the notification for that move synchronously and re-entrantly. It is not called
/// on the spot; instead it is called again with the latest position after the outer
/// call returns. Discarding the nested notification would keep the user from
/// receiving the final position.
#[derive(Default)]
pub struct ScrollNotifier {
    handler: Handler,
    last_offset: Cell<(f64, f64)>,
    /// In the middle of notifying (or inside a [`batch`](Self::batch)).
    busy: Cell<bool>,
}

impl ScrollNotifier {
    /// Replaces the handler. The `current` position counts as already notified.
    pub fn set(&self, current: &ScrollMetrics, f: impl FnMut(ScrollMetrics) + 'static) {
        self.last_offset.set(offset_of(current));
        *self.handler.borrow_mut() = Some(Box::new(f));
    }

    /// Notifies if the position changed since the last notification. A change in size
    /// alone does not call it.
    ///
    /// `read` returns the current position and sizes. If the position changes during
    /// the notification, it is read again and notified again after the handler returns.
    pub fn notify_if_moved(&self, read: impl Fn() -> ScrollMetrics) {
        if self.busy.get() {
            // 外側の呼び出しが、戻ってから読み直す。
            return;
        }
        let _busy = Busy::enter(&self.busy);
        for _ in 0..MAX_RENOTIFY {
            let metrics = read();
            let offset = offset_of(&metrics);
            if offset == self.last_offset.replace(offset) {
                return;
            }
            let Some(mut f) = self.handler.borrow_mut().take() else {
                return;
            };
            f(metrics);
            // 呼び出し中に差し替えられていたら、新しいほうを残す。
            let mut slot = self.handler.borrow_mut();
            if slot.is_none() {
                *slot = Some(f);
            }
        }
    }

    /// Gathers the notifications that arrive during `apply` and notifies once
    /// afterwards.
    ///
    /// Used on platforms that move the vertical and horizontal positions separately, so
    /// that an intermediate position with only one of them moved is not notified.
    pub fn batch(&self, apply: impl FnOnce(), read: impl Fn() -> ScrollMetrics) {
        if self.busy.get() {
            apply();
            return;
        }
        {
            let _busy = Busy::enter(&self.busy);
            apply();
        }
        self.notify_if_moved(read);
    }
}

fn offset_of(metrics: &ScrollMetrics) -> (f64, f64) {
    (metrics.x, metrics.y)
}

/// Restores `busy` even if the notification panics.
struct Busy<'a>(&'a Cell<bool>);

impl<'a> Busy<'a> {
    fn enter(flag: &'a Cell<bool>) -> Self {
        flag.set(true);
        Self(flag)
    }
}

impl Drop for Busy<'_> {
    fn drop(&mut self) {
        self.0.set(false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::rc::Rc;

    fn at(y: f64) -> ScrollMetrics {
        ScrollMetrics {
            y,
            viewport_width: 100.0,
            viewport_height: 100.0,
            content_width: 100.0,
            content_height: 1000.0,
            ..ScrollMetrics::default()
        }
    }

    #[test]
    fn notifies_only_moves() {
        let notifier = ScrollNotifier::default();
        let seen = Rc::new(RefCell::new(Vec::new()));
        notifier.set(&at(0.0), {
            let seen = seen.clone();
            move |m| seen.borrow_mut().push(m.y)
        });
        notifier.notify_if_moved(|| at(0.0));
        notifier.notify_if_moved(|| at(10.0));
        notifier.notify_if_moved(|| at(10.0));
        notifier.notify_if_moved(|| ScrollMetrics {
            content_height: 2000.0,
            ..at(10.0)
        });
        assert_eq!(*seen.borrow(), vec![10.0]);
    }

    /// When the handler scrolls, the final position arrives even if that move is
    /// delivered synchronously and re-entrantly.
    #[test]
    fn nested_move_is_notified_after_the_handler_returns() {
        let notifier = Rc::new(ScrollNotifier::default());
        let position = Rc::new(Cell::new(0.0));
        let seen = Rc::new(RefCell::new(Vec::new()));
        notifier.set(&at(0.0), {
            let notifier = notifier.clone();
            let position = position.clone();
            let seen = seen.clone();
            move |m| {
                seen.borrow_mut().push(m.y);
                if m.y == 55.0 {
                    // 行の境界へ吸着させる `scroll_to` と、その同期の通知。
                    position.set(50.0);
                    let position = position.clone();
                    notifier.notify_if_moved(move || at(position.get()));
                }
            }
        });
        position.set(55.0);
        notifier.notify_if_moved(|| at(position.get()));
        assert_eq!(*seen.borrow(), vec![55.0, 50.0]);
    }

    #[test]
    fn batch_notifies_once_after_both_axes_move() {
        let notifier = ScrollNotifier::default();
        let x = Cell::new(0.0);
        let y = Cell::new(0.0);
        let read = || ScrollMetrics {
            x: x.get(),
            content_width: 1000.0,
            ..at(y.get())
        };
        let seen = Rc::new(RefCell::new(Vec::new()));
        notifier.set(&read(), {
            let seen = seen.clone();
            move |m| seen.borrow_mut().push((m.x, m.y))
        });
        notifier.batch(
            || {
                x.set(30.0);
                notifier.notify_if_moved(read);
                y.set(40.0);
                notifier.notify_if_moved(read);
            },
            read,
        );
        assert_eq!(*seen.borrow(), vec![(30.0, 40.0)]);
    }

    #[test]
    fn a_handler_that_keeps_moving_stops() {
        let notifier = Rc::new(ScrollNotifier::default());
        let position = Rc::new(Cell::new(0.0));
        let calls = Rc::new(Cell::new(0));
        notifier.set(&at(0.0), {
            let position = position.clone();
            let calls = calls.clone();
            move |_| {
                calls.set(calls.get() + 1);
                position.set(position.get() + 1.0);
            }
        });
        position.set(1.0);
        notifier.notify_if_moved(|| at(position.get()));
        assert_eq!(calls.get(), MAX_RENOTIFY);
    }
}
