//! Sharing a grid's width (or height) among its columns (or rows).
//!
//! For toolkits whose grid has no notion of weights (`GtkGrid`, `NSGridView`), so
//! that [`Track::Fill`] weights work the same as on the Web (`fr`) and Windows
//! (`Star`). One [`GridAxis`] handles one direction: the columns, or the rows.
//!
//! How the size is shared:
//!
//! - [`Track::Fixed`] lines are always their fixed size, even when empty.
//! - [`Track::Auto`] lines get their content's natural size, and shrink toward their
//!   content's minimum when there is not enough room.
//! - [`Track::Fill`] lines share what is left in proportion to their weights, but
//!   never below their content's minimum (the way CSS sizes `fr` tracks). A line
//!   that would get less than its minimum keeps the minimum, and the others share
//!   the rest.
//! - Lines nobody occupies (and that are not `Fixed`) take no space, and no spacing
//!   is put beside them.
//!
//! A child spanning several lines does not decide any one line's size; when the
//! lines it spans are too small for it, the shortfall goes to the `Fill` lines among
//! them in proportion to their weights, or else evenly to the `Auto` ones.

use crate::Track;

/// What one child asks of the lines it occupies.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GridItem {
    /// The first line it occupies.
    pub start: usize,
    /// The number of lines it occupies (1 or more).
    pub span: usize,
    /// The smallest size it can take.
    pub minimum: f64,
    /// The size it would like.
    pub natural: f64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct Line {
    track: Track,
    minimum: f64,
    natural: f64,
    used: bool,
}

/// The lines of one direction of a grid, with what their content asks for.
#[derive(Debug, Clone, PartialEq)]
pub struct GridAxis {
    lines: Vec<Line>,
    spacing: f64,
}

impl GridAxis {
    /// Gathers what the children ask of `count` lines. Lines without a setting in
    /// `tracks` are [`Track::Auto`].
    pub fn new(count: usize, tracks: &[Track], spacing: f64, items: &[GridItem]) -> Self {
        let mut lines: Vec<Line> = (0..count)
            .map(|index| {
                let track = tracks.get(index).copied().unwrap_or_default();
                let fixed = track.fixed_size();
                Line {
                    track,
                    minimum: fixed.unwrap_or(0.0),
                    natural: fixed.unwrap_or(0.0),
                    used: fixed.is_some(),
                }
            })
            .collect();
        let items: Vec<GridItem> = items
            .iter()
            .filter(|item| item.span > 0 && item.start < count)
            .map(|item| GridItem {
                span: item.span.min(count - item.start),
                minimum: item.minimum.max(0.0),
                natural: item.natural.max(item.minimum).max(0.0),
                ..*item
            })
            .collect();
        for item in &items {
            for line in &mut lines[item.start..item.start + item.span] {
                line.used = true;
            }
        }
        for item in items.iter().filter(|item| item.span == 1) {
            let line = &mut lines[item.start];
            if line.track.fixed_size().is_none() {
                line.minimum = line.minimum.max(item.minimum);
                line.natural = line.natural.max(item.natural);
            }
        }
        let mut axis = Self { lines, spacing };
        // 狭い範囲にまたがるものから先に満たすと、広い範囲の不足が少なくなる。
        let mut spanning: Vec<&GridItem> = items.iter().filter(|item| item.span > 1).collect();
        spanning.sort_by_key(|item| item.span);
        for item in spanning {
            axis.cover(item.start, item.span, item.minimum, true);
            axis.cover(item.start, item.span, item.natural, false);
        }
        for line in &mut axis.lines {
            line.natural = line.natural.max(line.minimum);
        }
        axis
    }

    /// Makes the lines `start..start + span` together at least `size`.
    fn cover(&mut self, start: usize, span: usize, size: f64, minimum: bool) {
        let range = start..start + span;
        let have: f64 = self.lines[range.clone()]
            .iter()
            .map(|line| if minimum { line.minimum } else { line.natural })
            .sum::<f64>()
            + self.spacing_between(range.clone());
        let missing = size - have;
        if missing <= 0.0 {
            return;
        }
        let fill_weight: f64 = self.lines[range.clone()]
            .iter()
            .map(|line| line.track.weight())
            .sum();
        let auto = self.lines[range.clone()]
            .iter()
            .filter(|line| line.track == Track::Auto)
            .count();
        for line in &mut self.lines[range] {
            let share = if fill_weight > 0.0 {
                missing * line.track.weight() / fill_weight
            } else if auto > 0 && line.track == Track::Auto {
                missing / auto as f64
            } else {
                0.0
            };
            if minimum {
                line.minimum += share;
            } else {
                line.natural += share;
            }
        }
    }

    /// The spacing between the occupied lines in `range`.
    fn spacing_between(&self, range: std::ops::Range<usize>) -> f64 {
        let used = self.lines[range].iter().filter(|line| line.used).count();
        used.saturating_sub(1) as f64 * self.spacing
    }

