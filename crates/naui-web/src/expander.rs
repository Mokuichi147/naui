//! A collapsible section (`<details>` + `<summary>`).
//!
//! HTML has an element for the same job, so it is used as is. The triangle and
//! expanding and collapsing are provided by the browser (no JavaScript
//! implementation of expanding is needed).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use naui_core::Result;
use wasm_bindgen::JsCast;
use web_sys::{Document, Element, HtmlDetailsElement, HtmlElement};

use crate::layout::{apply_child_layout, ParentLayout};
use crate::to_error;
use crate::widgets::{create, impl_widget, Listener, ValueHandler, Widget};

struct ExpanderInner {
    /// `<details><summary>heading</summary>content</details>`
    element: HtmlDetailsElement,
    summary: HtmlElement,
    /// Holds the content's handle, keeping it alive along with its callbacks.
    child: RefCell<Option<Box<dyn Widget>>>,
    /// The expanded state naui knows. Tells whether a `toggle` is a user action.
    state: Rc<Cell<bool>>,
    handler: Rc<ValueHandler<bool>>,
    /// The relay is held with the same lifetime as the handle (so it can be expanded
    /// or collapsed from inside the notification).
    listener: RefCell<Option<Listener>>,
    /// `on_file_drop`.
    file_drop: crate::file_drop::FileDrop,
}

/// A container whose contents are shown and hidden by clicking its heading
/// (`<details>`).
#[derive(Clone)]
pub struct Expander(Rc<ExpanderInner>);
impl_widget!(Expander, element);

impl Expander {
    pub(crate) fn new(doc: &Document, text: &str) -> Result<Self> {
        let element: HtmlDetailsElement = create(doc, "details")?.unchecked_into();
        let summary: HtmlElement = create(doc, "summary")?.unchecked_into();
        summary.set_text_content(Some(text));
        element
            .append_child(&summary)
            .map_err(|e| to_error("build the expander", e))?;
        // 見出しは押せるので、指の形にしておく (ブラウザ既定は矢印)。
        let _ = summary.style().set_property("cursor", "pointer");

        let this = Self(Rc::new(ExpanderInner {
            element,
            summary,
            child: RefCell::new(None),
            state: Rc::new(Cell::new(false)),
            handler: Rc::new(ValueHandler::default()),
            listener: RefCell::new(None),
            file_drop: Default::default(),
        }));

        // `toggle` は `open` を書き換えたときにも (あとから) 飛ぶので、
        // naui が知っている状態と食い違ったときだけ利用者の操作と見なす。
        let listener = Listener::attach(this.0.element.as_ref(), "toggle", {
            let element = this.0.element.clone();
            let state = this.0.state.clone();
            let handler = this.0.handler.clone();
            move || {
                let open = element.open();
                if state.get() == open {
                    return;
                }
                state.set(open);
                handler.emit(open);
            }
        })?;
        *this.0.listener.borrow_mut() = Some(listener);
        Ok(this)
    }

    /// The heading text.
    pub fn text(&self) -> String {
        self.0.summary.text_content().unwrap_or_default()
    }

    pub fn set_text(&self, text: &str) {
        self.0.summary.set_text_content(Some(text));
    }

    /// Whether it is expanded.
    pub fn is_expanded(&self) -> bool {
        self.0.element.open()
    }

    /// Expands or collapses it from code. `on_toggle` is not called.
    pub fn set_expanded(&self, expanded: bool) {
        self.0.state.set(expanded);
        self.0.element.set_open(expanded);
    }

    /// Whether the heading can be clicked.
    ///
    /// `<details>` has no `disabled`, so it is expressed as a heading that does not
    /// open when clicked. The mouse is stopped with `pointer-events`, and the keyboard
    /// by removing it from the tab order (`<summary>` does not open with Enter unless
    /// it has focus).
    pub fn set_enabled(&self, enabled: bool) {
        let style = self.0.summary.style();
        if enabled {
            let _ = style.remove_property("pointer-events");
            let _ = style.remove_property("opacity");
            self.0.summary.remove_attribute("tabindex").ok();
            self.0.summary.remove_attribute("aria-disabled").ok();
        } else {
            let _ = style.set_property("pointer-events", "none");
            let _ = style.set_property("opacity", "0.5");
            let _ = self.0.summary.set_attribute("tabindex", "-1");
            let _ = self.0.summary.set_attribute("aria-disabled", "true");
        }
    }

    /// The contents to collapse. Each call replaces them.
    pub fn set_child(&self, child: &dyn Widget) {
        if let Some(previous) = self.0.child.borrow_mut().take() {
            let _ = self.0.element.remove_child(&previous.native_element());
        }
        let element = child.native_element();
        if self.0.element.append_child(&element).is_ok() {
            apply_child_layout(&element, ParentLayout::Block);
            *self.0.child.borrow_mut() = Some(child.boxed_clone());
        }
    }

    /// Called with the new state every time the user expands or collapses it.
    pub fn on_toggle(&self, f: impl FnMut(bool) + 'static) {
        self.0.handler.set(f);
    }
}
