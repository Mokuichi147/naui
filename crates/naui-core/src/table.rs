//! Table value types.
//!
//! A table has the structure "column definitions + a sequence of rows + the rows
//! currently selected". It resembles a list ([`crate::ListItem`]) in shape, but
//! differs in that
//!
//! - each row is split into several cells, and width and alignment are set per
//!   column
//! - it has column headings (a header)
//!
//! so it is a separate type. Backends map it onto `NSTableView` columns or the
//! `<th>` / `<td>` of a `<table>`.
//!
//! As with lists, rows are identified by the order (index) the app owns, and
//! selection notifications come back as indices. Use
//! [`crate::SelectionMode::normalize_by`] to normalize the selection.

use crate::Align;

/// The sort direction.
///
/// Each click on the header toggles `Ascending` → `Descending` → `Ascending`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortOrder {
    /// Smallest at the top (default).
    #[default]
    Ascending,
    /// Largest at the top.
    Descending,
}

impl SortOrder {
    pub fn is_ascending(self) -> bool {
        matches!(self, SortOrder::Ascending)
    }

    /// Reverses the direction.
    ///
    /// ```
    /// # use naui_core::SortOrder;
    /// assert_eq!(SortOrder::Ascending.reversed(), SortOrder::Descending);
    /// assert_eq!(SortOrder::Descending.reversed(), SortOrder::Ascending);
    /// ```
    pub fn reversed(self) -> Self {
        match self {
            SortOrder::Ascending => SortOrder::Descending,
            SortOrder::Descending => SortOrder::Ascending,
        }
    }
}

/// One table column.
///
/// ```
/// # use naui_core::{Align, TableColumn};
/// let column = TableColumn::new("Population").width(120.0).align(Align::End).sortable(true);
/// assert_eq!(column.width, Some(120.0));
/// assert!(column.sortable);
/// ```
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TableColumn {
    /// The text shown in the heading.
    pub title: String,
    /// The column width (logical pixels). If `None`, it grows by sharing the leftover
    /// space.
    pub width: Option<f64>,
    /// The alignment of the text in its cells.
    ///
    /// The three used are [`Align::Start`] / [`Align::Center`] / [`Align::End`];
    /// [`Align::Fill`] is treated the same as `Start` (text has no "stretch").
    pub align: Align,
    /// Whether clicking the heading sorts.
    ///
    /// **naui does not do the sorting itself.** The click is reported as "which column,
    /// in which direction", and the app sorts its own data and passes it again. Only
    /// the app knows what the strings contain (numbers or dates), so the app also
    /// decides how to compare them.
    pub sortable: bool,
}

impl TableColumn {
    /// Creates a column from its heading text. The width shares the leftover space,
    /// text is aligned left, and sorting is off.
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            width: None,
            align: Align::Start,
            sortable: false,
        }
    }

    /// Fixes the column width in logical pixels.
    pub fn width(mut self, width: f64) -> Self {
        self.width = Some(width);
        self
    }

    /// Sets the alignment of the text in its cells (left by default).
    pub fn align(mut self, align: Align) -> Self {
        self.align = align;
        self
    }

    /// Lets the column be sorted by clicking the heading (off by default).
    pub fn sortable(mut self, sortable: bool) -> Self {
        self.sortable = sortable;
        self
    }

    /// Creates column definitions from a sequence of strings.
    ///
    /// ```
    /// # use naui_core::TableColumn;
    /// let columns = TableColumn::list(["City", "Population"]);
    /// assert_eq!(columns.len(), 2);
    /// assert!(columns[0].width.is_none(), "by default columns share the leftover space");
    /// ```
    pub fn list<I, S>(titles: I) -> Vec<TableColumn>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        titles.into_iter().map(TableColumn::new).collect()
    }
}

impl From<&str> for TableColumn {
    fn from(title: &str) -> Self {
        TableColumn::new(title)
    }
}

impl From<String> for TableColumn {
    fn from(title: String) -> Self {
        TableColumn::new(title)
    }
}

