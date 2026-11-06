//! How to find "the rows on screen".
//!
//! Once there are tens of thousands of rows, turning each row into a widget makes a
//! list freeze just by opening. `NSTableView` builds only the rows it needs by
//! itself, but `GtkListBox` / `ListView` / `<table>` create every row they are
//! given. So **the visible range is computed from the scroll position, and only
//! that many rows are built**.
//!
//! The range is computed the same way on every backend, so only the calculation
//! lives here. There is no drawing or measuring here, only numbers (measuring row
//! heights is the backend's job).
//!
//! ```
//! # use naui_core::{row_window, RowWindow};
//! // 100,000 rows of 20 px. A 200 px viewport, 1,000 px from the top.
//! let window = row_window(100_000, 1_000.0, 200.0, 20.0, 5);
//! assert_eq!(window.start, 45, "from a little before the visible 50th row");
//! assert!(window.end >= 60 && window.end < 100, "to a little past the screen");
//! // Rows that are not visible become the height of the top and bottom spacers.
//! assert_eq!(window.leading(20.0), 900.0);
//! ```

use std::cell::{Cell, RefCell};
use std::collections::BTreeSet;
use std::ops::Range;
use std::rc::Rc;

use crate::Result;

/// The row count at which rows start being limited to "just the visible ones".
///
/// At or below this, every row is built. Limiting a small number of rows would make
/// keyboard navigation and scroll bar behavior stop being purely native, so
/// **tables of ordinary size stay as they always were**.
pub const ROW_WINDOW_THRESHOLD: usize = 200;

/// Extra rows built outside the screen.
///
/// A few more rows are built above and below so that blank space is not seen before
/// the scroll notification arrives. A larger value makes blanks less likely but
/// builds more.
pub const ROW_WINDOW_OVERSCAN: usize = 12;

/// How many times a build starts over because the rows were replaced during it.
///
/// A function that replaces the rows every time it is called would otherwise never
/// finish. The limit only has to be above what an app does on purpose.
const ROW_REBUILD_LIMIT: usize = 8;

/// The range of rows currently built.
///
/// A half-open range `start..end`; `end` is not included. `count` is the total
/// number of rows in the table, used to compute the heights of the top and bottom
/// spacers ([`RowWindow::leading`] / [`RowWindow::trailing`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RowWindow {
    /// The first row built.
    pub start: usize,
    /// One past the last row built (this row is not included).
    pub end: usize,
    /// The total number of rows in the table.
    pub count: usize,
}

impl RowWindow {
    /// A range that builds every row.
    pub fn all(count: usize) -> Self {
        Self {
            start: 0,
            end: count,
            count,
        }
    }

    /// The number of rows built.
    pub fn len(self) -> usize {
        self.end.saturating_sub(self.start)
    }

    pub fn is_empty(self) -> bool {
        self.len() == 0
    }

    /// Whether every row is built. `true` if nothing is limited.
    pub fn is_complete(self) -> bool {
        self.start == 0 && self.end >= self.count
    }

    /// Whether the row is built.
    pub fn contains(self, index: usize) -> bool {
        self.start <= index && index < self.end
    }

    /// The sequence of rows built.
    pub fn indices(self) -> Range<usize> {
        self.start..self.end
    }

    /// Whether every row of `needed` is inside this window.
    ///
    /// Used to avoid rebuilding on every scroll. Building with [`ROW_WINDOW_OVERSCAN`]
    /// extra rows and checking with half of that means nothing needs rebuilding until
    /// you have scrolled through half of the extra.
    ///
    /// ```
    /// # use naui_core::{row_window, RowWindow, ROW_WINDOW_OVERSCAN};
    /// let built = row_window(10_000, 1_000.0, 200.0, 20.0, ROW_WINDOW_OVERSCAN);
    /// // Scrolling by just one row does not need a rebuild yet.
    /// let needed = row_window(10_000, 1_020.0, 200.0, 20.0, ROW_WINDOW_OVERSCAN / 2);
    /// assert!(built.covers(needed));
    /// ```
    pub fn covers(self, needed: RowWindow) -> bool {
        self.count == needed.count && self.start <= needed.start && self.end >= needed.end
    }

    /// The rows that stay built when the window moves to `next`. Empty if there are none.
    ///
    /// Rebuilding a row that stays would take the keyboard focus from a control inside it,
    /// so the window moves by building only the rows that were not there. Windows over a
    /// different number of rows have nothing in common.
    ///
    /// ```
    /// # use naui_core::RowWindow;
    /// let built = RowWindow { start: 10, end: 30, count: 100 };
    /// let next = RowWindow { start: 20, end: 40, count: 100 };
    /// assert_eq!(built.overlap(next), 20..30);
    /// ```
    pub fn overlap(self, next: RowWindow) -> Range<usize> {
        let start = self.start.max(next.start);
        let end = self.end.min(next.end);
        match self.count == next.count && start < end {
            true => start..end,
            false => 0..0,
        }
    }

