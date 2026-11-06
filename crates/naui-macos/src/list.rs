//! Lists (AppKit).
//!
//! A single-column `NSTableView` placed in an `NSScrollView`. It is the same
//! control as macOS's source lists and file lists, and row drawing, scrolling,
//! multiple selection with ⌘ / Shift and keyboard navigation are all done by
//! AppKit.
//!
//! Text-only [`ListItem`]s are also normalized into [`ListRow`]s and handled
//! through the same row model and cell creation path. Selection and row activation
//! are passed to Rust closures.
//!
//! **Row clicks are received through the `NSTableView`'s action.** Hit-testing a
//! click on a display-only label or icon returns the table rather than the cell, so
//! real clicks never reach the cell's `mouseDown:`. Buttons and fields are hit
//! themselves, so this action is not called and nothing fires twice.
//!
//! **Row activation (double-click) is also received through the `NSTableView`'s
//! `doubleAction`.** Return / Enter are keys `NSTableView` does nothing with, so
//! they are caught in `keyDown:` and sent to the same notification. Right-clicks
//! come through `rightMouseDown:`, and other ways of asking for a context menu, such
//! as Control + click, through `menuForEvent:`, which AppKit calls when it wants a
//! context menu; the clicked row and position are passed to the app. The app shows
//! the menu itself with [`crate::PopupMenu`].

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use dispatch2::{DispatchQueue, MainThreadBound};
use naui_core::{
    Align, ListItem, Orientation, Point, Result, RowSource, SelectionMode, Sizing, TextColor,
    TextStyle,
};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{define_class, msg_send, sel, DefinedClass, MainThreadMarker, MainThreadOnly, Message};
use objc2_app_kit::{
    NSColor, NSControlTextEditingDelegate, NSEvent, NSEventModifierFlags, NSLayoutConstraint,
    NSMenu, NSScrollView, NSTableCellView, NSTableColumn, NSTableColumnResizingOptions,
    NSTableView, NSTableViewColumnAutoresizingStyle, NSTableViewDataSource, NSTableViewDelegate,
    NSTableViewStyle, NSTextField, NSView, NSViewNoIntrinsicMetric,
};
use objc2_foundation::{
    NSArray, NSIndexSet, NSInteger, NSMutableIndexSet, NSNotification, NSSize, NSString,
};

use crate::trampoline::{SelectHandler, ValueHandler};
use crate::widgets::{Label, Stack, Widget};

/// Only one column is used, so the identifier can be fixed.
const COLUMN_ID: &str = "naui.list.column";

/// The padding above and below a row. AppKit computes the row height from the
/// constraints.
pub(crate) const ROW_PADDING: f64 = 4.0;
/// The spacing between the label and the secondary text.
const DETAIL_SPACING: f64 = 1.0;

/// The handler notified when a row is clicked.
///
/// Replacing the same row's callback during the call does not double-borrow.
#[derive(Clone, Default)]
pub(crate) struct ActivationHandler(Rc<RefCell<Option<Box<dyn FnMut()>>>>);

impl ActivationHandler {
    pub(crate) fn set(&self, f: impl FnMut() + 'static) {
        *self.0.borrow_mut() = Some(Box::new(f));
    }

    pub(crate) fn emit(&self) {
        let Some(mut f) = self.0.borrow_mut().take() else {
            return;
        };
        f();
        let mut slot = self.0.borrow_mut();
        if slot.is_none() {
            *slot = Some(f);
        }
    }
}

/// The content of one row. Plain text rows and custom widget rows are normalized
/// into the same type.
enum ListRowContent {
    Item(ListItem),
    Custom(Box<dyn Widget>),
}

impl Clone for ListRowContent {
    fn clone(&self) -> Self {
        match self {
            Self::Item(item) => Self::Item(item.clone()),
            Self::Custom(content) => Self::Custom(content.boxed_clone()),
        }
    }
}

/// One row to put in a list.
///
/// [`ListItem`] is a convenience API for lists that need only strings. To combine
/// icons, several labels, check boxes, trailing buttons and so on, as in a settings
/// screen, build the content with `Grid` / `Stack` and wrap it in this type.
///
/// ```no_run
/// # use naui_macos::{ListRow, Widget};
/// # fn row(content: &dyn Widget) {
/// let row = ListRow::new(content).selectable(false);
/// row.on_activate(|| println!("row clicked"));
/// # let _ = row;
/// # }
/// ```
pub struct ListRow {
    content: ListRowContent,
    selectable: bool,
    activation: ActivationHandler,
}

impl Clone for ListRow {
    fn clone(&self) -> Self {
        Self {
            content: self.content.clone(),
            selectable: self.selectable,
            activation: self.activation.clone(),
        }
    }
}

impl ListRow {
    /// Uses a widget as the content of one row. By default the whole row can be
    /// selected too.
    pub fn new(content: &dyn Widget) -> Self {
        Self {
            content: ListRowContent::Custom(content.boxed_clone()),
            selectable: true,
            activation: ActivationHandler::default(),
        }
    }

