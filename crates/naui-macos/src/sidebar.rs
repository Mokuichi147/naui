//! サイドバー (AppKit)。
//!
//! 「システム設定」や Finder の左側と同じく、`NSSplitViewController` の
//! **サイドバー項目** (`NSSplitViewItem.sidebarWithViewController:`) を使う。
//! ウィンドウの高さいっぱいに伸び (`allowsFullHeightLayout`)、背景の材質
//! (macOS 26 では Liquid Glass、それより前はすりガラス) と開閉の動きは
//! AppKit が持つ。
//!
//! | naui | AppKit |
//! | --- | --- |
//! | サイドバー全体 | `NSSplitViewController` (ウィンドウの `contentViewController`) |
//! | 左の区画 | `NSSplitViewItem` (サイドバー) + `NSScrollView` |
//! | 項目の一覧 | `NSTableView` (`NSTableViewStyleSourceList`) |
//! | 項目 | `NSTableCellView` (SF Symbols の `NSImageView` + `NSTextField`) |
//! | まとまりの見出し | グループ行 (`tableView:isGroupRow:`) |
//! | 右の区画 | `NSSplitViewItem` (中身)。ウィンドウの子をここへ置く |
//!
//! 開閉は AppKit 標準のサイドバーボタン (`NSToolbarToggleSidebarItemIdentifier`)
//! で行う。ボタンはツールバーの項目なので、アプリがツールバーを付けていれば
//! その先頭へ差し込み、付けていなければボタンだけのツールバーをウィンドウへ
//! 付ける ([`Window::set_sidebar`](crate::Window::set_sidebar))。利用者が
//! 開閉したことは `NSSplitView` の
//! 大きさの変化 (`NSSplitViewDidResizeSubviewsNotification`) から拾う。
//!
//! 項目の一覧は `NSOutlineView` ではなく `NSTableView` にしてある。naui の
//! サイドバーは 2 段 (まとまりと項目) までで開閉を持たないので、開閉の
//! 三角や「隠す」ボタンが付く `NSOutlineView` のグループ項目は合わない。
//! まとまりの間の隙間は、選べない空の行で空ける。

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use naui_core::{
    sidebar_len, sidebar_rows, SidebarItem, SidebarRow, SidebarSection, DEFAULT_SIDEBAR_WIDTH,
    SIDEBAR_MIN_WIDTH,
};
use objc2::rc::Retained;
use objc2::runtime::{NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{define_class, msg_send, sel, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSColor, NSControlTextEditingDelegate, NSEventType, NSFont, NSImage,
    NSImageView, NSLayoutConstraint, NSScrollView, NSSplitViewController,
    NSSplitViewDidResizeSubviewsNotification, NSSplitViewItem, NSTableCellView, NSTableColumn,
    NSTableView, NSTableViewColumnAutoresizingStyle, NSTableViewDataSource, NSTableViewDelegate,
    NSTableViewStyle, NSTextField, NSView, NSViewController,
};
use objc2_foundation::{
    NSArray, NSIndexSet, NSInteger, NSNotification, NSNotificationCenter, NSString,
};

use crate::toolbar::Toolbar;
use crate::trampoline::{SelectHandler, ValueHandler};

/// 1 列しか使わないので、識別子は固定でよい。
const COLUMN_ID: &str = "naui.sidebar.column";

/// まとまりの間に空ける隙間の高さ。
///
/// 「システム設定」のサイドバーで、見出しの無いまとまりの間に空いている
/// くらいの幅にしてある。
const GAP_HEIGHT: f64 = 12.0;

/// アイコンと文字の間隔。
const ICON_SPACING: f64 = 6.0;

/// アイコンを置く枠の幅。
///
/// SF Symbols は記号ごとに幅が違う (はさみは広く、鉛筆は狭い) ので、
/// そのまま並べると文字の左端がそろわない。枠の幅を決めて中央へ置く。
const ICON_WIDTH: f64 = 20.0;

/// 画面に並ぶ 1 行。`SidebarRow` を持ち主のある形にしたもの。
#[derive(Clone)]
enum Row {
    Gap,
    Header(String),
    Item(usize, SidebarItem),
}

/// 画面に並ぶ行。
///
/// 見出しの前の隙間は省く。グループ行は項目と同じ高さで文字を上下中央に
/// 置くので、上側がそのまま隙間になる。隙間の行まで足すと、見出しの無い
/// まとまりの間より大きく空いてしまう。
fn rows_of(sections: &[SidebarSection]) -> Vec<Row> {
    let flat = sidebar_rows(sections);
    flat.iter()
        .enumerate()
        .filter(|(i, row)| {
            !(matches!(row, SidebarRow::Gap)
                && matches!(flat.get(i + 1), Some(SidebarRow::Header(_))))
        })
        .map(|(_, row)| match *row {
            SidebarRow::Gap => Row::Gap,
            SidebarRow::Header(title) => Row::Header(title.to_string()),
            SidebarRow::Item(index, item) => Row::Item(index, item.clone()),
        })
        .collect()
}

/// データソース兼デリゲートが見る状態。ハンドルと共有する。
struct SourceState {
    rows: Rc<RefCell<Vec<Row>>>,
    selected: Rc<Cell<Option<usize>>>,
    handler: SelectHandler,
    /// プログラムから選択を変えている間だけ通知を止める。
    /// AppKit は `selectRowIndexes:` でもデリゲートを呼ぶため。
    silent: Rc<Cell<bool>>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "NauiSidebarSource"]
    #[ivars = SourceState]
    struct SidebarSource;

    unsafe impl NSObjectProtocol for SidebarSource {}

    unsafe impl NSTableViewDataSource for SidebarSource {
        #[unsafe(method(numberOfRowsInTableView:))]
        fn number_of_rows(&self, _table_view: &NSTableView) -> NSInteger {
            self.ivars().rows.borrow().len() as NSInteger
        }
    }

    // NSTableViewDelegate は NSControlTextEditingDelegate を継承している。
    unsafe impl NSControlTextEditingDelegate for SidebarSource {}

    unsafe impl NSTableViewDelegate for SidebarSource {
        #[unsafe(method_id(tableView:viewForTableColumn:row:))]
        fn view_for_row(
            &self,
            _table_view: &NSTableView,
            _column: Option<&NSTableColumn>,
            row: NSInteger,
        ) -> Option<Retained<NSView>> {
            let mtm = MainThreadMarker::from(self);
            self.row(row).map(|row| match row {
                Row::Gap => NSView::new(mtm),
                Row::Header(title) => header_view(mtm, &title),
                Row::Item(_, item) => item_view(mtm, &item),
            })
        }

        #[unsafe(method(tableView:isGroupRow:))]
        fn is_group_row(&self, _table_view: &NSTableView, row: NSInteger) -> bool {
            matches!(self.row(row), Some(Row::Header(_)))
        }

        #[unsafe(method(tableView:heightOfRow:))]
        fn height_of_row(&self, table_view: &NSTableView, row: NSInteger) -> f64 {
            match self.row(row) {
                Some(Row::Gap) => GAP_HEIGHT,
                // 項目と見出しは AppKit が決める高さのまま。利用者が
                // 「サイドバーのアイコンのサイズ」を変えると、それに従う。
                _ => table_view.rowHeight(),
            }
        }

        #[unsafe(method(tableView:shouldSelectRow:))]
        fn should_select_row(&self, _table_view: &NSTableView, row: NSInteger) -> bool {
            matches!(self.row(row), Some(Row::Item(_, item)) if item.enabled)
        }

        #[unsafe(method(tableViewSelectionDidChange:))]
        fn selection_did_change(&self, notification: &NSNotification) {
            let state = self.ivars();
            let Some(object) = notification.object() else {
                return;
            };
            let Ok(table) = object.downcast::<NSTableView>() else {
                return;
            };
            let index = self.row(table.selectedRow()).and_then(|row| match row {
                Row::Item(index, _) => Some(index),
                _ => None,
            });
            state.selected.set(index);
            if state.silent.get() {
                return;
            }
            if let Some(index) = index {
                state.handler.emit(index);
            }
        }
    }
);

