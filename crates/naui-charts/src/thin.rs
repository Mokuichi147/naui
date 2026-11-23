//! Fewer points for the same picture, so that charts of many points stay quick to
//! draw. Works on points already on the surface (logical pixels).

use std::collections::HashSet;

use naui::Point;

/// Below this many points per pixel column, a line is drawn as it is.
const POINTS_PER_COLUMN: usize = 4;

/// Splits a line at the points that are not finite (a gap in the data), giving
/// the runs of finite points between them.
pub(crate) fn runs(points: &[Point]) -> Vec<&[Point]> {
    points
        .split(|p| !p.x.is_finite() || !p.y.is_finite())
        .filter(|run| !run.is_empty())
        .collect()
}

/// A run of a line with no more than about four points per pixel column.
///
/// Each column keeps its first and last point and its highest and lowest, in the
/// order they come, so the line looks the same: spikes are kept, and where it
/// enters and leaves the column too. Lines whose `x` goes back somewhere are left
/// as they are, as are lines with few points for their width.
pub(crate) fn thin_line(run: &[Point]) -> Vec<Point> {
    let (Some(first), Some(last)) = (run.first(), run.last()) else {
        return Vec::new();
    };
    let columns = (last.x - first.x).abs().ceil() as usize + 1;
    let monotonic = run.windows(2).all(|pair| pair[1].x >= pair[0].x);
    if !monotonic || run.len() <= columns.saturating_mul(POINTS_PER_COLUMN) {
        return run.to_vec();
    }
    let mut thinned = Vec::with_capacity(columns * POINTS_PER_COLUMN);
    let mut start = 0;
    while start < run.len() {
        let column = run[start].x.floor();
        let mut end = start + 1;
        while end < run.len() && run[end].x.floor() == column {
            end += 1;
        }
        let bucket = &run[start..end];
        let mut low = 0;
        let mut high = 0;
        for (i, p) in bucket.iter().enumerate() {
            if p.y < bucket[low].y {
                low = i;
            }
            if p.y > bucket[high].y {
                high = i;
            }
        }
        // 来た順に並べ、同じ点は 1 度だけ置く。
        let mut keep = [0, low, high, bucket.len() - 1];
        keep.sort_unstable();
        let mut previous = None;
        for i in keep {
            if previous != Some(i) {
                thinned.push(bucket[i]);
                previous = Some(i);
            }
        }
        start = end;
    }
    thinned
}

/// The markers worth drawing: those inside `area`, and of those landing in the
/// same `cell`-sized square as an earlier one, only the earlier one.
///
/// Points are kept where they are (not moved to the cell), and with no more than
/// `exact_up_to` points all of those inside `area` are kept.
pub(crate) fn thin_markers(
    points: impl Iterator<Item = Point>,
    area: naui::Rect,
    cell: f64,
    exact_up_to: usize,
    total: usize,
) -> Vec<Point> {
    let inside = points.filter(|p| {
        p.x.is_finite()
            && p.y.is_finite()
            && p.x >= area.x
            && p.x <= area.right()
            && p.y >= area.y
            && p.y <= area.bottom()
    });
    if total <= exact_up_to || cell <= 0.0 {
        return inside.collect();
    }
    let mut seen = HashSet::new();
    inside
        .filter(|p| {
            let key = ((p.x / cell).floor() as i64, (p.y / cell).floor() as i64);
            seen.insert(key)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use naui::Rect;

    fn p(x: f64, y: f64) -> Point {
        Point::new(x, y)
    }

    #[test]
    fn gaps_split_a_line() {
        let points = [
            p(0.0, 0.0),
            p(1.0, 1.0),
            p(2.0, f64::NAN),
            p(3.0, 3.0),
            p(f64::INFINITY, 0.0),
            p(5.0, 5.0),
            p(6.0, 6.0),
        ];
        let split = runs(&points);
        let lengths: Vec<_> = split.iter().map(|run| run.len()).collect();
        assert_eq!(lengths, [2, 1, 2]);
        assert!(runs(&[p(f64::NAN, 0.0)]).is_empty());
    }

    #[test]
    fn few_points_are_left_alone() {
        let run: Vec<_> = (0..40).map(|i| p(i as f64, (i % 7) as f64)).collect();
        assert_eq!(thin_line(&run), run);
    }

    #[test]
    fn many_points_keep_ends_and_extremes_in_order() {
        // 10 画素の幅に 10 万点。各列は先頭・最小・最大・末尾の 4 点まで。
        let run: Vec<_> = (0..100_000)
            .map(|i| {
                let x = i as f64 / 10_000.0;
                let y = if i == 54_321 { 100.0 } else { (i % 13) as f64 };
                p(x, y)
            })
            .collect();
        let thinned = thin_line(&run);
        assert!(thinned.len() <= 11 * POINTS_PER_COLUMN, "{}", thinned.len());
        assert_eq!(thinned.first(), run.first());
        assert_eq!(thinned.last(), run.last());
        // 飛び出た 1 点は残る。
        assert!(thinned.iter().any(|q| q.y == 100.0));
        // 並びは崩れない。
        assert!(thinned.windows(2).all(|w| w[1].x >= w[0].x));
        // 各列の最小と最大が残る。
        for column in 0..10 {
            let in_column = |q: &&Point| q.x.floor() == column as f64;
            let kept: Vec<_> = thinned.iter().filter(in_column).collect();
            let all: Vec<_> = run.iter().filter(in_column).collect();
            let max = |v: &[&Point]| v.iter().map(|q| q.y).fold(f64::MIN, f64::max);
            let min = |v: &[&Point]| v.iter().map(|q| q.y).fold(f64::MAX, f64::min);
            assert_eq!(max(&kept), max(&all));
            assert_eq!(min(&kept), min(&all));
        }
    }

    #[test]
    fn lines_going_back_are_left_alone() {
        let mut run: Vec<_> = (0..1000).map(|i| p((i % 5) as f64, i as f64)).collect();
        run.push(p(0.0, 0.0));
        assert_eq!(thin_line(&run).len(), run.len());
    }

    #[test]
    fn markers_outside_or_crowded_are_dropped() {
        let area = Rect::new(0.0, 0.0, 100.0, 100.0);
        let points = [
            p(1.0, 1.0),
            p(1.5, 1.5),
            p(50.0, 50.0),
            p(150.0, 50.0),
            p(f64::NAN, 1.0),
        ];
        // 少なければ、外の点だけ落とす。
        let kept = thin_markers(points.into_iter(), area, 4.0, 10, points.len());
        assert_eq!(kept, [p(1.0, 1.0), p(1.5, 1.5), p(50.0, 50.0)]);
        // 多ければ、同じ升の後の点も落とす。点は動かさない。
        let kept = thin_markers(points.into_iter(), area, 4.0, 2, points.len());
        assert_eq!(kept, [p(1.0, 1.0), p(50.0, 50.0)]);
    }

    #[test]
    fn crowded_markers_are_bounded_by_the_area() {
        let area = Rect::new(0.0, 0.0, 100.0, 50.0);
        let total = 200_000;
        let points = (0..total).map(|i| {
            let t = i as f64;
            p((t * 0.618).fract() * 100.0, (t * 0.414).fract() * 50.0)
        });
        let kept = thin_markers(points, area, 4.0, 1000, total);
        // 升は 26 × 13 個 (端を含む)。
        assert!(kept.len() <= 26 * 14, "{}", kept.len());
    }
}
