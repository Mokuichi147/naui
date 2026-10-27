//! Items of popup (context) menus.

use crate::menu::{node_at, node_at_mut, node_can_choose, node_walk, MenuNode};

/// One item of a popup menu.
///
/// Describes the contents of the menu shown on right-click (secondary button).
/// Items are identified by **a path**: the index at each level, separators
/// included. `[2]` is the third item, and `[3, 0]` is the first item in the
/// submenu that is the fourth item. Separators and submenu headings cannot be
/// chosen, so their paths never come back.
///
/// ```
/// # use naui_core::PopupItem;
/// let items = [
///     PopupItem::new("Copy"),
///     PopupItem::separator(),
///     PopupItem::new("Delete").enabled(false),
///     PopupItem::new("Wrap Lines").checked(true),
///     PopupItem::submenu("Sort By", ["Name", "Date"]),
/// ];
/// assert!(items[1].is_separator());
/// assert!(!items[2].enabled);
/// assert_eq!(items[3].checked, Some(true));
/// assert!(items[4].is_submenu());
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PopupItem {
    /// The text shown on screen. Not used for separators.
    pub label: String,
    /// Whether it can be selected. For a submenu, `false` keeps it from opening.
    pub enabled: bool,
    /// Whether this is a separator.
    pub separator: bool,
    /// The check mark. `None` for an ordinary item, `Some(on)` for an item that shows
    /// whether something is on.
    ///
    /// Choosing the item (by the user or with `select`) flips it before the
    /// notification. Not used for separators and submenus.
    pub checked: Option<bool>,
    /// The items of the submenu that opens from this item. `None` for an ordinary
    /// item. The paths of its items continue from this item's path.
    pub submenu: Option<Vec<PopupItem>>,
}

impl PopupItem {
    /// Creates a clickable item.
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            enabled: true,
            separator: false,
            checked: None,
            submenu: None,
        }
    }

    /// Creates a separator. It cannot be chosen.
    pub fn separator() -> Self {
        Self {
            label: String::new(),
            enabled: false,
            separator: true,
            checked: None,
            submenu: None,
        }
    }

    /// Creates an item that opens a submenu of `items`.
    ///
    /// Items can also be passed as plain strings (`["Name", "Date"]`).
    pub fn submenu<I, T>(label: impl Into<String>, items: I) -> Self
    where
        I: IntoIterator<Item = T>,
        T: Into<PopupItem>,
    {
        Self {
            submenu: Some(items.into_iter().map(Into::into).collect()),
            ..Self::new(label)
        }
    }

    /// Makes it an item with a check mark, initially on or off (an ordinary item by
    /// default).
    pub fn checked(mut self, checked: bool) -> Self {
        self.checked = Some(checked);
        self
    }

    /// Whether this item opens a submenu.
    pub fn is_submenu(&self) -> bool {
        self.submenu.is_some()
    }

    /// Whether this item shows a check mark (on or off).
    pub fn is_checkable(&self) -> bool {
        self.checked.is_some() && !self.separator && self.submenu.is_none()
    }

    /// Follows `path` through `items` and their submenus. `None` if out of range.
    ///
    /// ```
    /// # use naui_core::PopupItem;
    /// let items = [PopupItem::new("Copy"), PopupItem::submenu("Sort By", ["Name", "Date"])];
    /// assert_eq!(PopupItem::at(&items, &[1, 1]).map(|i| i.label.as_str()), Some("Date"));
    /// assert!(PopupItem::at(&items, &[0, 0]).is_none());
    /// ```
    pub fn at<'a>(items: &'a [PopupItem], path: &[usize]) -> Option<&'a PopupItem> {
        node_at(items, path)
    }

    /// The mutable version of [`at`](Self::at).
    pub fn at_mut<'a>(items: &'a mut [PopupItem], path: &[usize]) -> Option<&'a mut PopupItem> {
        node_at_mut(items, path)
    }

    /// Whether the item at `path` can be chosen: it is neither a separator nor a
    /// submenu, and it and every submenu leading to it are enabled.
    pub fn can_choose(items: &[PopupItem], path: &[usize]) -> bool {
        node_can_choose(items, path)
    }

    /// Calls `f` with every item and its path, depth-first (separators and submenu
    /// headings included).
    pub fn walk(items: &[PopupItem], mut f: impl FnMut(&[usize], &PopupItem)) {
        node_walk(items, &mut Vec::new(), &mut f);
    }

    /// Sets whether it can be selected (selectable by default).
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Whether this is a separator.
    pub fn is_separator(&self) -> bool {
        self.separator
    }

    /// Creates a list of items from a sequence of strings.
    ///
    /// ```
    /// # use naui_core::PopupItem;
    /// let items = PopupItem::list(["Copy", "Paste"]);
    /// assert_eq!(items.len(), 2);
    /// ```
    pub fn list<I, S>(labels: I) -> Vec<PopupItem>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        labels.into_iter().map(PopupItem::new).collect()
    }
}

