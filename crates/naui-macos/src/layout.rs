//! Size specifications and layout containers (Grid / Scroll / Spacer).
//!
//! The computing is done by AppKit's Auto Layout and NSGridView / NSScrollView;
//! naui only sets constraints and properties.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use std::rc::Weak;

use dispatch2::{DispatchQueue, MainThreadBound};
use naui_core::{
    GridCell, Padding, ScrollMetrics, ScrollNotifier, ScrollPolicy, ScrollTarget, Sizing, Track,
};
use objc2::rc::{Allocated, Retained};
use objc2::runtime::NSObjectProtocol;
use objc2::{define_class, msg_send, sel, DefinedClass, MainThreadMarker, MainThreadOnly, Message};
use objc2_app_kit::{
    NSClipView, NSGridCell, NSGridCellPlacement, NSGridView, NSLayoutAttribute, NSLayoutConstraint,
    NSLayoutConstraintOrientation, NSLayoutPriority, NSLayoutRelation, NSScrollView, NSView,
    NSViewBoundsDidChangeNotification, NSViewFrameDidChangeNotification,
};
use objc2_foundation::{
    NSArray, NSNotification, NSNotificationCenter, NSObject, NSPoint, NSRange, NSRect, NSString,
};

use crate::widgets::{impl_widget, Widget};

/// A marker for constraints naui added.
///
/// AppKit internally adds constraints for the intrinsic content size to the same
/// view (`NSContentSizeLayoutConstraint`). Selecting by attribute alone would
/// remove those too, so the ones we add carry an identifier to tell them apart.
const SIZING_ID: &str = "naui.sizing";

/// The priority of the "want to grow up to the maximum" constraint when `Fill` has
/// a maximum.
///
/// It is weaker than every other constraint, so it grows to the maximum only when
/// there is room.
const PREFERRED_SIZE_PRIORITY: NSLayoutPriority = 1.0;

/// The priority expressing "want to match the parent's width / height" for `Fill`
/// on the cross axis.
///
/// If it were required, it would clash with another required constraint when the
/// same axis has a maximum ([`Sizing::max_width`] and the like), and AppKit would
/// drop one arbitrarily. We want the maximum to win, so it is one step lower (the
/// "don't overflow" side stays required and is added separately).
pub(crate) const CROSS_FILL_PRIORITY: NSLayoutPriority = 999.0;

/// The hugging priority for `Fill`. The lower it is, the more leftover space it
/// takes.
const FILL_HUGGING: NSLayoutPriority = 1.0;
/// The hugging priority for `Auto` (fit the content).
const HUG_CONTENT: NSLayoutPriority = 750.0;
/// The priority that keeps `Auto` children of a `Grid` / `Stack` from taking the
/// parent's leftover space.
///
/// It takes precedence over constraints AppKit's containers add on relayout,
/// keeping the content width. An explicit `Fill` does not use this value and lowers
/// hugging to 1.
const AUTO_HUGGING: NSLayoutPriority = 999.0;
/// The identifier of the "want to grow to fill the grid" wish placed on children
/// in `Fill` rows and columns.
const GRID_GROW_ID: &str = "naui.grid.grow";

/// The priority of that wish.
///
/// It is set to beat the weak preference with which `NSGridView` hands leftover
/// space to `Auto` rows (equivalent to `NSLayoutPriorityDefaultLow` = 250), and to
/// **lose** to the compression resistance of `Auto` children ([`HUG_CONTENT`]).
/// Any stronger and `Fill` rows would take too much, squashing the content of
/// `Auto` rows (headings shrink to 30pt).
const GRID_GROW_PRIORITY: NSLayoutPriority = HUG_CONTENT - 1.0;

/// The priority of the wish "want to grow up to the maximum" placed on a grid child
/// whose `Fill` has a maximum, in place of [`GRID_GROW_PRIORITY`].
///
/// It has to beat what keeps the child itself small: its `Fill` hugging
/// ([`FILL_HUGGING`]) and, for a `Stack`, the tail spacer ([`TAIL_PRIORITY`]).
/// Anything stronger would push the grid out against its parent (a window keeps its
/// size at 500), so the child would stay at its maximum when space is tight.
const GRID_CAPPED_GROW_PRIORITY: NSLayoutPriority = TAIL_PRIORITY + 0.5;

/// The compression resistance priority for `Fill`.
///
/// Keeps the intrinsic size of images and video from becoming the parent's minimum
/// width.
const FILL_COMPRESSION_RESISTANCE: NSLayoutPriority = 1.0;

/// Applies the size specification to the view. Each call discards the previous one.
pub(crate) fn apply_sizing(view: &NSView, sizing: Sizing) {
    clear_sizing_constraints(view);

    let width = view.widthAnchor();
    let height = view.heightAnchor();
    let mut constraints: Vec<Retained<NSLayoutConstraint>> = Vec::new();
    if let Some(value) = sizing.width.fixed_value() {
        constraints.push(width.constraintEqualToConstant(value));
    }
    if let Some(value) = sizing.height.fixed_value() {
        constraints.push(height.constraintEqualToConstant(value));
    }
    if let Some(value) = sizing.min_width {
        constraints.push(width.constraintGreaterThanOrEqualToConstant(value));
    }
    if let Some(value) = sizing.max_width {
        constraints.push(width.constraintLessThanOrEqualToConstant(value));
    }
    if let Some(value) = sizing.min_height {
        constraints.push(height.constraintGreaterThanOrEqualToConstant(value));
    }
    if let Some(value) = sizing.max_height {
        constraints.push(height.constraintLessThanOrEqualToConstant(value));
    }

    // `Fill` に上限を付けたときは、上限を「通常時に確保したい大きさ」として
    // 扱う。CSS の `flex` / WinUI の `Stretch` は空間があれば上限まで伸びる
    // ため、AppKit でも同じ見え方になるよう弱い制約で希望を出しておく。
    // 中身の intrinsic size が当てにならないウィジェット (AVPlayerView など)
    // でも、これで表示欄の高さが決まる。
    for (fill, max, anchor) in [
        (sizing.width.is_fill(), sizing.max_width, &width),
        (sizing.height.is_fill(), sizing.max_height, &height),
    ] {
        let (true, Some(value)) = (fill, max) else {
            continue;
        };
        let preferred = anchor.constraintGreaterThanOrEqualToConstant(value.max(0.0));
        // 優先度は活性化する前にしか変えられない。
        preferred.setPriority(PREFERRED_SIZE_PRIORITY);
        constraints.push(preferred);
    }

    let identifier = NSString::from_str(SIZING_ID);
    for constraint in &constraints {
        constraint.setIdentifier(Some(&identifier));
    }
    if !constraints.is_empty() {
        NSLayoutConstraint::activateConstraints(&NSArray::from_retained_slice(&constraints));
    }

    // 主軸方向の `Fill` は「余りを受け取る」= hugging priority を下げること。
    set_hugging(view, true, sizing.width.is_fill());
    set_hugging(view, false, sizing.height.is_fill());
    // `Fill` は中身の intrinsic size より狭くなってもよい。
    set_compression_resistance(view, true, sizing.width.is_fill());
    set_compression_resistance(view, false, sizing.height.is_fill());

    // すでにコンテナへ入っていれば、親の側の制約と配置も張り直してもらう。
    refresh_in_container(view);
}

