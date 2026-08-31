//! ツリー (AppKit)。
//!
//! `NSOutlineView` を 1 列で使い、`NSScrollView` に載せている。Finder の
//! サイドバーや Xcode のナビゲーターと同じコントロールで、開閉の三角、
//! 段付け、キーボード操作 (←→ で開閉)、スクロールはすべて AppKit が行う。
//!
//! naui が足しているのは「子の数と中身を答える」「選べない項目を教える」
//! 「選択と開閉を Rust のクロージャへ渡す」の 3 つだけ。
//!
//! `NSOutlineView` は項目を**オブジェクトの同一性**で識別するため、
//! パス (`[0, 2]` のような子インデックスの並び) 1 つにつき `NSObject` を
//! 1 つ作り、[`Nodes`] で両方向に引けるようにしている。

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

/// 1 列しか使わないので、識別子は固定でよい。
const COLUMN_ID: &str = "naui.tree.column";

/// 開閉の通知が、どの項目のものかを載せてくる `userInfo` のキー
/// (`NSOutlineViewItemDidExpandNotification` の仕様)。
const ITEM_KEY: &str = "NSObject";

/// 開閉が変わったことの通知先。パスと、変わった後の状態で呼ぶ。
#[derive(Clone, Default)]
struct ExpandHandler(Rc<RefCell<Option<Box<dyn FnMut(&[usize], bool)>>>>);

impl ExpandHandler {
    fn set(&self, f: impl FnMut(&[usize], bool) + 'static) {
        *self.0.borrow_mut() = Some(Box::new(f));
    }

    /// 呼び出しの間だけクロージャを取り出す (`SelectionHandler` と同じ形)。
    /// 通知の中からツリーを操作しても二重借用にならない。
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

/// パスと `NSOutlineView` へ渡すオブジェクトの対応表。
///
/// `NSOutlineView` は「同じ項目には同じオブジェクトが返る」ことを前提に
/// 開閉状態や選択を覚えるため、パスごとのオブジェクトは作り直さずに使い回す。
#[derive(Default)]
struct Nodes {
    by_path: RefCell<HashMap<Vec<usize>, Retained<NSObject>>>,
    /// オブジェクトのアドレスから引くパス。デリゲートが受け取る項目を戻す。
    by_address: RefCell<HashMap<usize, Vec<usize>>>,
}

impl Nodes {
    /// 木の形に合わせて作り直す。
    fn reset(&self, items: &[TreeItem]) {
        self.by_path.borrow_mut().clear();
        self.by_address.borrow_mut().clear();
        TreeItem::walk(items, |path, _| {
            self.object(path);
        });
    }

    /// パスに対応するオブジェクト。無ければ作る。
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

    /// すでに作ってあるオブジェクトだけを引く。
    fn find(&self, path: &[usize]) -> Option<Retained<NSObject>> {
        self.by_path.borrow().get(path).cloned()
    }

    /// AppKit から渡された項目を、パスへ戻す。
    ///
    /// 根 (`nil`) は空のパスになる。
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

/// データソース兼デリゲートが見る状態。ハンドルと共有する。
struct SourceState {
    items: Rc<RefCell<Vec<TreeItem>>>,
    nodes: Rc<Nodes>,
    handler: SelectionHandler,
    expand: ExpandHandler,
    /// いま開閉している項目。連鎖の通知を落とすために使う。
    target: RefCell<Option<Vec<usize>>>,
    /// 開いた状態として覚えている項目。
    ///
    /// AppKit は**閉じた枝の中の開閉も覚えていて、開き直すと元へ戻す**
    /// (Finder と同じ)。閉じている間 `isItemExpanded` は `false` を返すので、
    /// naui が答える開閉はここで持つ。
    expanded: Rc<RefCell<HashSet<Vec<usize>>>>,
    /// プログラムから選択や開閉を変えている間だけ通知を止める。
    /// AppKit は `expandItem:` や `selectRowIndexes:` でも通知を出すため。
    silent: Rc<Cell<bool>>,
}

impl SourceState {
    /// 開閉の控えを更新する。
    fn mark(&self, path: &[usize], expanded: bool) {
        let mut set = self.expanded.borrow_mut();
        match expanded {
            true => set.insert(path.to_vec()),
            false => set.remove(path),
        };
    }

    /// パスの指す項目の子の数。空のパスは根を指す。
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

    /// 項目 1 つ分のビューを組み立てる。
    fn build_cell(&self, mtm: MainThreadMarker, item: &AnyObject) -> Option<Retained<NSView>> {
        let state = self.ivars();
        let path = state.nodes.path(Some(item))?;
        let items = state.items.borrow();
        let node = TreeItem::at(&items, &path)?;
        // 選べるかどうかは祖先まで見て決まる。淡い見た目もそれにそろえる。
        let enabled = TreeItem::selectable(&items, &path);
        Some(cell_view(mtm, &node.label, node.detail.as_deref(), enabled))
    }

    /// これから開閉する項目を覚える。連鎖の 1 つ目だけが操作された項目。
    fn remember_target(&self, notification: &NSNotification) {
        let state = self.ivars();
        if state.target.borrow().is_some() {
            return;
        }
        *state.target.borrow_mut() = notified_path(state, notification);
    }

