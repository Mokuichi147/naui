//! Values passed through keyboard input (`on_key_down`).
//!
//! Text-input widgets and `Window` report pressed keys as a [`KeyEvent`], and the
//! app returns an [`EventResponse`] saying whether it "handled it here". Returning
//! `Handled` keeps the key from reaching the native control (inserting a newline,
//! for example) and the window.
//!
//! **Keys pressed while an IME is composing are not delivered.** The IME receives
//! the Enter that confirms the composition, so a "send on `Enter`" handler will not
//! send when the user merely confirms a conversion.
//!
//! Modifier keys (⇧, Ctrl, Alt, ⌘ / Windows key / Super) are not delivered when
//! pressed on their own. Check whether they were held through
//! [`KeyEvent::modifiers`] of other keys.

/// A pressed key.
///
/// Character keys are [`Key::Character`], with letters **normalized to lowercase**
/// (the same as [`MenuShortcut`](crate::MenuShortcut)). Check
/// [`Modifiers::shift`] for whether ⇧ was held. Which character a symbol key gives
/// with ⇧ depends on the platform (keyboard layout).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Key {
    Enter,
    Escape,
    Tab,
    Backspace,
    Delete,
    Space,
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
    Home,
    End,
    PageUp,
    PageDown,
    /// A function key. `F(1)` is F1.
    F(u8),
    /// A key that types a character. Letters are lowercase.
    Character(char),
    /// A key that matches none of the above (the modifier keys themselves, media keys
    /// and so on).
    Other,
}

impl Key {
    /// Creates a character key. Letters are lowercased, and a space becomes
    /// [`Key::Space`].
    pub fn character(c: char) -> Self {
        match c {
            ' ' => Self::Space,
            '\r' | '\n' => Self::Enter,
            '\t' => Self::Tab,
            c if c.is_control() => Self::Other,
            c => Self::Character(c.to_lowercase().next().unwrap_or(c)),
        }
    }

    /// Creates one from the name in the Web's `KeyboardEvent.key`.
    ///
    /// ```
    /// # use naui_core::Key;
    /// assert_eq!(Key::from_web_key("Enter"), Key::Enter);
    /// assert_eq!(Key::from_web_key("A"), Key::Character('a'));
    /// assert_eq!(Key::from_web_key("F5"), Key::F(5));
    /// assert_eq!(Key::from_web_key("Shift"), Key::Other);
    /// ```
    pub fn from_web_key(key: &str) -> Self {
        match key {
            "Enter" => Self::Enter,
            "Escape" | "Esc" => Self::Escape,
            "Tab" => Self::Tab,
            "Backspace" => Self::Backspace,
            "Delete" | "Del" => Self::Delete,
            " " | "Spacebar" => Self::Space,
            "ArrowUp" | "Up" => Self::ArrowUp,
            "ArrowDown" | "Down" => Self::ArrowDown,
            "ArrowLeft" | "Left" => Self::ArrowLeft,
            "ArrowRight" | "Right" => Self::ArrowRight,
            "Home" => Self::Home,
            "End" => Self::End,
            "PageUp" => Self::PageUp,
            "PageDown" => Self::PageDown,
            _ => {
                let mut chars = key.chars();
                match (chars.next(), chars.next()) {
                    (Some(c), None) => Self::character(c),
                    _ => function_key(key).unwrap_or(Self::Other),
                }
            }
        }
    }
}

/// Reads `"F1"` to `"F24"` as function keys.
fn function_key(name: &str) -> Option<Key> {
    let number: u8 = name.strip_prefix('F')?.parse().ok()?;
    (1..=24).contains(&number).then_some(Key::F(number))
}

/// The modifier keys that were held.
///
/// `meta` is ⌘ on macOS, the Windows key on Windows and Super on Linux.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Modifiers {
    pub shift: bool,
    pub control: bool,
    pub alt: bool,
    pub meta: bool,
}

