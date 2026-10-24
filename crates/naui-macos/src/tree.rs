//! Trees (AppKit).
//!
//! A single-column `NSOutlineView` placed in an `NSScrollView`. It is the same
//! control as Finder's sidebar and Xcode's navigators; disclosure triangles,
//! indentation, keyboard navigation (←→ to expand and collapse) and scrolling are
//! all done by AppKit.
//!
//! naui adds only three things: "answer the number of children and their
//! contents", "report items that cannot be selected" and "pass selection and
//! expansion to Rust closures".
//!
//! `NSOutlineView` identifies items by **object identity**, so one `NSObject` is
//! created per path (a sequence of child indices like `[0, 2]`), and [`Nodes`]
//! allows lookups in both directions.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use naui_core::TreeItem;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{define_class, msg_send, DefinedClass, MainThreadMarker, MainThreadOnly, Message};
use objc2_app_kit::{
    NSControlTextEditingDelegate, NSOutlineView, NSOutlineViewDataSource, NSOutlineViewDelegate,
    NSScrollView, NSTableColumn, NSTableColumnResizingOptions, NSTableViewColumnAutoresizingStyle,
    NSTableViewDataSource, NSTableViewDelegate, NSTableViewStyle, NSView,
};
use objc2_foundation::{NSIndexSet, NSInteger, NSMutableIndexSet, NSNotification, NSString};

use crate::list::{cell_view, SelectionHandler};
use crate::widgets::Widget;

/// Only one column is used, so the identifier can be fixed.
const COLUMN_ID: &str = "naui.tree.column";

/// The `userInfo` key that expansion notifications use to carry which item they are
/// about (per the spec of `NSOutlineViewItemDidExpandNotification`).
const ITEM_KEY: &str = "NSObject";

/// Row heights. A tree row is either one line or two lines with secondary text.
///
/// **It is not automatic (`usesAutomaticRowHeights`).** When a branch that was
/// collapsed is expanded again, AppKit **also restores the expansion of the
/// branches inside it**. That restoration happens inside the delegate call for the
/// parent's expansion, so if building row views for automatic heights ran there
/// too, AppKit would warn about "re-entering inside the delegate" (it will become an
/// assertion in the future). There are only two kinds of rows, so the heights are
/// measured here.
#[derive(Clone, Copy)]
struct RowHeights {
    single: f64,
    double: f64,
}

impl RowHeights {
    /// Builds and measures one real cell of each kind. The content is just text and
    /// does not wrap ([`crate::list::cell_view`]), so the height does not depend on the
    /// string.
    fn measure(mtm: MainThreadMarker) -> Self {
        let probe = |detail| cell_view(mtm, "Ag", detail, true).fittingSize().height;
        Self {
            single: probe(None),
            double: probe(Some("Ag")),
        }
    }

    fn of(&self, has_detail: bool) -> f64 {
        match has_detail {
            true => self.double,
            false => self.single,
        }
    }
}

/// The handler notified when expansion changes. Called with the path and the new
/// state.
#[derive(Clone, Default)]
struct ExpandHandler(Rc<RefCell<Option<Box<dyn FnMut(&[usize], bool)>>>>);

impl ExpandHandler {
    fn set(&self, f: impl FnMut(&[usize], bool) + 'static) {
        *self.0.borrow_mut() = Some(Box::new(f));
    }

    /// Takes out the closure only for the duration of the call (the same shape as
    /// `SelectionHandler`). Operating on the tree from inside the notification does not
    /// double-borrow.
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

/// The mapping between paths and the objects passed to `NSOutlineView`.
///
/// `NSOutlineView` remembers expansion state and selection assuming "the same item
/// returns the same object", so the per-path objects are reused rather than
/// recreated.
#[derive(Default)]
struct Nodes {
    by_path: RefCell<HashMap<Vec<usize>, Retained<NSObject>>>,
    /// Paths looked up by object address. Maps items received by the delegate back.
    by_address: RefCell<HashMap<usize, Vec<usize>>>,
}

impl Nodes {
    /// Rebuilds it to match the shape of the tree.
    fn reset(&self, items: &[TreeItem]) {
        self.by_path.borrow_mut().clear();
        self.by_address.borrow_mut().clear();
        TreeItem::walk(items, |path, _| {
            self.object(path);
        });
    }

