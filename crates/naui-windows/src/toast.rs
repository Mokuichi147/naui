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
//! `DispatcherQueueTimer` counts down until it disappears. Which toast is on screen
//! and which wait is decided by [`naui_core::ToastQueue`], shared with the other
//! backends.
//!
//! One action button goes in `ActionButton`, the place WinUI gives it (right of the
//! text, or below when narrow). `InfoBar` has room for only one there, so with two or
//! more they all go in a row in `Content`, which WinUI lays out on its own line
//! below the text (where WinUI's guidance puts additional buttons). The position is
//! the `InfoBar`'s alignment in the layer it is overlaid on; `Left` and `Right` are
//! mirrored by a right-to-left `FlowDirection`, so they are the start and end.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use naui_core::{Align, Result, ToastPosition, ToastPriority, ToastQueue, ToastSlot, ToastSpec};
use naui_winui3::Microsoft::UI::Dispatching::{DispatcherQueue, DispatcherQueueTimer};
use naui_winui3::Microsoft::UI::Xaml::Controls::{
    Button, Grid, InfoBar, InfoBarSeverity, Orientation as XamlOrientation, StackPanel, TextBlock,
};
use naui_winui3::Microsoft::UI::Xaml::Markup::XamlReader;
use naui_winui3::Microsoft::UI::Xaml::{
    HorizontalAlignment, RoutedEventHandler, Thickness, UIElement, VerticalAlignment,
};
use windows::Foundation::TimeSpan;
use windows_core::{Interface, HSTRING};

use crate::to_error;
use crate::ui_thread::UiThreadCell;
use crate::window::owner_content_layer;

/// The distance from the edges of the layer it is overlaid on.
const TOAST_MARGIN: f64 = 24.0;
/// The spacing between action buttons in a row.
const BUTTON_SPACING: f64 = 8.0;

/// The overlaid `InfoBar`, with an opaque background.
///
/// `InfoBar`'s own backgrounds are made for a band laid inside the page:
/// `Informational`'s is nearly transparent (`#08FFFFFF` in the dark theme,
/// `#80F6F6F6` in the light one), so overlaid on content (a light `Canvas`, an
/// image, text) the toast became hard to read. Its `Background` is drawn over the
/// severity color, so the solid fill for raised surfaces is set there. It is
/// written as a `ThemeResource` so that it follows theme changes.
const TOAST_XAML: &str = r##"<InfoBar
    xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
    Background="{ThemeResource SolidBackgroundFillColorQuarternaryBrush}"/>"##;

/// One millisecond of `TimeSpan` (in 100-nanosecond units).
const TICKS_PER_MILLI: i64 = 10_000;

thread_local! {
    /// The toast on screen and the ones waiting their turn.
    static QUEUE: RefCell<ToastQueue<ToastInner>> = const { RefCell::new(ToastQueue::new()) };
}

/// The slot for one notification closure.
type Handler<A> = RefCell<Option<Box<dyn FnMut(A)>>>;

struct ToastInner {
    /// The overlaid body.
    native: InfoBar,
    /// The action buttons, one per index, with their text (a `TextBlock`, the same
    /// construction as other buttons). **They only ever grow**: each is created with
    /// its `Click` handler once and reused, so replacing the buttons from inside
    /// `on_action` does not drop the handler that is running.
    buttons: RefCell<Vec<(Button, TextBlock)>>,
    /// The row in `Content` holding the buttons when there are two or more.
    row: StackPanel,
    spec: RefCell<ToastSpec>,
    /// Where it is overlaid. Held only while shown.
    layer: RefCell<Option<Grid>>,
    /// The timer for disappearing automatically. Absent if it never disappears.
    timer: RefCell<Option<DispatcherQueueTimer>>,
    on_action: Handler<usize>,
    on_dismiss: Handler<()>,
    visible: Cell<bool>,
    /// Whether the window it is on screen in has been closed ([`window_closing`]).
    stranded: Cell<bool>,
}

impl ToastSlot for ToastInner {
    fn present(this: &Rc<Self>) -> bool {
        let Some(layer) = owner_content_layer() else {
            return false;
        };
        let toast = Toast(this.clone());
        let Ok(element) = this.native.cast::<UIElement>() else {
            return false;
        };
        toast.place();
        if layer
            .Children()
            .and_then(|children| children.Append(&element))
            .is_err()
        {
            return false;
        }
        *this.layer.borrow_mut() = Some(layer);
        this.stranded.set(false);
        let _ = this.native.SetIsOpen(true);
        this.visible.set(true);
        toast.start_timer();
        true
    }

