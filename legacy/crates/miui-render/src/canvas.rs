//! CPU ラスタライザ。GPU も外部描画ライブラリも使わない。
//!
//! 出力先は `0x00RRGGBB` の u32 バッファ (softbuffer / Canvas ImageData と同形式)。
//! すべての図形は `sdf` の距離関数から被覆率を求めて合成するため、
//! 塗り・線・影・アイコンが同じ品質のアンチエイリアスで描かれる。

use miui_core::color::{Brush, Color};
use miui_core::geometry::{Corners, Point, Rect};
use miui_core::painter::{LineMetrics, Painter, TextMeasurer, TextStyle};

use crate::fonts::{Fonts, Glyph};
use crate::sdf;

/// 物理ピクセル座標の矩形。
#[derive(Debug, Clone, Copy)]
struct PRect {
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
}

impl PRect {
    fn intersect(self, o: PRect) -> PRect {
        PRect {
            x0: self.x0.max(o.x0),
            y0: self.y0.max(o.y0),
            x1: self.x1.min(o.x1),
            y1: self.y1.min(o.y1),
        }
    }

    fn is_empty(self) -> bool {
        self.x1 <= self.x0 || self.y1 <= self.y0
    }

    fn inset(self, d: f32) -> PRect {
        PRect {
            x0: self.x0 + d,
            y0: self.y0 + d,
            x1: self.x1 - d,
            y1: self.y1 - d,
        }
    }

    fn contains(self, x: f32, y: f32) -> bool {
        x >= self.x0 && x < self.x1 && y >= self.y0 && y < self.y1
    }
}

/// 描画対象のフレームバッファ。
pub struct Canvas<'a> {
    buffer: &'a mut [u32],
    width: usize,
    height: usize,
    /// 論理 → 物理のスケール (HiDPI)。
    scale: f32,
    clips: Vec<PRect>,
    fonts: &'a mut Fonts,
}

impl<'a> Canvas<'a> {
    pub fn new(
        buffer: &'a mut [u32],
        width: usize,
        height: usize,
        scale: f32,
        fonts: &'a mut Fonts,
    ) -> Self {
        fonts.set_scale(scale);
        Self {
            buffer,
            width,
            height,
            scale,
            clips: vec![PRect {
                x0: 0.0,
                y0: 0.0,
                x1: width as f32,
                y1: height as f32,
            }],
            fonts,
        }
    }

    /// バッファ全体を単色で埋める。
    pub fn clear(&mut self, color: Color) {
        let v = pack(color.r * 255.0, color.g * 255.0, color.b * 255.0);
        self.buffer.fill(v);
    }

    fn clip(&self) -> PRect {
        *self.clips.last().expect("クリップスタックが空")
    }

    fn to_phys(&self, r: Rect) -> PRect {
        PRect {
            x0: r.min_x() * self.scale,
            y0: r.min_y() * self.scale,
            x1: r.max_x() * self.scale,
            y1: r.max_y() * self.scale,
        }
    }

    /// 領域を走査して被覆率を合成する共通ループ。
    ///
    /// `skip` に矩形を渡すと、その内部は評価しない (線の内側を省くのに使う)。
    fn shade(
        &mut self,
        bbox: PRect,
        skip: Option<PRect>,
        brush: &Brush,
        grad_span: (f32, f32),
        mut coverage: impl FnMut(f32, f32) -> f32,
    ) {
        let area = bbox.intersect(self.clip());
        if area.is_empty() {
            return;
        }
        let x_start = area.x0.floor().max(0.0) as usize;
        let x_end = (area.x1.ceil() as usize).min(self.width);
        let y_start = area.y0.floor().max(0.0) as usize;
        let y_end = (area.y1.ceil() as usize).min(self.height);
        if x_start >= x_end || y_start >= y_end {
            return;
        }

        let (grad_top, grad_height) = grad_span;
        let solid = matches!(brush, Brush::Solid(_));
        let mut color = brush.sample(0.0);

        for y in y_start..y_end {
            let py = y as f32 + 0.5;
            if !solid {
                let t = if grad_height > 0.0 {
                    (py - grad_top) / grad_height
                } else {
                    0.0
                };
                color = brush.sample(t.clamp(0.0, 1.0));
            }
            if color.a <= 0.0 {
                continue;
            }
            let cr = color.r * 255.0;
            let cg = color.g * 255.0;
            let cb = color.b * 255.0;
            let row = y * self.width;
            for x in x_start..x_end {
                let px = x as f32 + 0.5;
                if let Some(s) = skip {
                    if s.contains(px, py) {
                        continue;
                    }
                }
                if !area.contains(px, py) {
                    continue;
                }
                let a = coverage(px, py) * color.a;
                if a <= 0.002 {
                    continue;
                }
                blend(&mut self.buffer[row + x], cr, cg, cb, a);
            }
        }
    }

