//! Color picking (`NSColorWell`).
//!
//! Clicking it opens the system color panel (`NSColorPanel`), and the chosen color
//! comes back through target/action. The panel's contents — color wheel, palettes,
//! eyedropper — are all AppKit's own.
//!
//! Transparency is not handled, so `supportsAlpha` is off (since [`Color`] holds
//! only opaque colors).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use naui_core::Color;
use objc2::rc::Retained;
use objc2::{sel, MainThreadMarker, Message};
use objc2_app_kit::{NSColor, NSColorSpace, NSColorWell, NSView};

use crate::trampoline::{ActionTarget, ValueHandler};
use crate::widgets::{impl_widget, Widget};

struct ColorPickerInner {
    native: Retained<NSColorWell>,
    handler: ValueHandler<Color>,
    /// Changes made by `set_value` do not call `on_change`.
    silent: Cell<bool>,
    /// AppKit targets are weak references, so they are kept alive here.
    target: RefCell<Option<Retained<ActionTarget>>>,
}

/// A control for picking a color (`NSColorWell`).
///
/// Its value right after creation is black ([`Color::BLACK`]).
#[derive(Clone)]
pub struct ColorPicker(Rc<ColorPickerInner>);
impl_widget!(ColorPicker);

impl ColorPicker {
    pub(crate) fn new(mtm: MainThreadMarker) -> Self {
        let native = NSColorWell::new(mtm);
        native.setSupportsAlpha(false);
        native.setColor(&to_ns_color(Color::BLACK));

        let this = Self(Rc::new(ColorPickerInner {
            native,
            handler: ValueHandler::default(),
            silent: Cell::new(false),
            target: RefCell::new(None),
        }));

        // 中継はハンドルと同じ寿命で持つ。作り直すと、通知の中から色を
        // 変えたときに実行中の中継そのものを解放してしまう。
        let target = ActionTarget::new(mtm, {
            let weak = Rc::downgrade(&this.0);
            move || {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                if inner.silent.get() {
                    return;
                }
                let color = from_ns_color(&inner.native.color());
                inner.handler.emit(color);
            }
        });
        unsafe {
            this.0.native.setTarget(Some(&target));
            this.0.native.setAction(Some(sel!(invoke:)));
        }
        *this.0.target.borrow_mut() = Some(target);
        this
    }

    /// The currently selected color.
    pub fn value(&self) -> Color {
        from_ns_color(&self.0.native.color())
    }

    /// Replaces the color from code. `on_change` is not called.
    pub fn set_value(&self, color: Color) {
        self.0.silent.set(true);
        self.0.native.setColor(&to_ns_color(color));
        self.0.silent.set(false);
    }

    /// Sets the color through the same path as a user's pick and notifies once.
    pub fn pick(&self, color: Color) {
        self.set_value(color);
        self.0.handler.emit(self.value());
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.0.native.setEnabled(enabled);
        if !enabled {
            // 無効にしたときはカラーパネルとのつながりも切る。
            // 開いたままだと、押せない色ウェルへ色が届いてしまう。
            self.0.native.deactivate();
        }
    }

    /// Called with the new color every time the color changes.
    pub fn on_change(&self, f: impl FnMut(Color) + 'static) {
        self.0.handler.set(f);
    }

    /// The color well itself. Public as a backend-specific escape hatch.
    pub fn native_well(&self) -> Retained<NSColorWell> {
        self.0.native.clone()
    }
}

/// Reads an `NSColor` as sRGB.
///
/// The color panel also returns catalog colors (`NSColor.systemBlue` and so on), so
/// it is always converted to sRGB before reading components. Colors that cannot be
/// converted are treated as black.
fn from_ns_color(color: &NSColor) -> Color {
    let Some(srgb) = color.colorUsingColorSpace(&NSColorSpace::sRGBColorSpace()) else {
        return Color::BLACK;
    };
    Color::from_unit(
        srgb.redComponent(),
        srgb.greenComponent(),
        srgb.blueComponent(),
    )
}

fn to_ns_color(color: Color) -> Retained<NSColor> {
    let (r, g, b) = color.to_unit();
    NSColor::colorWithSRGBRed_green_blue_alpha(r, g, b, 1.0)
}
