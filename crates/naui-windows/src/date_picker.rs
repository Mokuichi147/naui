//! 日付と時刻の選択 (WinUI 3 の `ComboBox` を並べたもの)。
//!
//! WinUI 3 には `CalendarDatePicker` と `TimePicker` があるが、naui が使っている
//! WinUI 3 のバインディングには含まれていない。そこで、Windows の `TimePicker`
//! 自身がそうであるように**選択肢を並べた回転式の選択**として組み立てる。
//!
//! 並びは年 / 月 / 日 の順に固定している。並び順はロケールによって違うが、
//! ここは naui が自分で組んでいる部分なので、環境によって順番が変わらない
//! ほうが読み違えにくい。
//!
//! 日の選択肢はその月の日数に合わせて作り直す (2 月なら 28 日か 29 日まで)。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use naui_core::{days_in_month, DatePickerMode, DateTime, Result};
use windows_core::{Interface, HSTRING};
use winui3::Microsoft::UI::Xaml::Controls::{
    Orientation as XamlOrientation, StackPanel, TextBlock,
};
use winui3::Microsoft::UI::Xaml::{UIElement, VerticalAlignment};

use crate::combo_box::ComboBox;
use crate::to_error;
use crate::ui_thread::UiThreadCell;
use crate::widgets::{impl_widget, Widget};

/// 下限が指定されていないときに、年の選択肢をどこまで遡るか。
const YEARS_BACK: i32 = 120;
/// 上限が指定されていないときに、年の選択肢をどこまで先に出すか。
const YEARS_AHEAD: i32 = 20;

type ChangeCallback = Box<dyn FnMut(DateTime)>;

/// 値が変わったことの通知先。
///
/// [`SelectHandler`](crate::widgets::SelectHandler) と同じ形で、呼ぶ間だけ
/// クロージャを取り出すことで再入を許す。
#[derive(Clone)]
struct ChangeHandler(std::sync::Arc<UiThreadCell<Option<ChangeCallback>>>);

