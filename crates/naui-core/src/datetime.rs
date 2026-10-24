//! Date and time values, and the display kinds for date selection.
//!
//! naui does no date arithmetic. All that is here is "a container for year, month,
//! day, hour and minute" ([`DateTime`]), "a container for just hour and minute"
//! ([`Time`]), and the minimal conversions backends need to pass values back and
//! forth. There is no calendar arithmetic (weekdays, addition and subtraction, time
//! zone conversion). The actual input is done by platform controls such as
//! `NSDatePicker` and `<input type="date">`.

use std::fmt;

/// Year, month, day, hour and minute. **No seconds.**
///
/// The part common to all four platforms has no control that lets you pick seconds
/// (`<input type="time">` stops at minutes by default, and WinUI 3's `TimePicker`
/// has no seconds either), so naui handles values down to the minute.
///
/// The ordering matches calendar order ([`Ord`] compares year → month → day →
/// hour → minute).
///
/// ```
/// # use naui_core::DateTime;
/// let morning = DateTime::new(2026, 8, 22, 9, 30);
/// assert!(DateTime::date(2026, 8, 22) < morning);
/// assert_eq!(morning.to_string(), "2026-08-22 09:30");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DateTime {
    /// The year (Common Era). [`normalized`](Self::normalized) keeps it within 1..=9999.
    pub year: i32,
    /// The month (1..=12).
    pub month: u8,
    /// The day (1..=the number of days in the month).
    pub day: u8,
    /// The hour (0..=23).
    pub hour: u8,
    /// The minute (0..=59).
    pub minute: u8,
}

impl DateTime {
    /// The date used by [`DateTime::time`]. Values that mean only a time of day share
    /// this year, month and day.
    pub const TIME_ORIGIN: (i32, u8, u8) = (1970, 1, 1);

    pub const fn new(year: i32, month: u8, day: u8, hour: u8, minute: u8) -> Self {
        Self {
            year,
            month,
            day,
            hour,
            minute,
        }
    }

    /// Creates a date at 00:00.
    pub const fn date(year: i32, month: u8, day: u8) -> Self {
        Self::new(year, month, day, 0, 0)
    }

    /// Creates a value that represents only a time of day. The date is
    /// [`TIME_ORIGIN`](Self::TIME_ORIGIN).
    ///
    /// The [`DatePickerMode::Time`] display ignores the date part, so use this when
    /// passing lower and upper bounds for a time.
    pub const fn time(hour: u8, minute: u8) -> Self {
        let (year, month, day) = Self::TIME_ORIGIN;
        Self::new(year, month, day, hour, minute)
    }

    /// Whether the value is a valid calendar date (February 29 in leap years is taken
    /// into account).
    pub fn is_valid(self) -> bool {
        (1..=9999).contains(&self.year)
            && (1..=12).contains(&self.month)
            && (1..=days_in_month(self.year, self.month)).contains(&self.day)
            && self.hour <= 23
            && self.minute <= 59
    }

    /// Clamps the value into a valid calendar date.
    ///
    /// Each field is only clamped into its own range; nothing carries over.
    /// "November 31" becomes **November 30**, not December 1. A date field that stops
    /// at the end of the month is less surprising than one whose value jumps into the
    /// next month.
    ///
    /// ```
    /// # use naui_core::DateTime;
    /// assert_eq!(
    ///     DateTime::date(2026, 11, 31).normalized(),
    ///     DateTime::date(2026, 11, 30)
    /// );
    /// assert_eq!(
    ///     DateTime::date(2025, 2, 29).normalized(),
    ///     DateTime::date(2025, 2, 28) // 2025 is not a leap year
    /// );
    /// ```
    pub fn normalized(self) -> Self {
        let year = self.year.clamp(1, 9999);
        let month = self.month.clamp(1, 12);
        Self {
            year,
            month,
            day: self.day.clamp(1, days_in_month(year, month)),
            hour: self.hour.min(23),
            minute: self.minute.min(59),
        }
    }

    /// Replaces only the date, keeping the time (the result is clamped).
    pub fn with_date(self, year: i32, month: u8, day: u8) -> Self {
        Self {
            year,
            month,
            day,
            ..self
        }
        .normalized()
    }

    /// Replaces only the time, keeping the date (the result is clamped).
    pub fn with_time(self, hour: u8, minute: u8) -> Self {
        Self {
            hour,
            minute,
            ..self
        }
        .normalized()
    }

