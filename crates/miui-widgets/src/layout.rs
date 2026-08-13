//! レイアウト用コンテナ。
//!
//! 主軸 / 交差軸を持つ 1 つの Flex 実装に、行 (`Row`) と列 (`Column`) を
//! 与えているだけ。フレックス係数と揃えは CSS Flexbox の部分集合。

use miui_core::color::{Brush, Color};
use miui_core::event::Event;
use miui_core::geometry::{Corners, Insets, Point, Size};
use miui_core::layout::{Alignment, BoxConstraints, CrossAxis, MainAxis};
use miui_core::widget::{Element, EventCx, LayoutCx, Node, PaintCx, Widget};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    Horizontal,
    Vertical,
}

impl Axis {
    fn main(self, s: Size) -> f32 {
        match self {
            Axis::Horizontal => s.width,
            Axis::Vertical => s.height,
        }
    }
    fn cross(self, s: Size) -> f32 {
        match self {
            Axis::Horizontal => s.height,
            Axis::Vertical => s.width,
        }
    }
    fn size(self, main: f32, cross: f32) -> Size {
        match self {
            Axis::Horizontal => Size::new(main, cross),
            Axis::Vertical => Size::new(cross, main),
        }
    }
    fn point(self, main: f32, cross: f32) -> Point {
        match self {
            Axis::Horizontal => Point::new(main, cross),
            Axis::Vertical => Point::new(cross, main),
        }
    }
}

struct Child<M> {
    element: Element<M>,
    flex: f32,
}

/// 行 / 列のレイアウト。
pub struct Flex<M> {
    axis: Axis,
    children: Vec<Child<M>>,
    spacing: f32,
    main: MainAxis,
    cross: CrossAxis,
    padding: Insets,
    fill_main: bool,
}

impl<M> Flex<M> {
    pub fn new(axis: Axis) -> Self {
        Self {
            axis,
            children: Vec::new(),
            spacing: 0.0,
            main: MainAxis::Start,
            cross: CrossAxis::Start,
            padding: Insets::ZERO,
            fill_main: false,
        }
    }

    pub fn child(mut self, widget: impl Widget<M> + 'static) -> Self {
        self.children.push(Child {
            element: Element::new(widget),
            flex: 0.0,
        });
        self
    }

    /// 余った主軸方向のスペースを `flex` の比率で受け取る子。
    pub fn child_flex(mut self, widget: impl Widget<M> + 'static, flex: f32) -> Self {
        self.children.push(Child {
            element: Element::new(widget),
            flex,
        });
        self
    }

    /// すでに構築済みの [`Element`] を追加する (動的な子リスト向け)。
    pub fn push(mut self, element: Element<M>) -> Self {
        self.children.push(Child { element, flex: 0.0 });
        self
    }

    /// 条件付きで子を追加する。
    pub fn child_if(self, cond: bool, widget: impl Widget<M> + 'static) -> Self {
        if cond {
            self.child(widget)
        } else {
            self
        }
    }

    pub fn spacing(mut self, spacing: f32) -> Self {
        self.spacing = spacing;
        self
    }

    pub fn padding(mut self, padding: Insets) -> Self {
        self.padding = padding;
        self
    }

    /// 主軸方向の配置。
    pub fn justify(mut self, main: MainAxis) -> Self {
        self.main = main;
        self
    }

    /// 交差軸方向の配置。
    pub fn align(mut self, cross: CrossAxis) -> Self {
        self.cross = cross;
        self
    }

    /// 主軸方向に与えられた最大サイズまで広がる。
    pub fn fill_main(mut self) -> Self {
        self.fill_main = true;
        self
    }
}

/// 縦に積む。
pub fn column<M>() -> Flex<M> {
    Flex::new(Axis::Vertical)
}

/// 横に並べる。
pub fn row<M>() -> Flex<M> {
    Flex::new(Axis::Horizontal)
}

