//! How a grid's columns (or rows) share the room.
//!
//! The Web and Windows hand [`Track::Fill`](crate::Track::Fill) to the native grid
//! (CSS `fr`, WinUI `Star`), which splits the leftover space by weight.
//! `NSGridView` and `GtkGrid` have no weights, so the macOS and Linux backends
//! decide the sizes themselves with the calculations here. They follow CSS's `fr`:
//! the leftover is split in proportion to the weights, but **no track gets less
//! than its content's minimum**; a track that would is held at its minimum and the
//! rest is split again among the others.

/// What one column (or row) asks for, as a backend that sizes tracks itself sees it.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct TrackRequest {
    /// The smallest size its content allows.
    pub minimum: f64,
    /// The size below which its content is cut off, when that is more than
    /// [`minimum`](Self::minimum). A `Fill` track keeps it while the room allows,
    /// but unlike the minimum it is not required: content with `Fill` reports a
    /// minimum of 0 so that it never pushes the parent wider (the Web writes
    /// `min-width: 0` for the same reason), and this keeps it from being cut off
    /// as long as there is room. 0 when the content has no such size.
    pub content_minimum: f64,
    /// The size its content would like.
    pub natural: f64,
    /// The weight of a `Fill` track ([`Track::weight`](crate::Track::weight)). 0 for
    /// `Auto` and `Fixed`.
    pub weight: f64,
    /// Whether a track that is not `Fill` takes the leftover when the grid has no
    /// `Fill` track (a child asked to grow, as `hexpand` does on GTK).
    pub expand: bool,
}

impl TrackRequest {
    fn is_fill(&self) -> bool {
        self.weight > 0.0
    }
}

/// Splits `room` among `Fill` tracks by weight, never below a track's minimum.
///
/// `tracks` holds each track's weight and minimum. Weights of 0 or less count as 1
/// (as [`Track::weight`](crate::Track::weight) does). When `room` does not cover
/// the minimums, every track gets its minimum and the sum exceeds `room`.
///
/// ```
/// # use naui_core::distribute_fill;
/// // 1 : 2 : 1.
/// assert_eq!(distribute_fill(400.0, &[(1.0, 0.0), (2.0, 0.0), (1.0, 0.0)]), [100.0, 200.0, 100.0]);
/// // The first track's content needs 150, so it keeps that and the others share the rest.
/// assert_eq!(distribute_fill(400.0, &[(1.0, 150.0), (1.0, 0.0), (1.0, 0.0)]), [150.0, 125.0, 125.0]);
/// ```
pub fn distribute_fill(room: f64, tracks: &[(f64, f64)]) -> Vec<f64> {
    let weight = |w: f64| if w.is_finite() && w > 0.0 { w } else { 1.0 };
    let minimum = |m: f64| if m.is_finite() { m.max(0.0) } else { 0.0 };
    let room = if room.is_finite() { room.max(0.0) } else { 0.0 };
    // 最小で止めたトラック。止めるたびに残りで分け直す (CSS の「fr の大きさを
    // 求める」手順と同じ)。止まるトラックは 1 周ごとに増えるので必ず終わる。
    let mut held: Vec<bool> = vec![false; tracks.len()];
    loop {
        let held_size: f64 = tracks
            .iter()
            .zip(&held)
            .filter(|(_, held)| **held)
            .map(|((_, m), _)| minimum(*m))
            .sum();
        let weights: f64 = tracks
            .iter()
            .zip(&held)
            .filter(|(_, held)| !**held)
            .map(|((w, _), _)| weight(*w))
            .sum();
        let unit = if weights > 0.0 {
            (room - held_size).max(0.0) / weights
        } else {
            0.0
        };
        let mut changed = false;
        for (index, (w, m)) in tracks.iter().enumerate() {
            if !held[index] && weight(*w) * unit < minimum(*m) {
                held[index] = true;
                changed = true;
            }
        }
        if !changed {
            return tracks
                .iter()
                .zip(&held)
                .map(|((w, m), held)| {
                    if *held {
                        minimum(*m)
                    } else {
                        weight(*w) * unit
                    }
                })
                .collect();
        }
    }
}