    /// Takes out just the time part as a [`Time`] (the result is clamped).
    ///
    /// Use [`with_time`](Self::with_time) for the opposite direction.
    ///
    /// ```
    /// # use naui_core::{DateTime, Time};
    /// let value = DateTime::new(2026, 8, 22, 9, 30);
    /// let time = value.time_of_day();
    /// assert_eq!(time, Time::new(9, 30));
    /// assert_eq!(value.with_time(time.hour, time.minute), value);
    /// ```
    pub fn time_of_day(self) -> Time {
        Time::new(self.hour, self.minute).normalized()
    }
}

impl Default for DateTime {
    /// 00:00 on [`TIME_ORIGIN`](DateTime::TIME_ORIGIN).
    fn default() -> Self {
        let (year, month, day) = Self::TIME_ORIGIN;
        Self::date(year, month, day)
    }
}

impl fmt::Display for DateTime {
    /// The form `2026-08-22 09:30`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:04}-{:02}-{:02} {:02}:{:02}",
            self.year, self.month, self.day, self.hour, self.minute
        )
    }
}

/// Hour and minute. **No seconds** (for the same reason as [`DateTime`]).
///
/// The value of a control that picks only a time of day (`TimePicker`). It has no
/// date, so use it for times detached from the calendar, such as "7:30 every
/// morning". Use [`DateTime`] when you want it paired with a date.
///
/// The ordering matches clock order ([`Ord`] compares hour → minute).
///
/// ```
/// # use naui_core::Time;
/// let alarm = Time::new(7, 30);
/// assert!(Time::MIDNIGHT < alarm);
/// assert_eq!(alarm.to_string(), "07:30");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Time {
    /// The hour (0..=23).
    pub hour: u8,
    /// The minute (0..=59).
    pub minute: u8,
}

impl Time {
    /// 00:00. Also the [`Default`].
    pub const MIDNIGHT: Time = Time::new(0, 0);
    /// Minutes in a day. The maximum of
    /// [`minutes_since_midnight`](Self::minutes_since_midnight) (= 23:59) is one less
    /// than this.
    pub const MINUTES_PER_DAY: u16 = 24 * 60;

    pub const fn new(hour: u8, minute: u8) -> Self {
        Self { hour, minute }
    }

    /// Whether the value is a valid time of day.
    pub fn is_valid(self) -> bool {
        self.hour <= 23 && self.minute <= 59
    }

    /// Clamps the value into a valid time of day.
    ///
    /// Each field is only clamped into its own range; nothing carries over (the same
    /// rule as [`DateTime::normalized`]). "25:70" becomes **23:59**, not 02:10 the
    /// next day.
    ///
    /// ```
    /// # use naui_core::Time;
    /// assert_eq!(Time::new(25, 70).normalized(), Time::new(23, 59));
    /// ```
    pub fn normalized(self) -> Self {
        Self {
            hour: self.hour.min(23),
            minute: self.minute.min(59),
        }
    }

    /// Clamps the value between the lower and upper bounds. Also normalizes it
    /// ([`normalized`](Self::normalized)).
    ///
    /// When the lower bound is after the upper bound, the upper bound, applied last,
    /// wins (the same rule as [`DatePickerMode::clamp`]).
    ///
    /// ```
    /// # use naui_core::Time;
    /// let (min, max) = (Time::new(9, 0), Time::new(18, 0));
    /// assert_eq!(Time::new(7, 30).clamped(Some(min), Some(max)), min);
    /// assert_eq!(Time::new(12, 0).clamped(Some(min), Some(max)), Time::new(12, 0));
    /// ```
    pub fn clamped(self, min: Option<Time>, max: Option<Time>) -> Time {
        let mut out = self.normalized();
        if let Some(min) = min {
            let min = min.normalized();
            if out < min {
                out = min;
            }
        }
        if let Some(max) = max {
            let max = max.normalized();
            if out > max {
                out = max;
            }
        }
        out
    }

    /// Parses the form `09:30`.
    ///
    /// `<input type="time">` sometimes returns seconds, so the form `09:30:00` is also
    /// accepted (the seconds are dropped). Strings of any other form (including the
    /// empty string) give `None`.
    ///
    /// ```
    /// # use naui_core::Time;
    /// assert_eq!(Time::parse("09:30"), Some(Time::new(9, 30)));
    /// assert_eq!(Time::parse("09:30:00"), Some(Time::new(9, 30)));
    /// assert_eq!(Time::parse("9"), None);
    /// ```
    pub fn parse(text: &str) -> Option<Time> {
        let (hour, minute) = parse_time(text)?;
        Some(Time::new(hour, minute).normalized())
    }

