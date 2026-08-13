//! チェックボックス / スイッチ / ラジオボタン。

use miui_core::color::{Brush, Color};
use miui_core::event::{Event, Key, MouseButton};
use miui_core::geometry::{Corners, Rect, Size};
use miui_core::layout::BoxConstraints;
use miui_core::painter::TextAlign;
use miui_core::theme::Theme;
use miui_core::widget::{EventCx, LayoutCx, Node, PaintCx, Widget};

use crate::style::{centered_text_y, check_path, control_fill, draw_focus_ring, ControlState};

/// 「ラベル付きの小さなコントロール」に共通する当たり判定とキー操作。
fn toggle_event<M>(
    cx: &mut EventCx<M>,
    event: &Event,
    node: Node,
    enabled: bool,
    mut activate: impl FnMut(&mut EventCx<M>),
) {
    if !enabled {
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
                    activate(cx);
                }
                cx.consume();
            }
        }
        Event::KeyPressed { key, .. } if cx.is_focused(node.id) => {
            if matches!(key, Key::Space | Key::Enter) {
                activate(cx);
                cx.consume();
            }
        }
        _ => {}
    }
}

/// ラベルを右に置く共通レイアウト。返り値は (全体サイズ, コントロール幅)。
fn labelled_layout(
    cx: &mut LayoutCx,
    label: &str,
    control: Size,
    bc: BoxConstraints,
) -> (Size, f32) {
    let style = cx.theme.typography.body;
    let gap = cx.theme.metrics.spacing_sm;
    let lm = cx.text.line_metrics(&style);
    let text_w = if label.is_empty() {
        0.0
    } else {
        cx.text.measure_line(label, &style) + gap
    };
    let h = control.height.max(lm.line_height);
    (
        bc.constrain(Size::new(control.width + text_w, h)),
        control.width,
    )
}

fn draw_label(cx: &mut PaintCx, node: Node, label: &str, control_w: f32, enabled: bool) {
    if label.is_empty() {
        return;
    }
    let style = cx.theme.typography.body;
    let gap = cx.theme.metrics.spacing_sm;
    let lm = cx.painter.line_metrics(&style);
    let color = cx.theme.text_color(enabled);
    let rect = Rect::new(
        node.rect.min_x() + control_w + gap,
        centered_text_y(node.rect, lm.line_height),
        (node.rect.width() - control_w - gap).max(0.0),
        lm.line_height,
    );
    cx.painter
        .draw_text_block(rect, label, &style, color, TextAlign::Start);
}

// ---------------------------------------------------------------- Checkbox

pub struct Checkbox<M> {
    label: String,
    checked: bool,
    enabled: bool,
    on_toggle: Option<Box<dyn Fn(bool) -> M>>,
}

impl<M> Checkbox<M> {
    pub fn new(label: impl Into<String>, checked: bool) -> Self {
        Self {
            label: label.into(),
            checked,
            enabled: true,
            on_toggle: None,
        }
    }

    pub fn on_toggle(mut self, f: impl Fn(bool) -> M + 'static) -> Self {
        self.on_toggle = Some(Box::new(f));
        self
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    fn active(&self) -> bool {
        self.enabled && self.on_toggle.is_some()
    }
}

impl<M> Widget<M> for Checkbox<M> {
    fn type_name(&self) -> &'static str {
        "Checkbox"
    }

    fn layout(&mut self, cx: &mut LayoutCx, bc: BoxConstraints) -> Size {
        if self.active() {
            cx.register_focusable();
        }
        let s = cx.theme.metrics.checkbox_size;
        let (size, _) = labelled_layout(cx, &self.label, Size::new(s, s), bc);
        size
    }

