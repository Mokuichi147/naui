//! An always-visible month calendar, composed of `<button>`s in a `<table>`.
//!
//! Browsers have no always-visible calendar (`<input type="date">` only opens one
//! in a popup), so it is composed of DOM parts, as `Pagination` is: a header with
//! previous / next month buttons around the month's name, and a table of day
//! buttons under the weekday names. naui draws nothing itself:
//!
//! - The selected day's number is wrapped in `<mark>` and today's in `<b>`, so the
//!   browser's own default styles show them (and `aria-pressed` /
//!   `aria-current="date"` tell assistive technology).
//! - Days outside the range, and every button while disabled, are `disabled`
//!   buttons.
//! - The month and weekday names come from `Intl.DateTimeFormat` (Gregorian), in
//!   the app's language when it named one, otherwise the browser's. The first day
//!   of the week comes from `Intl.Locale`'s week information, and is Sunday in
//!   browsers that do not provide it.
//!
//! The only styles are for layout (the header in a row, buttons as wide as their
//! column, an invisible button keeping the height of a week without days). The month buttons only show another month; the selected day stays.
//! The arrow keys move the selection (left / right by a day, swapped in
//! right-to-left text, up / down by a week, Page Up / Page Down by a month).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use js_sys::{Array, Date, Function, Intl, Object, Reflect};
use naui_core::{app_language, days_in_month, ui_strings, DatePickerMode, DateTime, Result};
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{Document, Element, HtmlButtonElement, HtmlElement, KeyboardEvent};

use crate::widgets::{create, impl_widget, Listener, ValueHandler, Widget};

/// Only the date is picked, as in a `DatePicker` that shows only the date.
const MODE: DatePickerMode = DatePickerMode::Date;

/// Weeks in the table. Always six, so the calendar keeps its height from month to
/// month (as the native calendars do).
const WEEKS: usize = 6;

const MS_PER_DAY: f64 = 86_400_000.0;

fn style(element: &HtmlElement, property: &str, value: &str) {
    let _ = element.style().set_property(property, value);
}

/// The `Intl.DateTimeFormat`s the calendar writes its text with.
struct Formats {
    /// "November 2026".
    month: Intl::DateTimeFormat,
    /// "22".
    day: Intl::DateTimeFormat,
    /// "Sunday, November 22, 2026", the accessible name of a day.
    full: Intl::DateTimeFormat,
}

struct CalendarInner {
    root: HtmlElement,
    document: Document,
    previous: HtmlButtonElement,
    next: HtmlButtonElement,
    heading: HtmlElement,
    table: HtmlElement,
    body: HtmlElement,
    formats: Formats,
    /// The first day of the week, 0 = Sunday … 6 = Saturday.
    first_weekday: u8,
    /// The current value, **including the time**, which the calendar does not show.
    value: Cell<DateTime>,
    /// The month the table shows, as (year, month).
    shown: Cell<(i32, u8)>,
    min: Cell<Option<DateTime>>,
    max: Cell<Option<DateTime>>,
    enabled: Cell<bool>,
    on_change: ValueHandler<DateTime>,
    /// The subscriptions for the month buttons, the day clicks and the keys.
    listeners: RefCell<Vec<Listener>>,
    /// `on_file_drop`.
    file_drop: crate::file_drop::FileDrop,
}

/// An always-visible month calendar (`<button>`s in a `<table>`).
///
/// Its value right after creation is the browser's current date and time (local
/// time).
#[derive(Clone)]
pub struct Calendar(Rc<CalendarInner>);
impl_widget!(Calendar, root);

