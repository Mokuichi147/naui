//! Lists (DOM).
//!
//! **The structure depends on the rows' contents.**
//!
//! | Rows | Built from |
//! | --- | --- |
//! | Text only | `<select size>` + `<option>` — the browser's standard list box itself |
//! | With `detail` / custom content | `<ul role="listbox">` + `<li role="option">` — composed |
//!
//! The content model of `<option>` is **text only**; it can hold neither elements
//! nor line breaks. Showing two-line rows means leaving `<select>`. Still,
//! `<select>` is a real control whose selection, keyboard navigation and scrolling
//! the browser takes care of, so **it keeps being used when that is not needed
//! (text-only rows)**.
//!
//! The composed one also uses the browser's system colors for the frame and the
//! selection (`Field` / `SelectedItem` / `Highlight` / `GrayText`) as is; naui
//! decides no colors.
//!
//! Row activation (`dblclick` and Enter) and right-clicks (`contextmenu`) are
//! received together on the outer frame, which stays the same even when the
//! contents are swapped, and the row is looked up from the clicked element.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use naui_core::{
    Align, ListItem, Orientation, Point, Result, SelectionMode, Sizing, TextColor, TextStyle,
};
use wasm_bindgen::JsCast;
use web_sys::{
    Document, Element, Event, HtmlElement, HtmlOptionElement, HtmlSelectElement, KeyboardEvent,
    MouseEvent,
};

use crate::widgets::{create, impl_widget, Label, Listener, Stack, ValueHandler, Widget};

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
/// [`ListItem`] is a convenience API for lists that need only strings. Rows that
/// combine icons, several labels, check boxes, trailing buttons and so on, as in a
/// settings screen, are built with `Grid` / `Stack` and wrapped in a `ListRow`.
///
/// ```no_run
/// # use naui_web::{ListRow, Widget};
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
    /// It is not called when a control such as a check box, button or text field is
    /// clicked directly, so the same action does not fire twice.
    pub fn on_activate(&self, f: impl FnMut() + 'static) {
        self.activation.set(f);
    }

    /// A row that can be neither selected nor operated (`ListItem::enabled(false)`).
    ///
    /// Custom-content rows are not dimmed, because the controls inside can still be
    /// clicked even when the whole row cannot be selected.
    fn is_dimmed(&self) -> bool {
        matches!(&self.content, ListRowContent::Item(item) if !item.enabled)
    }
}

/// Controls inside a row, distinguished from operations on the row itself.
///
/// The distinction the other platforms make through hit-testing and event routing
/// is made in the DOM by walking up the ancestors of the clicked element.
/// `<label>` operates the input inside it, so it is included here.
pub(crate) const ROW_CONTROLS: &str = "button, input, select, textarea, a[href], label, summary, \
[role=\"button\"], [contenteditable=\"true\"]";

/// Fields inside a row that keep the browser's own context menu (cut, paste and so
/// on). Right-clicking them does not become the row's context menu.
const TEXT_ENTRIES: &str = "input, textarea, [contenteditable=\"true\"]";

/// Whether the event was aimed at a control inside the row.
///
/// Used for key operations as well as clicks. Pass the row as `container` for
/// clicks, and the list itself for key operations.
pub(crate) fn control_was_targeted(container: &HtmlElement, event: &Event) -> bool {
    let Some(target) = event.target().and_then(|t| t.dyn_into::<Element>().ok()) else {
        return false;
    };
    let Ok(Some(control)) = target.closest(ROW_CONTROLS) else {
        return false;
    };
    // 内側で見つかったときだけ、コントロールの操作とみなす。
    !control.is_same_node(Some(container.as_ref())) && container.contains(Some(control.as_ref()))
}

/// The minimum `size`. 1 or less becomes a drop-down.
const MIN_ROWS: u32 = 2;

thread_local! {
    /// A per-list running number for referring to rows from
    /// `aria-activedescendant`.
    static NEXT_ID: Cell<u32> = const { Cell::new(0) };
}

fn next_list_id() -> u32 {
    NEXT_ID.with(|n| {
        let id = n.get();
        n.set(id + 1);
        id
    })
}

/// The spacing between the label and the secondary text.
const DETAIL_SPACING: f64 = 2.0;

