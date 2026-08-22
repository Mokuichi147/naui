//! 日付と時刻の選択 (`GtkMenuButton` + `GtkCalendar` / `GtkSpinButton`)。
//!
//! GTK4 には日付を選ぶ 1 つのコントロールが無い。カレンダーの `GtkCalendar` と
//! 数値の `GtkSpinButton` はあるので、GNOME のアプリと同じ組み方をする。
//!
//! - 日付: 現在の日付を出す `GtkMenuButton` を押すと、`GtkPopover` の中の
//!   `GtkCalendar` が開く。
//! - 時刻: 時と分の `GtkSpinButton` を `:` で挟んで並べる (時計アプリの
//!   アラーム設定と同じ形)。
//!
//! **日付を選んでもポップオーバーは閉じない。** `GtkCalendar` は「日を押した」
//! と「月を送った」を区別せず、どちらも `day-selected` で届く。押すたびに
//! 閉じると、月を送っただけで閉じてしまうため。
//!
//! ボタンに出す日付はロケールの書式 (`%x`) に従う。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::glib;
use gtk::prelude::*;
use naui_core::{DatePickerMode, DateTime};

use crate::bin::SizeBin;
use crate::callback::Notifier;
use crate::widgets::{impl_widget, without_signal, Widget};

struct DatePickerInner {
    native: gtk::Box,
    bin: SizeBin,
    mode: DatePickerMode,
    /// 日付を出すボタンと、その中のカレンダー。
    /// 時刻だけを選ばせるときは作るだけで並べない。
    button: gtk::MenuButton,
    calendar: gtk::Calendar,
    hour: gtk::SpinButton,
    minute: gtk::SpinButton,
    /// プログラムから値を書くときに止めるシグナル。
    calendar_handler: RefCell<Option<glib::SignalHandlerId>>,
    hour_handler: RefCell<Option<glib::SignalHandlerId>>,
    minute_handler: RefCell<Option<glib::SignalHandlerId>>,
    /// **選ばせていない部分も含めた**現在値。
    value: Cell<DateTime>,
    min: Cell<Option<DateTime>>,
    max: Cell<Option<DateTime>>,
    on_change: Notifier<DateTime>,
}

/// 日付と時刻を選ばせるコントロール。
///
/// 作った直後の値は、その環境の現在日時 (ローカル時刻)。
#[derive(Clone)]
pub struct DatePicker(Rc<DatePickerInner>);
impl_widget!(DatePicker);

