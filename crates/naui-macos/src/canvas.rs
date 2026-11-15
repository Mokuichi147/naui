//! The drawing surface (an `NSView` subclass + `drawRect:`).
//!
//! In AppKit, the standard form of "a surface the app draws itself" is subclassing
//! `NSView` and writing `drawRect:`. naui uses that form as is: inside `drawRect:`
//! it calls the app's `on_draw` and replays the commands recorded in the
//! [`Painter`] with `NSBezierPath` and `NSString` drawing
//! (`drawAtPoint:withAttributes:`). Rasterization, antialiasing, Retina scaling and
//! text drawing are all done by AppKit (Core Graphics / Core Text).
//!
//! Coordinates are made **top-left origin, pointing down** by setting `isFlipped`
//! to `true` (the same as the other three platforms). The font is the system font
//! (`systemFontOfSize:`).
//!
//! Pointer notifications come from `mouseDown:` / `mouseDragged:` / `mouseUp:`
//! (and the same set for the right and middle buttons) and from `mouseMoved:`
//! through an `NSTrackingArea`. The button is identified by which method AppKit
//! dispatched to (`mouseDown:` is left, `rightMouseDown:` is right). Other buttons
//! all come to `otherMouseDown:`, so `NSEvent.buttonNumber` is checked (2 for
//! middle, 3 and 4 for the side buttons). Two-finger clicks on a trackpad and the
//! secondary-click setting are also dispatched by AppKit to `rightMouseDown:`, so
//! they arrive as the right button.
//!
//! The wheel comes from `scrollWheel:`. `scrollingDeltaX` / `scrollingDeltaY` are
//! logical pixels for finely measuring devices such as trackpads
//! (`hasPreciseScrollingDeltas`) and lines for notched wheels (about 1 per notch),
//! so the latter are multiplied by [`WHEEL_STEP`]. AppKit's amounts are signed by
//! "the direction the content moves", the opposite of the other three platforms, so
//! the sign is flipped. When the app returns `Continue`, it is passed to `super`,
//! and the outer `NSScrollView` scrolls as usual.

use std::cell::RefCell;
use std::rc::Rc;

use naui_core::{
    Align, Bitmap, BitmapData, Color, DrawCommand, EventResponse, Gradient, GradientShape, Paint,
    Painter, Path, PathSegment, PointerButton, PointerEvent, PointerPhase, Rect, WheelHandler,
    WHEEL_STEP,
};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObjectProtocol};
use objc2::{
    define_class, msg_send, AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, Message,
};
use objc2_app_kit::{
    NSAffineTransformNSAppKitAdditions, NSBezierPath, NSColor, NSColorSpace,
    NSCompositingOperation, NSEvent, NSFont, NSFontAttributeName, NSForegroundColorAttributeName,
    NSGradient, NSGradientDrawingOptions, NSGraphicsContext, NSImage, NSLineCapStyle,
    NSLineJoinStyle, NSStringDrawing, NSTrackingArea, NSTrackingAreaOptions, NSView,
};
use objc2_foundation::{
    NSAffineTransform, NSAffineTransformStruct, NSArray, NSAttributedStringKey, NSData,
    NSDictionary, NSPoint, NSRect, NSSize, NSString,
};

use crate::trampoline::ValueHandler;
use crate::widgets::{impl_widget, Widget};

/// The slot for the closure that decides what to draw.
///
/// Taken out while it is being called (the same rule as `ValueHandler`). Calling
/// `on_draw` again mid-draw does not double-borrow.
type DrawSlot = RefCell<Option<Box<dyn FnMut(&mut Painter)>>>;

struct CanvasState {
    draw: DrawSlot,
    pointer: ValueHandler<PointerEvent>,
    wheel: WheelHandler,
    /// The tracking area for receiving `mouseMoved:`. Recreated whenever the size
    /// changes.
    tracking: RefCell<Option<Retained<NSTrackingArea>>>,
}

