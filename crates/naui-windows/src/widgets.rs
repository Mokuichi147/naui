//! WinUI 3 (Fluent 2) の実コントロールを包むハンドル群。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use naui_core::{Align, Orientation, Padding, Result, TextColor, TextStyle};
use naui_winui3::Microsoft::UI::Dispatching::{DispatcherQueue, DispatcherQueueTimer};
use naui_winui3::Microsoft::UI::Xaml::Controls::{
    Button as XamlButton, CheckBox as XamlCheckBox, Grid, Orientation as XamlOrientation,
    PasswordBox, ScrollBarVisibility, ScrollViewer, Slider as XamlSlider, StackPanel, TextBlock,
    TextBox,
};
use naui_winui3::Microsoft::UI::Xaml::Markup::XamlReader;
use naui_winui3::Microsoft::UI::Xaml::{
    Application, FrameworkElement, ResourceDictionary, RoutedEventHandler, Style, TextWrapping,
    Thickness, UIElement,
};
use windows::Foundation::{EventHandler, PropertyValue, TypedEventHandler};
use windows_core::{IInspectable, Interface, HSTRING};

use crate::to_error;
use crate::ui_thread::UiThreadCell;

/// naui のウィジェットが実装する共通インタフェース。
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
            /// 大きさを指定する。呼ぶたびに以前の指定は置き換わる。
            ///
            /// 実際の大きさを決めるのは WinUI のレイアウトパスなので、
            /// ここで渡すのは `Width` / `MinWidth` などの指定だけ。
            pub fn set_sizing(&self, sizing: naui_core::Sizing) {
                let element = <$t as Widget>::native_element(self);
                crate::layout::apply_sizing(&element, sizing);
            }

            /// 表示するかどうか。既定は表示。
            ///
            /// 隠したものは `Stack` の中では場所を空けない。`Grid` ではマスが残る。
            pub fn set_visible(&self, visible: bool) {
                crate::interaction::set_visible(&<$t as Widget>::native_element(self), visible);
            }

            /// [`set_visible`](Self::set_visible) で指定した表示。親が隠れていても
            /// `true` のまま。
            pub fn is_visible(&self) -> bool {
                crate::interaction::is_visible(&<$t as Widget>::native_element(self))
            }

            /// キーボードフォーカスを移す。移せたら `true`。
            ///
            /// 自分が受け取れないとき (コンテナなど) は、中で最初に受け取れるものへ
            /// 移す。表示前・隠れている・無効なときは `false`。
            pub fn request_focus(&self) -> bool {
                crate::interaction::request_focus(&<$t as Widget>::native_element(self))
            }

            /// ポインターを重ねたときに出す説明 (`ToolTipService`)。`None` で外す。
            pub fn set_tooltip(&self, text: Option<&str>) {
                crate::interaction::set_tooltip(&<$t as Widget>::native_element(self), text);
            }

            /// 読み上げソフトに伝える名前。`None` で外す (見えている文字が使われる)。
            ///
            /// アイコンだけのボタンのように、見えている文字が無いか意味を
            /// 表しきれないときに付ける。
            pub fn set_accessible_label(&self, text: Option<&str>) {
                crate::interaction::set_accessible_label(
                    &<$t as Widget>::native_element(self),
                    text,
                );
            }
        }
    };
}

pub(crate) use impl_widget;

/// 選択が変わったことの通知先。
///
/// コールバックを呼ぶ間はセルから一度取り出す。これにより、コールバックから
/// 同じウィジェットを操作しても再入時の借用が衝突せず、`on_select` を呼び直して
/// コールバックを差し替えた場合も新しいものを上書きしない。
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
    /// いま当てている段階と役割。片方だけ変えても、両方を持つ `Style` を
    /// 組み直せるように覚えておく。
    style: Cell<TextStyle>,
    color: Cell<TextColor>,
}

/// テキスト表示 (TextBlock)。
#[derive(Clone)]
pub struct Label(Rc<LabelInner>);
impl_widget!(Label, native);

