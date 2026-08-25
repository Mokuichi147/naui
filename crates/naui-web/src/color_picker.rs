//! 色の選択 (`<input type="color">`)。
//!
//! 色を選ぶ UI はブラウザ (と OS) が出す。naui は `value` の `"#rrggbb"` を
//! [`Color`] と行き来させるだけで、パレットやスポイトを自前で描くことはない。
//!
//! 透明度は扱わない。`<input type="color">` の `value` は不透明な色の
//! 16 進表記だけと決まっているため。

use std::cell::RefCell;
use std::rc::Rc;

use naui_core::{Color, Result};
use wasm_bindgen::JsCast;
use web_sys::{Document, Element, HtmlInputElement};

use crate::widgets::{create, impl_widget, Listener, ValueHandler, Widget};

struct ColorPickerInner {
    input: HtmlInputElement,
    on_change: ValueHandler<Color>,
    /// `change` の購読。落とすと購読も外れる。
    _listener: RefCell<Option<Listener>>,
}

/// 色を選ばせるコントロール (`<input type="color">`)。
///
/// 作った直後の値は黒 ([`Color::BLACK`])。これは `<input type="color">` の
/// 既定値そのもの。
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

    /// いま選ばれている色。
    pub fn value(&self) -> Color {
        Color::from_hex(&self.0.input.value()).unwrap_or_default()
    }

    /// プログラムから色を差し替える。`on_change` は呼ばれない
    /// (DOM は `value` を書いても `change` を出さない)。
    pub fn set_value(&self, color: Color) {
        self.0.input.set_value(&color.to_hex());
    }

    /// 利用者が選んだのと同じ経路で色を決め、1 回通知する。
    pub fn pick(&self, color: Color) {
        self.set_value(color);
        self.0.on_change.emit(self.value());
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.0.input.set_disabled(!enabled);
    }

    /// 色が変わるたびに、変わった後の色で呼ばれる。
    pub fn on_change(&self, f: impl FnMut(Color) + 'static) {
        self.0.on_change.set(f);
    }

    /// 入力欄本体。バックエンド固有の脱出口として公開している。
    pub fn native_input(&self) -> HtmlInputElement {
        self.0.input.clone()
    }
}
