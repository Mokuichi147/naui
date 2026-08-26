//! テーブル (AppKit)。
//!
//! `NSTableView` を複数列で使い、`NSScrollView` に載せている。リスト
//! ([`crate::List`]) と同じコントロールだが、こちらは列ごとに
//! `NSTableColumn` を作り、`NSTableHeaderView` で見出しを出す。
//! 列幅の調整・行の描画・スクロール・⌘ / Shift を使った複数選択・
//! キーボード操作はすべて AppKit が行う。
//!
//! naui が足しているのは「セルの文字列を返す」「選べない行を教える」
//! 「選択が変わったら Rust のクロージャへ渡す」の 3 つだけで、
//! そのためのデータソース兼デリゲートを 1 クラス定義している。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use naui_core::{Align, SelectionMode, SortOrder, TableColumn, TableRow};
use objc2::rc::Retained;
use objc2::runtime::{NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{define_class, msg_send, DefinedClass, MainThreadMarker, MainThreadOnly, Message};
use objc2_app_kit::{
    NSColor, NSControlTextEditingDelegate, NSLayoutConstraint, NSLayoutConstraintOrientation,
    NSLayoutPriorityDefaultLow, NSScrollView, NSTableCellView, NSTableColumn,
    NSTableColumnResizingOptions, NSTableHeaderView, NSTableView,
    NSTableViewColumnAutoresizingStyle, NSTableViewDataSource, NSTableViewDelegate,
    NSTableViewGridLineStyle, NSTableViewStyle, NSTextAlignment, NSTextField, NSView,
};
use objc2_foundation::{
    NSArray, NSIndexSet, NSInteger, NSMutableIndexSet, NSNotification, NSSize, NSSortDescriptor,
    NSString,
};

use crate::list::{selected_indices, SelectionHandler};
use crate::widgets::Widget;

/// 列の識別子の頭。うしろに列のインデックスを付ける。
const COLUMN_ID_PREFIX: &str = "naui.table.column.";
/// セルの左右に取る余白。
const CELL_PADDING: f64 = 4.0;
/// 文字の上下に取る余白。行の高さは「ラベルの高さ + これ × 2」。
const ROW_SPACING: f64 = 5.0;
/// 幅を指定しない列に与える最小幅。これより狭くはできない。
const MIN_COLUMN_WIDTH: f64 = 40.0;

/// 識別子から列のインデックスを取り出す。
fn column_index(column: Option<&NSTableColumn>) -> Option<usize> {
    let column = column?;
    let identifier = column.identifier().to_string();
    identifier.strip_prefix(COLUMN_ID_PREFIX)?.parse().ok()
}

/// 並べ替えが変わったことの通知先。
///
/// 呼び出しの間だけクロージャを取り出すので、コールバックの中から
/// 同じ表を操作しても二重借用にならない ([`SelectionHandler`] と同じ形)。
#[derive(Clone, Default)]
struct SortHandler(Rc<RefCell<Option<Box<dyn FnMut(usize, SortOrder)>>>>);

impl SortHandler {
    fn set(&self, f: impl FnMut(usize, SortOrder) + 'static) {
        *self.0.borrow_mut() = Some(Box::new(f));
    }

    fn emit(&self, column: usize, order: SortOrder) {
        let Some(mut f) = self.0.borrow_mut().take() else {
            return;
        };
        f(column, order);
        let mut slot = self.0.borrow_mut();
        if slot.is_none() {
            *slot = Some(f);
        }
    }
}

/// テーブルが持つ並べ替えの指定を、列と向きとして読む。
fn sort_from(table: &NSTableView) -> Option<(usize, SortOrder)> {
    let descriptor = table.sortDescriptors().firstObject()?;
    let key = descriptor.key()?.to_string();
    let column = key.strip_prefix(COLUMN_ID_PREFIX)?.parse().ok()?;
    Some((
        column,
        match descriptor.ascending() {
            true => SortOrder::Ascending,
            false => SortOrder::Descending,
        },
    ))
}

/// 幅を指定していない列へ、余った幅を均等に配る。
///
/// AppKit の `columnAutoresizingStyle` は**幅を固定した列があると配りきれない**。
/// 比率で広げようとして、`minWidth == maxWidth` の列は動かせず、その分が
/// 余ったままになる (表の右側が空く)。表の幅は naui が決めているので、
/// 配り方もこちらで決める。
fn distribute_widths(table: &NSTableView, columns: &[TableColumn]) {
    let available = table.frame().size.width;
    if available <= 0.0 {
        return;
    }
    let flexible: Vec<usize> = columns
        .iter()
        .enumerate()
        .filter(|(_, column)| column.width.is_none())
        .map(|(index, _)| index)
        .collect();
    if flexible.is_empty() {
        return;
    }
    // AppKit は「列の幅の合計 + 列の間の余白」を表の幅として並べる。
    let fixed: f64 = columns.iter().filter_map(|column| column.width).sum();
    let spacing = table.intercellSpacing().width * columns.len() as f64;
    let each = ((available - fixed - spacing) / flexible.len() as f64).max(MIN_COLUMN_WIDTH);

    let native = table.tableColumns();
    for &index in &flexible {
        if index >= native.len() {
            continue;
        }
        let column = native.objectAtIndex(index);
        // 同じ幅を書き戻すと `setFrameSize:` が何度も呼ばれて回り続けるので、
        // 変わるときだけ書く。
        if (column.width() - each).abs() > 0.5 {
            column.setWidth(each);
        }
    }
}

/// 幅を配るために持っておく状態。ハンドルと共有する。
struct TableViewState {
    columns: Rc<RefCell<Vec<TableColumn>>>,
}

define_class!(
    #[unsafe(super(NSTableView))]
    #[thread_kind = MainThreadOnly]
    #[name = "NauiTableView"]
    #[ivars = TableViewState]
    /// 幅が変わるたびに、余りを列へ配り直す `NSTableView`。
    struct TableView;

    unsafe impl NSObjectProtocol for TableView {}

    impl TableView {
        #[unsafe(method(setFrameSize:))]
        fn set_frame_size(&self, size: NSSize) {
            let _: () = unsafe { msg_send![super(self), setFrameSize: size] };
            distribute_widths(self, &self.ivars().columns.borrow());
        }
    }
);

impl TableView {
    fn new(mtm: MainThreadMarker, state: TableViewState) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(state);
        unsafe { msg_send![super(this), init] }
    }
}

