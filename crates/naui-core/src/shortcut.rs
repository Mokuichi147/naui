//! Keyboard shortcuts: the key combinations given to menu items, windows and
//! widgets.
//!
//! A [`Shortcut`] is a key plus modifiers. The same value goes to a menu bar item
//! ([`MenuItem::shortcut`](crate::MenuItem::shortcut)), to a window and to any
//! widget (`on_shortcut`). A menu takes fewer keys than windows and widgets do (see
//! [`Shortcut::is_valid_for_menu`]).
//!
//! Where a key goes, and in which order, is described at [`deliver_key`].

use std::cell::RefCell;
use std::rc::Rc;

use crate::{EventResponse, Key, KeyEvent, KeyHandler, Modifiers};

/// A keyboard shortcut: a key plus modifiers.
///
/// **The primary modifier differs by platform**, as in "⌘S / Ctrl+S" (⌘ on macOS,
/// Ctrl on Windows and Linux, either on the Web), so naui takes only the meaning of
/// the combination and maps it to that platform's notation. ⇧ and ⌥ (Alt) can be
/// added.
///
/// What can be given:
///
/// - A character key: one alphanumeric character **with the primary modifier**
///   ([`new`](Self::new)). Without it the key would be taken away from text input.
/// - A function key from F1 to F24, on its own or with modifiers
///   ([`function`](Self::function)).
/// - A named key (Enter, Escape, Tab, Backspace, Delete, the arrow keys, Home, End,
///   Page Up, Page Down), on its own or with modifiers
///   ([`from_key`](Self::from_key)). Space needs the primary modifier, like a
///   character. Windows and widgets take these; menus do not
///   ([`is_valid_for_menu`](Self::is_valid_for_menu)).
///
/// Shortcuts that are not [valid](Self::is_valid) (symbol keys, or a character key
/// without the primary modifier) never fire.
///
/// ```
/// # use naui_core::{Key, Shortcut};
/// let save = Shortcut::new('s');
/// assert_eq!(save.label(), "Ctrl+S");
/// assert_eq!(save.accelerator(), "<Control>s");
///
/// let redo = Shortcut::new('z').shift(true);
/// assert_eq!(redo.label(), "Ctrl+Shift+Z");
///
/// let full_screen = Shortcut::function(11);
/// assert_eq!(full_screen.label(), "F11");
/// assert_eq!(full_screen.accelerator(), "F11");
///
/// let delete = Shortcut::from_key(Key::Delete);
/// assert!(delete.is_valid());
/// assert!(!delete.is_valid_for_menu());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Shortcut {
    /// The key in the combination. [`Key::Character`] (stored lowercase),
    /// [`Key::F`] or one of the named keys.
    pub key: Key,
    /// Whether the primary modifier (⌘ on macOS, Ctrl elsewhere) is included.
    pub primary: bool,
    /// Whether ⇧ (Shift) is included.
    pub shift: bool,
    /// Whether ⌥ / Alt is included.
    pub alt: bool,
}

/// The name `Shortcut` had while only menu items took one.
#[deprecated(since = "0.20.0", note = "renamed to `Shortcut`")]
pub type MenuShortcut = Shortcut;

impl Shortcut {
    /// Creates the combination of the primary modifier and `key`.
    ///
    /// `key` is one alphanumeric character. Uppercase is normalized to lowercase (use
    /// [`shift`](Self::shift) to include ⇧).
    pub fn new(key: char) -> Self {
        Self {
            key: Key::Character(key.to_ascii_lowercase()),
            primary: true,
            shift: false,
            alt: false,
        }
    }

    /// Creates a function key on its own. `function(5)` is F5.
    ///
    /// Modifiers can be added with [`primary`](Self::primary), [`shift`](Self::shift)
    /// and [`alt`](Self::alt). Numbers outside 1 to 24 make an invalid shortcut.
    pub fn function(number: u8) -> Self {
        Self::from_key(Key::F(number))
    }

