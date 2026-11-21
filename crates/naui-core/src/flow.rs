//! How a `Flow` breaks its children into lines.
//!
//! The Web hands wrapping to CSS (`flex-wrap`). The other toolkits have no
//! container that wraps children of different widths line by line: `GtkFlowBox`
//! lines its children up in columns (and is a list of selectable items, with a
//! focusable wrapper around each child), `AdwWrapBox` needs libadwaita 1.7, WinUI's
//! `VariableSizedWrapGrid` gives every child the same cell, and AppKit has only
//! `NSCollectionView`'s flow layout, which lays out items it creates itself. So the
//! macOS, Linux and Windows backends measure the children and place them with the
//! calculation here, the same way `flex-wrap` does.

use crate::Align;

/// One child of a flow, as a backend that lays it out itself sees it.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct FlowItem {
    /// The smallest width its content allows. A child wider than the flow is
    /// narrowed to the flow's width, but never below this.
    pub minimum: f64,
    /// The width its content would like.
    pub natural: f64,
    /// Whether it takes the room left in its line (`Fill` on the width).
    pub grow: bool,
}

/// Where one child goes: its line, and its start and width along the line.
///
/// `x` counts from the line's start. A backend that mirrors right-to-left layouts
/// itself flips it (`width - x - slot width`).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct FlowSlot {
    pub line: usize,
    pub x: f64,
    pub width: f64,
}

/// The children of a flow broken into lines for one width.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FlowLayout {
    /// One per item, in the same order.
    pub slots: Vec<FlowSlot>,
    /// The number of lines (0 without items).
    pub lines: usize,
}

impl FlowLayout {
    /// Breaks `items` into lines `width` wide, `spacing` apart within a line.
    ///
    /// A child goes on the current line while it fits at its natural width, and
    /// starts a new one otherwise; a child alone on its line always stays there,
    /// narrowed to the line (never below its minimum). The room left in a line goes
    /// to the children that `grow`, shared evenly. Without such children, `align`
    /// places the line's children at the start, the center or the end;
    /// [`Align::Fill`] counts as the start, since only the children themselves can
    /// say they may be widened.
    ///
    /// ```
    /// # use naui_core::{Align, FlowItem, FlowLayout};
    /// let item = |natural| FlowItem { minimum: 0.0, natural, grow: false };
    /// let layout = FlowLayout::new(&[item(40.0), item(40.0), item(40.0)], 100.0, 10.0, Align::Start);
    /// // Two fit on the first line (40 + 10 + 40), the third goes to the next.
    /// assert_eq!(layout.lines, 2);
    /// assert_eq!(layout.slots[1].x, 50.0);
    /// assert_eq!((layout.slots[2].line, layout.slots[2].x), (1, 0.0));
    /// ```
    pub fn new(items: &[FlowItem], width: f64, spacing: f64, align: Align) -> Self {
        let width = finite_or_zero(width);
        let spacing = finite_or_zero(spacing);
        let mut slots = vec![FlowSlot::default(); items.len()];
        let mut lines = 0;
        let mut start = 0;
        while start < items.len() {
            // 1 つ目は必ず載せ、あとは自然な幅で収まるあいだ足す。端数で折り返さない
            // よう、半画素までのはみ出しは収まったものとみなす。
            let mut used = natural(&items[start]);
            let mut end = start + 1;
            while end < items.len() {
                let next = used + spacing + natural(&items[end]);
                if next > width + 0.5 {
                    break;
                }
                used = next;
                end += 1;
            }
            place_line(
                &items[start..end],
                &mut slots[start..end],
                lines,
                width,
                spacing,
                align,
            );
            lines += 1;
            start = end;
        }
        Self { slots, lines }
    }

    /// The height of each line: the tallest of its children's `heights` (one per
    /// item, measured at the slot's width).
    pub fn line_heights(&self, heights: &[f64]) -> Vec<f64> {
        let mut lines = vec![0.0_f64; self.lines];
        for (slot, height) in self.slots.iter().zip(heights) {
            lines[slot.line] = lines[slot.line].max(finite_or_zero(*height));
        }
        lines
    }
}

/// The top of each line and the total height, for lines `spacing` apart.
///
/// ```
/// # use naui_core::flow_line_tops;
/// assert_eq!(flow_line_tops(&[20.0, 30.0, 10.0], 5.0), (vec![0.0, 25.0, 60.0], 70.0));
/// ```
pub fn flow_line_tops(line_heights: &[f64], spacing: f64) -> (Vec<f64>, f64) {
    let spacing = finite_or_zero(spacing);
    let mut tops = Vec::with_capacity(line_heights.len());
    let mut y = 0.0;
    for (index, height) in line_heights.iter().enumerate() {
        if index > 0 {
            y += spacing;
        }
        tops.push(y);
        y += finite_or_zero(*height);
    }
    (tops, y)
}

/// The width the items take on one line: what a flow asks for when it has room.
pub fn flow_natural_width(items: &[FlowItem], spacing: f64) -> f64 {
    let gaps = items.len().saturating_sub(1) as f64 * finite_or_zero(spacing);
    items.iter().map(natural).sum::<f64>() + gaps
}

/// The narrowest a flow can get: the widest minimum among the items, each on a line
/// of its own.
pub fn flow_minimum_width(items: &[FlowItem]) -> f64 {
    items
        .iter()
        .map(|item| finite_or_zero(item.minimum))
        .fold(0.0, f64::max)
}

fn natural(item: &FlowItem) -> f64 {
    finite_or_zero(item.natural).max(finite_or_zero(item.minimum))
}

