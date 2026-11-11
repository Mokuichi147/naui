//! Value types for the drawing surface (`Canvas`) and the record of what to draw.
//!
//! Things **the app draws itself**, such as charts and shapes, cannot be built by
//! combining standard controls. naui therefore provides only the drawing surface,
//! and the app writes what to draw into a [`Painter`]. A `Painter` **only records**
//! drawing commands; turning them into pixels is the job of each platform's 2D API
//! (Core Graphics / cairo / Direct2D / the 2D context of `<canvas>`). There
//! is no rasterization here, and text is measured by the platform through a
//! function the backend hands the `Painter`.
//!
//! Coordinates are **logical pixels** with the origin at the top left and positive
//! values going right and down. Each platform applies the scale factor for
//! high-resolution displays.
//!
//! The vocabulary is kept to the minimum that all four platforms share. Every line
//! and fill becomes a [`Path`], and a `Path` contains only straight lines and cubic
//! Béziers. Circles and arcs are converted to Béziers here before being handed
//! over, so they come out the same shape on every platform. Transforms are combined
//! here too ([`Transform`]), and invalid values (infinities, NaN, transforms that
//! squash everything flat, empty clips) are left out before reaching a platform,
//! since each handles them differently.

use std::fmt;
use std::rc::Rc;

use crate::{Align, Bitmap, Color, EventResponse};

/// A coordinate (logical pixels). The origin is the top left; right and down are positive.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}

impl From<(f64, f64)> for Point {
    fn from((x, y): (f64, f64)) -> Self {
        Point::new(x, y)
    }
}

/// A rectangle (logical pixels). `x` / `y` is the top-left corner.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    pub const fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// The top-left corner.
    pub fn origin(&self) -> Point {
        Point::new(self.x, self.y)
    }

    /// The center.
    pub fn center(&self) -> Point {
        Point::new(self.x + self.width / 2.0, self.y + self.height / 2.0)
    }

    /// The x coordinate of the right edge.
    pub fn right(&self) -> f64 {
        self.x + self.width
    }

    /// The y coordinate of the bottom edge.
    pub fn bottom(&self) -> f64 {
        self.y + self.height
    }

    /// Whether the point lies inside (points on the edges count).
    ///
    /// Use it to check whether the pointer is over a shape.
    ///
    /// ```
    /// # use naui_core::{Point, Rect};
    /// let rect = Rect::new(10.0, 10.0, 20.0, 20.0);
    /// assert!(rect.contains(Point::new(15.0, 30.0)));
    /// assert!(!rect.contains(Point::new(31.0, 15.0)));
    /// ```
    pub fn contains(&self, point: Point) -> bool {
        point.x >= self.x
            && point.x <= self.right()
            && point.y >= self.y
            && point.y <= self.bottom()
    }

    /// The rectangle with all four sides moved inward by `amount`. A negative amount
    /// grows it outward.
    ///
    /// The width and height stop at 0 instead of going negative.
    pub fn inset(&self, amount: f64) -> Rect {
        Rect::new(
            self.x + amount,
            self.y + amount,
            (self.width - amount * 2.0).max(0.0),
            (self.height - amount * 2.0).max(0.0),
        )
    }
}

/// A 2D affine transform: moving, rotating, scaling and skewing.
///
/// It maps `(x, y)` to `(a·x + c·y + e, b·x + d·y + f)`, the same six numbers as
/// `<canvas>`'s `transform(a, b, c, d, e, f)`, `CGAffineTransform` and cairo's
/// matrix. Angles are in radians, and positive turns clockwise (because y points
/// down).
///
/// ```
/// # use naui_core::{Point, Transform};
/// use std::f64::consts::FRAC_PI_2;
/// // Turn a quarter clockwise, then move 10 to the right.
/// let t = Transform::rotation(FRAC_PI_2).then(Transform::translation(10.0, 0.0));
/// let p = t.apply(Point::new(1.0, 0.0));
/// assert!((p.x - 10.0).abs() < 1e-9 && (p.y - 1.0).abs() < 1e-9);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transform {
    pub a: f64,
    pub b: f64,
    pub c: f64,
    pub d: f64,
    pub e: f64,
    pub f: f64,
}

impl Default for Transform {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Transform {
    /// Leaves every point where it is.
    pub const IDENTITY: Transform = Transform::new(1.0, 0.0, 0.0, 1.0, 0.0, 0.0);

    pub const fn new(a: f64, b: f64, c: f64, d: f64, e: f64, f: f64) -> Self {
        Self { a, b, c, d, e, f }
    }

    /// Moves by `dx` to the right and `dy` down.
    pub const fn translation(dx: f64, dy: f64) -> Self {
        Self::new(1.0, 0.0, 0.0, 1.0, dx, dy)
    }

    /// Scales by `sx` horizontally and `sy` vertically, around the origin.
    pub const fn scaling(sx: f64, sy: f64) -> Self {
        Self::new(sx, 0.0, 0.0, sy, 0.0, 0.0)
    }

    /// Turns by `angle` radians around the origin (clockwise for positive angles).
    pub fn rotation(angle: f64) -> Self {
        let (sin, cos) = angle.sin_cos();
        Self::new(cos, sin, -sin, cos, 0.0, 0.0)
    }

    /// The transform that applies `self` first and then `next`.
    pub fn then(self, next: Transform) -> Transform {
        Transform::new(
            self.a * next.a + self.b * next.c,
            self.a * next.b + self.b * next.d,
            self.c * next.a + self.d * next.c,
            self.c * next.b + self.d * next.d,
            self.e * next.a + self.f * next.c + next.e,
            self.e * next.b + self.f * next.d + next.f,
        )
    }

    /// Where `point` goes.
    pub fn apply(&self, point: Point) -> Point {
        Point::new(
            self.a * point.x + self.c * point.y + self.e,
            self.b * point.x + self.d * point.y + self.f,
        )
    }

    /// The transform that undoes this one. `None` when it squashes the plane onto a
    /// line or a point (a scale of 0, for example), or holds infinities or NaN.
    ///
    /// Use it to turn a pointer position on the surface back into the coordinates a
    /// shape was drawn in.
    pub fn invert(&self) -> Option<Transform> {
        let det = self.determinant();
        if det == 0.0 || !det.is_finite() || !self.is_finite() {
            return None;
        }
        let inverse = Transform::new(
            self.d / det,
            -self.b / det,
            -self.c / det,
            self.a / det,
            (self.c * self.f - self.d * self.e) / det,
            (self.b * self.e - self.a * self.f) / det,
        );
        inverse.is_finite().then_some(inverse)
    }

    /// `a·d − b·c`: how much areas are scaled (negative when mirrored).
    pub fn determinant(&self) -> f64 {
        self.a * self.d - self.b * self.c
    }

    /// Whether all six numbers are finite.
    pub fn is_finite(&self) -> bool {
        [self.a, self.b, self.c, self.d, self.e, self.f]
            .iter()
            .all(|v| v.is_finite())
    }

    /// Whether it keeps lines along the axes along the axes (only moves and scales,
    /// no rotation or skew).
    pub fn is_axis_aligned(&self) -> bool {
        self.b == 0.0 && self.c == 0.0
    }
}

/// One step in building a [`Path`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PathSegment {
    /// Lifts the pen and moves it to `to` (starts a new subpath).
    MoveTo(Point),
    /// Draws a straight line from the current point to `to`.
    LineTo(Point),
    /// Draws a cubic Bézier curve from the current point to `to`.
    CubicTo {
        control1: Point,
        control2: Point,
        to: Point,
    },
    /// Closes the current subpath with a straight line back to its start.
    Close,
}

/// A shape made of straight lines and cubic Béziers.
///
/// It is expressed using only the `moveTo` / `lineTo` / `curveTo` / `close` that
/// the 2D APIs of all four platforms share. There are methods that add quadratic
/// Béziers, circles and arcs, but they are **converted to cubic Béziers here**
/// before being stored, so the shape does not vary between platforms.
///
/// ```
/// # use naui_core::{Path, Point};
/// let triangle = Path::new()
///     .move_to(Point::new(0.0, 0.0))
///     .line_to(Point::new(10.0, 0.0))
///     .line_to(Point::new(5.0, 8.0))
///     .close();
/// assert_eq!(triangle.segments().len(), 4);
/// ```
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Path {
    segments: Vec<PathSegment>,
}

