//! 大きさの指定と、レイアウト用のコンテナ (Grid / Scroll / Spacer)。
//!
//! 計算するのは AppKit の Auto Layout と NSGridView / NSScrollView で、
//! naui 側は制約とプロパティを設定するだけ。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use naui_core::{GridCell, Padding, ScrollPolicy, Sizing, Track};
use objc2::rc::{Allocated, Retained};
use objc2::runtime::NSObjectProtocol;
use objc2::{define_class, msg_send, DefinedClass, MainThreadMarker, MainThreadOnly, Message};
use objc2_app_kit::{
    NSClipView, NSGridCell, NSGridCellPlacement, NSGridView, NSLayoutConstraint,
    NSLayoutConstraintOrientation, NSLayoutPriority, NSScrollView, NSView,
};
use objc2_foundation::{NSArray, NSRange, NSString};

use crate::widgets::{impl_widget, Widget};

/// naui が付けた制約であることの目印。
///
/// AppKit は内部で intrinsic content size 用の制約を同じビューに付ける
/// (`NSContentSizeLayoutConstraint`)。属性だけで選ぶとそれらまで外して
/// しまうため、自分で付けたものに識別子を入れて区別する。
const SIZING_ID: &str = "naui.sizing";

/// `Fill` に上限を付けたときの「上限まで広がりたい」制約の優先度。
///
/// 他のどの制約よりも弱くしておき、空間があるときだけ上限まで伸ばす。
const PREFERRED_SIZE_PRIORITY: NSLayoutPriority = 1.0;

/// 交差軸の `Fill` で「親の幅 / 高さに合わせたい」を表す優先度。
///
/// 必須にすると、同じ軸に上限 ([`Sizing::max_width`] など) があるときに
/// 必須どうしがぶつかり、AppKit がどちらかを勝手に落とす。上限を勝たせたいので
/// 1 段だけ下げてある (「はみ出さない」ほうは必須のまま別に張る)。
pub(crate) const CROSS_FILL_PRIORITY: NSLayoutPriority = 999.0;

/// `Fill` のときの hugging priority。低いほど余りを受け取る。
const FILL_HUGGING: NSLayoutPriority = 1.0;
/// `Auto` (中身に合わせる) のときの hugging priority。
const HUG_CONTENT: NSLayoutPriority = 750.0;
/// `Grid` / `Stack` の `Auto` 子が、親の余りを受け取らないための優先度。
///
/// AppKit のコンテナが再レイアウト時に追加する制約より優先し、内容幅を
/// 保つ。明示的な `Fill` はこの値を使わず、hugging を 1 に下げる。
const AUTO_HUGGING: NSLayoutPriority = 999.0;
/// `Fill` 行・列の子に張る「グリッドいっぱいまで伸びたい」という希望の識別子。
const GRID_GROW_ID: &str = "naui.grid.grow";

/// その希望の優先度。
///
/// `NSGridView` が余りを `Auto` 行へ渡してしまうときの弱い好み
/// (`NSLayoutPriorityDefaultLow` = 250 相当) には勝ち、`Auto` の子が持つ
/// compression resistance ([`HUG_CONTENT`]) には**負ける**値にしてある。
/// これより強くすると `Fill` 行が余りを取りすぎ、`Auto` 行の中身が
/// 潰れてしまう (見出しが 30pt まで縮む)。
const GRID_GROW_PRIORITY: NSLayoutPriority = HUG_CONTENT - 1.0;

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

/// `Auto` の子が内容幅を保つようにする。
pub(crate) fn keep_auto_size(view: &NSView, horizontal: bool) {
    view.setContentHuggingPriority_forOrientation(AUTO_HUGGING, orientation(horizontal));
}

/// セルごとの希望の識別子。同じセルに置き直したときだけ張り替える。
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

