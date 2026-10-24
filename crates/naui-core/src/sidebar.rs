//! Sidebar items and sections.

use crate::ToolbarIcon;

/// The default sidebar width (logical pixels).
///
/// Leaving it to each platform's default (about 200 on macOS, 320 for WinUI 3's
/// `OpenPaneLength`, 25% of the width clamped to 180–280 in libadwaita) would make
/// the same app appear at different widths per platform, so naui unifies it across
/// all four.
pub const DEFAULT_SIDEBAR_WIDTH: f64 = 220.0;

/// The minimum width the user can shrink the sidebar to with the divider (logical
/// pixels).
///
/// On all four platforms the width can be changed with the divider, and naui
/// uniformly sets a minimum so it cannot be shrunk until the item text becomes
/// unreadable.
pub const SIDEBAR_MIN_WIDTH: f64 = 140.0;

/// One sidebar item.
///
/// It looks like a single row of "icon + text", and the icon can be omitted. Icons
/// come from the same set as [`ToolbarIcon`] and map to the platform's standard
/// icons.
///
/// ```
/// # use naui_core::{SidebarItem, ToolbarIcon};
/// let item = SidebarItem::new("General").icon(ToolbarIcon::Settings);
/// assert_eq!(item.label, "General");
/// assert_eq!(item.icon, Some(ToolbarIcon::Settings));
/// assert!(item.enabled);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SidebarItem {
    /// The text shown on screen.
    pub label: String,
    /// The icon shown before the text. Text only if `None`.
    pub icon: Option<ToolbarIcon>,
    /// Whether it can be selected.
    pub enabled: bool,
}

impl SidebarItem {
    /// Creates a selectable item without an icon.
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            icon: None,
            enabled: true,
        }
    }

    /// Sets the icon shown before the text (none by default).
    pub fn icon(mut self, icon: ToolbarIcon) -> Self {
        self.icon = Some(icon);
        self
    }

    /// Sets whether it can be selected (selectable by default).
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Creates a list of items from a sequence of strings.
    ///
    /// ```
    /// # use naui_core::SidebarItem;
    /// let items = SidebarItem::list(["Home", "Search", "Settings"]);
    /// assert_eq!(items.len(), 3);
    /// ```
    pub fn list<I, S>(labels: I) -> Vec<SidebarItem>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        labels.into_iter().map(SidebarItem::new).collect()
    }
}

impl From<&str> for SidebarItem {
    fn from(label: &str) -> Self {
        SidebarItem::new(label)
    }
}

impl From<String> for SidebarItem {
    fn from(label: String) -> Self {
        SidebarItem::new(label)
    }
}

/// A group of sidebar items.
///
/// Groups are separated by a gap, and a heading, if any, appears small above the
/// group (the same shape as the sidebars of macOS's System Settings and Finder).
/// Headings cannot be selected.
///
/// ```
/// # use naui_core::{SidebarItem, SidebarSection};
/// let places = SidebarSection::new("Places", ["Documents", "Downloads"]);
/// assert_eq!(places.title.as_deref(), Some("Places"));
/// assert_eq!(places.len(), 2);
///
/// // A group without a heading is separated by the gap alone.
/// let plain = SidebarSection::untitled(["General", "Appearance"]);
/// assert_eq!(plain.title, None);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SidebarSection {
    /// The group's heading. No heading is shown if `None`.
    pub title: Option<String>,
    /// The items in the group.
    pub items: Vec<SidebarItem>,
}

impl SidebarSection {
    /// Creates a group with a heading. Items can be passed as plain strings.
    pub fn new<I, T>(title: impl Into<String>, items: I) -> Self
    where
        I: IntoIterator<Item = T>,
        T: Into<SidebarItem>,
    {
        Self {
            title: Some(title.into()),
            items: items.into_iter().map(Into::into).collect(),
        }
    }

    /// Creates a group without a heading.
    pub fn untitled<I, T>(items: I) -> Self
    where
        I: IntoIterator<Item = T>,
        T: Into<SidebarItem>,
    {
        Self {
            title: None,
            items: items.into_iter().map(Into::into).collect(),
        }
    }