    fn paint(&self, cx: &mut PaintCx, node: Node) {
        let theme = cx.theme;
        let m = &theme.metrics;
        let state = ControlState::new(cx.is_hovered(node.id), cx.is_active(node.id), self.active());
        let s = m.checkbox_size;
        let box_rect = Rect::new(
            node.rect.min_x(),
            node.rect.min_y() + (node.rect.height() - s) * 0.5,
            s,
            s,
        );
        let corners = Corners::all(m.checkbox_radius);

        if self.checked {
            let fill = if !state.enabled {
                theme.color.control_disabled
            } else if state.pressed {
                theme.color.accent_active
            } else if state.hovered {
                theme.color.accent_hover
            } else {
                theme.color.accent
            };
            cx.painter.fill_rrect(box_rect, corners, &Brush::Solid(fill));
            let mark = if state.enabled {
                theme.color.text_on_accent
            } else {
                theme.color.text_disabled
            };
            cx.painter
                .stroke_polyline(&check_path(box_rect), (s * 0.11).max(1.3), mark);
        } else {
            let fill = control_fill(theme, state);
            cx.painter.fill_rrect(box_rect, corners, &Brush::Solid(fill));
            let border = if state.enabled {
                theme.color.border_strong
            } else {
                theme.color.border
            };
            cx.painter
                .stroke_rrect(box_rect, corners, m.border_width, &Brush::Solid(border));
        }

        if cx.shows_focus_ring(node.id) {
            draw_focus_ring(cx.painter, theme, box_rect, m.checkbox_radius);
        }

        draw_label(cx, node, &self.label, s, state.enabled);
    }

    fn event(&mut self, cx: &mut EventCx<M>, event: &Event, node: Node) {
        let checked = self.checked;
        let handler = self.on_toggle.as_ref();
        toggle_event(cx, event, node, self.active(), |cx| {
            if let Some(f) = handler {
                let msg = f(!checked);
                cx.emit(msg);
            }
        });
    }
}

// ------------------------------------------------------------------ Switch

pub struct Switch<M> {
    label: String,
    on: bool,
    enabled: bool,
    on_toggle: Option<Box<dyn Fn(bool) -> M>>,
}

impl<M> Switch<M> {
    pub fn new(label: impl Into<String>, on: bool) -> Self {
        Self {
            label: label.into(),
            on,
            enabled: true,
            on_toggle: None,
        }
    }

    pub fn on_toggle(mut self, f: impl Fn(bool) -> M + 'static) -> Self {
        self.on_toggle = Some(Box::new(f));
        self
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    fn active(&self) -> bool {
        self.enabled && self.on_toggle.is_some()
    }

    fn track_color(&self, theme: &Theme, state: ControlState) -> Color {
        if !state.enabled {
            return theme.color.control_disabled;
        }
        if self.on {
            if state.pressed {
                theme.color.accent_active
            } else if state.hovered {
                theme.color.accent_hover
            } else {
                theme.color.accent
            }
        } else if state.hovered {
            // ホバー時だけわずかに濃くする。
            theme.color.switch_track_off.over(theme.color.control_hover)
        } else {
            theme.color.switch_track_off
        }
    }
}

impl<M> Widget<M> for Switch<M> {
    fn type_name(&self) -> &'static str {
        "Switch"
    }

    fn layout(&mut self, cx: &mut LayoutCx, bc: BoxConstraints) -> Size {
        if self.active() {
            cx.register_focusable();
        }
        let m = &cx.theme.metrics;
        let control = Size::new(m.switch_width, m.switch_height);
        let (size, _) = labelled_layout(cx, &self.label, control, bc);
        size
    }

    fn paint(&self, cx: &mut PaintCx, node: Node) {
        let theme = cx.theme;
        let m = &theme.metrics;
        let state = ControlState::new(cx.is_hovered(node.id), cx.is_active(node.id), self.active());
        let w = m.switch_width;
        let h = m.switch_height;
        let track = Rect::new(
            node.rect.min_x(),
            node.rect.min_y() + (node.rect.height() - h) * 0.5,
            w,
            h,
        );
        let corners = Corners::all(h * 0.5);

        cx.painter
            .fill_rrect(track, corners, &Brush::Solid(self.track_color(theme, state)));
        if !self.on || theme.is_cupertino() {
            let border = if theme.is_adwaita() {
                Color::TRANSPARENT
            } else if self.on {
                Color::hexa(0x000000, 0.12)
            } else {
                theme.color.border_strong
            };
            if !border.is_transparent() {
                cx.painter
                    .stroke_rrect(track, corners, m.border_width, &Brush::Solid(border));
            }
        }

        // つまみ。オン / オフで左右に移動する。
        let pad = if theme.is_fluent() { 4.0 } else { 2.5 };
        let knob_r = (h * 0.5 - pad).max(3.0);
        let cy = track.center().y;
        let cx_pos = if self.on {
            track.max_x() - pad - knob_r
        } else {
            track.min_x() + pad + knob_r
        };
        let knob_color = if !state.enabled {
            theme.color.text_disabled
        } else if self.on {
            theme.color.text_on_accent
        } else if theme.is_fluent() {
            theme.color.text_secondary
        } else {
            Color::hex(0xFFFFFF)
        };
        if theme.is_cupertino() && state.enabled {
            cx.painter.shadow_rrect(
                Rect::new(cx_pos - knob_r, cy - knob_r + 1.0, knob_r * 2.0, knob_r * 2.0),
                Corners::all(knob_r),
                4.0,
                -0.5,
                theme.color.shadow.scale_alpha(0.8),
            );
        }
        cx.painter.fill_circle(
            miui_core::geometry::Point::new(cx_pos, cy),
            knob_r,
            &Brush::Solid(knob_color),
        );

        if cx.shows_focus_ring(node.id) {
            draw_focus_ring(cx.painter, theme, track, h * 0.5);
        }

        draw_label(cx, node, &self.label, w, state.enabled);
    }