fn set_hugging(view: &NSView, horizontal: bool, fill: bool) {
    let priority = if fill { FILL_HUGGING } else { HUG_CONTENT };
    view.setContentHuggingPriority_forOrientation(priority, orientation(horizontal));
}

fn set_compression_resistance(view: &NSView, horizontal: bool, fill: bool) {
    let priority = if fill {
        FILL_COMPRESSION_RESISTANCE
    } else {
        HUG_CONTENT
    };
    view.setContentCompressionResistancePriority_forOrientation(priority, orientation(horizontal));
}

fn orientation(horizontal: bool) -> NSLayoutConstraintOrientation {
    if horizontal {
        NSLayoutConstraintOrientation::Horizontal
    } else {
        NSLayoutConstraintOrientation::Vertical
    }
}

/// Whether this view wants to grow in that direction (= was given `Length::Fill`).
pub(crate) fn wants_fill(view: &NSView, horizontal: bool) -> bool {
    view.contentHuggingPriorityForOrientation(orientation(horizontal)) <= FILL_HUGGING
}

/// The maximum `set_sizing` gave this view in that direction
/// ([`Sizing::max_width`] / [`Sizing::max_height`]).
pub(crate) fn max_size(view: &NSView, horizontal: bool) -> Option<f64> {
    let attribute = if horizontal {
        NSLayoutAttribute::Width
    } else {
        NSLayoutAttribute::Height
    };
    let constraints = view.constraints();
    (0..constraints.len())
        .map(|index| constraints.objectAtIndex(index))
        .find(|constraint| {
            constraint.relation() == NSLayoutRelation::LessThanOrEqual
                && constraint.firstAttribute() == attribute
                && constraint
                    .identifier()
                    .is_some_and(|id| id.to_string() == SIZING_ID)
        })
        .map(|constraint| constraint.constant())
}

/// Makes an `Auto` child keep its content width.
pub(crate) fn keep_auto_size(view: &NSView, horizontal: bool) {
    view.setContentHuggingPriority_forOrientation(AUTO_HUGGING, orientation(horizontal));
}

/// The priority of the tail spacer's desire to shrink when it "may take the
/// leftover space".
///
/// Higher than an explicit `Fill` / `Spacer` ([`FILL_HUGGING`]) and lower than
/// `Auto` children. The tail spacer absorbs leftover space only when nothing else
/// asks for it.
pub(crate) const TAIL_PRIORITY: NSLayoutPriority = 2.0;

/// How strongly the tail spacer of a nested `Stack` wants to shrink.
///
/// The tail spacer is an `NSView` with no content and no intrinsic size, so **as is
/// it offers no resistance to growing**. If the outer and inner tail spacers were
/// treated the same, either one growing would be an equally valid Auto Layout
/// solution, and the inner one might grow, leaving a large gap in the middle of a
/// section. Making only the nested ones shrink strongly settles that the outermost
/// one absorbs the leftover space.
pub(crate) const NESTED_TAIL_PRIORITY: NSLayoutPriority = HUG_CONTENT;

/// Creates the constraint "want the tail spacer to be 0 on the main axis" (not
/// activated yet).
pub(crate) fn tail_zero_constraint(tail: &NSView, vertical: bool) -> Retained<NSLayoutConstraint> {
    let anchor = if vertical {
        tail.heightAnchor()
    } else {
        tail.widthAnchor()
    };
    let constraint = anchor.constraintEqualToConstant(0.0);
    constraint.setPriority(TAIL_PRIORITY);
    constraint
}

/// The identifier of the wish per cell. It is replaced only when placed into the
/// same cell again.
fn grow_identifier(cell: &GridCell, horizontal: bool) -> String {
    format!(
        "{GRID_GROW_ID}.{}.{}.{}",
        if horizontal { "width" } else { "height" },
        cell.column,
        cell.row
    )
}

fn clear_sizing_constraints(view: &NSView) {
    let constraints = view.constraints();
    let mine: Vec<Retained<NSLayoutConstraint>> = (0..constraints.len())
        .map(|index| constraints.objectAtIndex(index))
        .filter(|constraint| {
            constraint
                .identifier()
                .is_some_and(|id| id.to_string() == SIZING_ID)
        })
        .collect();
    if !mine.is_empty() {
        NSLayoutConstraint::deactivateConstraints(&NSArray::from_retained_slice(&mine));
    }
}

/// Prepares a child to be put into a container. Leaves it to Auto Layout.
pub(crate) fn prepare_child(view: &NSView) {
    view.setTranslatesAutoresizingMaskIntoConstraints(false);
}

/// Tells the chain of parents that the content's size changed.
///
/// **Children whose height changes after being shown** (an expanded `Expander`, a
/// list with rows added, a `Stack` with children added) do not get the parent's
/// layout to run without this. `Auto` rows of a `Grid` recompute their heights in
/// their own `layout`, so unless they are told layout is needed, the rows keep
/// their old heights, squashing the content and overlapping the rows after them.
/// Going up just one level is not enough (if the grid is inside a further Stack or
/// scroll view, it would stop there).
pub(crate) fn invalidate_ancestors(view: &NSView) {
    view.invalidateIntrinsicContentSize();
    view.setNeedsLayout(true);
    let mut current = unsafe { view.superview() };
    while let Some(parent) = current {
        parent.invalidateIntrinsicContentSize();
        parent.setNeedsLayout(true);
        current = unsafe { parent.superview() };
    }
}

// ------------------------------------------------- Telling the container

/// What a container does to redo its own side when a child's specification changes.
type Refresh = Rc<dyn Fn(&NSView)>;

thread_local! {
    /// Looks up, by the container's view (its address), what redoes its side.
    static CONTAINERS: RefCell<HashMap<usize, Refresh>> = RefCell::new(HashMap::new());
}

fn view_key(view: &NSView) -> usize {
    view as *const NSView as usize
}

/// Registers a container whose own constraints or placement depend on its children's
/// specifications.
///
/// In AppKit, cross-axis `Fill` and placement within a grid cell are expressed by
/// constraints and settings **on the parent's side**. [`apply_sizing`] on the child
/// alone does not change them, so `apply_sizing` looks up the child's `superview` in
/// this table and has the parent redo them (the counterpart of Windows's
/// `LayoutState` and the Web's `ParentLayout`).
///
/// This way the result does not depend on whether `set_sizing` is called before or
/// after adding to the container. A registered container is removed with
/// [`unregister_container`] when it is dropped.
pub(crate) fn register_container(native: &NSView, refresh: impl Fn(&NSView) + 'static) {
    CONTAINERS.with(|containers| {
        containers
            .borrow_mut()
            .insert(view_key(native), Rc::new(refresh));
    });
}

/// Removes what [`register_container`] registered.
pub(crate) fn unregister_container(native: &NSView) {
    // スレッドの終了中は表そのものが先に破棄されていることがある。
    let _ = CONTAINERS.try_with(|containers| {
        containers.borrow_mut().remove(&view_key(native));
    });
}