/// `NSGridView` が Auto 行の高さを決めるために使う情報。
///
/// AppKit は `NSStackView` の `fittingSize` を計算できても、Grid セルの
/// 自然高としては使わず、同じ行のボタンなどに合わせて Stack を潰すことがある。
/// 行に載せた実ビューを保持し、Auto 行だけ必要高を再計算する。
type GridCells = Rc<RefCell<Vec<(Retained<NSView>, GridCell)>>>;

/// `Fill` の子に張った「横に伸びたい」希望と、その置き場所。
///
/// 伸びる量はほかの列が要る幅で決まるので、レイアウトのたびに定数を
/// 引き直す。そのために、張った制約を場所とともに持っておく。
type GridGrows = Rc<RefCell<Vec<(Retained<NSLayoutConstraint>, GridCell)>>>;

struct ContentSizedGridState {
    row_tracks: Rc<RefCell<Vec<Track>>>,
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
            let widths = self.update_grow_widths();
            if rows || widths {
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
        cells: GridCells,
        grows: GridGrows,
    ) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(ContentSizedGridState {
            row_tracks,
            cells,
            grows,
            updating: Cell::new(false),
        });
        unsafe { msg_send![super(this), init] }
    }

    /// Auto 行を、その行に掛かるセル内容の fitting height 以上にする。
    /// 複数行セルは必要高を span へ均等に配り、過大な行高を避ける。
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

    /// `Fill` の子に張った「横に伸びたい」希望を、そのセルの取り分に合わせる。
    ///
    /// 希望する幅は「グリッドの幅 − ほかの列が要る幅」。ほかの列の内容幅は
    /// レイアウトのたびに変わるので、制約の定数として引き直す。
    fn update_grow_widths(&self) -> bool {
        let grows = self.ivars().grows.borrow();
        if grows.is_empty() {
            return false;
        }
        let widths = self.column_content_widths();
        let mut changed = false;
        for (constraint, cell) in grows.iter() {
            let outside = self.width_outside_cell(cell, &widths);
            if (constraint.constant() + outside).abs() > 0.5 {
                constraint.setConstant(-outside);
                changed = true;
            }
        }
        changed
    }

    /// 列ごとに要る幅。
    ///
    /// 幅を決め打ちした列 (`Track::Fixed`) は中身に関わらずその幅を占めるので、
    /// 内容幅ではなく指定された幅を使う。それ以外は載っている中身の
    /// `fittingSize` から決め、複数列にまたがるセルは span へ均等に配る。
    fn column_content_widths(&self) -> Vec<f64> {
        let columns = self.numberOfColumns().max(0) as usize;
        let fixed: Vec<Option<f64>> = (0..columns)
            .map(|index| self.fixed_column_width(index))
            .collect();
        let mut widths: Vec<f64> = fixed.iter().map(|width| width.unwrap_or(0.0)).collect();
        for (view, cell) in self.ivars().cells.borrow().iter() {
            if view.isHidden() {
                continue;
            }
            let span = cell.column_span.max(1);
            let each = (view.fittingSize().width / span as f64).max(0.0);
            for index in cell.column..(cell.column + span).min(columns) {
                if fixed[index].is_some() {
                    continue;
                }
                widths[index] = f64::max(widths[index], each);
            }
        }
        widths
    }

    /// 幅を決め打ちした列なら、その幅。中身に合わせる列 (`Auto` / `Fill`) は `None`。
    fn fixed_column_width(&self, index: usize) -> Option<f64> {
        let width = self.columnAtIndex(index as isize).width();
        // `NSGridViewSizeForContent` は「中身に合わせる」を表す番兵値。
        let content = unsafe { objc2_app_kit::NSGridViewSizeForContent };
        (width != content && width.is_finite() && width > 0.0).then_some(width)
    }

    /// このセルが使えない幅 (ほかの列の内容と、外周・列間の余白)。
    fn width_outside_cell(&self, cell: &GridCell, widths: &[f64]) -> f64 {
        let columns = widths.len();
        let span = cell.column_span.max(1);
        let end = (cell.column + span).min(columns);
        let mut outside: f64 = widths
            .iter()
            .enumerate()
            .filter(|(index, _)| *index < cell.column || *index >= end)
            .map(|(_, width)| *width)
            .sum();
        // 列間のすき間。セルがまたぐぶんはセルの取り分なので数えない。
        outside += self.columnSpacing() * columns.saturating_sub(span) as f64;
        // 外周の余白 (apply_padding が両端の列へ入れる)。
        for index in 0..columns {
            let column = self.columnAtIndex(index as isize);
            outside += column.leadingPadding() + column.trailingPadding();
        }
        outside
    }
}

