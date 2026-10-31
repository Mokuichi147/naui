//! Value types for the drawing surface (`Canvas`) and the record of what to draw.
//!
//! Things **the app draws itself**, such as charts and shapes, cannot be built by
//! combining standard controls. naui therefore provides only the drawing surface,
//! and the app writes what to draw into a [`Painter`]. A `Painter` **only records**
//! drawing commands; turning them into pixels is the job of each platform's 2D API
//! (Core Graphics / cairo / Direct2D / the 2D context of `<canvas>`). There
//! is no rasterization or font measurement here.
//!
//! Coordinates are **logical pixels** with the origin at the top left and positive
//! values going right and down. Each platform applies the scale factor for
//! high-resolution displays.
//!
//! The vocabulary is kept to the minimum that all four platforms share. Every line
//! and fill becomes a [`Path`], and a `Path` contains only straight lines and cubic
//! Béziers. Circles and arcs are converted to Béziers here before being handed
//! over, so they come out the same shape on every platform.

use crate::{Align, Color, EventResponse};

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

/// A drawing command recorded by a [`Painter`]. Backends replay them in order.
///
/// `opacity` runs from 0.0 (transparent) to 1.0 (opaque). The color itself is an
/// opaque [`Color`]; transparency is carried separately by each command.
#[derive(Debug, Clone, PartialEq)]
pub enum DrawCommand {
    /// Fills the inside of the shape. The fill rule is nonzero (the default on all four
    /// platforms).
    Fill {
        path: Path,
        color: Color,
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
}

/// The record of what to draw. Write to it inside `Canvas::on_draw`.
///
/// It only stores drawing commands and never touches pixels. Line widths, text
/// sizes and coordinates are all in logical pixels. Opacity, dashes and text
/// alignment are kept as **state** and apply to later commands until changed (the
/// same model as `globalAlpha` and friends in `<canvas>`).
///
/// ```
/// # use naui_core::{Color, Painter, Point, Rect};
/// let mut painter = Painter::new(200.0, 100.0);
/// painter.fill_rect(Rect::new(10.0, 10.0, 50.0, 30.0), Color::rgb(0x33, 0x66, 0xff));
/// painter.line(Point::new(0.0, 0.0), Point::new(200.0, 100.0), Color::BLACK, 1.0);
/// assert_eq!(painter.commands().len(), 2);
/// ```
#[derive(Debug, Clone)]
pub struct Painter {
    width: f64,
    height: f64,
    opacity: f64,
    dash: Vec<f64>,
    text_align: Align,
    commands: Vec<DrawCommand>,
}

impl Painter {
    /// Starts recording for a `width` × `height` surface.
    pub fn new(width: f64, height: f64) -> Self {
        Self {
            width,
            height,
            opacity: 1.0,
            dash: Vec::new(),
            text_align: Align::Start,
            commands: Vec::new(),
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
    pub fn bounds(&self) -> Rect {
        Rect::new(0.0, 0.0, self.width, self.height)
    }

    /// The recorded commands. Backends replay them.
    pub fn commands(&self) -> &[DrawCommand] {
        &self.commands
    }

    /// Takes out the recorded commands.
    pub fn into_commands(self) -> Vec<DrawCommand> {
        self.commands
    }

    /// Opacity for subsequent commands. 0.0 is transparent and 1.0 is opaque. Values
    /// out of range are clamped.
    pub fn set_opacity(&mut self, opacity: f64) {
        self.opacity = if opacity.is_finite() {
            opacity.clamp(0.0, 1.0)
        } else {
            1.0
        };
    }

    pub fn opacity(&self) -> f64 {
        self.opacity
    }

    /// Makes subsequent lines dashed. `pattern` repeats the lengths of dashes and gaps
    /// (`[4.0, 2.0]` draws 4 and skips 2). An empty pattern goes back to solid lines.
    ///
    /// A pattern containing negative or infinite values is ignored (lines stay solid).
    pub fn set_dash(&mut self, pattern: &[f64]) {
        if pattern.iter().all(|v| v.is_finite() && *v >= 0.0) && pattern.iter().any(|v| *v > 0.0) {
            self.dash = pattern.to_vec();
        } else {
            self.dash.clear();
        }
    }

    pub fn dash(&self) -> &[f64] {
        &self.dash
    }

    /// Horizontal alignment for subsequent text. `Fill` is treated as `Start`.
    pub fn set_text_align(&mut self, align: Align) {
        self.text_align = match align {
            Align::Fill => Align::Start,
            other => other,
        };
    }

    pub fn text_align(&self) -> Align {
        self.text_align
    }

    /// Fills the inside of the shape.
    pub fn fill_path(&mut self, path: &Path, color: Color) {
        if path.is_empty() {
            return;
        }
        self.commands.push(DrawCommand::Fill {
            path: path.clone(),
            color,
            opacity: self.opacity,
        });
    }

    /// Strokes the outline of the shape. `width` is the line thickness (logical pixels).
    ///
    /// Widths that are 0 or less, infinite or NaN are not drawn (platforms handle them
    /// differently).
    pub fn stroke_path(&mut self, path: &Path, color: Color, width: f64) {
        if path.is_empty() || !width.is_finite() || width <= 0.0 {
            return;
        }
        self.commands.push(DrawCommand::Stroke {
            path: path.clone(),
            color,
            width,
            dash: self.dash.clone(),
            opacity: self.opacity,
        });
    }

    pub fn fill_rect(&mut self, rect: Rect, color: Color) {
        self.fill_path(&Path::new().rect(rect), color);
    }

    pub fn stroke_rect(&mut self, rect: Rect, color: Color, width: f64) {
        self.stroke_path(&Path::new().rect(rect), color, width);
    }

    pub fn fill_circle(&mut self, center: Point, radius: f64, color: Color) {
        self.fill_path(&Path::new().circle(center, radius), color);
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
    pub fn fill_polygon(&mut self, points: &[Point], color: Color) {
        if points.len() < 3 {
            return;
        }
        self.fill_path(&Path::new().polygon(points), color);
    }

    /// Draws one line of text. `at` is the **top** of the text, and the horizontal
    /// position follows the alignment set with
    /// [`set_text_align`](Self::set_text_align). `size` is the text size (logical
    /// pixels). The typeface is the platform's standard UI font.
    ///
    /// Newlines do not wrap (that is left to how each platform draws). Empty strings
    /// and sizes that are 0 or less, infinite or NaN are not drawn.
    pub fn text(&mut self, text: &str, at: Point, size: f64, color: Color) {
        if text.is_empty() || !size.is_finite() || size <= 0.0 {
            return;
        }
        self.commands.push(DrawCommand::Text {
            text: text.to_string(),
            at,
            size,
            color,
            align: self.text_align,
            opacity: self.opacity,
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
}