impl SidebarSource {
    fn new(mtm: MainThreadMarker, state: SourceState) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(state);
        unsafe { msg_send![super(this), init] }
    }

    fn row(&self, row: NSInteger) -> Option<Row> {
        let rows = self.ivars().rows.borrow();
        usize::try_from(row)
            .ok()
            .and_then(|row| rows.get(row).cloned())
    }
}

define_class!(
    /// `NSSplitView` の大きさの変化を受け取り、開閉が変わったかを確かめる。
    ///
    /// [`ActionTarget`](crate::trampoline::ActionTarget) は呼び出しの間
    /// クロージャを借りたままにするので、通知の中から `set_collapsed` を呼ぶと
    /// 同じ通知が入れ子で届いて二重借用になる。こちらは弱参照をたどるだけに
    /// して、入れ子で呼ばれても借用が残らないようにしてある。
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "NauiSidebarResizeObserver"]
    #[ivars = Weak<SidebarInner>]
    struct ResizeObserver;

    unsafe impl NSObjectProtocol for ResizeObserver {}

    impl ResizeObserver {
        #[unsafe(method(splitViewDidResize:))]
        fn split_view_did_resize(&self, _notification: &NSNotification) {
            let Some(inner) = self.ivars().upgrade() else {
                return;
            };
            let sidebar = Sidebar(inner);
            sidebar.sync_collapsed();
            // 利用者が仕切りをつかんでいる間、`NSSplitView` はマウスの
            // ドラッグのイベントを回しながら区画を並べ直す。開閉のアニメー
            // ションやウィンドウへの取り付けでは、いまのイベントはドラッグでは
            // ない (仕切りの番号付きの通知はどちらでも来るので、目印にならない)。
            let mtm = MainThreadMarker::from(self);
            let dragging = NSApplication::sharedApplication(mtm)
                .currentEvent()
                .is_some_and(|event| event.r#type() == NSEventType::LeftMouseDragged);
            if dragging {
                sidebar.sync_width();
            }
        }
    }
);