    /// The object for the path, created if absent.
    fn object(&self, path: &[usize]) -> Retained<NSObject> {
        if let Some(object) = self.by_path.borrow().get(path) {
            return object.clone();
        }
        let object = NSObject::new();
        self.by_address
            .borrow_mut()
            .insert(Retained::as_ptr(&object) as usize, path.to_vec());
        self.by_path
            .borrow_mut()
            .insert(path.to_vec(), object.clone());
        object
    }

    /// Looks up only objects already created.
    fn find(&self, path: &[usize]) -> Option<Retained<NSObject>> {
        self.by_path.borrow().get(path).cloned()
    }

    /// Maps an item passed by AppKit back to a path.
    ///
    /// The root (`nil`) becomes the empty path.
    fn path(&self, item: Option<&AnyObject>) -> Option<Vec<usize>> {
        let Some(item) = item else {
            return Some(Vec::new());
        };
        self.by_address
            .borrow()
            .get(&(item as *const AnyObject as usize))
            .cloned()
    }
}

/// The state seen by the data source and delegate. Shared with the handle.
struct SourceState {
    items: Rc<RefCell<Vec<TreeItem>>>,
    nodes: Rc<Nodes>,
    handler: SelectionHandler,
    expand: ExpandHandler,
    /// The item being expanded or collapsed right now. Used to drop chained
    /// notifications.
    target: RefCell<Option<Vec<usize>>>,
    /// The items remembered as expanded.
    ///
    /// AppKit **also remembers the expansion inside collapsed branches and restores it
    /// when expanded again** (the same as Finder). While collapsed, `isItemExpanded`
    /// returns `false`, so the expansion naui answers with is held here.
    expanded: Rc<RefCell<HashSet<Vec<usize>>>>,
    /// Stops notifications only while selection or expansion is being changed from
    /// code, since AppKit sends notifications even for `expandItem:` and
    /// `selectRowIndexes:`.
    silent: Rc<Cell<bool>>,
    /// Row heights. Measured once at creation.
    heights: RowHeights,
}

impl SourceState {
    /// Updates the record of expansion.
    fn mark(&self, path: &[usize], expanded: bool) {
        let mut set = self.expanded.borrow_mut();
        match expanded {
            true => set.insert(path.to_vec()),
            false => set.remove(path),
        };
    }

    /// The number of children of the item the path points at. The empty path points at
    /// the root.
    fn child_count(&self, path: &[usize]) -> usize {
        let items = self.items.borrow();
        match path.is_empty() {
            true => items.len(),
            false => TreeItem::at(&items, path).map_or(0, |item| item.children.len()),
        }
    }
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "NauiTreeSource"]
    #[ivars = SourceState]
    struct TreeSource;

    unsafe impl NSObjectProtocol for TreeSource {}

    // NSOutlineViewDataSource / Delegate は NSTableView 側を継承している。
    unsafe impl NSTableViewDataSource for TreeSource {}
    unsafe impl NSControlTextEditingDelegate for TreeSource {}
    unsafe impl NSTableViewDelegate for TreeSource {}

    unsafe impl NSOutlineViewDataSource for TreeSource {
        #[unsafe(method(outlineView:numberOfChildrenOfItem:))]
        fn number_of_children(
            &self,
            _outline_view: &NSOutlineView,
            item: Option<&AnyObject>,
        ) -> NSInteger {
            let state = self.ivars();
            state
                .nodes
                .path(item)
                .map_or(0, |path| state.child_count(&path)) as NSInteger
        }

