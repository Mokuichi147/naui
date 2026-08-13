//! テキスト表示。

use miui_core::color::Color;
use miui_core::geometry::Size;
use miui_core::layout::BoxConstraints;
use miui_core::painter::{TextAlign, TextStyle};
use miui_core::theme::Theme;
use miui_core::widget::{LayoutCx, Node, PaintCx, Widget};

/// タイポグラフィ上の役割。テーマ側の定義を参照する。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextRole {
    Body,
    BodyStrong,
    Caption,
    Subtitle,
    Title,
}

impl TextRole {
    fn style(self, theme: &Theme) -> TextStyle {
        let t = &theme.typography;
        match self {
            TextRole::Body => t.body,
            TextRole::BodyStrong => t.body_strong,
            TextRole::Caption => t.caption,
            TextRole::Subtitle => t.subtitle,
            TextRole::Title => t.title,
        }
    }
}

/// 文字列を描くだけのウィジェット。
pub struct Text {
    content: String,
    role: TextRole,
    size_override: Option<f32>,
    color: Option<Color>,
    secondary: bool,
    align: TextAlign,
    wrap: bool,
}

impl Text {
    pub fn new(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            role: TextRole::Body,
            size_override: None,
            color: None,
            secondary: false,
            align: TextAlign::Start,
            wrap: true,
        }
    }

    pub fn role(mut self, role: TextRole) -> Self {
        self.role = role;
        self
    }
    pub fn title(self) -> Self {
        self.role(TextRole::Title)
    }
    pub fn subtitle(self) -> Self {
        self.role(TextRole::Subtitle)
    }
    pub fn caption(self) -> Self {
        self.role(TextRole::Caption)
    }
    pub fn strong(self) -> Self {
        self.role(TextRole::BodyStrong)
    }
    pub fn size(mut self, size: f32) -> Self {
        self.size_override = Some(size);
        self
    }
    pub fn color(mut self, color: Color) -> Self {
        self.color = Some(color);
        self
    }
    /// 二次テキスト色 (説明文など) にする。
    pub fn secondary(mut self) -> Self {
        self.secondary = true;
        self
    }
    pub fn align(mut self, align: TextAlign) -> Self {
        self.align = align;
        self
    }
    pub fn wrap(mut self, wrap: bool) -> Self {
        self.wrap = wrap;
        self
    }

    fn resolved_style(&self, theme: &Theme) -> TextStyle {
        let mut s = self.role.style(theme);
        if let Some(size) = self.size_override {
            s.size = size;
        }
        s
    }

    fn resolved_color(&self, theme: &Theme) -> Color {
        self.color.unwrap_or(if self.secondary {
            theme.color.text_secondary
        } else {
            theme.color.text
        })
    }
}

impl<M> Widget<M> for Text {
    fn type_name(&self) -> &'static str {
        "Text"
    }

    fn layout(&mut self, cx: &mut LayoutCx, bc: BoxConstraints) -> Size {
        let style = self.resolved_style(cx.theme);
        let size = if self.wrap && bc.has_bounded_width() {
            cx.text.measure_block(&self.content, &style, bc.max.width)
        } else {
            let w = cx.text.measure_line(&self.content, &style);
            let m = cx.text.line_metrics(&style);
            Size::new(w, m.line_height)
        };
        bc.constrain(size)
    }

    fn paint(&self, cx: &mut PaintCx, node: Node) {
        let style = self.resolved_style(cx.theme);
        let color = self.resolved_color(cx.theme);
        cx.painter
            .draw_text_block(node.rect, &self.content, &style, color, self.align);
    }
}
