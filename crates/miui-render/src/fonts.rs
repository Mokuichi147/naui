//! フォント読み込み・グリフキャッシュ・テキスト計測。
//!
//! 方針:
//! - フォントファイルの探索は OS ごとの候補パス表で行う (fontconfig 等に依存しない)。
//! - 太字ファイルが無い環境では、グリフのカバレッジを水平方向に膨張させて合成する。
//! - 1 つのフォントに無い文字は、後続のフォールバックフォントへ順に問い合わせる
//!   (日本語などの CJK はこの経路で描画される)。

use std::collections::HashMap;

use fontdue::{Font, FontSettings};
use miui_core::painter::{FontFamily, FontWeight, LineMetrics, TextMeasurer, TextStyle};

/// 読み込み済みのフォント 1 つ。
struct Face {
    font: Font,
    family: FontFamily,
    weight: FontWeight,
    /// 他に候補が無いときだけ使うフォールバックか。
    fallback: bool,
}

#[derive(PartialEq, Eq, Hash, Clone, Copy)]
struct GlyphKey {
    face: u16,
    ch: char,
    /// 物理ピクセルサイズ (1/4 px 単位に量子化)。
    px_q: u32,
    /// 合成ボールドの強さ (0..=255)。
    embolden: u8,
}

pub(crate) struct Glyph {
    pub(crate) width: usize,
    pub(crate) height: usize,
    /// ペン位置からのオフセット。
    pub(crate) left: i32,
    pub(crate) top: i32,
    pub(crate) coverage: Vec<u8>,
}

/// フォント集合。`Canvas` から借用して使う。
pub struct Fonts {
    faces: Vec<Face>,
    glyphs: HashMap<GlyphKey, Glyph>,
    advances: HashMap<(u16, char, u32), f32>,
    /// 論理 → 物理のスケール。グリフはこの倍率で焼く。
    scale: f32,
}

impl Default for Fonts {
    fn default() -> Self {
        Self::new()
    }
}

impl Fonts {
    pub fn new() -> Self {
        Self {
            faces: Vec::new(),
            glyphs: HashMap::new(),
            advances: HashMap::new(),
            scale: 1.0,
        }
    }

    /// OS 標準のフォントを探索して読み込む。
    pub fn with_system_fonts() -> Self {
        let mut fonts = Self::new();
        fonts.load_system_fonts();
        fonts
    }

    pub fn set_scale(&mut self, scale: f32) {
        if (scale - self.scale).abs() > f32::EPSILON {
            self.scale = scale;
            self.glyphs.clear();
            self.advances.clear();
        }
    }

    pub fn is_empty(&self) -> bool {
        self.faces.is_empty()
    }

    pub fn face_count(&self) -> usize {
        self.faces.len()
    }

    /// バイト列からフォントを登録する。Web (wasm) ではこれが唯一の供給経路。
    pub fn register_bytes(
        &mut self,
        bytes: &[u8],
        family: FontFamily,
        weight: FontWeight,
        fallback: bool,
    ) -> bool {
        self.register_bytes_indexed(bytes, family, weight, fallback, 0)
    }