    fn event(&mut self, cx: &mut EventCx<M>, event: &Event, node: Node) {
        let on = self.on;
        let handler = self.on_toggle.as_ref();
        toggle_event(cx, event, node, self.active(), |cx| {
            if let Some(f) = handler {
                let msg = f(!on);
                cx.emit(msg);
            }
        });
    }
}

// ------------------------------------------------------------------- Radio

pub struct Radio<M> {
    label: String,
    selected: bool,
    enabled: bool,
    on_select: Option<M>,
}

impl<M> Radio<M> {
    pub fn new(label: impl Into<String>, selected: bool) -> Self {
        Self {
            label: label.into(),
            selected,
            enabled: true,
            on_select: None,
        }
    }

    pub fn on_select(mut self, msg: M) -> Self {
        self.on_select = Some(msg);
        self
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    fn active(&self) -> bool {
        self.enabled && self.on_select.is_some()
    }
}

impl<M: Clone> Widget<M> for Radio<M> {
    fn type_name(&self) -> &'static str {
        "Radio"
    }

    fn layout(&mut self, cx: &mut LayoutCx, bc: BoxConstraints) -> Size {
        if self.active() {
            cx.register_focusable();
        }
        let s = cx.theme.metrics.checkbox_size;
        let (size, _) = labelled_layout(cx, &self.label, Size::new(s, s), bc);
        size
    }

    fn paint(&self, cx: &mut PaintCx, node: Node) {
        let theme = cx.theme;
        let m = &theme.metrics;
        let state = ControlState::new(cx.is_hovered(node.id), cx.is_active(node.id), self.active());
        let s = m.checkbox_size;
        let r = s * 0.5;
        let center = miui_core::geometry::Point::new(
            node.rect.min_x() + r,
            node.rect.min_y() + node.rect.height() * 0.5,
        );

        if self.selected {
            let fill = if !state.enabled {
                theme.color.control_disabled
            } else if state.pressed {
                theme.color.accent_active
            } else if state.hovered {
                theme.color.accent_hover
            } else {
                theme.color.accent
            };
            cx.painter.fill_circle(center, r, &Brush::Solid(fill));
            let dot = if state.enabled {
                theme.color.text_on_accent
            } else {
                theme.color.text_disabled
            };
            // 押下時はドットが縮む (WinUI の挙動)。
            let dot_r = if state.pressed { r * 0.24 } else { r * 0.32 };
            cx.painter.fill_circle(center, dot_r, &Brush::Solid(dot));
        } else {
            cx.painter
                .fill_circle(center, r, &Brush::Solid(control_fill(theme, state)));
            let border = if state.enabled {
                theme.color.border_strong
            } else {
                theme.color.border
            };
            cx.painter
                .stroke_circle(center, r, m.border_width, &Brush::Solid(border));
        }

        if cx.shows_focus_ring(node.id) {
            draw_focus_ring(
                cx.painter,
                theme,
                Rect::new(center.x - r, center.y - r, s, s),
                r,
            );
        }

        draw_label(cx, node, &self.label, s, state.enabled);
    }

    fn event(&mut self, cx: &mut EventCx<M>, event: &Event, node: Node) {
        let msg = self.on_select.clone();
        toggle_event(cx, event, node, self.active(), |cx| {
            if let Some(m) = msg.clone() {
                cx.emit(m);
            }
        });
    }
}
