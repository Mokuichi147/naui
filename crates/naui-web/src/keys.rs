//! `on_key_down` and `on_shortcut` (widgets and `Window`).
//!
//! One `keydown` listener is attached to the `document` **in the capture phase**,
//! so it runs before any listener on the element the key goes to (naui's own, such
//! as `SearchInput`'s Enter, and the menu bar's, which listens on the `document` in
//! the bubbling phase). It walks up from the event's target, collects the widgets
//! that have an `on_key_down` or shortcuts, and delivers the key in naui's order
//! ([`naui_core::deliver_key`]): the innermost `on_key_down`, the widgets' shortcuts
//! from the inside out, the window's `on_key_down`, then the window's shortcuts.
//! With `Handled`, the default action is prevented and the event goes no further.
//!
//! The window is the one the key came from. Keys from places belonging to no window
//! (when `<body>` has focus, for example) go to every shown window in the order they
//! were created, until one takes it; keys from a `<dialog>` (`Dialog`, `Sheet`) go
//! to no window, as on the other platforms, where those are windows of their own.
//!
//! Keys during composition (`isComposing`, and the `keyCode` 229 Safari attaches
//! during composition) belong to the IME and are not delivered. Modifier keys
//! pressed on their own are not delivered either (the same as macOS).
//!
//! The registry holds widgets weakly: an `on_key_down` handler belongs to its
//! widget, and the shortcuts are kept with a `WeakRef` to the element, so they go
//! away once the element is collected.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use naui_core::{
    EventResponse, Key, KeyEvent, KeyHandler, KeyTarget, Modifiers, Shortcut, ShortcutHandlers,
};
use wasm_bindgen::prelude::wasm_bindgen;
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{Document, Element, KeyboardEvent};

use crate::widgets::Listener;

/// The `keyCode` attached to keys the IME is processing.
const IME_PROCESS_KEY_CODE: u32 = 229;

#[wasm_bindgen]
extern "C" {
    /// JavaScript's `WeakRef` (not in `js-sys`).
    #[wasm_bindgen(js_name = WeakRef)]
    type JsWeakRef;

    #[wasm_bindgen(constructor, js_class = "WeakRef")]
    fn new(target: &JsValue) -> JsWeakRef;

    #[wasm_bindgen(method, js_class = "WeakRef", js_name = deref)]
    fn target(this: &JsWeakRef) -> JsValue;
}

thread_local! {
    /// The widgets with an `on_key_down`. The handler belongs to the widget.
    static KEY_DOWNS: RefCell<Vec<(Element, Weak<KeyHandler>)>> = const { RefCell::new(Vec::new()) };
    /// The widgets with shortcuts, by a weak reference to the element.
    static SHORTCUTS: RefCell<Vec<(JsWeakRef, Rc<ShortcutHandlers>)>> =
        const { RefCell::new(Vec::new()) };
    /// The windows, in the order they were created.
    static WINDOWS: RefCell<Vec<(Element, Weak<WindowKeys>)>> = const { RefCell::new(Vec::new()) };
    /// The `document`'s capture listener. Attached once.
    static LISTENER: RefCell<Option<Listener>> = const { RefCell::new(None) };
}

/// A text-input widget's `on_key_down` handler.
#[derive(Default)]
pub(crate) struct KeyDown {
    handler: Rc<KeyHandler>,
    registered: Cell<bool>,
}

impl KeyDown {
    /// Receives keys on the widget's element.
    pub(crate) fn set_on_element(
        &self,
        element: &Element,
        f: impl FnMut(&KeyEvent) -> EventResponse + 'static,
    ) {
        self.handler.set(f);
        if !self.registered.replace(true) {
            KEY_DOWNS.with(|entries| {
                let mut entries = entries.borrow_mut();
                entries.retain(|(_, handler)| handler.strong_count() > 0);
                entries.push((element.clone(), Rc::downgrade(&self.handler)));
            });
        }
        ensure_listener(element.owner_document().as_ref());
    }
}

/// What a window has for keys: its `on_key_down` and its shortcuts.
#[derive(Default)]
pub(crate) struct WindowKeys {
    key_down: Rc<KeyHandler>,
    shortcuts: Rc<ShortcutHandlers>,
}

impl WindowKeys {
    /// Registers the window's element. Called once, when the window is created.
    pub(crate) fn register(self: &Rc<Self>, window: &Element) {
        WINDOWS.with(|entries| {
            let mut entries = entries.borrow_mut();
            entries.retain(|(_, keys)| keys.strong_count() > 0);
            entries.push((window.clone(), Rc::downgrade(self)));
        });
        ensure_listener(window.owner_document().as_ref());
    }

    pub(crate) fn set_key_down(&self, f: impl FnMut(&KeyEvent) -> EventResponse + 'static) {
        self.key_down.set(f);
    }

    pub(crate) fn set_shortcut(&self, shortcut: Shortcut, f: impl FnMut() + 'static) {
        self.shortcuts.set(shortcut, f);
    }

    pub(crate) fn remove_shortcut(&self, shortcut: Shortcut) {
        self.shortcuts.remove(shortcut);
    }

    fn target(&self) -> KeyTarget {
        KeyTarget {
            key_down: Some(self.key_down.clone()),
            shortcuts: Some(self.shortcuts.clone()),
        }
    }
}

