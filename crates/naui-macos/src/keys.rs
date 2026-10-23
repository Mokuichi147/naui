//! `on_key_down` (text-input widgets and `Window`).
//!
//! One local `NSEvent` monitor is installed for the whole app, receiving `keyDown`
//! **before AppKit dispatches it**. It walks up the parent views from the first
//! responder of the window the key came to, and notifies the first registered
//! widget found, then the window. With `Handled`, the monitor discards the event,
//! so it reaches neither the field (inserting a newline, for example) nor menu
//! shortcuts.
//!
//! Editing in text fields is done by the field editor (a shared `NSTextView`), but
//! the field editor is placed inside the `NSTextField` being edited, so walking up
//! the parents reaches the field.
//!
//! Keys during composition (the first responder `hasMarkedText`) belong to the IME
//! and are not delivered.
//!
//! The registry holds views and windows by weak reference. Once all of a widget's
//! handles are dropped and the view is freed, its handler is removed from the table
//! too.

use std::cell::RefCell;
use std::ptr::NonNull;
use std::rc::Rc;

use block2::RcBlock;
use naui_core::{EventResponse, Key, KeyEvent, KeyHandler, Modifiers};
use objc2::rc::{Retained, Weak};
use objc2::runtime::{AnyObject, NSObjectProtocol};
use objc2::{msg_send, sel, MainThreadMarker};
use objc2_app_kit::{NSEvent, NSEventMask, NSEventModifierFlags, NSResponder, NSView, NSWindow};

type Entries<T> = RefCell<Vec<(Weak<T>, Rc<KeyHandler>)>>;

thread_local! {
    static VIEWS: Entries<NSView> = const { RefCell::new(Vec::new()) };
    static WINDOWS: Entries<NSWindow> = const { RefCell::new(Vec::new()) };
    /// The monitor's return value. The monitor lives only while it is held.
    static MONITOR: RefCell<Option<Retained<AnyObject>>> = const { RefCell::new(None) };
}

/// Sets the handler on the widget's view. Each call replaces it.
pub(crate) fn set_view_handler(view: &NSView, f: impl FnMut(&KeyEvent) -> EventResponse + 'static) {
    VIEWS.with(|views| handler_for(&mut views.borrow_mut(), view).set(f));
    ensure_monitor();
}

/// Sets the handler on the window. Each call replaces it.
pub(crate) fn set_window_handler(
    window: &NSWindow,
    f: impl FnMut(&KeyEvent) -> EventResponse + 'static,
) {
    WINDOWS.with(|windows| handler_for(&mut windows.borrow_mut(), window).set(f));
    ensure_monitor();
}

/// The handler for `target`, created if absent. Entries for objects that are gone
/// are cleaned up here too.
fn handler_for<T: objc2::Message>(
    entries: &mut Vec<(Weak<T>, Rc<KeyHandler>)>,
    target: &T,
) -> Rc<KeyHandler> {
    entries.retain(|(weak, _)| weak.load().is_some());
    if let Some((_, handler)) = entries.iter().find(|(weak, _)| {
        weak.load()
            .is_some_and(|other| std::ptr::eq(&*other, target))
    }) {
        return handler.clone();
    }
    let handler = Rc::new(KeyHandler::default());
    entries.push((Weak::from(target), handler.clone()));
    handler
}

fn lookup<T: objc2::Message>(
    entries: &'static std::thread::LocalKey<Entries<T>>,
    target: &T,
) -> Option<Rc<KeyHandler>> {
    entries.with(|entries| {
        entries
            .borrow()
            .iter()
            .find(|(weak, _)| {
                weak.load()
                    .is_some_and(|other| std::ptr::eq(&*other, target))
            })
            .map(|(_, handler)| handler.clone())
    })
}

