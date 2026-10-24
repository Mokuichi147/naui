//! Date and time picking (WinUI 3's native `DatePicker` / `TimePicker`).
//!
//! Both are created directly from the [`naui_winui3`] projection. Only tightening the
//! display touches the standard template's contents, so the root is looked up via
//! `VisualTreeHelper`.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use naui_core::{DatePickerMode, DateTime, Result, Time};
use naui_winui3::Microsoft::UI::Xaml::Controls::{
    ColumnDefinition, Control, DatePicker as XamlDatePicker, DatePickerValueChangedEventArgs,
    Grid as XamlGrid, Orientation as XamlOrientation, StackPanel, TextBlock,
    TimePicker as XamlTimePicker, TimePickerValueChangedEventArgs,
};
use naui_winui3::Microsoft::UI::Xaml::Media::VisualTreeHelper;
use naui_winui3::Microsoft::UI::Xaml::{
    DependencyObject, FrameworkElement, GridLength, GridUnitType, RoutedEventHandler,
    TextAlignment, Thickness, UIElement,
};
use windows::Foundation::{DateTime as WinDateTime, EventHandler, TimeSpan};
use windows::Globalization::{Calendar, CalendarIdentifiers};
use windows_core::{Interface, HSTRING};

use crate::combo_box::ComboBox;
use crate::to_error;
use crate::ui_thread::UiThreadCell;
use crate::widgets::{impl_widget, Widget};

const TICKS_PER_MINUTE: i64 = 60 * 10_000_000;
const DATE_PICKER_COMPACT_WIDTH: f64 = 216.0;
const TIME_PICKER_COMPACT_WIDTH: f64 = 152.0;

type ChangeCallback = Box<dyn FnMut(DateTime)>;

/// The handler notified when the value changes. The closure is taken out only while
/// it is called, allowing re-entrancy.
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
    date: Option<XamlDatePicker>,
    time: Option<XamlTimePicker>,
    value: Cell<DateTime>,
    min: Cell<Option<DateTime>>,
    max: Cell<Option<DateTime>>,
    handler: ChangeHandler,
    silent: Cell<bool>,
}