    /// Creates `key` on its own, without modifiers (`from_key(Key::Escape)`).
    ///
    /// Meant for the named keys and function keys. A character key or Space made this
    /// way is invalid until [`primary`](Self::primary) is added. Letters are
    /// lowercased.
    pub fn from_key(key: Key) -> Self {
        let key = match key {
            Key::Character(c) => Key::Character(c.to_ascii_lowercase()),
            key => key,
        };
        Self {
            key,
            primary: false,
            shift: false,
            alt: false,
        }
    }

    /// Sets whether the primary modifier is included (included by default for
    /// characters, not for other keys).
    pub fn primary(mut self, primary: bool) -> Self {
        self.primary = primary;
        self
    }

    /// Sets whether ⇧ (Shift) is included (not included by default).
    pub fn shift(mut self, shift: bool) -> Self {
        self.shift = shift;
        self
    }

    /// Sets whether ⌥ / Alt is included (not included by default).
    pub fn alt(mut self, alt: bool) -> Self {
        self.alt = alt;
        self
    }

    /// Whether a window or a widget can take it (`on_shortcut`): one alphanumeric
    /// character or Space with the primary modifier, F1 to F24, or a named key.
    ///
    /// A character key without the primary modifier would take the key away from text
    /// input, so it is not valid.
    ///
    /// ```
    /// # use naui_core::{Key, Shortcut};
    /// assert!(Shortcut::new('s').is_valid());
    /// assert!(Shortcut::function(5).is_valid());
    /// assert!(Shortcut::from_key(Key::Escape).is_valid());
    /// assert!(Shortcut::from_key(Key::ArrowUp).alt(true).is_valid());
    /// assert!(!Shortcut::new('/').is_valid());
    /// assert!(!Shortcut::new('s').primary(false).is_valid());
    /// assert!(!Shortcut::from_key(Key::Space).is_valid());
    /// assert!(!Shortcut::function(25).is_valid());
    /// ```
    pub fn is_valid(self) -> bool {
        match self.key {
            Key::Character(c) => self.primary && c.is_ascii_alphanumeric(),
            Key::Space => self.primary,
            Key::F(number) => (1..=24).contains(&number),
            Key::Enter
            | Key::Escape
            | Key::Tab
            | Key::Backspace
            | Key::Delete
            | Key::ArrowUp
            | Key::ArrowDown
            | Key::ArrowLeft
            | Key::ArrowRight
            | Key::Home
            | Key::End
            | Key::PageUp
            | Key::PageDown => true,
            Key::Other => false,
        }
    }

    /// Whether every platform's menu bar can register it: one alphanumeric character
    /// with the primary modifier, or F1 to F24.
    ///
    /// Menu items with any other shortcut still appear, without the shortcut.
    ///
    /// ```
    /// # use naui_core::{Key, Shortcut};
    /// assert!(Shortcut::new('s').is_valid_for_menu());
    /// assert!(Shortcut::function(5).is_valid_for_menu());
    /// assert!(!Shortcut::from_key(Key::Delete).is_valid_for_menu());
    /// assert!(!Shortcut::new('/').is_valid_for_menu());
    /// ```
    pub fn is_valid_for_menu(self) -> bool {
        matches!(self.key, Key::Character(_) | Key::F(_)) && self.is_valid()
    }

    /// The string passed to macOS (AppKit)'s `keyEquivalent`, for menu items.
    ///
    /// ⌘, ⇧ and ⌥ are specified with `keyEquivalentModifierMask`, so only the
    /// character goes here. Function keys are AppKit's private-use characters
    /// (`NSF1FunctionKey` and on). Empty for keys menus do not take.
    ///
    /// ```
    /// # use naui_core::Shortcut;
    /// assert_eq!(Shortcut::new('S').key_equivalent(), "s");
    /// assert_eq!(Shortcut::function(1).key_equivalent(), "\u{F704}");
    /// ```
    pub fn key_equivalent(self) -> String {
        match self.key {
            Key::Character(c) => c.to_ascii_lowercase().to_string(),
            // NSF1FunctionKey は 0xF704 で、F35 まで続き番号になっている。
            Key::F(number) => char::from_u32(0xF703 + u32::from(number))
                .map(String::from)
                .unwrap_or_default(),
            _ => String::new(),
        }
    }