    /// 角丸なしの矩形を高速に塗る (背景など面積の大きい塗りに効く)。
    fn fill_plain_rect(&mut self, r: PRect, brush: &Brush) {
        let area = r.intersect(self.clip());
        if area.is_empty() {
            return;
        }
        let solid = matches!(brush, Brush::Solid(_));
        let y_start = area.y0.floor().max(0.0) as usize;
        let y_end = (area.y1.ceil() as usize).min(self.height);
        let x_start = area.x0.floor().max(0.0) as usize;
        let x_end = (area.x1.ceil() as usize).min(self.width);
        let mut color = brush.sample(0.0);
        let grad_top = r.y0;
        let grad_h = r.y1 - r.y0;

        for y in y_start..y_end {
            // 行方向の被覆率 (上下端のはみ出しを考慮)。
            let top = (y as f32).max(area.y0);
            let bottom = ((y + 1) as f32).min(area.y1);
            let cov_y = (bottom - top).clamp(0.0, 1.0);
            if cov_y <= 0.0 {
                continue;
            }
            if !solid {
                let t = if grad_h > 0.0 {
                    (y as f32 + 0.5 - grad_top) / grad_h
                } else {
                    0.0
                };
                color = brush.sample(t.clamp(0.0, 1.0));
            }
            if color.a <= 0.0 {
                continue;
            }
            let cr = color.r * 255.0;
            let cg = color.g * 255.0;
            let cb = color.b * 255.0;
            let row = y * self.width;
            for x in x_start..x_end {
                let left = (x as f32).max(area.x0);
                let right = ((x + 1) as f32).min(area.x1);
                let cov_x = (right - left).clamp(0.0, 1.0);
                let a = cov_x * cov_y * color.a;
                if a <= 0.002 {
                    continue;
                }
                blend(&mut self.buffer[row + x], cr, cg, cb, a);
            }
        }
    }
}

impl<'a> TextMeasurer for Canvas<'a> {
    fn measure_line(&mut self, text: &str, style: &TextStyle) -> f32 {
        self.fonts.measure_line(text, style)
    }
    fn line_metrics(&mut self, style: &TextStyle) -> LineMetrics {
        self.fonts.line_metrics(style)
    }
    fn wrap_lines(&mut self, text: &str, style: &TextStyle, max_width: f32) -> Vec<(usize, usize)> {
        self.fonts.wrap_lines(text, style, max_width)
    }
}

impl<'a> Painter for Canvas<'a> {
    fn push_clip(&mut self, rect: Rect) {
        let r = self.to_phys(rect).intersect(self.clip());
        self.clips.push(r);
    }

    fn pop_clip(&mut self) {
        if self.clips.len() > 1 {
            self.clips.pop();
        }
    }

    fn clip_rect(&self) -> Rect {
        let c = self.clip();
        Rect::new(
            c.x0 / self.scale,
            c.y0 / self.scale,
            (c.x1 - c.x0) / self.scale,
            (c.y1 - c.y0) / self.scale,
        )
    }