/// ラベルの土台。
///
/// 省略記号 (`TextTrimming`) は XAML で持たせておく。折り返さないときだけ
/// 効く指定なので、実行時に
/// 切り替える必要はない (切り替えるのは `TextWrapping` のほう)。
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

    /// 文字を選んでコピーできるようにするか。既定は 選べない。
    ///
    /// チャットの発言やエラーの詳細のように、読む人が写し取りたい文字に使う。
    /// 入力欄と違い、文字は書き換えられない。
    pub fn set_selectable(&self, selectable: bool) {
        let _ = self.0.native.SetIsTextSelectionEnabled(selectable);
    }

    pub fn is_selectable(&self) -> bool {
        self.0.native.IsTextSelectionEnabled().unwrap_or(false)
    }

    /// 長い文字列を折り返すかどうか。既定は折り返さない。
    ///
    /// 折り返さないときは 1 行のまま、入りきらない分を末尾の省略記号 (…) で
    /// 切る (`TextTrimming="CharacterEllipsis"`)。
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

    /// 文字の大きさと太さの段階。既定は [`TextStyle::Body`]。
    ///
    /// WinUI 3 の type ramp が持つ `Style` (`TitleTextBlockStyle` など) を
    /// そのまま当てる。級数と太さを決めるのは Fluent のテーマのほう。
    pub fn set_style(&self, style: TextStyle) {
        self.0.style.set(style);
        self.apply_text_style();
    }

    /// 文字色の役割。既定は [`TextColor::Default`]。
    ///
    /// 色は `{ThemeResource}` として置くので、ウィンドウの `RequestedTheme` を
    /// 切り替えたときもそのまま追従する (`Foreground` へ直に書いた色は
    /// 切り替えに付いてこない)。
    pub fn set_color(&self, color: TextColor) {
        self.0.color.set(color);
        self.apply_text_style();
    }

    /// 段階と役割を 1 つの `Style` にまとめて当てる。
    ///
    /// 要素に当てられる `Style` は 1 つだけなので、色の `Setter` だけを持つ
    /// `Style` を type ramp の `Style` の上へ重ねる (`BasedOn`)。
    /// どちらかが引けなければ何もしない (前の見た目のまま続ける)。
    fn apply_text_style(&self) {
        let Some(style) = label_style(self.0.style.get(), self.0.color.get()) else {
            eprintln!("naui-windows: ラベルのスタイルが引けませんでした");
            return;
        };
        let _ = self.0.native.SetStyle(&style);
    }
}

/// 色の `Setter` を持つ `Style` の土台。`{brush}` にテーマリソースの名前が入る。
///
/// `x:Key` は [`LABEL_COLOR_STYLE_KEY`] と同じ文字列にする。
const LABEL_COLOR_STYLE_XAML: &str = r##"<ResourceDictionary
    xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
    xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml">
    <Style x:Key="NauiLabelColorStyle" TargetType="TextBlock">
        <Setter Property="Foreground" Value="{ThemeResource {brush}}"/>
    </Style>
</ResourceDictionary>"##;

const LABEL_COLOR_STYLE_KEY: &str = "NauiLabelColorStyle";

/// 段階と役割に対応する `Style`。引けなければ `None`。
///
/// naui の `Label` を使えない場所 (表のセルのように文字揃えを直に決める
/// ところ) でも、同じ見た目を当てられるように公開している。
pub(crate) fn label_style(style: TextStyle, color: TextColor) -> Option<Style> {
    let ramp = app_resource(style.xaml_style_key())?.cast::<Style>().ok()?;
    let Some(brush_key) = color.xaml_brush_key() else {
        return Some(ramp);
    };
    let tinted = label_color_style(brush_key)?;
    tinted.SetBasedOn(&ramp).ok()?;
    Some(tinted)
}

