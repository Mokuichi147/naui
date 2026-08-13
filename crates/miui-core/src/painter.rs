//! 描画バックエンドが実装するインタフェース。
//!
//! ウィジェットはここで定義された数種類のプリミティブだけで表現する。
//! 角丸矩形 (SDF) + 折れ線 + テキストの 3 つで、Fluent / macOS / Adwaita の
//! いずれのスタイルも描けるように意図的に絞ってある。

use crate::color::{Brush, Color};
use crate::geometry::{Corners, Point, Rect, Size};

/// フォントの太さ。実ファイルが無い場合はバックエンドが合成する。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FontWeight {
    Regular,
    Medium,
    SemiBold,
    Bold,
}

impl FontWeight {
    pub fn numeric(self) -> u16 {
        match self {
            FontWeight::Regular => 400,
            FontWeight::Medium => 500,
            FontWeight::SemiBold => 600,
            FontWeight::Bold => 700,
        }
    }
}

/// フォントファミリの種別。実際のフォント選択はテーマ + バックエンドが行う。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FontFamily {
    /// UI 用サンセリフ (各 OS のシステムフォント)。
    Sans,
    /// 等幅。
    Mono,
}

/// テキストの描画スタイル。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextStyle {
    pub size: f32,
    pub weight: FontWeight,
    pub family: FontFamily,
    /// 字間の追加量 (論理ピクセル)。
    pub letter_spacing: f32,
}

impl TextStyle {
    pub const fn new(size: f32) -> Self {
        Self {
            size,
            weight: FontWeight::Regular,
            family: FontFamily::Sans,
            letter_spacing: 0.0,
        }
    }

    pub const fn with_weight(mut self, weight: FontWeight) -> Self {
        self.weight = weight;
        self
    }

    pub const fn with_family(mut self, family: FontFamily) -> Self {
        self.family = family;
        self
    }

    pub const fn with_letter_spacing(mut self, spacing: f32) -> Self {
        self.letter_spacing = spacing;
        self
    }

    /// キャッシュキー用の量子化した表現。
    pub fn key(&self) -> (u32, FontWeight, FontFamily) {
        ((self.size * 64.0) as u32, self.weight, self.family)
    }
}

impl Default for TextStyle {
    fn default() -> Self {
        TextStyle::new(14.0)
    }
}

/// 1 行分の縦方向メトリクス。
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct LineMetrics {
    /// ベースラインから上端まで (正値)。
    pub ascent: f32,
    /// ベースラインから下端まで (正値)。
    pub descent: f32,
    /// 推奨行送り。
    pub line_height: f32,
}

/// 水平方向の揃え。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextAlign {
    Start,
    Center,
    End,
}

/// テキスト計測。レイアウト時 (描画前) にも必要なので描画とは分離してある。
pub trait TextMeasurer {
    /// 1 行として描いたときの幅。
    fn measure_line(&mut self, text: &str, style: &TextStyle) -> f32;

    fn line_metrics(&mut self, style: &TextStyle) -> LineMetrics;

    /// `max_width` で折り返したときの各行 (バイト範囲) を返す。
    fn wrap_lines(&mut self, text: &str, style: &TextStyle, max_width: f32) -> Vec<(usize, usize)>;

    /// 折り返し後の全体サイズ。
    fn measure_block(&mut self, text: &str, style: &TextStyle, max_width: f32) -> Size {
        let lines = self.wrap_lines(text, style, max_width);
        let m = self.line_metrics(style);
        let mut w: f32 = 0.0;
        for (s, e) in &lines {
            w = w.max(self.measure_line(&text[*s..*e], style));
        }
        Size::new(w, m.line_height * lines.len().max(1) as f32)
    }

    /// 行頭からの x 座標が `x` のとき、最も近い文字境界のバイトオフセット。
    fn index_at_x(&mut self, text: &str, style: &TextStyle, x: f32) -> usize {
        let mut best = 0usize;
        let mut best_d = f32::MAX;
        for (i, _) in text.char_indices().chain(std::iter::once((text.len(), ' '))) {
            let w = self.measure_line(&text[..i], style);
            let d = (w - x).abs();
            if d < best_d {
                best_d = d;
                best = i;
            }
        }
        best
    }
}

/// 実際の描画命令。座標はすべてウィンドウ絶対座標 (論理ピクセル)。
pub trait Painter: TextMeasurer {
    /// 現在のクリップ矩形を積む。以降の描画はこの矩形に制限される。
    fn push_clip(&mut self, rect: Rect);
    fn pop_clip(&mut self);
    /// 現在のクリップ矩形。
    fn clip_rect(&self) -> Rect;

    /// 角丸矩形の塗り。
    fn fill_rrect(&mut self, rect: Rect, corners: Corners, brush: &Brush);

    /// 角丸矩形の線 (矩形の境界を中心に `width` の太さ)。
    fn stroke_rrect(&mut self, rect: Rect, corners: Corners, width: f32, brush: &Brush);

    /// 角丸矩形の外側に落ちる影。`blur` はぼけ幅、`spread` は形状の拡大量。
    fn shadow_rrect(&mut self, rect: Rect, corners: Corners, blur: f32, spread: f32, color: Color);

    /// 折れ線。チェックマークや矢印など、アイコン相当の描画に使う。
    fn stroke_polyline(&mut self, points: &[Point], width: f32, color: Color);

    /// 1 行テキストを描画する。`origin` は行ボックスの左上。
    fn draw_text(&mut self, origin: Point, text: &str, style: &TextStyle, color: Color);

    /// 矩形内にテキストを折り返して描画する。
    fn draw_text_block(
        &mut self,
        rect: Rect,
        text: &str,
        style: &TextStyle,
        color: Color,
        align: TextAlign,
    ) {
        let lines = self.wrap_lines(text, style, rect.width());
        let m = self.line_metrics(style);
        for (i, (s, e)) in lines.iter().enumerate() {
            let line = &text[*s..*e];
            let w = self.measure_line(line, style);
            let x = match align {
                TextAlign::Start => rect.min_x(),
                TextAlign::Center => rect.min_x() + (rect.width() - w) * 0.5,
                TextAlign::End => rect.max_x() - w,
            };
            let y = rect.min_y() + m.line_height * i as f32;
            self.draw_text(Point::new(x, y), line, style, color);
        }
    }

    /// 円の塗り。
    fn fill_circle(&mut self, center: Point, radius: f32, brush: &Brush) {
        let r = Rect::new(
            center.x - radius,
            center.y - radius,
            radius * 2.0,
            radius * 2.0,
        );
        self.fill_rrect(r, Corners::all(radius), brush);
    }

    /// 円の線。
    fn stroke_circle(&mut self, center: Point, radius: f32, width: f32, brush: &Brush) {
        let r = Rect::new(
            center.x - radius,
            center.y - radius,
            radius * 2.0,
            radius * 2.0,
        );
        self.stroke_rrect(r, Corners::all(radius), width, brush);
    }
}
