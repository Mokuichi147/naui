//! 色。内部は sRGB のストレート (非乗算) アルファ、成分は 0.0..=1.0 の f32。

/// sRGB + アルファ。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Color {
    pub const TRANSPARENT: Color = Color::rgba(0.0, 0.0, 0.0, 0.0);
    pub const BLACK: Color = Color::rgb(0.0, 0.0, 0.0);
    pub const WHITE: Color = Color::rgb(1.0, 1.0, 1.0);

    pub const fn rgba(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }

    pub const fn rgb(r: f32, g: f32, b: f32) -> Self {
        Self::rgba(r, g, b, 1.0)
    }

    /// `0xRRGGBB` から生成する。
    pub const fn hex(v: u32) -> Self {
        Self::rgba(
            ((v >> 16) & 0xFF) as f32 / 255.0,
            ((v >> 8) & 0xFF) as f32 / 255.0,
            (v & 0xFF) as f32 / 255.0,
            1.0,
        )
    }

    /// `0xRRGGBB` + アルファ (0.0..=1.0) から生成する。
    pub const fn hexa(v: u32, a: f32) -> Self {
        Self::rgba(
            ((v >> 16) & 0xFF) as f32 / 255.0,
            ((v >> 8) & 0xFF) as f32 / 255.0,
            (v & 0xFF) as f32 / 255.0,
            a,
        )
    }

    pub const fn gray(v: f32) -> Self {
        Self::rgb(v, v, v)
    }

    pub fn with_alpha(self, a: f32) -> Self {
        Self { a, ..self }
    }

    /// アルファを倍率で変化させる。
    pub fn scale_alpha(self, k: f32) -> Self {
        Self {
            a: (self.a * k).clamp(0.0, 1.0),
            ..self
        }
    }

    pub fn is_transparent(&self) -> bool {
        self.a <= 0.001
    }

    /// `t` = 0 で self、1 で other。
    pub fn lerp(self, other: Color, t: f32) -> Color {
        let t = t.clamp(0.0, 1.0);
        Color::rgba(
            self.r + (other.r - self.r) * t,
            self.g + (other.g - self.g) * t,
            self.b + (other.b - self.b) * t,
            self.a + (other.a - self.a) * t,
        )
    }

    /// 明度を上げる (白に寄せる)。
    pub fn lighten(self, t: f32) -> Color {
        self.lerp(Color::rgba(1.0, 1.0, 1.0, self.a), t)
    }

    /// 明度を下げる (黒に寄せる)。
    pub fn darken(self, t: f32) -> Color {
        self.lerp(Color::rgba(0.0, 0.0, 0.0, self.a), t)
    }

    /// 相対輝度 (WCAG)。前景色の自動選択などに使う。
    pub fn luminance(&self) -> f32 {
        fn lin(c: f32) -> f32 {
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        }
        0.2126 * lin(self.r) + 0.7152 * lin(self.g) + 0.0722 * lin(self.b)
    }

    /// 背景として見たとき、可読なテキスト色 (黒 or 白) を返す。
    pub fn readable_foreground(&self) -> Color {
        if self.luminance() > 0.45 {
            Color::hex(0x000000)
        } else {
            Color::hex(0xFFFFFF)
        }
    }

    /// `self` を `bg` の上に通常合成した不透明色。
    pub fn over(self, bg: Color) -> Color {
        let a = self.a + bg.a * (1.0 - self.a);
        if a <= 0.0 {
            return Color::TRANSPARENT;
        }
        Color::rgba(
            (self.r * self.a + bg.r * bg.a * (1.0 - self.a)) / a,
            (self.g * self.a + bg.g * bg.a * (1.0 - self.a)) / a,
            (self.b * self.a + bg.b * bg.a * (1.0 - self.a)) / a,
            a,
        )
    }

    /// 0xAARRGGBB (ストレートアルファ) に変換する。
    pub fn to_argb8(self) -> u32 {
        let f = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u32;
        (f(self.a) << 24) | (f(self.r) << 16) | (f(self.g) << 8) | f(self.b)
    }
}

/// 塗りブラシ。ソフトウェアラスタライザで安価に扱える種類だけを持つ。
#[derive(Debug, Clone, PartialEq)]
pub enum Brush {
    /// 単色。
    Solid(Color),
    /// 垂直方向の 2 色グラデーション (上 → 下)。
    VerticalGradient { top: Color, bottom: Color },
}

impl Brush {
    pub fn solid(c: Color) -> Brush {
        Brush::Solid(c)
    }

    /// 0.0 (上端) ..= 1.0 (下端) における色。
    pub fn sample(&self, t: f32) -> Color {
        match self {
            Brush::Solid(c) => *c,
            Brush::VerticalGradient { top, bottom } => top.lerp(*bottom, t),
        }
    }

    pub fn is_transparent(&self) -> bool {
        match self {
            Brush::Solid(c) => c.is_transparent(),
            Brush::VerticalGradient { top, bottom } => top.is_transparent() && bottom.is_transparent(),
        }
    }
}

impl From<Color> for Brush {
    fn from(c: Color) -> Self {
        Brush::Solid(c)
    }
}