    /// Minutes counted from 00:00 (0..=1439). Normalizes before counting.
    ///
    /// Used by backends that want to pass the time as a single number, such as WinUI 3's
    /// `TimePicker`, which holds its value as a `TimeSpan`.
    pub fn minutes_since_midnight(self) -> u16 {
        let v = self.normalized();
        u16::from(v.hour) * 60 + u16::from(v.minute)
    }

    /// The inverse of [`minutes_since_midnight`](Self::minutes_since_midnight).
    ///
    /// Minutes outside a day are clamped to the ends (00:00 when negative, 23:59 at 24
    /// hours or more).
    ///
    /// ```
    /// # use naui_core::Time;
    /// assert_eq!(Time::from_minutes_since_midnight(570), Time::new(9, 30));
    /// assert_eq!(Time::from_minutes_since_midnight(-1), Time::MIDNIGHT);
    /// ```
    pub fn from_minutes_since_midnight(minutes: i64) -> Time {
        let minutes = minutes.clamp(0, i64::from(Self::MINUTES_PER_DAY) - 1);
        Time::new((minutes / 60) as u8, (minutes % 60) as u8)
    }
}

impl fmt::Display for Time {
    /// The form `09:30`. This is the same form as the `value` of `<input type="time">`,
    /// so the Web backend can use it directly as the value and in the `min` / `max`
    /// attributes.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:02}:{:02}", self.hour, self.minute)
    }
}

/// The number of days in the month (leap years included).
pub fn days_in_month(year: i32, month: u8) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        // 範囲外の月は 1 か月ぶんの最小値として扱う。normalized が月を
        // 先に丸めるので、ここへ来るのは月を丸めずに問い合わせた場合だけ。
        _ => 28,
    }
}

/// Whether the year is a leap year (Gregorian calendar).
pub fn is_leap_year(year: i32) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

/// What a date picker lets the user pick.
///
/// Decided at creation and cannot be changed later, because native controls fix
/// the fields they show when they are created (`datePickerElements` of
/// `NSDatePicker`, the `type` of `<input>` and so on).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum DatePickerMode {
    /// Year, month and day only. The time part is left untouched.
    #[default]
    Date,
    /// Hour and minute only. The date part is left untouched.
    Time,
    /// Both year/month/day and hour/minute.
    DateTime,
}

impl DatePickerMode {
    /// Whether the year, month and day can be picked.
    pub fn has_date(self) -> bool {
        matches!(self, DatePickerMode::Date | DatePickerMode::DateTime)
    }

    /// Whether the hour and minute can be picked.
    pub fn has_time(self) -> bool {
        matches!(self, DatePickerMode::Time | DatePickerMode::DateTime)
    }

    /// Clamps the value between the lower and upper bounds. Also normalizes it
    /// ([`DateTime::normalized`]).
    ///
    /// Only **the parts this display lets the user pick** are compared: the time is
    /// ignored for [`Date`](Self::Date), and the date for [`Time`](Self::Time). Parts
    /// that cannot be picked are not rewritten even when clamping to a bound.
    ///
    /// When the lower bound is after the upper bound, the upper bound, applied last,
    /// wins.
    ///
    /// ```
    /// # use naui_core::{DatePickerMode, DateTime};
    /// let min = DateTime::date(2026, 8, 1);
    /// let value = DateTime::new(2026, 7, 20, 9, 30);
    /// assert_eq!(
    ///     DatePickerMode::Date.clamp(value, Some(min), None),
    ///     DateTime::new(2026, 8, 1, 9, 30) // the time does not move
    /// );
    /// ```
    pub fn clamp(self, value: DateTime, min: Option<DateTime>, max: Option<DateTime>) -> DateTime {
        let mut out = value.normalized();
        if let Some(min) = min {
            let min = min.normalized();
            if self.key(out) < self.key(min) {
                out = self.apply(out, min);
            }
        }
        if let Some(max) = max {
            let max = max.normalized();
            if self.key(out) > self.key(max) {
                out = self.apply(out, max);
            }
        }
        out
    }

