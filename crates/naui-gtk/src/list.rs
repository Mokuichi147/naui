//! A list of selectable rows (a `GtkListBox` in a `GtkScrolledWindow`).
//!
//! Both text-only [`ListItem`]s and custom-content [`ListRow`]s are normalized into
//! the same row model, and `GtkListBoxRow`s are built through the same path.
//! Selection and row activation are passed to Rust closures.
//!
//! **Row activation (double-click) and right-clicks are received by a
//! `GtkGestureClick` on each row.** `GtkListBox`'s `row-activated` fires on a
//! single click in single selection (which [`ListRow::on_activate`] relies on), so
//! it cannot be used to detect double-clicks. The gestures are attached to the
//! rows in the bubble phase, so they are not delivered over controls inside the row
//! that take clicks themselves, such as buttons. Enter is caught by a key receiver
//! in `GtkListBox`'s capture phase, only when the row itself has focus.
//!
//! **With many rows, only the rows on screen are built** (the same mechanism as
//! [`crate::Table`]). `GtkListBoxRow`s exist only for the window, spacers outside the
//! `GtkListBox` carry the height of the rest, and the selection is held here because
//! `GtkListBox` knows only the rows it has.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use gtk::prelude::*;
use gtk::{gdk, glib, graphene};
use naui_core::{
    keeps_row_window, list_row_window, merge_hidden_selection, Align, ListItem, Orientation, Point,
    Result, RowSource, RowWindow, SelectionGesture, SelectionMode, Sizing, TextColor, TextStyle,
    ROW_WINDOW_OVERSCAN,
};

use crate::bin::SizeBin;
use crate::callback::{Notifier, SelectionNotifier, Slot};
use crate::widgets::{impl_widget, without_signal, Label, Stack, Widget};

/// The spacing between the label and the secondary text.
const DETAIL_SPACING: f64 = 2.0;

/// The handler notified when a row is clicked.
///
/// Replacing the same row's callback during the call does not double-borrow.
#[derive(Clone, Default)]
pub(crate) struct ActivationHandler(Rc<Slot<dyn FnMut()>>);

impl ActivationHandler {
    pub(crate) fn set(&self, f: impl FnMut() + 'static) {
        *self.0.borrow_mut() = Some(Box::new(f));
    }

