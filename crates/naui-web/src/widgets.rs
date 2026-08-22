//! DOM の標準コントロールを包むハンドル群。
//!
//! 見た目はブラウザ既定のまま。CSS は Flexbox のレイアウトにしか使わない。

use std::cell::RefCell;
use std::rc::Rc;

use naui_core::{Align, Orientation, Padding, Result};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use web_sys::{
    Document, Element, HtmlElement, HtmlInputElement, HtmlProgressElement, HtmlTextAreaElement,
};

use crate::to_error;

/// naui のウィジェットが実装する共通インタフェース。
pub trait Widget: 'static {
    /// 対応する DOM 要素。バックエンド固有の脱出口として公開している。
    fn native_element(&self) -> Element;

    #[doc(hidden)]
    fn boxed_clone(&self) -> Box<dyn Widget>;
}

macro_rules! impl_widget {
    ($t:ty, $field:ident) => {
        impl Widget for $t {
            fn native_element(&self) -> Element {
                self.0.$field.clone().unchecked_into()
            }
            fn boxed_clone(&self) -> Box<dyn Widget> {
                Box::new(self.clone())
            }
        }

        impl $t {
            /// 大きさを指定する。呼ぶたびに以前の指定は外れる。
            ///
            /// 実際の大きさを決めるのはブラウザの CSS レイアウトなので、
            /// ここで渡すのは `width` / `min-width` などの指定だけ。
            pub fn set_sizing(&self, sizing: naui_core::Sizing) {
                let element = <$t as Widget>::native_element(self);
                crate::layout::apply_sizing(&element, sizing);
            }
        }
    };
}

pub(crate) use impl_widget;

type SelectCallback = Box<dyn FnMut(usize)>;

/// 選択されたインデックスの通知先。
///
/// 呼び出している間だけクロージャを取り出すため、通知の中から同じ
/// ウィジェットを操作しても `RefCell` の二重借用にならない。通知中に
/// `on_select` を呼び直した場合は、新しいクロージャを残す。
#[derive(Default)]
pub(crate) struct SelectionHandler(RefCell<Option<SelectCallback>>);

impl SelectionHandler {
    pub(crate) fn set(&self, f: impl FnMut(usize) + 'static) {
        *self.0.borrow_mut() = Some(Box::new(f));
    }

    pub(crate) fn emit(&self, index: usize) {
        let Some(mut f) = self.0.borrow_mut().take() else {
            return;
        };
        f(index);
        let mut slot = self.0.borrow_mut();
        if slot.is_none() {
            *slot = Some(f);
        }
    }
}

/// 値を 1 つ受け取る通知先。
///
/// [`SelectionHandler`] と同じ再入対応を、インデックス以外の値でも使うための
/// もの (日付ピッカーの `on_change` など)。
pub(crate) struct ValueHandler<T>(RefCell<Option<Box<dyn FnMut(T)>>>);

impl<T> Default for ValueHandler<T> {
    fn default() -> Self {
        Self(RefCell::new(None))
    }
}

impl<T> ValueHandler<T> {
    pub(crate) fn set(&self, f: impl FnMut(T) + 'static) {
        *self.0.borrow_mut() = Some(Box::new(f));
    }

    pub(crate) fn emit(&self, value: T) {
        let Some(mut f) = self.0.borrow_mut().take() else {
            return;
        };
        f(value);
        let mut slot = self.0.borrow_mut();
        if slot.is_none() {
            *slot = Some(f);
        }
    }
}

pub(crate) fn create(doc: &Document, tag: &str) -> Result<Element> {
    doc.create_element(tag)
        .map_err(|e| to_error("DOM 要素の生成", e))
}

/// クリック等のイベントを購読し、ハンドルが生きている間だけ有効にする。
pub(crate) struct Listener {
    target: web_sys::EventTarget,
    event: &'static str,
    closure: Closure<dyn FnMut(web_sys::Event)>,
}

impl Listener {
    pub(crate) fn attach(
        target: &web_sys::EventTarget,
        event: &'static str,
        mut f: impl FnMut() + 'static,
    ) -> Result<Self> {
        Self::attach_event(target, event, move |_event| f())
    }

