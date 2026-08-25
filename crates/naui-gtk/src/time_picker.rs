//! 時刻の選択 (`GtkSpinButton` の組)。
//!
//! GTK4 には時刻を選ぶ 1 つのコントロールが無い。時と分の `GtkSpinButton` を
//! `:` で挟んで並べるのが GNOME のアプリ (時計のアラーム設定など) の形なので、
//! `DatePicker` の時刻の部分と同じ組み方をする。
//!
//! `GtkSpinButton` には「時刻としての範囲」が無いので、下限・上限は naui 側の
//! 丸めだけで守る。時・分それぞれの端 (23 と 59) では次の桁へ繰り上がらず
//! 0 へ戻る。

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
    /// プログラムから値を書くときに止めるシグナル。
    hour_handler: RefCell<Option<glib::SignalHandlerId>>,
    minute_handler: RefCell<Option<glib::SignalHandlerId>>,
    value: Cell<Time>,
    min: Cell<Option<Time>>,
    max: Cell<Option<Time>>,
    on_change: Notifier<Time>,
}

/// 時刻を選ばせるコントロール (時と分の `GtkSpinButton`)。
///
/// 作った直後の値は、その環境の現在時刻 (ローカル時刻)。
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
        self.set_value(self.value());
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.0.native.set_sensitive(enabled);
    }

    /// 値が変わったときに、変わったあとの値で呼ばれる。
    /// 設定し直すと以前のコールバックは外れる。
    pub fn on_change(&self, f: impl FnMut(Time) + 'static) {
        self.0.on_change.set(f);
    }

    /// 時と分の `GtkSpinButton`。バックエンド固有の脱出口として公開している。
    pub fn native_spins(&self) -> (gtk::SpinButton, gtk::SpinButton) {
        (self.0.hour.clone(), self.0.minute.clone())
    }

    /// どちらかのスピンボタンで表示が変わったときの処理。
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

    /// いま画面に出ている時分。
    fn read_native(&self) -> Time {
        Time::new(self.0.hour.value() as u8, self.0.minute.value() as u8).normalized()
    }

    fn clamp(&self, value: Time) -> Time {
        value.clamped(self.0.min.get(), self.0.max.get())
    }

    /// 値を 2 つのスピンボタンへ書く。この間はシグナルを止める。
    fn write_native(&self, value: Time) {
        without_signal(&self.0.hour, &self.0.hour_handler, || {
            self.0.hour.set_value(value.hour as f64);
        });
        without_signal(&self.0.minute, &self.0.minute_handler, || {
            self.0.minute.set_value(value.minute as f64);
        });
    }
}

/// GTK 側の現在時刻 (ローカル時刻)。
fn now() -> Time {
    let Ok(now) = glib::DateTime::now_local() else {
        return Time::MIDNIGHT;
    };
    Time::new(now.hour() as u8, now.minute() as u8).normalized()
}