define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "NauiCanvasView"]
    #[ivars = CanvasState]
    struct CanvasView;

    unsafe impl NSObjectProtocol for CanvasView {}

    impl CanvasView {
        /// Top-left origin, pointing down. The same coordinates as the other three
        /// platforms.
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        /// Accepts the first click even when the window is not frontmost (so that, when
        /// clicking on a drawing to select, the first click is not eaten by "bring to
        /// front").
        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, _event: Option<&NSEvent>) -> bool {
            true
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            let bounds = self.bounds();
            let mut painter =
                Painter::with_text_measure(bounds.size.width, bounds.size.height, text_width);
            let Some(mut draw) = self.ivars().draw.borrow_mut().take() else {
                return;
            };
            draw(&mut painter);
            let mut slot = self.ivars().draw.borrow_mut();
            if slot.is_none() {
                *slot = Some(draw);
            }
            drop(slot);
            replay(&painter.into_commands());
        }

        #[unsafe(method(updateTrackingAreas))]
        fn update_tracking_areas(&self) {
            let _: () = unsafe { msg_send![super(self), updateTrackingAreas] };
            if let Some(old) = self.ivars().tracking.borrow_mut().take() {
                self.removeTrackingArea(&old);
            }
            let options = NSTrackingAreaOptions::MouseMoved
                | NSTrackingAreaOptions::ActiveInKeyWindow
                | NSTrackingAreaOptions::InVisibleRect;
            let area = unsafe {
                NSTrackingArea::initWithRect_options_owner_userInfo(
                    NSTrackingArea::alloc(),
                    self.bounds(),
                    options,
                    Some(self),
                    None,
                )
            };
            self.addTrackingArea(&area);
            *self.ivars().tracking.borrow_mut() = Some(area);
        }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, event: &NSEvent) {
            self.emit_pointer(PointerPhase::Down, event, Some(PointerButton::Primary));
        }

        #[unsafe(method(mouseDragged:))]
        fn mouse_dragged(&self, event: &NSEvent) {
            self.emit_pointer(PointerPhase::Move, event, None);
        }

        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, event: &NSEvent) {
            self.emit_pointer(PointerPhase::Up, event, Some(PointerButton::Primary));
        }

        #[unsafe(method(mouseMoved:))]
        fn mouse_moved(&self, event: &NSEvent) {
            self.emit_pointer(PointerPhase::Move, event, None);
        }

        // 右ボタンと中ボタンは AppKit が別のメソッドへ配るので、同じ通知へ寄せる。
        // 既定の `rightMouseDown:` はコンテキストメニューの経路へ回るため、
        // ここで受け止めて `super` は呼ばない。

        #[unsafe(method(rightMouseDown:))]
        fn right_mouse_down(&self, event: &NSEvent) {
            self.emit_pointer(PointerPhase::Down, event, Some(PointerButton::Secondary));
        }

        #[unsafe(method(rightMouseDragged:))]
        fn right_mouse_dragged(&self, event: &NSEvent) {
            self.emit_pointer(PointerPhase::Move, event, None);
        }

        #[unsafe(method(rightMouseUp:))]
        fn right_mouse_up(&self, event: &NSEvent) {
            self.emit_pointer(PointerPhase::Up, event, Some(PointerButton::Secondary));
        }

        #[unsafe(method(otherMouseDown:))]
        fn other_mouse_down(&self, event: &NSEvent) {
            self.emit_pointer(PointerPhase::Down, event, other_button(event));
        }

        #[unsafe(method(otherMouseDragged:))]
        fn other_mouse_dragged(&self, event: &NSEvent) {
            self.emit_pointer(PointerPhase::Move, event, None);
        }

        #[unsafe(method(otherMouseUp:))]
        fn other_mouse_up(&self, event: &NSEvent) {
            self.emit_pointer(PointerPhase::Up, event, other_button(event));
        }

        #[unsafe(method(scrollWheel:))]
        fn scroll_wheel(&self, event: &NSEvent) {
            let scale = if event.hasPreciseScrollingDeltas() {
                1.0
            } else {
                WHEEL_STEP
            };
            let wheel = PointerEvent::wheel(
                self.local_point(event),
                -event.scrollingDeltaX() * scale,
                -event.scrollingDeltaY() * scale,
            );
            if self.ivars().wheel.emit(wheel) == EventResponse::Continue {
                let _: () = unsafe { msg_send![super(self), scrollWheel: event] };
            }
        }
    }
);