/// データソース兼デリゲートが見る状態。ハンドルと共有する。
struct SourceState {
    columns: Rc<RefCell<Vec<TableColumn>>>,
    rows: Rc<RefCell<Vec<TableRow>>>,
    handler: SelectionHandler,
    sort_handler: SortHandler,
    /// プログラムから選択を変えている間だけ通知を止める。
    /// AppKit は `selectRowIndexes:` でもデリゲートを呼ぶため。
    silent: Rc<Cell<bool>>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "NauiTableSource"]
    #[ivars = SourceState]
    struct TableSource;

    unsafe impl NSObjectProtocol for TableSource {}

    unsafe impl NSTableViewDataSource for TableSource {
        #[unsafe(method(numberOfRowsInTableView:))]
        fn number_of_rows(&self, _table_view: &NSTableView) -> NSInteger {
            self.ivars().rows.borrow().len() as NSInteger
        }

        // 並べ替えの通知はデリゲートではなくデータソースへ来る。
        #[unsafe(method(tableView:sortDescriptorsDidChange:))]
        fn sort_descriptors_did_change(
            &self,
            table_view: &NSTableView,
            _old: &NSArray<NSSortDescriptor>,
        ) {
            // 指標 (▲▼) の描き替えとヘッダーの押し込みは AppKit が行う。
            // naui は「どの列を、どちら向きに」だけをアプリへ渡す。
            let state = self.ivars();
            if state.silent.get() {
                return;
            }
            let Some((column, order)) = sort_from(table_view) else {
                return;
            };
            state.sort_handler.emit(column, order);
        }
    }

    // NSTableViewDelegate は NSControlTextEditingDelegate を継承している。
    unsafe impl NSControlTextEditingDelegate for TableSource {}

    unsafe impl NSTableViewDelegate for TableSource {
        // Retained を返すので `method_id`。所有権の扱いは objc2 が面倒を見る。
        #[unsafe(method_id(tableView:viewForTableColumn:row:))]
        fn view_for_row(
            &self,
            _table_view: &NSTableView,
            column: Option<&NSTableColumn>,
            row: NSInteger,
        ) -> Option<Retained<NSView>> {
            // `?` は define_class! の中では使えないので、組み立ては外の関数で行う。
            view_for_cell(self.ivars(), MainThreadMarker::from(self), column, row)
        }

        #[unsafe(method(tableView:shouldSelectRow:))]
        fn should_select_row(&self, _table_view: &NSTableView, row: NSInteger) -> bool {
            usize::try_from(row)
                .ok()
                .and_then(|row| self.ivars().rows.borrow().get(row).map(|r| r.enabled))
                .unwrap_or(false)
        }

        #[unsafe(method(tableViewSelectionDidChange:))]
        fn selection_did_change(&self, notification: &NSNotification) {
            // 行数は `rows` から見る。`set_rows` が `rows` を書き換えてから
            // `reloadData` を呼ぶまでの間はテーブルと食い違うが、その区間は
            // `without_notifying` で囲まれていてここへは来ない。
            let state = self.ivars();
            if state.silent.get() {
                return;
            }
            let Some(object) = notification.object() else {
                return;
            };
            let Ok(table) = object.downcast::<NSTableView>() else {
                return;
            };
            let indices = selected_indices(&table, state.rows.borrow().len());
            state.handler.emit(&indices);
        }
    }
);

