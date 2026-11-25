//! An always-visible month calendar (WinUI 3's `CalendarView`).
//!
//! `CalendarView` is used in its `Single` selection mode with the Gregorian
//! calendar. Three of its behaviors are adjusted so that it matches the other
//! platforms:
//!
//! - In `Single` mode the user can deselect the selected day by clicking it
//!   again, leaving no day selected. naui's calendar always has a value, so the
//!   day is selected again (without notifying).
//! - Without a range, `MinDate` and `MaxDate` default to 100 years before and after
//!   today. naui widens them to everything the Gregorian calendar of
//!   `Windows.Globalization` covers.
//! - `FirstDayOfWeek` defaults to Sunday whatever the locale. naui sets it to the
//!   first day of the week of the user's locale (`LOCALE_IFIRSTDAYOFWEEK`, which
//!   includes the choice in the Region settings) when the calendar is created.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use naui_core::{DatePickerMode, DateTime, Result};
use naui_winui3::Microsoft::UI::Xaml::Controls::{
    CalendarView, CalendarViewSelectedDatesChangedEventArgs, CalendarViewSelectionMode, Control,
};
use naui_winui3::Microsoft::UI::Xaml::UIElement;
use windows::Foundation::{DateTime as WinDateTime, TypedEventHandler};
use windows::Globalization::{CalendarIdentifiers, ClockIdentifiers, DayOfWeek};
use windows::Win32::Globalization::{
    GetLocaleInfoEx, LOCALE_IFIRSTDAYOFWEEK, LOCALE_RETURN_NUMBER,
};
use windows_core::{Interface, PCWSTR};

use crate::date_picker::{from_native_date, gregorian_calendar, local_now};
use crate::to_error;
use crate::ui_thread::UiThreadCell;
use crate::widgets::{impl_widget, Widget};

/// Only the date is picked, as in a `DatePicker` that shows only the date.
const MODE: DatePickerMode = DatePickerMode::Date;

type ChangeCallback = Box<dyn FnMut(DateTime)>;

/// The handler notified when the value changes. The closure is taken out only while
/// it is called, allowing re-entrancy.
#[derive(Clone)]
struct ChangeHandler(Arc<UiThreadCell<Option<ChangeCallback>>>);

impl ChangeHandler {
    fn new() -> Self {
        Self(Arc::new(UiThreadCell::new(None)))
    }

    fn set(&self, f: impl FnMut(DateTime) + 'static) {
        self.0.with_mut(|slot| *slot = Some(Box::new(f)));
    }

    fn emit(&self, value: DateTime) {
        let Some(Some(mut f)) = self.0.try_with_mut(|slot| slot.take()) else {
            return;
        };
        f(value);
        let _ = self.0.try_with_mut(|slot| {
            if slot.is_none() {
                *slot = Some(f);
            }
        });
    }
}

struct CalendarInner {
    native: CalendarView,
    /// The current value, **including the time**, which the calendar does not show.
    value: Cell<DateTime>,
    min: Cell<Option<DateTime>>,
    max: Cell<Option<DateTime>>,
    handler: ChangeHandler,
    /// Ignores `SelectedDatesChanged` only while writing the selection.
    silent: Cell<bool>,
}

/// An always-visible month calendar (WinUI 3's `CalendarView`).
///
/// Its value right after creation is the platform's current date and time (local
/// time).
#[derive(Clone)]
pub struct Calendar(Rc<CalendarInner>);
impl_widget!(Calendar, native);