    /// The number of lines.
    pub fn len(&self) -> usize {
        self.lines.len()
    }

    /// Whether there are no lines.
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// The smallest size the lines fit into, spacing included.
    pub fn minimum(&self) -> f64 {
        self.lines.iter().map(|line| line.minimum).sum::<f64>()
            + self.spacing_between(0..self.lines.len())
    }

    /// The size at which every line gets what it would like, spacing included.
    ///
    /// `Fill` lines are counted in proportion to their weights, so that each gets at
    /// least its natural size (the size CSS gives a grid with `fr` tracks when
    /// nothing limits it).
    pub fn natural(&self) -> f64 {
        let fill_unit = self
            .lines
            .iter()
            .filter(|line| line.track.is_fill())
            .map(|line| line.natural / line.track.weight())
            .fold(0.0, f64::max);
        self.lines
            .iter()
            .map(|line| {
                if line.track.is_fill() {
                    fill_unit * line.track.weight()
                } else {
                    line.natural
                }
            })
            .sum::<f64>()
            + self.spacing_between(0..self.lines.len())
    }

    /// The size of each line when the lines together get `size` (spacing included).
    ///
    /// Below [`minimum`](Self::minimum) every line keeps its minimum (the content
    /// overflows). Space beyond what the lines would like is left unused when there
    /// is no `Fill` line.
    pub fn allocate(&self, size: f64) -> Vec<f64> {
        let mut sizes: Vec<f64> = self.lines.iter().map(|line| line.minimum).collect();
        let mut free = size - self.minimum();
        if free <= 0.0 {
            return sizes;
        }

        // `Auto` が自然な大きさまで伸びてから、残りを `Fill` が分ける。
        let wanted: f64 = self
            .lines
            .iter()
            .filter(|line| line.track == Track::Auto)
            .map(|line| line.natural - line.minimum)
            .sum();
        if wanted > 0.0 {
            let ratio = (free / wanted).min(1.0);
            for (line, size) in self.lines.iter().zip(&mut sizes) {
                if line.track == Track::Auto {
                    *size += (line.natural - line.minimum) * ratio;
                }
            }
            free -= wanted * ratio;
        }
        if free <= 0.0 {
            return sizes;
        }

        // CSS の `fr` と同じく、重みの比で配って最小に届かない線は最小に留め、
        // 残りでもう一度配る。
        let mut frozen: Vec<bool> = self
            .lines
            .iter()
            .map(|line| !line.track.is_fill())
            .collect();
        let mut space: f64 = free
            + self
                .lines
                .iter()
                .filter(|line| line.track.is_fill())
                .map(|line| line.minimum)
                .sum::<f64>();
        loop {
            let weight: f64 = self
                .lines
                .iter()
                .zip(&frozen)
                .filter(|(_, frozen)| !**frozen)
                .map(|(line, _)| line.track.weight())
                .sum();
            if weight <= 0.0 {
                break;
            }
            let unit = space / weight;
            let mut changed = false;
            for (index, line) in self.lines.iter().enumerate() {
                if !frozen[index] && unit * line.track.weight() < line.minimum {
                    frozen[index] = true;
                    space -= line.minimum;
                    sizes[index] = line.minimum;
                    changed = true;
                }
            }
            if !changed {
                for (index, line) in self.lines.iter().enumerate() {
                    if !frozen[index] {
                        sizes[index] = unit * line.track.weight();
                    }
                }
                break;
            }
        }
        sizes
    }

    /// Where each line starts and how long it is, given the line sizes from
    /// [`allocate`](Self::allocate). Spacing is put only between occupied lines.
    pub fn positions(&self, sizes: &[f64]) -> Vec<(f64, f64)> {
        let mut position = 0.0;
        let mut seen_used = false;
        self.lines
            .iter()
            .zip(sizes)
            .map(|(line, &size)| {
                if line.used {
                    if seen_used {
                        position += self.spacing;
                    }
                    seen_used = true;
                }
                let start = position;
                position += size;
                (start, size)
            })
            .collect()
    }

    /// Where a child occupying `start..start + span` starts and how long it is,
    /// spacing between its lines included.
    pub fn extent(positions: &[(f64, f64)], start: usize, span: usize) -> (f64, f64) {
        let Some(&(first, _)) = positions.get(start) else {
            return (0.0, 0.0);
        };
        let last = (start + span.max(1)).min(positions.len()) - 1;
        let (end_start, end_size) = positions[last];
        (first, (end_start + end_size - first).max(0.0))
    }
}

