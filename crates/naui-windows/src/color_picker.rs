//! 色の選択 (WinUI 3 のネイティブ `ColorPicker` をボタンのフライアウトへ入れたもの)。
//!
//! WinUI 3 の `ColorPicker` は、スペクトラム・スライダー・16 進入力を縦に
//! 並べた**大きな面**で、他の 3 環境の「色の見本を押すと選択の UI が開く」
//! という形とは並び方が違う。そこで naui では、WinUI 3 の作法どおり
//! `Button` の `Flyout` へ `ColorPicker` を入れ、ボタンには選んだ色の
//! 見本 (`Border` + `SolidColorBrush`) を出す。色を選ぶ UI そのものは
//! WinUI の `ColorPicker` のままで、naui が描くものは無い。
//!
//! 型は [`naui_winui3`] の投影をそのまま使う。組み立ては XAML に書いて
//! `XamlReader` へ渡し (`Flyout` は投影に無い)、中の要素は `x:Name` から
//! 引く。
//!
//! 透明度は扱わないので `IsAlphaEnabled` を切ってある ([`Color`] が
//! 不透明な色しか持たないため)。

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use naui_core::{Color, Result};
use naui_winui3::Microsoft::UI::Xaml::Controls::{
    ColorChangedEventArgs, ColorPicker as XamlColorPicker, Control,
};
use naui_winui3::Microsoft::UI::Xaml::Markup::XamlReader;
use naui_winui3::Microsoft::UI::Xaml::Media::SolidColorBrush;
use naui_winui3::Microsoft::UI::Xaml::{FrameworkElement, UIElement};
use windows::Foundation::TypedEventHandler;
use windows::UI::Color as WinColor;
use windows_core::{Interface, HSTRING};

use crate::to_error;
use crate::ui_thread::{HandlerCell, UiThreadCell};
use crate::widgets::{impl_widget, Widget};

/// 色の見本を出すボタンと、その中に開く `ColorPicker`。
///
/// ボタンの `Padding` は、アプリ全体の `Button` スタイル (16,8) では
/// 見本に対して広すぎるのでここで上書きする。
const COLOR_PICKER_XAML: &str = r##"<Button
    xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
    xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml"
    Padding="8,6">
    <Border x:Name="SwatchBorder" Width="48" Height="16"
            BorderThickness="1"
            BorderBrush="{ThemeResource ControlStrokeColorDefaultBrush}"
            CornerRadius="{ThemeResource ControlCornerRadius}">
        <Border.Background>
            <SolidColorBrush x:Name="Swatch" Color="#FF000000"/>
        </Border.Background>
    </Border>
    <Button.Flyout>
        <Flyout>
            <ColorPicker x:Name="Picker" IsAlphaEnabled="False"
                         IsMoreButtonVisible="True"/>
        </Flyout>
    </Button.Flyout>
</Button>"##;

/// 色が変わったことの通知先。
///
/// WinRT のデリゲートは `Send + Sync` を要求するので [`UiThreadCell`] に
/// 載せる。呼び出しの間だけクロージャを取り出すため、通知の中から同じ
/// ピッカーを操作しても二重借用にならない。
#[derive(Clone)]
struct ColorHandler(HandlerCell<dyn FnMut(Color)>);

impl ColorHandler {
    fn new() -> Self {
        Self(Arc::new(UiThreadCell::new(None)))
    }

    fn set(&self, f: impl FnMut(Color) + 'static) {
        self.0.with_mut(|slot| *slot = Some(Box::new(f)));
    }

    fn emit(&self, color: Color) {
        let Some(Some(mut f)) = self.0.try_with_mut(|slot| slot.take()) else {
            return;
        };
        f(color);
        let _ = self.0.try_with_mut(|slot| {
            if slot.is_none() {
                *slot = Some(f);
            }
        });
    }
}

struct ColorPickerInner {
    /// 見本を出すボタン。レイアウトへ載るのはこれ。
    native: FrameworkElement,
    /// フライアウトの中身。値を持っているのはこちら。
    picker: XamlColorPicker,
    /// ボタンの見本を塗るブラシ。
    swatch: SolidColorBrush,
    handler: ColorHandler,
    /// `set_value` による変更では `on_change` を呼ばない。
    silent: Cell<bool>,
}