/// If `view`'s parent is a registered container, has it redo its side for `view`.
fn refresh_in_container(view: &NSView) {
    let Some(parent) = (unsafe { view.superview() }) else {
        return;
    };
    // 表を借りたまま呼ぶと、張り直しの中で登録・解除が起きたときにぶつかる。
    let refresh =
        CONTAINERS.with(|containers| containers.borrow().get(&view_key(&parent)).cloned());
    if let Some(refresh) = refresh {
        refresh(view);
    }
}

// ----------------------------------------------------------------- Spacer

struct SpacerInner {
    native: Retained<NSView>,
}

/// A widget that is just empty space.
///
/// It has no content and takes all of a stack's leftover space. Placed in the
/// middle of a vertical stack, it pushes the elements after it to the bottom.
#[derive(Clone)]
pub struct Spacer(Rc<SpacerInner>);
impl_widget!(Spacer);

impl Spacer {
    pub(crate) fn new(mtm: MainThreadMarker) -> Self {
        let native = NSView::new(mtm);
        relax_hugging(&native);
        Self(Rc::new(SpacerInner { native }))
    }
}

/// Makes a content-less view resist neither shrinking nor growing.
///
/// Views without a natural size, such as `Spacer` and `Canvas`, become recipients
/// of the parent's leftover space this way.
pub(crate) fn relax_hugging(view: &NSView) {
    for horizontal in [true, false] {
        view.setContentHuggingPriority_forOrientation(FILL_HUGGING, orientation(horizontal));
        view.setContentCompressionResistancePriority_forOrientation(
            FILL_HUGGING,
            orientation(horizontal),
        );
    }
}

// ------------------------------------------------------------------- Grid

/// Information `NSGridView` uses to decide the height of Auto rows.
///
/// Even though AppKit can compute an `NSStackView`'s `fittingSize`, it does not use
/// it as the natural height of a Grid cell, and may squash the Stack to match a
/// button in the same row. The real views placed in rows are kept, and the required
/// height is recomputed only for Auto rows.
type GridCells = Rc<RefCell<Vec<(Retained<NSView>, GridCell)>>>;

/// The "want to grow" wishes placed on `Fill` children, where they are placed, and
/// whether they are horizontal.
///
/// How much they grow depends on the sizes the other columns (rows) need, so the
/// constants are recomputed on every layout. For that, the constraints are kept
/// together with their positions.
type GridGrows = Rc<RefCell<Vec<(Retained<NSLayoutConstraint>, GridCell, bool)>>>;

struct ContentSizedGridState {
    row_tracks: Rc<RefCell<Vec<Track>>>,
    column_tracks: Rc<RefCell<Vec<Track>>>,
    cells: GridCells,
    grows: GridGrows,
    updating: Cell<bool>,
}

define_class!(
    #[unsafe(super(NSGridView))]
    #[thread_kind = MainThreadOnly]
    #[name = "NauiContentSizedGridView"]
    #[ivars = ContentSizedGridState]
    struct ContentSizedGridView;

    unsafe impl NSObjectProtocol for ContentSizedGridView {}

    impl ContentSizedGridView {
        #[unsafe(method(layout))]
        fn layout(&self) {
            let _: () = unsafe { msg_send![super(self), layout] };
            if self.ivars().updating.replace(true) {
                return;
            }
            let rows = self.update_auto_row_heights();
            let columns = self.update_auto_column_widths();
            let grows = self.update_grow_sizes();
            if rows || columns || grows {
                let _: () = unsafe { msg_send![super(self), layout] };
                self.invalidateIntrinsicContentSize();
            }
            self.ivars().updating.set(false);
        }
    }
);