impl Path {
    pub fn new() -> Self {
        Self::default()
    }

    /// The recorded steps. Backends replay them as they are.
    pub fn segments(&self) -> &[PathSegment] {
        &self.segments
    }

    pub fn is_empty(&self) -> bool {
        self.segments.is_empty()
    }

    /// Moves the pen to `to` and starts a new subpath.
    pub fn move_to(mut self, to: Point) -> Self {
        self.segments.push(PathSegment::MoveTo(to));
        self
    }

    /// Draws a straight line from the current point to `to`.
    ///
    /// If there are no steps yet, this is treated as `move_to` (there is nowhere for
    /// the pen to be).
    pub fn line_to(mut self, to: Point) -> Self {
        if self.segments.is_empty() {
            return self.move_to(to);
        }
        self.segments.push(PathSegment::LineTo(to));
        self
    }

    /// Draws a cubic Bézier curve from the current point to `to`.
    pub fn cubic_to(mut self, control1: Point, control2: Point, to: Point) -> Self {
        if self.segments.is_empty() {
            return self.move_to(to);
        }
        self.segments.push(PathSegment::CubicTo {
            control1,
            control2,
            to,
        });
        self
    }

    /// Draws a quadratic Bézier curve from the current point to `to`.
    ///
    /// It is stored as a cubic Bézier (with two control points placed at the 2/3
    /// positions). The shape of the curve does not change.
    pub fn quad_to(self, control: Point, to: Point) -> Self {
        let Some(from) = self.current_point() else {
            return self.move_to(to);
        };
        let control1 = Point::new(
            from.x + 2.0 / 3.0 * (control.x - from.x),
            from.y + 2.0 / 3.0 * (control.y - from.y),
        );
        let control2 = Point::new(
            to.x + 2.0 / 3.0 * (control.x - to.x),
            to.y + 2.0 / 3.0 * (control.y - to.y),
        );
        self.cubic_to(control1, control2, to)
    }

    /// Closes the current subpath.
    pub fn close(mut self) -> Self {
        if !self.segments.is_empty() {
            self.segments.push(PathSegment::Close);
        }
        self
    }

    /// Adds a rectangle as one closed subpath.
    pub fn rect(self, rect: Rect) -> Self {
        self.move_to(rect.origin())
            .line_to(Point::new(rect.right(), rect.y))
            .line_to(Point::new(rect.right(), rect.bottom()))
            .line_to(Point::new(rect.x, rect.bottom()))
            .close()
    }

    /// Adds a circle as one closed subpath.
    pub fn circle(self, center: Point, radius: f64) -> Self {
        self.ellipse(center, radius, radius)
    }

    /// Adds an ellipse as one closed subpath.
    ///
    /// It is approximated with four cubic Béziers (error at most 0.03% of the radius).
    pub fn ellipse(self, center: Point, radius_x: f64, radius_y: f64) -> Self {
        self.move_to(Point::new(center.x + radius_x, center.y))
            .arc_segments(center, radius_x, radius_y, 0.0, std::f64::consts::TAU)
            .close()
    }

    /// Adds a circular arc. Angles are in radians; 0 points right and positive is
    /// clockwise (because y points down).
    ///
    /// If the pen is not at the start of the arc, a straight line is drawn there first
    /// (if there are no steps yet, the pen moves to the start). A pie slice looks like
    /// `move_to(center).arc(...).close()`.
    ///
    /// Anything beyond one full turn in `end - start` is capped at one turn.
    pub fn arc(self, center: Point, radius: f64, start: f64, end: f64) -> Self {
        let first = Point::new(
            center.x + radius * start.cos(),
            center.y + radius * start.sin(),
        );
        let this = if self.segments.is_empty() {
            self.move_to(first)
        } else {
            self.line_to(first)
        };
        this.arc_segments(center, radius, radius, start, end)
    }

    /// Adds the arc as Béziers. Assumes the pen is already at the start.
    fn arc_segments(
        mut self,
        center: Point,
        radius_x: f64,
        radius_y: f64,
        start: f64,
        end: f64,
    ) -> Self {
        let sweep = (end - start).clamp(-std::f64::consts::TAU, std::f64::consts::TAU);
        if sweep == 0.0 || !sweep.is_finite() {
            return self;
        }
        // 1 本のベジェで 90° まで。それより大きい弧は分ける。
        let pieces = (sweep.abs() / std::f64::consts::FRAC_PI_2).ceil().max(1.0) as usize;
        let step = sweep / pieces as f64;
        // 弧を 3 次ベジェで近似するときの制御点の張り出し。
        let kappa = 4.0 / 3.0 * (step / 4.0).tan();
        let point_at = |angle: f64| {
            Point::new(
                center.x + radius_x * angle.cos(),
                center.y + radius_y * angle.sin(),
            )
        };
        let mut from_angle = start;
        for _ in 0..pieces {
            let to_angle = from_angle + step;
            let (sin_from, cos_from) = from_angle.sin_cos();
            let (sin_to, cos_to) = to_angle.sin_cos();
            let control1 = Point::new(
                center.x + radius_x * (cos_from - kappa * sin_from),
                center.y + radius_y * (sin_from + kappa * cos_from),
            );
            let control2 = Point::new(
                center.x + radius_x * (cos_to + kappa * sin_to),
                center.y + radius_y * (sin_to - kappa * cos_to),
            );
            self.segments.push(PathSegment::CubicTo {
                control1,
                control2,
                to: point_at(to_angle),
            });
            from_angle = to_angle;
        }
        self
    }

    /// Adds a polyline. Moves to the first point and draws lines to the rest.
    pub fn polyline(mut self, points: &[Point]) -> Self {
        let mut points = points.iter().copied();
        let Some(first) = points.next() else {
            return self;
        };
        self = self.move_to(first);
        for point in points {
            self = self.line_to(point);
        }
        self
    }

    /// Adds a polygon. Draws the polyline and closes it.
    pub fn polygon(self, points: &[Point]) -> Self {
        if points.is_empty() {
            return self;
        }
        self.polyline(points).close()
    }

    /// The point where the pen currently is.
    ///
    /// Right after `Close`, this is the start of the subpath that was closed.
    pub fn current_point(&self) -> Option<Point> {
        let mut start = None;
        let mut current = None;
        for segment in &self.segments {
            match *segment {
                PathSegment::MoveTo(p) => {
                    start = Some(p);
                    current = Some(p);
                }
                PathSegment::LineTo(p) | PathSegment::CubicTo { to: p, .. } => current = Some(p),
                PathSegment::Close => current = start,
            }
        }
        current
    }

    /// The smallest rectangle containing every point, control points included.
    ///
    /// `None` when there are no steps.
    pub fn bounds(&self) -> Option<Rect> {
        let mut min = Point::new(f64::INFINITY, f64::INFINITY);
        let mut max = Point::new(f64::NEG_INFINITY, f64::NEG_INFINITY);
        let mut extend = |p: Point| {
            min.x = min.x.min(p.x);
            min.y = min.y.min(p.y);
            max.x = max.x.max(p.x);
            max.y = max.y.max(p.y);
        };
        for segment in &self.segments {
            match *segment {
                PathSegment::MoveTo(p) | PathSegment::LineTo(p) => extend(p),
                PathSegment::CubicTo {
                    control1,
                    control2,
                    to,
                } => {
                    extend(control1);
                    extend(control2);
                    extend(to);
                }
                PathSegment::Close => {}
            }
        }
        if min.x.is_finite() {
            Some(Rect::new(min.x, min.y, max.x - min.x, max.y - min.y))
        } else {
            None
        }
    }
}

/// One color of a [`Gradient`], at `offset` along it (0.0 at the start, 1.0 at the
/// end).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GradientStop {
    pub offset: f64,
    pub color: Color,
    /// From 0.0 (transparent) to 1.0 (opaque).
    pub opacity: f64,
}

/// The shape a [`Gradient`] spreads along.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GradientShape {
    /// Along the line from `start` (offset 0) to `end` (offset 1). Lines across it
    /// have one color.
    Linear { start: Point, end: Point },
    /// Outward from `center` (offset 0) to the circle of `radius` (offset 1).
    Radial { center: Point, radius: f64 },
}

