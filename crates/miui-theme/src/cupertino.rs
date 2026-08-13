//! macOS のトークン。
//!
//! コントロールに微妙な縦グラデーションと下方向の影を持たせるのが特徴で、
//! `Metrics::gradient_controls` と `press_shrink` で表現する。

use miui_core::color::Color;
use miui_core::painter::FontWeight;
use miui_core::theme::{
    typography_from, ColorMode, Metrics, Palette, PlatformStyle, Theme, Typography,
};

pub fn theme(mode: ColorMode) -> Theme {
    Theme {
        style: PlatformStyle::Cupertino,
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
        window_bg: Color::hex(0xECECEC),
        surface: Color::hex(0xFFFFFF),
        surface_sunken: Color::hex(0xFFFFFF),

        control: Color::hex(0xFFFFFF),
        control_hover: Color::hex(0xFAFAFA),
        control_active: Color::hex(0xE0E0E0),
        control_disabled: Color::hexa(0xFFFFFF, 0.5),
        switch_track_off: Color::hex(0xE9E9EA),

        border: Color::hexa(0x000000, 0.12),
        border_strong: Color::hexa(0x000000, 0.20),
        divider: Color::hexa(0x000000, 0.10),

        text: Color::hexa(0x000000, 0.85),
        text_secondary: Color::hexa(0x3C3C43, 0.60),
        text_disabled: Color::hexa(0x3C3C43, 0.26),
        text_on_accent: Color::hex(0xFFFFFF),

        accent: Color::hex(0x007AFF),
        accent_hover: Color::hex(0x0A84FF),
        accent_active: Color::hex(0x0062CC),
        accent_subtle: Color::hexa(0x007AFF, 0.12),

        danger: Color::hex(0xFF3B30),
        success: Color::hex(0x34C759),
        warning: Color::hex(0xFF9500),

        focus_ring: Color::hexa(0x007AFF, 0.55),
        focus_ring_inner: Color::TRANSPARENT,

        shadow: Color::hexa(0x000000, 0.18),
    }
}

fn dark() -> Palette {
    Palette {
        window_bg: Color::hex(0x1E1E1E),
        surface: Color::hex(0x282828),
        surface_sunken: Color::hexa(0x000000, 0.22),

        control: Color::hexa(0xFFFFFF, 0.10),
        control_hover: Color::hexa(0xFFFFFF, 0.15),
        control_active: Color::hexa(0xFFFFFF, 0.05),
        control_disabled: Color::hexa(0xFFFFFF, 0.05),
        switch_track_off: Color::hexa(0xFFFFFF, 0.16),

        border: Color::hexa(0xFFFFFF, 0.14),
        border_strong: Color::hexa(0x000000, 0.50),
        divider: Color::hexa(0xFFFFFF, 0.12),

        text: Color::hexa(0xFFFFFF, 0.92),
        text_secondary: Color::hexa(0xEBEBF5, 0.60),
        text_disabled: Color::hexa(0xEBEBF5, 0.26),
        text_on_accent: Color::hex(0xFFFFFF),

        accent: Color::hex(0x0A84FF),
        accent_hover: Color::hex(0x3D9BFF),
        accent_active: Color::hex(0x0066CC),
        accent_subtle: Color::hexa(0x0A84FF, 0.20),

        danger: Color::hex(0xFF453A),
        success: Color::hex(0x30D158),
        warning: Color::hex(0xFF9F0A),

        focus_ring: Color::hexa(0x0A84FF, 0.65),
        focus_ring_inner: Color::TRANSPARENT,

        shadow: Color::hexa(0x000000, 0.45),
    }
}

fn metrics() -> Metrics {
    Metrics {
        control_height: 28.0,
        control_radius: 6.0,
        surface_radius: 10.0,
        control_padding_x: 14.0,
        border_width: 1.0,
        focus_ring_width: 3.0,
        focus_ring_offset: 1.5,

        spacing_xs: 4.0,
        spacing_sm: 8.0,
        spacing_md: 12.0,
        spacing_lg: 20.0,

        checkbox_size: 16.0,
        checkbox_radius: 4.0,
        switch_width: 38.0,
        switch_height: 22.0,
        slider_track: 4.0,
        slider_thumb: 20.0,
        scrollbar_width: 8.0,

        shadow_blur: 10.0,
        shadow_offset_y: 2.0,

        gradient_controls: true,
        bottom_edge_stroke: false,
        press_shrink: 0.0,
    }
}

fn typography() -> Typography {
    let mut t = typography_from(
        13.0,
        vec!["SF Pro Text", "Helvetica Neue", "Hiragino Sans"],
        vec!["SF Mono", "Menlo"],
    );
    t.subtitle.size = 17.0;
    t.subtitle.weight = FontWeight::SemiBold;
    t.title.size = 26.0;
    t.title.weight = FontWeight::Bold;
    t.caption.size = 11.0;
    t.control.weight = FontWeight::Medium;
    t
}
