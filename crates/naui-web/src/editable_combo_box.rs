//! A combo box that accepts free input (the DOM's `<input list>` + `<datalist>`).
//!
//! This combination is the browser's standard "field you can type into plus a list
//! of suggestions", so naui builds no visuals. How suggestions are narrowed (prefix
//! or substring), whether an arrow is shown and how the list opens are left to the
//! browser.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use naui_core::Result;
use wasm_bindgen::JsCast;
use web_sys::{Document, Element, HtmlDataListElement, HtmlElement, HtmlInputElement};

use crate::keys::KeyDown;
use crate::to_error;
use crate::widgets::{create, impl_widget, Listener, TextHandler, Widget};

thread_local! {
    /// A counter for creating a unique `id` per `<datalist>`.
    static NEXT_LIST: Cell<u64> = const { Cell::new(0) };
}

fn next_list_id() -> String {
    NEXT_LIST.with(|next| {
        let id = next.get();
        next.set(id.wrapping_add(1));
        format!("naui-datalist-{id}")
    })
}

struct EditableComboBoxInner {
    /// `<span><input list="…"><datalist id="…">…</datalist></span>`
    element: HtmlElement,
    input: HtmlInputElement,
    list: HtmlDataListElement,
    document: Document,
    /// A copy of the suggestions. Used to check matches for `selected` and to write for
    /// `set_selected`.
    items: RefCell<Vec<String>>,
    on_change: TextHandler,
    /// The `input` subscription. Dropping it also unsubscribes.
    _listener: RefCell<Option<Listener>>,
    key_down: KeyDown,
    focus: crate::focus::Focus,
}

/// A field where the user can either choose from suggestions or type freely
/// (`<input list>` + `<datalist>`).
///
/// The value is a string, empty right after creation.
#[derive(Clone)]
pub struct EditableComboBox(Rc<EditableComboBoxInner>);
impl_widget!(EditableComboBox, element);
crate::keys::impl_key_down!(EditableComboBox);
crate::focus::impl_focus_change!(EditableComboBox);

impl EditableComboBox {
    pub(crate) fn new(document: &Document) -> Result<Self> {
        let element: HtmlElement = create(document, "span")?.unchecked_into();
        let input: HtmlInputElement = create(document, "input")?.unchecked_into();
        let list: HtmlDataListElement = create(document, "datalist")?.unchecked_into();

        let id = next_list_id();
        list.set_id(&id);
        input.set_type("text");
        // `list` 属性は id で指す決まりなので、`<datalist>` も同じ木へ入れる
        // (`<datalist>` 自体はブラウザが描かない)。
        let _ = input.set_attribute("list", &id);

        element
            .append_child(&input)
            .map_err(|e| to_error("コンボボックスの組み立て", e))?;
        element
            .append_child(&list)
            .map_err(|e| to_error("コンボボックスの組み立て", e))?;

        // 入れ物は中身ぶんの大きさで、`set_sizing` で広げたときは
        // 入力欄もいっしょに広がるようにしておく。
        let style = element.style();
        let _ = style.set_property("display", "inline-flex");
        let input_style = input.style();
        let _ = input_style.set_property("flex", "1 1 auto");
        let _ = input_style.set_property("min-width", "0");
        let _ = input_style.set_property("box-sizing", "border-box");

        let inner = Rc::new(EditableComboBoxInner {
            element,
            input,
            list,
            document: document.clone(),
            items: RefCell::new(Vec::new()),
            on_change: TextHandler::default(),
            _listener: RefCell::new(None),
            key_down: KeyDown::default(),
            focus: Default::default(),
        });
        // 打鍵でも候補の選択でも `input` が飛ぶので、購読は 1 本で足りる。
        let listener = Listener::attach(inner.input.as_ref(), "input", {
            let weak = Rc::downgrade(&inner);
            move || {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                let text = inner.input.value();
                inner.on_change.emit(&text);
            }
        })?;
        *inner._listener.borrow_mut() = Some(listener);
        Ok(Self(inner))
    }

    /// Rebuilds the suggestions. **The entered text does not change**, and no
    /// notification is sent.
    pub fn set_items<S: AsRef<str>>(&self, items: &[S]) {
        while let Some(child) = self.0.list.first_child() {
            let _ = self.0.list.remove_child(&child);
        }
        for item in items {
            let Ok(option) = create(&self.0.document, "option") else {
                continue;
            };
            // `<datalist>` の中では、値は `value` 属性で渡す。
            let _ = option.set_attribute("value", item.as_ref());
            let _ = self.0.list.append_child(&option);
        }
        *self.0.items.borrow_mut() = items.iter().map(|s| s.as_ref().to_string()).collect();
    }

    /// The number of suggestions.
    pub fn len(&self) -> usize {
        self.0.items.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The entered text.
    pub fn text(&self) -> String {
        self.0.input.value()
    }

    /// Replaces the text from code. `on_change` is not called.
    pub fn set_text(&self, text: &str) {
        self.0.input.set_value(text);
    }

    /// The position of the suggestion that **exactly matches** the entered text.
    ///
    /// `None` if the typed text matches no suggestion.
    pub fn selected(&self) -> Option<usize> {
        let text = self.text();
        self.0.items.borrow().iter().position(|item| *item == text)
    }

    /// Chooses an in-range suggestion without notifying. Does nothing if out of range.
    pub fn set_selected(&self, index: usize) {
        let Some(text) = self.0.items.borrow().get(index).cloned() else {
            return;
        };
        self.set_text(&text);
    }

    /// Empties the text without notifying.
    pub fn clear(&self) {
        self.set_text("");
    }

    /// Chooses an in-range suggestion and notifies, as if the user had chosen it.
    pub fn select(&self, index: usize) {
        let Some(text) = self.0.items.borrow().get(index).cloned() else {
            return;
        };
        self.set_text(&text);
        self.0.on_change.emit(&text);
    }

    pub fn set_placeholder(&self, text: &str) {
        self.0.input.set_placeholder(text);
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.0.input.set_disabled(!enabled);
    }

    /// Called with the current contents every time the text changes. Called for both
    /// typing and choosing a suggestion. Setting it again removes the previous one.
    pub fn on_change(&self, f: impl FnMut(&str) + 'static) {
        self.0.on_change.set(f);
    }

    /// The field's `<input>`. Public as a backend-specific escape hatch.
    pub fn native_input(&self) -> HtmlInputElement {
        self.0.input.clone()
    }
}
