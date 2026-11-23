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
//!
//! Items made from widgets ([`TreeRow`]) are put in cells the same way as `List`
//! rows ([`crate::row_cell`]): the tree takes each widget when it is handed over, and a
//! new cell puts it in itself only if it is in an earlier cell of this tree or in no
//! parent.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use naui_core::{keeps_hidden_selection, SelectionGesture, SelectionMode, TreeItem};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{define_class, msg_send, DefinedClass, MainThreadMarker, MainThreadOnly, Message};
use objc2_app_kit::{
    NSApplication, NSControlTextEditingDelegate, NSEventModifierFlags, NSEventType, NSOutlineView,
    NSOutlineViewDataSource, NSOutlineViewDelegate, NSScrollView, NSTableColumn,
    NSTableColumnResizingOptions, NSTableViewColumnAutoresizingStyle, NSTableViewDataSource,
    NSTableViewDelegate, NSTableViewStyle, NSView,
};
use objc2_foundation::{NSIndexSet, NSInteger, NSMutableIndexSet, NSNotification, NSString};

use crate::list::{cell_view, custom_cell_view, SelectionHandler, ROW_PADDING};
use crate::row_cell::CellOwner;
use crate::trampoline::SharedSlot;
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

/// The widgets of items made from [`TreeRow`]s, by path.
type Contents = HashMap<Vec<usize>, Box<dyn Widget>>;

/// One item to put in a tree, with a widget as its content.
///
/// [`TreeItem`] is a convenience API for trees that need only strings. To combine
/// icons, several labels, check boxes, buttons and so on, build the content with
/// `Grid` / `Stack` and wrap it in this type.
///
/// ```no_run
/// # use naui_macos::{TreeRow, Widget};
/// # fn row(folder: &dyn Widget, file: &dyn Widget) {
/// let row = TreeRow::new(folder)
///     .expanded(true)
///     .child(TreeRow::new(file).selectable(false));
/// # let _ = row;
/// # }
/// ```
pub struct TreeRow {
    content: Box<dyn Widget>,
    selectable: bool,
    expanded: bool,
    children: Vec<TreeRow>,
}

impl Clone for TreeRow {
    fn clone(&self) -> Self {
        Self {
            content: self.content.boxed_clone(),
            selectable: self.selectable,
            expanded: self.expanded,
            children: self.children.clone(),
        }
    }
}

impl TreeRow {
    /// Uses a widget as the content of one item. By default the item can be selected,
    /// starts collapsed and has no children.
    pub fn new(content: &dyn Widget) -> Self {
        Self {
            content: content.boxed_clone(),
            selectable: true,
            expanded: false,
            children: Vec::new(),
        }
    }

    /// Set to `false` for items where only the controls inside are operated.
    ///
    /// Its descendants cannot be selected either, as with [`TreeItem::enabled`]. The row
    /// is not dimmed, and the controls inside keep working.
    pub fn selectable(mut self, selectable: bool) -> Self {
        self.selectable = selectable;
        self
    }

    pub fn is_selectable(&self) -> bool {
        self.selectable
    }

    /// Sets whether it is shown expanded from the start (collapsed by default).
    pub fn expanded(mut self, expanded: bool) -> Self {
        self.expanded = expanded;
        self
    }

    pub fn is_expanded(&self) -> bool {
        self.expanded
    }

    /// Replaces the child items.
    pub fn children(mut self, children: impl IntoIterator<Item = TreeRow>) -> Self {
        self.children = children.into_iter().collect();
        self
    }

    /// Adds one child item.
    pub fn child(mut self, child: TreeRow) -> Self {
        self.children.push(child);
        self
    }

    /// Whether the item has no children (cannot be expanded).
    pub fn is_leaf(&self) -> bool {
        self.children.is_empty()
    }

    /// The shape of the rows as items without text: selection and expansion are
    /// handled by the same code as for [`TreeItem`]s.
    fn skeleton(rows: &[TreeRow]) -> Vec<TreeItem> {
        rows.iter()
            .map(|row| {
                TreeItem::new("")
                    .enabled(row.selectable)
                    .expanded(row.expanded)
                    .children(TreeRow::skeleton(&row.children))
            })
            .collect()
    }