    /// TrueType Collection (.ttc) 内のインデックスを指定して登録する。
    pub fn register_bytes_indexed(
        &mut self,
        bytes: &[u8],
        family: FontFamily,
        weight: FontWeight,
        fallback: bool,
        collection_index: u32,
    ) -> bool {
        let settings = FontSettings {
            collection_index,
            scale: 64.0,
            ..FontSettings::default()
        };
        match Font::from_bytes(bytes, settings) {
            Ok(font) => {
                self.faces.push(Face {
                    font,
                    family,
                    weight,
                    fallback,
                });
                true
            }
            Err(_) => false,
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn register_path(
        &mut self,
        path: &str,
        family: FontFamily,
        weight: FontWeight,
        fallback: bool,
        collection_index: u32,
    ) -> bool {
        match std::fs::read(path) {
            Ok(bytes) => {
                self.register_bytes_indexed(&bytes, family, weight, fallback, collection_index)
            }
            Err(_) => false,
        }
    }

    /// プラットフォームごとの候補パスを順に試す。
    #[cfg(not(target_arch = "wasm32"))]
    pub fn load_system_fonts(&mut self) {
        for (path, family, weight, fallback, index) in system_font_candidates() {
            self.register_path(path, family, weight, fallback, index);
        }
    }

    #[cfg(target_arch = "wasm32")]
    pub fn load_system_fonts(&mut self) {
        // Web ではファイルシステムから読めないため、
        // `register_bytes` による明示登録のみを受け付ける。
    }

    /// 指定の文字を描けるフェイスを選ぶ。返り値は (index, 合成ボールド量)。
    fn resolve(&self, ch: char, family: FontFamily, weight: FontWeight) -> Option<(u16, f32)> {
        let mut best: Option<(u16, FontWeight)> = None;
        // 1) 同じファミリで、要求以下の最大の太さ。
        for (i, face) in self.faces.iter().enumerate() {
            if face.family != family || face.fallback {
                continue;
            }
            if face.font.lookup_glyph_index(ch) == 0 {
                continue;
            }
            if face.weight > weight {
                continue;
            }
            match best {
                Some((_, w)) if w >= face.weight => {}
                _ => best = Some((i as u16, face.weight)),
            }
        }
        // 2) 同じファミリなら太さ不問。
        if best.is_none() {
            for (i, face) in self.faces.iter().enumerate() {
                if face.family == family && !face.fallback && face.font.lookup_glyph_index(ch) != 0 {
                    best = Some((i as u16, face.weight));
                    break;
                }
            }
        }
        // 3) フォールバック (CJK など) を含めて総当たり。
        if best.is_none() {
            for (i, face) in self.faces.iter().enumerate() {
                if face.font.lookup_glyph_index(ch) != 0 {
                    best = Some((i as u16, face.weight));
                    break;
                }
            }
        }
        // 4) それでも無ければ先頭 (.notdef を描く)。
        let (idx, face_weight) = best.or_else(|| self.faces.first().map(|f| (0u16, f.weight)))?;
        let deficit = weight.numeric().saturating_sub(face_weight.numeric()) as f32;
        // 100 の重み差につき 0.28px 膨張させる。
        let embolden = (deficit / 100.0) * 0.28;
        Some((idx, embolden))
    }

    pub(crate) fn glyph(&mut self, ch: char, style: &TextStyle) -> Option<&Glyph> {
        let px = style.size * self.scale;
        let (face, embolden) = self.resolve(ch, style.family, style.weight)?;
        let embolden_px = embolden * self.scale;
        let key = GlyphKey {
            face,
            ch,
            px_q: (px * 4.0) as u32,
            embolden: (embolden_px * 64.0).min(255.0) as u8,
        };
        if !self.glyphs.contains_key(&key) {
            let font = &self.faces[face as usize].font;
            let (metrics, bitmap) = font.rasterize(ch, px);
            let mut glyph = Glyph {
                width: metrics.width,
                height: metrics.height,
                left: metrics.xmin,
                top: -(metrics.ymin + metrics.height as i32),
                coverage: bitmap,
            };
            if embolden_px > 0.01 && glyph.width > 0 {
                embolden_bitmap(&mut glyph, embolden_px);
            }
            self.glyphs.insert(key, glyph);
        }
        self.glyphs.get(&key)
    }

    fn advance(&mut self, ch: char, style: &TextStyle) -> f32 {
        let px = style.size * self.scale;
        let Some((face, embolden)) = self.resolve(ch, style.family, style.weight) else {
            return 0.0;
        };
        let key = (face, ch, (px * 4.0) as u32);
        let base = if let Some(a) = self.advances.get(&key) {
            *a
        } else {
            let a = self.faces[face as usize].font.metrics(ch, px).advance_width;
            self.advances.insert(key, a);
            a
        };
        (base + embolden * self.scale) / self.scale
    }

    fn kern(&mut self, prev: char, ch: char, style: &TextStyle) -> f32 {
        let px = style.size * self.scale;
        let Some((face, _)) = self.resolve(ch, style.family, style.weight) else {
            return 0.0;
        };
        let Some((prev_face, _)) = self.resolve(prev, style.family, style.weight) else {
            return 0.0;
        };
        if prev_face != face {
            return 0.0;
        }
        self.faces[face as usize]
            .font
            .horizontal_kern(prev, ch, px)
            .unwrap_or(0.0)
            / self.scale
    }

    /// 1 文字ずつの (バイト位置, ペン位置 x) を返す。
    pub fn advances_of(&mut self, text: &str, style: &TextStyle) -> Vec<(usize, f32)> {
        let mut out = Vec::with_capacity(text.len() + 1);
        let mut x = 0.0f32;
        let mut prev: Option<char> = None;
        for (i, ch) in text.char_indices() {
            if let Some(p) = prev {
                x += self.kern(p, ch, style);
            }
            out.push((i, x));
            x += self.advance(ch, style) + style.letter_spacing;
            prev = Some(ch);
        }
        out.push((text.len(), x));
        out
    }

}

/// カバレッジを水平方向に膨張させて疑似的な太字を作る。
fn embolden_bitmap(glyph: &mut Glyph, amount_px: f32) {
    let extra = amount_px.ceil().max(1.0) as usize;
    let frac = (amount_px / extra as f32).clamp(0.0, 1.0);
    let new_w = glyph.width + extra;
    let mut out = vec![0u8; new_w * glyph.height];
    for y in 0..glyph.height {
        for x in 0..new_w {
            let mut v = 0u32;
            for k in 0..=extra {
                let sx = x as isize - k as isize;
                if sx < 0 || sx >= glyph.width as isize {
                    continue;
                }
                let s = glyph.coverage[y * glyph.width + sx as usize] as u32;
                let s = if k == extra {
                    (s as f32 * frac) as u32
                } else {
                    s
                };
                v = v.max(s);
            }
            out[y * new_w + x] = v.min(255) as u8;
        }
    }
    glyph.width = new_w;
    glyph.coverage = out;
}

impl TextMeasurer for Fonts {
    fn measure_line(&mut self, text: &str, style: &TextStyle) -> f32 {
        if text.is_empty() {
            return 0.0;
        }
        let adv = self.advances_of(text, style);
        // 末尾の letter_spacing は幅に含めない。
        (adv.last().map(|(_, x)| *x).unwrap_or(0.0) - style.letter_spacing).max(0.0)
    }

    fn line_metrics(&mut self, style: &TextStyle) -> LineMetrics {
        let px = style.size * self.scale;
        // 代表的な字面から縦メトリクスを得る。
        let face = self
            .faces
            .iter()
            .position(|f| f.family == style.family && !f.fallback)
            .or(if self.faces.is_empty() { None } else { Some(0) });
        let Some(face) = face else {
            return LineMetrics {
                ascent: style.size * 0.8,
                descent: style.size * 0.2,
                line_height: style.size * 1.4,
            };
        };
        let lm = self.faces[face].font.horizontal_line_metrics(px);
        match lm {
            Some(m) => LineMetrics {
                ascent: m.ascent / self.scale,
                descent: -m.descent / self.scale,
                line_height: (m.ascent - m.descent + m.line_gap) / self.scale,
            },
            None => LineMetrics {
                ascent: style.size * 0.8,
                descent: style.size * 0.2,
                line_height: style.size * 1.4,
            },
        }
    }

    fn wrap_lines(&mut self, text: &str, style: &TextStyle, max_width: f32) -> Vec<(usize, usize)> {
        let mut lines = Vec::new();
        if text.is_empty() {
            lines.push((0, 0));
            return lines;
        }
        let mut line_start = 0usize;
        let mut last_break: Option<usize> = None;
        let mut x = 0.0f32;
        let mut prev: Option<char> = None;

        for (i, ch) in text.char_indices() {
            if ch == '\n' {
                lines.push((line_start, i));
                line_start = i + 1;
                last_break = None;
                x = 0.0;
                prev = None;
                continue;
            }
            if let Some(p) = prev {
                x += self.kern(p, ch, style);
            }
            let w = self.advance(ch, style) + style.letter_spacing;
            // 分割可能位置: 空白の直後、および CJK 文字の直前。
            if is_break_opportunity(prev, ch) {
                last_break = Some(i);
            }
            if max_width.is_finite() && x + w > max_width && i > line_start {
                let brk = last_break.filter(|b| *b > line_start).unwrap_or(i);
                let end = trim_end(text, line_start, brk);
                lines.push((line_start, end));
                line_start = skip_leading_space(text, brk);
                last_break = None;
                x = 0.0;
                prev = None;
                if line_start > i {
                    continue;
                }
                // 折り返し後の行頭として現在の文字を再計算する。
                x = self.advance(ch, style) + style.letter_spacing;
                prev = Some(ch);
                continue;
            }
            x += w;
            prev = Some(ch);
        }
        lines.push((line_start, text.len()));
        lines
    }
}

fn is_break_opportunity(prev: Option<char>, ch: char) -> bool {
    if let Some(p) = prev {
        if p == ' ' || p == '\t' || p == '-' || p == '/' {
            return true;
        }
    }
    // CJK は文字境界で折り返せる (行頭禁則は未実装)。
    is_cjk(ch) && prev.map(is_cjk).unwrap_or(false)
}

fn is_cjk(ch: char) -> bool {
    matches!(ch as u32,
        0x2E80..=0x9FFF | 0xAC00..=0xD7AF | 0xF900..=0xFAFF | 0xFF00..=0xFF60 | 0xFFE0..=0xFFE6)
}

fn trim_end(text: &str, start: usize, end: usize) -> usize {
    let mut e = end;
    while e > start {
        let prev = text[..e].chars().next_back();
        match prev {
            Some(c) if c == ' ' || c == '\t' => e -= c.len_utf8(),
            _ => break,
        }
    }
    e
}

fn skip_leading_space(text: &str, mut i: usize) -> usize {
    while i < text.len() {
        match text[i..].chars().next() {
            Some(c) if c == ' ' || c == '\t' => i += c.len_utf8(),
            _ => break,
        }
    }
    i
}

/// (パス, ファミリ, 太さ, フォールバックか, コレクション index)
#[cfg(not(target_arch = "wasm32"))]
fn system_font_candidates() -> Vec<(&'static str, FontFamily, FontWeight, bool, u32)> {
    #[cfg(target_os = "macos")]
    {
        vec![
            ("/System/Library/Fonts/SFNS.ttf", FontFamily::Sans, FontWeight::Regular, false, 0),
            ("/System/Library/Fonts/Helvetica.ttc", FontFamily::Sans, FontWeight::Regular, true, 0),
            ("/System/Library/Fonts/SFNSMono.ttf", FontFamily::Mono, FontWeight::Regular, false, 0),
            ("/System/Library/Fonts/Menlo.ttc", FontFamily::Mono, FontWeight::Regular, true, 0),
            // 日本語フォールバック。
            ("/System/Library/Fonts/ヒラギノ角ゴシック W3.ttc", FontFamily::Sans, FontWeight::Regular, true, 0),
            ("/System/Library/Fonts/ヒラギノ角ゴシック W6.ttc", FontFamily::Sans, FontWeight::Bold, true, 0),
            ("/System/Library/Fonts/Apple Symbols.ttf", FontFamily::Sans, FontWeight::Regular, true, 0),
        ]
    }
    #[cfg(target_os = "windows")]
    {
        vec![
            // Windows 11 / WinUI 3 の標準 UI フォント。
            ("C:\\Windows\\Fonts\\SegUIVar.ttf", FontFamily::Sans, FontWeight::Regular, false, 0),
            ("C:\\Windows\\Fonts\\segoeui.ttf", FontFamily::Sans, FontWeight::Regular, false, 0),
            ("C:\\Windows\\Fonts\\seguisb.ttf", FontFamily::Sans, FontWeight::SemiBold, false, 0),
            ("C:\\Windows\\Fonts\\segoeuib.ttf", FontFamily::Sans, FontWeight::Bold, false, 0),
            ("C:\\Windows\\Fonts\\CascadiaMono.ttf", FontFamily::Mono, FontWeight::Regular, false, 0),
            ("C:\\Windows\\Fonts\\consola.ttf", FontFamily::Mono, FontWeight::Regular, true, 0),
            ("C:\\Windows\\Fonts\\YuGothM.ttc", FontFamily::Sans, FontWeight::Regular, true, 0),
            ("C:\\Windows\\Fonts\\meiryo.ttc", FontFamily::Sans, FontWeight::Regular, true, 0),
        ]
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        vec![
            // GNOME / libadwaita の標準は Cantarell、次点で Inter / DejaVu。
            ("/usr/share/fonts/cantarell/Cantarell-VF.otf", FontFamily::Sans, FontWeight::Regular, false, 0),
            ("/usr/share/fonts/truetype/cantarell/Cantarell-Regular.otf", FontFamily::Sans, FontWeight::Regular, false, 0),
            ("/usr/share/fonts/truetype/cantarell/Cantarell-Bold.otf", FontFamily::Sans, FontWeight::Bold, false, 0),
            ("/usr/share/fonts/truetype/inter/Inter-Regular.ttf", FontFamily::Sans, FontWeight::Regular, false, 0),
            ("/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf", FontFamily::Sans, FontWeight::Regular, true, 0),
            ("/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf", FontFamily::Sans, FontWeight::Bold, true, 0),
            ("/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf", FontFamily::Mono, FontWeight::Regular, false, 0),
            ("/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc", FontFamily::Sans, FontWeight::Regular, true, 0),
            ("/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc", FontFamily::Sans, FontWeight::Regular, true, 0),
        ]
    }
}
