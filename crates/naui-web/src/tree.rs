//! Trees (DOM).
//!
//! Browsers have no standard "tree" control, so it is built following WAI-ARIA's
//! `role="tree"`.
//!
//! | Part | Element |
//! | --- | --- |
//! | The whole | `<ul role="tree" tabindex="0">` |
//! | Item | `<li role="treeitem" aria-expanded aria-selected>` |
//! | Sequence of children | `<ul role="group">` (nested inside the parent) |
//!
//! Nested `<ul>`s are created as they are, and expanding and collapsing is done by
//! switching `display`. That the expansion inside a collapsed branch stays as is
//! (showing as before when expanded again) comes from this structure, giving the
//! same behavior as macOS's `NSOutlineView`.
//!
//! The frame and selection colors use the browser's system colors (`Field` /
//! `SelectedItem` / `Highlight` / `GrayText`) as is; naui decides no colors.

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;

use naui_core::{Result, SelectionMode, TreeItem};
use wasm_bindgen::JsCast;
use web_sys::{Document, Element, HtmlElement, KeyboardEvent, MouseEvent};

use crate::widgets::{create, impl_widget, Listener, Widget};

/// The indentation per level (px).
const INDENT: f64 = 16.0;

thread_local! {
    /// A per-tree running number for referring to items from
    /// `aria-activedescendant`.
    static NEXT_ID: Cell<u32> = const { Cell::new(0) };
}

fn next_tree_id() -> u32 {
    NEXT_ID.with(|n| {
        let id = n.get();
        n.set(id + 1);
        id
    })
}

fn style(element: &HtmlElement, property: &str, value: &str) {
    let _ = element.style().set_property(property, value);
}

/// The handler notified when the selection changes. Called with the path of the
/// selected item (an empty path when the selection is cleared).
#[derive(Clone, Default)]
struct SelectionHandler(Rc<RefCell<Option<Box<dyn FnMut(&[usize])>>>>);

impl SelectionHandler {
    fn set(&self, f: impl FnMut(&[usize]) + 'static) {
        *self.0.borrow_mut() = Some(Box::new(f));
    }

    /// The closure is taken out only for the duration of the call, so operating on the
    /// tree from inside the notification does not double-borrow.
    fn emit(&self, path: &[usize]) {
        let Some(mut f) = self.0.borrow_mut().take() else {
            return;
        };
        f(path);
        let mut slot = self.0.borrow_mut();
        if slot.is_none() {
            *slot = Some(f);
        }
    }
}

/// The handler notified when expansion changes.
#[derive(Clone, Default)]
struct ExpandHandler(Rc<RefCell<Option<Box<dyn FnMut(&[usize], bool)>>>>);

impl ExpandHandler {
    fn set(&self, f: impl FnMut(&[usize], bool) + 'static) {
        *self.0.borrow_mut() = Some(Box::new(f));
    }

    fn emit(&self, path: &[usize], expanded: bool) {
        let Some(mut f) = self.0.borrow_mut().take() else {
            return;
        };
        f(path, expanded);
        let mut slot = self.0.borrow_mut();
        if slot.is_none() {
            *slot = Some(f);
        }
    }
}

/// One item on screen. In depth-first order (parents first).
struct Node {
    path: Vec<usize>,
    /// The `role="treeitem"`. The expansion and selection states are put on it.
    item: HtmlElement,
    /// One row holding the text and the expander mark. Clicking it selects.
    row: HtmlElement,
    /// The children's `role="group"`. `None` for leaves.
    group: Option<HtmlElement>,
    /// The expander mark. `None` for leaves.
    twisty: Option<HtmlElement>,
}

struct TreeInner {
    /// The `role="tree"` visible from outside. This element stays the same even when
    /// the items are rebuilt.
    root: HtmlElement,
    document: Document,
    id: u32,
    items: RefCell<Vec<TreeItem>>,
    nodes: RefCell<Vec<Node>>,
    /// The items remembered as expanded. Those inside collapsed branches remain too.
    expanded: RefCell<HashSet<Vec<usize>>>,
    /// The selected items, normalized ([`SelectionMode::normalize_paths`]). Those
    /// inside collapsed branches remain too.
    selected: RefCell<Vec<Vec<usize>>>,
    mode: Cell<SelectionMode>,
    /// The item the keyboard is pointing at now.
    active: RefCell<Option<Vec<usize>>>,
    /// The anchor of a Shift range selection.
    anchor: RefCell<Option<Vec<usize>>>,
    handler: SelectionHandler,
    expand: ExpandHandler,
    /// The subscriptions of rows and expander marks. Removed on rebuild.
    listeners: RefCell<Vec<Listener>>,
    /// The subscription for key operations on the whole tree.
    _keys: RefCell<Option<Listener>>,
    /// `on_file_drop`.
    file_drop: crate::file_drop::FileDrop,
}