fn finite_or_zero(value: f64) -> f64 {
    if value.is_finite() {
        value.max(0.0)
    } else {
        0.0
    }
}

fn place_line(
    items: &[FlowItem],
    slots: &mut [FlowSlot],
    line: usize,
    width: f64,
    spacing: f64,
    align: Align,
) {
    // 1 つで行より広い子は行の幅まで縮める (最小より下げない)。
    let mut widths: Vec<f64> = items
        .iter()
        .map(|item| natural(item).min(width).max(finite_or_zero(item.minimum)))
        .collect();
    let gaps = items.len().saturating_sub(1) as f64 * spacing;
    let left = width - widths.iter().sum::<f64>() - gaps;
    let mut offset = 0.0;
    if left > 0.0 {
        let growing = items.iter().filter(|item| item.grow).count();
        if growing > 0 {
            let share = left / growing as f64;
            for (width, item) in widths.iter_mut().zip(items) {
                if item.grow {
                    *width += share;
                }
            }
        } else {
            match align {
                Align::Center => offset = left / 2.0,
                Align::End => offset = left,
                Align::Start | Align::Fill => {}
            }
        }
    }
    let mut x = offset;
    for (slot, width) in slots.iter_mut().zip(widths) {
        *slot = FlowSlot { line, x, width };
        x += width + spacing;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(natural: f64) -> FlowItem {
        FlowItem {
            minimum: 0.0,
            natural,
            grow: false,
        }
    }

    fn lines(layout: &FlowLayout) -> Vec<usize> {
        layout.slots.iter().map(|slot| slot.line).collect()
    }

    #[test]
    fn breaks_lines_independently_of_columns() {
        // 列に揃えず、行ごとに詰める (flex-wrap と同じ)。
        let items = [item(80.0), item(20.0), item(20.0), item(50.0), item(50.0)];
        let layout = FlowLayout::new(&items, 130.0, 5.0, Align::Start);
        assert_eq!(lines(&layout), [0, 0, 0, 1, 1]);
        assert_eq!(layout.slots[2].x, 110.0);
        assert_eq!(layout.slots[4].x, 55.0);
    }

    #[test]
    fn an_item_wider_than_the_line_gets_a_line_of_its_own_narrowed_to_it() {
        let items = [item(30.0), item(500.0), item(30.0)];
        let layout = FlowLayout::new(&items, 100.0, 10.0, Align::Start);
        assert_eq!(lines(&layout), [0, 1, 2]);
        assert_eq!(layout.slots[1].width, 100.0);
    }

    #[test]
    fn narrowing_stops_at_the_minimum() {
        let items = [FlowItem {
            minimum: 150.0,
            natural: 200.0,
            grow: false,
        }];
        let layout = FlowLayout::new(&items, 100.0, 0.0, Align::Start);
        assert_eq!(layout.slots[0].width, 150.0);
    }

    #[test]
    fn growing_items_share_the_room_left_in_their_line() {
        let mut items = [item(30.0), item(30.0), item(30.0)];
        items[1].grow = true;
        items[2].grow = true;
        let layout = FlowLayout::new(&items, 100.0, 10.0, Align::End);
        // 30 + 10 + 30 + 10 + 30 = 110 は収まらないので 3 つ目は次の行。
        assert_eq!(lines(&layout), [0, 0, 1]);
        // 1 行目の残り 30 は伸びる 2 つ目だけが受け取り、揃えは効かない。
        assert_eq!(layout.slots[0].x, 0.0);
        assert_eq!(layout.slots[1].width, 60.0);
        // 2 行目は伸びる子だけなので行いっぱい。
        assert_eq!(layout.slots[2].width, 100.0);
    }

    #[test]
    fn align_places_lines_without_growing_items() {
        let items = [item(20.0), item(20.0)];
        let at = |align| {
            let layout = FlowLayout::new(&items, 100.0, 10.0, align);
            (layout.slots[0].x, layout.slots[1].x, layout.slots[1].width)
        };
        assert_eq!(at(Align::Start), (0.0, 30.0, 20.0));
        assert_eq!(at(Align::Center), (25.0, 55.0, 20.0));
        assert_eq!(at(Align::End), (50.0, 80.0, 20.0));
        assert_eq!(at(Align::Fill), (0.0, 30.0, 20.0));
    }

    #[test]
    fn half_a_pixel_over_still_fits() {
        let items = [item(50.25), item(50.25)];
        let layout = FlowLayout::new(&items, 100.0, 0.0, Align::Start);
        assert_eq!(layout.lines, 1);
    }

    #[test]
    fn line_heights_take_the_tallest_child() {
        let items = [item(60.0), item(30.0), item(60.0)];
        let layout = FlowLayout::new(&items, 100.0, 0.0, Align::Start);
        assert_eq!(layout.line_heights(&[10.0, 25.0, 15.0]), [25.0, 15.0]);
    }

    #[test]
    fn empty_flow_has_no_lines() {
        let layout = FlowLayout::new(&[], 100.0, 10.0, Align::Start);
        assert_eq!(layout.lines, 0);
        assert_eq!(
            flow_line_tops(&layout.line_heights(&[]), 5.0),
            (vec![], 0.0)
        );
        assert_eq!(flow_natural_width(&[], 10.0), 0.0);
    }

    #[test]
    fn natural_and_minimum_widths() {
        let items = [
            FlowItem {
                minimum: 10.0,
                natural: 40.0,
                grow: false,
            },
            FlowItem {
                minimum: 30.0,
                natural: 20.0,
                grow: true,
            },
        ];
        assert_eq!(flow_natural_width(&items, 5.0), 75.0);
        assert_eq!(flow_minimum_width(&items), 30.0);
    }
}