/// The content of a text-only row.
///
/// The smallness and dimness of the secondary text come from
/// [`TextStyle::Caption`] and [`TextColor::Secondary`], so no CSS is written here.
pub(crate) fn item_content(doc: &Document, label: &str, detail: Option<&str>) -> Result<Stack> {
    let content = Stack::new(doc, Orientation::Vertical)?;
    content.set_align(Align::Fill);
    content.set_spacing(DETAIL_SPACING);
    content.append(&row_label(doc, label)?);
    if let Some(detail) = detail {
        let sub = row_label(doc, detail)?;
        sub.set_style(TextStyle::Caption);
        sub.set_color(TextColor::Secondary);
        content.append(&sub);
    }
    Ok(content)
}

/// One piece of text placed in a row.
///
/// **It stretches to the full width of the row.** By default `Label` does not wrap
/// and cuts the end with an ellipsis, but that works only once the width is
/// decided; at the content's width, a long label would stick out of the row
/// horizontally.
fn row_label(doc: &Document, text: &str) -> Result<Label> {
    let label = Label::new(doc, text)?;
    label.set_sizing(Sizing::fill_width());
    Ok(label)
}

fn style(element: &HtmlElement, property: &str, value: &str) {
    let _ = element.style().set_property(property, value);
}

/// The handler notified when the selection changes.
///
/// To use the same shape for single and multiple selection, the selected rows are
/// passed in ascending order. The closure is taken out only for the duration of the
/// call, so operating on the list from inside the callback does not double-borrow.
#[derive(Clone, Default)]
struct SelectionHandler(Rc<RefCell<Option<Box<dyn FnMut(&[usize])>>>>);

impl SelectionHandler {
    fn set(&self, f: impl FnMut(&[usize]) + 'static) {
        *self.0.borrow_mut() = Some(Box::new(f));
    }