/// A list whose nested items can be expanded and collapsed (`role="tree"`).
///
/// Items are referred to by the sequence of child indices from the root (a path).
/// `[0, 2]` is "the third child of the first root", and an empty path means "no
/// selection".
///
/// Its height is not determined by its contents, so set it with `set_sizing`.
#[derive(Clone)]
pub struct Tree(Rc<TreeInner>);
impl_widget!(Tree, root);

impl Tree {
    pub(crate) fn new(doc: &Document) -> Result<Self> {
        let root: HtmlElement = create(doc, "ul")?.unchecked_into();
        let _ = root.set_attribute("role", "tree");
        // キーボードで入れるようにする。中の項目は `aria-activedescendant` で指す。
        let _ = root.set_attribute("tabindex", "0");
        style(&root, "list-style", "none");
        style(&root, "margin", "0");
        style(&root, "padding", "0");
        style(&root, "overflow", "auto");
        // 項目の `offsetTop` がこの要素を基準になるようにする。
        style(&root, "position", "relative");
        // 枠と地の色は、ブラウザが入力欄に使うシステム色に任せる。
        // 線は `GrayText` にする。Safari (WebKit) の `ButtonBorder` は明暗
        // どちらの配色でも白に解決されるため、明るい配色では枠が消え、
        // 暗い配色では白く出すぎる (`SplitView` の仕切りや `Table` と同じ判断)。
        style(&root, "border", "1px solid");
        style(&root, "border-color", "GrayText");
        style(&root, "background-color", "Field");
        style(&root, "color", "FieldText");

        let this = Self(Rc::new(TreeInner {
            root,
            document: doc.clone(),
            id: next_tree_id(),
            items: RefCell::new(Vec::new()),
            nodes: RefCell::new(Vec::new()),
            expanded: RefCell::new(HashSet::new()),
            selected: RefCell::new(Vec::new()),
            mode: Cell::new(SelectionMode::Single),
            active: RefCell::new(None),
            anchor: RefCell::new(None),
            handler: SelectionHandler::default(),
            expand: ExpandHandler::default(),
            listeners: RefCell::new(Vec::new()),
            _keys: RefCell::new(None),
            file_drop: Default::default(),
        }));

        let keys = Listener::attach_event(this.0.root.as_ref(), "keydown", {
            let weak = Rc::downgrade(&this.0);
            move |event| {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                if let Some(key) = event.dyn_ref::<KeyboardEvent>() {
                    if Tree(inner).on_key(key) {
                        event.prevent_default();
                    }
                }
            }
        })?;
        *this.0._keys.borrow_mut() = Some(keys);
        Ok(this)
    }

    /// Rebuilds the items. Paths change meaning, so the selection is cleared.
    ///
    /// Expansion returns to what [`TreeItem::expanded`] says.
    pub fn set_items(&self, items: &[TreeItem]) {
        *self.0.items.borrow_mut() = items.to_vec();
        self.0.selected.borrow_mut().clear();
        *self.0.active.borrow_mut() = None;
        *self.0.anchor.borrow_mut() = None;
        let mut expanded = HashSet::new();
        TreeItem::walk(items, |path, item| {
            if item.expanded && !item.is_leaf() {
                expanded.insert(path.to_vec());
            }
        });
        *self.0.expanded.borrow_mut() = expanded;
        let _ = self.rebuild();
    }

