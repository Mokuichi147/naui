use crate::sys;
use crate::Widget;
use naui_core::{GridCell, Padding, ScrollMetrics, ScrollPolicy, Track};

/// A container that positions children by row and column (`GtkGrid`).
#[derive(Clone)]
pub struct Grid(pub(crate) sys::Grid);

impl_widget!(Grid);

impl Grid {
    #[inline]
    pub fn set_spacing(&self, column: f64, row: f64) {
        self.0.set_spacing(column, row)
    }

    #[inline]
    pub fn set_padding(&self, padding: Padding) {
        self.0.set_padding(padding)
    }

    /// Places a child in a cell. It can be stacked onto a cell that already holds
    /// something.
    #[inline]
    pub fn attach(&self, child: &dyn Widget, cell: GridCell) {
        self.0.attach(child.as_backend(), cell)
    }

    /// Replaces what is in the cell. Whatever was in the same cell is removed.
    #[inline]
    pub fn replace(&self, child: &dyn Widget, cell: GridCell) {
        self.0.replace(child.as_backend(), cell)
    }

    /// Removes every child. The row and column settings stay.
    #[inline]
    pub fn clear(&self) {
        self.0.clear()
    }

    /// Removes whatever is in the given cell. Does nothing if it is empty.
    ///
    /// Only the column and row of `cell` are considered, not the span.
    #[inline]
    pub fn remove(&self, cell: GridCell) {
        self.0.remove(cell)
    }

    /// How a column's width is determined.
    ///
    /// `Fill` columns split the leftover space by weight, never going below what
    /// their content needs (as CSS's `fr` does). On Linux a `Fixed` width is applied
    /// **to the children in that column**, which ends up setting the column width.
    #[inline]
    pub fn set_column_track(&self, index: usize, track: Track) {
        self.0.set_column_track(index, track)
    }

    /// How a row's height is determined. Has the same limitations as
    /// [`Grid::set_column_track`].
    #[inline]
    pub fn set_row_track(&self, index: usize, track: Track) {
        self.0.set_row_track(index, track)
    }

    /// The number of columns the placed children need.
    #[inline]
    pub fn columns(&self) -> usize {
        self.0.columns()
    }

    /// The number of rows the placed children need.
    #[inline]
    pub fn rows(&self) -> usize {
        self.0.rows()
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// A container that scrolls when its contents overflow (`GtkScrolledWindow`).
#[derive(Clone)]
pub struct Scroll(pub(crate) sys::Scroll);

impl_widget!(Scroll);

impl Scroll {
    /// The current scroll position and sizes.
    ///
    /// GTK4 cannot be made to lay out synchronously, so right after the contents
    /// change, the previous sizes remain until the next frame.
    #[inline]
    pub fn metrics(&self) -> ScrollMetrics {
        self.0.metrics()
    }

    /// Scrolls to the given position, clamped to the scrollable range.
    ///
    /// Even if the contents just changed, it scrolls again using the sizes after the
    /// next layout.
    #[inline]
    pub fn scroll_to(&self, x: f64, y: f64) {
        self.0.scroll_to(x, y)
    }

    /// Scrolls to the vertical end (the very bottom). The horizontal position stays.
    #[inline]
    pub fn scroll_to_end(&self) {
        self.0.scroll_to_end()
    }

    /// Notified when the scroll position changes.
    ///
    /// Besides user interaction, it also arrives for moves caused by `scroll_to` or by
    /// the contents shrinking. It does not arrive when only the sizes change.
    #[inline]
    pub fn on_scroll(&self, f: impl FnMut(ScrollMetrics) + 'static) {
        self.0.on_scroll(f)
    }

    #[inline]
    pub fn set_policy(&self, horizontal: ScrollPolicy, vertical: ScrollPolicy) {
        self.0.set_policy(horizontal, vertical)
    }

    #[inline]
    pub fn set_child(&self, child: &dyn Widget) {
        self.0.set_child(child.as_backend())
    }
}

/// A widget that is just empty space. It absorbs a stack's leftover space and
/// pushes the others away.
#[derive(Clone)]
pub struct Spacer(pub(crate) sys::Spacer);

impl_widget!(Spacer);
