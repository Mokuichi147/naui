//! Windows 11 / WinUI 3 (Fluent 2) のトークン。
//!
//! 値は WinUI 3 の共通リソース (`ControlFillColorDefault`,
//! `TextFillColorPrimary`, `AccentFillColorDefault` など) に対応させてある。
//! 半透明の指定はそのままアルファ付きの色として保持し、
//! ラスタライザ側で下地と合成する。

use miui_core::color::Color;
use miui_core::painter::FontWeight;
use miui_core::theme::{
    typography_from, ColorMode, Metrics, Palette, PlatformStyle, Theme, Typography,
};

pub fn theme(mode: ColorMode) -> Theme {
    Theme {
        style: PlatformStyle::Fluent,
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
        // Mica / Layer 相当。
        window_bg: Color::hex(0xF3F3F3),
        surface: Color::hexa(0xFFFFFF, 0.70),
        surface_sunken: Color::hexa(0xF9F9F9, 0.50),

        control: Color::hexa(0xFFFFFF, 0.70),
        control_hover: Color::hexa(0xF9F9F9, 0.50),
        control_active: Color::hexa(0xF9F9F9, 0.30),
        control_disabled: Color::hexa(0xF9F9F9, 0.30),
        // Fluent のオフ状態は「ほぼ透明 + 枠線」。つまみは text_secondary で描く。
        switch_track_off: Color::hexa(0x000000, 0.024),

        // ControlStrokeColorDefault / Secondary。
        border: Color::hexa(0x000000, 0.0578),
        border_strong: Color::hexa(0x000000, 0.1622),
        divider: Color::hexa(0x000000, 0.0803),

        text: Color::hexa(0x000000, 0.8956),
        text_secondary: Color::hexa(0x000000, 0.6063),
        text_disabled: Color::hexa(0x000000, 0.3614),
        text_on_accent: Color::hex(0xFFFFFF),

        // SystemAccentColorDark1 系。
        accent: Color::hex(0x005FB8),
        accent_hover: Color::hexa(0x005FB8, 0.90),
        accent_active: Color::hexa(0x005FB8, 0.80),
        accent_subtle: Color::hexa(0x0078D4, 0.10),

        danger: Color::hex(0xC42B1C),
        success: Color::hex(0x0F7B0F),
        warning: Color::hex(0x9D5D00),

        focus_ring: Color::hexa(0x000000, 0.8956),
        focus_ring_inner: Color::hex(0xFFFFFF),

        shadow: Color::hexa(0x000000, 0.13),
    }
}

fn dark() -> Palette {
    Palette {
        window_bg: Color::hex(0x202020),
        surface: Color::hexa(0xFFFFFF, 0.0538),
        surface_sunken: Color::hexa(0xFFFFFF, 0.0326),

        control: Color::hexa(0xFFFFFF, 0.0605),
        control_hover: Color::hexa(0xFFFFFF, 0.0837),
        control_active: Color::hexa(0xFFFFFF, 0.0326),
        control_disabled: Color::hexa(0xFFFFFF, 0.0419),
        switch_track_off: Color::hexa(0xFFFFFF, 0.024),

        border: Color::hexa(0xFFFFFF, 0.0698),
        border_strong: Color::hexa(0xFFFFFF, 0.0930),
        divider: Color::hexa(0xFFFFFF, 0.0837),

        text: Color::hex(0xFFFFFF),
        text_secondary: Color::hexa(0xFFFFFF, 0.7860),
        text_disabled: Color::hexa(0xFFFFFF, 0.3628),
        // ダークテーマのアクセントは明るいため、上に載る文字は黒。
        text_on_accent: Color::hexa(0x000000, 0.9),

        accent: Color::hex(0x60CDFF),
        accent_hover: Color::hexa(0x60CDFF, 0.90),
        accent_active: Color::hexa(0x60CDFF, 0.80),
        accent_subtle: Color::hexa(0x60CDFF, 0.12),

        danger: Color::hex(0xFF99A4),
        success: Color::hex(0x6CCB5F),
        warning: Color::hex(0xFCE100),

        focus_ring: Color::hex(0xFFFFFF),
        focus_ring_inner: Color::hexa(0x000000, 0.7),

        shadow: Color::hexa(0x000000, 0.37),
    }
}

fn metrics() -> Metrics {
    Metrics {
        control_height: 32.0,
        control_radius: 4.0,
        surface_radius: 8.0,
        control_padding_x: 12.0,
        border_width: 1.0,
        focus_ring_width: 2.0,
        focus_ring_offset: 1.0,

        spacing_xs: 4.0,
        spacing_sm: 8.0,
        spacing_md: 12.0,
        spacing_lg: 20.0,

        checkbox_size: 20.0,
        checkbox_radius: 4.0,
        switch_width: 40.0,
        switch_height: 20.0,
        slider_track: 4.0,
        slider_thumb: 20.0,
        scrollbar_width: 6.0,

        shadow_blur: 16.0,
        shadow_offset_y: 4.0,

        gradient_controls: false,
        // Fluent の特徴である「下辺だけ濃いストローク」。
        bottom_edge_stroke: true,
        press_shrink: 0.0,
    }
}

fn typography() -> Typography {
    let mut t = typography_from(
        14.0,
        vec!["Segoe UI Variable Text", "Segoe UI", "Yu Gothic UI"],
        vec!["Cascadia Mono", "Consolas"],
    );
    // Fluent の Body Strong / Subtitle / Title。
    t.subtitle.size = 20.0;
    t.subtitle.weight = FontWeight::SemiBold;
    t.title.size = 28.0;
    t.title.weight = FontWeight::SemiBold;
    t.caption.size = 12.0;
    t
}
