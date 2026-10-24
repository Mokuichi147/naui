//! Values passed in window notifications, and containers for their handlers.

/// The reply from [`Window::on_close_request`].
///
/// [`Window::on_close_request`]: ../../naui/struct.Window.html#method.on_close_request
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CloseResponse {
    /// Close as requested.
    #[default]
    Close,
    /// Don't close (the app decides later, for example after asking whether to save).
    KeepOpen,
}

/// Holds one handler for `on_close_request`. Shared by all backends.
///
/// The closure is taken out while it is called, so you may operate on the same
/// window or replace the handler from inside the notification. When there is no
/// handler (and when it is called re-entrantly), the window closes.
#[derive(Default)]
pub struct CloseHandler(std::cell::RefCell<crate::Slot<CloseCallback>>);

type CloseCallback = dyn FnMut() -> CloseResponse;

impl CloseHandler {
    pub fn set(&self, f: impl FnMut() -> CloseResponse + 'static) {
        *self.0.borrow_mut() = Some(Box::new(f));
    }

    pub fn ask(&self) -> CloseResponse {
        let Some(mut f) = self.0.borrow_mut().take() else {
            return CloseResponse::Close;
        };
        let response = f();
        let mut slot = self.0.borrow_mut();
        if slot.is_none() {
            *slot = Some(f);
        }
        response
    }
}

/// The display state of a window (`Window::state` / `Window::set_state`).
///
/// On some platforms states can overlap (minimized while maximized, for example),
/// but naui returns only the one the user sees. When they overlap, the priority is
/// minimized → fullscreen → maximized.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum WindowState {
    /// None of maximized, minimized or fullscreen. At the position and size the user
    /// left it.
    #[default]
    Normal,
    /// Expanded to fill the screen (excluding the menu bar and taskbar).
    Maximized,
    /// Put away in the Dock or taskbar.
    Minimized,
    /// Using the whole screen without even a title bar.
    Fullscreen,
}

impl WindowState {
    /// Picks the state the user sees from overlapping states.
    pub fn from_flags(minimized: bool, fullscreen: bool, maximized: bool) -> Self {
        if minimized {
            Self::Minimized
        } else if fullscreen {
            Self::Fullscreen
        } else if maximized {
            Self::Maximized
        } else {
            Self::Normal
        }
    }
}

/// The `on_state_change` handler and the state last reported. Shared by all
/// backends.
///
/// Native notifications can arrive several times for one change (a size change and
/// a state change arriving separately, for example), so the handler is called only
/// when the state differs from the one last reported. The closure is taken out
/// while it is called, so you may operate on the same window from inside the
/// notification.
///
/// When the state changes again inside the notification (by calling `set_state`
/// from it, for example), it is not reported on the spot but remembered, and the
/// final state is reported after the handler returns. If it has returned to the
/// original state by then, nothing is reported.
#[derive(Default)]
pub struct StateHandler {
    /// The state last received.
    last: std::cell::Cell<WindowState>,
    /// The state last passed to the handler (or set by `reset`).
    delivered: std::cell::Cell<WindowState>,
    /// In the middle of calling the handler.
    notifying: std::cell::Cell<bool>,
    callback: std::cell::RefCell<crate::Slot<StateCallback>>,
}

type StateCallback = dyn FnMut(WindowState);

impl StateHandler {
    pub fn set(&self, f: impl FnMut(WindowState) + 'static) {
        *self.callback.borrow_mut() = Some(Box::new(f));
    }

    /// The state last received.
    pub fn last(&self) -> WindowState {
        self.last.get()
    }

    /// Records the current state without reporting it (the state before showing, for
    /// example).
    pub fn reset(&self, state: WindowState) {
        self.last.set(state);
        self.delivered.set(state);
    }

