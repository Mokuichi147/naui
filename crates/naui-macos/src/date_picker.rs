//! Date and time picking (`NSDatePicker`).
//!
//! AppKit has a dedicated date control, so it is enough to decide the fields to
//! show (`datePickerElements`) from [`DatePickerMode`].
//!
//! The calendar is **fixed to Gregorian**. `NSDatePicker` uses the locale's
//! calendar by default, so in a Japanese-calendar locale `year` would mean the 8 of
//! "Reiwa 8", and the meaning of [`DateTime::year`] would vary by environment.
//! Month names and field order follow the locale, so the display language does not
//! change.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use naui_core::{DatePickerMode, DateTime};
use objc2::rc::Retained;
use objc2::{sel, AnyThread, MainThreadMarker, Message};
use objc2_app_kit::{NSDatePicker, NSDatePickerElementFlags, NSDatePickerStyle, NSView};
use objc2_foundation::{
    NSCalendar, NSCalendarIdentifierGregorian, NSCalendarUnit, NSDate, NSDateComponents,
};

use crate::trampoline::{ActionTarget, ValueHandler};
use crate::widgets::{impl_widget, Widget};

struct DatePickerInner {
    native: Retained<NSDatePicker>,
    mode: DatePickerMode,
    /// The calendar used for conversion. The same object as the one passed to the
    /// picker.
    calendar: Retained<NSCalendar>,
    /// The current value, **including the parts that cannot be picked**.
    /// `NSDatePicker` also keeps those parts inside its `NSDate`, but naui keeps them
    /// too so that all four backends return the same answer.
    value: Cell<DateTime>,
    min: Cell<Option<DateTime>>,
    max: Cell<Option<DateTime>>,
    handler: ValueHandler<DateTime>,
    /// Ignores notifications from the native side only while writing the value.
    silent: Cell<bool>,
    /// The target that receives changes. AppKit targets are weak, so it is kept alive.
    target: RefCell<Option<Retained<ActionTarget>>>,
}

/// A control for picking a date and time (`NSDatePicker`).
///
/// Its value right after creation is the platform's current date and time (local
/// time).
#[derive(Clone)]
pub struct DatePicker(Rc<DatePickerInner>);
impl_widget!(DatePicker);

impl DatePicker {
    pub(crate) fn new(mtm: MainThreadMarker, mode: DatePickerMode) -> Self {
        let native = NSDatePicker::new(mtm);
        native.setDatePickerStyle(NSDatePickerStyle::TextFieldAndStepper);
        native.setDatePickerElements(elements(mode));
        // 日付を触るときはカレンダーを重ねて出す。GTK のポップオーバーや
        // ブラウザのカレンダーと同じく、**押せば暦が見える**ようにするため。
        // 場所を取らずに済むよう、常時表示の ClockAndCalendar 形式ではなく
        // 編集中だけ出るオーバーレイを使う。時刻だけの表示には暦が無いので
        // 付けない。
        native.setPresentsCalendarOverlay(mode.has_date());
        let calendar = gregorian_calendar();
        native.setCalendar(Some(&calendar));

        let now = now_in(&calendar);
        let this = Self(Rc::new(DatePickerInner {
            native,
            mode,
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
                DatePicker(inner).native_changed();
            }
        });
        unsafe {
            this.0.native.setTarget(Some(&target));
            this.0.native.setAction(Some(sel!(invoke:)));
        }
        *this.0.target.borrow_mut() = Some(target);
        this
    }

    /// What it lets the user pick. Fixed at creation and never changes.
    pub fn mode(&self) -> DatePickerMode {
        self.0.mode
    }

    /// The current value. Parts that cannot be picked are returned as they were passed.
    pub fn value(&self) -> DateTime {
        self.0.value.get()
    }

    /// Changes the value without notifying. Out-of-range values are clamped to the
    /// ends, and values that are not valid dates are normalized.
    pub fn set_value(&self, value: DateTime) {
        let value = self.clamp(value);
        self.0.value.set(value);
        self.write_native(value);
    }

    /// Sets the selectable range. `None` places no limit on that side.
    ///
    /// If the current value falls outside the range, it is clamped to the end without
    /// notifying.
    pub fn set_range(&self, min: Option<DateTime>, max: Option<DateTime>) {
        self.0.min.set(min.map(DateTime::normalized));
        self.0.max.set(max.map(DateTime::normalized));
        self.write_native_range();
        self.set_value(self.value());
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.0.native.setEnabled(enabled);
    }

