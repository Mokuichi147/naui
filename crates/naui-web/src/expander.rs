//! 折りたたみ (`<details>` + `<summary>`)。
//!
//! HTML に同じ役目の要素があるので、そのまま使う。三角の印も、開閉も
//! ブラウザが出す (JavaScript による開閉の実装は要らない)。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use naui_core::Result;
use wasm_bindgen::JsCast;
use web_sys::{Document, Element, HtmlDetailsElement, HtmlElement};

use crate::layout::{apply_child_layout, ParentLayout};
use crate::to_error;
use crate::widgets::{create, impl_widget, Listener, ValueHandler, Widget};

struct ExpanderInner {
    /// `<details><summary>見出し</summary>中身</details>`
    element: HtmlDetailsElement,
    summary: HtmlElement,
    /// 中身のハンドルを保持し、コールバックごと生かしておく。
    child: RefCell<Option<Box<dyn Widget>>>,
    /// naui が知っている開閉の状態。`toggle` が利用者の操作かを見分ける。
    state: Rc<Cell<bool>>,
    handler: Rc<ValueHandler<bool>>,
    /// 中継はハンドルと同じ寿命で持つ (通知の中から開閉できるように)。
    listener: RefCell<Option<Listener>>,
}

/// 見出しを押して中身を出し入れするコンテナ (`<details>`)。
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
            .map_err(|e| to_error("折りたたみの組み立て", e))?;
        // 見出しは押せるので、指の形にしておく (ブラウザ既定は矢印)。
        let _ = summary.style().set_property("cursor", "pointer");

        let this = Self(Rc::new(ExpanderInner {
            element,
            summary,
            child: RefCell::new(None),
            state: Rc::new(Cell::new(false)),
            handler: Rc::new(ValueHandler::default()),
            listener: RefCell::new(None),
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

    /// 見出しの文字。
    pub fn text(&self) -> String {
        self.0.summary.text_content().unwrap_or_default()
    }

    pub fn set_text(&self, text: &str) {
        self.0.summary.set_text_content(Some(text));
    }

    /// 開いているかどうか。
    pub fn is_expanded(&self) -> bool {
        self.0.element.open()
    }

    /// プログラムから開閉する。`on_toggle` は呼ばれない。
    pub fn set_expanded(&self, expanded: bool) {
        self.0.state.set(expanded);
        self.0.element.set_open(expanded);
    }

    /// 見出しを押せるかどうか。
    ///
    /// `<details>` に `disabled` は無いので、押しても開かない見出しとして
    /// 表す。マウスは `pointer-events` で、キーボードはタブ順から外して
    /// 止める (`<summary>` は焦点が当たっていないと Enter で開かない)。
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

    /// 折りたたむ中身。呼ぶたびに置き換わる。
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

    /// 利用者が開閉するたびに、変わった後の状態で呼ばれる。
    pub fn on_toggle(&self, f: impl FnMut(bool) + 'static) {
        self.0.handler.set(f);
    }
}