impl CanvasView {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(CanvasState {
            draw: RefCell::new(None),
            pointer: ValueHandler::default(),
            wheel: WheelHandler::default(),
            tracking: RefCell::new(None),
        });
        let this: Retained<Self> = unsafe { msg_send![super(this), init] };
        // macOS 14 からは既定でクリップしないので、面の外へ描いた分がはみ出して
        // 見えてしまう (ほかのバックエンドは面の大きさで切れる)。
        this.setClipsToBounds(true);
        this
    }

    fn emit_pointer(&self, phase: PointerPhase, event: &NSEvent, button: Option<PointerButton>) {
        self.ivars()
            .pointer
            .emit(PointerEvent::new(phase, self.local_point(event)).button(button));
    }

    /// Converts an event's position into coordinates with the surface's top left as
    /// origin.
    fn local_point(&self, event: &NSEvent) -> naui_core::Point {
        let point = self.convertPoint_fromView(event.locationInWindow(), None);
        naui_core::Point::new(point.x, point.y)
    }
}

struct CanvasInner {
    native: Retained<NSView>,
    /// The same object as `native`. Held to reach the closure slots.
    view: Retained<CanvasView>,
}

/// A surface the app draws on itself (`NSView` + `drawRect:`).
///
/// Its size is not determined by its contents, so set it with `set_sizing` (the
/// same as `Scroll`).
#[derive(Clone)]
pub struct Canvas(Rc<CanvasInner>);
impl_widget!(Canvas);

impl Canvas {
    pub(crate) fn new(mtm: MainThreadMarker) -> Self {
        let view = CanvasView::new(mtm);
        let native = view.clone().into_super();
        // 中身が無いので、余りを受け取る側に回る (`Spacer` と同じ)。
        crate::layout::relax_hugging(&native);
        Self(Rc::new(CanvasInner { native, view }))
    }

    /// The closure that decides what to draw. Called with a fresh [`Painter`] every
    /// time it is redrawn.
    ///
    /// It is called after [`redraw`](Self::redraw), when the surface changes size (and
    /// when AppKit asks for a redraw).
    pub fn on_draw(&self, f: impl FnMut(&mut Painter) + 'static) {
        *self.0.view.ivars().draw.borrow_mut() = Some(Box::new(f));
        self.redraw();
    }

    /// Requests a redraw. Nothing is drawn on the spot; `on_draw` is called at the next
    /// draw.
    pub fn redraw(&self) {
        self.0.native.setNeedsDisplay(true);
    }

    /// Pointer (mouse) presses, moves and releases. Positions are logical pixels with
    /// the surface's top left as origin. Moves also arrive while nothing is pressed
    /// (hover). Presses and releases carry which button ([`PointerEvent::button`]).
    pub fn on_pointer(&self, f: impl FnMut(PointerEvent) + 'static) {
        self.0.view.ivars().pointer.set(f);
    }

    /// The wheel (including two-finger scrolling on a trackpad). Amounts are logical
    /// pixels.
    ///
    /// Returning `Handled` keeps it from reaching the outer scroll view. With
    /// `Continue`, the outer `NSScrollView` scrolls as usual.
    pub fn on_wheel(&self, f: impl FnMut(PointerEvent) -> EventResponse + 'static) {
        self.0.view.ivars().wheel.set(f);
    }

    /// The current size of the surface (width, height). 0 if not laid out yet.
    pub fn size(&self) -> (f64, f64) {
        let bounds = self.0.native.bounds();
        (bounds.size.width, bounds.size.height)
    }

    /// Draws on the spot. **For automated tests only.**
    ///
    /// Real apps do not use it, because AppKit decides when to draw.
    #[doc(hidden)]
    pub fn draw_for_test(&self) {
        self.0.native.displayIfNeeded();
    }
}

/// The button that came to `otherMouseDown:` / `otherMouseUp:`. Anything other than
/// left and right comes here.
fn other_button(event: &NSEvent) -> Option<PointerButton> {
    match event.buttonNumber() {
        2 => Some(PointerButton::Middle),
        3 => Some(PointerButton::Back),
        4 => Some(PointerButton::Forward),
        _ => None,
    }
}