impl ResizeObserver {
    fn new(mtm: MainThreadMarker, inner: Weak<SidebarInner>) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(inner);
        unsafe { msg_send![super(this), init] }
    }
}

/// まとまりの見出し。
///
/// グループ行の見た目 (小さく淡い文字) は、Finder のサイドバーの見出しに
/// 合わせて AppKit の標準のフォントと色で付ける。
fn header_view(mtm: MainThreadMarker, title: &str) -> Retained<NSView> {
    let field = NSTextField::labelWithString(&NSString::from_str(title), mtm);
    field.setFont(Some(&NSFont::boldSystemFontOfSize(
        NSFont::smallSystemFontSize(),
    )));
    field.setTextColor(Some(&NSColor::secondaryLabelColor()));
    let cell = NSTableCellView::new(mtm);
    unsafe { cell.setTextField(Some(&field)) };
    pin_row_content(&cell, &field, None);
    Retained::into_super(cell)
}

/// 項目 1 行。アイコンがあれば文字の前に置く。
///
/// `NSTableCellView` の `imageView` / `textField` に入れておくと、選択中の
/// 行で文字と記号を白抜きにする・記号へアクセントカラーを付ける、といった
/// ソースリストの見た目を AppKit が付ける。
fn item_view(mtm: MainThreadMarker, item: &SidebarItem) -> Retained<NSView> {
    let field = NSTextField::labelWithString(&NSString::from_str(&item.label), mtm);
    field.setLineBreakMode(objc2_app_kit::NSLineBreakMode::ByTruncatingTail);
    if !item.enabled {
        field.setTextColor(Some(&NSColor::disabledControlTextColor()));
    }
    let cell = NSTableCellView::new(mtm);
    unsafe { cell.setTextField(Some(&field)) };

    let image = item.icon.and_then(|icon| {
        let symbol = NSString::from_str(icon.sf_symbol());
        let label = NSString::from_str(&item.label);
        NSImage::imageWithSystemSymbolName_accessibilityDescription(&symbol, Some(&label))
    });
    let image_view = image.map(|image| {
        let view = NSImageView::imageViewWithImage(&image, mtm);
        if !item.enabled {
            view.setEnabled(false);
        }
        unsafe { cell.setImageView(Some(&view)) };
        view
    });
    pin_row_content(&cell, &field, image_view.as_deref());
    Retained::into_super(cell)
}

