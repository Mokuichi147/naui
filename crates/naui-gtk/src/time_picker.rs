//! Time picking (a pair of `GtkSpinButton`s).
//!
//! GTK4 has no single control for picking a time. GNOME apps (the alarm settings in
//! Clocks, for example) lay out `GtkSpinButton`s for hours and minutes separated by
//! `:`, so it is built the same way as the time part of `DatePicker`.
//!
//! `GtkSpinButton` has no "time range", so the minimum and maximum are enforced only
//! by naui's clamping. At the ends of hours and minutes (23 and 59), they wrap back
//! to 0 without carrying into the next field.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::glib;
use gtk::prelude::*;
use naui_core::Time;

use crate::bin::SizeBin;
use crate::callback::Notifier;
use crate::date_picker::spin;
use crate::widgets::{impl_widget, without_signal, Widget};

struct TimePickerInner {
    native: gtk::Box,
    bin: SizeBin,
    hour: gtk::SpinButton,
    minute: gtk::SpinButton,
    /// The signal that is blocked while writing the value from code.
    hour_handler: RefCell<Option<glib::SignalHandlerId>>,
    minute_handler: RefCell<Option<glib::SignalHandlerId>>,
    value: Cell<Time>,
    min: Cell<Option<Time>>,
    max: Cell<Option<Time>>,
    on_change: Notifier<Time>,
}

/// A control for picking a time of day (`GtkSpinButton`s for hours and minutes).
///
/// Its value right after creation is the platform's current time (local time).
#[derive(Clone)]
pub struct TimePicker(Rc<TimePickerInner>);
impl_widget!(TimePicker);

impl TimePicker {
    pub(crate) fn new() -> Self {
        let native = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        let bin = SizeBin::wrap(&native);

        let hour = spin(0.0, 23.0);
        let minute = spin(0.0, 59.0);
        native.append(&hour);
        native.append(&gtk::Label::new(Some(":")));
        native.append(&minute);

        let this = Self(Rc::new(TimePickerInner {
            native,
            bin,
            hour,
            minute,
            hour_handler: RefCell::new(None),
            minute_handler: RefCell::new(None),
            value: Cell::new(now()),
            min: Cell::new(None),
            max: Cell::new(None),
            on_change: Notifier::default(),
        }));
        this.write_native(this.value());

        // どちらのスピンボタンも「表示されている値が変わった」を伝えるだけ。
        // 値の組み立ては read_native がまとめて行う。
        for (spin, slot) in [
            (&this.0.hour, &this.0.hour_handler),
            (&this.0.minute, &this.0.minute_handler),
        ] {
            let handler = {
                let weak = Rc::downgrade(&this.0);
                spin.connect_value_changed(move |_| {
                    if let Some(inner) = weak.upgrade() {
                        TimePicker(inner).native_changed();
                    }
                })
            };
            *slot.borrow_mut() = Some(handler);
        }
        this
    }

    /// The currently selected time.
    pub fn value(&self) -> Time {
        self.0.value.get()
    }

    /// Changes the value without notifying. Out-of-range values are clamped to the
    /// ends, and values that are not valid times are normalized.
    pub fn set_value(&self, value: Time) {
        let value = self.clamp(value);
        self.0.value.set(value);
        self.write_native(value);
    }

    /// Sets the selectable range. `None` places no limit on that side.
    ///
    /// If the current value falls outside the range, it is clamped to the end without
    /// notifying.
    pub fn set_range(&self, min: Option<Time>, max: Option<Time>) {
        self.0.min.set(min.map(Time::normalized));
        self.0.max.set(max.map(Time::normalized));
        self.set_value(self.value());
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.0.native.set_sensitive(enabled);
    }

    /// Called with the new value when the value changes. Setting it again removes the
    /// previous callback.
    pub fn on_change(&self, f: impl FnMut(Time) + 'static) {
        self.0.on_change.set(f);
    }

    /// The `GtkSpinButton`s for hours and minutes. Public as a backend-specific escape
    /// hatch.
    pub fn native_spins(&self) -> (gtk::SpinButton, gtk::SpinButton) {
        (self.0.hour.clone(), self.0.minute.clone())
    }

    /// Handles a change in what either spin button shows.
    fn native_changed(&self) {
        let accepted = self.clamp(self.read_native());
        // 丸めや範囲で押し戻したときのために、表示は必ず書き直す
        // (同じ値なら GTK 側は何もしない)。
        self.write_native(accepted);
        if accepted == self.value() {
            return;
        }
        self.0.value.set(accepted);
        self.0.on_change.emit(accepted);
    }

    /// The hour and minute currently on screen.
    fn read_native(&self) -> Time {
        Time::new(self.0.hour.value() as u8, self.0.minute.value() as u8).normalized()
    }

    fn clamp(&self, value: Time) -> Time {
        value.clamped(self.0.min.get(), self.0.max.get())
    }

    /// Writes the value into the two spin buttons. Signals are blocked meanwhile.
    fn write_native(&self, value: Time) {
        without_signal(&self.0.hour, &self.0.hour_handler, || {
            self.0.hour.set_value(value.hour as f64);
        });
        without_signal(&self.0.minute, &self.0.minute_handler, || {
            self.0.minute.set_value(value.minute as f64);
        });
    }
}

/// The current time on the GTK side (local time).
fn now() -> Time {
    let Ok(now) = glib::DateTime::now_local() else {
        return Time::MIDNIGHT;
    };
    Time::new(now.hour() as u8, now.minute() as u8).normalized()
}