    /// Converts a `ListItem` into a plain text row. The normalization path for
    /// `List::set_items`.
    fn from_item(item: ListItem) -> Self {
        let selectable = item.enabled;
        Self {
            content: ListRowContent::Item(item),
            selectable,
            activation: ActivationHandler::default(),
        }
    }

    /// Whether the whole row can be selected.
    ///
    /// Set to `false` for settings rows where only the buttons or check boxes inside
    /// are operated.
    pub fn selectable(mut self, selectable: bool) -> Self {
        self.selectable = selectable;
        self
    }

    pub fn is_selectable(&self) -> bool {
        self.selectable
    }

    /// What to do when a label, icon or empty space inside the row is clicked.
    ///
    /// It is not called when a control such as a check box, button or text field is
    /// clicked directly, so the same action does not fire twice.
    pub fn on_activate(&self, f: impl FnMut() + 'static) {
        self.activation.set(f);
    }

    /// The view for one row. With `min_height`, the row is at least that tall.
    fn cell_view(&self, mtm: MainThreadMarker, min_height: Option<f64>) -> Retained<NSView> {
        match &self.content {
            ListRowContent::Item(item) => list_item_cell_view(mtm, item, min_height),
            ListRowContent::Custom(content) => custom_cell_view(mtm, &**content, min_height),
        }
    }

    /// A row that can be neither selected nor operated (`ListItem::enabled(false)`).
    ///
    /// The other three platforms disable the row itself, so neither activation nor the
    /// context menu reaches it. `NSTableView` has no disabled rows, so this checks it
    /// here to match.
    fn is_disabled_item(&self) -> bool {
        matches!(&self.content, ListRowContent::Item(item) if !item.enabled)
    }
}

/// Where rows come from: rows the app passed ([`List::set_rows`]) or rows built when
/// they become visible ([`List::set_row_builder`]).
///
/// `NSTableView` asks only for the rows it shows, so nothing else limits the rows here.
/// Built rows are remembered until they leave the screen, because whether a row can be
/// selected and its activation are looked up from them.
#[derive(Default)]
struct ListRows {
    source: RowSource<ListRow>,
    built: RefCell<HashMap<usize, ListRow>>,
    /// Whether the app's function is building a row right now.
    building: Cell<bool>,
}

impl ListRows {
    fn len(&self) -> usize {
        self.source.len()
    }

    fn set_rows(&self, rows: Vec<ListRow>) {
        self.built.borrow_mut().clear();
        self.source.set_rows(rows);
    }

    fn set_builder(&self, count: usize, build: impl FnMut(usize) -> Result<ListRow> + 'static) {
        self.built.borrow_mut().clear();
        self.source.set_builder(count, build);
    }

    /// Whether this is being called from inside the app's function that builds a row.
    fn is_building(&self) -> bool {
        self.building.get()
    }

    /// The contents of that row, building it if it is not built yet.
    ///
    /// `None` if the rows were replaced while building it: the row belongs to the old
    /// rows, and the table reloads afterwards ([`List::reload_after_build`]).
    fn row(&self, index: usize) -> Option<ListRow> {
        if !self.source.is_built() {
            return self.source.row(index)?.ok();
        }
        if let Some(row) = self.built.borrow().get(&index) {
            return Some(row.clone());
        }
        let generation = self.source.generation();
        let was_building = self.building.replace(true);
        let row = self.source.row(index);
        self.building.set(was_building);
        if self.source.generation() != generation {
            return None;
        }
        // 組み立てに失敗した行は、空の行にする。
        let row = row?.unwrap_or_else(|_| ListRow::from_item(ListItem::new("")));
        self.built.borrow_mut().insert(index, row.clone());
        Some(row)
    }

    /// A row that can already be read, without building anything.
    fn read<R>(&self, index: usize, read: impl FnOnce(&ListRow) -> R) -> Option<R> {
        if !self.source.is_built() {
            return self.source.with_stored(index, read);
        }
        self.built.borrow().get(&index).map(read)
    }

    fn is_selectable(&self, index: usize) -> bool {
        self.source
            .is_selectable(index, ListRow::is_selectable, || {
                self.read(index, ListRow::is_selectable)
            })
    }

    fn is_disabled_item(&self, index: usize) -> bool {
        self.read(index, ListRow::is_disabled_item).unwrap_or(false)
    }

    fn activation(&self, index: usize) -> Option<ActivationHandler> {
        self.read(index, |row| row.activation.clone())
    }

    fn forget_built(&self) {
        self.built.borrow_mut().clear();
    }

    /// Forgets a row that left the screen, so built rows do not pile up without limit.
    fn forget_row(&self, index: usize) {
        self.built.borrow_mut().remove(&index);
    }
}