/// セルの中身を、左右いっぱい・上下中央に制約でつなぐ。
fn pin_row_content(cell: &NSTableCellView, field: &NSTextField, image: Option<&NSImageView>) {
    let view: &NSView = cell;
    field.setTranslatesAutoresizingMaskIntoConstraints(false);
    view.addSubview(field);
    let mut constraints = vec![
        field
            .trailingAnchor()
            .constraintLessThanOrEqualToAnchor(&view.trailingAnchor()),
        field
            .centerYAnchor()
            .constraintEqualToAnchor(&view.centerYAnchor()),
    ];
    match image {
        Some(image) => {
            image.setTranslatesAutoresizingMaskIntoConstraints(false);
            view.addSubview(image);
            constraints.extend([
                image
                    .leadingAnchor()
                    .constraintEqualToAnchor(&view.leadingAnchor()),
                image
                    .centerYAnchor()
                    .constraintEqualToAnchor(&view.centerYAnchor()),
                image.widthAnchor().constraintEqualToConstant(ICON_WIDTH),
                field
                    .leadingAnchor()
                    .constraintEqualToAnchor_constant(&image.trailingAnchor(), ICON_SPACING),
            ]);
        }
        None => constraints.push(
            field
                .leadingAnchor()
                .constraintEqualToAnchor(&view.leadingAnchor()),
        ),
    }
    NSLayoutConstraint::activateConstraints(&NSArray::from_retained_slice(&constraints));
}

struct SidebarInner {
    controller: Retained<NSSplitViewController>,
    sidebar_item: Retained<NSSplitViewItem>,
    /// 右の区画のビュー。ウィンドウの子はここへ置く。
    content_host: Retained<NSView>,
    /// いま右の区画に置いている子のビュー。
    content: RefCell<Option<Retained<NSView>>>,
    table: Retained<NSTableView>,
    /// `NSTableView` の dataSource / delegate は弱参照なので持っておく。
    _source: Retained<SidebarSource>,
    sections: RefCell<Vec<SidebarSection>>,
    rows: Rc<RefCell<Vec<Row>>>,
    selected: Rc<Cell<Option<usize>>>,
    handler: SelectHandler,
    silent: Rc<Cell<bool>>,
    /// サイドバーの幅。`set_width` の値か、利用者が仕切りで変えた幅。
    width: Cell<f64>,
    on_resize: ValueHandler<f64>,
    /// naui が仕切りを置いている間、またはまだ幅を置いていない間は真。
    /// この間の大きさの変化は利用者の操作ではない。
    applying: Cell<bool>,
    /// 最後に知っている開閉。利用者の開閉だけを通知するために比べる。
    collapsed: Cell<bool>,
    on_collapse: ValueHandler<bool>,
    /// アプリがツールバーを付けていないときに使う、サイドバーボタンだけの
    /// ツールバー。
    controls: Toolbar,
    /// 大きさの変化の受け口。通知センターは observer を強く持たない。
    observer: RefCell<Option<Retained<ResizeObserver>>>,
}

impl Drop for SidebarInner {
    fn drop(&mut self) {
        if let Some(observer) = self.observer.borrow_mut().take() {
            unsafe { NSNotificationCenter::defaultCenter().removeObserver(&observer) };
        }
    }
}

/// ウィンドウの左に付けるサイドバー。
///
/// [`Window::set_sidebar`](crate::Window::set_sidebar) で取り付ける。
/// レイアウトには置かないので [`Widget`](crate::Widget) ではない。
#[derive(Clone)]
pub struct Sidebar(Rc<SidebarInner>);