impl<M> Widget<M> for Flex<M> {
    fn type_name(&self) -> &'static str {
        match self.axis {
            Axis::Horizontal => "Row",
            Axis::Vertical => "Column",
        }
    }

    fn layout(&mut self, cx: &mut LayoutCx, bc: BoxConstraints) -> Size {
        let axis = self.axis;
        let inner = bc.shrink(self.padding.horizontal(), self.padding.vertical());
        let n = self.children.len();
        let total_spacing = if n > 1 {
            self.spacing * (n - 1) as f32
        } else {
            0.0
        };
        let cross_max = axis.cross(inner.max);
        let stretch = matches!(self.cross, CrossAxis::Stretch) && cross_max.is_finite();

        // 1) 固定サイズの子。
        let mut used_main = 0.0f32;
        let mut max_cross = 0.0f32;
        let mut total_flex = 0.0f32;
        for child in self.children.iter_mut() {
            if child.flex > 0.0 {
                total_flex += child.flex;
                continue;
            }
            let cbc = BoxConstraints {
                min: axis.size(0.0, if stretch { cross_max } else { 0.0 }),
                max: axis.size(f32::INFINITY, cross_max),
            };
            let size = child.element.layout(cx, cbc);
            used_main += axis.main(size);
            max_cross = max_cross.max(axis.cross(size));
        }

        // 2) フレックスな子へ残りを配分。
        let main_max = axis.main(inner.max);
        let free = if main_max.is_finite() {
            (main_max - total_spacing - used_main).max(0.0)
        } else {
            0.0
        };
        let mut flex_used = 0.0f32;
        for child in self.children.iter_mut() {
            if child.flex <= 0.0 {
                continue;
            }
            let share = if total_flex > 0.0 {
                free * (child.flex / total_flex)
            } else {
                0.0
            };
            let cbc = BoxConstraints {
                min: axis.size(share, if stretch { cross_max } else { 0.0 }),
                max: axis.size(share, cross_max),
            };
            let size = child.element.layout(cx, cbc);
            flex_used += axis.main(size);
            max_cross = max_cross.max(axis.cross(size));
        }

        let content_main = used_main + flex_used + total_spacing;
        let want_fill = self.fill_main || total_flex > 0.0 || self.main != MainAxis::Start;
        let self_main = if want_fill && main_max.is_finite() {
            main_max
        } else {
            content_main
        };
        let self_cross = if stretch { cross_max } else { max_cross };

        // 3) 配置。
        let slack = (self_main - content_main).max(0.0);
        let (mut pos, gap_extra) = match self.main {
            MainAxis::Start => (0.0, 0.0),
            MainAxis::Center => (slack * 0.5, 0.0),
            MainAxis::End => (slack, 0.0),
            MainAxis::SpaceBetween => {
                if n > 1 {
                    (0.0, slack / (n - 1) as f32)
                } else {
                    (slack * 0.5, 0.0)
                }
            }
            MainAxis::SpaceAround => {
                if n > 0 {
                    let each = slack / n as f32;
                    (each * 0.5, each)
                } else {
                    (0.0, 0.0)
                }
            }
        };

        for child in self.children.iter_mut() {
            let size = child.element.size();
            let cross_extent = axis.cross(size);
            let cross_pos = match self.cross {
                CrossAxis::Start | CrossAxis::Stretch => 0.0,
                CrossAxis::Center => (self_cross - cross_extent) * 0.5,
                CrossAxis::End => self_cross - cross_extent,
            };
            let origin = axis.point(pos, cross_pos.max(0.0));
            child.element.place(Point::new(
                origin.x + self.padding.left,
                origin.y + self.padding.top,
            ));
            pos += axis.main(size) + self.spacing + gap_extra;
        }

        bc.constrain(Size::new(
            axis.size(self_main, self_cross).width + self.padding.horizontal(),
            axis.size(self_main, self_cross).height + self.padding.vertical(),
        ))
    }

    fn paint(&self, cx: &mut PaintCx, node: Node) {
        for child in &self.children {
            child.element.paint(cx, node.rect.origin);
        }
    }

    fn event(&mut self, cx: &mut EventCx<M>, event: &Event, node: Node) {
        // 手前に描かれている (= 後ろの) 子を優先する。
        for child in self.children.iter_mut().rev() {
            if cx.is_handled() {
                return;
            }
            child.element.event(cx, event, node.rect.origin);
        }
    }
}

