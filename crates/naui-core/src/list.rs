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

/// Rows moved to another place in the same list, as when the user drags them.
///
/// `rows` are the rows that move, in ascending order, and `to` is the gap they are
/// put into. Both count the rows **before** the move: `to` is 0 for the gap above
/// the first row and the number of rows for the gap below the last one. The moved
/// rows keep their order and end up next to each other, starting at
/// [`RowMove::destination`].
///
/// Apply it to the app's own data with [`RowMove::apply`], and map an index the app
/// keeps (the row it is showing, for example) with [`RowMove::new_index`].
///
/// ```
/// # use naui_core::RowMove;
/// let mut cities = vec!["Tokyo", "Osaka", "Sapporo", "Naha"];
/// // Drag Tokyo and Sapporo below Naha.
/// let moved = RowMove::new([0, 2], 4);
/// moved.apply(&mut cities);
/// assert_eq!(cities, ["Osaka", "Naha", "Tokyo", "Sapporo"]);
/// assert_eq!(moved.destination(), 2);
/// assert_eq!(moved.new_index(1), 0); // Osaka
/// assert_eq!(moved.new_index(2), 3); // Sapporo
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct RowMove {
    /// The rows that move, in ascending order without duplicates.
    pub rows: Vec<usize>,
    /// The gap they are put into, counted before the move (0 to the number of rows).
    pub to: usize,
}

impl RowMove {
    /// Moves `rows` into the gap `to`. The rows are sorted and duplicates removed.
    pub fn new(rows: impl IntoIterator<Item = usize>, to: usize) -> Self {
        let mut rows: Vec<usize> = rows.into_iter().collect();
        rows.sort_unstable();
        rows.dedup();
        Self { rows, to }
    }

    /// The move limited to a list of `len` rows: rows past the end are dropped and `to`
    /// is kept within the list. `None` when nothing would change place.
    ///
    /// ```
    /// # use naui_core::RowMove;
    /// assert_eq!(RowMove::new([1, 9], 7).within(3), Some(RowMove::new([1], 3)));
    /// // Dropping a row next to itself leaves everything where it was.
    /// assert_eq!(RowMove::new([1], 2).within(3), None);
    /// assert_eq!(RowMove::new([], 0).within(3), None);
    /// ```
    pub fn within(&self, len: usize) -> Option<Self> {
        let rows: Vec<usize> = self.rows.iter().copied().filter(|&row| row < len).collect();
        let moved = Self {
            rows,
            to: self.to.min(len),
        };
        (!moved.is_noop()).then_some(moved)
    }

    /// Whether every row stays where it was: no rows, or rows that already lie next to
    /// each other dropped into a gap at or inside their own block.
    pub fn is_noop(&self) -> bool {
        let (Some(&first), Some(&last)) = (self.rows.first(), self.rows.last()) else {
            return true;
        };
        let contiguous = last - first + 1 == self.rows.len();
        contiguous && (first..=last + 1).contains(&self.to)
    }

    /// The index of the first moved row after the move.
    pub fn destination(&self) -> usize {
        let above = self.rows.partition_point(|&row| row < self.to);
        self.to - above
    }

    /// Where the row that was at `old` is after the move.
    pub fn new_index(&self, old: usize) -> usize {
        let destination = self.destination();
        if let Ok(position) = self.rows.binary_search(&old) {
            return destination + position;
        }
        // 動かない行の中での順番。動いた行の塊より前か後ろかで決まる。
        let rank = old - self.rows.partition_point(|&row| row < old);
        if rank < destination {
            rank
        } else {
            rank + self.rows.len()
        }
    }

    /// Maps several indices with [`RowMove::new_index`], in ascending order. Handy for a
    /// selection.
    ///
    /// ```
    /// # use naui_core::RowMove;
    /// assert_eq!(RowMove::new([0], 3).new_indices(&[0, 1]), vec![0, 2]);
    /// ```
    pub fn new_indices(&self, old: &[usize]) -> Vec<usize> {
        let mut indices: Vec<usize> = old.iter().map(|&index| self.new_index(index)).collect();
        indices.sort_unstable();
        indices
    }

    /// The same move as single-row moves `(from, to)`, for native lists that move one
    /// row at a time (`moveRowAtIndex:toIndex:`, for example).
    ///
    /// Each step takes the row at `from` out and puts it back so that it ends up at `to`,
    /// both counted in the order left by the steps before. Rows that already lie where
    /// they belong are skipped.
    ///
    /// ```
    /// # use naui_core::RowMove;
    /// let mut items = vec!['a', 'b', 'c', 'd', 'e'];
    /// for (from, to) in RowMove::new([0, 2], 5).steps() {
    ///     let item = items.remove(from);
    ///     items.insert(to, item);
    /// }
    /// assert_eq!(items, ['b', 'd', 'e', 'a', 'c']);
    /// ```
    pub fn steps(&self) -> impl Iterator<Item = (usize, usize)> + '_ {
        // 隙間より上の行は下のものから、下の行は上のものから動かす。この順なら、
        // まだ動かしていない行も置き終えた行もずれないので、どの行も自分の番に
        // 元の番号のところにいる。
        let destination = self.destination();
        let above = self.rows.partition_point(|&row| row < self.to);
        (0..above)
            .rev()
            .chain(above..self.rows.len())
            .map(move |offset| (self.rows[offset], destination + offset))
            .filter(|(from, to)| from != to)
    }

    /// Moves the items of `items` the same way. Rows past its end are left out of the
    /// move.
    pub fn apply<T>(&self, items: &mut Vec<T>) {
        let Some(moved) = self.within(items.len()) else {
            return;
        };
        let mut picked = Vec::with_capacity(moved.rows.len());
        let mut rest = Vec::with_capacity(items.len() - moved.rows.len());
        for (index, item) in items.drain(..).enumerate() {
            if moved.rows.binary_search(&index).is_ok() {
                picked.push(item);
            } else {
                rest.push(item);
            }
        }
        let at = moved.destination();
        rest.splice(at..at, picked);
        *items = rest;
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

    #[test]
    fn row_move_matches_moving_a_vec() {
        // 全部の組み合わせで、apply と new_index が食い違わないことを見る。
        let len = 5;
        for mask in 1u32..(1 << len) {
            let rows: Vec<usize> = (0..len).filter(|i| mask & (1 << i) != 0).collect();
            for to in 0..=len {
                let moved = RowMove::new(rows.clone(), to);
                let mut items: Vec<usize> = (0..len).collect();
                moved.apply(&mut items);
                for old in 0..len {
                    assert_eq!(items[moved.new_index(old)], old, "{rows:?} -> {to}");
                }
                let mut stepped: Vec<usize> = (0..len).collect();
                for (from, to) in moved.steps() {
                    let item = stepped.remove(from);
                    stepped.insert(to, item);
                }
                assert_eq!(stepped, items, "steps: {rows:?} -> {to}");
                let original: Vec<usize> = (0..len).collect();
                assert_eq!(moved.is_noop(), items == original, "{rows:?} -> {to}");
                assert_eq!(moved.within(len).is_none(), moved.is_noop());
            }
        }
    }

    #[test]
    fn row_move_moves_down_and_up() {
        let mut items = vec!['a', 'b', 'c', 'd'];
        RowMove::new([0], 3).apply(&mut items);
        assert_eq!(items, ['b', 'c', 'a', 'd']);
        RowMove::new([3], 0).apply(&mut items);
        assert_eq!(items, ['d', 'b', 'c', 'a']);
        assert_eq!(RowMove::new([2, 0, 2], 1).rows, vec![0, 2]);
    }
}
