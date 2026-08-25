//! 時刻の選択 (`<input type="time">`)。
//!
//! 時刻の入力 UI (スピナーや時計のポップアップ) はブラウザが出す。naui は
//! `value` の `"09:30"` を [`Time`] と行き来させるだけで、自前で組み立てる
//! ものは無い。12 時間制 / 24 時間制の別はブラウザのロケールに従う。
//!
//! 秒は扱わない。`<input type="time">` は `step` を細かくしない限り分までしか
//! 返さないので、[`Time`] の持つ範囲と同じになる。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use naui_core::{Result, Time};
use wasm_bindgen::JsCast;
use web_sys::{Document, Element, HtmlInputElement};

use crate::widgets::{create, impl_widget, Listener, ValueHandler, Widget};

struct TimePickerInner {
    input: HtmlInputElement,
    value: Cell<Time>,
    min: Cell<Option<Time>>,
    max: Cell<Option<Time>>,
    on_change: ValueHandler<Time>,
    /// 入力中と確定時の購読。落とすと購読も外れる。
    listeners: RefCell<Vec<Listener>>,
}

/// 時刻を選ばせるコントロール (`<input type="time">`)。
///
/// 作った直後の値は、そのブラウザの現在時刻 (ローカル時刻)。
#[derive(Clone)]
pub struct TimePicker(Rc<TimePickerInner>);
impl_widget!(TimePicker, input);

impl TimePicker {
    pub(crate) fn new(document: &Document) -> Result<Self> {
        let input: HtmlInputElement = create(document, "input")?.unchecked_into();
        input.set_type("time");

        let this = Self(Rc::new(TimePickerInner {
            input,
            value: Cell::new(now()),
            min: Cell::new(None),
            max: Cell::new(None),
            on_change: ValueHandler::default(),
            listeners: RefCell::new(Vec::new()),
        }));
        this.write_native(this.value());

        // 打鍵やスピナーの操作は `input`、欄を離れたときの確定は `change`。
        // 途中の状態 (`09:` など) では値が空文字で届くので、読めたものだけ
        // 受け取り、確定時には読めなかった表示を元の値へ戻す
        // (`DatePicker` と同じ扱い)。
        let mut listeners = Vec::new();
        for event in ["input", "change"] {
            let listener = Listener::attach(this.0.input.as_ref(), event, {
                let weak = Rc::downgrade(&this.0);
                let commit = event == "change";
                move || {
                    if let Some(inner) = weak.upgrade() {
                        TimePicker(inner).native_changed(commit);
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
        let format = |value: Option<Time>| value.map(|v| v.to_string()).unwrap_or_default();
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
    pub fn on_change(&self, f: impl FnMut(Time) + 'static) {
        self.0.on_change.set(f);
    }

    /// 入力欄本体。バックエンド固有の脱出口として公開している。
    pub fn native_input(&self) -> HtmlInputElement {
        self.0.input.clone()
    }

    /// ブラウザ側で値が変わったときの処理。
    ///
    /// `commit` は欄を離れたときの `change` かどうか。入力の途中で
    /// 表示を書き換えると打ちづらくなるので、書き戻しは確定時だけ行う。
    fn native_changed(&self, commit: bool) {
        let text = self.0.input.value();
        let Some(shown) = Time::parse(&text) else {
            // 読めない (空欄を含む)。確定なら元の値へ戻す。
            if commit {
                self.write_native(self.value());
            }
            return;
        };
        let accepted = self.clamp(shown);
        if accepted.to_string() != text {
            // 範囲で押し戻したときは表示も直す。
            self.write_native(accepted);
        }
        if accepted == self.value() {
            return;
        }
        self.0.value.set(accepted);
        self.0.on_change.emit(accepted);
    }

    fn clamp(&self, value: Time) -> Time {
        value.clamped(self.0.min.get(), self.0.max.get())
    }

    /// 値を `<input>` へ書く。`value` の代入では `input` も `change` も
    /// 起きないので、購読を止める必要はない。
    fn write_native(&self, value: Time) {
        self.0.input.set_value(&value.to_string());
    }
}

/// ブラウザの現在時刻 (ローカル時刻)。
fn now() -> Time {
    let now = js_sys::Date::new_0();
    Time::new(now.get_hours() as u8, now.get_minutes() as u8).normalized()
}
