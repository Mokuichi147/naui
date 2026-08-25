//! 色の選択 (`NSColorWell`)。
//!
//! 押すとシステムのカラーパネル (`NSColorPanel`) が開き、選ばれた色が
//! target/action で戻ってくる。パネルの中身 — カラーホイール・パレット・
//! スポイト — はすべて AppKit のものをそのまま使う。
//!
//! 透明度は扱わないので `supportsAlpha` を切ってある ([`Color`] が
//! 不透明な色しか持たないため)。

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
    /// `set_value` による変更では `on_change` を呼ばない。
    silent: Cell<bool>,
    /// AppKit の target は弱参照なので、ここで生かしておく。
    target: RefCell<Option<Retained<ActionTarget>>>,
}

/// 色を選ばせるコントロール (`NSColorWell`)。
///
/// 作った直後の値は黒 ([`Color::BLACK`])。
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

    /// いま選ばれている色。
    pub fn value(&self) -> Color {
        from_ns_color(&self.0.native.color())
    }

    /// プログラムから色を差し替える。`on_change` は呼ばれない。
    pub fn set_value(&self, color: Color) {
        self.0.silent.set(true);
        self.0.native.setColor(&to_ns_color(color));
        self.0.silent.set(false);
    }

    /// 利用者が選んだのと同じ経路で色を決め、1 回通知する。
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

    /// 色が変わるたびに、変わった後の色で呼ばれる。
    pub fn on_change(&self, f: impl FnMut(Color) + 'static) {
        self.0.handler.set(f);
    }

    /// 色ウェル本体。バックエンド固有の脱出口として公開している。
    pub fn native_well(&self) -> Retained<NSColorWell> {
        self.0.native.clone()
    }
}

/// `NSColor` を sRGB として読む。
///
/// カラーパネルはカタログ色 (`NSColor.systemBlue` など) も返すので、
/// 成分を読む前に必ず sRGB へ変換する。変換できない色は黒として扱う。
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
