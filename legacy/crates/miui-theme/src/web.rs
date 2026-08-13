//! Web 既定のニュートラルテーマ。
//!
//! ブラウザ上ではホスト OS を判別して Fluent / macOS / Adwaita を選ぶのが
//! 基本だが、判別できない場合や明示的に「Web らしい見た目」を選んだ場合に
//! 使う中庸なトークン。

use miui_core::color::Color;
use miui_core::painter::FontWeight;
use miui_core::theme::{
    typography_from, ColorMode, Metrics, Palette, PlatformStyle, Theme, Typography,
};

pub fn theme(mode: ColorMode) -> Theme {
    Theme {
        style: PlatformStyle::Web,
        mode,
        color: match mode {
            ColorMode::Light => light(),
            ColorMode::Dark => dark(),
        },
        metrics: metrics(),
        typography: typography(),
    }
}

fn light() -> Palette {
    Palette {
        window_bg: Color::hex(0xF8FAFC),
        surface: Color::hex(0xFFFFFF),
        surface_sunken: Color::hex(0xF1F5F9),

        control: Color::hex(0xFFFFFF),
        control_hover: Color::hex(0xF1F5F9),
        control_active: Color::hex(0xE2E8F0),
        control_disabled: Color::hex(0xF1F5F9),
        switch_track_off: Color::hex(0xCBD5E1),

        border: Color::hex(0xE2E8F0),
        border_strong: Color::hex(0xCBD5E1),
        divider: Color::hex(0xE2E8F0),

        text: Color::hex(0x0F172A),
        text_secondary: Color::hex(0x64748B),
        text_disabled: Color::hex(0xCBD5E1),
        text_on_accent: Color::hex(0xFFFFFF),

        accent: Color::hex(0x2563EB),
        accent_hover: Color::hex(0x1D4ED8),
        accent_active: Color::hex(0x1E40AF),
        accent_subtle: Color::hexa(0x2563EB, 0.10),

        danger: Color::hex(0xDC2626),
        success: Color::hex(0x16A34A),
        warning: Color::hex(0xD97706),

        focus_ring: Color::hexa(0x2563EB, 0.55),
        focus_ring_inner: Color::TRANSPARENT,

        shadow: Color::hexa(0x0F172A, 0.12),
    }
}

fn dark() -> Palette {
    Palette {
        window_bg: Color::hex(0x0F172A),
        surface: Color::hex(0x1E293B),
        surface_sunken: Color::hex(0x162032),

        control: Color::hex(0x1E293B),
        control_hover: Color::hex(0x293548),
        control_active: Color::hex(0x334155),
        control_disabled: Color::hex(0x1B2436),
        switch_track_off: Color::hex(0x475569),

        border: Color::hex(0x334155),
        border_strong: Color::hex(0x475569),
        divider: Color::hex(0x334155),

        text: Color::hex(0xF1F5F9),
        text_secondary: Color::hex(0x94A3B8),
        text_disabled: Color::hex(0x475569),
        text_on_accent: Color::hex(0xFFFFFF),

        accent: Color::hex(0x3B82F6),
        accent_hover: Color::hex(0x60A5FA),
        accent_active: Color::hex(0x2563EB),
        accent_subtle: Color::hexa(0x3B82F6, 0.18),

        danger: Color::hex(0xF87171),
        success: Color::hex(0x4ADE80),
        warning: Color::hex(0xFBBF24),

        focus_ring: Color::hexa(0x60A5FA, 0.60),
        focus_ring_inner: Color::TRANSPARENT,

        shadow: Color::hexa(0x000000, 0.45),
    }
}

fn metrics() -> Metrics {
    Metrics {
        control_height: 36.0,
        control_radius: 8.0,
        surface_radius: 12.0,
        control_padding_x: 16.0,
        border_width: 1.0,
        focus_ring_width: 2.0,
        focus_ring_offset: 2.0,

        spacing_xs: 4.0,
        spacing_sm: 8.0,
        spacing_md: 16.0,
        spacing_lg: 24.0,

        checkbox_size: 18.0,
        checkbox_radius: 4.0,
        switch_width: 44.0,
        switch_height: 24.0,
        slider_track: 6.0,
        slider_thumb: 18.0,
        scrollbar_width: 8.0,

        shadow_blur: 14.0,
        shadow_offset_y: 3.0,

        gradient_controls: false,
        bottom_edge_stroke: false,
        press_shrink: 0.0,
    }
}

fn typography() -> Typography {
    let mut t = typography_from(
        14.0,
        vec!["Inter", "system-ui", "Helvetica Neue"],
        vec!["JetBrains Mono", "Menlo"],
    );
    t.subtitle.size = 18.0;
    t.subtitle.weight = FontWeight::SemiBold;
    t.title.size = 26.0;
    t.title.weight = FontWeight::Bold;
    t.caption.size = 12.0;
    t
}
