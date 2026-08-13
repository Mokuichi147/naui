//! 縦スクロール領域。
//!
//! スクロールバーは macOS / GNOME / Windows 11 のいずれでも
//! 「内容に重ねて表示する細いバー」が現行スタイルなので、
//! 領域の幅は消費せずオーバーレイで描く。

use miui_core::color::Brush;
use miui_core::event::Event;
use miui_core::geometry::{Corners, Point, Rect, Size};
use miui_core::layout::BoxConstraints;
use miui_core::widget::{Element, EventCx, LayoutCx, Node, PaintCx, Widget};

#[derive(Debug, Clone, Default)]
pub struct ScrollState {
    pub offset: f32,
    pub content_height: f32,
    pub view_height: f32,
}

impl ScrollState {
    pub fn max_offset(&self) -> f32 {
        (self.content_height - self.view_height).max(0.0)
    }
    pub fn is_scrollable(&self) -> bool {
        self.max_offset() > 0.5
    }
}

pub struct Scroll<M> {
    child: Option<Element<M>>,
}

impl<M> Default for Scroll<M> {
    fn default() -> Self {
        Self::new()
    }
}

impl<M> Scroll<M> {
    pub fn new() -> Self {
        Self { child: None }
    }

    pub fn child(mut self, widget: impl Widget<M> + 'static) -> Self {
        self.child = Some(Element::new(widget));
        self
    }
}

impl<M> Widget<M> for Scroll<M> {
    fn type_name(&self) -> &'static str {
        "Scroll"
    }

    fn layout(&mut self, cx: &mut LayoutCx, bc: BoxConstraints) -> Size {
        let id = cx.current_id();
        let width = if bc.has_bounded_width() {
            bc.max.width
        } else {
            0.0
        };
        let child_bc = BoxConstraints {
            min: Size::new(0.0, 0.0),
            max: Size::new(width, f32::INFINITY),
        };
        let content = match &mut self.child {
            Some(c) => c.layout(cx, child_bc),
            None => Size::ZERO,
        };
        let view_h = if bc.has_bounded_height() {
            bc.max.height
        } else {
            content.height
        };

        let state = cx.state::<ScrollState>(id);
        state.content_height = content.height;
        state.view_height = view_h;
        let max = (content.height - view_h).max(0.0);
        state.offset = state.offset.clamp(0.0, max);
        let offset = state.offset;

        if let Some(child) = &mut self.child {
            child.place(Point::new(0.0, -offset));
        }
        bc.constrain(Size::new(width, view_h))
    }

    fn paint(&self, cx: &mut PaintCx, node: Node) {
        cx.painter.push_clip(node.rect);
        if let Some(child) = &self.child {
            child.paint(cx, node.rect.origin);
        }
        cx.painter.pop_clip();

        let Some(state) = cx.state::<ScrollState>(node.id) else {
            return;
        };
        if !state.is_scrollable() {
            return;
        }
        let m = &cx.theme.metrics;
        let w = m.scrollbar_width;
        let track_h = node.rect.height();
        let ratio = (state.view_height / state.content_height).clamp(0.05, 1.0);
        let thumb_h = (track_h * ratio).max(24.0);
        let t = if state.max_offset() > 0.0 {
            state.offset / state.max_offset()
        } else {
            0.0
        };
        let y = node.rect.min_y() + (track_h - thumb_h) * t;
        let x = node.rect.max_x() - w - 2.0;
        let color = cx.theme.color.text_secondary.scale_alpha(0.55);
        cx.painter.fill_rrect(
            Rect::new(x, y, w, thumb_h),
            Corners::all(w * 0.5),
            &Brush::Solid(color),
        );
    }

    fn event(&mut self, cx: &mut EventCx<M>, event: &Event, node: Node) {
        if let Event::Scrolled {
            position, delta_y, ..
        } = event
        {
            if node.rect.contains(*position) {
                let state = cx.state::<ScrollState>(node.id);
                let max = (state.content_height - state.view_height).max(0.0);
                if max > 0.0 {
                    let before = state.offset;
                    state.offset = (state.offset - delta_y).clamp(0.0, max);
                    if (state.offset - before).abs() > f32::EPSILON {
                        cx.request_redraw();
                        cx.consume();
                        return;
                    }
                }
            }
        }
        if let Some(child) = &mut self.child {
            child.event(cx, event, node.rect.origin);
        }
    }
}
