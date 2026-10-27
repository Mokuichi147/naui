//! Items and shortcuts for the menu bar (the OS's application menu).
//!
//! A menu bar is "a sequence of headings" with "the items hanging under each".
//! One heading is a [`MenuSpec`], one row in it is a [`MenuItem`], and the key
//! combination assigned to an item is a [`MenuShortcut`]. An item can hold a
//! submenu of further items ([`MenuItem::submenu`]), to any depth.
//!
//! Items are identified by **a path**: the heading index followed by the item
//! index at each level, counting separators. `[1, 0]` is the first item under the
//! second heading, and `[1, 2, 0]` is the first item in the submenu that is the
//! third item under the second heading.

use crate::Key;

/// A keyboard shortcut assigned to a menu item.
///
/// Expressed as a key plus modifiers. A character key is one alphanumeric character
/// with the primary modifier (⌘ on macOS, Ctrl on Windows, Linux and the Web), and a
/// function key (F1 to F24) works on its own or with modifiers. **The primary
/// modifier differs by platform**, as in "⌘S / Ctrl+S", so naui takes only the
/// meaning of the combination and maps it to that platform's notation. ⇧ and ⌥
/// (Alt) can be added.
///
/// Shortcuts that are not [valid](Self::is_valid) (symbol keys, or a character key
/// without the primary modifier) are not registered on any platform; the item
/// itself still appears.
///
/// ```
/// # use naui_core::MenuShortcut;
/// let save = MenuShortcut::new('s');
/// assert_eq!(save.label(), "Ctrl+S");
/// assert_eq!(save.accelerator(), "<Control>s");
///
/// let redo = MenuShortcut::new('z').shift(true);
/// assert_eq!(redo.label(), "Ctrl+Shift+Z");
///
/// let full_screen = MenuShortcut::function(11);
/// assert_eq!(full_screen.label(), "F11");
/// assert_eq!(full_screen.accelerator(), "F11");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MenuShortcut {
    /// The key in the combination. [`Key::Character`] (stored lowercase) or
    /// [`Key::F`].
    pub key: Key,
    /// Whether the primary modifier (⌘ on macOS, Ctrl elsewhere) is included.
    pub primary: bool,
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
        Self {
            key: Key::F(number),
            primary: false,
            shift: false,
            alt: false,
        }
    }

    /// Sets whether the primary modifier is included (included by default for
    /// characters, not for function keys).
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

    /// Whether every platform can register it: one alphanumeric character with the
    /// primary modifier, or F1 to F24.
    ///
    /// A character key without the primary modifier would take the key away from text
    /// input, so it is not valid.
    ///
    /// ```
    /// # use naui_core::MenuShortcut;
    /// assert!(MenuShortcut::new('s').is_valid());
    /// assert!(MenuShortcut::function(5).is_valid());
    /// assert!(!MenuShortcut::new('/').is_valid());
    /// assert!(!MenuShortcut::new('s').primary(false).is_valid());
    /// assert!(!MenuShortcut::function(25).is_valid());
    /// ```
    pub fn is_valid(self) -> bool {
        match self.key {
            Key::Character(c) => self.primary && c.is_ascii_alphanumeric(),
            Key::F(number) => (1..=24).contains(&number),
            _ => false,
        }
    }

    /// The string passed to macOS (AppKit)'s `keyEquivalent`.
    ///
    /// ⌘, ⇧ and ⌥ are specified with `keyEquivalentModifierMask`, so only the
    /// character goes here. Function keys are AppKit's private-use characters
    /// (`NSF1FunctionKey` and on).
    ///
    /// ```
    /// # use naui_core::MenuShortcut;
    /// assert_eq!(MenuShortcut::new('S').key_equivalent(), "s");
    /// assert_eq!(MenuShortcut::function(1).key_equivalent(), "\u{F704}");
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

    /// The notation read by Linux (GTK4)'s `gtk_accelerator_parse`.
    ///
    /// ```
    /// # use naui_core::MenuShortcut;
    /// let s = MenuShortcut::new('o').shift(true).alt(true);
    /// assert_eq!(s.accelerator(), "<Control><Shift><Alt>o");
    /// assert_eq!(MenuShortcut::function(5).shift(true).accelerator(), "<Shift>F5");
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

    /// The Windows virtual-key code. `0` for invalid shortcuts.
    ///
    /// `VK_A`..`VK_Z` and `VK_0`..`VK_9` have the same values as ASCII uppercase
    /// letters and digits, and `VK_F1`..`VK_F24` run from `0x70`.
    ///
    /// ```
    /// # use naui_core::MenuShortcut;
    /// assert_eq!(MenuShortcut::new('s').virtual_key(), 0x53);
    /// assert_eq!(MenuShortcut::new('1').virtual_key(), 0x31);
    /// assert_eq!(MenuShortcut::function(5).virtual_key(), 0x74);
    /// ```
    pub fn virtual_key(self) -> i32 {
        if !self.is_valid() {
            return 0;
        }
        match self.key {
            Key::Character(c) => c.to_ascii_uppercase() as i32,
            Key::F(number) => 0x6F + i32::from(number),
            _ => 0,
        }
    }

    /// The key's own name as shown on screen (`S`, `F5`).
    fn key_label(self) -> String {
        match self.key {
            Key::Character(c) => c.to_ascii_uppercase().to_string(),
            Key::F(number) => format!("F{number}"),
            _ => String::new(),
        }
    }

    /// The text shown on screen (in the form `Ctrl+Shift+S`).
    ///
    /// macOS and GTK4 render the right edge of menu items themselves, so only the
    /// platforms where naui owns the display (Windows and the Web) use this.
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

    /// The notation of WAI-ARIA's `aria-keyshortcuts` (`Control+Shift+S`, `F5`).
    ///
    /// WAI-ARIA requires modifier names to match the DOM's `KeyboardEvent.key`, so it
    /// is separate from the display-oriented [`label`](Self::label).
    ///
    /// ```
    /// # use naui_core::MenuShortcut;
    /// assert_eq!(MenuShortcut::new('s').shift(true).aria_key_shortcuts(), "Control+Shift+S");
    /// assert_eq!(MenuShortcut::function(11).aria_key_shortcuts(), "F11");
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
        out.push_str(&self.key_label());
        out
    }

    /// Whether the pressed key is this shortcut.
    ///
    /// Used on platforms where naui watches pressed keys itself (Windows and the Web).
    /// `key` is the pressed key's name in the form of the DOM's `KeyboardEvent.key`
    /// (a character, case-insensitive, or `F5`). `primary` means that either ⌘ or Ctrl
    /// is held. Invalid shortcuts never match.
    ///
    /// ```
    /// # use naui_core::MenuShortcut;
    /// let s = MenuShortcut::new('s');
    /// assert!(s.matches("S", true, false, false));
    /// assert!(!s.matches("s", false, false, false)); // no modifier
    /// assert!(!s.matches("s", true, true, false));   // extra ⇧
    ///
    /// let f5 = MenuShortcut::function(5);
    /// assert!(f5.matches("F5", false, false, false));
    /// assert!(!f5.matches("F5", true, false, false)); // extra primary modifier
    /// ```
    pub fn matches(self, key: &str, primary: bool, shift: bool, alt: bool) -> bool {
        if !self.is_valid() || primary != self.primary || shift != self.shift || alt != self.alt {
            return false;
        }
        match self.key {
            Key::Character(c) => {
                key.len() == c.len_utf8()
                    && key
                        .chars()
                        .next()
                        .is_some_and(|k| k.eq_ignore_ascii_case(&c))
            }
            Key::F(number) => key
                .strip_prefix('F')
                .and_then(|n| n.parse::<u8>().ok())
                .is_some_and(|n| n == number),
            _ => false,
        }
    }
}