/// Decides the size of every track for `room` (the grid's size without the spacing
/// between tracks and the padding).
///
/// 1. Tracks that are not `Fill` get their natural size, and the `Fill` tracks
///    share the rest by weight ([`distribute_fill`]).
/// 2. Without a `Fill` track, the rest goes evenly to the tracks with
///    [`TrackRequest::expand`] (and stays unused when there are none).
/// 3. When `room` is smaller than that, every track gives up what it would like
///    (its natural size, or for a `Fill` track its
///    [`content_minimum`](TrackRequest::content_minimum)), the closest to its
///    minimum first (the way GTK shrinks a box), down to its minimum.
///
/// In step 1 a `Fill` track never gets less than its `content_minimum` either. No
/// track gets less than its minimum, so the sum can exceed `room`.
///
/// ```
/// # use naui_core::{allocate_tracks, TrackRequest};
/// let label = TrackRequest { minimum: 60.0, natural: 60.0, ..Default::default() };
/// let fill = |weight| TrackRequest { weight, ..Default::default() };
/// // The label keeps its width, and the two `Fill` columns share 300 by 1 : 2.
/// assert_eq!(allocate_tracks(360.0, &[label, fill(1.0), fill(2.0)]), [60.0, 100.0, 200.0]);
/// ```
pub fn allocate_tracks(room: f64, tracks: &[TrackRequest]) -> Vec<f64> {
    let room = if room.is_finite() { room.max(0.0) } else { 0.0 };
    let natural = |t: &TrackRequest| t.natural.max(t.minimum).max(0.0);
    let minimum = |t: &TrackRequest| t.minimum.max(0.0);
    // 場所が足りる限り確保したい大きさ。Fill は中身が切れない大きさ、ほかは自然な大きさ。
    let wanted = |t: &TrackRequest| {
        if t.is_fill() {
            t.content_minimum.max(minimum(t))
        } else {
            natural(t)
        }
    };
    let others: f64 = tracks.iter().filter(|t| !t.is_fill()).map(natural).sum();
    let fills_wanted: f64 = tracks.iter().filter(|t| t.is_fill()).map(wanted).sum();

    if room >= others + fills_wanted {
        let rest = room - others;
        let mut sizes: Vec<f64> = tracks
            .iter()
            .map(|t| if t.is_fill() { 0.0 } else { natural(t) })
            .collect();
        let fills: Vec<usize> = (0..tracks.len()).filter(|i| tracks[*i].is_fill()).collect();
        if fills.is_empty() {
            let growing: Vec<usize> = (0..tracks.len()).filter(|i| tracks[*i].expand).collect();
            if !growing.is_empty() {
                let each = rest / growing.len() as f64;
                for index in growing {
                    sizes[index] += each;
                }
            }
        } else {
            let requests: Vec<(f64, f64)> = fills
                .iter()
                .map(|i| (tracks[*i].weight, wanted(&tracks[*i])))
                .collect();
            for (index, size) in fills.iter().zip(distribute_fill(rest, &requests)) {
                sizes[*index] = size;
            }
        }
        return sizes;
    }

    // 足りないとき。どれも最小から確保したい大きさへ向けて、差の小さいものから
    // 順に満たす (水を注ぐように、同じ量ずつ、差で頭打ち)。
    let mut sizes: Vec<f64> = tracks.iter().map(minimum).collect();
    let used: f64 = sizes.iter().sum();
    let mut spare = (room - used).max(0.0);
    let gap = |i: usize| wanted(&tracks[i]) - minimum(&tracks[i]);
    let mut order: Vec<usize> = (0..tracks.len()).filter(|i| gap(*i) > 0.0).collect();
    order.sort_by(|a, b| gap(*a).total_cmp(&gap(*b)));
    let mut left = order.len();
    for index in order {
        let gap = gap(index);
        let given = gap.min(spare / left as f64);
        sizes[index] += given;
        spare -= given;
        left -= 1;
    }
    sizes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fill(weight: f64, minimum: f64) -> TrackRequest {
        TrackRequest {
            minimum,
            natural: minimum,
            weight,
            ..TrackRequest::default()
        }
    }

    fn content(minimum: f64, natural: f64) -> TrackRequest {
        TrackRequest {
            minimum,
            natural,
            ..TrackRequest::default()
        }
    }

    #[test]
    fn weights_split_the_room() {
        assert_eq!(
            distribute_fill(300.0, &[(1.0, 0.0), (2.0, 0.0)]),
            [100.0, 200.0]
        );
        assert_eq!(
            distribute_fill(90.0, &[(0.5, 0.0), (1.0, 0.0)]),
            [30.0, 60.0]
        );
    }

    #[test]
    fn weights_of_zero_or_less_count_as_one() {
        assert_eq!(
            distribute_fill(200.0, &[(0.0, 0.0), (-2.0, 0.0)]),
            [100.0, 100.0]
        );
        assert_eq!(
            distribute_fill(200.0, &[(f64::NAN, 0.0), (1.0, 0.0)]),
            [100.0, 100.0]
        );
    }

    #[test]
    fn a_track_keeps_its_minimum_and_the_rest_is_split_again() {
        // 1 : 3 で 400 なら 100 : 300 だが、1 つ目は 180 要る。
        assert_eq!(
            distribute_fill(400.0, &[(1.0, 180.0), (3.0, 0.0)]),
            [180.0, 220.0]
        );
        // 止めたことで、別のトラックも最小に届かなくなる。
        assert_eq!(
            distribute_fill(300.0, &[(1.0, 150.0), (1.0, 120.0), (2.0, 0.0)]),
            [150.0, 120.0, 30.0]
        );
    }

    #[test]
    fn too_little_room_gives_every_minimum() {
        assert_eq!(
            distribute_fill(50.0, &[(1.0, 40.0), (1.0, 30.0)]),
            [40.0, 30.0]
        );
        assert_eq!(
            distribute_fill(-10.0, &[(1.0, 0.0), (1.0, 0.0)]),
            [0.0, 0.0]
        );
        assert!(distribute_fill(100.0, &[]).is_empty());
    }

    #[test]
    fn content_tracks_take_their_natural_size_and_fills_share_the_rest() {
        let sizes = allocate_tracks(
            500.0,
            &[content(40.0, 80.0), fill(1.0, 0.0), fill(3.0, 0.0)],
        );
        assert_eq!(sizes, [80.0, 105.0, 315.0]);
    }

    #[test]
    fn natural_size_does_not_tilt_the_shares() {
        // 自然な大きさの違う Fill 列でも、重みが同じなら同じ幅になる
        // (GtkGrid の「自然な大きさ + 余りの等分」とは違い、CSS の fr と同じ)。
        let wide = TrackRequest {
            natural: 200.0,
            weight: 1.0,
            expand: true,
            ..TrackRequest::default()
        };
        assert_eq!(
            allocate_tracks(300.0, &[wide, fill(1.0, 0.0)]),
            [150.0, 150.0]
        );
    }

    #[test]
    fn without_fill_the_rest_goes_to_growing_tracks() {
        let grows = TrackRequest {
            expand: true,
            ..content(10.0, 20.0)
        };
        assert_eq!(
            allocate_tracks(100.0, &[content(10.0, 20.0), grows, grows]),
            [20.0, 40.0, 40.0]
        );
        // 広がるトラックが無ければ余りは使わない。
        assert_eq!(
            allocate_tracks(100.0, &[content(10.0, 20.0), content(0.0, 30.0)]),
            [20.0, 30.0]
        );
    }

    #[test]
    fn fills_do_not_take_from_growing_tracks() {
        // Fill の列があれば、余りはそちらだけが受け取る (CSS の fr、WinUI の Star と同じ)。
        let grows = TrackRequest {
            expand: true,
            ..content(10.0, 20.0)
        };
        assert_eq!(
            allocate_tracks(100.0, &[grows, fill(1.0, 0.0)]),
            [20.0, 80.0]
        );
    }

    #[test]
    fn short_room_shrinks_content_tracks_towards_their_minimum() {
        // 自然な大きさの合計 140 に対して 100。差の小さい方 (10) は満たし、
        // 残りをもう一方へ。Fill は最小のまま。
        let sizes = allocate_tracks(
            100.0,
            &[content(20.0, 30.0), content(20.0, 110.0), fill(1.0, 10.0)],
        );
        assert_eq!(sizes, [30.0, 60.0, 10.0]);
        // 最小にも届かないときは最小。
        let sizes = allocate_tracks(10.0, &[content(20.0, 30.0), fill(1.0, 10.0)]);
        assert_eq!(sizes, [20.0, 10.0]);
    }

    #[test]
    fn fills_keep_their_content_minimum_while_there_is_room() {
        // 中身が切れない大きさ 216 の Fill は、等分の 141.5 ではなくそれを保ち、
        // 残りをもう一方へ回す (CSS の fr が min-content を守るのと同じ)。
        let long = TrackRequest {
            natural: 216.0,
            content_minimum: 216.0,
            weight: 1.0,
            ..TrackRequest::default()
        };
        let short = TrackRequest {
            natural: 40.0,
            content_minimum: 40.0,
            weight: 1.0,
            ..TrackRequest::default()
        };
        assert_eq!(
            allocate_tracks(372.0, &[content(89.0, 89.0), short, long]),
            [89.0, 67.0, 216.0]
        );
        // 広ければ重みどおり。
        assert_eq!(allocate_tracks(600.0, &[short, long]), [300.0, 300.0]);
        // 足りなければ、ほかのトラックと同じく差の小さいものから満たす。
        assert_eq!(
            allocate_tracks(200.0, &[content(60.0, 100.0), short, long]),
            [100.0, 40.0, 60.0]
        );
    }
}
