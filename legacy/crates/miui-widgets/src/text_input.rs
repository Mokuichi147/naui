//! 1 行テキスト入力。
//!
//! IME の未確定文字列 (プリエディット) をキャレット位置にインラインで表示し、
//! 下線を引く。日本語入力はこの経路で扱う。

use miui_core::color::Brush;
use miui_core::event::{Event, Key, MouseButton};
use miui_core::geometry::{Corners, Point, Rect, Size};
use miui_core::layout::BoxConstraints;
use miui_core::painter::TextStyle;
use miui_core::widget::{CursorIcon, EventCx, Id, LayoutCx, Node, PaintCx, Widget};

use crate::style::{centered_text_y, draw_focus_ring};

/// `Id` ごとに保持される編集状態。
#[derive(Debug, Clone, Default)]
pub struct TextInputState {
    /// キャレットのバイト位置。
    pub caret: usize,
    /// 選択の始点 (キャレットと一致していれば選択なし)。
    pub anchor: usize,
    /// 水平スクロール量。
    pub scroll: f32,
    /// IME 未確定文字列。
    pub preedit: String,
}

impl TextInputState {
    fn selection(&self) -> (usize, usize) {
        (self.caret.min(self.anchor), self.caret.max(self.anchor))
    }
    fn has_selection(&self) -> bool {
        self.caret != self.anchor
    }
    fn collapse(&mut self, at: usize) {
        self.caret = at;
        self.anchor = at;
    }
}

pub struct TextInput<M> {
    value: String,
    placeholder: String,
    enabled: bool,
    width: Option<f32>,
    on_change: Option<Box<dyn Fn(String) -> M>>,
    on_submit: Option<M>,
}

impl<M> TextInput<M> {
    pub fn new(value: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            placeholder: String::new(),
            enabled: true,
            width: None,
            on_change: None,
            on_submit: None,
        }
    }

    pub fn placeholder(mut self, text: impl Into<String>) -> Self {
        self.placeholder = text.into();
        self
    }

    pub fn on_change(mut self, f: impl Fn(String) -> M + 'static) -> Self {
        self.on_change = Some(Box::new(f));
        self
    }

    /// Enter が押されたときのメッセージ。
    pub fn on_submit(mut self, msg: M) -> Self {
        self.on_submit = Some(msg);
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

    fn inner_padding(&self, padding_x: f32) -> f32 {
        padding_x.max(8.0)
    }

    /// 表示文字列 (未確定文字列をキャレット位置に差し込んだもの)。
    fn display(&self, state: &TextInputState) -> String {
        if state.preedit.is_empty() {
            return self.value.clone();
        }
        let caret = clamp_boundary(&self.value, state.caret);
        let mut s = String::with_capacity(self.value.len() + state.preedit.len());
        s.push_str(&self.value[..caret]);
        s.push_str(&state.preedit);
        s.push_str(&self.value[caret..]);
        s
    }

    fn edit(&self, cx: &mut EventCx<M>, id: Id, new_value: String, caret: usize) {
        if let Some(f) = &self.on_change {
            let msg = f(new_value);
            cx.emit(msg);
        }
        let state: &mut TextInputState = cx.state(id);
        state.collapse(caret);
    }
}

