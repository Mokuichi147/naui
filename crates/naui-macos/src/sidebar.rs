//! The sidebar (AppKit).
//!
//! As on the left of System Settings and Finder, it uses the **sidebar item** of an
//! `NSSplitViewController` (`NSSplitViewItem.sidebarWithViewController:`). It
//! extends to the full height of the window (`allowsFullHeightLayout`), and AppKit
//! owns the background material (Liquid Glass on macOS 26, frosted glass before
//! that) and the collapse animation.
//!
//! | naui | AppKit |
//! | --- | --- |
//! | The whole sidebar | `NSSplitViewController` (the window's `contentViewController`) |
//! | Left pane | `NSSplitViewItem` (sidebar) + `NSScrollView` |
//! | Item list | `NSTableView` (`NSTableViewStyleSourceList`) |
//! | Item | `NSTableCellView` (an SF Symbols `NSImageView` + `NSTextField`) |
//! | Group heading | A group row (`tableView:isGroupRow:`) |
//! | Right pane | `NSSplitViewItem` (content). The window's child goes here |
//!
//! It is collapsed and expanded with AppKit's standard sidebar button
//! (`NSToolbarToggleSidebarItemIdentifier`). The button is a toolbar item, so if
//! the app has attached a toolbar it is inserted at its start; otherwise a toolbar
//! with just the button is attached to the window
//! ([`Window::set_sidebar`](crate::Window::set_sidebar)). User collapsing and
//! expanding is picked up from the `NSSplitView`'s size changes
//! (`NSSplitViewDidResizeSubviewsNotification`).
//!
//! Collapsing when the window narrows is AppKit's own
//! (`canCollapseFromWindowResize`). It works only when the content pane has a minimum
//! width, so the content item gets [`SIDEBAR_COLLAPSE_CONTENT_WIDTH`] as its
//! `minimumThickness`. When the user narrows the window, the sidebar first narrows
//! down to [`SIDEBAR_MIN_WIDTH`], then collapses; widening the window again expands it
//! back to its width.
//!
//! AppKit does this only while the user resizes the window (live resize). A size
//! set from code that is too narrow would instead widen the window to fit both
//! panes, so there naui collapses the sidebar first ([`make_room`](Sidebar::make_room)),
//! and expands it again in the next main queue iteration once the window is wide
//! enough.
//!
//! The item list is an `NSTableView` rather than an `NSOutlineView`. naui's sidebar
//! has at most two levels (groups and items) and no expansion, so
//! `NSOutlineView`'s group items, which come with disclosure triangles and "Hide"
//! buttons, do not fit. Gaps between groups are made with empty unselectable rows.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use dispatch2::{DispatchQueue, MainThreadBound};
use naui_core::{
    sidebar_len, sidebar_rows, SidebarItem, SidebarRow, SidebarSection, DEFAULT_SIDEBAR_WIDTH,
    SIDEBAR_COLLAPSE_CONTENT_WIDTH, SIDEBAR_MIN_WIDTH,
};
use objc2::rc::Retained;
use objc2::runtime::{NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{define_class, msg_send, sel, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSAutoresizingMaskOptions, NSColor, NSControlTextEditingDelegate, NSEventType,
    NSFont, NSImage, NSImageView, NSLayoutConstraint, NSScrollView, NSSplitViewController,
    NSSplitViewDidResizeSubviewsNotification, NSSplitViewItem, NSSplitViewItemUnspecifiedDimension,
    NSTableCellView, NSTableColumn, NSTableView, NSTableViewColumnAutoresizingStyle,
    NSTableViewDataSource, NSTableViewDelegate, NSTableViewStyle, NSTextField, NSView,
    NSViewController,
};
use objc2_foundation::{
    NSArray, NSIndexSet, NSInteger, NSNotification, NSNotificationCenter, NSSize, NSString,
};

use crate::toolbar::Toolbar;
use crate::trampoline::{SelectHandler, ValueHandler};

/// Only one column is used, so the identifier can be fixed.
const COLUMN_ID: &str = "naui.sidebar.column";

/// How much wider than the collapsing width the window has to get before a sidebar
/// collapsed by [`Sidebar::make_room`] expands again (logical pixels).
const EXPAND_HYSTERESIS: f64 = 8.0;

/// The height of the gap left between groups.
///
/// About the gap between groups without headings in the System Settings sidebar.
const GAP_HEIGHT: f64 = 12.0;

/// The spacing between the icon and the text.
const ICON_SPACING: f64 = 6.0;

/// The width of the frame the icon is placed in.
///
/// SF Symbols vary in width per symbol (scissors are wide, a pencil is narrow), so
/// laid out as is the text's left edges would not line up. The frame width is
/// fixed and the icon centered in it.
const ICON_WIDTH: f64 = 20.0;

/// One row on screen. `SidebarRow` in an owned form.
#[derive(Clone)]
enum Row {
    Gap,
    Header(String),
    Item(usize, SidebarItem),
}

/// The rows on screen.
///
/// The gap before a heading is omitted. Group rows are the same height as items
/// with text centered vertically, so their top part already serves as the gap.
/// Adding a gap row too would leave a larger space than between groups without
/// headings.
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

/// The state seen by the data source and delegate. Shared with the handle.
struct SourceState {
    rows: Rc<RefCell<Vec<Row>>>,
    selected: Rc<Cell<Option<usize>>>,
    handler: SelectHandler,
    /// Stops notifications only while the selection is being changed from code, since
    /// AppKit calls the delegate even for `selectRowIndexes:`.
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
    /// Receives size changes of the `NSSplitView` and checks whether the collapsed
    /// state changed.
    ///
    /// [`ActionTarget`](crate::trampoline::ActionTarget) keeps the closure borrowed
    /// during the call, so calling `set_collapsed` from inside the notification would
    /// deliver the same notification re-entrantly and double-borrow. This one only
    /// follows a weak reference, so no borrow remains even if called re-entrantly.
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
            sidebar.expand_if_room();
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

/// A group heading.
///
/// The group row's look (small, dim text) uses AppKit's standard font and color to
/// match the headings in Finder's sidebar.
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

/// One item row. The icon, if any, goes before the text.
///
/// Putting them in the `NSTableCellView`'s `imageView` / `textField` lets AppKit
/// apply the source list look, such as knocking out the text and symbol in white on
/// the selected row and giving the symbol the accent color.
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

/// Connects the cell's content with constraints: full width, vertically centered.
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

define_class!(
    /// A stand inside the right pane that places the window's child by frame rather
    /// than by constraints.
    ///
    /// Connecting the child to the pane's four edges with constraints would let the
    /// child's vertical content hugging (750 for labels) beat both the priority that
    /// keeps the window's size (`NSLayoutPriorityWindowSizeStayPut`, 500) and the
    /// user's drag (510), and the window would shrink to the child's height. Placing it
    /// by frame, as the `contentView` does without a sidebar, keeps the child's size
    /// preferences from reaching the window.
    ///
    /// The stand itself has no size preferences, so it is connected to the pane with
    /// constraints (aligning the top to the safe area below the title bar is left to
    /// the constraints).
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "NauiSidebarContentFrame"]
    #[ivars = RefCell<Option<Retained<NSView>>>]
    struct ContentFrame;

    unsafe impl NSObjectProtocol for ContentFrame {}

    impl ContentFrame {
        /// Called every time the stand's size changes (`autoresizesSubviews` is true by
        /// default).
        #[unsafe(method(resizeSubviewsWithOldSize:))]
        fn resize_subviews(&self, old_size: NSSize) {
            let _: () = unsafe { msg_send![super(self), resizeSubviewsWithOldSize: old_size] };
            self.place_content();
        }
    }
);

impl ContentFrame {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(RefCell::new(None));
        unsafe { msg_send![super(this), init] }
    }

    /// Places the child to fill the stand.
    fn place_content(&self) {
        let Some(view) = self.ivars().borrow().clone() else {
            return;
        };
        let frame = self.bounds();
        if view.frame() != frame {
            view.setFrame(frame);
        }
    }

    fn set_content(&self, view: Option<Retained<NSView>>) {
        if let Some(old) = self.ivars().borrow_mut().take() {
            old.removeFromSuperview();
        }
        let Some(view) = view else {
            return;
        };
        // 枠から作られる制約は、台の大きさに関わらない定数にする
        // (伸縮を付けると台とつながり、子の hugging がまた伝わる)。
        // 大きさは台の大きさが変わるたびに `place_content` で合わせる。
        view.setTranslatesAutoresizingMaskIntoConstraints(true);
        view.setAutoresizingMask(NSAutoresizingMaskOptions::ViewNotSizable);
        self.addSubview(&view);
        *self.ivars().borrow_mut() = Some(view);
        self.place_content();
    }

    fn take_content(&self) -> Option<Retained<NSView>> {
        let view = self.ivars().borrow_mut().take()?;
        view.removeFromSuperview();
        Some(view)
    }
}

