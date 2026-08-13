//! デザイントークンの「型」定義。
//!
//! 実際の値 (Fluent / macOS / Adwaita / Web) は `miui-theme` クレートが持つ。
//! ここで型だけを定義することで、ウィジェットはプラットフォームを知らずに描ける。

use crate::color::Color;
use crate::painter::{FontWeight, TextStyle};

/// 各プラットフォームの最新デザイン言語。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PlatformStyle {
    /// Windows 11 / WinUI 3 (Fluent 2)。
    Fluent,
    /// macOS (Aqua 系の現行デザイン)。
    Cupertino,
    /// GNOME / libadwaita。
    Adwaita,
    /// Web 既定 (ホスト OS 不明時のニュートラル)。
    Web,
}

impl PlatformStyle {
    /// ビルドターゲットから既定のスタイルを選ぶ。
    pub fn detect() -> PlatformStyle {
        #[cfg(target_arch = "wasm32")]
        {
            PlatformStyle::Web
        }
        #[cfg(all(not(target_arch = "wasm32"), target_os = "windows"))]
        {
            PlatformStyle::Fluent
        }
        #[cfg(all(not(target_arch = "wasm32"), target_os = "macos"))]
        {
            PlatformStyle::Cupertino
        }
        #[cfg(all(
            not(target_arch = "wasm32"),
            not(target_os = "windows"),
            not(target_os = "macos")
        ))]
        {
            PlatformStyle::Adwaita
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            PlatformStyle::Fluent => "Fluent 2 (WinUI 3)",
            PlatformStyle::Cupertino => "macOS",
            PlatformStyle::Adwaita => "Adwaita (GNOME)",
            PlatformStyle::Web => "Web",
        }
    }
}

/// ライト / ダーク。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ColorMode {
    Light,
    Dark,
}

impl ColorMode {
    pub fn is_dark(self) -> bool {
        matches!(self, ColorMode::Dark)
    }

    pub fn toggled(self) -> ColorMode {
        match self {
            ColorMode::Light => ColorMode::Dark,
            ColorMode::Dark => ColorMode::Light,
        }
    }
}

/// 色トークン。
#[derive(Debug, Clone, PartialEq)]
pub struct Palette {
    /// ウィンドウの地。
    pub window_bg: Color,
    /// カード / パネル面。
    pub surface: Color,
    /// 一段沈んだ面 (入力欄の背景など)。
    pub surface_sunken: Color,

    /// 通常コントロールの地。
    pub control: Color,
    pub control_hover: Color,
    pub control_active: Color,
    pub control_disabled: Color,
    /// スイッチのオフ状態のトラック。白いつまみと必ず区別が付く色にする
    /// (`control` は白いテーマがあるため、専用トークンとして持つ)。
    pub switch_track_off: Color,

    /// コントロールの枠線。
    pub border: Color,
    /// 立体感を出すための強い枠 (Fluent の下辺、macOS の上辺など)。
    pub border_strong: Color,
    pub divider: Color,

    pub text: Color,
    pub text_secondary: Color,
    pub text_disabled: Color,
    /// アクセント面の上に載るテキスト。
    pub text_on_accent: Color,

    pub accent: Color,
    pub accent_hover: Color,
    pub accent_active: Color,
    /// 選択行の背景など、薄いアクセント。
    pub accent_subtle: Color,

    pub danger: Color,
    pub success: Color,
    pub warning: Color,

    pub focus_ring: Color,
    /// Fluent の二重フォーカスリングの内側 (他スタイルでは透明)。
    pub focus_ring_inner: Color,

    pub shadow: Color,
}

/// 寸法トークン。
#[derive(Debug, Clone, PartialEq)]
pub struct Metrics {
    /// 標準的なコントロールの高さ。
    pub control_height: f32,
    /// コントロールの角丸。
    pub control_radius: f32,
    /// カード / ダイアログの角丸。
    pub surface_radius: f32,
    /// コントロール内の水平パディング。
    pub control_padding_x: f32,
    pub border_width: f32,
    pub focus_ring_width: f32,
    /// フォーカスリングをコントロール外側へ離す量。
    pub focus_ring_offset: f32,

    pub spacing_xs: f32,
    pub spacing_sm: f32,
    pub spacing_md: f32,
    pub spacing_lg: f32,

    pub checkbox_size: f32,
    pub checkbox_radius: f32,
    pub switch_width: f32,
    pub switch_height: f32,
    pub slider_track: f32,
    pub slider_thumb: f32,
    pub scrollbar_width: f32,

    pub shadow_blur: f32,
    pub shadow_offset_y: f32,

    /// コントロールに微グラデーションを掛ける (macOS 風)。
    pub gradient_controls: bool,
    /// 下辺だけ濃い枠を引く (Fluent のコントロールストローク)。
    pub bottom_edge_stroke: bool,
    /// 押下時にコントロールをわずかに縮める。
    pub press_shrink: f32,
}

/// 文字トークン。
#[derive(Debug, Clone, PartialEq)]
pub struct Typography {
    pub body: TextStyle,
    pub body_strong: TextStyle,
    pub caption: TextStyle,
    pub subtitle: TextStyle,
    pub title: TextStyle,
    /// ボタンなどコントロールのラベル。
    pub control: TextStyle,
    /// 優先フォントファミリ名 (先頭から順に解決を試みる)。
    pub families: Vec<&'static str>,
    /// 等幅フォントの候補。
    pub mono_families: Vec<&'static str>,
}

/// テーマ一式。
#[derive(Debug, Clone, PartialEq)]
pub struct Theme {
    pub style: PlatformStyle,
    pub mode: ColorMode,
    pub color: Palette,
    pub metrics: Metrics,
    pub typography: Typography,
}

impl Theme {
    /// 無効状態を考慮した文字色。
    pub fn text_color(&self, enabled: bool) -> Color {
        if enabled {
            self.color.text
        } else {
            self.color.text_disabled
        }
    }

    /// スタイルが Fluent かどうか (細部の描き分け用)。
    pub fn is_fluent(&self) -> bool {
        matches!(self.style, PlatformStyle::Fluent)
    }

    pub fn is_cupertino(&self) -> bool {
        matches!(self.style, PlatformStyle::Cupertino)
    }

    pub fn is_adwaita(&self) -> bool {
        matches!(self.style, PlatformStyle::Adwaita)
    }
}

/// 各テーマ実装が使う共通の文字サイズ生成ヘルパ。
pub fn typography_from(base: f32, families: Vec<&'static str>, mono: Vec<&'static str>) -> Typography {
    Typography {
        body: TextStyle::new(base),
        body_strong: TextStyle::new(base).with_weight(FontWeight::SemiBold),
        caption: TextStyle::new((base - 2.0).max(9.0)),
        subtitle: TextStyle::new(base + 6.0).with_weight(FontWeight::SemiBold),
        title: TextStyle::new(base + 14.0).with_weight(FontWeight::Bold),
        control: TextStyle::new(base),
        families,
        mono_families: mono,
    }
}
