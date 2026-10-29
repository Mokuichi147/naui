//! Handles wrapping the DOM's standard controls.
//!
//! The look stays the browser's default. CSS is used only for Flexbox layout.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use naui_core::{Align, Orientation, Padding, Result, TextColor, TextStyle};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use web_sys::{
    Document, Element, HtmlElement, HtmlInputElement, HtmlProgressElement, HtmlTextAreaElement,
    KeyboardEvent,
};

use crate::keys::{impl_key_down, KeyDown};
use crate::to_error;

/// The common interface implemented by naui's widgets.
pub trait Widget: 'static {
    /// The corresponding DOM element. Public as a backend-specific escape hatch.
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
            /// Sets the size. Each call discards the previous setting.
            ///
            /// The actual size is decided by the browser's CSS layout, so what you pass here is
            /// only settings such as `width` / `min-width`.
            pub fn set_sizing(&self, sizing: naui_core::Sizing) {
                let element = <$t as Widget>::native_element(self);
                crate::layout::apply_sizing(&element, sizing);
            }

            /// Whether it is shown. Shown by default.
            ///
            /// A hidden widget takes no space in a `Stack`. In a `Grid`, its cell remains.
            pub fn set_visible(&self, visible: bool) {
                crate::interaction::set_visible(&<$t as Widget>::native_element(self), visible);
            }

            /// The visibility set with [`set_visible`](Self::set_visible). Stays `true` even
            /// if a parent is hidden.
            pub fn is_visible(&self) -> bool {
                crate::interaction::is_visible(&<$t as Widget>::native_element(self))
            }

            /// Moves keyboard focus here. `true` if it moved.
            ///
            /// If the widget itself cannot take focus (containers, for example), focus moves to
            /// the first thing inside that can. `false` before it is in the document, while
            /// hidden, or while disabled.
            pub fn request_focus(&self) -> bool {
                crate::interaction::request_focus(&<$t as Widget>::native_element(self))
            }

            /// The description shown when the pointer hovers over it (the `title` attribute).
            /// `None` removes it.
            pub fn set_tooltip(&self, text: Option<&str>) {
                crate::interaction::set_tooltip(&<$t as Widget>::native_element(self), text);
            }

            /// The name reported to screen readers. `None` removes it (the visible text is
            /// used).
            ///
            /// Set it when there is no visible text, or the text does not fully convey the
            /// meaning, as with icon-only buttons.
            pub fn set_accessible_label(&self, text: Option<&str>) {
                crate::interaction::set_accessible_label(
                    &<$t as Widget>::native_element(self),
                    text,
                );
            }

            /// Called with the files dropped onto this widget from the OS.
            ///
            /// When widgets with this closure are nested, the innermost one under the
            /// pointer takes the drop, and the window's
            /// [`on_file_drop`](crate::Window::on_file_drop) gets only drops no widget
            /// takes. Setting it again replaces the previous closure.
            ///
            /// The browser hands over contents, not paths:
            /// [`FileEntry::path`](naui_core::FileEntry::path) is `None`, and
            /// [`FileEntry::source`](naui_core::FileEntry::source) is a `blob:` URL that
            /// stays valid until the next drop onto this widget, or until the widget is
            /// dropped. A dropped folder comes with only its name. Drops onto text
            /// fields do not arrive.
            pub fn on_file_drop(&self, f: impl FnMut(&[naui_core::FileEntry]) + 'static) {
                self.0
                    .file_drop
                    .set(&<$t as Widget>::native_element(self), f);
            }

            /// Creates a weak reference. Capture it when touching this widget from its own
            /// notification closures (capturing a `clone()` creates a cycle and it is never
            /// freed).
            pub fn downgrade(&self) -> naui_core::WeakWidget<Self> {
                let weak = std::rc::Rc::downgrade(&self.0);
                naui_core::WeakWidget::new(move || weak.upgrade().map(Self))
            }
        }
    };
}

