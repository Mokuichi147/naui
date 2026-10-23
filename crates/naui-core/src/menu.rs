//! Items and shortcuts for the menu bar (the OS's application menu).
//!
//! A menu bar has a two-level structure: "a sequence of headings" and "the items
//! hanging under each". One heading is a [`MenuSpec`], one row in it is a
//! [`MenuItem`], and the key combination assigned to an item is a
//! [`MenuShortcut`].
//!
//! Items are identified by the order the app owns, and notifications come back as
//! **(heading index, item index counting separators)**.

/// A keyboard shortcut assigned to a menu item.
///
/// Expressed as the primary modifier (⌘ on macOS, Ctrl on Windows, Linux and the
/// Web) plus one alphanumeric character. **The primary modifier differs by
/// platform**, as in "⌘S / Ctrl+S", so naui takes only the meaning of the
/// combination and maps it to that platform's notation. ⇧ and ⌥ (Alt) can be
/// added.
///
/// ```
/// # use naui_core::MenuShortcut;
/// let save = MenuShortcut::new('s');
/// assert_eq!(save.label(), "Ctrl+S");
/// assert_eq!(save.accelerator(), "<Control>s");
///
/// let redo = MenuShortcut::new('z').shift(true);
/// assert_eq!(redo.label(), "Ctrl+Shift+Z");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MenuShortcut {
    /// The character in the combination. One alphanumeric character, stored lowercase.
    pub key: char,
    /// Whether ⇧ (Shift) is included.
    pub shift: bool,
    /// Whether ⌥ / Alt is included.
    pub alt: bool,
}

impl MenuShortcut {
    /// Creates the combination of the primary modifier and `key`.
    ///
    /// `key` is one alphanumeric character. Uppercase is normalized to lowercase (use
    /// [`shift`](Self::shift) to include ⇧).
    pub fn new(key: char) -> Self {
        Self {
            key: key.to_ascii_lowercase(),
            shift: false,
            alt: false,
        }
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

    /// Whether it is usable as a single alphanumeric character. Unusable characters are
    /// handled differently per platform.
    pub fn is_valid(self) -> bool {
        self.key.is_ascii_alphanumeric()
    }

    /// The string passed to macOS (AppKit)'s `keyEquivalent`.
    ///
    /// ⌘, ⇧ and ⌥ are specified with `keyEquivalentModifierMask`, so only the
    /// character goes here.
    ///
    /// ```
    /// # use naui_core::MenuShortcut;
    /// assert_eq!(MenuShortcut::new('S').key_equivalent(), "s");
    /// ```
    pub fn key_equivalent(self) -> String {
        self.key.to_ascii_lowercase().to_string()
    }

    /// The notation read by Linux (GTK4)'s `gtk_accelerator_parse`.
    ///
    /// ```
    /// # use naui_core::MenuShortcut;
    /// let s = MenuShortcut::new('o').shift(true).alt(true);
    /// assert_eq!(s.accelerator(), "<Control><Shift><Alt>o");
    /// ```
    pub fn accelerator(self) -> String {
        let mut out = String::from("<Control>");
        if self.shift {
            out.push_str("<Shift>");
        }
        if self.alt {
            out.push_str("<Alt>");
        }
        out.push(self.key.to_ascii_lowercase());
        out
    }

    /// The Windows virtual-key code. `0` for anything not alphanumeric.
    ///
    /// `VK_A`..`VK_Z` and `VK_0`..`VK_9` have the same values as ASCII uppercase
    /// letters and digits.
    ///
    /// ```
    /// # use naui_core::MenuShortcut;
    /// assert_eq!(MenuShortcut::new('s').virtual_key(), 0x53);
    /// assert_eq!(MenuShortcut::new('1').virtual_key(), 0x31);
    /// ```
    pub fn virtual_key(self) -> i32 {
        if self.is_valid() {
            self.key.to_ascii_uppercase() as i32
        } else {
            0
        }
    }

    /// The text shown on screen (in the form `Ctrl+Shift+S`).
    ///
    /// macOS and GTK4 render the right edge of menu items themselves, so only the
    /// platforms where naui owns the display (Windows and the Web) use this.
    pub fn label(self) -> String {
        let mut out = String::from("Ctrl+");
        if self.shift {
            out.push_str("Shift+");
        }
        if self.alt {
            out.push_str("Alt+");
        }
        out.push(self.key.to_ascii_uppercase());
        out
    }

    /// Whether the pressed key is this shortcut.
    ///
    /// Used on platforms where naui watches the primary modifier itself (Windows and
    /// the Web). `key` is the pressed character, case-insensitive. `primary` means that
    /// either ⌘ or Ctrl is held.
    ///
    /// ```
    /// # use naui_core::MenuShortcut;
    /// let s = MenuShortcut::new('s');
    /// assert!(s.matches("S", true, false, false));
    /// assert!(!s.matches("s", false, false, false)); // no modifier
    /// assert!(!s.matches("s", true, true, false));   // extra ⇧
    /// ```
    pub fn matches(self, key: &str, primary: bool, shift: bool, alt: bool) -> bool {
        primary
            && shift == self.shift
            && alt == self.alt
            && key.len() == self.key.len_utf8()
            && key
                .chars()
                .next()
                .is_some_and(|c| c.eq_ignore_ascii_case(&self.key))
    }
}

/// One item in the menu bar.
///
/// Clicking it runs the command right away. Separators cannot be chosen, so their
/// indices never come back in notifications.
///
/// ```
/// # use naui_core::{MenuItem, MenuShortcut};
/// let items = [
///     MenuItem::new("Open").shortcut(MenuShortcut::new('o')),
///     MenuItem::separator(),
///     MenuItem::new("Save").enabled(false),
/// ];
/// assert!(items[1].is_separator());
/// assert!(!items[2].enabled);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MenuItem {
    /// The text shown on screen. Not used for separators.
    pub label: String,
    /// Whether it can be selected.
    pub enabled: bool,
    /// Whether this is a separator.
    pub separator: bool,
    /// The keyboard shortcut assigned to it. Not used for separators.
    pub shortcut: Option<MenuShortcut>,
}