        // Retained を返すので `method_id`。所有権の扱いは objc2 が面倒を見る。
        #[unsafe(method_id(outlineView:child:ofItem:))]
        fn child_of_item(
            &self,
            _outline_view: &NSOutlineView,
            index: NSInteger,
            item: Option<&AnyObject>,
        ) -> Retained<AnyObject> {
            let state = self.ivars();
            let mut path = state.nodes.path(item).unwrap_or_default();
            path.push(usize::try_from(index).unwrap_or(0));
            let node = state.nodes.object(&path);
            // NSObject から AnyObject への付け替え。実体は同じオブジェクト。
            Retained::into_super(node)
        }

        #[unsafe(method(outlineView:isItemExpandable:))]
        fn is_item_expandable(&self, _outline_view: &NSOutlineView, item: &AnyObject) -> bool {
            let state = self.ivars();
            state
                .nodes
                .path(Some(item))
                .is_some_and(|path| state.child_count(&path) > 0)
        }
    }

    unsafe impl NSOutlineViewDelegate for TreeSource {
        #[unsafe(method_id(outlineView:viewForTableColumn:item:))]
        fn view_for_item(
            &self,
            _outline_view: &NSOutlineView,
            _column: Option<&NSTableColumn>,
            item: &AnyObject,
        ) -> Option<Retained<NSView>> {
            // `?` は method_id の本体では使えないので、中身は関数へ分ける。
            self.build_cell(MainThreadMarker::from(self), item)
        }

        // 高さは AppKit に測らせず、こちらが答える ([`RowHeights`])。
        #[unsafe(method(outlineView:heightOfRowByItem:))]
        fn height_of_row(&self, _outline_view: &NSOutlineView, item: &AnyObject) -> f64 {
            let state = self.ivars();
            state.heights.of(self.has_detail(item))
        }

        #[unsafe(method(outlineView:shouldSelectItem:))]
        fn should_select_item(&self, _outline_view: &NSOutlineView, item: &AnyObject) -> bool {
            let state = self.ivars();
            state
                .nodes
                .path(Some(item))
                .is_some_and(|path| TreeItem::selectable(&state.items.borrow(), &path))
        }

        #[unsafe(method(outlineViewSelectionDidChange:))]
        fn selection_did_change(&self, notification: &NSNotification) {
            let state = self.ivars();
            if state.silent.get() {
                return;
            }
            let Some(object) = notification.object() else {
                return;
            };
            let Ok(outline) = object.downcast::<NSOutlineView>() else {
                return;
            };
            let path = selected_path(&outline, &state.nodes);
            state.handler.emit(&path);
        }

        // AppKit は枝を開閉すると、その中の枝の開閉も連鎖して通知してくる
        // (閉じるときは中から、開き直すときは元どおりに)。naui が通知するのは
        // **操作された枝 1 つ**だけなので、最初に来る Will で見分ける。
        #[unsafe(method(outlineViewItemWillExpand:))]
        fn item_will_expand(&self, notification: &NSNotification) {
            self.remember_target(notification);
        }

        #[unsafe(method(outlineViewItemWillCollapse:))]
        fn item_will_collapse(&self, notification: &NSNotification) {
            self.remember_target(notification);
        }

        #[unsafe(method(outlineViewItemDidExpand:))]
        fn item_did_expand(&self, notification: &NSNotification) {
            self.notify_expansion(notification, true);
        }

        #[unsafe(method(outlineViewItemDidCollapse:))]
        fn item_did_collapse(&self, notification: &NSNotification) {
            self.notify_expansion(notification, false);
        }
    }
);

