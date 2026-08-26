//! 自由に入力できるコンボボックス (DOM の `<input list>` + `<datalist>`)。
//!
//! ブラウザ標準の「打ち込める入力欄と候補の一覧」がこの組み合わせなので、
//! naui は見た目を作らない。候補の絞り込み方 (前方一致か部分一致か)・矢印を
//! 出すかどうか・一覧の開き方はブラウザに任せている。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use naui_core::Result;
use wasm_bindgen::JsCast;
use web_sys::{Document, Element, HtmlDataListElement, HtmlElement, HtmlInputElement};

use crate::to_error;
use crate::widgets::{create, impl_widget, Listener, TextHandler, Widget};

thread_local! {
    /// `<datalist>` ごとに一意な `id` を作るための連番。
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
    /// 候補の控え。`selected` の一致判定と `set_selected` の書き込みに使う。
    items: RefCell<Vec<String>>,
    on_change: TextHandler,
    /// `input` の購読。落とすと購読も外れる。
    _listener: RefCell<Option<Listener>>,
}

/// 候補から選ぶことも、自由に打ち込むこともできる入力欄
/// (`<input list>` + `<datalist>`)。
///
/// 値は文字列で、作った直後は空。
#[derive(Clone)]
pub struct EditableComboBox(Rc<EditableComboBoxInner>);
impl_widget!(EditableComboBox, element);

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

    /// 候補を作り直す。**入力されている文字列は変わらず**、通知も出ない。
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

    /// 候補の数。
    pub fn len(&self) -> usize {
        self.0.items.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// 入力されている文字列。
    pub fn text(&self) -> String {
        self.0.input.value()
    }

    /// プログラムから文字列を差し替える。`on_change` は呼ばれない。
    pub fn set_text(&self, text: &str) {
        self.0.input.set_value(text);
    }

    /// 入力されている文字列と**そのまま一致する**候補の位置。
    ///
    /// 打ち込まれた文字列がどの候補とも一致しなければ `None`。
    pub fn selected(&self) -> Option<usize> {
        let text = self.text();
        self.0.items.borrow().iter().position(|item| *item == text)
    }

    /// 範囲内の候補を通知せずに選ぶ。範囲外なら何もしない。
    pub fn set_selected(&self, index: usize) {
        let Some(text) = self.0.items.borrow().get(index).cloned() else {
            return;
        };
        self.set_text(&text);
    }

    /// 通知せずに文字列を空にする。
    pub fn clear(&self) {
        self.set_text("");
    }

    /// 利用者が候補を選んだのと同じように、範囲内の候補を選んで通知する。
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

    /// 文字列が変わるたびに、その時点の中身で呼ばれる。
    /// 打鍵と候補の選択のどちらでも呼ばれる。設定し直すと以前のものは外れる。
    pub fn on_change(&self, f: impl FnMut(&str) + 'static) {
        self.0.on_change.set(f);
    }

    /// 入力欄の `<input>`。バックエンド固有の脱出口として公開している。
    pub fn native_input(&self) -> HtmlInputElement {
        self.0.input.clone()
    }
}
