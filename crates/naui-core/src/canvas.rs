//! 描画面 (`Canvas`) で使う値型と、描く内容の記録。
//!
//! グラフや図形のように**アプリが自分で描くもの**は、標準コントロールの
//! 組み合わせでは作れない。そこで naui は描画面だけを用意し、描く内容は
//! アプリが [`Painter`] へ書く。`Painter` は描画命令を**記録するだけ**で、
//! 実際に画素へ落とすのは各環境の 2D API (Core Graphics / cairo /
//! XAML の `Path` / `<canvas>` の 2D コンテキスト) の仕事。ここには
//! ラスタライズもフォントの計量も無い。
//!
//! 座標は左上が原点で、右と下が正の**論理ピクセル**。高解像度の表示への
//! 倍率は各環境が掛ける。
//!
//! 4 環境でそろう最小の語彙にしぼってある。線と塗りは全部 [`Path`] に
//! なり、`Path` の中身は直線と 3 次ベジェだけ。円と弧はここでベジェへ
//! 直してから渡すので、どの環境でも同じ形になる。

use crate::{Align, Color};

/// 座標 (論理ピクセル)。左上が原点で、右と下が正。
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}

impl From<(f64, f64)> for Point {
    fn from((x, y): (f64, f64)) -> Self {
        Point::new(x, y)
    }
}

/// 矩形 (論理ピクセル)。`x` / `y` は左上の角。
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    pub const fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// 左上の角。
    pub fn origin(&self) -> Point {
        Point::new(self.x, self.y)
    }

    /// 中心。
    pub fn center(&self) -> Point {
        Point::new(self.x + self.width / 2.0, self.y + self.height / 2.0)
    }

    /// 右端の x 座標。
    pub fn right(&self) -> f64 {
        self.x + self.width
    }

    /// 下端の y 座標。
    pub fn bottom(&self) -> f64 {
        self.y + self.height
    }

    /// 点が中に入っているか (辺の上も含む)。
    ///
    /// ポインターの位置が図形の上かどうかを見るのに使う。
    ///
    /// ```
    /// # use naui_core::{Point, Rect};
    /// let rect = Rect::new(10.0, 10.0, 20.0, 20.0);
    /// assert!(rect.contains(Point::new(15.0, 30.0)));
    /// assert!(!rect.contains(Point::new(31.0, 15.0)));
    /// ```
    pub fn contains(&self, point: Point) -> bool {
        point.x >= self.x
            && point.x <= self.right()
            && point.y >= self.y
            && point.y <= self.bottom()
    }

    /// 四辺を `amount` だけ内側へ寄せた矩形。負なら外側へ広がる。
    ///
    /// 幅や高さが負になるときは 0 で止める。
    pub fn inset(&self, amount: f64) -> Rect {
        Rect::new(
            self.x + amount,
            self.y + amount,
            (self.width - amount * 2.0).max(0.0),
            (self.height - amount * 2.0).max(0.0),
        )
    }
}

/// [`Path`] を組み立てる 1 手。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PathSegment {
    /// ペンを持ち上げて `to` へ移す (新しい部分図形を始める)。
    MoveTo(Point),
    /// いまの点から `to` へ直線を引く。
    LineTo(Point),
    /// いまの点から `to` へ 3 次ベジェ曲線を引く。
    CubicTo {
        control1: Point,
        control2: Point,
        to: Point,
    },
    /// いまの部分図形を、始点へ戻る直線で閉じる。
    Close,
}

/// 直線と 3 次ベジェでできた図形。
///
/// 4 環境の 2 D API に共通する `moveTo` / `lineTo` / `curveTo` / `close`
/// だけで表す。2 次ベジェ・円・弧を足すメソッドもあるが、それらは**ここで
/// 3 次ベジェへ直して**から積むので、環境ごとに形が変わらない。
///
/// ```
/// # use naui_core::{Path, Point};
/// let triangle = Path::new()
///     .move_to(Point::new(0.0, 0.0))
///     .line_to(Point::new(10.0, 0.0))
///     .line_to(Point::new(5.0, 8.0))
///     .close();
/// assert_eq!(triangle.segments().len(), 4);
/// ```
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Path {
    segments: Vec<PathSegment>,
}