pub(crate) use impl_widget;

type SelectCallback = Box<dyn FnMut(usize)>;

/// The handler for the selected index.
///
/// The closure is taken out only while it is being called, so operating on the same
/// widget from inside the notification does not double-borrow the `RefCell`. If
/// `on_select` is called again during the notification, the new closure is kept.
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

/// A handler that receives a string.
///
/// The same re-entrancy handling as [`SelectionHandler`], with `&str`.
/// `ValueHandler<T>` cannot receive borrowed values (it would need a lifetime
/// parameter), so this is provided separately.
#[derive(Default)]
pub(crate) struct TextHandler(RefCell<Option<Box<dyn FnMut(&str)>>>);

impl TextHandler {
    pub(crate) fn set(&self, f: impl FnMut(&str) + 'static) {
        *self.0.borrow_mut() = Some(Box::new(f));
    }

    pub(crate) fn emit(&self, text: &str) {
        let Some(mut f) = self.0.borrow_mut().take() else {
            return;
        };
        f(text);
        let mut slot = self.0.borrow_mut();
        if slot.is_none() {
            *slot = Some(f);
        }
    }
}

/// A handler that receives one value.
///
/// For using the same re-entrancy handling as [`SelectionHandler`] with values other
/// than indices (the date picker's `on_change`, for example).
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

    /// Whether a closure is set.
    pub(crate) fn is_set(&self) -> bool {
        self.0.borrow().is_some()
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

/// Subscribes to events such as clicks, valid only while the handle is alive.
pub(crate) struct Listener {
    target: web_sys::EventTarget,
    event: &'static str,
    closure: Callback,
    capture: bool,
}

/// The body of a subscription. wasm-bindgen throws if an `FnMut` closure is called
/// again while it is being called (as when `focus()` fires `focusin`
/// synchronously). Things that may be called re-entrantly are held as `Fn`.
enum Callback {
    Mut(Closure<dyn FnMut(web_sys::Event)>),
    Shared(Closure<dyn Fn(web_sys::Event)>),
}

impl Callback {
    fn function(&self) -> &js_sys::Function {
        match self {
            Self::Mut(closure) => closure.as_ref().unchecked_ref(),
            Self::Shared(closure) => closure.as_ref().unchecked_ref(),
        }
    }
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
        let closure = Callback::Mut(Closure::<dyn FnMut(web_sys::Event)>::new(f));
        Self::add(target, event, closure, false)
    }

    /// A subscription that may be called re-entrantly. Even if the same event fires
    /// synchronously from inside it (calling `focus()` inside `focusin`, for example),
    /// the inner call is delivered too.
    pub(crate) fn attach_reentrant(
        target: &web_sys::EventTarget,
        event: &'static str,
        f: impl Fn(web_sys::Event) + 'static,
    ) -> Result<Self> {
        let closure = Callback::Shared(Closure::<dyn Fn(web_sys::Event)>::new(f));
        Self::add(target, event, closure, false)
    }

    /// Subscribes in the capture phase. On the target element, it is called before
    /// regular subscriptions.
    pub(crate) fn attach_capture(
        target: &web_sys::EventTarget,
        event: &'static str,
        f: impl FnMut(web_sys::Event) + 'static,
    ) -> Result<Self> {
        let closure = Callback::Mut(Closure::<dyn FnMut(web_sys::Event)>::new(f));
        Self::add(target, event, closure, true)
    }

    fn add(
        target: &web_sys::EventTarget,
        event: &'static str,
        closure: Callback,
        capture: bool,
    ) -> Result<Self> {
        target
            .add_event_listener_with_callback_and_bool(event, closure.function(), capture)
            .map_err(|e| to_error("イベントの購読", e))?;
        Ok(Self {
            target: target.clone(),
            event,
            closure,
            capture,
        })
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        let _ = self.target.remove_event_listener_with_callback_and_bool(
            self.event,
            self.closure.function(),
            self.capture,
        );
    }
}

