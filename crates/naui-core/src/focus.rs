//! Notifications of focus entering and leaving (`on_focus_change`).
//!
//! Whenever the focused element in a window changes, each backend walks up the
//! parents from there and passes the first registered widget it finds to
//! [`FocusOwner::move_to`]. Only when the owner changes does the previous owner
//! receive `false` and the new owner `true`.
//!
//! Because it counts by owner, moving between parts within one widget (between a
//! number input's field and its step buttons, for example) does not notify. When
//! the window itself becomes inactive, focus within that window does not change,
//! so nothing is notified.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;

use crate::Slot;

/// The handler for focus entering and leaving. `true` on entering, `false` on
/// leaving.
///
/// The closure is taken out while it is called, so you may operate on the same
/// widget or replace the handler with `on_focus_change` from inside the
/// notification.
#[derive(Default)]
pub struct FocusHandler(RefCell<Slot<dyn FnMut(bool)>>);

impl FocusHandler {
    pub fn set(&self, f: impl FnMut(bool) + 'static) {
        *self.0.borrow_mut() = Some(Box::new(f));
    }

    pub fn emit(&self, focused: bool) {
        let Some(mut f) = self.0.borrow_mut().take() else {
            return;
        };
        f(focused);
        let mut slot = self.0.borrow_mut();
        if slot.is_none() {
            *slot = Some(f);
        }
    }
}

/// For one window, the registered widget that currently has focus.
///
/// Even if focus is moved inside a notification (moving to another field as soon as
/// one is entered, for example), notifications are queued and sent one at a time,
/// so none are lost or reordered.
#[derive(Default)]
pub struct FocusOwner {
    current: RefCell<Option<Rc<FocusHandler>>>,
    pending: RefCell<VecDeque<(Rc<FocusHandler>, bool)>>,
    emitting: Cell<bool>,
}

impl FocusOwner {
    /// Reports that focus moved into `next` (`None` means inside no registered
    /// widget). Notifies only when the owner changes.
    ///
    /// `false` to the previous owner is sent first, then `true` to the new owner.
    pub fn move_to(&self, next: Option<Rc<FocusHandler>>) {
        {
            let mut current = self.current.borrow_mut();
            let same = match (current.as_ref(), next.as_ref()) {
                (Some(a), Some(b)) => Rc::ptr_eq(a, b),
                (None, None) => true,
                _ => false,
            };
            if same {
                return;
            }
            let previous = std::mem::replace(&mut *current, next.clone());
            let mut pending = self.pending.borrow_mut();
            pending.extend(previous.map(|previous| (previous, false)));
            pending.extend(next.map(|next| (next, true)));
        }
        if self.emitting.replace(true) {
            // 通知の中から呼ばれた。外側の送り出しが続きを送る。
            return;
        }
        loop {
            let Some((handler, focused)) = self.pending.borrow_mut().pop_front() else {
                break;
            };
            handler.emit(focused);
        }
        self.emitting.set(false);
    }

    /// Resets the owner without notifying.
    ///
    /// Used when focus is already inside the widget at the moment its handler is set.
    /// Entering is not reported, but `false` will arrive when focus leaves it.
    pub fn reset(&self, current: Option<Rc<FocusHandler>>) {
        *self.current.borrow_mut() = current;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn recorder(name: &'static str, log: &Rc<RefCell<Vec<String>>>) -> Rc<FocusHandler> {
        let handler = Rc::new(FocusHandler::default());
        let log = log.clone();
        handler.set(move |focused| log.borrow_mut().push(format!("{name}:{focused}")));
        handler
    }

    #[test]
    fn only_a_change_of_owner_is_reported() {
        let log = Rc::new(RefCell::new(Vec::new()));
        let a = recorder("a", &log);
        let b = recorder("b", &log);
        let owner = FocusOwner::default();

        owner.move_to(Some(a.clone()));
        owner.move_to(Some(a.clone()));
        owner.move_to(Some(b.clone()));
        owner.move_to(None);
        owner.move_to(None);
        assert_eq!(*log.borrow(), ["a:true", "a:false", "b:true", "b:false"]);
    }

    #[test]
    fn a_reset_owner_is_told_when_the_focus_leaves() {
        let log = Rc::new(RefCell::new(Vec::new()));
        let a = recorder("a", &log);
        let owner = FocusOwner::default();
        owner.reset(Some(a.clone()));
        assert!(log.borrow().is_empty(), "置き直しは通知しない");
        owner.move_to(Some(a));
        owner.move_to(None);
        assert_eq!(*log.borrow(), ["a:false"]);
    }

    #[test]
    fn the_handler_may_move_the_focus_again() {
        let log = Rc::new(RefCell::new(Vec::new()));
        let owner = Rc::new(FocusOwner::default());
        let a = recorder("a", &log);
        let b = Rc::new(FocusHandler::default());
        b.set({
            let log = log.clone();
            let owner = owner.clone();
            let a = a.clone();
            move |focused| {
                log.borrow_mut().push(format!("b:{focused}"));
                // 入ったとたんに別の欄へ移す (検証で戻すときなど)。
                if focused {
                    owner.move_to(Some(a.clone()));
                }
            }
        });
        owner.move_to(Some(b));
        assert_eq!(*log.borrow(), ["b:true", "b:false", "a:true"]);
    }
}
