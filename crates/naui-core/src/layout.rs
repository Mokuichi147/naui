//! Layout value types.
//!
//! Everything here only describes "how you want things placed"; no layout is
//! computed. The actual placement is done by each backend's native layout system
//! (Auto Layout / the XAML layout pass / CSS).

/// How a width or height is determined.
///
/// | Value | Meaning |
/// | --- | --- |
/// | [`Length::Auto`] | Fit the content (default) |
/// | [`Length::Fixed`] | Fixed in logical pixels |
/// | [`Length::Fill`] | Grow to take the parent's leftover space |
///
/// What `Fill` means depends on the axis. Along the parent's stacking direction
/// (the main axis) it **takes the leftover space**; in the perpendicular direction
/// (the cross axis) it **stretches to fill the parent**.
///
/// Per-backend behavior:
///
/// | | `Fill` on the main axis | `Fill` on the cross axis |
/// | --- | --- | --- |
/// | macOS (AppKit) | Lowers the hugging priority to take leftover space from the NSStackView | A constraint matching the parent's width / height |
/// | Web (DOM) | `flex-grow: 1` | `align-self: stretch` |
/// | Windows (WinUI 3) | **No effect in a Stack** (StackPanel does not hand out leftover space). Use [`Track::Fill`] in a `Grid` | `Stretch` for `HorizontalAlignment`/`VerticalAlignment` |
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Length {
    /// Fit the content (default).
    #[default]
    Auto,
    /// Fixed in logical pixels.
    Fixed(f64),
    /// Grow to take the parent's leftover space.
    Fill,
}

impl Length {
    /// The value, if this is a fixed length.
    pub fn fixed_value(self) -> Option<f64> {
        match self {
            Length::Fixed(v) => Some(v),
            _ => None,
        }
    }

    pub fn is_fill(self) -> bool {
        matches!(self, Length::Fill)
    }
}

/// The size specification for one widget.
///
/// Everything defaults to "fit the content", and only the fields you set are
/// applied on the native side.
///
/// Adding a maximum ([`Sizing::max_width`] / [`Sizing::max_height`]) to
/// [`Length::Fill`] makes the maximum double as "the size you want under normal
/// conditions". **When the parent can hand out space it grows up to the maximum,
/// and when there is not enough it becomes smaller than the maximum.** Use this
/// form to size the display area of widgets whose intrinsic size cannot be relied
/// on (video, for example).
///
/// ```
/// # use naui_core::Sizing;
/// // Full width; 315px high when there is room, less when it is tight.
/// let sizing = Sizing::fill().max_height(315.0);
/// ```
///
/// ```
/// # use naui_core::{Length, Sizing};
/// // Full width and 200px high, but at least 120px wide.
/// let sizing = Sizing::new()
///     .width(Length::Fill)
///     .height(Length::Fixed(200.0))
///     .min_width(120.0);
/// assert_eq!(sizing.min_width, Some(120.0));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Sizing {
    pub width: Length,
    pub height: Length,
    pub min_width: Option<f64>,
    pub min_height: Option<f64>,
    pub max_width: Option<f64>,
    pub max_height: Option<f64>,
}

impl Sizing {
    /// Everything "fits the content".
    pub const AUTO: Sizing = Sizing::new();

    pub const fn new() -> Self {
        Self {
            width: Length::Auto,
            height: Length::Auto,
            min_width: None,
            min_height: None,
            max_width: None,
            max_height: None,
        }
    }

    /// Fixes both width and height.
    pub const fn fixed(width: f64, height: f64) -> Self {
        Self::new()
            .width(Length::Fixed(width))
            .height(Length::Fixed(height))
    }

    /// Stretches both width and height to fill the parent.
    pub const fn fill() -> Self {
        Self::new().width(Length::Fill).height(Length::Fill)
    }

    /// Stretches only the width to fill the parent.
    pub const fn fill_width() -> Self {
        Self::new().width(Length::Fill)
    }

    /// Stretches only the height to fill the parent.
    pub const fn fill_height() -> Self {
        Self::new().height(Length::Fill)
    }

    pub const fn width(mut self, width: Length) -> Self {
        self.width = width;
        self
    }

    pub const fn height(mut self, height: Length) -> Self {
        self.height = height;
        self
    }

    pub const fn min_width(mut self, value: f64) -> Self {
        self.min_width = Some(value);
        self
    }

    pub const fn min_height(mut self, value: f64) -> Self {
        self.min_height = Some(value);
        self
    }

    pub const fn max_width(mut self, value: f64) -> Self {
        self.max_width = Some(value);
        self
    }

    pub const fn max_height(mut self, value: f64) -> Self {
        self.max_height = Some(value);
        self
    }

    /// Sets the minimum width and height together.
    pub const fn min_size(self, width: f64, height: f64) -> Self {
        self.min_width(width).min_height(height)
    }

    /// Sets the maximum width and height together.
    pub const fn max_size(self, width: f64, height: f64) -> Self {
        self.max_width(width).max_height(height)
    }
}