/// Draws the commands recorded in the [`Painter`] into the current graphics
/// context.
///
/// The whole replay sits inside one saved graphics state, so a transform or clip
/// does not outlast `drawRect:`, and `Save` / `Restore` map onto
/// `saveGraphicsState` / `restoreGraphicsState`, which cover the transform and
/// the clip.
fn replay(commands: &[DrawCommand]) {
    NSGraphicsContext::saveGraphicsState_class();
    let mut depth = 0_usize;
    for command in commands {
        match command {
            DrawCommand::Fill {
                path,
                paint,
                opacity,
            } => {
                let bezier = to_bezier(path);
                match paint {
                    Paint::Color(color) => {
                        ns_color(*color, *opacity).setFill();
                        bezier.fill();
                    }
                    Paint::Gradient(gradient) => fill_gradient(&bezier, gradient, *opacity),
                }
            }
            DrawCommand::Stroke {
                path,
                color,
                width,
                dash,
                opacity,
            } => {
                let bezier = to_bezier(path);
                bezier.setLineWidth(*width);
                bezier.setLineCapStyle(NSLineCapStyle::Butt);
                bezier.setLineJoinStyle(NSLineJoinStyle::Miter);
                if !dash.is_empty() {
                    // SAFETY: `dash` は呼び出しの間生きている配列で、長さも一緒に渡す。
                    unsafe {
                        bezier.setLineDash_count_phase(dash.as_ptr(), dash.len() as isize, 0.0)
                    };
                }
                ns_color(*color, *opacity).setStroke();
                bezier.stroke();
            }
            DrawCommand::Text {
                text,
                at,
                size,
                color,
                align,
                opacity,
            } => draw_text(text, *at, *size, *color, *align, *opacity),
            DrawCommand::Image {
                bitmap,
                source,
                dest,
                opacity,
            } => draw_image(bitmap, *source, *dest, *opacity),
            DrawCommand::Save => {
                NSGraphicsContext::saveGraphicsState_class();
                depth += 1;
            }
            DrawCommand::Restore => {
                if depth > 0 {
                    NSGraphicsContext::restoreGraphicsState_class();
                    depth -= 1;
                }
            }
            DrawCommand::Transform(t) => {
                let transform = NSAffineTransform::transform();
                transform.setTransformStruct(NSAffineTransformStruct {
                    m11: t.a,
                    m12: t.b,
                    m21: t.c,
                    m22: t.d,
                    tX: t.e,
                    tY: t.f,
                });
                // 今の変換の前に掛ける (`<canvas>` の transform と同じ向き)。
                transform.concat();
            }
            DrawCommand::Clip(path) => to_bezier(path).addClip(),
        }
    }
    for _ in 0..=depth {
        NSGraphicsContext::restoreGraphicsState_class();
    }
}

/// Fills `bezier` with a gradient: clips to it, then draws the gradient over the
/// clip. `NSGradient` stops at its ends unless told to carry the end colors on,
/// which the other three platforms do.
fn fill_gradient(bezier: &NSBezierPath, gradient: &Gradient, opacity: f64) {
    let stops = gradient.stops();
    let colors: Vec<Retained<NSColor>> = stops
        .iter()
        .map(|stop| ns_color(stop.color, stop.opacity * opacity))
        .collect();
    let colors = NSArray::from_retained_slice(&colors);
    let locations: Vec<f64> = stops.iter().map(|stop| stop.offset).collect();
    // SAFETY: 位置の配列は色と同じ長さで、呼び出しの間生きている。
    let Some(ns_gradient) = (unsafe {
        NSGradient::initWithColors_atLocations_colorSpace(
            NSGradient::alloc(),
            &colors,
            locations.as_ptr(),
            &NSColorSpace::sRGBColorSpace(),
        )
    }) else {
        return;
    };
    let options = NSGradientDrawingOptions::DrawsBeforeStartingLocation
        | NSGradientDrawingOptions::DrawsAfterEndingLocation;
    NSGraphicsContext::saveGraphicsState_class();
    bezier.addClip();
    match gradient.shape() {
        GradientShape::Linear { start, end } => {
            ns_gradient.drawFromPoint_toPoint_options(ns_point(start), ns_point(end), options)
        }
        GradientShape::Radial { center, radius } => ns_gradient
            .drawFromCenter_radius_toCenter_radius_options(
                ns_point(center),
                0.0,
                ns_point(center),
                radius,
                options,
            ),
    }
    NSGraphicsContext::restoreGraphicsState_class();
}

/// The `NSImage` made from a [`Bitmap`], kept with it. `None` when AppKit cannot
/// read it.
struct NativeBitmap(Option<Retained<NSImage>>);

