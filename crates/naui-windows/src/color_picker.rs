//! 色の選択 (WinUI 3 のネイティブ `ColorPicker` をボタンのフライアウトへ入れたもの)。
//!
//! WinUI 3 の `ColorPicker` は、スペクトラム・スライダー・16 進入力を縦に
//! 並べた**大きな面**で、他の 3 環境の「色の見本を押すと選択の UI が開く」
//! という形とは並び方が違う。そこで naui では、WinUI 3 の作法どおり
//! `Button` の `Flyout` へ `ColorPicker` を入れ、ボタンには選んだ色の
//! 見本 (`Border` + `SolidColorBrush`) を出す。色を選ぶ UI そのものは
//! WinUI の `ColorPicker` のままで、naui が描くものは無い。
//!
//! 公開 WinRT インターフェイスの必要な部分をこのモジュールで定義している。
//! `ColorPicker` と `SolidColorBrush` は [`naui_winui3`] にも入ったので、
//! この手書きの投影は将来そちらへ寄せられる。組み立ては XAML に書いて
//! `XamlReader` へ渡し、
//! 中の要素は `x:Name` から引く。
//!
//! 透明度は扱わないので `IsAlphaEnabled` を切ってある ([`Color`] が
//! 不透明な色しか持たないため)。

use std::cell::Cell;
use std::ffi::c_void;
use std::rc::Rc;
use std::sync::Arc;