    fn withdraw(this: &Rc<Self>) {
        if let Some(timer) = this.timer.borrow_mut().take() {
            let _ = timer.Stop();
        }
        this.visible.set(false);
        let _ = this.native.SetIsOpen(false);
        let Ok(element) = this.native.cast::<UIElement>() else {
            return;
        };
        // 重ねた先から外す。出したあとにウィンドウが変わっても、
        // 足した側の入れ物から確実に取れるように覚えておく。
        let Some(layer) = this.layer.borrow_mut().take() else {
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

    fn is_stranded(this: &Rc<Self>) -> bool {
        this.stranded.get()
    }
}

/// A transient notification (an overlaid `InfoBar`).
///
/// It is not a widget, so it does not go into containers (the same as `Dialog`).
#[derive(Clone)]
pub struct Toast(Rc<ToastInner>);

impl Toast {
    pub(crate) fn new(message: &str) -> Result<Self> {
        let native = build_surface()?;
        native
            .SetMessage(&HSTRING::from(message))
            .map_err(|e| to_error("set the toast text", e))?;
        let row = StackPanel::new().map_err(|e| to_error("create the toast button row", e))?;
        row.SetOrientation(XamlOrientation::Horizontal)
            .and_then(|()| row.SetSpacing(BUTTON_SPACING))
            .map_err(|e| to_error("set the toast button row", e))?;

        let this = Self(Rc::new(ToastInner {
            native,
            buttons: RefCell::new(Vec::new()),
            row,
            spec: RefCell::new(ToastSpec::new(message)),
            layer: RefCell::new(None),
            timer: RefCell::new(None),
            on_action: RefCell::new(None),
            on_dismiss: RefCell::new(None),
            visible: Cell::new(false),
            stranded: Cell::new(false),
        }));
        Ok(this)
    }

    /// The button for `index`, created with its `Click` handler on first use.
    fn button(&self, index: usize) -> Option<(Button, TextBlock)> {
        let mut buttons = self.0.buttons.borrow_mut();
        while buttons.len() <= index {
            let i = buttons.len();
            let button = Button::new().ok()?;
            let label = TextBlock::new().ok()?;
            button.SetContent(&label).ok()?;
            let weak = UiThreadCell::new(Rc::downgrade(&self.0));
            let handler = RoutedEventHandler::new(move |_sender, _args| {
                weak.try_with_mut(|weak: &mut Weak<ToastInner>| {
                    if let Some(inner) = weak.upgrade() {
                        Toast(inner).finish(Some(i));
                    }
                });
                Ok(())
            });
            button.Click(&handler).ok()?;
            buttons.push((button, label));
        }
        buttons.get(index).cloned()
    }

    /// Puts the action buttons in place: one in `ActionButton`, two or more in a row
    /// in `Content`.
    fn apply_actions(&self, labels: &[String]) {
        let native = &self.0.native;
        // ボタンを付け替える前に、両方の置き場から外しておく (親は 1 つだけ)。
        let _ = native.SetActionButton(None);
        let _ = native.SetContent(None::<&windows_core::IInspectable>);
        if let Ok(children) = self.0.row.Children() {
            let _ = children.Clear();
        }
        let buttons: Vec<Button> = labels
            .iter()
            .enumerate()
            .filter_map(|(index, text)| {
                let (button, label) = self.button(index)?;
                let _ = label.SetText(&HSTRING::from(text.as_str()));
                Some(button)
            })
            .collect();
        match buttons.as_slice() {
            [] => {}
            [only] => {
                let _ = native.SetActionButton(only);
            }
            many => {
                if let Ok(children) = self.0.row.Children() {
                    for button in many {
                        if let Ok(element) = button.cast::<UIElement>() {
                            let _ = children.Append(&element);
                        }
                    }
                }
                let _ = native.SetContent(&self.0.row);
            }
        }
    }

    /// Aligns the `InfoBar` in the layer it is overlaid on.
    fn place(&self) {
        let position = self.0.spec.borrow().position();
        let horizontal = match position.align() {
            Align::Start => HorizontalAlignment::Left,
            Align::End => HorizontalAlignment::Right,
            _ => HorizontalAlignment::Center,
        };
        let vertical = if position.is_top() {
            VerticalAlignment::Top
        } else {
            VerticalAlignment::Bottom
        };
        let _ = self.0.native.SetHorizontalAlignment(horizontal);
        let _ = self.0.native.SetVerticalAlignment(vertical);
    }

    /// The string to show. Calling it while shown updates it on the spot.
    pub fn set_message(&self, message: &str) {
        self.0.spec.borrow_mut().set_message(message);
        let _ = self.0.native.SetMessage(&HSTRING::from(message));
    }

    pub fn message(&self) -> String {
        self.0.spec.borrow().message().to_string()
    }

    /// The action buttons' labels, laid out in this order. **Passing an empty list
    /// removes the buttons.**
    ///
    /// Calling it while shown replaces them on the spot.
    pub fn set_actions<S: AsRef<str>>(&self, labels: &[S]) {
        self.0.spec.borrow_mut().set_actions(labels);
        let labels = self.0.spec.borrow().actions().to_vec();
        self.apply_actions(&labels);
    }

    /// The action buttons' labels. Empty if there are none.
    pub fn actions(&self) -> Vec<String> {
        self.0.spec.borrow().actions().to_vec()
    }

    /// Seconds until it disappears automatically. **Passing 0 means it never
    /// disappears automatically.**
    ///
    /// Takes effect from the next time it comes up.
    pub fn set_timeout(&self, seconds: f64) {
        self.0.spec.borrow_mut().set_timeout(seconds);
    }

    pub fn timeout(&self) -> f64 {
        self.0.spec.borrow().timeout()
    }

    /// How it takes its turn when shown. Takes effect from the next
    /// [`show`](Self::show).
    pub fn set_priority(&self, priority: ToastPriority) {
        self.0.spec.borrow_mut().set_priority(priority);
    }

    pub fn priority(&self) -> ToastPriority {
        self.0.spec.borrow().priority()
    }

    /// Where in the window it appears. Takes effect from the next time it comes up.
    pub fn set_position(&self, position: ToastPosition) {
        self.0.spec.borrow_mut().set_position(position);
    }

    pub fn position(&self) -> ToastPosition {
        self.0.spec.borrow().position()
    }

    /// The current settings.
    pub fn spec(&self) -> ToastSpec {
        self.0.spec.borrow().clone()
    }

    /// Called with the index of the clicked action button. Setting it again removes
    /// the previous one.
    ///
    /// Clicking it makes the toast disappear, so `on_dismiss` is called next.
    pub fn on_action(&self, f: impl FnMut(usize) + 'static) {
        *self.0.on_action.borrow_mut() = Some(Box::new(f));
    }

    /// Called when it disappears. Setting it again removes the previous one.
    ///
    /// It is called **when it times out** and **when it is closed by an action
    /// button**. It is not called when it is closed with [`dismiss`](Self::dismiss)
    /// or put back in the queue by a `High` toast (the same rule as
    /// [`Dialog::close`](crate::Dialog::close): the app's own actions are not
    /// notified).
    pub fn on_dismiss(&self, mut f: impl FnMut() + 'static) {
        *self.0.on_dismiss.borrow_mut() = Some(Box::new(move |()| f()));
    }

    /// Shows the toast: on screen at once, or at the back of the queue (see
    /// [`naui_core::ToastQueue`]). Dropped if no window has been shown when its turn
    /// comes (there is nowhere to overlay it, the same as `Dialog`).
    pub fn show(&self) {
        let priority = self.0.spec.borrow().priority();
        ToastQueue::show(&QUEUE, &self.0, priority);
    }

    /// Closes the toast, or takes it out of the queue. `on_dismiss` is not called.
    pub fn dismiss(&self) {
        ToastQueue::remove(&QUEUE, &self.0);
    }

    /// Whether it is waiting its turn in the queue.
    pub fn is_queued(&self) -> bool {
        ToastQueue::is_waiting_in(&QUEUE, &self.0)
    }

    /// Whether it is on screen now.
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
                    Toast(inner).finish(None);
                }
            });
            Ok(())
        });
        if timer.Tick(&handler).is_err() || timer.Start().is_err() {
            return;
        }
        *self.0.timer.borrow_mut() = Some(timer);
    }

    /// Tells the app it disappeared by itself. `action` is the index of the action
    /// button that closed it, if one did.
    ///
    /// The next toast in the queue comes up before the app is told.
    fn finish(&self, action: Option<usize>) {
        if !self.is_visible() {
            return;
        }
        ToastQueue::remove(&QUEUE, &self.0);
        if let Some(index) = action {
            emit(&self.0.on_action, index);
        }
        emit(&self.0.on_dismiss, ());
    }
}

