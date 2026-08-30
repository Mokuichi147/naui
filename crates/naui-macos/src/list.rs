//! リスト (AppKit)。
//!
//! `NSTableView` を 1 列で使い、`NSScrollView` に載せている。macOS の
//! ソースリストやファイル一覧と同じコントロールで、行の描画・スクロール・
//! ⌘ / Shift を使った複数選択・キーボード操作はすべて AppKit が行う。
//!
//! naui が足しているのは「行の文字列を返す」「選べない行を教える」
//! 「選択が変わったら Rust のクロージャへ渡す」の 3 つだけで、
//! そのためのデータソース兼デリゲートを 1 クラス定義している。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use naui_core::{ListItem, SelectionMode};
use objc2::rc::Retained;
use objc2::runtime::{NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{define_class, msg_send, DefinedClass, MainThreadMarker, MainThreadOnly, Message};
use objc2_app_kit::{
    NSColor, NSControlTextEditingDelegate, NSFont, NSLayoutConstraint, NSScrollView,
    NSTableCellView, NSTableColumn, NSTableColumnResizingOptions, NSTableView,
    NSTableViewColumnAutoresizingStyle, NSTableViewDataSource, NSTableViewDelegate,
    NSTableViewStyle, NSTextField, NSView, NSViewNoIntrinsicMetric,
};
use objc2_foundation::{
    NSArray, NSIndexSet, NSInteger, NSMutableIndexSet, NSNotification, NSSize, NSString,
};

use crate::widgets::Widget;

/// 1 列しか使わないので、識別子は固定でよい。
const COLUMN_ID: &str = "naui.list.column";

/// 行の上下に取る余白。行の高さは AppKit が制約から求める。
pub(crate) const ROW_PADDING: f64 = 4.0;
/// ラベルと補助の文字の間隔。
const DETAIL_SPACING: f64 = 1.0;

/// リストへ載せる任意内容の行。
///
/// [`ListItem`] は文字列だけで済む一覧向けの簡便 API であり、設定画面のように
/// アイコン、複数のラベル、チェックボックス、末尾のボタンなどを組み合わせる
/// ときは、`Grid` / `Stack` で内容を作ってこの型に包む。
///
/// ```no_run
/// # use naui_macos::{ListRow, Widget};
/// # fn row(content: &dyn Widget) {
/// let row = ListRow::new(content).selectable(false);
/// # let _ = row;
/// # }
/// ```
pub struct ListRow {
    content: Box<dyn Widget>,
    selectable: bool,
}

impl Clone for ListRow {
    fn clone(&self) -> Self {
        Self {
            content: self.content.boxed_clone(),
            selectable: self.selectable,
        }
    }
}

impl ListRow {
    /// ウィジェットを 1 行の内容として使う。既定では行全体も選択できる。
    pub fn new(content: &dyn Widget) -> Self {
        Self {
            content: content.boxed_clone(),
            selectable: true,
        }
    }

    /// 行全体を選択対象にするかどうか。
    ///
    /// 行内のボタンやチェックボックスだけを操作する設定行では `false` にする。
    pub fn selectable(mut self, selectable: bool) -> Self {
        self.selectable = selectable;
        self
    }

    pub fn is_selectable(&self) -> bool {
        self.selectable
    }
}

enum Row {
    Text(ListItem),
    Custom(ListRow),
}

impl Row {
    fn selectable(&self) -> bool {
        match self {
            Row::Text(item) => item.enabled,
            Row::Custom(row) => row.selectable,
        }
    }
}

/// 選択が変わったことの通知先。
///
/// 単一選択でも複数選択でも同じ形にするため、選ばれている行を
/// 昇順の並びで渡す。複数選択では 0 件にもなる。
#[derive(Clone, Default)]
pub(crate) struct SelectionHandler(Rc<RefCell<Option<Box<dyn FnMut(&[usize])>>>>);

impl SelectionHandler {
    pub(crate) fn set(&self, f: impl FnMut(&[usize]) + 'static) {
        *self.0.borrow_mut() = Some(Box::new(f));
    }

    /// 選択を通知する。まだ設定されていなければ何もしない。
    pub(crate) fn emit(&self, indices: &[usize]) {
        // クロージャの中からリストを操作しても二重借用にならないよう、
        // 呼び出しの間だけ取り出す (`SelectHandler` と同じ形)。
        let Some(mut f) = self.0.borrow_mut().take() else {
            return;
        };
        f(indices);
        let mut slot = self.0.borrow_mut();
        if slot.is_none() {
            *slot = Some(f);
        }
    }
}

/// データソース兼デリゲートが見る状態。ハンドルと共有する。
struct SourceState {
    rows: Rc<RefCell<Vec<Row>>>,
    handler: SelectionHandler,
    /// プログラムから選択を変えている間だけ通知を止める。
    /// AppKit は `selectRowIndexes:` でもデリゲートを呼ぶため。
    silent: Rc<Cell<bool>>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "NauiListSource"]
    #[ivars = SourceState]
    struct ListSource;

    unsafe impl NSObjectProtocol for ListSource {}

    unsafe impl NSTableViewDataSource for ListSource {
        #[unsafe(method(numberOfRowsInTableView:))]
        fn number_of_rows(&self, _table_view: &NSTableView) -> NSInteger {
            self.ivars().rows.borrow().len() as NSInteger
        }
    }

    // NSTableViewDelegate は NSControlTextEditingDelegate を継承している。
    unsafe impl NSControlTextEditingDelegate for ListSource {}

    unsafe impl NSTableViewDelegate for ListSource {
        // Retained を返すので `method_id`。所有権の扱いは objc2 が面倒を見る。
        #[unsafe(method_id(tableView:viewForTableColumn:row:))]
        fn view_for_row(
            &self,
            _table_view: &NSTableView,
            _column: Option<&NSTableColumn>,
            row: NSInteger,
        ) -> Option<Retained<NSView>> {
            let mtm = MainThreadMarker::from(self);
            let rows = self.ivars().rows.borrow();
            match usize::try_from(row).ok().and_then(|row| rows.get(row)) {
                Some(Row::Text(item)) => Some(cell_view(
                    mtm,
                    &item.label,
                    item.detail.as_deref(),
                    item.enabled,
                )),
                Some(Row::Custom(row)) => Some(custom_cell_view(mtm, &*row.content)),
                None => None,
            }
        }

        #[unsafe(method(tableView:shouldSelectRow:))]
        fn should_select_row(&self, _table_view: &NSTableView, row: NSInteger) -> bool {
            usize::try_from(row)
                .ok()
                .and_then(|row| self.ivars().rows.borrow().get(row).map(Row::selectable))
                .unwrap_or(false)
        }

        #[unsafe(method(tableViewSelectionDidChange:))]
        fn selection_did_change(&self, notification: &NSNotification) {
            // 行数は `items` から見る。`set_items` が `items` を書き換えてから
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

/// 行 1 つ分のビューを作る。
///
/// 文字だけを載せた `NSTextField` をそのまま行にすると、AppKit は枠の上端に
/// 文字を描くため、行いっぱいに出る選択の帯と文字がずれる。AppKit 標準の
/// `NSTableCellView` に入れ、上下の余白まで**制約でつないで**おくと、
/// 高さを決めるのは Auto Layout (`usesAutomaticRowHeights`) になり、
/// 1 行でも 2 行でも帯と文字がそろう。
pub(crate) fn cell_view(
    mtm: MainThreadMarker,
    label: &str,
    detail: Option<&str>,
    enabled: bool,
) -> Retained<NSView> {
    let cell = NSTableCellView::new(mtm);
    let title = row_label(mtm, label, enabled, false);
    cell.addSubview(&title);
    // `textField` に入れておくと、行の背景色に合わせた文字色や
    // アクセシビリティの扱いを NSTableCellView が面倒を見る。
    unsafe { cell.setTextField(Some(&title)) };

    let mut constraints = vec![
        title
            .leadingAnchor()
            .constraintEqualToAnchor(&cell.leadingAnchor()),
        title
            .trailingAnchor()
            .constraintLessThanOrEqualToAnchor(&cell.trailingAnchor()),
        title
            .topAnchor()
            .constraintEqualToAnchor_constant(&cell.topAnchor(), ROW_PADDING),
    ];

    // 下端をどこにつなぐかで、行が 1 行になるか 2 行になるかが決まる。
    let bottom = match detail {
        None => title.bottomAnchor(),
        Some(detail) => {
            let sub = row_label(mtm, detail, enabled, true);
            cell.addSubview(&sub);
            constraints.push(
                sub.leadingAnchor()
                    .constraintEqualToAnchor(&cell.leadingAnchor()),
            );
            constraints.push(
                sub.trailingAnchor()
                    .constraintLessThanOrEqualToAnchor(&cell.trailingAnchor()),
            );
            constraints.push(
                sub.topAnchor()
                    .constraintEqualToAnchor_constant(&title.bottomAnchor(), DETAIL_SPACING),
            );
            sub.bottomAnchor()
        }
    };
    constraints.push(
        cell.bottomAnchor()
            .constraintEqualToAnchor_constant(&bottom, ROW_PADDING),
    );
    NSLayoutConstraint::activateConstraints(&NSArray::from_retained_slice(&constraints));

    let view: &NSView = cell.as_ref();
    view.retain()
}

/// 任意のウィジェットを、行の高さも含めて Auto Layout へつなぐ。
fn custom_cell_view(mtm: MainThreadMarker, content: &dyn Widget) -> Retained<NSView> {
    let cell = NSTableCellView::new(mtm);
    let content = content.native_view();
    content.setTranslatesAutoresizingMaskIntoConstraints(false);
    cell.addSubview(&content);
    NSLayoutConstraint::activateConstraints(&NSArray::from_retained_slice(&[
        content
            .leadingAnchor()
            .constraintEqualToAnchor(&cell.leadingAnchor()),
        content
            .trailingAnchor()
            .constraintEqualToAnchor(&cell.trailingAnchor()),
        content
            .topAnchor()
            .constraintEqualToAnchor_constant(&cell.topAnchor(), ROW_PADDING),
        cell.bottomAnchor()
            .constraintEqualToAnchor_constant(&content.bottomAnchor(), ROW_PADDING),
    ]));
    let view: &NSView = cell.as_ref();
    view.retain()
}

/// 行に載せる 1 本の文字。`secondary` なら小さく淡い見た目にする。
fn row_label(
    mtm: MainThreadMarker,
    text: &str,
    enabled: bool,
    secondary: bool,
) -> Retained<NSTextField> {
    let field = NSTextField::labelWithString(&NSString::from_str(text), mtm);
    if secondary {
        field.setFont(Some(&NSFont::systemFontOfSize(
            NSFont::smallSystemFontSize(),
        )));
    }
    let color = if !enabled {
        // 選べない行は、AppKit が無効なコントロールに使う色で描く。
        NSColor::disabledControlTextColor()
    } else if secondary {
        NSColor::secondaryLabelColor()
    } else {
        NSColor::labelColor()
    };
    field.setTextColor(Some(&color));
    field.setTranslatesAutoresizingMaskIntoConstraints(false);
    field
}

impl ListSource {
    fn new(mtm: MainThreadMarker, state: SourceState) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(state);
        unsafe { msg_send![super(this), init] }
    }
}

/// テーブルの選択を昇順の並びとして読み出す。
pub(crate) fn selected_indices(table: &NSTableView, len: usize) -> Vec<usize> {
    let indexes = table.selectedRowIndexes();
    (0..len).filter(|&i| indexes.containsIndex(i)).collect()
}

/// `NSScrollView` が持たない「全行を表示する自然な高さ」を補う。
///
/// `NSTableView` 自体は自動行高を足した intrinsic size を返すので、
/// それを外側のスクロールビューへ伝える。固定高さと `Fill` は
/// `apply_sizing` がこの値より優先するため、従来どおりスクロール可能である。
struct ContentSizedScrollState {
    last_height: Cell<f64>,
}

define_class!(
    #[unsafe(super(NSScrollView))]
    #[thread_kind = MainThreadOnly]
    #[name = "NauiContentSizedListScrollView"]
    #[ivars = ContentSizedScrollState]
    struct ContentSizedScrollView;

    unsafe impl NSObjectProtocol for ContentSizedScrollView {}

    impl ContentSizedScrollView {
        #[unsafe(method(intrinsicContentSize))]
        fn intrinsic_content_size(&self) -> NSSize {
            let height = self.document_height();
            self.ivars().last_height.set(height);
            NSSize::new(unsafe { NSViewNoIntrinsicMetric }, height)
        }

        /// 親から幅が配られると、折り返しを持つ行の高さは作成時から変わる。
        /// その変化だけを次の Auto Layout に戻す。
        #[unsafe(method(layout))]
        fn layout(&self) {
            let _: () = unsafe { msg_send![super(self), layout] };
            if let Some(document) = self.documentView() {
                document.layoutSubtreeIfNeeded();
            }
            let height = self.document_height();
            let previous = self.ivars().last_height.get();
            if !previous.is_finite() || (previous - height).abs() > 0.5 {
                self.ivars().last_height.set(height);
                self.invalidateIntrinsicContentSize();
            }
        }
    }
);

impl ContentSizedScrollView {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(ContentSizedScrollState {
            last_height: Cell::new(f64::NAN),
        });
        unsafe { msg_send![super(this), init] }
    }

    fn document_height(&self) -> f64 {
        self.documentView()
            .map(|document| document.intrinsicContentSize().height.max(0.0))
            .unwrap_or(0.0)
    }

    fn invalidate_document_size(&self) {
        self.ivars().last_height.set(f64::NAN);
        self.invalidateIntrinsicContentSize();
    }
}

struct ListInner {
    /// 外から見えるビュー。リストはこのスクロールビューごと 1 つのウィジェット。
    scroll: Retained<ContentSizedScrollView>,
    table: Retained<NSTableView>,
    rows: Rc<RefCell<Vec<Row>>>,
    mode: Cell<SelectionMode>,
    handler: SelectionHandler,
    silent: Rc<Cell<bool>>,
    /// デリゲートとデータソースは weak 参照なので保持する。
    _source: Retained<ListSource>,
}

/// 縦に並ぶ選択できる一覧 (NSTableView)。
///
/// 中身は `NSScrollView` に載った 1 列の `NSTableView` で、
/// [`Widget::native_view`] が返すのはそのスクロールビュー。
/// 高さを指定しない ([`naui_core::Length::Auto`]) ときは全行の高さに追従し、
/// 固定高さや `Fill` ではみ出した分をスクロールする。
#[derive(Clone)]
pub struct List(Rc<ListInner>);

impl Widget for List {
    fn native_view(&self) -> Retained<NSView> {
        let scroll: &NSScrollView = self.0.scroll.as_ref();
        let view: &NSView = scroll;
        view.retain()
    }
    fn boxed_clone(&self) -> Box<dyn Widget> {
        Box::new(self.clone())
    }
}

crate::widgets::impl_sizing!(List);

impl List {
    pub(crate) fn new(mtm: MainThreadMarker) -> Self {
        let table = NSTableView::new(mtm);
        table.setStyle(NSTableViewStyle::Inset);
        table.setHeaderView(None);
        table.setAllowsMultipleSelection(false);
        // 単一選択でも「何も選ばれていない」状態を持てるようにする。
        // `<select>` と WinUI の ListBox がどちらも未選択を持てるため。
        table.setAllowsEmptySelection(true);
        table.setColumnAutoresizingStyle(
            NSTableViewColumnAutoresizingStyle::UniformColumnAutoresizingStyle,
        );
        // 行の高さは中身の制約から AppKit に求めさせる。
        // 補助の文字がある行だけ高くなるのはこの指定による。
        table.setUsesAutomaticRowHeights(true);

        let column = NSTableColumn::initWithIdentifier(
            NSTableColumn::alloc(mtm),
            &NSString::from_str(COLUMN_ID),
        );
        column.setResizingMask(NSTableColumnResizingOptions::AutoresizingMask);
        table.addTableColumn(&column);

        let rows: Rc<RefCell<Vec<Row>>> = Rc::new(RefCell::new(Vec::new()));
        let handler = SelectionHandler::default();
        let silent = Rc::new(Cell::new(false));
        let source = ListSource::new(
            mtm,
            SourceState {
                rows: rows.clone(),
                handler: handler.clone(),
                silent: silent.clone(),
            },
        );
        unsafe {
            table.setDataSource(Some(ProtocolObject::from_ref(&*source)));
            table.setDelegate(Some(ProtocolObject::from_ref(&*source)));
        }

        let scroll = ContentSizedScrollView::new(mtm);
        scroll.setHasVerticalScroller(true);
        scroll.setDocumentView(Some(&table));

        Self(Rc::new(ListInner {
            scroll,
            table,
            rows,
            mode: Cell::new(SelectionMode::Single),
            handler,
            silent,
            _source: source,
        }))
    }

    /// 行を作り直す。インデックスの意味が変わるため、選択は外れる。
    pub fn set_items(&self, items: &[ListItem]) {
        *self.0.rows.borrow_mut() = items.iter().cloned().map(Row::Text).collect();
        self.reload_rows();
    }

    /// 任意のウィジェットで作った行へ置き換える。
    ///
    /// 行内の各コントロールは通常どおりそれぞれのコールバックを持てる。
    /// インデックスの意味が変わるため、選択は外れる。
    pub fn set_rows(&self, rows: &[ListRow]) {
        *self.0.rows.borrow_mut() = rows.iter().cloned().map(Row::Custom).collect();
        self.reload_rows();
    }

    fn reload_rows(&self) {
        self.without_notifying(|this| {
            this.0.table.reloadData();
            unsafe { this.0.table.deselectAll(None) };
            this.0.scroll.invalidate_document_size();
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
    /// ([`SelectionMode::normalize`])。
    pub fn set_selection(&self, indices: &[usize]) {
        let picked = {
            let rows = self.0.rows.borrow();
            self.0.mode.get().normalize_by(indices, |index| {
                rows.get(index).is_some_and(Row::selectable)
            })
        };
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
        let picked = {
            let rows = self.0.rows.borrow();
            self.0.mode.get().normalize_by(indices, |index| {
                rows.get(index).is_some_and(Row::selectable)
            })
        };
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

    /// 中身の `NSTableView`。バックエンド固有の脱出口として公開している。
    ///
    /// 列の追加や並べ替えなど、共通 API に無い設定はここから行う。
    pub fn native_table(&self) -> Retained<NSTableView> {
        self.0.table.clone()
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