    fn emit(&self, indices: &[usize]) {
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

/// The body currently in use. [`List::set_items`] chooses again by looking at the
/// rows' contents.
enum Body {
    /// The browser's standard list box.
    Select {
        select: HtmlSelectElement,
        /// The `change` and `click` subscriptions. Dropping them also unsubscribes.
        _listeners: Vec<Listener>,
    },
    /// A composition for showing two-line rows.
    Listbox {
        list: HtmlElement,
        options: Vec<HtmlElement>,
        /// The subscriptions for clicks on each row and key operations on the whole list.
        _listeners: Vec<Listener>,
    },
}

struct ListInner {
    /// The frame visible from outside. This element stays the same even when the
    /// contents are swapped.
    root: HtmlElement,
    document: Document,
    id: u32,
    body: RefCell<Body>,
    /// The rows. Whether they can be selected and their activation are looked up from
    /// here. It also keeps the event subscriptions and the like contained in the rows
    /// alive.
    rows: RefCell<Vec<ListRow>>,
    mode: Cell<SelectionMode>,
    /// The selected rows (ascending). This is the source of truth for both `<select>`
    /// and the composed list.
    selected: RefCell<Vec<usize>>,
    /// The row the keyboard is pointing at now. Used only for the composed list.
    active: Cell<Option<usize>>,
    /// The anchor of a Shift range selection.
    anchor: Cell<Option<usize>>,
    handler: SelectionHandler,
    /// The handler for double-clicks and Enter.
    row_activate: ValueHandler<usize>,
    /// The handler for right-clicks.
    context_menu: ValueHandler<(usize, Point)>,
    /// The `dblclick` / `contextmenu` / `keydown` subscriptions on the outer frame.
    _row_listeners: RefCell<Vec<Listener>>,
}

/// A vertical list of selectable rows.
///
/// Text-only rows become a `<select size>`; rows with `detail` become a
/// `<ul role="listbox">`. An `Auto` height follows the number and content of the
/// rows; with a fixed height or `Fill`, it scrolls whatever overflows.
#[derive(Clone)]
pub struct List(Rc<ListInner>);
impl_widget!(List, root);

impl List {
    pub(crate) fn new(doc: &Document) -> Result<Self> {
        let root: HtmlElement = create(doc, "div")?.unchecked_into();
        // 中身を枠いっぱいに広げる。枠は入れ替わらないので、
        // `set_sizing` の指定も親への追加もそのまま生き続ける。
        style(&root, "display", "flex");
        style(&root, "min-height", "0");

        let this = Self(Rc::new(ListInner {
            root,
            document: doc.clone(),
            id: next_list_id(),
            body: RefCell::new(Body::Select {
                select: create(doc, "select")?.unchecked_into(),
                _listeners: Vec::new(),
            }),
            rows: RefCell::new(Vec::new()),
            mode: Cell::new(SelectionMode::Single),
            selected: RefCell::new(Vec::new()),
            active: Cell::new(None),
            anchor: Cell::new(None),
            handler: SelectionHandler::default(),
            row_activate: ValueHandler::default(),
            context_menu: ValueHandler::default(),
            _row_listeners: RefCell::new(Vec::new()),
        }));
        this.build_select(&[])?;
        this.listen_row_events()?;
        Ok(this)
    }

    /// Rebuilds the rows. Indices change meaning, so the selection is cleared.
    ///
    /// If even one row has `detail`, it is rebuilt as a `<ul role="listbox">`;
    /// otherwise as a `<select size>`.
    pub fn set_items(&self, items: &[ListItem]) {
        let rows: Vec<ListRow> = items.iter().cloned().map(ListRow::from_item).collect();
        self.set_rows(&rows);
    }

    /// Rebuilds the rows. Both plain text rows and custom-content rows are built
    /// through this path.
    ///
    /// Controls inside a row can have their own callbacks as usual. Indices change
    /// meaning, so the selection is cleared.
    pub fn set_rows(&self, rows: &[ListRow]) {
        *self.0.rows.borrow_mut() = rows.to_vec();
        self.0.selected.borrow_mut().clear();
        self.0.active.set(None);
        self.0.anchor.set(None);

        // `<option>` はテキストしか持てないので、文字だけの行がそろっている
        // ときだけブラウザ標準の `<select>` を使い続ける。
        let plain = rows
            .iter()
            .all(|row| matches!(&row.content, ListRowContent::Item(item) if item.detail.is_none()));
        let _ = if plain {
            self.build_select(rows)
        } else {
            self.build_listbox(rows)
        };
    }

    /// The number of rows.
    pub fn len(&self) -> usize {
        self.0.rows.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Changes how rows are selected. The meaning of the selection changes, so the
    /// selection is cleared.
    pub fn set_selection_mode(&self, mode: SelectionMode) {
        self.0.mode.set(mode);
        self.apply_mode();
        self.write_selection(&[]);
    }

    pub fn selection_mode(&self) -> SelectionMode {
        self.0.mode.get()
    }

    /// The topmost of the selected rows.
    pub fn selected(&self) -> Option<usize> {
        self.0.selected.borrow().first().copied()
    }

    /// The selected rows (ascending). Zero or one for single selection.
    pub fn selection(&self) -> Vec<usize> {
        self.0.selected.borrow().clone()
    }

    /// Scrolls until row `index` is visible. The selection does not change.
    ///
    /// If it is already visible nothing moves; otherwise it is aligned to the nearer
    /// edge (the top if it is above, the bottom if it is below). Does nothing if out of
    /// range.
    pub fn scroll_to_row(&self, index: usize) {
        self.reveal_row(index);
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
        self.write_selection(&picked);
    }

    /// Clears the whole selection without notifying.
    pub fn clear_selection(&self) {
        self.write_selection(&[]);
    }

    /// Selects one row through the same path as a user's selection (notifies).
    pub fn select(&self, index: usize) {
        self.select_many(&[index]);
    }

    /// Replaces the selection through the same path as a user's selection (notifies).
    pub fn select_many(&self, indices: &[usize]) {
        self.set_selection(indices);
        // ブラウザはプログラムからの変更でイベントを出さないため、
        // ここで 1 回だけ通知する。
        let actual = self.selection();
        self.0.handler.emit(&actual);
    }

    /// Called with the selected rows (ascending) when the selection changes.
    ///
    /// With multiple selection it may be called with none.
    pub fn on_select(&self, f: impl FnMut(&[usize]) + 'static) {
        self.0.handler.set(f);
    }

    /// Called with the row's index when a row is activated (double-clicked, or Enter
    /// while the list has focus).
    ///
    /// With Enter, the target is the row the keyboard points at (for `<select>`, the
    /// first selected row). To open everything in a multiple selection, read
    /// [`List::selection`] inside it. Not called for rows of `ListItem`s that cannot be
    /// selected (`enabled(false)`), nor over controls inside a row.
    pub fn on_row_activate(&self, f: impl FnMut(usize) + 'static) {
        self.0.row_activate.set(f);
    }

    /// Called with the row's index and the click position when a row is right-clicked.
    ///
    /// The position is in logical pixels from the top left of this list and can be
    /// passed straight to [`crate::PopupMenu::open_at`]. Once set, the browser's default
    /// menu is suppressed over rows (not over fields inside a row, which are neither
    /// suppressed nor notified). The selection does not change. Not called for empty
    /// space without rows.
    pub fn on_context_menu(&self, mut f: impl FnMut(usize, Point) + 'static) {
        self.0
            .context_menu
            .set(move |(index, point)| f(index, point));
    }

    /// The inner `<select>`. Public as a backend-specific escape hatch.
    ///
    /// When some row has `detail` it is a `<ul role="listbox">` instead, so this returns
    /// `None`. The frame itself can be obtained from [`Widget::native_element`].
    pub fn native_select(&self) -> Option<HtmlSelectElement> {
        match &*self.0.body.borrow() {
            Body::Select { select, .. } => Some(select.clone()),
            Body::Listbox { .. } => None,
        }
    }

    // ------------------------------------------------------------ 組み立て

    /// Creates the browser's standard `<select size>`. The structure for text-only
    /// rows.
    fn build_select(&self, rows: &[ListRow]) -> Result<()> {
        let select: HtmlSelectElement = create(&self.0.document, "select")?.unchecked_into();
        for row in rows {
            let ListRowContent::Item(item) = &row.content else {
                continue;
            };
            let option: HtmlOptionElement = create(&self.0.document, "option")?.unchecked_into();
            option.set_text_content(Some(&item.label));
            option.set_disabled(!item.enabled);
            let _ = select.append_child(&option);
        }
        // `size` が無いとドロップダウンになるので、必ず 2 以上を入れる。
        let size = u32::try_from(rows.len()).unwrap_or(u32::MAX).max(MIN_ROWS);
        select.set_size(size);

        let mut listeners = Vec::with_capacity(2);
        // 選択はブラウザが動かすので、その結果を読み取って通知するだけ。
        if let Ok(listener) = Listener::attach(select.as_ref(), "change", {
            let weak = Rc::downgrade(&self.0);
            move || {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                let list = List(inner);
                let picked = list.read_select_selection();
                *list.0.selected.borrow_mut() = picked.clone();
                list.0.handler.emit(&picked);
            }
        }) {
            listeners.push(listener);
        }
        // `<option>` の中はテキストだけで、行内のコントロールも無い。
        // 押された `<option>` を辿って、その行の activation を出す。
        if let Ok(listener) = Listener::attach_event(select.as_ref(), "click", {
            let weak = Rc::downgrade(&self.0);
            move |event| {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                let Some(index) = clicked_option_index(&event) else {
                    return;
                };
                let activation = inner
                    .rows
                    .borrow()
                    .get(index)
                    .map(|row| row.activation.clone());
                if let Some(activation) = activation {
                    activation.emit();
                }
            }
        }) {
            listeners.push(listener);
        }

        self.swap_body(select.as_ref())?;
        *self.0.body.borrow_mut() = Body::Select {
            select,
            _listeners: listeners,
        };
        self.apply_mode();
        Ok(())
    }

    /// Builds the `role="listbox"`. Both two-line rows and custom-content rows go
    /// through here.
    fn build_listbox(&self, rows: &[ListRow]) -> Result<()> {
        let list: HtmlElement = create(&self.0.document, "ul")?.unchecked_into();
        let _ = list.set_attribute("role", "listbox");
        // キーボードで入れるようにする。中の行は `aria-activedescendant` で指す。
        let _ = list.set_attribute("tabindex", "0");
        style(&list, "list-style", "none");
        style(&list, "margin", "0");
        style(&list, "padding", "0");
        style(&list, "overflow-y", "auto");
        // 行の `offsetTop` がこの要素を基準になるようにする
        // (`reveal_active` がスクロール位置を求めるのに使う)。
        style(&list, "position", "relative");
        // 枠と地の色は、ブラウザが入力欄に使うシステム色に任せる。
        // 線は `GrayText` にする。Safari (WebKit) の `ButtonBorder` は明暗
        // どちらの配色でも白に解決されるため、明るい配色では枠が消え、
        // 暗い配色では白く出すぎる (`SplitView` の仕切りや `Table` と同じ判断)。
        style(&list, "border", "1px solid");
        style(&list, "border-color", "GrayText");
        style(&list, "background-color", "Field");
        style(&list, "color", "FieldText");

        let mut options = Vec::with_capacity(rows.len());
        let mut listeners = Vec::with_capacity(rows.len() + 1);
        for (index, row) in rows.iter().enumerate() {
            let option: HtmlElement = create(&self.0.document, "li")?.unchecked_into();
            let _ = option.set_attribute("role", "option");
            let _ = option.set_attribute("id", &self.option_id(index));
            let _ = option.set_attribute("aria-selected", "false");
            match &row.content {
                ListRowContent::Item(item) => {
                    style(&option, "padding", "2px 4px");
                    let content =
                        item_content(&self.0.document, &item.label, item.detail.as_deref())?;
                    let _ = option.append_child(&content.native_element());
                }
                ListRowContent::Custom(content) => {
                    style(&option, "padding", "6px 10px");
                    let _ = option.append_child(&content.native_element());
                }
            }

            if !row.selectable {
                let _ = option.set_attribute("aria-disabled", "true");
            }
            if row.is_dimmed() {
                // 無効な文字にブラウザが使うシステム色。
                style(&option, "color", "GrayText");
                // 行そのものも中身も操作できないので、購読も付けない。
                let _ = list.append_child(&option);
                options.push(option);
                continue;
            }

            let selectable = row.selectable;
            listeners.push(Listener::attach_event(option.as_ref(), "click", {
                let weak = Rc::downgrade(&self.0);
                let option = option.clone();
                let list = list.clone();
                move |event| {
                    let Some(inner) = weak.upgrade() else {
                        return;
                    };
                    // Button や input 自身の操作はそのまま通し、行そのものが
                    // 押されたときだけ activation を出す。
                    if !control_was_targeted(&option, &event) {
                        // クリックの続きを矢印キーで行えるように、
                        // リストへフォーカスを移す (行内のコントロールを
                        // 押したときは、そのコントロールから奪わない)。
                        let _ = list.focus();
                        let activation = inner
                            .rows
                            .borrow()
                            .get(index)
                            .map(|row| row.activation.clone());
                        if let Some(activation) = activation {
                            activation.emit();
                        }
                    }
                    if !selectable {
                        return;
                    }
                    // 行選択は通常のクリックと同じ規則で更新する。
                    let mouse = event.dyn_ref::<MouseEvent>();
                    let toggle = mouse.is_some_and(|e| e.meta_key() || e.ctrl_key());
                    let extend = mouse.is_some_and(|e| e.shift_key());
                    List(inner).on_row_activated(index, toggle, extend);
                }
            })?);

            let _ = list.append_child(&option);
            options.push(option);
        }

        listeners.push(Listener::attach_event(list.as_ref(), "keydown", {
            let weak = Rc::downgrade(&self.0);
            let list = list.clone();
            move |event| {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                // 行内のコントロールが受けたキーは、そのコントロールのもの。
                // リスト操作として横取りすると、チェックボックスの Space や
                // 入力欄の矢印キーが動かなくなる。
                if control_was_targeted(&list, &event) {
                    return;
                }
                if let Some(key) = event.dyn_ref::<KeyboardEvent>() {
                    if List(inner).on_key(key) {
                        event.prevent_default();
                    }
                }
            }
        })?);

        self.swap_body(&list)?;
        *self.0.body.borrow_mut() = Body::Listbox {
            list,
            options,
            _listeners: listeners,
        };
        self.apply_mode();
        Ok(())
    }

    /// Receives row activation and right-clicks on the outer frame. Not reattached when
    /// the contents are rebuilt.
    fn listen_row_events(&self) -> Result<()> {
        let root: &web_sys::EventTarget = self.0.root.as_ref();
        let mut listeners = Vec::with_capacity(3);
        listeners.push(Listener::attach_event(root, "dblclick", {
            let weak = Rc::downgrade(&self.0);
            move |event| {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                let list = List(inner);
                if let Some((index, row)) = list.row_of_event(&event) {
                    if !control_was_targeted(&row, &event) {
                        list.0.row_activate.emit(index);
                    }
                }
            }
        })?);
        listeners.push(Listener::attach_event(root, "contextmenu", {
            let weak = Rc::downgrade(&self.0);
            move |event| {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                let list = List(inner);
                if !list.0.context_menu.is_set() {
                    return;
                }
                let Some((index, row)) = list.row_of_event(&event) else {
                    return;
                };
                if entry_was_targeted(&row, &event) {
                    return;
                }
                let Some(mouse) = event.dyn_ref::<MouseEvent>() else {
                    return;
                };
                let rect = list.0.root.get_bounding_client_rect();
                let point = Point::new(
                    f64::from(mouse.client_x()) - rect.left(),
                    f64::from(mouse.client_y()) - rect.top(),
                );
                // ブラウザ既定のメニューも、`PopupMenu::attach` で同じ枠や
                // 祖先に付けたメニューも出さない (この購読は枠を作るときに
                // 付けるので、同じ枠のほかの購読より先に呼ばれる)。
                event.prevent_default();
                event.stop_immediate_propagation();
                list.0.context_menu.emit((index, point));
            }
        })?);
        listeners.push(Listener::attach_event(root, "keydown", {
            let weak = Rc::downgrade(&self.0);
            move |event| {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                let Some(key) = event.dyn_ref::<KeyboardEvent>() else {
                    return;
                };
                let modified = key.shift_key() || key.ctrl_key() || key.alt_key() || key.meta_key();
                if key.key() != "Enter" || key.is_composing() || modified {
                    return;
                }
                let list = List(inner);
                if let Some(index) = list.keyboard_row(&event) {
                    list.0.row_activate.emit(index);
                }
            }
        })?);
        *self.0._row_listeners.borrow_mut() = listeners;
        Ok(())
    }

    /// The row the event was aimed at, and that row's element. `None` for rows that
    /// receive neither activation nor right-clicks.
    ///
    /// The row's position is looked up from the clicked element on the spot (so it stays
    /// correct even if the rows are rebuilt).
    fn row_of_event(&self, event: &Event) -> Option<(usize, HtmlElement)> {
        let target = event.target()?.dyn_into::<Element>().ok()?;
        let (index, row) = match &*self.0.body.borrow() {
            Body::Select { .. } => {
                let option = target.closest("option").ok()??;
                let index = option.clone().dyn_into::<HtmlOptionElement>().ok()?.index();
                (usize::try_from(index).ok()?, option.dyn_into().ok()?)
            }
            Body::Listbox { options, .. } => {
                let option = target.closest("[role=\"option\"]").ok()??;
                let index = options
                    .iter()
                    .position(|row| row.is_same_node(Some(option.as_ref())))?;
                (index, options[index].clone())
            }
        };
        self.is_live(index).then_some((index, row))
    }

    /// The row activated by Enter: the row the keyboard points at, or the first
    /// selected row.
    fn keyboard_row(&self, event: &Event) -> Option<usize> {
        let index = match &*self.0.body.borrow() {
            Body::Select { select, .. } => usize::try_from(select.selected_index()).ok()?,
            Body::Listbox { list, .. } => {
                // 行内のコントロールが受けた Enter は、そのコントロールのもの。
                if control_was_targeted(list, event) {
                    return None;
                }
                self.0.active.get().or_else(|| self.selected())?
            }
        };
        self.is_live(index).then_some(index)
    }

    /// Whether the row can receive activation and right-clicks (not an unselectable
    /// `ListItem`).
    fn is_live(&self, index: usize) -> bool {
        self.0
            .rows
            .borrow()
            .get(index)
            .is_some_and(|row| !row.is_dimmed())
    }

    /// Swaps the contents of the frame.
    fn swap_body(&self, element: &HtmlElement) -> Result<()> {
        while let Some(child) = self.0.root.last_element_child() {
            let _ = self.0.root.remove_child(&child);
        }
        style(element, "flex", "1");
        style(element, "min-width", "0");
        self.0
            .root
            .append_child(element)
            .map(|_| ())
            .map_err(|e| crate::to_error("リストの組み立て", e))
    }

    fn option_id(&self, index: usize) -> String {
        format!("naui-list-{}-row-{index}", self.0.id)
    }

    /// Applies the single / multiple setting to the current body.
    fn apply_mode(&self) {
        let multiple = self.0.mode.get().is_multiple();
        match &*self.0.body.borrow() {
            Body::Select { select, .. } => select.set_multiple(multiple),
            Body::Listbox { list, .. } => {
                let _ = list.set_attribute(
                    "aria-multiselectable",
                    if multiple { "true" } else { "false" },
                );
            }
        }
    }

    // --------------------------------------------------------------- 選択

    /// Reads the `<select>`'s selection from the DOM.
    fn read_select_selection(&self) -> Vec<usize> {
        let body = self.0.body.borrow();
        let Body::Select { select, .. } = &*body else {
            return Vec::new();
        };
        let options = select.options();
        (0..options.length())
            .filter(|&i| {
                options
                    .get_with_index(i)
                    .and_then(|node| node.dyn_into::<HtmlOptionElement>().ok())
                    .is_some_and(|option| option.selected())
            })
            .map(|i| i as usize)
            .collect()
    }

    fn normalize(&self, indices: &[usize]) -> Vec<usize> {
        let rows = self.0.rows.borrow();
        self.0.mode.get().normalize_by(indices, |index| {
            rows.get(index).is_some_and(ListRow::is_selectable)
        })
    }

    /// Remembers the selection and writes it into the body as is (no notification).
    fn write_selection(&self, indices: &[usize]) {
        *self.0.selected.borrow_mut() = indices.to_vec();
        let body = self.0.body.borrow();
        match &*body {
            Body::Select { select, .. } => {
                let options = select.options();
                for i in 0..options.length() {
                    if let Some(option) = options
                        .get_with_index(i)
                        .and_then(|node| node.dyn_into::<HtmlOptionElement>().ok())
                    {
                        option.set_selected(indices.contains(&(i as usize)));
                    }
                }
            }
            Body::Listbox { options, .. } => {
                let rows = self.0.rows.borrow();
                for (index, option) in options.iter().enumerate() {
                    let picked = indices.contains(&index);
                    let _ = option
                        .set_attribute("aria-selected", if picked { "true" } else { "false" });
                    if picked {
                        // 選択の色はブラウザのシステム色をそのまま使う。
                        // 新しい名前に対応していれば、後の指定が勝つ。
                        style(option, "background-color", "Highlight");
                        style(option, "background-color", "SelectedItem");
                        style(option, "color", "HighlightText");
                        style(option, "color", "SelectedItemText");
                    } else {
                        style(option, "background-color", "");
                        style(option, "color", "");
                        if rows.get(index).is_some_and(ListRow::is_dimmed) {
                            style(option, "color", "GrayText");
                        }
                    }
                }
            }
        }
    }

    /// When a row of the composed list is clicked.
    fn on_row_activated(&self, index: usize, toggle: bool, extend: bool) {
        let multiple = self.0.mode.get().is_multiple();
        let picked = if multiple && extend {
            let anchor = self.0.anchor.get().unwrap_or(index);
            self.range(anchor, index)
        } else if multiple && toggle {
            let mut picked = self.selection();
            match picked.iter().position(|&i| i == index) {
                Some(at) => {
                    picked.remove(at);
                }
                None => picked.push(index),
            }
            self.0.anchor.set(Some(index));
            picked
        } else {
            self.0.anchor.set(Some(index));
            vec![index]
        };
        self.0.active.set(Some(index));
        self.commit(&picked);
    }

    /// Key operations. Returns `true` if handled (preventing the browser's default
    /// action).
    fn on_key(&self, event: &KeyboardEvent) -> bool {
        let len = self.len();
        if len == 0 {
            return false;
        }
        let current = self.0.active.get().or_else(|| self.selected());
        let target = match event.key().as_str() {
            "ArrowDown" => self.step(current, 1),
            "ArrowUp" => self.step(current, -1),
            "Home" => self.first_enabled(0, 1),
            "End" => self.first_enabled(len as isize - 1, -1),
            // 複数選択では Space で、いま指している行を入れたり外したりする。
            " " if self.0.mode.get().is_multiple() => {
                let Some(index) = current else {
                    return false;
                };
                self.on_row_activated(index, true, false);
                return true;
            }
            _ => return false,
        };
        let Some(target) = target else {
            return false;
        };
        self.0.active.set(Some(target));
        if self.0.mode.get().is_multiple() && event.shift_key() {
            let anchor = self.0.anchor.get().unwrap_or(target);
            let picked = self.range(anchor, target);
            self.commit(&picked);
        } else {
            self.0.anchor.set(Some(target));
            self.commit(&[target]);
        }
        true
    }

    /// Looks for the next selectable row from `from` in the direction of `step`.
    fn step(&self, from: Option<usize>, step: isize) -> Option<usize> {
        let start = match from {
            Some(index) => index as isize + step,
            None if step > 0 => 0,
            None => self.len() as isize - 1,
        };
        self.first_enabled(start, step)
    }

    /// Returns the first selectable row going from `start` in the direction of `step`.
    fn first_enabled(&self, start: isize, step: isize) -> Option<usize> {
        let rows = self.0.rows.borrow();
        let mut at = start;
        while at >= 0 && (at as usize) < rows.len() {
            if rows[at as usize].is_selectable() {
                return Some(at as usize);
            }
            at += step;
        }
        None
    }

    /// The selectable rows from `a` to `b`.
    fn range(&self, a: usize, b: usize) -> Vec<usize> {
        let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
        let rows = self.0.rows.borrow();
        (lo..=hi.min(rows.len().saturating_sub(1)))
            .filter(|&i| rows[i].is_selectable())
            .collect()
    }

    /// Settles the result of a user action and notifies.
    fn commit(&self, indices: &[usize]) {
        let picked = self.normalize(indices);
        self.write_selection(&picked);
        self.reveal_active();
        self.0.handler.emit(&picked);
    }

    /// Brings the row the keyboard points at into the scroll area.
    fn reveal_active(&self) {
        let Some(active) = self.0.active.get() else {
            return;
        };
        if let Body::Listbox { list, .. } = &*self.0.body.borrow() {
            let _ = list.set_attribute("aria-activedescendant", &self.option_id(active));
        }
        self.reveal_row(active);
    }

    /// Brings row `index` into the scroll area.
    fn reveal_row(&self, index: usize) {
        match &*self.0.body.borrow() {
            Body::Listbox { list, options, .. } => {
                if let Some(option) = options.get(index) {
                    reveal_within(list, option);
                }
            }
            Body::Select { select, .. } => reveal_option(select, index),
        }
    }
}

/// Whether the event was aimed at a field inside the row.
fn entry_was_targeted(row: &HtmlElement, event: &Event) -> bool {
    let Some(target) = event.target().and_then(|t| t.dyn_into::<Element>().ok()) else {
        return false;
    };
    let Ok(Some(entry)) = target.closest(TEXT_ENTRIES) else {
        return false;
    };
    row.contains(Some(entry.as_ref()))
}

/// Which row the clicked `<option>` is. `None` for clicks on the `<select>`'s empty
/// space.
fn clicked_option_index(event: &Event) -> Option<usize> {
    let target = event.target()?.dyn_into::<Element>().ok()?;
    let option = target.closest("option").ok()??;
    let index = option.dyn_into::<HtmlOptionElement>().ok()?.index();
    usize::try_from(index).ok()
}

/// Makes `row` visible inside the scroll area `container`.
///
/// If it is visible nothing moves; if it sticks out above, the top is aligned, and
/// if below, the bottom. The position is computed from the difference of the drawn
/// rectangles, so it does not depend on where the `offsetParent` is.
pub(crate) fn reveal_within(container: &HtmlElement, row: &HtmlElement) {
    let view = container.get_bounding_client_rect();
    let rect = row.get_bounding_client_rect();
    let top = rect.top() - view.top() + f64::from(container.scroll_top());
    reveal_range(container, top, top + rect.height());
}

/// Moves `container`'s vertical scroll to where `top..bottom` (content coordinates)
/// is visible.
pub(crate) fn reveal_range(container: &HtmlElement, top: f64, bottom: f64) {
    let scroll_top = f64::from(container.scroll_top());
    let visible = f64::from(container.client_height());
    if top < scroll_top {
        container.set_scroll_top(top.floor() as i32);
    } else if bottom > scroll_top + visible {
        container.set_scroll_top((bottom - visible).ceil() as i32);
    }
}

/// Brings row `index` of a `<select size>` into view.
///
/// WebKit rounds a `<select>`'s scroll position down to a row boundary, so scrolling
/// to the position computed from the rectangle leaves the last row half hidden. Row
/// heights are uniform, so the first row to show is decided in whole rows.
fn reveal_option(select: &web_sys::HtmlSelectElement, index: usize) {
    let count = select.length();
    if count == 0 || index >= count as usize {
        return;
    }
    // 行の高さは `<option>` から測る (全体の高さを行数で割ると、上下の
    // 余白のぶん小さくなり、切り下げで 1 行ずれる)。
    let measured = select
        .options()
        .item(index as u32)
        .map(|option| option.get_bounding_client_rect().height())
        .unwrap_or(0.0);
    let height = if measured > 0.0 {
        measured
    } else {
        f64::from(select.scroll_height()) / f64::from(count)
    };
    if height <= 0.0 {
        return;
    }
    let visible_rows = (f64::from(select.client_height()) / height)
        .floor()
        .max(1.0) as usize;
    let first = (f64::from(select.scroll_top()) / height).ceil() as usize;
    let target = if index < first {
        index
    } else if index >= first + visible_rows {
        index + 1 - visible_rows
    } else {
        return;
    };
    select.set_scroll_top((target as f64 * height).round() as i32);
}
