//! What a chart shows, and how it is drawn on a [`Painter`].

use std::fmt;
use std::rc::Rc;

use naui::{Align, Ink, Painter, Path, Point, Rect, TextColor};

use crate::palette::series_color;
use crate::thin;
use crate::Ticks;

/// Space around the chart, inside the surface (logical pixels).
const PADDING: f64 = 8.0;
/// Text sizes: tick labels, then axis titles and the legend.
const TICK_TEXT: f64 = 11.0;
const TITLE_TEXT: f64 = 12.0;
/// The height of a line of text, as a multiple of its size. Platforms do not tell
/// the height of text, so this is an estimate that suits the UI fonts.
const LINE_HEIGHT: f64 = 1.3;
/// Where the middle of a line of text sits below its top, as a multiple of its size.
const TEXT_MIDDLE: f64 = 0.62;
/// The tick marks below the x axis, and the space between marks and labels.
const TICK_LENGTH: f64 = 4.0;
const LABEL_GAP: f64 = 4.0;
/// Line and marker sizes.
const LINE_WIDTH: f64 = 2.0;
const MARKER_RADIUS: f64 = 4.0;
/// The share of a category's band its bars take, and the space between bars.
const BAR_GROUP: f64 = 0.8;
const BAR_GAP: f64 = 2.0;
/// The rounding of the end of a bar away from the baseline.
const BAR_RADIUS: f64 = 4.0;
/// About how far apart ticks are on the y axis (logical pixels).
const Y_TICK_SPACING: f64 = 48.0;
/// The opacity of the grid lines, over the secondary text color.
const GRID_OPACITY: f64 = 0.35;
/// Legend items: the mark, the space after it, and the space between items.
const SWATCH: f64 = 14.0;
const SWATCH_GAP: f64 = 6.0;
const LEGEND_ITEM_GAP: f64 = 16.0;
/// Scatter charts with more points than this drop markers that would land on
/// top of one drawn already.
const EXACT_MARKERS: usize = 2000;

/// How a series is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SeriesKind {
    /// A line through the points in the order given. A point whose `y` (or `x`)
    /// is NaN or infinite leaves a gap.
    Line,
    /// Bars from zero to each value. Several bar series stand side by side.
    Bar,
    /// A dot at each point.
    Scatter,
}

/// One set of values in a chart, with its name (shown in the legend) and how it
/// is drawn.
///
/// Points are `(x, y)`. On a chart with [categories](Chart::categories), `x` is
/// the category's index (0 for the first); [`line`](Self::line) and
/// [`bar`](Self::bar) number the values that way. Points that are not finite are
/// left out (a line has a gap there).
///
/// ```
/// # use naui_charts::{Series, SeriesKind};
/// # use naui::TextColor;
/// let sales = Series::bar("Sales", [120.0, 90.0, 140.0]);
/// let trend = Series::line_xy("Trend", [(0.0, 100.0), (2.0, 130.0)]).color(TextColor::Accent);
/// assert_eq!(sales.kind(), SeriesKind::Bar);
/// assert_eq!(trend.points()[1], (2.0, 130.0));
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct Series {
    name: String,
    kind: SeriesKind,
    points: Vec<(f64, f64)>,
    color: Option<Ink>,
}

impl Series {
    /// A series of the given kind.
    pub fn new(
        kind: SeriesKind,
        name: impl Into<String>,
        points: impl IntoIterator<Item = (f64, f64)>,
    ) -> Series {
        Series {
            name: name.into(),
            kind,
            points: points.into_iter().collect(),
            color: None,
        }
    }

    /// A line through `values`, the first at `x` 0, the next at 1, and so on.
    pub fn line(name: impl Into<String>, values: impl IntoIterator<Item = f64>) -> Series {
        Series::new(SeriesKind::Line, name, numbered(values))
    }

    /// A line through `(x, y)` points.
    pub fn line_xy(
        name: impl Into<String>,
        points: impl IntoIterator<Item = (f64, f64)>,
    ) -> Series {
        Series::new(SeriesKind::Line, name, points)
    }

