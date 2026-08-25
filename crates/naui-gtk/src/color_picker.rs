//! 色の選択 (`GtkColorDialogButton` + `GtkColorDialog`)。
//!
//! 押すと GTK4 の色選択ダイアログが開き、選ばれた色がボタンの `rgba`
//! プロパティへ入る。パレット・カラーホイール・16 進入力はすべて
//! `GtkColorDialog` のものをそのまま使う。
//!
//! 透明度は扱わないので `with-alpha` を切ってある ([`Color`] が不透明な色
//! しか持たないため)。

use std::cell::RefCell;
use std::rc::Rc;

use gtk::gdk;
use gtk::glib;
use gtk::prelude::*;
use naui_core::Color;

use crate::bin::SizeBin;
use crate::callback::Notifier;
use crate::widgets::{impl_widget, without_signal, Widget};

struct ColorPickerInner {
    native: gtk::ColorDialogButton,
    bin: SizeBin,
    on_change: Notifier<Color>,
    handler: RefCell<Option<glib::SignalHandlerId>>,
}

/// 色を選ばせるコントロール (`GtkColorDialogButton`)。
///
/// 作った直後の値は黒 ([`Color::BLACK`])。
#[derive(Clone)]
pub struct ColorPicker(Rc<ColorPickerInner>);
impl_widget!(ColorPicker);

impl ColorPicker {
    pub(crate) fn new() -> Self {
        let dialog = gtk::ColorDialog::new();
        dialog.set_with_alpha(false);
        let native = gtk::ColorDialogButton::new(Some(dialog));
        native.set_rgba(&to_rgba(Color::BLACK));
        let bin = SizeBin::wrap(&native);

        let inner = Rc::new(ColorPickerInner {
            native,
            bin,
            on_change: Notifier::default(),
            handler: RefCell::new(None),
        });
        let id = {
            let weak = Rc::downgrade(&inner);
            inner.native.connect_rgba_notify(move |native| {
                if let Some(inner) = weak.upgrade() {
                    inner.on_change.emit(from_rgba(native.rgba()));
                }
            })
        };
        *inner.handler.borrow_mut() = Some(id);
        Self(inner)
    }

    /// いま選ばれている色。
    pub fn value(&self) -> Color {
        from_rgba(self.0.native.rgba())
    }

    /// プログラムから色を差し替える。`on_change` は呼ばれない。
    pub fn set_value(&self, color: Color) {
        without_signal(&self.0.native, &self.0.handler, || {
            self.0.native.set_rgba(&to_rgba(color));
        });
    }

    /// 利用者が選んだのと同じ経路で色を決め、1 回通知する。
    pub fn pick(&self, color: Color) {
        self.set_value(color);
        self.0.on_change.emit(self.value());
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.0.native.set_sensitive(enabled);
    }

    /// 色が変わるたびに、変わった後の色で呼ばれる。
    pub fn on_change(&self, f: impl FnMut(Color) + 'static) {
        self.0.on_change.set(f);
    }

    /// ボタン本体。バックエンド固有の脱出口として公開している。
    pub fn native_button(&self) -> gtk::ColorDialogButton {
        self.0.native.clone()
    }
}

fn to_rgba(color: Color) -> gdk::RGBA {
    let (r, g, b) = color.to_unit();
    gdk::RGBA::new(r as f32, g as f32, b as f32, 1.0)
}

fn from_rgba(rgba: gdk::RGBA) -> Color {
    Color::from_unit(
        f64::from(rgba.red()),
        f64::from(rgba.green()),
        f64::from(rgba.blue()),
    )
}