struct SidebarInner {
    controller: Retained<NSSplitViewController>,
    sidebar_item: Retained<NSSplitViewItem>,
    /// The right pane's item. Its minimum width decides when the sidebar collapses
    /// automatically.
    content_item: Retained<NSSplitViewItem>,
    /// Whether the sidebar collapses automatically when the window narrows.
    auto_collapse: Cell<bool>,
    /// Collapsed because the window got too narrow (by [`Sidebar::make_room`] or
    /// AppKit's live resize), so expanded again when the window gets wide enough from
    /// code too. AppKit restores only what it collapsed itself, and only during live
    /// resize.
    collapsed_for_size: Cell<bool>,
    /// The stand inside the right pane that holds the window's child.
    content_frame: Retained<ContentFrame>,
    table: Retained<NSTableView>,
    /// The `NSTableView`'s dataSource / delegate are weak references, so it is held.
    _source: Retained<SidebarSource>,
    sections: RefCell<Vec<SidebarSection>>,
    rows: Rc<RefCell<Vec<Row>>>,
    selected: Rc<Cell<Option<usize>>>,
    handler: SelectHandler,
    silent: Rc<Cell<bool>>,
    /// The sidebar width. Either the `set_width` value or the width the user changed
    /// with the divider.
    width: Cell<f64>,
    on_resize: ValueHandler<f64>,
    /// True while naui is positioning the divider. Collapse and size changes during
    /// this are not user actions.
    applying: Cell<bool>,
    /// The last known collapsed state. Compared so that only the user's toggles are
    /// notified.
    collapsed: Cell<bool>,
    on_collapse: ValueHandler<bool>,
    /// A toolbar with just the sidebar button, used when the app has not attached a
    /// toolbar.
    controls: Toolbar,
    /// The receiver of size changes. The notification center does not hold observers
    /// strongly.
    observer: RefCell<Option<Retained<ResizeObserver>>>,
}

