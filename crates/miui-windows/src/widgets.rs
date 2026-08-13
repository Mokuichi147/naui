//! WinUI 3 (Fluent 2) の実コントロールを包むハンドル群。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use miui_core::{Align, Orientation, Padding, Result};
use windows_core::{Interface, HSTRING};
use winui3::Microsoft::UI::Xaml::Controls::{
    Button as XamlButton, CheckBox as XamlCheckBox, Grid, Orientation as XamlOrientation,
    Slider as XamlSlider, StackPanel, TextBlock, TextBox,
};
use winui3::Microsoft::UI::Xaml::Markup::XamlReader;
use winui3::Microsoft::UI::Xaml::{
    FrameworkElement, HorizontalAlignment, RoutedEventHandler, Thickness, UIElement,
    VerticalAlignment,
};

use crate::to_error;
use crate::ui_thread::UiThreadCell;

/// miui のウィジェットが実装する共通インタフェース。
pub trait Widget: 'static {
    /// 対応する WinUI 3 の要素。バックエンド固有の脱出口として公開している。
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
    };
}

// ------------------------------------------------------------------ Label

struct LabelInner {
    native: TextBlock,
}

/// テキスト表示 (TextBlock)。
#[derive(Clone)]
pub struct Label(Rc<LabelInner>);
impl_widget!(Label, native);

impl Label {
    pub(crate) fn new(text: &str) -> Result<Self> {
        let native = TextBlock::new().map_err(|e| to_error("TextBlock の生成", e))?;
        native
            .SetText(&HSTRING::from(text))
            .map_err(|e| to_error("TextBlock への設定", e))?;
        Ok(Self(Rc::new(LabelInner { native })))
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
}

// ----------------------------------------------------------------- Button

struct ButtonInner {
    native: XamlButton,
    label: TextBlock,
    /// 登録したイベントのトークン。付け替え時に外す。
    token: RefCell<Option<i64>>,
}

/// 押しボタン (Button)。
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

    /// クリックされたときに呼ばれる。設定し直すと以前のものは外れる。
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
}

