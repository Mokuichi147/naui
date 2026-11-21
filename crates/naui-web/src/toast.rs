//! Toasts (DOM).
//!
//! Browsers have no standard element for "a notification shown for a while that
//! then disappears by itself" (the `Notification` API shows things outside the
//! page, which is different). So, as with [`PopupMenu`](crate::PopupMenu), it is
//! built from a `<div role="status">` and `<button>`s.
//!
//! Colors use CSS system colors (`Canvas` / `CanvasText`), so they follow the
//! `color-scheme` that [`crate::Ui::set_theme`] sets on `<html>` as is. CSS decides
//! only the position, padding and corner radius. The position is written with
//! logical properties (`inset-inline-start` and the like), so the start and end
//! sides swap in a right-to-left page.
//!
//! Screen reader announcements are left to `role="status"` (`aria-live="polite"`).
//! Once the text is in, it is read without interrupting what is being read.
//!
//! `setTimeout` counts down until it disappears. Which toast is on screen and which
//! wait is decided by [`naui_core::ToastQueue`], shared with the other backends.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use naui_core::{
    Align, Error, Result, ToastPosition, ToastPriority, ToastQueue, ToastSlot, ToastSpec,
};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use web_sys::{Document, Element, HtmlElement};

use crate::to_error;
use crate::widgets::{create, Listener};

/// The distance from the window's edges.
const EDGE_MARGIN: &str = "24px";

thread_local! {
    /// The toast on screen and the ones waiting their turn.
    static QUEUE: RefCell<ToastQueue<ToastInner>> = const { RefCell::new(ToastQueue::new()) };
}

fn style(element: &HtmlElement, property: &str, value: &str) {
    let _ = element.style().set_property(property, value);
}

/// The slot for one notification closure.
type Handler<A> = RefCell<Option<Box<dyn FnMut(A)>>>;

struct ToastInner {
    doc: Document,
    /// The toast itself. Placed directly under `<body>`, with `display: none` while not
    /// on screen.
    element: HtmlElement,
    /// The `<span>` showing the text.
    label: HtmlElement,
    /// The `<span>` holding the action buttons, in order.
    buttons: HtmlElement,
    spec: RefCell<ToastSpec>,
    /// The buttons' click subscription. **One, on `buttons`**, so that replacing the
    /// buttons from inside `on_action` does not drop the closure that is running.
    _click: RefCell<Option<Listener>>,
    /// The closure that receives the timeout from the browser. **Kept alive as long as
    /// the toast.**
    ///
    /// It is not recreated every time it is shown so that dismissing from inside it
    /// **does not free the closure that is currently running**.
    tick: RefCell<Option<Closure<dyn FnMut()>>>,
    /// The `setTimeout` reservation. Absent if it never disappears.
    timeout: Cell<Option<i32>>,
    on_action: Handler<usize>,
    on_dismiss: Handler<()>,
    visible: Cell<bool>,
}

impl ToastSlot for ToastInner {
    fn present(this: &Rc<Self>) -> bool {
        let toast = Toast(this.clone());
        toast.place();
        style(&this.element, "display", "flex");
        this.visible.set(true);
        toast.start_timeout();
        true
    }

    fn withdraw(this: &Rc<Self>) {
        if let (Some(handle), Some(window)) = (this.timeout.take(), web_sys::window()) {
            window.clear_timeout_with_handle(handle);
        }
        style(&this.element, "display", "none");
        this.visible.set(false);
    }
}

/// A transient notification (`<div role="status">`).
///
/// It is not a widget laid out on screen, so it is not a
/// [`Widget`](crate::Widget).
#[derive(Clone)]
pub struct Toast(Rc<ToastInner>);

impl Toast {
    pub(crate) fn new(doc: &Document, message: &str) -> Result<Self> {
        let element: HtmlElement = create(doc, "div")?.unchecked_into();
        element
            .set_attribute("role", "status")
            .map_err(|e| to_error("build the toast", e))?;
        // 位置と重なりだけを CSS で作る。色はシステムカラーに任せる。
        style(&element, "position", "fixed");
        style(&element, "display", "none");
        style(&element, "z-index", "1000");
        style(&element, "align-items", "center");
        style(&element, "gap", "12px");
        style(&element, "padding", "10px 16px");
        style(&element, "width", "fit-content");
        style(&element, "max-width", "calc(100% - 48px)");
        style(&element, "box-sizing", "border-box");
        style(&element, "background", "Canvas");
        style(&element, "color", "CanvasText");
        style(&element, "border", "1px solid CanvasText");
        style(&element, "border-radius", "10px");
        style(&element, "box-shadow", "0 4px 12px rgba(0, 0, 0, 0.25)");

        let label: HtmlElement = create(doc, "span")?.unchecked_into();
        label.set_text_content(Some(message));
        let buttons: HtmlElement = create(doc, "span")?.unchecked_into();
        style(&buttons, "display", "flex");
        style(&buttons, "gap", "8px");
        style(&buttons, "flex-shrink", "0");
        for part in [&label, &buttons] {
            element
                .append_child(part)
                .map_err(|e| to_error("build the toast", e))?;
        }

        doc.body()
            .ok_or_else(|| Error::new("place the toast", "there is no body element"))?
            .append_child(&element)
            .map_err(|e| to_error("place the toast", e))?;

        let this = Self(Rc::new(ToastInner {
            doc: doc.clone(),
            element,
            label,
            buttons,
            spec: RefCell::new(ToastSpec::new(message)),
            _click: RefCell::new(None),
            tick: RefCell::new(None),
            timeout: Cell::new(None),
            on_action: RefCell::new(None),
            on_dismiss: RefCell::new(None),
            visible: Cell::new(false),
        }));
        this.install_action_handler()?;
        this.install_tick();
        Ok(this)
    }