    /// The notation read by Linux (GTK4)'s `gtk_accelerator_parse`, for menu items.
    /// Only the modifiers for keys menus do not take.
    ///
    /// ```
    /// # use naui_core::Shortcut;
    /// let s = Shortcut::new('o').shift(true).alt(true);
    /// assert_eq!(s.accelerator(), "<Control><Shift><Alt>o");
    /// assert_eq!(Shortcut::function(5).shift(true).accelerator(), "<Shift>F5");
    /// ```
    pub fn accelerator(self) -> String {
        let mut out = String::new();
        if self.primary {
            out.push_str("<Control>");
        }
        if self.shift {
            out.push_str("<Shift>");
        }
        if self.alt {
            out.push_str("<Alt>");
        }
        match self.key {
            Key::Character(c) => out.push(c.to_ascii_lowercase()),
            Key::F(number) => out.push_str(&format!("F{number}")),
            _ => {}
        }
        out
    }

    /// The Windows virtual-key code, for menu items. `0` for shortcuts that are not
    /// [valid for a menu](Self::is_valid_for_menu).
    ///
    /// `VK_A`..`VK_Z` and `VK_0`..`VK_9` have the same values as ASCII uppercase
    /// letters and digits, and `VK_F1`..`VK_F24` run from `0x70`.
    ///
    /// ```
    /// # use naui_core::Shortcut;
    /// assert_eq!(Shortcut::new('s').virtual_key(), 0x53);
    /// assert_eq!(Shortcut::new('1').virtual_key(), 0x31);
    /// assert_eq!(Shortcut::function(5).virtual_key(), 0x74);
    /// ```
    pub fn virtual_key(self) -> i32 {
        if !self.is_valid_for_menu() {
            return 0;
        }
        match self.key {
            Key::Character(c) => c.to_ascii_uppercase() as i32,
            Key::F(number) => 0x6F + i32::from(number),
            _ => 0,
        }
    }

    /// The key's own name as shown on screen (`S`, `F5`, `Delete`).
    fn key_label(self) -> String {
        match self.key {
            Key::Character(c) => c.to_ascii_uppercase().to_string(),
            Key::F(number) => format!("F{number}"),
            Key::Enter => "Enter".into(),
            Key::Escape => "Esc".into(),
            Key::Tab => "Tab".into(),
            Key::Backspace => "Backspace".into(),
            Key::Delete => "Delete".into(),
            Key::Space => "Space".into(),
            Key::ArrowUp => "Up".into(),
            Key::ArrowDown => "Down".into(),
            Key::ArrowLeft => "Left".into(),
            Key::ArrowRight => "Right".into(),
            Key::Home => "Home".into(),
            Key::End => "End".into(),
            Key::PageUp => "PageUp".into(),
            Key::PageDown => "PageDown".into(),
            Key::Other => String::new(),
        }
    }

    /// The key's name in the DOM's `KeyboardEvent.key` (`S`, `F5`, `ArrowUp`).
    fn web_key_name(self) -> String {
        match self.key {
            Key::Escape => "Escape".into(),
            Key::Space => "Space".into(),
            Key::ArrowUp => "ArrowUp".into(),
            Key::ArrowDown => "ArrowDown".into(),
            Key::ArrowLeft => "ArrowLeft".into(),
            Key::ArrowRight => "ArrowRight".into(),
            _ => self.key_label(),
        }
    }

    /// The text shown on screen (in the form `Ctrl+Shift+S`).
    ///
    /// macOS and GTK4 render the right edge of menu items themselves, so for menus
    /// only the platforms where naui owns the display (Windows and the Web) use this.
    pub fn label(self) -> String {
        let mut out = String::new();
        if self.primary {
            out.push_str("Ctrl+");
        }
        if self.shift {
            out.push_str("Shift+");
        }
        if self.alt {
            out.push_str("Alt+");
        }
        out.push_str(&self.key_label());
        out
    }