    /// The number of items.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

/// One row in the sidebar. Used by backends when building the list.
///
/// The groups flattened into a single sequence; items carry **a running number
/// counted across groups**. naui's notifications and selection use this number.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarRow<'a> {
    /// The gap between groups. Inserted only before the second and later groups.
    Gap,
    /// A group heading.
    Header(&'a str),
    /// An item and its running number.
    Item(usize, &'a SidebarItem),
}

impl SidebarRow<'_> {
    /// The running number, if this is an item row.
    pub fn item_index(&self) -> Option<usize> {
        match self {
            SidebarRow::Item(index, _) => Some(*index),
            _ => None,
        }
    }
}

/// Flattens the groups into rows in on-screen order.
///
/// A [`SidebarRow::Gap`] goes before the second and later groups. If there is a
/// heading, a [`SidebarRow::Header`] follows the gap. Groups without items still
/// show their heading (for screens that add content later).
///
/// ```
/// # use naui_core::{sidebar_rows, SidebarRow, SidebarSection};
/// let sections = [
///     SidebarSection::untitled(["General"]),
///     SidebarSection::new("Places", ["Documents"]),
/// ];
/// let rows = sidebar_rows(&sections);
/// assert!(matches!(rows[0], SidebarRow::Item(0, _)));
/// assert_eq!(rows[1], SidebarRow::Gap);
/// assert_eq!(rows[2], SidebarRow::Header("Places"));
/// assert!(matches!(rows[3], SidebarRow::Item(1, _)));
/// ```
pub fn sidebar_rows(sections: &[SidebarSection]) -> Vec<SidebarRow<'_>> {
    let mut rows = Vec::new();
    let mut next = 0;
    for (i, section) in sections.iter().enumerate() {
        if i > 0 {
            rows.push(SidebarRow::Gap);
        }
        if let Some(title) = &section.title {
            rows.push(SidebarRow::Header(title));
        }
        for item in &section.items {
            rows.push(SidebarRow::Item(next, item));
            next += 1;
        }
    }
    rows
}

/// The total number of items across groups.
pub fn sidebar_len(sections: &[SidebarSection]) -> usize {
    sections.iter().map(SidebarSection::len).sum()
}

/// The item with the running number. `None` if out of range.
pub fn sidebar_item(sections: &[SidebarSection], index: usize) -> Option<&SidebarItem> {
    sections.iter().flat_map(|s| s.items.iter()).nth(index)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn item_defaults_to_enabled_without_icon() {
        let item = SidebarItem::new("一般");
        assert_eq!(item.label, "一般");
        assert_eq!(item.icon, None);
        assert!(item.enabled);
        assert!(!SidebarItem::new("x").enabled(false).enabled);
    }

    #[test]
    fn sections_take_labels_or_items() {
        let section =
            SidebarSection::new("場所", [SidebarItem::new("書類").icon(ToolbarIcon::Open)]);
        assert_eq!(section.items[0].icon, Some(ToolbarIcon::Open));
        let plain = SidebarSection::untitled(vec![String::from("一般")]);
        assert_eq!(plain.items[0].label, "一般");
        assert!(SidebarSection::untitled(Vec::<SidebarItem>::new()).is_empty());
    }

    #[test]
    fn rows_number_items_across_sections() {
        let sections = [
            SidebarSection::new("A", ["a0", "a1"]),
            SidebarSection::untitled(["b0"]),
            SidebarSection::new("C", Vec::<SidebarItem>::new()),
            SidebarSection::new("D", ["d0"]),
        ];
        let rows = sidebar_rows(&sections);
        let shape: Vec<String> = rows
            .iter()
            .map(|row| match row {
                SidebarRow::Gap => "-".to_string(),
                SidebarRow::Header(title) => format!("#{title}"),
                SidebarRow::Item(index, item) => format!("{index}:{}", item.label),
            })
            .collect();
        assert_eq!(
            shape,
            ["#A", "0:a0", "1:a1", "-", "2:b0", "-", "#C", "-", "#D", "3:d0"]
        );
        assert_eq!(sidebar_len(&sections), 4);
        assert_eq!(
            sidebar_item(&sections, 3).map(|i| i.label.as_str()),
            Some("d0")
        );
        assert_eq!(sidebar_item(&sections, 4), None);
        assert_eq!(rows[1].item_index(), Some(0));
        assert_eq!(rows[0].item_index(), None);
    }

    #[test]
    fn empty_sections_make_no_rows() {
        assert!(sidebar_rows(&[]).is_empty());
        assert_eq!(sidebar_len(&[]), 0);
    }
}
