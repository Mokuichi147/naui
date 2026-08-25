//! 時刻の選択 (`NSDatePicker` の時分だけを出したもの)。
//!
//! AppKit には時刻専用のコントロールが無く、`NSDatePicker` の
//! `datePickerElements` を `HourMinute` だけにするのが時刻入力の作法。
//! 12 時間制 / 24 時間制の別や `AM` / `PM` の位置はロケールのままになる。
//!
//! 暦は**グレゴリオ暦に固定**している (`DatePicker` と同じ理由)。日付の
//! 部分は画面に出ないので、[`DateTime::TIME_ORIGIN`] の 1 日へ固定して
//! 時分だけを行き来させる。

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
    /// 変換に使う暦。ピッカーへ渡したものと同じ物を使う。
    calendar: Retained<NSCalendar>,
    value: Cell<Time>,
    min: Cell<Option<Time>>,
    max: Cell<Option<Time>>,
    handler: ValueHandler<Time>,
    /// 値を書き込んでいる間だけ、ネイティブからの通知を無視する。
    silent: Cell<bool>,
    /// 変更を受け取る target。AppKit の target は weak なので生かしておく。
    target: RefCell<Option<Retained<ActionTarget>>>,
}

/// 時刻を選ばせるコントロール (`NSDatePicker` の時分)。
///
/// 作った直後の値は、その環境の現在時刻 (ローカル時刻)。
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

    /// いま選ばれている時刻。
    pub fn value(&self) -> Time {
        self.0.value.get()
    }

    /// 値を通知せずに変える。範囲外なら端へ寄せ、時計として成り立たない値は丸める。
    pub fn set_value(&self, value: Time) {
        let value = self.clamp(value);
        self.0.value.set(value);
        self.write_native(value);
    }

    /// 選べる範囲を決める。`None` はその側に制限を置かない。
    ///
    /// いまの値が範囲から外れていれば、通知せずに端へ寄せる。
    pub fn set_range(&self, min: Option<Time>, max: Option<Time>) {
        self.0.min.set(min.map(Time::normalized));
        self.0.max.set(max.map(Time::normalized));
        self.write_native_range();
        self.set_value(self.value());
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.0.native.setEnabled(enabled);
    }

    /// 値が変わったときに、変わったあとの値で呼ばれる。
    /// 設定し直すと以前のコールバックは外れる。
    pub fn on_change(&self, f: impl FnMut(Time) + 'static) {
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

    /// ネイティブへ値を書く。この間の通知は無視する。
    fn write_native(&self, value: Time) {
        let Some(date) = to_ns_date(&self.0.calendar, value) else {
            return;
        };
        let previous = self.0.silent.replace(true);
        self.0.native.setDateValue(&date);
        self.0.silent.set(previous);
    }

    /// 下限・上限をネイティブへ渡す。
    ///
    /// 日付を [`DateTime::TIME_ORIGIN`] へ固定しているので、`NSDate` どうしの
    /// 比較がそのまま時刻の比較になる (日付も選ばせる `DatePicker` の時刻
    /// モードと違い、ここではネイティブ側にも範囲を持たせられる)。
    /// 下限が上限より後ろのときは、naui の丸めと同じく上限へそろえる。
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

/// グレゴリオ暦。時間帯は既定 (システムの現在の時間帯) のまま。
fn gregorian_calendar() -> Retained<NSCalendar> {
    let identifier = unsafe { NSCalendarIdentifierGregorian };
    NSCalendar::initWithCalendarIdentifier(NSCalendar::alloc(), identifier)
        .unwrap_or_else(NSCalendar::currentCalendar)
}

/// その暦・時間帯での現在時刻。
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
