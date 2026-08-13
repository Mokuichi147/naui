//! 符号付き距離関数。
//!
//! miui の描画プリミティブはすべてこの 2 つの距離関数から導出される。
//! 塗り・線・影・アイコンを 1 種類の評価ループで扱えるため、実装量が小さく、
//! アンチエイリアスの品質も一定になる。

/// 角丸矩形の符号付き距離。
///
/// `px`, `py` は矩形中心からの相対座標、`hw`, `hh` は半幅・半高。
/// `radii` は (左上, 右上, 右下, 左下)。負値なら内側。
#[inline]
pub fn round_rect(px: f32, py: f32, hw: f32, hh: f32, radii: [f32; 4]) -> f32 {
    // 象限に応じて半径を選ぶ。
    let r = if px >= 0.0 {
        if py <= 0.0 {
            radii[1]
        } else {
            radii[2]
        }
    } else if py <= 0.0 {
        radii[0]
    } else {
        radii[3]
    };
    let qx = px.abs() - hw + r;
    let qy = py.abs() - hh + r;
    let ox = qx.max(0.0);
    let oy = qy.max(0.0);
    (ox * ox + oy * oy).sqrt() + qx.max(qy).min(0.0) - r
}

/// 線分 (a→b) までの距離。
#[inline]
pub fn segment(px: f32, py: f32, ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
    let pax = px - ax;
    let pay = py - ay;
    let bax = bx - ax;
    let bay = by - ay;
    let denom = bax * bax + bay * bay;
    let h = if denom <= f32::EPSILON {
        0.0
    } else {
        ((pax * bax + pay * bay) / denom).clamp(0.0, 1.0)
    };
    let dx = pax - bax * h;
    let dy = pay - bay * h;
    (dx * dx + dy * dy).sqrt()
}

/// 距離 → 被覆率 (1px のアンチエイリアス幅)。
#[inline]
pub fn coverage(d: f32) -> f32 {
    (0.5 - d).clamp(0.0, 1.0)
}

/// 距離 → ぼかし付き被覆率 (影用)。
#[inline]
pub fn blurred_coverage(d: f32, blur: f32) -> f32 {
    if blur <= 0.0 {
        return coverage(d);
    }
    let t = (0.5 - d / blur).clamp(0.0, 1.0);
    // smoothstep。ガウシアンではないが、UI の影としては十分に自然。
    t * t * (3.0 - 2.0 * t)
}