struct GridInner {
    native: Retained<NSGridView>,
    children: RefCell<Vec<Box<dyn Widget>>>,
    row_tracks: Rc<RefCell<Vec<Track>>>,
    cells: GridCells,
    /// 横に伸びたい希望。セルの取り分に合わせて定数を引き直すため保持する。
    grows: GridGrows,
    /// `native` と同じオブジェクト。Auto 行を再計算する subclass として保持する。
    content_sized: Retained<ContentSizedGridView>,
    padding: Cell<Padding>,
}

/// 行と列で位置を決めるコンテナ (NSGridView)。
#[derive(Clone)]
pub struct Grid(Rc<GridInner>);
impl_widget!(Grid);

impl Grid {
    pub(crate) fn new(mtm: MainThreadMarker) -> Self {
        let row_tracks = Rc::new(RefCell::new(Vec::new()));
        let cells = Rc::new(RefCell::new(Vec::new()));
        let grows: GridGrows = Rc::new(RefCell::new(Vec::new()));
        let content_sized =
            ContentSizedGridView::new(mtm, row_tracks.clone(), cells.clone(), grows.clone());
        let native = content_sized.clone().into_super();
        Self(Rc::new(GridInner {
            native,
            children: RefCell::new(Vec::new()),
            row_tracks,
            cells,
            grows,
            content_sized,
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
        // 列が `Fill` でも、子が `Auto` なら列の幅を継承して広げない。
        // `Inherited` のままだと、再レイアウト時に NSGridView の列配置を
        // 継承してラベルまで列いっぱいになることがある。
        let fill_width = wants_fill(&view, true);
        if !fill_width {
            // Leading だけでは、NSGridView が再レイアウト時に作る幅の
            // 制約に intrinsic size が負けることがある。Auto の意味を
            // hugging priority でも固定しておく。
            keep_auto_size(&view, true);
            // 列の `Fill` も残すと、AppKit の再レイアウトでセルの配置が
            // `Inherited` 相当に戻る環境がある。Fill の子はセル側で明示
            // しているので、列の既定を先頭寄せにして継承経路も断つ。
            self.0
                .native
                .columnAtIndex(column)
                .setXPlacement(NSGridCellPlacement::Leading);
        }
        target.setXPlacement(if fill_width {
            NSGridCellPlacement::Fill
        } else {
            NSGridCellPlacement::Leading
        });
        self.set_grow_hint(&view, &cell, true, fill_width);
        // 縦は中央ぞろえ。NSGridView の既定 (上ぞろえ) だと、同じ行に置いた
        // ラベルと入力欄のように高さの違うものが上端で揃ってしまう。
        let fill_height = wants_fill(&view, false);
        if !fill_height {
            // 横と同じ理由で、`Auto` の子は行の余りを受け取らない。
            // NSStackView のようにコンテナ自身の hugging priority が低い子は、
            // これが無いと `Fill` 行より先に余りを吸ってしまい、`Fill` 行が
            // 中身の高さ (タブなら見出しだけ) まで潰れる。
            keep_auto_size(&view, false);
        }
        self.set_grow_hint(&view, &cell, false, fill_height);
        target.setYPlacement(if fill_height {
            NSGridCellPlacement::Fill
        } else {
            NSGridCellPlacement::Center
        });
        self.apply_padding();
        self.0.cells.borrow_mut().push((view, cell));
        self.0.children.borrow_mut().push(child.boxed_clone());
        self.0.content_sized.update_auto_row_heights();
        self.0.content_sized.update_grow_widths();
    }

    /// `Fill` 行・列の子に「余りのぶんだけ伸びたい」という弱い希望を張る。
    ///
    /// `NSGridView` の行・列の大きさは中身から決まり、**余りをどこへ渡すかは
    /// 決まっていない**。hugging priority だけでは足りず、`Auto` 側へ余白が
    /// 入ることがある。
    ///
    /// そこで「グリッドと同じ大きさになりたい」を弱い優先度で足し、余りを
    /// `Fill` の行・列へ誘導する。ただし**横はそのままだと、複数列のときに
    /// 1 つのセルの子がグリッド全体の幅を要求してしまう**ので、ほかの列が
    /// 要る幅を定数として差し引き、そのセル (結合していれば結合後の領域) の
    /// 取り分だけを望むようにする。差し引く量は中身で変わるため、
    /// [`ContentSizedGridView::update_grow_widths`] がレイアウトのたびに
    /// 引き直す。
    ///
    /// 縦は `Auto` 行の高さを [`ContentSizedGridView::update_auto_row_heights`]
    /// が必須の行高として決めているので、グリッドの高さのままでよい。
    fn set_grow_hint(&self, view: &NSView, cell: &GridCell, horizontal: bool, wanted: bool) {
        let identifier = grow_identifier(cell, horizontal);
        if horizontal {
            // 同じセルの古い希望は、定数を引き直す対象から外す。
            self.0
                .grows
                .borrow_mut()
                .retain(|(_, old)| old.column != cell.column || old.row != cell.row);
        }
        // 同じセルに前へ張った希望があれば外す。
        let constraints = self.0.native.constraints();
        let previous: Vec<Retained<NSLayoutConstraint>> = (0..constraints.len())
            .map(|index| constraints.objectAtIndex(index))
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
        let grow = if horizontal {
            view.widthAnchor()
                .constraintEqualToAnchor(&self.0.native.widthAnchor())
        } else {
            view.heightAnchor()
                .constraintEqualToAnchor(&self.0.native.heightAnchor())
        };
        grow.setPriority(GRID_GROW_PRIORITY);
        grow.setIdentifier(Some(&NSString::from_str(&identifier)));
        NSLayoutConstraint::activateConstraints(&NSArray::from_retained_slice(&[grow.clone()]));
        if horizontal {
            self.0.grows.borrow_mut().push((grow, *cell));
        }
    }

    /// いまの子を外し、指定した 1 つだけを置く。
    pub fn replace(&self, child: &dyn Widget, cell: GridCell) {
        // NSGridCell::setContentView(None) だけでは、既存のビューが
        // NSGridView の subviews に残ることがある。先に親から明示的に外し、
        // 写真ペインが動画ペインの下に残らないようにする。
        let old_views: Vec<Retained<NSView>> = self
            .0
            .children
            .borrow()
            .iter()
            .map(|old| old.native_view())
            .collect();
        for old in &old_views {
            old.removeFromSuperview();
        }
        let target = self
            .0
            .native
            .cellAtColumnIndex_rowIndex(cell.column as isize, cell.row as isize);
        target.setContentView(None);
        self.0.children.borrow_mut().clear();
        self.0.cells.borrow_mut().clear();
        // 外したビューに掛かる制約は AppKit が落とすので、こちらの控えも捨てる。
        self.0.grows.borrow_mut().clear();
        self.attach(child, cell);
    }

    /// 列の幅の決め方。
    pub fn set_column_track(&self, index: usize, track: Track) {
        self.ensure_size(index + 1, 0);
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
    }

    /// 行の高さの決め方。
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
    /// スクロール内容を上端から並べるための NSClipView。
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
        self.0
            .native
            .setHasHorizontalScroller(horizontal.is_enabled());
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
