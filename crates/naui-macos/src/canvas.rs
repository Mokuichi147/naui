//! 描画面 (`NSView` のサブクラス + `drawRect:`)。
//!
//! AppKit で「アプリが自分で描く面」は、`NSView` を継承して `drawRect:` を
//! 書くのが標準の形。naui はその形をそのまま使い、`drawRect:` の中で
//! アプリの `on_draw` を呼んで [`Painter`] に記録された命令を
//! `NSBezierPath` と `NSString` の描画 (`drawAtPoint:withAttributes:`) で
//! 再生する。ラスタライズ・アンチエイリアス・Retina の倍率・文字の描画は
//! すべて AppKit (Core Graphics / Core Text) が行う。
//!
//! 座標は `isFlipped` を `true` にして**左上原点・下向き**にそろえる
//! (他の 3 環境と同じ)。フォントはシステムフォント (`systemFontOfSize:`)。
//!
//! ポインターの通知は `mouseDown:` / `mouseDragged:` / `mouseUp:` (右ボタンと
//! 中ボタンの同じ組も) と、`NSTrackingArea` を通した `mouseMoved:` から取る。
//! どのボタンかは、AppKit がどのメソッドへ配ったかで見分ける (`mouseDown:` が
//! 左、`rightMouseDown:` が右)。それ以外のボタンはまとめて `otherMouseDown:` へ
//! 来るので、`NSEvent.buttonNumber` (中が 2、側面が 3 と 4) を見る。
//! トラックパッドの 2 本指のクリックや、副ボタンのクリックの設定も、AppKit が
//! `rightMouseDown:` へ配るので右ボタンとして届く。
//!
//! ホイールは `scrollWheel:` から取る。`scrollingDeltaX` / `scrollingDeltaY` は
//! トラックパッドのような細かく測れる装置 (`hasPreciseScrollingDeltas`) なら
//! 論理ピクセル、刻みのあるホイールなら行 (1 刻みでおよそ 1) なので、後者は
//! [`WHEEL_STEP`] を掛ける。AppKit の量は「中身が動く向き」で正負が他の
//! 3 環境と逆なので、符号を反転して届ける。アプリが `Continue` を返したら
//! `super` へ回し、外側の `NSScrollView` がいつもどおりスクロールする。

use std::cell::RefCell;
use std::rc::Rc;

use naui_core::{
    Align, Color, DrawCommand, EventResponse, Painter, Path, PathSegment, PointerButton,
    PointerEvent, PointerPhase, WheelHandler, WHEEL_STEP,
};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObjectProtocol};
use objc2::{
    define_class, msg_send, AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, Message,
};
use objc2_app_kit::{
    NSBezierPath, NSColor, NSEvent, NSFont, NSFontAttributeName, NSForegroundColorAttributeName,
    NSLineCapStyle, NSLineJoinStyle, NSStringDrawing, NSTrackingArea, NSTrackingAreaOptions,
    NSView,
};
use objc2_foundation::{NSDictionary, NSPoint, NSRect, NSSize, NSString};

use crate::trampoline::ValueHandler;
use crate::widgets::{impl_widget, Widget};

/// 描く内容を決めるクロージャの置き場。
///
/// 呼んでいる間は取り出しておく (`ValueHandler` と同じ決まり)。描いている
/// 最中に `on_draw` を呼び直しても二重借用にならない。
type DrawSlot = RefCell<Option<Box<dyn FnMut(&mut Painter)>>>;

struct CanvasState {
    draw: DrawSlot,
    pointer: ValueHandler<PointerEvent>,
    wheel: WheelHandler,
    /// `mouseMoved:` を受けるための追跡範囲。大きさが変わるたびに張り直す。
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
        /// 左上原点・下向き。他の 3 環境の座標と同じにする。
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        /// ウィンドウが手前でなくても最初のクリックを受ける
        /// (図の上を押して選ぶ操作で、1 回目が「手前へ出す」に食われないように)。
        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, _event: Option<&NSEvent>) -> bool {
            true
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            let bounds = self.bounds();
            let mut painter = Painter::new(bounds.size.width, bounds.size.height);
            let Some(mut draw) = self.ivars().draw.borrow_mut().take() else {
                return;
            };
            draw(&mut painter);
            let mut slot = self.ivars().draw.borrow_mut();
            if slot.is_none() {
                *slot = Some(draw);
            }
            drop(slot);
            replay(painter.commands());
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
        unsafe { msg_send![super(this), init] }
    }

    fn emit_pointer(&self, phase: PointerPhase, event: &NSEvent, button: Option<PointerButton>) {
        self.ivars()
            .pointer
            .emit(PointerEvent::new(phase, self.local_point(event)).button(button));
    }

    /// イベントの位置を、面の左上を原点にした座標へ直す。
    fn local_point(&self, event: &NSEvent) -> naui_core::Point {
        let point = self.convertPoint_fromView(event.locationInWindow(), None);
        naui_core::Point::new(point.x, point.y)
    }
}