    /// The notation of WAI-ARIA's `aria-keyshortcuts` (`Control+Shift+S`, `F5`,
    /// `ArrowUp`).
    ///
    /// WAI-ARIA requires key names to match the DOM's `KeyboardEvent.key`, so it is
    /// separate from the display-oriented [`label`](Self::label).
    ///
    /// ```
    /// # use naui_core::{Key, Shortcut};
    /// assert_eq!(Shortcut::new('s').shift(true).aria_key_shortcuts(), "Control+Shift+S");
    /// assert_eq!(Shortcut::function(11).aria_key_shortcuts(), "F11");
    /// assert_eq!(Shortcut::from_key(Key::Escape).aria_key_shortcuts(), "Escape");
    /// ```
    pub fn aria_key_shortcuts(self) -> String {
        let mut out = String::new();
        if self.primary {
            out.push_str("Control+");
        }
        if self.shift {
            out.push_str("Shift+");
        }
        if self.alt {
            out.push_str("Alt+");
        }
        out.push_str(&self.web_key_name());
        out
    }

    /// Whether the pressed key is this shortcut.
    ///
    /// `key` is the pressed key's name in the form of the DOM's `KeyboardEvent.key`
    /// (a character, case-insensitive, `F5` or `ArrowUp`). `primary` means that
    /// either ⌘ or Ctrl is held. Invalid shortcuts never match.
    ///
    /// ```
    /// # use naui_core::Shortcut;
    /// let s = Shortcut::new('s');
    /// assert!(s.matches("S", true, false, false));
    /// assert!(!s.matches("s", false, false, false)); // no modifier
    /// assert!(!s.matches("s", true, true, false));   // extra ⇧
    ///
    /// let f5 = Shortcut::function(5);
    /// assert!(f5.matches("F5", false, false, false));
    /// assert!(!f5.matches("F5", true, false, false)); // extra primary modifier
    /// ```
    pub fn matches(self, key: &str, primary: bool, shift: bool, alt: bool) -> bool {
        self.matches_parts(Key::from_web_key(key), primary, shift, alt)
    }

    /// Whether the key event is this shortcut.
    ///
    /// The primary modifier is read with [`Modifiers::primary`]. Holding the
    /// platform's other modifier as well (Ctrl on macOS, the Windows key or Super on
    /// Windows and Linux) makes it a different combination. Invalid shortcuts never
    /// match.
    ///
    /// ```
    /// # use naui_core::{Key, KeyEvent, Modifiers, Shortcut};
    /// let primary = if cfg!(target_os = "macos") {
    ///     Modifiers { meta: true, ..Modifiers::default() }
    /// } else {
    ///     Modifiers { control: true, ..Modifiers::default() }
    /// };
    /// let event = KeyEvent::new(Key::Character('s'), primary);
    /// assert!(Shortcut::new('s').matches_event(&event));
    /// assert!(!Shortcut::new('s').shift(true).matches_event(&event));
    /// ```
    pub fn matches_event(self, event: &KeyEvent) -> bool {
        let modifiers = event.modifiers;
        !other_platform_modifier(&modifiers)
            && self.matches_parts(
                event.key,
                modifiers.primary(),
                modifiers.shift,
                modifiers.alt,
            )
    }

    fn matches_parts(self, key: Key, primary: bool, shift: bool, alt: bool) -> bool {
        if !self.is_valid() || primary != self.primary || shift != self.shift || alt != self.alt {
            return false;
        }
        match (self.key, key) {
            (Key::Character(a), Key::Character(b)) => a.eq_ignore_ascii_case(&b),
            (a, b) => a == b,
        }
    }

    /// The same shortcut with its letter lowercased (fields can be set directly).
    fn normalized(self) -> Self {
        Self {
            key: Self::from_key(self.key).key,
            ..self
        }
    }
}

