//! An always-visible month calendar (`NSDatePicker` in the calendar style).
//!
//! `NSDatePickerStyle::ClockAndCalendar` with only `YearMonthDay` shows the month
//! grid alone, without the clock. Like [`DatePicker`](crate::DatePicker), the
//! calendar is fixed to Gregorian, so [`DateTime::year`] means the same in every
//! locale; the month names, weekday names and the first day of the week still
//! follow the user's settings.

use std::cell::{Cell, RefCell};
use std::ptr::NonNull;
use std::rc::Rc;

use naui_core::{DatePickerMode, DateTime};
use objc2::rc::Retained;
use objc2::runtime::{NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{define_class, msg_send, sel, DefinedClass, MainThreadMarker, MainThreadOnly, Message};
use objc2_app_kit::{
    NSDatePicker, NSDatePickerCell, NSDatePickerCellDelegate, NSDatePickerElementFlags,
    NSDatePickerStyle, NSView,
};
use objc2_foundation::{NSCalendar, NSDate, NSTimeInterval};

use crate::date_picker::{from_ns_date, gregorian_calendar, now_in, to_ns_date};
use crate::main_thread::on_main_run_loop;
use crate::trampoline::{ActionTarget, ValueHandler};
use crate::widgets::{impl_widget, Widget};

/// Only the date is picked, as in a `DatePicker` that shows only the date.
const MODE: DatePickerMode = DatePickerMode::Date;

define_class!(
    /// Hears every change of the selected day, including the ones the picker does
    /// not send its action for.
    ///
    /// Moving the selection with the arrow keys changes `dateValue` without sending
    /// the action (even with `continuous` set), but the cell asks its delegate to
    /// validate every proposed value, from the keyboard and from clicks alike.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "NauiCalendarDelegate"]
    #[ivars = Box<dyn Fn()>]
    struct CalendarDelegate;

    unsafe impl NSObjectProtocol for CalendarDelegate {}

    unsafe impl NSDatePickerCellDelegate for CalendarDelegate {
        #[unsafe(method(datePickerCell:validateProposedDateValue:timeInterval:))]
        unsafe fn validate_proposed(
            &self,
            _cell: &NSDatePickerCell,
            _proposed: NonNull<NonNull<NSDate>>,
            _interval: *mut NSTimeInterval,
        ) {
            // 提案の値はまだ入っていない。ここで通知すると、受け取った側が
            // set_value で書いた値を直後に提案の値が上書きしてしまうので、
            // 入った後の値を読む処理を積むだけにする。提案の値は変えない。
            (self.ivars())();
        }
    }
);

impl CalendarDelegate {
    fn new(mtm: MainThreadMarker, f: impl Fn() + 'static) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(Box::new(f) as Box<dyn Fn()>);
        unsafe { msg_send![super(this), init] }
    }
}

struct CalendarInner {
    native: Retained<NSDatePicker>,
    /// The calendar used for conversion. The same object as the one passed to the
    /// picker.
    calendar: Retained<NSCalendar>,
    /// The current value, **including the time**, which the calendar does not show.
    value: Cell<DateTime>,
    min: Cell<Option<DateTime>>,
    max: Cell<Option<DateTime>>,
    handler: ValueHandler<DateTime>,
    /// Ignores notifications from the native side only while writing the value.
    silent: Cell<bool>,
    /// The target that receives changes. AppKit targets are weak, so it is kept alive.
    target: RefCell<Option<Retained<ActionTarget>>>,
    /// The delegate that hears changes the action is not sent for (the arrow keys).
    /// Delegates are weak too.
    delegate: RefCell<Option<Retained<CalendarDelegate>>>,
}

/// An always-visible month calendar (`NSDatePicker` in the calendar style).
///
/// Its value right after creation is the platform's current date and time (local
/// time).
#[derive(Clone)]
pub struct Calendar(Rc<CalendarInner>);
impl_widget!(Calendar);

impl Calendar {
    pub(crate) fn new(mtm: MainThreadMarker) -> Self {
        let native = NSDatePicker::new(mtm);
        native.setDatePickerStyle(NSDatePickerStyle::ClockAndCalendar);
        native.setDatePickerElements(NSDatePickerElementFlags::YearMonthDay);
        // 枠と背景は付けない。ほかのウィジェットと並べたとき、暦だけが
        // 箱に入って見えないようにする (macOS の標準アプリの暦も枠なし)。
        native.setBordered(false);
        native.setDrawsBackground(false);
        let calendar = gregorian_calendar();
        // 西暦に固定した暦は、ユーザーが設定した週の始まりを引き継がない。
        // 表示の並びは設定に従わせたいので、いまの暦から写す。
        calendar.setFirstWeekday(NSCalendar::currentCalendar().firstWeekday());
        native.setCalendar(Some(&calendar));

        let now = now_in(&calendar);
        let this = Self(Rc::new(CalendarInner {
            native,
            calendar,
            value: Cell::new(now),
            min: Cell::new(None),
            max: Cell::new(None),
            handler: ValueHandler::default(),
            silent: Cell::new(false),
            target: RefCell::new(None),
            delegate: RefCell::new(None),
        }));
        this.write_native(now);

        let target = ActionTarget::new(mtm, {
            let weak = Rc::downgrade(&this.0);
            move || {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                Calendar(inner).native_changed();
            }
        });
        unsafe {
            this.0.native.setTarget(Some(&target));
            this.0.native.setAction(Some(sel!(invoke:)));
        }
        *this.0.target.borrow_mut() = Some(target);

        // 矢印キーで選択を動かしてもアクションは来ないので、デリゲートで
        // 変化を聞き、値が入った後に読み直す。クリックではアクションの方が
        // 先に届くので、読み直したときには同じ値で、二重には通知しない。
        // モーダルの入れ子のループの中でも届くよう、run loop の共通モードで回す。
        let delegate = CalendarDelegate::new(mtm, {
            let weak = Rc::downgrade(&this.0);
            move || {
                let weak = weak.clone();
                on_main_run_loop(move || {
                    if let Some(inner) = weak.upgrade() {
                        Calendar(inner).native_changed();
                    }
                });
            }
        });
        this.0
            .native
            .setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
        *this.0.delegate.borrow_mut() = Some(delegate);
        this
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
    /// notifying. The days outside the range are shown dimmed (`minDate` /
    /// `maxDate`); clicking one selects the nearest end, which is notified.
    pub fn set_range(&self, min: Option<DateTime>, max: Option<DateTime>) {
        self.0.min.set(min.map(DateTime::normalized));
        self.0.max.set(max.map(DateTime::normalized));
        self.write_native_range();
        self.set_value(self.value());
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.0.native.setEnabled(enabled);
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
    /// The picker's action is sent as a click sends it. For tests and automation.
    pub fn pick(&self, value: DateTime) {
        if !self.0.native.isEnabled() {
            return;
        }
        let Some(date) = to_ns_date(&self.0.calendar, self.clamp(value)) else {
            return;
        };
        // クリックと同じく、表示を変えてからアクションを送る。値の受け取りは
        // native_changed がまとめて行う。
        self.0.native.setDateValue(&date);
        unsafe {
            let target = self.0.native.target();
            self.0
                .native
                .sendAction_to(self.0.native.action(), target.as_deref());
        }
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
        let shown = from_ns_date(&self.0.calendar, &self.0.native.dateValue());
        let accepted = self.clamp(MODE.apply(self.value(), shown));
        if MODE.apply(accepted, shown) != accepted {
            self.write_native(accepted);
        }
        if accepted == self.value() {
            return;
        }
        self.0.value.set(accepted);
        self.0.handler.emit(accepted);
    }

    fn clamp(&self, value: DateTime) -> DateTime {
        MODE.clamp(value, self.0.min.get(), self.0.max.get())
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

    /// Passes the range to the native side, from 00:00 on the first day to 23:59 on
    /// the last, so that the time kept in the value never makes a day unselectable.
    fn write_native_range(&self) {
        let calendar = &self.0.calendar;
        let min = self
            .0
            .min
            .get()
            .map(|min| DateTime::date(min.year, min.month, min.day));
        let max = self.0.max.get().map(|max| max.with_time(23, 59));
        self.0
            .native
            .setMinDate(min.and_then(|v| to_ns_date(calendar, v)).as_deref());
        self.0
            .native
            .setMaxDate(max.and_then(|v| to_ns_date(calendar, v)).as_deref());
    }
}
