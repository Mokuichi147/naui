//! Time picking (WinUI 3's native `TimePicker`).
//!
//! The control is the `TimePicker` of the [`naui_winui3`] projection itself, sharing
//! creation and display tightening with [`crate::date_picker`]. This module builds
//! the widget that handles only times.
//!
//! WinUI 3's `TimePicker` has no minimum or maximum, so the range is enforced only
//! by naui's clamping (`MinuteIncrement` is not used either; it stays at 1-minute
//! steps).

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use naui_core::{Result, Time};
use naui_winui3::Microsoft::UI::Xaml::Controls::Control;
use naui_winui3::Microsoft::UI::Xaml::{FrameworkElement, RoutedEventHandler, UIElement};
use windows::Foundation::EventHandler;
use windows_core::Interface;

use naui_winui3::Microsoft::UI::Xaml::Controls::{
    TimePicker as XamlTimePicker, TimePickerValueChangedEventArgs,
};

use crate::date_picker::{
    compact_time_picker, from_native_time, local_now, new_time_picker, to_native_time,
};
use crate::to_error;
use crate::ui_thread::UiThreadCell;
use crate::widgets::{impl_widget, Widget};

type ChangeCallback = Box<dyn FnMut(Time)>;

/// The handler notified when the value changes. The closure is taken out only while
/// it is called, allowing re-entrancy.
#[derive(Clone)]
struct ChangeHandler(Arc<UiThreadCell<Option<ChangeCallback>>>);

impl ChangeHandler {
    fn new() -> Self {
        Self(Arc::new(UiThreadCell::new(None)))
    }

    fn set(&self, f: impl FnMut(Time) + 'static) {
        self.0.with_mut(|slot| *slot = Some(Box::new(f)));
    }

    fn emit(&self, value: Time) {
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

struct TimePickerInner {
    /// The element that goes into the layout. Its content is the same control as
    /// `picker`.
    native: FrameworkElement,
    picker: XamlTimePicker,
    value: Cell<Time>,
    min: Cell<Option<Time>>,
    max: Cell<Option<Time>>,
    handler: ChangeHandler,
    /// Ignores notifications from the native side only while writing the value.
    silent: Cell<bool>,
}

/// A control for picking a time of day (WinUI 3's `TimePicker`).
///
/// Its value right after creation is the platform's current time (local time).
#[derive(Clone)]
pub struct TimePicker(Rc<TimePickerInner>);
impl_widget!(TimePicker, native);

impl TimePicker {
    pub(crate) fn new() -> Result<Self> {
        let picker = new_time_picker()?;
        let native = picker
            .cast::<FrameworkElement>()
            .map_err(|e| to_error("convert the TimePicker to FrameworkElement", e))?;

        let this = Self(Rc::new(TimePickerInner {
            native,
            picker,
            value: Cell::new(local_now().time_of_day()),
            min: Cell::new(None),
            max: Cell::new(None),
            handler: ChangeHandler::new(),
            silent: Cell::new(false),
        }));

        // 標準テンプレートの最小幅は英語の `AM` / `PM` を想定して広い。
        // 日付ピッカーと同じ幅へそろえる。テンプレートは読み込み後に
        // 作り直されることがあるので、`Loaded` でも当て直す。
        compact_time_picker(&this.0.picker);
        let loaded_picker = Arc::new(UiThreadCell::new(this.0.picker.clone()));
        let loaded = RoutedEventHandler::new(move |_, _| {
            let _ = loaded_picker.try_with_mut(|picker| compact_time_picker(picker));
            Ok(())
        });
        this.0
            .native
            .Loaded(&loaded)
            .map_err(|e| to_error("subscribe to TimePicker Loaded", e))?;

        let target = Arc::new(UiThreadCell::new(Rc::downgrade(&this.0)));
        let changed = EventHandler::<TimePickerValueChangedEventArgs>::new(move |_, _| {
            let _ = target.try_with_mut(|weak| {
                if let Some(inner) = weak.upgrade() {
                    TimePicker(inner).native_changed();
                }
            });
            Ok(())
        });
        this.0
            .picker
            .TimeChanged(&changed)
            .map_err(|e| to_error("subscribe to TimePicker changes", e))?;

        this.write_native(this.value());
        Ok(this)
    }

    /// The currently selected time.
    pub fn value(&self) -> Time {
        self.0.value.get()
    }

    /// Changes the value without notifying. Out-of-range values are clamped to the
    /// ends, and values that are not valid times are normalized.
    pub fn set_value(&self, value: Time) {
        let value = self.clamp(value);
        self.0.value.set(value);
        self.write_native(value);
    }

    /// Sets the selectable range. `None` places no limit on that side.
    ///
    /// If the current value falls outside the range, it is clamped to the end without
    /// notifying. WinUI 3's `TimePicker` has no range, so a chosen time is clamped to
    /// the ends on change.
    pub fn set_range(&self, min: Option<Time>, max: Option<Time>) {
        self.0.min.set(min.map(Time::normalized));
        self.0.max.set(max.map(Time::normalized));
        self.set_value(self.value());
    }

    pub fn set_enabled(&self, enabled: bool) {
        if let Ok(control) = self.0.native.cast::<Control>() {
            let _ = control.SetIsEnabled(enabled);
        }
    }

    /// Called with the new value when the value changes. Setting it again removes the
    /// previous callback.
    pub fn on_change(&self, f: impl FnMut(Time) + 'static) {
        self.0.handler.set(f);
    }

    /// WinUI 3's `TimePicker`. Public as a backend-specific escape hatch.
    pub fn native_picker_element(&self) -> Option<UIElement> {
        self.0.native.cast().ok()
    }

    /// Handles a value change on the native side.
    fn native_changed(&self) {
        if self.0.silent.get() {
            return;
        }
        let Ok(shown) = self.0.picker.Time() else {
            return;
        };
        let accepted = self.clamp(from_native_time(shown));
        // 丸めや範囲で押し戻したときのために、表示は必ず書き直す。
        self.write_native(accepted);
        if accepted == self.value() {
            return;
        }
        self.0.value.set(accepted);
        self.0.handler.emit(accepted);
    }

    fn clamp(&self, value: Time) -> Time {
        value.clamped(self.0.min.get(), self.0.max.get())
    }

    /// Writes the value to the native side. Notifications during this are ignored.
    fn write_native(&self, value: Time) {
        let previous = self.0.silent.replace(true);
        let _ = self.0.picker.SetTime(to_native_time(value));
        self.0.silent.set(previous);
    }
}