    /// The height taken by the rows before the range (the top spacer).
    pub fn leading(self, row_height: f64) -> f64 {
        self.start as f64 * row_height.max(0.0)
    }

    /// The height taken by the rows after the range (the bottom spacer).
    pub fn trailing(self, row_height: f64) -> f64 {
        self.count.saturating_sub(self.end) as f64 * row_height.max(0.0)
    }
}

/// Computes the range of rows to build from the scroll position.
///
/// - `count`: the total number of rows in the table
/// - `scroll_top`: the distance from the first row to the top of the viewport
///   (logical pixels)
/// - `viewport`: the height in which rows are visible (logical pixels)
/// - `row_height`: the height of one row (logical pixels); every row is assumed to
///   be the same height, and a height under 1 counts as 1
/// - `overscan`: rows to build outside the screen as well ([`ROW_WINDOW_OVERSCAN`])
///
/// If the row count is at most [`ROW_WINDOW_THRESHOLD`], every row is returned
/// without limiting. While the row height or the viewport height is still unknown
/// (0 or less, or `NaN`), the first [`ROW_WINDOW_THRESHOLD`] rows are returned as a
/// foothold for measuring.
///
/// ```
/// # use naui_core::row_window;
/// // With few rows, nothing is limited.
/// assert!(row_window(50, 0.0, 100.0, 20.0, 5).is_complete());
/// // While the heights are unknown, build just the start so it can be measured.
/// assert_eq!(row_window(10_000, 0.0, 0.0, 0.0, 5).start, 0);
/// ```
pub fn row_window(
    count: usize,
    scroll_top: f64,
    viewport: f64,
    row_height: f64,
    overscan: usize,
) -> RowWindow {
    if count <= ROW_WINDOW_THRESHOLD {
        return RowWindow::all(count);
    }
    let unknown = !row_height.is_finite() || row_height <= 0.0 || !viewport.is_finite();
    if unknown || viewport <= 0.0 {
        // まだ大きさが決まっていない。先頭だけを作れば、そこから
        // 行の高さを測って次の回で絞り直せる。
        return RowWindow {
            start: 0,
            end: count.min(ROW_WINDOW_THRESHOLD),
            count,
        };
    }

    // 1 ピクセルより低い行は描けない。指定をそのまま使うと、画面に入る行数の見積もりが
    // 全行に近づいてしまう。実際の行は、組み立てたあとで測った高さになる。
    let row_height = row_height.max(1.0);
    let top = scroll_top.max(0.0);
    // 行数が減った直後など、いまの位置が表の外を指していることがある。
    // 範囲が空にならないよう、最後の行までに抑える。
    let first = ((top / row_height).floor() as usize).min(count - 1);
    // 端が半分だけ見えている分を足して、画面の下端を含む行まで数える。
    let visible = ((viewport / row_height).ceil() as usize).saturating_add(1);
    let start = first.saturating_sub(overscan);
    let end = first
        .saturating_add(visible)
        .saturating_add(overscan)
        .min(count);
    RowWindow {
        start: start.min(end),
        end,
        count,
    }
}

/// [`row_window`] for a list, whose height may follow its rows.
///
/// A height the app limited (`Fixed` or `Fill`) scrolls, so only the rows on screen are
/// built. An `Auto` height is as tall as all the rows, so **every row is built**, each at
/// its own height.
///
/// That waits until the list has a size. Until then the first rows are built as usual,
/// because a height is often set after the rows: a list of a million rows given `Fill`
/// on the next line must not build them all first.
///
/// ```
/// # use naui_core::{list_row_window, ROW_WINDOW_THRESHOLD};
/// // An `Auto` height with a size builds every row.
/// assert_eq!(list_row_window(false, 5_000, 0.0, 200.0, 20.0, 5).len(), 5_000);
/// // Not placed yet: only the first rows, whatever the height turns out to be.
/// let first = list_row_window(false, 5_000, 0.0, 0.0, 0.0, 5);
/// assert_eq!(first.len(), ROW_WINDOW_THRESHOLD);
/// // A limited height builds what is on screen.
/// assert!(list_row_window(true, 5_000, 0.0, 200.0, 20.0, 5).len() < 20);
/// ```
pub fn list_row_window(
    height_limited: bool,
    count: usize,
    scroll_top: f64,
    viewport: f64,
    row_height: f64,
    overscan: usize,
) -> RowWindow {
    if !height_limited && viewport > 0.0 {
        return RowWindow::all(count);
    }
    row_window(count, scroll_top, viewport, row_height, overscan)
}

