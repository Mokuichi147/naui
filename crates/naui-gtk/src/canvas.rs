//! The drawing surface (`GtkDrawingArea` + cairo).
//!
//! In GTK4, "a surface the app draws itself" is a `GtkDrawingArea`, whose draw
//! function receives a cairo context. naui calls the app's `on_draw` inside it and
//! replays the commands recorded in the [`Painter`] with cairo's `move_to` /
//! `line_to` / `curve_to` / `fill` / `stroke` and Pango layouts (`show_layout`).
//! cairo and Pango do the rasterization, antialiasing, HiDPI scaling and text
//! shaping.
//!
//! Text is created from the widget's Pango context, so the typeface is the theme's
//! UI font, with only the size replaced by the command's `size`.
//!
//! Pointer notifications come from three controllers. `GtkGestureClick` gives
//! presses and releases (the button comes from `current_button`: 1 for left, 2 for
//! middle, 3 for right, 8 and 9 for the side buttons), `GtkGestureDrag` gives
//! **movement while pressed** (delivered even outside the surface), and
//! `GtkEventControllerMotion` gives movement while not pressed (hover, delivered
//! only while inside the surface).
//!
//! The wheel comes from `GtkEventControllerScroll`. The unit of the amount is given
//! by `gtk_event_controller_scroll_get_unit`: notched wheels
//! (`GDK_SCROLL_UNIT_WHEEL`, 1 per notch) are multiplied by [`WHEEL_STEP`], and
//! trackpads (`GDK_SCROLL_UNIT_SURFACE`) are delivered in logical pixels as is.
//! Scroll notifications carry no position, so the last position seen while
//! hovering is used. When the app returns `Continue`, propagation is not stopped
//! and the outer `GtkScrolledWindow` scrolls as usual.
//!
//! Stopping propagation with `Handled` does not prevent the outer
//! `GtkScrolledWindow`'s momentum. The outer one also accumulates a history of
//! amounts with a capture-phase controller (which runs before its children), and
//! starts momentum scrolling from that history when the fingers lift (the stop
//! event). The stop event then reaches the Canvas too, so if the last amount was
//! received with `Handled`, the Canvas's `scroll-end` cancels the outer momentum
//! ([`cancel_ancestor_kinetic`]).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::cairo;
use gtk::pango;
use gtk::prelude::*;
use naui_core::{
    Align, Bitmap, BitmapData, DrawCommand, EventResponse, Gradient, GradientShape, Paint, Painter,
    Path, PathSegment, Point, PointerButton, PointerEvent, PointerPhase, Rect, WheelHandler,
    WHEEL_STEP,
};

use crate::bin::SizeBin;
use crate::callback::Notifier;
use crate::widgets::{impl_widget, Widget};

/// The slot for the closure that decides what to draw. Taken out while it is being
/// called (the same rule as [`Notifier`]).
type DrawSlot = RefCell<Option<Box<dyn FnMut(&mut Painter)>>>;

struct CanvasInner {
    native: gtk::DrawingArea,
    bin: SizeBin,
    draw: Rc<DrawSlot>,
    pointer: Rc<Notifier<PointerEvent>>,
    wheel: Rc<WheelHandler>,
}

/// A surface the app draws on itself (`GtkDrawingArea`).
///
/// Its size is not determined by its contents, so set it with `set_sizing` (the
/// same as `Scroll`).
#[derive(Clone)]
pub struct Canvas(Rc<CanvasInner>);
impl_widget!(Canvas);