/// 1 つの子を装飾する箱。カード / パネル / 余白付けを兼ねる。
pub struct Container<M> {
    child: Option<Element<M>>,
    padding: Insets,
    background: Option<Color>,
    border: Option<Color>,
    radius: Option<f32>,
    shadow: bool,
    align: Option<Alignment>,
    width: Option<f32>,
    height: Option<f32>,
    fill_width: bool,
}

impl<M> Default for Container<M> {
    fn default() -> Self {
        Self::new()
    }
}

impl<M> Container<M> {
    pub fn new() -> Self {
        Self {
            child: None,
            padding: Insets::ZERO,
            background: None,
            border: None,
            radius: None,
            shadow: false,
            align: None,
            width: None,
            height: None,
            fill_width: false,
        }
    }

    pub fn child(mut self, widget: impl Widget<M> + 'static) -> Self {
        self.child = Some(Element::new(widget));
        self
    }

    pub fn padding(mut self, padding: Insets) -> Self {
        self.padding = padding;
        self
    }
    pub fn background(mut self, color: Color) -> Self {
        self.background = Some(color);
        self
    }
    pub fn border(mut self, color: Color) -> Self {
        self.border = Some(color);
        self
    }
    pub fn radius(mut self, r: f32) -> Self {
        self.radius = Some(r);
        self
    }
    pub fn shadow(mut self, on: bool) -> Self {
        self.shadow = on;
        self
    }
    pub fn align(mut self, align: Alignment) -> Self {
        self.align = Some(align);
        self
    }
    pub fn width(mut self, w: f32) -> Self {
        self.width = Some(w);
        self
    }
    pub fn height(mut self, h: f32) -> Self {
        self.height = Some(h);
        self
    }
    pub fn fill_width(mut self) -> Self {
        self.fill_width = true;
        self
    }

    /// テーマの `surface` を使ったカード。
    pub fn card(child: impl Widget<M> + 'static) -> Self {
        Container::new().child(child).shadow(true).fill_width()
    }
}

impl<M> Widget<M> for Container<M> {
    fn type_name(&self) -> &'static str {
        "Container"
    }

    fn layout(&mut self, cx: &mut LayoutCx, bc: BoxConstraints) -> Size {
        let pad = self.padding;
        let mut inner = bc.shrink(pad.horizontal(), pad.vertical()).loosen();
        if let Some(w) = self.width {
            inner = inner.with_max_width((w - pad.horizontal()).max(0.0));
        } else if self.fill_width && bc.has_bounded_width() {
            inner = inner.with_max_width((bc.max.width - pad.horizontal()).max(0.0));
        }
        if let Some(h) = self.height {
            inner = inner.with_max_height((h - pad.vertical()).max(0.0));
        }

        let child_size = match &mut self.child {
            Some(c) => c.layout(cx, inner),
            None => Size::ZERO,
        };

        let mut size = Size::new(
            child_size.width + pad.horizontal(),
            child_size.height + pad.vertical(),
        );
        if let Some(w) = self.width {
            size.width = w;
        } else if self.fill_width && bc.has_bounded_width() {
            size.width = bc.max.width;
        }
        if let Some(h) = self.height {
            size.height = h;
        }
        let size = bc.constrain(size);

        if let Some(child) = &mut self.child {
            let (fx, fy) = self.align.unwrap_or(Alignment::TopLeft).factors();
            let free_w = (size.width - pad.horizontal() - child.size().width).max(0.0);
            let free_h = (size.height - pad.vertical() - child.size().height).max(0.0);
            child.place(Point::new(pad.left + free_w * fx, pad.top + free_h * fy));
        }
        size
    }

    fn paint(&self, cx: &mut PaintCx, node: Node) {
        let theme = cx.theme;
        let radius = self.radius.unwrap_or(theme.metrics.surface_radius);
        let corners = Corners::all(radius);
        if self.shadow {
            cx.painter.shadow_rrect(
                node.rect.translate(0.0, theme.metrics.shadow_offset_y),
                corners,
                theme.metrics.shadow_blur,
                -1.0,
                theme.color.shadow,
            );
        }
        if let Some(bg) = self.background.or(if self.shadow {
            Some(theme.color.surface)
        } else {
            None
        }) {
            cx.painter.fill_rrect(node.rect, corners, &Brush::Solid(bg));
        }
        let border = self.border.or(if self.shadow && !theme.is_adwaita() {
            Some(theme.color.border)
        } else {
            None
        });
        if let Some(b) = border {
            cx.painter.stroke_rrect(
                node.rect,
                corners,
                theme.metrics.border_width,
                &Brush::Solid(b),
            );
        }
        if let Some(child) = &self.child {
            child.paint(cx, node.rect.origin);
        }
    }

    fn event(&mut self, cx: &mut EventCx<M>, event: &Event, node: Node) {
        if let Some(child) = &mut self.child {
            child.event(cx, event, node.rect.origin);
        }
    }
}

