//! 入り切りのスイッチ (`<input type="checkbox" switch>`)。
//!
//! HTML のスイッチは、チェックボックスへ `switch` 属性を付けたもの。
//! ブラウザが対応していれば入り切りのつまみとして描かれ、対応していない
//! ブラウザではチェックボックスのまま出る (値の扱いはどちらも同じ)。
//!
//! **対応しているのは Safari 17.4 以降だけ** (Chromium 148 で未対応を確認)
//! だが、つまみの見た目を naui 側の CSS で作ることはしない。naui はウィジェット
//! を自前で描画せず、ブラウザにあるものをそのまま使う方針のため。

use std::cell::RefCell;
use std::rc::Rc;

use naui_core::Result;
use wasm_bindgen::JsCast;
use web_sys::{Document, Element, HtmlElement, HtmlInputElement};

use crate::to_error;
use crate::widgets::{create, impl_widget, Listener, Widget};

struct ToggleInner {
    /// `<label><input type="checkbox" switch>テキスト</label>`
    element: HtmlElement,
    input: HtmlInputElement,
    listener: RefCell<Option<Listener>>,
}

/// 入り切りを切り替えるスイッチ (`<input type="checkbox" switch>`)。
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

    /// 入っているかどうか。
    pub fn is_on(&self) -> bool {
        self.0.input.checked()
    }

    /// プログラムから切り替える。`on_toggle` は呼ばれない
    /// (DOM は `checked` を書いても `change` を出さない)。
    pub fn set_on(&self, on: bool) {
        self.0.input.set_checked(on);
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.0.input.set_disabled(!enabled);
    }

    /// 利用者が切り替えるたびに、切り替えた後の状態で呼ばれる。
    pub fn on_toggle(&self, mut f: impl FnMut(bool) + 'static) {
        let input = self.0.input.clone();
        let listener = Listener::attach(self.0.input.as_ref(), "change", move || {
            f(input.checked());
        })
        .ok();
        *self.0.listener.borrow_mut() = listener;
    }

    /// クリックを発生させる (テストや自動操作用)。
    pub fn click(&self) {
        self.0.input.click();
    }

    /// スイッチ本体。バックエンド固有の脱出口として公開している。
    pub fn native_switch(&self) -> HtmlInputElement {
        self.0.input.clone()
    }
}
