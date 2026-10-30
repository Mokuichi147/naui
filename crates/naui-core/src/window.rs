//! Values passed in window notifications, and containers for their handlers.

use std::fmt;

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

impl WindowState {
    /// The lowercase name used by [`WindowPlacement`]'s text form.
    fn name(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Maximized => "maximized",
            Self::Minimized => "minimized",
            Self::Fullscreen => "fullscreen",
        }
    }

    fn from_name(name: &str) -> Option<Self> {
        [
            Self::Normal,
            Self::Maximized,
            Self::Minimized,
            Self::Fullscreen,
        ]
        .into_iter()
        .find(|state| state.name() == name)
    }
}

/// Where a window is and how large it is, for saving it when the app quits and
/// putting it back the next time it starts (`Window::placement` /
/// `Window::set_placement`).
///
/// `size` and `position` are those of the normal state: while the window is maximized
/// or in fullscreen, they are where it goes back to, not the frame filling the screen.
/// So restoring a window that was closed while maximized maximizes it again, and
/// un-maximizing it then returns it to where the user had it.
///
/// [`Display`](fmt::Display) and [`parse`](Self::parse) turn it into one line of text
/// and back, for writing it to the app's settings file:
///
/// ```
/// # use naui_core::{WindowPlacement, WindowState};
/// let placement = WindowPlacement {
///     size: (800.0, 600.0),
///     position: Some((80.0, 60.0)),
///     state: WindowState::Maximized,
/// };
/// let text = placement.to_string();
/// assert_eq!(text, "size=800x600 position=80,60 state=maximized");
/// assert_eq!(WindowPlacement::parse(&text), Some(placement));
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WindowPlacement {
    /// The content size (width, height) in the normal state, in the same units as
    /// `Window::size`.
    pub size: (f64, f64),
    /// The position of the window frame's top-left corner in the normal state, the
    /// same as `Window::position`. `None` where positions are not available (Linux and
    /// the Web).
    pub position: Option<(f64, f64)>,
    /// The state to show the window in. `Window::placement` never returns
    /// [`WindowState::Minimized`]: a minimized window reports the state it returns to
    /// when brought back.
    pub state: WindowState,
}

impl WindowPlacement {
    /// Reads the text written by [`Display`](fmt::Display).
    ///
    /// Items are `key=value` pairs separated by spaces, in any order. `size` is
    /// required; without `position` it is `None`, and without `state` it is
    /// [`WindowState::Normal`]. Unknown keys are skipped, so text written by a later
    /// version that adds items still reads. Returns `None` when `size` is missing, an
    /// item is not `key=value`, or a value cannot be read (including numbers that are
    /// not finite).
    pub fn parse(text: &str) -> Option<Self> {
        let mut size = None;
        let mut position = None;
        let mut state = WindowState::Normal;
        for item in text.split_whitespace() {
            let (key, value) = item.split_once('=')?;
            match key {
                "size" => size = Some(parse_pair(value, 'x')?),
                "position" => position = Some(parse_pair(value, ',')?),
                "state" => state = WindowState::from_name(value)?,
                _ => {}
            }
        }
        Some(Self {
            size: size?,
            position,
            state,
        })
    }
}

/// Reads two finite numbers separated by `separator`.
fn parse_pair(text: &str, separator: char) -> Option<(f64, f64)> {
    let (a, b) = text.split_once(separator)?;
    let a: f64 = a.parse().ok()?;
    let b: f64 = b.parse().ok()?;
    (a.is_finite() && b.is_finite()).then_some((a, b))
}

impl fmt::Display for WindowPlacement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "size={}x{}", self.size.0, self.size.1)?;
        if let Some((x, y)) = self.position {
            write!(f, " position={x},{y}")?;
        }
        write!(f, " state={}", self.state.name())
    }
}

/// The height of the band along a window's top edge that has to be on a screen for
/// [`is_on_screen`] (about a title bar).
const GRAB_BAND_HEIGHT: f64 = 40.0;