/// 列と行からセルのビューを組み立てる。どちらかが無ければ `None`。
fn view_for_cell(
    state: &SourceState,
    mtm: MainThreadMarker,
    column: Option<&NSTableColumn>,
    row: NSInteger,
) -> Option<Retained<NSView>> {
    let index = column_index(column)?;
    let align = state
        .columns
        .borrow()
        .get(index)
        .map(|column| column.align)
        .unwrap_or(Align::Start);
    let rows = state.rows.borrow();
    let row = rows.get(usize::try_from(row).ok()?)?;
    Some(cell_view(mtm, row.cell(index), align, row.enabled))
}

impl TableSource {
    fn new(mtm: MainThreadMarker, state: SourceState) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(state);
        unsafe { msg_send![super(this), init] }
    }
}

/// 行の高さ。システムフォントで組んだラベルを 1 つ測って決める。
///
/// **自動 (`usesAutomaticRowHeights`) にはしない。** 列を足し引きすると、
/// AppKit が行ビューとセルの間へ張る制約が、前の列で行から外れたセルを
/// 指したまま有効化されて例外になる
/// (`-[NSTableRowData setColumnHidden:atColumnIndex:]` から
/// 「no common ancestor」)。表はどの行も同じ高さでよいので、
/// 高さはこちらで決めてしまう。
fn row_height(mtm: MainThreadMarker) -> f64 {
    let probe = NSTextField::labelWithString(&NSString::from_str("Ag"), mtm);
    probe.fittingSize().height + ROW_SPACING * 2.0
}

/// AppKit の文字揃えへ写す。`Fill` は文字に意味が無いので左と同じ扱い。
fn text_alignment(align: Align) -> NSTextAlignment {
    match align {
        Align::Center => NSTextAlignment::Center,
        Align::End => NSTextAlignment::Right,
        Align::Start | Align::Fill => NSTextAlignment::Left,
    }
}

/// セル 1 つ分のビューを作る。
///
/// リストの行 ([`crate::list::cell_view`]) と違い、左右の端をどちらも
/// **列の幅につないでいる**。文字揃えは `NSTextField` が自分の幅の中で
/// 決めるので、列いっぱいに広げないと右寄せも中央ぞろえも効かない。
/// 列より長い文字は、圧縮への抵抗を下げて AppKit に縮めさせる。
///
/// 縦は中央ぞろえだけにする。行の高さは [`ROW_HEIGHT`] で決まっていて、
/// 上下を端につなぐと高さの取り合いになるため。
fn cell_view(mtm: MainThreadMarker, text: &str, align: Align, enabled: bool) -> Retained<NSView> {
    let cell = NSTableCellView::new(mtm);
    let field = NSTextField::labelWithString(&NSString::from_str(text), mtm);
    field.setAlignment(text_alignment(align));
    let color = if enabled {
        NSColor::labelColor()
    } else {
        // 選べない行は、AppKit が無効なコントロールに使う色で描く。
        NSColor::disabledControlTextColor()
    };
    field.setTextColor(Some(&color));
    field.setTranslatesAutoresizingMaskIntoConstraints(false);
    // 列より長い文字列があっても、列幅につないだ制約のほうを通す。
    field.setContentCompressionResistancePriority_forOrientation(
        NSLayoutPriorityDefaultLow - 1.0,
        NSLayoutConstraintOrientation::Horizontal,
    );
    cell.addSubview(&field);
    // `textField` に入れておくと、行の背景色に合わせた文字色や
    // アクセシビリティの扱いを NSTableCellView が面倒を見る。
    unsafe { cell.setTextField(Some(&field)) };

    NSLayoutConstraint::activateConstraints(&NSArray::from_retained_slice(&[
        field
            .leadingAnchor()
            .constraintEqualToAnchor_constant(&cell.leadingAnchor(), CELL_PADDING),
        cell.trailingAnchor()
            .constraintEqualToAnchor_constant(&field.trailingAnchor(), CELL_PADDING),
        field
            .centerYAnchor()
            .constraintEqualToAnchor(&cell.centerYAnchor()),
    ]));

    let view: &NSView = cell.as_ref();
    view.retain()
}

