//! GNOME / libadwaita のトークン。
//!
//! 枠線をほとんど使わず、地の色に対する半透明のオーバーレイで階層を作る
//! のが libadwaita の流儀なので、コントロール色はアルファ付きで持つ。

use miui_core::color::Color;
use miui_core::painter::FontWeight;
use miui_core::theme::{
    typography_from, ColorMode, Metrics, Palette, PlatformStyle, Theme, Typography,
};

pub fn theme(mode: ColorMode) -> Theme {
    Theme {
        style: PlatformStyle::Adwaita,
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
        window_bg: Color::hex(0xFAFAFA),
        surface: Color::hex(0xFFFFFF),
        surface_sunken: Color::hexa(0x000000, 0.04),

        control: Color::hexa(0x000000, 0.07),
        control_hover: Color::hexa(0x000000, 0.12),
        control_active: Color::hexa(0x000000, 0.18),
        control_disabled: Color::hexa(0x000000, 0.04),
        switch_track_off: Color::hexa(0x000000, 0.15),

        border: Color::hexa(0x000000, 0.10),
        border_strong: Color::hexa(0x000000, 0.15),
        divider: Color::hexa(0x000000, 0.10),

        text: Color::hexa(0x000000, 0.80),
        text_secondary: Color::hexa(0x000000, 0.55),
        text_disabled: Color::hexa(0x000000, 0.35),
        text_on_accent: Color::hex(0xFFFFFF),

        accent: Color::hex(0x3584E4),
        accent_hover: Color::hex(0x1C71D8),
        accent_active: Color::hex(0x1A5FB4),
        accent_subtle: Color::hexa(0x3584E4, 0.15),

        danger: Color::hex(0xE01B24),
        success: Color::hex(0x2EC27E),
        warning: Color::hex(0xE5A50A),

        focus_ring: Color::hexa(0x3584E4, 0.50),
        focus_ring_inner: Color::TRANSPARENT,

        shadow: Color::hexa(0x000000, 0.12),
    }
}

fn dark() -> Palette {
    Palette {
        window_bg: Color::hex(0x242424),
        surface: Color::hex(0x303030),
        surface_sunken: Color::hexa(0x000000, 0.20),

        control: Color::hexa(0xFFFFFF, 0.10),
        control_hover: Color::hexa(0xFFFFFF, 0.15),
        control_active: Color::hexa(0xFFFFFF, 0.20),
        control_disabled: Color::hexa(0xFFFFFF, 0.05),
        switch_track_off: Color::hexa(0xFFFFFF, 0.15),

        border: Color::hexa(0xFFFFFF, 0.12),
        border_strong: Color::hexa(0xFFFFFF, 0.18),
        divider: Color::hexa(0xFFFFFF, 0.12),

        text: Color::hexa(0xFFFFFF, 0.90),
        text_secondary: Color::hexa(0xFFFFFF, 0.60),
        text_disabled: Color::hexa(0xFFFFFF, 0.35),
        text_on_accent: Color::hex(0xFFFFFF),

        accent: Color::hex(0x3584E4),
        accent_hover: Color::hex(0x62A0EA),
        accent_active: Color::hex(0x1C71D8),
        accent_subtle: Color::hexa(0x78AEED, 0.20),

        danger: Color::hex(0xFF7B63),
        success: Color::hex(0x78E9AB),
        warning: Color::hex(0xF8E45C),

        focus_ring: Color::hexa(0x78AEED, 0.55),
        focus_ring_inner: Color::TRANSPARENT,

        shadow: Color::hexa(0x000000, 0.40),
    }
}

fn metrics() -> Metrics {
    Metrics {
        control_height: 34.0,
        control_radius: 6.0,
        surface_radius: 12.0,
        control_padding_x: 14.0,
        border_width: 1.0,
        focus_ring_width: 2.0,
        focus_ring_offset: 2.0,

        spacing_xs: 6.0,
        spacing_sm: 8.0,
        spacing_md: 12.0,
        spacing_lg: 24.0,

        checkbox_size: 20.0,
        checkbox_radius: 6.0,
        switch_width: 44.0,
        switch_height: 26.0,
        slider_track: 4.0,
        slider_thumb: 18.0,
        scrollbar_width: 8.0,

        shadow_blur: 12.0,
        shadow_offset_y: 2.0,

        gradient_controls: false,
        bottom_edge_stroke: false,
        press_shrink: 0.0,
    }
}

fn typography() -> Typography {
    let mut t = typography_from(
        15.0,
        vec!["Cantarell", "Inter", "Noto Sans"],
        vec!["Source Code Pro", "DejaVu Sans Mono"],
    );
    t.subtitle.size = 18.0;
    t.subtitle.weight = FontWeight::Bold;
    t.title.size = 24.0;
    t.title.weight = FontWeight::Bold;
    t.caption.size = 13.0;
    t.body_strong.weight = FontWeight::Bold;
    t
}