/// チェックボックス (CheckBox)。
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
        })))
    }

    pub fn is_checked(&self) -> bool {
        self.0
            .native
            .IsChecked()
            .and_then(|r| r.Value())
            .unwrap_or(false)
    }

    pub fn set_checked(&self, checked: bool) {
        let _ = self.0.native.SetIsChecked(bool_ref(checked).ok().as_ref());
    }

    pub fn set_enabled(&self, enabled: bool) {
        let _ = self.0.native.SetIsEnabled(enabled);
    }

    /// 状態が変わったときに、変更後の値で呼ばれる。
    pub fn on_toggle(&self, f: impl FnMut(bool) + 'static) {
        for (checked, token) in self.0.tokens.borrow_mut().drain(..) {
            let _ = if checked {
                self.0.native.RemoveChecked(token)
            } else {
                self.0.native.RemoveUnchecked(token)
            };
        }
        let f = std::sync::Arc::new(UiThreadCell::new(f));
        let mut tokens = Vec::new();
        for checked in [true, false] {
            let f = f.clone();
            let handler = RoutedEventHandler::new(move |_sender, _args| {
                f.with_mut(|f| f(checked));
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
}

/// 1 行テキスト入力 (TextBox)。IME は Windows が処理する。
#[derive(Clone)]
pub struct TextInput(Rc<TextInputInner>);
impl_widget!(TextInput, native);

impl TextInput {
    pub(crate) fn new(text: &str) -> Result<Self> {
        let native = TextBox::new().map_err(|e| to_error("TextBox の生成", e))?;
        native
            .SetText(&HSTRING::from(text))
            .map_err(|e| to_error("TextBox への設定", e))?;
        Ok(Self(Rc::new(TextInputInner {
            native,
            token: RefCell::new(None),
        })))
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

    /// 1 文字入力するたびに、その時点の文字列で呼ばれる。
    pub fn on_change(&self, f: impl FnMut(&str) + 'static) {
        use winui3::Microsoft::UI::Xaml::Controls::TextChangedEventHandler;
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
}

// ----------------------------------------------------------------- Slider

struct SliderInner {
    native: XamlSlider,
    min: f64,
    max: f64,
}

/// スライダー (Slider)。
#[derive(Clone)]
pub struct Slider(Rc<SliderInner>);
impl_widget!(Slider, native);

impl Slider {
    pub(crate) fn new(min: f64, max: f64) -> Result<Self> {
        let native = XamlSlider::new().map_err(|e| to_error("Slider の生成", e))?;
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

    /// つまみが動くたびに、その値で呼ばれる。
    pub fn on_change(&self, f: impl FnMut(f64) + 'static) {
        use winui3::Microsoft::UI::Xaml::Controls::Primitives::RangeBaseValueChangedEventHandler;
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
    max_width: f64,
}

/// 進捗バー (ProgressBar)。
#[derive(Clone)]
pub struct ProgressBar(Rc<ProgressInner>);
impl_widget!(ProgressBar, native);

impl ProgressBar {
    pub(crate) fn new() -> Result<Self> {
        // Windows App SDK 2.3.1 の未パッケージ起動では、ProgressBar の
        // 既定テンプレートが適用される瞬間にランタイムが fail-fast する。
        // 同じ WinUI XAML の Border を使えば、見た目を保ったまま回避できる。
        const MAX_WIDTH: f64 = 96.0;
        let grid = XamlReader::Load(&HSTRING::from(
            r##"<Grid xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
                Width="96" Height="6">
                <Border Background="#E5E5E5" CornerRadius="3"/>
                <Border Width="0" HorizontalAlignment="Left"
                    Background="#0078D4" CornerRadius="3"/>
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
        Ok(Self(Rc::new(ProgressInner {
            native,
            fill,
            value: Cell::new(0.0),
            max_width: MAX_WIDTH,
        })))
    }

    /// 0.0..=1.0。
    pub fn set_value(&self, value: f64) {
        let value = value.clamp(0.0, 1.0);
        self.0.value.set(value);
        let _ = self.0.fill.SetWidth(self.0.max_width * value);
    }

    pub fn value(&self) -> f64 {
        self.0.value.get()
    }
}

// ------------------------------------------------------------------ Stack

struct StackInner {
    native: StackPanel,
    children: RefCell<Vec<Box<dyn Widget>>>,
}

/// 縦 / 横に子を並べるコンテナ (StackPanel)。
#[derive(Clone)]
pub struct Stack(Rc<StackInner>);
impl_widget!(Stack, native);

impl Stack {
    pub(crate) fn new(orientation: Orientation) -> Result<Self> {
        let native = StackPanel::new().map_err(|e| to_error("StackPanel の生成", e))?;
        native
            .SetOrientation(if orientation.is_vertical() {
                XamlOrientation::Vertical
            } else {
                XamlOrientation::Horizontal
            })
            .map_err(|e| to_error("StackPanel の向き設定", e))?;
        Ok(Self(Rc::new(StackInner {
            native,
            children: RefCell::new(Vec::new()),
        })))
    }

    pub fn set_spacing(&self, spacing: f64) {
        let _ = self.0.native.SetSpacing(spacing);
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
        let vertical = self
            .0
            .native
            .Orientation()
            .map(|o| o == XamlOrientation::Vertical)
            .unwrap_or(true);
        if vertical {
            let value = match align {
                Align::Start => HorizontalAlignment::Left,
                Align::Center => HorizontalAlignment::Center,
                Align::End => HorizontalAlignment::Right,
                Align::Fill => HorizontalAlignment::Stretch,
            };
            let _ = self.0.native.SetHorizontalAlignment(value);
        } else {
            let value = match align {
                Align::Start => VerticalAlignment::Top,
                Align::Center => VerticalAlignment::Center,
                Align::End => VerticalAlignment::Bottom,
                Align::Fill => VerticalAlignment::Stretch,
            };
            let _ = self.0.native.SetVerticalAlignment(value);
        }
    }

    /// 末尾に子を追加する。
    pub fn append(&self, child: &dyn Widget) {
        let appended = self
            .0
            .native
            .Children()
            .and_then(|c| c.Append(&child.native_element()));
        if appended.is_ok() {
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

/// `bool` を WinRT の `IReference<bool>` に包む。
fn bool_ref(value: bool) -> Result<windows::Foundation::IReference<bool>> {
    use windows_core::Interface;
    windows::Foundation::PropertyValue::CreateBoolean(value)
        .and_then(|v| v.cast())
        .map_err(|e| to_error("bool の boxing", e))
}
