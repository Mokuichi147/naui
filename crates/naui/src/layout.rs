use crate::sys;
use crate::AsWidget;
use naui_core::{Align, GridCell, Padding, ScrollMetrics, ScrollPolicy, Track};

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
    pub fn attach(&self, child: &dyn AsWidget, cell: GridCell) {
        self.0.attach(child.as_widget().as_backend(), cell)
    }

    /// Replaces what is in the cell. Whatever was in the same cell is removed.
    #[inline]
    pub fn replace(&self, child: &dyn AsWidget, cell: GridCell) {
        self.0.replace(child.as_widget().as_backend(), cell)
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

/// A container that lays out its children in a row and wraps them onto new lines
/// when the row runs out of width, as text wraps (CSS `flex-wrap`).
///
/// Each line is packed on its own: the children are not lined up in columns. A
/// child goes on the current line while it fits at its natural width and starts the
/// next line otherwise. A child wider than the flow gets a line of its own; with
/// `Fill` on the width it is narrowed to the flow's width. On Linux and the Web any
/// child is narrowed, down to its minimum; on Windows and macOS one without `Fill`
/// keeps its width and is cut off.
///
/// Within a line the children are centered vertically; one with `Fill` on the
/// height is stretched to the line's height, and one with `Fill` on the width takes
/// the room left in its line (shared evenly with the other `Fill` children of that
/// line). Lines start on the left, or on the right in a right-to-left layout.
///
/// The flow's height follows its width, so give it the width to wrap in: `Fill` on
/// the width, or a parent that stretches it. With its width fitting its content
/// (`Auto`) it is as wide as its children on one line, as far as its parent allows.
///
/// Only the Web has a native container that wraps children like this. Elsewhere
/// naui measures the children and places them itself, in the toolkit's own layout
/// pass (a `GtkWidget`'s measuring, an `NSView`'s `layout`, a WinUI `Panel`'s
/// `MeasureOverride` / `ArrangeOverride`), so the lines follow every resize without
/// a round trip through the app.
///
/// ```no_run
/// # use naui::{Result, Sizing, Ui};
/// # fn build(ui: &Ui) -> Result<()> {
/// let tags = ui.flow()?;
/// tags.set_sizing(Sizing::fill_width());
/// tags.set_spacing(8.0, 8.0);
/// for name in ["Rust", "GTK", "AppKit", "WinUI", "Web"] {
///     tags.append(&ui.button(name)?);
/// }
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
pub struct Flow(pub(crate) sys::Flow);

impl_widget!(Flow);

impl Flow {
    /// The spacing between children within a line (`column`) and between lines
    /// (`row`), in the same order as [`Grid::set_spacing`].
    #[inline]
    pub fn set_spacing(&self, column: f64, row: f64) {
        self.0.set_spacing(column, row)
    }

    #[inline]
    pub fn set_padding(&self, padding: Padding) {
        self.0.set_padding(padding)
    }

    /// Where the children of a line sit when the line has room left. The default is
    /// [`Align::Start`].
    ///
    /// `Center` and `End` move the whole line. `Fill` is the same as `Start`: to
    /// widen children so that they fill their line, give them `Fill` on the width. A
    /// line with such a child gives the room to it, whatever the alignment.
    #[inline]
    pub fn set_align(&self, align: Align) {
        self.0.set_align(align)
    }

    #[inline]
    pub fn append(&self, child: &dyn AsWidget) {
        self.0.append(child.as_widget().as_backend())
    }

    /// Inserts a child at the given position. If `index` is at least the current
    /// count, it is appended at the end.
    #[inline]
    pub fn insert(&self, index: usize, child: &dyn AsWidget) {
        self.0.insert(index, child.as_widget().as_backend())
    }

    /// Removes the child at the given position. Does nothing if out of range.
    #[inline]
    pub fn remove(&self, index: usize) {
        self.0.remove(index)
    }

    /// Removes every child.
    #[inline]
    pub fn clear(&self) {
        self.0.clear()
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
    pub fn set_child(&self, child: &dyn AsWidget) {
        self.0.set_child(child.as_widget().as_backend())
    }
}

/// A widget that is just empty space. It absorbs a stack's leftover space and
/// pushes the others away.
#[derive(Clone)]
pub struct Spacer(pub(crate) sys::Spacer);

impl_widget!(Spacer);
