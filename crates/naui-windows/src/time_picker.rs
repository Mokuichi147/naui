//! 時刻の選択 (WinUI 3 のネイティブ `TimePicker`)。
//!
//! `winio-winui3` は WinUI 3 API の subset で `TimePicker` を投影していない。
//! 公開 WinRT インターフェイスの投影は [`crate::date_picker`] にあるものを
//! そのまま使い、このモジュールは時刻だけを扱うウィジェットを組み立てる。
//! コントロール自体は `XamlReader` から生成される本物の WinUI 3 コントロール。
//!
//! WinUI 3 の `TimePicker` に下限・上限は無いので、範囲は naui 側の丸めだけで
//! 守る (`MinuteIncrement` も使わず、1 分刻みのまま)。

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use naui_core::{Result, Time};
use windows::Foundation::TypedEventHandler;
use windows_core::Interface;
use winui3::Microsoft::UI::Xaml::Controls::Control;
use winui3::Microsoft::UI::Xaml::{FrameworkElement, RoutedEventHandler, UIElement};

use crate::date_picker::{
    compact_time_picker, from_native_time, load_time_picker, local_now, to_native_time,
    NativeTimePicker, NativeTimePickerValueChangedEventArgs,
};
use crate::to_error;
use crate::ui_thread::UiThreadCell;
use crate::widgets::{impl_widget, Widget};

type ChangeCallback = Box<dyn FnMut(Time)>;

/// 値が変わったことの通知先。呼ぶ間だけクロージャを取り出して再入を許す。
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
    /// レイアウトへ載る要素。中身は `picker` と同じコントロール。
    native: FrameworkElement,
    picker: NativeTimePicker,
    value: Cell<Time>,
    min: Cell<Option<Time>>,
    max: Cell<Option<Time>>,
    handler: ChangeHandler,
    /// 値を書き込んでいる間だけ、ネイティブからの通知を無視する。
    silent: Cell<bool>,
}

/// 時刻を選ばせるコントロール (WinUI 3 の `TimePicker`)。
///
/// 作った直後の値は、その環境の現在時刻 (ローカル時刻)。
#[derive(Clone)]
pub struct TimePicker(Rc<TimePickerInner>);
impl_widget!(TimePicker, native);

impl TimePicker {
    pub(crate) fn new() -> Result<Self> {
        let picker = load_time_picker()?;
        let native = picker
            .cast::<FrameworkElement>()
            .map_err(|e| to_error("TimePicker のレイアウト要素化", e))?;

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
            .map_err(|e| to_error("TimePicker の読み込み購読", e))?;

        let target = Arc::new(UiThreadCell::new(Rc::downgrade(&this.0)));
        let changed =
            TypedEventHandler::<NativeTimePicker, NativeTimePickerValueChangedEventArgs>::new(
                move |_, _| {
                    let _ = target.try_with_mut(|weak| {
                        if let Some(inner) = weak.upgrade() {
                            TimePicker(inner).native_changed();
                        }
                    });
                    Ok(())
                },
            );
        this.0
            .picker
            .time_changed(&changed)
            .map_err(|e| to_error("TimePicker の変更購読", e))?;

        this.write_native(this.value());
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
    /// WinUI 3 の `TimePicker` に範囲は無いので、選ばれた時刻は変更時に
    /// 端へ寄せる。
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

    /// 値が変わったときに、変わったあとの値で呼ばれる。
    /// 設定し直すと以前のコールバックは外れる。
    pub fn on_change(&self, f: impl FnMut(Time) + 'static) {
        self.0.handler.set(f);
    }

    /// WinUI 3 の `TimePicker`。バックエンド固有の脱出口として公開している。
    pub fn native_picker_element(&self) -> Option<UIElement> {
        self.0.native.cast().ok()
    }

    /// ネイティブ側で値が変わったときの処理。
    fn native_changed(&self) {
        if self.0.silent.get() {
            return;
        }
        let Ok(shown) = self.0.picker.time() else {
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

    /// ネイティブへ値を書く。この間の通知は無視する。
    fn write_native(&self, value: Time) {
        let previous = self.0.silent.replace(true);
        let _ = self.0.picker.set_time(to_native_time(value));
        self.0.silent.set(previous);
    }
}
