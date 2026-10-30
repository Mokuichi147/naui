//! Toasts (WinUI 3).
//!
//! The notification's look is WinUI 3's `InfoBar` itself. `Windows.UI.Notifications`
//! toasts appear **outside the app** (in the notification center), which is
//! different, so they are not used.
//!
//! `InfoBar` is meant to be a band laid out inside the page, but naui's toast is
//! overlaid, so it is moved to the bottom center and overlaid on the window
//! content's layer. It is overlaid below the title bar, in the same layer as the
//! app's content (the second row created by [`crate::window`]). A `Grid` places its
//! children in stacking order, so a toast added later appears above the content.
//!
//! The close button (×) is not shown (`IsClosable` is off). naui's toast disappears
//! through three things — timeout, the action button and [`Toast::dismiss`] — and
//! the other three platforms have no × either.
//!
//! `DispatcherQueueTimer` counts down until it disappears.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use naui_core::{Result, ToastSpec};
use naui_winui3::Microsoft::UI::Dispatching::{DispatcherQueue, DispatcherQueueTimer};
use naui_winui3::Microsoft::UI::Xaml::Controls::{
    Button, Grid, InfoBar, InfoBarSeverity, TextBlock,
};
use naui_winui3::Microsoft::UI::Xaml::{
    HorizontalAlignment, RoutedEventHandler, Thickness, UIElement, VerticalAlignment,
};
use windows::Foundation::TimeSpan;
use windows_core::{Interface, HSTRING};

use crate::to_error;
use crate::ui_thread::UiThreadCell;
use crate::window::owner_content_layer;

/// The overlay position: 24 away from the bottom center.
const TOAST_MARGIN: f64 = 24.0;

/// One millisecond of `TimeSpan` (in 100-nanosecond units).
const TICKS_PER_MILLI: i64 = 10_000;

thread_local! {
    /// The toast showing now. Only one is shown at a time, and a new one replaces it.
    static CURRENT: RefCell<Option<Toast>> = const { RefCell::new(None) };
}

struct ToastInner {
    /// The overlaid body.
    native: InfoBar,
    /// The action button. Attached to `ActionButton` only when it has text.
    button: Button,
    /// The action button's text. The `Button`'s content is a `TextBlock` (the same
    /// construction as other buttons).
    button_label: TextBlock,
    spec: RefCell<ToastSpec>,
    /// Where it is overlaid. Held only while shown.
    layer: RefCell<Option<Grid>>,
    /// The timer for disappearing automatically. Absent if it never disappears.
    timer: RefCell<Option<DispatcherQueueTimer>>,
    on_action: RefCell<Option<Box<dyn FnMut()>>>,
    on_dismiss: RefCell<Option<Box<dyn FnMut()>>>,
    visible: Cell<bool>,
}

/// A transient notification (an overlaid `InfoBar`).
///
/// It is not a widget, so it does not go into containers (the same as `Dialog`).
#[derive(Clone)]
pub struct Toast(Rc<ToastInner>);

impl Toast {
    pub(crate) fn new(message: &str) -> Result<Self> {
        let (native, button, button_label) = build_surface()?;
        native
            .SetMessage(&HSTRING::from(message))
            .map_err(|e| to_error("set the toast text", e))?;

        let this = Self(Rc::new(ToastInner {
            native,
            button,
            button_label,
            spec: RefCell::new(ToastSpec::new(message)),
            layer: RefCell::new(None),
            timer: RefCell::new(None),
            on_action: RefCell::new(None),
            on_dismiss: RefCell::new(None),
            visible: Cell::new(false),
        }));
        this.install_action_handler()?;
        Ok(this)
    }

    /// Makes clicking the action button notify and dismiss the toast.
    fn install_action_handler(&self) -> Result<()> {
        let weak = UiThreadCell::new(Rc::downgrade(&self.0));
        let handler = RoutedEventHandler::new(move |_sender, _args| {
            weak.try_with_mut(|weak: &mut Weak<ToastInner>| {
                if let Some(inner) = weak.upgrade() {
                    Toast(inner).finish(true);
                }
            });
            Ok(())
        });
        self.0
            .button
            .Click(&handler)
            .map_err(|e| to_error("subscribe to the toast button", e))?;
        Ok(())
    }