fn ensure_monitor() {
    MONITOR.with(|monitor| {
        if monitor.borrow().is_some() {
            return;
        }
        let block = RcBlock::new(|event: NonNull<NSEvent>| -> *mut NSEvent {
            // SAFETY: AppKit が渡す、生きている NSEvent。
            let event_ref = unsafe { event.as_ref() };
            match dispatch(event_ref) {
                EventResponse::Handled => std::ptr::null_mut(),
                EventResponse::Continue => event.as_ptr(),
            }
        });
        let token = unsafe {
            NSEvent::addLocalMonitorForEventsMatchingMask_handler(NSEventMask::KeyDown, &block)
        };
        *monitor.borrow_mut() = token;
    });
}

/// Passes the arriving key to the focused widget, then the window.
pub(crate) fn dispatch(event: &NSEvent) -> EventResponse {
    // モニターはメインスレッドで呼ばれる。
    let Some(mtm) = MainThreadMarker::new() else {
        return EventResponse::Continue;
    };
    let Some(window) = event.window(mtm) else {
        return EventResponse::Continue;
    };
    let responder = window.firstResponder();
    if responder.as_deref().is_some_and(is_composing) {
        return EventResponse::Continue;
    }
    let key_event = to_key_event(event);

    let mut view = responder.and_then(|responder| responder.downcast::<NSView>().ok());
    while let Some(current) = view {
        if let Some(handler) = lookup(&VIEWS, &current) {
            if handler.emit(&key_event) == EventResponse::Handled {
                return EventResponse::Handled;
            }
            break;
        }
        view = unsafe { current.superview() };
    }
    lookup(&WINDOWS, &window).map_or(EventResponse::Continue, |handler| handler.emit(&key_event))
}

/// Whether the IME is in the middle of composing (`NSTextInputClient`'s
/// `hasMarkedText`).
fn is_composing(responder: &NSResponder) -> bool {
    if !responder.respondsToSelector(sel!(hasMarkedText)) {
        return false;
    }
    unsafe { msg_send![responder, hasMarkedText] }
}

/// Maps an AppKit key to a naui key.
fn to_key_event(event: &NSEvent) -> KeyEvent {
    let flags = event.modifierFlags();
    let modifiers = Modifiers {
        shift: flags.contains(NSEventModifierFlags::Shift),
        control: flags.contains(NSEventModifierFlags::Control),
        alt: flags.contains(NSEventModifierFlags::Option),
        meta: flags.contains(NSEventModifierFlags::Command),
    };
    KeyEvent::new(key_of(event), modifiers).repeat(event.isARepeat())
}

/// Named keys are read by virtual key code (`kVK_*`, independent of the layout),
/// and others by their character with modifiers removed.
fn key_of(event: &NSEvent) -> Key {
    match event.keyCode() {
        36 | 76 => return Key::Enter, // Return / テンキーの Enter
        53 => return Key::Escape,
        48 => return Key::Tab,
        51 => return Key::Backspace,
        117 => return Key::Delete,
        49 => return Key::Space,
        126 => return Key::ArrowUp,
        125 => return Key::ArrowDown,
        123 => return Key::ArrowLeft,
        124 => return Key::ArrowRight,
        115 => return Key::Home,
        119 => return Key::End,
        116 => return Key::PageUp,
        121 => return Key::PageDown,
        code => {
            if let Some(number) = function_key(code) {
                return Key::F(number);
            }
        }
    }
    event
        .charactersIgnoringModifiers()
        .and_then(|text| text.to_string().chars().next())
        .map_or(Key::Other, Key::character)
}

fn function_key(code: u16) -> Option<u8> {
    const CODES: [u16; 12] = [122, 120, 99, 118, 96, 97, 98, 100, 101, 109, 103, 111];
    CODES
        .iter()
        .position(|&c| c == code)
        .map(|index| index as u8 + 1)
}

/// Adds `on_key_down` to text-input widgets.
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
                let view = <$t as crate::widgets::Widget>::native_view(self);
                crate::keys::set_view_handler(&view, f);
            }
        }
    };
}

pub(crate) use impl_key_down;
