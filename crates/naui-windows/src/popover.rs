//! Popovers (WinUI 3's native `Flyout`).
//!
//! The content widget becomes the `Flyout`'s `Content`, and [`Popover::open`] shows
//! it with `ShowAt` against the widget's element. The presenter's look, light
//! dismiss (a click outside, Esc) and moving to another side when there is no room
//! are all `Flyout`'s own. naui only chooses the `Placement`.
//!
//! Closing a `Flyout` is asynchronous: `Hide` (or `ShowAt` against another element)
//! clears `IsOpen` on the spot, but `Closed` arrives only after the close finishes.
//! A `ShowAt` in between is queued by `Flyout` and runs after `Closed`, and a later
//! `Hide` does not cancel it. So naui never calls `ShowAt` while the flyout is shown
//! or closing. Moving it (another widget, another side) hides it and shows it again
//! on the turn of the loop after `Closed` (inside `Closed`, `ShowAt` against the same
//! element does not open it).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use naui_core::{PopoverPosition, Result};
use naui_winui3::Microsoft::UI::Dispatching::{DispatcherQueue, DispatcherQueueHandler};
use naui_winui3::Microsoft::UI::Xaml::Controls::Flyout;
use naui_winui3::Microsoft::UI::Xaml::Controls::Primitives::FlyoutPlacementMode;
use naui_winui3::Microsoft::UI::Xaml::FrameworkElement;
use windows::Foundation::EventHandler;
use windows_core::{IInspectable, Interface};

use crate::list::ActivationHandler;
use crate::to_error;
use crate::ui_thread::UiThreadCell;
use crate::widgets::Widget;

struct PopoverInner {
    flyout: Flyout,
    /// The content's handle. Kept alive along with its callbacks.
    child: RefCell<Option<Box<dyn Widget>>>,
    /// The widget it is shown beside. Held while open so it can be shown there again
    /// when the position changes.
    anchor: RefCell<Option<Box<dyn Widget>>>,
    position: Cell<PopoverPosition>,
    /// Whether it is open as far as naui knows. Cleared by `close` before `Hide`, so
    /// the `Closed` that follows is not passed on.
    open: Cell<bool>,
    /// Whether `ShowAt` has been called and its `Closed` has not arrived yet (the
    /// flyout is shown or still closing).
    shown: Cell<bool>,
    /// Set when it was hidden only to be shown again (beside another widget, or on
    /// another side). The `Closed` that follows shows it again instead of notifying.
    reshow: Cell<bool>,
    /// Set between that `Closed` and the next turn of the loop, where it is shown
    /// again. The flyout is already closed, so `Hide` would do nothing.
    queued: Cell<bool>,
    on_close: ActivationHandler,
}

impl PopoverInner {
    /// Shows it beside `anchor` with `ShowAt`. Only while it is neither shown nor
    /// closing. Gives up being open when that is not possible.
    fn show(&self) {
        let target = self
            .anchor
            .borrow()
            .as_ref()
            .and_then(|anchor| anchor.native_element().cast::<FrameworkElement>().ok());
        let shown = target.is_some_and(|target| {
            target.IsLoaded().unwrap_or(false) && self.flyout.ShowAt(&target).is_ok()
        });
        self.shown.set(shown);
        if !shown {
            self.open.set(false);
            self.anchor.borrow_mut().take();
        }
    }

    /// Hides it and shows it again beside `anchor` once `Closed` arrives.
    fn show_again(&self) {
        // 出し直しを待っているなら、その時に今の `anchor` で出る。
        if self.queued.get() {
            return;
        }
        self.reshow.set(true);
        // 閉じかけているなら `Hide` は何もせず、すでに来る予定の `Closed` を待つ。
        let _ = self.flyout.Hide();
    }
}

/// A panel shown beside a widget (`Flyout`).
///
/// It is not a widget laid out on screen, so it is not a [`Widget`]. [`crate::Ui`]
/// holds the popovers it creates, so dropping the return value does not stop
/// notifications.
#[derive(Clone)]
pub struct Popover(Rc<PopoverInner>);

