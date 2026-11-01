//! The widget behind `Grid`, which places its children on rows and columns itself.
//!
//! `GtkGrid` has no weights. It gives each column its natural size and splits what
//! is left **evenly** among the growing columns, so `Track::Fill(2.0)` would look
//! the same as `Track::Fill(1.0)`, and two `Fill` columns would differ by their
//! content. This widget measures the children the way `GtkGrid` does and splits
//! the room with naui-core's [`allocate_tracks`] (CSS's `fr`: by weight, never below
//! a column's minimum), the same as the Web (`fr`) and Windows (`Star`).
//!
//! What it keeps from `GtkGrid`:
//!
//! - Columns and rows without a shown child take neither room nor spacing.
//! - A child spanning several cells that needs more than they have spreads the
//!   difference over the `Fill` lines it spans (the growing ones when there is no
//!   `Fill`, all of them when none grows).
//! - Without a `Fill` line, the leftover goes evenly to lines whose children grow
//!   (`hexpand`).
//! - Heights are measured for the widths the columns actually get, so wrapping
//!   labels are measured at their real width.
//! - Columns are mirrored in right-to-left text.
//!
//! Inside its cell a child is placed by GTK4 itself (`gtk_widget_size_allocate`
//! applies its margins and `halign` / `valign`), as in `GtkGrid`.

use gtk::glib;
use gtk::prelude::*;
use naui_core::{allocate_tracks, GridCell, Track, TrackRequest};

// glib のサブクラス化マクロが `unsafe impl` を作るため、この中だけ許可する。
#[allow(unsafe_code)]
mod imp {
    use std::cell::{Cell, RefCell};

    use gtk::glib;
    use gtk::prelude::*;
    use gtk::subclass::prelude::*;
    use naui_core::{GridCell, Track};

    #[derive(Default)]
    pub struct TrackGrid {
        /// The children in the order they were placed (later ones are drawn on top).
        pub(super) cells: RefCell<Vec<(gtk::Widget, GridCell)>>,
        pub(super) column_tracks: RefCell<Vec<Track>>,
        pub(super) row_tracks: RefCell<Vec<Track>>,
        pub(super) column_spacing: Cell<i32>,
        pub(super) row_spacing: Cell<i32>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for TrackGrid {
        const NAME: &'static str = "NauiGrid";
        type Type = super::TrackGrid;
        type ParentType = gtk::Widget;

        fn class_init(klass: &mut Self::Class) {
            // テーマが `GtkGrid` に当てる書式をそのまま受けられるように。
            klass.set_css_name("grid");
        }
    }

    impl ObjectImpl for TrackGrid {
        fn dispose(&self) {
            // GTK4 では、親が壊れる前に子を外しておく必要がある。
            self.cells.borrow_mut().clear();
            while let Some(child) = self.obj().first_child() {
                child.unparent();
            }
        }
    }

    impl WidgetImpl for TrackGrid {
        /// Follows the children, as `GtkGrid` does: whichever of "height for width"
        /// and "width for height" more children ask for.
        fn request_mode(&self) -> gtk::SizeRequestMode {
            let (mut height_for_width, mut width_for_height) = (0, 0);
            for (child, _) in self.cells.borrow().iter() {
                if !child.should_layout() {
                    continue;
                }
                match child.request_mode() {
                    gtk::SizeRequestMode::HeightForWidth => height_for_width += 1,
                    gtk::SizeRequestMode::WidthForHeight => width_for_height += 1,
                    _ => {}
                }
            }
            if height_for_width == 0 && width_for_height == 0 {
                gtk::SizeRequestMode::ConstantSize
            } else if width_for_height > height_for_width {
                gtk::SizeRequestMode::WidthForHeight
            } else {
                gtk::SizeRequestMode::HeightForWidth
            }
        }

        fn measure(&self, orientation: gtk::Orientation, for_size: i32) -> (i32, i32, i32, i32) {
            let (minimum, natural) = self.obj().measure_lines(orientation, for_size);
            (minimum, natural, -1, -1)
        }