impl Canvas {
    pub(crate) fn new() -> Self {
        let native = gtk::DrawingArea::new();
        let bin = SizeBin::wrap(&native);
        let draw: Rc<DrawSlot> = Rc::new(RefCell::new(None));
        let pointer = Rc::new(Notifier::default());
        let wheel = Rc::new(WheelHandler::default());

        native.set_draw_func({
            let draw = draw.clone();
            move |area, cr, width, height| {
                let measured = area.clone();
                let mut painter = Painter::with_text_measure(
                    f64::from(width),
                    f64::from(height),
                    move |text, size| text_width(&text_layout(&measured, text, size)),
                );
                let Some(mut f) = draw.borrow_mut().take() else {
                    return;
                };
                f(&mut painter);
                let mut slot = draw.borrow_mut();
                if slot.is_none() {
                    *slot = Some(f);
                }
                drop(slot);
                replay(area, cr, &painter.into_commands());
            }
        });

        // 押下と解放。`pressed` は押した瞬間に届き (ドラッグの判定を待たない)、
        // `released` は面の外で離しても届く。どのボタンでも受け、`current_button`
        // で見分ける。
        let click = gtk::GestureClick::new();
        click.set_button(0);
        click.connect_pressed({
            let pointer = pointer.clone();
            move |gesture, _, x, y| {
                pointer.emit(
                    PointerEvent::new(PointerPhase::Down, Point::new(x, y))
                        .button(button_from_gdk(gesture.current_button())),
                )
            }
        });
        click.connect_released({
            let pointer = pointer.clone();
            move |gesture, _, x, y| {
                pointer.emit(
                    PointerEvent::new(PointerPhase::Up, Point::new(x, y))
                        .button(button_from_gdk(gesture.current_button())),
                )
            }
        });
        native.add_controller(click);

        // 押している間の移動。`drag-update` は始点からのずれで届き、面の外へ
        // 出ても (暗黙のグラブが続くので) 止まらない。
        let dragging = Rc::new(Cell::new(false));
        let drag = gtk::GestureDrag::new();
        drag.set_button(0);
        drag.connect_drag_begin({
            let dragging = dragging.clone();
            move |_, _, _| dragging.set(true)
        });
        drag.connect_drag_update({
            let pointer = pointer.clone();
            move |gesture, dx, dy| {
                let (sx, sy) = gesture.start_point().unwrap_or((0.0, 0.0));
                pointer.emit(PointerEvent::new(
                    PointerPhase::Move,
                    Point::new(sx + dx, sy + dy),
                ));
            }
        });
        drag.connect_drag_end({
            let dragging = dragging.clone();
            move |_, _, _| dragging.set(false)
        });
        // 親のスクロールなどにシーケンスを奪われると `drag-end` は来ず
        // `cancel` になる。ここでも戻さないと、ホバーが止まったままになる。
        drag.connect_cancel({
            let dragging = dragging.clone();
            move |_, _| dragging.set(false)
        });
        native.add_controller(drag);

        // 押していない間の移動 (ホバー)。`motion` は面の中にいる間しか出ないので、
        // 押している間は上の `drag-update` に任せて二重に出さない。
        // ホイールの位置にも使うので、最後に見た位置を覚えておく。
        let hover = Rc::new(Cell::new(Point::default()));
        let motion = gtk::EventControllerMotion::new();
        motion.connect_enter({
            let hover = hover.clone();
            move |_, x, y| hover.set(Point::new(x, y))
        });
        motion.connect_motion({
            let pointer = pointer.clone();
            let dragging = dragging.clone();
            let hover = hover.clone();
            move |_, x, y| {
                hover.set(Point::new(x, y));
                if !dragging.get() {
                    pointer.emit(PointerEvent::new(PointerPhase::Move, Point::new(x, y)));
                }
            }
        });
        native.add_controller(motion);

        // ホイール。縦と横の両方を受ける。慣性 (`KINETIC`) は付けず、OS が
        // 出した量だけを届ける。
        // 最後の量を `Handled` で受けたかを覚えておき、指を離したときに
        // 外側の慣性を取り消すのに使う。
        let last_handled = Rc::new(Cell::new(false));
        let scroll = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::BOTH_AXES);
        scroll.connect_scroll({
            let wheel = wheel.clone();
            let hover = hover.clone();
            let last_handled = last_handled.clone();
            move |controller, dx, dy| {
                let scale = match controller.unit() {
                    gtk::gdk::ScrollUnit::Surface => 1.0,
                    _ => WHEEL_STEP,
                };
                let event = PointerEvent::wheel(hover.get(), dx * scale, dy * scale);
                let response = wheel.emit(event);
                last_handled.set(response == EventResponse::Handled);
                match response {
                    EventResponse::Handled => gtk::glib::Propagation::Stop,
                    EventResponse::Continue => gtk::glib::Propagation::Proceed,
                }
            }
        });
        scroll.connect_scroll_begin({
            let last_handled = last_handled.clone();
            move |_| last_handled.set(false)
        });
        // 止まりのイベントは外側の捕捉フェーズで慣性を始めさせた後にここへ
        // 届く。最後を `Continue` で外側へ回していたなら、慣性はそのまま残す。
        scroll.connect_scroll_end(move |controller| {
            if last_handled.replace(false) {
                if let Some(widget) = controller.widget() {
                    cancel_ancestor_kinetic(&widget);
                }
            }
        });
        native.add_controller(scroll);