    /// Called with the new value when the value changes. Setting it again removes the
    /// previous callback.
    pub fn on_change(&self, f: impl FnMut(DateTime) + 'static) {
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
        let accepted = self.clamp(self.0.mode.apply(self.value(), shown));
        // 選ばせている部分が表示と食い違うなら (範囲で押し戻したとき)、
        // 表示のほうを直す。`apply` で値が動く = その部分が違う、という意味。
        if self.0.mode.apply(accepted, shown) != accepted {
            self.write_native(accepted);
        }
        if accepted == self.value() {
            return;
        }
        self.0.value.set(accepted);
        self.0.handler.emit(accepted);
    }

    fn clamp(&self, value: DateTime) -> DateTime {
        self.0.mode.clamp(value, self.0.min.get(), self.0.max.get())
    }

    /// Writes the value to the native side. Notifications during this are ignored.
    fn write_native(&self, value: DateTime) {
        let Some(date) = to_ns_date(&self.0.calendar, value) else {
            return;
        };
        let previous = self.0.silent.replace(true);
        self.0.native.setDateValue(&date);
        self.0.silent.set(previous);
    }

    /// Passes the minimum and maximum to the native side.
    ///
    /// When only the date can be picked, the minimum is set to 00:00 and the maximum to
    /// 23:59. `NSDatePicker` compares `NSDate`s, so without this a value on the same day
    /// would be rejected just because its time is earlier.
    ///
    /// When only the time can be picked, the date part means nothing, so the bounds are
    /// not passed to the native side (the range is enforced only by naui's clamping).
    fn write_native_range(&self) {
        if self.0.mode == DatePickerMode::Time {
            self.0.native.setMinDate(None);
            self.0.native.setMaxDate(None);
            return;
        }
        let min = self.0.min.get().map(|min| match self.0.mode {
            DatePickerMode::Date => DateTime::date(min.year, min.month, min.day),
            _ => min,
        });
        let max = self.0.max.get().map(|max| match self.0.mode {
            DatePickerMode::Date => max.with_time(23, 59),
            _ => max,
        });
        let calendar = &self.0.calendar;
        self.0
            .native
            .setMinDate(min.and_then(|v| to_ns_date(calendar, v)).as_deref());
        self.0
            .native
            .setMaxDate(max.and_then(|v| to_ns_date(calendar, v)).as_deref());
    }

    fn read_native(&self) -> DateTime {
        from_ns_date(&self.0.calendar, &self.0.native.dateValue())
    }
}

/// Decides the fields to show from [`DatePickerMode`].
fn elements(mode: DatePickerMode) -> NSDatePickerElementFlags {
    let mut flags = NSDatePickerElementFlags::empty();
    if mode.has_date() {
        flags |= NSDatePickerElementFlags::YearMonthDay;
    }
    if mode.has_time() {
        flags |= NSDatePickerElementFlags::HourMinute;
    }
    flags
}

/// The Gregorian calendar. The time zone stays the default (the system's current
/// time zone).
fn gregorian_calendar() -> Retained<NSCalendar> {
    let identifier = unsafe { NSCalendarIdentifierGregorian };
    NSCalendar::initWithCalendarIdentifier(NSCalendar::alloc(), identifier)
        .unwrap_or_else(NSCalendar::currentCalendar)
}

/// The current date and time in that calendar and time zone.
fn now_in(calendar: &NSCalendar) -> DateTime {
    from_ns_date(calendar, &NSDate::now())
}

fn to_ns_date(calendar: &NSCalendar, value: DateTime) -> Option<Retained<NSDate>> {
    let value = value.normalized();
    let components = NSDateComponents::new();
    components.setYear(value.year as isize);
    components.setMonth(value.month as isize);
    components.setDay(value.day as isize);
    components.setHour(value.hour as isize);
    components.setMinute(value.minute as isize);
    components.setSecond(0);
    calendar.dateFromComponents(&components)
}

fn from_ns_date(calendar: &NSCalendar, date: &NSDate) -> DateTime {
    let units = NSCalendarUnit::Year
        | NSCalendarUnit::Month
        | NSCalendarUnit::Day
        | NSCalendarUnit::Hour
        | NSCalendarUnit::Minute;
    let c = calendar.components_fromDate(units, date);
    DateTime {
        year: c.year() as i32,
        month: c.month().clamp(1, 12) as u8,
        day: c.day().clamp(1, 31) as u8,
        hour: c.hour().clamp(0, 23) as u8,
        minute: c.minute().clamp(0, 59) as u8,
    }
    .normalized()
}