impl Calendar {
    pub(crate) fn new(document: &Document) -> Result<Self> {
        let root: HtmlElement = create(document, "div")?.unchecked_into();
        style(&root, "display", "inline-flex");
        style(&root, "flex-direction", "column");
        style(&root, "gap", "4px");
        let _ = root.set_attribute("role", "group");

        let header: HtmlElement = create(document, "div")?.unchecked_into();
        style(&header, "display", "flex");
        style(&header, "align-items", "center");
        style(&header, "gap", "4px");
        let strings = ui_strings();
        // ‹ と › は双方向で鏡映される文字なので、右から左の配置で左右が入れ替わる
        // と向きも入れ替わる。
        let previous = month_button(document, "\u{2039}", strings.previous_month)?;
        let next = month_button(document, "\u{203A}", strings.next_month)?;
        let heading: HtmlElement = create(document, "span")?.unchecked_into();
        style(&heading, "flex", "1");
        style(&heading, "text-align", "center");
        let _ = heading.set_attribute("aria-live", "polite");
        append(&header, &previous)?;
        append(&header, &heading)?;
        append(&header, &next)?;

        let table: HtmlElement = create(document, "table")?.unchecked_into();
        let head = create(document, "thead")?;
        let body: HtmlElement = create(document, "tbody")?.unchecked_into();
        append(&table, &head)?;
        append(&table, &body)?;
        append(&root, &header)?;
        append(&root, &table)?;

        let locales = locales();
        let formats = Formats {
            month: format(&locales, &[("year", "numeric"), ("month", "long")]),
            day: format(&locales, &[("day", "numeric")]),
            full: format(&locales, &[("dateStyle", "full")]),
        };
        let first_weekday = first_weekday(&formats.month);
        write_weekdays(document, &head, &locales, first_weekday)?;

        let now = now();
        let this = Self(Rc::new(CalendarInner {
            root,
            document: document.clone(),
            previous,
            next,
            heading,
            table,
            body,
            formats,
            first_weekday,
            value: Cell::new(now),
            shown: Cell::new((now.year, now.month)),
            min: Cell::new(None),
            max: Cell::new(None),
            enabled: Cell::new(true),
            on_change: ValueHandler::default(),
            listeners: RefCell::new(Vec::new()),
            file_drop: Default::default(),
        }));
        this.render();

        let mut listeners = Vec::new();
        for (button, step) in [(&this.0.previous, -1), (&this.0.next, 1)] {
            listeners.push(Listener::attach(button.as_ref(), "click", {
                let weak = Rc::downgrade(&this.0);
                move || {
                    if let Some(inner) = weak.upgrade() {
                        Calendar(inner).show_month(step);
                    }
                }
            })?);
        }
        // 日のボタンは描き直すたびに作り直すので、表でまとめて受ける。
        listeners.push(Listener::attach_event(this.0.table.as_ref(), "click", {
            let weak = Rc::downgrade(&this.0);
            move |event| {
                if let Some(inner) = weak.upgrade() {
                    Calendar(inner).day_clicked(&event);
                }
            }
        })?);
        listeners.push(Listener::attach_event(this.0.table.as_ref(), "keydown", {
            let weak = Rc::downgrade(&this.0);
            move |event| {
                if let Some(inner) = weak.upgrade() {
                    Calendar(inner).key_down(&event);
                }
            }
        })?);
        *this.0.listeners.borrow_mut() = listeners;
        Ok(this)
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
        self.0.shown.set((value.year, value.month));
        self.render();
    }

    /// Sets the selectable range of days. `None` places no limit on that side. Only
    /// the dates are compared; the times of `min` and `max` are ignored.
    ///
    /// If the current value falls outside the range, it is clamped to the end without
    /// notifying. The days outside the range are disabled buttons, and the month
    /// buttons stop at the range's months.
    pub fn set_range(&self, min: Option<DateTime>, max: Option<DateTime>) {
        self.0.min.set(min.map(DateTime::normalized));
        self.0.max.set(max.map(DateTime::normalized));
        self.set_value(self.value());
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.0.enabled.set(enabled);
        self.render();
    }

    /// Called with the new value when the user selects another day. Setting it again
    /// removes the previous callback.
    pub fn on_change(&self, f: impl FnMut(DateTime) + 'static) {
        self.0.on_change.set(f);
    }