        Self(Rc::new(CanvasInner {
            native,
            bin,
            draw,
            pointer,
            wheel,
        }))
    }

    /// The closure that decides what to draw. Called with a fresh [`Painter`] every
    /// time it is redrawn.
    ///
    /// It is called after [`redraw`](Self::redraw), when the surface changes size (and
    /// when GTK asks for a redraw).
    pub fn on_draw(&self, f: impl FnMut(&mut Painter) + 'static) {
        *self.0.draw.borrow_mut() = Some(Box::new(f));
        self.redraw();
    }

    /// Requests a redraw. Nothing is drawn on the spot; `on_draw` is called at the next
    /// draw.
    pub fn redraw(&self) {
        self.0.native.queue_draw();
    }

    /// Pointer (mouse, touch, pen) presses, moves and releases. Positions are logical
    /// pixels with the surface's top left as origin. Moves also arrive while nothing is
    /// pressed (hover). Presses and releases carry which button
    /// ([`PointerEvent::button`]).
    pub fn on_pointer(&self, f: impl FnMut(PointerEvent) + 'static) {
        self.0.pointer.set(f);
    }

    /// The wheel (including two-finger scrolling on a trackpad). Amounts are logical
    /// pixels.
    ///
    /// Returning `Handled` keeps it from reaching the outer scroll view (and no
    /// momentum follows after the fingers lift). With `Continue`, the outer
    /// `GtkScrolledWindow` scrolls as usual.
    pub fn on_wheel(&self, f: impl FnMut(PointerEvent) -> EventResponse + 'static) {
        self.0.wheel.set(f);
    }

    /// The current size of the surface (width, height). 0 if not laid out yet.
    pub fn size(&self) -> (f64, f64) {
        (
            f64::from(self.0.native.width()),
            f64::from(self.0.native.height()),
        )
    }

    /// The drawing surface itself. Public as a backend-specific escape hatch.
    pub fn native_area(&self) -> gtk::DrawingArea {
        self.0.native.clone()
    }
}

/// Cancels momentum scrolling started by an outer `GtkScrolledWindow`.
///
/// There is no public API to stop momentum, but turning off `kinetic-scrolling`
/// also stops momentum in progress (`cancel_deceleration`). It is turned back on
/// immediately, so the setting does not change. Nested scroll views all accumulate
/// history, so every ancestor is visited.
fn cancel_ancestor_kinetic(widget: &gtk::Widget) {
    let mut next = widget.parent();
    while let Some(ancestor) = next {
        if let Some(scrolled) = ancestor.downcast_ref::<gtk::ScrolledWindow>() {
            if scrolled.is_kinetic_scrolling() {
                scrolled.set_kinetic_scrolling(false);
                scrolled.set_kinetic_scrolling(true);
            }
        }
        next = ancestor.parent();
    }
}

/// Converts a GDK button number into a button.
///
/// GTK passes touch as 1 (the primary button). 0 (no button) and unknown numbers
/// give `None`.
fn button_from_gdk(button: u32) -> Option<PointerButton> {
    match button {
        gtk::gdk::BUTTON_PRIMARY => Some(PointerButton::Primary),
        gtk::gdk::BUTTON_MIDDLE => Some(PointerButton::Middle),
        gtk::gdk::BUTTON_SECONDARY => Some(PointerButton::Secondary),
        8 => Some(PointerButton::Back),
        9 => Some(PointerButton::Forward),
        _ => None,
    }
}