impl ChangeHandler {
    fn new() -> Self {
        Self(std::sync::Arc::new(UiThreadCell::new(None)))
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

struct DatePickerInner {
    native: StackPanel,
    mode: DatePickerMode,
    year: ComboBox,
    month: ComboBox,
    day: ComboBox,
    hour: ComboBox,
    minute: ComboBox,
    /// 年の選択肢に並んでいる西暦。インデックスから年を引くために持つ。
    years: RefCell<Vec<i32>>,
    /// **選ばせていない部分も含めた**現在値。
    value: Cell<DateTime>,
    min: Cell<Option<DateTime>>,
    max: Cell<Option<DateTime>>,
    handler: ChangeHandler,
    /// 値を書き込んでいる間だけ、選択の通知を無視する。
    silent: Cell<bool>,
}

/// 日付と時刻を選ばせるコントロール (`ComboBox` の組)。
///
/// 作った直後の値は、その環境の現在日時 (ローカル時刻)。
#[derive(Clone)]
pub struct DatePicker(Rc<DatePickerInner>);
impl_widget!(DatePicker, native);

impl DatePicker {
    pub(crate) fn new(mode: DatePickerMode) -> Result<Self> {
        let native = StackPanel::new().map_err(|e| to_error("StackPanel の生成", e))?;
        native
            .SetOrientation(XamlOrientation::Horizontal)
            .map_err(|e| to_error("日付ピッカーの向きの設定", e))?;
        native
            .SetSpacing(4.0)
            .map_err(|e| to_error("日付ピッカーの間隔の設定", e))?;

        let this = Self(Rc::new(DatePickerInner {
            native,
            mode,
            year: ComboBox::new()?,
            month: ComboBox::new()?,
            day: ComboBox::new()?,
            hour: ComboBox::new()?,
            minute: ComboBox::new()?,
            years: RefCell::new(Vec::new()),
            value: Cell::new(local_now()),
            min: Cell::new(None),
            max: Cell::new(None),
            handler: ChangeHandler::new(),
            silent: Cell::new(false),
        }));

        // 選択肢は固定のものだけ先に入れておく。年と日は値に合わせて作り直す。
        this.0.month.set_items(&numbers(1..=12));
        this.0.hour.set_items(&padded(0..=23));
        this.0.minute.set_items(&padded(0..=59));

        if mode.has_date() {
            this.append(&this.0.year)?;
            this.append_text("/")?;
            this.append(&this.0.month)?;
            this.append_text("/")?;
            this.append(&this.0.day)?;
        }
        if mode.has_time() {
            this.append(&this.0.hour)?;
            this.append_text(":")?;
            this.append(&this.0.minute)?;
        }

        for combo in this.combos() {
            combo.on_select({
                let weak = Rc::downgrade(&this.0);
                move |_index| {
                    if let Some(inner) = weak.upgrade() {
                        DatePicker(inner).native_changed();
                    }
                }
            });
        }

        this.write_native(this.value());
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
    /// いまの値が範囲から外れていれば、通知せずに端へ寄せる。年の選択肢も
    /// この範囲に合わせて作り直す。
    pub fn set_range(&self, min: Option<DateTime>, max: Option<DateTime>) {
        self.0.min.set(min.map(DateTime::normalized));
        self.0.max.set(max.map(DateTime::normalized));
        self.0.years.borrow_mut().clear();
        self.set_value(self.value());
    }

    pub fn set_enabled(&self, enabled: bool) {
        for combo in self.combos() {
            combo.set_enabled(enabled);
        }
    }

    /// 値が変わったときに、変わったあとの値で呼ばれる。
    /// 設定し直すと以前のコールバックは置き換わる。
    pub fn on_change(&self, f: impl FnMut(DateTime) + 'static) {
        self.0.handler.set(f);
    }

    /// 組み立てに使っている `ComboBox` (年・月・日・時・分の順)。
    /// バックエンド固有の脱出口として公開している。
    pub fn native_combo_boxes(&self) -> Vec<ComboBox> {
        self.combos().to_vec()
    }

    /// どれかの `ComboBox` で選択が変わったときの処理。
    fn native_changed(&self) {
        if self.0.silent.get() {
            return;
        }
        let Some(shown) = self.read_native() else {
            return;
        };
        let accepted = self.clamp(self.0.mode.apply(self.value(), shown));
        // 日数の変わる月へ移ったときのために、選択肢ごと作り直す。
        self.write_native(accepted);
        if accepted == self.value() {
            return;
        }
        self.0.value.set(accepted);
        self.0.handler.emit(accepted);
    }

    /// いま選ばれている年月日と時分。どれか 1 つでも未選択なら `None`。
    fn read_native(&self) -> Option<DateTime> {
        let year = *self.0.years.borrow().get(self.0.year.selected()?)?;
        Some(
            DateTime {
                year,
                month: (self.0.month.selected()? + 1) as u8,
                day: (self.0.day.selected()? + 1) as u8,
                hour: self.0.hour.selected()? as u8,
                minute: self.0.minute.selected()? as u8,
            }
            .normalized(),
        )
    }

    fn clamp(&self, value: DateTime) -> DateTime {
        self.0.mode.clamp(value, self.0.min.get(), self.0.max.get())
    }

    /// 値を 5 つの `ComboBox` へ書く。
    ///
    /// `ComboBox::set_items` / `set_selected` は通知しないので、ここから
    /// `on_change` が呼ばれることはない。
    fn write_native(&self, value: DateTime) {
        let previous = self.0.silent.replace(true);

        // `if` の条件に借用を書くと、その借用が本体の実行中まで生きてしまい、
        // 中で `borrow_mut` する rebuild_years と衝突する。先に受けておく。
        let known = self.0.years.borrow().contains(&value.year);
        if !known {
            self.rebuild_years(value.year);
        }
        let index = self.0.years.borrow().iter().position(|y| *y == value.year);
        if let Some(index) = index {
            self.0.year.set_selected(index);
        }
        self.0.month.set_selected(value.month as usize - 1);

        let days = days_in_month(value.year, value.month) as usize;
        if self.0.day.len() != days {
            self.0.day.set_items(&numbers(1..=days as i32));
        }
        self.0.day.set_selected(value.day as usize - 1);

        self.0.hour.set_selected(value.hour as usize);
        self.0.minute.set_selected(value.minute as usize);

        self.0.silent.set(previous);
    }

    /// 年の選択肢を作り直す。範囲があればその範囲、無ければ現在の年を挟む
    /// [`YEARS_BACK`]..[`YEARS_AHEAD`] 年ぶん。`needed` は必ず含める。
    fn rebuild_years(&self, needed: i32) {
        let now = local_now().year;
        let first = self.0.min.get().map_or(now - YEARS_BACK, |min| min.year);
        let last = self.0.max.get().map_or(now + YEARS_AHEAD, |max| max.year);
        let first = first.min(needed).max(1);
        let last = last.max(needed).max(first);
        let years: Vec<i32> = (first..=last).collect();
        self.0.year.set_items(&numbers_from(&years));
        *self.0.years.borrow_mut() = years;
    }

    fn combos(&self) -> [ComboBox; 5] {
        [
            self.0.year.clone(),
            self.0.month.clone(),
            self.0.day.clone(),
            self.0.hour.clone(),
            self.0.minute.clone(),
        ]
    }

    fn append(&self, combo: &ComboBox) -> Result<()> {
        self.append_element(&combo.native_element())
    }

    /// 区切りの文字を並びへ入れる。
    fn append_text(&self, text: &str) -> Result<()> {
        let label = TextBlock::new().map_err(|e| to_error("区切りの生成", e))?;
        label
            .SetText(&HSTRING::from(text))
            .map_err(|e| to_error("区切りの設定", e))?;
        label
            .SetVerticalAlignment(VerticalAlignment::Center)
            .map_err(|e| to_error("区切りの配置", e))?;
        let element = label
            .cast::<UIElement>()
            .map_err(|e| to_error("区切りの要素化", e))?;
        self.append_element(&element)
    }

    fn append_element(&self, element: &UIElement) -> Result<()> {
        self.0
            .native
            .Children()
            .map_err(|e| to_error("日付ピッカーの子の取得", e))?
            .Append(element)
            .map_err(|e| to_error("日付ピッカーへの追加", e))
    }
}

fn numbers(range: std::ops::RangeInclusive<i32>) -> Vec<String> {
    range.map(|n| n.to_string()).collect()
}

fn numbers_from(values: &[i32]) -> Vec<String> {
    values.iter().map(|n| n.to_string()).collect()
}

/// 2 桁でそろえた選択肢 (`00`, `01`, …)。時刻に使う。
fn padded(range: std::ops::RangeInclusive<i32>) -> Vec<String> {
    range.map(|n| format!("{n:02}")).collect()
}

/// Windows の現在日時 (ローカル時刻)。
fn local_now() -> DateTime {
    // SAFETY: 出力を受け取るだけの Win32 呼び出しで、引数も状態も持たない。
    let now = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
    DateTime {
        year: now.wYear as i32,
        month: now.wMonth as u8,
        day: now.wDay as u8,
        hour: now.wHour as u8,
        minute: now.wMinute as u8,
    }
    .normalized()
}
