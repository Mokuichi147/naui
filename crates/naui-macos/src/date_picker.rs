//! 日付と時刻の選択 (`NSDatePicker`)。
//!
//! AppKit には日付専用のコントロールがあるので、表示する項目
//! (`datePickerElements`) を [`DatePickerMode`] から決めるだけで済む。
//!
//! 暦は**グレゴリオ暦に固定**している。`NSDatePicker` は既定でロケールの暦を
//! 使うため、和暦のロケールでは `year` が「令和 8 年」の 8 を指してしまい、
//! [`DateTime::year`] の意味が環境ごとに変わってしまうため。月名や並び順は
//! ロケールのままなので、表示の言語は変わらない。

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
    /// 変換に使う暦。ピッカーへ渡したものと同じ物を使う。
    calendar: Retained<NSCalendar>,
    /// **選ばせていない部分も含めた**現在値。`NSDatePicker` は選ばせていない
    /// 部分も `NSDate` の中に持つが、4 バックエンドで同じ答えを返すために
    /// naui 側でも持つ。
    value: Cell<DateTime>,
    min: Cell<Option<DateTime>>,
    max: Cell<Option<DateTime>>,
    handler: ValueHandler<DateTime>,
    /// 値を書き込んでいる間だけ、ネイティブからの通知を無視する。
    silent: Cell<bool>,
    /// 変更を受け取る target。AppKit の target は weak なので生かしておく。
    target: RefCell<Option<Retained<ActionTarget>>>,
}

/// 日付と時刻を選ばせるコントロール (`NSDatePicker`)。
///
/// 作った直後の値は、その環境の現在日時 (ローカル時刻)。
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

    /// 何を選ばせるか。生成時に決まり、あとから変わらない。
    pub fn mode(&self) -> DatePickerMode {
        self.0.mode
    }

    /// 現在の値。選ばせていない部分も、渡された値のまま返る。
    pub fn value(&self) -> DateTime {
        self.0.value.get()
    }

    /// 値を通知せずに変える。範囲外なら端へ寄せ、暦として成り立たない値は丸める。
    pub fn set_value(&self, value: DateTime) {
        let value = self.clamp(value);
        self.0.value.set(value);
        self.write_native(value);
    }

    /// 選べる範囲を決める。`None` はその側に制限を置かない。
    ///
    /// いまの値が範囲から外れていれば、通知せずに端へ寄せる。
    pub fn set_range(&self, min: Option<DateTime>, max: Option<DateTime>) {
        self.0.min.set(min.map(DateTime::normalized));
        self.0.max.set(max.map(DateTime::normalized));
        self.write_native_range();
        self.set_value(self.value());
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.0.native.setEnabled(enabled);
    }

    /// 値が変わったときに、変わったあとの値で呼ばれる。
    /// 設定し直すと以前のコールバックは外れる。
    pub fn on_change(&self, f: impl FnMut(DateTime) + 'static) {
        self.0.handler.set(f);
    }

    /// AppKit の実コントロール。バックエンド固有の脱出口として公開している。
    pub fn native_picker(&self) -> Retained<NSDatePicker> {
        self.0.native.clone()
    }

    /// ネイティブ側で値が変わったときの処理。
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

    /// ネイティブへ値を書く。この間の通知は無視する。
    fn write_native(&self, value: DateTime) {
        let Some(date) = to_ns_date(&self.0.calendar, value) else {
            return;
        };
        let previous = self.0.silent.replace(true);
        self.0.native.setDateValue(&date);
        self.0.silent.set(previous);
    }

    /// 下限・上限をネイティブへ渡す。
    ///
    /// 日付だけを選ばせるときは、下限を 0 時・上限を 23 時 59 分にそろえる。
    /// `NSDatePicker` は `NSDate` どうしで比べるため、そろえないと
    /// 「同じ日なのに時刻が早い」というだけで弾かれてしまう。
    ///
    /// 時刻だけを選ばせるときは日付の部分に意味が無いので、ネイティブへは
    /// 渡さない (naui 側の丸めだけで範囲を守る)。
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

/// 表示する項目を [`DatePickerMode`] から決める。
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

/// グレゴリオ暦。時間帯は既定 (システムの現在の時間帯) のまま。
fn gregorian_calendar() -> Retained<NSCalendar> {
    let identifier = unsafe { NSCalendarIdentifierGregorian };
    NSCalendar::initWithCalendarIdentifier(NSCalendar::alloc(), identifier)
        .unwrap_or_else(NSCalendar::currentCalendar)
}

/// その暦・時間帯での現在日時。
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