impl Calendar {
    pub(crate) fn new() -> Result<Self> {
        let native = CalendarView::new().map_err(|e| to_error("create the CalendarView", e))?;
        native
            .SetSelectionMode(CalendarViewSelectionMode::Single)
            .map_err(|e| to_error("set the CalendarView selection mode", e))?;
        let gregorian = CalendarIdentifiers::Gregorian()
            .map_err(|e| to_error("get the Gregorian calendar", e))?;
        native
            .SetCalendarIdentifier(&gregorian)
            .map_err(|e| to_error("set the CalendarView calendar", e))?;
        // `FirstDayOfWeek` の既定はロケールに関わらず日曜日なので、ユーザーの設定を写す。
        if let Some(first) = first_day_of_week(PCWSTR::null(), 0) {
            let _ = native.SetFirstDayOfWeek(first);
        }

        let this = Self(Rc::new(CalendarInner {
            native,
            value: Cell::new(local_now()),
            min: Cell::new(None),
            max: Cell::new(None),
            handler: ChangeHandler::new(),
            silent: Cell::new(false),
        }));
        this.write_native_range();

        let target = Arc::new(UiThreadCell::new(Rc::downgrade(&this.0)));
        let changed =
            TypedEventHandler::<CalendarView, CalendarViewSelectedDatesChangedEventArgs>::new(
                move |_, _| {
                    let _ = target.try_with_mut(|weak| {
                        if let Some(inner) = weak.upgrade() {
                            Calendar(inner).native_changed();
                        }
                    });
                    Ok(())
                },
            );
        this.0
            .native
            .SelectedDatesChanged(&changed)
            .map_err(|e| to_error("subscribe to CalendarView changes", e))?;

        this.write_native(this.value());
        Ok(this)
    }

    /// The selected date. The time part is returned as it was passed.
    pub fn value(&self) -> DateTime {
        self.0.value.get()
    }

    /// Selects a date without notifying, and shows its month. Out-of-range dates are
    /// clamped to the ends, and values that are not valid dates are normalized.
    pub fn set_value(&self, value: DateTime) {
        let value = self.clamp(value);
        self.0.value.set(value);
        self.write_native(value);
    }

    /// Sets the selectable range of days. `None` places no limit on that side. Only
    /// the dates are compared; the times of `min` and `max` are ignored.
    ///
    /// If the current value falls outside the range, it is clamped to the end without
    /// notifying. The calendar shows only the days inside the range
    /// (`MinDate` / `MaxDate`).
    pub fn set_range(&self, min: Option<DateTime>, max: Option<DateTime>) {
        self.0.min.set(min.map(DateTime::normalized));
        self.0.max.set(max.map(DateTime::normalized));
        self.write_native_range();
        self.set_value(self.value());
    }

    pub fn set_enabled(&self, enabled: bool) {
        if let Ok(control) = self.0.native.cast::<Control>() {
            let _ = control.SetIsEnabled(enabled);
        }
    }

    /// Called with the new value when the user selects another day. Setting it again
    /// removes the previous callback.
    pub fn on_change(&self, f: impl FnMut(DateTime) + 'static) {
        self.0.handler.set(f);
    }

    /// Selects the day of `value` the way the user does by clicking it: the
    /// calendar shows its month, the day is selected, and
    /// [`on_change`](Self::on_change) is called with the new value (the time part
    /// stays as it was). A date outside the range is clamped to the end first.
    /// Nothing is called when the day is already selected. Does nothing while
    /// disabled.
    ///
    /// The day is put in `SelectedDates`, and naui notifies before this returns,
    /// whether or not the window has been drawn. For tests and automation.
    pub fn pick(&self, value: DateTime) {
        let enabled = self
            .0
            .native
            .cast::<Control>()
            .and_then(|control| control.IsEnabled())
            .unwrap_or(false);
        if !enabled {
            return;
        }
        let accepted = self.clamp(MODE.apply(self.value(), value));
        self.write_native(accepted);
        self.accept(accepted);
    }

    /// WinUI 3's `CalendarView`. Public as a backend-specific escape hatch.
    pub fn native_calendar_view(&self) -> CalendarView {
        self.0.native.clone()
    }

    /// Handles a change of `SelectedDates`.
    fn native_changed(&self) {
        if self.0.silent.get() {
            return;
        }
        let selected = self
            .0
            .native
            .SelectedDates()
            .ok()
            .and_then(|dates| dates.GetAt(0).ok())
            .and_then(from_native_date);
        let Some(selected) = selected else {
            // 選んでいた日をもう一度押して選択が外れた。naui のカレンダーは
            // 常に日を持つので、通知せずに選び直す。
            self.write_native(self.value());
            return;
        };
        let accepted = self.clamp(MODE.apply(self.value(), selected));
        if MODE.apply(accepted, selected) != accepted {
            self.write_native(accepted);
        }
        self.accept(accepted);
    }

