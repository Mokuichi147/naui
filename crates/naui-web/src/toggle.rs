//! An on/off switch (`<input type="checkbox" switch>`).
//!
//! An HTML switch is a check box with the `switch` attribute. Browsers that support
//! it draw an on/off knob; browsers that do not show a check box (values are
//! handled the same either way).
//!
//! **Only Safari 17.4 and later support it** (confirmed unsupported in Chromium
//! 148), but naui does not create the knob's look with its own CSS: naui's policy
//! is not to draw widgets itself and to use what the browser has as is.

use std::cell::RefCell;
use std::rc::Rc;

use naui_core::Result;
use wasm_bindgen::JsCast;
use web_sys::{Document, Element, HtmlElement, HtmlInputElement};

use crate::to_error;
use crate::widgets::{create, impl_widget, Listener, Widget};

struct ToggleInner {
    /// `<label><input type="checkbox" switch>text</label>`
    element: HtmlElement,
    input: HtmlInputElement,
    listener: RefCell<Option<Listener>>,
}

/// An on/off switch (`<input type="checkbox" switch>`).
#[derive(Clone)]
pub struct Toggle(Rc<ToggleInner>);
impl_widget!(Toggle, element);

impl Toggle {
    pub(crate) fn new(doc: &Document, label: &str) -> Result<Self> {
        let element: HtmlElement = create(doc, "label")?.unchecked_into();
        let input: HtmlInputElement = create(doc, "input")?.unchecked_into();
        input.set_type("checkbox");
        // `switch` はスイッチとして描くようブラウザへ頼む属性。
        // `role` は、まだ対応していないブラウザでも読み上げがスイッチだと
        // 分かるように添える。
        let _ = input.set_attribute("switch", "");
        let _ = input.set_attribute("role", "switch");
        let text = create(doc, "span")?;
        text.set_text_content(Some(label));
        element
            .append_child(&input)
            .map_err(|e| to_error("スイッチの組み立て", e))?;
        element
            .append_child(&text)
            .map_err(|e| to_error("スイッチの組み立て", e))?;
        // スイッチと文字を横に並べる。
        let style = element.style();
        let _ = style.set_property("display", "inline-flex");
        let _ = style.set_property("align-items", "center");
        let _ = style.set_property("gap", "0.4em");

        Ok(Self(Rc::new(ToggleInner {
            element,
            input,
            listener: RefCell::new(None),
        })))
    }

    /// Whether it is on.
    pub fn is_on(&self) -> bool {
        self.0.input.checked()
    }

    /// Switches it from code. `on_toggle` is not called (the DOM does not fire `change`
    /// when `checked` is written).
    pub fn set_on(&self, on: bool) {
        self.0.input.set_checked(on);
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.0.input.set_disabled(!enabled);
    }

    /// Called with the new state every time the user switches it.
    pub fn on_toggle(&self, mut f: impl FnMut(bool) + 'static) {
        let input = self.0.input.clone();
        let listener = Listener::attach(self.0.input.as_ref(), "change", move || {
            f(input.checked());
        })
        .ok();
        *self.0.listener.borrow_mut() = listener;
    }

    /// Triggers a click (for tests and automation).
    pub fn click(&self) {
        self.0.input.click();
    }

    /// The switch itself. Public as a backend-specific escape hatch.
    pub fn native_switch(&self) -> HtmlInputElement {
        self.0.input.clone()
    }
}
