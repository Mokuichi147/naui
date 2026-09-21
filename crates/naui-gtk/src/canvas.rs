//! 描画面 (`GtkDrawingArea` + cairo)。
//!
//! GTK4 で「アプリが自分で描く面」は `GtkDrawingArea` で、描く関数へ
//! cairo のコンテキストが渡ってくる。naui はその中でアプリの `on_draw` を
//! 呼び、[`Painter`] に記録された命令を cairo の `move_to` / `line_to` /
//! `curve_to` / `fill` / `stroke` と Pango のレイアウト (`show_layout`) で
//! 再生する。ラスタライズ・アンチエイリアス・HiDPI の倍率・文字の整形は
//! cairo と Pango が行う。
//!
//! 文字はウィジェットの Pango コンテキストから作るので、書体はテーマの
//! UI フォントで、大きさだけを命令の `size` に差し替える。
//!
//! ポインターの通知は `GtkGestureDrag` (押下・解放) と
//! `GtkEventControllerMotion` (移動) から取る。

use std::cell::RefCell;
use std::rc::Rc;

use gtk::cairo;
use gtk::pango;
use gtk::prelude::*;
use naui_core::{
    Align, DrawCommand, Painter, Path, PathSegment, Point, PointerEvent, PointerPhase,
};

use crate::bin::SizeBin;
use crate::callback::Notifier;
use crate::widgets::{impl_widget, Widget};

/// 描く内容を決めるクロージャの置き場。呼んでいる間は取り出しておく
/// ([`Notifier`] と同じ決まり)。
type DrawSlot = RefCell<Option<Box<dyn FnMut(&mut Painter)>>>;

struct CanvasInner {
    native: gtk::DrawingArea,
    bin: SizeBin,
    draw: Rc<DrawSlot>,
    pointer: Rc<Notifier<PointerEvent>>,
}

/// アプリが自分で描く面 (`GtkDrawingArea`)。
///
/// 中身から大きさは決まらないので、`set_sizing` で指定する (`Scroll` と同じ)。
#[derive(Clone)]
pub struct Canvas(Rc<CanvasInner>);
impl_widget!(Canvas);

impl Canvas {
    pub(crate) fn new() -> Self {
        let native = gtk::DrawingArea::new();
        let bin = SizeBin::wrap(&native);
        let draw: Rc<DrawSlot> = Rc::new(RefCell::new(None));
        let pointer = Rc::new(Notifier::default());

        native.set_draw_func({
            let draw = draw.clone();
            move |area, cr, width, height| {
                let mut painter = Painter::new(f64::from(width), f64::from(height));
                let Some(mut f) = draw.borrow_mut().take() else {
                    return;
                };
                f(&mut painter);
                let mut slot = draw.borrow_mut();
                if slot.is_none() {
                    *slot = Some(f);
                }
                drop(slot);
                replay(area, cr, painter.commands());
            }
        });

        // 押下と解放。`drag-end` は面の外で離しても届く。
        let drag = gtk::GestureDrag::new();
        drag.connect_drag_begin({
            let pointer = pointer.clone();
            move |_, x, y| pointer.emit(PointerEvent::new(PointerPhase::Down, Point::new(x, y)))
        });
        drag.connect_drag_end({
            let pointer = pointer.clone();
            move |gesture, dx, dy| {
                let (sx, sy) = gesture.start_point().unwrap_or((0.0, 0.0));
                pointer.emit(PointerEvent::new(
                    PointerPhase::Up,
                    Point::new(sx + dx, sy + dy),
                ));
            }
        });
        native.add_controller(drag);

        // 移動。押していない間 (ホバー) も、押している間も届く。
        let motion = gtk::EventControllerMotion::new();
        motion.connect_motion({
            let pointer = pointer.clone();
            move |_, x, y| pointer.emit(PointerEvent::new(PointerPhase::Move, Point::new(x, y)))
        });
        native.add_controller(motion);

        Self(Rc::new(CanvasInner {
            native,
            bin,
            draw,
            pointer,
        }))
    }

    /// 描く内容を決めるクロージャ。描き直すたびに新しい [`Painter`] で呼ばれる。
    ///
    /// 呼ばれるのは [`redraw`](Self::redraw) の後と、面の大きさが変わったとき
    /// (と、GTK が再描画を求めたとき)。
    pub fn on_draw(&self, f: impl FnMut(&mut Painter) + 'static) {
        *self.0.draw.borrow_mut() = Some(Box::new(f));
        self.redraw();
    }

    /// 描き直しを頼む。その場では描かず、次の描画のときに `on_draw` が呼ばれる。
    pub fn redraw(&self) {
        self.0.native.queue_draw();
    }

    /// ポインター (マウス・タッチ・ペン) の押下・移動・解放。位置は面の左上を
    /// 原点にした論理ピクセル。移動は押していない間 (ホバー) も届く。
    pub fn on_pointer(&self, f: impl FnMut(PointerEvent) + 'static) {
        self.0.pointer.set(f);
    }

    /// いまの面の大きさ (幅, 高さ)。まだレイアウトされていなければ 0。
    pub fn size(&self) -> (f64, f64) {
        (
            f64::from(self.0.native.width()),
            f64::from(self.0.native.height()),
        )
    }

    /// 描画面本体。バックエンド固有の脱出口として公開している。
    pub fn native_area(&self) -> gtk::DrawingArea {
        self.0.native.clone()
    }
}

/// [`Painter`] に記録された命令を cairo へ描く。
fn replay(area: &gtk::DrawingArea, cr: &cairo::Context, commands: &[DrawCommand]) {
    for command in commands {
        match command {
            DrawCommand::Fill {
                path,
                color,
                opacity,
            } => {
                trace(cr, path);
                let (r, g, b) = color.to_unit();
                cr.set_source_rgba(r, g, b, *opacity);
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
                let layout = area.create_pango_layout(Some(text));
                let mut font = area.pango_context().font_description().unwrap_or_default();
                font.set_absolute_size(size * f64::from(pango::SCALE));
                layout.set_font_description(Some(&font));
                let (width, _) = layout.pixel_size();
                let x = match align {
                    Align::Center => at.x - f64::from(width) / 2.0,
                    Align::End => at.x - f64::from(width),
                    Align::Start | Align::Fill => at.x,
                };
                let (r, g, b) = color.to_unit();
                cr.set_source_rgba(r, g, b, *opacity);
                cr.move_to(x, at.y);
                pangocairo::functions::show_layout(cr, &layout);
                cr.new_path();
            }
        }
    }
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
