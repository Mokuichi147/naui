//! 大きさの指定と、レイアウト用のコンテナ (Grid / Scroll / Spacer)。
//!
//! 計算するのは AppKit の Auto Layout と NSGridView / NSScrollView で、
//! miui 側は制約とプロパティを設定するだけ。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use miui_core::{GridCell, Padding, ScrollPolicy, Sizing, Track};
use objc2::rc::{Allocated, Retained};
use objc2::runtime::NSObjectProtocol;
use objc2::{define_class, msg_send, MainThreadMarker, MainThreadOnly, Message};
use objc2_app_kit::{
    NSClipView, NSGridCell, NSGridCellPlacement, NSGridView, NSLayoutConstraint,
    NSLayoutConstraintOrientation, NSLayoutPriority, NSScrollView, NSView,
};
use objc2_foundation::{NSArray, NSRange, NSString};

use crate::widgets::{impl_widget, Widget};

/// miui が付けた制約であることの目印。
///
/// AppKit は内部で intrinsic content size 用の制約を同じビューに付ける
/// (`NSContentSizeLayoutConstraint`)。属性だけで選ぶとそれらまで外して
/// しまうため、自分で付けたものに識別子を入れて区別する。
const SIZING_ID: &str = "miui.sizing";

/// `Fill` のときの hugging priority。低いほど余りを受け取る。
const FILL_HUGGING: NSLayoutPriority = 1.0;
/// `Auto` (中身に合わせる) のときの hugging priority。
const HUG_CONTENT: NSLayoutPriority = 750.0;
/// `Fill` のときの compression resistance priority。
///
/// 画像や動画の intrinsic size が親の最小幅にならないようにする。
const FILL_COMPRESSION_RESISTANCE: NSLayoutPriority = 1.0;

/// 大きさの指定をビューへ反映する。呼ぶたびに以前の指定は外れる。
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