/// Colors blending into each other, to fill shapes with
/// ([`Painter::fill_path`] and the other fills take one in place of a [`Color`]).
///
/// The positions are in the same coordinates as the shape (they move, turn and
/// scale with it). Beyond the ends, the first and last colors continue. Each stop has
/// its own opacity, so a color can fade out; the painter's
/// [`opacity`](Painter::set_opacity) applies on top.
///
/// ```
/// # use naui_core::{Color, Gradient, Painter, Point, Rect};
/// let fade = Gradient::linear(Point::new(0.0, 0.0), Point::new(0.0, 100.0))
///     .stop(0.0, Color::rgb(0x33, 0x66, 0xff))
///     .stop_with_opacity(1.0, Color::rgb(0x33, 0x66, 0xff), 0.0);
/// let mut painter = Painter::new(200.0, 100.0);
/// painter.fill_rect(Rect::new(0.0, 0.0, 200.0, 100.0), &fade);
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct Gradient {
    shape: GradientShape,
    stops: Vec<GradientStop>,
}

impl Gradient {
    /// A gradient along the line from `start` to `end`.
    pub fn linear(start: Point, end: Point) -> Self {
        Self {
            shape: GradientShape::Linear { start, end },
            stops: Vec::new(),
        }
    }

    /// A gradient outward from `center` to the circle of `radius`.
    pub fn radial(center: Point, radius: f64) -> Self {
        Self {
            shape: GradientShape::Radial { center, radius },
            stops: Vec::new(),
        }
    }

    /// Adds an opaque color at `offset` (0.0 to 1.0, clamped).
    pub fn stop(self, offset: f64, color: Color) -> Self {
        self.stop_with_opacity(offset, color, 1.0)
    }

    /// Adds a color with an opacity (0.0 transparent to 1.0 opaque) at `offset` (0.0
    /// to 1.0, clamped).
    ///
    /// Stops can be added in any order; they are kept sorted by offset, and stops at
    /// the same offset stay in the order they were added (a hard edge between two
    /// colors). A NaN offset is ignored.
    pub fn stop_with_opacity(mut self, offset: f64, color: Color, opacity: f64) -> Self {
        if offset.is_nan() {
            return self;
        }
        let offset = offset.clamp(0.0, 1.0);
        let opacity = if opacity.is_finite() {
            opacity.clamp(0.0, 1.0)
        } else {
            1.0
        };
        let at = self.stops.partition_point(|stop| stop.offset <= offset);
        self.stops.insert(
            at,
            GradientStop {
                offset,
                color,
                opacity,
            },
        );
        self
    }

    pub fn shape(&self) -> GradientShape {
        self.shape
    }

    /// The colors, sorted by offset.
    pub fn stops(&self) -> &[GradientStop] {
        &self.stops
    }

    /// Whether the gradient covers some area. A line of length 0, a radius of 0 or
    /// less, and positions with infinities or NaN cover none (`<canvas>` paints
    /// nothing for these, and the other platforms differ).
    fn has_extent(&self) -> bool {
        let finite = |p: Point| p.x.is_finite() && p.y.is_finite();
        match self.shape {
            GradientShape::Linear { start, end } => finite(start) && finite(end) && start != end,
            GradientShape::Radial { center, radius } => {
                finite(center) && radius.is_finite() && radius > 0.0
            }
        }
    }
}

/// What fills a shape: one color, or a [`Gradient`].
///
/// The fills of [`Painter`] take anything that turns into one, so a [`Color`], a
/// `Gradient` or a `&Gradient` can be passed as is.
#[derive(Debug, Clone, PartialEq)]
pub enum Paint {
    Color(Color),
    Gradient(Gradient),
}

impl From<Color> for Paint {
    fn from(color: Color) -> Self {
        Paint::Color(color)
    }
}

impl From<Gradient> for Paint {
    fn from(gradient: Gradient) -> Self {
        Paint::Gradient(gradient)
    }
}

impl From<&Gradient> for Paint {
    fn from(gradient: &Gradient) -> Self {
        Paint::Gradient(gradient.clone())
    }
}

/// A drawing command recorded by a [`Painter`]. Backends replay them in order.
///
/// `opacity` runs from 0.0 (transparent) to 1.0 (opaque). Colors themselves are
/// opaque [`Color`]s; transparency is carried separately by each command.
///
/// Coordinates are in the space set up by the [`Transform`] commands before them,
/// and drawing is limited to the area of the [`Clip`](DrawCommand::Clip) commands
/// before them. Both last until the [`Restore`](DrawCommand::Restore) matching the
/// [`Save`](DrawCommand::Save) before them, or to the end. A `Painter` records every
/// `Restore` after a matching `Save`, and
/// [`into_commands`](Painter::into_commands) closes the ones left open.
#[derive(Debug, Clone, PartialEq)]
pub enum DrawCommand {
    /// Fills the inside of the shape. The fill rule is nonzero (the default on all four
    /// platforms).
    ///
    /// A gradient here has at least two stops and covers some area (the `Painter`
    /// turns one stop into a plain color and leaves out the rest).
    Fill {
        path: Path,
        paint: Paint,
        opacity: f64,
    },
    /// Strokes the outline of the shape. A solid line when `dash` is empty.
    Stroke {
        path: Path,
        color: Color,
        width: f64,
        dash: Vec<f64>,
        opacity: f64,
    },
    /// Draws one line of text. `at` is **the top of the text**, and the horizontal
    /// position depends on `align` (`at.x` is the left edge for `Start`, the center for
    /// `Center` and the right edge for `End`).
    Text {
        text: String,
        at: Point,
        size: f64,
        color: Color,
        align: Align,
        opacity: f64,
    },
    /// Draws the `source` part of the image (in its pixels, origin at the top left)
    /// stretched over `dest`. `source` lies within the image and neither rectangle is
    /// empty.
    Image {
        bitmap: Bitmap,
        source: Rect,
        dest: Rect,
        opacity: f64,
    },
    /// Remembers the current transform and clip, for the matching `Restore`.
    Save,
    /// Goes back to the transform and clip at the matching `Save`.
    Restore,
    /// Applies the transform to the coordinates of later commands, before the
    /// transform already in place (as `<canvas>`'s `transform` does). Its determinant
    /// is finite and not 0.
    Transform(Transform),
    /// Limits later drawing to the inside of the shape (nonzero rule), within the
    /// clip already in place. The path is not empty.
    Clip(Path),
}

/// The record of what to draw. Write to it inside `Canvas::on_draw`.
///
/// It only stores drawing commands and never touches pixels. Line widths, text
/// sizes and coordinates are all in logical pixels. Opacity, dashes, text
/// alignment, the transform and the clip are kept as **state** that applies to later
/// commands until changed, and [`save`](Self::save) / [`restore`](Self::restore)
/// put them back (the same model as `globalAlpha`, `save()` and friends in
/// `<canvas>`).
///
/// ```
/// # use naui_core::{Color, Painter, Point, Rect};
/// let mut painter = Painter::new(200.0, 100.0);
/// painter.fill_rect(Rect::new(10.0, 10.0, 50.0, 30.0), Color::rgb(0x33, 0x66, 0xff));
/// painter.line(Point::new(0.0, 0.0), Point::new(200.0, 100.0), Color::BLACK, 1.0);
/// assert_eq!(painter.commands().len(), 2);
/// ```
#[derive(Clone)]
pub struct Painter {
    width: f64,
    height: f64,
    state: State,
    /// The states put aside by `save`, innermost last.
    saved: Vec<State>,
    commands: Vec<DrawCommand>,
    /// Measures text as the platform draws it. `None` outside a `Canvas`.
    measure: Option<Rc<TextMeasure>>,
}

/// Measures the width of one line of text at a size, both in logical pixels.
type TextMeasure = dyn Fn(&str, f64) -> f64;

/// What `save` puts aside and `restore` brings back.
#[derive(Debug, Clone)]
struct State {
    opacity: f64,
    dash: Vec<f64>,
    text_align: Align,
    /// From the coordinates the app draws in to the surface's.
    transform: Transform,
    /// Whether nothing can show: the transform squashes everything flat, or the clip
    /// is empty. Commands are left out until `restore` brings back a state without it.
    hidden: bool,
}

impl fmt::Debug for Painter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Painter")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("state", &self.state)
            .field("saved", &self.saved.len())
            .field("commands", &self.commands)
            .field("measures_text", &self.measure.is_some())
            .finish()
    }
}

