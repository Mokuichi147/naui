//! `Scroll::on_scroll` の決まりを、各バックエンドで共有する。
//!
//! 「位置が変わったときだけ呼ぶ」「通知の中からの `scroll_to` も取りこぼさない」
//! をここで一度だけ書き、バックエンドはネイティブの通知を受けたら
//! [`ScrollNotifier::notify_if_moved`] を呼ぶだけにする。

use std::cell::{Cell, RefCell};

use crate::layout::ScrollMetrics;

/// 通知の中で動かし続けるクロージャがあっても、止まらなくならないための上限。
const MAX_RENOTIFY: usize = 16;

type Handler = RefCell<Option<Box<dyn FnMut(ScrollMetrics)>>>;

/// スクロール位置の通知先と、最後に通知した位置。
///
/// 通知の中から `scroll_to` を呼ぶと、環境によってはその移動の通知が同期で
/// 入れ子に来る。その場では呼ばず、外側の呼び出しが戻ってから最新の位置で
/// 呼び直す。入れ子の通知を捨てると、利用者は最後の位置を受け取れない。
#[derive(Default)]
pub struct ScrollNotifier {
    handler: Handler,
    last_offset: Cell<(f64, f64)>,
    /// 通知している最中 (または [`batch`](Self::batch) の中)。
    busy: Cell<bool>,
}

impl ScrollNotifier {
    /// 通知先を差し替える。`current` の位置は通知済みとみなす。
    pub fn set(&self, current: &ScrollMetrics, f: impl FnMut(ScrollMetrics) + 'static) {
        self.last_offset.set(offset_of(current));
        *self.handler.borrow_mut() = Some(Box::new(f));
    }

    /// 前回の通知から位置が変わっていれば通知する。大きさだけの変化では呼ばない。
    ///
    /// `read` はいまの位置と大きさを返す。通知の中で位置が変わったときは、
    /// 戻ってから読み直して通知し直す。
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

    /// `apply` の間に来た通知をまとめ、終わってから 1 回だけ通知する。
    ///
    /// 縦と横を別々に動かす環境で、片方だけ動いた途中の位置を通知しない
    /// ために使う。
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

/// 通知の中で panic しても、`busy` を戻す。
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

    /// 通知の中で動かしたとき、その移動が同期で入れ子に来ても最後の位置が届く。
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