/// A WinUI 3 native control for picking a date and time.
///
/// In `DateTime` mode, a `DatePicker` and a `TimePicker` are laid out side by side.
/// Its value right after creation is the platform's current date and time (local
/// time).
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
            .SetSpacing(8.0)
            .map_err(|e| to_error("日付ピッカーの間隔の設定", e))?;

        let date = mode.has_date().then(new_date_picker).transpose()?;
        let time = mode.has_time().then(new_time_picker).transpose()?;
        let this = Self(Rc::new(DatePickerInner {
            native,
            mode,
            date,
            time,
            value: Cell::new(local_now()),
            min: Cell::new(None),
            max: Cell::new(None),
            handler: ChangeHandler::new(),
            silent: Cell::new(false),
        }));

        if let Some(date) = &this.0.date {
            let element = date
                .cast::<UIElement>()
                .map_err(|e| to_error("DatePicker の要素化", e))?;
            this.append(&element)?;
            balance_date_picker_columns(date);
            let loaded_date = Arc::new(UiThreadCell::new(date.clone()));
            let loaded = RoutedEventHandler::new(move |_, _| {
                let _ = loaded_date.try_with_mut(|date| balance_date_picker_columns(date));
                Ok(())
            });
            element
                .cast::<FrameworkElement>()
                .map_err(|e| to_error("DatePicker のレイアウト要素化", e))?
                .Loaded(&loaded)
                .map_err(|e| to_error("DatePicker の読み込み購読", e))?;
            let target = Arc::new(UiThreadCell::new(Rc::downgrade(&this.0)));
            let changed = EventHandler::<DatePickerValueChangedEventArgs>::new(move |_, _| {
                let _ = target.try_with_mut(|weak| {
                    if let Some(inner) = weak.upgrade() {
                        DatePicker(inner).native_changed();
                    }
                });
                Ok(())
            });
            date.DateChanged(&changed)
                .map_err(|e| to_error("DatePicker の変更購読", e))?;
        }

        if let Some(time) = &this.0.time {
            let element = time
                .cast::<UIElement>()
                .map_err(|e| to_error("TimePicker の要素化", e))?;
            this.append(&element)?;
            compact_time_picker(time);
            let loaded_time = Arc::new(UiThreadCell::new(time.clone()));
            let loaded = RoutedEventHandler::new(move |_, _| {
                let _ = loaded_time.try_with_mut(|time| compact_time_picker(time));
                Ok(())
            });
            element
                .cast::<FrameworkElement>()
                .map_err(|e| to_error("TimePicker のレイアウト要素化", e))?
                .Loaded(&loaded)
                .map_err(|e| to_error("TimePicker の読み込み購読", e))?;
            let target = Arc::new(UiThreadCell::new(Rc::downgrade(&this.0)));
            let changed = EventHandler::<TimePickerValueChangedEventArgs>::new(move |_, _| {
                let _ = target.try_with_mut(|weak| {
                    if let Some(inner) = weak.upgrade() {
                        DatePicker(inner).native_changed();
                    }
                });
                Ok(())
            });
            time.TimeChanged(&changed)
                .map_err(|e| to_error("TimePicker の変更購読", e))?;
        }

        this.write_native(this.value());
        Ok(this)
    }

    pub fn mode(&self) -> DatePickerMode {
        self.0.mode
    }

    pub fn value(&self) -> DateTime {
        self.0.value.get()
    }

    pub fn set_value(&self, value: DateTime) {
        let value = self.clamp(value);
        self.0.value.set(value);
        self.write_native(value);
    }

    /// The year range is applied on the WinUI side, and month/day and time bounds are
    /// clamped to the ends on change.
    pub fn set_range(&self, min: Option<DateTime>, max: Option<DateTime>) {
        self.0.min.set(min.map(DateTime::normalized));
        self.0.max.set(max.map(DateTime::normalized));
        self.write_native_range();
        self.set_value(self.value());
    }

    pub fn set_enabled(&self, enabled: bool) {
        for element in self.native_picker_elements() {
            if let Ok(control) = element.cast::<Control>() {
                let _ = control.SetIsEnabled(enabled);
            }
        }
    }

    pub fn on_change(&self, f: impl FnMut(DateTime) + 'static) {
        self.0.handler.set(f);
    }

    /// WinUI 3's `DatePicker`. `None` in modes that do not show a date.
    pub fn native_date_picker_element(&self) -> Option<UIElement> {
        self.0.date.as_ref()?.cast().ok()
    }

    /// WinUI 3's `TimePicker`. `None` in modes that do not show a time.
    pub fn native_time_picker_element(&self) -> Option<UIElement> {
        self.0.time.as_ref()?.cast().ok()
    }

    /// For source compatibility with the former ComboBox implementation. Always empty
    /// since the switch to native controls.
    #[deprecated(note = "use native_date_picker_element / native_time_picker_element instead")]
    pub fn native_combo_boxes(&self) -> Vec<ComboBox> {
        Vec::new()
    }

    fn native_changed(&self) {
        if self.0.silent.get() {
            return;
        }
        let Some(shown) = self.read_native() else {
            return;
        };
        let accepted = self.clamp(shown);
        self.write_native(accepted);
        if accepted == self.value() {
            return;
        }
        self.0.value.set(accepted);
        self.0.handler.emit(accepted);
    }

    fn read_native(&self) -> Option<DateTime> {
        let mut value = self.value();
        if let Some(date) = &self.0.date {
            let selected = from_native_date(date.Date().ok()?)?;
            value = value.with_date(selected.year, selected.month, selected.day);
        }
        if let Some(time) = &self.0.time {
            let shown = from_native_time(time.Time().ok()?);
            value = value.with_time(shown.hour, shown.minute);
        }
        Some(value.normalized())
    }

    fn clamp(&self, value: DateTime) -> DateTime {
        self.0.mode.clamp(value, self.0.min.get(), self.0.max.get())
    }

    fn write_native(&self, value: DateTime) {
        let previous = self.0.silent.replace(true);
        if let (Some(date), Ok(value)) = (&self.0.date, to_native_date(value)) {
            let _ = date.SetDate(value);
        }
        if let Some(time) = &self.0.time {
            let _ = time.SetTime(to_native_time(value.time_of_day()));
        }
        self.0.silent.set(previous);
    }

    fn write_native_range(&self) {
        let Some(date) = &self.0.date else {
            return;
        };
        let min = self.0.min.get().unwrap_or(DateTime::date(1, 1, 1));
        let max = self.0.max.get().unwrap_or(DateTime::date(9999, 12, 31));
        // WinUI は設定途中でも MinYear <= MaxYear を要求する。いったん最大範囲へ
        // 戻してから上限、下限の順で狭める。逆転範囲は naui の clamp と同じく
        // 上限側へ一点化する。
        let min = if min > max { max } else { min };
        let values = [
            (false, DateTime::date(9999, 12, 31)),
            (true, DateTime::date(1, 1, 1)),
            (false, max),
            (true, min),
        ];
        for (is_min, value) in values {
            let Ok(value) = to_native_date(value) else {
                continue;
            };
            if is_min {
                let _ = date.SetMinYear(value);
            } else {
                let _ = date.SetMaxYear(value);
            }
        }
    }

    fn native_picker_elements(&self) -> impl Iterator<Item = UIElement> + '_ {
        self.native_date_picker_element()
            .into_iter()
            .chain(self.native_time_picker_element())
    }

    fn append(&self, element: &UIElement) -> Result<()> {
        self.0
            .native
            .Children()
            .map_err(|e| to_error("日付ピッカーの子の取得", e))?
            .Append(element)
            .map_err(|e| to_error("日付ピッカーへの追加", e))
    }
}