/// The key codes of Return and the keypad Enter.
const RETURN_KEY_CODES: [u16; 2] = [36, 76];

/// The state seen by the `NSTableView` that receives row activation and context
/// menus.
struct ListTableState {
    rows: Rc<ListRows>,
    /// The handler for double-clicks and Return / Enter.
    row_activate: SelectHandler,
    /// The handler for right-clicks.
    context_menu: ValueHandler<(usize, Point)>,
}

impl ListTableState {
    /// The index, if `row` is a row that can receive activation and context menus.
    fn live_row(&self, row: NSInteger) -> Option<usize> {
        let index = usize::try_from(row).ok().filter(|&i| i < self.rows.len())?;
        (!self.rows.is_disabled_item(index)).then_some(index)
    }
}

define_class!(
    #[unsafe(super(NSTableView))]
    #[thread_kind = MainThreadOnly]
    #[name = "NauiListTableView"]
    #[ivars = ListTableState]
    /// An `NSTableView` that receives row clicks, activation and context menus.
    ///
    /// It also receives its own target / action (`NSTableView` does not retain its
    /// target, but there is no lifetime concern when it is itself).
    struct ListTableView;

    unsafe impl NSObjectProtocol for ListTableView {}

    impl ListTableView {
        /// A row click (action).
        #[unsafe(method(nauiRowClicked:))]
        fn row_clicked(&self, _sender: Option<&AnyObject>) {
            let Ok(index) = usize::try_from(self.clickedRow()) else {
                return;
            };
            // 通知の中から `set_rows` を呼べるように、借りたまま呼ばない。
            let activation = self.ivars().rows.activation(index);
            if let Some(activation) = activation {
                activation.emit();
            }
        }

        /// A row double-click (doubleAction).
        #[unsafe(method(nauiRowDoubleClicked:))]
        fn row_double_clicked(&self, _sender: Option<&AnyObject>) {
            let state = self.ivars();
            if let Some(index) = state.live_row(self.clickedRow()) {
                state.row_activate.emit(index);
            }
        }

        /// Right-click. Over a row with a handler set, it notifies and stops here.
        ///
        /// If `menuForEvent:` merely returned `nil`, AppKit would pass the right-click to
        /// the parent (the scroll view), and even a menu attached with `PopupMenu::attach`
        /// would appear.
        #[unsafe(method(rightMouseDown:))]
        fn right_mouse_down(&self, event: &NSEvent) {
            if !self.notify_context_menu(event) {
                let _: () = unsafe { msg_send![super(self), rightMouseDown: event] };
            }
        }

        /// Where AppKit asks for a context menu by means other than right-click (Control +
        /// click and so on).
        ///
        /// If a handler is set and the click was over a row, it passes the row and position
        /// and returns `nil` (the app shows the menu with `PopupMenu::open_at`). Otherwise
        /// it leaves it to `NSTableView`'s default behavior (its own `menu`).
        #[unsafe(method_id(menuForEvent:))]
        fn menu_for_event(&self, event: &NSEvent) -> Option<Retained<NSMenu>> {
            if self.notify_context_menu(event) {
                None
            } else {
                unsafe { msg_send![super(self), menuForEvent: event] }
            }
        }

        /// Return / Enter activates the selected row (the most recently selected one).
        #[unsafe(method(keyDown:))]
        fn key_down(&self, event: &NSEvent) {
            let state = self.ivars();
            let modifiers = event.modifierFlags()
                & (NSEventModifierFlags::Command
                    | NSEventModifierFlags::Option
                    | NSEventModifierFlags::Control
                    | NSEventModifierFlags::Shift);
            if RETURN_KEY_CODES.contains(&event.keyCode())
                && modifiers.is_empty()
                && state.row_activate.is_set()
            {
                if let Some(index) = state.live_row(self.selectedRow()) {
                    state.row_activate.emit(index);
                    return;
                }
            }
            let _: () = unsafe { msg_send![super(self), keyDown: event] };
        }
    }
);

impl ListTableView {
    fn new(mtm: MainThreadMarker, state: ListTableState) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(state);
        unsafe { msg_send![super(this), init] }
    }

    /// If a handler is set and the click was over a row, notifies the row and position
    /// and returns `true`.
    fn notify_context_menu(&self, event: &NSEvent) -> bool {
        let state = self.ivars();
        if !state.context_menu.is_set() {
            return false;
        }
        let at_table = self.convertPoint_fromView(event.locationInWindow(), None);
        let Some(index) = state.live_row(self.rowAtPoint(at_table)) else {
            return false;
        };
        let point = self.list_point(event);
        state.context_menu.emit((index, point));
        true
    }

    /// Returns the event's position in coordinates from the top left of the `List`'s
    /// view (the outer scroll view). Can be passed straight to `PopupMenu::open_at`.
    fn list_point(&self, event: &NSEvent) -> Point {
        let outer: Retained<NSView> = match self.enclosingScrollView() {
            Some(scroll) => {
                let view: &NSView = &scroll;
                view.retain()
            }
            None => {
                let view: &NSView = self;
                view.retain()
            }
        };
        let local = outer.convertPoint_fromView(event.locationInWindow(), None);
        // AppKit のビュー座標は既定で左下原点。naui は左上原点でそろえる。
        let y = if outer.isFlipped() {
            local.y
        } else {
            outer.bounds().size.height - local.y
        };
        Point::new(local.x, y)
    }
}