/// 色を選ばせるコントロール (`Button` + `Flyout` + `ColorPicker`)。
///
/// 作った直後の値は黒 ([`Color::BLACK`])。
#[derive(Clone)]
pub struct ColorPicker(Rc<ColorPickerInner>);
impl_widget!(ColorPicker, native);

impl ColorPicker {
    pub(crate) fn new() -> Result<Self> {
        let native = load_button()?;
        let picker: XamlColorPicker = find(&native, "Picker")?;
        let swatch: SolidColorBrush = find(&native, "Swatch")?;
        picker
            .SetIsAlphaEnabled(false)
            .map_err(|e| to_error("ColorPicker の初期化", e))?;
        picker
            .SetColor(to_win_color(Color::BLACK))
            .map_err(|e| to_error("ColorPicker の初期化", e))?;

        let this = Self(Rc::new(ColorPickerInner {
            native,
            picker,
            swatch,
            handler: ColorHandler::new(),
            silent: Cell::new(false),
        }));
        this.connect()?;
        Ok(this)
    }

    /// WinUI の `ColorChanged` を Rust のクロージャへつなぐ。
    ///
    /// `ColorChanged` は `Color` を書き換えたときにも飛ぶので、`set_value`
    /// の間は黙らせる (macOS / GTK / Web と同じく、通知は利用者の操作の
    /// ときだけ)。見本の塗り直しは黙っている間も行う。
    fn connect(&self) -> Result<()> {
        let target = Arc::new(UiThreadCell::new(Rc::downgrade(&self.0)));
        let changed = TypedEventHandler::new(
            move |_sender: windows_core::Ref<'_, XamlColorPicker>,
                  args: windows_core::Ref<'_, ColorChangedEventArgs>| {
                let _ = target.try_with_mut(|weak| {
                    let Some(inner) = weak.upgrade() else {
                        return;
                    };
                    let Some(color) = args.as_ref().and_then(|a| a.NewColor().ok()) else {
                        return;
                    };
                    let _ = inner.swatch.SetColor(color);
                    if !inner.silent.get() {
                        inner.handler.emit(from_win_color(color));
                    }
                });
                Ok(())
            },
        );
        self.0
            .picker
            .ColorChanged(&changed)
            .map_err(|e| to_error("ColorPicker の購読", e))?;
        Ok(())
    }

    /// いま選ばれている色。
    pub fn value(&self) -> Color {
        self.0
            .picker
            .Color()
            .map(from_win_color)
            .unwrap_or(Color::BLACK)
    }

    /// プログラムから色を差し替える。`on_change` は呼ばれない。
    pub fn set_value(&self, color: Color) {
        self.0.silent.set(true);
        let _ = self.0.picker.SetColor(to_win_color(color));
        self.0.silent.set(false);
        // `ColorChanged` が飛ばなかったときのために、見本はここでも塗る。
        let _ = self.0.swatch.SetColor(to_win_color(color));
    }

    /// 利用者が選んだのと同じ経路で色を決め、1 回通知する。
    pub fn pick(&self, color: Color) {
        self.set_value(color);
        self.0.handler.emit(self.value());
    }

    pub fn set_enabled(&self, enabled: bool) {
        if let Ok(control) = self.0.native.cast::<Control>() {
            let _ = control.SetIsEnabled(enabled);
        }
    }

    /// 色が変わるたびに、変わった後の色で呼ばれる。
    pub fn on_change(&self, f: impl FnMut(Color) + 'static) {
        self.0.handler.set(f);
    }
}

fn load_button() -> Result<FrameworkElement> {
    XamlReader::Load(&HSTRING::from(COLOR_PICKER_XAML))
        .and_then(|element| element.cast::<FrameworkElement>())
        .map_err(|e| to_error("色ピッカーの生成", e))
}

/// XAML の `x:Name` から中の要素を引く。
fn find<T: Interface>(root: &FrameworkElement, name: &str) -> Result<T> {
    root.FindName(&HSTRING::from(name))
        .and_then(|value| value.cast::<T>())
        .map_err(|e| to_error("色ピッカーの組み立て", e))
}

fn to_win_color(color: Color) -> WinColor {
    WinColor {
        A: 255,
        R: color.r,
        G: color.g,
        B: color.b,
    }
}

fn from_win_color(color: WinColor) -> Color {
    Color::rgb(color.R, color.G, color.B)
}