    pub(crate) fn is_some(&self) -> bool {
        self.0.borrow().is_some()
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
/// [`ListItem`] is a convenience API for lists that need only strings. Rows that
/// combine icons, several labels, check boxes, trailing buttons and so on, as in a
/// settings screen, are built with `Grid` / `Stack` and wrapped in a `ListRow`.
///
/// ```no_run
/// # use naui_gtk::{ListRow, Widget};
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

    /// Set to `false` for rows where only the controls inside are operated.
    pub fn selectable(mut self, selectable: bool) -> Self {
        self.selectable = selectable;
        self
    }

    pub fn is_selectable(&self) -> bool {
        self.selectable
    }

    /// What to do when a label, icon or empty space inside the row is clicked.
    ///
    /// When a control such as a check box, button or text field is clicked directly, it
    /// is not called (each control receives its own clicks), so the same action does
    /// not fire twice.
    ///
    /// Whether a `GtkListBoxRow` is clickable is decided when the row is built, so set
    /// this before passing it to [`List::set_rows`].
    pub fn on_activate(&self, f: impl FnMut() + 'static) {
        self.activation.set(f);
    }
}

struct ListInner {
    native: gtk::ListBox,
    /// `GtkListBox` does not scroll by itself, so it is placed in a scroll area.
    scroller: gtk::ScrolledWindow,
    /// Fills in for the rows outside the window, keeping the scroll bar length.
    top_spacer: gtk::Box,
    bottom_spacer: gtk::Box,
    bin: SizeBin,
    rows: RowSource<ListRow>,
    /// The range of rows currently built. With many rows, only those around the screen.
    window: Cell<RowWindow>,
    /// The first row the `GtkListBox` has now. `window` moves ahead of it just before the
    /// rows are rebuilt.
    built_start: Cell<usize>,
    /// Whether rows are being built right now. Prevents nested rebuilds.
    rebuilding: Cell<bool>,
    /// Whether moving the window is already waiting for the current handling to finish.
    window_update_queued: Cell<bool>,
    /// How the selection was changed (modifier keys), as inferred from the last input.
    gesture: Cell<SelectionGesture>,
    /// The contents of the built rows, in order from `window.start`.
    realized: RefCell<Vec<ListRow>>,
    /// The selected rows (ascending). **Rows outside the window are included**, so this
    /// is the source of truth, not the `GtkListBox`.
    selected: RefCell<Vec<usize>>,
    /// The height of one row (logical pixels). Either measured or set with
    /// [`List::set_row_height`].
    row_height: Cell<f64>,
    /// Whether `row_height` is from rows that are gone, so the next measurement replaces
    /// it instead of only growing it.
    row_height_stale: Cell<bool>,
    /// The row height decided by the app. If absent, it is measured from the built rows.
    fixed_row_height: Cell<Option<i32>>,
    on_select: SelectionNotifier,
    handler: RefCell<Option<glib::SignalHandlerId>>,
    /// The handler for double-clicks and Enter.
    row_activate: Notifier<usize>,
    /// The handler for right-clicks.
    context_menu: Notifier<(usize, Point)>,
}

/// A list of selectable rows. It scrolls by itself.
///
/// Without a height set, it follows the height of all its rows; with a fixed height
/// or `Fill`, it scrolls whatever overflows.
#[derive(Clone)]
pub struct List(Rc<ListInner>);
impl_widget!(List, after_sizing: |list: &List| list.update_window());

impl List {
    pub(crate) fn new() -> Self {
        let native = gtk::ListBox::new();
        native.set_selection_mode(gtk::SelectionMode::Single);
        // libadwaita の `boxed-list` (角丸のカード) は付けない。スクロール領域の
        // 枠と二重になって、角丸の中身に四角い枠が付いて見えるため。枠のほうを
        // 残すのは、一覧の高さが指定で決まる (行が少なくても場所を取る) ので、
        // どこまでが一覧なのかが分かるようにするため。macOS の `NSTableView` /
        // Windows の `ListBox` / Web の `<select size>` とも同じ見え方になる。
        let scroller = gtk::ScrolledWindow::new();
        scroller.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        scroller.set_has_frame(true);
        scroller.set_propagate_natural_height(true);
        // 詰め物を一覧の**外**に置くのは、`GtkListBox` の行として入れると
        // キーボード操作や行のインデックスに混ざってしまうため (`Table` と同じ)。
        let top_spacer = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let bottom_spacer = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
        content.append(&top_spacer);
        content.append(&native);
        content.append(&bottom_spacer);
        scroller.set_child(Some(&content));

        let bin = SizeBin::wrap(&scroller);
        let inner = Rc::new(ListInner {
            native,
            scroller,
            top_spacer,
            bottom_spacer,
            bin,
            rows: RowSource::default(),
            window: Cell::new(RowWindow::default()),
            built_start: Cell::new(0),
            rebuilding: Cell::new(false),
            window_update_queued: Cell::new(false),
            gesture: Cell::new(SelectionGesture::Unknown),
            realized: RefCell::new(Vec::new()),
            selected: RefCell::new(Vec::new()),
            row_height: Cell::new(0.0),
            row_height_stale: Cell::new(false),
            fixed_row_height: Cell::new(None),
            on_select: SelectionNotifier::default(),
            handler: RefCell::new(None),
            row_activate: Notifier::default(),
            context_menu: Notifier::default(),
        });
        // 選択の通知は常時つないでおき、プログラムから変えるときだけ止める。
        let id = {
            let weak = Rc::downgrade(&inner);
            inner.native.connect_selected_rows_changed(move |_| {
                if let Some(inner) = weak.upgrade() {
                    let list = List(inner);
                    let selection = list.read_native_selection();
                    *list.0.selected.borrow_mut() = selection.clone();
                    list.0.on_select.emit(&selection);
                }
            })
        };
        *inner.handler.borrow_mut() = Some(id);
        // 行のクリックは `GtkListBox` の `row-activated` にだけ出る。
        // `GtkListBoxRow` の `activate` (キーボードの Enter / Space) も
        // 同じ経路へ入るので、通知はここ 1 か所で受ける。
        {
            let weak = Rc::downgrade(&inner);
            inner.native.connect_row_activated(move |_, row| {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                let Ok(offset) = usize::try_from(row.index()) else {
                    return;
                };
                // 通知の中から `set_rows` を呼べるように、借りたまま呼ばない。
                let activation = inner
                    .realized
                    .borrow()
                    .get(offset)
                    .map(|row| row.activation.clone());
                if let Some(activation) = activation {
                    activation.emit();
                }
            });
        }
        inner
            .native
            .add_controller(enter_controller(Rc::downgrade(&inner)));
        // 選択が変わる前に、そのとき押されていた修飾キーを覚えておく。
        // `selected-rows-changed` には修飾キーが乗らないが、窓の外の選択を
        // 残すかどうかの判断に要る。捕捉の段で見るので、一覧が選択を決めるより
        // 先に入る (`Table` と同じ)。
        {
            let controller = gtk::EventControllerLegacy::new();
            controller.set_propagation_phase(gtk::PropagationPhase::Capture);
            let weak = Rc::downgrade(&inner);
            controller.connect_event(move |_, event| {
                if let Some(inner) = weak.upgrade() {
                    inner
                        .gesture
                        .set(crate::table::gesture_of(event.modifier_state()));
                }
                glib::Propagation::Proceed
            });
            inner.native.add_controller(controller);
        }
        // スクロールと一覧の大きさに合わせて、組み立てる範囲を動かす。
        {
            let weak = Rc::downgrade(&inner);
            let update = move |_: &gtk::Adjustment| {
                if let Some(inner) = weak.upgrade() {
                    List(inner).queue_window_update();
                }
            };
            let adjustment = inner.scroller.vadjustment();
            adjustment.connect_value_changed(update.clone());
            adjustment.connect_page_size_notify(move |adjustment| update(adjustment));
        }
        Self(inner)
    }

    /// Rebuilds the rows. Indices change meaning, so the selection is cleared.
    pub fn set_items(&self, items: &[ListItem]) {
        self.0
            .rows
            .set_rows(items.iter().cloned().map(ListRow::from_item).collect());
        self.reset_rows();
    }

