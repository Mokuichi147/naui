//! 日付と時刻の値、および日付選択の表示種別。
//!
//! naui は日付の計算をしない。ここにあるのは「年月日と時分の入れ物」
//! ([`DateTime`]) と「時分だけの入れ物」([`Time`])、そしてバックエンドが値を
//! 行き来させるための最小限の変換だけで、暦の計算 (曜日・加減算・タイムゾーン
//! 変換) は持たない。実際の入力は `NSDatePicker` や `<input type="date">` と
//! いった環境側のコントロールが行う。

use std::fmt;

/// 年月日と時分。**秒は持たない**。
///
/// 秒まで選ばせるコントロールが 4 環境の共通部分に無いため
/// (`<input type="time">` は既定で分まで、WinUI 3 の `TimePicker` にも
/// 秒が無い)、naui は分までを扱う。
///
/// 並び順は暦の順序と一致する ([`Ord`] は年 → 月 → 日 → 時 → 分の順で比べる)。
///
/// ```
/// # use naui_core::DateTime;
/// let morning = DateTime::new(2026, 8, 22, 9, 30);
/// assert!(DateTime::date(2026, 8, 22) < morning);
/// assert_eq!(morning.to_string(), "2026-08-22 09:30");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DateTime {
    /// 西暦。[`normalized`](Self::normalized) は 1..=9999 に収める。
    pub year: i32,
    /// 月 (1..=12)。
    pub month: u8,
    /// 日 (1..=その月の日数)。
    pub day: u8,
    /// 時 (0..=23)。
    pub hour: u8,
    /// 分 (0..=59)。
    pub minute: u8,
}

impl DateTime {
    /// [`DateTime::time`] が使う日付。時刻だけを指す値の年月日をそろえておく。
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

    /// 0 時 0 分の日付を作る。
    pub const fn date(year: i32, month: u8, day: u8) -> Self {
        Self::new(year, month, day, 0, 0)
    }

    /// 時刻だけを表す値を作る。日付は [`TIME_ORIGIN`](Self::TIME_ORIGIN) になる。
    ///
    /// [`DatePickerMode::Time`] の表示では日付の部分を見ないので、
    /// 時刻の下限・上限を渡すときにはこれを使う。
    pub const fn time(hour: u8, minute: u8) -> Self {
        let (year, month, day) = Self::TIME_ORIGIN;
        Self::new(year, month, day, hour, minute)
    }

    /// 暦として成り立つ値かどうか (うるう年の 2 月 29 日も含めて判定する)。
    pub fn is_valid(self) -> bool {
        (1..=9999).contains(&self.year)
            && (1..=12).contains(&self.month)
            && (1..=days_in_month(self.year, self.month)).contains(&self.day)
            && self.hour <= 23
            && self.minute <= 59
    }

    /// 暦として成り立つ値へ丸める。
    ///
    /// 各項目をそれぞれの範囲へ収めるだけで、繰り上がりはしない。
    /// 「11 月 31 日」は 12 月 1 日ではなく **11 月 30 日**になる。
    /// 日付の入力欄は月をまたいで値が飛ぶより、その月の端で止まるほうが
    /// 驚きが少ないため。
    ///
    /// ```
    /// # use naui_core::DateTime;
    /// assert_eq!(
    ///     DateTime::date(2026, 11, 31).normalized(),
    ///     DateTime::date(2026, 11, 30)
    /// );
    /// assert_eq!(
    ///     DateTime::date(2025, 2, 29).normalized(),
    ///     DateTime::date(2025, 2, 28) // 2025 年はうるう年ではない
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

    /// 時刻はそのままに、日付だけを差し替える (結果は丸められる)。
    pub fn with_date(self, year: i32, month: u8, day: u8) -> Self {
        Self {
            year,
            month,
            day,
            ..self
        }
        .normalized()
    }