impl ContentSizedGridView {
    fn new(
        mtm: MainThreadMarker,
        row_tracks: Rc<RefCell<Vec<Track>>>,
        column_tracks: Rc<RefCell<Vec<Track>>>,
        cells: GridCells,
        grows: GridGrows,
    ) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(ContentSizedGridState {
            row_tracks,
            column_tracks,
            cells,
            grows,
            updating: Cell::new(false),
        });
        unsafe { msg_send![super(this), init] }
    }

    /// Makes each Auto row at least the fitting height of the cell contents that span
    /// it. Multi-row cells spread their required height evenly over their span to avoid
    /// oversized rows.
    fn update_auto_row_heights(&self) -> bool {
        let tracks = self.ivars().row_tracks.borrow();
        let cells = self.ivars().cells.borrow();
        let mut changed = false;
        for (index, track) in tracks.iter().copied().enumerate() {
            if track != Track::Auto || index >= self.numberOfRows() as usize {
                continue;
            }
            let desired = cells
                .iter()
                .filter(|(view, cell)| {
                    !view.isHidden()
                        && index >= cell.row
                        && index < cell.row.saturating_add(cell.row_span)
                })
                .map(|(view, cell)| {
                    let span = cell.row_span.max(1) as f64;
                    (view.fittingSize().height / span).max(0.0)
                })
                .reduce(f64::max);
            let row = self.rowAtIndex(index as isize);
            match desired {
                Some(desired) if (row.height() - desired).abs() > 0.5 => {
                    row.setHeight(desired);
                    changed = true;
                }
                None if row.height() > 0.5 => {
                    row.setHeight(unsafe { objc2_app_kit::NSGridViewSizeForContent });
                    changed = true;
                }
                _ => {}
            }
        }
        changed
    }

    /// Sets the width of each Auto column to the width of its content.
    ///
    /// NSGridView has no rule for which column receives the leftover, and the
    /// children's hugging alone does not stop an Auto column from absorbing it after
    /// a relayout (switching the theme and so on). Like row heights, column widths
    /// are recomputed from the content on every layout.
    ///
    /// Only cells spanning Auto columns alone are counted (spread evenly over their
    /// span). Cells that also span a `Fill` or `Fixed` column are left out, since
    /// that column takes their width. An Auto column without content gets width 0;
    /// left at the "fit the content" sentinel, an empty column could take the
    /// leftover.
    fn update_auto_column_widths(&self) -> bool {
        let tracks = self.ivars().column_tracks.borrow();
        let cells = self.ivars().cells.borrow();
        let columns = (self.numberOfColumns().max(0) as usize).min(tracks.len());
        let auto_only = |cell: &GridCell| {
            let end = cell.column.saturating_add(cell.column_span.max(1));
            end <= columns && tracks[cell.column..end].iter().all(|t| *t == Track::Auto)
        };
        let mut changed = false;
        for index in 0..columns {
            if tracks[index] != Track::Auto {
                continue;
            }
            let desired = cells
                .iter()
                .filter(|(view, cell)| {
                    !view.isHidden()
                        && index >= cell.column
                        && index < cell.column.saturating_add(cell.column_span.max(1))
                        && auto_only(cell)
                })
                .map(|(view, cell)| {
                    // 列の幅は枠ではなく位置合わせの矩形で決まる (ラベルなら
                    // 左右 2pt 内側)。枠の幅で決めると列が広がってしまう。
                    let fitting = NSRect::new(NSPoint::ZERO, view.fittingSize());
                    let width = view.alignmentRectForFrame(fitting).size.width;
                    let span = cell.column_span.max(1) as f64;
                    (width / span).max(0.0)
                })
                .fold(0.0, f64::max);
            let column = self.columnAtIndex(index as isize);
            if (column.width() - desired).abs() > 0.5 {
                column.setWidth(desired);
                changed = true;
            }
        }
        changed
    }

    /// Matches the "want to grow" wishes on `Fill` children to their cell's share.
    ///
    /// The desired size is "the grid's size − the sizes the other columns (rows)
    /// need", where a capped track counts its maximum only as far as the grid has
    /// room beyond what the tracks need. The other tracks' needs change on every layout, so it is recomputed as
    /// the constraint's constant. Vertically too: other `Fill` rows are not decided
    /// by required heights the way `Auto` rows are, so wishing for the whole grid's
    /// height would squash them (a row whose child stops at a maximum only wishes
    /// weakly for it).
    fn update_grow_sizes(&self) -> bool {
        let grows = self.ivars().grows.borrow();
        if grows.is_empty() {
            return false;
        }
        let widths = (self.track_needs(true, false), self.track_needs(true, true));
        let heights = (
            self.track_needs(false, false),
            self.track_needs(false, true),
        );
        let size = self.frame().size;
        let mut changed = false;
        for (constraint, cell, horizontal) in grows.iter() {
            let (needs, room) = if *horizontal {
                (&widths, size.width)
            } else {
                (&heights, size.height)
            };
            // 上限付きのトラックに残す分 (望む分から必須の分を引いたもの) は、
            // グリッドの今の大きさのうち必須の分を除いた残りまでに抑える。抑えないと、
            // 上限より狭いときにこの子が負の大きさを望み、その差を埋めようと
            // グリッド (ひいてはウィンドウ) を押し広げる。
            let required = self.outside_cell(cell, *horizontal, &needs.0);
            let wished = self.outside_cell(cell, *horizontal, &needs.1);
            let outside = required + (wished - required).min((room - required).max(0.0));
            if (constraint.constant() + outside).abs() > 0.5 {
                constraint.setConstant(-outside);
                changed = true;
            }
        }
        changed
    }

    /// The size each column (row) needs, or wishes for with `wished`.
    ///
    /// Tracks with a decided size ([`Self::decided_track_size`]) take that size
    /// regardless of their content. Others are decided from what their content needs
    /// ([`needed_size`]), and cells spanning several tracks spread it evenly over
    /// their span.
    fn track_needs(&self, horizontal: bool, wished: bool) -> Vec<f64> {
        let tracks = if horizontal {
            self.numberOfColumns()
        } else {
            self.numberOfRows()
        }
        .max(0) as usize;
        let decided: Vec<Option<f64>> = (0..tracks)
            .map(|index| self.decided_track_size(index, horizontal))
            .collect();
        let mut needs: Vec<f64> = decided.iter().map(|size| size.unwrap_or(0.0)).collect();
        for (view, cell) in self.ivars().cells.borrow().iter() {
            if view.isHidden() {
                continue;
            }
            let (start, span) = if horizontal {
                (cell.column, cell.column_span.max(1))
            } else {
                (cell.row, cell.row_span.max(1))
            };
            let each = (needed_size(view, horizontal, wished) / span as f64).max(0.0);
            for index in start..(start + span).min(tracks) {
                if decided[index].is_some() {
                    continue;
                }
                needs[index] = f64::max(needs[index], each);
            }
        }
        needs
    }

    /// The size, if the column's (row's) size is decided. `None` for tracks that fit
    /// their content (`Fill`).
    ///
    /// Besides `Fixed` tracks, this includes `Auto` tracks whose size
    /// [`Self::update_auto_column_widths`] / [`Self::update_auto_row_heights`]
    /// decided. Tracks of size 0 (such as an `Auto` column without content) count as
    /// decided too; otherwise the size of cells spanning them is spread onto them,
    /// overestimating what the other tracks need.
    fn decided_track_size(&self, index: usize, horizontal: bool) -> Option<f64> {
        let size = if horizontal {
            self.columnAtIndex(index as isize).width()
        } else {
            self.rowAtIndex(index as isize).height()
        };
        // `NSGridViewSizeForContent` は「中身に合わせる」を表す番兵値。
        let content = unsafe { objc2_app_kit::NSGridViewSizeForContent };
        (size != content && size.is_finite() && size >= 0.0).then_some(size)
    }

    /// The size this cell cannot use (the other tracks' needs, plus the outer padding
    /// and the spacing between tracks).
    fn outside_cell(&self, cell: &GridCell, horizontal: bool, needs: &[f64]) -> f64 {
        let tracks = needs.len();
        let (start, span, spacing) = if horizontal {
            (cell.column, cell.column_span.max(1), self.columnSpacing())
        } else {
            (cell.row, cell.row_span.max(1), self.rowSpacing())
        };
        let end = (start + span).min(tracks);
        let mut outside: f64 = needs
            .iter()
            .enumerate()
            .filter(|(index, _)| *index < start || *index >= end)
            .map(|(_, size)| *size)
            .sum();
        // トラック間のすき間。セルがまたぐぶんはセルの取り分なので数えない。
        outside += spacing * tracks.saturating_sub(span) as f64;
        // 外周の余白 (apply_padding が両端のトラックへ入れる)。
        for index in 0..tracks {
            outside += if horizontal {
                let column = self.columnAtIndex(index as isize);
                column.leadingPadding() + column.trailingPadding()
            } else {
                let row = self.rowAtIndex(index as isize);
                row.topPadding() + row.bottomPadding()
            };
        }
        outside
    }
}

/// The size a grid child needs in that direction, or wishes for with `wished`.
///
/// What it needs is its `fittingSize`. A `Fill` child with a maximum also wishes for
/// its maximum, only weakly ([`GRID_CAPPED_GROW_PRIORITY`]), which `fittingSize`
/// compresses away. So with `wished` its maximum counts too: the other tracks then
/// leave it that much while the grid has room for it
/// ([`ContentSizedGridView::update_grow_sizes`]).
fn needed_size(view: &NSView, horizontal: bool, wished: bool) -> f64 {
    let fitting = view.fittingSize();
    let fitting = if horizontal {
        fitting.width
    } else {
        fitting.height
    };
    match max_size(view, horizontal) {
        Some(max) if wished && wants_fill(view, horizontal) => fitting.max(max),
        _ => fitting,
    }
}

struct GridInner {
    native: Retained<NSGridView>,
    children: RefCell<Vec<Box<dyn Widget>>>,
    row_tracks: Rc<RefCell<Vec<Track>>>,
    column_tracks: Rc<RefCell<Vec<Track>>>,
    cells: GridCells,
    /// The wishes to grow horizontally. Kept so their constants can be recomputed to
    /// match the cell's share.
    grows: GridGrows,
    /// The same object as `native`. Kept as the subclass that recomputes Auto rows.
    content_sized: Retained<ContentSizedGridView>,
    padding: Cell<Padding>,
}

impl Drop for GridInner {
    fn drop(&mut self) {
        unregister_container(&self.native);
    }
}

/// A container that positions children by row and column (NSGridView).
#[derive(Clone)]
pub struct Grid(Rc<GridInner>);
impl_widget!(Grid);