impl Track {
    /// The size of a [`Track::Fixed`] line (never negative). `None` otherwise.
    pub fn fixed_size(self) -> Option<f64> {
        match self {
            Track::Fixed(value) => Some(value.max(0.0)),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(start: usize, minimum: f64, natural: f64) -> GridItem {
        GridItem {
            start,
            span: 1,
            minimum,
            natural,
        }
    }

    #[test]
    fn auto_lines_take_their_natural_size() {
        let axis = GridAxis::new(2, &[], 10.0, &[item(0, 20.0, 50.0), item(1, 5.0, 30.0)]);
        assert_eq!(axis.minimum(), 35.0);
        assert_eq!(axis.natural(), 90.0);
        assert_eq!(axis.allocate(200.0), vec![50.0, 30.0], "余りは使わない");
        // 足りないときは最小から自然な大きさへの残りを比で縮める。
        let sizes = axis.allocate(60.0);
        assert!((sizes[0] - 20.0 - 30.0 * 25.0 / 55.0).abs() < 1e-9);
        assert!((sizes[1] - 5.0 - 25.0 * 25.0 / 55.0).abs() < 1e-9);
        assert_eq!(
            axis.allocate(10.0),
            vec![20.0, 5.0],
            "最小より小さくはしない"
        );
    }

    #[test]
    fn fill_lines_share_by_weight() {
        let tracks = [Track::Fixed(100.0), Track::Fill(1.0), Track::Fill(2.0)];
        let axis = GridAxis::new(3, &tracks, 0.0, &[item(1, 0.0, 10.0), item(2, 0.0, 10.0)]);
        assert_eq!(axis.allocate(400.0), vec![100.0, 100.0, 200.0]);
        // 自然な大きさは、どの Fill も自然な大きさに届く重みの比。
        assert_eq!(axis.natural(), 100.0 + 10.0 + 20.0);
    }

    #[test]
    fn fill_lines_keep_their_minimum() {
        let tracks = [Track::Fill(1.0), Track::Fill(3.0)];
        let axis = GridAxis::new(2, &tracks, 0.0, &[item(0, 80.0, 80.0)]);
        // 比のままなら 50 : 150 だが、最初の線は最小の 80 に留まる。
        assert_eq!(axis.allocate(200.0), vec![80.0, 120.0]);
        assert_eq!(axis.allocate(400.0), vec![100.0, 300.0]);
    }

    #[test]
    fn fixed_lines_are_fixed_even_when_empty() {
        let tracks = [Track::Fixed(40.0), Track::Auto, Track::Fixed(30.0)];
        let axis = GridAxis::new(3, &tracks, 5.0, &[]);
        assert_eq!(
            axis.minimum(),
            40.0 + 30.0 + 5.0,
            "空の Auto には間隔も付かない"
        );
        let sizes = axis.allocate(500.0);
        assert_eq!(sizes, vec![40.0, 0.0, 30.0]);
        assert_eq!(
            axis.positions(&sizes),
            vec![(0.0, 40.0), (40.0, 0.0), (45.0, 30.0)]
        );
    }

    #[test]
    fn spanning_items_grow_fill_lines_first() {
        let tracks = [Track::Auto, Track::Fill(1.0), Track::Fill(3.0)];
        let spanning = GridItem {
            start: 0,
            span: 3,
            minimum: 100.0,
            natural: 100.0,
        };
        let axis = GridAxis::new(3, &tracks, 10.0, &[item(0, 20.0, 20.0), spanning]);
        // 20 + 間隔 20 で 40。足りない 60 を Fill へ 1 : 3 で配る。
        assert_eq!(axis.minimum(), 100.0);
        assert_eq!(axis.allocate(100.0), vec![20.0, 15.0, 45.0]);
    }

    #[test]
    fn spanning_items_without_fill_grow_auto_lines_evenly() {
        let spanning = GridItem {
            start: 0,
            span: 2,
            minimum: 50.0,
            natural: 80.0,
        };
        let axis = GridAxis::new(2, &[Track::Auto, Track::Fixed(10.0)], 0.0, &[spanning]);
        assert_eq!(axis.minimum(), 50.0);
        assert_eq!(axis.natural(), 80.0);
        assert_eq!(axis.allocate(80.0), vec![70.0, 10.0]);
    }

    #[test]
    fn extent_includes_spacing_between_lines() {
        let tracks = [Track::Fixed(10.0), Track::Fixed(20.0), Track::Fixed(30.0)];
        let axis = GridAxis::new(3, &tracks, 4.0, &[]);
        let positions = axis.positions(&axis.allocate(0.0));
        assert_eq!(GridAxis::extent(&positions, 1, 2), (14.0, 54.0));
        assert_eq!(GridAxis::extent(&positions, 0, 1), (0.0, 10.0));
        assert_eq!(GridAxis::extent(&positions, 5, 1), (0.0, 0.0));
    }

    #[test]
    fn items_outside_the_lines_are_ignored() {
        let axis = GridAxis::new(1, &[], 0.0, &[item(3, 10.0, 10.0)]);
        assert_eq!(axis.minimum(), 0.0);
        assert!(!axis.is_empty());
    }
}