    /// A bar for each of `values`, the first at `x` 0, the next at 1, and so on.
    pub fn bar(name: impl Into<String>, values: impl IntoIterator<Item = f64>) -> Series {
        Series::new(SeriesKind::Bar, name, numbered(values))
    }

    /// A bar at each `(x, y)` point. Without categories, the bars are as wide as
    /// the closest two `x` allow.
    pub fn bar_xy(name: impl Into<String>, points: impl IntoIterator<Item = (f64, f64)>) -> Series {
        Series::new(SeriesKind::Bar, name, points)
    }

    /// A dot at each `(x, y)` point.
    pub fn scatter(
        name: impl Into<String>,
        points: impl IntoIterator<Item = (f64, f64)>,
    ) -> Series {
        Series::new(SeriesKind::Scatter, name, points)
    }

    /// Draws the series in `color` instead of the palette's
    /// ([`series_color`](crate::series_color)). A [`Color`](naui::Color) stays the
    /// same in light and dark; a role such as `TextColor::Accent` follows them.
    pub fn color(mut self, color: impl Into<Ink>) -> Series {
        self.color = Some(color.into());
        self
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn kind(&self) -> SeriesKind {
        self.kind
    }

    pub fn points(&self) -> &[(f64, f64)] {
        &self.points
    }

    /// The points, to change in place (to add the latest value, say).
    pub fn points_mut(&mut self) -> &mut Vec<(f64, f64)> {
        &mut self.points
    }

    /// The color set with [`color`](Self::color), if any.
    pub fn custom_color(&self) -> Option<Ink> {
        self.color
    }

    fn finite_points(&self) -> impl Iterator<Item = (f64, f64)> + '_ {
        self.points
            .iter()
            .copied()
            .filter(|(x, y)| x.is_finite() && y.is_finite())
    }
}

fn numbered(values: impl IntoIterator<Item = f64>) -> impl Iterator<Item = (f64, f64)> {
    values
        .into_iter()
        .enumerate()
        .map(|(i, value)| (i as f64, value))
}

/// Turns a value into the text of a tick label.
type Format = Rc<dyn Fn(f64) -> String>;

/// The settings of one axis: a title, a fixed range, how its values are written,
/// and whether grid lines cross the plot at its ticks.
///
/// ```
/// # use naui_charts::Axis;
/// let y = Axis::new().title("Sales").range(0.0, 200.0).format(|v| format!("${v}"));
/// assert_eq!(y.range_value(), Some((0.0, 200.0)));
/// ```
#[derive(Clone, Default)]
pub struct Axis {
    title: Option<String>,
    range: Option<(f64, f64)>,
    format: Option<Format>,
    grid: Option<bool>,
}

impl Axis {
    pub fn new() -> Axis {
        Axis::default()
    }

    /// The name written along the axis.
    pub fn title(mut self, title: impl Into<String>) -> Axis {
        self.title = Some(title.into());
        self
    }

    /// Shows `min..max` instead of fitting the range to the data. Ticks fall only
    /// inside it, and what lies outside is cut at the edge of the plot. A range
    /// that is empty or not finite is ignored.
    pub fn range(mut self, min: f64, max: f64) -> Axis {
        self.range = (min.is_finite() && max.is_finite() && min != max)
            .then(|| (min.min(max), min.max(max)));
        self
    }

    /// Writes the tick labels with `format` instead of with the decimals the step
    /// needs. It is not used for the labels of categories.
    pub fn format(mut self, format: impl Fn(f64) -> String + 'static) -> Axis {
        self.format = Some(Rc::new(format));
        self
    }

    /// Whether grid lines cross the plot at the ticks. By default the y axis has
    /// them and the x axis does not.
    pub fn grid(mut self, grid: bool) -> Axis {
        self.grid = Some(grid);
        self
    }

    pub fn title_value(&self) -> Option<&str> {
        self.title.as_deref()
    }

    pub fn range_value(&self) -> Option<(f64, f64)> {
        self.range
    }

    fn label(&self, ticks: &Ticks, value: f64) -> String {
        match &self.format {
            Some(format) => format(value),
            None => ticks.label(value),
        }
    }
}

impl fmt::Debug for Axis {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Axis")
            .field("title", &self.title)
            .field("range", &self.range)
            .field("format", &self.format.is_some())
            .field("grid", &self.grid)
            .finish()
    }
}