    /// Selects the day of `value` the way the user does by clicking it: the
    /// calendar shows its month, the day is selected, and
    /// [`on_change`](Self::on_change) is called with the new value (the time part
    /// stays as it was). A date outside the range is clamped to the end first.
    /// Nothing is called when the day is already selected. Does nothing while
    /// disabled.
    ///
    /// The day's `<button>` is clicked with the DOM's `click()`. For tests and
    /// automation.
    pub fn pick(&self, value: DateTime) {
        if !self.0.enabled.get() {
            return;
        }
        let target = self.clamp(MODE.apply(self.value(), value));
        self.0.shown.set((target.year, target.month));
        self.render();
        if let Some(button) = self.day_button(target.day) {
            button.click();
        }
    }

    /// The previous and next month `<button>`s.
    pub fn native_month_buttons(&self) -> (HtmlButtonElement, HtmlButtonElement) {
        (self.0.previous.clone(), self.0.next.clone())
    }

    /// The `<table>` of days.
    pub fn native_table(&self) -> HtmlElement {
        self.0.table.clone()
    }

    fn clamp(&self, value: DateTime) -> DateTime {
        MODE.clamp(value, self.0.min.get(), self.0.max.get())
    }

    /// Selects a day as the user does, and notifies if the value changed.
    fn select(&self, year: i32, month: u8, day: u8, focus: bool) {
        let accepted = self.clamp(self.value().with_date(year, month, day));
        let changed = accepted != self.value();
        self.0.value.set(accepted);
        self.0.shown.set((accepted.year, accepted.month));
        self.render();
        if focus {
            if let Some(button) = self.day_button(accepted.day) {
                let _ = button.focus();
            }
        }
        if changed {
            self.0.on_change.emit(accepted);
        }
    }

    /// Shows the month `step` months away, stopping at the range's months.
    fn show_month(&self, step: i32) {
        let (year, month) = self.0.shown.get();
        let shown = add_months(year, month, step);
        if self.month_is_selectable(shown) {
            self.0.shown.set(shown);
            self.render();
        }
    }

    fn day_clicked(&self, event: &web_sys::Event) {
        let Some(button) = event
            .target()
            .and_then(|target| target.dyn_into::<Element>().ok())
            .and_then(|element| element.closest("button[data-day]").ok().flatten())
        else {
            return;
        };
        let Some(day) = button
            .get_attribute("data-day")
            .and_then(|day| day.parse().ok())
        else {
            return;
        };
        let (year, month) = self.0.shown.get();
        self.select(year, month, day, false);
    }

    fn key_down(&self, event: &web_sys::Event) {
        let Some(event) = event.dyn_ref::<KeyboardEvent>() else {
            return;
        };
        if !self.0.enabled.get() || event.alt_key() || event.ctrl_key() || event.meta_key() {
            return;
        }
        let rtl = self.is_right_to_left();
        let value = self.value();
        let target = match event.key().as_str() {
            "ArrowLeft" => add_days(value, if rtl { 1 } else { -1 }),
            "ArrowRight" => add_days(value, if rtl { -1 } else { 1 }),
            "ArrowUp" => add_days(value, -7),
            "ArrowDown" => add_days(value, 7),
            "PageUp" => {
                let (year, month) = add_months(value.year, value.month, -1);
                value.with_date(year, month, value.day)
            }
            "PageDown" => {
                let (year, month) = add_months(value.year, value.month, 1);
                value.with_date(year, month, value.day)
            }
            _ => return,
        };
        event.prevent_default();
        self.select(target.year, target.month, target.day, true);
    }

    fn is_right_to_left(&self) -> bool {
        web_sys::window()
            .and_then(|window| window.get_computed_style(&self.0.root).ok().flatten())
            .and_then(|style| style.get_property_value("direction").ok())
            .is_some_and(|direction| direction == "rtl")
    }