impl Path {
    pub fn new() -> Self {
        Self::default()
    }

    /// 積んだ手の並び。バックエンドがそのまま再生する。
    pub fn segments(&self) -> &[PathSegment] {
        &self.segments
    }

    pub fn is_empty(&self) -> bool {
        self.segments.is_empty()
    }

    /// ペンを `to` へ移して新しい部分図形を始める。
    pub fn move_to(mut self, to: Point) -> Self {
        self.segments.push(PathSegment::MoveTo(to));
        self
    }

    /// いまの点から `to` へ直線を引く。
    ///
    /// まだ 1 手も無いときは `move_to` として扱う (ペンの場所が無いため)。
    pub fn line_to(mut self, to: Point) -> Self {
        if self.segments.is_empty() {
            return self.move_to(to);
        }
        self.segments.push(PathSegment::LineTo(to));
        self
    }

    /// いまの点から `to` へ 3 次ベジェ曲線を引く。
    pub fn cubic_to(mut self, control1: Point, control2: Point, to: Point) -> Self {
        if self.segments.is_empty() {
            return self.move_to(to);
        }
        self.segments.push(PathSegment::CubicTo {
            control1,
            control2,
            to,
        });
        self
    }

    /// いまの点から `to` へ 2 次ベジェ曲線を引く。
    ///
    /// 3 次ベジェへ直して積む (制御点を 2/3 の位置へ 2 つ置く)。
    /// 曲線の形は変わらない。
    pub fn quad_to(self, control: Point, to: Point) -> Self {
        let Some(from) = self.current_point() else {
            return self.move_to(to);
        };
        let control1 = Point::new(
            from.x + 2.0 / 3.0 * (control.x - from.x),
            from.y + 2.0 / 3.0 * (control.y - from.y),
        );
        let control2 = Point::new(
            to.x + 2.0 / 3.0 * (control.x - to.x),
            to.y + 2.0 / 3.0 * (control.y - to.y),
        );
        self.cubic_to(control1, control2, to)
    }

    /// いまの部分図形を閉じる。
    pub fn close(mut self) -> Self {
        if !self.segments.is_empty() {
            self.segments.push(PathSegment::Close);
        }
        self
    }

    /// 矩形を 1 つの閉じた部分図形として足す。
    pub fn rect(self, rect: Rect) -> Self {
        self.move_to(rect.origin())
            .line_to(Point::new(rect.right(), rect.y))
            .line_to(Point::new(rect.right(), rect.bottom()))
            .line_to(Point::new(rect.x, rect.bottom()))
            .close()
    }

    /// 円を 1 つの閉じた部分図形として足す。
    pub fn circle(self, center: Point, radius: f64) -> Self {
        self.ellipse(center, radius, radius)
    }

    /// 楕円を 1 つの閉じた部分図形として足す。
    ///
    /// 4 本の 3 次ベジェで近似する (誤差は半径の 0.03% 以下)。
    pub fn ellipse(self, center: Point, radius_x: f64, radius_y: f64) -> Self {
        self.move_to(Point::new(center.x + radius_x, center.y))
            .arc_segments(center, radius_x, radius_y, 0.0, std::f64::consts::TAU)
            .close()
    }

    /// 円弧を足す。角度はラジアンで、0 が右、正が時計回り (y が下向きのため)。
    ///
    /// ペンの場所が弧の始点と違うときは、そこまで直線を引いてから弧を描く
    /// (まだ 1 手も無ければ始点へ移る)。扇形は
    /// `move_to(中心).arc(...).close()` の形になる。
    ///
    /// `end - start` が 1 周を超える分は 1 周で止める。
    pub fn arc(self, center: Point, radius: f64, start: f64, end: f64) -> Self {
        let first = Point::new(
            center.x + radius * start.cos(),
            center.y + radius * start.sin(),
        );
        let this = if self.segments.is_empty() {
            self.move_to(first)
        } else {
            self.line_to(first)
        };
        this.arc_segments(center, radius, radius, start, end)
    }