impl Grid {
    pub(crate) fn new(mtm: MainThreadMarker) -> Self {
        let row_tracks = Rc::new(RefCell::new(Vec::new()));
        let column_tracks = Rc::new(RefCell::new(Vec::new()));
        let cells = Rc::new(RefCell::new(Vec::new()));
        let grows: GridGrows = Rc::new(RefCell::new(Vec::new()));
        let content_sized = ContentSizedGridView::new(
            mtm,
            row_tracks.clone(),
            column_tracks.clone(),
            cells.clone(),
            grows.clone(),
        );
        let native = content_sized.clone().into_super();
        let this = Self(Rc::new(GridInner {
            native,
            children: RefCell::new(Vec::new()),
            row_tracks,
            column_tracks,
            cells,
            grows,
            content_sized,
            padding: Cell::new(Padding::ZERO),
        }));
        let weak = Rc::downgrade(&this.0);
        register_container(&this.0.native, move |view| {
            if let Some(inner) = weak.upgrade() {
                Grid(inner).refresh_child(view);
            }
        });
        this
    }

    /// Redoes the placement in the cell of a child whose specification changed.
    ///
    /// Called from [`apply_sizing`] through [`register_container`].
    fn refresh_child(&self, view: &NSView) {
        let cell = self
            .0
            .cells
            .borrow()
            .iter()
            .find(|(placed, _)| std::ptr::eq::<NSView>(&**placed, view))
            .map(|(_, cell)| *cell);
        let Some(cell) = cell else {
            return;
        };
        self.apply_cell_layout(view, &cell);
        self.update_tracks();
        invalidate_ancestors(&self.0.native);
    }

    /// Recomputes the sizes naui decides (Auto rows and columns, and the share of
    /// `Fill` children).
    fn update_tracks(&self) {
        self.0.content_sized.update_auto_row_heights();
        self.0.content_sized.update_auto_column_widths();
        self.0.content_sized.update_grow_sizes();
    }

    /// The spacing between columns and between rows.
    pub fn set_spacing(&self, column: f64, row: f64) {
        self.0.native.setColumnSpacing(column);
        self.0.native.setRowSpacing(row);
    }

    /// The outer padding.
    ///
    /// NSGridView has no padding setting, so it is passed as padding of the rows and
    /// columns at both ends.
    pub fn set_padding(&self, padding: Padding) {
        self.0.padding.set(padding);
        self.apply_padding();
    }

    fn apply_padding(&self) {
        let padding = self.0.padding.get();
        let columns = self.0.native.numberOfColumns();
        let rows = self.0.native.numberOfRows();
        for index in 0..columns {
            let column = self.0.native.columnAtIndex(index);
            column.setLeadingPadding(if index == 0 { padding.left } else { 0.0 });
            column.setTrailingPadding(if index == columns - 1 {
                padding.right
            } else {
                0.0
            });
        }
        for index in 0..rows {
            let row = self.0.native.rowAtIndex(index);
            row.setTopPadding(if index == 0 { padding.top } else { 0.0 });
            row.setBottomPadding(if index == rows - 1 {
                padding.bottom
            } else {
                0.0
            });
        }
    }

    /// Places a child at the given position. Missing rows and columns are added
    /// automatically.
    ///
    /// **Children cannot be stacked in the same cell.** An `NSGridCell` can hold only
    /// one content view, so placing something into a cell that already holds one
    /// removes the previous child (the other three platforms allow stacking). A merged
    /// cell is the same cell whichever position within it you point at, so this rule
    /// applies to the whole range.
    pub fn attach(&self, child: &dyn Widget, cell: GridCell) {
        let view = child.native_view();
        prepare_child(&view);
        self.ensure_size(cell.columns_needed(), cell.rows_needed());

        let column = cell.column as isize;
        let row = cell.row as isize;
        if cell.column_span > 1 || cell.row_span > 1 {
            self.0.native.mergeCellsInHorizontalRange_verticalRange(
                NSRange::new(cell.column, cell.column_span),
                NSRange::new(cell.row, cell.row_span),
            );
        }
        let target: Retained<NSGridCell> = self.0.native.cellAtColumnIndex_rowIndex(column, row);
        // `NSGridCell` は中身を 1 つしか持てない。重ねて置くと前のビューが
        // 宙に浮いたまま残るので、先に外しておく (結合したマスでは、範囲内の
        // どの位置を指しても同じマスになる)。
        self.remove_sharing(&target);
        target.setContentView(Some(&view));
        self.apply_cell_layout(&view, &cell);
        self.apply_padding();
        self.0.cells.borrow_mut().push((view, cell));
        self.0.children.borrow_mut().push(child.boxed_clone());
        self.update_tracks();
    }

    /// Decides the placement within the cell and the wish to grow from the child's
    /// specification (whether it is `Fill`).
    ///
    /// Called when the child is placed, and again when its specification changes
    /// after that.
    fn apply_cell_layout(&self, view: &NSView, cell: &GridCell) {
        let column = cell.column as isize;
        let target: Retained<NSGridCell> = self
            .0
            .native
            .cellAtColumnIndex_rowIndex(column, cell.row as isize);
        // 列が `Fill` でも、子が `Auto` なら列の幅を継承して広げない。
        // `Inherited` のままだと、再レイアウト時に NSGridView の列配置を
        // 継承してラベルまで列いっぱいになることがある。
        let fill_width = wants_fill(view, true);
        // 上限付きの `Fill` は、セルいっぱいには広げずに Auto と同じ位置へ置く。
        // 配置を `Fill` にするとセルの端と子の端が必須で結ばれ、子の上限が
        // そのまま行や列の上限になる。そうなると `Fill` の行が余りを受け取れず、
        // 上限のぶんまで縮んでしまう (GTK や WinUI では行が余りを受け取り、
        // 子はその中で上限まで広がる)。子を上限まで広げるのは set_grow_hint。
        let stretch_width = fill_width && max_size(view, true).is_none();
        if !fill_width {
            // Leading だけでは、NSGridView が再レイアウト時に作る幅の
            // 制約に intrinsic size が負けることがある。Auto の意味を
            // hugging priority でも固定しておく。
            keep_auto_size(view, true);
            // 列の `Fill` も残すと、AppKit の再レイアウトでセルの配置が
            // `Inherited` 相当に戻る環境がある。Fill の子はセル側で明示
            // しているので、列の既定を先頭寄せにして継承経路も断つ。
            self.0
                .native
                .columnAtIndex(column)
                .setXPlacement(NSGridCellPlacement::Leading);
        }
        target.setXPlacement(if stretch_width {
            NSGridCellPlacement::Fill
        } else {
            NSGridCellPlacement::Leading
        });
        self.set_grow_hint(view, cell, true, fill_width);
        // 縦は中央ぞろえ。NSGridView の既定 (上ぞろえ) だと、同じ行に置いた
        // ラベルと入力欄のように高さの違うものが上端で揃ってしまう。
        let fill_height = wants_fill(view, false);
        let stretch_height = fill_height && max_size(view, false).is_none();
        if !fill_height {
            // 横と同じ理由で、`Auto` の子は行の余りを受け取らない。
            // NSStackView のようにコンテナ自身の hugging priority が低い子は、
            // これが無いと `Fill` 行より先に余りを吸ってしまい、`Fill` 行が
            // 中身の高さ (タブなら見出しだけ) まで潰れる。
            keep_auto_size(view, false);
        }
        self.set_grow_hint(view, cell, false, fill_height);
        target.setYPlacement(if stretch_height {
            NSGridCellPlacement::Fill
        } else {
            NSGridCellPlacement::Center
        });
    }