/// Whether the window built now can be kept.
///
/// Rebuilding everything on every scroll is heavy, so **the window is reused until
/// you have scrolled through half of the extra rows**. `needed` is the "at least
/// this much" range computed with half of [`ROW_WINDOW_OVERSCAN`], and `next` is the
/// window that a rebuild would produce.
///
/// It is rebuilt in either of these cases:
///
/// - the current window does not cover `needed` (scrolled out of it)
/// - the current window is larger than `next` (drops the extra built before the
///   sizes were known)
///
/// ```
/// # use naui_core::{keeps_row_window, row_window, ROW_WINDOW_OVERSCAN};
/// let current = row_window(10_000, 1_000.0, 200.0, 20.0, ROW_WINDOW_OVERSCAN);
/// let needed = row_window(10_000, 1_040.0, 200.0, 20.0, ROW_WINDOW_OVERSCAN / 2);
/// let next = row_window(10_000, 1_040.0, 200.0, 20.0, ROW_WINDOW_OVERSCAN);
/// // Scrolling by two rows does not rebuild.
/// assert!(keeps_row_window(current, needed, next));
/// ```
pub fn keeps_row_window(current: RowWindow, needed: RowWindow, next: RowWindow) -> bool {
    current.covers(needed) && current.len() <= next.len()
}

/// The gesture that changed the selection. Determined from the modifier keys held.
///
/// In lists whose rows are limited, this decides "whether to keep the selection
/// outside the window" ([`keeps_hidden_selection`]). Adding or removing keeps it;
/// selecting anew drops it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SelectionGesture {
    /// No modifier. Selects only the clicked row anew.
    Plain,
    /// Holding ⌘ / Ctrl. Adds to / removes from the selection.
    Toggle,
    /// Holding Shift. Selects the range from the anchor anew.
    Extend,
    /// Unknown. When the modifier keys could not be read (default).
    #[default]
    Unknown,
}

/// Whether to keep the selection outside the window.
///
/// In a list whose rows are limited, the native control knows **only the selection
/// of the rows that are built**. What comes back when the user clicks a row is also
/// limited to that range, so we must decide whether the selection outside the
/// window may be kept or should be dropped as a new selection.
///
/// | Gesture | Selection outside the window |
/// | --- | --- |
/// | [`SelectionGesture::Toggle`] (⌘ / Ctrl) | Kept |
/// | [`SelectionGesture::Plain`] / [`SelectionGesture::Extend`] | Dropped |
/// | [`SelectionGesture::Unknown`] | Inferred from how it changed |
///
/// **A Shift range selection cannot be told apart by how it changed.** A range
/// extended from the anchor can include the previous selection (`[1]` →
/// `[1, 2, 3]`), which looks the same as adding with ⌘ / Ctrl. Backends that can
/// read modifier keys should pass a [`SelectionGesture`].
///
/// `previous` is the part of the previous selection that was inside the window, and
/// `current` is the selection the native control returned. Both must be in
/// ascending order without duplicates.
///
/// ```
/// # use naui_core::{keeps_hidden_selection, SelectionGesture};
/// // Adding or removing with ⌘ / Ctrl leaves rows selected off screen as they are.
/// assert!(keeps_hidden_selection(&[1], &[1, 3], SelectionGesture::Toggle));
/// // A Shift range selection is a new selection even with the same shape, so drop.
/// assert!(!keeps_hidden_selection(&[1], &[1, 3], SelectionGesture::Extend));
/// // When modifier keys cannot be read, infer from how it changed.
/// assert!(keeps_hidden_selection(&[1], &[1, 3], SelectionGesture::Unknown));
/// assert!(!keeps_hidden_selection(&[1, 3], &[5], SelectionGesture::Unknown));
/// ```
pub fn keeps_hidden_selection(
    previous: &[usize],
    current: &[usize],
    gesture: SelectionGesture,
) -> bool {
    match gesture {
        // 足し引きなので、窓の外はそのまま。
        SelectionGesture::Toggle => true,
        // ふつうのクリックも Shift の範囲選びも、選び直し。
        SelectionGesture::Plain | SelectionGesture::Extend => false,
        SelectionGesture::Unknown => {
            let added = current.iter().all(|index| previous.contains(index));
            let removed = previous.iter().all(|index| current.contains(index));
            // どちらか一方だけの変化なら、押し足し / 押し外しとみなす。
            added || removed
        }
    }
}

/// The selection after the user changed it in a windowed list.
///
/// `picked` is what the native control returned, which covers only the rows built;
/// `previous` is the whole selection before the change. Whether the rows outside the
/// window stay is decided by [`keeps_hidden_selection`]. The result is ascending
/// without duplicates.
///
/// ```
/// # use naui_core::{merge_hidden_selection, RowWindow, SelectionGesture};
/// let window = RowWindow { start: 100, end: 140, count: 10_000 };
/// // Row 5 is off screen. Adding row 120 with ⌘ / Ctrl leaves it selected.
/// let toggled = merge_hidden_selection(window, &[5], vec![120], SelectionGesture::Toggle);
/// assert_eq!(toggled, vec![5, 120]);
/// // A plain click selects anew.
/// let plain = merge_hidden_selection(window, &[5], vec![120], SelectionGesture::Plain);
/// assert_eq!(plain, vec![120]);
/// ```
pub fn merge_hidden_selection(
    window: RowWindow,
    previous: &[usize],
    mut picked: Vec<usize>,
    gesture: SelectionGesture,
) -> Vec<usize> {
    picked.sort_unstable();
    picked.dedup();
    if window.is_complete() {
        return picked;
    }
    let inside: Vec<usize> = previous
        .iter()
        .copied()
        .filter(|&index| window.contains(index))
        .collect();
    if !keeps_hidden_selection(&inside, &picked, gesture) {
        return picked;
    }
    picked.extend(previous.iter().copied().filter(|&i| !window.contains(i)));
    picked.sort_unstable();
    picked.dedup();
    picked
}

