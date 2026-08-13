//! ボタン。

use miui_core::color::Color;
use miui_core::event::{Event, Key, MouseButton};
use miui_core::geometry::{Corners, Point, Rect, Size};
use miui_core::layout::BoxConstraints;
use miui_core::painter::TextAlign;
use miui_core::theme::Theme;
use miui_core::widget::{EventCx, LayoutCx, Node, PaintCx, Widget};

use crate::style::{
    accent_fill, centered_text_y, control_fill, draw_control_frame, draw_focus_ring, ControlFrame,
    ControlState,
};

/// ボタンの強調度。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonVariant {
    /// 既定のコントロール色。
    Standard,
    /// アクセント色 (WinUI の AccentButton / macOS の default button)。
    Accent,
    /// 地の色を持たないボタン。
    Subtle,
    /// 破壊的操作。
    Danger,
}

pub struct Button<M> {
    label: String,
    on_press: Option<M>,
    variant: ButtonVariant,
    enabled: bool,
    min_width: Option<f32>,
    fill_width: bool,
}

impl<M> Button<M> {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            on_press: None,
            variant: ButtonVariant::Standard,
            enabled: true,
            min_width: None,
            fill_width: false,
        }
    }

    /// 押されたときに送るメッセージ。設定しなければ押せない。
    pub fn on_press(mut self, msg: M) -> Self {
        self.on_press = Some(msg);
        self
    }

    pub fn variant(mut self, variant: ButtonVariant) -> Self {
        self.variant = variant;
        self
    }
    pub fn accent(self) -> Self {
        self.variant(ButtonVariant::Accent)
    }
    pub fn subtle(self) -> Self {
        self.variant(ButtonVariant::Subtle)
    }
    pub fn danger(self) -> Self {
        self.variant(ButtonVariant::Danger)
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub fn min_width(mut self, w: f32) -> Self {
        self.min_width = Some(w);
        self
    }

    /// 与えられた幅いっぱいに広げる。
    pub fn fill_width(mut self) -> Self {
        self.fill_width = true;
        self
    }

    fn is_active(&self) -> bool {
        self.enabled && self.on_press.is_some()
    }

    fn fill_color(&self, theme: &Theme, state: ControlState) -> Color {
        match self.variant {
            ButtonVariant::Standard => control_fill(theme, state),
            ButtonVariant::Accent => accent_fill(theme, state),
            ButtonVariant::Subtle => {
                if !state.enabled {
                    Color::TRANSPARENT
                } else if state.pressed {
                    theme.color.control_active
                } else if state.hovered {
                    theme.color.control_hover
                } else {
                    Color::TRANSPARENT
                }
            }
            ButtonVariant::Danger => {
                if !state.enabled {
                    theme.color.control_disabled
                } else if state.pressed {
                    theme.color.danger.darken(0.15)
                } else if state.hovered {
                    theme.color.danger.lighten(0.08)
                } else {
                    theme.color.danger
                }
            }
        }
    }

    fn label_color(&self, theme: &Theme, state: ControlState) -> Color {
        if !state.enabled {
            return theme.color.text_disabled;
        }
        match self.variant {
            ButtonVariant::Accent => theme.color.text_on_accent,
            ButtonVariant::Danger => theme
                .color
                .danger
                .over(theme.color.window_bg)
                .readable_foreground(),
            _ => theme.color.text,
        }
    }
}

impl<M: Clone> Widget<M> for Button<M> {
    fn type_name(&self) -> &'static str {
        "Button"
    }

    fn layout(&mut self, cx: &mut LayoutCx, bc: BoxConstraints) -> Size {
        if self.is_active() {
            // レイアウト順がそのままタブ順になる。
            cx.register_focusable();
        }
        let style = cx.theme.typography.control;
        let text_w = cx.text.measure_line(&self.label, &style);
        let m = &cx.theme.metrics;
        let mut w = text_w + m.control_padding_x * 2.0;
        if let Some(min) = self.min_width {
            w = w.max(min);
        }
        if self.fill_width && bc.has_bounded_width() {
            w = bc.max.width;
        }
        bc.constrain(Size::new(w, m.control_height))
    }

    fn paint(&self, cx: &mut PaintCx, node: Node) {
        let theme = cx.theme;
        let state = ControlState::new(
            cx.is_hovered(node.id),
            cx.is_active(node.id),
            self.is_active(),
        );
        let radius = theme.metrics.control_radius;
        let corners = Corners::all(radius);
        let fill = self.fill_color(theme, state);
        let accent_surface = matches!(self.variant, ButtonVariant::Accent | ButtonVariant::Danger)
            && state.enabled;

        if fill.is_transparent() && matches!(self.variant, ButtonVariant::Subtle) {
            // 地を持たないので枠も影も描かない。
        } else {
            draw_control_frame(
                cx.painter,
                theme,
                &ControlFrame {
                    rect: node.rect,
                    corners,
                    fill,
                    state,
                    elevated: true,
                    accent_surface,
                },
            );
        }

        if cx.shows_focus_ring(node.id) {
            draw_focus_ring(cx.painter, theme, node.rect, radius);
        }

        let style = theme.typography.control;
        let lm = cx.painter.line_metrics(&style);
        let color = self.label_color(theme, state);
        // macOS では押下時にラベルが 0.5px 沈む。
        let dy = if state.pressed && theme.is_cupertino() {
            0.5
        } else {
            0.0
        };
        let line_rect = Rect::new(
            node.rect.min_x(),
            centered_text_y(node.rect, lm.line_height) + dy,
            node.rect.width(),
            lm.line_height,
        );
        cx.painter
            .draw_text_block(line_rect, &self.label, &style, color, TextAlign::Center);
    }

    fn event(&mut self, cx: &mut EventCx<M>, event: &Event, node: Node) {
        if !self.is_active() {
            return;
        }
        match event {
            Event::PointerMoved(p) => {
                if node.rect.contains(*p) {
                    cx.interaction.hovered = Some(node.id);
                }
            }
            Event::PointerPressed {
                position,
                button: MouseButton::Left,
            } => {
                if node.rect.contains(*position) {
                    cx.interaction.active = Some(node.id);
                    cx.focus(node.id, false);
                    cx.consume();
                }
            }
            Event::PointerReleased {
                position,
                button: MouseButton::Left,
            } => {
                if cx.is_active(node.id) {
                    cx.interaction.active = None;
                    if node.rect.contains(*position) {
                        if let Some(msg) = self.on_press.clone() {
                            cx.emit(msg);
                        }
                    }
                    cx.consume();
                }
            }
            Event::KeyPressed { key, .. } if cx.is_focused(node.id) => {
                if matches!(key, Key::Enter | Key::Space) {
                    if let Some(msg) = self.on_press.clone() {
                        cx.emit(msg);
                    }
                    cx.consume();
                }
            }
            _ => {}
        }
    }
}