    /// The total number of items, descendants included.
    pub fn len(&self) -> usize {
        TreeItem::count(&self.0.items.borrow())
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Switches between single and multiple selection. The selection is cleared
    /// without notifying.
    pub fn set_selection_mode(&self, mode: SelectionMode) {
        self.0.mode.set(mode);
        match mode.is_multiple() {
            true => {
                let _ = self.0.root.set_attribute("aria-multiselectable", "true");
            }
            false => {
                let _ = self.0.root.remove_attribute("aria-multiselectable");
            }
        }
        self.0.selected.borrow_mut().clear();
        *self.0.anchor.borrow_mut() = None;
        self.paint();
    }

    pub fn selection_mode(&self) -> SelectionMode {
        self.0.mode.get()
    }

    /// The path of the first selected item. `None` if nothing is selected.
    pub fn selected(&self) -> Option<Vec<usize>> {
        self.0.selected.borrow().first().cloned()
    }

    /// The paths of the selected items in the order the tree shows them, including
    /// items hidden inside collapsed branches.
    pub fn selection(&self) -> Vec<Vec<usize>> {
        self.0.selected.borrow().clone()
    }

    /// Scrolls until the item at `path` is visible. The selection does not change.
    ///
    /// If it is inside a collapsed branch, its ancestors are expanded first (as with
    /// [`set_selected`](Self::set_selected), expansion is not notified). If it is
    /// already visible nothing moves. Does nothing for a path that does not exist.
    pub fn scroll_to_item(&self, path: &[usize]) {
        if TreeItem::at(&self.0.items.borrow(), path).is_none() {
            return;
        }
        self.write_expanded(&path[..path.len().saturating_sub(1)], true);
        self.reveal_path(path);
    }

    /// Selects one item without notifying.
    ///
    /// Passing an unselectable item or a path that does not exist clears the
    /// selection. Items inside a collapsed branch are selected after their ancestors
    /// are expanded so they become visible.
    pub fn set_selected(&self, path: &[usize]) {
        self.write_selection(&[path.to_vec()]);
    }

    /// Replaces the selection without notifying. Paths that cannot be selected are
    /// left out, and the branches holding the rest are expanded.
    pub fn set_selection(&self, paths: &[Vec<usize>]) {
        self.write_selection(paths);
    }

    /// Clears the selection without notifying.
    pub fn clear_selection(&self) {
        self.write_selection(&[]);
    }

    /// Selects one item through the same path as a user's selection (notifies).
    pub fn select(&self, path: &[usize]) {
        self.select_many(&[path.to_vec()]);
    }

    /// Replaces the selection through the same path as a user's selection
    /// (notifies).
    pub fn select_many(&self, paths: &[Vec<usize>]) {
        self.write_selection(paths);
        // ブラウザはプログラムからの変更でイベントを出さないため、
        // ここで 1 回だけ通知する。
        self.notify();
    }

    /// Called with the path of the first selected item when the selection changes.
    ///
    /// When the selection is cleared, it is called with an empty path.
    pub fn on_select(&self, f: impl FnMut(&[usize]) + 'static) {
        self.0.handler.set(f);
    }

    /// Whether the item is expanded.
    ///
    /// Even an item hidden inside a collapsed branch returns `true` if it is
    /// remembered as expanded (reopening the parent shows it expanded as before).
    pub fn is_expanded(&self, path: &[usize]) -> bool {
        self.0.expanded.borrow().contains(path)
    }

    /// Changes expansion without notifying. Expanding also expands the ancestors.
    pub fn set_expanded(&self, path: &[usize], expanded: bool) {
        self.write_expanded(path, expanded);
    }

    /// Expands through the same path as a user's action (notifies).
    pub fn expand(&self, path: &[usize]) {
        self.toggle(path, true);
    }

    /// Collapses through the same path as a user's action (notifies).
    pub fn collapse(&self, path: &[usize]) {
        self.toggle(path, false);
    }

    /// Expands every branch without notifying.
    pub fn expand_all(&self) {
        self.set_all(true);
    }

    /// Collapses every branch without notifying.
    pub fn collapse_all(&self) {
        self.set_all(false);
    }

    /// Called with the item's path and its new state when its expansion changes.
    pub fn on_expand(&self, f: impl FnMut(&[usize], bool) + 'static) {
        self.0.expand.set(f);
    }

    /// The inner `role="tree"` element. Public as a backend-specific escape hatch.
    pub fn native_tree(&self) -> HtmlElement {
        self.0.root.clone()
    }

    // ------------------------------------------------------------ 組み立て

    /// Rebuilds the nesting of items in the DOM.
    fn rebuild(&self) -> Result<()> {
        while let Some(child) = self.0.root.last_element_child() {
            let _ = self.0.root.remove_child(&child);
        }
        self.0.nodes.borrow_mut().clear();
        self.0.listeners.borrow_mut().clear();

        let items = self.0.items.borrow().clone();
        let root: Element = self.0.root.clone().unchecked_into();
        self.build_level(&items, &mut Vec::new(), &root)?;
        self.paint();
        Ok(())
    }

    /// Builds one level of items at the same depth.
    fn build_level(
        &self,
        items: &[TreeItem],
        path: &mut Vec<usize>,
        parent: &Element,
    ) -> Result<()> {
        for (index, item) in items.iter().enumerate() {
            path.push(index);
            let node = self.build_item(item, path)?;
            let _ = parent.append_child(&node.item);
            if let Some(group) = node.group.clone() {
                let group: Element = group.unchecked_into();
                self.build_level(&item.children, path, &group)?;
            }
            self.0.nodes.borrow_mut().push(node);
            path.pop();
        }
        Ok(())
    }

    /// Creates the elements for one item.
    fn build_item(&self, item: &TreeItem, path: &[usize]) -> Result<Node> {
        let doc = &self.0.document;
        let element: HtmlElement = create(doc, "li")?.unchecked_into();
        let _ = element.set_attribute("role", "treeitem");
        let _ = element.set_attribute("id", &self.item_id(path));
        let _ = element.set_attribute("aria-selected", "false");
        style(&element, "list-style", "none");

        let row: HtmlElement = create(doc, "div")?.unchecked_into();
        style(&row, "display", "flex");
        style(&row, "align-items", "center");
        style(&row, "gap", "4px");
        style(&row, "padding", "2px 4px");
        // 深さは字下げで表す。`<ul>` の入れ子ではなく余白で付けるのは、
        // 選択の帯を行いっぱいに出すため。
        style(
            &row,
            "padding-inline-start",
            &format!("{}px", 4.0 + INDENT * (path.len().saturating_sub(1)) as f64),
        );

        let selectable = TreeItem::selectable(&self.0.items.borrow(), path);
        let twisty = match item.is_leaf() {
            true => {
                // 葉でも文字の左端をそろえるため、印と同じ幅を空ける。
                let spacer: HtmlElement = create(doc, "span")?.unchecked_into();
                style(&spacer, "width", "12px");
                style(&spacer, "flex", "none");
                let _ = row.append_child(&spacer);
                None
            }
            false => {
                let twisty: HtmlElement = create(doc, "span")?.unchecked_into();
                // 印そのものは飾りで、開閉の状態は `aria-expanded` が伝える。
                let _ = twisty.set_attribute("aria-hidden", "true");
                style(&twisty, "width", "12px");
                style(&twisty, "flex", "none");
                style(&twisty, "cursor", "pointer");
                style(&twisty, "user-select", "none");
                style(&twisty, "text-align", "center");
                let _ = row.append_child(&twisty);
                Some(twisty)
            }
        };

        let text: HtmlElement = create(doc, "span")?.unchecked_into();
        // `<option>` に合わせて 1 行に収める (2 行にすると行の高さがそろわない)。
        let label = match &item.detail {
            Some(detail) => format!("{} — {detail}", item.label),
            None => item.label.clone(),
        };
        text.set_text_content(Some(&label));
        style(&text, "white-space", "nowrap");
        let _ = row.append_child(&text);
        let _ = element.append_child(&row);

        if !selectable {
            let _ = element.set_attribute("aria-disabled", "true");
            // 無効な文字にブラウザが使うシステム色。
            style(&text, "color", "GrayText");
        }

        let group = match item.is_leaf() {
            true => None,
            false => {
                let group: HtmlElement = create(doc, "ul")?.unchecked_into();
                let _ = group.set_attribute("role", "group");
                style(&group, "list-style", "none");
                style(&group, "margin", "0");
                style(&group, "padding", "0");
                let _ = element.append_child(&group);
                Some(group)
            }
        };

        // 行を押すと選ぶ。印を押すと開閉する (選択は動かさない)。
        let mut listeners = Vec::new();
        if selectable {
            listeners.push(Listener::attach_event(row.as_ref(), "click", {
                let weak = Rc::downgrade(&self.0);
                let path = path.to_vec();
                move |event| {
                    let Some(inner) = weak.upgrade() else {
                        return;
                    };
                    // ⌘ / Ctrl は 1 つずつ足し引きし、Shift は起点からの範囲を選ぶ
                    // (`List` と同じ)。
                    let mouse = event.dyn_ref::<MouseEvent>();
                    let toggle = mouse.is_some_and(|e| e.meta_key() || e.ctrl_key());
                    let extend = mouse.is_some_and(|e| e.shift_key());
                    Tree(inner).on_row_clicked(&path, toggle, extend);
                }
            })?);
        }
        if let Some(twisty) = twisty.as_ref() {
            listeners.push(Listener::attach_event(twisty.as_ref(), "click", {
                let weak = Rc::downgrade(&self.0);
                let path = path.to_vec();
                move |event| {
                    // 行の選択まで届かせない。
                    event.stop_propagation();
                    if let Some(inner) = weak.upgrade() {
                        let tree = Tree(inner);
                        let expanded = tree.is_expanded(&path);
                        tree.toggle(&path, !expanded);
                    }
                }
            })?);
        }
        self.0.listeners.borrow_mut().extend(listeners);

        Ok(Node {
            path: path.to_vec(),
            item: element,
            row,
            group,
            twisty,
        })
    }

    fn item_id(&self, path: &[usize]) -> String {
        let path: Vec<String> = path.iter().map(|i| i.to_string()).collect();
        format!("naui-tree-{}-item-{}", self.0.id, path.join("-"))
    }

    // --------------------------------------------------------- 状態の反映

    /// Redraws the expansion and selection according to the current state.
    fn paint(&self) {
        let expanded = self.0.expanded.borrow();
        let selected: HashSet<Vec<usize>> = self.0.selected.borrow().iter().cloned().collect();
        for node in self.0.nodes.borrow().iter() {
            let open = expanded.contains(&node.path);
            if let Some(group) = &node.group {
                style(group, "display", if open { "block" } else { "none" });
            }
            if let Some(twisty) = &node.twisty {
                // 閉じた印は子のある向き (行の終わり側) を指す。
                twisty.set_text_content(Some(match (open, crate::is_rtl()) {
                    (true, _) => "▾",
                    (false, true) => "◂",
                    (false, false) => "▸",
                }));
                let _ = node
                    .item
                    .set_attribute("aria-expanded", if open { "true" } else { "false" });
            }

            let picked = selected.contains(&node.path);
            let _ = node
                .item
                .set_attribute("aria-selected", if picked { "true" } else { "false" });
            if picked {
                // 選択の色はブラウザのシステム色をそのまま使う。
                // 新しい名前に対応していれば、後の指定が勝つ。
                style(&node.row, "background-color", "Highlight");
                style(&node.row, "background-color", "SelectedItem");
                style(&node.row, "color", "HighlightText");
                style(&node.row, "color", "SelectedItemText");
            } else {
                style(&node.row, "background-color", "");
                style(&node.row, "color", "");
            }
        }
    }

    /// Remembers the selection from code and redraws (no notification).
    ///
    /// The branches holding the selected items are expanded, and the keyboard and
    /// Shift ranges carry on from the last of them.
    fn write_selection(&self, paths: &[Vec<usize>]) {
        let picked = self
            .0
            .mode
            .get()
            .normalize_paths(&self.0.items.borrow(), paths);
        for path in &picked {
            // 見えていないと選んだことが分からないので、祖先を開く。
            // 葉には開閉が無いので、開くのは親から上だけ。
            self.write_expanded(&path[..path.len().saturating_sub(1)], true);
        }
        if let Some(last) = picked.last() {
            *self.0.active.borrow_mut() = Some(last.clone());
            *self.0.anchor.borrow_mut() = Some(last.clone());
        }
        *self.0.selected.borrow_mut() = picked;
        self.paint();
        self.reveal_active();
    }

    /// Settles the selection made by a user's action, redraws and notifies.
    ///
    /// Unlike [`Tree::write_selection`] it expands nothing: the user can only pick
    /// visible items, and hidden ones kept by ⌘ / Ctrl stay hidden.
    fn commit(&self, paths: &[Vec<usize>]) {
        let picked = self
            .0
            .mode
            .get()
            .normalize_paths(&self.0.items.borrow(), paths);
        *self.0.selected.borrow_mut() = picked;
        self.paint();
        self.reveal_active();
        self.notify();
    }

    /// Notifies the first selected item (an empty path for none).
    fn notify(&self) {
        let actual = self.selected().unwrap_or_default();
        self.0.handler.emit(&actual);
    }

    /// Remembers the expansion state and redraws (no notification). Expanding also
    /// expands the ancestors.
    fn write_expanded(&self, path: &[usize], expanded: bool) {
        if TreeItem::at(&self.0.items.borrow(), path).is_none_or(|item| item.is_leaf()) {
            return;
        }
        {
            let mut set = self.0.expanded.borrow_mut();
            match expanded {
                true => {
                    for depth in 1..=path.len() {
                        set.insert(path[..depth].to_vec());
                    }
                }
                false => {
                    set.remove(path);
                }
            }
        }
        self.paint();
    }

    /// Changes the expansion state and notifies once.
    fn toggle(&self, path: &[usize], expanded: bool) {
        let before = self.is_expanded(path);
        self.write_expanded(path, expanded);
        if self.is_expanded(path) != before {
            self.0.expand.emit(path, expanded);
        }
    }

    /// Expands or collapses every branch at once (no notification).
    fn set_all(&self, expanded: bool) {
        let mut set = self.0.expanded.borrow_mut();
        set.clear();
        if expanded {
            TreeItem::walk(&self.0.items.borrow(), |path, item| {
                if !item.is_leaf() {
                    set.insert(path.to_vec());
                }
            });
        }
        drop(set);
        self.paint();
    }

    // --------------------------------------------------------- 操作の受け口

    /// When a row is clicked (or picked with the keyboard).
    ///
    /// With multiple selection, `toggle` (⌘ / Ctrl) adds or removes the item and
    /// keeps the rest, hidden items included, and `extend` (Shift) selects the
    /// visible items from the anchor to it. Otherwise only the item is selected.
    fn on_row_clicked(&self, path: &[usize], toggle: bool, extend: bool) {
        let multiple = self.0.mode.get().is_multiple();
        let picked = if multiple && extend {
            let anchor = self.0.anchor.borrow().clone();
            self.range(anchor.as_deref().unwrap_or(path), path)
        } else if multiple && toggle {
            let mut picked = self.selection();
            match picked.iter().position(|p| p == path) {
                Some(at) => {
                    picked.remove(at);
                }
                None => picked.push(path.to_vec()),
            }
            *self.0.anchor.borrow_mut() = Some(path.to_vec());
            picked
        } else {
            *self.0.anchor.borrow_mut() = Some(path.to_vec());
            vec![path.to_vec()]
        };
        *self.0.active.borrow_mut() = Some(path.to_vec());
        self.commit(&picked);
    }

    /// The visible items from `a` to `b`, both included. If `a` is hidden now (its
    /// branch was collapsed), only `b`.
    fn range(&self, a: &[usize], b: &[usize]) -> Vec<Vec<usize>> {
        let rows = self.visible_paths();
        let at = |path: &[usize]| rows.iter().position(|row| row == path);
        let Some(end) = at(b) else {
            return vec![b.to_vec()];
        };
        let start = at(a).unwrap_or(end);
        rows[start.min(end)..=start.max(end)].to_vec()
    }

    /// Key operations. Returns `true` if handled (preventing the browser's default
    /// action).
    ///
    /// Following WAI-ARIA's Tree View pattern, ↑↓ move between rows, → expands / moves
    /// to the child, and ← collapses / moves to the parent.
    fn on_key(&self, event: &KeyboardEvent) -> bool {
        let rows = self.visible_paths();
        if rows.is_empty() {
            return false;
        }
        let current = self
            .0
            .active
            .borrow()
            .clone()
            .or_else(|| self.selected())
            .filter(|path| rows.contains(path));
        let at = current
            .as_ref()
            .and_then(|path| rows.iter().position(|row| row == path));

        // 右から左では、開く・子へ進むのが左、閉じる・親へ戻るのが右になる。
        let key = match (event.key().as_str(), crate::is_rtl()) {
            ("ArrowLeft", true) => "ArrowRight".to_owned(),
            ("ArrowRight", true) => "ArrowLeft".to_owned(),
            (key, _) => key.to_owned(),
        };
        let multiple = self.0.mode.get().is_multiple();
        // 複数選択の Ctrl (⌘) + 矢印は、選択を変えずに指す項目だけを動かす
        // (そのあと Space で足し引きする)。Shift + 矢印は起点からの範囲を選ぶ。
        let only_move = multiple && (event.ctrl_key() || event.meta_key()) && !event.shift_key();
        let extend = multiple && event.shift_key();
        let go = |path: &[usize]| match only_move {
            true => {
                *self.0.active.borrow_mut() = Some(path.to_vec());
                self.reveal_active();
            }
            false => self.focus(path, extend),
        };
        match key.as_str() {
            "ArrowDown" => {
                let next = at.map_or(0, |at| (at + 1).min(rows.len() - 1));
                go(&rows[next]);
            }
            "ArrowUp" => {
                let next = at.map_or(rows.len() - 1, |at| at.saturating_sub(1));
                go(&rows[next]);
            }
            "Home" => go(&rows[0]),
            "End" => go(&rows[rows.len() - 1]),
            // 見えている項目をすべて足す。閉じた枝の中の選択はそのまま残す。
            "a" | "A"
                if multiple
                    && (event.ctrl_key() || event.meta_key())
                    && !event.shift_key()
                    && !event.alt_key() =>
            {
                let mut picked = self.selection();
                picked.extend(rows.iter().cloned());
                let picked = self
                    .0
                    .mode
                    .get()
                    .normalize_paths(&self.0.items.borrow(), &picked);
                if picked != self.selection() {
                    self.commit(&picked);
                }
            }
            "ArrowRight" => {
                let Some(path) = current else {
                    return false;
                };
                let branch =
                    TreeItem::at(&self.0.items.borrow(), &path).is_some_and(|item| !item.is_leaf());
                if !branch {
                    return false;
                }
                if self.is_expanded(&path) {
                    // 開いているなら、最初の子へ移る。
                    let mut child = path.clone();
                    child.push(0);
                    self.focus(&child, false);
                } else {
                    self.toggle(&path, true);
                }
            }
            "ArrowLeft" => {
                let Some(path) = current else {
                    return false;
                };
                if self.is_expanded(&path) {
                    self.toggle(&path, false);
                } else if path.len() > 1 {
                    // 閉じているなら、親へ移る。
                    self.focus(&path[..path.len() - 1], false);
                } else {
                    return false;
                }
            }
            // 複数選択の Space は、指している項目を足したり外したりする。
            " " if multiple => {
                let Some(path) = current else {
                    return false;
                };
                if TreeItem::selectable(&self.0.items.borrow(), &path) {
                    self.on_row_clicked(&path, true, false);
                }
            }
            "Enter" | " " => {
                let Some(path) = current else {
                    return false;
                };
                if TreeItem::selectable(&self.0.items.borrow(), &path) {
                    self.on_row_clicked(&path, false, false);
                }
            }
            _ => return false,
        }
        true
    }

    /// Moves the item the keyboard points at, and selects it if it can be selected
    /// (`extend` selects the range from the anchor instead).
    fn focus(&self, path: &[usize], extend: bool) {
        *self.0.active.borrow_mut() = Some(path.to_vec());
        self.reveal_active();
        // カーソルの移動がそのまま選択になる (`NSOutlineView` の ↑↓ と同じ)。
        // 選べない項目では選択を動かさない。
        if TreeItem::selectable(&self.0.items.borrow(), path) {
            self.on_row_clicked(path, false, extend);
        }
    }

    /// Returns the paths of the currently visible items, from the top.
    fn visible_paths(&self) -> Vec<Vec<usize>> {
        let expanded = self.0.expanded.borrow();
        TreeItem::visible(&self.0.items.borrow(), |path| expanded.contains(path))
    }

    /// Brings the item the keyboard points at into the scroll area.
    fn reveal_active(&self) {
        let Some(active) = self.0.active.borrow().clone() else {
            return;
        };
        let _ = self
            .0
            .root
            .set_attribute("aria-activedescendant", &self.item_id(&active));
        self.reveal_path(&active);
    }

    /// Brings the item at `path` into the scroll area.
    fn reveal_path(&self, path: &[usize]) {
        let nodes = self.0.nodes.borrow();
        let Some(node) = nodes.iter().find(|node| node.path == path) else {
            return;
        };
        let row: HtmlElement = node.row.clone().unchecked_into();
        crate::list::reveal_within(&self.0.root, &row);
    }
}