/// Holds one callback that receives the path of a chosen menu item. Shared by all
/// backends for `MenuBar::on_activate` and `PopupMenu::on_select`.
///
/// The closure is taken out while it is called, so you may rebuild the same menu or
/// replace the callback from inside the notification. When it is replaced inside,
/// the new one is kept.
#[derive(Default)]
pub struct MenuHandler(std::cell::RefCell<crate::Slot<MenuCallback>>);

type MenuCallback = dyn FnMut(&[usize]);

impl MenuHandler {
    pub fn set(&self, f: impl FnMut(&[usize]) + 'static) {
        *self.0.borrow_mut() = Some(Box::new(f));
    }

    pub fn emit(&self, path: &[usize]) {
        let Some(mut f) = self.0.borrow_mut().take() else {
            return;
        };
        f(path);
        let mut slot = self.0.borrow_mut();
        // 呼び出しの中で差し替えられていたら、新しいほうを残す。
        if slot.is_none() {
            *slot = Some(f);
        }
    }
}

/// What [`MenuItem`] and [`PopupItem`](crate::PopupItem) share: nested items
/// reached by a path.
pub(crate) trait MenuNode: Sized {
    fn children(&self) -> Option<&[Self]>;
    fn children_mut(&mut self) -> Option<&mut [Self]>;
    fn is_enabled(&self) -> bool;
    fn is_separator_node(&self) -> bool;
}

