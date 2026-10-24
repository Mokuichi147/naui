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

use std::ops::Range;

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
///   be the same height
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

    let top = scroll_top.max(0.0);
    // 行数が減った直後など、いまの位置が表の外を指していることがある。
    // 範囲が空にならないよう、最後の行までに抑える。
    let first = ((top / row_height).floor() as usize).min(count - 1);
    // 端が半分だけ見えている分を足して、画面の下端を含む行まで数える。
    let visible = (viewport / row_height).ceil() as usize + 1;
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

#[cfg(test)]
mod tests {
    use super::*;

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
    fn empty_window_is_reported() {
        let window = RowWindow::all(0);
        assert!(window.is_empty());
        assert!(window.is_complete());
        assert_eq!(window.indices(), 0..0);
    }
}