    /// Collects the widgets by path.
    fn collect(rows: &[TreeRow], path: &mut Vec<usize>, out: &mut Contents) {
        for (index, row) in rows.iter().enumerate() {
            path.push(index);
            out.insert(path.clone(), row.content.boxed_clone());
            TreeRow::collect(&row.children, path, out);
            path.pop();
        }
    }
}

/// The closure notified with the path and the new expansion state.
type ExpandFn = dyn FnMut(&[usize], bool);

/// The handler notified when expansion changes. Called with the path and the new
/// state.
#[derive(Clone, Default)]
struct ExpandHandler(SharedSlot<ExpandFn>);

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
    /// The selected items as seen by naui (normalized, in tree order).
    ///
    /// AppKit **deselects the rows inside a branch when it is collapsed**, while naui
    /// keeps them selected but hidden (as GTK, Windows and the Web do). So the
    /// selection is held here, and written back to AppKit when the branch is
    /// expanded again ([`TreeSource::restore_selection`]).
    selection: Rc<RefCell<Vec<Vec<usize>>>>,
    mode: Rc<Cell<SelectionMode>>,
    /// Row heights. Measured once at creation.
    heights: RowHeights,
    /// The widgets of items made from [`TreeRow`]s, by path.
    contents: Rc<RefCell<Contents>>,
    /// The mark on the cells this tree makes for widgets ([`crate::row_cell`]).
    owner: CellOwner,
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

    /// Whether the item at the path has a row now (all its ancestors are expanded).
    fn is_shown(&self, outline: &NSOutlineView, path: &[usize]) -> bool {
        self.nodes
            .find(path)
            .is_some_and(|node| unsafe { outline.rowForItem(Some(&node)) } >= 0)
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
            match self.content_height(item) {
                Some(height) => height,
                None => state.heights.of(self.has_detail(item)),
            }
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
            if self.take_native_selection(&outline) {
                let first = state
                    .selection
                    .borrow()
                    .first()
                    .cloned()
                    .unwrap_or_default();
                state.handler.emit(&first);
            }
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
            // 開いて見えるようになった行へ、覚えている選択を書き戻す。連鎖して
            // 開き直された中の枝の分も含めて、来るたびに合わせる。
            if let Some(outline) = notification
                .object()
                .and_then(|object| object.downcast::<NSOutlineView>().ok())
            {
                self.restore_selection(&outline);
            }
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
        if let Some(content) = state.contents.borrow().get(&path) {
            let height = self.content_height(item);
            return Some(custom_cell_view(mtm, &**content, height, state.owner));
        }
        let items = state.items.borrow();
        let node = TreeItem::at(&items, &path)?;
        // 選べるかどうかは祖先まで見て決まる。淡い見た目もそれにそろえる。
        let enabled = TreeItem::selectable(&items, &path);
        Some(cell_view(mtm, &node.label, node.detail.as_deref(), enabled))
    }

    /// The height of the row of an item made from a widget. `None` for text items.
    ///
    /// It is measured from the widget each time AppKit asks, rather than by AppKit from
    /// the cell ([`RowHeights`] explains why). `fittingSize` reads only the constraints
    /// inside the widget, so it does not depend on the cell it is in. A widget the app
    /// has moved elsewhere leaves an empty row of one line.
    fn content_height(&self, item: &AnyObject) -> Option<f64> {
        let state = self.ivars();
        let path = state.nodes.path(Some(item))?;
        let contents = state.contents.borrow();
        let view = contents.get(&path)?.native_view();
        let single = state.heights.single;
        if !state.owner.may_take(&view) {
            return Some(single);
        }
        Some((view.fittingSize().height + ROW_PADDING * 2.0).max(single))
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

    /// Takes in a selection the user changed on the AppKit side. Returns whether naui's
    /// selection changed (whether to notify).
    ///
    /// - When rows disappear only because their branch was collapsed, nothing has
    ///   changed for naui: they stay selected while hidden.
    /// - With ⌘ held (adding or removing), hidden selected items are kept along with
    ///   what AppKit selected.
    /// - Otherwise (a plain click, an arrow key or a ⇧ range) the selection is made
    ///   anew, hidden items dropped, as GTK's `GtkListBox` does. This is the same rule
    ///   as for rows outside the window of a long `List` ([`keeps_hidden_selection`]).
    fn take_native_selection(&self, outline: &NSOutlineView) -> bool {
        let state = self.ivars();
        let native = selected_paths(outline, &state.nodes);
        let before = state.selection.borrow().clone();
        let removed_only_hidden = native.iter().all(|path| before.contains(path))
            && before
                .iter()
                .filter(|path| !native.contains(path))
                .all(|path| !state.is_shown(outline, path));
        if removed_only_hidden && native.len() < before.len() {
            return false;
        }
        let (shown, hidden): (Vec<_>, Vec<_>) = before
            .into_iter()
            .partition(|path| state.is_shown(outline, path));
        let gesture = current_gesture(MainThreadMarker::from(self));
        let mut picked = native;
        if state.mode.get().is_multiple() && keeps_hidden_selection(&shown, &picked, gesture) {
            picked.extend(hidden);
        }
        let picked = state
            .mode
            .get()
            .normalize_paths(&state.items.borrow(), &picked);
        *state.selection.borrow_mut() = picked;
        true
    }

    /// Writes the remembered selection back to the rows AppKit shows now (without
    /// notifying). Only when they differ, since AppKit notifies even for the same
    /// selection.
    fn restore_selection(&self, outline: &NSOutlineView) {
        let state = self.ivars();
        let wanted = selection_rows(outline, &state.nodes, &state.selection.borrow());
        let current = outline.selectedRowIndexes();
        if current.isEqualToIndexSet(&wanted) {
            return;
        }
        let previous = state.silent.replace(true);
        outline.selectRowIndexes_byExtendingSelection(&wanted, false);
        state.silent.set(previous);
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

/// The paths of the rows AppKit has selected, from the top (only visible rows).
fn selected_paths(outline: &NSOutlineView, nodes: &Nodes) -> Vec<Vec<usize>> {
    let rows = outline.selectedRowIndexes();
    let mut paths = Vec::new();
    let mut row = rows.firstIndex();
    while row != objc2_foundation::NSNotFound as usize {
        if let Some(path) = outline
            .itemAtRow(row as NSInteger)
            .and_then(|item| nodes.path(Some(&item)))
        {
            paths.push(path);
        }
        row = rows.indexGreaterThanIndex(row);
    }
    paths
}

/// The rows of the selected items that are visible now. Hidden ones have no row.
fn selection_rows(
    outline: &NSOutlineView,
    nodes: &Nodes,
    selection: &[Vec<usize>],
) -> Retained<NSIndexSet> {
    let set = NSMutableIndexSet::new();
    for path in selection {
        let Some(node) = nodes.find(path) else {
            continue;
        };
        let row = unsafe { outline.rowForItem(Some(&node)) };
        if row >= 0 {
            set.addIndex(row as usize);
        }
    }
    Retained::into_super(set)
}

/// How the event AppKit is handling now changes the selection, read from its
/// modifier keys as `NSTableView` reads them (⇧ takes precedence, as on Windows).
/// `Unknown` when no click or key is being handled.
fn current_gesture(mtm: MainThreadMarker) -> SelectionGesture {
    let Some(event) = NSApplication::sharedApplication(mtm).currentEvent() else {
        return SelectionGesture::Unknown;
    };
    let pointer_or_key = matches!(
        event.r#type(),
        NSEventType::LeftMouseDown
            | NSEventType::LeftMouseUp
            | NSEventType::LeftMouseDragged
            | NSEventType::KeyDown
    );
    if !pointer_or_key {
        return SelectionGesture::Unknown;
    }
    let flags = event.modifierFlags();
    if flags.contains(NSEventModifierFlags::Shift) {
        SelectionGesture::Extend
    } else if flags.contains(NSEventModifierFlags::Command) {
        SelectionGesture::Toggle
    } else {
        SelectionGesture::Plain
    }
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
    /// The selection as seen by naui (the same as [`SourceState::selection`]).
    selection: Rc<RefCell<Vec<Vec<usize>>>>,
    mode: Rc<Cell<SelectionMode>>,
    /// The widgets of items made from [`TreeRow`]s (the same as [`SourceState::contents`]).
    contents: Rc<RefCell<Contents>>,
    owner: CellOwner,
    /// Delegates and data sources are weak references, so they are held.
    source: Retained<TreeSource>,
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
        let selection: Rc<RefCell<Vec<Vec<usize>>>> = Rc::new(RefCell::new(Vec::new()));
        let mode = Rc::new(Cell::new(SelectionMode::Single));
        let contents: Rc<RefCell<Contents>> = Rc::default();
        let owner = CellOwner::new();
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
                selection: selection.clone(),
                mode: mode.clone(),
                heights: RowHeights::measure(mtm),
                contents: contents.clone(),
                owner,
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
            selection,
            mode,
            contents,
            owner,
            source,
        }))
    }

    /// Rebuilds the items. Paths change meaning, so the selection is cleared.
    ///
    /// Expansion returns to what [`TreeItem::expanded`] says.
    pub fn set_items(&self, items: &[TreeItem]) {
        self.load(items, Contents::new());
    }

    /// Rebuilds the items with widgets as their content. Paths change meaning, so the
    /// selection is cleared.
    ///
    /// Expansion returns to what [`TreeRow::expanded`] says. Each widget is taken from
    /// wherever it is now; if the app moves it elsewhere afterwards, its row stays
    /// empty ([`crate::row_cell`]).
    pub fn set_rows(&self, rows: &[TreeRow]) {
        let mut contents = Contents::new();
        TreeRow::collect(rows, &mut Vec::new(), &mut contents);
        for content in contents.values() {
            crate::row_cell::claim(&content.native_view(), self.0.owner);
        }
        self.load(&TreeRow::skeleton(rows), contents);
    }

    /// Replaces the items and their widgets, and shows them anew.
    fn load(&self, items: &[TreeItem], contents: Contents) {
        *self.0.contents.borrow_mut() = contents;
        *self.0.items.borrow_mut() = items.to_vec();
        self.0.nodes.reset(items);
        self.0.expanded.borrow_mut().clear();
        self.0.selection.borrow_mut().clear();
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

    /// Switches between single and multiple selection. The selection is cleared
    /// without notifying.
    pub fn set_selection_mode(&self, mode: SelectionMode) {
        self.0.mode.set(mode);
        self.0
            .outline
            .setAllowsMultipleSelection(mode.is_multiple());
        self.clear_selection();
    }

    pub fn selection_mode(&self) -> SelectionMode {
        self.0.mode.get()
    }

    /// The path of the first selected item. `None` if nothing is selected.
    pub fn selected(&self) -> Option<Vec<usize>> {
        self.0.selection.borrow().first().cloned()
    }

    /// The paths of the selected items in the order the tree shows them, including
    /// items hidden inside collapsed branches.
    pub fn selection(&self) -> Vec<Vec<usize>> {
        self.0.selection.borrow().clone()
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
        self.set_selection(&[path.to_vec()]);
    }

    /// Replaces the selection without notifying. Paths that cannot be selected are
    /// left out, and the branches holding the rest are expanded.
    pub fn set_selection(&self, paths: &[Vec<usize>]) {
        self.without_notifying(|this| this.apply_selection(paths));
    }

    /// Clears the selection without notifying.
    pub fn clear_selection(&self) {
        self.set_selection(&[]);
    }

    /// Selects one item through the same path as a user's selection (notifies).
    pub fn select(&self, path: &[usize]) {
        self.select_many(&[path.to_vec()]);
    }

    /// Replaces the selection through the same path as a user's selection
    /// (notifies).
    pub fn select_many(&self, paths: &[Vec<usize>]) {
        // AppKit は同じ項目を選び直すとデリゲートを呼ばない。
        // 通知の回数をそろえるため、ここで 1 回だけ出す。
        self.set_selection(paths);
        let first = self.selected().unwrap_or_default();
        self.0.handler.emit(&first);
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

    /// Remembers the selection and applies it to the native side.
    fn apply_selection(&self, paths: &[Vec<usize>]) {
        let picked = self
            .0
            .mode
            .get()
            .normalize_paths(&self.0.items.borrow(), paths);
        // 見えていない行は選べないので、先にすべての祖先を開いてから行を引く
        // (開くたびに行の番号がずれる)。
        for path in &picked {
            self.apply_expanded(&path[..path.len() - 1], true);
        }
        *self.0.selection.borrow_mut() = picked;
        self.0.source.restore_selection(&self.0.outline);
    }

    /// Operates with notifications from AppKit stopped.
    fn without_notifying(&self, f: impl FnOnce(&Self)) {
        let previous = self.0.silent.replace(true);
        f(self);
        self.0.silent.set(previous);
    }
}