    fn fill_rrect(&mut self, rect: Rect, corners: Corners, brush: &Brush) {
        if rect.is_empty() || brush.is_transparent() {
            return;
        }
        let p = self.to_phys(rect);
        let c = corners.clamped(rect.size);
        if c.max_radius() * self.scale < 0.05 {
            self.fill_plain_rect(p, brush);
            return;
        }
        let s = self.scale;
        let radii = [
            c.top_left * s,
            c.top_right * s,
            c.bottom_right * s,
            c.bottom_left * s,
        ];
        let cx = (p.x0 + p.x1) * 0.5;
        let cy = (p.y0 + p.y1) * 0.5;
        let hw = (p.x1 - p.x0) * 0.5;
        let hh = (p.y1 - p.y0) * 0.5;
        self.shade(p, None, brush, (p.y0, p.y1 - p.y0), move |x, y| {
            sdf::coverage(sdf::round_rect(x - cx, y - cy, hw, hh, radii))
        });
    }

    fn stroke_rrect(&mut self, rect: Rect, corners: Corners, width: f32, brush: &Brush) {
        if rect.is_empty() || width <= 0.0 || brush.is_transparent() {
            return;
        }
        let p = self.to_phys(rect);
        let c = corners.clamped(rect.size);
        let s = self.scale;
        let w = (width * s).max(0.7);
        let radii = [
            c.top_left * s,
            c.top_right * s,
            c.bottom_right * s,
            c.bottom_left * s,
        ];
        let cx = (p.x0 + p.x1) * 0.5;
        let cy = (p.y0 + p.y1) * 0.5;
        let hw = (p.x1 - p.x0) * 0.5;
        let hh = (p.y1 - p.y0) * 0.5;
        // 内側は確実に線の範囲外なので走査から外す。
        let skip_inset = w + c.max_radius() * s + 1.0;
        let skip = if hw > skip_inset && hh > skip_inset {
            Some(p.inset(skip_inset))
        } else {
            None
        };
        self.shade(p, skip, brush, (p.y0, p.y1 - p.y0), move |x, y| {
            let d = sdf::round_rect(x - cx, y - cy, hw, hh, radii);
            (sdf::coverage(d) - sdf::coverage(d + w)).clamp(0.0, 1.0)
        });
    }

    fn shadow_rrect(&mut self, rect: Rect, corners: Corners, blur: f32, spread: f32, color: Color) {
        if rect.is_empty() || color.is_transparent() || blur <= 0.0 {
            return;
        }
        let s = self.scale;
        let p = self.to_phys(rect);
        let c = corners.clamped(rect.size);
        let blur_p = blur * s;
        let spread_p = spread * s;
        let radii = [
            (c.top_left + spread) * s,
            (c.top_right + spread) * s,
            (c.bottom_right + spread) * s,
            (c.bottom_left + spread) * s,
        ];
        let cx = (p.x0 + p.x1) * 0.5;
        let cy = (p.y0 + p.y1) * 0.5;
        let hw = (p.x1 - p.x0) * 0.5 + spread_p;
        let hh = (p.y1 - p.y0) * 0.5 + spread_p;
        let bbox = PRect {
            x0: cx - hw - blur_p,
            y0: cy - hh - blur_p,
            x1: cx + hw + blur_p,
            y1: cy + hh + blur_p,
        };
        let brush = Brush::Solid(color);
        self.shade(bbox, None, &brush, (bbox.y0, bbox.y1 - bbox.y0), move |x, y| {
            sdf::blurred_coverage(sdf::round_rect(x - cx, y - cy, hw, hh, radii), blur_p * 2.0)
        });
    }

    fn stroke_polyline(&mut self, points: &[Point], width: f32, color: Color) {
        if points.len() < 2 || color.is_transparent() {
            return;
        }
        let s = self.scale;
        let hw = (width * s * 0.5).max(0.35);
        let brush = Brush::Solid(color);
        for seg in points.windows(2) {
            let ax = seg[0].x * s;
            let ay = seg[0].y * s;
            let bx = seg[1].x * s;
            let by = seg[1].y * s;
            let bbox = PRect {
                x0: ax.min(bx) - hw - 1.0,
                y0: ay.min(by) - hw - 1.0,
                x1: ax.max(bx) + hw + 1.0,
                y1: ay.max(by) + hw + 1.0,
            };
            self.shade(bbox, None, &brush, (0.0, 0.0), move |x, y| {
                sdf::coverage(sdf::segment(x, y, ax, ay, bx, by) - hw)
            });
        }
    }