/// アイコン代わりに使う、正方形の小さなボタン。
pub struct IconButton<M> {
    inner: Button<M>,
    size: f32,
    glyph: IconGlyph,
}

/// 組み込みで描ける最小限のアイコン。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IconGlyph {
    Plus,
    Minus,
    Check,
    ChevronDown,
    ChevronUp,
    Close,
}

impl<M> IconButton<M> {
    pub fn new(glyph: IconGlyph) -> Self {
        Self {
            inner: Button::new(""),
            size: 32.0,
            glyph,
        }
    }

    pub fn on_press(mut self, msg: M) -> Self {
        self.inner = self.inner.on_press(msg);
        self
    }
    pub fn variant(mut self, variant: ButtonVariant) -> Self {
        self.inner = self.inner.variant(variant);
        self
    }
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.inner = self.inner.enabled(enabled);
        self
    }
    pub fn size(mut self, size: f32) -> Self {
        self.size = size;
        self
    }
}

impl<M: Clone> Widget<M> for IconButton<M> {
    fn type_name(&self) -> &'static str {
        "IconButton"
    }

    fn layout(&mut self, cx: &mut LayoutCx, bc: BoxConstraints) -> Size {
        if self.inner.is_active() {
            cx.register_focusable();
        }
        let h = self.size.max(cx.theme.metrics.control_height);
        bc.constrain(Size::new(h, h))
    }

    fn paint(&self, cx: &mut PaintCx, node: Node) {
        Widget::<M>::paint(&self.inner, cx, node);
        let theme = cx.theme;
        let state = ControlState::new(
            cx.is_hovered(node.id),
            cx.is_active(node.id),
            self.inner.is_active(),
        );
        let color = self.inner.label_color(theme, state);
        let r = node.rect;
        let c = r.center();
        let s = r.width().min(r.height()) * 0.28;
        let w = 1.4;
        match self.glyph {
            IconGlyph::Plus => {
                cx.painter.stroke_polyline(
                    &[Point::new(c.x - s, c.y), Point::new(c.x + s, c.y)],
                    w,
                    color,
                );
                cx.painter.stroke_polyline(
                    &[Point::new(c.x, c.y - s), Point::new(c.x, c.y + s)],
                    w,
                    color,
                );
            }
            IconGlyph::Minus => {
                cx.painter.stroke_polyline(
                    &[Point::new(c.x - s, c.y), Point::new(c.x + s, c.y)],
                    w,
                    color,
                );
            }
            IconGlyph::Check => {
                let box_r = Rect::new(c.x - s, c.y - s, s * 2.0, s * 2.0);
                cx.painter
                    .stroke_polyline(&crate::style::check_path(box_r), w, color);
            }
            IconGlyph::ChevronDown => {
                cx.painter.stroke_polyline(
                    &[
                        Point::new(c.x - s, c.y - s * 0.4),
                        Point::new(c.x, c.y + s * 0.4),
                        Point::new(c.x + s, c.y - s * 0.4),
                    ],
                    w,
                    color,
                );
            }
            IconGlyph::ChevronUp => {
                cx.painter.stroke_polyline(
                    &[
                        Point::new(c.x - s, c.y + s * 0.4),
                        Point::new(c.x, c.y - s * 0.4),
                        Point::new(c.x + s, c.y + s * 0.4),
                    ],
                    w,
                    color,
                );
            }
            IconGlyph::Close => {
                cx.painter.stroke_polyline(
                    &[
                        Point::new(c.x - s, c.y - s),
                        Point::new(c.x + s, c.y + s),
                    ],
                    w,
                    color,
                );
                cx.painter.stroke_polyline(
                    &[
                        Point::new(c.x + s, c.y - s),
                        Point::new(c.x - s, c.y + s),
                    ],
                    w,
                    color,
                );
            }
        }
    }

    fn event(&mut self, cx: &mut EventCx<M>, event: &Event, node: Node) {
        self.inner.event(cx, event, node);
    }
}