impl Painter {
    /// Starts recording for a `width` × `height` surface.
    ///
    /// Text widths ([`text_width`](Self::text_width)) are only estimated, since there
    /// is no platform to measure them. A `Canvas` gives `on_draw` a painter made with
    /// [`with_text_measure`](Self::with_text_measure) instead.
    pub fn new(width: f64, height: f64) -> Self {
        Self {
            width,
            height,
            state: State {
                opacity: 1.0,
                dash: Vec::new(),
                text_align: Align::Start,
                transform: Transform::IDENTITY,
                hidden: false,
            },
            saved: Vec::new(),
            commands: Vec::new(),
            measure: None,
        }
    }

    /// Starts recording for a `width` × `height` surface whose text widths are
    /// measured by `measure`.
    ///
    /// `measure` receives one line of text and its size (logical pixels) and returns
    /// its width (logical pixels). Backends pass the platform's own measurement, the
    /// one they align text with; tests can pass their own.
    pub fn with_text_measure(
        width: f64,
        height: f64,
        measure: impl Fn(&str, f64) -> f64 + 'static,
    ) -> Self {
        Self {
            measure: Some(Rc::new(measure)),
            ..Self::new(width, height)
        }
    }

    /// The width of the drawable surface (logical pixels).
    pub fn width(&self) -> f64 {
        self.width
    }

    /// The height of the drawable surface (logical pixels).
    pub fn height(&self) -> f64 {
        self.height
    }

    /// The rectangle covering the whole drawable surface.
    ///
    /// It is in the surface's coordinates, before any [`transform`](Self::transform).
    pub fn bounds(&self) -> Rect {
        Rect::new(0.0, 0.0, self.width, self.height)
    }

    /// The recorded commands. Backends replay them.
    ///
    /// [`save`](Self::save)s not yet restored are still open here (see
    /// [`into_commands`](Self::into_commands)).
    pub fn commands(&self) -> &[DrawCommand] {
        &self.commands
    }

    /// Takes out the recorded commands, closing every [`save`](Self::save) not yet
    /// restored, so each `Save` has its `Restore`.
    pub fn into_commands(mut self) -> Vec<DrawCommand> {
        for _ in 0..self.saved.len() {
            self.commands.push(DrawCommand::Restore);
        }
        self.commands
    }

    /// Opacity for subsequent commands. 0.0 is transparent and 1.0 is opaque. Values
    /// out of range are clamped.
    pub fn set_opacity(&mut self, opacity: f64) {
        self.state.opacity = if opacity.is_finite() {
            opacity.clamp(0.0, 1.0)
        } else {
            1.0
        };
    }

    pub fn opacity(&self) -> f64 {
        self.state.opacity
    }

    /// Makes subsequent lines dashed. `pattern` repeats the lengths of dashes and gaps
    /// (`[4.0, 2.0]` draws 4 and skips 2). An empty pattern goes back to solid lines.
    ///
    /// A pattern containing negative or infinite values is ignored (lines stay solid).
    pub fn set_dash(&mut self, pattern: &[f64]) {
        if pattern.iter().all(|v| v.is_finite() && *v >= 0.0) && pattern.iter().any(|v| *v > 0.0) {
            self.state.dash = pattern.to_vec();
        } else {
            self.state.dash.clear();
        }
    }

    pub fn dash(&self) -> &[f64] {
        &self.state.dash
    }

    /// Horizontal alignment for subsequent text. `Fill` is treated as `Start`.
    pub fn set_text_align(&mut self, align: Align) {
        self.state.text_align = match align {
            Align::Fill => Align::Start,
            other => other,
        };
    }

    pub fn text_align(&self) -> Align {
        self.state.text_align
    }

    /// Puts the current state aside: opacity, dashes, text alignment, the transform and
    /// the clip. [`restore`](Self::restore) brings it back.
    ///
    /// Saves nest. Ones still open when `on_draw` returns are closed then.
    ///
    /// ```
    /// # use naui_core::{Color, Painter, Point, Rect};
    /// # let mut painter = Painter::new(200.0, 100.0);
    /// painter.save();
    /// painter.translate(100.0, 50.0);
    /// painter.rotate(0.5);
    /// painter.fill_rect(Rect::new(-20.0, -10.0, 40.0, 20.0), Color::BLACK); // turned
    /// painter.restore();
    /// painter.fill_rect(Rect::new(0.0, 0.0, 10.0, 10.0), Color::BLACK); // not turned
    /// ```
    pub fn save(&mut self) {
        self.saved.push(self.state.clone());
        self.commands.push(DrawCommand::Save);
    }

    /// Brings back the state put aside by the last [`save`](Self::save) still open. Does
    /// nothing when there is none.
    pub fn restore(&mut self) {
        if let Some(state) = self.saved.pop() {
            self.state = state;
            self.commands.push(DrawCommand::Restore);
        }
    }

    /// The transform currently applied: from the coordinates later commands are
    /// written in to the surface's.
    ///
    /// Its [`invert`](Transform::invert) turns a pointer position on the surface into
    /// those coordinates.
    pub fn transform(&self) -> Transform {
        self.state.transform
    }

    /// Applies `transform` to the coordinates of later commands, before the transform
    /// already in place (the same as `<canvas>`'s `transform`).
    ///
    /// A transform holding infinities or NaN is ignored. One that squashes the plane
    /// flat (a scale of 0) makes later commands draw nothing until
    /// [`restore`](Self::restore).
    pub fn apply_transform(&mut self, transform: Transform) {
        if !transform.is_finite() || self.state.hidden {
            return;
        }
        let combined = transform.then(self.state.transform);
        let det = combined.determinant();
        if det == 0.0 || !det.is_finite() || !combined.is_finite() {
            // cairo は潰れた行列を受けるとコンテキストごと壊れる。送らずに隠す。
            self.state.hidden = true;
            return;
        }
        self.state.transform = combined;
        self.commands.push(DrawCommand::Transform(transform));
    }

    /// Moves later drawing by `dx` to the right and `dy` down.
    pub fn translate(&mut self, dx: f64, dy: f64) {
        self.apply_transform(Transform::translation(dx, dy));
    }

    /// Turns later drawing by `angle` radians around the current origin (clockwise for
    /// positive angles). To turn around a point, [`translate`](Self::translate) to it
    /// first.
    pub fn rotate(&mut self, angle: f64) {
        self.apply_transform(Transform::rotation(angle));
    }

    /// Scales later drawing by `sx` horizontally and `sy` vertically, around the
    /// current origin. Line widths, dashes and text sizes scale too.
    pub fn scale(&mut self, sx: f64, sy: f64) {
        self.apply_transform(Transform::scaling(sx, sy));
    }

    /// Limits later drawing to the inside of `path` (nonzero rule), within the clip
    /// already in place. The edge is antialiased.
    ///
    /// The clip only narrows; [`restore`](Self::restore) is the way to widen it again,
    /// so call [`save`](Self::save) first. An empty path leaves nothing to draw on.
    ///
    /// ```
    /// # use naui_core::{Color, Painter, Point, Rect};
    /// # let mut painter = Painter::new(200.0, 100.0);
    /// let plot = Rect::new(40.0, 10.0, 150.0, 70.0);
    /// painter.save();
    /// painter.clip_rect(plot);
    /// // A line running past the plot area is cut at its edges.
    /// painter.line(Point::new(0.0, 90.0), Point::new(200.0, 0.0), Color::BLACK, 2.0);
    /// painter.restore();
    /// ```
    pub fn clip(&mut self, path: &Path) {
        if self.state.hidden {
            return;
        }
        if path.is_empty() {
            self.state.hidden = true;
            return;
        }
        self.commands.push(DrawCommand::Clip(path.clone()));
    }

    /// Limits later drawing to the inside of `rect`. See [`clip`](Self::clip).
    pub fn clip_rect(&mut self, rect: Rect) {
        self.clip(&Path::new().rect(rect));
    }

    /// Fills the inside of the shape with a color or a [`Gradient`].
    ///
    /// A gradient without stops, or one that covers no area (a line of length 0, a
    /// radius of 0), draws nothing. One with a single stop fills with that color.
    pub fn fill_path(&mut self, path: &Path, paint: impl Into<Paint>) {
        if path.is_empty() || self.state.hidden {
            return;
        }
        let (paint, opacity) = match paint.into() {
            Paint::Color(color) => (Paint::Color(color), self.state.opacity),
            Paint::Gradient(gradient) => match gradient.stops() {
                [] => return,
                _ if !gradient.has_extent() => return,
                [only] => (Paint::Color(only.color), self.state.opacity * only.opacity),
                _ => (Paint::Gradient(gradient), self.state.opacity),
            },
        };
        self.commands.push(DrawCommand::Fill {
            path: path.clone(),
            paint,
            opacity,
        });
    }

