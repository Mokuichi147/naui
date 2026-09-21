//! 描画面 (`<canvas>` + 2D コンテキスト)。
//!
//! ブラウザで「アプリが自分で描く面」は `<canvas>` そのもの。naui は
//! `on_draw` が [`Painter`] に記録した命令を `CanvasRenderingContext2D` の
//! `moveTo` / `lineTo` / `bezierCurveTo` / `fill` / `stroke` / `fillText` で
//! 再生するだけで、ラスタライズ・アンチエイリアス・文字の描画はブラウザが
//! 行う。
//!
//! 高解像度の表示 (`devicePixelRatio` > 1) では、裏の画素数を倍率ぶん増やし
//! `scale` を掛けてから描く。アプリが見る座標は CSS ピクセルのまま。
//!
//! 面の大きさは CSS のレイアウトで決まる (`set_sizing`)。大きさが変わったことは
//! `ResizeObserver` で拾い、そのたびに描き直す。描き直しの要求
//! (`redraw`) は `requestAnimationFrame` へまとめ、その場では描かない
//! (他の 3 環境と同じ決まり)。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use naui_core::{
    Align, DrawCommand, Painter, Path, PathSegment, Point, PointerEvent, PointerPhase, Result,
};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{
    CanvasRenderingContext2d, Document, Element, HtmlCanvasElement, HtmlElement, ResizeObserver,
};

use crate::to_error;
use crate::widgets::{create, impl_widget, Listener, ValueHandler, Widget};

/// 描く内容を決めるクロージャの置き場。呼んでいる間は取り出しておく。
type DrawSlot = RefCell<Option<Box<dyn FnMut(&mut Painter)>>>;
/// JS 側へ渡すクロージャの置き場。ハンドルと同じ寿命で持つ。
type JsSlot<F> = RefCell<Option<Closure<F>>>;

struct CanvasInner {
    canvas: HtmlCanvasElement,
    context: CanvasRenderingContext2d,
    draw: DrawSlot,
    pointer: ValueHandler<PointerEvent>,
    /// `requestAnimationFrame` を予約済みかどうか。
    scheduled: Cell<bool>,
    /// 予約した描画を実行するクロージャ。1 つ作って使い回す。
    frame: JsSlot<dyn FnMut()>,
    /// 大きさの変化の購読。落とすと購読も外れる。
    observer: RefCell<Option<ResizeObserver>>,
    _observer_callback: JsSlot<dyn FnMut(JsValue)>,
    _listeners: RefCell<Vec<Listener>>,
}

/// アプリが自分で描く面 (`<canvas>`)。
///
/// 中身から大きさは決まらないので、`set_sizing` で指定する (`Scroll` と同じ)。
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
            .map_err(|e| to_error("2D コンテキストの取得", e))?
            .ok_or_else(|| naui_core::Error::new("2D コンテキストの取得", "対応していません"))?
            .dyn_into::<CanvasRenderingContext2d>()
            .map_err(|e| to_error("2D コンテキストの取得", JsValue::from(e)))?;

        let inner = Rc::new(CanvasInner {
            canvas,
            context,
            draw: RefCell::new(None),
            pointer: ValueHandler::default(),
            scheduled: Cell::new(false),
            frame: RefCell::new(None),
            observer: RefCell::new(None),
            _observer_callback: RefCell::new(None),
            _listeners: RefCell::new(Vec::new()),
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
            .map_err(|e| to_error("ResizeObserver の生成", e))?;
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
                // 押している間は面の外へ出ても動きと解放が届くようにする。
                if phase == PointerPhase::Down {
                    let _ = inner.canvas.set_pointer_capture(event.pointer_id());
                }
                // `offsetX` はつかんでいる間に当てにならないので、面の位置から引く。
                let rect = inner.canvas.get_bounding_client_rect();
                let point = Point::new(
                    f64::from(event.client_x()) - rect.left(),
                    f64::from(event.client_y()) - rect.top(),
                );
                inner.pointer.emit(PointerEvent::new(phase, point));
            })?);
        }
        *self.0._listeners.borrow_mut() = listeners;
        Ok(())
    }

    /// 描く内容を決めるクロージャ。描き直すたびに新しい [`Painter`] で呼ばれる。
    ///
    /// 呼ばれるのは [`redraw`](Self::redraw) の後と、面の大きさが変わったとき。
    pub fn on_draw(&self, f: impl FnMut(&mut Painter) + 'static) {
        *self.0.draw.borrow_mut() = Some(Box::new(f));
        self.redraw();
    }

    /// 描き直しを頼む。その場では描かず、次のフレームで `on_draw` が呼ばれる。
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

    /// ポインター (マウス・タッチ・ペン) の押下・移動・解放。位置は面の左上を
    /// 原点にした CSS ピクセル。移動は押していない間 (ホバー) も届く。
    pub fn on_pointer(&self, f: impl FnMut(PointerEvent) + 'static) {
        self.0.pointer.set(f);
    }

    /// いまの面の大きさ (幅, 高さ)。まだレイアウトされていなければ 0。
    pub fn size(&self) -> (f64, f64) {
        self.0.css_size()
    }

    /// その場で描く。**自動テスト専用**。
    #[doc(hidden)]
    pub fn draw_for_test(&self) {
        self.0.paint();
    }

    /// `<canvas>` 本体。バックエンド固有の脱出口として公開している。
    pub fn native_canvas(&self) -> HtmlCanvasElement {
        self.0.canvas.clone()
    }
}

impl CanvasInner {
    /// CSS 上の大きさ。
    fn css_size(&self) -> (f64, f64) {
        let element: &HtmlElement = self.canvas.as_ref();
        (
            f64::from(element.client_width()),
            f64::from(element.client_height()),
        )
    }

    /// 裏の画素数を CSS の大きさ × 倍率にそろえてから、`on_draw` を呼んで描く。
    fn paint(&self) {
        let (width, height) = self.css_size();
        if width <= 0.0 || height <= 0.0 {
            return;
        }
        let ratio = web_sys::window()
            .map(|w| w.device_pixel_ratio())
            .filter(|r| r.is_finite() && *r > 0.0)
            .unwrap_or(1.0);
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

/// [`Painter`] に記録された命令を 2D コンテキストへ描く。
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