impl TreeSource {
    fn new(mtm: MainThreadMarker, state: SourceState) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(state);
        unsafe { msg_send![super(this), init] }
    }

    /// Builds the view for one item.
    fn build_cell(&self, mtm: MainThreadMarker, item: &AnyObject) -> Option<Retained<NSView>> {
        let state = self.ivars();
        let path = state.nodes.path(Some(item))?;
        let items = state.items.borrow();
        let node = TreeItem::at(&items, &path)?;
        // 選べるかどうかは祖先まで見て決まる。淡い見た目もそれにそろえる。
        let enabled = TreeItem::selectable(&items, &path);
        Some(cell_view(mtm, &node.label, node.detail.as_deref(), enabled))
    }

    /// Whether the item has secondary text (= becomes two lines).
    fn has_detail(&self, item: &AnyObject) -> bool {
        let state = self.ivars();
        let Some(path) = state.nodes.path(Some(item)) else {
            return false;
        };
        let items = state.items.borrow();
        TreeItem::at(&items, &path).is_some_and(|item| item.detail.is_some())
    }

    /// Remembers the item about to be expanded or collapsed. Only the first in a chain
    /// is the item that was operated on.
    fn remember_target(&self, notification: &NSNotification) {
        let state = self.ivars();
        if state.target.borrow().is_some() {
            return;
        }
        *state.target.borrow_mut() = notified_path(state, notification);
    }

    /// Passes an expansion notification to the Rust closure.
    ///
    /// Which item was expanded (collapsed) comes in `userInfo`. The inner branches that
    /// arrive as part of a chain do not match the remembered target, so they are
    /// dropped.
    fn notify_expansion(&self, notification: &NSNotification, expanded: bool) {
        let state = self.ivars();
        let Some(path) = notified_path(state, notification) else {
            return;
        };
        if state.target.borrow().as_deref() != Some(path.as_slice()) {
            return;
        }
        *state.target.borrow_mut() = None;
        state.mark(&path, expanded);
        if !state.silent.get() {
            state.expand.emit(&path, expanded);
        }
    }
}

/// The path of the item carried by the expansion notification.
fn notified_path(state: &SourceState, notification: &NSNotification) -> Option<Vec<usize>> {
    let info = notification.userInfo()?;
    let key = NSString::from_str(ITEM_KEY);
    let item = info.objectForKey(key.as_ref())?;
    state.nodes.path(Some(&item))
}

/// The path of the selected item. Empty if nothing is selected.
fn selected_path(outline: &NSOutlineView, nodes: &Nodes) -> Vec<usize> {
    let row = outline.selectedRow();
    if row < 0 {
        return Vec::new();
    }
    outline
        .itemAtRow(row)
        .and_then(|item| nodes.path(Some(&item)))
        .unwrap_or_default()
}

struct TreeInner {
    /// The view visible from outside. The tree is one widget together with this scroll
    /// view.
    scroll: Retained<NSScrollView>,
    outline: Retained<NSOutlineView>,
    items: Rc<RefCell<Vec<TreeItem>>>,
    nodes: Rc<Nodes>,
    handler: SelectionHandler,
    expand: ExpandHandler,
    /// The expansion state as seen by naui (the same as [`SourceState::expanded`]).
    expanded: Rc<RefCell<HashSet<Vec<usize>>>>,
    silent: Rc<Cell<bool>>,
    /// Delegates and data sources are weak references, so they are held.
    _source: Retained<TreeSource>,
}

/// A list whose nested items can be expanded and collapsed (NSOutlineView).
///
/// Items are referred to by the sequence of child indices from the root (a path).
/// `[0, 2]` is "the third child of the first root", and an empty path means "no
/// selection".
///
/// Inside is a single-column `NSOutlineView` in an `NSScrollView`, and
/// [`Widget::native_view`] returns that scroll view. **A scroll view has no height
/// that fits its contents**, so set the height with `set_sizing`.
#[derive(Clone)]
pub struct Tree(Rc<TreeInner>);

impl Widget for Tree {
    fn native_view(&self) -> Retained<NSView> {
        let view: &NSView = self.0.scroll.as_ref();
        view.retain()
    }
    fn boxed_clone(&self) -> Box<dyn Widget> {
        Box::new(self.clone())
    }
}

crate::widgets::impl_sizing!(Tree);