    /// Creates the closure that receives the timeout. It is reused from then on.
    fn install_tick(&self) {
        let weak = Rc::downgrade(&self.0);
        let tick = Closure::<dyn FnMut()>::new(move || {
            let Some(inner) = weak.upgrade() else {
                return;
            };
            Toast(inner).finish(None);
        });
        *self.0.tick.borrow_mut() = Some(tick);
    }

    /// Makes clicking an action button notify and dismiss the toast.
    ///
    /// The click is picked up on the `<span>` holding the buttons, and the button's
    /// place among its siblings is the index.
    fn install_action_handler(&self) -> Result<()> {
        let weak = Rc::downgrade(&self.0);
        let listener = Listener::attach_event(self.0.buttons.as_ref(), "click", move |event| {
            let Some(inner) = weak.upgrade() else {
                return;
            };
            let Some(button) = event
                .target()
                .and_then(|t| t.dyn_into::<Element>().ok())
                .and_then(|e| e.closest("button").ok().flatten())
            else {
                return;
            };
            let children = inner.buttons.children();
            let index = (0..children.length())
                .find(|&i| children.item(i).is_some_and(|child| child == button));
            if let Some(index) = index {
                Toast(inner).finish(Some(index as usize));
            }
        })?;
        *self.0._click.borrow_mut() = Some(listener);
        Ok(())
    }

    /// The string to show. Calling it while shown updates it on the spot.
    pub fn set_message(&self, message: &str) {
        self.0.spec.borrow_mut().set_message(message);
        self.0.label.set_text_content(Some(message));
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
        self.0.buttons.set_text_content(None);
        let labels = self.0.spec.borrow().actions().to_vec();
        for label in labels {
            let Ok(button) = create(&self.0.doc, "button") else {
                continue;
            };
            button.set_text_content(Some(&label));
            let _ = self.0.buttons.append_child(&button);
        }
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
    /// [`naui_core::ToastQueue`]).
    pub fn show(&self) {
        let priority = self.0.spec.borrow().priority();
        ToastQueue::show(&QUEUE, &self.0, priority);
    }

    /// Closes the toast, or takes it out of the queue. `on_dismiss` is not called.
    pub fn dismiss(&self) {
        ToastQueue::remove(&QUEUE, &self.0);
    }

    /// Whether it is on screen now.
    pub fn is_visible(&self) -> bool {
        self.0.visible.get()
    }

    /// Whether it is waiting its turn in the queue.
    pub fn is_queued(&self) -> bool {
        ToastQueue::is_waiting_in(&QUEUE, &self.0)
    }

    /// The toast's element. A backend-specific escape hatch.
    pub fn native_element(&self) -> Element {
        self.0.element.clone().unchecked_into()
    }

    /// Writes the position into the element's style.
    ///
    /// Only logical properties are used, so that the start and end sides follow the
    /// page's `direction`. Centering uses both inline insets with automatic margins
    /// (the element's width is `fit-content`), which, unlike `left: 50%` and a
    /// transform, needs no mirroring.
    fn place(&self) {
        let element = &self.0.element;
        let position = self.0.spec.borrow().position();
        let (block_start, block_end) = if position.is_top() {
            (EDGE_MARGIN, "auto")
        } else {
            ("auto", EDGE_MARGIN)
        };
        style(element, "inset-block-start", block_start);
        style(element, "inset-block-end", block_end);
        let (inline_start, inline_end, margin) = match position.align() {
            Align::Start => (EDGE_MARGIN, "auto", "0"),
            Align::End => ("auto", EDGE_MARGIN, "0"),
            _ => ("0", "0", "auto"),
        };
        style(element, "inset-inline-start", inline_start);
        style(element, "inset-inline-end", inline_end);
        style(element, "margin-inline", margin);
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

    /// Starts counting down. Does nothing if it never disappears.
    fn start_timeout(&self) {
        let Some(millis) = self.0.spec.borrow().timeout_millis() else {
            return;
        };
        let Some(window) = web_sys::window() else {
            return;
        };
        let handle = self.0.tick.borrow().as_ref().and_then(|tick| {
            window
                .set_timeout_with_callback_and_timeout_and_arguments_0(
                    tick.as_ref().unchecked_ref(),
                    millis,
                )
                .ok()
        });
        self.0.timeout.set(handle);
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