    /// Calls the handler if `state` differs from what was last reported.
    ///
    /// When called from inside the handler, it only records the state and reports it
    /// after the outer call returns from the handler.
    pub fn update(&self, state: WindowState) {
        self.last.set(state);
        if self.notifying.replace(true) {
            return;
        }
        // 通知先が panic しても、次の通知を止めないように戻す。
        struct Notifying<'a>(&'a std::cell::Cell<bool>);
        impl Drop for Notifying<'_> {
            fn drop(&mut self) {
                self.0.set(false);
            }
        }
        let _notifying = Notifying(&self.notifying);
        while self.delivered.get() != self.last.get() {
            let next = self.last.get();
            self.delivered.set(next);
            let Some(mut f) = self.callback.borrow_mut().take() else {
                continue;
            };
            f(next);
            let mut slot = self.callback.borrow_mut();
            if slot.is_none() {
                *slot = Some(f);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    #[test]
    fn picks_the_state_the_user_sees() {
        assert_eq!(
            WindowState::from_flags(false, false, false),
            WindowState::Normal
        );
        assert_eq!(
            WindowState::from_flags(false, false, true),
            WindowState::Maximized
        );
        assert_eq!(
            WindowState::from_flags(false, true, true),
            WindowState::Fullscreen
        );
        assert_eq!(
            WindowState::from_flags(true, true, true),
            WindowState::Minimized
        );
    }

    #[test]
    fn notifies_only_changes() {
        let handler = StateHandler::default();
        let seen = Rc::new(RefCell::new(Vec::new()));
        handler.set({
            let seen = seen.clone();
            move |state| seen.borrow_mut().push(state)
        });
        handler.update(WindowState::Normal);
        handler.update(WindowState::Maximized);
        handler.update(WindowState::Maximized);
        handler.reset(WindowState::Fullscreen);
        handler.update(WindowState::Fullscreen);
        handler.update(WindowState::Normal);
        assert_eq!(
            seen.borrow().as_slice(),
            [WindowState::Maximized, WindowState::Normal]
        );
        assert_eq!(handler.last(), WindowState::Normal);
    }

    /// When the state changes inside the notification, it is reported after returning
    /// (not lost).
    #[test]
    fn notifies_changes_made_inside_the_callback() {
        let handler = Rc::new(StateHandler::default());
        let seen = Rc::new(RefCell::new(Vec::new()));
        handler.set({
            let seen = seen.clone();
            let handler = Rc::downgrade(&handler);
            move |state| {
                seen.borrow_mut().push(state);
                if state == WindowState::Maximized {
                    if let Some(handler) = handler.upgrade() {
                        handler.update(WindowState::Normal);
                        assert_eq!(seen.borrow().len(), 1, "通知の中では呼ばない");
                    }
                }
            }
        });
        handler.update(WindowState::Maximized);
        assert_eq!(
            seen.borrow().as_slice(),
            [WindowState::Maximized, WindowState::Normal]
        );
        assert_eq!(handler.last(), WindowState::Normal);
    }

    /// When it changes inside the notification and changes back, it is not reported
    /// again.
    #[test]
    fn skips_changes_undone_inside_the_callback() {
        let handler = Rc::new(StateHandler::default());
        let seen = Rc::new(RefCell::new(Vec::new()));
        handler.set({
            let seen = seen.clone();
            let handler = Rc::downgrade(&handler);
            move |state| {
                seen.borrow_mut().push(state);
                if let Some(handler) = handler.upgrade() {
                    handler.update(WindowState::Normal);
                    handler.update(state);
                }
            }
        });
        handler.update(WindowState::Fullscreen);
        assert_eq!(seen.borrow().as_slice(), [WindowState::Fullscreen]);
    }

    /// Even if the handler panics, the next change is still reported.
    #[test]
    fn keeps_notifying_after_a_panic() {
        let handler = StateHandler::default();
        handler.set(|state| {
            if state == WindowState::Maximized {
                panic!("通知先の失敗");
            }
        });
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            handler.update(WindowState::Maximized)
        }));
        assert!(result.is_err());
        let seen = Rc::new(RefCell::new(Vec::new()));
        handler.set({
            let seen = seen.clone();
            move |state| seen.borrow_mut().push(state)
        });
        handler.update(WindowState::Normal);
        assert_eq!(seen.borrow().as_slice(), [WindowState::Normal]);
    }
}