impl DatePicker {
    pub(crate) fn new(mode: DatePickerMode) -> Self {
        let native = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        let bin = SizeBin::wrap(&native);

        let calendar = gtk::Calendar::new();
        let popover = gtk::Popover::new();
        popover.set_child(Some(&calendar));
        let button = gtk::MenuButton::new();
        button.set_popover(Some(&popover));

        let hour = spin(0.0, 23.0);
        let minute = spin(0.0, 59.0);

        if mode.has_date() {
            native.append(&button);
        }
        if mode.has_time() {
            let separator = gtk::Label::new(Some(":"));
            native.append(&hour);
            native.append(&separator);
            native.append(&minute);
        }

        let inner = Rc::new(DatePickerInner {
            native,
            bin,
            mode,
            button,
            calendar,
            hour,
            minute,
            calendar_handler: RefCell::new(None),
            hour_handler: RefCell::new(None),
            minute_handler: RefCell::new(None),
            value: Cell::new(now()),
            min: Cell::new(None),
            max: Cell::new(None),
            on_change: Notifier::default(),
        });
        let this = Self(inner);
        this.write_native(this.value());

        // 3 つのコントロールは、どれも「表示されている値が変わった」を
        // 伝えるだけ。値の組み立ては read_native がまとめて行う。
        let calendar_handler = {
            let weak = Rc::downgrade(&this.0);
            this.0.calendar.connect_day_selected(move |_| {
                if let Some(inner) = weak.upgrade() {
                    DatePicker(inner).native_changed();
                }
            })
        };
        *this.0.calendar_handler.borrow_mut() = Some(calendar_handler);
        for (spin, slot) in [
            (&this.0.hour, &this.0.hour_handler),
            (&this.0.minute, &this.0.minute_handler),
        ] {
            let handler = {
                let weak = Rc::downgrade(&this.0);
                spin.connect_value_changed(move |_| {
                    if let Some(inner) = weak.upgrade() {
                        DatePicker(inner).native_changed();
                    }
                })
            };
            *slot.borrow_mut() = Some(handler);
        }
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
    /// `GtkCalendar` にも `GtkSpinButton` にも「暦としての範囲」は無いので、
    /// 範囲は naui 側の丸めだけで守る。
    pub fn set_range(&self, min: Option<DateTime>, max: Option<DateTime>) {
        self.0.min.set(min.map(DateTime::normalized));
        self.0.max.set(max.map(DateTime::normalized));
        self.set_value(self.value());
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.0.native.set_sensitive(enabled);
    }

    /// 値が変わったときに、変わったあとの値で呼ばれる。
    /// 設定し直すと以前のコールバックは外れる。
    pub fn on_change(&self, f: impl FnMut(DateTime) + 'static) {
        self.0.on_change.set(f);
    }

    /// 日付を出しているボタン。バックエンド固有の脱出口として公開している。
    pub fn native_button(&self) -> gtk::MenuButton {
        self.0.button.clone()
    }

    /// ポップオーバーの中のカレンダー。
    pub fn native_calendar(&self) -> gtk::Calendar {
        self.0.calendar.clone()
    }

    /// 時と分の `GtkSpinButton`。
    pub fn native_spins(&self) -> (gtk::SpinButton, gtk::SpinButton) {
        (self.0.hour.clone(), self.0.minute.clone())
    }

    /// どれかのコントロールで表示が変わったときの処理。
    fn native_changed(&self) {
        let shown = self.read_native();
        let accepted = self.clamp(self.0.mode.apply(self.value(), shown));
        // 丸めや範囲で押し戻したときのために、表示は必ず書き直す
        // (同じ値なら GTK 側は何もしない)。
        self.write_native(accepted);
        if accepted == self.value() {
            return;
        }
        self.0.value.set(accepted);
        self.0.on_change.emit(accepted);
    }

    /// いま画面に出ている年月日と時分。
    fn read_native(&self) -> DateTime {
        let date = self.0.calendar.date();
        DateTime {
            year: date.year(),
            month: date.month() as u8,
            day: date.day_of_month() as u8,
            hour: self.0.hour.value() as u8,
            minute: self.0.minute.value() as u8,
        }
        .normalized()
    }

    fn clamp(&self, value: DateTime) -> DateTime {
        self.0.mode.clamp(value, self.0.min.get(), self.0.max.get())
    }

    /// 値を 3 つのコントロールへ書く。この間はシグナルを止める。
    fn write_native(&self, value: DateTime) {
        if let Ok(date) = to_glib_date_time(value) {
            without_signal(&self.0.calendar, &self.0.calendar_handler, || {
                self.0.calendar.select_day(&date);
            });
            self.0.button.set_label(&format_date(&date));
        }
        without_signal(&self.0.hour, &self.0.hour_handler, || {
            self.0.hour.set_value(value.hour as f64);
        });
        without_signal(&self.0.minute, &self.0.minute_handler, || {
            self.0.minute.set_value(value.minute as f64);
        });
    }
}

/// 2 桁で表示する 0 始まりのスピンボタン。
fn spin(min: f64, max: f64) -> gtk::SpinButton {
    let spin = gtk::SpinButton::with_range(min, max, 1.0);
    spin.set_numeric(true);
    // 23 の次は 0 へ戻る。時刻の入力では端で止まるより自然。
    spin.set_wrap(true);
    spin.set_width_chars(2);
    spin.set_max_width_chars(2);
    // 既定の表示は `9` になるので、`09` へそろえる。
    spin.connect_output(|spin| {
        spin.set_text(&format!("{:02}", spin.value() as i32));
        glib::Propagation::Stop
    });
    spin
}

/// ロケールの日付書式 (`%x`)。取れなければ ISO の形にする。
fn format_date(date: &glib::DateTime) -> String {
    date.format("%x")
        .map(|text| text.to_string())
        .unwrap_or_else(|_| {
            format!(
                "{:04}-{:02}-{:02}",
                date.year(),
                date.month(),
                date.day_of_month()
            )
        })
}

fn to_glib_date_time(value: DateTime) -> Result<glib::DateTime, glib::BoolError> {
    let v = value.normalized();
    glib::DateTime::from_local(
        v.year,
        v.month as i32,
        v.day as i32,
        v.hour as i32,
        v.minute as i32,
        0.0,
    )
}

/// GTK 側の現在日時 (ローカル時刻)。
fn now() -> DateTime {
    let Ok(now) = glib::DateTime::now_local() else {
        return DateTime::default();
    };
    DateTime {
        year: now.year(),
        month: now.month() as u8,
        day: now.day_of_month() as u8,
        hour: now.hour() as u8,
        minute: now.minute() as u8,
    }
    .normalized()
}
