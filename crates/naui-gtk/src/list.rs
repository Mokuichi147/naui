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

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use gtk::prelude::*;
use gtk::{gdk, glib, graphene};
use naui_core::{Align, ListItem, Orientation, Point, SelectionMode, Sizing, TextColor, TextStyle};

use crate::bin::SizeBin;
use crate::callback::{Notifier, SelectionNotifier};
use crate::widgets::{impl_widget, without_signal, Label, Stack, Widget};

/// The spacing between the label and the secondary text.
const DETAIL_SPACING: f64 = 2.0;

/// The handler notified when a row is clicked.
///
/// Replacing the same row's callback during the call does not double-borrow.
#[derive(Clone, Default)]
pub(crate) struct ActivationHandler(Rc<RefCell<Option<Box<dyn FnMut()>>>>);

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
    _scroller: gtk::ScrolledWindow,
    bin: SizeBin,
    rows: RefCell<Vec<ListRow>>,
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
impl_widget!(List);

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
        scroller.set_child(Some(&native));

        let bin = SizeBin::wrap(&scroller);
        let inner = Rc::new(ListInner {
            native,
            _scroller: scroller,
            bin,
            rows: RefCell::new(Vec::new()),
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
                    let selection = list.selection();
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
                let Ok(index) = usize::try_from(row.index()) else {
                    return;
                };
                // 通知の中から `set_rows` を呼べるように、借りたまま呼ばない。
                let activation = inner
                    .rows
                    .borrow()
                    .get(index)
                    .map(|row| row.activation.clone());
                if let Some(activation) = activation {
                    activation.emit();
                }
            });
        }
        inner
            .native
            .add_controller(enter_controller(Rc::downgrade(&inner)));
        Self(inner)
    }

    /// Rebuilds the rows. Indices change meaning, so the selection is cleared.
    pub fn set_items(&self, items: &[ListItem]) {
        let rows: Vec<ListRow> = items.iter().cloned().map(ListRow::from_item).collect();
        self.set_rows(&rows);
    }

    /// Rebuilds the rows. Both plain rows and rows with custom content are drawn
    /// through this path.
    ///
    /// Controls inside a row can have their own callbacks as usual.
    pub fn set_rows(&self, rows: &[ListRow]) {
        without_signal(&self.0.native, &self.0.handler, || {
            while let Some(row) = self.0.native.first_child() {
                self.0.native.remove(&row);
            }
            for row in rows {
                self.0
                    .native
                    .append(&build_row(row, Rc::downgrade(&self.0)));
            }
            self.0.native.unselect_all();
        });
        *self.0.rows.borrow_mut() = rows.to_vec();
    }

    pub fn len(&self) -> usize {
        self.0.rows.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn set_selection_mode(&self, mode: SelectionMode) {
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
    pub fn selection(&self) -> Vec<usize> {
        let mut indices: Vec<usize> = self
            .0
            .native
            .selected_rows()
            .iter()
            .map(|row| row.index().max(0) as usize)
            .collect();
        indices.sort_unstable();
        indices
    }

    /// Scrolls until row `index` is visible. The selection does not change.
    ///
    /// If it is already visible nothing moves; otherwise it is aligned to the nearer
    /// edge (the top if it is above, the bottom if it is below). Does nothing if out of
    /// range.
    pub fn scroll_to_row(&self, index: usize) {
        let Ok(index) = i32::try_from(index) else {
            return;
        };
        if let Some(row) = self.0.native.row_at_index(index) {
            reveal_widget(&self.0._scroller, &row, false);
        }
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
        without_signal(&self.0.native, &self.0.handler, || {
            self.0.native.unselect_all();
        });
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

    /// Normalizes the requested selection into a form meaningful for this list.
    fn normalize(&self, indices: &[usize]) -> Vec<usize> {
        let rows = self.0.rows.borrow();
        self.selection_mode().normalize_by(indices, |index| {
            rows.get(index).is_some_and(ListRow::is_selectable)
        })
    }

    /// Applies the selection to the native side.
    fn show(&self, picked: &[usize]) {
        self.0.native.unselect_all();
        for &index in picked {
            if let Some(row) = self.0.native.row_at_index(index as i32) {
                self.0.native.select_row(Some(&row));
            }
        }
    }
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

    let row = gtk::ListBoxRow::new();
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
    let index = usize::try_from(row.index()).ok()?;
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
            if let Ok(index) = usize::try_from(row.index()) {
                inner.row_activate.emit(index);
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
