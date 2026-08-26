//! テーブルの値型。
//!
//! テーブルは「列の定義 + 行の並び + いま選ばれている行」という構造を持つ。
//! リスト ([`crate::ListItem`]) と形は似ているが、
//!
//! - 1 行が複数のセルに分かれ、列ごとに幅と揃えが決まる
//! - 列の見出し (ヘッダー) がある
//!
//! という違いがあるため、別の型として持つ。バックエンドはこれを
//! `NSTableView` の列や `<table>` の `<th>` / `<td>` へ写す。
//!
//! 行の識別はリストと同じくアプリ側が持つ順序 (インデックス) で行い、
//! 選択の通知もインデックスで返る。選択の正規化には
//! [`crate::SelectionMode::normalize_by`] を使う。

use crate::Align;

/// 並べ替えの向き。
///
/// ヘッダーを押すたびに `Ascending` → `Descending` → `Ascending` と反転する。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortOrder {
    /// 小さいものが上 (既定)。
    #[default]
    Ascending,
    /// 大きいものが上。
    Descending,
}

impl SortOrder {
    pub fn is_ascending(self) -> bool {
        matches!(self, SortOrder::Ascending)
    }

    /// 向きを反転する。
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

/// テーブルの 1 列。
///
/// ```
/// # use naui_core::{Align, TableColumn};
/// let column = TableColumn::new("人口").width(120.0).align(Align::End).sortable(true);
/// assert_eq!(column.width, Some(120.0));
/// assert!(column.sortable);
/// ```
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TableColumn {
    /// 見出しに出る文字列。
    pub title: String,
    /// 列の幅 (論理ピクセル)。`None` なら余りを分け合って広がる。
    pub width: Option<f64>,
    /// セルの文字の揃え。
    ///
    /// 使うのは [`Align::Start`] / [`Align::Center`] / [`Align::End`] の 3 つで、
    /// [`Align::Fill`] は `Start` と同じ扱いになる (文字に「広げる」は無いため)。
    pub align: Align,
    /// 見出しを押して並べ替えられるかどうか。
    ///
    /// **並べ替えそのものは naui では行わない。** 押されたことが
    /// 「どの列を、どちら向きに」という形で通知されるので、アプリが
    /// 自分のデータを並べ替えて渡し直す。文字列の中身 (数値なのか日付なのか)
    /// を知っているのはアプリだけなので、比べ方もアプリが決める。
    pub sortable: bool,
}

impl TableColumn {
    /// 見出しの文字列から列を作る。幅は余りを分け合い、文字は左に寄る。
    /// 並べ替えは切ってある。
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            width: None,
            align: Align::Start,
            sortable: false,
        }
    }

    /// 列の幅を論理ピクセルで固定する。
    pub fn width(mut self, width: f64) -> Self {
        self.width = Some(width);
        self
    }

    /// セルの文字の揃えを指定する (既定は左)。
    pub fn align(mut self, align: Align) -> Self {
        self.align = align;
        self
    }

    /// 見出しを押して並べ替えられるようにする (既定は不可)。
    pub fn sortable(mut self, sortable: bool) -> Self {
        self.sortable = sortable;
        self
    }

    /// 文字列の並びから列の定義を作る。
    ///
    /// ```
    /// # use naui_core::TableColumn;
    /// let columns = TableColumn::list(["都市", "人口"]);
    /// assert_eq!(columns.len(), 2);
    /// assert!(columns[0].width.is_none(), "既定では余りを分け合う");
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

/// テーブルの 1 行。
///
/// セルは列と同じ順に並べる。**列数と食い違っていてもよい**。足りない分は
/// 空のセルになり、余った分は表示されない ([`TableRow::cell`])。
///
/// ```
/// # use naui_core::TableRow;
/// let row = TableRow::new(["東京", "13,960,000"]);
/// assert_eq!(row.cell(0), "東京");
/// assert_eq!(row.cell(5), "", "列が足りない行は空のセルになる");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TableRow {
    /// 左から順に並ぶセルの文字列。
    pub cells: Vec<String>,
    /// 選べるかどうか。
    pub enabled: bool,
}

impl TableRow {
    /// セルの並びから行を作る。
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

    /// 選べるかどうかを指定する (既定は選べる)。
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// `index` 列のセル。無ければ空文字列。
    pub fn cell(&self, index: usize) -> &str {
        self.cells.get(index).map(String::as_str).unwrap_or("")
    }

    /// セルの並びの並びから、行の列を作る。
    ///
    /// ```
    /// # use naui_core::TableRow;
    /// let rows = TableRow::list([["東京", "13,960,000"], ["大阪", "8,838,000"]]);
    /// assert_eq!(rows.len(), 2);
    /// assert_eq!(rows[1].cell(0), "大阪");
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
        let rows = vec![
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
