//! Color picking (`GtkColorDialogButton` + `GtkColorDialog`).
//!
//! Clicking it opens GTK4's color selection dialog, and the chosen color goes into
//! the button's `rgba` property. The palette, color wheel and hex input are all
//! `GtkColorDialog`'s own.
//!
//! Transparency is not handled, so `with-alpha` is off (since [`Color`] holds only
//! opaque colors).

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

/// A control for picking a color (`GtkColorDialogButton`).
///
/// Its value right after creation is black ([`Color::BLACK`]).
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

    /// The currently selected color.
    pub fn value(&self) -> Color {
        from_rgba(self.0.native.rgba())
    }

    /// Replaces the color from code. `on_change` is not called.
    pub fn set_value(&self, color: Color) {
        without_signal(&self.0.native, &self.0.handler, || {
            self.0.native.set_rgba(&to_rgba(color));
        });
    }

    /// Sets the color through the same path as a user's pick and notifies once.
    pub fn pick(&self, color: Color) {
        self.set_value(color);
        self.0.on_change.emit(self.value());
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.0.native.set_sensitive(enabled);
    }

    /// Called with the new color every time the color changes.
    pub fn on_change(&self, f: impl FnMut(Color) + 'static) {
        self.0.on_change.set(f);
    }

    /// The button itself. Public as a backend-specific escape hatch.
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