/// Whether the modifier that is neither the primary one nor ⇧ / ⌥ is held: Ctrl on
/// macOS, the Windows key or Super elsewhere. The Web counts both Ctrl and ⌘ as the
/// primary modifier, so nothing is left over.
fn other_platform_modifier(modifiers: &Modifiers) -> bool {
    if cfg!(target_arch = "wasm32") {
        false
    } else if cfg!(target_os = "macos") {
        modifiers.control
    } else {
        modifiers.meta
    }
}

type ShortcutCallback = dyn FnMut();
type ShortcutSlot = Rc<RefCell<crate::Slot<ShortcutCallback>>>;

/// The shortcuts given to one window or widget with `on_shortcut`, each with its
/// closure. Shared by all backends.
///
/// A closure is taken out while it is called, so you may replace or remove
/// shortcuts (that one included) from inside the notification.
#[derive(Default)]
pub struct ShortcutHandlers(RefCell<Vec<(Shortcut, ShortcutSlot)>>);

impl ShortcutHandlers {
    /// Gives `shortcut` the closure `f`, replacing the one it had.
    pub fn set(&self, shortcut: Shortcut, f: impl FnMut() + 'static) {
        let shortcut = shortcut.normalized();
        let mut entries = self.0.borrow_mut();
        if let Some((_, slot)) = entries.iter().find(|(s, _)| *s == shortcut) {
            *slot.borrow_mut() = Some(Box::new(f));
        } else {
            entries.push((shortcut, Rc::new(RefCell::new(Some(Box::new(f))))));
        }
    }

    /// Removes `shortcut`. Nothing happens if it was not given.
    pub fn remove(&self, shortcut: Shortcut) {
        let shortcut = shortcut.normalized();
        // 閉包は借用の外で捨てる (捨てるときに何かを呼んでも二重借用にならない)。
        let removed: Vec<_> = {
            let mut entries = self.0.borrow_mut();
            let (removed, kept) = entries.drain(..).partition(|(s, _)| *s == shortcut);
            *entries = kept;
            removed
        };
        drop(removed);
    }

    /// Whether no shortcuts are given.
    pub fn is_empty(&self) -> bool {
        self.0.try_borrow().is_ok_and(|entries| entries.is_empty())
    }

    /// The shortcuts given, in the order they were first given.
    pub fn shortcuts(&self) -> Vec<Shortcut> {
        self.0.borrow().iter().map(|(s, _)| *s).collect()
    }

    /// Runs the closure of the valid shortcut that `event` presses. `Handled` if there
    /// was one (also when it is already running, from a key it sent itself).
    pub fn emit(&self, event: &KeyEvent) -> EventResponse {
        let slot = self
            .0
            .borrow()
            .iter()
            .find(|(s, _)| s.matches_event(event))
            .map(|(_, slot)| slot.clone());
        let Some(slot) = slot else {
            return EventResponse::Continue;
        };
        let Some(mut f) = slot.borrow_mut().take() else {
            return EventResponse::Handled;
        };
        f();
        let mut current = slot.borrow_mut();
        // 呼び出しの中で差し替えられていたら新しいほうを残す。
        if current.is_none() {
            *current = Some(f);
        }
        EventResponse::Handled
    }
}

/// What a widget or window has for keys: its `on_key_down` and its shortcuts.
#[doc(hidden)]
#[derive(Clone, Default)]
pub struct KeyTarget {
    pub key_down: Option<Rc<KeyHandler>>,
    pub shortcuts: Option<Rc<ShortcutHandlers>>,
}

impl KeyTarget {
    pub fn is_empty(&self) -> bool {
        self.key_down.is_none() && self.shortcuts.is_none()
    }
}

