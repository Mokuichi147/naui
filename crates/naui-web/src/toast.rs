//! Toasts (DOM).
//!
//! Browsers have no standard element for "a notification shown for a while that
//! then disappears by itself" (the `Notification` API shows things outside the
//! page, which is different). So, as with [`PopupMenu`](crate::PopupMenu), it is
//! built from a `<div role="status">` and a `<button>`.
//!
//! Colors use CSS system colors (`Canvas` / `CanvasText`), so they follow the
//! `color-scheme` that [`crate::Ui::set_theme`] sets on `<html>` as is. CSS decides
//! only the position (bottom center), padding and corner radius.
//!
//! Screen reader announcements are left to `role="status"` (`aria-live="polite"`).
//! Once the text is in, it is read without interrupting what is being read.
//!
//! `setTimeout` counts down until it disappears.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use naui_core::{Error, Result, ToastSpec};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use web_sys::{Document, Element, HtmlElement};

use crate::to_error;
use crate::widgets::{create, Listener};

thread_local! {
    /// The toast showing now. Only one is shown at a time, and a new one replaces it.
    static CURRENT: RefCell<Option<Toast>> = const { RefCell::new(None) };
}

fn style(element: &HtmlElement, property: &str, value: &str) {
    let _ = element.style().set_property(property, value);
}

struct ToastInner {
    /// The toast itself. Placed directly under `<body>`, with `display: none` while not
    /// shown.
    element: HtmlElement,
    /// The `<span>` showing the text.
    label: HtmlElement,
    /// The action button. `hidden` while there is none.
    button: HtmlElement,
    spec: RefCell<ToastSpec>,
    /// The action button's click subscription.
    _click: RefCell<Option<Listener>>,
    /// The closure that receives the timeout from the browser. **Kept alive as long as
    /// the toast.**
    ///
    /// It is not recreated every time it is shown so that dismissing from inside it
    /// **does not free the closure that is currently running**.
    tick: RefCell<Option<Closure<dyn FnMut()>>>,
    /// The `setTimeout` reservation. Absent if it never disappears.
    timeout: Cell<Option<i32>>,
    on_action: RefCell<Option<Box<dyn FnMut()>>>,
    on_dismiss: RefCell<Option<Box<dyn FnMut()>>>,
    visible: Cell<bool>,
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
        style(&element, "left", "50%");
        style(&element, "bottom", "24px");
        style(&element, "transform", "translateX(-50%)");
        style(&element, "z-index", "1000");
        style(&element, "align-items", "center");
        style(&element, "gap", "12px");
        style(&element, "padding", "10px 16px");
        style(&element, "max-width", "calc(100% - 48px)");
        style(&element, "box-sizing", "border-box");
        style(&element, "background", "Canvas");
        style(&element, "color", "CanvasText");
        style(&element, "border", "1px solid CanvasText");
        style(&element, "border-radius", "10px");
        style(&element, "box-shadow", "0 4px 12px rgba(0, 0, 0, 0.25)");

        let label: HtmlElement = create(doc, "span")?.unchecked_into();
        label.set_text_content(Some(message));
        let button: HtmlElement = create(doc, "button")?.unchecked_into();
        button.set_hidden(true);
        for part in [&label, &button] {
            element
                .append_child(part)
                .map_err(|e| to_error("build the toast", e))?;
        }

        doc.body()
            .ok_or_else(|| Error::new("place the toast", "there is no body element"))?
            .append_child(&element)
            .map_err(|e| to_error("place the toast", e))?;

        let this = Self(Rc::new(ToastInner {
            element,
            label,
            button,
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
            Toast(inner).finish(false);
        });
        *self.0.tick.borrow_mut() = Some(tick);
    }

    /// Makes clicking the action button notify and dismiss the toast.
    fn install_action_handler(&self) -> Result<()> {
        let weak = Rc::downgrade(&self.0);
        let listener = Listener::attach(self.0.button.as_ref(), "click", move || {
            let Some(inner) = weak.upgrade() else {
                return;
            };
            Toast(inner).finish(true);
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

    /// The action button's label. **Passing an empty string removes the button.**
    pub fn set_action(&self, label: &str) {
        self.0.spec.borrow_mut().set_action(label);
        let action = self.0.spec.borrow().action().map(str::to_string);
        match action {
            Some(label) => {
                self.0.button.set_text_content(Some(&label));
                self.0.button.set_hidden(false);
            }
            None => {
                self.0.button.set_text_content(None);
                self.0.button.set_hidden(true);
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
    /// (the replaced one's `on_dismiss` is not called).
    pub fn show(&self) {
        if let Some(previous) = CURRENT.with(|slot| slot.borrow_mut().take()) {
            previous.take_down();
        }
        style(&self.0.element, "display", "flex");
        self.0.visible.set(true);
        self.start_timeout();
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

    /// The toast's element. A backend-specific escape hatch.
    pub fn native_element(&self) -> Element {
        self.0.element.clone().unchecked_into()
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

    /// Hides it and stops counting down. No notification.
    fn take_down(&self) {
        if let (Some(handle), Some(window)) = (self.0.timeout.take(), web_sys::window()) {
            window.clear_timeout_with_handle(handle);
        }
        style(&self.0.element, "display", "none");
        self.0.visible.set(false);
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