/// バイト位置を文字境界へ丸める。
fn clamp_boundary(s: &str, i: usize) -> usize {
    let mut i = i.min(s.len());
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

fn prev_boundary(s: &str, i: usize) -> usize {
    let i = clamp_boundary(s, i);
    s[..i].chars().next_back().map_or(0, |c| i - c.len_utf8())
}

fn next_boundary(s: &str, i: usize) -> usize {
    let i = clamp_boundary(s, i);
    s[i..].chars().next().map_or(i, |c| i + c.len_utf8())
}

impl<M: Clone> Widget<M> for TextInput<M> {
    fn type_name(&self) -> &'static str {
        "TextInput"
    }

    fn layout(&mut self, cx: &mut LayoutCx, bc: BoxConstraints) -> Size {
        if self.active() {
            cx.register_focusable();
        }
        let m = cx.theme.metrics.clone();
        let style = cx.theme.typography.body;
        let id = cx.current_id();
        let height = m.control_height.max(32.0);
        let width = self
            .width
            .unwrap_or(if bc.has_bounded_width() { bc.max.width } else { 200.0 });

        // キャレットが見えるように水平スクロールを調整する。
        let pad = self.inner_padding(m.control_padding_x);
        let inner_w = (width - pad * 2.0).max(1.0);
        let state = cx.state::<TextInputState>(id).clone();
        let display = self.display(&state);
        let caret_byte = clamp_boundary(&self.value, state.caret) + state.preedit.len();
        let caret_x = cx
            .text
            .measure_line(&display[..clamp_boundary(&display, caret_byte)], &style);
        let total = cx.text.measure_line(&display, &style);

        let mut scroll = state.scroll;
        if caret_x - scroll > inner_w {
            scroll = caret_x - inner_w;
        }
        if caret_x - scroll < 0.0 {
            scroll = caret_x;
        }
        scroll = scroll.clamp(0.0, (total - inner_w).max(0.0));
        cx.state::<TextInputState>(id).scroll = scroll;

        bc.constrain(Size::new(width, height))
    }

    fn paint(&self, cx: &mut PaintCx, node: Node) {
        let theme = cx.theme;
        let m = &theme.metrics;
        let focused = cx.is_focused(node.id);
        let hovered = cx.is_hovered(node.id);
        let corners = Corners::all(m.control_radius);
        let state = cx
            .state::<TextInputState>(node.id)
            .cloned()
            .unwrap_or_default();
        let state = &state;

        // 地。
        let fill = if !self.enabled {
            theme.color.control_disabled
        } else if focused {
            theme.color.surface
        } else if hovered {
            theme.color.control_hover
        } else {
            theme.color.surface_sunken
        };
        cx.painter
            .fill_rrect(node.rect, corners, &Brush::Solid(fill));
        cx.painter.stroke_rrect(
            node.rect,
            corners,
            m.border_width,
            &Brush::Solid(if focused && !theme.is_fluent() {
                theme.color.accent
            } else {
                theme.color.border_strong
            }),
        );

        // WinUI のテキストボックスは、フォーカス時に下辺が 2px のアクセント線になる。
        if theme.is_fluent() {
            let y = node.rect.max_y() - if focused { 1.0 } else { 0.5 };
            let color = if focused {
                theme.color.accent
            } else {
                theme.color.border_strong
            };
            let r = m.control_radius;
            cx.painter.stroke_polyline(
                &[
                    Point::new(node.rect.min_x() + r, y),
                    Point::new(node.rect.max_x() - r, y),
                ],
                if focused { 2.0 } else { 1.0 },
                color,
            );
        }

        if cx.shows_focus_ring(node.id) {
            draw_focus_ring(cx.painter, theme, node.rect, m.control_radius);
        }

        // 文字。
        let style: TextStyle = theme.typography.body;
        let lm = cx.painter.line_metrics(&style);
        let pad = self.inner_padding(m.control_padding_x);
        let inner = Rect::new(
            node.rect.min_x() + pad,
            centered_text_y(node.rect, lm.line_height),
            (node.rect.width() - pad * 2.0).max(0.0),
            lm.line_height,
        );
        cx.painter.push_clip(Rect::new(
            node.rect.min_x() + pad * 0.5,
            node.rect.min_y(),
            (node.rect.width() - pad).max(0.0),
            node.rect.height(),
        ));

        let display = self.display(state);
        let x0 = inner.min_x() - state.scroll;

        if display.is_empty() {
            if !self.placeholder.is_empty() {
                cx.painter.draw_text(
                    Point::new(x0, inner.min_y()),
                    &self.placeholder,
                    &style,
                    theme.color.text_disabled,
                );
            }
        } else {
            // 選択範囲。
            if focused && state.has_selection() && state.preedit.is_empty() {
                let (s, e) = state.selection();
                let sx = cx
                    .painter
                    .measure_line(&self.value[..clamp_boundary(&self.value, s)], &style);
                let ex = cx
                    .painter
                    .measure_line(&self.value[..clamp_boundary(&self.value, e)], &style);
                cx.painter.fill_rrect(
                    Rect::new(x0 + sx, inner.min_y(), (ex - sx).max(1.0), lm.line_height),
                    Corners::all(2.0),
                    &Brush::Solid(theme.color.accent_subtle),
                );
            }
            let color = theme.text_color(self.enabled);
            cx.painter
                .draw_text(Point::new(x0, inner.min_y()), &display, &style, color);
        }

        // IME 未確定文字列の下線。
        if !state.preedit.is_empty() {
            let caret = clamp_boundary(&self.value, state.caret);
            let start = cx.painter.measure_line(&self.value[..caret], &style);
            let width = cx.painter.measure_line(&state.preedit, &style);
            let y = inner.min_y() + lm.ascent + 1.5;
            cx.painter.stroke_polyline(
                &[Point::new(x0 + start, y), Point::new(x0 + start + width, y)],
                1.5,
                theme.color.accent,
            );
        }

        // キャレット。
        if focused && self.enabled {
            let caret_byte =
                clamp_boundary(&self.value, state.caret) + state.preedit.len();
            let cx_pos = cx
                .painter
                .measure_line(&display[..clamp_boundary(&display, caret_byte)], &style);
            cx.painter.fill_rrect(
                Rect::new(x0 + cx_pos, inner.min_y() + 1.0, 1.4, lm.line_height - 2.0),
                Corners::ZERO,
                &Brush::Solid(theme.color.text),
            );
        }

        cx.painter.pop_clip();
    }

    fn event(&mut self, cx: &mut EventCx<M>, event: &Event, node: Node) {
        if !self.active() {
            return;
        }
        let style = cx.theme.typography.body;
        let pad = self.inner_padding(cx.theme.metrics.control_padding_x);
        let id = node.id;

        match event {
            Event::PointerMoved(p) => {
                if node.rect.contains(*p) {
                    cx.interaction.hovered = Some(id);
                    cx.set_cursor(CursorIcon::Text);
                }
                if cx.is_active(id) {
                    let scroll = cx.state::<TextInputState>(id).scroll;
                    let x = p.x - node.rect.min_x() - pad + scroll;
                    let i = cx.text.index_at_x(&self.value, &style, x);
                    cx.state::<TextInputState>(id).caret = i;
                    cx.consume();
                }
            }
            Event::PointerPressed {
                position,
                button: MouseButton::Left,
            } => {
                if node.rect.contains(*position) {
                    cx.focus(id, false);
                    cx.interaction.active = Some(id);
                    let scroll = cx.state::<TextInputState>(id).scroll;
                    let x = position.x - node.rect.min_x() - pad + scroll;
                    let i = cx.text.index_at_x(&self.value, &style, x);
                    cx.state::<TextInputState>(id).collapse(i);
                    cx.consume();
                }
            }
            Event::PointerReleased {
                button: MouseButton::Left,
                ..
            } => {
                if cx.is_active(id) {
                    cx.interaction.active = None;
                    cx.consume();
                }
            }
            Event::Text(s) if cx.is_focused(id) => {
                if s.chars().any(|c| c.is_control()) {
                    return;
                }
                let state = cx.state::<TextInputState>(id);
                state.preedit.clear();
                let (a, b) = state.selection();
                let a = clamp_boundary(&self.value, a);
                let b = clamp_boundary(&self.value, b);
                let mut v = String::with_capacity(self.value.len() + s.len());
                v.push_str(&self.value[..a]);
                v.push_str(s);
                v.push_str(&self.value[b..]);
                self.edit(cx, id, v, a + s.len());
                cx.consume();
            }
            Event::ImePreedit { text, .. } if cx.is_focused(id) => {
                cx.state::<TextInputState>(id).preedit = text.clone();
                cx.request_redraw();
                cx.consume();
            }
            Event::KeyPressed { key, modifiers } if cx.is_focused(id) => {
                let state = cx.state::<TextInputState>(id).clone();
                let (sel_a, sel_b) = state.selection();
                let sel_a = clamp_boundary(&self.value, sel_a);
                let sel_b = clamp_boundary(&self.value, sel_b);
                match key {
                    Key::Backspace => {
                        if state.has_selection() {
                            let mut v = String::new();
                            v.push_str(&self.value[..sel_a]);
                            v.push_str(&self.value[sel_b..]);
                            self.edit(cx, id, v, sel_a);
                        } else if sel_a > 0 {
                            let p = prev_boundary(&self.value, sel_a);
                            let mut v = String::new();
                            v.push_str(&self.value[..p]);
                            v.push_str(&self.value[sel_a..]);
                            self.edit(cx, id, v, p);
                        }
                        cx.consume();
                    }
                    Key::Delete => {
                        if state.has_selection() {
                            let mut v = String::new();
                            v.push_str(&self.value[..sel_a]);
                            v.push_str(&self.value[sel_b..]);
                            self.edit(cx, id, v, sel_a);
                        } else if sel_b < self.value.len() {
                            let n = next_boundary(&self.value, sel_b);
                            let mut v = String::new();
                            v.push_str(&self.value[..sel_b]);
                            v.push_str(&self.value[n..]);
                            self.edit(cx, id, v, sel_b);
                        }
                        cx.consume();
                    }
                    Key::ArrowLeft => {
                        let target = if state.has_selection() && !modifiers.shift {
                            sel_a
                        } else {
                            prev_boundary(&self.value, state.caret)
                        };
                        let st = cx.state::<TextInputState>(id);
                        st.caret = target;
                        if !modifiers.shift {
                            st.anchor = target;
                        }
                        cx.consume();
                    }
                    Key::ArrowRight => {
                        let target = if state.has_selection() && !modifiers.shift {
                            sel_b
                        } else {
                            next_boundary(&self.value, state.caret)
                        };
                        let st = cx.state::<TextInputState>(id);
                        st.caret = target;
                        if !modifiers.shift {
                            st.anchor = target;
                        }
                        cx.consume();
                    }
                    Key::Home => {
                        let st = cx.state::<TextInputState>(id);
                        st.caret = 0;
                        if !modifiers.shift {
                            st.anchor = 0;
                        }
                        cx.consume();
                    }
                    Key::End => {
                        let end = self.value.len();
                        let st = cx.state::<TextInputState>(id);
                        st.caret = end;
                        if !modifiers.shift {
                            st.anchor = end;
                        }
                        cx.consume();
                    }
                    Key::Enter => {
                        if let Some(msg) = self.on_submit.clone() {
                            cx.emit(msg);
                        }
                        cx.consume();
                    }
                    Key::Escape => {
                        cx.interaction.focused = None;
                        cx.request_redraw();
                        cx.consume();
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
}