pub(crate) fn node_at<'a, T: MenuNode>(items: &'a [T], path: &[usize]) -> Option<&'a T> {
    let (&first, rest) = path.split_first()?;
    let item = items.get(first)?;
    if rest.is_empty() {
        Some(item)
    } else {
        node_at(item.children()?, rest)
    }
}

pub(crate) fn node_at_mut<'a, T: MenuNode>(
    items: &'a mut [T],
    path: &[usize],
) -> Option<&'a mut T> {
    let (&first, rest) = path.split_first()?;
    let item = items.get_mut(first)?;
    if rest.is_empty() {
        Some(item)
    } else {
        node_at_mut(item.children_mut()?, rest)
    }
}

/// Whether the item at `path` can be chosen: it is neither a separator nor a
/// submenu, and it and every submenu leading to it are enabled.
pub(crate) fn node_can_choose<T: MenuNode>(items: &[T], path: &[usize]) -> bool {
    let Some((&first, rest)) = path.split_first() else {
        return false;
    };
    let Some(item) = items.get(first) else {
        return false;
    };
    if !item.is_enabled() || item.is_separator_node() {
        return false;
    }
    match (rest.is_empty(), item.children()) {
        (true, None) => true,
        (false, Some(children)) => node_can_choose(children, rest),
        // サブメニューの見出しそのものは選べない。
        _ => false,
    }
}

pub(crate) fn node_walk<T: MenuNode>(
    items: &[T],
    path: &mut Vec<usize>,
    f: &mut dyn FnMut(&[usize], &T),
) {
    for (index, item) in items.iter().enumerate() {
        path.push(index);
        f(path, item);
        if let Some(children) = item.children() {
            node_walk(children, path, f);
        }
        path.pop();
    }
}

/// One item in the menu bar.
///
/// Clicking it runs the command right away. Separators and submenu headings cannot
/// be chosen, so their paths never come back in notifications.
///
/// ```
/// # use naui_core::{MenuItem, MenuShortcut};
/// let items = [
///     MenuItem::new("Open").shortcut(MenuShortcut::new('o')),
///     MenuItem::separator(),
///     MenuItem::new("Save").enabled(false),
///     MenuItem::new("Show Sidebar").checked(true),
///     MenuItem::submenu("Zoom", ["Zoom In", "Zoom Out"]),
/// ];
/// assert!(items[1].is_separator());
/// assert!(!items[2].enabled);
/// assert_eq!(items[3].checked, Some(true));
/// assert!(items[4].is_submenu());
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MenuItem {
    /// The text shown on screen. Not used for separators.
    pub label: String,
    /// Whether it can be selected. For a submenu, `false` keeps it from opening.
    pub enabled: bool,
    /// Whether this is a separator.
    pub separator: bool,
    /// The keyboard shortcut assigned to it. Not used for separators and submenus.
    pub shortcut: Option<MenuShortcut>,
    /// The check mark. `None` for an ordinary item, `Some(on)` for an item that shows
    /// whether something is on.
    ///
    /// Choosing the item (by the user or with `activate`) flips it before the
    /// notification. Not used for separators and submenus.
    pub checked: Option<bool>,
    /// The items of the submenu that opens from this item. `None` for an ordinary
    /// item. The paths of its items continue from this item's path.
    pub submenu: Option<Vec<MenuItem>>,
}

