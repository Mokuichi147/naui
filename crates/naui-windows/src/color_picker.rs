//! Color picking (WinUI 3's native `ColorPicker` placed in a button's flyout).
//!
//! WinUI 3's `ColorPicker` is **a large panel** stacking a spectrum, sliders and hex
//! input vertically, arranged differently from the other three platforms' "click a
//! color swatch to open the picking UI". So, following WinUI 3 conventions, naui
//! puts the `ColorPicker` in a `Button`'s `Flyout` and shows a swatch of the chosen
//! color (`Border` + `SolidColorBrush`) on the button. The color picking UI itself
//! remains WinUI's `ColorPicker`; naui draws nothing.
//!
//! The types come from the [`naui_winui3`] projection as is. It is assembled in XAML
//! passed to `XamlReader`, and the inner elements are looked up by `x:Name`.
//!
//! Transparency is not handled, so `IsAlphaEnabled` is off (since [`Color`] holds
//! only opaque colors).

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

/// The button showing the color swatch, and the `ColorPicker` that opens inside it.
///
/// The app-wide `Button` style's `Padding` (16,8) is too wide for the swatch, so it
/// is overridden here.
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

/// The handler notified when the color changes.
///
/// WinRT delegates require `Send + Sync`, so it is placed in a [`UiThreadCell`].
/// The closure is taken out only for the duration of the call, so operating on the
/// same picker from inside the notification does not double-borrow.
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
    /// The button showing the swatch. This is what goes into the layout.
    native: FrameworkElement,
    /// The flyout's contents. This one holds the value.
    picker: XamlColorPicker,
    /// The brush that paints the button's swatch.
    swatch: SolidColorBrush,
    handler: ColorHandler,
    /// Changes made by `set_value` do not call `on_change`.
    silent: Cell<bool>,
}

/// A control for picking a color (`Button` + `Flyout` + `ColorPicker`).
///
/// Its value right after creation is black ([`Color::BLACK`]).
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
            .map_err(|e| to_error("initialize the ColorPicker", e))?;
        picker
            .SetColor(to_win_color(Color::BLACK))
            .map_err(|e| to_error("initialize the ColorPicker", e))?;

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

    /// Connects WinUI's `ColorChanged` to the Rust closure.
    ///
    /// `ColorChanged` also fires when `Color` is written, so it is silenced during
    /// `set_value` (as on macOS / GTK / Web, notifications are only for user actions).
    /// The swatch is repainted even while silenced.
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
            .map_err(|e| to_error("subscribe to the ColorPicker", e))?;
        Ok(())
    }

    /// The currently selected color.
    pub fn value(&self) -> Color {
        self.0
            .picker
            .Color()
            .map(from_win_color)
            .unwrap_or(Color::BLACK)
    }

    /// Replaces the color from code. `on_change` is not called.
    pub fn set_value(&self, color: Color) {
        self.0.silent.set(true);
        let _ = self.0.picker.SetColor(to_win_color(color));
        self.0.silent.set(false);
        // `ColorChanged` が飛ばなかったときのために、見本はここでも塗る。
        let _ = self.0.swatch.SetColor(to_win_color(color));
    }

    /// Sets the color through the same path as a user's pick and notifies once.
    pub fn pick(&self, color: Color) {
        self.set_value(color);
        self.0.handler.emit(self.value());
    }

    pub fn set_enabled(&self, enabled: bool) {
        if let Ok(control) = self.0.native.cast::<Control>() {
            let _ = control.SetIsEnabled(enabled);
        }
    }

    /// Called with the new color every time the color changes.
    pub fn on_change(&self, f: impl FnMut(Color) + 'static) {
        self.0.handler.set(f);
    }
}

fn load_button() -> Result<FrameworkElement> {
    XamlReader::Load(&HSTRING::from(COLOR_PICKER_XAML))
        .and_then(|element| element.cast::<FrameworkElement>())
        .map_err(|e| to_error("create the color picker", e))
}

/// Looks up an inner element by its XAML `x:Name`.
fn find<T: Interface>(root: &FrameworkElement, name: &str) -> Result<T> {
    root.FindName(&HSTRING::from(name))
        .and_then(|value| value.cast::<T>())
        .map_err(|e| to_error("build the color picker", e))
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