impl MenuNode for PopupItem {
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

/// The same item as a menu bar item (without a shortcut), for backends that build
/// both kinds of menus the same way.
impl From<PopupItem> for crate::MenuItem {
    fn from(item: PopupItem) -> Self {
        crate::MenuItem {
            label: item.label,
            enabled: item.enabled,
            separator: item.separator,
            shortcut: None,
            checked: item.checked,
            submenu: item
                .submenu
                .map(|children| children.into_iter().map(Into::into).collect()),
        }
    }
}

impl Default for PopupItem {
    fn default() -> Self {
        PopupItem::new("")
    }
}

impl From<&str> for PopupItem {
    fn from(label: &str) -> Self {
        PopupItem::new(label)
    }
}

impl From<String> for PopupItem {
    fn from(label: String) -> Self {
        PopupItem::new(label)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn popup_item_defaults_to_enabled() {
        let item = PopupItem::new("コピー");
        assert_eq!(item.label, "コピー");
        assert!(item.enabled);
        assert!(!item.is_separator());
        assert!(!PopupItem::new("削除").enabled(false).enabled);
    }

    #[test]
    fn separator_is_never_selectable() {
        let item = PopupItem::separator();
        assert!(item.is_separator());
        assert!(!item.enabled);
        assert!(item.label.is_empty());
    }

    #[test]
    fn nested_items_are_reached_by_path() {
        let items = [
            PopupItem::new("コピー"),
            PopupItem::separator(),
            PopupItem::submenu(
                "並べ替え",
                [PopupItem::new("名前").checked(true), PopupItem::new("日付")],
            ),
            PopupItem::submenu("閉じた", ["中"]).enabled(false),
        ];
        assert_eq!(
            PopupItem::at(&items, &[2, 0]).and_then(|i| i.checked),
            Some(true)
        );
        assert!(PopupItem::can_choose(&items, &[0]));
        assert!(!PopupItem::can_choose(&items, &[1]));
        assert!(!PopupItem::can_choose(&items, &[2]), "サブメニューの見出し");
        assert!(PopupItem::can_choose(&items, &[2, 1]));
        assert!(!PopupItem::can_choose(&items, &[3, 0]), "親が押せない");
        assert!(!PopupItem::can_choose(&items, &[]));

        let mut paths = Vec::new();
        PopupItem::walk(&items, |path, _| paths.push(path.to_vec()));
        assert_eq!(paths.len(), 7);
        assert_eq!(paths[3], vec![2, 0]);
    }

    #[test]
    fn popup_items_convert_to_menu_items() {
        let item =
            PopupItem::submenu("並べ替え", [PopupItem::new("名前").checked(true)]).enabled(false);
        let menu: crate::MenuItem = item.into();
        assert_eq!(menu.label, "並べ替え");
        assert!(!menu.enabled);
        assert_eq!(
            crate::MenuItem::at(&[menu], &[0, 0]).and_then(|i| i.checked),
            Some(true)
        );
    }

    #[test]
    fn popup_item_list_keeps_order() {
        let items = PopupItem::list(["切り取り", "コピー"]);
        assert_eq!(items[0], PopupItem::from("切り取り"));
        assert_eq!(items[1].label, "コピー");
    }
}