impl MenuItem {
    /// Creates a clickable item.
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            enabled: true,
            ..Self::default()
        }
    }

    /// Creates a separator that divides groups of items. It cannot be clicked.
    pub fn separator() -> Self {
        Self {
            separator: true,
            ..Self::default()
        }
    }

    /// Creates an item that opens a submenu of `items`.
    ///
    /// Items can also be passed as plain strings (`["Zoom In", "Zoom Out"]`).
    pub fn submenu<I, T>(label: impl Into<String>, items: I) -> Self
    where
        I: IntoIterator<Item = T>,
        T: Into<MenuItem>,
    {
        Self {
            submenu: Some(items.into_iter().map(Into::into).collect()),
            ..Self::new(label)
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

    /// Makes it an item with a check mark, initially on or off (an ordinary item by
    /// default).
    pub fn checked(mut self, checked: bool) -> Self {
        self.checked = Some(checked);
        self
    }

    /// Whether this is a separator.
    pub fn is_separator(&self) -> bool {
        self.separator
    }

    /// Whether this item opens a submenu.
    pub fn is_submenu(&self) -> bool {
        self.submenu.is_some()
    }

    /// Whether this item shows a check mark (on or off).
    pub fn is_checkable(&self) -> bool {
        self.checked.is_some() && !self.separator && self.submenu.is_none()
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

    /// Follows `path` through `items` and their submenus. `None` if out of range.
    ///
    /// ```
    /// # use naui_core::MenuItem;
    /// let items = [MenuItem::submenu("Zoom", ["In", "Out"])];
    /// assert_eq!(MenuItem::at(&items, &[0, 1]).map(|i| i.label.as_str()), Some("Out"));
    /// assert!(MenuItem::at(&items, &[0, 2]).is_none());
    /// assert!(MenuItem::at(&items, &[]).is_none());
    /// ```
    pub fn at<'a>(items: &'a [MenuItem], path: &[usize]) -> Option<&'a MenuItem> {
        node_at(items, path)
    }

    /// The mutable version of [`at`](Self::at).
    pub fn at_mut<'a>(items: &'a mut [MenuItem], path: &[usize]) -> Option<&'a mut MenuItem> {
        node_at_mut(items, path)
    }

    /// Whether the item at `path` can be chosen: it is neither a separator nor a
    /// submenu, and it and every submenu leading to it are enabled.
    pub fn can_choose(items: &[MenuItem], path: &[usize]) -> bool {
        node_can_choose(items, path)
    }

    /// Calls `f` with every item and its path, depth-first (separators and submenu
    /// headings included).
    pub fn walk(items: &[MenuItem], mut f: impl FnMut(&[usize], &MenuItem)) {
        node_walk(items, &mut Vec::new(), &mut f);
    }
}

impl MenuNode for MenuItem {
    fn children(&self) -> Option<&[Self]> {
        self.submenu.as_deref()
    }

    fn children_mut(&mut self) -> Option<&mut [Self]> {
        self.submenu.as_deref_mut()
    }

    fn is_enabled(&self) -> bool {
        self.enabled
    }