/// 主軸方向の余白を食うだけの子。`child_flex` と組み合わせて使う。
pub struct Spacer {
    min: f32,
}

impl Default for Spacer {
    fn default() -> Self {
        Spacer::new()
    }
}

impl Spacer {
    pub fn new() -> Self {
        Self { min: 0.0 }
    }
    /// 最低限確保する大きさ。
    pub fn size(min: f32) -> Self {
        Self { min }
    }
}

impl<M> Widget<M> for Spacer {
    fn type_name(&self) -> &'static str {
        "Spacer"
    }

    fn layout(&mut self, _cx: &mut LayoutCx, bc: BoxConstraints) -> Size {
        bc.constrain(Size::new(
            self.min.min(bc.max.width),
            self.min.min(bc.max.height),
        ))
    }

    fn paint(&self, _cx: &mut PaintCx, _node: Node) {}
}

/// 区切り線。
pub struct Divider {
    axis: Axis,
}

impl Divider {
    pub fn horizontal() -> Self {
        Self {
            axis: Axis::Horizontal,
        }
    }
    pub fn vertical() -> Self {
        Self {
            axis: Axis::Vertical,
        }
    }
}

impl<M> Widget<M> for Divider {
    fn type_name(&self) -> &'static str {
        "Divider"
    }

    fn layout(&mut self, _cx: &mut LayoutCx, bc: BoxConstraints) -> Size {
        let size = match self.axis {
            Axis::Horizontal => Size::new(
                if bc.has_bounded_width() {
                    bc.max.width
                } else {
                    0.0
                },
                1.0,
            ),
            Axis::Vertical => Size::new(
                1.0,
                if bc.has_bounded_height() {
                    bc.max.height
                } else {
                    0.0
                },
            ),
        };
        bc.constrain(size)
    }

    fn paint(&self, cx: &mut PaintCx, node: Node) {
        let color = cx.theme.color.divider;
        cx.painter
            .fill_rrect(node.rect, Corners::ZERO, &Brush::Solid(color));
    }
}

/// 子を固定サイズの箱に入れる。
pub struct SizedBox<M> {
    inner: Container<M>,
}

impl<M> SizedBox<M> {
    pub fn new(width: f32, height: f32) -> Self {
        Self {
            inner: Container::new().width(width).height(height),
        }
    }
    pub fn child(mut self, widget: impl Widget<M> + 'static) -> Self {
        self.inner = self.inner.child(widget);
        self
    }
    pub fn align(mut self, align: Alignment) -> Self {
        self.inner = self.inner.align(align);
        self
    }
}

impl<M> Widget<M> for SizedBox<M> {
    fn type_name(&self) -> &'static str {
        "SizedBox"
    }
    fn layout(&mut self, cx: &mut LayoutCx, bc: BoxConstraints) -> Size {
        self.inner.layout(cx, bc)
    }
    fn paint(&self, cx: &mut PaintCx, node: Node) {
        Widget::<M>::paint(&self.inner, cx, node);
    }
    fn event(&mut self, cx: &mut EventCx<M>, event: &Event, node: Node) {
        self.inner.event(cx, event, node);
    }
}
