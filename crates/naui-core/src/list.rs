//! List value types.
//!
//! A list has the structure "a sequence of rows + the rows currently selected". It
//! resembles navigation ([`crate::NavItem`]) in shape, but differs in that
//!
//! - it has many rows and scrolls by itself
//! - it supports multiple selection (and the selection can be empty)
//!
//! so it is a separate type. Backends map it onto rows of `NSTableView` /
//! `ListBox` / `<select>`.

/// One row of a list.
///
/// Rows are identified by their order (index), which the app owns. Selection
/// notifications also come back as indices, so whoever built the `&[ListItem]` can
/// map them straight back to their own data.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ListItem {
    /// The text shown on screen.
    pub label: String,
    /// Secondary text. When set, it appears small on a second line.
    ///
    /// Its size and dimness come from [`crate::TextStyle::Caption`] and
    /// [`crate::TextColor::Secondary`], so on every platform it looks like that
    /// platform's "secondary text".
    ///
    /// The Web's `<option>` can hold only one line of text, so if even one row has
    /// secondary text, the whole list switches to being built from
    /// `<ul role="listbox">`.
    pub detail: Option<String>,
    /// Whether it can be selected.
    pub enabled: bool,
}

impl ListItem {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            detail: None,
            enabled: true,
        }
    }

    /// Sets the secondary text shown on the second line.
    ///
    /// ```
    /// # use naui_core::ListItem;
    /// let item = ListItem::new("Tokyo").detail("13,960,000 people");
    /// assert_eq!(item.detail.as_deref(), Some("13,960,000 people"));
    /// ```
    pub fn detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    /// Sets whether it can be selected (selectable by default).
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Creates a list of rows from a sequence of strings.
    ///
    /// ```
    /// # use naui_core::ListItem;
    /// let rows = ListItem::list(["Tokyo", "Osaka", "Sapporo"]);
    /// assert_eq!(rows.len(), 3);
    /// assert!(rows[0].enabled);
    /// ```
    pub fn list<I, S>(labels: I) -> Vec<ListItem>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        labels.into_iter().map(ListItem::new).collect()
    }
}

impl From<&str> for ListItem {
    fn from(label: &str) -> Self {
        ListItem::new(label)
    }
}

impl From<String> for ListItem {
    fn from(label: String) -> Self {
        ListItem::new(label)
    }
}

/// How rows in a list are selected.
///
/// | Value | macOS | Windows | Web |
/// | --- | --- | --- | --- |
/// | [`SelectionMode::Single`] | `allowsMultipleSelection = false` | `SelectionMode::Single` | `<select>` |
/// | [`SelectionMode::Multiple`] | `allowsMultipleSelection = true` | `SelectionMode::Extended` | `<select multiple>` |
///
/// On every platform `Multiple` means "select while holding ⌘ / Ctrl or Shift".
/// That is why it maps to WinUI's `Extended` rather than `SelectionMode::Multiple`
/// (which toggles on every click).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SelectionMode {
    /// Only one row can be selected (default).
    #[default]
    Single,
    /// Multiple rows can be selected. The selection can also be empty.
    Multiple,
}

impl SelectionMode {
    pub fn is_multiple(self) -> bool {
        matches!(self, SelectionMode::Multiple)
    }

    /// Normalizes the requested selection into a form that can be passed on as is.
    ///
    /// - Drops out-of-range indices
    /// - Drops rows that cannot be selected ([`ListItem::enabled`] is `false`)
    /// - Sorts in ascending order and removes duplicates
    /// - Keeps only the first one for [`SelectionMode::Single`]
    ///
    /// Every backend passes the selection through this before handing it to the
    /// native side, so `set_selection` means the same on all three platforms.
    ///
    /// ```
    /// # use naui_core::{ListItem, SelectionMode};
    /// let items = ListItem::list(["a", "b", "c"]);
    /// let picked = SelectionMode::Multiple.normalize(&items, &[2, 0, 2, 9]);
    /// assert_eq!(picked, vec![0, 2]);
    /// // With single selection only the first one remains.
    /// assert_eq!(SelectionMode::Single.normalize(&items, &[2, 0]), vec![0]);
    /// ```
    pub fn normalize(self, items: &[ListItem], indices: &[usize]) -> Vec<usize> {
        self.normalize_by(indices, |i| items.get(i).is_some_and(|item| item.enabled))
    }

    /// [`normalize`] for lists whose rows are not [`ListItem`]s (tables, for example).
    ///
    /// `enabled` returns "whether the row exists and can be selected". For
    /// out-of-range rows it can just return `false`. The normalization is the same as
    /// [`normalize`].
    ///
    /// [`normalize`]: SelectionMode::normalize
    ///
    /// ```
    /// # use naui_core::SelectionMode;
    /// let enabled = |i: usize| matches!(i, 0 | 2);
    /// assert_eq!(SelectionMode::Multiple.normalize_by(&[2, 1, 0], enabled), vec![0, 2]);
    /// assert_eq!(SelectionMode::Single.normalize_by(&[2, 0], enabled), vec![0]);
    /// ```
    pub fn normalize_by(
        self,
        indices: &[usize],
        mut enabled: impl FnMut(usize) -> bool,
    ) -> Vec<usize> {
        let mut picked: Vec<usize> = indices.iter().copied().filter(|&i| enabled(i)).collect();
        picked.sort_unstable();
        picked.dedup();
        if !self.is_multiple() {
            picked.truncate(1);
        }
        picked
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_item_defaults_to_enabled() {
        let item = ListItem::new("東京");
        assert_eq!(item.label, "東京");
        assert_eq!(item.detail, None);
        assert!(item.enabled);
        assert!(!ListItem::new("大阪").enabled(false).enabled);
        assert_eq!(ListItem::from("札幌"), ListItem::new("札幌"));
        assert_eq!(ListItem::from(String::from("那覇")).label, "那覇");
    }

    #[test]
    fn detail_is_optional_and_keeps_the_other_fields() {
        let item = ListItem::new("東京").detail("首都").enabled(false);
        assert_eq!(item.label, "東京");
        assert_eq!(item.detail.as_deref(), Some("首都"));
        assert!(!item.enabled);
        assert!(ListItem::list(["東京"])[0].detail.is_none());
    }

    #[test]
    fn selection_mode_defaults_to_single() {
        assert_eq!(SelectionMode::default(), SelectionMode::Single);
        assert!(SelectionMode::Multiple.is_multiple());
        assert!(!SelectionMode::Single.is_multiple());
    }

    #[test]
    fn normalize_sorts_and_drops_out_of_range() {
        let items = ListItem::list(["a", "b", "c"]);
        assert_eq!(
            SelectionMode::Multiple.normalize(&items, &[2, 0, 2, 3, 100]),
            vec![0, 2]
        );
        assert_eq!(SelectionMode::Multiple.normalize(&items, &[]), Vec::new());
    }

    #[test]
    fn normalize_drops_disabled_rows() {
        let items = vec![
            ListItem::new("a"),
            ListItem::new("b").enabled(false),
            ListItem::new("c"),
        ];
        assert_eq!(
            SelectionMode::Multiple.normalize(&items, &[0, 1, 2]),
            vec![0, 2]
        );
        // 選べない行だけを指定したときは、何も選ばれない。
        assert!(SelectionMode::Single.normalize(&items, &[1]).is_empty());
    }

    #[test]
    fn single_keeps_only_the_first_row() {
        let items = ListItem::list(["a", "b", "c"]);
        assert_eq!(SelectionMode::Single.normalize(&items, &[2, 1]), vec![1]);
    }
}