/// The closure that builds a row when it becomes visible.
type RowBuild<T> = Box<dyn FnMut(usize) -> Result<T>>;

/// The function that answers whether a row can be selected without building it.
type RowSelectable = Rc<dyn Fn(usize) -> bool>;

/// Where the rows of a windowed list come from: either rows the app passed all at
/// once, or a function that builds a row when it becomes visible.
///
/// `T` is the backend's row type. Nothing is drawn here; backends ask for the rows in
/// their window with [`RowSource::row`].
///
/// ```
/// # use naui_core::RowSource;
/// let rows: RowSource<String> = RowSource::default();
/// rows.set_builder(100_000, |index| Ok(format!("Row {index}")));
/// assert_eq!(rows.len(), 100_000);
/// assert_eq!(rows.row(42).unwrap().unwrap(), "Row 42");
/// assert!(rows.row(100_000).is_none());
/// ```
pub struct RowSource<T> {
    rows: RefCell<Vec<T>>,
    /// Taken out only for the duration of the call, so the app can operate on the list
    /// from inside it without a double borrow.
    build: RefCell<Option<RowBuild<T>>>,
    /// Whether rows come from `build`. `build` itself is empty during the call.
    built: Cell<bool>,
    count: Cell<usize>,
    selectable: RefCell<Option<RowSelectable>>,
    /// The built rows that said they cannot be selected, kept after they leave the window.
    unselectable: RefCell<BTreeSet<usize>>,
    /// Counts the replacements, so a build under way can notice one.
    generation: Cell<u64>,
}

impl<T> Default for RowSource<T> {
    fn default() -> Self {
        Self {
            rows: RefCell::new(Vec::new()),
            build: RefCell::new(None),
            built: Cell::new(false),
            count: Cell::new(0),
            selectable: RefCell::new(None),
            unselectable: RefCell::new(BTreeSet::new()),
            generation: Cell::new(0),
        }
    }
}