impl MenuItem {
    /// Creates a clickable item.
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            enabled: true,
            separator: false,
            shortcut: None,
        }
    }

    /// Creates a separator that divides groups of items. It cannot be clicked.
    pub fn separator() -> Self {
        Self {
            label: String::new(),
            enabled: false,
            separator: true,
            shortcut: None,
        }
    }

    /// Sets whether it can be selected (selectable by default).
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Assigns a keyboard shortcut (none by default).
    pub fn shortcut(mut self, shortcut: MenuShortcut) -> Self {
        self.shortcut = Some(shortcut);
        self
    }

    /// Whether this is a separator.
    pub fn is_separator(&self) -> bool {
        self.separator
    }

    /// Creates a list of items from a sequence of strings.
    ///
    /// ```
    /// # use naui_core::MenuItem;
    /// let items = MenuItem::list(["Undo", "Redo"]);
    /// assert_eq!(items.len(), 2);
    /// ```
    pub fn list<I, S>(labels: I) -> Vec<MenuItem>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        labels.into_iter().map(MenuItem::new).collect()
    }
}

impl From<&str> for MenuItem {
    fn from(label: &str) -> Self {
        MenuItem::new(label)
    }
}

impl From<String> for MenuItem {
    fn from(label: String) -> Self {
        MenuItem::new(label)
    }
}

/// One menu in the menu bar (a heading and the items in it).
///
/// ```
/// # use naui_core::{MenuItem, MenuShortcut, MenuSpec};
/// let file = MenuSpec::new(
///     "File",
///     [
///         MenuItem::new("New").shortcut(MenuShortcut::new('n')),
///         MenuItem::separator(),
///         MenuItem::new("Close"),
///     ],
/// );
/// assert_eq!(file.title, "File");
/// assert_eq!(file.len(), 3);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MenuSpec {
    /// The heading shown in the menu bar.
    pub title: String,
    /// The items under the heading. Their order, separators included, is the index.
    pub items: Vec<MenuItem>,
}