/// Notification. So that setting it again from inside the notification does not
/// double-borrow, the closure is taken out only for the duration of the call.
fn emit<A>(slot: &Handler<A>, arg: A) {
    let Some(mut f) = slot.borrow_mut().take() else {
        return;
    };
    f(arg);
    let mut slot = slot.borrow_mut();
    if slot.is_none() {
        *slot = Some(f);
    }
}

/// Creates the overlaid `InfoBar`. Its alignment is set each time it comes up.
fn build_surface() -> Result<InfoBar> {
    let native = XamlReader::Load(&HSTRING::from(TOAST_XAML))
        .and_then(|element| element.cast::<InfoBar>())
        .map_err(|e| to_error("create the toast", e))?;
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
    Ok(native)
}

/// Called when the window whose content layer is `layer` is closed with
/// `Window::close`. A toast on screen there is marked, so that the next `show()`
/// takes it down instead of waiting behind it (a toast that never disappears by
/// itself would wait for good).
///
/// Closing a window from its close button winds the whole app down, so only
/// `Window::close` needs this.
pub(crate) fn window_closing(layer: &Grid) {
    let Some(shown) = QUEUE.with(|q| q.borrow().shown()) else {
        return;
    };
    if shown.layer.borrow().as_ref() == Some(layer) {
        shown.stranded.set(true);
    }
}