        fn size_allocate(&self, width: i32, height: i32, _baseline: i32) {
            self.obj().allocate_children(width, height);
        }
    }
}

glib::wrapper! {
    /// A grid that splits the room among `Fill` columns and rows by weight.
    pub struct TrackGrid(ObjectSubclass<imp::TrackGrid>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

/// One column or row while it is being measured.
#[derive(Clone, Copy, Default)]
struct Line {
    minimum: i32,
    natural: i32,
    /// No shown child covers it. It takes neither room nor spacing.
    empty: bool,
    /// A child in it grows (`hexpand` / `vexpand`).
    expand: bool,
    /// The weight of a `Fill` track, 0 otherwise.
    weight: f64,
}

/// Where one column or row ended up: its start and its size.
#[derive(Clone, Copy, Default)]
struct Span {
    start: i32,
    size: i32,
}

impl TrackGrid {
    pub(crate) fn new() -> Self {
        glib::Object::new()
    }

    fn imp(&self) -> &imp::TrackGrid {
        gtk::subclass::prelude::ObjectSubclassIsExt::imp(self)
    }

    /// Places a child (which must not have a parent yet).
    pub(crate) fn attach(&self, child: &impl IsA<gtk::Widget>, cell: GridCell) {
        let child = child.as_ref();
        child.set_parent(self);
        self.imp().cells.borrow_mut().push((child.clone(), cell));
        self.queue_resize();
    }

    /// Takes a child out. Does nothing if it is not in this grid.
    pub(crate) fn remove(&self, child: &impl IsA<gtk::Widget>) {
        let child = child.as_ref();
        let found = {
            let mut cells = self.imp().cells.borrow_mut();
            let before = cells.len();
            cells.retain(|(placed, _)| placed != child);
            cells.len() != before
        };
        if found {
            child.unparent();
            self.queue_resize();
        }
    }

    pub(crate) fn set_spacing(&self, column: i32, row: i32) {
        self.imp().column_spacing.set(column.max(0));
        self.imp().row_spacing.set(row.max(0));
        self.queue_resize();
    }

    pub(crate) fn column_spacing(&self) -> i32 {
        self.imp().column_spacing.get()
    }

    pub(crate) fn row_spacing(&self) -> i32 {
        self.imp().row_spacing.get()
    }

    pub(crate) fn set_column_track(&self, index: usize, track: Track) {
        set_track(&mut self.imp().column_tracks.borrow_mut(), index, track);
        self.queue_resize();
    }

    pub(crate) fn set_row_track(&self, index: usize, track: Track) {
        set_track(&mut self.imp().row_tracks.borrow_mut(), index, track);
        self.queue_resize();
    }

    pub(crate) fn column_track(&self, index: usize) -> Option<Track> {
        self.imp().column_tracks.borrow().get(index).copied()
    }

    pub(crate) fn row_track(&self, index: usize) -> Option<Track> {
        self.imp().row_tracks.borrow().get(index).copied()
    }

    fn spacing(&self, orientation: gtk::Orientation) -> i32 {
        if orientation == gtk::Orientation::Horizontal {
            self.column_spacing()
        } else {
            self.row_spacing()
        }
    }

    /// The minimum and natural size in `orientation`.
    ///
    /// For the axis that depends on the other (the height of a "height for width"
    /// grid), the other axis is first split for `for_size`, and the children are
    /// measured at the sizes they would get.
    fn measure_lines(&self, orientation: gtk::Orientation, for_size: i32) -> (i32, i32) {
        let opposite = opposite(orientation);
        let context = (for_size >= 0 && self.depends_on_other(orientation)).then(|| {
            let lines = self.lines(opposite, None);
            split(&lines, for_size, self.spacing(opposite))
        });
        let lines = self.lines(orientation, context.as_deref());
        let spacing = self.spacing(orientation);
        let shown = lines.iter().filter(|line| !line.empty).count() as i32;
        let gaps = spacing * (shown - 1).max(0);
        let minimum: i32 = lines.iter().map(|line| line.minimum).sum::<i32>() + gaps;
        let natural: i32 = lines.iter().map(|line| line.natural).sum::<i32>() + gaps;
        (minimum, natural.max(minimum))
    }

    /// Whether the size in `orientation` is measured after the other is decided.
    fn depends_on_other(&self, orientation: gtk::Orientation) -> bool {
        match self.request_mode() {
            gtk::SizeRequestMode::HeightForWidth => orientation == gtk::Orientation::Vertical,
            gtk::SizeRequestMode::WidthForHeight => orientation == gtk::Orientation::Horizontal,
            _ => false,
        }
    }

    fn allocate_children(&self, width: i32, height: i32) {
        let (horizontal, vertical) = (gtk::Orientation::Horizontal, gtk::Orientation::Vertical);
        // 測る順を守る (高さが幅で決まるなら、先に列を決めてから行を測る)。
        let (columns, rows) = if self.depends_on_other(horizontal) {
            let rows = split(&self.lines(vertical, None), height, self.row_spacing());
            let columns = split(
                &self.lines(horizontal, Some(&rows)),
                width,
                self.column_spacing(),
            );
            (columns, rows)
        } else {
            let columns = split(&self.lines(horizontal, None), width, self.column_spacing());
            let rows = split(
                &self.lines(vertical, Some(&columns)),
                height,
                self.row_spacing(),
            );
            (columns, rows)
        };
        let mirrored = self.direction() == gtk::TextDirection::Rtl;
        for (child, cell) in self.imp().cells.borrow().iter() {
            if !child.should_layout() {
                continue;
            }
            let (x, w) = extent(&columns, cell.column, cell.column_span);
            let (y, h) = extent(&rows, cell.row, cell.row_span);
            let x = if mirrored { width - x - w } else { x };
            child.size_allocate(&gtk::Allocation::new(x, y, w, h), -1);
        }
    }

    /// Measures the columns (or rows) from the children in them.
    ///
    /// With `context`, each child is measured for the size its cell gets on the
    /// other axis.
    fn lines(&self, orientation: gtk::Orientation, context: Option<&[Span]>) -> Vec<Line> {
        let horizontal = orientation == gtk::Orientation::Horizontal;
        let cells = self.imp().cells.borrow();
        let shown: Vec<&(gtk::Widget, GridCell)> = cells
            .iter()
            .filter(|(child, _)| child.should_layout())
            .collect();
        let place = |cell: &GridCell| {
            if horizontal {
                (cell.column, cell.column_span.max(1))
            } else {
                (cell.row, cell.row_span.max(1))
            }
        };
        let count = shown
            .iter()
            .map(|(_, cell)| {
                let (start, span) = place(cell);
                start + span
            })
            .max()
            .unwrap_or(0);
        let tracks = if horizontal {
            self.imp().column_tracks.borrow()
        } else {
            self.imp().row_tracks.borrow()
        };
        let mut lines: Vec<Line> = (0..count)
            .map(|index| Line {
                empty: true,
                weight: tracks.get(index).copied().unwrap_or_default().weight(),
                ..Line::default()
            })
            .collect();
        drop(tracks);

        let opposite = opposite(orientation);
        let measure = |child: &gtk::Widget, cell: &GridCell| {
            let for_size = context.map_or(-1, |spans| {
                let (start, span) = if horizontal {
                    (cell.row, cell.row_span.max(1))
                } else {
                    (cell.column, cell.column_span.max(1))
                };
                // 中身の最小より小さい大きさで測ると GTK4 が警告する。
                extent(spans, start, span)
                    .1
                    .max(child.measure(opposite, -1).0)
            });
            let (minimum, natural, _, _) = child.measure(orientation, for_size);
            (minimum, natural.max(minimum))
        };

        // 1 マスの子が先。またぐ子は、それで足りない分だけを足す。
        for (child, cell) in shown.iter().copied() {
            let (start, span) = place(cell);
            for line in &mut lines[start..start + span] {
                line.empty = false;
            }
            if span != 1 {
                continue;
            }
            let (minimum, natural) = measure(child, cell);
            let line = &mut lines[start];
            line.minimum = line.minimum.max(minimum);
            line.natural = line.natural.max(natural);
            line.expand |= child.compute_expand(orientation);
        }
        let spacing = self.spacing(orientation);
        for (child, cell) in shown.iter().copied() {
            let (start, span) = place(cell);
            if span == 1 {
                continue;
            }
            let range = start..start + span;
            if child.compute_expand(orientation) && !lines[range.clone()].iter().any(|l| l.expand) {
                for line in &mut lines[range.clone()] {
                    line.expand = true;
                }
            }
            let (minimum, natural) = measure(child, cell);
            let gaps = spacing * (span as i32 - 1);
            let have: i32 = lines[range.clone()].iter().map(|l| l.minimum).sum::<i32>() + gaps;
            spread(&mut lines[range.clone()], minimum - have, |line| {
                &mut line.minimum
            });
            for line in &mut lines[range.clone()] {
                line.natural = line.natural.max(line.minimum);
            }
            let have: i32 = lines[range.clone()].iter().map(|l| l.natural).sum::<i32>() + gaps;
            spread(&mut lines[range], natural - have, |line| &mut line.natural);
        }
        for line in &mut lines {
            line.natural = line.natural.max(line.minimum);
        }
        lines
    }
}

/// Spreads `missing` over the lines a spanning child crosses: the `Fill` ones, else
/// the growing ones, else all of them. Any pixels left over by the division go to
/// the first ones.
fn spread(lines: &mut [Line], missing: i32, field: impl Fn(&mut Line) -> &mut i32) {
    if missing <= 0 || lines.is_empty() {
        return;
    }
    let fill = lines.iter().any(|line| line.weight > 0.0);
    let grow = lines.iter().any(|line| line.expand);
    let targets: Vec<usize> = (0..lines.len())
        .filter(|index| {
            let line = &lines[*index];
            if fill {
                line.weight > 0.0
            } else if grow {
                line.expand
            } else {
                true
            }
        })
        .collect();
    let count = targets.len() as i32;
    for (order, index) in targets.into_iter().enumerate() {
        let extra = missing / count + i32::from((order as i32) < missing % count);
        *field(&mut lines[index]) += extra;
    }
}

/// Splits `size` among the lines and returns where each one goes.
///
/// The sizes come from [`allocate_tracks`]; they are rounded down to whole pixels
/// and the pixels that leaves are handed to the lines that lost the most.
fn split(lines: &[Line], size: i32, spacing: i32) -> Vec<Span> {
    let shown: Vec<usize> = (0..lines.len()).filter(|i| !lines[*i].empty).collect();
    let gaps = spacing * (shown.len() as i32 - 1).max(0);
    let room = (size - gaps).max(0);
    let requests: Vec<TrackRequest> = shown
        .iter()
        .map(|index| {
            let line = &lines[*index];
            TrackRequest {
                minimum: f64::from(line.minimum),
                natural: f64::from(line.natural),
                weight: line.weight,
                expand: line.expand,
            }
        })
        .collect();
    let exact = allocate_tracks(f64::from(room), &requests);
    let mut sizes: Vec<i32> = exact
        .iter()
        .zip(&shown)
        // 割り算の誤差で最小を 1 画素下回らないように。
        .map(|(size, index)| (size.floor() as i32).max(lines[*index].minimum))
        .collect();
    let mut left = room - sizes.iter().sum::<i32>();
    if left > 0 {
        let mut order: Vec<usize> = (0..sizes.len()).collect();
        order.sort_by(|a, b| {
            let lost = |i: usize| exact[i] - f64::from(sizes[i]);
            lost(*b).total_cmp(&lost(*a))
        });
        for index in order {
            if left == 0 {
                break;
            }
            if exact[index] - f64::from(sizes[index]) > 0.0 {
                sizes[index] += 1;
                left -= 1;
            }
        }
    }

    let mut spans = vec![Span::default(); lines.len()];
    let mut position = 0;
    let mut first = true;
    for (index, size) in shown.into_iter().zip(sizes) {
        if !first {
            position += spacing;
        }
        first = false;
        spans[index] = Span {
            start: position,
            size,
        };
        position += size;
    }
    // 空の列 (行) は、手前の列の終わりに大きさ 0 で置く。
    let mut end = 0;
    for (span, line) in spans.iter_mut().zip(lines) {
        if line.empty {
            span.start = end;
        } else {
            end = span.start + span.size;
        }
    }
    spans
}

/// The start and size of `span` lines from `start`, including the spacing between
/// them.
fn extent(spans: &[Span], start: usize, span: usize) -> (i32, i32) {
    let end = (start + span.max(1)).min(spans.len());
    let Some(first) = spans.get(start) else {
        return (0, 0);
    };
    let last = &spans[end - 1];
    (first.start, (last.start + last.size - first.start).max(0))
}

fn opposite(orientation: gtk::Orientation) -> gtk::Orientation {
    if orientation == gtk::Orientation::Horizontal {
        gtk::Orientation::Vertical
    } else {
        gtk::Orientation::Horizontal
    }
}

/// Writes a column / row setting, first extending the list as far as needed.
fn set_track(tracks: &mut Vec<Track>, index: usize, track: Track) {
    if tracks.len() <= index {
        tracks.resize(index + 1, Track::Auto);
    }
    tracks[index] = track;
}