impl Modifiers {
    /// Whether the primary modifier (⌘ on macOS, Ctrl on Windows and Linux) was held.
    ///
    /// On the Web either Ctrl or ⌘ counts, without checking the OS (the same treatment
    /// as menu bar shortcuts).
    pub fn primary(&self) -> bool {
        if cfg!(target_arch = "wasm32") {
            self.control || self.meta
        } else if cfg!(target_os = "macos") {
            self.meta
        } else {
            self.control
        }
    }

    /// Whether no modifier keys are held.
    pub fn is_empty(&self) -> bool {
        !(self.shift || self.control || self.alt || self.meta)
    }
}

/// One key press delivered to `on_key_down`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct KeyEvent {
    pub key: Key,
    pub modifiers: Modifiers,
    /// Whether this is a repeat from holding the key down.
    pub repeat: bool,
}

impl KeyEvent {
    pub fn new(key: Key, modifiers: Modifiers) -> Self {
        Self {
            key,
            modifiers,
            repeat: false,
        }
    }

    /// Sets whether this is a repeat from holding the key down.
    pub fn repeat(mut self, repeat: bool) -> Self {
        self.repeat = repeat;
        self
    }
}

/// The reply from `on_key_down`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EventResponse {
    /// Handled here. Not passed to the native control or the window.
    Handled,
    /// Not handled. Passed to the native control as usual.
    #[default]
    Continue,
}

/// Holds one handler for `on_key_down`. Shared by all backends.
///
/// The closure is taken out while it is called, so you may operate on the same
/// widget or replace the handler with `on_key_down` from inside the notification.
/// Keys that arrive re-entrantly (when the handler triggers another key, for
/// example) are treated as `Continue`.
#[derive(Default)]
pub struct KeyHandler(std::cell::RefCell<crate::Slot<KeyCallback>>);

type KeyCallback = dyn FnMut(&KeyEvent) -> EventResponse;

impl KeyHandler {
    pub fn set(&self, f: impl FnMut(&KeyEvent) -> EventResponse + 'static) {
        *self.0.borrow_mut() = Some(Box::new(f));
    }

    /// Whether a handler is set.
    pub fn is_set(&self) -> bool {
        self.0.try_borrow().map_or(true, |slot| slot.is_some())
    }

    pub fn emit(&self, event: &KeyEvent) -> EventResponse {
        let Some(mut f) = self.0.borrow_mut().take() else {
            return EventResponse::Continue;
        };
        let response = f(event);
        let mut slot = self.0.borrow_mut();
        if slot.is_none() {
            *slot = Some(f);
        }
        response
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn characters_are_lowercased() {
        assert_eq!(Key::character('S'), Key::Character('s'));
        assert_eq!(Key::character('あ'), Key::Character('あ'));
        assert_eq!(Key::character(' '), Key::Space);
        assert_eq!(Key::character('\r'), Key::Enter);
        assert_eq!(Key::character('\u{1b}'), Key::Other);
    }

    #[test]
    fn web_names_map_to_keys() {
        assert_eq!(Key::from_web_key("ArrowLeft"), Key::ArrowLeft);
        assert_eq!(Key::from_web_key(" "), Key::Space);
        assert_eq!(Key::from_web_key("F12"), Key::F(12));
        assert_eq!(Key::from_web_key("F99"), Key::Other);
        assert_eq!(Key::from_web_key("Process"), Key::Other);
    }

    #[test]
    fn handler_can_be_replaced_while_running() {
        use std::rc::Rc;
        let handler = Rc::new(KeyHandler::default());
        assert_eq!(
            handler.emit(&KeyEvent::new(Key::Enter, Modifiers::default())),
            EventResponse::Continue,
            "通知先が無ければ Continue"
        );
        handler.set({
            let handler = handler.clone();
            move |_| {
                handler.set(|_| EventResponse::Continue);
                EventResponse::Handled
            }
        });
        let enter = KeyEvent::new(Key::Enter, Modifiers::default());
        assert_eq!(handler.emit(&enter), EventResponse::Handled);
        assert_eq!(
            handler.emit(&enter),
            EventResponse::Continue,
            "差し替えた方が残る"
        );
    }
}
