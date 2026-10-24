//! Items of popup (context) menus.

/// One item of a popup menu.
///
/// Describes the contents of the menu shown on right-click (secondary button).
/// Items are identified by the order (index) the app owns, and selection
/// notifications come back as **the index within the sequence, separators
/// included**. Separators cannot be chosen, so their indices never come back.
///
/// ```
/// # use naui_core::PopupItem;
/// let items = [
///     PopupItem::new("Copy"),
///     PopupItem::separator(),
///     PopupItem::new("Delete").enabled(false),
/// ];
/// assert!(items[1].is_separator());
/// assert!(!items[2].enabled);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PopupItem {
    /// The text shown on screen. Not used for separators.
    pub label: String,
    /// Whether it can be selected.
    pub enabled: bool,
    /// Whether this is a separator.
    pub separator: bool,
}

impl PopupItem {
    /// Creates a clickable item.
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            enabled: true,
            separator: false,
        }
    }

    /// Creates a separator. It cannot be chosen.
    pub fn separator() -> Self {
        Self {
            label: String::new(),
            enabled: false,
            separator: true,
        }
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
    fn popup_item_list_keeps_order() {
        let items = PopupItem::list(["切り取り", "コピー"]);
        assert_eq!(items[0], PopupItem::from("切り取り"));
        assert_eq!(items[1].label, "コピー");
    }
}