    /// 弧をベジェで足す。ペンはすでに始点にある前提。
    fn arc_segments(
        mut self,
        center: Point,
        radius_x: f64,
        radius_y: f64,
        start: f64,
        end: f64,
    ) -> Self {
        let sweep = (end - start).clamp(-std::f64::consts::TAU, std::f64::consts::TAU);
        if sweep == 0.0 || !sweep.is_finite() {
            return self;
        }
        // 1 本のベジェで 90° まで。それより大きい弧は分ける。
        let pieces = (sweep.abs() / std::f64::consts::FRAC_PI_2).ceil().max(1.0) as usize;
        let step = sweep / pieces as f64;
        // 弧を 3 次ベジェで近似するときの制御点の張り出し。
        let kappa = 4.0 / 3.0 * (step / 4.0).tan();
        let point_at = |angle: f64| {
            Point::new(
                center.x + radius_x * angle.cos(),
                center.y + radius_y * angle.sin(),
            )
        };
        let mut from_angle = start;
        for _ in 0..pieces {
            let to_angle = from_angle + step;
            let (sin_from, cos_from) = from_angle.sin_cos();
            let (sin_to, cos_to) = to_angle.sin_cos();
            let control1 = Point::new(
                center.x + radius_x * (cos_from - kappa * sin_from),
                center.y + radius_y * (sin_from + kappa * cos_from),
            );
            let control2 = Point::new(
                center.x + radius_x * (cos_to + kappa * sin_to),
                center.y + radius_y * (sin_to - kappa * cos_to),
            );
            self.segments.push(PathSegment::CubicTo {
                control1,
                control2,
                to: point_at(to_angle),
            });
            from_angle = to_angle;
        }
        self
    }

    /// 折れ線を足す。最初の点へ移り、残りへ直線を引く。
    pub fn polyline(mut self, points: &[Point]) -> Self {
        let mut points = points.iter().copied();
        let Some(first) = points.next() else {
            return self;
        };
        self = self.move_to(first);
        for point in points {
            self = self.line_to(point);
        }
        self
    }

    /// 多角形を足す。折れ線を引いて閉じる。
    pub fn polygon(self, points: &[Point]) -> Self {
        if points.is_empty() {
            return self;
        }
        self.polyline(points).close()
    }

    /// いまペンがある点。
    ///
    /// `Close` の直後は、閉じた部分図形の始点。
    pub fn current_point(&self) -> Option<Point> {
        let mut start = None;
        let mut current = None;
        for segment in &self.segments {
            match *segment {
                PathSegment::MoveTo(p) => {
                    start = Some(p);
                    current = Some(p);
                }
                PathSegment::LineTo(p) | PathSegment::CubicTo { to: p, .. } => current = Some(p),
                PathSegment::Close => current = start,
            }
        }
        current
    }

    /// 全部の点を含むいちばん小さい矩形。制御点も含む。
    ///
    /// 1 手も無いときは `None`。
    pub fn bounds(&self) -> Option<Rect> {
        let mut min = Point::new(f64::INFINITY, f64::INFINITY);
        let mut max = Point::new(f64::NEG_INFINITY, f64::NEG_INFINITY);
        let mut extend = |p: Point| {
            min.x = min.x.min(p.x);
            min.y = min.y.min(p.y);
            max.x = max.x.max(p.x);
            max.y = max.y.max(p.y);
        };
        for segment in &self.segments {
            match *segment {
                PathSegment::MoveTo(p) | PathSegment::LineTo(p) => extend(p),
                PathSegment::CubicTo {
                    control1,
                    control2,
                    to,
                } => {
                    extend(control1);
                    extend(control2);
                    extend(to);
                }
                PathSegment::Close => {}
            }
        }
        if min.x.is_finite() {
            Some(Rect::new(min.x, min.y, max.x - min.x, max.y - min.y))
        } else {
            None
        }
    }
}