/// One table row.
///
/// Cells are in the same order as the columns. **They need not match the number of
/// columns.** Missing cells are empty, and extra cells are not shown
/// ([`TableRow::cell`]).
///
/// ```
/// # use naui_core::TableRow;
/// let row = TableRow::new(["Tokyo", "13,960,000"]);
/// assert_eq!(row.cell(0), "Tokyo");
/// assert_eq!(row.cell(5), "", "a row short of columns gets empty cells");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TableRow {
    /// The cell strings, from left to right.
    pub cells: Vec<String>,
    /// Whether it can be selected.
    pub enabled: bool,
}

impl TableRow {
    /// Creates a row from a sequence of cells.
    pub fn new<I, S>(cells: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            cells: cells.into_iter().map(Into::into).collect(),
            enabled: true,
        }
    }

    /// Sets whether it can be selected (selectable by default).
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// The cell in column `index`. The empty string if there is none.
    pub fn cell(&self, index: usize) -> &str {
        self.cells.get(index).map(String::as_str).unwrap_or("")
    }

    /// Creates a list of rows from a sequence of cell sequences.
    ///
    /// ```
    /// # use naui_core::TableRow;
    /// let rows = TableRow::list([["Tokyo", "13,960,000"], ["Osaka", "8,838,000"]]);
    /// assert_eq!(rows.len(), 2);
    /// assert_eq!(rows[1].cell(0), "Osaka");
    /// ```
    pub fn list<I, R, S>(rows: I) -> Vec<TableRow>
    where
        I: IntoIterator<Item = R>,
        R: IntoIterator<Item = S>,
        S: Into<String>,
    {
        rows.into_iter().map(TableRow::new).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SelectionMode;

    #[test]
    fn column_defaults_to_flexible_and_leading() {
        let column = TableColumn::new("都市");
        assert_eq!(column.title, "都市");
        assert_eq!(column.width, None);
        assert_eq!(column.align, Align::Start);
        assert!(!column.sortable, "既定では並べ替えられない");
        assert_eq!(TableColumn::from("都市"), column);
        assert_eq!(TableColumn::from(String::from("都市")), column);
    }

    #[test]
    fn column_builders_keep_the_other_fields() {
        let column = TableColumn::new("人口")
            .width(120.0)
            .align(Align::End)
            .sortable(true);
        assert_eq!(column.title, "人口");
        assert_eq!(column.width, Some(120.0));
        assert_eq!(column.align, Align::End);
        assert!(column.sortable);
    }

    #[test]
    fn sort_order_defaults_to_ascending() {
        assert_eq!(SortOrder::default(), SortOrder::Ascending);
        assert!(SortOrder::Ascending.is_ascending());
        assert!(!SortOrder::Descending.is_ascending());
        assert_eq!(SortOrder::Ascending.reversed(), SortOrder::Descending);
    }

    #[test]
    fn row_defaults_to_enabled() {
        let row = TableRow::new(["東京", "13,960,000"]);
        assert_eq!(row.cells.len(), 2);
        assert!(row.enabled);
        assert!(!TableRow::new(["大阪"]).enabled(false).enabled);
    }

    #[test]
    fn cell_fills_in_the_missing_columns() {
        let row = TableRow::new(["東京"]);
        assert_eq!(row.cell(0), "東京");
        assert_eq!(row.cell(1), "");
        assert_eq!(TableRow::default().cell(0), "");
    }

    #[test]
    fn list_keeps_the_order() {
        let rows = TableRow::list([["東京", "13,960,000"], ["大阪", "8,838,000"]]);
        assert_eq!(rows[0].cell(1), "13,960,000");
        assert_eq!(rows[1].cell(0), "大阪");
        assert!(TableRow::list::<_, [&str; 0], _>([]).is_empty());
    }

    #[test]
    fn selection_is_normalized_by_the_row_state() {
        let rows = [
            TableRow::new(["東京"]),
            TableRow::new(["大阪"]).enabled(false),
            TableRow::new(["札幌"]),
        ];
        let enabled = |i: usize| rows.get(i).is_some_and(|row: &TableRow| row.enabled);
        assert_eq!(
            SelectionMode::Multiple.normalize_by(&[2, 0, 1, 9], enabled),
            vec![0, 2]
        );
        assert_eq!(
            SelectionMode::Single.normalize_by(&[2, 0], enabled),
            vec![0]
        );
    }
}