impl Sidebar {
    pub(crate) fn new(mtm: MainThreadMarker) -> Self {
        let table = NSTableView::new(mtm);
        table.setStyle(NSTableViewStyle::SourceList);
        table.setHeaderView(None);
        table.setFloatsGroupRows(false);
        table.setAllowsEmptySelection(true);
        table.setColumnAutoresizingStyle(
            NSTableViewColumnAutoresizingStyle::FirstColumnOnlyAutoresizingStyle,
        );
        let column = NSTableColumn::initWithIdentifier(
            NSTableColumn::alloc(mtm),
            &NSString::from_str(COLUMN_ID),
        );
        table.addTableColumn(&column);

        let rows = Rc::new(RefCell::new(Vec::new()));
        let selected = Rc::new(Cell::new(None));
        let handler = SelectHandler::default();
        let silent = Rc::new(Cell::new(false));
        let source = SidebarSource::new(
            mtm,
            SourceState {
                rows: rows.clone(),
                selected: selected.clone(),
                handler: handler.clone(),
                silent: silent.clone(),
            },
        );
        unsafe {
            table.setDataSource(Some(ProtocolObject::from_ref(&*source)));
            table.setDelegate(Some(ProtocolObject::from_ref(&*source)));
        }

        let scroll = NSScrollView::new(mtm);
        scroll.setDocumentView(Some(&table));
        scroll.setHasVerticalScroller(true);
        scroll.setAutohidesScrollers(true);
        // 背景はサイドバーの材質に任せる。
        scroll.setDrawsBackground(false);

        let sidebar_controller = view_controller(mtm, &scroll);
        let sidebar_item = NSSplitViewItem::sidebarWithViewController(&sidebar_controller);
        sidebar_item.setAllowsFullHeightLayout(true);
        sidebar_item.setCanCollapse(true);
        // 仕切りで狭められる下限。これより左へ引くと、AppKit の作法どおり
        // サイドバーごと閉じる (`on_collapse` が呼ばれる)。
        sidebar_item.setMinimumThickness(SIDEBAR_MIN_WIDTH);

        let content_host = NSView::new(mtm);
        let content_controller = view_controller(mtm, &content_host);
        let content_item = NSSplitViewItem::splitViewItemWithViewController(&content_controller);

        let controller = NSSplitViewController::new(mtm);
        controller.addSplitViewItem(&sidebar_item);
        controller.addSplitViewItem(&content_item);

        let controls = Toolbar::new(mtm);
        controls.set_sidebar_controls(true);

        let this = Self(Rc::new(SidebarInner {
            controller,
            sidebar_item,
            content_host,
            content: RefCell::new(None),
            table,
            _source: source,
            sections: RefCell::new(Vec::new()),
            rows,
            selected,
            handler,
            silent,
            width: Cell::new(DEFAULT_SIDEBAR_WIDTH),
            collapsed: Cell::new(false),
            on_collapse: ValueHandler::default(),
            on_resize: ValueHandler::default(),
            applying: Cell::new(true),
            controls,
            observer: RefCell::new(None),
        }));
        this.apply_width();

        let observer = ResizeObserver::new(mtm, Rc::downgrade(&this.0));
        let split = this.0.controller.splitView();
        unsafe {
            NSNotificationCenter::defaultCenter().addObserver_selector_name_object(
                &observer,
                sel!(splitViewDidResize:),
                Some(NSSplitViewDidResizeSubviewsNotification),
                Some(&split),
            );
        }
        *this.0.observer.borrow_mut() = Some(observer);
        this
    }

    /// 項目をまとまりごとに並べる。呼ぶたびに置き換わる。
    ///
    /// 選ばれていた項目は、同じ通し番号がまだ選べるなら選ばれたまま残る
    /// (通知はしない)。
    pub fn set_sections(&self, sections: &[SidebarSection]) {
        *self.0.sections.borrow_mut() = sections.to_vec();
        *self.0.rows.borrow_mut() = rows_of(sections);
        let keep = self
            .0
            .selected
            .get()
            .filter(|&index| self.is_selectable(index));
        self.without_notifying(|| {
            self.0.table.reloadData();
            self.0.selected.set(None);
            if let Some(index) = keep {
                self.select_row_of(index);
            }
        });
    }

    /// 見出しの無いまとまり 1 つだけで並べる。
    pub fn set_items(&self, items: &[SidebarItem]) {
        self.set_sections(&[SidebarSection::untitled(items.iter().cloned())]);
    }