    /// Places a weak wish "want to grow by the leftover amount" on children in `Fill`
    /// rows and columns.
    ///
    /// The sizes of `NSGridView` rows and columns are decided by their content, and
    /// **where leftover space goes is undefined**. Hugging priority alone is not
    /// enough; blank space sometimes goes to the `Auto` side.
    ///
    /// So "want to be the same size as the grid" is added at a weak priority, steering
    /// leftover space to the `Fill` rows and columns. **With several tracks, however,
    /// this as is would make one cell's child demand the whole grid**, so the sizes
    /// the other columns (rows) need are subtracted as a constant, leaving it wanting
    /// only that cell's share (the merged area, if merged). The amount subtracted
    /// depends on the content, so [`ContentSizedGridView::update_grow_sizes`]
    /// recomputes it on every layout.
    ///
    /// **A child whose `Fill` has a maximum wishes for its maximum instead.** It stops
    /// there, so "the same size as the grid" cannot hold, and the solver would shrink
    /// the grid toward the child to make up for it, leaving the `Fill` row no
    /// leftover. Such a child is not tied to the cell's edges (see `attach`), so the
    /// row grows past it and the child sits where an `Auto` child would (at the start
    /// horizontally, in the middle vertically).
    fn set_grow_hint(&self, view: &NSView, cell: &GridCell, horizontal: bool, wanted: bool) {
        let identifier = grow_identifier(cell, horizontal);
        // 同じセルの古い希望は、定数を引き直す対象から外す。
        self.0
            .grows
            .borrow_mut()
            .retain(|(_, old, old_horizontal)| {
                *old_horizontal != horizontal || old.column != cell.column || old.row != cell.row
            });
        // 同じセルに前へ張った希望があれば外す。上限への希望は子のビュー自身に、
        // グリッドと結ぶ希望はグリッドに付いている。
        let previous: Vec<Retained<NSLayoutConstraint>> =
            [self.0.native.constraints(), view.constraints()]
                .iter()
                .flat_map(|constraints| {
                    (0..constraints.len()).map(|index| constraints.objectAtIndex(index))
                })
                .filter(|constraint| {
                    constraint
                        .identifier()
                        .is_some_and(|id| id.to_string() == identifier)
                })
                .collect();
        if !previous.is_empty() {
            NSLayoutConstraint::deactivateConstraints(&NSArray::from_retained_slice(&previous));
        }
        if !wanted {
            return;
        }
        let (child, grid) = if horizontal {
            (view.widthAnchor(), self.0.native.widthAnchor())
        } else {
            (view.heightAnchor(), self.0.native.heightAnchor())
        };
        let max = max_size(view, horizontal);
        let grow = match max {
            Some(max) => child.constraintGreaterThanOrEqualToConstant(max),
            None => child.constraintEqualToAnchor(&grid),
        };
        grow.setPriority(if max.is_some() {
            GRID_CAPPED_GROW_PRIORITY
        } else {
            GRID_GROW_PRIORITY
        });
        grow.setIdentifier(Some(&NSString::from_str(&identifier)));
        NSLayoutConstraint::activateConstraints(&NSArray::from_retained_slice(&[grow.clone()]));
        // 上限への希望は定数が決まっているので、引き直す対象にしない。
        if max.is_none() {
            self.0.grows.borrow_mut().push((grow, *cell, horizontal));
        }
    }

    /// Replaces what is in the cell. Whatever was in the same cell is removed.
    pub fn replace(&self, child: &dyn Widget, cell: GridCell) {
        self.remove(cell);
        self.attach(child, cell);
    }

    /// Removes whatever is in the given cell. Does nothing if it is empty.
    ///
    /// Only the column and row of `cell` are considered, not the span.
    ///
    /// **Removing a merged cell (a child with a span) leaves the merge itself in
    /// place.** `NSGridView` has no API to unmerge, and rows and columns crossing a
    /// merge cannot be removed either (AppKit throws an exception if you try). naui
    /// treats a merged range as one cell, so a one-cell child placed in its place gets
    /// the same position and size as an ordinary cell. There are two differences: a
    /// `Fill` child stretches to the merged range, and placing into another position
    /// within the range is the same cell, so the previous child is removed.
    pub fn remove(&self, cell: GridCell) {
        self.remove_at(cell);
        self.apply_padding();
        self.update_tracks();
    }

    /// Removes every child. The row and column settings stay.
    pub fn clear(&self) {
        let cells: Vec<GridCell> = self
            .0
            .cells
            .borrow()
            .iter()
            .map(|(_, cell)| *cell)
            .collect();
        for cell in cells {
            self.remove_at(cell);
        }
        self.apply_padding();
        self.update_tracks();
    }

    /// Removes the child in the given cell. If several are stacked in the same cell,
    /// removes them all.
    fn remove_at(&self, cell: GridCell) {
        loop {
            let found = self
                .0
                .cells
                .borrow()
                .iter()
                .position(|(_, placed)| placed.column == cell.column && placed.row == cell.row);
            let Some(found) = found else {
                return;
            };
            self.detach(found);
        }
    }

    /// Removes the children using the same `NSGridCell` as `target`.
    ///
    /// A merged cell returns the same `NSGridCell` from any position. A cell can hold
    /// only one content view, so the child is removed before placing another there.
    fn remove_sharing(&self, target: &NSGridCell) {
        loop {
            let found = self.0.cells.borrow().iter().position(|(_, placed)| {
                let existing = self
                    .0
                    .native
                    .cellAtColumnIndex_rowIndex(placed.column as isize, placed.row as isize);
                std::ptr::eq(Retained::as_ptr(&existing), target as *const NSGridCell)
            });
            let Some(found) = found else {
                return;
            };
            self.detach(found);
        }
    }

    /// Removes the `found`-th recorded child from its cell and from the view
    /// hierarchy.
    fn detach(&self, found: usize) {
        let (view, placed) = self.0.cells.borrow_mut().remove(found);
        self.0.children.borrow_mut().remove(found);
        // 「余りのぶんだけ伸びたい」という希望を外す。
        self.set_grow_hint(&view, &placed, true, false);
        self.set_grow_hint(&view, &placed, false, false);
        // NSGridCell::setContentView(None) だけでは、外したビューが
        // NSGridView の subviews に残ることがある。親からも明示的に外す。
        let target = self
            .0
            .native
            .cellAtColumnIndex_rowIndex(placed.column as isize, placed.row as isize);
        target.setContentView(None);
        view.removeFromSuperview();
    }

    /// How a column's width is determined.
    pub fn set_column_track(&self, index: usize, track: Track) {
        self.ensure_size(index + 1, 0);
        self.0.column_tracks.borrow_mut()[index] = track;
        let column = self.0.native.columnAtIndex(index as isize);
        match track {
            // NSGridViewSizeForContent は「中身に合わせる」を表す番兵値。
            Track::Auto => {
                column.setWidth(unsafe { objc2_app_kit::NSGridViewSizeForContent });
                column.setXPlacement(NSGridCellPlacement::Leading);
            }
            Track::Fixed(value) => {
                column.setWidth(value);
                column.setXPlacement(NSGridCellPlacement::Leading);
            }
            Track::Fill(_) => {
                column.setWidth(unsafe { objc2_app_kit::NSGridViewSizeForContent });
                column.setXPlacement(NSGridCellPlacement::Fill);
            }
        }
        self.0.content_sized.update_auto_column_widths();
    }

