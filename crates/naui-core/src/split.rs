//! 分割ビューの仕切りの位置。
//!
//! 位置は **start 側 (左または上) の大きさ**を論理ピクセルで表す。
//! 実際に仕切りを動かすのは各バックエンドのネイティブなコントロール
//! (`NSSplitView` / `GtkPaned` / WinUI の `Grid` / Flexbox) で、ここにあるのは
//! 「どこまで動かしてよいか」を決める値と計算だけ。

/// 仕切りの初期位置 (start 側の大きさ、論理ピクセル)。
///
/// 作った直後の `SplitView` はこの位置で始まる。4 環境で同じ見え方になるよう、
/// 環境ごとの既定 (中身の自然な大きさや等分) には任せず naui が決めている。
pub const DEFAULT_SPLIT_POSITION: f64 = 200.0;

/// 仕切りの位置を、両側の最小の大きさに収める。
///
/// - `total` は**仕切りを除いた**、2 つの区画が分け合える大きさ。
///   まだレイアウトされておらず分からないときは 0 以下を渡す
///   (そのときは下限だけを見る)。
/// - `min_start` / `min_end` は両側の最小の大きさ。
///
/// 両方の最小を満たせないほど `total` が小さいときは **start 側の最小が勝つ**。
/// 先に置いたもの (サイドバーなど) の形を保つほうが、両方が中途半端に潰れるより
/// 分かりやすいため。
///
/// ```
/// # use naui_core::clamp_split_position;
/// // 100〜(400-120)=280 の範囲へ収まる。
/// assert_eq!(clamp_split_position(50.0, 400.0, 100.0, 120.0), 100.0);
/// assert_eq!(clamp_split_position(320.0, 400.0, 100.0, 120.0), 280.0);
/// assert_eq!(clamp_split_position(200.0, 400.0, 100.0, 120.0), 200.0);
/// // 全体が分からないときは下限だけを見る。
/// assert_eq!(clamp_split_position(50.0, 0.0, 100.0, 120.0), 100.0);
/// ```
pub fn clamp_split_position(position: f64, total: f64, min_start: f64, min_end: f64) -> f64 {
    let low = min_start.max(0.0);
    if !total.is_finite() || total <= 0.0 {
        return position.max(low);
    }
    // 両方の最小を満たせないときは start 側を優先する。
    let high = (total - min_end.max(0.0)).max(low);
    position.clamp(low, high)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_a_position_inside_the_range() {
        assert_eq!(clamp_split_position(200.0, 500.0, 80.0, 80.0), 200.0);
    }

    #[test]
    fn pushes_up_to_the_start_minimum() {
        assert_eq!(clamp_split_position(10.0, 500.0, 80.0, 80.0), 80.0);
        assert_eq!(clamp_split_position(-40.0, 500.0, 0.0, 0.0), 0.0);
    }

    #[test]
    fn pulls_back_to_leave_the_end_minimum() {
        assert_eq!(clamp_split_position(490.0, 500.0, 80.0, 120.0), 380.0);
    }

    #[test]
    fn start_minimum_wins_when_both_do_not_fit() {
        // 100 + 100 > 150 なので、start 側の最小だけが残る。
        assert_eq!(clamp_split_position(10.0, 150.0, 100.0, 100.0), 100.0);
        assert_eq!(clamp_split_position(999.0, 150.0, 100.0, 100.0), 100.0);
    }

    #[test]
    fn unknown_total_looks_at_the_lower_bound_only() {
        assert_eq!(clamp_split_position(300.0, 0.0, 80.0, 80.0), 300.0);
        assert_eq!(clamp_split_position(20.0, -1.0, 80.0, 80.0), 80.0);
    }

    #[test]
    fn negative_minimums_are_treated_as_zero() {
        assert_eq!(clamp_split_position(-5.0, 500.0, -10.0, -10.0), 0.0);
        assert_eq!(clamp_split_position(600.0, 500.0, 0.0, -10.0), 500.0);
    }
}