/// Draws the `source` part of the image over `dest`.
///
/// The image's size is set to its pixel size, so its points are its pixels (a PNG
/// saying 144 dpi would otherwise be half as many points). `fromRect:` is in the
/// image's own coordinates, whose origin is the bottom left, so `source` (from the
/// top left) is turned over. `respectFlipped:` draws it upright in this flipped
/// view.
fn draw_image(bitmap: &Bitmap, source: Rect, dest: Rect, opacity: f64) {
    let native = bitmap.native(|bitmap| {
        let image = match bitmap.data() {
            BitmapData::Encoded(bytes) => Some(bytes.to_vec()),
            // 画素は PNG に包み、ファイルと同じく sRGB として読ませる (`Image` と同じ)。
            BitmapData::Pixels(rgba) => {
                naui_core::media::encode_png(bitmap.width(), bitmap.height(), rgba).ok()
            }
        }
        .and_then(|bytes| NSImage::initWithData(NSImage::alloc(), &NSData::with_bytes(&bytes)));
        if let Some(image) = &image {
            image.setSize(NSSize::new(
                f64::from(bitmap.width()),
                f64::from(bitmap.height()),
            ));
        }
        NativeBitmap(image)
    });
    let Some(image) = &native.0 else {
        return;
    };
    let from = NSRect::new(
        NSPoint::new(source.x, f64::from(bitmap.height()) - source.bottom()),
        NSSize::new(source.width, source.height),
    );
    let to = NSRect::new(
        NSPoint::new(dest.x, dest.y),
        NSSize::new(dest.width, dest.height),
    );
    // SAFETY: ヒントは渡さない。
    unsafe {
        image.drawInRect_fromRect_operation_fraction_respectFlipped_hints(
            to,
            from,
            NSCompositingOperation::SourceOver,
            opacity,
            true,
            None,
        )
    };
}

fn to_bezier(path: &Path) -> Retained<NSBezierPath> {
    let bezier = NSBezierPath::bezierPath();
    for segment in path.segments() {
        match *segment {
            PathSegment::MoveTo(p) => bezier.moveToPoint(ns_point(p)),
            PathSegment::LineTo(p) => bezier.lineToPoint(ns_point(p)),
            PathSegment::CubicTo {
                control1,
                control2,
                to,
            } => bezier.curveToPoint_controlPoint1_controlPoint2(
                ns_point(to),
                ns_point(control1),
                ns_point(control2),
            ),
            PathSegment::Close => bezier.closePath(),
        }
    }
    bezier
}

/// The attributes text is drawn and measured with: the system font at `size`, and
/// the color when there is one.
fn text_attributes(
    size: f64,
    fill: Option<&NSColor>,
) -> Retained<NSDictionary<NSAttributedStringKey, AnyObject>> {
    let font = NSFont::systemFontOfSize(size);
    let font_object: &AnyObject = font.as_ref();
    match fill {
        Some(fill) => {
            let color_object: &AnyObject = fill.as_ref();
            NSDictionary::from_slices(
                &[unsafe { NSFontAttributeName }, unsafe {
                    NSForegroundColorAttributeName
                }],
                &[font_object, color_object],
            )
        }
        None => NSDictionary::from_slices(&[unsafe { NSFontAttributeName }], &[font_object]),
    }
}

/// The width of one line of text as [`draw_text`] draws it.
fn text_width(text: &str, size: f64) -> f64 {
    let string = NSString::from_str(text);
    let attributes = text_attributes(size, None);
    let measured: NSSize = unsafe { string.sizeWithAttributes(Some(&attributes)) };
    measured.width
}

/// Draws one line of text. `at` is the top, and the horizontal position follows
/// `align`.
///
/// In an `isFlipped` view, the point given to `drawAtPoint:` is the text's top left,
/// so only the width is measured for alignment.
fn draw_text(
    text: &str,
    at: naui_core::Point,
    size: f64,
    color: Color,
    align: Align,
    opacity: f64,
) {
    let string = NSString::from_str(text);
    let fill = ns_color(color, opacity);
    let attributes = text_attributes(size, Some(&fill));
    let x = match align {
        Align::Start | Align::Fill => at.x,
        Align::Center | Align::End => {
            let measured: NSSize = unsafe { string.sizeWithAttributes(Some(&attributes)) };
            if matches!(align, Align::Center) {
                at.x - measured.width / 2.0
            } else {
                at.x - measured.width
            }
        }
    };
    unsafe { string.drawAtPoint_withAttributes(NSPoint::new(x, at.y), Some(&attributes)) };
}

fn ns_color(color: Color, opacity: f64) -> Retained<NSColor> {
    let (r, g, b) = color.to_unit();
    NSColor::colorWithSRGBRed_green_blue_alpha(r, g, b, opacity)
}

fn ns_point(p: naui_core::Point) -> NSPoint {
    NSPoint::new(p.x, p.y)
}