// ------------------------------------------------------------------ Label

struct LabelInner {
    element: HtmlElement,
    /// `on_file_drop`.
    file_drop: crate::file_drop::FileDrop,
}

/// Text display (`<span>`).
#[derive(Clone)]
pub struct Label(Rc<LabelInner>);
impl_widget!(Label, element);

impl Label {
    pub(crate) fn new(doc: &Document, text: &str) -> Result<Self> {
        let element: HtmlElement = create(doc, "span")?.unchecked_into();
        element.set_text_content(Some(text));
        let this = Self(Rc::new(LabelInner {
            element,
            file_drop: Default::default(),
        }));
        this.set_wrap(false);
        // 他の 3 環境のラベルと同じく、既定では選べない。
        this.set_selectable(false);
        Ok(this)
    }

    pub fn text(&self) -> String {
        self.0.element.text_content().unwrap_or_default()
    }

    pub fn set_text(&self, text: &str) {
        self.0.element.set_text_content(Some(text));
    }

    /// Whether the text can be selected and copied. Not selectable by default.
    ///
    /// Use it for text readers will want to copy, such as chat messages or error
    /// details. Unlike a text field, the text cannot be edited.
    ///
    /// All text in a browser is selectable by default, so when it should not be, naui
    /// adds `user-select: none` to match the other three platforms.
    pub fn set_selectable(&self, selectable: bool) {
        let style = self.0.element.style();
        if selectable {
            let _ = style.remove_property("user-select");
            let _ = style.remove_property("-webkit-user-select");
        } else {
            let _ = style.set_property("user-select", "none");
            let _ = style.set_property("-webkit-user-select", "none");
        }
    }

    pub fn is_selectable(&self) -> bool {
        // WebKit は接頭辞の無い `user-select` を style に残さないので、両方を見る。
        let style = self.0.element.style();
        ["user-select", "-webkit-user-select"]
            .iter()
            .all(|name| style.get_property_value(name).map_or(true, |v| v != "none"))
    }

    /// Whether long text wraps. Does not wrap by default.
    ///
    /// When not wrapping, it stays on one line and whatever does not fit is cut off
    /// with a trailing ellipsis (…) (`text-overflow: ellipsis`). **A `<span>` wraps by
    /// default**, so this is one place where naui uses CSS to match the other three
    /// platforms.
    pub fn set_wrap(&self, wrap: bool) {
        let style = self.0.element.style();
        if wrap {
            let _ = style.remove_property("white-space");
            let _ = style.remove_property("overflow");
            let _ = style.remove_property("text-overflow");
        } else {
            let _ = style.set_property("white-space", "nowrap");
            let _ = style.set_property("overflow", "hidden");
            let _ = style.set_property("text-overflow", "ellipsis");
        }
    }

    /// The level of text size and weight. The default is [`TextStyle::Body`].
    ///
    /// **Browsers have no heading levels as such**, so this is one place where naui uses
    /// CSS to match the other three platforms (the same treatment as label wrapping).
    /// Sizes are relative `em` values, so they follow the base font size set by the
    /// browser and the user.
    pub fn set_style(&self, style: TextStyle) {
        set_or_clear(&self.0.element, "font-size", style.css_font_size());
        set_or_clear(&self.0.element, "font-weight", style.css_font_weight());
    }

    /// The text color role. The default is [`TextColor::Default`].
    ///
    /// The secondary and accent colors use the CSS system colors (`GrayText` /
    /// `AccentColor`) as is, so they follow the browser and OS settings. There are no
    /// system colors for success, warning and danger, so naui decides just those with
    /// `light-dark()`.
    pub fn set_color(&self, color: TextColor) {
        set_or_clear(&self.0.element, "color", color.css_color());
    }
}