/// The handler notified when the selection changes.
///
/// To use the same shape for single and multiple selection, the selected rows are
/// passed in ascending order. With multiple selection it can be empty.
#[derive(Clone, Default)]
pub(crate) struct SelectionHandler(Rc<RefCell<Option<Box<dyn FnMut(&[usize])>>>>);

impl SelectionHandler {
    pub(crate) fn set(&self, f: impl FnMut(&[usize]) + 'static) {
        *self.0.borrow_mut() = Some(Box::new(f));
    }

    /// Notifies the selection. Does nothing if not set yet.
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

/// The state seen by the data source and delegate. Shared with the handle.
struct SourceState {
    rows: Rc<ListRows>,
    handler: SelectionHandler,
    /// Stops notifications only while the selection is being changed from code, since
    /// AppKit calls the delegate even for `selectRowIndexes:`.
    silent: Rc<Cell<bool>>,
    /// The row height decided by the app ([`List::set_row_height`]).
    fixed_row_height: Rc<Cell<Option<f64>>>,
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
            self.ivars().rows.len() as NSInteger
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
            // 組み立てでアプリのコードが動くので、借用を持ったまま呼ばない。
            usize::try_from(row)
                .ok()
                .and_then(|row| self.ivars().rows.row(row))
                .map(|row| row.cell_view(mtm, self.ivars().fixed_row_height.get()))
        }

        #[unsafe(method(tableView:shouldSelectRow:))]
        fn should_select_row(&self, _table_view: &NSTableView, row: NSInteger) -> bool {
            usize::try_from(row)
                .ok()
                .is_some_and(|row| self.ivars().rows.is_selectable(row))
        }

        // 行が画面から外れたら、その行のために組み立てたものを忘れる。
        // ここからテーブルへ問い合わせない (`Table` と同じ。再入の警告になる)。
        #[unsafe(method(tableView:didRemoveRowView:forRow:))]
        fn did_remove_row_view(
            &self,
            _table_view: &NSTableView,
            _row_view: &NSView,
            row: NSInteger,
        ) {
            // 行が消されたときは -1 が来る。
            if let Ok(row) = usize::try_from(row) {
                self.ivars().rows.forget_row(row);
            }
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
            let indices = selected_indices(&table, state.rows.len());
            state.handler.emit(&indices);
        }
    }
);

/// Creates the view for one row.
///
/// The content is built from naui's `Stack` and `Label`. The smallness and dimness
/// of the secondary text come from [`TextStyle::Caption`] and
/// [`TextColor::Secondary`], so no point size or color is written here.
pub(crate) fn cell_view(
    mtm: MainThreadMarker,
    label: &str,
    detail: Option<&str>,
    enabled: bool,
) -> Retained<NSView> {
    item_cell_view(mtm, label, detail, enabled, None)
}

/// [`cell_view`] with a minimum row height.
fn item_cell_view(
    mtm: MainThreadMarker,
    label: &str,
    detail: Option<&str>,
    enabled: bool,
    min_height: Option<f64>,
) -> Retained<NSView> {
    let (content, title) = item_content(mtm, label, detail, enabled);
    let cell = cell_with_content(mtm, &content, min_height);
    // `textField` に入れておくと、行の背景色に合わせた文字色や
    // アクセシビリティの扱いを NSTableCellView が面倒を見る。
    unsafe { cell.setTextField(Some(&title)) };
    let view: &NSView = cell.as_ref();
    view.retain()
}

/// Puts `ListItem`s on the same cell path as `ListRow`s.
fn list_item_cell_view(
    mtm: MainThreadMarker,
    item: &ListItem,
    min_height: Option<f64>,
) -> Retained<NSView> {
    item_cell_view(
        mtm,
        &item.label,
        item.detail.as_deref(),
        item.enabled,
        min_height,
    )
}

/// The content of a text-only row. The returned `NSTextField` is the first line's.
fn item_content(
    mtm: MainThreadMarker,
    label: &str,
    detail: Option<&str>,
    enabled: bool,
) -> (Stack, Retained<NSTextField>) {
    let content = Stack::new(mtm, Orientation::Vertical);
    content.set_align(Align::Fill);
    content.set_spacing(DETAIL_SPACING);

    let title = row_label(mtm, label);
    let mut lines = vec![title.clone()];
    content.append(&title);
    if let Some(detail) = detail {
        let sub = row_label(mtm, detail);
        sub.set_style(TextStyle::Caption);
        sub.set_color(TextColor::Secondary);
        content.append(&sub);
        lines.push(sub);
    }
    // 無効の色は**役割の色より後に**当てる。補助は役割としては `Secondary`
    // だが、選べない行ではそちらより無効の色が勝つ。
    if !enabled {
        for line in &lines {
            dim_disabled(line);
        }
    }
    (content, native_field(&title))
}

