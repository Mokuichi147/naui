//! スライダとプログレスバー。

use miui_core::color::{Brush, Color};
use miui_core::event::{Event, Key, MouseButton};
use miui_core::geometry::{Corners, Point, Rect, Size};
use miui_core::layout::BoxConstraints;
use miui_core::widget::{EventCx, LayoutCx, Node, PaintCx, Widget};

use crate::style::{draw_focus_ring, ControlState};

pub struct Slider<M> {
    value: f32,
    min: f32,
    max: f32,
    step: Option<f32>,
    enabled: bool,
    width: Option<f32>,
    on_change: Option<Box<dyn Fn(f32) -> M>>,
}

impl<M> Slider<M> {
    pub fn new(value: f32, range: std::ops::RangeInclusive<f32>) -> Self {
        Self {
            value,
            min: *range.start(),
            max: *range.end(),
            step: None,
            enabled: true,
            width: None,
            on_change: None,
        }
    }

    pub fn on_change(mut self, f: impl Fn(f32) -> M + 'static) -> Self {
        self.on_change = Some(Box::new(f));
        self
    }

    pub fn step(mut self, step: f32) -> Self {
        self.step = Some(step);
        self
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub fn width(mut self, w: f32) -> Self {
        self.width = Some(w);
        self
    }

    fn active(&self) -> bool {
        self.enabled && self.on_change.is_some()
    }

    fn fraction(&self) -> f32 {
        if (self.max - self.min).abs() < f32::EPSILON {
            0.0
        } else {
            ((self.value - self.min) / (self.max - self.min)).clamp(0.0, 1.0)
        }
    }

    fn quantize(&self, v: f32) -> f32 {
        let v = v.clamp(self.min, self.max);
        match self.step {
            Some(s) if s > 0.0 => {
                let n = ((v - self.min) / s).round();
                (self.min + n * s).clamp(self.min, self.max)
            }
            _ => v,
        }
    }

    /// x 座標から値を求める。
    fn value_at(&self, rect: Rect, thumb: f32, x: f32) -> f32 {
        let usable = (rect.width() - thumb).max(1.0);
        let t = ((x - rect.min_x() - thumb * 0.5) / usable).clamp(0.0, 1.0);
        self.quantize(self.min + t * (self.max - self.min))
    }

    fn emit_delta(&self, cx: &mut EventCx<M>, delta: f32) {
        if let Some(f) = &self.on_change {
            let step = self.step.unwrap_or((self.max - self.min) / 20.0);
            let v = self.quantize(self.value + delta * step);
            if (v - self.value).abs() > f32::EPSILON {
                let msg = f(v);
                cx.emit(msg);
            }
        }
    }
}

impl<M> Widget<M> for Slider<M> {
    fn type_name(&self) -> &'static str {
        "Slider"
    }

    fn layout(&mut self, cx: &mut LayoutCx, bc: BoxConstraints) -> Size {
        if self.active() {
            cx.register_focusable();
        }
        let m = &cx.theme.metrics;
        let h = m.slider_thumb.max(m.control_height * 0.7);
        let w = self
            .width
            .unwrap_or(if bc.has_bounded_width() { bc.max.width } else { 200.0 });
        bc.constrain(Size::new(w, h))
    }

    fn paint(&self, cx: &mut PaintCx, node: Node) {
        let theme = cx.theme;
        let m = &theme.metrics;
        let state = ControlState::new(cx.is_hovered(node.id), cx.is_active(node.id), self.active());
        let thumb = m.slider_thumb;
        let track_h = m.slider_track;
        let cy = node.rect.center().y;
        let track = Rect::new(
            node.rect.min_x() + thumb * 0.5,
            cy - track_h * 0.5,
            (node.rect.width() - thumb).max(0.0),
            track_h,
        );
        let corners = Corners::all(track_h * 0.5);

        // 未選択部分。
        let rail = if state.enabled {
            theme.color.border_strong
        } else {
            theme.color.control_disabled
        };
        cx.painter.fill_rrect(track, corners, &Brush::Solid(rail));

        // 選択済み部分。
        let t = self.fraction();
        let filled = Rect::new(track.min_x(), track.min_y(), track.width() * t, track_h);
        let fill_color = if !state.enabled {
            theme.color.text_disabled
        } else {
            theme.color.accent
        };
        cx.painter
            .fill_rrect(filled, corners, &Brush::Solid(fill_color));

        // つまみ。
        let center = Point::new(track.min_x() + track.width() * t, cy);
        let r = thumb * 0.5;
        if theme.is_fluent() {
            // 外側はアクセント、内側は白。押下 / ホバーで内側の大きさが変わる。
            let outer = if state.pressed {
                theme.color.accent_active
            } else if state.hovered {
                theme.color.accent_hover
            } else {
                theme.color.accent
            };
            cx.painter.fill_circle(center, r, &Brush::Solid(outer));
            let inner_r = if state.pressed {
                r * 0.42
            } else if state.hovered {
                r * 0.62
            } else {
                r * 0.55
            };
            let inner = if state.enabled {
                theme.color.text_on_accent
            } else {
                theme.color.control_disabled
            };
            cx.painter.fill_circle(center, inner_r, &Brush::Solid(inner));
        } else {
            if state.enabled {
                cx.painter.shadow_rrect(
                    Rect::new(center.x - r, center.y - r + 1.0, r * 2.0, r * 2.0),
                    Corners::all(r),
                    m.shadow_blur * 0.5,
                    -0.5,
                    theme.color.shadow,
                );
            }
            let knob = if !state.enabled {
                theme.color.control_disabled
            } else if state.pressed {
                theme.color.control_active
            } else {
                Color::hex(0xFFFFFF)
            };
            cx.painter.fill_circle(center, r, &Brush::Solid(knob));
            cx.painter.stroke_circle(
                center,
                r,
                m.border_width,
                &Brush::Solid(theme.color.border_strong),
            );
        }

        if cx.shows_focus_ring(node.id) {
            draw_focus_ring(
                cx.painter,
                theme,
                Rect::new(center.x - r, center.y - r, r * 2.0, r * 2.0),
                r,
            );
        }
    }