    fn is_separator_node(&self) -> bool {
        self.separator
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

    /// The item at `path` (heading index first). `None` if out of range or the path
    /// points at a heading.
    ///
    /// ```
    /// # use naui_core::{MenuItem, MenuSpec};
    /// let menus = [MenuSpec::new("View", [MenuItem::submenu("Zoom", ["In", "Out"])])];
    /// assert_eq!(MenuSpec::item_at(&menus, &[0, 0, 1]).map(|i| i.label.as_str()), Some("Out"));
    /// assert!(MenuSpec::item_at(&menus, &[0]).is_none());
    /// ```
    pub fn item_at<'a>(menus: &'a [MenuSpec], path: &[usize]) -> Option<&'a MenuItem> {
        let (&menu, rest) = path.split_first()?;
        MenuItem::at(&menus.get(menu)?.items, rest)
    }

    /// The mutable version of [`item_at`](Self::item_at).
    pub fn item_at_mut<'a>(menus: &'a mut [MenuSpec], path: &[usize]) -> Option<&'a mut MenuItem> {
        let (&menu, rest) = path.split_first()?;
        MenuItem::at_mut(&mut menus.get_mut(menu)?.items, rest)
    }

    /// Whether the item at `path` (heading index first) can be chosen, by the rules of
    /// [`MenuItem::can_choose`].
    pub fn can_choose(menus: &[MenuSpec], path: &[usize]) -> bool {
        path.split_first()
            .and_then(|(&menu, rest)| Some(MenuItem::can_choose(&menus.get(menu)?.items, rest)))
            .unwrap_or(false)
    }

    /// Whether the item at `path` (heading index first) and every submenu leading to it
    /// are enabled. Unlike [`can_choose`](Self::can_choose), it is also `true` for an
    /// enabled submenu heading. `false` if out of range.
    ///
    /// ```
    /// # use naui_core::{MenuItem, MenuSpec};
    /// let menus = [MenuSpec::new("View", [MenuItem::submenu("Zoom", ["In"]).enabled(false)])];
    /// assert!(!MenuSpec::enabled_at(&menus, &[0, 0]));
    /// assert!(!MenuSpec::enabled_at(&menus, &[0, 0, 0]), "blocked by the submenu");
    /// ```
    pub fn enabled_at(menus: &[MenuSpec], path: &[usize]) -> bool {
        path.len() >= 2
            && (2..=path.len())
                .all(|end| Self::item_at(menus, &path[..end]).is_some_and(|item| item.enabled))
    }

    /// Calls `f` with every item and its path (heading index first), depth-first.
    pub fn walk(menus: &[MenuSpec], mut f: impl FnMut(&[usize], &MenuItem)) {
        let mut path = Vec::new();
        for (index, spec) in menus.iter().enumerate() {
            path.push(index);
            node_walk(&spec.items, &mut path, &mut f);
            path.pop();
        }
    }

    /// The path of the first item that can be chosen now and has a valid shortcut for
    /// which `matches` returns `true`.
    ///
    /// Used on platforms where naui matches pressed keys itself (Windows and the Web).
    ///
    /// ```
    /// # use naui_core::{MenuItem, MenuShortcut, MenuSpec};
    /// let menus = [MenuSpec::new(
    ///     "View",
    ///     [MenuItem::submenu("Zoom", [MenuItem::new("In").shortcut(MenuShortcut::new('i'))])],
    /// )];
    /// let found = MenuSpec::find_shortcut(&menus, |s| s.matches("i", true, false, false));
    /// assert_eq!(found, Some(vec![0, 0, 0]));
    /// ```
    pub fn find_shortcut(
        menus: &[MenuSpec],
        mut matches: impl FnMut(MenuShortcut) -> bool,
    ) -> Option<Vec<usize>> {
        let mut found = None;
        Self::walk(menus, |path, item| {
            if found.is_some() {
                return;
            }
            let Some(shortcut) = item.shortcut else {
                return;
            };
            if shortcut.is_valid() && Self::can_choose(menus, path) && matches(shortcut) {
                found = Some(path.to_vec());
            }
        });
        found
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
        assert!(!item.is_submenu());
        assert!(!item.is_checkable());
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
        assert!(!MenuItem::can_choose(&[item], &[0]));
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

    #[test]
    fn checked_items_keep_their_state() {
        let on = MenuItem::new("サイドバー").checked(true);
        assert!(on.is_checkable());
        assert_eq!(on.checked, Some(true));
        assert_eq!(MenuItem::new("x").checked(false).checked, Some(false));
        // サブメニューに印は付かない。
        assert!(!MenuItem::submenu("拡大", ["a"])
            .checked(true)
            .is_checkable());
    }

    #[test]
    fn paths_reach_nested_items() {
        let menus = [
            MenuSpec::new("ファイル", ["開く"]),
            MenuSpec::new(
                "表示",
                [
                    MenuItem::new("サイドバー").checked(true),
                    MenuItem::separator(),
                    MenuItem::submenu(
                        "拡大",
                        [MenuItem::new("拡大"), MenuItem::submenu("深い", ["さらに"])],
                    ),
                ],
            ),
        ];
        let label = |path: &[usize]| MenuSpec::item_at(&menus, path).map(|i| i.label.clone());
        assert_eq!(label(&[0, 0]).as_deref(), Some("開く"));
        assert_eq!(label(&[1, 2, 0]).as_deref(), Some("拡大"));
        assert_eq!(label(&[1, 2, 1, 0]).as_deref(), Some("さらに"));
        assert_eq!(label(&[1, 0, 0]), None, "普通の項目の下には何も無い");
        assert_eq!(label(&[2, 0]), None);
        assert_eq!(label(&[1]), None, "見出しは項目ではない");

        assert!(MenuSpec::can_choose(&menus, &[1, 0]));
        assert!(!MenuSpec::can_choose(&menus, &[1, 1]), "区切り線");
        assert!(
            !MenuSpec::can_choose(&menus, &[1, 2]),
            "サブメニューの見出し"
        );
        assert!(MenuSpec::can_choose(&menus, &[1, 2, 1, 0]));

        let mut paths = Vec::new();
        MenuSpec::walk(&menus, |path, _| paths.push(path.to_vec()));
        assert_eq!(
            paths,
            vec![
                vec![0, 0],
                vec![1, 0],
                vec![1, 1],
                vec![1, 2],
                vec![1, 2, 0],
                vec![1, 2, 1],
                vec![1, 2, 1, 0],
            ]
        );
    }

    #[test]
    fn disabled_submenus_block_their_items() {
        let mut menus = [MenuSpec::new("表示", [MenuItem::submenu("拡大", ["拡大"])])];
        assert!(MenuSpec::can_choose(&menus, &[0, 0, 0]));
        MenuSpec::item_at_mut(&mut menus, &[0, 0]).unwrap().enabled = false;
        assert!(!MenuSpec::can_choose(&menus, &[0, 0, 0]));
    }

    #[test]
    fn find_shortcut_skips_what_cannot_be_chosen() {
        let s = MenuShortcut::new('i');
        let menus = [
            MenuSpec::new(
                "a",
                [
                    MenuItem::new("押せない").shortcut(s).enabled(false),
                    MenuItem::submenu("閉じた", [MenuItem::new("中").shortcut(s)]).enabled(false),
                ],
            ),
            MenuSpec::new(
                "b",
                [MenuItem::submenu("開く", [MenuItem::new("中").shortcut(s)])],
            ),
        ];
        let found = MenuSpec::find_shortcut(&menus, |c| c == s);
        assert_eq!(found, Some(vec![1, 0, 0]));
        // 有効でないショートカットは見つからない。
        let bad = MenuShortcut::new('/');
        let menus = [MenuSpec::new("a", [MenuItem::new("x").shortcut(bad)])];
        assert_eq!(MenuSpec::find_shortcut(&menus, |_| true), None);
    }

    #[test]
    fn handler_keeps_a_replacement_made_inside() {
        use std::cell::RefCell;
        use std::rc::Rc;
        let handler = Rc::new(MenuHandler::default());
        handler.emit(&[0]); // 通知先が無くても何も起きない
        let seen = Rc::new(RefCell::new(Vec::new()));
        handler.set({
            let handler = handler.clone();
            let seen = seen.clone();
            move |path| {
                seen.borrow_mut().push(path.to_vec());
                let seen = seen.clone();
                handler.set(move |_| seen.borrow_mut().push(vec![99]));
            }
        });
        handler.emit(&[1, 2]);
        handler.emit(&[3]);
        assert_eq!(*seen.borrow(), vec![vec![1, 2], vec![99]]);
    }

    /// Even when passed in uppercase, the key is stored lowercase (⇧ is specified
    /// separately).
    #[test]
    fn shortcut_key_is_lowercased() {
        let s = MenuShortcut::new('S');
        assert_eq!(s.key, Key::Character('s'));
        assert!(s.primary);
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
    fn function_keys_write_each_backend_notation() {
        let f5 = MenuShortcut::function(5);
        assert!(!f5.primary);
        assert_eq!(f5.accelerator(), "F5");
        assert_eq!(f5.label(), "F5");
        assert_eq!(f5.aria_key_shortcuts(), "F5");
        assert_eq!(f5.virtual_key(), 0x74);
        assert_eq!(f5.key_equivalent(), "\u{F708}");

        let ctrl_f12 = MenuShortcut::function(12).primary(true).shift(true);
        assert_eq!(ctrl_f12.accelerator(), "<Control><Shift>F12");
        assert_eq!(ctrl_f12.label(), "Ctrl+Shift+F12");
        assert_eq!(ctrl_f12.aria_key_shortcuts(), "Control+Shift+F12");
        assert_eq!(MenuShortcut::function(1).virtual_key(), 0x70);
        assert_eq!(MenuShortcut::function(24).virtual_key(), 0x87);
        assert_eq!(MenuShortcut::function(0).virtual_key(), 0);
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
        assert!(!s.matches("/", true, false, false));
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

    #[test]
    fn function_keys_match_by_name() {
        let s = MenuShortcut::function(11);
        assert!(s.matches("F11", false, false, false));
        assert!(!s.matches("F1", false, false, false));
        assert!(!s.matches("F111", false, false, false));
        assert!(!s.matches("F11", false, true, false));
        let ctrl = s.primary(true);
        assert!(ctrl.matches("F11", true, false, false));
        assert!(!ctrl.matches("F11", false, false, false));
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