struct CanvasInner {
    native: Retained<NSView>,
    /// `native` と同じオブジェクト。クロージャの置き場へ触るために持つ。
    view: Retained<CanvasView>,
}

/// アプリが自分で描く面 (`NSView` + `drawRect:`)。
///
/// 中身から大きさは決まらないので、`set_sizing` で指定する
/// (`Scroll` と同じ)。
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

    /// 描く内容を決めるクロージャ。描き直すたびに新しい [`Painter`] で呼ばれる。
    ///
    /// 呼ばれるのは [`redraw`](Self::redraw) の後と、面の大きさが変わったとき
    /// (と、AppKit が再描画を求めたとき)。
    pub fn on_draw(&self, f: impl FnMut(&mut Painter) + 'static) {
        *self.0.view.ivars().draw.borrow_mut() = Some(Box::new(f));
        self.redraw();
    }

    /// 描き直しを頼む。その場では描かず、次の描画のときに `on_draw` が呼ばれる。
    pub fn redraw(&self) {
        self.0.native.setNeedsDisplay(true);
    }

    /// ポインター (マウス) の押下・移動・解放。位置は面の左上を原点にした
    /// 論理ピクセル。移動は押していない間 (ホバー) も届く。押下と解放には
    /// どのボタンかが付く ([`PointerEvent::button`])。
    pub fn on_pointer(&self, f: impl FnMut(PointerEvent) + 'static) {
        self.0.view.ivars().pointer.set(f);
    }

    /// ホイール (トラックパッドの 2 本指のスクロールも含む)。量は論理ピクセル。
    ///
    /// `Handled` を返すと外側のスクロールへは回らない。`Continue` なら
    /// 外側の `NSScrollView` がいつもどおりスクロールする。
    pub fn on_wheel(&self, f: impl FnMut(PointerEvent) -> EventResponse + 'static) {
        self.0.view.ivars().wheel.set(f);
    }

    /// いまの面の大きさ (幅, 高さ)。まだレイアウトされていなければ 0。
    pub fn size(&self) -> (f64, f64) {
        let bounds = self.0.native.bounds();
        (bounds.size.width, bounds.size.height)
    }

    /// その場で描く。**自動テスト専用**。
    ///
    /// 実際のアプリでは AppKit が描画のタイミングを決めるので使わない。
    #[doc(hidden)]
    pub fn draw_for_test(&self) {
        self.0.native.displayIfNeeded();
    }
}

/// `otherMouseDown:` / `otherMouseUp:` に来たボタン。左右以外がここへ来る。
fn other_button(event: &NSEvent) -> Option<PointerButton> {
    match event.buttonNumber() {
        2 => Some(PointerButton::Middle),
        3 => Some(PointerButton::Back),
        4 => Some(PointerButton::Forward),
        _ => None,
    }
}

/// [`Painter`] に記録された命令を、いまのグラフィックスコンテキストへ描く。
fn replay(commands: &[DrawCommand]) {
    for command in commands {
        match command {
            DrawCommand::Fill {
                path,
                color,
                opacity,
            } => {
                let bezier = to_bezier(path);
                ns_color(*color, *opacity).setFill();
                bezier.fill();
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
        }
    }
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

/// 文字を 1 行描く。`at` は上端で、横は `align` に従う。
///
/// `isFlipped` なビューでは `drawAtPoint:` の点が文字の左上になるので、
/// 幅だけ測って寄せる。
fn draw_text(
    text: &str,
    at: naui_core::Point,
    size: f64,
    color: Color,
    align: Align,
    opacity: f64,
) {
    let string = NSString::from_str(text);
    let font = NSFont::systemFontOfSize(size);
    let fill = ns_color(color, opacity);
    let font_object: &AnyObject = font.as_ref();
    let color_object: &AnyObject = fill.as_ref();
    let attributes = NSDictionary::from_slices(
        &[unsafe { NSFontAttributeName }, unsafe {
            NSForegroundColorAttributeName
        }],
        &[font_object, color_object],
    );
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