/// Writes a CSS property, or with `None`, removes it to return to the browser's
/// default.
fn set_or_clear(element: &HtmlElement, property: &str, value: Option<&str>) {
    let style = element.style();
    match value {
        Some(value) => {
            let _ = style.set_property(property, value);
        }
        None => {
            let _ = style.remove_property(property);
        }
    }
}

// ----------------------------------------------------------------- Button

struct ButtonInner {
    element: HtmlElement,
    listener: RefCell<Option<Listener>>,
    /// `on_file_drop`.
    file_drop: crate::file_drop::FileDrop,
}

/// A push button (`<button>`).
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
            file_drop: Default::default(),
        })))
    }

    pub fn set_text(&self, text: &str) {
        self.0.element.set_text_content(Some(text));
    }

    pub fn set_enabled(&self, enabled: bool) {
        set_disabled(&self.0.element, !enabled);
    }

    /// Called on click. Setting it again removes the previous one.
    pub fn on_click(&self, f: impl FnMut() + 'static) {
        let listener = Listener::attach(self.0.element.as_ref(), "click", f).ok();
        *self.0.listener.borrow_mut() = listener;
    }

    /// Triggers a click (for tests and automation).
    pub fn click(&self) {
        self.0.element.click();
    }
}

// --------------------------------------------------------------- Checkbox

struct CheckboxInner {
    /// `<label><input type="checkbox">text</label>`
    element: HtmlElement,
    input: HtmlInputElement,
    listener: RefCell<Option<Listener>>,
    /// `on_file_drop`.
    file_drop: crate::file_drop::FileDrop,
}

/// A check box (`<input type="checkbox">`).
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
            file_drop: Default::default(),
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

    /// Called with the new value when the state changes.
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
}

// -------------------------------------------------------------- TextInput

struct TextInputInner {
    input: HtmlInputElement,
    on_change: RefCell<Option<Listener>>,
    on_submit: RefCell<Option<Listener>>,
    key_down: KeyDown,
    focus: crate::focus::Focus,
    /// `on_file_drop`.
    file_drop: crate::file_drop::FileDrop,
}

/// Single-line text input (`<input type="text">`). The browser handles the IME.
#[derive(Clone)]
pub struct TextInput(Rc<TextInputInner>);
impl_key_down!(TextInput);
crate::focus::impl_focus_change!(TextInput);
impl_widget!(TextInput, input);