/// [`Painter`] が記録する描画命令。バックエンドはこれを順に再生する。
///
/// `opacity` は 0.0 (透明) から 1.0 (不透明)。色そのものは不透明な
/// [`Color`] で、透け具合は命令ごとに別に持つ。
#[derive(Debug, Clone, PartialEq)]
pub enum DrawCommand {
    /// 図形の中を塗る。塗り方は nonzero (4 環境の既定と同じ)。
    Fill {
        path: Path,
        color: Color,
        opacity: f64,
    },
    /// 図形の線をなぞる。`dash` が空なら実線。
    Stroke {
        path: Path,
        color: Color,
        width: f64,
        dash: Vec<f64>,
        opacity: f64,
    },
    /// 文字を 1 行描く。`at` は**文字の上端**で、横の位置は `align` で決まる
    /// (`Start` なら左端、`Center` なら中央、`End` なら右端が `at.x`)。
    Text {
        text: String,
        at: Point,
        size: f64,
        color: Color,
        align: Align,
        opacity: f64,
    },
}

/// 描く内容の記録。`Canvas::on_draw` の中でこれへ書く。
///
/// 描画命令を積むだけで、画素には触らない。線の幅・文字の大きさ・座標は
/// すべて論理ピクセル。透け具合と破線と文字の寄せ方は**状態**として持ち、
/// 変えるまで後の命令に効く (`<canvas>` の `globalAlpha` などと同じ形)。
///
/// ```
/// # use naui_core::{Color, Painter, Point, Rect};
/// let mut painter = Painter::new(200.0, 100.0);
/// painter.fill_rect(Rect::new(10.0, 10.0, 50.0, 30.0), Color::rgb(0x33, 0x66, 0xff));
/// painter.line(Point::new(0.0, 0.0), Point::new(200.0, 100.0), Color::BLACK, 1.0);
/// assert_eq!(painter.commands().len(), 2);
/// ```
#[derive(Debug, Clone)]
pub struct Painter {
    width: f64,
    height: f64,
    opacity: f64,
    dash: Vec<f64>,
    text_align: Align,
    commands: Vec<DrawCommand>,
}

impl Painter {
    /// `width` × `height` の面に描く記録を始める。
    pub fn new(width: f64, height: f64) -> Self {
        Self {
            width,
            height,
            opacity: 1.0,
            dash: Vec::new(),
            text_align: Align::Start,
            commands: Vec::new(),
        }
    }

    /// 描ける面の幅 (論理ピクセル)。
    pub fn width(&self) -> f64 {
        self.width
    }

    /// 描ける面の高さ (論理ピクセル)。
    pub fn height(&self) -> f64 {
        self.height
    }

    /// 描ける面全体の矩形。
    pub fn bounds(&self) -> Rect {
        Rect::new(0.0, 0.0, self.width, self.height)
    }

    /// 記録した命令。バックエンドが再生する。
    pub fn commands(&self) -> &[DrawCommand] {
        &self.commands
    }

    /// 記録した命令を取り出す。
    pub fn into_commands(self) -> Vec<DrawCommand> {
        self.commands
    }

    /// 以後の命令の透け具合。0.0 が透明、1.0 が不透明。範囲の外は端へ寄せる。
    pub fn set_opacity(&mut self, opacity: f64) {
        self.opacity = if opacity.is_finite() {
            opacity.clamp(0.0, 1.0)
        } else {
            1.0
        };
    }

    pub fn opacity(&self) -> f64 {
        self.opacity
    }

    /// 以後の線を破線にする。`pattern` は線と空きの長さの繰り返し
    /// (`[4.0, 2.0]` なら 4 引いて 2 空ける)。空なら実線に戻る。
    ///
    /// 負や無限の値が混じった指定は無視する (実線のまま)。
    pub fn set_dash(&mut self, pattern: &[f64]) {
        if pattern.iter().all(|v| v.is_finite() && *v >= 0.0) && pattern.iter().any(|v| *v > 0.0) {
            self.dash = pattern.to_vec();
        } else {
            self.dash.clear();
        }
    }

    pub fn dash(&self) -> &[f64] {
        &self.dash
    }