struct TableInner {
    /// 外から見えるビュー。テーブルはこのスクロールビューごと 1 つのウィジェット。
    scroll: Retained<NSScrollView>,
    table: Retained<TableView>,
    columns: Rc<RefCell<Vec<TableColumn>>>,
    rows: Rc<RefCell<Vec<TableRow>>>,
    mode: Cell<SelectionMode>,
    handler: SelectionHandler,
    sort_handler: SortHandler,
    silent: Rc<Cell<bool>>,
    /// デリゲートとデータソースは weak 参照なので保持する。
    _source: Retained<TableSource>,
}

/// 列見出しを持つ表 (NSTableView)。
///
/// 中身は `NSScrollView` に載った `NSTableView` で、
/// [`Widget::native_view`] が返すのはそのスクロールビュー。
/// **スクロールビューは中身に合わせた高さを持たない**ため、
/// `set_sizing` で高さを指定すること (`List` と同じ)。
#[derive(Clone)]
pub struct Table(Rc<TableInner>);

impl Widget for Table {
    fn native_view(&self) -> Retained<NSView> {
        let view: &NSView = self.0.scroll.as_ref();
        view.retain()
    }
    fn boxed_clone(&self) -> Box<dyn Widget> {
        Box::new(self.clone())
    }
}

crate::widgets::impl_sizing!(Table);

impl Table {
    pub(crate) fn new(mtm: MainThreadMarker) -> Self {
        let columns: Rc<RefCell<Vec<TableColumn>>> = Rc::new(RefCell::new(Vec::new()));
        let table = TableView::new(
            mtm,
            TableViewState {
                columns: columns.clone(),
            },
        );
        // 見出しが端まで伸びる、表向きの見た目。
        table.setStyle(NSTableViewStyle::FullWidth);
        table.setHeaderView(Some(&NSTableHeaderView::new(mtm)));
        // 行の区切りだけを引く。縦線まで引くと、macOS の表としては強すぎる。
        table.setGridStyleMask(NSTableViewGridLineStyle::SolidHorizontalGridLineMask);
        table.setAllowsMultipleSelection(false);
        // 単一選択でも「何も選ばれていない」状態を持てるようにする。
        table.setAllowsEmptySelection(true);
        // 列の順番は `set_columns` に渡した並びと同じ意味を持つので、
        // ユーザーに入れ替えさせない。幅の調整は自由にできる。
        table.setAllowsColumnReordering(false);
        table.setAllowsColumnResizing(true);
        table.setColumnAutoresizingStyle(
            NSTableViewColumnAutoresizingStyle::UniformColumnAutoresizingStyle,
        );
        table.setRowHeight(row_height(mtm));

        let rows: Rc<RefCell<Vec<TableRow>>> = Rc::new(RefCell::new(Vec::new()));
        let handler = SelectionHandler::default();
        let sort_handler = SortHandler::default();
        let silent = Rc::new(Cell::new(false));
        let source = TableSource::new(
            mtm,
            SourceState {
                columns: columns.clone(),
                rows: rows.clone(),
                handler: handler.clone(),
                sort_handler: sort_handler.clone(),
                silent: silent.clone(),
            },
        );
        unsafe {
            table.setDataSource(Some(ProtocolObject::from_ref(&*source)));
            table.setDelegate(Some(ProtocolObject::from_ref(&*source)));
        }

        let scroll = NSScrollView::new(mtm);
        scroll.setHasVerticalScroller(true);
        scroll.setDocumentView(Some(&table));

        Self(Rc::new(TableInner {
            scroll,
            table,
            columns,
            rows,
            mode: Cell::new(SelectionMode::Single),
            handler,
            sort_handler,
            silent,
            _source: source,
        }))
    }