impl<T: Clone> RowSource<T> {
    /// The number of rows.
    pub fn len(&self) -> usize {
        self.count.get()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Whether rows are built when they become visible ([`RowSource::set_builder`]).
    pub fn is_built(&self) -> bool {
        self.built.get()
    }

    /// Replaces the rows with ones that already exist.
    pub fn set_rows(&self, rows: Vec<T>) {
        *self.build.borrow_mut() = None;
        self.built.set(false);
        self.count.set(rows.len());
        *self.rows.borrow_mut() = rows;
        self.forget_selectable();
        self.invalidate();
    }

    /// Replaces the rows with `count` rows that `build` makes when they become visible.
    pub fn set_builder(&self, count: usize, build: impl FnMut(usize) -> Result<T> + 'static) {
        self.rows.borrow_mut().clear();
        *self.build.borrow_mut() = Some(Box::new(build));
        self.built.set(true);
        self.count.set(count);
        self.forget_selectable();
        self.invalidate();
    }

    /// Changes whenever the rows are replaced. A row built while this changed belongs
    /// to the old rows.
    pub fn generation(&self) -> u64 {
        self.generation.get()
    }

    /// Makes a build under way start over, as if the rows had been replaced.
    pub fn invalidate(&self) {
        self.generation.set(self.generation.get().wrapping_add(1));
    }

    /// Runs `build`, and runs it again whenever the rows were replaced during it.
    ///
    /// The builder is the app's code, so it can replace the rows itself. `build` receives
    /// `false` the first time and `true` when starting over, which is when to choose the
    /// range to build again. It stops after a limited number of attempts, so a builder
    /// that replaces the rows on every call leaves the last attempt as it is instead of
    /// hanging.
    ///
    /// ```
    /// # use naui_core::RowSource;
    /// let rows: RowSource<String> = RowSource::default();
    /// rows.set_rows(vec!["old".to_string()]);
    /// let mut attempts = Vec::new();
    /// rows.build_until_settled(|again| {
    ///     attempts.push(again);
    ///     if !again {
    ///         rows.set_rows(vec!["new".to_string()]);
    ///     }
    /// });
    /// assert_eq!(attempts, [false, true]);
    /// ```
    pub fn build_until_settled(&self, mut build: impl FnMut(bool)) {
        let mut generation = self.generation();
        build(false);
        for _ in 0..ROW_REBUILD_LIMIT {
            let current = self.generation();
            if current == generation {
                return;
            }
            generation = current;
            build(true);
        }
    }

    /// The contents of that row. `None` if there is no such row, or if it is asked for
    /// from inside the builder itself.
    pub fn row(&self, index: usize) -> Option<Result<T>> {
        if index >= self.len() {
            return None;
        }
        if !self.is_built() {
            return self.rows.borrow().get(index).cloned().map(Ok);
        }
        let mut build = self.build.borrow_mut().take()?;
        let row = build(index);
        let mut slot = self.build.borrow_mut();
        // 呼び出しの中で別の関数へ差し替えられていたら、そちらを残す。
        if slot.is_none() && self.is_built() {
            *slot = Some(build);
        }
        Some(row)
    }

    /// Reads a row that already exists, without building anything. `None` for rows that
    /// come from the builder.
    pub fn with_stored<R>(&self, index: usize, read: impl FnOnce(&T) -> R) -> Option<R> {
        self.rows.borrow().get(index).map(read)
    }

    /// Whether any row that already exists satisfies `test`. `false` for built rows.
    pub fn any_stored(&self, test: impl FnMut(&T) -> bool) -> bool {
        self.rows.borrow().iter().any(test)
    }

    /// Passes a function that answers whether a row can be selected without building it.
    ///
    /// It stays when the rows are replaced, so pass a new one if its answers change.
    pub fn set_selectable(&self, selectable: impl Fn(usize) -> bool + 'static) {
        *self.selectable.borrow_mut() = Some(Rc::new(selectable));
    }

    /// Remembers what a row said when it was built, so the answer outlives the row.
    ///
    /// A row is discarded when it leaves the window, and a list can go on asking about it
    /// (a Shift range, [`RowSource::is_selectable`] for an arrow key).
    pub fn note_selectable(&self, index: usize, selectable: bool) {
        let mut unselectable = self.unselectable.borrow_mut();
        match selectable {
            true => unselectable.remove(&index),
            false => unselectable.insert(index),
        };
    }

    /// Forgets what the rows said when built, for when they may answer differently.
    pub fn forget_selectable(&self) {
        self.unselectable.borrow_mut().clear();
    }

    /// Whether the row can be selected.
    ///
    /// Rows that already exist answer with `of_row`. For built rows, the function from
    /// [`RowSource::set_selectable`] answers first, then what the row said when it was
    /// built ([`RowSource::note_selectable`]). Rows nobody can answer for are assumed
    /// selectable. No borrow is held while the app's function runs.
    pub fn is_selectable(&self, index: usize, of_row: impl FnOnce(&T) -> bool) -> bool {
        if index >= self.len() {
            return false;
        }
        if !self.is_built() {
            return self.with_stored(index, of_row).unwrap_or(false);
        }
        let answer = self.selectable.borrow().clone();
        match answer {
            Some(answer) => answer(index),
            None => !self.unselectable.borrow().contains(&index),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_list_with_an_auto_height_builds_every_row_once_it_has_a_size() {
        let count = ROW_WINDOW_THRESHOLD * 10;
        assert!(list_row_window(false, count, 500.0, 200.0, 20.0, 5).is_complete());
        // 行の高さがまだ分からなくても、全行を作ることは決まっている。
        assert!(list_row_window(false, count, 0.0, 200.0, 0.0, 5).is_complete());

        // 大きさが分かるまでは、あとから高さが指定されるかもしれない。
        let unplaced = list_row_window(false, count, 0.0, 0.0, 0.0, 5);
        assert_eq!(unplaced.len(), ROW_WINDOW_THRESHOLD);
        assert_eq!(unplaced.start, 0);
    }

    #[test]
    fn a_list_with_a_limited_height_builds_what_is_on_screen() {
        let count = ROW_WINDOW_THRESHOLD * 10;
        assert_eq!(
            list_row_window(true, count, 500.0, 200.0, 20.0, 5),
            row_window(count, 500.0, 200.0, 20.0, 5)
        );
    }

    #[test]
    fn replacing_the_rows_changes_the_generation() {
        let rows: RowSource<usize> = RowSource::default();
        let generation = rows.generation();
        rows.set_rows(vec![1]);
        assert_ne!(rows.generation(), generation);
        let generation = rows.generation();
        rows.set_builder(1, Ok);
        assert_ne!(rows.generation(), generation);
        let generation = rows.generation();
        rows.row(0);
        assert_eq!(rows.generation(), generation, "読むだけでは変わらない");
    }

    #[test]
    fn a_build_without_a_replacement_runs_once() {
        let rows: RowSource<usize> = RowSource::default();
        let mut attempts = 0;
        rows.build_until_settled(|_| attempts += 1);
        assert_eq!(attempts, 1);
    }

    #[test]
    fn a_build_that_always_replaces_the_rows_stops() {
        let rows: RowSource<usize> = RowSource::default();
        let mut attempts = 0;
        rows.build_until_settled(|_| {
            attempts += 1;
            rows.set_rows(Vec::new());
        });
        assert_eq!(attempts, ROW_REBUILD_LIMIT + 1);
    }

    #[test]
    fn small_tables_are_not_windowed() {
        let window = row_window(ROW_WINDOW_THRESHOLD, 0.0, 100.0, 20.0, 5);
        assert!(window.is_complete());
        assert_eq!(window.len(), ROW_WINDOW_THRESHOLD);
        assert_eq!(window.leading(20.0), 0.0);
        assert_eq!(window.trailing(20.0), 0.0);
    }

    #[test]
    fn window_follows_the_scroll_position() {
        let window = row_window(10_000, 1_000.0, 200.0, 20.0, 5);
        // 50 行目が上端。その 5 行手前から。
        assert_eq!(window.start, 45);
        // 画面には 10 行入る。端の分と余分を足した先まで。
        assert_eq!(window.end, 50 + 11 + 5);
        assert_eq!(window.count, 10_000);
        assert!(!window.is_complete());
        assert!(window.contains(50));
        assert!(!window.contains(44));
    }

    #[test]
    fn a_built_row_still_answers_after_it_is_gone() {
        let rows: RowSource<bool> = RowSource::default();
        rows.set_builder(100, |index| Ok(index != 7));
        let selectable = |index| rows.is_selectable(index, |row| *row);
        assert!(selectable(7), "組み立てる前は分からない");

        let row = rows.row(7).unwrap().unwrap();
        rows.note_selectable(7, row);
        assert!(!selectable(7), "組み立てたときの答えを覚えている");
        assert!(selectable(6));

        rows.note_selectable(7, true);
        assert!(selectable(7), "組み立て直した答えに変わる");

        rows.note_selectable(7, false);
        rows.forget_selectable();
        assert!(selectable(7), "忘れたあとは分からない扱い");

        rows.note_selectable(7, false);
        rows.set_builder(100, |_| Ok(true));
        assert!(selectable(7), "行を差し替えると答えも消える");
    }

    #[test]
    fn a_tiny_row_height_does_not_build_every_row() {
        // いちばん小さい値では、画面に入る行数が usize に収まらない。
        for height in [1e-3, 1e-300, f64::MIN_POSITIVE] {
            let window = row_window(1_000_000, 0.0, 200.0, height, 5);
            assert!(window.len() <= 206, "{height}: {window:?}");
            let window = row_window(1_000_000, 1_000.0, 200.0, height, 5);
            assert_eq!(window.start, 995, "{height}");
        }
    }

    #[test]
    fn window_stays_inside_the_table() {
        // いちばん上。上へはみ出さない。
        let top = row_window(10_000, 0.0, 200.0, 20.0, 5);
        assert_eq!(top.start, 0);
        assert_eq!(top.leading(20.0), 0.0);
        // いちばん下。下へはみ出さない。
        let bottom = row_window(10_000, 10_000.0 * 20.0, 200.0, 20.0, 5);
        assert_eq!(bottom.end, 10_000);
        assert_eq!(bottom.trailing(20.0), 0.0);
        assert!(bottom.start < bottom.end, "行が 1 つも無い範囲にはしない");
    }

    #[test]
    fn spacers_cover_the_rows_outside_the_window() {
        let window = row_window(10_000, 1_000.0, 200.0, 20.0, 5);
        let total = window.leading(20.0) + window.len() as f64 * 20.0 + window.trailing(20.0);
        assert_eq!(total, 10_000.0 * 20.0, "詰め物と合わせて全行分の高さになる");
    }

    #[test]
    fn unknown_sizes_fall_back_to_the_first_rows() {
        for (viewport, height) in [(0.0, 20.0), (200.0, 0.0), (200.0, f64::NAN), (-1.0, 20.0)] {
            let window = row_window(10_000, 0.0, viewport, height, 5);
            assert_eq!(window.start, 0);
            assert_eq!(window.end, ROW_WINDOW_THRESHOLD);
        }
    }

    /// From a scroll position, the "at least this much" window and the "if rebuilt"
    /// window.
    fn windows(count: usize, top: f64) -> (RowWindow, RowWindow) {
        (
            row_window(count, top, 200.0, 20.0, ROW_WINDOW_OVERSCAN / 2),
            row_window(count, top, 200.0, 20.0, ROW_WINDOW_OVERSCAN),
        )
    }

    #[test]
    fn a_window_survives_small_scrolls() {
        let current = row_window(10_000, 1_000.0, 200.0, 20.0, ROW_WINDOW_OVERSCAN);
        // 余りの半分までスクロールしても、作り直さずに済む。
        for rows in 0..=(ROW_WINDOW_OVERSCAN / 2) {
            let (needed, next) = windows(10_000, 1_000.0 + rows as f64 * 20.0);
            assert!(
                keeps_row_window(current, needed, next),
                "{rows} 行ぶんのスクロール"
            );
        }
        // それより動いたら作り直す。
        let (needed, next) = windows(10_000, 1_000.0 + 20.0 * 20.0);
        assert!(!keeps_row_window(current, needed, next));
        // 行数が変わったら、範囲が同じでも作り直す。
        let (needed, next) = windows(20_000, 1_000.0);
        assert!(!keeps_row_window(current, needed, next));
    }

    #[test]
    fn an_oversized_window_is_rebuilt() {
        // 大きさが分かる前に作った窓 (先頭から threshold 行) は、
        // 高さが分かった時点で作り直す。
        let current = row_window(10_000, 0.0, 0.0, 0.0, ROW_WINDOW_OVERSCAN);
        assert_eq!(current.len(), ROW_WINDOW_THRESHOLD);
        let (needed, next) = windows(10_000, 0.0);
        assert!(current.covers(needed), "覆えてはいる");
        assert!(
            !keeps_row_window(current, needed, next),
            "覆えていても、多く作りすぎていれば作り直す"
        );
    }

    #[test]
    fn window_is_never_empty_past_the_end() {
        // 行数が減って、位置が表の外を指しているとき。
        let window = row_window(10_000, 1_000_000.0, 200.0, 20.0, 5);
        assert!(!window.is_empty(), "行が 1 つも無い範囲にはしない");
        assert_eq!(window.end, 10_000);
        assert!(window.contains(9_999));
    }

    #[test]
    fn hidden_selection_follows_the_gesture() {
        use SelectionGesture::*;
        // ⌘ / Ctrl の足し引きなら残す。
        assert!(keeps_hidden_selection(&[1], &[1, 3], Toggle));
        assert!(keeps_hidden_selection(&[1, 3], &[1], Toggle));
        // ふつうのクリックと Shift の範囲選びは落とす。形が同じでも変わらない。
        assert!(!keeps_hidden_selection(&[1], &[1, 3], Extend));
        assert!(!keeps_hidden_selection(&[1], &[3], Plain));
        assert!(!keeps_hidden_selection(&[1, 2, 3], &[1, 2, 3], Extend));
    }

    #[test]
    fn hidden_selection_falls_back_to_the_difference() {
        use SelectionGesture::Unknown;
        // 修飾キーが読めないときは、足す / 外すだけなら残す。
        assert!(keeps_hidden_selection(&[1], &[1, 3], Unknown));
        assert!(keeps_hidden_selection(&[1, 3], &[1], Unknown));
        assert!(
            keeps_hidden_selection(&[], &[2], Unknown),
            "0 件から足したとき"
        );
        assert!(keeps_hidden_selection(&[2], &[], Unknown), "全部外したとき");
        // 入れ替わったら選び直しとみなす。
        assert!(!keeps_hidden_selection(&[1, 3], &[5], Unknown));
        assert!(!keeps_hidden_selection(&[1, 3], &[3, 5], Unknown));
        assert_eq!(SelectionGesture::default(), Unknown);
    }

    #[test]
    fn merged_selection_keeps_hidden_rows_only_when_adding() {
        let window = RowWindow {
            start: 100,
            end: 140,
            count: 10_000,
        };
        use SelectionGesture::*;
        assert_eq!(
            merge_hidden_selection(window, &[5, 120], vec![120, 130], Toggle),
            vec![5, 120, 130]
        );
        assert_eq!(
            merge_hidden_selection(window, &[5, 120], vec![130], Plain),
            vec![130]
        );
        assert_eq!(
            merge_hidden_selection(window, &[5, 120], vec![130, 120], Extend),
            vec![120, 130],
            "届いた並びによらず昇順になる"
        );
        assert_eq!(
            merge_hidden_selection(window, &[9_000], vec![], Unknown),
            vec![9_000],
            "窓の中に選択が無ければ、外すだけとみなして残す"
        );
    }

    #[test]
    fn merged_selection_of_a_complete_window_is_what_arrived() {
        assert_eq!(
            merge_hidden_selection(
                RowWindow::all(3),
                &[0],
                vec![2, 1],
                SelectionGesture::Toggle
            ),
            vec![1, 2]
        );
    }

    #[test]
    fn stored_rows_come_back_without_a_builder() {
        let rows: RowSource<&str> = RowSource::default();
        assert!(rows.is_empty());
        rows.set_rows(vec!["東京", "大阪"]);
        assert_eq!(rows.len(), 2);
        assert!(!rows.is_built());
        assert_eq!(rows.row(1).unwrap().unwrap(), "大阪");
        assert!(rows.row(2).is_none());
        assert_eq!(rows.with_stored(0, |row| row.len()), Some(6));
        assert!(rows.any_stored(|row| *row == "大阪"));
    }

    #[test]
    fn builder_is_asked_only_for_the_rows_requested() {
        let asked = Rc::new(RefCell::new(Vec::new()));
        let rows: RowSource<usize> = RowSource::default();
        rows.set_builder(1_000_000, {
            let asked = asked.clone();
            move |index| {
                asked.borrow_mut().push(index);
                Ok(index * 2)
            }
        });
        assert!(rows.is_built());
        assert_eq!(rows.len(), 1_000_000);
        assert!(asked.borrow().is_empty(), "渡しただけでは呼ばれない");
        assert_eq!(rows.row(7).unwrap().unwrap(), 14);
        assert!(rows.row(1_000_000).is_none());
        assert_eq!(*asked.borrow(), vec![7]);
        assert_eq!(rows.with_stored(7, |row| *row), None);

        // 文字の行へ戻すと、組み立ての関数は使われなくなる。
        rows.set_rows(vec![1]);
        assert!(!rows.is_built());
        assert_eq!(rows.row(0).unwrap().unwrap(), 1);
        assert_eq!(asked.borrow().len(), 1);
    }

    #[test]
    fn builder_failure_reaches_the_caller() {
        let rows: RowSource<usize> = RowSource::default();
        rows.set_builder(2, |index| match index {
            0 => Ok(0),
            _ => Err(crate::Error::new("build a row", "組み立てに失敗")),
        });
        assert!(rows.row(0).unwrap().is_ok());
        assert!(rows.row(1).unwrap().is_err());
        assert!(rows.row(0).unwrap().is_ok(), "失敗のあとも関数は残る");
    }

    #[test]
    fn builder_can_touch_the_source_from_inside() {
        let rows: Rc<RowSource<usize>> = Rc::new(RowSource::default());
        let weak = Rc::downgrade(&rows);
        rows.set_builder(3, move |index| {
            let rows = weak.upgrade().expect("生きている");
            assert_eq!(rows.len(), 3, "組み立ての中でも行数は読める");
            assert!(rows.row(0).is_none(), "入れ子の組み立ては起きない");
            Ok(index)
        });
        assert_eq!(rows.row(1).unwrap().unwrap(), 1);
        assert_eq!(rows.row(2).unwrap().unwrap(), 2, "関数は元へ戻っている");
    }

    #[test]
    fn builder_replaced_from_inside_stays_replaced() {
        let rows: Rc<RowSource<usize>> = Rc::new(RowSource::default());
        let weak = Rc::downgrade(&rows);
        rows.set_builder(3, move |_| {
            let rows = weak.upgrade().expect("生きている");
            rows.set_builder(5, |_| Ok(99));
            Ok(0)
        });
        assert_eq!(rows.row(0).unwrap().unwrap(), 0);
        assert_eq!(rows.len(), 5);
        assert_eq!(rows.row(0).unwrap().unwrap(), 99, "新しい関数が残ること");

        // 文字の行へ差し替えられたときも、古い関数を戻さない。
        let weak = Rc::downgrade(&rows);
        rows.set_builder(3, move |_| {
            weak.upgrade().expect("生きている").set_rows(vec![7]);
            Ok(0)
        });
        assert_eq!(rows.row(0).unwrap().unwrap(), 0);
        assert!(!rows.is_built());
        assert_eq!(rows.row(0).unwrap().unwrap(), 7);
    }

    #[test]
    fn selectable_is_answered_in_order() {
        let rows: RowSource<bool> = RowSource::default();
        rows.set_rows(vec![true, false]);
        let of_row = |row: &bool| *row;
        // 持っている行は、その行が答える。関数があっても優先しない。
        rows.set_selectable(|_| false);
        assert!(rows.is_selectable(0, of_row));
        assert!(!rows.is_selectable(1, of_row));
        assert!(!rows.is_selectable(2, of_row), "範囲の外");

        let rows: RowSource<bool> = RowSource::default();
        rows.set_builder(10, |_| Ok(true));
        // 誰も答えられなければ、選べる扱い。
        assert!(rows.is_selectable(5, of_row));
        // 組み立て済みなら、その行の答え。
        rows.note_selectable(5, false);
        assert!(!rows.is_selectable(5, of_row));
        // 組み立てずに答える関数があれば、それが先。
        rows.note_selectable(6, false);
        rows.set_selectable(|index| index != 5);
        assert!(!rows.is_selectable(5, of_row));
        assert!(rows.is_selectable(6, of_row));
        assert!(!rows.is_selectable(10, of_row), "範囲の外");
    }

    #[test]
    fn overlap_is_what_both_windows_build() {
        let window = |start, end| RowWindow {
            start,
            end,
            count: 100,
        };
        assert_eq!(window(10, 30).overlap(window(20, 40)), 20..30, "先へ動く");
        assert_eq!(window(20, 40).overlap(window(10, 30)), 20..30, "手前へ動く");
        assert_eq!(window(10, 30).overlap(window(15, 25)), 15..25, "狭まる");
        assert!(
            window(10, 20).overlap(window(20, 30)).is_empty(),
            "接するだけ"
        );
        assert!(
            window(10, 20).overlap(window(50, 60)).is_empty(),
            "離れている"
        );
        let other = RowWindow {
            start: 10,
            end: 30,
            count: 99,
        };
        assert!(window(10, 30).overlap(other).is_empty(), "行数が違う");
    }

    #[test]
    fn empty_window_is_reported() {
        let window = RowWindow::all(0);
        assert!(window.is_empty());
        assert!(window.is_complete());
        assert_eq!(window.indices(), 0..0);
    }
}