/// One piece of text placed in a row.
///
/// **It stretches to the full width of the row.** Only once the width is decided
/// can text that does not fit be cut off with an ellipsis.
fn row_label(mtm: MainThreadMarker, text: &str) -> Label {
    let label = Label::new(mtm, text);
    label.set_sizing(Sizing::fill_width());
    label
}

/// The text color of rows that cannot be selected.
///
/// `NSTableView` has no notion of disabled rows, so the color AppKit uses for
/// disabled controls is applied to the text (the other three platforms can disable
/// the row itself, so this treatment is macOS only).
fn dim_disabled(label: &Label) {
    native_field(label).setTextColor(Some(&NSColor::disabledControlTextColor()));
}

/// The label's real object. Needed only when passing it to `NSTableCellView`.
fn native_field(label: &Label) -> Retained<NSTextField> {
    label
        .native_view()
        .downcast()
        .expect("the label is an NSTextField")
}

/// Creates an `NSTableCellView` holding one content view.
///
/// If an `NSTextField` with only text were used as the row as is, AppKit would draw
/// the text at the top of its frame, misaligning it from the selection highlight
/// that fills the row. Putting it inside AppKit's standard `NSTableCellView`, with
/// the top and bottom margins **connected by constraints**, lets Auto Layout
/// (`usesAutomaticRowHeights`) decide the height, so the highlight and the text line
/// up with one line or two.
///
/// With `min_height`, the row is at least that tall and the content is centered in it. A
/// row whose content needs more grows, so nothing is cut off.
fn cell_with_content(
    mtm: MainThreadMarker,
    content: &dyn Widget,
    min_height: Option<f64>,
) -> Retained<NSTableCellView> {
    let cell = NSTableCellView::new(mtm);
    let view = content.native_view();
    view.setTranslatesAutoresizingMaskIntoConstraints(false);
    cell.addSubview(&view);
    let mut constraints = vec![
        view.leadingAnchor()
            .constraintEqualToAnchor(&cell.leadingAnchor()),
        view.trailingAnchor()
            .constraintEqualToAnchor(&cell.trailingAnchor()),
    ];
    match min_height {
        None => constraints.extend([
            view.topAnchor()
                .constraintEqualToAnchor_constant(&cell.topAnchor(), ROW_PADDING),
            cell.bottomAnchor()
                .constraintEqualToAnchor_constant(&view.bottomAnchor(), ROW_PADDING),
        ]),
        Some(height) => constraints.extend([
            view.centerYAnchor()
                .constraintEqualToAnchor(&cell.centerYAnchor()),
            view.topAnchor()
                .constraintGreaterThanOrEqualToAnchor_constant(&cell.topAnchor(), ROW_PADDING),
            cell.bottomAnchor()
                .constraintGreaterThanOrEqualToAnchor_constant(&view.bottomAnchor(), ROW_PADDING),
            cell.heightAnchor()
                .constraintGreaterThanOrEqualToConstant(height),
        ]),
    }
    NSLayoutConstraint::activateConstraints(&NSArray::from_retained_slice(&constraints));
    cell
}

/// Connects any widget to Auto Layout, including the row height.
fn custom_cell_view(
    mtm: MainThreadMarker,
    content: &dyn Widget,
    min_height: Option<f64>,
) -> Retained<NSView> {
    let cell = cell_with_content(mtm, content, min_height);
    let view: &NSView = cell.as_ref();
    view.retain()
}

impl ListSource {
    fn new(mtm: MainThreadMarker, state: SourceState) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(state);
        unsafe { msg_send![super(this), init] }
    }
}

/// Reads the table's selection as an ascending sequence.
pub(crate) fn selected_indices(table: &NSTableView, len: usize) -> Vec<usize> {
    let indexes = table.selectedRowIndexes();
    (0..len).filter(|&i| indexes.containsIndex(i)).collect()
}

/// Supplies the "natural height showing every row" that `NSScrollView` lacks.
///
/// `NSTableView` itself returns an intrinsic size including the automatic row
/// heights, so that is passed on to the outer scroll view. Fixed heights and
/// `Fill` take precedence over this value in `apply_sizing`, so it still scrolls as
/// before.
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

        /// Once the parent hands out a width, the heights of rows that wrap change from
        /// when they were created. Only that change is fed back into the next Auto Layout
        /// pass.
        #[unsafe(method(layout))]
        fn layout(&self) {
            let _: () = unsafe { msg_send![super(self), layout] };
            // 子 Grid の Auto 行高は、親から幅が配られた後に確定する。
            // 先に文書側を解き、更新後の NSTableView の自然高をこの回で読む。
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
        // 行が増減すると一覧の自然高が変わる。intrinsic size を捨てるだけでは
        // 親のレイアウトが回らないので (`Grid` の `Auto` 行は自分の `layout` で
        // 行高を引き直す)、親の連なりへも伝える。
        let view: &NSView = self;
        crate::layout::invalidate_ancestors(view);
    }
}

