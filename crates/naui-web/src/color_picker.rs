//! Color picking (`<input type="color">`).
//!
//! The browser (and the OS) shows the color picking UI. naui only converts the
//! `value`'s `"#rrggbb"` to and from [`Color`]; it never draws palettes or
//! eyedroppers itself.
//!
//! Transparency is not handled, because the `value` of `<input type="color">` is
//! defined to be only the hex notation of an opaque color.

use std::cell::RefCell;
use std::rc::Rc;

use naui_core::{Color, Result};
use wasm_bindgen::JsCast;
use web_sys::{Document, Element, HtmlInputElement};

use crate::widgets::{create, impl_widget, Listener, ValueHandler, Widget};

struct ColorPickerInner {
    input: HtmlInputElement,
    on_change: ValueHandler<Color>,
    /// The `change` subscription. Dropping it also unsubscribes.
    _listener: RefCell<Option<Listener>>,
}

/// A control for picking a color (`<input type="color">`).
///
/// Its value right after creation is black ([`Color::BLACK`]), which is exactly the
/// default of `<input type="color">`.
#[derive(Clone)]
pub struct ColorPicker(Rc<ColorPickerInner>);
impl_widget!(ColorPicker, input);

impl ColorPicker {
    pub(crate) fn new(document: &Document) -> Result<Self> {
        let input: HtmlInputElement = create(document, "input")?.unchecked_into();
        input.set_type("color");
        input.set_value(&Color::BLACK.to_hex());

        let inner = Rc::new(ColorPickerInner {
            input,
            on_change: ValueHandler::default(),
            _listener: RefCell::new(None),
        });
        // 選んでいる最中は `input` が何度も飛ぶので、確定の `change` だけを
        // 受ける (`DatePicker` が確定で表示をそろえるのと同じ考え方)。
        let listener = Listener::attach(inner.input.as_ref(), "change", {
            let weak = Rc::downgrade(&inner);
            move || {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                let color = Color::from_hex(&inner.input.value()).unwrap_or_default();
                inner.on_change.emit(color);
            }
        })?;
        *inner._listener.borrow_mut() = Some(listener);
        Ok(Self(inner))
    }

    /// The currently selected color.
    pub fn value(&self) -> Color {
        Color::from_hex(&self.0.input.value()).unwrap_or_default()
    }

    /// Replaces the color from code. `on_change` is not called (the DOM does not fire
    /// `change` when `value` is written).
    pub fn set_value(&self, color: Color) {
        self.0.input.set_value(&color.to_hex());
    }

    /// Sets the color through the same path as a user's pick and notifies once.
    pub fn pick(&self, color: Color) {
        self.set_value(color);
        self.0.on_change.emit(self.value());
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.0.input.set_disabled(!enabled);
    }

    /// Called with the new color every time the color changes.
    pub fn on_change(&self, f: impl FnMut(Color) + 'static) {
        self.0.on_change.set(f);
    }

    /// The input itself. Public as a backend-specific escape hatch.
    pub fn native_input(&self) -> HtmlInputElement {
        self.0.input.clone()
    }
}
