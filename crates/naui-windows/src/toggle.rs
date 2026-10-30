//! On/off switches (WinUI 3's native `ToggleSwitch`).
//!
//! The control is created directly from the [`naui_winui3`] projection. Drawing the
//! knob, the animation and keyboard navigation are done by WinUI's standard
//! template.
//!
//! The label is shown next to the switch by putting the same text in `OnContent` and
//! `OffContent` (WinUI's default is text that switches between "On" and "Off").

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use naui_core::Result;
use naui_winui3::Microsoft::UI::Xaml::Controls::{Control, TextBlock, ToggleSwitch};
use naui_winui3::Microsoft::UI::Xaml::{RoutedEventHandler, UIElement};
use windows_core::{IInspectable, Interface, HSTRING};

use crate::to_error;
use crate::ui_thread::{HandlerCell, UiThreadCell};
use crate::widgets::{impl_widget, Widget};

/// The handler notified when it is switched.
///
/// WinRT delegates require `Send + Sync`, so it is placed in a [`UiThreadCell`].
/// The closure is taken out only for the duration of the call, so operating on the
/// same switch from inside the notification does not double-borrow.
#[derive(Clone)]
struct ToggleHandler(HandlerCell<dyn FnMut(bool)>);

impl ToggleHandler {
    fn new() -> Self {
        Self(Arc::new(UiThreadCell::new(None)))
    }

    fn set(&self, f: impl FnMut(bool) + 'static) {
        self.0.with_mut(|slot| *slot = Some(Box::new(f)));
    }

    fn emit(&self, on: bool) {
        let Some(Some(mut f)) = self.0.try_with_mut(|slot| slot.take()) else {
            return;
        };
        f(on);
        let _ = self.0.try_with_mut(|slot| {
            if slot.is_none() {
                *slot = Some(f);
            }
        });
    }
}

struct ToggleInner {
    native: ToggleSwitch,
    handler: ToggleHandler,
    /// Changes made by `set_on` do not call `on_toggle`.
    silent: Cell<bool>,
}

/// An on/off switch (`ToggleSwitch`).
#[derive(Clone)]
pub struct Toggle(Rc<ToggleInner>);
impl_widget!(Toggle, native);

impl Toggle {
    pub(crate) fn new(label: &str) -> Result<Self> {
        let native = ToggleSwitch::new().map_err(|e| to_error("create the ToggleSwitch", e))?;
        native
            .SetIsOn(false)
            .map_err(|e| to_error("initialize the ToggleSwitch", e))?;
        // 入り切りのどちらでも同じ文字を出す。切り替わる文字にすると、
        // ほかの環境と読みが変わってしまうため。
        for on in [true, false] {
            let text =
                TextBlock::new().map_err(|e| to_error("create the ToggleSwitch label", e))?;
            text.SetText(&HSTRING::from(label))
                .map_err(|e| to_error("set the ToggleSwitch label text", e))?;
            let content: IInspectable =
                text.cast().map_err(|e| to_error("convert the label", e))?;
            let set = if on {
                native.SetOnContent(&content)
            } else {
                native.SetOffContent(&content)
            };
            set.map_err(|e| to_error("set the ToggleSwitch label", e))?;
        }

        let this = Self(Rc::new(ToggleInner {
            native,
            handler: ToggleHandler::new(),
            silent: Cell::new(false),
        }));
        this.connect()?;
        Ok(this)
    }

    /// Connects WinUI's `Toggled` to the Rust closure.
    ///
    /// `Toggled` also fires when `IsOn` is rewritten, so it is silenced during `set_on`
    /// (as on macOS / GTK / Web, notifications are only for user actions).
    fn connect(&self) -> Result<()> {
        let target = Arc::new(UiThreadCell::new(Rc::downgrade(&self.0)));
        let toggled = RoutedEventHandler::new(move |_sender, _args| {
            let _ = target.try_with_mut(|weak| {
                if let Some(inner) = weak.upgrade() {
                    if !inner.silent.get() {
                        let on = inner.native.IsOn().unwrap_or(false);
                        inner.handler.emit(on);
                    }
                }
            });
            Ok(())
        });
        self.0
            .native
            .Toggled(&toggled)
            .map_err(|e| to_error("subscribe to the ToggleSwitch", e))?;
        Ok(())
    }

    /// Whether it is on.
    pub fn is_on(&self) -> bool {
        self.0.native.IsOn().unwrap_or(false)
    }

    /// Switches it from code. `on_toggle` is not called.
    pub fn set_on(&self, on: bool) {
        self.0.silent.set(true);
        let _ = self.0.native.SetIsOn(on);
        self.0.silent.set(false);
    }

    pub fn set_enabled(&self, enabled: bool) {
        if let Ok(control) = self.0.native.cast::<Control>() {
            let _ = control.SetIsEnabled(enabled);
        }
    }

    /// Called with the new state every time the user switches it.
    pub fn on_toggle(&self, f: impl FnMut(bool) + 'static) {
        self.0.handler.set(f);
    }
}
