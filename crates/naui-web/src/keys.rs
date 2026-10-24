//! `on_key_down` (text-input widgets and `Window`).
//!
//! Widgets attach `keydown` to their own element **in the capture phase**. On the
//! target element, capture listeners are called before regular ones, so the app can
//! decide before naui's own `keydown` (Enter in search input, for example). With
//! `Handled`, the default action is prevented and the event goes to neither other
//! listeners nor the parents.
//!
//! Windows receive it on `document`, and only the window the key bubbled up from
//! receives it (from places belonging to no window, any of them) — the same check
//! as menu bar shortcuts.
//!
//! Keys during composition (`isComposing`, and the `keyCode` 229 Safari attaches
//! during composition) belong to the IME and are not delivered. Modifier keys
//! pressed on their own are not delivered either (the same as macOS).

use std::cell::RefCell;
use std::rc::Rc;

use naui_core::{EventResponse, Key, KeyEvent, KeyHandler, Modifiers, Result};
use wasm_bindgen::JsCast;
use web_sys::{Element, EventTarget, KeyboardEvent};

use crate::widgets::Listener;

/// The `keyCode` attached to keys the IME is processing.
const IME_PROCESS_KEY_CODE: u32 = 229;

/// The `on_key_down` handler and its subscription. The subscription is attached the
/// first time a handler is set.
#[derive(Default)]
pub(crate) struct KeyDown {
    handler: Rc<KeyHandler>,
    listener: RefCell<Option<Listener>>,
}

impl KeyDown {
    /// Receives it on the widget's element.
    pub(crate) fn set_on_element(
        &self,
        target: &EventTarget,
        f: impl FnMut(&KeyEvent) -> EventResponse + 'static,
    ) {
        self.handler.set(f);
        if self.listener.borrow().is_some() {
            return;
        }
        let handler = self.handler.clone();
        let listener = Listener::attach_capture(target, "keydown", move |event| {
            let Some(key) = event.dyn_ref::<KeyboardEvent>() else {
                return;
            };
            if let Some(key_event) = to_key_event(key) {
                if handler.emit(&key_event) == EventResponse::Handled {
                    event.prevent_default();
                    event.stop_immediate_propagation();
                }
            }
        });
        *self.listener.borrow_mut() = listener.ok();
    }

    /// Receives it on `document` as a window. `window` is that window's element.
    pub(crate) fn set_on_window(
        &self,
        document: &EventTarget,
        window: &Element,
        f: impl FnMut(&KeyEvent) -> EventResponse + 'static,
    ) -> Result<()> {
        self.handler.set(f);
        if self.listener.borrow().is_some() {
            return Ok(());
        }
        let handler = self.handler.clone();
        let window = window.clone();
        let listener = Listener::attach_event(document, "keydown", move |event| {
            if !comes_from(&event, &window) {
                return;
            }
            let Some(key) = event.dyn_ref::<KeyboardEvent>() else {
                return;
            };
            if let Some(key_event) = to_key_event(key) {
                if handler.emit(&key_event) == EventResponse::Handled {
                    event.prevent_default();
                    event.stop_immediate_propagation();
                }
            }
        })?;
        *self.listener.borrow_mut() = Some(listener);
        Ok(())
    }
}

/// Whether the key bubbled up from `window`. Keys from places belonging to no
/// window (when `<body>` has focus, for example) are received if nobody has
/// prevented the default action yet.
fn comes_from(event: &web_sys::Event, window: &Element) -> bool {
    if !window.is_connected() {
        return false;
    }
    let owner = event
        .target()
        .and_then(|target| target.dyn_into::<Element>().ok())
        .and_then(|element| {
            element
                .closest(crate::window::WINDOW_SELECTOR)
                .ok()
                .flatten()
        });
    match owner {
        Some(owner) => &owner == window,
        None => !event.default_prevented(),
    }
}

/// Whether the IME is receiving the key (composing, or the `keyCode` 229 Safari
/// attaches when confirming a conversion).
pub(crate) fn is_ime_key(event: &KeyboardEvent) -> bool {
    event.is_composing() || event.key_code() == IME_PROCESS_KEY_CODE
}

/// Whether it is an Enter that confirms the field. An Enter that confirms a
/// conversion does not count.
pub(crate) fn is_submit_key(event: &KeyboardEvent) -> bool {
    event.key() == "Enter" && !is_ime_key(event)
}

/// Maps a browser key to a naui key. `None` for keys during composition and for
/// modifier keys.
fn to_key_event(event: &KeyboardEvent) -> Option<KeyEvent> {
    if is_ime_key(event) {
        return None;
    }
    let key = event.key();
    if matches!(
        key.as_str(),
        "Shift" | "Control" | "Alt" | "AltGraph" | "Meta" | "OS" | "Super" | "Hyper"
    ) {
        return None;
    }
    let modifiers = Modifiers {
        shift: event.shift_key(),
        control: event.ctrl_key(),
        alt: event.alt_key(),
        meta: event.meta_key(),
    };
    Some(KeyEvent::new(Key::from_web_key(&key), modifiers).repeat(event.repeat()))
}

/// Adds `on_key_down` to text-input widgets. The `Inner` must hold a
/// `key_down: KeyDown`.
macro_rules! impl_key_down {
    ($t:ty) => {
        impl $t {
            /// Notified when a key is pressed. Returning `Handled` keeps the key from reaching
            /// the field and the window (so you can stop newlines or confirmation).
            ///
            /// Keys pressed while an IME is composing are not delivered.
            pub fn on_key_down(
                &self,
                f: impl FnMut(&naui_core::KeyEvent) -> naui_core::EventResponse + 'static,
            ) {
                let element = <$t as crate::widgets::Widget>::native_element(self);
                self.0.key_down.set_on_element(element.as_ref(), f);
            }
        }
    };
}

pub(crate) use impl_key_down;
