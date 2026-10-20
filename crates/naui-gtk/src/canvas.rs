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
//! ポインターの通知は 3 つのコントローラーから取る。`GtkGestureClick` が
//! 押下・解放 (どのボタンかは `current_button` で、左が 1、中が 2、右が 3、
//! 側面が 8 と 9)、`GtkGestureDrag` が**押している間の移動** (面の外へ出ても
//! 届く)、`GtkEventControllerMotion` が押していない間の移動 (ホバー。こちらは
//! 面の中にいる間しか出ない)。
//!
//! ホイールは `GtkEventControllerScroll` から取る。量の単位は
//! `gtk_event_controller_scroll_get_unit` で分かり、刻みのあるホイール
//! (`GDK_SCROLL_UNIT_WHEEL`、1 刻みで 1) は [`WHEEL_STEP`] を掛け、
//! トラックパッド (`GDK_SCROLL_UNIT_SURFACE`) は論理ピクセルのまま届ける。
//! スクロールの通知は位置を持たないので、最後にホバーで見た位置を使う。
//! アプリが `Continue` を返したら伝播を止めず、外側の `GtkScrolledWindow` が
//! いつもどおりスクロールする。
//!
//! `Handled` で伝播を止めても、外側の `GtkScrolledWindow` の慣性は防げない。
//! 外側は捕捉フェーズ (子より先) のコントローラーでも量の履歴をためていて、
//! 指を離したとき (止まりのイベント) にその履歴から慣性スクロールを始めるため。
//! 止まりのイベントは続けて Canvas にも届くので、最後の量を `Handled` で
//! 受けていたら、Canvas の `scroll-end` で外側の慣性を取り消す
//! ([`cancel_ancestor_kinetic`])。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::cairo;
use gtk::pango;
use gtk::prelude::*;
use naui_core::{
    Align, DrawCommand, EventResponse, Painter, Path, PathSegment, Point, PointerButton,
    PointerEvent, PointerPhase, WheelHandler, WHEEL_STEP,
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
    wheel: Rc<WheelHandler>,
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
        let wheel = Rc::new(WheelHandler::default());

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
    /// 原点にした論理ピクセル。移動は押していない間 (ホバー) も届く。押下と
    /// 解放にはどのボタンかが付く ([`PointerEvent::button`])。
    pub fn on_pointer(&self, f: impl FnMut(PointerEvent) + 'static) {
        self.0.pointer.set(f);
    }

    /// ホイール (トラックパッドの 2 本指のスクロールも含む)。量は論理ピクセル。
    ///
    /// `Handled` を返すと外側のスクロールへは回らない (指を離した後の慣性も
    /// 起きない)。`Continue` なら外側の `GtkScrolledWindow` がいつもどおり
    /// スクロールする。
    pub fn on_wheel(&self, f: impl FnMut(PointerEvent) -> EventResponse + 'static) {
        self.0.wheel.set(f);
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

/// 外側の `GtkScrolledWindow` が始めた慣性スクロールを取り消す。
///
/// 慣性を止める公開の API は無いが、`kinetic-scrolling` を切ると進行中の
/// 慣性 (`cancel_deceleration`) も止まる。すぐ元へ戻すので、設定は変わらない。
/// 入れ子のスクロールはどれも履歴をためているので、祖先をすべて辿る。
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

/// GDK のボタン番号をボタンへ直す。
///
/// タッチは GTK が 1 (主ボタン) として渡す。0 (どのボタンでもない) と
/// 知らない番号は `None`。
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