/// Whether the legend is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum Legend {
    /// Shown when there are two series or more (one series needs no legend).
    #[default]
    Auto,
    Shown,
    Hidden,
}

/// A line, bar or scatter chart, or a mix of them, as a value: its series, its
/// axes and its legend. [`draw`](Self::draw) draws it on a [`Painter`], and
/// [`ChartView`](crate::ChartView) shows it in a `Canvas`.
///
/// The axes, tick labels, grid lines and legend text are drawn in the platform's
/// text colors ([`TextColor::Secondary`] and [`TextColor::Default`]) and the series
/// in a palette chosen for light or dark, so the chart reads in both and follows a
/// switch between them.
///
/// ```
/// # use naui_charts::{Axis, Chart, Series};
/// let chart = Chart::new()
///     .categories(["Apr", "May", "Jun"])
///     .y_axis(Axis::new().title("Visits"))
///     .series(Series::bar("2025", [120.0, 90.0, 140.0]))
///     .series(Series::line("2026", [130.0, 110.0, 150.0]));
/// assert_eq!(chart.series_list().len(), 2);
/// ```
#[derive(Debug, Clone, Default)]
pub struct Chart {
    series: Vec<Series>,
    categories: Option<Vec<String>>,
    x_axis: Axis,
    y_axis: Axis,
    legend: Legend,
}

impl Chart {
    pub fn new() -> Chart {
        Chart::default()
    }

    /// Adds a series. Series take the palette's colors in the order they are added.
    pub fn series(mut self, series: Series) -> Chart {
        self.series.push(series);
        self
    }

    /// Makes the x axis one of named categories, a band each, labeled under it.
    /// The values of [`Series::line`] and [`Series::bar`] fall on them in order.
    pub fn categories<S: Into<String>>(mut self, categories: impl IntoIterator<Item = S>) -> Chart {
        self.set_categories(categories);
        self
    }

    pub fn x_axis(mut self, axis: Axis) -> Chart {
        self.x_axis = axis;
        self
    }

    pub fn y_axis(mut self, axis: Axis) -> Chart {
        self.y_axis = axis;
        self
    }

    pub fn legend(mut self, legend: Legend) -> Chart {
        self.legend = legend;
        self
    }

    pub fn series_list(&self) -> &[Series] {
        &self.series
    }

    /// The series, to add, remove or change in place.
    ///
    /// Colors from the palette go by position, so removing a series changes the
    /// color of those after it. Give series that come and go a
    /// [`color`](Series::color) of their own.
    pub fn series_mut(&mut self) -> &mut Vec<Series> {
        &mut self.series
    }

    /// The categories, or `None` for a numeric x axis.
    pub fn categories_list(&self) -> Option<&[String]> {
        self.categories.as_deref()
    }

    pub fn set_categories<S: Into<String>>(&mut self, categories: impl IntoIterator<Item = S>) {
        self.categories = Some(categories.into_iter().map(Into::into).collect());
    }

    /// Makes the x axis numeric again.
    pub fn clear_categories(&mut self) {
        self.categories = None;
    }

    pub fn x_axis_mut(&mut self) -> &mut Axis {
        &mut self.x_axis
    }

    pub fn y_axis_mut(&mut self) -> &mut Axis {
        &mut self.y_axis
    }

    pub fn set_legend(&mut self, legend: Legend) {
        self.legend = legend;
    }

    /// Draws the chart over the whole surface of `painter`, and tells where the
    /// plot ended up.
    ///
    /// The painter's state (opacity, alignment, clip) is the same afterwards. To
    /// draw the chart in part of a surface, `translate` and `clip_rect` first.
    pub fn draw(&self, painter: &mut Painter) -> ChartLayout {
        painter.save();
        let layout = Drawing::new(self, painter).draw(painter);
        painter.restore();
        layout
    }

    fn legend_shown(&self) -> bool {
        match self.legend {
            Legend::Auto => self.series.len() >= 2,
            Legend::Shown => !self.series.is_empty(),
            Legend::Hidden => false,
        }
    }