    fn draw_text(&mut self, origin: Point, text: &str, style: &TextStyle, color: Color) {
        if text.is_empty() || color.is_transparent() {
            return;
        }
        let metrics = self.line_metrics(style);
        let baseline = origin.y + metrics.ascent;
        let positions = self.fonts.advances_of(text, style);

        // フィールドごとに借用を分けるため分解する。
        let Canvas {
            buffer,
            width,
            height,
            scale,
            clips,
            fonts,
        } = self;
        let clip = *clips.last().expect("クリップスタックが空");
        let scale = *scale;
        let baseline_px = (baseline * scale).round();
        let mut target = Target {
            buffer,
            width: *width,
            height: *height,
            clip,
        };

        for ((_, dx), ch) in positions.iter().zip(text.chars()) {
            if ch == '\n' {
                continue;
            }
            let pen_x = ((origin.x + dx) * scale).round();
            let Some(glyph) = fonts.glyph(ch, style) else {
                continue;
            };
            blit_glyph(&mut target, glyph, pen_x as i32, baseline_px as i32, color);
        }
    }
}

/// グリフ合成先。`Canvas` のフィールドを個別に借用するためのまとめ。
struct Target<'a> {
    buffer: &'a mut [u32],
    width: usize,
    height: usize,
    clip: PRect,
}

/// グリフのカバレッジをバッファへ合成する。
fn blit_glyph(target: &mut Target, glyph: &Glyph, pen_x: i32, baseline_y: i32, color: Color) {
    if glyph.width == 0 || glyph.height == 0 {
        return;
    }
    let gx = pen_x + glyph.left;
    let gy = baseline_y + glyph.top;
    let cr = color.r * 255.0;
    let cg = color.g * 255.0;
    let cb = color.b * 255.0;

    let x_start = gx.max(target.clip.x0.ceil() as i32).max(0);
    let x_end = (gx + glyph.width as i32)
        .min(target.clip.x1.floor() as i32)
        .min(target.width as i32);
    let y_start = gy.max(target.clip.y0.ceil() as i32).max(0);
    let y_end = (gy + glyph.height as i32)
        .min(target.clip.y1.floor() as i32)
        .min(target.height as i32);
    if x_start >= x_end || y_start >= y_end {
        return;
    }

    for y in y_start..y_end {
        let src_row = (y - gy) as usize * glyph.width;
        let dst_row = y as usize * target.width;
        for x in x_start..x_end {
            let cov = glyph.coverage[src_row + (x - gx) as usize];
            if cov == 0 {
                continue;
            }
            let a = (cov as f32 / 255.0) * color.a;
            blend(&mut target.buffer[dst_row + x as usize], cr, cg, cb, a);
        }
    }
}

#[inline]
fn pack(r: f32, g: f32, b: f32) -> u32 {
    ((r.clamp(0.0, 255.0) as u32) << 16)
        | ((g.clamp(0.0, 255.0) as u32) << 8)
        | (b.clamp(0.0, 255.0) as u32)
}

#[inline]
fn blend(dst: &mut u32, r: f32, g: f32, b: f32, a: f32) {
    let a = a.clamp(0.0, 1.0);
    if a >= 0.998 {
        *dst = pack(r, g, b);
        return;
    }
    let d = *dst;
    let dr = ((d >> 16) & 0xFF) as f32;
    let dg = ((d >> 8) & 0xFF) as f32;
    let db = (d & 0xFF) as f32;
    *dst = pack(
        dr + (r - dr) * a,
        dg + (g - dg) * a,
        db + (b - db) * a,
    );
}