    /// The string to show. Calling it while shown updates it on the spot.
    pub fn set_message(&self, message: &str) {
        self.0.spec.borrow_mut().set_message(message);
        let _ = self.0.native.SetMessage(&HSTRING::from(message));
    }

    pub fn message(&self) -> String {
        self.0.spec.borrow().message().to_string()
    }

    /// The action button's label. **Passing an empty string removes the button.**
    pub fn set_action(&self, label: &str) {
        self.0.spec.borrow_mut().set_action(label);
        let action = self.0.spec.borrow().action().map(str::to_string);
        match action {
            Some(label) => {
                let _ = self.0.button_label.SetText(&HSTRING::from(label));
                let _ = self.0.native.SetActionButton(&self.0.button);
            }
            None => {
                let _ = self.0.native.SetActionButton(None);
            }
        }
    }

    /// The action button's label. The empty string if there is no button.
    pub fn action(&self) -> String {
        self.0
            .spec
            .borrow()
            .action()
            .unwrap_or_default()
            .to_string()
    }

    /// Seconds until it disappears automatically. **Passing 0 means it never
    /// disappears automatically.**
    ///
    /// Takes effect from the next [`show`](Self::show).
    pub fn set_timeout(&self, seconds: f64) {
        self.0.spec.borrow_mut().set_timeout(seconds);
    }

    pub fn timeout(&self) -> f64 {
        self.0.spec.borrow().timeout()
    }

    /// The current settings.
    pub fn spec(&self) -> ToastSpec {
        self.0.spec.borrow().clone()
    }

    /// Called when the action button is clicked. Setting it again removes the previous
    /// one.
    ///
    /// Clicking it makes the toast disappear, so `on_dismiss` is called next.
    pub fn on_action(&self, f: impl FnMut() + 'static) {
        *self.0.on_action.borrow_mut() = Some(Box::new(f));
    }

    /// Called when it disappears. Setting it again removes the previous one.
    ///
    /// It is called **when it times out** and **when it is closed by the action
    /// button**. It is not called when it is closed with [`dismiss`](Self::dismiss) or
    /// replaced by another toast (the same rule as
    /// [`Dialog::close`](crate::Dialog::close): the app's own actions are not
    /// notified).
    pub fn on_dismiss(&self, f: impl FnMut() + 'static) {
        *self.0.on_dismiss.borrow_mut() = Some(Box::new(f));
    }

    /// Shows the toast.
    ///
    /// **Only one is shown at a time**; if another toast is showing, it is replaced
    /// (the replaced one's `on_dismiss` is not called). Does nothing if no window has
    /// been shown yet (there is nowhere to overlay it, the same as `Dialog`).
    pub fn show(&self) {
        let Some(layer) = owner_content_layer() else {
            return;
        };
        if let Some(previous) = CURRENT.with(|slot| slot.borrow_mut().take()) {
            previous.take_down();
        }
        let element = match self.0.native.cast::<UIElement>() {
            Ok(element) => element,
            Err(_) => return,
        };
        if layer
            .Children()
            .and_then(|children| children.Append(&element))
            .is_err()
        {
            return;
        }
        *self.0.layer.borrow_mut() = Some(layer);
        let _ = self.0.native.SetIsOpen(true);
        self.0.visible.set(true);
        self.start_timer();
        CURRENT.with(|slot| *slot.borrow_mut() = Some(self.clone()));
    }

    /// Closes the toast being shown. `on_dismiss` is not called.
    pub fn dismiss(&self) {
        if !self.is_visible() {
            return;
        }
        self.forget_current();
        self.take_down();
    }

    /// Whether it is showing now.
    pub fn is_visible(&self) -> bool {
        self.0.visible.get()
    }

    /// The overlaid element. A backend-specific escape hatch.
    pub fn native_element(&self) -> UIElement {
        self.0
            .native
            .cast::<UIElement>()
            .expect("an InfoBar is a UIElement")
    }