    /// Rebuilds the rows. Both plain rows and rows with custom content are drawn
    /// through this path.
    ///
    /// Controls inside a row can have their own callbacks as usual.
    pub fn set_rows(&self, rows: &[ListRow]) {
        self.0.rows.set_rows(rows.to_vec());
        self.reset_rows();
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
    /// # use naui_gtk::{List, ListRow, Ui};
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
        self.reset_rows();
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
        self.0.rows.set_selectable(selectable);
    }

    /// Rebuilds the visible rows. The row count and selection stay.
    ///
    /// Use it when what the function passed to [`List::set_row_builder`] returns has
    /// changed.
    pub fn refresh(&self) {
        self.0.row_height_stale.set(true);
        self.build_window();
        // 中身や指定が変わって行の高さが動いていれば、窓の位置も変わる。測った結果が
        // 同じでも窓を引き直す (`set_row_height` は先に高さを書き換えている)。
        self.settle_row_height();
        self.update_window();
    }

    /// Sets the row height in logical pixels. Passing 0 or less measures it from the
    /// rows.
    ///
    /// Every row gets at least this height. With more rows than
    /// [`ROW_WINDOW_THRESHOLD`](naui_core::ROW_WINDOW_THRESHOLD), rows off screen are
    /// filled in as "row count × this height", so set it when the rows' heights vary. A
    /// row built later that needs more raises the height for every row.
    ///
    /// GTK sizes are whole pixels, so a fraction is rounded up.
    pub fn set_row_height(&self, height: f64) {
        // 指定は下限なので、切り捨てると指定を下回る。
        let height = height.max(0.0).ceil();
        self.0
            .fixed_row_height
            .set((height > 0.0).then_some(height as i32));
        // 指定を外したときは、組み立て直した行から測り直す。
        self.0.row_height.set(height);
        self.refresh();
    }

    /// The row height (logical pixels). 0 if no row has been built yet.
    ///
    /// With few rows and no height set, rows may differ, and this is the tallest built.
    pub fn row_height(&self) -> f64 {
        self.0.row_height.get()
    }