    /// 列を作り直す。行と選択はそのまま残り、セルの並べ直しだけが起きる。
    pub fn set_columns(&self, columns: &[TableColumn]) {
        // 列を入れ替えると AppKit は選択も並べ替えの指定も落とすので、
        // どちらも覚えて書き戻す。
        let picked = self.selection();
        let sorted = self.sort();
        *self.0.columns.borrow_mut() = columns.to_vec();
        // `tableColumns` はテーブルが持つ配列そのものなので、
        // たどりながら消すと AppKit に「列挙中の変更」として弾かれる。
        for column in self.0.table.tableColumns().to_vec() {
            self.0.table.removeTableColumn(&column);
        }
        let mtm = MainThreadMarker::from(&*self.0.table);
        for (index, spec) in columns.iter().enumerate() {
            let column = NSTableColumn::initWithIdentifier(
                NSTableColumn::alloc(mtm),
                &NSString::from_str(&format!("{COLUMN_ID_PREFIX}{index}")),
            );
            column.setTitle(&NSString::from_str(&spec.title));
            // 見出しを押したときの並べ替えは AppKit に任せる。
            // これを持つ列だけがヘッダーで押せるようになり、
            // 指標 (▲▼) の描き替えと向きの反転も AppKit が行う。
            if spec.sortable {
                let key = NSString::from_str(&format!("{COLUMN_ID_PREFIX}{index}"));
                let prototype = NSSortDescriptor::sortDescriptorWithKey_ascending(Some(&key), true);
                column.setSortDescriptorPrototype(Some(&prototype));
            }
            // 見出しの揃えは、その列のセルに合わせる。
            column.headerCell().setAlignment(text_alignment(spec.align));
            match spec.width {
                // 幅の指定がある列は、自動調整の対象から外して固定する。
                Some(width) => {
                    column.setWidth(width);
                    column.setMinWidth(width);
                    column.setMaxWidth(width);
                    column.setResizingMask(NSTableColumnResizingOptions::UserResizingMask);
                }
                // 指定が無い列だけで、余った幅を分け合う。
                None => {
                    column.setMinWidth(MIN_COLUMN_WIDTH);
                    column.setResizingMask(NSTableColumnResizingOptions::AutoresizingMask);
                }
            }
            self.0.table.addTableColumn(&column);
        }
        self.without_notifying(|this| {
            this.0.table.reloadData();
            this.apply_selection(&picked);
            // 列を作り直すと指定も落ちるので、並べ替えの指標を戻す。
            this.apply_sort(sorted);
        });
        // 大きさが決まっていれば、この場で余りを配る。まだなら
        // `setFrameSize:` が決まった時点で配る。
        distribute_widths(&self.0.table, &self.0.columns.borrow());
    }

    /// 列数。
    pub fn column_count(&self) -> usize {
        self.0.columns.borrow().len()
    }

    /// 行を作り直す。インデックスの意味が変わるため、選択は外れる。
    pub fn set_rows(&self, rows: &[TableRow]) {
        *self.0.rows.borrow_mut() = rows.to_vec();
        self.without_notifying(|this| {
            this.0.table.reloadData();
            unsafe { this.0.table.deselectAll(None) };
        });
    }