/// How much of [`GRAB_BAND_HEIGHT`] has to be on a screen, horizontally and
/// vertically, for the user to grab it.
const GRAB_MIN_VISIBLE: (f64, f64) = (48.0, 8.0);

/// Whether a window `width` wide with its frame's top-left at `position` would show
/// enough of its top edge on one of `screens` (work areas, in the same coordinates)
/// for the user to drag it.
///
/// Used when restoring a saved position: if the screen it was on has been
/// disconnected, or the resolution has dropped, the saved position is dropped and the
/// window is placed as if none had been saved.
pub fn is_on_screen(position: (f64, f64), width: f64, screens: &[crate::Rect]) -> bool {
    let (x, y) = position;
    let need_width = GRAB_MIN_VISIBLE.0.min(width.max(0.0));
    screens.iter().any(|screen| {
        let visible_width = (x + width).min(screen.right()) - x.max(screen.x);
        let visible_height = (y + GRAB_BAND_HEIGHT).min(screen.bottom()) - y.max(screen.y);
        visible_width >= need_width && visible_height >= GRAB_MIN_VISIBLE.1
    })
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

    #[test]
    fn placement_round_trips_through_text() {
        let placements = [
            WindowPlacement {
                size: (800.0, 600.0),
                position: Some((-1280.5, 40.25)),
                state: WindowState::Fullscreen,
            },
            WindowPlacement {
                size: (1024.0, 768.0),
                position: None,
                state: WindowState::Normal,
            },
        ];
        for placement in placements {
            assert_eq!(
                WindowPlacement::parse(&placement.to_string()),
                Some(placement)
            );
        }
        assert_eq!(
            WindowPlacement {
                size: (1024.0, 768.0),
                position: None,
                state: WindowState::Normal,
            }
            .to_string(),
            "size=1024x768 state=normal"
        );
    }

    #[test]
    fn placement_text_is_lenient_about_order_and_unknown_keys() {
        assert_eq!(
            WindowPlacement::parse("  state=maximized display=2 size=640x480  "),
            Some(WindowPlacement {
                size: (640.0, 480.0),
                position: None,
                state: WindowState::Maximized,
            })
        );
    }

    #[test]
    fn placement_text_rejects_broken_values() {
        for text in [
            "",
            "position=1,2",
            "size=640",
            "size=640x",
            "size=NaNx480",
            "size=infx480",
            "size=640x480 state=huge",
            "size=640x480 position=1;2",
            "size=640x480 stray",
        ] {
            assert_eq!(WindowPlacement::parse(text), None, "{text:?}");
        }
    }

    #[test]
    fn on_screen_needs_the_top_edge_on_a_screen() {
        use crate::Rect;
        let screens = [
            Rect::new(0.0, 25.0, 1440.0, 875.0),
            Rect::new(1440.0, 0.0, 1920.0, 1080.0),
        ];
        assert!(is_on_screen((100.0, 100.0), 800.0, &screens));
        // 2 つの画面にまたがっていても、片方に十分出ていればよい。
        assert!(is_on_screen((1400.0, 100.0), 800.0, &screens));
        // 右の画面が外れた後の位置。
        assert!(!is_on_screen((1600.0, 100.0), 800.0, &screens[..1]));
        // 上端が画面の下へ出てしまった。
        assert!(!is_on_screen((100.0, 896.0), 800.0, &screens[..1]));
        // メニューバーの裏から始まっても、帯の大半が見えていればよい。
        assert!(is_on_screen((100.0, 0.0), 800.0, &screens[..1]));
        // 左へはみ出し、右端だけ少し見えている。
        assert!(!is_on_screen((-780.0, 100.0), 800.0, &screens[..1]));
        assert!(is_on_screen((-700.0, 100.0), 800.0, &screens[..1]));
        // 細いウィンドウは、幅全体が見えていればよい。
        assert!(is_on_screen((1400.0, 100.0), 40.0, &screens));
        assert!(!is_on_screen((100.0, 100.0), 800.0, &[]));
    }
}
