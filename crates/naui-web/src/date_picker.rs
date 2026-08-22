//! 日付と時刻の選択 (`<input type="date">` / `"time"` / `"datetime-local"`)。
//!
//! カレンダーや時刻の入力 UI はブラウザが出す。naui は `type` を
//! [`DatePickerMode`] から決め、値の文字列を [`DateTime`] と行き来させるだけ。
//!
//! `datetime-local` を使うのは、ブラウザ側にタイムゾーン変換をさせないため。
//! naui の [`DateTime`] は「画面に出ている暦どおりの値」で、UTC からの
//! ずれを持たない。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use naui_core::{DatePickerMode, DateTime, Result};
use wasm_bindgen::JsCast;
use web_sys::{Document, Element, HtmlInputElement};

use crate::widgets::{create, impl_widget, Listener, ValueHandler, Widget};

struct DatePickerInner {
    input: HtmlInputElement,
    mode: DatePickerMode,
    /// **選ばせていない部分も含めた**現在値。`<input type="date">` は時刻を
    /// 持たないので、日付だけを選ばせるときの時刻はここにしか残らない。
    value: Cell<DateTime>,
    min: Cell<Option<DateTime>>,
    max: Cell<Option<DateTime>>,
    on_change: ValueHandler<DateTime>,
    /// 入力中と確定時の購読。落とすと購読も外れる。
    listeners: RefCell<Vec<Listener>>,
}

/// 日付と時刻を選ばせるコントロール (`<input>` の日付系)。
///
/// 作った直後の値は、そのブラウザの現在日時 (ローカル時刻)。
#[derive(Clone)]
pub struct DatePicker(Rc<DatePickerInner>);
impl_widget!(DatePicker, input);

impl DatePicker {
    pub(crate) fn new(document: &Document, mode: DatePickerMode) -> Result<Self> {
        let input: HtmlInputElement = create(document, "input")?.unchecked_into();
        input.set_type(input_type(mode));

        let this = Self(Rc::new(DatePickerInner {
            input,
            mode,
            value: Cell::new(now()),
            min: Cell::new(None),
            max: Cell::new(None),
            on_change: ValueHandler::default(),
            listeners: RefCell::new(Vec::new()),
        }));
        this.write_native(this.value());

        // 打鍵やカレンダーの選択は `input`、欄を離れたときの確定は `change`。
        // 途中の状態 (`2026-0` など) では値が空文字で届くので、読めたものだけ
        // 受け取り、確定時には読めなかった表示を元の値へ戻す。
        let mut listeners = Vec::new();
        for event in ["input", "change"] {
            let listener = Listener::attach(this.0.input.as_ref(), event, {
                let weak = Rc::downgrade(&this.0);
                let commit = event == "change";
                move || {
                    if let Some(inner) = weak.upgrade() {
                        DatePicker(inner).native_changed(commit);
                    }
                }
            });
            if let Ok(listener) = listener {
                listeners.push(listener);
            }
        }
        *this.0.listeners.borrow_mut() = listeners;
        Ok(this)
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
        let format =
            |value: Option<DateTime>| value.map(|v| self.0.mode.format(v)).unwrap_or_default();
        // 空文字を渡すと属性が外れ、制限なしに戻る。
        self.0.input.set_min(&format(self.0.min.get()));
        self.0.input.set_max(&format(self.0.max.get()));
        self.set_value(self.value());
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.0.input.set_disabled(!enabled);
    }

    /// 値が変わったときに、変わったあとの値で呼ばれる。
    /// 設定し直すと以前のコールバックは外れる。
    pub fn on_change(&self, f: impl FnMut(DateTime) + 'static) {
        self.0.on_change.set(f);
    }

    /// 対応する `<input>`。バックエンド固有の脱出口として公開している。
    pub fn native_input(&self) -> HtmlInputElement {
        self.0.input.clone()
    }

    /// ブラウザ側で値が変わったときの処理。
    ///
    /// `commit` は欄を離れたときの `change` かどうか。入力の途中で
    /// 表示を書き換えると打ちづらくなるので、書き戻しは確定時だけ行う。
    fn native_changed(&self, commit: bool) {
        let text = self.0.input.value();
        let Some(shown) = self.0.mode.parse(&text, self.value()) else {
            // 読めない (空欄を含む)。確定なら元の値へ戻す。
            if commit {
                self.write_native(self.value());
            }
            return;
        };
        let accepted = self.clamp(shown);
        if self.0.mode.format(accepted) != text {
            // 範囲で押し戻したときは表示も直す。
            self.write_native(accepted);
        }
        if accepted == self.value() {
            return;
        }
        self.0.value.set(accepted);
        self.0.on_change.emit(accepted);
    }

    fn clamp(&self, value: DateTime) -> DateTime {
        self.0.mode.clamp(value, self.0.min.get(), self.0.max.get())
    }

    /// 値を `<input>` へ書く。`value` の代入では `input` も `change` も
    /// 起きないので、購読を止める必要はない。
    fn write_native(&self, value: DateTime) {
        self.0.input.set_value(&self.0.mode.format(value));
    }
}

fn input_type(mode: DatePickerMode) -> &'static str {
    match mode {
        DatePickerMode::Date => "date",
        DatePickerMode::Time => "time",
        DatePickerMode::DateTime => "datetime-local",
    }
}

/// ブラウザの現在日時 (ローカル時刻)。
fn now() -> DateTime {
    let now = js_sys::Date::new_0();
    DateTime {
        year: now.get_full_year() as i32,
        // JavaScript の月は 0 始まり。
        month: (now.get_month() + 1) as u8,
        day: now.get_date() as u8,
        hour: now.get_hours() as u8,
        minute: now.get_minutes() as u8,
    }
    .normalized()
}