impl Popover {
    pub(crate) fn new() -> Result<Self> {
        let flyout = Flyout::new().map_err(|e| to_error("create the Flyout", e))?;
        let this = Self(Rc::new(PopoverInner {
            flyout,
            child: RefCell::new(None),
            anchor: RefCell::new(None),
            position: Cell::new(PopoverPosition::default()),
            open: Cell::new(false),
            shown: Cell::new(false),
            reshow: Cell::new(false),
            queued: Cell::new(false),
            on_close: ActivationHandler::new(),
        }));
        this.apply_placement();

        let state = UiThreadCell::new(Rc::downgrade(&this.0));
        let closed = EventHandler::<IInspectable>::new(move |_, _| {
            let inner = state.try_with_mut(|weak| weak.upgrade()).flatten();
            if let Some(inner) = inner {
                let reshow = inner.reshow.replace(false);
                // `close` が先に下ろしていれば、アプリが閉じたので知らせない。
                if !inner.open.get() {
                    inner.shown.set(false);
                    return Ok(());
                }
                // 移すために隠しただけなら、閉じ終えたあとで新しい場所に出し直す。
                // `Closed` の中で同じ要素へ `ShowAt` しても開き直らないので、
                // 抜けてからにする。それまでは閉じかけと同じに扱う。
                if reshow {
                    let queued = DispatcherQueue::GetForCurrentThread().is_ok_and(|queue| {
                        let weak = Rc::downgrade(&inner);
                        let state = UiThreadCell::new(weak);
                        queue
                            .TryEnqueue(&DispatcherQueueHandler::new(move || {
                                let inner = state.try_with_mut(|weak| weak.upgrade()).flatten();
                                if let Some(inner) = inner {
                                    inner.queued.set(false);
                                    inner.shown.set(false);
                                    inner.reshow.set(false);
                                    if inner.open.get() {
                                        inner.show();
                                    }
                                }
                                Ok(())
                            }))
                            .is_ok()
                    });
                    inner.queued.set(queued);
                    if !queued {
                        inner.shown.set(false);
                        inner.show();
                    }
                    return Ok(());
                }
                inner.shown.set(false);
                inner.open.set(false);
                inner.anchor.borrow_mut().take();
                inner.on_close.emit();
            }
            Ok(())
        });
        this.0
            .flyout
            .Closed(&closed)
            .map_err(|e| to_error("subscribe to the Flyout closing", e))?;
        Ok(this)
    }

    /// The content. Each call replaces it.
    pub fn set_child(&self, child: &dyn Widget) {
        if self.0.flyout.SetContent(None).is_err() {
            return;
        }
        let element = child.native_element();
        if self.0.flyout.SetContent(&element).is_ok() {
            *self.0.child.borrow_mut() = Some(child.boxed_clone());
        }
    }

    /// The side it prefers to appear on. Bottom by default.
    pub fn set_position(&self, position: PopoverPosition) {
        if self.0.position.replace(position) == position {
            return;
        }
        self.apply_placement();
        // `Placement` は次に出すときに効くので、出ている間は同じ場所で出し直す。
        if self.0.open.get() && self.0.shown.get() {
            self.0.show_again();
        }
    }

    pub fn position(&self) -> PopoverPosition {
        self.0.position.get()
    }

    fn apply_placement(&self) {
        let mode = match self.0.position.get() {
            PopoverPosition::Top => FlyoutPlacementMode::Top,
            PopoverPosition::Bottom => FlyoutPlacementMode::Bottom,
            PopoverPosition::Left => FlyoutPlacementMode::Left,
            PopoverPosition::Right => FlyoutPlacementMode::Right,
        };
        let _ = self.0.flyout.SetPlacement(mode);
    }

    /// Shows it beside `widget`. If it is already open, it moves to `widget`.
    ///
    /// Does nothing while `widget` is not in a shown window (`ShowAt` needs an element
    /// in the visual tree).
    pub fn open(&self, widget: &dyn Widget) {
        let Ok(target) = widget.native_element().cast::<FrameworkElement>() else {
            return;
        };
        if !target.IsLoaded().unwrap_or(false) {
            return;
        }
        // 同じウィジェットの横にもう出ているなら、そのままでよい。
        let in_place = self.0.open.get()
            && !self.0.reshow.get()
            && self.0.flyout.IsOpen().unwrap_or(false)
            && self.0.flyout.Target().ok().as_ref() == Some(&target);
        self.0.open.set(true);
        *self.0.anchor.borrow_mut() = Some(widget.boxed_clone());
        if in_place {
            return;
        }
        if self.0.shown.get() {
            self.0.show_again();
        } else {
            self.0.show();
        }
    }

    /// Closes it. `on_close` is not called. Does nothing if it is not open.
    pub fn close(&self) {
        if !self.0.open.replace(false) {
            return;
        }
        self.0.anchor.borrow_mut().take();
        self.0.reshow.set(false);
        let _ = self.0.flyout.Hide();
    }

    /// Whether it is open.
    pub fn is_open(&self) -> bool {
        self.0.open.get()
    }

    /// Called when the user closes it (a click outside, Esc).
    ///
    /// Not called when it is closed with [`close`](Self::close). Setting it again
    /// replaces the previous closure.
    pub fn on_close(&self, f: impl FnMut() + 'static) {
        self.0.on_close.set(f);
    }

    /// Closes it the way a click outside does, notifying `on_close` (for tests and
    /// automation).
    pub fn dismiss(&self) {
        if !self.0.open.get() {
            return;
        }
        // 出し直しを待っている間はもう閉じているので、`Closed` は来ない。ここで閉じる。
        if self.0.queued.get() {
            self.0.open.set(false);
            self.0.anchor.borrow_mut().take();
            self.0.on_close.emit();
            return;
        }
        self.0.reshow.set(false);
        let _ = self.0.flyout.Hide();
    }

    /// The `Flyout` itself. Public as a backend-specific escape hatch.
    pub fn native_flyout(&self) -> Flyout {
        self.0.flyout.clone()
    }
}