    /// Writes the value in the form HTML's `<input>` uses.
    ///
    /// [`Date`](Self::Date) gives `2026-08-22`, [`Time`](Self::Time) gives `09:30` and
    /// [`DateTime`](Self::DateTime) gives `2026-08-22T09:30`. The `min` / `max`
    /// attributes use the same form, so this also works for specifying a range.
    pub fn format(self, value: DateTime) -> String {
        let v = value.normalized();
        match self {
            DatePickerMode::Date => format!("{:04}-{:02}-{:02}", v.year, v.month, v.day),
            DatePickerMode::Time => format!("{:02}:{:02}", v.hour, v.minute),
            DatePickerMode::DateTime => format!(
                "{:04}-{:02}-{:02}T{:02}:{:02}",
                v.year, v.month, v.day, v.hour, v.minute
            ),
        }
    }

    /// The inverse of [`format`](Self::format). Overlays only the parts it could read
    /// onto `base` and returns the result.
    ///
    /// Parts that are not picked keep the values from `base`. This keeps the original
    /// date when reading back from a control without a date, such as
    /// `<input type="time">`. Returns `None` for strings of any other form (including
    /// the empty string).
    pub fn parse(self, text: &str, base: DateTime) -> Option<DateTime> {
        match self {
            DatePickerMode::Date => {
                let (year, month, day) = parse_date(text)?;
                Some(base.with_date(year, month, day))
            }
            DatePickerMode::Time => {
                let (hour, minute) = parse_time(text)?;
                Some(base.with_time(hour, minute))
            }
            DatePickerMode::DateTime => {
                let (date, time) = text.split_once('T').or_else(|| text.split_once(' '))?;
                let (year, month, day) = parse_date(date)?;
                let (hour, minute) = parse_time(time)?;
                Some(
                    DateTime {
                        year,
                        month,
                        day,
                        hour,
                        minute,
                    }
                    .normalized(),
                )
            }
        }
    }

    /// Takes **only the parts that are picked** from `edited` and leaves the rest as in
    /// `base`.
    ///
    /// Used when reading back from a native control. A date-only display keeps the
    /// time, and a time-only display keeps the date, at their original values even
    /// though the control does not hold them.
    ///
    /// ```
    /// # use naui_core::{DatePickerMode, DateTime};
    /// let base = DateTime::new(2026, 8, 22, 9, 30);
    /// let edited = DateTime::new(2027, 1, 5, 18, 45);
    /// assert_eq!(
    ///     DatePickerMode::Date.apply(base, edited),
    ///     DateTime::new(2027, 1, 5, 9, 30)
    /// );
    /// ```
    pub fn apply(self, base: DateTime, edited: DateTime) -> DateTime {
        match self {
            DatePickerMode::Date => base.with_date(edited.year, edited.month, edited.day),
            DatePickerMode::Time => base.with_time(edited.hour, edited.minute),
            DatePickerMode::DateTime => edited.normalized(),
        }
    }

    /// The value with only the parts used for comparison. The parts that are not
    /// picked are filled with 0.
    fn key(self, value: DateTime) -> DateTime {
        match self {
            DatePickerMode::Date => DateTime::date(value.year, value.month, value.day),
            DatePickerMode::Time => DateTime::new(0, 0, 0, value.hour, value.minute),
            DatePickerMode::DateTime => value,
        }
    }
}

/// Parses `2026-08-22`.
fn parse_date(text: &str) -> Option<(i32, u8, u8)> {
    let mut parts = text.split('-');
    let year: i32 = parts.next()?.parse().ok()?;
    let month: u8 = parts.next()?.parse().ok()?;
    let day: u8 = parts.next()?.parse().ok()?;
    parts.next().is_none().then_some((year, month, day))
}

