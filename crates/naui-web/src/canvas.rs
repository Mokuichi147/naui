//! The drawing surface (`<canvas>` + a 2D context).
//!
//! In the browser, "a surface the app draws itself" is the `<canvas>` itself. naui
//! just replays the commands `on_draw` recorded in the [`Painter`] with
//! `CanvasRenderingContext2D`'s `moveTo` / `lineTo` / `bezierCurveTo` / `fill` /
//! `stroke` / `fillText`; rasterization, antialiasing and text drawing are done by
//! the browser.
//!
//! On high-resolution displays (`devicePixelRatio` > 1), the backing pixel count is
//! multiplied by the ratio and `scale` is applied before drawing. The coordinates
//! the app sees stay in CSS pixels.
//!
//! The surface's size is decided by CSS layout (`set_sizing`). Size changes are
//! caught with `ResizeObserver`, redrawing each time. Changes in only the ratio
//! while the CSS size stays the same (moving to another monitor, changing the
//! browser zoom) are not visible to `ResizeObserver`, so they are caught with the
//! `change` of a `matchMedia("(resolution: …dppx)")` conditioned on the current
//! ratio. Redraw requests (`redraw`) are batched into `requestAnimationFrame` and
//! not drawn on the spot (the same rule as the other three platforms).
//!
//! The button is identified by `PointerEvent.button`. Browsers report presses and
//! releases of second and later buttons (pressing right while holding left, for
//! example) through `pointermove` rather than `pointerdown` / `pointerup` (a
//! `pointermove` whose `button` is not -1), so those are delivered as presses and
//! releases too. The browser's context menu on right-click is not shown over the
//! surface (the drawing surfaces of the other three platforms have none either).
//!
//! The wheel comes from `wheel` events. When `deltaMode` is lines (as in older
//! Firefox defaults), one line becomes 1/3 of [`WHEEL_STEP`]; when pages, the
//! surface's size. For pixels the browser's amount is delivered as is, so the amount
//! per notch of a notched wheel differs between browsers. Only when the app returns
//! `Handled` is `preventDefault` called to stop page scrolling.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use naui_core::{
    Align, DrawCommand, EventResponse, Painter, Path, PathSegment, Point, PointerButton,
    PointerEvent, PointerPhase, Result, WheelHandler, WHEEL_STEP,
};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{
    CanvasRenderingContext2d, Document, Element, HtmlCanvasElement, HtmlElement, ResizeObserver,
};

use crate::to_error;
use crate::widgets::{create, impl_widget, Listener, ValueHandler, Widget};

/// The slot for the closure that decides what to draw. Taken out while it is being
/// called.
type DrawSlot = RefCell<Option<Box<dyn FnMut(&mut Painter)>>>;
/// The slot for closures passed to the JS side. Held with the same lifetime as the
/// handle.
type JsSlot<F> = RefCell<Option<Closure<F>>>;

struct CanvasInner {
    canvas: HtmlCanvasElement,
    context: CanvasRenderingContext2d,
    draw: DrawSlot,
    pointer: ValueHandler<PointerEvent>,
    wheel: WheelHandler,
    /// Whether a `requestAnimationFrame` is already scheduled.
    scheduled: Cell<bool>,
    /// The closure that runs the scheduled draw. Created once and reused.
    frame: JsSlot<dyn FnMut()>,
    /// The subscription to size changes. Dropping it also unsubscribes.
    observer: RefCell<Option<ResizeObserver>>,
    _observer_callback: JsSlot<dyn FnMut(JsValue)>,
    _listeners: RefCell<Vec<Listener>>,
    /// The ratio at the last draw, and the subscription reporting a departure from that
    /// ratio.
    ///
    /// When the ratio changes the condition stops matching and `change` fires, so it is
    /// re-attached with the new ratio each time.
    ratio_watch: RefCell<Option<(f64, Listener)>>,
    /// `on_file_drop`.
    file_drop: crate::file_drop::FileDrop,
}

/// A surface the app draws on itself (`<canvas>`).
///
/// Its size is not determined by its contents, so set it with `set_sizing` (the
/// same as `Scroll`).
#[derive(Clone)]
pub struct Canvas(Rc<CanvasInner>);
impl_widget!(Canvas, canvas);

