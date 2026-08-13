//! ウィジェット間で共有する描画ヘルパ。
//!
//! 「Fluent は下辺に濃いストロークが入る」「macOS はコントロールに縦グラデと
//! 影が付く」「Adwaita は枠線を持たず半透明の重ねで階層を作る」といった
//! プラットフォーム固有の癖は、すべてここに閉じ込めてある。

use miui_core::color::{Brush, Color};
use miui_core::geometry::{Corners, Point, Rect};
use miui_core::painter::Painter;
use miui_core::theme::{PlatformStyle, Theme};

/// コントロールの相互作用状態。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ControlState {
    pub hovered: bool,
    pub pressed: bool,
    pub enabled: bool,
}

impl ControlState {
    pub fn new(hovered: bool, pressed: bool, enabled: bool) -> Self {
        Self {
            hovered: hovered && enabled,
            pressed: pressed && enabled,
            enabled,
        }
    }
}

/// 単色をプラットフォームの流儀に合わせてブラシ化する。
///
/// macOS だけはコントロールに上→下のわずかなグラデーションが入る。
pub fn control_brush(theme: &Theme, base: Color, pressed: bool) -> Brush {
    if theme.metrics.gradient_controls && base.a > 0.5 {
        let k: f32 = if pressed { -0.02 } else { 0.03 };
        Brush::VerticalGradient {
            top: base.lighten(k.max(0.0)),
            bottom: base.darken(0.045 + (-k).max(0.0)),
        }
    } else {
        Brush::Solid(base)
    }
}

/// 通常コントロールの地の色。
pub fn control_fill(theme: &Theme, state: ControlState) -> Color {
    let c = &theme.color;
    if !state.enabled {
        c.control_disabled
    } else if state.pressed {
        c.control_active
    } else if state.hovered {
        c.control_hover
    } else {
        c.control
    }
}

/// アクセントコントロールの地の色。
pub fn accent_fill(theme: &Theme, state: ControlState) -> Color {
    let c = &theme.color;
    if !state.enabled {
        // 無効時はアクセントを落として地に馴染ませる。
        c.control_disabled
    } else if state.pressed {
        c.accent_active
    } else if state.hovered {
        c.accent_hover
    } else {
        c.accent
    }
}

/// コントロールの地・枠・影・下辺ストロークをまとめて指定する。
pub struct ControlFrame {
    pub rect: Rect,
    pub corners: Corners,
    pub fill: Color,
    pub state: ControlState,
    /// macOS でドロップシャドウを落とすか。
    pub elevated: bool,
    /// アクセント色の面か (縁取りの描き方が変わる)。
    pub accent_surface: bool,
}

/// コントロールの枠と、必要なら影 / 下辺ストロークを描く。
pub fn draw_control_frame(painter: &mut dyn Painter, theme: &Theme, frame: &ControlFrame) {
    let ControlFrame {
        rect,
        corners,
        fill,
        state,
        elevated,
        accent_surface,
    } = *frame;
    let m = &theme.metrics;

    // 1) 影 (macOS の押し出しボタン)。
    if elevated && state.enabled && !state.pressed && theme.is_cupertino() {
        painter.shadow_rrect(
            rect.translate(0.0, m.shadow_offset_y * 0.5),
            corners,
            m.shadow_blur * 0.35,
            -0.5,
            theme.color.shadow.scale_alpha(0.6),
        );
    }

    // 2) 地。
    let brush = control_brush(theme, fill, state.pressed);
    painter.fill_rrect(rect, corners, &brush);

    // 3) 枠。Adwaita は枠を持たない (半透明の地だけで階層を示す)。
    let border = if accent_surface {
        // アクセント面の上には、暗い縁を薄く乗せて縁取りする。
        Color::hexa(0x000000, if theme.mode.is_dark() { 0.30 } else { 0.15 })
    } else if theme.is_adwaita() {
        Color::TRANSPARENT
    } else {
        theme.color.border
    };
    if !border.is_transparent() {
        painter.stroke_rrect(rect, corners, m.border_width, &Brush::Solid(border));
    }

    // 4) Fluent の下辺ストローク。押下時は消える。
    if m.bottom_edge_stroke && !state.pressed && state.enabled {
        let color = if accent_surface {
            Color::hexa(0x000000, 0.40)
        } else {
            theme.color.border_strong
        };
        let r = corners.clamped(rect.size).bottom_left.max(1.0);
        let y = rect.max_y() - m.border_width * 0.5;
        painter.stroke_polyline(
            &[
                Point::new(rect.min_x() + r, y),
                Point::new(rect.max_x() - r, y),
            ],
            m.border_width,
            color,
        );
    }
}

/// フォーカスリングをコントロールの外側へ描く。
///
/// Fluent だけは「内側に地の色の 1px + 外側に濃い 2px」の二重リング。
pub fn draw_focus_ring(painter: &mut dyn Painter, theme: &Theme, rect: Rect, radius: f32) {
    let m = &theme.metrics;
    match theme.style {
        PlatformStyle::Fluent => {
            let inner = rect.inset(-1.0);
            painter.stroke_rrect(
                inner,
                Corners::all(radius + 1.0),
                1.0,
                &Brush::Solid(theme.color.focus_ring_inner),
            );
            let outer = inner.inset(-m.focus_ring_width);
            painter.stroke_rrect(
                outer,
                Corners::all(radius + 1.0 + m.focus_ring_width),
                m.focus_ring_width,
                &Brush::Solid(theme.color.focus_ring),
            );
        }
        _ => {
            let outer = rect.inset(-(m.focus_ring_offset + m.focus_ring_width));
            painter.stroke_rrect(
                outer,
                Corners::all(radius + m.focus_ring_offset + m.focus_ring_width),
                m.focus_ring_width,
                &Brush::Solid(theme.color.focus_ring),
            );
        }
    }
}

/// 1 行テキストを矩形内で縦中央に置くときの上端 y。
pub fn centered_text_y(rect: Rect, line_height: f32) -> f32 {
    rect.min_y() + ((rect.height() - line_height) * 0.5).max(0.0)
}

/// チェックマークの折れ線 (与えられた正方形に内接)。
pub fn check_path(rect: Rect) -> [Point; 3] {
    let s = rect.width().min(rect.height());
    let x = rect.min_x() + (rect.width() - s) * 0.5;
    let y = rect.min_y() + (rect.height() - s) * 0.5;
    [
        Point::new(x + s * 0.22, y + s * 0.52),
        Point::new(x + s * 0.42, y + s * 0.72),
        Point::new(x + s * 0.78, y + s * 0.30),
    ]
}