    /// Strokes the outline of the shape. `width` is the line thickness (logical pixels).
    ///
    /// Widths that are 0 or less, infinite or NaN are not drawn (platforms handle them
    /// differently).
    pub fn stroke_path(&mut self, path: &Path, color: Color, width: f64) {
        if path.is_empty() || !width.is_finite() || width <= 0.0 || self.state.hidden {
            return;
        }
        self.commands.push(DrawCommand::Stroke {
            path: path.clone(),
            color,
            width,
            dash: self.state.dash.clone(),
            opacity: self.state.opacity,
        });
    }

    pub fn fill_rect(&mut self, rect: Rect, paint: impl Into<Paint>) {
        self.fill_path(&Path::new().rect(rect), paint);
    }

    pub fn stroke_rect(&mut self, rect: Rect, color: Color, width: f64) {
        self.stroke_path(&Path::new().rect(rect), color, width);
    }

    pub fn fill_circle(&mut self, center: Point, radius: f64, paint: impl Into<Paint>) {
        self.fill_path(&Path::new().circle(center, radius), paint);
    }

    pub fn stroke_circle(&mut self, center: Point, radius: f64, color: Color, width: f64) {
        self.stroke_path(&Path::new().circle(center, radius), color, width);
    }

    /// A straight line between two points.
    pub fn line(&mut self, from: Point, to: Point, color: Color, width: f64) {
        self.stroke_path(&Path::new().move_to(from).line_to(to), color, width);
    }

    /// A polyline. Draws nothing with fewer than two points.
    pub fn polyline(&mut self, points: &[Point], color: Color, width: f64) {
        if points.len() < 2 {
            return;
        }
        self.stroke_path(&Path::new().polyline(points), color, width);
    }

    /// Fills the inside of a polygon. Draws nothing with fewer than three points.
    pub fn fill_polygon(&mut self, points: &[Point], paint: impl Into<Paint>) {
        if points.len() < 3 {
            return;
        }
        self.fill_path(&Path::new().polygon(points), paint);
    }

    /// Draws one line of text. `at` is the **top** of the text, and the horizontal
    /// position follows the alignment set with
    /// [`set_text_align`](Self::set_text_align). `size` is the text size (logical
    /// pixels). The typeface is the platform's standard UI font.
    ///
    /// Newlines do not wrap (that is left to how each platform draws). Empty strings
    /// and sizes that are 0 or less, infinite or NaN are not drawn.
    pub fn text(&mut self, text: &str, at: Point, size: f64, color: Color) {
        if text.is_empty() || !size.is_finite() || size <= 0.0 || self.state.hidden {
            return;
        }
        self.commands.push(DrawCommand::Text {
            text: text.to_string(),
            at,
            size,
            color,
            align: self.state.text_align,
            opacity: self.state.opacity,
        });
    }

    /// The width of one line of text drawn with [`text`](Self::text) at `size`
    /// (logical pixels, before any transform).
    ///
    /// Inside `Canvas::on_draw` it is measured by the platform with the font it draws
    /// with, so it can be used to lay out labels and to make room for them. A painter
    /// made with [`new`](Self::new) has nothing to measure with and estimates half the
    /// size per character. Empty strings and sizes that are 0 or less, infinite or NaN
    /// give 0.
    ///
    /// ```
    /// # use naui_core::Painter;
    /// let painter = Painter::with_text_measure(100.0, 100.0, |text, size| {
    ///     text.chars().count() as f64 * size * 0.6
    /// });
    /// assert_eq!(painter.text_width("abc", 10.0), 18.0);
    /// ```
    pub fn text_width(&self, text: &str, size: f64) -> f64 {
        if text.is_empty() || !size.is_finite() || size <= 0.0 {
            return 0.0;
        }
        let width = match &self.measure {
            Some(measure) => measure(text, size),
            None => text.chars().count() as f64 * size * 0.5,
        };
        if width.is_finite() {
            width.max(0.0)
        } else {
            0.0
        }
    }

    /// Draws the whole image stretched over `dest`.
    ///
    /// To keep its proportions, make `dest` the image's [`size`](Bitmap::size) times
    /// one scale. It is drawn smoothly (not with blocky pixels) when stretched.
    pub fn draw_image(&mut self, bitmap: &Bitmap, dest: Rect) {
        let (width, height) = bitmap.size();
        let source = Rect::new(0.0, 0.0, f64::from(width), f64::from(height));
        self.draw_image_region(bitmap, source, dest);
    }

    /// Draws the `source` part of the image (in its pixels, origin at the top left)
    /// stretched over `dest`. Use it to draw one picture out of a sheet of them.
    ///
    /// The part of `source` outside the image is left out, along with the matching
    /// part of `dest`. Rectangles with a width or height of 0 or less, infinite or NaN
    /// draw nothing.
    pub fn draw_image_region(&mut self, bitmap: &Bitmap, source: Rect, dest: Rect) {
        if self.state.hidden {
            return;
        }
        let usable = |r: &Rect| {
            [r.x, r.y, r.width, r.height].iter().all(|v| v.is_finite())
                && r.width > 0.0
                && r.height > 0.0
        };
        if !usable(&source) || !usable(&dest) {
            return;
        }
        // 画像の外へはみ出す分は、宛先も同じ割合だけ削る (`<canvas>` の drawImage と同じ)。
        let (width, height) = bitmap.size();
        let left = source.x.max(0.0);
        let top = source.y.max(0.0);
        let right = source.right().min(f64::from(width));
        let bottom = source.bottom().min(f64::from(height));
        if right <= left || bottom <= top {
            return;
        }
        let sx = dest.width / source.width;
        let sy = dest.height / source.height;
        let dest = Rect::new(
            dest.x + (left - source.x) * sx,
            dest.y + (top - source.y) * sy,
            (right - left) * sx,
            (bottom - top) * sy,
        );
        self.commands.push(DrawCommand::Image {
            bitmap: bitmap.clone(),
            source: Rect::new(left, top, right - left, bottom - top),
            dest,
            opacity: self.state.opacity,
        });
    }
}

/// The kind of pointer (mouse, touch, pen) movement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PointerPhase {
    /// Pressed. See [`PointerEvent::button`] for which button.
    Down,
    /// Moved. Also delivered while nothing is pressed (hover).
    Move,
    /// Released. See [`PointerEvent::button`] for which button.
    Up,
    /// The wheel was turned (including two-finger scrolling on a trackpad). See
    /// [`PointerEvent::wheel_x`] / [`PointerEvent::wheel_y`] for the amount.
    ///
    /// Not delivered to `on_pointer`; only to `Canvas::on_wheel`.
    Wheel,
}

/// The button that was pressed or released.
///
/// Button numbering differs between platforms (AppKit has left as 0, GTK has left
/// as 1 and right as 3, the Web has middle as 1 and right as 2), so buttons are
/// matched by meaning. The OS applies left-handed button swapping first, so
/// `Primary` is always "the button you mainly use". Touch and pen tips are
/// `Primary`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PointerButton {
    /// The primary button (usually left). Touch and pen tips are this too.
    Primary,
    /// The secondary button (usually right). The one that opens context menus.
    Secondary,
    /// The middle button (pressing the wheel).
    Middle,
    /// The back button (the front side button on a five-button mouse).
    Back,
    /// The forward button (the rear side button on a five-button mouse).
    Forward,
}

impl PointerButton {
    /// Creates one from the Web's `MouseEvent.button` number. Unknown numbers give
    /// `None`.
    ///
    /// ```
    /// # use naui_core::PointerButton;
    /// assert_eq!(PointerButton::from_web_button(0), Some(PointerButton::Primary));
    /// assert_eq!(PointerButton::from_web_button(2), Some(PointerButton::Secondary));
    /// assert_eq!(PointerButton::from_web_button(-1), None);
    /// ```
    pub fn from_web_button(button: i16) -> Option<Self> {
        match button {
            0 => Some(Self::Primary),
            1 => Some(Self::Middle),
            2 => Some(Self::Secondary),
            3 => Some(Self::Back),
            4 => Some(Self::Forward),
            _ => None,
        }
    }
}