    /// 開閉の通知を Rust のクロージャへ渡す。
    ///
    /// どの項目が開いた (閉じた) かは `userInfo` に載ってくる。連鎖で来る
    /// 中の枝の分は、覚えておいた操作対象と食い違うので落とす。
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

/// 開閉の通知に載っている項目のパス。
fn notified_path(state: &SourceState, notification: &NSNotification) -> Option<Vec<usize>> {
    let info = notification.userInfo()?;
    let key = NSString::from_str(ITEM_KEY);
    let item = info.objectForKey(key.as_ref())?;
    state.nodes.path(Some(&item))
}

/// 選ばれている項目のパス。何も選ばれていなければ空。
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
    /// 外から見えるビュー。ツリーはこのスクロールビューごと 1 つのウィジェット。
    scroll: Retained<NSScrollView>,
    outline: Retained<NSOutlineView>,
    items: Rc<RefCell<Vec<TreeItem>>>,
    nodes: Rc<Nodes>,
    handler: SelectionHandler,
    expand: ExpandHandler,
    /// naui から見た開閉の状態 ([`SourceState::expanded`] と同じもの)。
    expanded: Rc<RefCell<HashSet<Vec<usize>>>>,
    silent: Rc<Cell<bool>>,
    /// デリゲートとデータソースは weak 参照なので保持する。
    _source: Retained<TreeSource>,
}

/// 入れ子の項目を開閉できる一覧 (NSOutlineView)。
///
/// 項目は根からの子インデックスの並び (パス) で指す。`[0, 2]` は
/// 「1 番目の根の 3 番目の子」で、空のパスは「選択なし」を表す。
///
/// 中身は `NSScrollView` に載った 1 列の `NSOutlineView` で、
/// [`Widget::native_view`] が返すのはそのスクロールビュー。
/// **スクロールビューは中身に合わせた高さを持たない**ため、
/// `set_sizing` で高さを指定すること。
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
        // 行の高さは中身の制約から AppKit に求めさせる (`List` と同じ)。
        outline.setUsesAutomaticRowHeights(true);

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

    /// 項目を作り直す。パスの意味が変わるため、選択は外れる。
    ///
    /// 開閉は [`TreeItem::expanded`] のとおりに戻る。
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

    /// 子孫まで数えた項目の総数。
    pub fn len(&self) -> usize {
        TreeItem::count(&self.0.items.borrow())
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// 選ばれている項目のパス。何も選ばれていなければ `None`。
    pub fn selected(&self) -> Option<Vec<usize>> {
        let path = selected_path(&self.0.outline, &self.0.nodes);
        (!path.is_empty()).then_some(path)
    }

    /// 通知せずに 1 項目を選ぶ。
    ///
    /// 選べない項目 ([`TreeItem::selectable`]) や無いパスを渡すと、
    /// 選択は外れる。閉じた枝の中にある項目は、見えるように祖先を開いてから選ぶ。
    pub fn set_selected(&self, path: &[usize]) {
        self.without_notifying(|this| this.apply_selected(path));
    }

    /// 通知せずに選択を外す。
    pub fn clear_selection(&self) {
        self.without_notifying(|this| unsafe { this.0.outline.deselectAll(None) });
    }

    /// ユーザーが選んだのと同じ経路で 1 項目を選ぶ (通知あり)。
    pub fn select(&self, path: &[usize]) {
        // AppKit は同じ項目を選び直すとデリゲートを呼ばない。
        // 通知の回数をそろえるため、ここで 1 回だけ出す。
        self.without_notifying(|this| this.apply_selected(path));
        let actual = selected_path(&self.0.outline, &self.0.nodes);
        self.0.handler.emit(&actual);
    }

    /// 選択が変わったときに、選ばれている項目のパスで呼ばれる。
    ///
    /// 選択が外れたときは空のパスで呼ばれる。
    pub fn on_select(&self, f: impl FnMut(&[usize]) + 'static) {
        self.0.handler.set(f);
    }

    /// その項目が開いているかどうか。
    ///
    /// 閉じた枝の中にあって見えていない項目でも、開いた状態として覚えていれば
    /// `true` を返す (親を開き直すと、そのまま開いて出てくる)。
    pub fn is_expanded(&self, path: &[usize]) -> bool {
        self.0.expanded.borrow().contains(path)
    }

    /// 通知せずに開閉を変える。開くときは祖先もまとめて開く。
    pub fn set_expanded(&self, path: &[usize], expanded: bool) {
        self.without_notifying(|this| this.apply_expanded(path, expanded));
    }

    /// ユーザーが開いたのと同じ経路で開く (通知あり)。
    pub fn expand(&self, path: &[usize]) {
        self.toggle(path, true);
    }

    /// ユーザーが閉じたのと同じ経路で閉じる (通知あり)。
    pub fn collapse(&self, path: &[usize]) {
        self.toggle(path, false);
    }

    /// 通知せずにすべての枝を開く。
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

    /// 通知せずにすべての枝を閉じる。
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

    /// 開閉が変わったときに、その項目のパスと変わった後の状態で呼ばれる。
    pub fn on_expand(&self, f: impl FnMut(&[usize], bool) + 'static) {
        self.0.expand.set(f);
    }

    /// 中身の `NSOutlineView`。バックエンド固有の脱出口として公開している。
    pub fn native_outline_view(&self) -> Retained<NSOutlineView> {
        self.0.outline.clone()
    }

    /// 開閉を変えて 1 回だけ通知する。
    fn toggle(&self, path: &[usize], expanded: bool) {
        self.without_notifying(|this| this.apply_expanded(path, expanded));
        if TreeItem::at(&self.0.items.borrow(), path).is_some_and(|item| !item.is_leaf()) {
            self.0.expand.emit(path, expanded);
        }
    }

    /// 開閉をネイティブへ写す。開くときは、閉じた祖先も上から順に開く。
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

    /// 選択をネイティブへ写す。
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

    /// AppKit からの通知を止めたまま操作する。
    fn without_notifying(&self, f: impl FnOnce(&Self)) {
        let previous = self.0.silent.replace(true);
        f(self);
        self.0.silent.set(previous);
    }
}