    /// Whether any day of the month is inside the range.
    fn month_is_selectable(&self, (year, month): (i32, u8)) -> bool {
        let first = DateTime::date(year, month, 1);
        let last = DateTime::date(year, month, days_in_month(year, month));
        (1..=9999).contains(&year)
            && self.0.min.get().is_none_or(|min| last >= date_only(min))
            && self.0.max.get().is_none_or(|max| first <= date_only(max))
    }

    fn day_is_selectable(&self, date: DateTime) -> bool {
        self.0.min.get().is_none_or(|min| date >= date_only(min))
            && self.0.max.get().is_none_or(|max| date <= date_only(max))
    }

    /// The `<button>` of a day in the month shown.
    fn day_button(&self, day: u8) -> Option<HtmlElement> {
        self.0
            .body
            .query_selector(&format!("button[data-day=\"{day}\"]"))
            .ok()
            .flatten()
            .map(JsCast::unchecked_into)
    }

    /// Writes the month shown into the header and the table.
    fn render(&self) {
        let inner = &self.0;
        let enabled = inner.enabled.get();
        let (year, month) = inner.shown.get();
        let first = days_from_civil(year, month, 1);
        let title = format_date(&inner.formats.month, first);
        inner.heading.set_text_content(Some(&title));
        let _ = inner.table.set_attribute("aria-label", &title);
        inner
            .previous
            .set_disabled(!enabled || !self.month_is_selectable(add_months(year, month, -1)));
        inner
            .next
            .set_disabled(!enabled || !self.month_is_selectable(add_months(year, month, 1)));

        let value = inner.value.get();
        let today = now();
        let selected = (value.year, value.month) == (year, month);
        // 選んだ日がこの月に無いときは、Tab で表へ入れるよう選べる最初の日を入口にする。
        let entry = if selected {
            Some(value.day)
        } else {
            (1..=days_in_month(year, month))
                .find(|&day| self.day_is_selectable(DateTime::date(year, month, day)))
        };
        let offset = (weekday(first) + 7 - inner.first_weekday) % 7;
        let doc = &inner.document;
        inner.body.set_text_content(None);
        for week in 0..WEEKS {
            let Ok(row) = create(doc, "tr") else {
                return;
            };
            for column in 0..7 {
                let Ok(cell) = create(doc, "td") else {
                    return;
                };
                let index = (week * 7 + column) as i64 - i64::from(offset);
                if (0..i64::from(days_in_month(year, month))).contains(&index) {
                    let day = index as u8 + 1;
                    let date = DateTime::date(year, month, day);
                    let is_selected = selected && day == value.day;
                    let is_today = (today.year, today.month, today.day) == (year, month, day);
                    if let Ok(button) = self.day_cell_button(
                        date,
                        first + index,
                        is_selected,
                        is_today,
                        enabled && self.day_is_selectable(date),
                        entry == Some(day),
                    ) {
                        let _ = cell.append_child(&button);
                    }
                }
                let _ = row.append_child(&cell);
            }
            // 日の無い週も、ほかの週と同じ高さを取る (月によって表の高さが
            // 変わらないように)。見えない押せないボタンを 1 つ置く。
            if i64::from(week as u8) * 7 - i64::from(offset)
                >= i64::from(days_in_month(year, month))
            {
                if let (Ok(placeholder), Some(cell)) = (create(doc, "button"), row.first_child()) {
                    let placeholder: HtmlButtonElement = placeholder.unchecked_into();
                    placeholder.set_type("button");
                    placeholder.set_disabled(true);
                    placeholder.set_tab_index(-1);
                    placeholder.set_text_content(Some("0"));
                    let _ = placeholder.set_attribute("aria-hidden", "true");
                    style(&placeholder, "visibility", "hidden");
                    let _ = cell.append_child(&placeholder);
                }
            }
            let _ = inner.body.append_child(&row);
        }
    }