    /// How a row's height is determined.
    pub fn set_row_track(&self, index: usize, track: Track) {
        self.ensure_size(0, index + 1);
        self.0.row_tracks.borrow_mut()[index] = track;
        let row = self.0.native.rowAtIndex(index as isize);
        match track {
            Track::Auto => {
                row.setHeight(unsafe { objc2_app_kit::NSGridViewSizeForContent });
                // Auto 行は、Fill 行が受け取る余白を継承しない。
                row.setYPlacement(NSGridCellPlacement::Center);
            }
            Track::Fixed(value) => {
                row.setHeight(value);
                row.setYPlacement(NSGridCellPlacement::Center);
            }
            Track::Fill(_) => {
                row.setHeight(unsafe { objc2_app_kit::NSGridViewSizeForContent });
                row.setYPlacement(NSGridCellPlacement::Fill);
            }
        }
        self.0.content_sized.update_auto_row_heights();
    }

    /// The current number of columns.
    pub fn columns(&self) -> usize {
        self.0.native.numberOfColumns().max(0) as usize
    }

    /// The current number of rows.
    pub fn rows(&self) -> usize {
        self.0.native.numberOfRows().max(0) as usize
    }

    /// The number of children placed.
    pub fn len(&self) -> usize {
        self.0.children.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn ensure_size(&self, columns: usize, rows: usize) {
        let empty = NSArray::from_slice(&[]);
        while (self.0.native.numberOfColumns() as usize) < columns {
            self.0.native.addColumnWithViews(&empty);
            self.0.column_tracks.borrow_mut().push(Track::Auto);
        }
        while (self.0.native.numberOfRows() as usize) < rows {
            self.0.native.addRowWithViews(&empty);
            self.0.row_tracks.borrow_mut().push(Track::Auto);
        }
    }
}

// ----------------------------------------------------------------- Scroll

// 上から下へ並べるためのクリップビュー。
//
// AppKit の座標系は左下原点なので、そのままだと中身が下端から積まれる。
// `isFlipped` を返すのは AppKit 自身が用意している切り替え手段。
define_class!(
    #[unsafe(super(NSClipView))]
    #[thread_kind = MainThreadOnly]
    #[name = "NauiFlippedClipView"]
    /// An NSClipView that lays out scroll content from the top.
    struct FlippedClipView;

    unsafe impl NSObjectProtocol for FlippedClipView {}

    impl FlippedClipView {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        // 中身の大きさはクリップへの制約で決めているが、クリップの大きさが
        // 変わっても中身のレイアウトはこの回では解き直されない。
        //
        // `NSScrollView` はスクローラの出入りを `tile` で処理し、そこで
        // クリップの frame を直接書き換える。ウィンドウ側から始まった
        // レイアウトは、どのビューを回るかを tile より前に決めているので、
        // 中身へは届かない。クリップ自身の `layout` は tile の後に呼ばれる
        // ため、ここで中身の分を解かせる。
        //
        // 直さないと、スクロールバーを「常に表示」にした環境で中身が
        // スクローラのぶん (17pt) はみ出したままになる。横へ送らない
        // 設定だと、はみ出した分は二度と見えない。
        #[unsafe(method(layout))]
        fn layout(&self) {
            let _: () = unsafe { msg_send![super(self), layout] };
            if let Some(document) = self.documentView() {
                document.layoutSubtreeIfNeeded();
            }
        }
    }
);

impl FlippedClipView {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this: Allocated<Self> = Self::alloc(mtm);
        unsafe { msg_send![this, init] }
    }
}

define_class!(
    /// Receives changes to the clip view's position and size.
    ///
    /// Position changes (bounds) drive scroll notifications, and size changes (frame)
    /// are used to apply a `scroll_to` requested before the size was decided. The
    /// notification center does not hold observers strongly, so `ScrollInner` holds
    /// it, and it refers back weakly.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "NauiScrollObserver"]
    #[ivars = Weak<ScrollInner>]
    struct ScrollObserver;

    unsafe impl NSObjectProtocol for ScrollObserver {}

    impl ScrollObserver {
        #[unsafe(method(clipBoundsDidChange:))]
        fn clip_bounds_did_change(&self, _notification: &NSNotification) {
            if let Some(inner) = self.ivars().upgrade() {
                Scroll(inner).notify_if_moved();
            }
        }

        #[unsafe(method(clipFrameDidChange:))]
        fn clip_frame_did_change(&self, _notification: &NSNotification) {
            if let Some(inner) = self.ivars().upgrade() {
                Scroll(inner).schedule_pending(self.mtm());
            }
        }
    }
);

impl ScrollObserver {
    fn new(mtm: MainThreadMarker, inner: Weak<ScrollInner>) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(inner);
        unsafe { msg_send![super(this), init] }
    }
}

struct ScrollInner {
    native: Retained<NSScrollView>,
    child: RefCell<Option<Box<dyn Widget>>>,
    constraints: RefCell<Vec<Retained<NSLayoutConstraint>>>,
    horizontal: Cell<ScrollPolicy>,
    vertical: Cell<ScrollPolicy>,
    on_scroll: ScrollNotifier,
    /// The destination requested before the size was decided. Applied once it is.
    pending: Cell<Option<ScrollTarget>>,
    /// Applying `pending` is queued on the main queue.
    pending_scheduled: Cell<bool>,
    observer: RefCell<Option<Retained<ScrollObserver>>>,
}

impl Drop for ScrollInner {
    fn drop(&mut self) {
        if let Some(observer) = self.observer.borrow_mut().take() {
            unsafe { NSNotificationCenter::defaultCenter().removeObserver(&observer) };
        }
    }
}

/// A container that scrolls when its contents overflow (NSScrollView).
#[derive(Clone)]
pub struct Scroll(Rc<ScrollInner>);
impl_widget!(Scroll);

impl Scroll {
    pub(crate) fn new(mtm: MainThreadMarker) -> Self {
        let native = NSScrollView::new(mtm);
        native.setContentView(&FlippedClipView::new(mtm));
        native.setDrawsBackground(false);
        let this = Self(Rc::new(ScrollInner {
            native,
            child: RefCell::new(None),
            constraints: RefCell::new(Vec::new()),
            horizontal: Cell::new(ScrollPolicy::Never),
            vertical: Cell::new(ScrollPolicy::Auto),
            on_scroll: ScrollNotifier::default(),
            pending: Cell::new(None),
            pending_scheduled: Cell::new(false),
            observer: RefCell::new(None),
        }));
        this.apply_policy();
        this.observe_clip(mtm);
        this
    }

    fn observe_clip(&self, mtm: MainThreadMarker) {
        let observer = ScrollObserver::new(mtm, Rc::downgrade(&self.0));
        let clip = self.0.native.contentView();
        clip.setPostsBoundsChangedNotifications(true);
        clip.setPostsFrameChangedNotifications(true);
        let center = NSNotificationCenter::defaultCenter();
        unsafe {
            center.addObserver_selector_name_object(
                &observer,
                sel!(clipBoundsDidChange:),
                Some(NSViewBoundsDidChangeNotification),
                Some(&clip),
            );
            center.addObserver_selector_name_object(
                &observer,
                sel!(clipFrameDidChange:),
                Some(NSViewFrameDidChangeNotification),
                Some(&clip),
            );
        }
        *self.0.observer.borrow_mut() = Some(observer);
    }