/// 文字色だけを決める `Style` を作る。
///
/// `Style` は一度当てると封をされて変えられなくなるので、当てるたびに
/// 作り直す。
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

/// アプリのリソース辞書から 1 つ引く。`XamlControlsResources` を通して
/// Fluent の type ramp とテーマリソースがここに入っている。
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
crate::keys::impl_key_down!(TextInput);
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

    /// 読み取り専用にするか。既定は書き換えられる。
    ///
    /// 読み取り専用の間も文字は選んでコピーでき、フォーカスも受け取る
    /// (`set_enabled(false)` と違い、薄く表示されない)。ログや生成結果を
    /// 見せる欄に使う。
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
}

/// パスワード入力 (PasswordBox)。
///
/// API の形は [`TextInput`] と同じで、違うのは**打った文字が伏せ字になる**
/// ことだけ。伏せ字を一時的に外すボタン (`IsPasswordRevealButtonEnabled`) は
/// WinUI 3 にあるが、4 環境の共通部分に無いので出さない。
#[derive(Clone)]
pub struct PasswordInput(Rc<PasswordInputInner>);
crate::keys::impl_key_down!(PasswordInput);
impl_widget!(PasswordInput, native);

impl PasswordInput {
    pub(crate) fn new() -> Result<Self> {
        let native = PasswordBox::new().map_err(|e| to_error("PasswordBox の生成", e))?;
        Ok(Self(Rc::new(PasswordInputInner {
            native,
            token: RefCell::new(None),
        })))
    }

    /// いま入力されている文字列。
    pub fn text(&self) -> String {
        self.0
            .native
            .Password()
            .map(|s| s.to_string())
            .unwrap_or_default()
    }

    /// 文字列を置き換える。
    ///
    /// WinUI がネイティブの `PasswordChanged` を出すため、**Windows だけは
    /// `on_change` も呼ばれる** ([`TextInput::set_text`] と同じ)。
    pub fn set_text(&self, text: &str) {
        let _ = self.0.native.SetPassword(&HSTRING::from(text));
    }

    pub fn set_placeholder(&self, text: &str) {
        let _ = self.0.native.SetPlaceholderText(&HSTRING::from(text));
    }

    pub fn set_enabled(&self, enabled: bool) {
        let _ = self.0.native.SetIsEnabled(enabled);
    }

    /// 1 文字入力するたびに、その時点の文字列で呼ばれる。
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
}

// --------------------------------------------------------------- TextArea

struct TextAreaInner {
    native: TextBox,
    token: RefCell<Option<i64>>,
}

/// 複数行テキスト入力 (改行を受け付ける TextBox)。IME は Windows が処理する。
#[derive(Clone)]
pub struct TextArea(Rc<TextAreaInner>);
crate::keys::impl_key_down!(TextArea);
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
        })))
    }

    /// いまの文字列。改行はそのまま含まれる。
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

    /// 何も入力されていないときに薄く出る文字。
    pub fn set_placeholder(&self, text: &str) {
        let _ = self.0.native.SetPlaceholderText(&HSTRING::from(text));
    }

    pub fn set_enabled(&self, enabled: bool) {
        let _ = self.0.native.SetIsEnabled(enabled);
    }

    /// 1 文字入力するたびに、その時点の文字列で呼ばれる。改行の入力でも呼ばれる。
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

    /// 読み取り専用にするか。既定は書き換えられる。
    ///
    /// 読み取り専用の間も文字は選んでコピーでき、フォーカスも受け取る
    /// (`set_enabled(false)` と違い、薄く表示されない)。ログや生成結果を
    /// 見せる欄に使う。
    pub fn set_read_only(&self, read_only: bool) {
        let _ = self.0.native.SetIsReadOnly(read_only);
    }

    pub fn is_read_only(&self) -> bool {
        self.0.native.IsReadOnly().unwrap_or(false)
    }
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