/// Parses `09:30`. `<input type="time">` sometimes returns seconds, so the form
/// `09:30:00` is also accepted (the seconds are dropped).
fn parse_time(text: &str) -> Option<(u8, u8)> {
    let mut parts = text.split(':');
    let hour: u8 = parts.next()?.parse().ok()?;
    let minute: u8 = parts.next()?.parse().ok()?;
    // 秒があってもよいが、その先まであるものは形が違う。
    if let Some(second) = parts.next() {
        second.parse::<f64>().ok()?;
        parts.next().is_none().then_some(())?;
    }
    Some((hour, minute))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordering_follows_the_calendar() {
        let mut days = [
            DateTime::new(2026, 8, 22, 9, 30),
            DateTime::date(2026, 8, 22),
            DateTime::date(2025, 12, 31),
            DateTime::new(2026, 8, 22, 9, 5),
        ];
        days.sort();
        assert_eq!(
            days,
            [
                DateTime::date(2025, 12, 31),
                DateTime::date(2026, 8, 22),
                DateTime::new(2026, 8, 22, 9, 5),
                DateTime::new(2026, 8, 22, 9, 30),
            ]
        );
    }

    #[test]
    fn leap_years_follow_the_gregorian_rule() {
        assert!(is_leap_year(2024));
        assert!(!is_leap_year(2025));
        assert!(!is_leap_year(1900)); // 100 の倍数は外れる
        assert!(is_leap_year(2000)); // 400 の倍数は戻る
        assert_eq!(days_in_month(2024, 2), 29);
        assert_eq!(days_in_month(2025, 2), 28);
        assert_eq!(days_in_month(2025, 4), 30);
    }

    #[test]
    fn normalized_clamps_each_field_without_carrying() {
        assert_eq!(
            DateTime::new(2026, 13, 40, 30, 90).normalized(),
            DateTime::new(2026, 12, 31, 23, 59)
        );
        assert_eq!(
            DateTime::date(2026, 11, 31).normalized(),
            DateTime::date(2026, 11, 30)
        );
        assert_eq!(DateTime::date(0, 1, 1).normalized().year, 1);
        assert!(DateTime::new(2026, 2, 29, 0, 0).normalized().is_valid());
    }

    #[test]
    fn parts_can_be_replaced() {
        let value = DateTime::new(2026, 8, 22, 9, 30);
        assert_eq!(value.with_time(18, 0), DateTime::new(2026, 8, 22, 18, 0));
        assert_eq!(
            value.with_date(2027, 1, 5),
            DateTime::new(2027, 1, 5, 9, 30)
        );
        // 日付を差し替えた結果もその月へ収まる。
        assert_eq!(value.with_date(2026, 2, 31).day, 28);
    }

    #[test]
    fn clamp_only_touches_the_part_being_edited() {
        let value = DateTime::new(2026, 7, 20, 9, 30);
        let min = DateTime::new(2026, 8, 1, 13, 0);
        // 日付だけの表示なので、時刻は下限のものへ変わらない。
        assert_eq!(
            DatePickerMode::Date.clamp(value, Some(min), None),
            DateTime::new(2026, 8, 1, 9, 30)
        );
        // 両方の表示なら下限そのものになる。
        assert_eq!(DatePickerMode::DateTime.clamp(value, Some(min), None), min);
        // 時刻だけの表示は日付を見ないので、7 月のままで時刻だけ上がる。
        assert_eq!(
            DatePickerMode::Time.clamp(value, Some(DateTime::time(13, 0)), None),
            DateTime::new(2026, 7, 20, 13, 0)
        );
    }

    #[test]
    fn clamp_keeps_values_inside_the_range() {
        let min = DateTime::date(2026, 1, 1);
        let max = DateTime::date(2026, 12, 31);
        let inside = DateTime::date(2026, 6, 15);
        assert_eq!(
            DatePickerMode::Date.clamp(inside, Some(min), Some(max)),
            inside
        );
        assert_eq!(
            DatePickerMode::Date.clamp(DateTime::date(2030, 1, 1), Some(min), Some(max)),
            max
        );
        assert_eq!(
            DatePickerMode::Date.clamp(DateTime::date(2020, 1, 1), Some(min), Some(max)),
            min
        );
        // 下限が上限より後ろなら、あとから当てる上限が勝つ。
        assert_eq!(
            DatePickerMode::Date.clamp(inside, Some(max), Some(min)),
            min
        );
    }

    #[test]
    fn format_matches_the_html_input_value() {
        let value = DateTime::new(2026, 8, 22, 9, 5);
        assert_eq!(DatePickerMode::Date.format(value), "2026-08-22");
        assert_eq!(DatePickerMode::Time.format(value), "09:05");
        assert_eq!(DatePickerMode::DateTime.format(value), "2026-08-22T09:05");
    }

    #[test]
    fn parse_keeps_the_part_that_is_not_shown() {
        let base = DateTime::new(2026, 8, 22, 9, 30);
        assert_eq!(
            DatePickerMode::Date.parse("2027-01-05", base),
            Some(DateTime::new(2027, 1, 5, 9, 30))
        );
        assert_eq!(
            DatePickerMode::Time.parse("18:45", base),
            Some(DateTime::new(2026, 8, 22, 18, 45))
        );
        // ブラウザが秒まで返すことがある。
        assert_eq!(
            DatePickerMode::Time.parse("18:45:00", base),
            Some(DateTime::new(2026, 8, 22, 18, 45))
        );
        assert_eq!(
            DatePickerMode::DateTime.parse("2027-01-05T18:45", base),
            Some(DateTime::new(2027, 1, 5, 18, 45))
        );
    }

    #[test]
    fn parse_rejects_other_shapes() {
        let base = DateTime::default();
        assert_eq!(DatePickerMode::Date.parse("", base), None);
        assert_eq!(DatePickerMode::Date.parse("2026-08", base), None);
        assert_eq!(DatePickerMode::Date.parse("2026-08-22-01", base), None);
        assert_eq!(DatePickerMode::Time.parse("18", base), None);
        assert_eq!(DatePickerMode::Time.parse("18:45:00:00", base), None);
        assert_eq!(DatePickerMode::DateTime.parse("2027-01-05", base), None);
    }

    #[test]
    fn time_ordering_follows_the_clock() {
        let mut times = [
            Time::new(9, 30),
            Time::MIDNIGHT,
            Time::new(23, 59),
            Time::new(9, 5),
        ];
        times.sort();
        assert_eq!(
            times,
            [
                Time::MIDNIGHT,
                Time::new(9, 5),
                Time::new(9, 30),
                Time::new(23, 59),
            ]
        );
        assert_eq!(Time::default(), Time::MIDNIGHT);
    }

    #[test]
    fn time_normalized_clamps_each_field_without_carrying() {
        assert_eq!(Time::new(25, 70).normalized(), Time::new(23, 59));
        assert!(Time::new(23, 59).is_valid());
        assert!(!Time::new(24, 0).is_valid());
        assert!(!Time::new(0, 60).is_valid());
    }

    #[test]
    fn time_clamped_keeps_values_inside_the_range() {
        let (min, max) = (Time::new(9, 0), Time::new(18, 0));
        assert_eq!(
            Time::new(12, 0).clamped(Some(min), Some(max)),
            Time::new(12, 0)
        );
        assert_eq!(Time::new(7, 30).clamped(Some(min), Some(max)), min);
        assert_eq!(Time::new(22, 0).clamped(Some(min), Some(max)), max);
        assert_eq!(Time::new(7, 30).clamped(None, None), Time::new(7, 30));
        // 丸めも行う。
        assert_eq!(Time::new(25, 70).clamped(None, None), Time::new(23, 59));
        // 下限が上限より後ろなら、あとから当てる上限が勝つ。
        assert_eq!(Time::new(12, 0).clamped(Some(max), Some(min)), min);
    }

    #[test]
    fn time_text_round_trips() {
        for time in [Time::MIDNIGHT, Time::new(9, 5), Time::new(23, 59)] {
            assert_eq!(Time::parse(&time.to_string()), Some(time));
        }
        assert_eq!(Time::new(9, 5).to_string(), "09:05");
        // ブラウザが秒まで返すことがある。
        assert_eq!(Time::parse("09:05:00"), Some(Time::new(9, 5)));
        // 形が違うものは読まない。
        assert_eq!(Time::parse(""), None);
        assert_eq!(Time::parse("9"), None);
        assert_eq!(Time::parse("09:05:00:00"), None);
    }

    #[test]
    fn time_minutes_round_trip_and_clamp() {
        assert_eq!(Time::new(9, 30).minutes_since_midnight(), 570);
        assert_eq!(Time::from_minutes_since_midnight(570), Time::new(9, 30));
        assert_eq!(Time::new(23, 59).minutes_since_midnight(), 1439);
        assert_eq!(Time::from_minutes_since_midnight(-1), Time::MIDNIGHT);
        assert_eq!(
            Time::from_minutes_since_midnight(i64::from(Time::MINUTES_PER_DAY)),
            Time::new(23, 59)
        );
        // 丸めてから数える。
        assert_eq!(Time::new(25, 70).minutes_since_midnight(), 1439);
    }

    #[test]
    fn time_of_day_takes_only_the_clock_part() {
        let value = DateTime::new(2026, 8, 22, 9, 30);
        assert_eq!(value.time_of_day(), Time::new(9, 30));
        let time = Time::new(18, 45);
        assert_eq!(
            value.with_time(time.hour, time.minute),
            DateTime::new(2026, 8, 22, 18, 45)
        );
    }

    #[test]
    fn parse_and_format_round_trip() {
        let value = DateTime::new(2026, 8, 22, 9, 5);
        for mode in [
            DatePickerMode::Date,
            DatePickerMode::Time,
            DatePickerMode::DateTime,
        ] {
            let text = mode.format(value);
            assert_eq!(mode.parse(&text, value), Some(value), "{mode:?}");
        }
    }
}