    pub(crate) fn attach_event(
        target: &web_sys::EventTarget,
        event: &'static str,
        f: impl FnMut(web_sys::Event) + 'static,
    ) -> Result<Self> {
        let closure = Closure::<dyn FnMut(web_sys::Event)>::new(f);
        target
            .add_event_listener_with_callback(event, closure.as_ref().unchecked_ref())
            .map_err(|e| to_error("イベントの購読", e))?;
        Ok(Self {
            target: target.clone(),
            event,
            closure,
        })
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        let _ = self
            .target
            .remove_event_listener_with_callback(self.event, self.closure.as_ref().unchecked_ref());
    }
}

// ------------------------------------------------------------------ Label

struct LabelInner {
    element: HtmlElement,
}

/// テキスト表示 (`<span>`)。
#[derive(Clone)]
pub struct Label(Rc<LabelInner>);
impl_widget!(Label, element);

impl Label {
    pub(crate) fn new(doc: &Document, text: &str) -> Result<Self> {
        let element: HtmlElement = create(doc, "span")?.unchecked_into();
        element.set_text_content(Some(text));
        Ok(Self(Rc::new(LabelInner { element })))
    }

    pub fn text(&self) -> String {
        self.0.element.text_content().unwrap_or_default()
    }

    pub fn set_text(&self, text: &str) {
        self.0.element.set_text_content(Some(text));
    }
}

// ----------------------------------------------------------------- Button

struct ButtonInner {
    element: HtmlElement,
    listener: RefCell<Option<Listener>>,
}

/// 押しボタン (`<button>`)。
#[derive(Clone)]
pub struct Button(Rc<ButtonInner>);
impl_widget!(Button, element);

impl Button {
    pub(crate) fn new(doc: &Document, text: &str) -> Result<Self> {
        let element: HtmlElement = create(doc, "button")?.unchecked_into();
        element.set_text_content(Some(text));
        Ok(Self(Rc::new(ButtonInner {
            element,
            listener: RefCell::new(None),
        })))
    }

    pub fn set_text(&self, text: &str) {
        self.0.element.set_text_content(Some(text));
    }

    pub fn set_enabled(&self, enabled: bool) {
        set_disabled(&self.0.element, !enabled);
    }

    /// クリックされたときに呼ばれる。設定し直すと以前のものは外れる。
    pub fn on_click(&self, f: impl FnMut() + 'static) {
        let listener = Listener::attach(self.0.element.as_ref(), "click", f).ok();
        *self.0.listener.borrow_mut() = listener;
    }

    /// クリックを発生させる (テストや自動操作用)。
    pub fn click(&self) {
        self.0.element.click();
    }
}

// --------------------------------------------------------------- Checkbox

struct CheckboxInner {
    /// `<label><input type="checkbox">テキスト</label>`
    element: HtmlElement,
    input: HtmlInputElement,
    listener: RefCell<Option<Listener>>,
}

/// チェックボックス (`<input type="checkbox">`)。
#[derive(Clone)]
pub struct Checkbox(Rc<CheckboxInner>);
impl_widget!(Checkbox, element);

impl Checkbox {
    pub(crate) fn new(doc: &Document, label: &str) -> Result<Self> {
        let element: HtmlElement = create(doc, "label")?.unchecked_into();
        let input: HtmlInputElement = create(doc, "input")?.unchecked_into();
        input.set_type("checkbox");
        let text = create(doc, "span")?;
        text.set_text_content(Some(label));
        element
            .append_child(&input)
            .map_err(|e| to_error("チェックボックスの組み立て", e))?;
        element
            .append_child(&text)
            .map_err(|e| to_error("チェックボックスの組み立て", e))?;
        // ラベルと四角を横に並べる。
        let style = element.style();
        let _ = style.set_property("display", "inline-flex");
        let _ = style.set_property("align-items", "center");
        let _ = style.set_property("gap", "0.4em");

        Ok(Self(Rc::new(CheckboxInner {
            element,
            input,
            listener: RefCell::new(None),
        })))
    }

    pub fn is_checked(&self) -> bool {
        self.0.input.checked()
    }

    pub fn set_checked(&self, checked: bool) {
        self.0.input.set_checked(checked);
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.0.input.set_disabled(!enabled);
    }