/// How many logical pixels one wheel step (notch) is reported as.
///
/// [`PointerEvent::wheel_x`] / [`PointerEvent::wheel_y`] unify each platform's
/// units (notches, lines, logical pixels) into logical pixels. For a mouse wheel
/// with notches, one notch becomes this value (Windows's default of "3 lines per
/// notch" times 16 pixels per line). Devices that measure finely, such as
/// trackpads, deliver the logical-pixel amount the OS reports as is.
///
/// When you want to decide "how much per notch", for example for zooming, divide
/// the amount by this.
pub const WHEEL_STEP: f64 = 48.0;

/// The pointer moved over the drawing surface.
///
/// The position is in logical pixels with the surface's top left as origin, the
/// same as [`Painter`] coordinates. For the wheel it is where the pointer is
/// (usable as the zoom center).
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct PointerEvent {
    pub phase: PointerPhase,
    pub point: Point,
    /// The button pressed or released. Set only for `Down` and `Up`.
    ///
    /// `None` for `Move` and `Wheel`, and for buttons that cannot be identified (the
    /// sixth and beyond, for example). Remember the button held during a drag at
    /// `Down`.
    pub button: Option<PointerButton>,
    /// Horizontal scroll amount (logical pixels). Set only for `Wheel`; 0 otherwise.
    ///
    /// Positive is the direction of reading onward to the right (content moves left).
    pub wheel_x: f64,
    /// Vertical scroll amount (logical pixels). Set only for `Wheel`; 0 otherwise.
    ///
    /// Positive is the direction of reading onward downward (the wheel turned toward
    /// you; content moves up). OS settings that reverse the direction, such as macOS's
    /// "natural scrolling", are delivered after reversal (the same direction as page
    /// scrolling).
    pub wheel_y: f64,
}

impl PointerEvent {
    /// Creates an event with no button and no wheel amount.
    pub fn new(phase: PointerPhase, point: Point) -> Self {
        Self {
            phase,
            point,
            button: None,
            wheel_x: 0.0,
            wheel_y: 0.0,
        }
    }

    /// Attaches the button that was pressed or released.
    pub fn button(mut self, button: Option<PointerButton>) -> Self {
        self.button = button;
        self
    }

    /// Creates a wheel event. The amounts are logical pixels (see
    /// [`wheel_y`](Self::wheel_y) for the direction).
    pub fn wheel(point: Point, wheel_x: f64, wheel_y: f64) -> Self {
        Self {
            phase: PointerPhase::Wheel,
            point,
            button: None,
            wheel_x,
            wheel_y,
        }
    }
}

/// Holds one handler for `Canvas::on_wheel`. Shared by all backends.
///
/// Following the same rules as [`KeyHandler`](crate::KeyHandler), the closure is
/// taken out while it is being called. When there is no handler, or the event
/// arrives re-entrantly, it is treated as `Continue` (passed on to the parent's
/// scrolling).
#[derive(Default)]
pub struct WheelHandler(std::cell::RefCell<crate::Slot<WheelCallback>>);

type WheelCallback = dyn FnMut(PointerEvent) -> EventResponse;

impl WheelHandler {
    pub fn set(&self, f: impl FnMut(PointerEvent) -> EventResponse + 'static) {
        *self.0.borrow_mut() = Some(Box::new(f));
    }