impl Drop for SidebarInner {
    fn drop(&mut self) {
        if let Some(observer) = self.observer.borrow_mut().take() {
            unsafe { NSNotificationCenter::defaultCenter().removeObserver(&observer) };
        }
    }
}

/// A sidebar attached to the left of the window.
///
/// Attach it with [`Window::set_sidebar`](crate::Window::set_sidebar). It is not
/// placed in a layout, so it is not a [`Widget`](crate::Widget).
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

        // 右の区画はタイトルバーの下まで伸びるので、子を置く台の上端は
        // タイトルバー (とツールバー) を避けた安全領域へつなぐ。
        let content_host = NSView::new(mtm);
        let content_frame = ContentFrame::new(mtm);
        content_frame.setTranslatesAutoresizingMaskIntoConstraints(false);
        content_host.addSubview(&content_frame);
        let safe = content_host.safeAreaLayoutGuide();
        NSLayoutConstraint::activateConstraints(&NSArray::from_retained_slice(&[
            content_frame
                .leadingAnchor()
                .constraintEqualToAnchor(&content_host.leadingAnchor()),
            content_frame
                .trailingAnchor()
                .constraintEqualToAnchor(&content_host.trailingAnchor()),
            content_frame
                .topAnchor()
                .constraintEqualToAnchor(&safe.topAnchor()),
            content_frame
                .bottomAnchor()
                .constraintEqualToAnchor(&content_host.bottomAnchor()),
        ]));
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
            content_item,
            auto_collapse: Cell::new(true),
            collapsed_for_size: Cell::new(false),
            content_frame,
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
            applying: Cell::new(false),
            controls,
            observer: RefCell::new(None),
        }));
        this.set_auto_collapse(true);
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

    /// Lays out items by group. Each call replaces them.
    ///
    /// The selected item stays selected (without notifying) if the same running number
    /// can still be selected.
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

    /// Lays out items as a single group without a heading.
    pub fn set_items(&self, items: &[SidebarItem]) {
        self.set_sections(&[SidebarSection::untitled(items.iter().cloned())]);
    }

    /// The number of items across groups.
    pub fn len(&self) -> usize {
        sidebar_len(&self.0.sections.borrow())
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The running number of the selected item.
    pub fn selected(&self) -> Option<usize> {
        self.0.selected.get()
    }

    /// Changes the selection without notifying. Out-of-range and unselectable items are
    /// ignored.
    pub fn set_selected(&self, index: usize) {
        if !self.is_selectable(index) {
            return;
        }
        self.without_notifying(|| self.select_row_of(index));
    }

    /// Clears the selection (without notifying).
    pub fn clear_selection(&self) {
        self.without_notifying(|| unsafe { self.0.table.deselectAll(None) });
    }

    /// Selects as if the user had chosen it, and notifies.
    ///
    /// Out-of-range and unselectable items are ignored. Notifies even if the item is
    /// already selected (the same as `select` in other navigation widgets).
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

    /// The handler called when an item is chosen. The argument is the running number.
    pub fn on_select(&self, f: impl FnMut(usize) + 'static) {
        self.0.handler.set(f);
    }

    /// The sidebar width (logical pixels). The default is [`DEFAULT_SIDEBAR_WIDTH`].
    ///
    /// The user can change the width by dragging the divider (the minimum is
    /// [`SIDEBAR_MIN_WIDTH`](naui_core::SIDEBAR_MIN_WIDTH)). The width after a change
    /// is returned by [`width`](Self::width) and delivered through
    /// [`on_resize`](Self::on_resize). This call does not call `on_resize`.
    pub fn set_width(&self, width: f64) {
        if !width.is_finite() || width <= 0.0 {
            return;
        }
        self.0.width.set(width.max(SIDEBAR_MIN_WIDTH));
        self.apply_width();
    }

    /// The sidebar width. Even while collapsed, returns the width it has when expanded.
    pub fn width(&self) -> f64 {
        self.0.width.get()
    }

    /// Called with the new width every time the user changes it with the divider.
    pub fn on_resize(&self, f: impl FnMut(f64) + 'static) {
        self.0.on_resize.set(f);
    }

    /// Reads the pane's width and, if it changed, records it again and notifies.
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

    /// Collapses (`true`) or expands (`false`) the sidebar.
    ///
    /// Items and selection are kept while collapsed. [`on_collapse`](Self::on_collapse)
    /// is not called (it is called only when the user toggles it with the sidebar
    /// button).
    pub fn set_collapsed(&self, collapsed: bool) {
        // 先に覚えておくと、このあと届く大きさの変化を通知しないで済む。
        self.0.collapsed.set(collapsed);
        self.0.collapsed_for_size.set(false);
        self.0.sidebar_item.setCollapsed(collapsed);
        if !collapsed {
            // 開き直した区画を、いま覚えている幅へそろえる。
            self.apply_width();
        }
    }

    /// The handler called when the sidebar collapses or expands other than by
    /// [`set_collapsed`](Self::set_collapsed). The argument is whether it is now
    /// collapsed.
    ///
    /// Called when the sidebar button is used, when the divider is dragged past the
    /// minimum, and when the window narrows or widens with
    /// [`set_auto_collapse`](Self::set_auto_collapse) on.
    pub fn on_collapse(&self, f: impl FnMut(bool) + 'static) {
        self.0.on_collapse.set(f);
    }

    /// Whether the sidebar collapses by itself when the window narrows, and expands
    /// again when it widens. On by default.
    ///
    /// It collapses when the content beside it would get narrower than
    /// [`SIDEBAR_COLLAPSE_CONTENT_WIDTH`]. This is AppKit's
    /// `canCollapseFromWindowResize`: the content pane gets that minimum width, the
    /// sidebar first narrows down to [`SIDEBAR_MIN_WIDTH`], and then it collapses. That
    /// follows the user resizing the window; for a size set from code
    /// ([`Window::set_size`], or attaching to a narrow window) naui collapses it, and
    /// expands it again when the window gets wide enough. Turning it off leaves the
    /// sidebar collapsed or expanded as it is.
    ///
    /// [`Window::set_size`]: crate::Window::set_size
    pub fn set_auto_collapse(&self, auto: bool) {
        self.0.auto_collapse.set(auto);
        self.0.collapsed_for_size.set(false);
        self.0.sidebar_item.setCanCollapseFromWindowResize(auto);
        let min = if auto {
            SIDEBAR_COLLAPSE_CONTENT_WIDTH
        } else {
            unsafe { NSSplitViewItemUnspecifiedDimension }
        };
        self.0.content_item.setMinimumThickness(min);
    }

    /// Whether the sidebar collapses by itself when the window narrows.
    pub fn auto_collapse(&self) -> bool {
        self.0.auto_collapse.get()
    }

    /// If the collapsed state changed, records it again and notifies.
    fn sync_collapsed(&self) {
        if self.0.applying.get() {
            return;
        }
        let now = self.is_collapsed();
        if self.0.collapsed.replace(now) != now {
            // ウィンドウを狭めて AppKit が畳んだものは、コードで広げたときにも
            // naui から開く (AppKit が開き直すのはライブリサイズの間だけ)。
            // 利用者がボタンや仕切りで開閉したものは開かない。
            self.0.collapsed_for_size.set(now && self.in_live_resize());
            self.0.on_collapse.emit(now);
        }
    }

    /// Collapses the sidebar when a window content width set from code leaves too
    /// little room for both panes. Called before the size is applied.
    ///
    /// The content pane's minimum width would otherwise make AppKit widen the window.
    /// `on_collapse` is called in the next main queue iteration, as on Linux and the
    /// Web.
    pub(crate) fn make_room(&self, width: f64) {
        if !self.0.auto_collapse.get()
            || self.is_collapsed()
            || width >= SIDEBAR_MIN_WIDTH + SIDEBAR_COLLAPSE_CONTENT_WIDTH
        {
            return;
        }
        self.0.collapsed.set(true);
        self.0.collapsed_for_size.set(true);
        // 畳むと分割ビューの大きさの通知が届くことがある (macOS 26)。窓はまだ
        // 広いままなので、その幅で開き直さないよう通知を聞き流す。
        self.0.applying.set(true);
        self.0.sidebar_item.setCollapsed(true);
        self.0.applying.set(false);
        self.on_next_turn(|sidebar| sidebar.0.on_collapse.emit(true));
    }

    /// Expands a sidebar collapsed by [`make_room`](Self::make_room) once the window is
    /// wide enough, in the next main queue iteration (not in the middle of the split
    /// view's layout).
    fn expand_if_room(&self) {
        if !self.0.collapsed_for_size.get() || self.0.applying.get() {
            return;
        }
        if !self.has_room() {
            return;
        }
        self.0.collapsed_for_size.set(false);
        self.on_next_turn(|sidebar| {
            if !sidebar.0.auto_collapse.get() || !sidebar.is_collapsed() {
                return;
            }
            // 頼まれてから窓が狭め直されていたら、開かずに待つ。
            if !sidebar.has_room() {
                sidebar.0.collapsed_for_size.set(true);
                return;
            }
            sidebar.0.collapsed.set(false);
            sidebar.0.sidebar_item.setCollapsed(false);
            sidebar.apply_width();
            sidebar.0.on_collapse.emit(false);
        });
    }

    /// Whether the split view is wide enough to expand a sidebar collapsed for size.
    ///
    /// Never during live resize: AppKit expands what it collapsed by itself then, and
    /// expanding it from naui as well reopens it while the window is still narrow
    /// (macOS 26).
    fn has_room(&self) -> bool {
        if self.in_live_resize() {
            return false;
        }
        let width = self.0.controller.splitView().frame().size.width;
        width >= SIDEBAR_MIN_WIDTH + SIDEBAR_COLLAPSE_CONTENT_WIDTH + EXPAND_HYSTERESIS
    }

    /// Whether the user is resizing the window the sidebar is in.
    fn in_live_resize(&self) -> bool {
        self.0
            .controller
            .view()
            .window()
            .is_some_and(|window| window.inLiveResize())
    }

    /// Calls `f` on the next iteration of the main queue, if the sidebar is still alive.
    fn on_next_turn(&self, f: fn(&Sidebar)) {
        let mtm = MainThreadMarker::from(&*self.0.table);
        let weak = MainThreadBound::new(Rc::downgrade(&self.0), mtm);
        DispatchQueue::main().exec_async(move || {
            // メインキューの仕事はメインスレッドで走る。
            let mtm = MainThreadMarker::new().expect("メインキューの外で呼ばれた");
            if let Some(inner) = weak.into_inner(mtm).upgrade() {
                f(&Sidebar(inner));
            }
        });
    }

    /// A toolbar with just the sidebar button, used when the app has not attached a
    /// toolbar.
    pub(crate) fn controls_toolbar(&self) -> Toolbar {
        self.0.controls.clone()
    }

    /// Whether the sidebar is collapsed.
    pub fn is_collapsed(&self) -> bool {
        self.0.sidebar_item.isCollapsed()
    }

    /// The real AppKit object (`NSSplitViewController`). A backend-specific escape
    /// hatch.
    pub fn native_split_view_controller(&self) -> Retained<NSSplitViewController> {
        self.0.controller.clone()
    }

    /// The item list (`NSTableView`). A backend-specific escape hatch.
    pub fn native_table_view(&self) -> Retained<NSTableView> {
        self.0.table.clone()
    }

    /// Places the window's child in the right pane. `None` empties it.
    ///
    /// The window's `fullSizeContentView` extends it under the title bar, so the
    /// child's top is aligned to the safe area clear of the title bar (and toolbar).
    /// The child is placed by frame rather than constraints (see [`ContentFrame`] for
    /// why).
    pub(crate) fn set_content(&self, view: Option<Retained<NSView>>) {
        self.0.content_frame.set_content(view);
    }

    /// Removes the child from the right pane and returns it.
    pub(crate) fn take_content(&self) -> Option<Retained<NSView>> {
        self.0.content_frame.take_content()
    }

    /// Puts the divider at the remembered width.
    ///
    /// The width is the width of the pane's view (on macOS 26's floating glass, it
    /// includes the margin around it). Before being attached to a window the pane's
    /// size is 0, so this is called again after attaching
    /// ([`Window::set_sidebar`](crate::Window::set_sidebar)).
    ///
    /// When collapsed, it silently expands, positions it before anything is drawn, and
    /// silently collapses again. The divider cannot be placed on a collapsed pane, and
    /// when expanding again (even with the sidebar button) AppKit restores the width it
    /// remembered before collapsing, so that remembered width is replaced with the new
    /// one too. Collapse and size changes during this are not notified (while
    /// `applying`, neither [`sync_collapsed`](Self::sync_collapsed) nor
    /// [`sync_width`](Self::sync_width) does anything).
    pub(crate) fn apply_width(&self) {
        let split = self.0.controller.splitView();
        self.0.applying.set(true);
        split.layoutSubtreeIfNeeded();
        if split.frame().size.width > 0.0 {
            let collapsed = self.0.sidebar_item.isCollapsed();
            if collapsed {
                self.0.sidebar_item.setCollapsed(false);
                split.layoutSubtreeIfNeeded();
            }
            split.setPosition_ofDividerAtIndex(self.0.width.get(), 0);
            split.layoutSubtreeIfNeeded();
            if collapsed {
                self.0.sidebar_item.setCollapsed(true);
                split.layoutSubtreeIfNeeded();
            }
        }
        // どの道を通っても必ず下ろす。立てたままにすると、この先の利用者の
        // ドラッグも開閉も拾えなくなる。
        self.0.applying.set(false);
    }

    fn is_selectable(&self, index: usize) -> bool {
        naui_core::sidebar_item(&self.0.sections.borrow(), index).is_some_and(|item| item.enabled)
    }

    /// Selects the row holding the item with the running number.
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

/// An `NSViewController` that just holds one view.
///
/// No nib is used, so `setView:` must always be called before `view` is read
/// (otherwise `loadView` looks for a nib and crashes).
fn view_controller(mtm: MainThreadMarker, view: &NSView) -> Retained<NSViewController> {
    let controller =
        NSViewController::initWithNibName_bundle(NSViewController::alloc(mtm), None, None);
    controller.setView(view);
    controller
}
