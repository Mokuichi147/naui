//! An on/off switch (`GtkSwitch` + a `GtkLabel` for the label).
//!
//! `GtkSwitch` has only the on/off part and no text (like the switches in GNOME
//! Settings). The label is added as a `GtkLabel` placed next to it.

use std::cell::RefCell;
use std::rc::Rc;

use gtk::glib;
use gtk::prelude::*;

use crate::bin::SizeBin;
use crate::callback::Notifier;
use crate::widgets::{impl_widget, without_signal, Widget};

/// The gap between the switch and the label (logical pixels).
const SPACING: i32 = 8;

struct ToggleInner {
    native: gtk::Box,
    switch: gtk::Switch,
    bin: SizeBin,
    on_toggle: Notifier<bool>,
    handler: RefCell<Option<glib::SignalHandlerId>>,
}

/// An on/off switch (`GtkSwitch`).
#[derive(Clone)]
pub struct Toggle(Rc<ToggleInner>);
impl_widget!(Toggle);

impl Toggle {
    pub(crate) fn new(label: &str) -> Self {
        let native = gtk::Box::new(gtk::Orientation::Horizontal, SPACING);

        let switch = gtk::Switch::new();
        // スイッチは自分の大きさのままでいる (行いっぱいに伸びない)。
        switch.set_valign(gtk::Align::Center);
        switch.set_halign(gtk::Align::Start);

        let text = gtk::Label::new(Some(label));
        text.set_xalign(0.0);

        native.append(&switch);
        native.append(&text);
        let bin = SizeBin::wrap(&native);

        let inner = Rc::new(ToggleInner {
            native,
            switch,
            bin,
            on_toggle: Notifier::default(),
            handler: RefCell::new(None),
        });
        let id = {
            let weak = Rc::downgrade(&inner);
            inner.switch.connect_active_notify(move |switch| {
                if let Some(inner) = weak.upgrade() {
                    inner.on_toggle.emit(switch.is_active());
                }
            })
        };
        *inner.handler.borrow_mut() = Some(id);
        Self(inner)
    }

    /// Whether it is on.
    pub fn is_on(&self) -> bool {
        self.0.switch.is_active()
    }

    /// Switches it from code. `on_toggle` is not called.
    pub fn set_on(&self, on: bool) {
        without_signal(&self.0.switch, &self.0.handler, || {
            self.0.switch.set_active(on);
        });
    }

    pub fn set_enabled(&self, enabled: bool) {
        // 箱ごと切ると、スイッチもラベルも同じ見た目になる。
        self.0.native.set_sensitive(enabled);
    }

    /// Called with the new state every time the user switches it.
    pub fn on_toggle(&self, f: impl FnMut(bool) + 'static) {
        self.0.on_toggle.set(f);
    }

    /// The switch itself. Public as a backend-specific escape hatch.
    pub fn native_switch(&self) -> gtk::Switch {
        self.0.switch.clone()
    }
}