impl Tree {
    pub(crate) fn new(mtm: MainThreadMarker) -> Self {
        let outline = NSOutlineView::new(mtm);
        outline.setStyle(NSTableViewStyle::Inset);
        outline.setHeaderView(None);
        outline.setAllowsMultipleSelection(false);
        // 単一選択でも「何も選ばれていない」状態を持てるようにする。
        outline.setAllowsEmptySelection(true);
        outline.setColumnAutoresizingStyle(
            NSTableViewColumnAutoresizingStyle::UniformColumnAutoresizingStyle,
        );
        // 行の高さは `outlineView:heightOfRowByItem:` で答える ([`RowHeights`])。
        outline.setUsesAutomaticRowHeights(false);

        let column = NSTableColumn::initWithIdentifier(
            NSTableColumn::alloc(mtm),
            &NSString::from_str(COLUMN_ID),
        );
        column.setResizingMask(NSTableColumnResizingOptions::AutoresizingMask);
        outline.addTableColumn(&column);
        // 開閉の三角と段付けを出す列。指定しないとどこにも三角が出ない。
        unsafe { outline.setOutlineTableColumn(Some(&column)) };

        let items: Rc<RefCell<Vec<TreeItem>>> = Rc::new(RefCell::new(Vec::new()));
        let nodes = Rc::new(Nodes::default());
        let handler = SelectionHandler::default();
        let expand = ExpandHandler::default();
        let expanded: Rc<RefCell<HashSet<Vec<usize>>>> = Rc::new(RefCell::new(HashSet::new()));
        let silent = Rc::new(Cell::new(false));
        let source = TreeSource::new(
            mtm,
            SourceState {
                items: items.clone(),
                nodes: nodes.clone(),
                handler: handler.clone(),
                expand: expand.clone(),
                target: RefCell::new(None),
                expanded: expanded.clone(),
                silent: silent.clone(),
                heights: RowHeights::measure(mtm),
            },
        );
        unsafe {
            outline.setDataSource(Some(ProtocolObject::from_ref(&*source)));
            outline.setDelegate(Some(ProtocolObject::from_ref(&*source)));
        }

        let scroll = NSScrollView::new(mtm);
        scroll.setHasVerticalScroller(true);
        scroll.setDocumentView(Some(&outline));

        Self(Rc::new(TreeInner {
            scroll,
            outline,
            items,
            nodes,
            handler,
            expand,
            expanded,
            silent,
            _source: source,
        }))
    }

    /// Rebuilds the items. Paths change meaning, so the selection is cleared.
    ///
    /// Expansion returns to what [`TreeItem::expanded`] says.
    pub fn set_items(&self, items: &[TreeItem]) {
        *self.0.items.borrow_mut() = items.to_vec();
        self.0.nodes.reset(items);
        self.0.expanded.borrow_mut().clear();
        self.without_notifying(|this| {
            this.0.outline.reloadData();
            unsafe { this.0.outline.deselectAll(None) };
            // 親が先に来るので、そのまま上から開いていける。
            TreeItem::walk(items, |path, item| {
                if item.expanded && !item.is_leaf() {
                    this.apply_expanded(path, true);
                }
            });
        });
    }

    /// The total number of items, descendants included.
    pub fn len(&self) -> usize {
        TreeItem::count(&self.0.items.borrow())
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The path of the selected item. `None` if nothing is selected.
    pub fn selected(&self) -> Option<Vec<usize>> {
        let path = selected_path(&self.0.outline, &self.0.nodes);
        (!path.is_empty()).then_some(path)
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
        // 開くと AppKit が開閉を知らせてくるので、ここでは通知を止める。
        self.without_notifying(|this| {
            this.apply_expanded(&path[..path.len().saturating_sub(1)], true)
        });
        let Some(node) = self.0.nodes.find(path) else {
            return;
        };
        let row = unsafe { self.0.outline.rowForItem(Some(&node)) };
        if row >= 0 {
            self.0.outline.scrollRowToVisible(row);
        }
    }

    /// Selects one item without notifying.
    ///
    /// Passing an unselectable item ([`TreeItem::selectable`]) or a path that does not
    /// exist clears the selection. Items inside a collapsed branch are selected after
    /// their ancestors are expanded so they become visible.
    pub fn set_selected(&self, path: &[usize]) {
        self.without_notifying(|this| this.apply_selected(path));
    }

    /// Clears the selection without notifying.
    pub fn clear_selection(&self) {
        self.without_notifying(|this| unsafe { this.0.outline.deselectAll(None) });
    }

    /// Selects one item through the same path as a user's selection (notifies).
    pub fn select(&self, path: &[usize]) {
        // AppKit は同じ項目を選び直すとデリゲートを呼ばない。
        // 通知の回数をそろえるため、ここで 1 回だけ出す。
        self.without_notifying(|this| this.apply_selected(path));
        let actual = selected_path(&self.0.outline, &self.0.nodes);
        self.0.handler.emit(&actual);
    }

    /// Called with the path of the selected item when the selection changes.
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
        self.without_notifying(|this| this.apply_expanded(path, expanded));
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
        self.without_notifying(|this| {
            let items = this.0.items.borrow().clone();
            TreeItem::walk(&items, |path, item| {
                if !item.is_leaf() {
                    this.apply_expanded(path, true);
                }
            });
        });
    }