    fn day_cell_button(
        &self,
        date: DateTime,
        days: i64,
        selected: bool,
        today: bool,
        enabled: bool,
        entry: bool,
    ) -> Result<HtmlButtonElement> {
        let doc = &self.0.document;
        let button: HtmlButtonElement = create(doc, "button")?.unchecked_into();
        button.set_type("button");
        style(&button, "width", "100%");
        let _ = button.set_attribute("data-day", &date.day.to_string());
        let _ = button.set_attribute("aria-label", &format_date(&self.0.formats.full, days));
        let _ = button.set_attribute("aria-pressed", if selected { "true" } else { "false" });
        if today {
            let _ = button.set_attribute("aria-current", "date");
        }
        // 矢印キーで動くので、Tab で止まるのは 1 日だけにする。
        button.set_tab_index(if entry { 0 } else { -1 });
        button.set_disabled(!enabled);

        // 印はブラウザの既定の見た目に任せる: 選んだ日は <mark>、今日は <b>。
        let number = format_day(&self.0.formats.day, days);
        let mut text_parent: Element = button.clone().unchecked_into();
        for (on, tag) in [(selected, "mark"), (today, "b")] {
            if on {
                let wrapper = create(doc, tag)?;
                append(&text_parent, &wrapper)?;
                text_parent = wrapper;
            }
        }
        text_parent.set_text_content(Some(&number));
        Ok(button)
    }
}

fn month_button(doc: &Document, text: &str, label: &str) -> Result<HtmlButtonElement> {
    let button: HtmlButtonElement = create(doc, "button")?.unchecked_into();
    button.set_type("button");
    button.set_text_content(Some(text));
    let _ = button.set_attribute("aria-label", label);
    let _ = button.set_attribute("title", label);
    Ok(button)
}

fn append(parent: &Element, child: &Element) -> Result<()> {
    parent
        .append_child(child)
        .map(|_| ())
        .map_err(|e| crate::to_error("add to the DOM", e))
}

/// The weekday names in the table's header, starting from the first day of the week.
fn write_weekdays(doc: &Document, head: &Element, locales: &Array, first: u8) -> Result<()> {
    let short = format(locales, &[("weekday", "short")]);
    let long = format(locales, &[("weekday", "long")]);
    let row = create(doc, "tr")?;
    for column in 0..7 {
        // 1970-01-04 は日曜日。
        let days = 3 + i64::from((first + column) % 7);
        let cell = create(doc, "th")?;
        let _ = cell.set_attribute("scope", "col");
        let _ = cell.set_attribute("abbr", &format_date(&long, days));
        cell.set_text_content(Some(&format_date(&short, days)));
        append(&row, &cell)?;
    }
    append(head, &row)
}

/// The locales the text is written in: the app's language if it named one,
/// otherwise the browser's (an empty list).
fn locales() -> Array {
    let locales = Array::new();
    if let Some(language) = app_language() {
        locales.push(&JsValue::from_str(&language));
    }
    locales
}

/// An `Intl.DateTimeFormat` in the Gregorian calendar and UTC (the dates are
/// passed as UTC midnights, so no time zone moves them to another day).
fn format(locales: &Array, fields: &[(&str, &str)]) -> Intl::DateTimeFormat {
    let options = Object::new();
    for (key, value) in [("calendar", "gregory"), ("timeZone", "UTC")]
        .iter()
        .chain(fields)
    {
        let _ = Reflect::set(&options, &(*key).into(), &(*value).into());
    }
    Intl::DateTimeFormat::new(locales, &options)
}

/// Formats the day counted from 1970-01-01.
fn format_date(format: &Intl::DateTimeFormat, days: i64) -> String {
    let date = Date::new(&JsValue::from_f64(days as f64 * MS_PER_DAY));
    let format: Function = format.format();
    format
        .call1(&JsValue::UNDEFINED, &date)
        .ok()
        .and_then(|text| text.as_string())
        .unwrap_or_default()
}