/// Delivers a key in naui's order. Shared by all backends, which collect the
/// targets: `widgets` from the focused widget outward (only those that have
/// something for keys), then the window.
///
/// 1. The `on_key_down` of the innermost widget that has one.
/// 2. The shortcuts of the widgets, innermost first.
/// 3. The window's `on_key_down`.
/// 4. The window's shortcuts.
///
/// The first `Handled` stops it. The menu bar's shortcuts and the focused control
/// itself come after this, in each backend.
#[doc(hidden)]
pub fn deliver_key(event: &KeyEvent, widgets: &[KeyTarget], window: &KeyTarget) -> EventResponse {
    if let Some(handler) = widgets.iter().find_map(|t| t.key_down.as_ref()) {
        if handler.emit(event) == EventResponse::Handled {
            return EventResponse::Handled;
        }
    }
    for shortcuts in widgets.iter().filter_map(|t| t.shortcuts.as_ref()) {
        if shortcuts.emit(event) == EventResponse::Handled {
            return EventResponse::Handled;
        }
    }
    if let Some(handler) = &window.key_down {
        if handler.emit(event) == EventResponse::Handled {
            return EventResponse::Handled;
        }
    }
    window
        .shortcuts
        .as_ref()
        .map_or(EventResponse::Continue, |s| s.emit(event))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn primary() -> Modifiers {
        if cfg!(target_os = "macos") {
            Modifiers {
                meta: true,
                ..Modifiers::default()
            }
        } else {
            Modifiers {
                control: true,
                ..Modifiers::default()
            }
        }
    }

    fn press(key: Key, modifiers: Modifiers) -> KeyEvent {
        KeyEvent::new(key, modifiers)
    }

    /// Even when passed in uppercase, the key is stored lowercase (⇧ is specified
    /// separately).
    #[test]
    fn shortcut_key_is_lowercased() {
        let s = Shortcut::new('S');
        assert_eq!(s.key, Key::Character('s'));
        assert!(s.primary);
        assert!(!s.shift);
        assert!(!s.alt);
        assert_eq!(s.key_equivalent(), "s");
        assert_eq!(
            Shortcut::from_key(Key::Character('Q')).key,
            Key::Character('q')
        );
    }

    #[test]
    fn shortcut_writes_each_backend_notation() {
        let plain = Shortcut::new('s');
        assert_eq!(plain.accelerator(), "<Control>s");
        assert_eq!(plain.label(), "Ctrl+S");
        assert_eq!(plain.virtual_key(), 0x53);

        let full = Shortcut::new('z').shift(true).alt(true);
        assert_eq!(full.accelerator(), "<Control><Shift><Alt>z");
        assert_eq!(full.label(), "Ctrl+Shift+Alt+Z");
        assert_eq!(full.virtual_key(), 0x5A);
    }

    #[test]
    fn function_keys_write_each_backend_notation() {
        let f5 = Shortcut::function(5);
        assert!(!f5.primary);
        assert_eq!(f5.accelerator(), "F5");
        assert_eq!(f5.label(), "F5");
        assert_eq!(f5.aria_key_shortcuts(), "F5");
        assert_eq!(f5.virtual_key(), 0x74);
        assert_eq!(f5.key_equivalent(), "\u{F708}");

        let ctrl_f12 = Shortcut::function(12).primary(true).shift(true);
        assert_eq!(ctrl_f12.accelerator(), "<Control><Shift>F12");
        assert_eq!(ctrl_f12.label(), "Ctrl+Shift+F12");
        assert_eq!(ctrl_f12.aria_key_shortcuts(), "Control+Shift+F12");
        assert_eq!(Shortcut::function(1).virtual_key(), 0x70);
        assert_eq!(Shortcut::function(24).virtual_key(), 0x87);
        assert_eq!(Shortcut::function(0).virtual_key(), 0);
    }

    #[test]
    fn named_keys_have_labels_but_no_menu_notation() {
        let up = Shortcut::from_key(Key::ArrowUp).alt(true);
        assert_eq!(up.label(), "Alt+Up");
        assert_eq!(up.aria_key_shortcuts(), "Alt+ArrowUp");
        assert_eq!(up.virtual_key(), 0, "menus do not take it");
        assert_eq!(up.key_equivalent(), "");
        let space = Shortcut::from_key(Key::Space).primary(true);
        assert!(space.is_valid());
        assert!(!space.is_valid_for_menu());
        assert_eq!(space.label(), "Ctrl+Space");
        assert_eq!(space.aria_key_shortcuts(), "Control+Space");
        assert!(!Shortcut::from_key(Key::Other).is_valid());
    }

    #[test]
    fn digits_map_to_their_ascii_code() {
        assert_eq!(Shortcut::new('0').virtual_key(), 0x30);
        assert_eq!(Shortcut::new('9').virtual_key(), 0x39);
        assert!(Shortcut::new('7').is_valid());
    }

    /// Non-alphanumeric keys are handled differently per platform, so they have no
    /// virtual key.
    #[test]
    fn non_alphanumeric_keys_are_rejected() {
        let s = Shortcut::new('/');
        assert!(!s.is_valid());
        assert_eq!(s.virtual_key(), 0);
        assert!(!s.matches("/", true, false, false));
    }

    #[test]
    fn matches_requires_the_exact_modifiers() {
        let s = Shortcut::new('z').shift(true);
        assert!(s.matches("Z", true, true, false));
        assert!(s.matches("z", true, true, false));
        // 主修飾キーが無い、⇧ が足りない、⌥ が余っている。
        assert!(!s.matches("z", false, true, false));
        assert!(!s.matches("z", true, false, false));
        assert!(!s.matches("z", true, true, true));
        // 別のキー。
        assert!(!s.matches("y", true, true, false));
    }

    #[test]
    fn function_keys_match_by_name() {
        let s = Shortcut::function(11);
        assert!(s.matches("F11", false, false, false));
        assert!(!s.matches("F1", false, false, false));
        assert!(!s.matches("F111", false, false, false));
        assert!(!s.matches("F11", false, true, false));
        let ctrl = s.primary(true);
        assert!(ctrl.matches("F11", true, false, false));
        assert!(!ctrl.matches("F11", false, false, false));
    }

    /// `KeyboardEvent.key` also delivers names such as "Enter". A character shortcut
    /// does not match them, and a named one does.
    #[test]
    fn matches_reads_named_keys() {
        let s = Shortcut::new('s');
        assert!(!s.matches("Enter", true, false, false));
        assert!(!s.matches("", true, false, false));
        let escape = Shortcut::from_key(Key::Escape);
        assert!(escape.matches("Escape", false, false, false));
        assert!(!escape.matches("Escape", false, true, false));
    }

    #[test]
    fn events_match_with_the_platform_primary_modifier() {
        let s = Shortcut::new('s');
        assert!(s.matches_event(&press(Key::Character('s'), primary())));
        assert!(!s.matches_event(&press(Key::Character('s'), Modifiers::default())));
        // 主修飾キーではないほうの Ctrl / ⌘ が加わると別の組み合わせ。
        let both = Modifiers {
            control: true,
            meta: true,
            ..Modifiers::default()
        };
        assert!(!s.matches_event(&press(Key::Character('s'), both)));
        let delete = Shortcut::from_key(Key::Delete);
        assert!(delete.matches_event(&press(Key::Delete, Modifiers::default())));
        assert!(!delete.matches_event(&press(Key::Backspace, Modifiers::default())));
    }

    #[test]
    fn handlers_replace_remove_and_normalize() {
        use std::cell::Cell;
        let handlers = ShortcutHandlers::default();
        assert!(handlers.is_empty());
        let count = Rc::new(Cell::new(0));
        let event = press(Key::Character('k'), primary());
        assert_eq!(handlers.emit(&event), EventResponse::Continue);
        handlers.set(Shortcut::new('k'), {
            let count = count.clone();
            move || count.set(count.get() + 1)
        });
        // 大文字で直接組み立てても同じショートカットとして差し替わる。
        handlers.set(
            Shortcut {
                key: Key::Character('K'),
                primary: true,
                shift: false,
                alt: false,
            },
            {
                let count = count.clone();
                move || count.set(count.get() + 10)
            },
        );
        assert_eq!(handlers.shortcuts(), vec![Shortcut::new('k')]);
        assert_eq!(handlers.emit(&event), EventResponse::Handled);
        assert_eq!(count.get(), 10);
        handlers.remove(Shortcut::new('K'));
        assert!(handlers.is_empty());
        assert_eq!(handlers.emit(&event), EventResponse::Continue);
    }

    #[test]
    fn invalid_shortcuts_never_fire() {
        let handlers = ShortcutHandlers::default();
        handlers.set(Shortcut::new('s').primary(false), || panic!("fired"));
        let event = press(Key::Character('s'), Modifiers::default());
        assert_eq!(handlers.emit(&event), EventResponse::Continue);
    }

    #[test]
    fn handlers_can_change_themselves_while_running() {
        use std::cell::Cell;
        let handlers = Rc::new(ShortcutHandlers::default());
        let count = Rc::new(Cell::new(0));
        let escape = Shortcut::from_key(Key::Escape);
        handlers.set(escape, {
            let handlers = handlers.clone();
            let count = count.clone();
            move || {
                count.set(count.get() + 1);
                // 自分を外し、別のショートカットを足す。
                handlers.remove(escape);
                handlers.set(Shortcut::function(1), || {});
            }
        });
        let event = press(Key::Escape, Modifiers::default());
        assert_eq!(handlers.emit(&event), EventResponse::Handled);
        assert_eq!(handlers.emit(&event), EventResponse::Continue);
        assert_eq!(count.get(), 1);
        assert_eq!(handlers.shortcuts(), vec![Shortcut::function(1)]);
    }

    /// The order every backend follows: the focused widget's `on_key_down`, the
    /// widgets' shortcuts from the inside out, the window's `on_key_down`, then the
    /// window's shortcuts.
    #[test]
    fn keys_are_delivered_from_the_inside_out() {
        let log = Rc::new(RefCell::new(Vec::<&'static str>::new()));
        let key_down = |name: &'static str, response: EventResponse| {
            let handler = Rc::new(KeyHandler::default());
            let log = log.clone();
            handler.set(move |_| {
                log.borrow_mut().push(name);
                response
            });
            Some(handler)
        };
        let shortcut = |name: &'static str| {
            let handlers = Rc::new(ShortcutHandlers::default());
            let log = log.clone();
            handlers.set(Shortcut::from_key(Key::Escape), move || {
                log.borrow_mut().push(name)
            });
            Some(handlers)
        };
        let event = press(Key::Escape, Modifiers::default());
        let inner = KeyTarget {
            key_down: key_down("inner key_down", EventResponse::Continue),
            shortcuts: shortcut("inner shortcut"),
        };
        let outer = KeyTarget {
            key_down: key_down("outer key_down", EventResponse::Continue),
            shortcuts: shortcut("outer shortcut"),
        };
        let window = KeyTarget {
            key_down: key_down("window key_down", EventResponse::Continue),
            shortcuts: shortcut("window shortcut"),
        };
        assert_eq!(
            deliver_key(&event, &[inner.clone(), outer.clone()], &window),
            EventResponse::Handled
        );
        assert_eq!(
            *log.borrow(),
            ["inner key_down", "inner shortcut"],
            "only the innermost on_key_down, and the first shortcut wins"
        );

        log.borrow_mut().clear();
        let bare_inner = KeyTarget {
            shortcuts: None,
            ..inner.clone()
        };
        let bare_outer = KeyTarget {
            shortcuts: None,
            ..outer.clone()
        };
        deliver_key(&event, &[bare_inner.clone(), bare_outer.clone()], &window);
        assert_eq!(
            *log.borrow(),
            ["inner key_down", "window key_down", "window shortcut"]
        );

        log.borrow_mut().clear();
        let stop = KeyTarget {
            key_down: key_down("window stops", EventResponse::Handled),
            shortcuts: window.shortcuts.clone(),
        };
        deliver_key(&event, &[bare_inner, outer], &stop);
        assert_eq!(*log.borrow(), ["inner key_down", "outer shortcut"]);

        log.borrow_mut().clear();
        assert_eq!(
            deliver_key(&event, &[], &stop),
            EventResponse::Handled,
            "the window's on_key_down comes before its shortcuts"
        );
        assert_eq!(*log.borrow(), ["window stops"]);
    }
}