/// How the width of one grid column (or row) is determined.
///
/// | Value | Web | Windows | macOS |
/// | --- | --- | --- | --- |
/// | [`Track::Auto`] | `auto` | `Auto` of `GridLength` | NSGridView's default (fit the content) |
/// | [`Track::Fixed`] | `<n>px` | `Pixel` of `GridLength` | `NSGridColumn::setWidth` |
/// | [`Track::Fill`] | `<w>fr` | `Star` of `GridLength` | Approximated (`Fill` placement + hugging priority) |
///
/// The weight of `Fill` expresses "in what ratio to split the leftover space".
/// macOS's NSGridView has no notion of weights, so differing weights have no
/// effect there.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Track {
    /// Fit the content (default).
    #[default]
    Auto,
    /// Fixed in logical pixels.
    Fixed(f64),
    /// Shares the leftover space in proportion to the weights.
    Fill(f64),
}

impl Track {
    /// Takes leftover space with a weight of 1.
    pub const FILL: Track = Track::Fill(1.0);

    /// The weight used when splitting leftover space. 0 for anything other than `Fill`.
    ///
    /// Weights of 0 or less are treated as 1 (`0fr` would receive nothing, which goes
    /// against the intent of asking it to grow).
    pub fn weight(self) -> f64 {
        match self {
            Track::Fill(w) if w > 0.0 => w,
            Track::Fill(_) => 1.0,
            _ => 0.0,
        }
    }

    pub fn is_fill(self) -> bool {
        matches!(self, Track::Fill(_))
    }
}

/// A position on the grid. The top left is `(0, 0)`.
///
/// Within a cell, content is centered vertically (only children with
/// [`Length::Fill`] stretch to fill the cell). Items of different heights in the
/// same row do not end up misaligned at the top.
///
/// ```
/// # use naui_core::GridCell;
/// // Placed in the 2nd column of the 1st row, spanning two cells horizontally.
/// let cell = GridCell::new(1, 0).span(2, 1);
/// assert_eq!(cell.column_span, 2);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GridCell {
    /// The column counted from the left (starting at 0).
    pub column: usize,
    /// The row counted from the top (starting at 0).
    pub row: usize,
    /// The number of cells spanned horizontally (1 or more).
    pub column_span: usize,
    /// The number of cells spanned vertically (1 or more).
    pub row_span: usize,
}

impl GridCell {
    pub const fn new(column: usize, row: usize) -> Self {
        Self {
            column,
            row,
            column_span: 1,
            row_span: 1,
        }
    }

    /// Sets how many cells to span. 0 is treated as 1.
    pub const fn span(mut self, column_span: usize, row_span: usize) -> Self {
        self.column_span = if column_span == 0 { 1 } else { column_span };
        self.row_span = if row_span == 0 { 1 } else { row_span };
        self
    }

    /// The number of columns this placement needs.
    pub const fn columns_needed(&self) -> usize {
        self.column + self.column_span
    }

    /// The number of rows this placement needs.
    pub const fn rows_needed(&self) -> usize {
        self.row + self.row_span
    }
}

impl Default for GridCell {
    fn default() -> Self {
        Self::new(0, 0)
    }
}

/// When scroll bars are shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ScrollPolicy {
    /// Scrollable only when the content overflows (default).
    #[default]
    Auto,
    /// Always scrollable (scroll bars are shown).
    Always,
    /// Never scrolls.
    Never,
}

impl ScrollPolicy {
    pub fn is_enabled(self) -> bool {
        !matches!(self, ScrollPolicy::Never)
    }
}

/// The scroll position and sizes, in logical pixels.
///
/// `x` / `y` is where in the content the top left of the visible area is. It is 0
/// at the very top (left) and [`max_y`](Self::max_y) ([`max_x`](Self::max_x)) at
/// the very bottom (right). The visible area (`viewport_*`) excludes the space
/// scroll bars take up when they take up space, and the content (`content_*`) is
/// never smaller than the visible area.
///
/// Decisions such as "follow along if within N px of the end" are up to the app,
/// using [`distance_to_end`](Self::distance_to_end) and friends.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ScrollMetrics {
    pub x: f64,
    pub y: f64,
    pub viewport_width: f64,
    pub viewport_height: f64,
    pub content_width: f64,
    pub content_height: f64,
}

impl ScrollMetrics {
    /// The furthest horizontal position. 0 if nothing overflows.
    pub fn max_x(&self) -> f64 {
        (self.content_width - self.viewport_width).max(0.0)
    }

    /// The furthest vertical position. 0 if nothing overflows.
    pub fn max_y(&self) -> f64 {
        (self.content_height - self.viewport_height).max(0.0)
    }

    /// The distance from the bottom of the visible area to the end (very bottom) of the
    /// content. 0 if the end is visible.
    pub fn distance_to_end(&self) -> f64 {
        (self.max_y() - self.y).max(0.0)
    }
}