impl Canvas {
    pub(crate) fn new(document: &Document) -> Result<Self> {
        let canvas: HtmlCanvasElement = create(document, "canvas")?.unchecked_into();
        // インライン要素の行送りのすき間を作らない。
        let style = canvas.style();
        let _ = style.set_property("display", "block");
        // タッチでのスクロールに取られず、ポインターの動きを全部受ける。
        let _ = style.set_property("touch-action", "none");
        let context = canvas
            .get_context("2d")
            .map_err(|e| to_error("get the 2D context", e))?
            .ok_or_else(|| naui_core::Error::new("get the 2D context", "not supported"))?
            .dyn_into::<CanvasRenderingContext2d>()
            .map_err(|e| to_error("get the 2D context", JsValue::from(e)))?;

        let inner = Rc::new(CanvasInner {
            canvas,
            context,
            draw: RefCell::new(None),
            pointer: ValueHandler::default(),
            wheel: WheelHandler::default(),
            scheduled: Cell::new(false),
            frame: RefCell::new(None),
            observer: RefCell::new(None),
            _observer_callback: RefCell::new(None),
            _listeners: RefCell::new(Vec::new()),
            ratio_watch: RefCell::new(None),
            file_drop: Default::default(),
        });

        // 予約した描画。
        let frame = Closure::<dyn FnMut()>::new({
            let weak = Rc::downgrade(&inner);
            move || {
                if let Some(inner) = weak.upgrade() {
                    inner.scheduled.set(false);
                    inner.paint();
                }
            }
        });
        *inner.frame.borrow_mut() = Some(frame);

        // 大きさが変わったら描き直す。載せた直後にも 1 回呼ばれるので、
        // 最初の描画もここから始まる。
        let observer_callback = Closure::<dyn FnMut(JsValue)>::new({
            let weak = Rc::downgrade(&inner);
            move |_entries: JsValue| {
                if let Some(inner) = weak.upgrade() {
                    inner.paint();
                }
            }
        });
        let observer = ResizeObserver::new(observer_callback.as_ref().unchecked_ref())
            .map_err(|e| to_error("create the ResizeObserver", e))?;
        observer.observe(inner.canvas.as_ref());
        *inner.observer.borrow_mut() = Some(observer);
        *inner._observer_callback.borrow_mut() = Some(observer_callback);

        let this = Self(inner);
        this.track_pointer()?;
        Ok(this)
    }

    fn track_pointer(&self) -> Result<()> {
        let target: &web_sys::EventTarget = self.0.canvas.as_ref();
        let mut listeners = Vec::new();
        for (name, phase) in [
            ("pointerdown", PointerPhase::Down),
            ("pointermove", PointerPhase::Move),
            ("pointerup", PointerPhase::Up),
        ] {
            let weak = Rc::downgrade(&self.0);
            listeners.push(Listener::attach_event(target, name, move |event| {
                let (Some(inner), Ok(event)) =
                    (weak.upgrade(), event.dyn_into::<web_sys::PointerEvent>())
                else {
                    return;
                };
                let button = event.button();
                // 2 つ目以降のボタンは `pointermove` で届く。押されたボタンが
                // `buttons` に残っていれば押下、消えていれば解放。
                let bit = web_buttons_bit(button);
                let phase = match phase {
                    PointerPhase::Move if bit != 0 => {
                        if event.buttons() & bit != 0 {
                            PointerPhase::Down
                        } else {
                            PointerPhase::Up
                        }
                    }
                    phase => phase,
                };
                // 押している間は面の外へ出ても動きと解放が届くようにする。
                if phase == PointerPhase::Down {
                    let _ = inner.canvas.set_pointer_capture(event.pointer_id());
                }
                let mut pointer = PointerEvent::new(phase, inner.local_point(&event));
                if phase != PointerPhase::Move {
                    pointer = pointer.button(PointerButton::from_web_button(button));
                }
                inner.pointer.emit(pointer);
            })?);
        }

        let weak = Rc::downgrade(&self.0);
        listeners.push(Listener::attach_event(target, "wheel", move |event| {
            let (Some(inner), Ok(event)) =
                (weak.upgrade(), event.dyn_into::<web_sys::WheelEvent>())
            else {
                return;
            };
            // 量を先に読む (Firefox は `deltaMode` を先に読むと行で返す)。
            let (dx, dy) = (event.delta_x(), event.delta_y());
            let (width, height) = inner.css_size();
            let (scale_x, scale_y) = match event.delta_mode() {
                web_sys::WheelEvent::DOM_DELTA_LINE => (WHEEL_STEP / 3.0, WHEEL_STEP / 3.0),
                web_sys::WheelEvent::DOM_DELTA_PAGE => (width, height),
                _ => (1.0, 1.0),
            };
            let wheel = PointerEvent::wheel(inner.local_point(&event), dx * scale_x, dy * scale_y);
            if inner.wheel.emit(wheel) == EventResponse::Handled {
                event.prevent_default();
            }
        })?);

        // 右クリックでブラウザのコンテキストメニューを出さない。
        listeners.push(Listener::attach_event(target, "contextmenu", |event| {
            event.prevent_default();
        })?);
        *self.0._listeners.borrow_mut() = listeners;
        Ok(())
    }