impl TextInput {
    pub(crate) fn new(doc: &Document, text: &str) -> Result<Self> {
        let input: HtmlInputElement = create(doc, "input")?.unchecked_into();
        input.set_type("text");
        input.set_value(text);
        Ok(Self(Rc::new(TextInputInner {
            input,
            on_change: RefCell::new(None),
            on_submit: RefCell::new(None),
            key_down: KeyDown::default(),
            focus: Default::default(),
            file_drop: Default::default(),
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

    /// Called with the current text every time a character is typed.
    pub fn on_change(&self, mut f: impl FnMut(&str) + 'static) {
        let input = self.0.input.clone();
        let listener = Listener::attach(self.0.input.as_ref(), "input", move || {
            f(&input.value());
        })
        .ok();
        *self.0.on_change.borrow_mut() = listener;
    }

    /// Called with the current text when the user confirms with Enter.
    ///
    /// Not called for an Enter pressed while an IME is composing (which confirms the
    /// conversion). Not called while read-only either.
    pub fn on_submit(&self, mut f: impl FnMut(&str) + 'static) {
        let listener = on_enter(&self.0.input, move |input| {
            if !input.read_only() {
                f(&input.value());
            }
        });
        *self.0.on_submit.borrow_mut() = listener;
    }

    /// Whether it is read-only. Editable by default.
    ///
    /// While read-only, the text can still be selected and copied, and it still takes
    /// focus (unlike `set_enabled(false)`, it is not dimmed). Use it for fields that
    /// show logs or generated output.
    pub fn set_read_only(&self, read_only: bool) {
        self.0.input.set_read_only(read_only);
    }

    pub fn is_read_only(&self) -> bool {
        self.0.input.read_only()
    }
}

// ----------------------------------------------------------- PasswordInput

struct PasswordInputInner {
    input: HtmlInputElement,
    on_change: RefCell<Option<Listener>>,
    on_submit: RefCell<Option<Listener>>,
    key_down: KeyDown,
    focus: crate::focus::Focus,
    /// `on_file_drop`.
    file_drop: crate::file_drop::FileDrop,
}

/// Password input (`<input type="password">`).
///
/// The API is the same as [`TextInput`]; the only difference is that **typed
/// characters are masked**. A mechanism to temporarily reveal the text is not part
/// of what all four platforms share, so there is none.
#[derive(Clone)]
pub struct PasswordInput(Rc<PasswordInputInner>);
impl_key_down!(PasswordInput);
crate::focus::impl_focus_change!(PasswordInput);
impl_widget!(PasswordInput, input);

impl PasswordInput {
    pub(crate) fn new(doc: &Document) -> Result<Self> {
        let input: HtmlInputElement = create(doc, "input")?.unchecked_into();
        input.set_type("password");
        Ok(Self(Rc::new(PasswordInputInner {
            input,
            on_change: RefCell::new(None),
            on_submit: RefCell::new(None),
            key_down: KeyDown::default(),
            focus: Default::default(),
            file_drop: Default::default(),
        })))
    }

    /// The text currently entered.
    pub fn text(&self) -> String {
        self.0.input.value()
    }

    /// Replaces the text. `on_change` is not called.
    pub fn set_text(&self, text: &str) {
        self.0.input.set_value(text);
    }

    pub fn set_placeholder(&self, text: &str) {
        self.0.input.set_placeholder(text);
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.0.input.set_disabled(!enabled);
    }

    /// Called with the current text every time a character is typed.
    pub fn on_change(&self, mut f: impl FnMut(&str) + 'static) {
        let input = self.0.input.clone();
        let listener = Listener::attach(self.0.input.as_ref(), "input", move || {
            f(&input.value());
        })
        .ok();
        *self.0.on_change.borrow_mut() = listener;
    }

    /// Called with the current text when the user confirms with Enter.
    pub fn on_submit(&self, mut f: impl FnMut(&str) + 'static) {
        let listener = on_enter(&self.0.input, move |input| f(&input.value()));
        *self.0.on_submit.borrow_mut() = listener;
    }
}

// ------------------------------------------------------------- SearchInput

struct SearchInputInner {
    input: HtmlInputElement,
    on_change: RefCell<Option<Listener>>,
    on_search: RefCell<Option<Listener>>,
    key_down: KeyDown,
    focus: crate::focus::Focus,
    /// `on_file_drop`.
    file_drop: crate::file_drop::FileDrop,
}

/// A search field (`<input type="search">`).
///
/// The magnifying glass and the clear button (✕) are shown by the browser (whether
/// they appear depends on the browser; naui builds no visuals).
#[derive(Clone)]
pub struct SearchInput(Rc<SearchInputInner>);
impl_key_down!(SearchInput);
crate::focus::impl_focus_change!(SearchInput);
impl_widget!(SearchInput, input);

impl SearchInput {
    pub(crate) fn new(doc: &Document) -> Result<Self> {
        let input: HtmlInputElement = create(doc, "input")?.unchecked_into();
        input.set_type("search");
        Ok(Self(Rc::new(SearchInputInner {
            input,
            on_change: RefCell::new(None),
            on_search: RefCell::new(None),
            key_down: KeyDown::default(),
            focus: Default::default(),
            file_drop: Default::default(),
        })))
    }

    /// The text currently entered.
    pub fn text(&self) -> String {
        self.0.input.value()
    }

    /// Replaces the text. `on_change` is not called.
    pub fn set_text(&self, text: &str) {
        self.0.input.set_value(text);
    }

    pub fn set_placeholder(&self, text: &str) {
        self.0.input.set_placeholder(text);
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.0.input.set_disabled(!enabled);
    }

    /// Called with the current text every time a character is typed.
    pub fn on_change(&self, mut f: impl FnMut(&str) + 'static) {
        let input = self.0.input.clone();
        let listener = Listener::attach(self.0.input.as_ref(), "input", move || {
            f(&input.value());
        })
        .ok();
        *self.0.on_change.borrow_mut() = listener;
    }

    /// Called with the current text when the user confirms with Enter.
    ///
    /// Some browsers do not support the `search` event, so it is picked up from key
    /// presses. **An Enter during composition does not count** (the IME is merely
    /// confirming a conversion, not the search).
    pub fn on_search(&self, mut f: impl FnMut(&str) + 'static) {
        let listener = on_enter(&self.0.input, move |input| f(&input.value()));
        *self.0.on_search.borrow_mut() = listener;
    }
}

/// Calls `f` when Enter is pressed in a single-line field.
///
/// `<input>` has no event that reports only confirmation (`change` also fires when
/// leaving the field), so it is picked up from key presses. **An Enter during
/// composition does not count** ([`is_submit_key`](crate::keys::is_submit_key)).
/// Keys for which `on_key_down` returned `Handled` are stopped in the capture phase
/// and never reach here.
pub(crate) fn on_enter(
    input: &HtmlInputElement,
    mut f: impl FnMut(&HtmlInputElement) + 'static,
) -> Option<Listener> {
    let target = input.clone();
    Listener::attach_event(input.as_ref(), "keydown", move |event| {
        let Some(key) = event.dyn_ref::<KeyboardEvent>() else {
            return;
        };
        if crate::keys::is_submit_key(key) {
            f(&target);
        }
    })
    .ok()
}

// --------------------------------------------------------------- TextArea

struct TextAreaInner {
    element: HtmlTextAreaElement,
    on_change: RefCell<Option<Listener>>,
    key_down: KeyDown,
    focus: crate::focus::Focus,
    /// `on_file_drop`.
    file_drop: crate::file_drop::FileDrop,
}

/// Multi-line text input (`<textarea>`). The browser handles the IME.
#[derive(Clone)]
pub struct TextArea(Rc<TextAreaInner>);
impl_key_down!(TextArea);
crate::focus::impl_focus_change!(TextArea);
impl_widget!(TextArea, element);

impl TextArea {
    pub(crate) fn new(doc: &Document, text: &str) -> Result<Self> {
        let element: HtmlTextAreaElement = create(doc, "textarea")?.unchecked_into();
        element.set_value(text);
        Ok(Self(Rc::new(TextAreaInner {
            element,
            on_change: RefCell::new(None),
            key_down: KeyDown::default(),
            focus: Default::default(),
            file_drop: Default::default(),
        })))
    }

    /// The current text. Newlines are included as is.
    pub fn text(&self) -> String {
        self.0.element.value()
    }

    /// Replaces the text. `on_change` is not called.
    pub fn set_text(&self, text: &str) {
        self.0.element.set_value(text);
    }

    /// Text shown faintly when nothing has been entered.
    pub fn set_placeholder(&self, text: &str) {
        self.0.element.set_placeholder(text);
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.0.element.set_disabled(!enabled);
    }

    /// Called with the current text every time a character is typed.
    ///
    /// Also called for newlines. Not called for `set_text`.
    pub fn on_change(&self, mut f: impl FnMut(&str) + 'static) {
        let element = self.0.element.clone();
        let listener = Listener::attach(self.0.element.as_ref(), "input", move || {
            f(&element.value());
        })
        .ok();
        *self.0.on_change.borrow_mut() = listener;
    }

    /// Whether it is read-only. Editable by default.
    ///
    /// While read-only, the text can still be selected and copied, and it still takes
    /// focus (unlike `set_enabled(false)`, it is not dimmed). Use it for fields that
    /// show logs or generated output.
    pub fn set_read_only(&self, read_only: bool) {
        self.0.element.set_read_only(read_only);
    }

    pub fn is_read_only(&self) -> bool {
        self.0.element.read_only()
    }
}

// ----------------------------------------------------------------- Slider

struct SliderInner {
    input: HtmlInputElement,
    listener: RefCell<Option<Listener>>,
    min: f64,
    max: f64,
    /// `on_file_drop`.
    file_drop: crate::file_drop::FileDrop,
}

/// A slider (`<input type="range">`).
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
            file_drop: Default::default(),
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

    /// Called with the value every time the knob moves.
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
    /// The value that was set. While indeterminate the `value` attribute is removed, so
    /// it is kept separately.
    value: Cell<f64>,
    indeterminate: Cell<bool>,
    /// `on_file_drop`.
    file_drop: crate::file_drop::FileDrop,
}

/// A progress bar (`<progress>`).
#[derive(Clone)]
pub struct ProgressBar(Rc<ProgressInner>);
impl_widget!(ProgressBar, element);

impl ProgressBar {
    pub(crate) fn new(doc: &Document) -> Result<Self> {
        let element: HtmlProgressElement = create(doc, "progress")?.unchecked_into();
        element.set_max(1.0);
        element.set_value(0.0);
        Ok(Self(Rc::new(ProgressInner {
            element,
            value: Cell::new(0.0),
            indeterminate: Cell::new(false),
            file_drop: Default::default(),
        })))
    }

    /// 0.0..=1.0.
    pub fn set_value(&self, value: f64) {
        let value = value.clamp(0.0, 1.0);
        self.0.value.set(value);
        if !self.0.indeterminate.get() {
            self.0.element.set_value(value);
        }
    }

    pub fn value(&self) -> f64 {
        self.0.value.get()
    }

    /// Whether to show work whose progress is unknown (indeterminate progress).
    ///
    /// While `true`, motion shows "working" instead of a value. Turning it off returns
    /// to showing the value set with [`set_value`](Self::set_value) (the value is
    /// remembered).
    ///
    /// `<progress>` shows as indeterminate when it has no `value` attribute.
    pub fn set_indeterminate(&self, indeterminate: bool) {
        self.0.indeterminate.set(indeterminate);
        if indeterminate {
            let _ = self.0.element.remove_attribute("value");
        } else {
            self.0.element.set_value(self.0.value.get());
        }
    }

    pub fn is_indeterminate(&self) -> bool {
        self.0.indeterminate.get()
    }
}

// ---------------------------------------------------------------- Spinner

struct SpinnerInner {
    element: HtmlProgressElement,
    spinning: Cell<bool>,
    /// `on_file_drop`.
    file_drop: crate::file_drop::FileDrop,
}

/// A spinning indicator. The DOM has no spinner, so it is an indeterminate
/// `<progress>` (a bar).
#[derive(Clone)]
pub struct Spinner(Rc<SpinnerInner>);
impl_widget!(Spinner, element);

impl Spinner {
    pub(crate) fn new(doc: &Document) -> Result<Self> {
        // `value` 属性を持たない `<progress>` は不確定の表示になる。
        let element: HtmlProgressElement = create(doc, "progress")?.unchecked_into();
        Ok(Self(Rc::new(SpinnerInner {
            element,
            spinning: Cell::new(true),
            file_drop: Default::default(),
        })))
    }

    /// Whether it spins. Spins from creation.
    ///
    /// While stopped it is not drawn but keeps its space (`visibility: hidden`). Use
    /// `set_visible(false)` to give the space up as well.
    pub fn set_spinning(&self, spinning: bool) {
        self.0.spinning.set(spinning);
        let style = self.0.element.style();
        let _ = if spinning {
            style.remove_property("visibility").map(drop)
        } else {
            style.set_property("visibility", "hidden")
        };
    }

    pub fn is_spinning(&self) -> bool {
        self.0.spinning.get()
    }
}

// ------------------------------------------------------------------ Stack

struct StackInner {
    element: HtmlElement,
    orientation: Cell<Orientation>,
    children: RefCell<Vec<Box<dyn Widget>>>,
    /// `on_file_drop`.
    file_drop: crate::file_drop::FileDrop,
}

/// A container that lays out its children vertically / horizontally (a Flexbox
/// `<div>`).
#[derive(Clone)]
pub struct Stack(Rc<StackInner>);
impl_widget!(Stack, element);

impl Stack {
    pub(crate) fn new(doc: &Document, orientation: Orientation) -> Result<Self> {
        let element: HtmlElement = create(doc, "div")?.unchecked_into();
        let style = element.style();
        let _ = style.set_property("display", "flex");
        let _ = style.set_property("align-items", "center");
        let this = Self(Rc::new(StackInner {
            element,
            orientation: Cell::new(orientation),
            children: RefCell::new(Vec::new()),
            file_drop: Default::default(),
        }));
        this.apply_direction();
        Ok(this)
    }

    /// Switches between laying out vertically and horizontally (`flex-direction`).
    ///
    /// The children stay in the element. Their `flex-grow` and `align-self` are
    /// rewritten for the new main axis, the same as when they are put into a `Stack`.
    pub fn set_orientation(&self, orientation: Orientation) {
        if self.0.orientation.replace(orientation) == orientation {
            return;
        }
        self.apply_direction();
        let parent = self.parent_layout();
        for child in self.0.children.borrow().iter() {
            crate::layout::layout_for_parent(&child.native_element(), parent);
        }
    }

    pub fn orientation(&self) -> Orientation {
        self.0.orientation.get()
    }

    fn apply_direction(&self) {
        let direction = if self.0.orientation.get().is_vertical() {
            "column"
        } else {
            "row"
        };
        let _ = self
            .0
            .element
            .style()
            .set_property("flex-direction", direction);
        crate::layout::mark_parent(&self.0.element, self.parent_layout());
    }

    fn parent_layout(&self) -> crate::layout::ParentLayout {
        crate::layout::ParentLayout::Flex(self.0.orientation.get())
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

    /// Appends a child at the end.
    ///
    /// If the child specifies [`naui_core::Length::Fill`], `flex-grow` is added for the
    /// main axis, or `align-self: stretch` for the cross axis, here.
    pub fn append(&self, child: &dyn Widget) {
        let element = child.native_element();
        if self.0.element.append_child(&element).is_ok() {
            crate::layout::apply_child_layout(&element, self.parent_layout());
            self.0.children.borrow_mut().push(child.boxed_clone());
        }
    }

    /// Inserts a child at the given position. If `index` is at least the current
    /// count, it is appended at the end.
    pub fn insert(&self, index: usize, child: &dyn Widget) {
        let element = child.native_element();
        let mut children = self.0.children.borrow_mut();
        let index = index.min(children.len());
        let anchor = children.get(index).map(|next| next.native_element());
        let inserted = self
            .0
            .element
            .insert_before(&element, anchor.as_ref().map(|next| &**next));
        if inserted.is_ok() {
            crate::layout::apply_child_layout(&element, self.parent_layout());
            children.insert(index, child.boxed_clone());
        }
    }

    /// Removes the child at the given position. Does nothing if out of range.
    pub fn remove(&self, index: usize) {
        let mut children = self.0.children.borrow_mut();
        if index >= children.len() {
            return;
        }
        let child = children.remove(index);
        let _ = self.0.element.remove_child(&child.native_element());
    }

    /// Removes every child.
    pub fn clear(&self) {
        for child in std::mem::take(&mut *self.0.children.borrow_mut()) {
            let _ = self.0.element.remove_child(&child.native_element());
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