    fn has_bars(&self) -> bool {
        self.series.iter().any(|s| s.kind == SeriesKind::Bar)
    }

    fn ink(&self, index: usize, dark: bool) -> Ink {
        self.series[index]
            .color
            .unwrap_or_else(|| Ink::Color(series_color(index, dark)))
    }
}

/// Where a chart was drawn: the plot area and the ranges of its axes, to turn a
/// pointer position into values and back.
///
/// On a chart with categories, `x` is in categories: the first one's band runs
/// from -0.5 to 0.5.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChartLayout {
    plot: Rect,
    x: (f64, f64),
    y: (f64, f64),
}

impl ChartLayout {
    /// The area the series are drawn in (logical pixels on the surface).
    pub fn plot(&self) -> Rect {
        self.plot
    }

    /// The values at the left and right edges of the plot.
    pub fn x_range(&self) -> (f64, f64) {
        self.x
    }

    /// The values at the bottom and top edges of the plot.
    pub fn y_range(&self) -> (f64, f64) {
        self.y
    }

    /// Where the value `(x, y)` is on the surface.
    pub fn to_point(&self, x: f64, y: f64) -> Point {
        Point::new(
            map(x, self.x, (self.plot.x, self.plot.right())),
            map(y, self.y, (self.plot.bottom(), self.plot.y)),
        )
    }

    /// The value at `point` on the surface (it may lie outside the plot).
    pub fn to_value(&self, point: Point) -> (f64, f64) {
        (
            map(point.x, (self.plot.x, self.plot.right()), self.x),
            map(point.y, (self.plot.bottom(), self.plot.y), self.y),
        )
    }
}

/// Maps `value` from one range onto another, in a straight line.
fn map(value: f64, from: (f64, f64), to: (f64, f64)) -> f64 {
    let span = from.1 - from.0;
    if span == 0.0 || !span.is_finite() {
        return (to.0 + to.1) / 2.0;
    }
    to.0 + (value - from.0) / span * (to.1 - to.0)
}

/// The x axis as it will be drawn.
enum XAxis {
    Categories {
        labels: Vec<String>,
        /// Every how many categories a label is written.
        every: usize,
    },
    Numeric(Ticks),
}

/// The layout worked out for one draw.
struct Drawing<'a> {
    chart: &'a Chart,
    dark: bool,
    layout: ChartLayout,
    x_axis: XAxis,
    y_ticks: Ticks,
    /// The width of a bar band, in values of `x` (1 with categories).
    band: f64,
    legend: Vec<LegendRow>,
}

/// A row of the legend: its top, and the series in it with where each starts.
struct LegendRow {
    top: f64,
    items: Vec<(usize, f64)>,
}