    /// 以後の文字の横の寄せ方。`Fill` は `Start` として扱う。
    pub fn set_text_align(&mut self, align: Align) {
        self.text_align = match align {
            Align::Fill => Align::Start,
            other => other,
        };
    }

    pub fn text_align(&self) -> Align {
        self.text_align
    }

    /// 図形の中を塗る。
    pub fn fill_path(&mut self, path: &Path, color: Color) {
        if path.is_empty() {
            return;
        }
        self.commands.push(DrawCommand::Fill {
            path: path.clone(),
            color,
            opacity: self.opacity,
        });
    }

    /// 図形の線をなぞる。`width` は線の太さ (論理ピクセル)。
    pub fn stroke_path(&mut self, path: &Path, color: Color, width: f64) {
        if path.is_empty() || width <= 0.0 || width.is_nan() {
            return;
        }
        self.commands.push(DrawCommand::Stroke {
            path: path.clone(),
            color,
            width,
            dash: self.dash.clone(),
            opacity: self.opacity,
        });
    }

    pub fn fill_rect(&mut self, rect: Rect, color: Color) {
        self.fill_path(&Path::new().rect(rect), color);
    }

    pub fn stroke_rect(&mut self, rect: Rect, color: Color, width: f64) {
        self.stroke_path(&Path::new().rect(rect), color, width);
    }

    pub fn fill_circle(&mut self, center: Point, radius: f64, color: Color) {
        self.fill_path(&Path::new().circle(center, radius), color);
    }

    pub fn stroke_circle(&mut self, center: Point, radius: f64, color: Color, width: f64) {
        self.stroke_path(&Path::new().circle(center, radius), color, width);
    }

    /// 2 点を結ぶ直線。
    pub fn line(&mut self, from: Point, to: Point, color: Color, width: f64) {
        self.stroke_path(&Path::new().move_to(from).line_to(to), color, width);
    }

    /// 折れ線。点が 2 つ未満なら何も描かない。
    pub fn polyline(&mut self, points: &[Point], color: Color, width: f64) {
        if points.len() < 2 {
            return;
        }
        self.stroke_path(&Path::new().polyline(points), color, width);
    }

    /// 多角形の中を塗る。点が 3 つ未満なら何も描かない。
    pub fn fill_polygon(&mut self, points: &[Point], color: Color) {
        if points.len() < 3 {
            return;
        }
        self.fill_path(&Path::new().polygon(points), color);
    }

    /// 文字を 1 行描く。`at` は文字の**上端**で、横の位置は
    /// [`set_text_align`](Self::set_text_align) で決めた寄せ方に従う。
    /// `size` は文字の大きさ (論理ピクセル)。書体はその環境の標準の UI フォント。
    ///
    /// 改行は折り返さない (その環境の描き方に任せる)。空文字は描かない。
    pub fn text(&mut self, text: &str, at: Point, size: f64, color: Color) {
        if text.is_empty() || size <= 0.0 || size.is_nan() {
            return;
        }
        self.commands.push(DrawCommand::Text {
            text: text.to_string(),
            at,
            size,
            color,
            align: self.text_align,
            opacity: self.opacity,
        });
    }
}

/// ポインター (マウス・タッチ・ペン) の動きの種類。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PointerPhase {
    /// 押した。
    Down,
    /// 動いた。押していない間 (ホバー) も届く。
    Move,
    /// 離した。
    Up,
}

/// 描画面の上でポインターが動いたこと。
///
/// 位置は描画面の左上を原点にした論理ピクセルで、[`Painter`] の座標と同じ。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PointerEvent {
    pub phase: PointerPhase,
    pub point: Point,
}