    fn event(&mut self, cx: &mut EventCx<M>, event: &Event, node: Node) {
        if !self.active() {
            return;
        }
        let thumb = cx.theme.metrics.slider_thumb;
        match event {
            Event::PointerMoved(p) => {
                if node.rect.contains(*p) {
                    cx.interaction.hovered = Some(node.id);
                }
                if cx.is_active(node.id) {
                    if let Some(f) = &self.on_change {
                        let v = self.value_at(node.rect, thumb, p.x);
                        if (v - self.value).abs() > f32::EPSILON {
                            let msg = f(v);
                            cx.emit(msg);
                        }
                    }
                    cx.consume();
                }
            }
            Event::PointerPressed {
                position,
                button: MouseButton::Left,
            } => {
                if node.rect.contains(*position) {
                    cx.interaction.active = Some(node.id);
                    cx.focus(node.id, false);
                    if let Some(f) = &self.on_change {
                        let v = self.value_at(node.rect, thumb, position.x);
                        if (v - self.value).abs() > f32::EPSILON {
                            let msg = f(v);
                            cx.emit(msg);
                        }
                    }
                    cx.consume();
                }
            }
            Event::PointerReleased {
                button: MouseButton::Left,
                ..
            } => {
                if cx.is_active(node.id) {
                    cx.interaction.active = None;
                    cx.consume();
                }
            }
            Event::KeyPressed { key, .. } if cx.is_focused(node.id) => match key {
                Key::ArrowLeft | Key::ArrowDown => {
                    self.emit_delta(cx, -1.0);
                    cx.consume();
                }
                Key::ArrowRight | Key::ArrowUp => {
                    self.emit_delta(cx, 1.0);
                    cx.consume();
                }
                Key::Home => {
                    if let Some(f) = &self.on_change {
                        let msg = f(self.min);
                        cx.emit(msg);
                    }
                    cx.consume();
                }
                Key::End => {
                    if let Some(f) = &self.on_change {
                        let msg = f(self.max);
                        cx.emit(msg);
                    }
                    cx.consume();
                }
                _ => {}
            },
            _ => {}
        }
    }
}

/// 進捗バー。
pub struct ProgressBar {
    value: f32,
    height: Option<f32>,
}

impl ProgressBar {
    /// `value` は 0.0..=1.0。
    pub fn new(value: f32) -> Self {
        Self {
            value: value.clamp(0.0, 1.0),
            height: None,
        }
    }

    pub fn height(mut self, h: f32) -> Self {
        self.height = Some(h);
        self
    }
}

impl<M> Widget<M> for ProgressBar {
    fn type_name(&self) -> &'static str {
        "ProgressBar"
    }

    fn layout(&mut self, _cx: &mut LayoutCx, bc: BoxConstraints) -> Size {
        let h = self.height.unwrap_or(4.0);
        let w = if bc.has_bounded_width() {
            bc.max.width
        } else {
            200.0
        };
        bc.constrain(Size::new(w, h))
    }

    fn paint(&self, cx: &mut PaintCx, node: Node) {
        let theme = cx.theme;
        let h = node.rect.height();
        let corners = Corners::all(h * 0.5);
        cx.painter.fill_rrect(
            node.rect,
            corners,
            &Brush::Solid(theme.color.border_strong),
        );
        let w = node.rect.width() * self.value;
        if w > 0.5 {
            cx.painter.fill_rrect(
                Rect::new(node.rect.min_x(), node.rect.min_y(), w, h),
                corners,
                &Brush::Solid(theme.color.accent),
            );
        }
    }
}