    /// Takes a newly selected value, notifying if it changed.
    fn accept(&self, accepted: DateTime) {
        if accepted == self.value() {
            return;
        }
        self.0.value.set(accepted);
        self.0.handler.emit(accepted);
    }

    fn clamp(&self, value: DateTime) -> DateTime {
        MODE.clamp(value, self.0.min.get(), self.0.max.get())
    }

    /// Selects the day and shows its month. `SelectedDatesChanged` is ignored
    /// meanwhile.
    fn write_native(&self, value: DateTime) {
        let Ok(date) = to_native_day(value, 12) else {
            return;
        };
        let previous = self.0.silent.replace(true);
        if let Ok(dates) = self.0.native.SelectedDates() {
            let _ = dates.Clear();
            let _ = dates.Append(date);
        }
        let _ = self.0.native.SetDisplayDate(date);
        self.0.silent.set(previous);
    }

    /// Passes the range to `MinDate` / `MaxDate`, from 00:00 on the first day to
    /// 23:59 on the last. Without a range, everything the Gregorian calendar covers.
    fn write_native_range(&self) {
        // 西暦の端は `Windows.Globalization` に尋ねる。取れないとき (端の日時が
        // `DateTime` で表せないなど) は、どの時間帯でも表せる日へ落とす。両端は
        // 別々に求め、片方が取れなくても範囲は書く。
        let widest_min =
            calendar_limit(false).or_else(|| to_native_day(DateTime::date(1601, 1, 2), 0).ok());
        let widest_max =
            calendar_limit(true).or_else(|| to_native_day(DateTime::date(9999, 12, 30), 23).ok());
        let min = self
            .0
            .min
            .get()
            .and_then(|min| to_native_day(min, 0).ok())
            .or(widest_min);
        let max = self
            .0
            .max
            .get()
            .and_then(|max| to_native_day(max, 23).ok())
            .or(widest_max);
        // 逆転した範囲は naui の clamp と同じく上限の日で一点にする。
        let min = match (min, max, self.0.max.get()) {
            (Some(lower), Some(upper), Some(last)) if lower.UniversalTime > upper.UniversalTime => {
                to_native_day(last, 0).ok().or(Some(upper))
            }
            _ => min,
        };
        // 範囲の途中で下限が上限を追い越さないよう、いったん最大の範囲へ広げてから
        // 上限、下限の順で狭める。
        let steps = [
            (true, widest_max),
            (false, widest_min),
            (true, max),
            (false, min),
        ];
        for (is_max, date) in steps {
            let Some(date) = date else {
                continue;
            };
            let _ = if is_max {
                self.0.native.SetMaxDate(date)
            } else {
                self.0.native.SetMinDate(date)
            };
        }
    }
}

/// The day of `value` at `hour` o'clock local time (the minute is 0, or 59 for the
/// last hour so that the whole day is covered).
fn to_native_day(value: DateTime, hour: i32) -> windows_core::Result<WinDateTime> {
    day_at(&gregorian_calendar()?, value, hour)
}

/// [`to_native_day`] with the given calendar, whose clock is switched to 24 hours.
fn day_at(
    calendar: &windows::Globalization::Calendar,
    value: DateTime,
    hour: i32,
) -> windows_core::Result<WinDateTime> {
    // 時計は言語で決まり、en-US などの 12 時間制では SetHour が 1〜12 しか
    // 受け付けない (0 時と 23 時が失敗して範囲が書けなくなる)。
    calendar.ChangeClock(&ClockIdentifiers::TwentyFourHour()?)?;
    let value = value.normalized();
    calendar.SetToNow()?;
    calendar.SetDay(1)?;
    calendar.SetYear(value.year)?;
    calendar.SetMonth(value.month as i32)?;
    calendar.SetDay(value.day as i32)?;
    calendar.SetHour(hour)?;
    calendar.SetMinute(if hour == 23 { 59 } else { 0 })?;
    calendar.SetSecond(0)?;
    calendar.SetNanosecond(0)?;
    calendar.GetDateTime()
}