/// The number of the day counted from 1970-01-01, in the locale's digits but
/// without words around it (Japanese writes "24日" for the day alone).
fn format_day(format: &Intl::DateTimeFormat, days: i64) -> String {
    let date = Date::new(&JsValue::from_f64(days as f64 * MS_PER_DAY));
    format
        .format_to_parts(&date)
        .iter()
        .find(|part| {
            Reflect::get(part, &"type".into())
                .ok()
                .and_then(|kind| kind.as_string())
                .as_deref()
                == Some("day")
        })
        .and_then(|part| Reflect::get(&part, &"value".into()).ok())
        .and_then(|value| value.as_string())
        .unwrap_or_else(|| civil_from_days(days).2.to_string())
}

/// The first day of the week of the locale the text is written in, from
/// `Intl.Locale`'s `getWeekInfo()` (or the older `weekInfo`). Sunday where the
/// browser does not provide it.
fn first_weekday(format: &Intl::DateTimeFormat) -> u8 {
    const SUNDAY: u8 = 0;
    let read = || -> Option<u8> {
        let tag = Reflect::get(&format.resolved_options(), &"locale".into()).ok()?;
        let intl = Reflect::get(&js_sys::global(), &"Intl".into()).ok()?;
        let constructor: Function = Reflect::get(&intl, &"Locale".into())
            .ok()?
            .dyn_into()
            .ok()?;
        let locale = Reflect::construct(&constructor, &Array::of1(&tag)).ok()?;
        let info = match Reflect::get(&locale, &"getWeekInfo".into())
            .ok()
            .and_then(|f| f.dyn_into::<Function>().ok())
        {
            Some(get) => get.call0(&locale).ok()?,
            None => Reflect::get(&locale, &"weekInfo".into()).ok()?,
        };
        // 1 が月曜日、7 が日曜日。
        let first = Reflect::get(&info, &"firstDay".into()).ok()?.as_f64()?;
        (1.0..=7.0).contains(&first).then_some(first as u8 % 7)
    };
    read().unwrap_or(SUNDAY)
}

/// The value with its time dropped, for comparing days.
fn date_only(value: DateTime) -> DateTime {
    DateTime::date(value.year, value.month, value.day)
}

/// The month `step` months away from (year, month).
fn add_months(year: i32, month: u8, step: i32) -> (i32, u8) {
    let index = year * 12 + i32::from(month) - 1 + step;
    (index.div_euclid(12), (index.rem_euclid(12) + 1) as u8)
}

/// The date `step` days away. The time is kept, and the result is clamped to the
/// years naui handles.
fn add_days(value: DateTime, step: i64) -> DateTime {
    let (year, month, day) =
        civil_from_days(days_from_civil(value.year, value.month, value.day) + step);
    value.with_date(year, month, day)
}

/// The weekday of the day counted from 1970-01-01, 0 = Sunday … 6 = Saturday.
fn weekday(days: i64) -> u8 {
    // 1970-01-01 は木曜日。
    (days + 4).rem_euclid(7) as u8
}

/// Days from 1970-01-01 to the date in the proleptic Gregorian calendar (Howard
/// Hinnant's `days_from_civil`).
fn days_from_civil(year: i32, month: u8, day: u8) -> i64 {
    let year = i64::from(year) - i64::from(month <= 2);
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let month = i64::from(month);
    let day_of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + i64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// The inverse of [`days_from_civil`].
fn civil_from_days(days: i64) -> (i32, u8, u8) {
    let days = days + 719_468;
    let era = days.div_euclid(146_097);
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * shifted_month + 2) / 5 + 1) as u8;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    } as u8;
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (
        year.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32,
        month,
        day,
    )
}

/// The browser's current date and time (local time).
fn now() -> DateTime {
    let now = Date::new_0();
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
