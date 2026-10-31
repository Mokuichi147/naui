//! General-purpose dialogs (DOM).
//!
//! The browser's modal is the **`<dialog>`** itself; once `showModal()` is called,
//! bringing it to the front, blocking interaction behind it and closing on Esc are
//! all done by the browser (including the `::backdrop` overlay).
//!
//! However, `<dialog>` is an empty box, so naui builds the title, body text and
//! buttons by laying out `<h2>` / `<p>` / `<button>`. CSS is used only for Flexbox
//! arrangement (direction, spacing, right alignment).
//!
//! ## Differences from other platforms
//!
//! [`Dialog::open`] returns right away, and the close notification arrives later
//! (macOS's `NSAlert` does not return until it is closed).
//!
//! That it closed is learned directly from **the clicked button** and **Esc's
//! `keydown`**, not from the `<dialog>`'s `close` event. `close` cannot tell the
//! side that called `close()` (including the app's own [`Dialog::close`]) from
//! user actions, and some embedded browsers never deliver it.

use std::cell::RefCell;
use std::rc::Rc;

use naui_core::{DialogButtons, DialogResponse, Result};
use wasm_bindgen::JsCast;
use web_sys::{Document, Element, HtmlDialogElement, HtmlElement, KeyboardEvent};

use crate::to_error;
use crate::widgets::{create, Listener, Widget};

/// The order in which buttons are laid out from the left. Following macOS, the
/// primary action goes at the right end.
const BUTTON_ORDER: [DialogResponse; 3] = [
    DialogResponse::Secondary,
    DialogResponse::Cancel,
    DialogResponse::Primary,
];

struct DialogInner {
    element: HtmlDialogElement,
    title: HtmlElement,
    message: HtmlElement,
    /// The box holding the content widget.
    content: HtmlElement,
    /// The box laying out the buttons.
    actions: HtmlElement,
    child: RefCell<Option<Box<dyn Widget>>>,
    buttons: RefCell<DialogButtons>,
    /// The receivers of button clicks. Replaced each time it is rebuilt.
    listeners: RefCell<Vec<Listener>>,
    /// The receivers for being closed as a cancel, by Esc and the like.
    dismiss: RefCell<Vec<Listener>>,
    on_response: RefCell<Option<Box<dyn FnMut(DialogResponse)>>>,
}

/// A modal dialog (`<dialog>`).
#[derive(Clone)]
pub struct Dialog(Rc<DialogInner>);

impl Dialog {
    pub(crate) fn new(doc: &Document, title: &str) -> Result<Self> {
        let element: HtmlDialogElement = create(doc, "dialog")?.unchecked_into();
        let body: HtmlElement = create(doc, "div")?.unchecked_into();
        let style = body.style();
        let _ = style.set_property("display", "flex");
        let _ = style.set_property("flex-direction", "column");
        let _ = style.set_property("gap", "12px");

        let title_element: HtmlElement = create(doc, "h2")?.unchecked_into();
        // 見出しの既定の余白は箱の間隔 (gap) と二重になるので外す。
        let _ = title_element.style().set_property("margin", "0");
        let message: HtmlElement = create(doc, "p")?.unchecked_into();
        let _ = message.style().set_property("margin", "0");
        message.set_hidden(true);
        let content: HtmlElement = create(doc, "div")?.unchecked_into();
        content.set_hidden(true);
        let actions: HtmlElement = create(doc, "div")?.unchecked_into();
        let actions_style = actions.style();
        let _ = actions_style.set_property("display", "flex");
        let _ = actions_style.set_property("gap", "8px");
        let _ = actions_style.set_property("justify-content", "flex-end");

        for part in [&title_element, &message, &content, &actions] {
            body.append_child(part)
                .map_err(|e| to_error("build the dialog", e))?;
        }
        element
            .append_child(&body)
            .map_err(|e| to_error("build the dialog", e))?;
        // `showModal()` は文書に入っている要素にしか効かないため、
        // 作った時点で `<body>` へ入れておく。
        doc.body()
            .ok_or_else(|| naui_core::Error::new("get body", "there is no body"))?
            .append_child(&element)
            .map_err(|e| to_error("add the dialog", e))?;

        let this = Self(Rc::new(DialogInner {
            element,
            title: title_element,
            message,
            content,
            actions,
            child: RefCell::new(None),
            buttons: RefCell::new(DialogButtons::new()),
            listeners: RefCell::new(Vec::new()),
            dismiss: RefCell::new(Vec::new()),
            on_response: RefCell::new(None),
        }));
        this.set_title(title);
        this.rebuild_buttons()?;
        this.install_dismiss_handlers()?;
        Ok(this)
    }