    /// The current scroll position and sizes.
    ///
    /// Even if the contents just changed, it measures after finishing layout. If a
    /// destination was requested before the size was decided, it is applied here
    /// first.
    pub fn metrics(&self) -> ScrollMetrics {
        self.0.native.layoutSubtreeIfNeeded();
        self.apply_pending();
        self.read_metrics()
    }

    /// Scrolls to the given position, clamped to the scrollable range.
    ///
    /// If the size is not decided yet (before it is in a window, for example), it
    /// scrolls once it is.
    pub fn scroll_to(&self, x: f64, y: f64) {
        self.request(ScrollTarget::To { x, y });
    }

    /// Scrolls to the vertical end (the very bottom). The horizontal position stays.
    pub fn scroll_to_end(&self) {
        self.request(ScrollTarget::End);
    }

    /// Notified when the scroll position changes.
    ///
    /// Besides user interaction, it also arrives for moves caused by `scroll_to` or by
    /// the contents shrinking. It does not arrive when only the sizes change.
    pub fn on_scroll(&self, f: impl FnMut(ScrollMetrics) + 'static) {
        self.0.on_scroll.set(&self.read_metrics(), f);
    }

    /// Reads the current values without waiting for layout.
    ///
    /// When it extends under the window's title bar, for example, the part hidden by
    /// `contentInsets` is not counted as visible, and the start of the content is
    /// normalized to 0.
    fn read_metrics(&self) -> ScrollMetrics {
        let insets = self.0.native.contentInsets();
        let visible = self.0.native.contentView().bounds();
        let viewport_width = (visible.size.width - insets.left - insets.right).max(0.0);
        let viewport_height = (visible.size.height - insets.top - insets.bottom).max(0.0);
        let content = self
            .0
            .native
            .documentView()
            .map(|document| document.frame().size)
            .unwrap_or(visible.size);
        ScrollMetrics {
            x: visible.origin.x + insets.left,
            y: visible.origin.y + insets.top,
            viewport_width,
            viewport_height,
            content_width: content.width.max(viewport_width),
            content_height: content.height.max(viewport_height),
        }
    }

    fn request(&self, target: ScrollTarget) {
        self.0.pending.set(Some(target));
        self.0.native.layoutSubtreeIfNeeded();
        self.apply_pending();
    }

    /// Called from the notification that the size has been decided (a change to the
    /// clip view's frame).
    ///
    /// This notification arrives in the middle of layout, so the content's size may not
    /// be decided yet at that point, and forcing layout again is risky too. It is
    /// applied after layout has settled (on the next iteration of the main queue).
    fn schedule_pending(&self, mtm: MainThreadMarker) {
        if self.0.pending.get().is_none() || self.0.pending_scheduled.replace(true) {
            return;
        }
        let weak = MainThreadBound::new(Rc::downgrade(&self.0), mtm);
        DispatchQueue::main().exec_async(move || {
            // メインキューの仕事はメインスレッドで走る。
            let mtm = MainThreadMarker::new().expect("メインキューの外で呼ばれた");
            let Some(inner) = weak.into_inner(mtm).upgrade() else {
                return;
            };
            inner.pending_scheduled.set(false);
            let scroll = Scroll(inner);
            scroll.0.native.layoutSubtreeIfNeeded();
            scroll.apply_pending();
        });
    }

    /// Scrolls to the remembered destination. If the size is not decided, keeps
    /// remembering it.
    ///
    /// The caller finishes layout beforehand.
    fn apply_pending(&self) {
        let Some(target) = self.0.pending.get() else {
            return;
        };
        let metrics = self.read_metrics();
        if metrics.viewport_width <= 0.0 || metrics.viewport_height <= 0.0 {
            return;
        }
        self.0.pending.set(None);
        let (x, y) = target.resolve(&metrics);
        if (x, y) == (metrics.x, metrics.y) {
            return;
        }
        let insets = self.0.native.contentInsets();
        let clip = self.0.native.contentView();
        clip.scrollToPoint(NSPoint::new(x - insets.left, y - insets.top));
        self.0.native.reflectScrolledClipView(&clip);
    }

    fn notify_if_moved(&self) {
        self.0.on_scroll.notify_if_moved(|| self.read_metrics());
    }

    /// Whether scrolling is allowed horizontally and vertically. The default is
    /// horizontal `Never`, vertical `Auto`.
    pub fn set_policy(&self, horizontal: ScrollPolicy, vertical: ScrollPolicy) {
        self.0.horizontal.set(horizontal);
        self.0.vertical.set(vertical);
        self.apply_policy();
        // 制約は許可によって変わるので、子があれば張り直す。
        let child = self.0.child.borrow().as_ref().map(|c| c.native_view());
        if let Some(view) = child {
            self.attach_child_constraints(&view);
        }
    }

    fn apply_policy(&self) {
        let horizontal = self.0.horizontal.get();
        let vertical = self.0.vertical.get();
        self.0
            .native
            .setHasHorizontalScroller(horizontal.is_enabled());
        self.0.native.setHasVerticalScroller(vertical.is_enabled());
        self.0.native.setAutohidesScrollers(
            horizontal != ScrollPolicy::Always && vertical != ScrollPolicy::Always,
        );
    }

    /// The content to scroll. Each call replaces it.
    pub fn set_child(&self, child: &dyn Widget) {
        let view = child.native_view();
        prepare_child(&view);
        self.0.native.setDocumentView(Some(&view));
        self.attach_child_constraints(&view);
        *self.0.child.borrow_mut() = Some(child.boxed_clone());
    }

    fn attach_child_constraints(&self, view: &NSView) {
        let mut constraints = self.0.constraints.borrow_mut();
        if !constraints.is_empty() {
            NSLayoutConstraint::deactivateConstraints(&NSArray::from_retained_slice(&constraints));
            constraints.clear();
        }
        let clip = self.0.native.contentView();
        constraints.push(
            view.leadingAnchor()
                .constraintEqualToAnchor(&clip.leadingAnchor()),
        );
        constraints.push(view.topAnchor().constraintEqualToAnchor(&clip.topAnchor()));
        // スクロールしない方向は、クリップ領域と同じ大きさに固定する。
        // スクロールする方向は、中身の側の大きさに任せる。
        constraints.push(if self.0.horizontal.get().is_enabled() {
            view.widthAnchor()
                .constraintGreaterThanOrEqualToAnchor(&clip.widthAnchor())
        } else {
            view.widthAnchor()
                .constraintEqualToAnchor(&clip.widthAnchor())
        });
        constraints.push(if self.0.vertical.get().is_enabled() {
            view.heightAnchor()
                .constraintGreaterThanOrEqualToAnchor(&clip.heightAnchor())
        } else {
            view.heightAnchor()
                .constraintEqualToAnchor(&clip.heightAnchor())
        });
        NSLayoutConstraint::activateConstraints(&NSArray::from_retained_slice(&constraints));
    }
}