    /// 日付はそのままに、時刻だけを差し替える (結果は丸められる)。
    pub fn with_time(self, hour: u8, minute: u8) -> Self {
        Self {
            hour,
            minute,
            ..self
        }
        .normalized()
    }

    /// 時刻の部分だけを [`Time`] として取り出す (結果は丸められる)。
    ///
    /// 戻す向きは [`with_time`](Self::with_time) を使う。
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
    /// [`TIME_ORIGIN`](DateTime::TIME_ORIGIN) の 0 時 0 分。
    fn default() -> Self {
        let (year, month, day) = Self::TIME_ORIGIN;
        Self::date(year, month, day)
    }
}

impl fmt::Display for DateTime {
    /// `2026-08-22 09:30` の形。
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:04}-{:02}-{:02} {:02}:{:02}",
            self.year, self.month, self.day, self.hour, self.minute
        )
    }
}

/// 時分。**秒は持たない** ([`DateTime`] と同じ理由)。
///
/// 時刻だけを選ばせるコントロール (`TimePicker`) の値。日付を持たないので、
/// 「毎朝 7 時 30 分」のように暦から切り離した時刻を扱うときはこちらを使う。
/// 日付と組で扱いたいときは [`DateTime`] を使う。
///
/// 並び順は時計の順序と一致する ([`Ord`] は時 → 分の順で比べる)。
///
/// ```
/// # use naui_core::Time;
/// let alarm = Time::new(7, 30);
/// assert!(Time::MIDNIGHT < alarm);
/// assert_eq!(alarm.to_string(), "07:30");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Time {
    /// 時 (0..=23)。
    pub hour: u8,
    /// 分 (0..=59)。
    pub minute: u8,
}

impl Time {
    /// 0 時 0 分。[`Default`] でもある。
    pub const MIDNIGHT: Time = Time::new(0, 0);
    /// 1 日の分数。[`minutes_since_midnight`](Self::minutes_since_midnight) の
    /// 上限 (= 23:59) はこれより 1 小さい。
    pub const MINUTES_PER_DAY: u16 = 24 * 60;

    pub const fn new(hour: u8, minute: u8) -> Self {
        Self { hour, minute }
    }

    /// 時計として成り立つ値かどうか。
    pub fn is_valid(self) -> bool {
        self.hour <= 23 && self.minute <= 59
    }

    /// 時計として成り立つ値へ丸める。
    ///
    /// 各項目をそれぞれの範囲へ収めるだけで、繰り上がりはしない
    /// ([`DateTime::normalized`] と同じ決まり)。「25 時 70 分」は翌日の
    /// 2 時 10 分ではなく **23 時 59 分**になる。
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

    /// 値を下限・上限へ収める。丸め ([`normalized`](Self::normalized)) も行う。
    ///
    /// 下限が上限より後ろにあるときは、あとから当てる上限が勝つ
    /// ([`DatePickerMode::clamp`] と同じ決まり)。
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

    /// `09:30` の形を読む。
    ///
    /// `<input type="time">` が秒まで返すことがあるので、`09:30:00` の形も
    /// 受け付ける (秒は捨てる)。形が違う文字列 (空文字を含む) では `None`。
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

    /// 0 時 0 分から数えた分数 (0..=1439)。丸めてから数える。
    ///
    /// WinUI 3 の `TimePicker` が `TimeSpan` で値を持つように、時刻を
    /// 1 つの数として渡したいバックエンドが使う。
    pub fn minutes_since_midnight(self) -> u16 {
        let v = self.normalized();
        u16::from(v.hour) * 60 + u16::from(v.minute)
    }

    /// [`minutes_since_midnight`](Self::minutes_since_midnight) の逆。
    ///
    /// 1 日の外にある分数は端へ寄せる (負なら 0 時 0 分、24 時間以上なら
    /// 23 時 59 分)。
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
    /// `09:30` の形。`<input type="time">` の `value` と同じ形なので、
    /// Web バックエンドはこれをそのまま値や `min` / `max` 属性に使える。
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:02}:{:02}", self.hour, self.minute)
    }
}