impl<'a> Drawing<'a> {
    fn new(chart: &'a Chart, painter: &Painter) -> Drawing<'a> {
        let bounds = painter.bounds();
        let line = |size: f64| size * LINE_HEIGHT;
        let mut top = PADDING;
        let mut bottom = bounds.height - PADDING;
        let mut left = PADDING;
        let right = bounds.width - PADDING;
        // y 軸の見出しは左端の列に縦に置く。凡例と重ならないよう、凡例はその右から。
        if chart.y_axis.title.is_some() {
            left += line(TITLE_TEXT) + LABEL_GAP;
        }

        // 凡例は上に、幅に収まるだけ並べて折り返す。
        let mut legend = Vec::new();
        if chart.legend_shown() {
            let mut x = left;
            let mut row = LegendRow {
                top,
                items: Vec::new(),
            };
            for (index, series) in chart.series.iter().enumerate() {
                let width = SWATCH + SWATCH_GAP + painter.text_width(&series.name, TITLE_TEXT);
                if !row.items.is_empty() && x + width > right {
                    let next = row.top + line(TITLE_TEXT) + 2.0;
                    legend.push(std::mem::replace(
                        &mut row,
                        LegendRow {
                            top: next,
                            items: Vec::new(),
                        },
                    ));
                    x = left;
                }
                row.items.push((index, x));
                x += width + LEGEND_ITEM_GAP;
            }
            top = row.top + line(TITLE_TEXT) + 8.0;
            legend.push(row);
        }

        // x 軸の目盛り・ラベル・見出しの分を下に取る。
        bottom -= TICK_LENGTH + LABEL_GAP + line(TICK_TEXT);
        if chart.x_axis.title.is_some() {
            bottom -= LABEL_GAP + line(TITLE_TEXT);
        }

        // y の目盛りは描画域の高さから本数を決め、ラベルの幅だけ左を空ける。
        let height = (bottom - top).max(0.0);
        let target = ((height / Y_TICK_SPACING) as usize + 1).clamp(2, 12);
        let (y_ticks, y_range) = fit(&chart.y_axis, y_extent(chart), target);
        let label_width = y_ticks
            .values()
            .map(|v| painter.text_width(&chart.y_axis.label(&y_ticks, v), TICK_TEXT))
            .fold(0.0, f64::max);
        left += label_width + LABEL_GAP + 2.0;

        let plot = Rect::new(left, top, (right - left).max(0.0), height);
        let (x_axis, x_range, band) = x_layout(chart, painter, plot.width);
        Drawing {
            chart,
            dark: painter.is_dark(),
            layout: ChartLayout {
                plot,
                x: x_range,
                y: y_range,
            },
            x_axis,
            y_ticks,
            band,
            legend,
        }
    }

    fn draw(self, painter: &mut Painter) -> ChartLayout {
        let plot = self.layout.plot;
        if plot.width <= 0.0 || plot.height <= 0.0 {
            return self.layout;
        }
        self.draw_grid(painter);
        // 系列は描画域で切り抜く。端の点の印が半分欠けないよう、印の大きさだけ広げる。
        painter.save();
        painter.clip_rect(plot.inset(-(MARKER_RADIUS + 1.0)));
        for kind in [SeriesKind::Bar, SeriesKind::Line, SeriesKind::Scatter] {
            for (index, series) in self.chart.series.iter().enumerate() {
                if series.kind != kind {
                    continue;
                }
                let ink = self.chart.ink(index, self.dark);
                match kind {
                    SeriesKind::Bar => self.draw_bars(painter, index, ink),
                    SeriesKind::Line => self.draw_line(painter, series, ink),
                    SeriesKind::Scatter => self.draw_scatter(painter, series, ink),
                }
            }
        }
        painter.restore();
        self.draw_axes(painter);
        self.draw_legend(painter);
        self.layout
    }

    fn draw_grid(&self, painter: &mut Painter) {
        let plot = self.layout.plot;
        painter.save();
        painter.set_opacity(GRID_OPACITY);
        if self.chart.y_axis.grid.unwrap_or(true) {
            for value in self.y_ticks.values() {
                let y = crisp(self.layout.to_point(0.0, value).y);
                painter.line(
                    Point::new(plot.x, y),
                    Point::new(plot.right(), y),
                    TextColor::Secondary,
                    1.0,
                );
            }
        }
        if self.chart.x_axis.grid.unwrap_or(false) {
            for x in self.x_tick_positions() {
                let x = crisp(x);
                painter.line(
                    Point::new(x, plot.y),
                    Point::new(x, plot.bottom()),
                    TextColor::Secondary,
                    1.0,
                );
            }
        }
        painter.restore();
    }

    /// Where the x ticks are on the surface: at the numeric ticks, or in the middle
    /// of each labeled category.
    fn x_tick_positions(&self) -> Vec<f64> {
        match &self.x_axis {
            XAxis::Numeric(ticks) => ticks
                .values()
                .map(|v| self.layout.to_point(v, 0.0).x)
                .collect(),
            XAxis::Categories { labels, every } => (0..labels.len())
                .step_by(*every)
                .map(|i| self.layout.to_point(i as f64, 0.0).x)
                .collect(),
        }
    }

    fn draw_axes(&self, painter: &mut Painter) {
        let plot = self.layout.plot;
        let bounds = painter.bounds();
        let axis = TextColor::Secondary;
        // x 軸の線。0 が下端より上にあれば、0 の線も引く (棒の基線)。
        let bottom = crisp(plot.bottom());
        painter.line(
            Point::new(plot.x, bottom),
            Point::new(plot.right(), bottom),
            axis,
            1.0,
        );
        let (low, high) = self.layout.y;
        if low < 0.0 && high > 0.0 {
            let zero = crisp(self.layout.to_point(0.0, 0.0).y);
            painter.line(
                Point::new(plot.x, zero),
                Point::new(plot.right(), zero),
                axis,
                1.0,
            );
        }

        // x の目盛りとラベル。ラベルは面の外へはみ出さないよう寄せる。
        let label_top = plot.bottom() + TICK_LENGTH + LABEL_GAP;
        let labels: Vec<(f64, String)> = match &self.x_axis {
            XAxis::Numeric(ticks) => ticks
                .values()
                .map(|v| {
                    (
                        self.layout.to_point(v, 0.0).x,
                        self.chart.x_axis.label(ticks, v),
                    )
                })
                .collect(),
            XAxis::Categories { labels, every } => labels
                .iter()
                .enumerate()
                .step_by(*every)
                .map(|(i, label)| (self.layout.to_point(i as f64, 0.0).x, label.clone()))
                .collect(),
        };
        painter.set_text_align(Align::Center);
        for (x, label) in labels {
            let tick = crisp(x);
            painter.line(
                Point::new(tick, bottom),
                Point::new(tick, bottom + TICK_LENGTH),
                axis,
                1.0,
            );
            let half = painter.text_width(&label, TICK_TEXT) / 2.0;
            let center = x.min(bounds.width - half).max(half);
            painter.text(&label, Point::new(center, label_top), TICK_TEXT, axis);
        }

        // y のラベルは描画域の左に右揃えで、目盛りの高さの中ほどへ。
        painter.set_text_align(Align::End);
        for value in self.y_ticks.values() {
            let y = self.layout.to_point(0.0, value).y;
            let label = self.chart.y_axis.label(&self.y_ticks, value);
            painter.text(
                &label,
                Point::new(plot.x - LABEL_GAP - 2.0, y - TICK_TEXT * TEXT_MIDDLE),
                TICK_TEXT,
                axis,
            );
        }

        // 軸の見出し。y は下から上へ読む向きに回す。
        painter.set_text_align(Align::Center);
        if let Some(title) = &self.chart.x_axis.title {
            let top = bounds.height - PADDING - TITLE_TEXT * LINE_HEIGHT;
            painter.text(
                title,
                Point::new(plot.center().x, top),
                TITLE_TEXT,
                TextColor::Default,
            );
        }
        if let Some(title) = &self.chart.y_axis.title {
            // 描画域が低くても収まりやすいよう、上端から描画域の下端までの中ほどに。
            painter.save();
            painter.translate(PADDING, (PADDING + plot.bottom()) / 2.0);
            painter.rotate(-std::f64::consts::FRAC_PI_2);
            painter.text(title, Point::new(0.0, 0.0), TITLE_TEXT, TextColor::Default);
            painter.restore();
        }
    }

    fn draw_legend(&self, painter: &mut Painter) {
        painter.set_text_align(Align::Start);
        for row in &self.legend {
            let middle = row.top + TITLE_TEXT * TEXT_MIDDLE;
            for &(index, x) in &row.items {
                let series = &self.chart.series[index];
                let ink = self.chart.ink(index, self.dark);
                match series.kind {
                    SeriesKind::Line => painter.line(
                        Point::new(x, middle),
                        Point::new(x + SWATCH, middle),
                        ink,
                        LINE_WIDTH,
                    ),
                    SeriesKind::Bar => painter.fill_path(
                        &rounded_bar(
                            Path::new(),
                            x + 2.0,
                            middle + 5.0,
                            x + SWATCH - 2.0,
                            middle - 5.0,
                            2.0,
                        ),
                        ink,
                    ),
                    SeriesKind::Scatter => painter.fill_circle(
                        Point::new(x + SWATCH / 2.0, middle),
                        MARKER_RADIUS,
                        ink,
                    ),
                }
                painter.text(
                    &series.name,
                    Point::new(x + SWATCH + SWATCH_GAP, row.top),
                    TITLE_TEXT,
                    TextColor::Default,
                );
            }
        }
    }

    fn draw_bars(&self, painter: &mut Painter, index: usize, ink: Ink) {
        let bars: Vec<usize> = (0..self.chart.series.len())
            .filter(|&i| self.chart.series[i].kind == SeriesKind::Bar)
            .collect();
        let Some(slot) = bars.iter().position(|&i| i == index) else {
            return;
        };
        let plot = self.layout.plot;
        let band = (self.band / (self.layout.x.1 - self.layout.x.0) * plot.width).abs();
        let group = band * BAR_GROUP;
        let count = bars.len() as f64;
        let gap = if group / count > BAR_GAP * 3.0 {
            BAR_GAP
        } else {
            0.0
        };
        let width = ((group - gap * (count - 1.0)) / count).max(0.5);
        let offset = -group / 2.0 + slot as f64 * (width + gap);
        // 基線は 0。0 が範囲の外なら近い方の端。
        let (low, high) = self.layout.y;
        let baseline = self.layout.to_point(0.0, 0.0f64.clamp(low, high)).y;

        // 1 本ずつ塗ると多いときに重いので、系列で 1 つの形にまとめて 1 回で塗る。
        let mut path = Path::new();
        for (x, y) in self.chart.series[index].finite_points() {
            let left = self.layout.to_point(x, 0.0).x + offset;
            if left + width < plot.x - 1.0 || left > plot.right() + 1.0 {
                continue;
            }
            let end = self.layout.to_point(0.0, y).y;
            let radius = BAR_RADIUS.min(width / 2.0).min((end - baseline).abs());
            path = rounded_bar(path, left, baseline, left + width, end, radius);
        }
        painter.fill_path(&path, ink);
    }

    fn draw_line(&self, painter: &mut Painter, series: &Series, ink: Ink) {
        let points: Vec<Point> = series
            .points
            .iter()
            .map(|&(x, y)| {
                if x.is_finite() && y.is_finite() {
                    self.layout.to_point(x, y)
                } else {
                    Point::new(f64::NAN, f64::NAN)
                }
            })
            .collect();
        let mut path = Path::new();
        let mut dots = Path::new();
        for run in thin::runs(&points) {
            if let [only] = run {
                // 前後が欠けた 1 点は、線にならないので点で描く。
                dots = dots.circle(*only, LINE_WIDTH * 1.25);
                continue;
            }
            path = path.polyline(&thin::thin_line(run));
        }
        painter.stroke_path(&path, ink, LINE_WIDTH);
        painter.fill_path(&dots, ink);
    }

    fn draw_scatter(&self, painter: &mut Painter, series: &Series, ink: Ink) {
        let area = self.layout.plot.inset(-MARKER_RADIUS);
        let points = series
            .finite_points()
            .map(|(x, y)| self.layout.to_point(x, y));
        let markers = thin::thin_markers(
            points,
            area,
            MARKER_RADIUS,
            EXACT_MARKERS,
            series.points.len(),
        );
        // 円は同じ向きに回るので、重なっても nonzero の規則で抜けない。
        let path = markers
            .iter()
            .fold(Path::new(), |path, &p| path.circle(p, MARKER_RADIUS));
        painter.fill_path(&path, ink);
    }
}

/// Puts a 1-pixel line on the middle of a pixel, so that it is not blurred across
/// two at a scale of 1.
fn crisp(value: f64) -> f64 {
    value.floor() + 0.5
}

/// Adds to `path` a bar from `base` to `end` between `left` and `right`, rounded at
/// the `end`.
fn rounded_bar(path: Path, left: f64, base: f64, right: f64, end: f64, radius: f64) -> Path {
    let radius = radius.max(0.0);
    // 上へ伸びる棒は上の角を、下へ伸びる棒は下の角を丸める。
    let toward = if end < base { 1.0 } else { -1.0 };
    let corner = end + toward * radius;
    path.move_to(Point::new(left, base))
        .line_to(Point::new(left, corner))
        .quad_to(Point::new(left, end), Point::new(left + radius, end))
        .line_to(Point::new(right - radius, end))
        .quad_to(Point::new(right, end), Point::new(right, corner))
        .line_to(Point::new(right, base))
        .close()
}

/// The lowest and highest `y` of the chart's data, with 0 when there are bars.
fn y_extent(chart: &Chart) -> (f64, f64) {
    let mut extent = (f64::INFINITY, f64::NEG_INFINITY);
    if chart.has_bars() {
        extent = (0.0, 0.0);
    }
    for series in &chart.series {
        for (_, y) in series.finite_points() {
            extent = (extent.0.min(y), extent.1.max(y));
        }
    }
    extent
}

/// The ticks of an axis and the range it shows: the range set on it, or one fitted
/// to `extent` and widened to whole ticks.
fn fit(axis: &Axis, extent: (f64, f64), target: usize) -> (Ticks, (f64, f64)) {
    match axis.range {
        Some((min, max)) => (Ticks::within(min, max, target), (min, max)),
        None => {
            let ticks = Ticks::nice(extent.0, extent.1, target);
            let range = ticks.range().unwrap_or((0.0, 1.0));
            (ticks, range)
        }
    }
}

/// The x axis, the range it shows and the width of a bar band, for a plot `width`
/// wide.
fn x_layout(chart: &Chart, painter: &Painter, width: f64) -> (XAxis, (f64, f64), f64) {
    if let Some(categories) = &chart.categories {
        // 系列の値がカテゴリより多ければ、その分も帯を作る。
        let last = chart
            .series
            .iter()
            .flat_map(|s| s.finite_points())
            .map(|(x, _)| x)
            .fold(-1.0, f64::max);
        let count = (categories.len() as f64).max((last + 1.0).ceil()).max(1.0);
        let count = count.min(1e7);
        let band = width / count;
        let widest = categories
            .iter()
            .map(|label| painter.text_width(label, TICK_TEXT))
            .fold(0.0, f64::max);
        let every = if band > 0.0 {
            ((widest + 8.0) / band).ceil().max(1.0) as usize
        } else {
            1
        };
        let axis = XAxis::Categories {
            labels: categories.clone(),
            every,
        };
        return (axis, (-0.5, count - 0.5), 1.0);
    }

    let mut extent = (f64::INFINITY, f64::NEG_INFINITY);
    for series in &chart.series {
        for (x, _) in series.finite_points() {
            extent = (extent.0.min(x), extent.1.max(x));
        }
    }
    // 棒があれば、隣り合う x のいちばん狭い間を帯の幅にし、両端に半分ずつ空ける。
    let band = bar_band(chart, extent);
    let padded = band.is_some() && chart.x_axis.range.is_none();
    if let Some(band) = band {
        extent = (extent.0 - band / 2.0, extent.1 + band / 2.0);
    }

    // ラベルが重ならない本数まで減らす。
    let mut target = ((width / 64.0) as usize + 1).clamp(2, 12);
    loop {
        let (ticks, range) = if padded {
            (Ticks::within(extent.0, extent.1, target), extent)
        } else {
            fit(&chart.x_axis, extent, target)
        };
        let widest = ticks
            .values()
            .map(|v| painter.text_width(&chart.x_axis.label(&ticks, v), TICK_TEXT))
            .fold(0.0, f64::max);
        let fits = (widest + 12.0) * ticks.len() as f64 <= width;
        if fits || target == 2 {
            return (XAxis::Numeric(ticks), range, band.unwrap_or(1.0));
        }
        target -= 1;
    }
}

/// The narrowest distance between two different `x` of the bar series.
fn bar_band(chart: &Chart, extent: (f64, f64)) -> Option<f64> {
    let mut xs: Vec<f64> = chart
        .series
        .iter()
        .filter(|s| s.kind == SeriesKind::Bar)
        .flat_map(|s| s.finite_points())
        .map(|(x, _)| x)
        .collect();
    if xs.is_empty() {
        return None;
    }
    xs.sort_by(f64::total_cmp);
    let narrowest = xs
        .windows(2)
        .map(|pair| pair[1] - pair[0])
        .filter(|&gap| gap > 0.0)
        .fold(f64::INFINITY, f64::min);
    if narrowest.is_finite() {
        Some(narrowest)
    } else {
        // 棒が 1 本 (または同じ x ばかり) なら、範囲の 1 割か 1。
        let span = extent.1 - extent.0;
        Some(if span > 0.0 { span / 10.0 } else { 1.0 })
    }
}