    /// Collapses every branch without notifying.
    pub fn collapse_all(&self) {
        self.without_notifying(|this| {
            let items = this.0.items.borrow().clone();
            let mut branches = Vec::new();
            TreeItem::walk(&items, |path, item| {
                if !item.is_leaf() {
                    branches.push(path.to_vec());
                }
            });
            // 子から先に閉じる。親を先に閉じると、その中の枝は
            // AppKit から見えないままになり、開き直したときに戻ってしまう。
            for path in branches.iter().rev() {
                this.apply_expanded(path, false);
            }
        });
    }

    /// Called with the item's path and its new state when its expansion changes.
    pub fn on_expand(&self, f: impl FnMut(&[usize], bool) + 'static) {
        self.0.expand.set(f);
    }

    /// The inner `NSOutlineView`. Public as a backend-specific escape hatch.
    pub fn native_outline_view(&self) -> Retained<NSOutlineView> {
        self.0.outline.clone()
    }

    /// Changes the expansion state and notifies once.
    fn toggle(&self, path: &[usize], expanded: bool) {
        self.without_notifying(|this| this.apply_expanded(path, expanded));
        if TreeItem::at(&self.0.items.borrow(), path).is_some_and(|item| !item.is_leaf()) {
            self.0.expand.emit(path, expanded);
        }
    }

    /// Applies expansion to the native side. When expanding, collapsed ancestors are
    /// also expanded, from the top down.
    fn apply_expanded(&self, path: &[usize], expanded: bool) {
        if expanded {
            for depth in 1..=path.len() {
                let prefix = &path[..depth];
                if let Some(node) = self.0.nodes.find(prefix) {
                    unsafe { self.0.outline.expandItem(Some(&node)) };
                    self.0.expanded.borrow_mut().insert(prefix.to_vec());
                }
            }
        } else if let Some(node) = self.0.nodes.find(path) {
            unsafe { self.0.outline.collapseItem(Some(&node)) };
            self.0.expanded.borrow_mut().remove(path);
        }
    }

    /// Applies the selection to the native side.
    fn apply_selected(&self, path: &[usize]) {
        let deselect = || unsafe { self.0.outline.deselectAll(None) };
        if !TreeItem::selectable(&self.0.items.borrow(), path) {
            deselect();
            return;
        }
        // 見えていない行は選べないので、祖先を開いてから探す。
        self.apply_expanded(path, true);
        let Some(node) = self.0.nodes.find(path) else {
            deselect();
            return;
        };
        let row = unsafe { self.0.outline.rowForItem(Some(&node)) };
        if row < 0 {
            deselect();
            return;
        }
        let set = NSMutableIndexSet::new();
        set.addIndex(row as usize);
        let set: &NSIndexSet = set.as_ref();
        self.0
            .outline
            .selectRowIndexes_byExtendingSelection(set, false);
    }

    /// Operates with notifications from AppKit stopped.
    fn without_notifying(&self, f: impl FnOnce(&Self)) {
        let previous = self.0.silent.replace(true);
        f(self);
        self.0.silent.set(previous);
    }
}