/// The first (`max == false`) or last date the Gregorian calendar of
/// `Windows.Globalization` covers.
fn calendar_limit(max: bool) -> Option<WinDateTime> {
    let calendar = gregorian_calendar().ok()?;
    if max {
        calendar.SetToMax().ok()?;
    } else {
        calendar.SetToMin().ok()?;
    }
    calendar.GetDateTime().ok()
}

/// The first day of the week of `locale` (`LOCALE_IFIRSTDAYOFWEEK`). A null `locale`
/// is the user's locale, including the first day chosen in the Region settings.
/// `None` if it cannot be read.
fn first_day_of_week(locale: PCWSTR, flags: u32) -> Option<DayOfWeek> {
    // LOCALE_RETURN_NUMBER は DWORD を 2 文字ぶんに書く。
    let mut number = [0u16; 2];
    // SAFETY: 手元の配列へ書かせるだけ。
    let written = unsafe {
        GetLocaleInfoEx(
            locale,
            LOCALE_IFIRSTDAYOFWEEK | LOCALE_RETURN_NUMBER | flags,
            Some(&mut number),
        )
    };
    if written == 0 {
        return None;
    }
    day_of_week_from_locale(u32::from(number[0]) | (u32::from(number[1]) << 16))
}

/// `LOCALE_IFIRSTDAYOFWEEK` counts from Monday (0) to Sunday (6), while `DayOfWeek`
/// counts from Sunday (0).
fn day_of_week_from_locale(value: u32) -> Option<DayOfWeek> {
    (value < 7).then(|| DayOfWeek(((value + 1) % 7) as i32))
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Globalization::LOCALE_NOUSEROVERRIDE;
    use windows_core::w;

    #[test]
    fn first_day_of_week_comes_from_the_locale() {
        assert_eq!(day_of_week_from_locale(0), Some(DayOfWeek::Monday));
        assert_eq!(day_of_week_from_locale(5), Some(DayOfWeek::Saturday));
        assert_eq!(day_of_week_from_locale(6), Some(DayOfWeek::Sunday));
        assert_eq!(day_of_week_from_locale(7), None);
        assert_eq!(
            first_day_of_week(w!("fr-FR"), LOCALE_NOUSEROVERRIDE),
            Some(DayOfWeek::Monday)
        );
        assert_eq!(
            first_day_of_week(w!("en-US"), LOCALE_NOUSEROVERRIDE),
            Some(DayOfWeek::Sunday)
        );
        assert!(
            first_day_of_week(PCWSTR::null(), 0).is_some(),
            "ユーザーの設定"
        );
    }

    /// The hour and minute of `date` in a 24-hour Gregorian calendar.
    fn hour_minute(date: WinDateTime) -> (i32, i32) {
        let calendar = gregorian_calendar().unwrap();
        calendar
            .ChangeClock(&ClockIdentifiers::TwentyFourHour().unwrap())
            .unwrap();
        calendar.SetDateTime(date).unwrap();
        (calendar.Hour().unwrap(), calendar.Minute().unwrap())
    }

    #[test]
    fn day_ends_are_set_with_a_12_hour_clock() {
        // en-US などの 12 時間制を言語に関わらず再現する。
        let calendar = gregorian_calendar().unwrap();
        calendar
            .ChangeClock(&ClockIdentifiers::TwelveHour().unwrap())
            .unwrap();
        let day = DateTime::date(2026, 6, 10);
        let start = day_at(&calendar, day, 0).expect("0 時");
        assert_eq!(hour_minute(start), (0, 0));
        assert_eq!(from_native_date(start), Some(day));
        let end = day_at(&calendar, day, 23).expect("23 時");
        assert_eq!(hour_minute(end), (23, 59));
        assert_eq!(from_native_date(end), Some(day));
        let noon = day_at(&calendar, day, 12).expect("12 時");
        assert_eq!(hour_minute(noon), (12, 0));
    }
}
