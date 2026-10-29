//! Handles wrapping real WinUI 3 (Fluent 2) controls.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use naui_core::{Align, Orientation, Padding, Result, TextColor, TextStyle};
use naui_winui3::Microsoft::UI::Dispatching::{DispatcherQueue, DispatcherQueueTimer};
use naui_winui3::Microsoft::UI::Xaml::Controls::{
    Button as XamlButton, CheckBox as XamlCheckBox, Grid, PasswordBox, ProgressRing,
    ScrollBarVisibility, ScrollViewer, Slider as XamlSlider, TextBlock, TextBox,
};
use naui_winui3::Microsoft::UI::Xaml::Markup::XamlReader;
use naui_winui3::Microsoft::UI::Xaml::{
    Application, FrameworkElement, ResourceDictionary, RoutedEventHandler, Style, TextWrapping,
    Thickness, UIElement,
};
use windows::Foundation::{EventHandler, PropertyValue, TypedEventHandler};
use windows_core::{IInspectable, Interface, HSTRING};

use crate::to_error;
use crate::ui_thread::{TextHandler, UiThreadCell};

/// The common interface implemented by naui's widgets.
pub trait Widget: 'static {
    /// The corresponding WinUI 3 element. Public as a backend-specific escape hatch.
    fn native_element(&self) -> UIElement;

    #[doc(hidden)]
    fn boxed_clone(&self) -> Box<dyn Widget>;
}