/// スライダー (Slider)。
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

    /// つまみが動くたびに、その値で呼ばれる。
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
    /// 不確定の間、前景の帯を往復させるタイマー。
    animation: RefCell<Option<DispatcherQueueTimer>>,
    /// 往復の進み具合 (0.0..2.0。1.0 を超えたら戻る向き)。
    phase: Cell<f64>,
}

impl Drop for ProgressInner {
    fn drop(&mut self) {
        if let Some(timer) = self.animation.borrow_mut().take() {
            let _ = timer.Stop();
        }
    }
}

/// 不確定の帯の長さ (バーの幅に対する割合)。
const INDETERMINATE_SEGMENT: f64 = 0.3;
/// 不確定の帯を動かす間隔と、1 回に進む量 (往復で約 2 秒)。
const INDETERMINATE_TICK_MILLIS: i64 = 33;
const INDETERMINATE_STEP: f64 = 0.033;

/// 進捗バー (ProgressBar)。
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

    /// 0.0..=1.0。
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

    /// 進み具合が分からない処理中の表示 (不確定の進捗) にするか。
    ///
    /// `true` の間は値の代わりに動きで「処理中」を示す。戻すと
    /// [`set_value`](Self::set_value) で置いた値の表示に戻る (値は覚えている)。
    ///
    /// WinUI の `ProgressBar` は未パッケージ起動でテンプレートを当てた瞬間に
    /// 落ちるため、naui は同じテーマ資源で組んだ帯を使っている。不確定の
    /// 表示も、その前景の帯を一定の間隔で往復させて表す。
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
    /// 前景の帯を 1 歩進める。端まで行ったら折り返す。
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

// ------------------------------------------------------------------ Stack

struct StackInner {
    native: StackPanel,
    /// 交差軸に置く子の既定の寄せ方。`Fill` の子は自分の指定を優先する。
    align: Cell<Align>,
    children: RefCell<Vec<Box<dyn Widget>>>,
}

impl Drop for StackInner {
    fn drop(&mut self) {
        if let Ok(children) = self.children.try_borrow() {
            for child in children.iter() {
                crate::layout::clear_parent_layout(&child.native_element());
            }
        }
    }
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
            align: Cell::new(Align::default()),
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
        self.0.align.set(align);
        let vertical = self.is_vertical();
        for child in self.0.children.borrow().iter() {
            crate::layout::set_stack_parent(&child.native_element(), align, vertical);
        }
    }

    fn is_vertical(&self) -> bool {
        self.0
            .native
            .Orientation()
            .map(|o| o == XamlOrientation::Vertical)
            .unwrap_or(true)
    }

    /// 末尾に子を追加する。
    pub fn append(&self, child: &dyn Widget) {
        let element = child.native_element();
        let appended = self.0.native.Children().and_then(|c| c.Append(&element));
        if appended.is_ok() {
            crate::layout::set_stack_parent(&element, self.0.align.get(), self.is_vertical());
            self.0.children.borrow_mut().push(child.boxed_clone());
        }
    }

    /// 指定した位置へ子を差し込む。`index` が今の数以上なら末尾へ足す。
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
            crate::layout::set_stack_parent(&element, self.0.align.get(), self.is_vertical());
            children.insert(index, child.boxed_clone());
        }
    }

    /// 指定した位置の子を外す。範囲外のときは何もしない。
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
            crate::layout::clear_parent_layout(&children[index].native_element());
            children.remove(index);
        }
    }

    /// 子をすべて外す。
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
            crate::layout::clear_parent_layout(&element);
        }
        self.0.children.borrow_mut().clear();
    }

    pub fn len(&self) -> usize {
        self.0.children.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// `bool` を WinRT の `IReference<bool>` に包む。
pub(crate) fn bool_ref(value: bool) -> Result<windows::Foundation::IReference<bool>> {
    use windows_core::Interface;
    windows::Foundation::PropertyValue::CreateBoolean(value)
        .and_then(|v| v.cast())
        .map_err(|e| to_error("bool の boxing", e))
}