    /// The closure that decides what to draw. Called with a fresh [`Painter`] every
    /// time it is redrawn.
    ///
    /// It is called after [`redraw`](Self::redraw) and when the surface changes size.
    pub fn on_draw(&self, f: impl FnMut(&mut Painter) + 'static) {
        *self.0.draw.borrow_mut() = Some(Box::new(f));
        self.redraw();
    }

    /// Requests a redraw. Nothing is drawn on the spot; `on_draw` is called on the next
    /// frame.
    pub fn redraw(&self) {
        if self.0.scheduled.replace(true) {
            return;
        }
        let frame = self.0.frame.borrow();
        let (Some(window), Some(frame)) = (web_sys::window(), frame.as_ref()) else {
            self.0.scheduled.set(false);
            return;
        };
        if window
            .request_animation_frame(frame.as_ref().unchecked_ref())
            .is_err()
        {
            self.0.scheduled.set(false);
        }
    }

    /// Pointer (mouse, touch, pen) presses, moves and releases. Positions are CSS
    /// pixels with the surface's top left as origin. Moves also arrive while nothing is
    /// pressed (hover). Presses and releases carry which button
    /// ([`PointerEvent::button`]).
    pub fn on_pointer(&self, f: impl FnMut(PointerEvent) + 'static) {
        self.0.pointer.set(f);
    }

    /// The wheel (including two-finger scrolling on a trackpad). Amounts are CSS
    /// pixels.
    ///
    /// Returning `Handled` calls `preventDefault` to stop page scrolling. With
    /// `Continue`, the page or an outer `Scroll` scrolls as usual.
    pub fn on_wheel(&self, f: impl FnMut(PointerEvent) -> EventResponse + 'static) {
        self.0.wheel.set(f);
    }

    /// The current size of the surface (width, height). 0 if not laid out yet.
    pub fn size(&self) -> (f64, f64) {
        self.0.css_size()
    }

    /// Draws on the spot. **For automated tests only.**
    #[doc(hidden)]
    pub fn draw_for_test(&self) {
        self.0.paint();
    }

    /// The `<canvas>` itself. Public as a backend-specific escape hatch.
    pub fn native_canvas(&self) -> HtmlCanvasElement {
        self.0.canvas.clone()
    }
}

/// The bit of `MouseEvent.buttons` corresponding to a `MouseEvent.button` number.
///
/// The two numberings swap middle and right (`button` has middle as 1 and right as
/// 2; `buttons` has right as 2 and middle as 4).
fn web_buttons_bit(button: i16) -> u16 {
    match button {
        0 => 1,
        1 => 4,
        2 => 2,
        3 => 8,
        4 => 16,
        _ => 0,
    }
}

impl CanvasInner {
    /// Converts a mouse event's position into CSS pixels with the surface's top left as
    /// origin.
    ///
    /// `offsetX` is unreliable while dragging, so it is subtracted from the surface's
    /// position.
    fn local_point(&self, event: &web_sys::MouseEvent) -> Point {
        let rect = self.canvas.get_bounding_client_rect();
        Point::new(
            f64::from(event.client_x()) - rect.left(),
            f64::from(event.client_y()) - rect.top(),
        )
    }

    /// The size in CSS.
    fn css_size(&self) -> (f64, f64) {
        let element: &HtmlElement = self.canvas.as_ref();
        (
            f64::from(element.client_width()),
            f64::from(element.client_height()),
        )
    }

    /// The current display ratio (`devicePixelRatio`). 1 if it cannot be read.
    fn ratio(&self) -> f64 {
        web_sys::window()
            .map(|w| w.device_pixel_ratio())
            .filter(|r| r.is_finite() && *r > 0.0)
            .unwrap_or(1.0)
    }