/// Draws the commands recorded in the [`Painter`] with cairo.
///
/// The whole replay sits inside one `save`, so a transform or clip does not outlast
/// the draw function, and `Save` / `Restore` map onto cairo's, which cover the
/// transform and the clip.
fn replay(area: &gtk::DrawingArea, cr: &cairo::Context, commands: &[DrawCommand]) {
    let _ = cr.save();
    let mut depth = 0_usize;
    for command in commands {
        match command {
            DrawCommand::Fill {
                path,
                paint,
                opacity,
            } => {
                trace(cr, path);
                match paint {
                    Paint::Color(color) => {
                        let (r, g, b) = color.to_unit();
                        cr.set_source_rgba(r, g, b, *opacity);
                    }
                    Paint::Gradient(gradient) => {
                        let _ = cr.set_source(gradient_pattern(gradient, *opacity));
                    }
                }
                cr.set_fill_rule(cairo::FillRule::Winding);
                let _ = cr.fill();
            }
            DrawCommand::Stroke {
                path,
                color,
                width,
                dash,
                opacity,
            } => {
                trace(cr, path);
                let (r, g, b) = color.to_unit();
                cr.set_source_rgba(r, g, b, *opacity);
                cr.set_line_width(*width);
                cr.set_line_cap(cairo::LineCap::Butt);
                cr.set_line_join(cairo::LineJoin::Miter);
                cr.set_dash(dash, 0.0);
                let _ = cr.stroke();
            }
            DrawCommand::Text {
                text,
                at,
                size,
                color,
                align,
                opacity,
            } => {
                let layout = text_layout(area, text, *size);
                let width = text_width(&layout);
                let x = match align {
                    Align::Center => at.x - width / 2.0,
                    Align::End => at.x - width,
                    Align::Start | Align::Fill => at.x,
                };
                let (r, g, b) = color.to_unit();
                cr.set_source_rgba(r, g, b, *opacity);
                cr.move_to(x, at.y);
                pangocairo::functions::show_layout(cr, &layout);
                cr.new_path();
            }
            DrawCommand::Image {
                bitmap,
                source,
                dest,
                opacity,
            } => draw_image(cr, bitmap, *source, *dest, *opacity),
            DrawCommand::Save => {
                let _ = cr.save();
                depth += 1;
            }
            DrawCommand::Restore => {
                if depth > 0 {
                    let _ = cr.restore();
                    depth -= 1;
                }
            }
            DrawCommand::Transform(t) => {
                // 今の変換の前に掛ける (`<canvas>` の transform と同じ向き)。
                cr.transform(cairo::Matrix::new(t.a, t.b, t.c, t.d, t.e, t.f));
            }
            DrawCommand::Clip(path) => {
                trace(cr, path);
                cr.set_fill_rule(cairo::FillRule::Winding);
                cr.clip();
            }
        }
    }
    for _ in 0..=depth {
        let _ = cr.restore();
    }
}

/// A Pango layout of one line of text in the theme's UI font at `size`.
fn text_layout(area: &gtk::DrawingArea, text: &str, size: f64) -> pango::Layout {
    let layout = area.create_pango_layout(Some(text));
    let mut font = area.pango_context().font_description().unwrap_or_default();
    font.set_absolute_size(size * f64::from(pango::SCALE));
    layout.set_font_description(Some(&font));
    layout
}

/// The logical width of a layout, in logical pixels (not rounded to whole pixels).
fn text_width(layout: &pango::Layout) -> f64 {
    f64::from(layout.size().0) / f64::from(pango::SCALE)
}

/// A cairo gradient carrying the end colors on past its ends (`EXTEND_PAD`, as on
/// the other three platforms), with `opacity` applied to every stop.
fn gradient_pattern(gradient: &Gradient, opacity: f64) -> cairo::Gradient {
    let pattern: cairo::Gradient = match gradient.shape() {
        GradientShape::Linear { start, end } => {
            (*cairo::LinearGradient::new(start.x, start.y, end.x, end.y)).clone()
        }
        GradientShape::Radial { center, radius } => {
            (*cairo::RadialGradient::new(center.x, center.y, 0.0, center.x, center.y, radius))
                .clone()
        }
    };
    for stop in gradient.stops() {
        let (r, g, b) = stop.color.to_unit();
        pattern.add_color_stop_rgba(stop.offset, r, g, b, stop.opacity * opacity);
    }
    pattern.set_extend(cairo::Extend::Pad);
    pattern
}