    /// 状態が変わったときに、変更後の値で呼ばれる。
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
}

// -------------------------------------------------------------- TextInput

struct TextInputInner {
    input: HtmlInputElement,
    on_change: RefCell<Option<Listener>>,
}

/// 1 行テキスト入力 (`<input type="text">`)。IME はブラウザが処理する。
#[derive(Clone)]
pub struct TextInput(Rc<TextInputInner>);
impl_widget!(TextInput, input);

impl TextInput {
    pub(crate) fn new(doc: &Document, text: &str) -> Result<Self> {
        let input: HtmlInputElement = create(doc, "input")?.unchecked_into();
        input.set_type("text");
        input.set_value(text);
        Ok(Self(Rc::new(TextInputInner {
            input,
            on_change: RefCell::new(None),
        })))
    }

    pub fn text(&self) -> String {
        self.0.input.value()
    }

    pub fn set_text(&self, text: &str) {
        self.0.input.set_value(text);
    }

    pub fn set_placeholder(&self, text: &str) {
        self.0.input.set_placeholder(text);
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.0.input.set_disabled(!enabled);
    }

    /// 1 文字入力するたびに、その時点の文字列で呼ばれる。
    pub fn on_change(&self, mut f: impl FnMut(&str) + 'static) {
        let input = self.0.input.clone();
        let listener = Listener::attach(self.0.input.as_ref(), "input", move || {
            f(&input.value());
        })
        .ok();
        *self.0.on_change.borrow_mut() = listener;
    }
}

// --------------------------------------------------------------- TextArea

struct TextAreaInner {
    element: HtmlTextAreaElement,
    on_change: RefCell<Option<Listener>>,
}

/// 複数行テキスト入力 (`<textarea>`)。IME はブラウザが処理する。
#[derive(Clone)]
pub struct TextArea(Rc<TextAreaInner>);
impl_widget!(TextArea, element);

impl TextArea {
    pub(crate) fn new(doc: &Document, text: &str) -> Result<Self> {
        let element: HtmlTextAreaElement = create(doc, "textarea")?.unchecked_into();
        element.set_value(text);
        Ok(Self(Rc::new(TextAreaInner {
            element,
            on_change: RefCell::new(None),
        })))
    }

    /// いまの文字列。改行はそのまま含まれる。
    pub fn text(&self) -> String {
        self.0.element.value()
    }

    /// 文字列を置き換える。`on_change` は呼ばれない。
    pub fn set_text(&self, text: &str) {
        self.0.element.set_value(text);
    }

    /// 何も入力されていないときに薄く出る文字。
    pub fn set_placeholder(&self, text: &str) {
        self.0.element.set_placeholder(text);
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.0.element.set_disabled(!enabled);
    }

    /// 1 文字入力するたびに、その時点の文字列で呼ばれる。
    ///
    /// 改行の入力でも呼ばれる。`set_text` では呼ばれない。
    pub fn on_change(&self, mut f: impl FnMut(&str) + 'static) {
        let element = self.0.element.clone();
        let listener = Listener::attach(self.0.element.as_ref(), "input", move || {
            f(&element.value());
        })
        .ok();
        *self.0.on_change.borrow_mut() = listener;
    }
}

// ----------------------------------------------------------------- Slider

struct SliderInner {
    input: HtmlInputElement,
    listener: RefCell<Option<Listener>>,
    min: f64,
    max: f64,
}

/// スライダー (`<input type="range">`)。
#[derive(Clone)]
pub struct Slider(Rc<SliderInner>);
impl_widget!(Slider, input);

impl Slider {
    pub(crate) fn new(doc: &Document, min: f64, max: f64) -> Result<Self> {
        let input: HtmlInputElement = create(doc, "input")?.unchecked_into();
        input.set_type("range");
        input.set_min(&min.to_string());
        input.set_max(&max.to_string());
        // 既定の step は 1 なので、連続値を扱えるように細かくする。
        input.set_step(&((max - min) / 1000.0).to_string());
        input.set_value(&min.to_string());
        Ok(Self(Rc::new(SliderInner {
            input,
            listener: RefCell::new(None),
            min,
            max,
        })))
    }

