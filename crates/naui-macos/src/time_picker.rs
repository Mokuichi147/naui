//! Time picking (an `NSDatePicker` showing only hours and minutes).
//!
//! AppKit has no dedicated time control; the convention for time input is to limit
//! `NSDatePicker`'s `datePickerElements` to `HourMinute`. Whether it is 12- or
//! 24-hour and where `AM` / `PM` goes follow the locale.
//!
//! The calendar is **fixed to Gregorian** (for the same reason as `DatePicker`).
//! The date part is not shown, so it is fixed to the day of
//! [`DateTime::TIME_ORIGIN`] and only hours and minutes are exchanged.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use naui_core::{DateTime, Time};
use objc2::rc::Retained;
use objc2::{sel, AnyThread, MainThreadMarker, Message};
use objc2_app_kit::{NSDatePicker, NSDatePickerElementFlags, NSDatePickerStyle, NSView};
use objc2_foundation::{
    NSCalendar, NSCalendarIdentifierGregorian, NSCalendarUnit, NSDate, NSDateComponents,
};

use crate::trampoline::{ActionTarget, ValueHandler};
use crate::widgets::{impl_widget, Widget};

struct TimePickerInner {
    native: Retained<NSDatePicker>,
    /// The calendar used for conversion. The same object as the one passed to the
    /// picker.
    calendar: Retained<NSCalendar>,
    value: Cell<Time>,
    min: Cell<Option<Time>>,
    max: Cell<Option<Time>>,
    handler: ValueHandler<Time>,
    /// Ignores notifications from the native side only while writing the value.
    silent: Cell<bool>,
    /// The target that receives changes. AppKit targets are weak, so it is kept alive.
    target: RefCell<Option<Retained<ActionTarget>>>,
}

/// A control for picking a time of day (the hours and minutes of an
/// `NSDatePicker`).
///
/// Its value right after creation is the platform's current time (local time).
#[derive(Clone)]
pub struct TimePicker(Rc<TimePickerInner>);
impl_widget!(TimePicker);

impl TimePicker {
    pub(crate) fn new(mtm: MainThreadMarker) -> Self {
        let native = NSDatePicker::new(mtm);
        native.setDatePickerStyle(NSDatePickerStyle::TextFieldAndStepper);
        native.setDatePickerElements(NSDatePickerElementFlags::HourMinute);
        // 時刻だけの表示に暦は要らない。
        native.setPresentsCalendarOverlay(false);
        let calendar = gregorian_calendar();
        native.setCalendar(Some(&calendar));

        let now = now_in(&calendar);
        let this = Self(Rc::new(TimePickerInner {
            native,
            calendar,
            value: Cell::new(now),
            min: Cell::new(None),
            max: Cell::new(None),
            handler: ValueHandler::default(),
            silent: Cell::new(false),
            target: RefCell::new(None),
        }));
        this.write_native(now);

        // `NSDatePicker` は NSControl なので、変更は target/action で届く。
        let target = ActionTarget::new(mtm, {
            let weak = Rc::downgrade(&this.0);
            move || {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                TimePicker(inner).native_changed();
            }
        });
        unsafe {
            this.0.native.setTarget(Some(&target));
            this.0.native.setAction(Some(sel!(invoke:)));
        }
        *this.0.target.borrow_mut() = Some(target);
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
        self.write_native_range();
        self.set_value(self.value());
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.0.native.setEnabled(enabled);
    }

    /// Called with the new value when the value changes. Setting it again removes the
    /// previous callback.
    pub fn on_change(&self, f: impl FnMut(Time) + 'static) {
        self.0.handler.set(f);
    }

    /// The real AppKit control. Public as a backend-specific escape hatch.
    pub fn native_picker(&self) -> Retained<NSDatePicker> {
        self.0.native.clone()
    }

    /// Handles a value change on the native side.
    fn native_changed(&self) {
        if self.0.silent.get() {
            return;
        }
        let shown = self.read_native();
        let accepted = self.clamp(shown);
        if accepted != shown {
            // 範囲で押し戻したときは表示のほうを直す。
            self.write_native(accepted);
        }
        if accepted == self.value() {
            return;
        }
        self.0.value.set(accepted);
        self.0.handler.emit(accepted);
    }

    fn clamp(&self, value: Time) -> Time {
        value.clamped(self.0.min.get(), self.0.max.get())
    }

    /// Writes the value to the native side. Notifications during this are ignored.
    fn write_native(&self, value: Time) {
        let Some(date) = to_ns_date(&self.0.calendar, value) else {
            return;
        };
        let previous = self.0.silent.replace(true);
        self.0.native.setDateValue(&date);
        self.0.silent.set(previous);
    }

    /// Passes the minimum and maximum to the native side.
    ///
    /// The date is fixed to [`DateTime::TIME_ORIGIN`], so comparing `NSDate`s is
    /// directly a comparison of times (unlike the time mode of `DatePicker`, which also
    /// picks dates, here the native side can hold the range too). When the minimum is
    /// after the maximum, both are set to the maximum, the same as naui's clamping.
    fn write_native_range(&self) {
        let (min, max) = (self.0.min.get(), self.0.max.get());
        let min = match (min, max) {
            (Some(min), Some(max)) if min > max => Some(max),
            (min, _) => min,
        };
        let calendar = &self.0.calendar;
        self.0
            .native
            .setMinDate(min.and_then(|v| to_ns_date(calendar, v)).as_deref());
        self.0
            .native
            .setMaxDate(max.and_then(|v| to_ns_date(calendar, v)).as_deref());
    }

    fn read_native(&self) -> Time {
        from_ns_date(&self.0.calendar, &self.0.native.dateValue())
    }
}

/// The Gregorian calendar. The time zone stays the default (the system's current
/// time zone).
fn gregorian_calendar() -> Retained<NSCalendar> {
    let identifier = unsafe { NSCalendarIdentifierGregorian };
    NSCalendar::initWithCalendarIdentifier(NSCalendar::alloc(), identifier)
        .unwrap_or_else(NSCalendar::currentCalendar)
}

/// The current time in that calendar and time zone.
fn now_in(calendar: &NSCalendar) -> Time {
    from_ns_date(calendar, &NSDate::now())
}

fn to_ns_date(calendar: &NSCalendar, value: Time) -> Option<Retained<NSDate>> {
    let value = value.normalized();
    let (year, month, day) = DateTime::TIME_ORIGIN;
    let components = NSDateComponents::new();
    components.setYear(year as isize);
    components.setMonth(month as isize);
    components.setDay(day as isize);
    components.setHour(value.hour as isize);
    components.setMinute(value.minute as isize);
    components.setSecond(0);
    calendar.dateFromComponents(&components)
}

fn from_ns_date(calendar: &NSCalendar, date: &NSDate) -> Time {
    let units = NSCalendarUnit::Hour | NSCalendarUnit::Minute;
    let c = calendar.components_fromDate(units, date);
    Time::new(c.hour().clamp(0, 23) as u8, c.minute().clamp(0, 59) as u8)
}