use naui_core::{Color, Result};
use naui_winui3::Microsoft::UI::Xaml::Controls::Control;
use naui_winui3::Microsoft::UI::Xaml::Markup::XamlReader;
use naui_winui3::Microsoft::UI::Xaml::{FrameworkElement, UIElement};
use windows::Foundation::TypedEventHandler;
use windows::UI::Color as WinColor;
use windows_core::{Interface, Param, HSTRING};

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
    picker: NativeColorPicker,
    /// ボタンの見本を塗るブラシ。
    swatch: NativeSolidColorBrush,
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
        let picker: NativeColorPicker = find(&native, "Picker")?;
        let swatch: NativeSolidColorBrush = find(&native, "Swatch")?;
        picker
            .set_is_alpha_enabled(false)
            .map_err(|e| to_error("ColorPicker の初期化", e))?;
        picker
            .set_color(to_win_color(Color::BLACK))
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
            move |_sender: windows_core::Ref<'_, NativeColorPicker>,
                  args: windows_core::Ref<'_, NativeColorChangedEventArgs>| {
                let _ = target.try_with_mut(|weak| {
                    let Some(inner) = weak.upgrade() else {
                        return;
                    };
                    let Some(color) = args.as_ref().and_then(|a| a.new_color().ok()) else {
                        return;
                    };
                    let _ = inner.swatch.set_color(color);
                    if !inner.silent.get() {
                        inner.handler.emit(from_win_color(color));
                    }
                });
                Ok(())
            },
        );
        self.0
            .picker
            .color_changed(&changed)
            .map_err(|e| to_error("ColorPicker の購読", e))?;
        Ok(())
    }

    /// いま選ばれている色。
    pub fn value(&self) -> Color {
        self.0
            .picker
            .color()
            .map(from_win_color)
            .unwrap_or(Color::BLACK)
    }

    /// プログラムから色を差し替える。`on_change` は呼ばれない。
    pub fn set_value(&self, color: Color) {
        self.0.silent.set(true);
        let _ = self.0.picker.set_color(to_win_color(color));
        self.0.silent.set(false);
        // `ColorChanged` が飛ばなかったときのために、見本はここでも塗る。
        let _ = self.0.swatch.set_color(to_win_color(color));
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

// -------------------------------------------------------------------------
// Microsoft.UI.Xaml.Controls.ColorPicker / ColorChangedEventArgs と
// Microsoft.UI.Xaml.Media.SolidColorBrush の最小 WinRT 投影
//
// IID と vtable の並びは cppwinrt の生成ヘッダー
// (winrt/impl/Microsoft.UI.Xaml.Controls.0.h ほか) の guid_v / abi<...> に
// 合わせている。使わないメソッドは `usize` で場所だけ空けてある。

windows_core::imp::define_interface!(
    IColorPicker,
    IColorPicker_Vtbl,
    0xae72b24b_f93f_5a19_8ce4_a18b73c3356d
);
impl windows_core::RuntimeType for IColorPicker {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}

#[repr(C)]
pub struct IColorPicker_Vtbl {
    base__: windows_core::IInspectable_Vtbl,
    color: unsafe extern "system" fn(*mut c_void, *mut WinColor) -> windows_core::HRESULT,
    set_color: unsafe extern "system" fn(*mut c_void, WinColor) -> windows_core::HRESULT,
    previous_color: usize,
    set_previous_color: usize,
    is_alpha_enabled: usize,
    set_is_alpha_enabled: unsafe extern "system" fn(*mut c_void, bool) -> windows_core::HRESULT,
    is_color_spectrum_visible: usize,
    set_is_color_spectrum_visible: usize,
    is_color_preview_visible: usize,
    set_is_color_preview_visible: usize,
    is_color_slider_visible: usize,
    set_is_color_slider_visible: usize,
    is_alpha_slider_visible: usize,
    set_is_alpha_slider_visible: usize,
    is_more_button_visible: usize,
    set_is_more_button_visible: usize,
    is_color_channel_text_input_visible: usize,
    set_is_color_channel_text_input_visible: usize,
    is_alpha_text_input_visible: usize,
    set_is_alpha_text_input_visible: usize,
    is_hex_input_visible: usize,
    set_is_hex_input_visible: usize,
    min_hue: usize,
    set_min_hue: usize,
    max_hue: usize,
    set_max_hue: usize,
    min_saturation: usize,
    set_min_saturation: usize,
    max_saturation: usize,
    set_max_saturation: usize,
    min_value: usize,
    set_min_value: usize,
    max_value: usize,
    set_max_value: usize,
    color_spectrum_shape: usize,
    set_color_spectrum_shape: usize,
    color_spectrum_components: usize,
    set_color_spectrum_components: usize,
    color_changed:
        unsafe extern "system" fn(*mut c_void, *mut c_void, *mut i64) -> windows_core::HRESULT,
    remove_color_changed: unsafe extern "system" fn(*mut c_void, i64) -> windows_core::HRESULT,
}

#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
struct NativeColorPicker(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    NativeColorPicker,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl windows_core::RuntimeType for NativeColorPicker {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IColorPicker>();
}
unsafe impl Interface for NativeColorPicker {
    type Vtable = IColorPicker_Vtbl;
    const IID: windows_core::GUID = IColorPicker::IID;
}
impl windows_core::RuntimeName for NativeColorPicker {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.ColorPicker";
}

impl NativeColorPicker {
    fn color(&self) -> windows_core::Result<WinColor> {
        unsafe {
            let mut result = WinColor::default();
            (Interface::vtable(self).color)(Interface::as_raw(self), &mut result).map(|| result)
        }
    }

    fn set_color(&self, value: WinColor) -> windows_core::Result<()> {
        unsafe { (Interface::vtable(self).set_color)(Interface::as_raw(self), value).ok() }
    }

    fn set_is_alpha_enabled(&self, value: bool) -> windows_core::Result<()> {
        unsafe {
            (Interface::vtable(self).set_is_alpha_enabled)(Interface::as_raw(self), value).ok()
        }
    }

    fn color_changed<P>(&self, handler: P) -> windows_core::Result<i64>
    where
        P: Param<TypedEventHandler<NativeColorPicker, NativeColorChangedEventArgs>>,
    {
        unsafe {
            let mut token = 0;
            (Interface::vtable(self).color_changed)(
                Interface::as_raw(self),
                handler.param().abi(),
                &mut token,
            )
            .map(|| token)
        }
    }
}

windows_core::imp::define_interface!(
    IColorChangedEventArgs,
    IColorChangedEventArgs_Vtbl,
    0x148d57a2_b1cb_5f5d_b6b5_512805d71761
);
impl windows_core::RuntimeType for IColorChangedEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}

#[repr(C)]
pub struct IColorChangedEventArgs_Vtbl {
    base__: windows_core::IInspectable_Vtbl,
    old_color: usize,
    new_color: unsafe extern "system" fn(*mut c_void, *mut WinColor) -> windows_core::HRESULT,
}

#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
struct NativeColorChangedEventArgs(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    NativeColorChangedEventArgs,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl windows_core::RuntimeType for NativeColorChangedEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IColorChangedEventArgs>();
}
unsafe impl Interface for NativeColorChangedEventArgs {
    type Vtable = IColorChangedEventArgs_Vtbl;
    const IID: windows_core::GUID = IColorChangedEventArgs::IID;
}
impl windows_core::RuntimeName for NativeColorChangedEventArgs {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.ColorChangedEventArgs";
}

impl NativeColorChangedEventArgs {
    fn new_color(&self) -> windows_core::Result<WinColor> {
        unsafe {
            let mut result = WinColor::default();
            (Interface::vtable(self).new_color)(Interface::as_raw(self), &mut result).map(|| result)
        }
    }
}

windows_core::imp::define_interface!(
    ISolidColorBrush,
    ISolidColorBrush_Vtbl,
    0xb3865c31_37c8_55c1_8a72_d41c67642e2a
);
impl windows_core::RuntimeType for ISolidColorBrush {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}

#[repr(C)]
pub struct ISolidColorBrush_Vtbl {
    base__: windows_core::IInspectable_Vtbl,
    color: usize,
    set_color: unsafe extern "system" fn(*mut c_void, WinColor) -> windows_core::HRESULT,
}

#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
struct NativeSolidColorBrush(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    NativeSolidColorBrush,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl windows_core::RuntimeType for NativeSolidColorBrush {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, ISolidColorBrush>();
}
unsafe impl Interface for NativeSolidColorBrush {
    type Vtable = ISolidColorBrush_Vtbl;
    const IID: windows_core::GUID = ISolidColorBrush::IID;
}
impl windows_core::RuntimeName for NativeSolidColorBrush {
    const NAME: &'static str = "Microsoft.UI.Xaml.Media.SolidColorBrush";
}

impl NativeSolidColorBrush {
    fn set_color(&self, value: WinColor) -> windows_core::Result<()> {
        unsafe { (Interface::vtable(self).set_color)(Interface::as_raw(self), value).ok() }
    }
}