    pub fn value(&self) -> f64 {
        self.0.input.value_as_number()
    }

    pub fn set_value(&self, value: f64) {
        self.0
            .input
            .set_value(&value.clamp(self.0.min, self.0.max).to_string());
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.0.input.set_disabled(!enabled);
    }

    /// つまみが動くたびに、その値で呼ばれる。
    pub fn on_change(&self, mut f: impl FnMut(f64) + 'static) {
        let input = self.0.input.clone();
        let listener = Listener::attach(self.0.input.as_ref(), "input", move || {
            f(input.value_as_number());
        })
        .ok();
        *self.0.listener.borrow_mut() = listener;
    }
}

// ------------------------------------------------------------ ProgressBar

struct ProgressInner {
    element: HtmlProgressElement,
}

/// 進捗バー (`<progress>`)。
#[derive(Clone)]
pub struct ProgressBar(Rc<ProgressInner>);
impl_widget!(ProgressBar, element);

impl ProgressBar {
    pub(crate) fn new(doc: &Document) -> Result<Self> {
        let element: HtmlProgressElement = create(doc, "progress")?.unchecked_into();
        element.set_max(1.0);
        element.set_value(0.0);
        Ok(Self(Rc::new(ProgressInner { element })))
    }

    /// 0.0..=1.0。
    pub fn set_value(&self, value: f64) {
        self.0.element.set_value(value.clamp(0.0, 1.0));
    }

    pub fn value(&self) -> f64 {
        self.0.element.value()
    }
}

// ------------------------------------------------------------------ Stack

struct StackInner {
    element: HtmlElement,
    orientation: Orientation,
    children: RefCell<Vec<Box<dyn Widget>>>,
}

/// 縦 / 横に子を並べるコンテナ (Flexbox の `<div>`)。
#[derive(Clone)]
pub struct Stack(Rc<StackInner>);
impl_widget!(Stack, element);

impl Stack {
    pub(crate) fn new(doc: &Document, orientation: Orientation) -> Result<Self> {
        let element: HtmlElement = create(doc, "div")?.unchecked_into();
        let style = element.style();
        let _ = style.set_property("display", "flex");
        let _ = style.set_property(
            "flex-direction",
            if orientation.is_vertical() {
                "column"
            } else {
                "row"
            },
        );
        let _ = style.set_property("align-items", "center");
        crate::layout::mark_parent(&element, crate::layout::ParentLayout::Flex(orientation));
        Ok(Self(Rc::new(StackInner {
            element,
            orientation,
            children: RefCell::new(Vec::new()),
        })))
    }

    pub fn set_spacing(&self, spacing: f64) {
        let _ = self
            .0
            .element
            .style()
            .set_property("gap", &format!("{spacing}px"));
    }

    pub fn set_padding(&self, padding: Padding) {
        let _ = self.0.element.style().set_property(
            "padding",
            &format!(
                "{}px {}px {}px {}px",
                padding.top, padding.right, padding.bottom, padding.left
            ),
        );
    }

    pub fn set_align(&self, align: Align) {
        let value = match align {
            Align::Start => "flex-start",
            Align::Center => "center",
            Align::End => "flex-end",
            Align::Fill => "stretch",
        };
        let _ = self.0.element.style().set_property("align-items", value);
    }

    /// 末尾に子を追加する。
    ///
    /// 子が [`naui_core::Length::Fill`] を指定していれば、主軸なら
    /// `flex-grow`、交差軸なら `align-self: stretch` をここで付ける。
    pub fn append(&self, child: &dyn Widget) {
        let element = child.native_element();
        if self.0.element.append_child(&element).is_ok() {
            crate::layout::apply_child_layout(
                &element,
                crate::layout::ParentLayout::Flex(self.0.orientation),
            );
            self.0.children.borrow_mut().push(child.boxed_clone());
        }
    }

    pub fn len(&self) -> usize {
        self.0.children.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

pub(crate) fn set_disabled(element: &HtmlElement, disabled: bool) {
    if disabled {
        let _ = element.set_attribute("disabled", "");
    } else {
        element.remove_attribute("disabled").ok();
    }
}