/// Where to scroll to. Lets a backend remember a `scroll_to` / `scroll_to_end`
/// request until layout has run.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ScrollTarget {
    /// To the given position.
    To { x: f64, y: f64 },
    /// To the vertical end (horizontal position unchanged).
    End,
}

impl ScrollTarget {
    /// Decides the actual position to scroll to with the current sizes, clamped to the
    /// scrollable range.
    pub fn resolve(self, metrics: &ScrollMetrics) -> (f64, f64) {
        let (x, y) = match self {
            Self::To { x, y } => (x, y),
            Self::End => (metrics.x, metrics.max_y()),
        };
        (
            clamp_offset(x, metrics.max_x()),
            clamp_offset(y, metrics.max_y()),
        )
    }
}

/// Treats NaN as 0 (`f64::clamp` returns NaN unchanged).
fn clamp_offset(value: f64, max: f64) -> f64 {
    if value.is_nan() {
        0.0
    } else {
        value.clamp(0.0, max)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizing_defaults_to_auto() {
        let s = Sizing::new();
        assert_eq!(s.width, Length::Auto);
        assert_eq!(s.height, Length::Auto);
        assert_eq!(s.min_width, None);
        assert_eq!(s, Sizing::default());
        assert_eq!(s, Sizing::AUTO);
    }

    #[test]
    fn sizing_builders_keep_other_fields() {
        let s = Sizing::fixed(100.0, 40.0)
            .min_width(10.0)
            .max_size(200.0, 80.0);
        assert_eq!(s.width, Length::Fixed(100.0));
        assert_eq!(s.height, Length::Fixed(40.0));
        assert_eq!(s.min_width, Some(10.0));
        assert_eq!(s.max_width, Some(200.0));
        assert_eq!(s.max_height, Some(80.0));
        assert_eq!(s.min_height, None);
    }

    #[test]
    fn fill_helpers_touch_one_axis_only() {
        assert!(Sizing::fill_width().width.is_fill());
        assert!(!Sizing::fill_width().height.is_fill());
        assert!(Sizing::fill().height.is_fill());
        assert_eq!(Length::Fixed(12.0).fixed_value(), Some(12.0));
        assert_eq!(Length::Fill.fixed_value(), None);
    }

    #[test]
    fn track_weight_falls_back_to_one() {
        assert_eq!(Track::FILL.weight(), 1.0);
        assert_eq!(Track::Fill(2.5).weight(), 2.5);
        // 0 以下は「広がらない」ではなく重み 1 として扱う。
        assert_eq!(Track::Fill(0.0).weight(), 1.0);
        assert_eq!(Track::Fill(-3.0).weight(), 1.0);
        assert_eq!(Track::Auto.weight(), 0.0);
        assert!(!Track::Fixed(10.0).is_fill());
    }

    #[test]
    fn grid_cell_span_is_at_least_one() {
        let cell = GridCell::new(2, 3);
        assert_eq!((cell.column_span, cell.row_span), (1, 1));
        let spanned = cell.span(0, 2);
        assert_eq!(spanned.column_span, 1);
        assert_eq!(spanned.row_span, 2);
        assert_eq!(spanned.columns_needed(), 3);
        assert_eq!(spanned.rows_needed(), 5);
    }

    #[test]
    fn scroll_metrics_measure_the_hidden_part() {
        let m = ScrollMetrics {
            x: 0.0,
            y: 100.0,
            viewport_width: 200.0,
            viewport_height: 300.0,
            content_width: 200.0,
            content_height: 1000.0,
        };
        assert_eq!(m.max_x(), 0.0);
        assert_eq!(m.max_y(), 700.0);
        assert_eq!(m.distance_to_end(), 600.0);

        // 中身が見えている範囲より小さくても負にならない。
        let short = ScrollMetrics {
            content_height: 10.0,
            ..m
        };
        assert_eq!(short.max_y(), 0.0);
        assert_eq!(short.distance_to_end(), 0.0);
    }

    #[test]
    fn scroll_target_clamps_to_the_range() {
        let m = ScrollMetrics {
            x: 5.0,
            y: 0.0,
            viewport_width: 100.0,
            viewport_height: 100.0,
            content_width: 150.0,
            content_height: 400.0,
        };
        assert_eq!(ScrollTarget::End.resolve(&m), (5.0, 300.0));
        assert_eq!(
            ScrollTarget::To { x: -10.0, y: 1e9 }.resolve(&m),
            (0.0, 300.0)
        );
        assert_eq!(
            ScrollTarget::To {
                x: f64::NAN,
                y: 20.0
            }
            .resolve(&m),
            (0.0, 20.0)
        );
    }

    #[test]
    fn scroll_policy_defaults_to_auto() {
        assert_eq!(ScrollPolicy::default(), ScrollPolicy::Auto);
        assert!(ScrollPolicy::Always.is_enabled());
        assert!(!ScrollPolicy::Never.is_enabled());
    }
}