    pub fn emit(&self, event: PointerEvent) -> EventResponse {
        let Some(mut f) = self
            .0
            .try_borrow_mut()
            .ok()
            .and_then(|mut slot| slot.take())
        else {
            return EventResponse::Continue;
        };
        let response = f(event);
        let mut slot = self.0.borrow_mut();
        if slot.is_none() {
            *slot = Some(f);
        }
        response
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::{FRAC_PI_2, PI, TAU};

    fn close_to(a: Point, b: Point) -> bool {
        (a.x - b.x).abs() < 1e-9 && (a.y - b.y).abs() < 1e-9
    }

    #[test]
    fn rect_helpers() {
        let rect = Rect::new(10.0, 20.0, 30.0, 40.0);
        assert_eq!(rect.right(), 40.0);
        assert_eq!(rect.bottom(), 60.0);
        assert_eq!(rect.center(), Point::new(25.0, 40.0));
        assert!(rect.contains(Point::new(10.0, 20.0)));
        assert!(rect.contains(Point::new(40.0, 60.0)));
        assert!(!rect.contains(Point::new(9.9, 30.0)));
        assert_eq!(rect.inset(5.0), Rect::new(15.0, 25.0, 20.0, 30.0));
        assert_eq!(rect.inset(100.0).width, 0.0);
    }

    #[test]
    fn line_without_a_pen_position_starts_a_subpath() {
        let path = Path::new().line_to(Point::new(1.0, 2.0));
        assert_eq!(
            path.segments(),
            &[PathSegment::MoveTo(Point::new(1.0, 2.0))]
        );
        assert!(
            Path::new().close().is_empty(),
            "空のまま閉じても何も積まない"
        );
    }

    #[test]
    fn quad_becomes_cubic_with_the_same_endpoints() {
        let path = Path::new()
            .move_to(Point::new(0.0, 0.0))
            .quad_to(Point::new(3.0, 6.0), Point::new(6.0, 0.0));
        let [_, PathSegment::CubicTo {
            control1,
            control2,
            to,
        }] = path.segments()
        else {
            panic!("3 次ベジェへ直っていること: {:?}", path.segments());
        };
        assert!(close_to(*control1, Point::new(2.0, 4.0)));
        assert!(close_to(*control2, Point::new(4.0, 4.0)));
        assert_eq!(*to, Point::new(6.0, 0.0));
    }

    #[test]
    fn circle_is_four_cubics_that_return_to_the_start() {
        let path = Path::new().circle(Point::new(50.0, 50.0), 10.0);
        let segments = path.segments();
        assert_eq!(segments.len(), 6, "move + 4 本 + close");
        assert_eq!(segments[0], PathSegment::MoveTo(Point::new(60.0, 50.0)));
        assert!(matches!(segments[5], PathSegment::Close));
        let PathSegment::CubicTo { to, .. } = segments[4] else {
            panic!("最後の手はベジェであること");
        };
        assert!(close_to(to, Point::new(60.0, 50.0)), "始点へ戻る: {to:?}");
        // 上端 (y が小さい側) を通る。
        let PathSegment::CubicTo { to, .. } = segments[3] else {
            panic!()
        };
        assert!(close_to(to, Point::new(50.0, 40.0)), "{to:?}");
        // 制御点の張り出しは半径 × 0.5523 (円の近似の定数)。
        let PathSegment::CubicTo { control1, .. } = segments[1] else {
            panic!()
        };
        assert!((control1.x - 60.0).abs() < 1e-9);
        assert!((control1.y - (50.0 + 10.0 * 0.552_284_749_830_793)).abs() < 1e-9);
    }

    #[test]
    fn arc_lines_to_its_start_and_splits_into_quarters() {
        let center = Point::new(0.0, 0.0);
        let path = Path::new().move_to(center).arc(center, 10.0, 0.0, PI);
        let segments = path.segments();
        // move (中心) + line (始点) + 2 本の 1/4 弧。
        assert_eq!(segments.len(), 4, "{segments:?}");
        assert_eq!(segments[1], PathSegment::LineTo(Point::new(10.0, 0.0)));
        let PathSegment::CubicTo { to, .. } = segments[2] else {
            panic!()
        };
        assert!(
            close_to(to, Point::new(0.0, 10.0)),
            "時計回りに下へ: {to:?}"
        );
        let PathSegment::CubicTo { to, .. } = segments[3] else {
            panic!()
        };
        assert!(close_to(to, Point::new(-10.0, 0.0)), "{to:?}");

        // 逆向きも引ける。
        let back = Path::new().arc(center, 10.0, FRAC_PI_2, 0.0);
        let PathSegment::CubicTo { to, .. } = back.segments()[1] else {
            panic!()
        };
        assert!(close_to(to, Point::new(10.0, 0.0)));

        // 1 周を超える分は 1 周で止める。
        let full = Path::new().arc(center, 10.0, 0.0, TAU * 3.0);
        assert_eq!(full.segments().len(), 5);
        // 0 の弧は始点だけ。
        assert_eq!(Path::new().arc(center, 10.0, 1.0, 1.0).segments().len(), 1);
    }

    #[test]
    fn current_point_follows_close() {
        let path = Path::new()
            .move_to(Point::new(1.0, 1.0))
            .line_to(Point::new(5.0, 1.0))
            .close();
        assert_eq!(path.current_point(), Some(Point::new(1.0, 1.0)));
        assert_eq!(Path::new().current_point(), None);
    }

    #[test]
    fn bounds_cover_control_points() {
        let path = Path::new().move_to(Point::new(0.0, 0.0)).cubic_to(
            Point::new(-5.0, 2.0),
            Point::new(10.0, 20.0),
            Point::new(4.0, 4.0),
        );
        assert_eq!(path.bounds(), Some(Rect::new(-5.0, 0.0, 15.0, 20.0)));
        assert_eq!(Path::new().bounds(), None);
        assert_eq!(
            Path::new().polygon(&[]).bounds(),
            None,
            "空の多角形は何も積まない"
        );
    }

    #[test]
    fn painter_records_state_with_each_command() {
        let mut painter = Painter::new(100.0, 50.0);
        assert_eq!(painter.bounds(), Rect::new(0.0, 0.0, 100.0, 50.0));

        painter.set_opacity(0.5);
        painter.set_dash(&[4.0, 2.0]);
        painter.set_text_align(Align::Fill);
        painter.fill_rect(Rect::new(0.0, 0.0, 10.0, 10.0), Color::BLACK);
        painter.line(
            Point::new(0.0, 0.0),
            Point::new(1.0, 1.0),
            Color::WHITE,
            2.0,
        );
        painter.text("x", Point::new(3.0, 4.0), 12.0, Color::BLACK);

        let commands = painter.commands();
        assert_eq!(commands.len(), 3);
        let DrawCommand::Fill { opacity, .. } = &commands[0] else {
            panic!()
        };
        assert_eq!(*opacity, 0.5);
        let DrawCommand::Stroke { dash, width, .. } = &commands[1] else {
            panic!()
        };
        assert_eq!(dash, &[4.0, 2.0]);
        assert_eq!(*width, 2.0);
        let DrawCommand::Text { align, at, .. } = &commands[2] else {
            panic!()
        };
        assert_eq!(*align, Align::Start, "Fill は Start として扱う");
        assert_eq!(*at, Point::new(3.0, 4.0));

        // 状態を戻せば後の命令には効かない。
        painter.set_opacity(f64::NAN);
        painter.set_dash(&[]);
        painter.stroke_rect(Rect::new(0.0, 0.0, 1.0, 1.0), Color::BLACK, 1.0);
        let DrawCommand::Stroke { dash, opacity, .. } = &painter.commands()[3] else {
            panic!()
        };
        assert!(dash.is_empty());
        assert_eq!(*opacity, 1.0);
    }

    #[test]
    fn painter_skips_empty_commands() {
        let mut painter = Painter::new(10.0, 10.0);
        painter.fill_path(&Path::new(), Color::BLACK);
        painter.stroke_rect(Rect::new(0.0, 0.0, 1.0, 1.0), Color::BLACK, 0.0);
        painter.polyline(&[Point::new(0.0, 0.0)], Color::BLACK, 1.0);
        painter.fill_polygon(&[Point::new(0.0, 0.0), Point::new(1.0, 1.0)], Color::BLACK);
        painter.text("", Point::new(0.0, 0.0), 12.0, Color::BLACK);
        painter.text("a", Point::new(0.0, 0.0), 0.0, Color::BLACK);
        // 無限や NaN の太さ・大きさは、環境ごとに扱いが変わるので積まない。
        painter.stroke_rect(Rect::new(0.0, 0.0, 1.0, 1.0), Color::BLACK, f64::INFINITY);
        painter.stroke_rect(Rect::new(0.0, 0.0, 1.0, 1.0), Color::BLACK, f64::NAN);
        painter.text("a", Point::new(0.0, 0.0), f64::INFINITY, Color::BLACK);
        painter.text("a", Point::new(0.0, 0.0), f64::NAN, Color::BLACK);
        assert!(painter.commands().is_empty());
        painter.set_dash(&[-1.0, 2.0]);
        assert!(painter.dash().is_empty(), "負の破線は無視する");
        painter.set_dash(&[0.0, 0.0]);
        assert!(painter.dash().is_empty(), "全部 0 は実線");
    }

    #[test]
    fn pointer_events_carry_button_and_wheel() {
        let down = PointerEvent::new(PointerPhase::Down, Point::new(1.0, 2.0))
            .button(Some(PointerButton::Secondary));
        assert_eq!(down.button, Some(PointerButton::Secondary));
        assert_eq!((down.wheel_x, down.wheel_y), (0.0, 0.0));

        let wheel = PointerEvent::wheel(Point::new(3.0, 4.0), -1.5, WHEEL_STEP);
        assert_eq!(wheel.phase, PointerPhase::Wheel);
        assert_eq!(wheel.point, Point::new(3.0, 4.0));
        assert_eq!(wheel.button, None);
        assert_eq!((wheel.wheel_x, wheel.wheel_y), (-1.5, 48.0));

        assert_eq!(
            PointerButton::from_web_button(1),
            Some(PointerButton::Middle),
            "Web は中ボタンが 1"
        );
        assert_eq!(PointerButton::from_web_button(3), Some(PointerButton::Back));
        assert_eq!(
            PointerButton::from_web_button(4),
            Some(PointerButton::Forward)
        );
        assert_eq!(PointerButton::from_web_button(5), None);
    }

    #[test]
    fn wheel_handler_continues_without_a_callback() {
        use std::rc::Rc;
        let handler = Rc::new(WheelHandler::default());
        let event = PointerEvent::wheel(Point::default(), 0.0, 1.0);
        assert_eq!(handler.emit(event), EventResponse::Continue);
        handler.set({
            let handler = handler.clone();
            move |event| {
                // 入れ子で届いた分は Continue。
                assert_eq!(handler.emit(event), EventResponse::Continue);
                EventResponse::Handled
            }
        });
        assert_eq!(handler.emit(event), EventResponse::Handled);
        assert_eq!(
            handler.emit(event),
            EventResponse::Handled,
            "呼んだ後も残る"
        );
    }

    fn close(a: Transform, b: Transform) -> bool {
        [a.a - b.a, a.b - b.b, a.c - b.c, a.d - b.d, a.e - b.e, a.f - b.f]
            .iter()
            .all(|v| v.abs() < 1e-9)
    }

    #[test]
    fn transforms_compose_in_order() {
        // 拡大してから移動: 移動量は拡大されない。
        let t = Transform::scaling(2.0, 3.0).then(Transform::translation(10.0, 20.0));
        assert_eq!(t.apply(Point::new(1.0, 1.0)), Point::new(12.0, 23.0));
        // 移動してから拡大: 移動量も拡大される。
        let t = Transform::translation(10.0, 20.0).then(Transform::scaling(2.0, 3.0));
        assert_eq!(t.apply(Point::new(1.0, 1.0)), Point::new(22.0, 63.0));
        // y が下向きなので、正の角度は右から下へ回る。
        let p = Transform::rotation(FRAC_PI_2).apply(Point::new(1.0, 0.0));
        assert!(close_to(p, Point::new(0.0, 1.0)), "{p:?}");
        assert!(!Transform::rotation(0.3).is_axis_aligned());
        assert!(Transform::scaling(2.0, -1.0).is_axis_aligned());
    }

    #[test]
    fn inverse_undoes_and_flat_transforms_have_none() {
        let t = Transform::rotation(0.7)
            .then(Transform::scaling(2.0, 0.5))
            .then(Transform::translation(3.0, -4.0));
        let inverse = t.invert().expect("逆がある");
        assert!(close(t.then(inverse), Transform::IDENTITY));
        let p = Point::new(5.0, 6.0);
        assert!(close_to(inverse.apply(t.apply(p)), p));
        assert_eq!(Transform::scaling(0.0, 1.0).invert(), None);
        assert_eq!(Transform::translation(f64::NAN, 0.0).invert(), None);
    }

    #[test]
    fn painter_transforms_apply_before_the_current_one() {
        let mut painter = Painter::new(100.0, 100.0);
        painter.translate(10.0, 0.0);
        painter.scale(2.0, 2.0);
        // 後から積んだ拡大が先に効き、その後で移動する (`<canvas>` と同じ)。
        assert_eq!(
            painter.transform().apply(Point::new(1.0, 1.0)),
            Point::new(12.0, 2.0)
        );
        assert_eq!(
            painter.commands(),
            &[
                DrawCommand::Transform(Transform::translation(10.0, 0.0)),
                DrawCommand::Transform(Transform::scaling(2.0, 2.0)),
            ],
            "命令には相対の変換だけを積む"
        );
        // 無限や NaN は無視する。
        painter.translate(f64::INFINITY, 0.0);
        painter.rotate(f64::NAN);
        assert_eq!(painter.commands().len(), 2);
    }

    #[test]
    fn save_and_restore_bring_back_the_state() {
        let mut painter = Painter::new(100.0, 100.0);
        painter.set_opacity(0.5);
        painter.save();
        painter.set_opacity(0.2);
        painter.set_dash(&[1.0, 1.0]);
        painter.set_text_align(Align::End);
        painter.translate(5.0, 5.0);
        painter.restore();
        assert_eq!(painter.opacity(), 0.5);
        assert!(painter.dash().is_empty());
        assert_eq!(painter.text_align(), Align::Start);
        assert_eq!(painter.transform(), Transform::IDENTITY);
        // 対になる save の無い restore は積まない。
        painter.restore();
        assert_eq!(
            painter.commands(),
            &[
                DrawCommand::Save,
                DrawCommand::Transform(Transform::translation(5.0, 5.0)),
                DrawCommand::Restore,
            ]
        );
        // 閉じ忘れた save は取り出すときに閉じる。
        painter.save();
        painter.save();
        let commands = painter.into_commands();
        assert_eq!(
            &commands[3..],
            &[
                DrawCommand::Save,
                DrawCommand::Save,
                DrawCommand::Restore,
                DrawCommand::Restore
            ]
        );
    }

    #[test]
    fn flat_transforms_and_empty_clips_hide_until_restore() {
        let mut painter = Painter::new(100.0, 100.0);
        let rect = Rect::new(0.0, 0.0, 10.0, 10.0);
        painter.save();
        painter.scale(0.0, 1.0);
        painter.fill_rect(rect, Color::BLACK);
        painter.translate(1.0, 1.0);
        painter.text("a", Point::default(), 10.0, Color::BLACK);
        painter.restore();
        painter.save();
        painter.clip(&Path::new());
        painter.line(Point::default(), Point::new(5.0, 5.0), Color::BLACK, 1.0);
        painter.restore();
        assert_eq!(
            painter.commands(),
            &[
                DrawCommand::Save,
                DrawCommand::Restore,
                DrawCommand::Save,
                DrawCommand::Restore
            ],
            "潰れた変換も空の切り抜きも送らず、その間の描画も積まない"
        );
        painter.fill_rect(rect, Color::BLACK);
        assert_eq!(painter.commands().len(), 5, "restore の後はまた描ける");
        painter.clip_rect(rect);
        assert!(matches!(painter.commands()[5], DrawCommand::Clip(_)));
    }

    #[test]
    fn gradient_stops_stay_sorted_and_clamped() {
        let red = Color::rgb(255, 0, 0);
        let blue = Color::rgb(0, 0, 255);
        let gradient = Gradient::linear(Point::new(0.0, 0.0), Point::new(10.0, 0.0))
            .stop(1.0, blue)
            .stop_with_opacity(-1.0, red, 2.0)
            .stop(0.5, red)
            .stop(0.5, blue)
            .stop(f64::NAN, red);
        let stops: Vec<_> = gradient
            .stops()
            .iter()
            .map(|s| (s.offset, s.color, s.opacity))
            .collect();
        assert_eq!(
            stops,
            [
                (0.0, red, 1.0),
                (0.5, red, 1.0),
                (0.5, blue, 1.0),
                (1.0, blue, 1.0)
            ],
            "同じ位置は足した順のまま"
        );
    }

    #[test]
    fn fills_take_colors_and_gradients() {
        let red = Color::rgb(255, 0, 0);
        let rect = Rect::new(0.0, 0.0, 10.0, 10.0);
        let mut painter = Painter::new(100.0, 100.0);
        painter.set_opacity(0.5);
        let two = Gradient::radial(Point::new(5.0, 5.0), 5.0)
            .stop(0.0, red)
            .stop(1.0, Color::BLACK);
        painter.fill_rect(rect, &two);
        painter.fill_rect(rect, red);
        // 1 色だけなら単色の塗りにし、その不透明度を掛ける。
        painter.fill_rect(
            rect,
            Gradient::linear(Point::new(0.0, 0.0), Point::new(1.0, 0.0)).stop_with_opacity(
                0.3,
                red,
                0.5,
            ),
        );
        // 色の無いもの、長さ 0 の線、半径 0 の円は塗らない。
        painter.fill_rect(
            rect,
            Gradient::linear(Point::new(0.0, 0.0), Point::new(1.0, 0.0)),
        );
        let still = Point::new(3.0, 3.0);
        painter.fill_rect(
            rect,
            Gradient::linear(still, still).stop(0.0, red).stop(1.0, red),
        );
        painter.fill_rect(
            rect,
            Gradient::radial(still, 0.0).stop(0.0, red).stop(1.0, red),
        );
        let fills: Vec<_> = painter
            .commands()
            .iter()
            .map(|c| match c {
                DrawCommand::Fill { paint, opacity, .. } => (paint.clone(), *opacity),
                other => panic!("塗りだけのはず: {other:?}"),
            })
            .collect();
        assert_eq!(
            fills,
            [
                (Paint::Gradient(two), 0.5),
                (Paint::Color(red), 0.5),
                (Paint::Color(red), 0.25),
            ]
        );
    }

    #[test]
    fn images_keep_the_part_inside_the_bitmap() {
        let bitmap = Bitmap::from_pixels(4, 2, &[0; 4 * 2 * 4]).unwrap();
        let mut painter = Painter::new(100.0, 100.0);
        painter.draw_image(&bitmap, Rect::new(10.0, 10.0, 40.0, 20.0));
        // 左へ 2 画素はみ出した元は、宛先も同じ割合 (1 画素 = 10) だけ削る。
        painter.draw_image_region(
            &bitmap,
            Rect::new(-2.0, 0.0, 4.0, 2.0),
            Rect::new(0.0, 0.0, 40.0, 20.0),
        );
        painter.draw_image_region(
            &bitmap,
            Rect::new(5.0, 0.0, 1.0, 1.0),
            Rect::new(0.0, 0.0, 10.0, 10.0),
        );
        painter.draw_image(&bitmap, Rect::new(0.0, 0.0, 0.0, 10.0));
        painter.draw_image(&bitmap, Rect::new(f64::NAN, 0.0, 10.0, 10.0));
        let images: Vec<_> = painter
            .commands()
            .iter()
            .map(|c| match c {
                DrawCommand::Image { source, dest, .. } => (*source, *dest),
                other => panic!("画像だけのはず: {other:?}"),
            })
            .collect();
        assert_eq!(
            images,
            [
                (
                    Rect::new(0.0, 0.0, 4.0, 2.0),
                    Rect::new(10.0, 10.0, 40.0, 20.0)
                ),
                (
                    Rect::new(0.0, 0.0, 2.0, 2.0),
                    Rect::new(20.0, 0.0, 20.0, 20.0)
                ),
            ]
        );
    }

    #[test]
    fn text_width_uses_the_platform_measure() {
        let estimate = Painter::new(10.0, 10.0);
        assert_eq!(estimate.text_width("abcd", 10.0), 20.0);
        assert_eq!(estimate.text_width("", 10.0), 0.0);
        assert_eq!(estimate.text_width("a", f64::NAN), 0.0);
        let measured = Painter::with_text_measure(10.0, 10.0, |text, size| {
            if text == "bad" {
                f64::NAN
            } else {
                size * 2.0
            }
        });
        assert_eq!(measured.text_width("x", 7.0), 14.0);
        assert_eq!(measured.text_width("bad", 7.0), 0.0, "測れない値は 0");
        // 複製しても測り方は残る。
        assert_eq!(measured.clone().text_width("x", 1.0), 2.0);
    }
}