/// Gives the widget's element a shortcut, replacing the closure it had.
pub(crate) fn set_element_shortcut(
    element: &Element,
    shortcut: Shortcut,
    f: impl FnMut() + 'static,
) {
    let handlers = SHORTCUTS.with(|entries| {
        let mut entries = entries.borrow_mut();
        entries.retain(|(weak, _)| !weak.target().is_undefined());
        if let Some((_, handlers)) = entries.iter().find(|(weak, _)| refers_to(weak, element)) {
            return handlers.clone();
        }
        let handlers = Rc::new(ShortcutHandlers::default());
        entries.push((JsWeakRef::new(element), handlers.clone()));
        handlers
    });
    handlers.set(shortcut, f);
    ensure_listener(element.owner_document().as_ref());
}

/// Removes a shortcut from the widget's element.
pub(crate) fn remove_element_shortcut(element: &Element, shortcut: Shortcut) {
    if let Some(handlers) = shortcuts_of(element) {
        handlers.remove(shortcut);
    }
}

/// Whether `weak` refers to `element`.
fn refers_to(weak: &JsWeakRef, element: &Element) -> bool {
    let element: &JsValue = element.as_ref();
    weak.target() == *element
}

fn shortcuts_of(element: &Element) -> Option<Rc<ShortcutHandlers>> {
    SHORTCUTS.with(|entries| {
        entries
            .borrow()
            .iter()
            .find(|(weak, _)| refers_to(weak, element))
            .map(|(_, handlers)| handlers.clone())
    })
}

fn key_down_of(element: &Element) -> Option<Rc<KeyHandler>> {
    KEY_DOWNS.with(|entries| {
        entries
            .borrow()
            .iter()
            .find(|(other, _)| other == element)
            .and_then(|(_, handler)| handler.upgrade())
    })
}

/// Attaches the `document`'s capture listener, once.
fn ensure_listener(document: Option<&Document>) {
    let Some(document) = document else {
        return;
    };
    LISTENER.with(|listener| {
        if listener.borrow().is_some() {
            return;
        }
        let attached = Listener::attach_capture(document.as_ref(), "keydown", |event| {
            let Some(key) = event.dyn_ref::<KeyboardEvent>() else {
                return;
            };
            let Some(key_event) = to_key_event(key) else {
                return;
            };
            let target = event.target().and_then(|t| t.dyn_into::<Element>().ok());
            if deliver(target.as_ref(), &key_event, None) == EventResponse::Handled {
                event.prevent_default();
                event.stop_immediate_propagation();
            }
        });
        *listener.borrow_mut() = attached.ok();
    });
}

/// Delivers `event` from `start` outward, in naui's order. `window` is the window to
/// deliver to; `None` finds it from `start`.
fn deliver(
    start: Option<&Element>,
    event: &KeyEvent,
    window: Option<&WindowKeys>,
) -> EventResponse {
    let mut widgets = Vec::new();
    let mut current = start.cloned();
    while let Some(element) = current {
        let target = KeyTarget {
            key_down: key_down_of(&element),
            shortcuts: shortcuts_of(&element).filter(|s| !s.is_empty()),
        };
        if !target.is_empty() {
            widgets.push(target);
        }
        current = element.parent_element();
    }
    if let Some(window) = window {
        return naui_core::deliver_key(event, &widgets, &window.target());
    }
    let windows = windows_for(start);
    if windows.is_empty() {
        return naui_core::deliver_key(event, &widgets, &KeyTarget::default());
    }
    // 窓が複数のときは、ウィジェットへ届けるのは最初の 1 回だけ。
    for (index, window) in windows.iter().enumerate() {
        let widgets = if index == 0 { &widgets[..] } else { &[] };
        if naui_core::deliver_key(event, widgets, &window.target()) == EventResponse::Handled {
            return EventResponse::Handled;
        }
    }
    EventResponse::Continue
}

/// The windows a key from `start` goes to: the window it is in, none from a
/// `<dialog>`, and every shown window from anywhere else.
fn windows_for(start: Option<&Element>) -> Vec<Rc<WindowKeys>> {
    let closest = |selector: &str| start.and_then(|e| e.closest(selector).ok().flatten());
    if closest("dialog").is_some() {
        return Vec::new();
    }
    let owner = closest(crate::window::WINDOW_SELECTOR);
    WINDOWS.with(|entries| {
        entries
            .borrow()
            .iter()
            .filter(|(element, _)| match &owner {
                Some(owner) => element == owner,
                // `display: none` の窓 (祖先を含む) は箱を持たない。
                None => element.is_connected() && element.get_client_rects().length() > 0,
            })
            .filter_map(|(_, keys)| keys.upgrade())
            .collect()
    })
}

/// Delivers `event` as if pressed in `window` with focus where it is now, for
/// `Window::send_key`. Starts from the window's element when the focus is outside
/// it.
pub(crate) fn send(window: &Element, keys: &WindowKeys, event: &KeyEvent) -> EventResponse {
    let focused = window
        .owner_document()
        .and_then(|document| document.active_element())
        .filter(|active| window.contains(Some(active)));
    deliver(Some(focused.as_ref().unwrap_or(window)), event, Some(keys))
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
                self.0.key_down.set_on_element(&element, f);
            }
        }
    };
}

pub(crate) use impl_key_down;