    /// Notifies a cancel when it is closed with Esc.
    ///
    /// What is received is **Esc's `keydown`** rather than `cancel`; the key's default
    /// action is prevented and the dialog is closed by naui. When the window is in
    /// fullscreen, Safari first uses Esc to leave fullscreen and **does not pass it to
    /// the dialog unless the page prevents the default** (it takes one Esc to leave and
    /// another to close). With it prevented, one press closes it even in fullscreen.
    /// Since the browser's default "close" is prevented, reopening from inside the
    /// notification does not get it closed afterwards.
    ///
    /// `cancel` is received as well. "Close requests" other than the key arrive only
    /// there. When closed with Esc, the default is prevented and `cancel` does not
    /// occur, so it is not notified twice.
    fn install_dismiss_handlers(&self) -> Result<()> {
        let mut listeners = Vec::new();

        let weak = Rc::downgrade(&self.0);
        listeners.push(Listener::attach_event(
            self.0.element.as_ref(),
            "keydown",
            move |event| {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                let Some(key) = event.dyn_ref::<KeyboardEvent>() else {
                    return;
                };
                // 変換中の Esc は入力を取り消すためのものなので、IME へ渡す。
                if key.key() != "Escape" || key.is_composing() {
                    return;
                }
                event.prevent_default();
                // 中のウィジェットから開いたポップオーバーがあれば、先にいちばん上の
                // ものを閉じる (ブラウザの Esc と同じ順)。
                if crate::popover::dismiss_topmost_within(inner.element.as_ref()) {
                    return;
                }
                Dialog(inner).dismiss();
            },
        )?);

        let weak = Rc::downgrade(&self.0);
        listeners.push(Listener::attach_event(
            self.0.element.as_ref(),
            "cancel",
            move |event| {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                event.prevent_default();
                Dialog(inner).dismiss();
            },
        )?);

        *self.0.dismiss.borrow_mut() = listeners;
        Ok(())
    }

    /// Closes as a cancel and notifies. Does nothing if already closed.
    fn dismiss(&self) {
        if !self.0.element.open() {
            return;
        }
        crate::popover::dismiss_within(self.0.element.as_ref());
        self.0.element.close();
        self.emit(DialogResponse::Cancel);
    }

    /// Notification. So that setting it again from inside the notification does not
    /// double-borrow, the closure is taken out only for the duration of the call.
    fn emit(&self, response: DialogResponse) {
        let Some(mut f) = self.0.on_response.borrow_mut().take() else {
            return;
        };
        f(response);
        let mut slot = self.0.on_response.borrow_mut();
        if slot.is_none() {
            *slot = Some(f);
        }
    }

    pub fn set_title(&self, title: &str) {
        self.0.title.set_text_content(Some(title));
        self.0.title.set_hidden(title.is_empty());
    }

    pub fn title(&self) -> String {
        self.0.title.text_content().unwrap_or_default()
    }

    /// The body text shown below the title. Not shown if empty.
    pub fn set_message(&self, message: &str) {
        self.0.message.set_text_content(Some(message));
        self.0.message.set_hidden(message.is_empty());
    }

    pub fn message(&self) -> String {
        self.0.message.text_content().unwrap_or_default()
    }

    /// The widget placed between the body text and the buttons. Each call replaces it.
    pub fn set_child(&self, child: &dyn Widget) {
        self.0.content.set_inner_html("");
        let element = child.native_element();
        if self.0.content.append_child(&element).is_ok() {
            self.0.content.set_hidden(false);
            // あとから `set_sizing` したときと同じ扱いにし、前の親のための指定を残さない。
            crate::layout::apply_child_layout(&element, crate::layout::ParentLayout::Block);
            *self.0.child.borrow_mut() = Some(child.boxed_clone());
        }
    }

    /// The buttons to show. By default there are none, in which case only "OK" is
    /// shown.
    pub fn set_buttons(&self, buttons: DialogButtons) {
        *self.0.buttons.borrow_mut() = buttons;
        let _ = self.rebuild_buttons();
    }

    pub fn buttons(&self) -> DialogButtons {
        self.0.buttons.borrow().clone()
    }

    /// Lays out the `<button>`s again. When clicked, it closes the `<dialog>` and
    /// notifies with that role.
    fn rebuild_buttons(&self) -> Result<()> {
        // 以前のボタンとその購読を外す。
        self.0.listeners.borrow_mut().clear();
        self.0.actions.set_inner_html("");

        let document = self
            .0
            .element
            .owner_document()
            .ok_or_else(|| naui_core::Error::new("get document", "there is no document"))?;
        let mut listeners = Vec::new();
        for (role, label) in self.0.buttons.borrow().resolved().in_order(&BUTTON_ORDER) {
            let button: HtmlElement = create(&document, "button")?.unchecked_into();
            button.set_text_content(Some(&label));
            self.0
                .actions
                .append_child(&button)
                .map_err(|e| to_error("add the dialog buttons", e))?;
            let weak = Rc::downgrade(&self.0);
            listeners.push(Listener::attach(button.as_ref(), "click", move || {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                crate::popover::dismiss_within(inner.element.as_ref());
                inner.element.close();
                Dialog(inner).emit(role);
            })?);
        }
        *self.0.listeners.borrow_mut() = listeners;
        Ok(())
    }

    /// Called with the reason when it closes. Setting it again removes the previous
    /// one.
    ///
    /// Not called when it is closed with [`Dialog::close`].
    pub fn on_response(&self, f: impl FnMut(DialogResponse) + 'static) {
        *self.0.on_response.borrow_mut() = Some(Box::new(f));
    }

    /// Shows the dialog. **It returns right away, and closing arrives through
    /// `on_response`.**
    ///
    /// Does nothing if it is already shown.
    pub fn open(&self) {
        if self.0.element.open() {
            return;
        }
        let _ = self.0.element.show_modal();
    }

    /// Closes the dialog being shown. `on_response` is not called.
    pub fn close(&self) {
        if !self.0.element.open() {
            return;
        }
        crate::popover::dismiss_within(self.0.element.as_ref());
        self.0.element.close();
    }

    /// Whether it is showing now.
    pub fn is_open(&self) -> bool {
        self.0.element.open()
    }

    /// The `<dialog>` element. A backend-specific escape hatch.
    pub fn native_element(&self) -> Element {
        self.0.element.clone().unchecked_into()
    }
}