/// このビューがその方向へ広がりたがっているか (= `Length::Fill` を指定されたか)。
pub(crate) fn wants_fill(view: &NSView, horizontal: bool) -> bool {
    view.contentHuggingPriorityForOrientation(orientation(horizontal)) <= FILL_HUGGING
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

/// コンテナへ入れる子の下ごしらえ。Auto Layout に任せる。
pub(crate) fn prepare_child(view: &NSView) {
    view.setTranslatesAutoresizingMaskIntoConstraints(false);
}

// ----------------------------------------------------------------- Spacer

struct SpacerInner {
    native: Retained<NSView>,
}

/// 余白そのものになるウィジェット。
///
/// 中身を持たず、スタックの余った空間をすべて受け取る。
/// 縦スタックの途中に置けば、後ろの要素を下端へ寄せられる。
#[derive(Clone)]
pub struct Spacer(Rc<SpacerInner>);
impl_widget!(Spacer);

impl Spacer {
    pub(crate) fn new(mtm: MainThreadMarker) -> Self {
        let native = NSView::new(mtm);
        // 中身が無いので、縮むことにも広がることにも抵抗しない。
        for horizontal in [true, false] {
            native.setContentHuggingPriority_forOrientation(FILL_HUGGING, orientation(horizontal));
            native.setContentCompressionResistancePriority_forOrientation(
                FILL_HUGGING,
                orientation(horizontal),
            );
        }
        Self(Rc::new(SpacerInner { native }))
    }
}

// ------------------------------------------------------------------- Grid

struct GridInner {
    native: Retained<NSGridView>,
    children: RefCell<Vec<Box<dyn Widget>>>,
    padding: Cell<Padding>,
}

/// 行と列で位置を決めるコンテナ (NSGridView)。
#[derive(Clone)]
pub struct Grid(Rc<GridInner>);
impl_widget!(Grid);

impl Grid {
    pub(crate) fn new(mtm: MainThreadMarker) -> Self {
        let native = NSGridView::gridViewWithNumberOfColumns_rows(0, 0, mtm);
        Self(Rc::new(GridInner {
            native,
            children: RefCell::new(Vec::new()),
            padding: Cell::new(Padding::ZERO),
        }))
    }

    /// 列間・行間のすき間。
    pub fn set_spacing(&self, column: f64, row: f64) {
        self.0.native.setColumnSpacing(column);
        self.0.native.setRowSpacing(row);
    }

    /// 外周の余白。
    ///
    /// NSGridView に余白の指定は無いため、両端の行と列の padding として渡す。
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

    /// 指定した場所に子を置く。足りない行と列は自動で足される。
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
        target.setContentView(Some(&view));
        // 子が `Fill` を指定していたら、マスいっぱいに広げる。
        if wants_fill(&view, true) {
            target.setXPlacement(NSGridCellPlacement::Fill);
        }
        // 縦は中央ぞろえ。NSGridView の既定 (上ぞろえ) だと、同じ行に置いた
        // ラベルと入力欄のように高さの違うものが上端で揃ってしまう。
        target.setYPlacement(if wants_fill(&view, false) {
            NSGridCellPlacement::Fill
        } else {
            NSGridCellPlacement::Center
        });

        self.apply_padding();
        self.0.children.borrow_mut().push(child.boxed_clone());
    }

    /// 列の幅の決め方。
    pub fn set_column_track(&self, index: usize, track: Track) {
        self.ensure_size(index + 1, 0);
        let column = self.0.native.columnAtIndex(index as isize);
        match track {
            // NSGridViewSizeForContent は「中身に合わせる」を表す番兵値。
            Track::Auto => column.setWidth(unsafe { objc2_app_kit::NSGridViewSizeForContent }),
            Track::Fixed(value) => column.setWidth(value),
            Track::Fill(_) => {
                column.setWidth(unsafe { objc2_app_kit::NSGridViewSizeForContent });
                column.setXPlacement(NSGridCellPlacement::Fill);
            }
        }
    }

    /// 行の高さの決め方。
    pub fn set_row_track(&self, index: usize, track: Track) {
        self.ensure_size(0, index + 1);
        let row = self.0.native.rowAtIndex(index as isize);
        match track {
            Track::Auto => row.setHeight(unsafe { objc2_app_kit::NSGridViewSizeForContent }),
            Track::Fixed(value) => row.setHeight(value),
            Track::Fill(_) => {
                row.setHeight(unsafe { objc2_app_kit::NSGridViewSizeForContent });
                row.setYPlacement(NSGridCellPlacement::Fill);
            }
        }
    }

    /// いまある列数。
    pub fn columns(&self) -> usize {
        self.0.native.numberOfColumns().max(0) as usize
    }

    /// いまある行数。
    pub fn rows(&self) -> usize {
        self.0.native.numberOfRows().max(0) as usize
    }

    /// 置いた子の数。
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
        }
        while (self.0.native.numberOfRows() as usize) < rows {
            self.0.native.addRowWithViews(&empty);
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
    #[name = "MiuiFlippedClipView"]
    /// スクロール内容を上端から並べるための NSClipView。
    struct FlippedClipView;

    unsafe impl NSObjectProtocol for FlippedClipView {}

    impl FlippedClipView {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }
    }
);

impl FlippedClipView {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this: Allocated<Self> = Self::alloc(mtm);
        unsafe { msg_send![this, init] }
    }
}

struct ScrollInner {
    native: Retained<NSScrollView>,
    child: RefCell<Option<Box<dyn Widget>>>,
    constraints: RefCell<Vec<Retained<NSLayoutConstraint>>>,
    horizontal: Cell<ScrollPolicy>,
    vertical: Cell<ScrollPolicy>,
}

/// 中身がはみ出したらスクロールさせるコンテナ (NSScrollView)。
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
        }));
        this.apply_policy();
        this
    }

    /// 横 / 縦それぞれのスクロールの許可。既定は横 `Never`・縦 `Auto`。
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
        self.0.native.setHasHorizontalScroller(horizontal.is_enabled());
        self.0.native.setHasVerticalScroller(vertical.is_enabled());
        self.0.native.setAutohidesScrollers(
            horizontal != ScrollPolicy::Always && vertical != ScrollPolicy::Always,
        );
    }

    /// スクロールさせる中身。呼ぶたびに置き換わる。
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
            view.widthAnchor().constraintEqualToAnchor(&clip.widthAnchor())
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