    /// まとまりをまたいだ項目の数。
    pub fn len(&self) -> usize {
        sidebar_len(&self.0.sections.borrow())
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// 選ばれている項目の通し番号。
    pub fn selected(&self) -> Option<usize> {
        self.0.selected.get()
    }

    /// 通知せずに選択を変える。範囲外・選べない項目は無視する。
    pub fn set_selected(&self, index: usize) {
        if !self.is_selectable(index) {
            return;
        }
        self.without_notifying(|| self.select_row_of(index));
    }

    /// 選択を外す (通知しない)。
    pub fn clear_selection(&self) {
        self.without_notifying(|| unsafe { self.0.table.deselectAll(None) });
    }

    /// 利用者が選んだのと同じように選び、通知する。
    ///
    /// 範囲外・選べない項目は無視する。すでに選ばれている項目でも通知する
    /// (ほかのナビゲーションの `select` と同じ)。
    pub fn select(&self, index: usize) {
        if !self.is_selectable(index) {
            return;
        }
        let already = self.0.selected.get() == Some(index);
        self.select_row_of(index);
        if already {
            // AppKit は選択が変わらないとデリゲートを呼ばない。
            self.0.handler.emit(index);
        }
    }

    /// 項目が選ばれたときの通知先。引数は通し番号。
    pub fn on_select(&self, f: impl FnMut(usize) + 'static) {
        self.0.handler.set(f);
    }

    /// サイドバーの幅 (論理ピクセル)。既定は [`DEFAULT_SIDEBAR_WIDTH`]。
    ///
    /// 利用者は仕切りをドラッグして幅を変えられる (下限は
    /// [`SIDEBAR_MIN_WIDTH`](naui_core::SIDEBAR_MIN_WIDTH))。変えたあとの幅は
    /// [`width`](Self::width) が返し、[`on_resize`](Self::on_resize) で届く。
    /// この呼び出しでは `on_resize` を呼ばない。
    pub fn set_width(&self, width: f64) {
        if !width.is_finite() || width <= 0.0 {
            return;
        }
        self.0.width.set(width.max(SIDEBAR_MIN_WIDTH));
        self.apply_width();
    }

    /// サイドバーの幅。閉じていても開いたときの幅を返す。
    pub fn width(&self) -> f64 {
        self.0.width.get()
    }

    /// 利用者が仕切りで幅を変えるたび、変えた後の幅で呼ばれる。
    pub fn on_resize(&self, f: impl FnMut(f64) + 'static) {
        self.0.on_resize.set(f);
    }

    /// 区画の幅を読み、変わっていれば覚え直して通知する。
    fn sync_width(&self) {
        if self.0.applying.get() || self.is_collapsed() {
            return;
        }
        let split = self.0.controller.splitView();
        let Some(pane) = split.arrangedSubviews().firstObject() else {
            return;
        };
        let width = pane.frame().size.width;
        if width > 0.0 && (width - self.0.width.replace(width)).abs() >= 0.5 {
            self.0.on_resize.emit(width);
        }
    }

    /// サイドバーを閉じる (`true`) か開く (`false`)。
    ///
    /// 閉じても項目と選択は残る。[`on_collapse`](Self::on_collapse) は
    /// 呼ばない (利用者がサイドバーボタンで開閉したときだけ呼ぶ)。
    pub fn set_collapsed(&self, collapsed: bool) {
        // 先に覚えておくと、このあと届く大きさの変化を通知しないで済む。
        self.0.collapsed.set(collapsed);
        self.0.sidebar_item.setCollapsed(collapsed);
    }

    /// 利用者がサイドバーを開閉したときの通知先。引数は閉じたかどうか。
    ///
    /// サイドバーボタンの操作で呼ばれ、
    /// [`set_collapsed`](Self::set_collapsed) では呼ばれない。
    pub fn on_collapse(&self, f: impl FnMut(bool) + 'static) {
        self.0.on_collapse.set(f);
    }

    /// 開閉が変わっていれば覚え直して通知する。
    fn sync_collapsed(&self) {
        let now = self.is_collapsed();
        if self.0.collapsed.replace(now) != now {
            self.0.on_collapse.emit(now);
        }
    }

    /// アプリがツールバーを付けていないときに使う、サイドバーボタンだけの
    /// ツールバー。
    pub(crate) fn controls_toolbar(&self) -> Toolbar {
        self.0.controls.clone()
    }

    /// サイドバーが閉じているかどうか。
    pub fn is_collapsed(&self) -> bool {
        self.0.sidebar_item.isCollapsed()
    }

    /// AppKit の実体 (`NSSplitViewController`)。バックエンド固有の脱出口。
    pub fn native_split_view_controller(&self) -> Retained<NSSplitViewController> {
        self.0.controller.clone()
    }

    /// 項目の一覧 (`NSTableView`)。バックエンド固有の脱出口。
    pub fn native_table_view(&self) -> Retained<NSTableView> {
        self.0.table.clone()
    }

    /// 右の区画へウィンドウの子を置く。`None` なら空にする。
    ///
    /// ウィンドウの `fullSizeContentView` でタイトルバーの下まで伸びるので、
    /// 子の上端はタイトルバー (とツールバー) を避けた安全領域へつなぐ。
    pub(crate) fn set_content(&self, view: Option<Retained<NSView>>) {
        if let Some(old) = self.0.content.borrow_mut().take() {
            old.removeFromSuperview();
        }
        let Some(view) = view else {
            return;
        };
        let host = &self.0.content_host;
        view.setTranslatesAutoresizingMaskIntoConstraints(false);
        host.addSubview(&view);
        let safe = host.safeAreaLayoutGuide();
        NSLayoutConstraint::activateConstraints(&NSArray::from_retained_slice(&[
            view.leadingAnchor()
                .constraintEqualToAnchor(&host.leadingAnchor()),
            view.trailingAnchor()
                .constraintEqualToAnchor(&host.trailingAnchor()),
            view.topAnchor().constraintEqualToAnchor(&safe.topAnchor()),
            view.bottomAnchor()
                .constraintEqualToAnchor(&host.bottomAnchor()),
        ]));
        *self.0.content.borrow_mut() = Some(view);
    }

    /// 右の区画から子を外して返す。
    pub(crate) fn take_content(&self) -> Option<Retained<NSView>> {
        let view = self.0.content.borrow_mut().take()?;
        view.removeFromSuperview();
        Some(view)
    }

    /// 覚えている幅へ仕切りを置く。
    ///
    /// 幅は区画のビューの幅 (macOS 26 の浮いたガラスでは、その周りの余白を
    /// 含む)。ウィンドウへ取り付ける前は区画の大きさが 0 なので、取り付けた
    /// あと ([`Window::set_sidebar`](crate::Window::set_sidebar)) にも呼び直す。
    pub(crate) fn apply_width(&self) {
        let split = self.0.controller.splitView();
        self.0.applying.set(true);
        split.layoutSubtreeIfNeeded();
        let placed = split.frame().size.width > 0.0 && !self.is_collapsed();
        if placed {
            split.setPosition_ofDividerAtIndex(self.0.width.get(), 0);
            split.layoutSubtreeIfNeeded();
        }
        // 置けたときだけ、この先の仕切りの動きを利用者の操作として拾う。
        self.0.applying.set(!placed);
    }

    fn is_selectable(&self, index: usize) -> bool {
        naui_core::sidebar_item(&self.0.sections.borrow(), index).is_some_and(|item| item.enabled)
    }

    /// 通し番号の項目がある行を選ぶ。
    fn select_row_of(&self, index: usize) {
        let row = self
            .0
            .rows
            .borrow()
            .iter()
            .position(|row| matches!(row, Row::Item(i, _) if *i == index));
        if let Some(row) = row {
            self.0
                .table
                .selectRowIndexes_byExtendingSelection(&NSIndexSet::indexSetWithIndex(row), false);
            self.0.table.scrollRowToVisible(row as NSInteger);
        }
    }

    fn without_notifying(&self, f: impl FnOnce()) {
        let previous = self.0.silent.replace(true);
        f();
        self.0.silent.set(previous);
    }
}

/// ビューを 1 つ持つだけの `NSViewController`。
///
/// nib を使わないので、`view` を読む前に必ず `setView:` しておく
/// (しないと `loadView` が nib を探して落ちる)。
fn view_controller(mtm: MainThreadMarker, view: &NSView) -> Retained<NSViewController> {
    let controller =
        NSViewController::initWithNibName_bundle(NSViewController::alloc(mtm), None, None);
    controller.setView(view);
    controller
}