struct ListInner {
    /// The view visible from outside. The list is one widget together with this scroll
    /// view.
    scroll: Retained<ContentSizedScrollView>,
    table: Retained<ListTableView>,
    rows: Rc<ListRows>,
    /// The row height decided by the app ([`List::set_row_height`]).
    fixed_row_height: Rc<Cell<Option<f64>>>,
    /// AppKit's row height, used as its estimate while heights are automatic.
    default_row_height: f64,
    mode: Cell<SelectionMode>,
    handler: SelectionHandler,
    silent: Rc<Cell<bool>>,
    /// The selection to apply once the table has reloaded, while a reload is waiting for
    /// a row build to finish ([`List::reload_after_build`]).
    pending_selection: RefCell<Option<Vec<usize>>>,
    /// Delegates and data sources are weak references, so they are held.
    _source: Retained<ListSource>,
}

/// A vertical list of selectable rows (NSTableView).
///
/// Inside is a single-column `NSTableView` in an `NSScrollView`, and
/// [`Widget::native_view`] returns that scroll view. Without a height set
/// ([`naui_core::Length::Auto`]), it follows the height of all its rows; with a
/// fixed height or `Fill`, it scrolls whatever overflows.
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
        let rows: Rc<ListRows> = Rc::default();
        let table = ListTableView::new(
            mtm,
            ListTableState {
                rows: rows.clone(),
                row_activate: SelectHandler::default(),
                context_menu: ValueHandler::default(),
            },
        );
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

        let handler = SelectionHandler::default();
        let silent = Rc::new(Cell::new(false));
        let fixed_row_height = Rc::new(Cell::new(None));
        let source = ListSource::new(
            mtm,
            SourceState {
                rows: rows.clone(),
                handler: handler.clone(),
                silent: silent.clone(),
                fixed_row_height: fixed_row_height.clone(),
            },
        );
        unsafe {
            table.setDataSource(Some(ProtocolObject::from_ref(&*source)));
            table.setDelegate(Some(ProtocolObject::from_ref(&*source)));
        }

        // 行のクリックは `NSTableView` の action で受ける。
        // 表示専用のラベルやアイコンを押すと、AppKit のヒットテストは
        // セルではなくテーブルを返すため、セル側の `mouseDown` では届かない。
        // ボタンやチェックボックスは自分がヒットするので、この action は
        // 呼ばれず、行の activation と二重に発火しない。ダブルクリックも
        // 同じ切り分けで `doubleAction` に届く。
        unsafe {
            let target: &AnyObject = &table;
            table.setTarget(Some(target));
            table.setAction(Some(sel!(nauiRowClicked:)));
            table.setDoubleAction(Some(sel!(nauiRowDoubleClicked:)));
        }

        let default_row_height = table.rowHeight();
        let scroll = ContentSizedScrollView::new(mtm);
        scroll.setHasVerticalScroller(true);
        let document: &NSView = &table;
        scroll.setDocumentView(Some(document));

        Self(Rc::new(ListInner {
            scroll,
            table,
            rows,
            fixed_row_height,
            default_row_height,
            mode: Cell::new(SelectionMode::Single),
            handler,
            silent,
            pending_selection: RefCell::new(None),
            _source: source,
        }))
    }

    /// Rebuilds the rows. Indices change meaning, so the selection is cleared.
    pub fn set_items(&self, items: &[ListItem]) {
        self.0
            .rows
            .set_rows(items.iter().cloned().map(ListRow::from_item).collect());
        self.reload_rows();
    }

    /// Replaces the rows with rows built from any widgets.
    ///
    /// Controls inside a row can have their own callbacks as usual. Indices change
    /// meaning, so the selection is cleared.
    pub fn set_rows(&self, rows: &[ListRow]) {
        self.0.rows.set_rows(rows.to_vec());
        self.reload_rows();
    }

    /// Passes rows in a form that **builds each row when it becomes visible**.
    ///
    /// `count` is the number of rows, and `build` receives an index from 0 to
    /// `count - 1` and returns that row ([`ListRow`]). It is called only for the rows on
    /// screen (and a few before and after), so opening stays fast even with hundreds of
    /// thousands of rows. Indices change meaning, so the selection is cleared.
    ///
    /// The widgets in a row may be recreated every time it is built. Keep per-row state
    /// in the app's data and build from it inside `build`.
    ///
    /// ```no_run
    /// # use naui_macos::{List, ListRow, Ui};
    /// # fn fill(ui: &Ui, list: &List, cities: Vec<String>) {
    /// let ui = ui.clone();
    /// list.set_row_builder(cities.len(), move |index| {
    ///     Ok(ListRow::new(&ui.label(&cities[index])?))
    /// });
    /// # }
    /// ```
    pub fn set_row_builder(
        &self,
        count: usize,
        build: impl FnMut(usize) -> Result<ListRow> + 'static,
    ) {
        self.0.rows.set_builder(count, build);
        self.reload_rows();
    }

    /// Passes a function that answers whether a row can be selected **without building
    /// the row**.
    ///
    /// For rows built with [`List::set_row_builder`], naui does not know whether rows
    /// not yet built can be selected. With this function, [`List::set_selection`] can
    /// keep its promise of "removing rows that cannot be selected" even for rows off
    /// screen. Without it, built rows follow [`ListRow::selectable`] and rows not yet
    /// built are treated as selectable.
    ///
    /// Its answers must match the [`ListRow::selectable`] returned by `build`.
    pub fn set_row_selectable(&self, selectable: impl Fn(usize) -> bool + 'static) {
        self.0.rows.source.set_selectable(selectable);
    }

    /// Rebuilds the visible rows. The row count and selection stay.
    ///
    /// Use it when what the function passed to [`List::set_row_builder`] returns has
    /// changed.
    pub fn refresh(&self) {
        let picked = self.selection();
        // 組み立ての最中は読み直しの結果を AppKit が使えないので、予約を残す。
        if !self.0.rows.is_building() {
            self.0.pending_selection.take();
        }
        self.0.rows.forget_built();
        self.without_notifying(|this| {
            this.apply_row_height();
            this.0.table.reloadData();
            this.apply_selection(&picked);
            this.0.scroll.invalidate_document_size();
        });
    }

    /// Sets the row height in logical pixels. Passing 0 or less measures it from the
    /// rows.
    ///
    /// Every row gets at least this height, and a row whose content needs more grows.
    /// AppKit measures rows only when they appear, so with many rows of varying height
    /// the scroll bar shifts as it finds out their real heights; setting a height that
    /// fits every row avoids that.
    pub fn set_row_height(&self, height: f64) {
        self.0
            .fixed_row_height
            .set((height > 0.0).then_some(height));
        self.refresh();
    }

    /// The row height (logical pixels). 0 if there are no rows.
    ///
    /// With no height set, rows may differ in height, and this is the first row's.
    pub fn row_height(&self) -> f64 {
        if let Some(height) = self.0.fixed_row_height.get() {
            return height;
        }
        match self.len() {
            0 => 0.0,
            _ => self.0.table.rectOfRow(0).size.height,
        }
    }

    fn reload_rows(&self) {
        // AppKit が行を聞いている最中には読み直せない (再入の警告になる)。
        if self.0.rows.is_building() {
            self.reload_after_build();
            return;
        }
        // ここで読み直すので、予約した読み直しと、そのときの行へ向けた選択は要らない。
        self.0.pending_selection.take();
        self.without_notifying(|this| {
            this.apply_row_height();
            this.0.table.reloadData();
            unsafe { this.0.table.deselectAll(None) };
            this.0.scroll.invalidate_document_size();
        });
    }

    /// Reloads the table once the row build in progress has finished, and clears the
    /// selection. The app can still set a selection until then; it is applied afterwards.
    ///
    /// A reload that happens first takes its place by clearing `pending_selection`.
    fn reload_after_build(&self) {
        if self.0.pending_selection.replace(Some(Vec::new())).is_some() {
            return;
        }
        let mtm = MainThreadMarker::from(&*self.0.table);
        let weak = MainThreadBound::new(Rc::downgrade(&self.0), mtm);
        DispatchQueue::main().exec_async(move || {
            // メインキューの仕事はメインスレッドで走る。
            let mtm = MainThreadMarker::new().expect("called outside the main queue");
            if let Some(inner) = weak.into_inner(mtm).upgrade() {
                let list = List(inner);
                // 先に読み直していれば、いまの選択を消さずに終わる。
                let Some(picked) = list.0.pending_selection.take() else {
                    return;
                };
                list.without_notifying(|this| {
                    this.apply_row_height();
                    this.0.table.reloadData();
                    this.apply_selection(&picked);
                    this.0.scroll.invalidate_document_size();
                });
            }
        });
    }

    /// Gives AppKit its estimate for rows it has not measured.
    ///
    /// **Heights stay automatic even with a height set.** A fixed `rowHeight` would cut
    /// off the rows that need more; the minimum is a constraint on each cell instead.
    fn apply_row_height(&self) {
        let estimate = self.0.fixed_row_height.get();
        self.0
            .table
            .setRowHeight(estimate.unwrap_or(self.0.default_row_height));
    }

    /// The number of rows.
    pub fn len(&self) -> usize {
        self.0.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Changes how rows are selected. The meaning of the selection changes, so the
    /// selection is cleared.
    pub fn set_selection_mode(&self, mode: SelectionMode) {
        self.0.mode.set(mode);
        self.0.table.setAllowsMultipleSelection(mode.is_multiple());
        self.without_notifying(|this| this.apply_selection(&[]));
    }

    pub fn selection_mode(&self) -> SelectionMode {
        self.0.mode.get()
    }

    /// The topmost of the selected rows.
    pub fn selected(&self) -> Option<usize> {
        self.selection().first().copied()
    }

    /// The selected rows (ascending). Zero or one for single selection.
    pub fn selection(&self) -> Vec<usize> {
        if let Some(picked) = &*self.0.pending_selection.borrow() {
            return picked.clone();
        }
        selected_indices(&self.0.table, self.len())
    }

    /// Scrolls until row `index` is visible. The selection does not change.
    ///
    /// If it is already visible nothing moves; otherwise it is aligned to the nearer
    /// edge (the top if it is above, the bottom if it is below). Does nothing if out of
    /// range.
    pub fn scroll_to_row(&self, index: usize) {
        if index < self.len() {
            self.0.table.scrollRowToVisible(index as isize);
        }
    }

    /// Selects exactly one row without notifying.
    pub fn set_selected(&self, index: usize) {
        self.set_selection(&[index]);
    }

    /// Replaces the selection without notifying.
    ///
    /// Out-of-range rows, unselectable rows and duplicates are removed, and with single
    /// selection only the first remains ([`SelectionMode::normalize`]).
    pub fn set_selection(&self, indices: &[usize]) {
        let picked = self.normalize(indices);
        self.without_notifying(|this| this.apply_selection(&picked));
    }

    /// Clears the whole selection without notifying.
    pub fn clear_selection(&self) {
        self.without_notifying(|this| this.apply_selection(&[]));
    }

    /// Selects one row through the same path as a user's selection (notifies).
    pub fn select(&self, index: usize) {
        self.select_many(&[index]);
    }

    /// Replaces the selection through the same path as a user's selection (notifies).
    pub fn select_many(&self, indices: &[usize]) {
        let picked = self.normalize(indices);
        // AppKit は同じ選択を選び直すとデリゲートを呼ばない。
        // 通知の回数をそろえるため、ここで 1 回だけ出す。
        self.without_notifying(|this| this.apply_selection(&picked));
        let actual = self.selection();
        self.0.handler.emit(&actual);
    }

    /// Called with the selected rows (ascending) when the selection changes.
    ///
    /// With multiple selection it may be called with none.
    pub fn on_select(&self, f: impl FnMut(&[usize]) + 'static) {
        self.0.handler.set(f);
    }

    /// Called with the row's index when a row is activated (double-clicked, or Return
    /// / Enter).
    ///
    /// With Return / Enter, the target is the selected row (the most recently selected
    /// one if several). To open everything in a multiple selection, read
    /// [`List::selection`] inside it. Not called for rows of `ListItem`s that cannot be
    /// selected (`enabled(false)`).
    pub fn on_row_activate(&self, f: impl FnMut(usize) + 'static) {
        self.0.table.ivars().row_activate.set(f);
    }

    /// Called with the row's index and the click position when a row is right-clicked
    /// (including Control + click).
    ///
    /// The position is in logical pixels from the top left of this list and can be
    /// passed straight to [`crate::PopupMenu::open_at`]. Once set, the `NSTableView`'s
    /// own `menu` no longer appears over rows. The selection does not change (the same
    /// as AppKit). Not called for empty space without rows.
    pub fn on_context_menu(&self, mut f: impl FnMut(usize, Point) + 'static) {
        self.0
            .table
            .ivars()
            .context_menu
            .set(move |(index, point)| f(index, point));
    }

    /// The inner `NSTableView`. Public as a backend-specific escape hatch.
    ///
    /// Settings not in the common API, such as adding columns or sorting, are done from
    /// here.
    pub fn native_table(&self) -> Retained<NSTableView> {
        let table: &NSTableView = &self.0.table;
        table.retain()
    }

    fn normalize(&self, indices: &[usize]) -> Vec<usize> {
        let rows = self.0.rows.clone();
        self.0
            .mode
            .get()
            .normalize_by(indices, |index| rows.is_selectable(index))
    }

    fn apply_selection(&self, indices: &[usize]) {
        // テーブルはまだ前の行のままなので、読み直してから当てる。
        if let Some(pending) = &mut *self.0.pending_selection.borrow_mut() {
            *pending = indices.to_vec();
            return;
        }
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

    /// Operates with notifications from AppKit stopped.
    fn without_notifying(&self, f: impl FnOnce(&Self)) {
        let previous = self.0.silent.replace(true);
        f(self);
        self.0.silent.set(previous);
    }
}