fn new_date_picker() -> Result<XamlDatePicker> {
    let picker = XamlDatePicker::new().map_err(|e| to_error("WinUI DatePicker の生成", e))?;
    let gregorian =
        CalendarIdentifiers::Gregorian().map_err(|e| to_error("グレゴリオ暦の取得", e))?;
    picker
        .SetCalendarIdentifier(&gregorian)
        .map_err(|e| to_error("DatePicker の暦設定", e))?;
    Ok(picker)
}

pub(crate) fn new_time_picker() -> Result<XamlTimePicker> {
    XamlTimePicker::new().map_err(|e| to_error("WinUI TimePicker の生成", e))
}

/// The standard template assumes long English month names, making only the month
/// wide and left-aligned. For Japanese display, the three columns are made equal
/// width and centered, and the default minimum width is reduced.
fn balance_date_picker_columns(picker: &XamlDatePicker) {
    let Some(root) = picker_template_root(picker) else {
        return;
    };
    let width = GridLength {
        Value: 1.0,
        GridUnitType: GridUnitType::Star,
    };
    for name in ["DayColumn", "MonthColumn", "YearColumn"] {
        let Ok(column) = root
            .FindName(&HSTRING::from(name))
            .and_then(|value| value.cast::<ColumnDefinition>())
        else {
            continue;
        };
        let _ = column.SetWidth(width);
    }

    if let Ok(month) = root
        .FindName(&HSTRING::from("MonthTextBlock"))
        .and_then(|value| value.cast::<TextBlock>())
    {
        let _ = month.SetTextAlignment(TextAlignment::Center);
        let _ = month.SetPadding(Thickness {
            Left: 0.0,
            Top: 3.0,
            Right: 0.0,
            Bottom: 6.0,
        });
        let _ = month.SetMargin(Thickness::default());
    }

    set_template_min_width(&root, DATE_PICKER_COMPACT_WIDTH);

    // テンプレートの内容 Grid が余白を再配分しないよう、3列の変更を
    // レイアウトへ即座に反映させる。
    if let Ok(grid) = root
        .FindName(&HSTRING::from("FlyoutButtonContentGrid"))
        .and_then(|value| value.cast::<XamlGrid>())
    {
        let _ = grid.InvalidateMeasure();
    }
}

pub(crate) fn compact_time_picker(picker: &XamlTimePicker) {
    let Some(root) = picker_template_root(picker) else {
        return;
    };
    set_template_min_width(&root, TIME_PICKER_COMPACT_WIDTH);
}

fn picker_template_root<T: Interface>(picker: &T) -> Option<FrameworkElement> {
    let control = picker.cast::<Control>().ok()?;
    let _ = control.ApplyTemplate();
    picker
        .cast::<DependencyObject>()
        .and_then(|picker| VisualTreeHelper::GetChild(&picker, 0))
        .and_then(|root| root.cast::<FrameworkElement>())
        .ok()
}

fn set_template_min_width(root: &FrameworkElement, width: f64) {
    if let Ok(button) = root
        .FindName(&HSTRING::from("FlyoutButton"))
        .and_then(|value| value.cast::<FrameworkElement>())
    {
        let _ = button.SetMinWidth(width);
        let _ = button.InvalidateMeasure();
    }
}

fn to_native_date(value: DateTime) -> windows_core::Result<WinDateTime> {
    let value = value.normalized();
    let calendar = gregorian_calendar()?;
    calendar.SetToNow()?;
    calendar.SetDay(1)?;
    calendar.SetYear(value.year)?;
    calendar.SetMonth(value.month as i32)?;
    calendar.SetDay(value.day as i32)?;
    calendar.SetHour(12)?;
    calendar.SetMinute(0)?;
    calendar.SetSecond(0)?;
    calendar.SetNanosecond(0)?;
    calendar.GetDateTime()
}

fn from_native_date(value: WinDateTime) -> Option<DateTime> {
    let calendar = gregorian_calendar().ok()?;
    calendar.SetDateTime(value).ok()?;
    Some(DateTime::date(
        calendar.Year().ok()?,
        calendar.Month().ok()? as u8,
        calendar.Day().ok()? as u8,
    ))
}

fn gregorian_calendar() -> windows_core::Result<Calendar> {
    let calendar = Calendar::new()?;
    calendar.ChangeCalendarSystem(&CalendarIdentifiers::Gregorian()?)?;
    Ok(calendar)
}

pub(crate) fn to_native_time(value: Time) -> TimeSpan {
    TimeSpan {
        Duration: i64::from(value.minutes_since_midnight()) * TICKS_PER_MINUTE,
    }
}

pub(crate) fn from_native_time(value: TimeSpan) -> Time {
    Time::from_minutes_since_midnight(value.Duration.div_euclid(TICKS_PER_MINUTE))
}

pub(crate) fn local_now() -> DateTime {
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