impl MenuSpec {
    /// Creates one menu from a heading and items.
    ///
    /// Items can also be passed as plain strings (`["Undo", "Redo"]`).
    pub fn new<I, T>(title: impl Into<String>, items: I) -> Self
    where
        I: IntoIterator<Item = T>,
        T: Into<MenuItem>,
    {
        Self {
            title: title.into(),
            items: items.into_iter().map(Into::into).collect(),
        }
    }

    /// The number of items, separators included.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_item_defaults_to_enabled() {
        let item = MenuItem::new("開く");
        assert_eq!(item.label, "開く");
        assert!(item.enabled);
        assert!(!item.is_separator());
        assert_eq!(item.shortcut, None);
        assert!(!MenuItem::new("保存").enabled(false).enabled);
    }

    #[test]
    fn separator_is_never_pressable() {
        let item = MenuItem::separator();
        assert!(item.is_separator());
        assert!(!item.enabled);
        assert!(item.label.is_empty());
        assert_eq!(item.shortcut, None);
    }

    #[test]
    fn menu_item_list_keeps_order() {
        let items = MenuItem::list(["元に戻す", "やり直す"]);
        assert_eq!(items[0], MenuItem::from("元に戻す"));
        assert_eq!(items[1].label, "やり直す");
    }

    #[test]
    fn menu_spec_takes_labels_or_items() {
        let spec = MenuSpec::new("編集", ["元に戻す", "やり直す"]);
        assert_eq!(spec.len(), 2);
        assert!(!spec.is_empty());
        assert!(MenuSpec::new("空", Vec::<MenuItem>::new()).is_empty());

        let spec = MenuSpec::new("編集", [MenuItem::new("元に戻す"), MenuItem::separator()]);
        assert!(spec.items[1].is_separator());
    }

    /// Even when passed in uppercase, the key is stored lowercase (⇧ is specified
    /// separately).
    #[test]
    fn shortcut_key_is_lowercased() {
        let s = MenuShortcut::new('S');
        assert_eq!(s.key, 's');
        assert!(!s.shift);
        assert!(!s.alt);
        assert_eq!(s.key_equivalent(), "s");
    }

    #[test]
    fn shortcut_writes_each_backend_notation() {
        let plain = MenuShortcut::new('s');
        assert_eq!(plain.accelerator(), "<Control>s");
        assert_eq!(plain.label(), "Ctrl+S");
        assert_eq!(plain.virtual_key(), 0x53);

        let full = MenuShortcut::new('z').shift(true).alt(true);
        assert_eq!(full.accelerator(), "<Control><Shift><Alt>z");
        assert_eq!(full.label(), "Ctrl+Shift+Alt+Z");
        assert_eq!(full.virtual_key(), 0x5A);
    }

    #[test]
    fn digits_map_to_their_ascii_code() {
        assert_eq!(MenuShortcut::new('0').virtual_key(), 0x30);
        assert_eq!(MenuShortcut::new('9').virtual_key(), 0x39);
        assert!(MenuShortcut::new('7').is_valid());
    }

    /// Non-alphanumeric keys are handled differently per platform, so they have no
    /// virtual key.
    #[test]
    fn non_alphanumeric_keys_are_rejected() {
        let s = MenuShortcut::new('/');
        assert!(!s.is_valid());
        assert_eq!(s.virtual_key(), 0);
    }

    #[test]
    fn matches_requires_the_exact_modifiers() {
        let s = MenuShortcut::new('z').shift(true);
        assert!(s.matches("Z", true, true, false));
        assert!(s.matches("z", true, true, false));
        // 主修飾キーが無い、⇧ が足りない、⌥ が余っている。
        assert!(!s.matches("z", false, true, false));
        assert!(!s.matches("z", true, false, false));
        assert!(!s.matches("z", true, true, true));
        // 別のキー。
        assert!(!s.matches("y", true, true, false));
    }

    /// `KeyboardEvent.key` also delivers names such as "Enter". Only single characters
    /// are considered.
    #[test]
    fn matches_ignores_named_keys() {
        let s = MenuShortcut::new('s');
        assert!(!s.matches("Enter", true, false, false));
        assert!(!s.matches("", true, false, false));
    }
}