impl PointerEvent {
    pub fn new(phase: PointerPhase, point: Point) -> Self {
        Self { phase, point }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::{FRAC_PI_2, PI, TAU};

    fn close_to(a: Point, b: Point) -> bool {
        (a.x - b.x).abs() < 1e-9 && (a.y - b.y).abs() < 1e-9
    }

    #[test]
    fn rect_helpers() {
        let rect = Rect::new(10.0, 20.0, 30.0, 40.0);
        assert_eq!(rect.right(), 40.0);
        assert_eq!(rect.bottom(), 60.0);
        assert_eq!(rect.center(), Point::new(25.0, 40.0));
        assert!(rect.contains(Point::new(10.0, 20.0)));
        assert!(rect.contains(Point::new(40.0, 60.0)));
        assert!(!rect.contains(Point::new(9.9, 30.0)));
        assert_eq!(rect.inset(5.0), Rect::new(15.0, 25.0, 20.0, 30.0));
        assert_eq!(rect.inset(100.0).width, 0.0);
    }

    #[test]
    fn line_without_a_pen_position_starts_a_subpath() {
        let path = Path::new().line_to(Point::new(1.0, 2.0));
        assert_eq!(
            path.segments(),
            &[PathSegment::MoveTo(Point::new(1.0, 2.0))]
        );
        assert!(
            Path::new().close().is_empty(),
            "空のまま閉じても何も積まない"
        );
    }

    #[test]
    fn quad_becomes_cubic_with_the_same_endpoints() {
        let path = Path::new()
            .move_to(Point::new(0.0, 0.0))
            .quad_to(Point::new(3.0, 6.0), Point::new(6.0, 0.0));
        let [_, PathSegment::CubicTo {
            control1,
            control2,
            to,
        }] = path.segments()
        else {
            panic!("3 次ベジェへ直っていること: {:?}", path.segments());
        };
        assert!(close_to(*control1, Point::new(2.0, 4.0)));
        assert!(close_to(*control2, Point::new(4.0, 4.0)));
        assert_eq!(*to, Point::new(6.0, 0.0));
    }

    #[test]
    fn circle_is_four_cubics_that_return_to_the_start() {
        let path = Path::new().circle(Point::new(50.0, 50.0), 10.0);
        let segments = path.segments();
        assert_eq!(segments.len(), 6, "move + 4 本 + close");
        assert_eq!(segments[0], PathSegment::MoveTo(Point::new(60.0, 50.0)));
        assert!(matches!(segments[5], PathSegment::Close));
        let PathSegment::CubicTo { to, .. } = segments[4] else {
            panic!("最後の手はベジェであること");
        };
        assert!(close_to(to, Point::new(60.0, 50.0)), "始点へ戻る: {to:?}");
        // 上端 (y が小さい側) を通る。
        let PathSegment::CubicTo { to, .. } = segments[3] else {
            panic!()
        };
        assert!(close_to(to, Point::new(50.0, 40.0)), "{to:?}");
        // 制御点の張り出しは半径 × 0.5523 (円の近似の定数)。
        let PathSegment::CubicTo { control1, .. } = segments[1] else {
            panic!()
        };
        assert!((control1.x - 60.0).abs() < 1e-9);
        assert!((control1.y - (50.0 + 10.0 * 0.552_284_749_830_793)).abs() < 1e-9);
    }

    #[test]
    fn arc_lines_to_its_start_and_splits_into_quarters() {
        let center = Point::new(0.0, 0.0);
        let path = Path::new().move_to(center).arc(center, 10.0, 0.0, PI);
        let segments = path.segments();
        // move (中心) + line (始点) + 2 本の 1/4 弧。
        assert_eq!(segments.len(), 4, "{segments:?}");
        assert_eq!(segments[1], PathSegment::LineTo(Point::new(10.0, 0.0)));
        let PathSegment::CubicTo { to, .. } = segments[2] else {
            panic!()
        };
        assert!(
            close_to(to, Point::new(0.0, 10.0)),
            "時計回りに下へ: {to:?}"
        );
        let PathSegment::CubicTo { to, .. } = segments[3] else {
            panic!()
        };
        assert!(close_to(to, Point::new(-10.0, 0.0)), "{to:?}");

        // 逆向きも引ける。
        let back = Path::new().arc(center, 10.0, FRAC_PI_2, 0.0);
        let PathSegment::CubicTo { to, .. } = back.segments()[1] else {
            panic!()
        };
        assert!(close_to(to, Point::new(10.0, 0.0)));

        // 1 周を超える分は 1 周で止める。
        let full = Path::new().arc(center, 10.0, 0.0, TAU * 3.0);
        assert_eq!(full.segments().len(), 5);
        // 0 の弧は始点だけ。
        assert_eq!(Path::new().arc(center, 10.0, 1.0, 1.0).segments().len(), 1);
    }

    #[test]
    fn current_point_follows_close() {
        let path = Path::new()
            .move_to(Point::new(1.0, 1.0))
            .line_to(Point::new(5.0, 1.0))
            .close();
        assert_eq!(path.current_point(), Some(Point::new(1.0, 1.0)));
        assert_eq!(Path::new().current_point(), None);
    }

    #[test]
    fn bounds_cover_control_points() {
        let path = Path::new().move_to(Point::new(0.0, 0.0)).cubic_to(
            Point::new(-5.0, 2.0),
            Point::new(10.0, 20.0),
            Point::new(4.0, 4.0),
        );
        assert_eq!(path.bounds(), Some(Rect::new(-5.0, 0.0, 15.0, 20.0)));
        assert_eq!(Path::new().bounds(), None);
        assert_eq!(
            Path::new().polygon(&[]).bounds(),
            None,
            "空の多角形は何も積まない"
        );
    }

    #[test]
    fn painter_records_state_with_each_command() {
        let mut painter = Painter::new(100.0, 50.0);
        assert_eq!(painter.bounds(), Rect::new(0.0, 0.0, 100.0, 50.0));

        painter.set_opacity(0.5);
        painter.set_dash(&[4.0, 2.0]);
        painter.set_text_align(Align::Fill);
        painter.fill_rect(Rect::new(0.0, 0.0, 10.0, 10.0), Color::BLACK);
        painter.line(
            Point::new(0.0, 0.0),
            Point::new(1.0, 1.0),
            Color::WHITE,
            2.0,
        );
        painter.text("x", Point::new(3.0, 4.0), 12.0, Color::BLACK);

        let commands = painter.commands();
        assert_eq!(commands.len(), 3);
        let DrawCommand::Fill { opacity, .. } = &commands[0] else {
            panic!()
        };
        assert_eq!(*opacity, 0.5);
        let DrawCommand::Stroke { dash, width, .. } = &commands[1] else {
            panic!()
        };
        assert_eq!(dash, &[4.0, 2.0]);
        assert_eq!(*width, 2.0);
        let DrawCommand::Text { align, at, .. } = &commands[2] else {
            panic!()
        };
        assert_eq!(*align, Align::Start, "Fill は Start として扱う");
        assert_eq!(*at, Point::new(3.0, 4.0));

        // 状態を戻せば後の命令には効かない。
        painter.set_opacity(f64::NAN);
        painter.set_dash(&[]);
        painter.stroke_rect(Rect::new(0.0, 0.0, 1.0, 1.0), Color::BLACK, 1.0);
        let DrawCommand::Stroke { dash, opacity, .. } = &painter.commands()[3] else {
            panic!()
        };
        assert!(dash.is_empty());
        assert_eq!(*opacity, 1.0);
    }

    #[test]
    fn painter_skips_empty_commands() {
        let mut painter = Painter::new(10.0, 10.0);
        painter.fill_path(&Path::new(), Color::BLACK);
        painter.stroke_rect(Rect::new(0.0, 0.0, 1.0, 1.0), Color::BLACK, 0.0);
        painter.polyline(&[Point::new(0.0, 0.0)], Color::BLACK, 1.0);
        painter.fill_polygon(&[Point::new(0.0, 0.0), Point::new(1.0, 1.0)], Color::BLACK);
        painter.text("", Point::new(0.0, 0.0), 12.0, Color::BLACK);
        painter.text("a", Point::new(0.0, 0.0), 0.0, Color::BLACK);
        assert!(painter.commands().is_empty());
        painter.set_dash(&[-1.0, 2.0]);
        assert!(painter.dash().is_empty(), "負の破線は無視する");
        painter.set_dash(&[0.0, 0.0]);
        assert!(painter.dash().is_empty(), "全部 0 は実線");
    }
}