/// The cairo surface made from a [`Bitmap`], kept with it. `None` when GDK cannot
/// read it.
struct NativeBitmap(Option<cairo::ImageSurface>);

/// Decodes a bitmap into cairo's `ARGB32` (premultiplied, native-endian words).
///
/// Files are read by `gdk::Texture::from_bytes`, as `Image::set_bytes` reads them,
/// and downloaded in GDK's default memory format, which is the same layout.
fn decode(bitmap: &Bitmap) -> Option<cairo::ImageSurface> {
    let (width, height, pixels) = match bitmap.data() {
        BitmapData::Encoded(bytes) => {
            let texture = gtk::gdk::Texture::from_bytes(&gtk::glib::Bytes::from(bytes))
                .map_err(|error| eprintln!("naui-gtk: could not read the image: {error}"))
                .ok()?;
            let (width, height) = (texture.width(), texture.height());
            let stride = usize::try_from(width).ok()? * 4;
            let mut pixels = vec![0; stride * usize::try_from(height).ok()?];
            texture.download(&mut pixels, stride);
            (width, height, pixels)
        }
        BitmapData::Pixels(rgba) => (
            i32::try_from(bitmap.width()).ok()?,
            i32::try_from(bitmap.height()).ok()?,
            naui_core::media::rgba_to_premultiplied_argb32(rgba),
        ),
    };
    cairo::ImageSurface::create_for_data(pixels, cairo::Format::ARgb32, width, height, width * 4)
        .ok()
}

/// Draws the `source` part of the image over `dest`: the surface is scaled so that
/// `source` covers `dest`, and painted within `dest`.
///
/// The edges are padded rather than faded to transparent, so a stretched image
/// keeps sharp borders (as `drawImage` does).
fn draw_image(cr: &cairo::Context, bitmap: &Bitmap, source: Rect, dest: Rect, opacity: f64) {
    let native = bitmap.native(|bitmap| NativeBitmap(decode(bitmap)));
    let Some(surface) = &native.0 else {
        return;
    };
    let _ = cr.save();
    cr.rectangle(dest.x, dest.y, dest.width, dest.height);
    cr.clip();
    cr.translate(dest.x, dest.y);
    cr.scale(dest.width / source.width, dest.height / source.height);
    if cr.set_source_surface(surface, -source.x, -source.y).is_ok() {
        cr.source().set_extend(cairo::Extend::Pad);
        let _ = cr.paint_with_alpha(opacity);
    }
    let _ = cr.restore();
}

fn trace(cr: &cairo::Context, path: &Path) {
    cr.new_path();
    for segment in path.segments() {
        match *segment {
            PathSegment::MoveTo(p) => cr.move_to(p.x, p.y),
            PathSegment::LineTo(p) => cr.line_to(p.x, p.y),
            PathSegment::CubicTo {
                control1,
                control2,
                to,
            } => cr.curve_to(control1.x, control1.y, control2.x, control2.y, to.x, to.y),
            PathSegment::Close => cr.close_path(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gdk_buttons_map_by_meaning() {
        // GDK は右が 3 で中が 2 (Web や AppKit と並びが違う)。
        assert_eq!(button_from_gdk(1), Some(PointerButton::Primary));
        assert_eq!(button_from_gdk(2), Some(PointerButton::Middle));
        assert_eq!(button_from_gdk(3), Some(PointerButton::Secondary));
        assert_eq!(button_from_gdk(8), Some(PointerButton::Back));
        assert_eq!(button_from_gdk(9), Some(PointerButton::Forward));
        assert_eq!(button_from_gdk(0), None, "どのボタンでもない");
        assert_eq!(button_from_gdk(4), None);
    }
}
