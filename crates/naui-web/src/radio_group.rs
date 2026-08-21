//! 選択肢を並べて 1 つだけ選ばせるラジオグループ (DOM の `<input type="radio">`)。
//!
//! 排他はブラウザに任せる。同じ `name` を持つラジオは、1 つ点けると残りが
//! 自動で外れる。`name` はグループごとに作った一意の文字列なので、同じ画面に
//! 複数のラジオグループを置いても混ざらない。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use naui_core::{Orientation, Result};
use wasm_bindgen::JsCast;
use web_sys::{Document, Element, HtmlElement, HtmlInputElement};

use crate::widgets::{create, impl_widget, Listener, SelectionHandler, Widget};

thread_local! {
    /// グループごとに一意な `name` を作るための連番。
    static NEXT_GROUP: Cell<u64> = const { Cell::new(0) };
}

fn next_group_name() -> String {
    NEXT_GROUP.with(|next| {
        let id = next.get();
        next.set(id.wrapping_add(1));
        format!("naui-radio-{id}")
    })
}

struct RadioGroupInner {
    /// `<div role="radiogroup">`
    element: HtmlElement,
    document: Document,
    /// 同じ `name` を共有するラジオ。ブラウザはこれを見て排他にする。
    name: String,
    inputs: RefCell<Vec<HtmlInputElement>>,
    /// 入力と同じ順に並ぶ `change` の購読。落とすと購読も外れる。
    listeners: RefCell<Vec<Listener>>,
    on_select: SelectionHandler,
    /// [`set_enabled`](RadioGroup::set_enabled) の指定。`set_items` で作り直す
    /// 入力にも同じ状態を引き継ぐために覚えておく。
    enabled: Cell<bool>,
}

/// 選択肢を並べて 1 つだけ選ばせるラジオグループ (`<input type="radio">`)。
///
/// 作った直後と [`set_items`](Self::set_items) の直後は、何も選ばれていない。
#[derive(Clone)]
pub struct RadioGroup(Rc<RadioGroupInner>);
impl_widget!(RadioGroup, element);

impl RadioGroup {
    pub(crate) fn new(document: &Document) -> Result<Self> {
        let element: HtmlElement = create(document, "div")?.unchecked_into();
        // 支援技術にもラジオの組として伝わるようにしておく。
        let _ = element.set_attribute("role", "radiogroup");
        let style = element.style();
        let _ = style.set_property("display", "flex");
        let _ = style.set_property("flex-direction", "column");
        let _ = style.set_property("align-items", "flex-start");

        Ok(Self(Rc::new(RadioGroupInner {
            element,
            document: document.clone(),
            name: next_group_name(),
            inputs: RefCell::new(Vec::new()),
            listeners: RefCell::new(Vec::new()),
            on_select: SelectionHandler::default(),
            enabled: Cell::new(true),
        })))
    }

    /// 選択肢を作り直し、選択を外す。選択通知は発生しない。
    pub fn set_items<S: AsRef<str>>(&self, items: &[S]) {
        while let Some(child) = self.0.element.first_child() {
            let _ = self.0.element.remove_child(&child);
        }
        self.0.inputs.borrow_mut().clear();
        self.0.listeners.borrow_mut().clear();

        let enabled = self.0.enabled.get();
        let mut inputs = Vec::with_capacity(items.len());
        let mut listeners = Vec::with_capacity(items.len());
        for (index, item) in items.iter().enumerate() {
            let Ok(input) = self.build_item(item.as_ref(), enabled) else {
                continue;
            };
            let listener = Listener::attach(input.as_ref(), "change", {
                let weak = Rc::downgrade(&self.0);
                move || {
                    if let Some(inner) = weak.upgrade() {
                        inner.on_select.emit(index);
                    }
                }
            });
            // `change` は点いたラジオにだけ届くので、外れた側は通知されない。
            if let Ok(listener) = listener {
                listeners.push(listener);
            }
            inputs.push(input);
        }
        *self.0.inputs.borrow_mut() = inputs;
        *self.0.listeners.borrow_mut() = listeners;
    }

    /// 選択肢の数。
    pub fn len(&self) -> usize {
        self.0.inputs.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// 現在選ばれている選択肢。未選択なら `None`。
    pub fn selected(&self) -> Option<usize> {
        self.0
            .inputs
            .borrow()
            .iter()
            .position(|input| input.checked())
    }

    /// 範囲内の選択肢を通知せずに選ぶ。範囲外なら何もしない。
    pub fn set_selected(&self, index: usize) {
        // `checked` を書き換えても `change` は起きないので、購読を止める必要はない。
        if let Some(input) = self.0.inputs.borrow().get(index) {
            input.set_checked(true);
        }
    }

    /// 選択を通知せずに外す。
    pub fn clear_selection(&self) {
        for input in self.0.inputs.borrow().iter() {
            input.set_checked(false);
        }
    }

    /// ユーザーが選んだのと同じように、範囲内の選択肢を選んで通知する。
    pub fn select(&self, index: usize) {
        if index < self.len() {
            self.set_selected(index);
            self.0.on_select.emit(index);
        }
    }

    /// 選択肢の並ぶ向き。既定は縦。
    pub fn set_orientation(&self, orientation: Orientation) {
        let _ = self.0.element.style().set_property(
            "flex-direction",
            if orientation.is_vertical() {
                "column"
            } else {
                "row"
            },
        );
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.0.enabled.set(enabled);
        for input in self.0.inputs.borrow().iter() {
            input.set_disabled(!enabled);
        }
    }

    /// 選択肢が選ばれたときに、そのインデックスで呼ばれる。
    /// 設定し直すと以前のコールバックは外れる。
    pub fn on_select(&self, f: impl FnMut(usize) + 'static) {
        self.0.on_select.set(f);
    }

    /// `<label><input type="radio">テキスト</label>` を 1 つ組み立てて並べる。
    fn build_item(&self, text: &str, enabled: bool) -> Result<HtmlInputElement> {
        let label: HtmlElement = create(&self.0.document, "label")?.unchecked_into();
        let input: HtmlInputElement = create(&self.0.document, "input")?.unchecked_into();
        input.set_type("radio");
        input.set_name(&self.0.name);
        input.set_checked(false);
        input.set_disabled(!enabled);
        let caption = create(&self.0.document, "span")?;
        caption.set_text_content(Some(text));
        let style = label.style();
        let _ = style.set_property("display", "inline-flex");
        let _ = style.set_property("align-items", "center");
        let _ = style.set_property("gap", "0.4em");
        label
            .append_child(&input)
            .map_err(|e| crate::to_error("ラジオボタンの組み立て", e))?;
        label
            .append_child(&caption)
            .map_err(|e| crate::to_error("ラジオボタンの組み立て", e))?;
        self.0
            .element
            .append_child(&label)
            .map_err(|e| crate::to_error("ラジオグループの組み立て", e))?;
        Ok(input)
    }
}