    /// 行数。
    pub fn len(&self) -> usize {
        self.0.rows.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// 選び方を変える。選択の意味が変わるため、選択は外れる。
    pub fn set_selection_mode(&self, mode: SelectionMode) {
        self.0.mode.set(mode);
        self.0.table.setAllowsMultipleSelection(mode.is_multiple());
        self.without_notifying(|this| unsafe { this.0.table.deselectAll(None) });
    }

    pub fn selection_mode(&self) -> SelectionMode {
        self.0.mode.get()
    }

    /// 選ばれている行のうち、いちばん上のもの。
    pub fn selected(&self) -> Option<usize> {
        self.selection().first().copied()
    }

    /// 選ばれている行 (昇順)。単一選択なら 0 件か 1 件。
    pub fn selection(&self) -> Vec<usize> {
        selected_indices(&self.0.table, self.len())
    }

    /// 通知せずに 1 行だけを選ぶ。
    pub fn set_selected(&self, index: usize) {
        self.set_selection(&[index]);
    }

    /// 通知せずに選択を置き換える。
    ///
    /// 範囲外・選べない行・重複は取り除かれ、単一選択なら先頭の 1 件だけが残る
    /// ([`SelectionMode::normalize_by`])。
    pub fn set_selection(&self, indices: &[usize]) {
        let picked = self.normalize(indices);
        self.without_notifying(|this| this.apply_selection(&picked));
    }

    /// 通知せずに選択をすべて外す。
    pub fn clear_selection(&self) {
        self.without_notifying(|this| unsafe { this.0.table.deselectAll(None) });
    }

    /// ユーザーが選んだのと同じ経路で 1 行を選ぶ (通知あり)。
    pub fn select(&self, index: usize) {
        self.select_many(&[index]);
    }

    /// ユーザーが選んだのと同じ経路で選択を置き換える (通知あり)。
    pub fn select_many(&self, indices: &[usize]) {
        let picked = self.normalize(indices);
        // AppKit は同じ選択を選び直すとデリゲートを呼ばない。
        // 通知の回数をそろえるため、ここで 1 回だけ出す。
        self.without_notifying(|this| this.apply_selection(&picked));
        let actual = self.selection();
        self.0.handler.emit(&actual);
    }

    /// 選択が変わったときに、選ばれている行 (昇順) で呼ばれる。
    ///
    /// 複数選択では 0 件で呼ばれることもある。
    pub fn on_select(&self, f: impl FnMut(&[usize]) + 'static) {
        self.0.handler.set(f);
    }

    /// 見出しが押されて並べ替えの指定が変わったときに、
    /// 押された列と向きで呼ばれる。
    ///
    /// [`TableColumn::sortable`] を立てた列だけが押せる。**行を並べ替えるのは
    /// アプリの仕事**で、通知を受けたら並べ替えた行を [`Table::set_rows`] で
    /// 渡し直す (`set_rows` は選択を外すので、必要なら選び直す)。
    pub fn on_sort(&self, f: impl FnMut(usize, SortOrder) + 'static) {
        self.0.sort_handler.set(f);
    }

    /// いまの並べ替えの指定 (列と向き)。押されたことが無ければ `None`。
    pub fn sort(&self) -> Option<(usize, SortOrder)> {
        sort_from(&self.0.table)
    }

    /// 通知せずに並べ替えの指定を置き換える。見出しの指標だけが変わる。
    ///
    /// 並べ替えられない列や範囲外の列を指すと、指定は外れる。
    pub fn set_sort(&self, sort: Option<(usize, SortOrder)>) {
        let sort = sort.filter(|&(column, _)| {
            self.0
                .columns
                .borrow()
                .get(column)
                .is_some_and(|spec| spec.sortable)
        });
        self.without_notifying(|this| this.apply_sort(sort));
    }

    /// 中身の `NSTableView`。バックエンド固有の脱出口として公開している。
    ///
    /// 並べ替えや列の見た目など、共通 API に無い設定はここから行う。
    pub fn native_table(&self) -> Retained<NSTableView> {
        let table: &NSTableView = &self.0.table;
        table.retain()
    }

    /// 指定された選択を、この表で意味を持つ形にそろえる。
    fn normalize(&self, indices: &[usize]) -> Vec<usize> {
        let rows = self.0.rows.borrow();
        self.0
            .mode
            .get()
            .normalize_by(indices, |i| rows.get(i).is_some_and(|row| row.enabled))
    }

    /// 並べ替えの指定をネイティブへ写す。
    fn apply_sort(&self, sort: Option<(usize, SortOrder)>) {
        let descriptors = match sort {
            None => NSArray::new(),
            Some((column, order)) => {
                let key = NSString::from_str(&format!("{COLUMN_ID_PREFIX}{column}"));
                let descriptor = NSSortDescriptor::sortDescriptorWithKey_ascending(
                    Some(&key),
                    order.is_ascending(),
                );
                NSArray::from_retained_slice(&[descriptor])
            }
        };
        self.0.table.setSortDescriptors(&descriptors);
    }

    fn apply_selection(&self, indices: &[usize]) {
        if indices.is_empty() {
            unsafe { self.0.table.deselectAll(None) };
            return;
        }
        let set = NSMutableIndexSet::new();
        for &index in indices {
            set.addIndex(index);
        }
        let set: &NSIndexSet = set.as_ref();
        self.0
            .table
            .selectRowIndexes_byExtendingSelection(set, false);
    }

    /// AppKit からの通知を止めたまま操作する。
    fn without_notifying(&self, f: impl FnOnce(&Self)) {
        let previous = self.0.silent.replace(true);
        f(self);
        self.0.silent.set(previous);
    }
}
