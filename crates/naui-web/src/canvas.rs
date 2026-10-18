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
//! `ResizeObserver` で拾い、そのたびに描き直す。CSS の大きさが変わらないまま
//! 倍率だけが変わること (別のモニターへ移す、ブラウザの拡大率を変える) は
//! `ResizeObserver` では分からないので、いまの倍率を条件にした
//! `matchMedia("(resolution: …dppx)")` の `change` で拾う。描き直しの要求
//! (`redraw`) は `requestAnimationFrame` へまとめ、その場では描かない
//! (他の 3 環境と同じ決まり)。
//!
//! どのボタンかは `PointerEvent.button` で見分ける。ブラウザは 2 つ目以降の
//! ボタンの押下と解放 (左を押したまま右を押す、など) を `pointerdown` /
//! `pointerup` ではなく `pointermove` で知らせる (`button` が -1 でない
//! `pointermove`) ので、それも押下・解放として届ける。右クリックで出る
//! ブラウザのコンテキストメニューは面の上では出さない (他の 3 環境の描画面にも
//! 無いため)。
//!
//! ホイールは `wheel` イベントから取る。`deltaMode` が行 (Firefox の古い
//! 既定など) なら 1 行を [`WHEEL_STEP`] の 1/3 に、ページなら面の大きさに直す。
//! ピクセルのときはブラウザの量をそのまま届けるので、刻みのあるホイールの
//! 1 刻みの量はブラウザによって違う。アプリが `Handled` を返したときだけ
//! `preventDefault` してページのスクロールを止める。

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

/// 描く内容を決めるクロージャの置き場。呼んでいる間は取り出しておく。
type DrawSlot = RefCell<Option<Box<dyn FnMut(&mut Painter)>>>;
/// JS 側へ渡すクロージャの置き場。ハンドルと同じ寿命で持つ。
type JsSlot<F> = RefCell<Option<Closure<F>>>;

struct CanvasInner {
    canvas: HtmlCanvasElement,
    context: CanvasRenderingContext2d,
    draw: DrawSlot,
    pointer: ValueHandler<PointerEvent>,
    wheel: WheelHandler,
    /// `requestAnimationFrame` を予約済みかどうか。
    scheduled: Cell<bool>,
    /// 予約した描画を実行するクロージャ。1 つ作って使い回す。
    frame: JsSlot<dyn FnMut()>,
    /// 大きさの変化の購読。落とすと購読も外れる。
    observer: RefCell<Option<ResizeObserver>>,
    _observer_callback: JsSlot<dyn FnMut(JsValue)>,
    _listeners: RefCell<Vec<Listener>>,
    /// 最後に描いたときの倍率と、その倍率から外れたことを知らせる購読。
    ///
    /// 倍率が変わると条件に合わなくなって `change` が飛ぶので、そのたびに
    /// 新しい倍率で張り直す。
    ratio_watch: RefCell<Option<(f64, Listener)>>,
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
            wheel: WheelHandler::default(),
            scheduled: Cell::new(false),
            frame: RefCell::new(None),
            observer: RefCell::new(None),
            _observer_callback: RefCell::new(None),
            _listeners: RefCell::new(Vec::new()),
            ratio_watch: RefCell::new(None),
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
    /// 原点にした CSS ピクセル。移動は押していない間 (ホバー) も届く。押下と
    /// 解放にはどのボタンかが付く ([`PointerEvent::button`])。
    pub fn on_pointer(&self, f: impl FnMut(PointerEvent) + 'static) {
        self.0.pointer.set(f);
    }

    /// ホイール (トラックパッドの 2 本指のスクロールも含む)。量は CSS ピクセル。
    ///
    /// `Handled` を返すと `preventDefault` してページのスクロールを止める。
    /// `Continue` ならページや外側の `Scroll` がいつもどおりスクロールする。
    pub fn on_wheel(&self, f: impl FnMut(PointerEvent) -> EventResponse + 'static) {
        self.0.wheel.set(f);
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

/// `MouseEvent.button` の番号に当たる、`MouseEvent.buttons` のビット。
///
/// 2 つの番号の振り方は中と右で入れ替わっている (`button` は中が 1 で右が 2、
/// `buttons` は右が 2 で中が 4)。
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
    /// マウスのイベントの位置を、面の左上を原点にした CSS ピクセルへ直す。
    ///
    /// `offsetX` はつかんでいる間に当てにならないので、面の位置から引く。
    fn local_point(&self, event: &web_sys::MouseEvent) -> Point {
        let rect = self.canvas.get_bounding_client_rect();
        Point::new(
            f64::from(event.client_x()) - rect.left(),
            f64::from(event.client_y()) - rect.top(),
        )
    }

    /// CSS 上の大きさ。
    fn css_size(&self) -> (f64, f64) {
        let element: &HtmlElement = self.canvas.as_ref();
        (
            f64::from(element.client_width()),
            f64::from(element.client_height()),
        )
    }

    /// いまの表示の倍率 (`devicePixelRatio`)。読めなければ 1。
    fn ratio(&self) -> f64 {
        web_sys::window()
            .map(|w| w.device_pixel_ratio())
            .filter(|r| r.is_finite() && *r > 0.0)
            .unwrap_or(1.0)
    }

    /// 倍率が `ratio` から外れたら描き直すように、購読を張り直す。
    ///
    /// すでに同じ倍率で張ってあれば何もしない。
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

    /// 裏の画素数を CSS の大きさ × 倍率にそろえてから、`on_draw` を呼んで描く。
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