    pub fn len(&self) -> usize {
        self.0.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn set_selection_mode(&self, mode: SelectionMode) {
        self.0.selected.borrow_mut().clear();
        let multiple = mode.is_multiple();
        self.0.native.set_selection_mode(if multiple {
            // `Multiple` は「⌘ / Ctrl や Shift を押しながら選ぶ」形。
            gtk::SelectionMode::Multiple
        } else {
            gtk::SelectionMode::Single
        });
        // `GtkListBox` は「1 クリックで確定」(既定) の間、クリックに付いている
        // Ctrl / Shift を読まず、必ず「その行だけを選ぶ」に倒す。複数選択では
        // これを切らないと、行を足すことも外すこともできない。
        self.0.native.set_activate_on_single_click(!multiple);
        // GTK4 がモードの変更で選択を落とすとは限らないので、明示的に外す。
        without_signal(&self.0.native, &self.0.handler, || {
            self.0.native.unselect_all();
        });
    }

    pub fn selection_mode(&self) -> SelectionMode {
        match self.0.native.selection_mode() {
            gtk::SelectionMode::Multiple => SelectionMode::Multiple,
            _ => SelectionMode::Single,
        }
    }

    /// The first selected row.
    pub fn selected(&self) -> Option<usize> {
        self.selection().first().copied()
    }

    /// Returns the selected rows in ascending order.
    ///
    /// Rows outside the window are included. `GtkListBox` knows only the rows that are
    /// built, so the remembered selection is returned.
    pub fn selection(&self) -> Vec<usize> {
        self.0.selected.borrow().clone()
    }

    /// Scrolls until row `index` is visible. The selection does not change.
    ///
    /// If it is already visible nothing moves; otherwise it is aligned to the nearer
    /// edge (the top if it is above, the bottom if it is below). Does nothing if out of
    /// range.
    pub fn scroll_to_row(&self, index: usize) {
        if index >= self.len() {
            return;
        }
        // すべての行があるなら、高さのそろわない行でも実際の位置へ送れる。
        if self.0.window.get().is_complete() {
            if let Some(row) = self.0.native.row_at_index(index as i32) {
                reveal_widget(&self.0.scroller, &row, false);
            }
            return;
        }
        // 絞っている間は、まだ無い行へ送れるように位置を行の高さから求める。
        // 詰め物の高さがまだレイアウトに載っていなければ、載るのを待つ。
        if self.reveal_computed(index) {
            return;
        }
        let list = Rc::downgrade(&self.0);
        let frames = Cell::new(0u8);
        self.0.scroller.add_tick_callback(move |_, _| {
            frames.set(frames.get() + 1);
            let revealed = list
                .upgrade()
                .is_none_or(|inner| List(inner).reveal_computed(index));
            if revealed || frames.get() >= REVEAL_FRAMES {
                glib::ControlFlow::Break
            } else {
                glib::ControlFlow::Continue
            }
        });
    }

    /// Selects a single row without notifying.
    pub fn set_selected(&self, index: usize) {
        self.set_selection(&[index]);
    }

    /// Replaces the selection without notifying.
    pub fn set_selection(&self, indices: &[usize]) {
        let picked = self.normalize(indices);
        without_signal(&self.0.native, &self.0.handler, || self.show(&picked));
    }

    /// Clears the whole selection without notifying.
    pub fn clear_selection(&self) {
        without_signal(&self.0.native, &self.0.handler, || self.show(&[]));
    }

    /// Selects one row through the same path as a user's selection (notifies).
    pub fn select(&self, index: usize) {
        self.select_many(&[index]);
    }

    /// Replaces the selection through the same path as a user's selection (notifies).
    pub fn select_many(&self, indices: &[usize]) {
        let picked = self.normalize(indices);
        without_signal(&self.0.native, &self.0.handler, || self.show(&picked));
        self.0.on_select.emit(&picked);
    }

    /// Called with the indices of the selected rows every time the selection changes.
    pub fn on_select(&self, f: impl FnMut(&[usize]) + 'static) {
        self.0.on_select.set(f);
    }

    /// Called with the row's index when a row is activated (double-clicked, or Enter
    /// while the row has focus).
    ///
    /// To open everything in a multiple selection, read [`List::selection`] inside it.
    /// Not called for rows of `ListItem`s that cannot be selected (`enabled(false)`).
    pub fn on_row_activate(&self, f: impl FnMut(usize) + 'static) {
        self.0.row_activate.set(f);
    }

    /// Called with the row's index and the click position when a row is right-clicked.
    ///
    /// The position is in this list's coordinates (logical pixels, y pointing down) and
    /// can be passed straight to [`crate::PopupMenu::open_at`]. The selection does not
    /// change. Not called for empty space without rows.
    pub fn on_context_menu(&self, mut f: impl FnMut(usize, Point) + 'static) {
        self.0
            .context_menu
            .set(move |(index, point)| f(index, point));
    }

    // ------------------------------------------------------- 行を絞る窓

    /// Rebuilds the rows and moves the window back to the start. The selection is
    /// cleared too.
    fn reset_rows(&self) {
        self.0.selected.borrow_mut().clear();
        self.0.row_height_stale.set(true);
        // 組み立ての途中で差し替えられたら、組み立てている側が新しい行でやり直す。
        if self.0.rebuilding.get() {
            self.0.realized.borrow_mut().clear();
            return;
        }
        self.0.window.set(self.compute_window(ROW_WINDOW_OVERSCAN));
        self.build_window();
        // 行の高さが分かったら、その高さで窓を引き直す。
        if self.settle_row_height() {
            self.update_window();
        }
    }

    /// Rebuilds every `GtkListBox` row of the current window.
    fn build_window(&self) {
        self.build_window_from(None);
    }

    /// Builds the current window. The rows `previous` has in common with it stay as they
    /// are, unless the rows are replaced during the build.
    fn build_window_from(&self, previous: Option<RowWindow>) {
        // 組み立ての途中でアプリのコードが動くので、そこから呼ばれても
        // 二重に作り直さない。
        if self.0.rebuilding.replace(true) {
            return;
        }
        self.0.rows.build_until_settled(|replaced| {
            if replaced {
                self.0.window.set(self.compute_window(ROW_WINDOW_OVERSCAN));
            }
            self.build_window_once(previous.filter(|_| !replaced));
        });
        self.0.rebuilding.set(false);
    }

    fn build_window_once(&self, previous: Option<RowWindow>) {
        let window = self.0.window.get();
        let generation = self.0.rows.generation();
        let fixed_height = self.0.fixed_row_height.get();
        let focused = self.focused_row();
        let built = self.native_rows();
        let built_start = self.0.built_start.get();
        // 窓に残る行は触らない。作り直すと、中のコントロールのフォーカスが失われる。
        // 前回の組み立てが途中で終わっていたら、位置が合わないので全部作り直す。
        // 全行を組み立てるときも、そろえた高さから行ごとの高さへ戻すために作り直す。
        let kept = previous
            .filter(|previous| previous.len() == built.len() && !window.is_complete())
            .map_or(0..0, |previous| previous.overlap(window));
        let mut realized: Vec<ListRow> = {
            let rows = self.0.realized.borrow();
            let offsets =
                kept.start.saturating_sub(built_start)..kept.end.saturating_sub(built_start);
            rows.get(offsets).map_or(Vec::new(), <[ListRow]>::to_vec)
        };
        let mut before_kept = 0;
        without_signal(&self.0.native, &self.0.handler, || {
            for (offset, row) in built.iter().enumerate() {
                if kept.contains(&(built_start + offset)) {
                    continue;
                }
                // 中身を先に放す。アプリが持ち続ける行の中身は、次に組み立てるときに
                // 別の行へ入る。古い行に親として残っていると入れられない。
                if let Some(row) = row.downcast_ref::<gtk::ListBoxRow>() {
                    row.set_child(gtk::Widget::NONE);
                }
                self.0.native.remove(row);
            }
            for index in window.indices().filter(|index| !kept.contains(index)) {
                let Some(row) = self.0.rows.row(index) else {
                    continue;
                };
                // 差し替えられたら、この行も残りの窓ももう古い。
                if self.0.rows.generation() != generation {
                    break;
                }
                // 組み立てに失敗した行は、空の行にする。
                let row = row.unwrap_or_else(|_| ListRow::from_item(ListItem::new("")));
                let native = build_row(&row, Rc::downgrade(&self.0));
                if let Some(height) = fixed_height {
                    native.set_size_request(-1, height);
                }
                report_row_position(&native, index, window.count);
                if index < kept.start {
                    self.0.native.insert(&native, before_kept as i32);
                    realized.insert(before_kept, row);
                    before_kept += 1;
                } else {
                    self.0.native.append(&native);
                    realized.push(row);
                }
            }
            self.0.native.unselect_all();
        });
        *self.0.realized.borrow_mut() = realized;
        self.0.built_start.set(window.start);

        // `GtkListBox` の Page Up / Down は自分の座標をそのままスクロール位置として
        // 使う。上に詰め物があるとずれるので、そのときは渡さない (カーソルは動き、
        // スクロールは `GtkViewport` がフォーカスに付いてくる)。
        let adjustment = self.0.scroller.vadjustment();
        self.0
            .native
            .set_adjustment((window.start == 0).then_some(&adjustment));

        let height = self.0.row_height.get();
        set_spacer_height(&self.0.top_spacer, window.leading(height));
        set_spacer_height(&self.0.bottom_spacer, window.trailing(height));

        // 覚えている選択を、組み立て直した行へ写す。
        let picked = self.selection();
        without_signal(&self.0.native, &self.0.handler, || self.show(&picked));

        // 矢印キーで窓の端まで来たときに組み立て直しても、操作が続けられるように。
        // 取り除いた行にフォーカスが残ったままだと、キーがどこにも届かなくなる。
        if let Some(index) = focused.filter(|index| !kept.contains(index)) {
            let offset = match window.contains(index) {
                true => index - window.start,
                // スクロールバーで遠くへ送ったときは、窓の先頭から続ける。
                false => 0,
            };
            if let Some(row) = self.0.native.row_at_index(offset as i32) {
                self.grab_focus_in_place(&row);
            }
        }
    }

    /// The row holding the keyboard focus (itself, or a control inside it).
    fn focused_row(&self) -> Option<usize> {
        let focus = self.0.native.root()?.focus()?;
        let row = focus
            .ancestor(gtk::ListBoxRow::static_type())
            .and_downcast::<gtk::ListBoxRow>()
            .filter(|row| row.parent().as_ref() == Some(self.0.native.upcast_ref()))?;
        let offset = usize::try_from(row.index()).ok()?;
        Some(self.0.built_start.get() + offset)
    }

    /// Moves the focus to a row just built, without scrolling.
    ///
    /// The row has no position yet, and `GtkViewport` would scroll to where it believes
    /// the focused widget is (`scroll-to-focus`), jumping to the top of the list.
    fn grab_focus_in_place(&self, row: &gtk::ListBoxRow) {
        let viewport = self.0.scroller.child().and_downcast::<gtk::Viewport>();
        if let Some(viewport) = &viewport {
            viewport.set_scroll_to_focus(false);
        }
        row.grab_focus();
        if let Some(viewport) = &viewport {
            viewport.set_scroll_to_focus(true);
        }
    }

    /// Computes the range of rows to build from the current scroll position.
    fn compute_window(&self, overscan: usize) -> RowWindow {
        let adjustment = self.0.scroller.vadjustment();
        // 詰め物が窓の外の行と同じ高さを持つので、スクロール位置は
        // 「全行を作ったとき」と同じ座標になる。
        list_row_window(
            self.0.bin.sizing().height != naui_core::Length::Auto,
            self.len(),
            adjustment.value(),
            adjustment.page_size(),
            self.0.row_height.get(),
            overscan,
        )
    }

    /// Moves the window at the start of the next frame.
    ///
    /// **The scroll position can change from inside `GtkListBox`'s own handling.** Moving
    /// the cursor focuses the next row, `GtkViewport` scrolls to it, and only then does
    /// `GtkListBox` select that row. Rebuilding the rows in the middle removes the row it
    /// is about to select, so the selection lands on the wrong row.
    ///
    /// It waits for a frame rather than just for the handling to finish, because the
    /// frame lays the new rows out right after this, before any more input. A key
    /// arriving while a new row has no position yet would make `GtkViewport` scroll to
    /// the top of the list.
    fn queue_window_update(&self) {
        // 表示されていなければフレームが来ない。キーの処理の途中でもない。
        if !self.0.scroller.is_mapped() {
            self.update_window();
            return;
        }
        if self.0.window_update_queued.replace(true) {
            return;
        }
        let weak = Rc::downgrade(&self.0);
        self.0.scroller.add_tick_callback(move |_, _| {
            if let Some(inner) = weak.upgrade() {
                inner.window_update_queued.set(false);
                List(inner).update_window();
            }
            glib::ControlFlow::Break
        });
    }

    /// Moves the built range along with scrolling.
    ///
    /// Rebuilding for every row scrolled is heavy, so the window is reused until you
    /// have scrolled through half of the extra rows ([`ROW_WINDOW_OVERSCAN`]). Even then,
    /// only the rows that were not there are built.
    fn update_window(&self) {
        if self.0.rebuilding.get() {
            return;
        }
        let current = self.0.window.get();
        let needed = self.compute_window(ROW_WINDOW_OVERSCAN / 2);
        let next = self.compute_window(ROW_WINDOW_OVERSCAN);
        if next == current || keeps_row_window(current, needed, next) {
            return;
        }
        self.0.window.set(next);
        self.build_window_from(Some(current));
        if self.settle_row_height() {
            self.update_window();
        }
    }

    /// Finds the height of one row from the built rows. `true` if it changed.
    ///
    /// Row height is decided by the content, so it is not known until a row is built.
    /// **The tallest built row is used, and every built row is given that height.**
    /// The rows outside the window are filled in as "row count × this height", so rows
    /// of differing height (only some with `detail`, for instance) would make the
    /// scroll bar drift every time the window moves. A taller row appearing later grows
    /// the height once.
    fn settle_row_height(&self) -> bool {
        let rows = self.native_rows();
        let tallest = rows
            .iter()
            .map(|row| row.measure(gtk::Orientation::Vertical, -1).1 as f64)
            .fold(0.0, f64::max);
        let previous = self.0.row_height.get();
        // 指定は下限。中身がそれより高い行は伸びるので、窓の計算もそちらに合わせる。
        let floor = f64::from(self.0.fixed_row_height.get().unwrap_or(0));
        let measured = match self.0.row_height_stale.replace(false) {
            true => tallest,
            false => tallest.max(previous),
        }
        .max(floor);
        if measured <= 0.0 {
            return false;
        }
        let window = self.0.window.get();
        // 行が少なければ行ごとの高さのまま (窓の外の行が無いので、ずれない)。
        if !window.is_complete() {
            for row in &rows {
                row.set_size_request(-1, measured as i32);
            }
        }
        self.0.row_height.set(measured);
        set_spacer_height(&self.0.top_spacer, window.leading(measured));
        set_spacer_height(&self.0.bottom_spacer, window.trailing(measured));
        (previous - measured).abs() >= 0.5
    }

    /// The rows the `GtkListBox` has now.
    fn native_rows(&self) -> Vec<gtk::Widget> {
        let mut rows = Vec::with_capacity(self.0.window.get().len());
        let mut child = self.0.native.first_child();
        while let Some(row) = child {
            child = row.next_sibling();
            rows.push(row);
        }
        rows
    }

    /// Scrolls to the position computed from the row height. `false` if the height is
    /// not known or the scroll range does not reach the row yet.
    fn reveal_computed(&self, index: usize) -> bool {
        let height = self.0.row_height.get();
        if height <= 0.0 {
            return false;
        }
        let top = index as f64 * height;
        if top + height > self.0.scroller.vadjustment().upper() + 0.5 {
            return false;
        }
        reveal_range(&self.0.scroller, top, top + height);
        true
    }

    /// Normalizes the requested selection into a form meaningful for this list.
    fn normalize(&self, indices: &[usize]) -> Vec<usize> {
        self.selection_mode()
            .normalize_by(indices, |index| self.is_selectable(index))
    }

    /// Whether the row can be selected.
    fn is_selectable(&self, index: usize) -> bool {
        self.0
            .rows
            .is_selectable(index, ListRow::is_selectable, || {
                let offset = index.checked_sub(self.0.window.get().start)?;
                self.0
                    .realized
                    .borrow()
                    .get(offset)
                    .map(ListRow::is_selectable)
            })
    }

    /// Remembers the selection and applies it to the built rows.
    ///
    /// Rows outside the window have no `GtkListBoxRow`, so it cannot be applied to them.
    /// When the window moves and rows are rebuilt, it is applied again from the
    /// remembered selection.
    fn show(&self, picked: &[usize]) {
        *self.0.selected.borrow_mut() = picked.to_vec();
        let window = self.0.window.get();
        self.0.native.unselect_all();
        for &index in picked {
            if !window.contains(index) {
                continue;
            }
            if let Some(row) = self.0.native.row_at_index((index - window.start) as i32) {
                self.0.native.select_row(Some(&row));
            }
        }
    }

    /// Reads the selection the user changed. How rows outside the window are treated is
    /// decided here too.
    fn read_native_selection(&self) -> Vec<usize> {
        let window = self.0.window.get();
        let picked = self
            .0
            .native
            .selected_rows()
            .iter()
            .map(|row| window.start + row.index().max(0) as usize)
            .collect();
        let previous = self.0.selected.borrow().clone();
        // 単一選択では、窓の外の行を残すと 2 行選ばれてしまう。
        let gesture = match self.selection_mode().is_multiple() {
            true => self.0.gesture.get(),
            false => SelectionGesture::Plain,
        };
        merge_hidden_selection(window, &previous, picked, gesture)
    }
}

/// Gives the spacer the height of the rows outside the window.
fn set_spacer_height(spacer: &gtk::Box, height: f64) {
    let height = height.max(0.0).min(i32::MAX as f64) as i32;
    spacer.set_size_request(-1, height);
    spacer.set_visible(height > 0);
}

/// The content of a text-only row.
///
/// The smallness and dimness of the secondary text come from
/// [`TextStyle::Caption`] and [`TextColor::Secondary`], so no style classes are
/// written here.
pub(crate) fn item_content(label: &str, detail: Option<&str>) -> Stack {
    let content = Stack::new(Orientation::Vertical);
    content.set_align(Align::Fill);
    content.set_spacing(DETAIL_SPACING);
    content.append(&row_label(label));
    if let Some(detail) = detail {
        let sub = row_label(detail);
        sub.set_style(TextStyle::Caption);
        sub.set_color(TextColor::Secondary);
        content.append(&sub);
    }
    content
}

/// One piece of text placed in a row.
///
/// **It stretches to the full width of the row.** `GtkLabel` cuts the end with
/// `PangoEllipsizeMode::End`, but that works only once the width is decided. Even
/// when stretched, the text stays left-aligned (`Label` sets `xalign` to 0).
fn row_label(text: &str) -> Label {
    let label = Label::new(text);
    label.set_sizing(Sizing::fill_width());
    label
}

/// Applies the same look as a `List`'s secondary text (small and dim) to a raw
/// `GtkLabel`.
///
/// For places where naui's `Label` cannot be used (such as table headings, which
/// deal with widths and `GtkSizeGroup` directly). The classes applied are taken
/// from [`TextStyle::Caption`] and [`TextColor::Secondary`].
pub(crate) fn apply_caption(label: &gtk::Label) {
    for class in [
        TextStyle::Caption.style_class(),
        TextColor::Secondary.style_class(),
    ]
    .into_iter()
    .flatten()
    {
        label.add_css_class(class);
    }
}

/// Builds one row. Both `ListItem`s and custom content go through the same path.
/// Tells assistive technology a row's place among all the rows and how many there are.
///
/// The `GtkListBox` has only the rows built, so it would answer with those alone.
pub(crate) fn report_row_position(row: &gtk::ListBoxRow, index: usize, count: usize) {
    row.update_relation(&[
        gtk::accessible::Relation::PosInSet(i32::try_from(index + 1).unwrap_or(i32::MAX)),
        gtk::accessible::Relation::SetSize(i32::try_from(count).unwrap_or(i32::MAX)),
    ]);
}

fn build_row(item: &ListRow, list: Weak<ListInner>) -> gtk::ListBoxRow {
    let content: SizeBin = match &item.content {
        ListRowContent::Item(item) => item_content(&item.label, item.detail.as_deref()).size_bin(),
        ListRowContent::Custom(content) => content.size_bin(),
    };
    // 行の幅いっぱいに置く。`SizeBin` の既定は `Center` (中身の大きさに合わせる)
    // なので、そのままでは文字が行の真ん中へ寄る。大きさを自分で指定している
    // 中身はそちらが優先される。
    content.fill_parent();
    content.set_margin_top(6);
    content.set_margin_bottom(6);
    content.set_margin_start(10);
    content.set_margin_end(10);

    let row = crate::row::new();
    row.set_child(Some(&content));
    row.set_selectable(item.selectable);
    row.set_activatable(item.selectable || item.activation.is_some());
    if matches!(&item.content, ListRowContent::Item(item) if !item.enabled) {
        // 無効の行にはジェスチャーも届かないので、起動も文脈メニューも出ない。
        row.set_sensitive(false);
    }
    row.add_controller(row_double_click_controller(list.clone()));
    row.add_controller(row_context_menu_controller(list));
    row
}

/// The gesture that receives double-clicks on a row.
///
/// It does not claim the sequence (passing it on to `GtkListBox`'s own selection
/// gesture).
fn row_double_click_controller(list: Weak<ListInner>) -> gtk::GestureClick {
    let gesture = gtk::GestureClick::new();
    gesture.set_button(gdk::BUTTON_PRIMARY);
    gesture.connect_pressed(move |gesture, presses, _, _| {
        if presses != 2 {
            return;
        }
        let Some((inner, _, index)) = pressed_row(&list, gesture) else {
            return;
        };
        inner.row_activate.emit(index);
    });
    gesture
}

/// The gesture that receives right-clicks on a row.
fn row_context_menu_controller(list: Weak<ListInner>) -> gtk::GestureClick {
    let gesture = gtk::GestureClick::new();
    gesture.set_button(gdk::BUTTON_SECONDARY);
    gesture.connect_pressed(move |gesture, _, x, y| {
        let Some((inner, row, index)) = pressed_row(&list, gesture) else {
            return;
        };
        // `PopupMenu::open_at(&list, ..)` は `GtkListBox` の座標で受けるので、
        // 行の座標からそこへ直す。
        let local = graphene::Point::new(x as f32, y as f32);
        let Some(at) = row.compute_point(&inner.native, &local) else {
            return;
        };
        // ほかの右クリックの受け口 (祖先に付けた `PopupMenu` など) と
        // 二重にメニューを出さないよう、通知先があるときはここで握る。
        if inner.context_menu.is_set() {
            gesture.set_state(gtk::EventSequenceState::Claimed);
        }
        inner
            .context_menu
            .emit((index, Point::new(f64::from(at.x()), f64::from(at.y()))));
    });
    gesture
}

/// The row the gesture is attached to, and that row's index.
///
/// The index is looked up from the position at the time of the press (so it stays
/// correct even if the rows are rebuilt).
fn pressed_row(
    list: &Weak<ListInner>,
    gesture: &gtk::GestureClick,
) -> Option<(Rc<ListInner>, gtk::ListBoxRow, usize)> {
    let inner = list.upgrade()?;
    let row = gesture
        .widget()
        .and_downcast::<gtk::ListBoxRow>()
        .filter(|row| row.is_sensitive())?;
    let index = inner.window.get().start + usize::try_from(row.index()).ok()?;
    Some((inner, row, index))
}

/// A key receiver that picks up Enter while a row has focus, as a row activation.
///
/// `GtkListBox`'s own Enter (`activate-cursor-row`) still works as before
/// (selection and [`ListRow::on_activate`]). Enter while a field or button inside
/// the row has focus belongs to that control, so it is not picked up.
fn enter_controller(list: Weak<ListInner>) -> gtk::EventControllerKey {
    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    keys.connect_key_pressed(move |_, key, _, state| {
        let enter = matches!(
            key,
            gdk::Key::Return | gdk::Key::KP_Enter | gdk::Key::ISO_Enter
        );
        let modifiers = gdk::ModifierType::SHIFT_MASK
            | gdk::ModifierType::CONTROL_MASK
            | gdk::ModifierType::ALT_MASK
            | gdk::ModifierType::SUPER_MASK
            | gdk::ModifierType::META_MASK;
        if !enter || state.intersects(modifiers) {
            return glib::Propagation::Proceed;
        }
        let Some(inner) = list.upgrade() else {
            return glib::Propagation::Proceed;
        };
        let focused = inner
            .native
            .root()
            .and_then(|root| root.focus())
            .and_downcast::<gtk::ListBoxRow>()
            .filter(|row| row.parent().as_ref() == Some(inner.native.upcast_ref()));
        if let Some(row) = focused.filter(|row| row.is_sensitive()) {
            if let Ok(offset) = usize::try_from(row.index()) {
                inner.row_activate.emit(inner.window.get().start + offset);
            }
        }
        glib::Propagation::Proceed
    });
    keys
}

/// Moves the vertical scroll to where `top..bottom` (content coordinates) is
/// visible.
///
/// If it is visible nothing moves; if it sticks out above, the top is aligned, and
/// if below, the bottom.
pub(crate) fn reveal_range(scroller: &gtk::ScrolledWindow, top: f64, bottom: f64) {
    let adjustment = scroller.vadjustment();
    let view_top = adjustment.value();
    let page = adjustment.page_size();
    if top < view_top {
        adjustment.set_value(top);
    } else if bottom > view_top + page {
        adjustment.set_value(bottom - page);
    }
}

/// Makes `row` (a descendant of the scroll area's content) visible.
///
/// Right after the rows are rebuilt, or right after a collapsed branch is
/// expanded, no space has been allotted yet, so it tries again on the next frame.
///
/// If `after_layout` is `true`, it does not try on the spot but after the next
/// layout has finished. A row right after its collapsed branch is expanded reports
/// a bogus position from before allocation (a few px high), so the immediate value
/// cannot be trusted.
pub(crate) fn reveal_widget(
    scroller: &gtk::ScrolledWindow,
    row: &impl IsA<gtk::Widget>,
    after_layout: bool,
) {
    fn attempt(scroller: &gtk::ScrolledWindow, row: &gtk::Widget) -> bool {
        // `GtkViewport` は中身をスクロールのぶんずらして置くので、ビューポート
        // 基準では「見えている範囲の中」の位置になる。中身の座標で測るため、
        // ビューポートの子を基準にする。
        let Some(content) = scroller.child().and_then(|viewport| viewport.first_child()) else {
            return false;
        };
        let Some(bounds) = row.compute_bounds(&content) else {
            return false;
        };
        if bounds.height() <= 0.0 {
            return false;
        }
        let top = f64::from(bounds.y());
        let bottom = top + f64::from(bounds.height());
        // 閉じていた枝の行は、前に配られた古い場所を持ったままのことがある。
        // 中身の高さがまだ伸びていなければ、レイアウトの前とみなして待つ。
        if bottom > scroller.vadjustment().upper() + 0.5 {
            return false;
        }
        reveal_range(scroller, top, bottom);
        true
    }
    let row = row.as_ref().clone();
    if !after_layout && attempt(scroller, &row) {
        return;
    }
    // フレームの最初 (tick) はレイアウトより前に来るので、1 回目の tick は
    // まだ古い場所を見る。2 回目からは前のフレームのレイアウトが済んでいる。
    // 数フレーム試してだめなら諦める。
    let frames = std::cell::Cell::new(0u8);
    scroller.add_tick_callback(move |scroller, _| {
        frames.set(frames.get() + 1);
        if frames.get() < 2 {
            return glib::ControlFlow::Continue;
        }
        if attempt(scroller, &row) || frames.get() >= REVEAL_FRAMES {
            glib::ControlFlow::Break
        } else {
            glib::ControlFlow::Continue
        }
    });
}

/// The maximum number of frames to wait for a row's position to be allotted.
const REVEAL_FRAMES: u8 = 5;