macro_rules! impl_widget {
    ($t:ty, $field:ident) => {
        impl Widget for $t {
            fn native_element(&self) -> UIElement {
                self.0
                    .$field
                    .cast::<UIElement>()
                    .expect("WinUI のコントロールは UIElement である")
            }
            fn boxed_clone(&self) -> Box<dyn Widget> {
                Box::new(self.clone())
            }
        }

        impl Drop for $t {
            fn drop(&mut self) {
                if std::rc::Rc::strong_count(&self.0) != 1 {
                    return;
                }
                let element = <$t as Widget>::native_element(self);
                crate::layout::clear_layout_state(&element);
            }
        }

        impl $t {
            /// Sets the size. Each call replaces the previous setting.
            ///
            /// The actual size is decided by WinUI's layout pass, so what you pass here is only
            /// settings such as `Width` / `MinWidth`.
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
            /// the first thing inside that can. `false` before it is shown, while hidden, or
            /// while disabled.
            pub fn request_focus(&self) -> bool {
                crate::interaction::request_focus(&<$t as Widget>::native_element(self))
            }

            /// The description shown when the pointer hovers over it (`ToolTipService`).
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

            /// Called with the files dropped onto this widget from Explorer or another
            /// app.
            ///
            /// When widgets with this closure are nested, the innermost one under the
            /// pointer takes the drop, and the window's
            /// [`on_file_drop`](crate::Window::on_file_drop) gets only drops no widget
            /// takes. Text fields (`TextBox`) handle drags themselves, so drops onto them
            /// do not arrive. The closure is called after the files have been read, a
            /// moment after the drop. Setting it again replaces the previous closure.
            pub fn on_file_drop(&self, f: impl FnMut(&[naui_core::FileEntry]) + 'static) {
                crate::file_drop::set_widget_handler(&<$t as Widget>::native_element(self), f);
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

/// The handler notified when the selection changes.
///
/// The callback is taken out of the cell once while it is being called. This keeps
/// borrows from conflicting on re-entry when the callback operates on the same
/// widget, and if `on_select` is called again to replace the callback, the new one
/// is not overwritten.
type SelectCallback = Box<dyn FnMut(usize)>;

#[derive(Clone)]
pub(crate) struct SelectHandler(std::sync::Arc<UiThreadCell<Option<SelectCallback>>>);

impl SelectHandler {
    pub(crate) fn new() -> Self {
        Self(std::sync::Arc::new(UiThreadCell::new(None)))
    }

    pub(crate) fn set(&self, f: impl FnMut(usize) + 'static) {
        self.0.with_mut(|slot| *slot = Some(Box::new(f)));
    }

    pub(crate) fn is_set(&self) -> bool {
        self.0.try_with_mut(|slot| slot.is_some()).unwrap_or(false)
    }

    pub(crate) fn emit(&self, index: usize) {
        let Some(Some(mut f)) = self.0.try_with_mut(|slot| slot.take()) else {
            return;
        };
        f(index);
        let _ = self.0.try_with_mut(|slot| {
            if slot.is_none() {
                *slot = Some(f);
            }
        });
    }
}

// ------------------------------------------------------------------ Label

struct LabelInner {
    native: TextBlock,
    /// The level and role currently applied. Remembered so that a `Style` holding both
    /// can be rebuilt even when only one of them changes.
    style: Cell<TextStyle>,
    color: Cell<TextColor>,
}

/// Text display (TextBlock).
#[derive(Clone)]
pub struct Label(Rc<LabelInner>);
impl_widget!(Label, native);

/// The base of a label.
///
/// The ellipsis (`TextTrimming`) is set in XAML. It is a setting that applies only
/// when not wrapping, so it need not be switched at runtime (what is switched is
/// `TextWrapping`).
const LABEL_XAML: &str = r##"<TextBlock
    xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
    TextTrimming="CharacterEllipsis"/>"##;

impl Label {
    pub(crate) fn new(text: &str) -> Result<Self> {
        let native = match XamlReader::Load(&HSTRING::from(LABEL_XAML))
            .and_then(|element| element.cast::<TextBlock>())
        {
            Ok(native) => native,
            Err(error) => {
                eprintln!("naui-windows: ラベルの生成に失敗 (省略記号なしで続けます): {error}");
                TextBlock::new().map_err(|e| to_error("TextBlock の生成", e))?
            }
        };
        native
            .SetText(&HSTRING::from(text))
            .map_err(|e| to_error("TextBlock への設定", e))?;
        let this = Self(Rc::new(LabelInner {
            native,
            style: Cell::new(TextStyle::default()),
            color: Cell::new(TextColor::default()),
        }));
        this.set_wrap(false);
        Ok(this)
    }

    /// Whether the text can be selected and copied. Not selectable by default.
    ///
    /// Use it for text readers will want to copy, such as chat messages or error
    /// details. Unlike a text field, the text cannot be edited.
    pub fn set_selectable(&self, selectable: bool) {
        let _ = self.0.native.SetIsTextSelectionEnabled(selectable);
    }

    pub fn is_selectable(&self) -> bool {
        self.0.native.IsTextSelectionEnabled().unwrap_or(false)
    }

    /// Whether long text wraps. Does not wrap by default.
    ///
    /// When not wrapping, it stays on one line and whatever does not fit is cut off
    /// with a trailing ellipsis (…) (`TextTrimming="CharacterEllipsis"`).
    pub fn set_wrap(&self, wrap: bool) {
        let _ = self.0.native.SetTextWrapping(if wrap {
            TextWrapping::Wrap
        } else {
            TextWrapping::NoWrap
        });
    }

    pub fn text(&self) -> String {
        self.0
            .native
            .Text()
            .map(|s| s.to_string())
            .unwrap_or_default()
    }

    pub fn set_text(&self, text: &str) {
        let _ = self.0.native.SetText(&HSTRING::from(text));
    }

    /// The level of text size and weight. The default is [`TextStyle::Body`].
    ///
    /// The `Style`s of WinUI 3's type ramp (`TitleTextBlockStyle` and so on) are applied
    /// as they are. The Fluent theme decides the point size and weight.
    pub fn set_style(&self, style: TextStyle) {
        self.0.style.set(style);
        self.apply_text_style();
    }

    /// The text color role. The default is [`TextColor::Default`].
    ///
    /// The color is set as a `{ThemeResource}`, so it follows when the window's
    /// `RequestedTheme` is switched (colors written directly into `Foreground` do not
    /// follow the switch).
    pub fn set_color(&self, color: TextColor) {
        self.0.color.set(color);
        self.apply_text_style();
    }

    /// Applies the level and role together as a single `Style`.
    ///
    /// An element can have only one `Style`, so a `Style` holding just the color
    /// `Setter` is layered on top of the type ramp's `Style` (`BasedOn`). If either
    /// cannot be obtained, nothing is done (the previous look continues).
    fn apply_text_style(&self) {
        let Some(style) = label_style(self.0.style.get(), self.0.color.get()) else {
            eprintln!("naui-windows: ラベルのスタイルが引けませんでした");
            return;
        };
        let _ = self.0.native.SetStyle(&style);
    }
}

/// The base of the `Style` holding the color `Setter`. `{brush}` is replaced with
/// the theme resource's name.
///
/// `x:Key` must be the same string as [`LABEL_COLOR_STYLE_KEY`].
const LABEL_COLOR_STYLE_XAML: &str = r##"<ResourceDictionary
    xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
    xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml">
    <Style x:Key="NauiLabelColorStyle" TargetType="TextBlock">
        <Setter Property="Foreground" Value="{ThemeResource {brush}}"/>
    </Style>
</ResourceDictionary>"##;

const LABEL_COLOR_STYLE_KEY: &str = "NauiLabelColorStyle";

/// The `Style` for the level and role. `None` if it cannot be obtained.
///
/// Exposed so that the same look can be applied even where naui's `Label` cannot be
/// used (such as table cells, which decide text alignment directly).
pub(crate) fn label_style(style: TextStyle, color: TextColor) -> Option<Style> {
    let ramp = app_resource(style.xaml_style_key())?.cast::<Style>().ok()?;
    let Some(brush_key) = color.xaml_brush_key() else {
        return Some(ramp);
    };
    let tinted = label_color_style(brush_key)?;
    tinted.SetBasedOn(&ramp).ok()?;
    Some(tinted)
}

/// Creates a `Style` that decides only the text color.
///
/// Once applied, a `Style` is sealed and cannot be changed, so it is recreated on
/// every application.
fn label_color_style(brush_key: &str) -> Option<Style> {
    let xaml = LABEL_COLOR_STYLE_XAML.replace("{brush}", brush_key);
    let dictionary = XamlReader::Load(&HSTRING::from(xaml))
        .and_then(|element| element.cast::<ResourceDictionary>())
        .ok()?;
    PropertyValue::CreateString(&HSTRING::from(LABEL_COLOR_STYLE_KEY))
        .and_then(|key| dictionary.Lookup(&key))
        .and_then(|value| value.cast::<Style>())
        .ok()
}

/// Looks up one entry in the app's resource dictionary. Fluent's type ramp and
/// theme resources are there via `XamlControlsResources`.
fn app_resource(key: &str) -> Option<IInspectable> {
    let resources = Application::Current()
        .and_then(|app| app.Resources())
        .ok()?;
    PropertyValue::CreateString(&HSTRING::from(key))
        .and_then(|key| resources.Lookup(&key))
        .ok()
}

// ----------------------------------------------------------------- Button

struct ButtonInner {
    native: XamlButton,
    label: TextBlock,
    /// The token of the registered event. Removed when reattaching.
    token: RefCell<Option<i64>>,
}

/// A push button (Button).
#[derive(Clone)]
pub struct Button(Rc<ButtonInner>);
impl_widget!(Button, native);

impl Button {
    pub(crate) fn new(text: &str) -> Result<Self> {
        let native = XamlButton::new().map_err(|e| to_error("Button の生成", e))?;
        let label = TextBlock::new().map_err(|e| to_error("Button ラベルの生成", e))?;
        label
            .SetText(&HSTRING::from(text))
            .map_err(|e| to_error("Button ラベルの設定", e))?;
        native
            .SetContent(&label)
            .map_err(|e| to_error("Button への内容設定", e))?;
        Ok(Self(Rc::new(ButtonInner {
            native,
            label,
            token: RefCell::new(None),
        })))
    }

    pub fn set_text(&self, text: &str) {
        let _ = self.0.label.SetText(&HSTRING::from(text));
    }

    pub fn set_enabled(&self, enabled: bool) {
        let _ = self.0.native.SetIsEnabled(enabled);
    }

    /// Called on click. Setting it again removes the previous one.
    pub fn on_click(&self, f: impl FnMut() + 'static) {
        if let Some(token) = self.0.token.borrow_mut().take() {
            let _ = self.0.native.RemoveClick(token);
        }
        let f = UiThreadCell::new(f);
        let handler = RoutedEventHandler::new(move |_sender, _args| {
            f.with_mut(|f| f());
            Ok(())
        });
        if let Ok(token) = self.0.native.Click(&handler) {
            *self.0.token.borrow_mut() = Some(token);
        }
    }
}

// --------------------------------------------------------------- Checkbox

struct CheckboxInner {
    native: XamlCheckBox,
    tokens: RefCell<Vec<(bool, i64)>>,
    /// Set during `set_checked`. `Checked` / `Unchecked` also fire when `IsChecked`
    /// is rewritten from code.
    silent: Cell<bool>,
}

/// A check box (CheckBox).
#[derive(Clone)]
pub struct Checkbox(Rc<CheckboxInner>);
impl_widget!(Checkbox, native);

impl Checkbox {
    pub(crate) fn new(label: &str) -> Result<Self> {
        let native = XamlCheckBox::new().map_err(|e| to_error("CheckBox の生成", e))?;
        let text = TextBlock::new().map_err(|e| to_error("CheckBox ラベルの生成", e))?;
        text.SetText(&HSTRING::from(label))
            .map_err(|e| to_error("CheckBox ラベルの設定", e))?;
        native
            .SetContent(&text)
            .map_err(|e| to_error("CheckBox への内容設定", e))?;
        native
            .SetIsChecked(&bool_ref(false)?)
            .map_err(|e| to_error("CheckBox の初期化", e))?;
        Ok(Self(Rc::new(CheckboxInner {
            native,
            tokens: RefCell::new(Vec::new()),
            silent: Cell::new(false),
        })))
    }

    pub fn is_checked(&self) -> bool {
        self.0
            .native
            .IsChecked()
            .and_then(|r| r.Value())
            .unwrap_or(false)
    }

    /// Changes the state from code. `on_toggle` is not called.
    pub fn set_checked(&self, checked: bool) {
        self.0.silent.set(true);
        let _ = self.0.native.SetIsChecked(bool_ref(checked).ok().as_ref());
        self.0.silent.set(false);
    }

    pub fn set_enabled(&self, enabled: bool) {
        let _ = self.0.native.SetIsEnabled(enabled);
    }

    /// Called with the new value every time the user toggles it.
    pub fn on_toggle(&self, f: impl FnMut(bool) + 'static) {
        for (checked, token) in self.0.tokens.borrow_mut().drain(..) {
            let _ = if checked {
                self.0.native.RemoveChecked(token)
            } else {
                self.0.native.RemoveUnchecked(token)
            };
        }
        let f = std::sync::Arc::new(UiThreadCell::new(f));
        let target = std::sync::Arc::new(UiThreadCell::new(Rc::downgrade(&self.0)));
        let mut tokens = Vec::new();
        for checked in [true, false] {
            let f = f.clone();
            let target = target.clone();
            let handler = RoutedEventHandler::new(move |_sender, _args| {
                let silent = target
                    .try_with_mut(|weak| weak.upgrade().is_some_and(|inner| inner.silent.get()))
                    .unwrap_or(false);
                if !silent {
                    f.with_mut(|f| f(checked));
                }
                Ok(())
            });
            let registered = if checked {
                self.0.native.Checked(&handler)
            } else {
                self.0.native.Unchecked(&handler)
            };
            if let Ok(token) = registered {
                tokens.push((checked, token));
            }
        }
        *self.0.tokens.borrow_mut() = tokens;
    }
}

// -------------------------------------------------------------- TextInput

struct TextInputInner {
    native: TextBox,
    token: RefCell<Option<i64>>,
    on_submit: TextHandler,
    /// The `on_focus_change` handler.
    focus: crate::focus::Focus,
}

/// Single-line text input (TextBox). Windows handles the IME.
#[derive(Clone)]
pub struct TextInput(Rc<TextInputInner>);
crate::keys::impl_key_down!(TextInput);
crate::focus::impl_focus_change!(TextInput);
impl_widget!(TextInput, native);

impl TextInput {
    pub(crate) fn new(text: &str) -> Result<Self> {
        let native = TextBox::new().map_err(|e| to_error("TextBox の生成", e))?;
        native
            .SetText(&HSTRING::from(text))
            .map_err(|e| to_error("TextBox への設定", e))?;
        let inner = Rc::new(TextInputInner {
            native,
            token: RefCell::new(None),
            on_submit: TextHandler::new(),
            focus: Default::default(),
        });
        let element: UIElement = inner
            .native
            .cast()
            .map_err(|e| to_error("TextBox の要素の取得", e))?;
        let weak = Rc::downgrade(&inner);
        crate::keys::on_enter(&element, move || {
            let Some(inner) = weak.upgrade() else {
                return;
            };
            // 読み取り専用の間は書き換えられないので、確定もしない。
            if inner.native.IsReadOnly().unwrap_or(false) {
                return;
            }
            let text = inner.native.Text().unwrap_or_default().to_string();
            inner.on_submit.emit(&text);
        })
        .map_err(|e| to_error("TextBox の Enter の購読", e))?;
        Ok(Self(inner))
    }

    pub fn text(&self) -> String {
        self.0
            .native
            .Text()
            .map(|s| s.to_string())
            .unwrap_or_default()
    }

    pub fn set_text(&self, text: &str) {
        let _ = self.0.native.SetText(&HSTRING::from(text));
    }

    pub fn set_placeholder(&self, text: &str) {
        let _ = self.0.native.SetPlaceholderText(&HSTRING::from(text));
    }

    pub fn set_enabled(&self, enabled: bool) {
        let _ = self.0.native.SetIsEnabled(enabled);
    }

    /// Called with the current text every time a character is typed.
    pub fn on_change(&self, f: impl FnMut(&str) + 'static) {
        use naui_winui3::Microsoft::UI::Xaml::Controls::TextChangedEventHandler;
        if let Some(token) = self.0.token.borrow_mut().take() {
            let _ = self.0.native.RemoveTextChanged(token);
        }
        let state = UiThreadCell::new((self.0.native.clone(), f));
        let handler = TextChangedEventHandler::new(move |_sender, _args| {
            state.with_mut(|(native, f)| {
                let text = native.Text().unwrap_or_default().to_string();
                f(&text);
            });
            Ok(())
        });
        if let Ok(token) = self.0.native.TextChanged(&handler) {
            *self.0.token.borrow_mut() = Some(token);
        }
    }

    /// Called with the current text when the user confirms with Enter.
    ///
    /// Not called for an Enter pressed while an IME is composing (which confirms the
    /// conversion). Not called while read-only either.
    pub fn on_submit(&self, f: impl FnMut(&str) + 'static) {
        self.0.on_submit.set(f);
    }

    /// Whether it is read-only. Editable by default.
    ///
    /// While read-only, the text can still be selected and copied, and it still takes
    /// focus (unlike `set_enabled(false)`, it is not dimmed). Use it for fields that
    /// show logs or generated output.
    pub fn set_read_only(&self, read_only: bool) {
        let _ = self.0.native.SetIsReadOnly(read_only);
    }

    pub fn is_read_only(&self) -> bool {
        self.0.native.IsReadOnly().unwrap_or(false)
    }
}

// ----------------------------------------------------------- PasswordInput

struct PasswordInputInner {
    native: PasswordBox,
    token: RefCell<Option<i64>>,
    on_submit: TextHandler,
    /// The `on_focus_change` handler.
    focus: crate::focus::Focus,
}

/// Password input (PasswordBox).
///
/// The API is the same as [`TextInput`]; the only difference is that **typed
/// characters are masked**. WinUI 3 has a button that temporarily reveals the text
/// (`IsPasswordRevealButtonEnabled`), but it is not shown because it is not part of
/// what all four platforms share.
#[derive(Clone)]
pub struct PasswordInput(Rc<PasswordInputInner>);
crate::keys::impl_key_down!(PasswordInput);
crate::focus::impl_focus_change!(PasswordInput);
impl_widget!(PasswordInput, native);

impl PasswordInput {
    pub(crate) fn new() -> Result<Self> {
        let native = PasswordBox::new().map_err(|e| to_error("PasswordBox の生成", e))?;
        let inner = Rc::new(PasswordInputInner {
            native,
            token: RefCell::new(None),
            on_submit: TextHandler::new(),
            focus: Default::default(),
        });
        let element: UIElement = inner
            .native
            .cast()
            .map_err(|e| to_error("PasswordBox の要素の取得", e))?;
        let weak = Rc::downgrade(&inner);
        crate::keys::on_enter(&element, move || {
            if let Some(inner) = weak.upgrade() {
                let text = inner.native.Password().unwrap_or_default().to_string();
                inner.on_submit.emit(&text);
            }
        })
        .map_err(|e| to_error("PasswordBox の Enter の購読", e))?;
        Ok(Self(inner))
    }

    /// The text currently entered.
    pub fn text(&self) -> String {
        self.0
            .native
            .Password()
            .map(|s| s.to_string())
            .unwrap_or_default()
    }

    /// Replaces the text.
    ///
    /// WinUI fires the native `PasswordChanged`, so **on Windows only, `on_change` is
    /// called too** (the same as [`TextInput::set_text`]).
    pub fn set_text(&self, text: &str) {
        let _ = self.0.native.SetPassword(&HSTRING::from(text));
    }

    pub fn set_placeholder(&self, text: &str) {
        let _ = self.0.native.SetPlaceholderText(&HSTRING::from(text));
    }

    pub fn set_enabled(&self, enabled: bool) {
        let _ = self.0.native.SetIsEnabled(enabled);
    }

    /// Called with the current text every time a character is typed.
    pub fn on_change(&self, f: impl FnMut(&str) + 'static) {
        if let Some(token) = self.0.token.borrow_mut().take() {
            let _ = self.0.native.RemovePasswordChanged(token);
        }
        let state = UiThreadCell::new((self.0.native.clone(), f));
        let handler = RoutedEventHandler::new(move |_sender, _args| {
            state.with_mut(|(native, f)| {
                let text = native.Password().unwrap_or_default().to_string();
                f(&text);
            });
            Ok(())
        });
        if let Ok(token) = self.0.native.PasswordChanged(&handler) {
            *self.0.token.borrow_mut() = Some(token);
        }
    }

    /// Called with the current text when the user confirms with Enter.
    pub fn on_submit(&self, f: impl FnMut(&str) + 'static) {
        self.0.on_submit.set(f);
    }
}

// --------------------------------------------------------------- TextArea

struct TextAreaInner {
    native: TextBox,
    token: RefCell<Option<i64>>,
    /// The `on_focus_change` handler.
    focus: crate::focus::Focus,
}

/// Multi-line text input (a TextBox that accepts newlines). Windows handles the IME.
#[derive(Clone)]
pub struct TextArea(Rc<TextAreaInner>);
crate::keys::impl_key_down!(TextArea);
crate::focus::impl_focus_change!(TextArea);
impl_widget!(TextArea, native);

impl TextArea {
    pub(crate) fn new(text: &str) -> Result<Self> {
        let native = TextBox::new().map_err(|e| to_error("TextBox の生成", e))?;
        // 1 行の TextBox との違いはこの 2 つ。Enter が改行になり、
        // 長い行は折り返す。
        native
            .SetAcceptsReturn(true)
            .map_err(|e| to_error("TextBox の複数行化", e))?;
        native
            .SetTextWrapping(TextWrapping::Wrap)
            .map_err(|e| to_error("TextBox の折り返し設定", e))?;
        // はみ出した分は縦にスクロールさせる。
        ScrollViewer::SetVerticalScrollBarVisibility2(&native, ScrollBarVisibility::Auto)
            .map_err(|e| to_error("TextBox のスクロール設定", e))?;
        native
            .SetText(&HSTRING::from(text))
            .map_err(|e| to_error("TextBox への設定", e))?;
        Ok(Self(Rc::new(TextAreaInner {
            native,
            token: RefCell::new(None),
            focus: Default::default(),
        })))
    }

    /// The current text. Newlines are included as `\n` (the same as the other
    /// platforms).
    pub fn text(&self) -> String {
        self.0
            .native
            .Text()
            .map(|s| unix_newlines(&s.to_string()))
            .unwrap_or_default()
    }

    pub fn set_text(&self, text: &str) {
        let _ = self.0.native.SetText(&HSTRING::from(text));
    }

    /// Text shown faintly when nothing has been entered.
    pub fn set_placeholder(&self, text: &str) {
        let _ = self.0.native.SetPlaceholderText(&HSTRING::from(text));
    }

    pub fn set_enabled(&self, enabled: bool) {
        let _ = self.0.native.SetIsEnabled(enabled);
    }

    /// Called with the current text every time a character is typed. Also called for
    /// newlines.
    pub fn on_change(&self, f: impl FnMut(&str) + 'static) {
        use naui_winui3::Microsoft::UI::Xaml::Controls::TextChangedEventHandler;
        if let Some(token) = self.0.token.borrow_mut().take() {
            let _ = self.0.native.RemoveTextChanged(token);
        }
        let state = UiThreadCell::new((self.0.native.clone(), f));
        let handler = TextChangedEventHandler::new(move |_sender, _args| {
            state.with_mut(|(native, f)| {
                let text = unix_newlines(&native.Text().unwrap_or_default().to_string());
                f(&text);
            });
            Ok(())
        });
        if let Ok(token) = self.0.native.TextChanged(&handler) {
            *self.0.token.borrow_mut() = Some(token);
        }
    }

    /// Whether it is read-only. Editable by default.
    ///
    /// While read-only, the text can still be selected and copied, and it still takes
    /// focus (unlike `set_enabled(false)`, it is not dimmed). Use it for fields that
    /// show logs or generated output.
    pub fn set_read_only(&self, read_only: bool) {
        let _ = self.0.native.SetIsReadOnly(read_only);
    }

    pub fn is_read_only(&self) -> bool {
        self.0.native.IsReadOnly().unwrap_or(false)
    }
}

/// WinUI's `TextBox` stores newlines as `\r`. They are converted to `\n` to match
/// the other platforms.
fn unix_newlines(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

// ----------------------------------------------------------------- Slider

const SLIDER_XAML: &str = r#"<Slider
    xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
    Style="{StaticResource DefaultSliderStyle}"/>"#;

struct SliderInner {
    native: XamlSlider,
    min: f64,
    max: f64,
}

/// A slider (Slider).
#[derive(Clone)]
pub struct Slider(Rc<SliderInner>);
impl_widget!(Slider, native);

impl Slider {
    pub(crate) fn new(min: f64, max: f64) -> Result<Self> {
        // ABI のコンストラクタから直接生成するのではなく XAML で作り、
        // XamlControlsResources の Fluent テンプレートを明示的に適用する。
        let native = XamlReader::Load(&HSTRING::from(SLIDER_XAML))
            .and_then(|element| element.cast::<XamlSlider>())
            .map_err(|e| to_error("Slider の生成", e))?;
        native
            .SetMinimum(min)
            .map_err(|e| to_error("Slider の範囲設定", e))?;
        native
            .SetMaximum(max)
            .map_err(|e| to_error("Slider の範囲設定", e))?;
        native
            .SetStepFrequency((max - min) / 1000.0)
            .map_err(|e| to_error("Slider の刻み設定", e))?;
        Ok(Self(Rc::new(SliderInner { native, min, max })))
    }

    pub fn value(&self) -> f64 {
        self.0.native.Value().unwrap_or(self.0.min)
    }

    pub fn set_value(&self, value: f64) {
        let _ = self.0.native.SetValue(value.clamp(self.0.min, self.0.max));
    }

    pub fn set_enabled(&self, enabled: bool) {
        let _ = self.0.native.SetIsEnabled(enabled);
    }

    /// Called with the value every time the knob moves.
    pub fn on_change(&self, f: impl FnMut(f64) + 'static) {
        use naui_winui3::Microsoft::UI::Xaml::Controls::Primitives::RangeBaseValueChangedEventHandler;
        let state = UiThreadCell::new((self.0.native.clone(), f));
        let handler = RangeBaseValueChangedEventHandler::new(move |_sender, _args| {
            state.with_mut(|(native, f)| f(native.Value().unwrap_or_default()));
            Ok(())
        });
        let _ = self.0.native.ValueChanged(&handler);
    }
}

// ------------------------------------------------------------ ProgressBar

struct ProgressInner {
    native: UIElement,
    fill: FrameworkElement,
    value: Cell<f64>,
    track_width: Cell<f64>,
    /// The timer that moves the foreground band back and forth while indeterminate.
    animation: RefCell<Option<DispatcherQueueTimer>>,
    /// How far along the back-and-forth is (0.0..2.0; past 1.0 it is going back).
    phase: Cell<f64>,
}

impl Drop for ProgressInner {
    fn drop(&mut self) {
        if let Some(timer) = self.animation.borrow_mut().take() {
            let _ = timer.Stop();
        }
    }
}

/// The length of the indeterminate band (as a fraction of the bar's width).
const INDETERMINATE_SEGMENT: f64 = 0.3;
/// The interval for moving the indeterminate band, and how far it moves each time
/// (about 2 seconds per round trip).
const INDETERMINATE_TICK_MILLIS: i64 = 33;
const INDETERMINATE_STEP: f64 = 0.033;

/// A progress bar (ProgressBar).
#[derive(Clone)]
pub struct ProgressBar(Rc<ProgressInner>);
impl_widget!(ProgressBar, native);

impl ProgressBar {
    pub(crate) fn new() -> Result<Self> {
        // Windows App SDK 2.3.1 の未パッケージ起動では、ProgressBar の
        // 既定テンプレートが適用される瞬間にランタイムが fail-fast する。
        // 代替の Border でも公式テンプレートと同じテーマ資源と寸法を使う。
        let grid = XamlReader::Load(&HSTRING::from(
            r##"<Grid xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
                HorizontalAlignment="Stretch" Height="3">
                <Border Height="1" VerticalAlignment="Center"
                    Background="{ThemeResource ProgressBarBackground}" CornerRadius="0.5"/>
                <Border Width="0" HorizontalAlignment="Left"
                    Background="{ThemeResource ProgressBarForeground}" CornerRadius="1.5"/>
            </Grid>"##,
        ))
        .map_err(|e| to_error("ProgressBar の生成", e))?
        .cast::<Grid>()
        .map_err(|e| to_error("ProgressBar の要素化", e))?;
        let fill = grid
            .Children()
            .and_then(|children| children.GetAt(1))
            .and_then(|element| element.cast::<FrameworkElement>())
            .map_err(|e| to_error("ProgressBar の前景要素取得", e))?;
        let native = grid
            .cast::<UIElement>()
            .map_err(|e| to_error("ProgressBar の要素化", e))?;
        let this = Self(Rc::new(ProgressInner {
            native,
            fill,
            value: Cell::new(0.0),
            // レイアウト前の一時値。LayoutUpdatedで実幅に置き換える。
            track_width: Cell::new(240.0),
            animation: RefCell::new(None),
            phase: Cell::new(0.0),
        }));
        let weak = Rc::downgrade(&this.0);
        let state = UiThreadCell::new(weak);
        let element = this
            .0
            .native
            .cast::<FrameworkElement>()
            .map_err(|e| to_error("ProgressBar のレイアウト要素化", e))?;
        let handler = EventHandler::<IInspectable>::new(move |_sender, _args| {
            state.with_mut(|weak| {
                let Some(inner) = weak.upgrade() else {
                    return Ok(());
                };
                let Ok(element) = inner.native.cast::<FrameworkElement>() else {
                    return Ok(());
                };
                let Ok(width) = element.ActualWidth() else {
                    return Ok(());
                };
                if width > 0.0 && (width - inner.track_width.get()).abs() > f64::EPSILON {
                    inner.track_width.set(width);
                    if inner.animation.borrow().is_none() {
                        let _ = inner.fill.SetWidth(width * inner.value.get());
                    }
                }
                Ok(())
            })
        });
        let _ = element.LayoutUpdated(&handler);
        Ok(this)
    }

    /// 0.0..=1.0.
    pub fn set_value(&self, value: f64) {
        let value = value.clamp(0.0, 1.0);
        self.0.value.set(value);
        if !self.is_indeterminate() {
            let _ = self.0.fill.SetWidth(self.0.track_width.get() * value);
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
    /// WinUI's `ProgressBar` crashes the moment its template is applied when launched
    /// unpackaged, so naui uses a band built from the same theme resources. The
    /// indeterminate display is also expressed by moving that foreground band back and
    /// forth at a fixed interval.
    pub fn set_indeterminate(&self, indeterminate: bool) {
        if self.is_indeterminate() == indeterminate {
            return;
        }
        if !indeterminate {
            if let Some(timer) = self.0.animation.borrow_mut().take() {
                let _ = timer.Stop();
            }
            let _ = self.0.fill.SetMargin(Thickness::default());
            let _ = self
                .0
                .fill
                .SetWidth(self.0.track_width.get() * self.0.value.get());
            return;
        }
        let Ok(timer) =
            DispatcherQueue::GetForCurrentThread().and_then(|queue| queue.CreateTimer())
        else {
            return;
        };
        let state = UiThreadCell::new(Rc::downgrade(&self.0));
        let handler = TypedEventHandler::<DispatcherQueueTimer, IInspectable>::new(move |_, _| {
            let _ = state.try_with_mut(|weak| {
                if let Some(inner) = weak.upgrade() {
                    inner.step_indeterminate();
                }
            });
            Ok(())
        });
        let interval = windows::Foundation::TimeSpan {
            Duration: INDETERMINATE_TICK_MILLIS * 10_000,
        };
        if timer.SetInterval(interval).is_err()
            || timer.SetIsRepeating(true).is_err()
            || timer.Tick(&handler).is_err()
            || timer.Start().is_err()
        {
            return;
        }
        self.0.phase.set(0.0);
        *self.0.animation.borrow_mut() = Some(timer);
        self.0.step_indeterminate();
    }

    pub fn is_indeterminate(&self) -> bool {
        self.0.animation.borrow().is_some()
    }
}

impl ProgressInner {
    /// Advances the foreground band one step. Turns around at the ends.
    fn step_indeterminate(&self) {
        let phase = (self.phase.get() + INDETERMINATE_STEP) % 2.0;
        self.phase.set(phase);
        let along = if phase <= 1.0 { phase } else { 2.0 - phase };
        let track = self.track_width.get();
        let segment = track * INDETERMINATE_SEGMENT;
        let _ = self.fill.SetWidth(segment);
        let _ = self.fill.SetMargin(Thickness {
            Left: (track - segment) * along,
            ..Thickness::default()
        });
    }
}

// ---------------------------------------------------------------- Spinner

/// The size of `ProgressRing` in WinUI's default style (`DefaultProgressRingStyle`).
const SPINNER_SIZE: f64 = 32.0;

struct SpinnerInner {
    native: ProgressRing,
    auto_width: Cell<bool>,
    auto_height: Cell<bool>,
}

/// A spinning indicator (ProgressRing).
#[derive(Clone)]
pub struct Spinner(Rc<SpinnerInner>);
impl_widget!(Spinner, native);

impl Spinner {
    pub(crate) fn new() -> Result<Self> {
        let native = ProgressRing::new().map_err(|e| to_error("ProgressRing の生成", e))?;
        native
            .SetIsActive(true)
            .map_err(|e| to_error("ProgressRing の開始", e))?;
        let this = Self(Rc::new(SpinnerInner {
            native,
            auto_width: Cell::new(true),
            auto_height: Cell::new(true),
        }));
        // Auto-sized widgets start at the left of a Grid cell, as on other backends.
        this.set_sizing(naui_core::Sizing::new());
        this.0.keep_default_size();
        // ProgressRing の中身は自然な大きさを持たない。Auto の軸には既定の
        // 大きさを与え、親が Fill で広げる軸では固定の大きさを外す。
        let target = Rc::downgrade(&this.0);
        crate::layout::on_sizing_change(&this.native_element(), move || {
            if let Some(inner) = target.upgrade() {
                inner.record_auto_axes();
                inner.keep_default_size();
            }
        });
        let target = Rc::downgrade(&this.0);
        crate::layout::on_parent_layout_change(&this.native_element(), move || {
            if let Some(inner) = target.upgrade() {
                inner.keep_default_size();
            }
        });
        Ok(this)
    }

    /// Whether it spins. Spins from creation.
    ///
    /// While stopped it is not drawn but keeps its space (`IsActive` is off). Use
    /// `set_visible(false)` to give the space up as well.
    pub fn set_spinning(&self, spinning: bool) {
        let _ = self.0.native.SetIsActive(spinning);
    }

    pub fn is_spinning(&self) -> bool {
        self.0.native.IsActive().unwrap_or(false)
    }
}

impl SpinnerInner {
    /// Records which axes are Auto before the default size is applied.
    fn record_auto_axes(&self) {
        let Ok(element) = self.native.cast::<FrameworkElement>() else {
            return;
        };
        self.auto_width.set(
            element.Width().is_ok_and(f64::is_nan) && !crate::layout::wants_fill(&element, true),
        );
        self.auto_height.set(
            element.Height().is_ok_and(f64::is_nan) && !crate::layout::wants_fill(&element, false),
        );
    }

    /// Gives Auto axes the default size unless their parent stretches them.
    fn keep_default_size(&self) {
        let Ok(element) = self.native.cast::<FrameworkElement>() else {
            return;
        };
        if self.auto_width.get() {
            let width = if crate::layout::parent_stretches_auto(&element, true) {
                f64::NAN
            } else {
                SPINNER_SIZE
            };
            let _ = element.SetWidth(width);
        }
        if self.auto_height.get() {
            let height = if crate::layout::parent_stretches_auto(&element, false) {
                f64::NAN
            } else {
                SPINNER_SIZE
            };
            let _ = element.SetHeight(height);
        }
    }
}

// ------------------------------------------------------------------ Stack

struct StackInner {
    native: Grid,
    vertical: Cell<bool>,
    /// The default alignment of children on the cross axis. `Fill` children follow their
    /// own setting instead.
    align: Cell<Align>,
    children: RefCell<Vec<Box<dyn Widget>>>,
}

impl Drop for StackInner {
    fn drop(&mut self) {
        crate::layout::unregister_stack(&self.native);
        if let Ok(children) = self.children.try_borrow() {
            for child in children.iter() {
                crate::layout::clear_parent_layout(&child.native_element());
            }
        }
    }
}

/// A container that lays out its children vertically / horizontally.
///
/// Built on a `Grid` with one row (column) per child rather than on `StackPanel`,
/// which never hands out leftover space: `Fill` children and `Spacer`s get `Star`
/// tracks (see `layout::arrange_stack`).
#[derive(Clone)]
pub struct Stack(Rc<StackInner>);
impl_widget!(Stack, native);

impl Stack {
    pub(crate) fn new(orientation: Orientation) -> Result<Self> {
        let native = Grid::new().map_err(|e| to_error("Stack の Grid の生成", e))?;
        crate::layout::register_stack(&native);
        Ok(Self(Rc::new(StackInner {
            native,
            vertical: Cell::new(orientation.is_vertical()),
            align: Cell::new(Align::default()),
            children: RefCell::new(Vec::new()),
        })))
    }

    /// Switches between laying out vertically and horizontally.
    ///
    /// The children stay in the `Grid`. Each one is told the new direction (giving
    /// back the alignment the previous cross axis overrode) and gets a track on the
    /// new main axis; `arrange_stack` empties the definitions of the other axis.
    pub fn set_orientation(&self, orientation: Orientation) {
        let vertical = orientation.is_vertical();
        if self.0.vertical.replace(vertical) == vertical {
            return;
        }
        for child in self.0.children.borrow().iter() {
            self.adopt(&child.native_element());
        }
        self.arrange();
    }

    pub fn orientation(&self) -> Orientation {
        if self.0.vertical.get() {
            Orientation::Vertical
        } else {
            Orientation::Horizontal
        }
    }

    pub fn set_spacing(&self, spacing: f64) {
        // 交差軸は 1 本なので、両方に入れても主軸の間隔にしか効かない。
        let _ = self.0.native.SetRowSpacing(spacing);
        let _ = self.0.native.SetColumnSpacing(spacing);
    }

    pub fn set_padding(&self, padding: Padding) {
        let _ = self.0.native.SetPadding(Thickness {
            Left: padding.left,
            Top: padding.top,
            Right: padding.right,
            Bottom: padding.bottom,
        });
    }

    pub fn set_align(&self, align: Align) {
        self.0.align.set(align);
        for child in self.0.children.borrow().iter() {
            self.adopt(&child.native_element());
        }
    }

    /// Records this Stack as the child's parent and applies its alignment.
    fn adopt(&self, element: &UIElement) {
        crate::layout::set_stack_parent(
            element,
            &self.0.native,
            self.0.align.get(),
            self.0.vertical.get(),
        );
    }

    fn arrange(&self) {
        crate::layout::arrange_stack(&self.0.native, self.0.vertical.get());
    }

    /// Appends a child at the end.
    pub fn append(&self, child: &dyn Widget) {
        let element = child.native_element();
        let appended = self.0.native.Children().and_then(|c| c.Append(&element));
        if appended.is_ok() {
            self.adopt(&element);
            self.0.children.borrow_mut().push(child.boxed_clone());
            self.arrange();
        }
    }

    /// Inserts a child at the given position. If `index` is at least the current
    /// count, it is appended at the end.
    pub fn insert(&self, index: usize, child: &dyn Widget) {
        let mut children = self.0.children.borrow_mut();
        let index = index.min(children.len());
        let element = child.native_element();
        let inserted = self
            .0
            .native
            .Children()
            .and_then(|c| c.InsertAt(index as u32, &element));
        if inserted.is_ok() {
            self.adopt(&element);
            children.insert(index, child.boxed_clone());
            drop(children);
            self.arrange();
        }
    }

    /// Removes the child at the given position. Does nothing if out of range.
    pub fn remove(&self, index: usize) {
        let mut children = self.0.children.borrow_mut();
        if index >= children.len() {
            return;
        }
        let removed = self
            .0
            .native
            .Children()
            .and_then(|c| c.RemoveAt(index as u32));
        if removed.is_ok() {
            crate::layout::release_stack_child(&children[index].native_element());
            children.remove(index);
            drop(children);
            self.arrange();
        }
    }

    /// Removes every child.
    pub fn clear(&self) {
        let elements = self
            .0
            .children
            .borrow()
            .iter()
            .map(|child| child.native_element())
            .collect::<Vec<_>>();
        let Ok(children) = self.0.native.Children() else {
            return;
        };
        if children.Clear().is_err() {
            return;
        }
        for element in elements {
            crate::layout::release_stack_child(&element);
        }
        self.0.children.borrow_mut().clear();
        self.arrange();
    }

    pub fn len(&self) -> usize {
        self.0.children.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Wraps a `bool` in WinRT's `IReference<bool>`.
pub(crate) fn bool_ref(value: bool) -> Result<windows::Foundation::IReference<bool>> {
    use windows_core::Interface;
    windows::Foundation::PropertyValue::CreateBoolean(value)
        .and_then(|v| v.cast())
        .map_err(|e| to_error("bool の boxing", e))
}