    /// Starts counting down. Does nothing if it never disappears.
    fn start_timer(&self) {
        let Some(millis) = self.0.spec.borrow().timeout_millis() else {
            return;
        };
        let Ok(queue) = DispatcherQueue::GetForCurrentThread() else {
            return;
        };
        let Ok(timer) = queue.CreateTimer() else {
            return;
        };
        let interval = TimeSpan {
            Duration: i64::from(millis) * TICKS_PER_MILLI,
        };
        if timer.SetInterval(interval).is_err() || timer.SetIsRepeating(false).is_err() {
            return;
        }
        let weak = UiThreadCell::new(Rc::downgrade(&self.0));
        let handler = windows::Foundation::TypedEventHandler::<
            DispatcherQueueTimer,
            windows_core::IInspectable,
        >::new(move |_sender, _args| {
            weak.try_with_mut(|weak: &mut Weak<ToastInner>| {
                if let Some(inner) = weak.upgrade() {
                    Toast(inner).finish(false);
                }
            });
            Ok(())
        });
        if timer.Tick(&handler).is_err() || timer.Start().is_err() {
            return;
        }
        *self.0.timer.borrow_mut() = Some(timer);
    }

    /// Tells the app it disappeared. `action` is whether the action button closed it.
    fn finish(&self, action: bool) {
        if !self.is_visible() {
            return;
        }
        self.forget_current();
        self.take_down();
        if action {
            emit(&self.0.on_action);
        }
        emit(&self.0.on_dismiss);
    }

    /// Removes the overlaid element and stops the timer. No notification.
    fn take_down(&self) {
        if let Some(timer) = self.0.timer.borrow_mut().take() {
            let _ = timer.Stop();
        }
        self.0.visible.set(false);
        let _ = self.0.native.SetIsOpen(false);
        let Ok(element) = self.0.native.cast::<UIElement>() else {
            return;
        };
        // 重ねた先から外す。出したあとにウィンドウが変わっても、
        // 足した側の入れ物から確実に取れるように覚えておく。
        let Some(layer) = self.0.layer.borrow_mut().take() else {
            return;
        };
        let Ok(children) = layer.Children() else {
            return;
        };
        let mut index = 0;
        if children.IndexOf(&element, &mut index).unwrap_or(false) {
            let _ = children.RemoveAt(index);
        }
    }

    /// If "the toast showing now" is this one, vacates that slot.
    fn forget_current(&self) {
        CURRENT.with(|slot| {
            let mine = slot
                .borrow()
                .as_ref()
                .is_some_and(|current| Rc::ptr_eq(&current.0, &self.0));
            if mine {
                slot.borrow_mut().take();
            }
        });
    }
}

/// Notification. So that setting it again from inside the notification does not
/// double-borrow, the closure is taken out only for the duration of the call.
fn emit(slot: &RefCell<Option<Box<dyn FnMut()>>>) {
    let Some(mut f) = slot.borrow_mut().take() else {
        return;
    };
    f();
    let mut slot = slot.borrow_mut();
    if slot.is_none() {
        *slot = Some(f);
    }
}

/// Creates the overlaid `InfoBar` and the action button.
///
/// The action button goes in `ActionButton`, so WinUI decides its layout inside the
/// `InfoBar` (right of the text, or below when narrow). Making its content a
/// `TextBlock` is the same construction as other buttons.
fn build_surface() -> Result<(InfoBar, Button, TextBlock)> {
    let native = InfoBar::new().map_err(|e| to_error("create the toast", e))?;
    native
        .SetSeverity(InfoBarSeverity::Informational)
        .map_err(|e| to_error("set the toast severity", e))?;
    // × は出さない。消えるのは時間切れ・操作ボタン・dismiss の 3 つだけ。
    native
        .SetIsClosable(false)
        .map_err(|e| to_error("set the toast close button", e))?;
    native
        .SetHorizontalAlignment(HorizontalAlignment::Center)
        .and_then(|()| native.SetVerticalAlignment(VerticalAlignment::Bottom))
        .and_then(|()| {
            native.SetMargin(Thickness {
                Left: TOAST_MARGIN,
                Top: TOAST_MARGIN,
                Right: TOAST_MARGIN,
                Bottom: TOAST_MARGIN,
            })
        })
        .map_err(|e| to_error("set the toast alignment", e))?;

    let button = Button::new().map_err(|e| to_error("create the toast button", e))?;
    let button_label = TextBlock::new().map_err(|e| to_error("create the toast button text", e))?;
    button
        .SetContent(&button_label)
        .map_err(|e| to_error("place the toast button text", e))?;
    Ok((native, button, button_label))
}