    /// Re-attaches the subscription so that it redraws when the ratio departs from
    /// `ratio`.
    ///
    /// Does nothing if it is already attached for the same ratio.
    fn watch_ratio(self: &Rc<Self>, ratio: f64) {
        if matches!(&*self.ratio_watch.borrow(), Some((watched, _)) if *watched == ratio) {
            return;
        }
        let Some(list) = web_sys::window()
            .and_then(|w| w.match_media(&format!("(resolution: {ratio}dppx)")).ok())
            .flatten()
        else {
            return;
        };
        let weak = Rc::downgrade(self);
        let listener = Listener::attach(list.as_ref(), "change", move || {
            if let Some(inner) = weak.upgrade() {
                inner.paint();
            }
        });
        // 前の購読は、ここで置き換わるときに落ちて外れる。
        *self.ratio_watch.borrow_mut() = listener.ok().map(|listener| (ratio, listener));
    }

    /// Sets the backing pixel count to the CSS size × the ratio, then calls `on_draw`
    /// and draws.
    fn paint(self: &Rc<Self>) {
        let (width, height) = self.css_size();
        if width <= 0.0 || height <= 0.0 {
            return;
        }
        let ratio = self.ratio();
        self.watch_ratio(ratio);
        let pixel_width = (width * ratio).round() as u32;
        let pixel_height = (height * ratio).round() as u32;
        if self.canvas.width() != pixel_width {
            self.canvas.set_width(pixel_width);
        }
        if self.canvas.height() != pixel_height {
            self.canvas.set_height(pixel_height);
        }

        let mut painter = Painter::new(width, height);
        let Some(mut draw) = self.draw.borrow_mut().take() else {
            return;
        };
        draw(&mut painter);
        let mut slot = self.draw.borrow_mut();
        if slot.is_none() {
            *slot = Some(draw);
        }
        drop(slot);

        let context = &self.context;
        let _ = context.reset_transform();
        context.clear_rect(0.0, 0.0, f64::from(pixel_width), f64::from(pixel_height));
        let _ = context.scale(ratio, ratio);
        replay(context, painter.commands());
    }
}

impl Drop for CanvasInner {
    fn drop(&mut self) {
        if let Some(observer) = self.observer.borrow_mut().take() {
            observer.disconnect();
        }
    }
}

/// Draws the commands recorded in the [`Painter`] into the 2D context.
fn replay(context: &CanvasRenderingContext2d, commands: &[DrawCommand]) {
    for command in commands {
        match command {
            DrawCommand::Fill {
                path,
                color,
                opacity,
            } => {
                trace(context, path);
                context.set_global_alpha(*opacity);
                context.set_fill_style_str(&color.to_hex());
                context.fill();
            }
            DrawCommand::Stroke {
                path,
                color,
                width,
                dash,
                opacity,
            } => {
                trace(context, path);
                context.set_global_alpha(*opacity);
                context.set_stroke_style_str(&color.to_hex());
                context.set_line_width(*width);
                context.set_line_cap("butt");
                context.set_line_join("miter");
                let segments = js_sys::Array::new();
                for value in dash {
                    segments.push(&JsValue::from_f64(*value));
                }
                let _ = context.set_line_dash(&segments);
                context.stroke();
            }
            DrawCommand::Text {
                text,
                at,
                size,
                color,
                align,
                opacity,
            } => {
                context.set_global_alpha(*opacity);
                context.set_fill_style_str(&color.to_hex());
                context.set_font(&format!("{size}px system-ui, sans-serif"));
                context.set_text_baseline("top");
                context.set_text_align(match align {
                    Align::Center => "center",
                    Align::End => "right",
                    Align::Start | Align::Fill => "left",
                });
                let _ = context.fill_text(text, at.x, at.y);
            }
        }
    }
}

fn trace(context: &CanvasRenderingContext2d, path: &Path) {
    context.begin_path();
    for segment in path.segments() {
        match *segment {
            PathSegment::MoveTo(p) => context.move_to(p.x, p.y),
            PathSegment::LineTo(p) => context.line_to(p.x, p.y),
            PathSegment::CubicTo {
                control1,
                control2,
                to,
            } => {
                context.bezier_curve_to(control1.x, control1.y, control2.x, control2.y, to.x, to.y)
            }
            PathSegment::Close => context.close_path(),
        }
    }
}