/// その月の日数 (うるう年を含む)。
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

/// うるう年かどうか (グレゴリオ暦)。
pub fn is_leap_year(year: i32) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

/// 日付選択で何を選ばせるか。
///
/// 生成時に決め、あとから変えられない。ネイティブのコントロールは
/// 表示する項目を作るときに決まるため (`NSDatePicker` の
/// `datePickerElements`、`<input>` の `type` など)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum DatePickerMode {
    /// 年月日だけ。時刻の部分は触られず、そのまま保たれる。
    #[default]
    Date,
    /// 時分だけ。日付の部分は触られず、そのまま保たれる。
    Time,
    /// 年月日と時分の両方。
    DateTime,
}

impl DatePickerMode {
    /// 年月日を選ばせるか。
    pub fn has_date(self) -> bool {
        matches!(self, DatePickerMode::Date | DatePickerMode::DateTime)
    }

    /// 時分を選ばせるか。
    pub fn has_time(self) -> bool {
        matches!(self, DatePickerMode::Time | DatePickerMode::DateTime)
    }

    /// 値を下限・上限へ収める。丸め ([`DateTime::normalized`]) も行う。
    ///
    /// 比べるのは**この表示で選ばせる部分だけ**で、[`Date`](Self::Date) なら
    /// 時刻を、[`Time`](Self::Time) なら日付を見ない。選ばせていない部分は
    /// 端へ寄せるときも書き換えない。
    ///
    /// 下限が上限より後ろにあるときは、あとから当てる上限が勝つ。
    ///
    /// ```
    /// # use naui_core::{DatePickerMode, DateTime};
    /// let min = DateTime::date(2026, 8, 1);
    /// let value = DateTime::new(2026, 7, 20, 9, 30);
    /// assert_eq!(
    ///     DatePickerMode::Date.clamp(value, Some(min), None),
    ///     DateTime::new(2026, 8, 1, 9, 30) // 時刻は動かない
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

    /// HTML の `<input>` が使う形へ書き出す。
    ///
    /// [`Date`](Self::Date) は `2026-08-22`、[`Time`](Self::Time) は `09:30`、
    /// [`DateTime`](Self::DateTime) は `2026-08-22T09:30`。`min` / `max` 属性も
    /// 同じ形なので、範囲の指定にも使える。
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

    /// [`format`](Self::format) の逆。読めた部分だけを `base` へ重ねて返す。
    ///
    /// 選ばせていない部分は `base` の値が残る。`<input type="time">` のように
    /// 日付を持たないコントロールから読み戻すときに、元の日付を保つため。
    /// 形が違う文字列 (空文字を含む) では `None` を返す。
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

    /// **選ばせている部分だけ**を `edited` から取り込み、残りは `base` のまま返す。
    ///
    /// ネイティブのコントロールから読み戻すときに使う。日付だけの表示なら
    /// 時刻を、時刻だけの表示なら日付を、コントロールが持っていなくても
    /// 元の値のまま保てる。
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

    /// 比較に使う部分だけを残した値。選ばせていない側は 0 で埋める。
    fn key(self, value: DateTime) -> DateTime {
        match self {
            DatePickerMode::Date => DateTime::date(value.year, value.month, value.day),
            DatePickerMode::Time => DateTime::new(0, 0, 0, value.hour, value.minute),
            DatePickerMode::DateTime => value,
        }
    }
}

/// `2026-08-22` を読む。
fn parse_date(text: &str) -> Option<(i32, u8, u8)> {
    let mut parts = text.split('-');
    let year: i32 = parts.next()?.parse().ok()?;
    let month: u8 = parts.next()?.parse().ok()?;
    let day: u8 = parts.next()?.parse().ok()?;
    parts.next().is_none().then_some((year, month, day))
}

/// `09:30` を読む。`<input type="time">` が秒まで返すことがあるので、
/// `09:30:00` の形も受ける (秒は捨てる)。
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
