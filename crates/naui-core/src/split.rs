//! The position of a split view's divider.
//!
//! The position is **the size of the start side (left or top)** in logical pixels.
//! The divider is actually moved by each backend's native control (`NSSplitView` /
//! `GtkPaned` / WinUI's `Grid` / Flexbox); all that is here are the values and
//! calculations deciding "how far it may move".

/// The initial divider position (size of the start side, logical pixels).
///
/// A freshly created `SplitView` starts at this position. So that it looks the same
/// on all four platforms, naui decides it instead of leaving it to each platform's
/// default (the content's natural size or an even split).
pub const DEFAULT_SPLIT_POSITION: f64 = 200.0;

/// Clamps the divider position to the minimum sizes of both sides.
///
/// - `total` is the size the two panes can share, **excluding the divider**. Pass 0
///   or less when it is unknown because layout has not run yet (then only the lower
///   bound is considered).
/// - `min_start` / `min_end` are the minimum sizes of the two sides.
///
/// When `total` is too small to satisfy both minimums, **the start side's minimum
/// wins**. Keeping the shape of what was placed first (a sidebar, for example) is
/// easier to understand than squashing both sides halfway.
///
/// ```
/// # use naui_core::clamp_split_position;
/// // Clamped to the range 100 to (400-120)=280.
/// assert_eq!(clamp_split_position(50.0, 400.0, 100.0, 120.0), 100.0);
/// assert_eq!(clamp_split_position(320.0, 400.0, 100.0, 120.0), 280.0);
/// assert_eq!(clamp_split_position(200.0, 400.0, 100.0, 120.0), 200.0);
/// // When the total is unknown, only the lower bound is considered.
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
