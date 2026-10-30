//! Trees (WinUI 3).
//!
//! WinUI's standard `TreeView` is used as is. Items are mapped to `TreeViewNode`s
//! and handed over as a tree, so indentation, expander chevrons, keyboard navigation
//! and the selection look are all owned by WinUI.
//!
//! | Part | Built from |
//! | --- | --- |
//! | Frame | A `Border` with the same color and rounded corners as `List` |
//! | Row content | Built elements put in `TreeViewNode.Content`, shown with an `ItemTemplate` |
//! | Indentation and expansion | `TreeView` (notified through `Expanding` / `Collapsed`) |
//! | Selection | The `TreeView`'s `SelectedNode` (notified through `SelectionChanged`) |
//!
//! Scrolling is owned by the `TreeView` itself (unlike `List`, it cannot be
//! entrusted to an outer `ScrollViewer`: the inner list lays out only as many rows
//! as fit its given height, so entrusting it outward would cut it off instead of
//! growing). For the window-wide wheel helper (`crate::layout`), the `ScrollViewer`
//! inside the template is found and registered.
//!
//! naui's [`TreeItem::enabled`] has no WinUI counterpart (disabling a
//! `TreeViewItem` also prevents expanding it for branches). So **the text is dimmed
//! and a cover that absorbs presses is laid over the row's content**. The cover
//! covers only what is right of the expander chevron, so branches that cannot be
//! selected can still be expanded.

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;
use std::sync::Arc;

use naui_core::{Result, TreeItem};
use naui_winui3::Microsoft::UI::Dispatching::{DispatcherQueue, DispatcherQueueHandler};
use naui_winui3::Microsoft::UI::Xaml::Controls::{
    Border, Grid as XamlGrid, ListViewBase, TreeView, TreeViewCollapsedEventArgs,
    TreeViewExpandingEventArgs, TreeViewNode, TreeViewSelectionChangedEventArgs,
    TreeViewSelectionMode,
};
use naui_winui3::Microsoft::UI::Xaml::Input::PointerEventHandler;
use naui_winui3::Microsoft::UI::Xaml::Markup::XamlReader;
use naui_winui3::Microsoft::UI::Xaml::{
    DataTemplate, ResourceDictionary, RoutedEventHandler, Style, UIElement,
};
use windows::Foundation::{PropertyValue, TypedEventHandler};
use windows_core::{IInspectable, Interface, HSTRING};

use crate::layout::ListScrollTarget;
use crate::list::{item_content, SelectionHandler};
use crate::to_error;
use crate::ui_thread::{HandlerCell, UiThreadCell};
use crate::widgets::{impl_widget, Widget};

/// The themed frame. The color and rounded corners are the same as `List`'s frame
/// (`crate::list`).
const SURFACE_XAML: &str = r##"<Border
    xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
    Background="{ThemeResource ControlFillColorDefaultBrush}"
    BorderBrush="{ThemeResource ControlStrokeColorDefaultBrush}"
    BorderThickness="1"
    CornerRadius="{ThemeResource ControlCornerRadius}"
    Padding="4">
    <TreeView Background="Transparent"/>
</Border>"##;

/// A `Style` for placing a row's content across the row's full width.
///
/// WinUI's `TreeViewItem` binds the content's horizontal position to
/// `HorizontalContentAlignment` (the default template's `ContentPresenter` has
/// `HorizontalAlignment="{TemplateBinding HorizontalContentAlignment}"`). By default
/// it spreads only to the content's width, and the text does not use the rest of the
/// row's width. As with `List` and `Table` rows, it is set to `Stretch`.
///
/// `x:Key` must be the same string as [`ITEM_STYLE_KEY`].
const ITEM_STYLE_XAML: &str = r##"<ResourceDictionary
    xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
    xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml">
    <Style x:Key="NauiTreeItemStyle" TargetType="TreeViewItem">
        <Setter Property="HorizontalContentAlignment" Value="Stretch"/>
    </Style>
</ResourceDictionary>"##;

const ITEM_STYLE_KEY: &str = "NauiTreeItemStyle";

/// The `DataTemplate` applied to rows.
///
/// It just shows the element put on the node ([`TreeViewNode::Content`]) as is.
/// Without it, WinUI would try to turn the node itself into text and list type names.
const ITEM_TEMPLATE_XAML: &str = r##"<DataTemplate
    xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation">
    <ContentPresenter Content="{Binding Content}"/>
</DataTemplate>"##;

/// The opacity of the text of rows that cannot be selected. The same as `List`'s
/// disabled rows.
const DISABLED_OPACITY: f64 = 0.4;

/// The cover wrapping the content of rows that cannot be selected.
///
/// Even when transparent, being filled gives it a hit area, so it absorbs presses
/// across the full row width. The chevron is on the `TreeViewItem` side, outside
/// this cover, so branches that cannot be selected can still be expanded.
const BLOCKER_XAML: &str = r##"<Grid
    xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
    Background="Transparent"
    HorizontalAlignment="Stretch" VerticalAlignment="Stretch"/>"##;

/// The notification receiving the path of the expanded or collapsed branch and
/// whether it expanded.
type ExpandCallback = dyn FnMut(&[usize], bool);

/// The handler notified when expansion changes.
///
/// WinRT delegates require `Send + Sync`, so it is placed in a `UiThreadCell`. The
/// closure is taken out only for the duration of the call, so operating on the same
/// tree from inside the notification does not double-borrow.
#[derive(Clone)]
struct ExpandHandler(HandlerCell<ExpandCallback>);

impl ExpandHandler {
    fn new() -> Self {
        Self(Arc::new(UiThreadCell::new(None)))
    }

    fn set(&self, f: impl FnMut(&[usize], bool) + 'static) {
        self.0.with_mut(|slot| *slot = Some(Box::new(f)));
    }

    fn emit(&self, path: &[usize], expanded: bool) {
        let Some(mut f) = self.0.with_mut(|slot| slot.take()) else {
            return;
        };
        f(path, expanded);
        self.0.with_mut(|slot| {
            if slot.is_none() {
                *slot = Some(f);
            }
        });
    }
}

struct TreeInner {
    native: Border,
    tree_view: TreeView,
    /// The registration with the wheel helper. It cannot be made until the template's
    /// `ScrollViewer` appears, so it waits for `Loaded`.
    wheel: RefCell<Option<Rc<ListScrollTarget>>>,
    /// The destination of a [`Tree::scroll_to_item`] requested before showing. Scrolled
    /// to on `Loaded`, when the inner list is assembled.
    pending_scroll: RefCell<Option<Vec<usize>>>,
    items: RefCell<Vec<TreeItem>>,
    /// The nodes for the items. In depth-first order by path (parents first), that is,
    /// lexicographic order of paths, so lookups by path can use binary search.
    nodes: RefCell<Vec<(Vec<usize>, TreeViewNode)>>,
    /// The items remembered as expanded. Those inside collapsed branches remain too.
    ///
    /// WinUI holds the same state, but this one is used **to decide whether to send a
    /// notification**. Matching what we wrote ourselves against what arrived from user
    /// actions prevents notifying twice for the same expansion.
    expanded: RefCell<HashSet<Vec<usize>>>,
    /// The selected item. `None` for no selection.
    selected: RefCell<Option<Vec<usize>>>,
    handler: SelectionHandler,
    expand: ExpandHandler,
    /// Stops notifications only while selection or expansion is being changed from
    /// code.
    silent: Rc<Cell<bool>>,
    /// State for the window-wide wheel helper to choose this ScrollViewer.
    hovered: Arc<UiThreadCell<usize>>,
}

/// A list whose nested items can be expanded and collapsed (TreeView).
///
/// Items are referred to by the sequence of child indices from the root (a path).
/// `[0, 2]` is "the third child of the first root", and an empty path means "no
/// selection".
///
/// To fix its height regardless of the number of rows, specify it with
/// `set_sizing`.
#[derive(Clone)]
pub struct Tree(Rc<TreeInner>);
impl_widget!(Tree, native);

impl Tree {
    pub(crate) fn new() -> Result<Self> {
        let (native, tree_view) = build_surface()?;
        tree_view
            .SetSelectionMode(TreeViewSelectionMode::Single)
            .map_err(|e| to_error("set the tree selection mode", e))?;
        match item_template() {
            Ok(template) => tree_view
                .SetItemTemplate(&template)
                .map_err(|e| to_error("set the tree row template", e))?,
            // 読めなくても動きは保てるので、見た目を諦めて先へ進む。
            Err(error) => {
                eprintln!("naui-windows: failed to create the tree row template: {error}")
            }
        }
        let hovered = Arc::new(UiThreadCell::new(0));

        let this = Self(Rc::new(TreeInner {
            native,
            tree_view,
            wheel: RefCell::new(None),
            pending_scroll: RefCell::new(None),
            items: RefCell::new(Vec::new()),
            nodes: RefCell::new(Vec::new()),
            expanded: RefCell::new(HashSet::new()),
            selected: RefCell::new(None),
            handler: SelectionHandler::new(),
            expand: ExpandHandler::new(),
            silent: Rc::new(Cell::new(false)),
            hovered,
        }));

        this.install_selection_handler()?;
        this.install_expand_handlers()?;
        this.install_pointer_handlers()?;
        this.install_wheel_target()?;
        Ok(this)
    }

    /// Rebuilds the items. Paths change meaning, so the selection is cleared.
    ///
    /// Expansion returns to what [`TreeItem::expanded`] says.
    pub fn set_items(&self, items: &[TreeItem]) {
        *self.0.items.borrow_mut() = items.to_vec();
        *self.0.selected.borrow_mut() = None;
        let mut expanded = HashSet::new();
        TreeItem::walk(items, |path, item| {
            if item.expanded && !item.is_leaf() {
                expanded.insert(path.to_vec());
            }
        });
        *self.0.expanded.borrow_mut() = expanded;
        if let Err(error) = self.rebuild() {
            eprintln!("naui-windows: failed to build the tree: {error}");
        }
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
        self.0.selected.borrow().clone()
    }

    /// Scrolls until the item at `path` is visible. The selection does not change.
    ///
    /// If it is inside a collapsed branch, its ancestors are expanded first (as with
    /// [`set_selected`](Self::set_selected), expansion is not notified). If it is
    /// already visible nothing moves. Does nothing for a path that does not exist. When
    /// called before showing, it scrolls once shown.
    pub fn scroll_to_item(&self, path: &[usize]) {
        if TreeItem::at(&self.0.items.borrow(), path).is_none() {
            return;
        }
        // 開くと WinUI が開閉を知らせてくるので、ここでは通知を止める。
        self.without_notifying(|this| {
            this.write_expanded(&path[..path.len().saturating_sub(1)], true)
        });
        let node = self
            .0
            .nodes
            .borrow()
            .iter()
            .find(|(candidate, _)| candidate.as_slice() == path)
            .map(|(_, node)| node.clone());
        let Some(node) = node else {
            return;
        };
        // 行の要素は見えている辺りの分しか作られない (仮想化) ので、要素では
        // なく、テンプレートの中の一覧 (`TreeViewList`) へ節そのものを渡して
        // 送らせる。一覧の項目は開いている節を平らに並べたものなので、祖先を
        // 開いた後なら節が入っている。既定の寄せ方は List と同じく「見えて
        // いれば動かさず、近いほうの端」。
        match crate::layout::descendant_of_type::<_, ListViewBase>(&self.0.tree_view) {
            Some(list) => {
                let _ = list.ScrollIntoView(&node);
            }
            // 中の一覧は `Loaded` まで組み上がらない。そこで送る。
            None => *self.0.pending_scroll.borrow_mut() = Some(path.to_vec()),
        }
    }

    /// Selects one item without notifying.
    ///
    /// Passing an unselectable item or a path that does not exist clears the
    /// selection. Items inside a collapsed branch are selected after their ancestors
    /// are expanded so they become visible.
    pub fn set_selected(&self, path: &[usize]) {
        self.write_selected(path);
    }

    /// Clears the selection without notifying.
    pub fn clear_selection(&self) {
        self.write_selected(&[]);
    }

    /// Selects one item through the same path as a user's selection (notifies).
    pub fn select(&self, path: &[usize]) {
        self.write_selected(path);
        // 同じ行を選び直すと `SelectionChanged` は起きないため、
        // 通知の回数をそろえてここで 1 回だけ出す。
        let actual = self.selected().unwrap_or_default();
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

    /// The inner `TreeView`. Public as a backend-specific escape hatch.
    pub fn native_tree_view(&self) -> TreeView {
        self.0.tree_view.clone()
    }

    // -------------------------------------------------------------- 購読

    /// The selection subscription.
    ///
    /// Holding the handle strongly would create a cycle with the subscription, so it is
    /// held weakly. Rewriting the selection inside the notification fires
    /// `SelectionChanged` again on the spot, so with `with_mut` a double-borrow panic
    /// would cross the WinRT boundary and crash. `try_with_mut`, which can treat
    /// re-entrancy as a miss, is used (on re-entry `silent` is set, so it is a
    /// notification to be discarded anyway).
    fn install_selection_handler(&self) -> Result<()> {
        let state = UiThreadCell::new(Rc::downgrade(&self.0));
        let handler = TypedEventHandler::<TreeView, TreeViewSelectionChangedEventArgs>::new(
            move |_sender, _args| {
                let _ = state.try_with_mut(|weak| {
                    if let Some(inner) = weak.upgrade() {
                        let tree = Tree(inner);
                        if !tree.0.silent.get() {
                            tree.on_native_selection();
                        }
                    }
                });
                Ok(())
            },
        );
        self.0
            .tree_view
            .SelectionChanged(&handler)
            .map_err(|e| to_error("subscribe to the tree", e))?;
        Ok(())
    }

    /// The expansion subscriptions. Expanding and collapsing come as different events.
    fn install_expand_handlers(&self) -> Result<()> {
        let state = UiThreadCell::new(Rc::downgrade(&self.0));
        let expanding =
            TypedEventHandler::<TreeView, TreeViewExpandingEventArgs>::new(move |_sender, args| {
                let node = args.as_ref().and_then(|args| args.Node().ok());
                let _ = state.try_with_mut(|weak| {
                    if let (Some(inner), Some(node)) = (weak.upgrade(), node) {
                        let tree = Tree(inner);
                        if !tree.0.silent.get() {
                            tree.on_native_expand(&node, true);
                        }
                    }
                });
                Ok(())
            });
        self.0
            .tree_view
            .Expanding(&expanding)
            .map_err(|e| to_error("subscribe to tree expanding and collapsing", e))?;

        let state = UiThreadCell::new(Rc::downgrade(&self.0));
        let collapsed =
            TypedEventHandler::<TreeView, TreeViewCollapsedEventArgs>::new(move |_sender, args| {
                let node = args.as_ref().and_then(|args| args.Node().ok());
                let _ = state.try_with_mut(|weak| {
                    if let (Some(inner), Some(node)) = (weak.upgrade(), node) {
                        let tree = Tree(inner);
                        if !tree.0.silent.get() {
                            tree.on_native_expand(&node, false);
                        }
                    }
                });
                Ok(())
            });
        self.0
            .tree_view
            .Collapsed(&collapsed)
            .map_err(|e| to_error("subscribe to tree expanding and collapsing", e))?;
        Ok(())
    }

    /// Registers the `ScrollViewer` inside the template as a destination of the wheel
    /// helper.
    ///
    /// The `TreeView`'s contents are not assembled until `Loaded`, so it waits for that.
    /// If it is not found it simply does not register; the control's own scrolling still
    /// works.
    fn install_wheel_target(&self) -> Result<()> {
        let state = UiThreadCell::new(Rc::downgrade(&self.0));
        let loaded = RoutedEventHandler::new(move |_, _| {
            let _ = state.try_with_mut(|weak| {
                if let Some(inner) = weak.upgrade() {
                    let tree = Tree(inner);
                    tree.register_wheel_target();
                    let pending = tree.0.pending_scroll.borrow_mut().take();
                    if let Some(path) = pending {
                        tree.scroll_to_item(&path);
                    }
                }
            });
            Ok(())
        });
        self.0
            .tree_view
            .Loaded(&loaded)
            .map_err(|e| to_error("subscribe to tree Loaded", e))?;
        Ok(())
    }

    /// Registers with the wheel helper, exactly once.
    fn register_wheel_target(&self) {
        if self.0.wheel.borrow().is_some() {
            return;
        }
        let Some(scroll) = crate::layout::scroll_viewer_within(&self.0.tree_view) else {
            return;
        };
        let target = crate::layout::register_list_scroll(scroll, self.0.hovered.clone());
        *self.0.wheel.borrow_mut() = Some(target);
    }

    /// The hover state used to choose the wheel's destination (the same as `List`).
    fn install_pointer_handlers(&self) -> Result<()> {
        let entered_state = self.0.hovered.clone();
        let entered = PointerEventHandler::new(move |_, _| {
            entered_state.with_mut(|hovered| *hovered = hovered.saturating_add(1));
            Ok(())
        });
        let exited_state = self.0.hovered.clone();
        let exited = PointerEventHandler::new(move |_, _| {
            exited_state.with_mut(|hovered| {
                if *hovered > 0 {
                    *hovered -= 1;
                }
            });
            Ok(())
        });
        let moved_state = self.0.hovered.clone();
        let moved = PointerEventHandler::new(move |_, _| {
            moved_state.with_mut(|hovered| {
                if *hovered == 0 {
                    *hovered = 1;
                }
            });
            Ok(())
        });
        self.0
            .native
            .PointerEntered(&entered)
            .map_err(|e| to_error("subscribe to tree pointer events", e))?;
        self.0
            .native
            .PointerExited(&exited)
            .map_err(|e| to_error("subscribe to tree pointer events", e))?;
        self.0
            .native
            .PointerMoved(&moved)
            .map_err(|e| to_error("subscribe to tree pointer events", e))?;
        Ok(())
    }

    // ------------------------------------------------------------ 組み立て

    /// Maps the items onto a tree of `TreeViewNode`s.
    fn rebuild(&self) -> Result<()> {
        let mut nodes = Vec::new();
        {
            let items = self.0.items.borrow();
            self.without_notifying(|this| -> Result<()> {
                this.0
                    .tree_view
                    .RootNodes()
                    .and_then(|roots| roots.Clear())
                    .map_err(|e| to_error("clear the tree", e))?;
                let mut path = Vec::new();
                append_nodes(&this.0.tree_view, None, &items, true, &mut path, &mut nodes)
            })?;
        }
        *self.0.nodes.borrow_mut() = nodes;

        // 覚えている開閉を写す。パスの辞書順は親が先なので、そのまま
        // 「祖先から順に開く」順になる。
        let mut expanded: Vec<Vec<usize>> = self.0.expanded.borrow().iter().cloned().collect();
        expanded.sort();
        for path in &expanded {
            self.write_native_expanded(path, true);
        }
        Ok(())
    }

    /// The node the path points at. `None` if there is none.
    fn node_at(&self, path: &[usize]) -> Option<TreeViewNode> {
        let nodes = self.0.nodes.borrow();
        let index = nodes
            .binary_search_by(|(candidate, _)| candidate.as_slice().cmp(path))
            .ok()?;
        nodes.get(index).map(|(_, node)| node.clone())
    }

    /// The path pointing at the node. `None` for nodes not in our table, such as those
    /// in notifications that arrive in the middle of a rebuild.
    fn path_of(&self, node: &TreeViewNode) -> Option<Vec<usize>> {
        self.0
            .nodes
            .borrow()
            .iter()
            .find(|(_, candidate)| candidate == node)
            .map(|(path, _)| path.clone())
    }

    // --------------------------------------------------------- 状態の反映

    /// Remembers and writes the selection (no notification).
    fn write_selected(&self, path: &[usize]) {
        let picked = TreeItem::selectable(&self.0.items.borrow(), path).then(|| path.to_vec());
        if let Some(path) = picked.as_deref() {
            // 見えていないと選んだことが分からないので、祖先を開く。
            // 葉には開閉が無いので、開くのは親から上だけ。
            self.write_expanded(&path[..path.len().saturating_sub(1)], true);
        }
        self.write_native_selection(picked.as_deref());
        *self.0.selected.borrow_mut() = picked;
    }

    /// Rewrites only the selection on the WinUI side (notifications are stopped).
    ///
    /// Nodes inside collapsed branches are not passed. WinUI's `SelectedNode` handles
    /// only visible nodes, and passing a hidden node makes it recurse internally until
    /// the stack overflows. naui keeps its own record separately, so
    /// [`Tree::restore_selection`] writes it back when the branch is reopened.
    fn write_native_selection(&self, path: Option<&[usize]>) {
        let node = path
            .filter(|path| self.is_path_shown(path))
            .and_then(|path| self.node_at(path));
        self.without_notifying(|this| match node {
            Some(node) => {
                let _ = this.0.tree_view.SetSelectedNode(&node);
            }
            // 選択を外す口は `SelectedNode` に無いので、選択の集合を空にする。
            None => {
                if let Ok(selected) = this.0.tree_view.SelectedNodes() {
                    let _ = selected.Clear();
                }
            }
        });
    }

    /// Remembers and applies the expansion state (no notification). Expanding also
    /// expands the ancestors.
    fn write_expanded(&self, path: &[usize], expanded: bool) {
        if TreeItem::at(&self.0.items.borrow(), path).is_none_or(|item| item.is_leaf()) {
            return;
        }
        let branches: Vec<Vec<usize>> = match expanded {
            // 祖先から順に開く。
            true => (1..=path.len())
                .map(|depth| path[..depth].to_vec())
                .collect(),
            false => vec![path.to_vec()],
        };
        {
            let mut set = self.0.expanded.borrow_mut();
            for branch in &branches {
                match expanded {
                    true => set.insert(branch.clone()),
                    false => set.remove(branch),
                };
            }
        }
        for branch in &branches {
            self.write_native_expanded(branch, expanded);
        }
        self.restore_selection();
    }

    /// Rewrites only the expansion on the WinUI side (notifications are stopped).
    fn write_native_expanded(&self, path: &[usize], expanded: bool) {
        let Some(node) = self.node_at(path) else {
            return;
        };
        self.without_notifying(|_| {
            let _ = node.SetIsExpanded(expanded);
        });
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
        let mut branches = Vec::new();
        TreeItem::walk(&self.0.items.borrow(), |path, item| {
            if !item.is_leaf() {
                branches.push(path.to_vec());
            }
        });
        {
            let mut set = self.0.expanded.borrow_mut();
            set.clear();
            if expanded {
                set.extend(branches.iter().cloned());
            }
        }
        // `walk` は親が先なので、開くときも祖先から順に届く。
        for path in &branches {
            self.write_native_expanded(path, expanded);
        }
        self.restore_selection();
    }

    /// Writes the remembered selection back to the WinUI side (no notification).
    ///
    /// When a branch is collapsed, WinUI discards the selection inside it. naui keeps it
    /// as "merely hidden", so the selection's look is restored here when the branch is
    /// reopened.
    ///
    /// **It writes only when they disagree.** Writing the selection also moves focus and
    /// the scroll position, so rewriting the same selection would make the screen jump
    /// to the selected row just from expanding an unrelated branch.
    fn restore_selection(&self) {
        // 見えていない選択は、WinUI 側では「選択なし」として表す。
        let wanted = self
            .0
            .selected
            .borrow()
            .clone()
            .filter(|path| self.is_path_shown(path));
        let current = self
            .0
            .tree_view
            .SelectedNode()
            .ok()
            .and_then(|node| self.path_of(&node));
        if current == wanted {
            return;
        }
        self.write_native_selection(wanted.as_deref());
    }

    /// Writes the remembered selection back after leaving the current event.
    ///
    /// Rewriting the selection inside a selection event makes WinUI reselect the
    /// clicked row and fire the event again, a back-and-forth that never stops until the
    /// stack overflows. So it is queued on the `DispatcherQueue` and written back
    /// outside the event.
    fn restore_selection_later(&self) {
        let Ok(queue) = DispatcherQueue::GetForCurrentThread() else {
            return;
        };
        let state = UiThreadCell::new(Rc::downgrade(&self.0));
        let work = DispatcherQueueHandler::new(move || {
            let _ = state.try_with_mut(|weak| {
                if let Some(inner) = weak.upgrade() {
                    Tree(inner).restore_selection();
                }
            });
            Ok(())
        });
        let _ = queue.TryEnqueue(&work);
    }

    /// Whether the path is visible now (whether all its ancestors are expanded).
    fn is_path_shown(&self, path: &[usize]) -> bool {
        let expanded = self.0.expanded.borrow();
        (1..path.len()).all(|depth| expanded.contains(&path[..depth]))
    }

    /// When the selection changes on the WinUI side.
    fn on_native_selection(&self) {
        let picked = self
            .0
            .tree_view
            .SelectedNode()
            .ok()
            .and_then(|node| self.path_of(&node));

        // 押下は覆い ([`pointer_blocker`]) で止めてあるが、矢印キーでは
        // 選べない項目にも選択が移る。その場合は直前の選択を書き戻して、
        // 選択状態を変えない。
        if picked
            .as_ref()
            .is_some_and(|path| !TreeItem::selectable(&self.0.items.borrow(), path))
        {
            self.restore_selection_later();
            return;
        }
        // 枝を閉じると WinUI は中の選択を捨てる。naui は「隠れているだけ」と
        // 見なすので、覚えたまま通知もしない (開き直すと戻る)。
        if picked.is_none()
            && self
                .0
                .selected
                .borrow()
                .as_deref()
                .is_some_and(|path| !self.is_path_shown(path))
        {
            return;
        }
        // 自分で書き換えた分が遅れて届くことがあるので、変わったときだけ
        // 通知する。
        if *self.0.selected.borrow() == picked {
            return;
        }
        *self.0.selected.borrow_mut() = picked;
        let actual = self.selected().unwrap_or_default();
        self.0.handler.emit(&actual);
    }

    /// When expansion changes on the WinUI side.
    fn on_native_expand(&self, node: &TreeViewNode, expanded: bool) {
        let Some(path) = self.path_of(node) else {
            return;
        };
        // 葉に開閉は無い。ただし `TreeViewItem` は子の無い節でも山形の位置に
        // 当たり判定を持っていて、そこを押すと `IsExpanded` が動く。naui では
        // 何も起きなかったことにする。
        if TreeItem::at(&self.0.items.borrow(), &path).is_none_or(|item| item.is_leaf()) {
            return;
        }
        let changed = {
            let mut set = self.0.expanded.borrow_mut();
            match expanded {
                true => set.insert(path.clone()),
                false => set.remove(&path),
            }
        };
        // 開いて出てきた選択の見た目を戻す (`restore_selection` 参照)。
        if expanded {
            self.restore_selection();
        }
        if changed {
            self.0.expand.emit(&path, expanded);
        }
    }

    /// Operates with notifications from WinUI stopped.
    fn without_notifying<R>(&self, f: impl FnOnce(&Self) -> R) -> R {
        let previous = self.0.silent.replace(true);
        let result = f(self);
        self.0.silent.set(previous);
        result
    }
}

impl Drop for TreeInner {
    fn drop(&mut self) {
        self.hovered.with_mut(|hovered| *hovered = 0);
    }
}

/// Maps items onto `TreeViewNode`s and adds them to the tree.
///
/// It walks depth-first (parents first), so `out` ends up in lexicographic order of
/// paths. `enabled` is whether all ancestors are enabled, the same check as
/// [`TreeItem::selectable`].
fn append_nodes(
    tree_view: &TreeView,
    parent: Option<&TreeViewNode>,
    items: &[TreeItem],
    enabled: bool,
    path: &mut Vec<usize>,
    out: &mut Vec<(Vec<usize>, TreeViewNode)>,
) -> Result<()> {
    let target = match parent {
        Some(node) => node.Children(),
        None => tree_view.RootNodes(),
    }
    .map_err(|e| to_error("get the tree children", e))?;
    for (index, item) in items.iter().enumerate() {
        path.push(index);
        let selectable = enabled && item.enabled;
        let node = TreeViewNode::new().map_err(|e| to_error("create the TreeViewNode", e))?;
        let content = row_content(item, selectable)?
            .cast::<IInspectable>()
            .map_err(|e| to_error("convert the row to UIElement", e))?;
        node.SetContent(&content)
            .map_err(|e| to_error("set the row content", e))?;
        target
            .Append(&node)
            .map_err(|e| to_error("add to the tree", e))?;
        out.push((path.clone(), node.clone()));
        append_nodes(
            tree_view,
            Some(&node),
            &item.children,
            selectable,
            path,
            out,
        )?;
        path.pop();
    }
    Ok(())
}

/// Builds a row's content. The text is built the same way as in `List` (two lines
/// if there is secondary text).
fn row_content(item: &TreeItem, selectable: bool) -> Result<UIElement> {
    let content = item_content(&item.label, item.detail.as_deref())?.native_element();
    if selectable {
        return Ok(content);
    }
    // 行そのものは無効にできない (枝の開閉まで止まる) ので、文字だけを
    // 無効な行と同じ濃さにしたうえで、押下を受け止める覆いで包む。
    let _ = content.SetOpacity(DISABLED_OPACITY);
    let blocker = pointer_blocker()?;
    blocker
        .Children()
        .and_then(|children| children.Append(&content))
        .map_err(|e| to_error("add to the row cover", e))?;
    blocker
        .cast::<UIElement>()
        .map_err(|e| to_error("convert the row to UIElement", e))
}

/// Creates the cover that wraps rows that cannot be selected.
///
/// Presses are swallowed here, so they never reach the `TreeViewItem` and the row is
/// not selected. Letting WinUI select it and then undoing the selection would visibly
/// move the selection and focus while undoing.
fn pointer_blocker() -> Result<XamlGrid> {
    let blocker = XamlReader::Load(&HSTRING::from(BLOCKER_XAML))
        .and_then(|element| element.cast::<XamlGrid>())
        .map_err(|e| to_error("create the row cover", e))?;
    let swallow = PointerEventHandler::new(|_, args| {
        if let Some(args) = args.as_ref() {
            let _ = args.SetHandled(true);
        }
        Ok(())
    });
    // 押した / 離したの両方を止める。`TreeViewItem` はどちらでも選ぶ。
    blocker
        .PointerPressed(&swallow)
        .map_err(|e| to_error("subscribe to the row cover", e))?;
    blocker
        .PointerReleased(&swallow)
        .map_err(|e| to_error("subscribe to the row cover", e))?;
    Ok(blocker)
}

/// Loads the themed frame. Falls back to a plain `Border` + `TreeView` if it cannot
/// be read.
fn build_surface() -> Result<(Border, TreeView)> {
    let surface = match load_surface() {
        Ok(surface) => surface,
        Err(error) => {
            eprintln!("naui-windows: failed to create the themed tree frame: {error}");
            plain_surface()?
        }
    };
    // 枠がテーマ付きでも素でも、行の寄せ方は同じにする。
    if let Some(style) = item_style() {
        let _ = surface.1.SetItemContainerStyle(&style);
    }
    Ok(surface)
}

/// The `Style` applied to rows. `None` if it cannot be read (WinUI's default
/// alignment stays).
fn item_style() -> Option<Style> {
    let dictionary = match XamlReader::Load(&HSTRING::from(ITEM_STYLE_XAML))
        .and_then(|element| element.cast::<ResourceDictionary>())
    {
        Ok(dictionary) => dictionary,
        Err(error) => {
            eprintln!("naui-windows: failed to create the tree row style: {error}");
            return None;
        }
    };
    PropertyValue::CreateString(&HSTRING::from(ITEM_STYLE_KEY))
        .and_then(|key| dictionary.Lookup(&key))
        .and_then(|style| style.cast::<Style>())
        .ok()
}

fn load_surface() -> Result<(Border, TreeView)> {
    let native = XamlReader::Load(&HSTRING::from(SURFACE_XAML))
        .and_then(|element| element.cast::<Border>())
        .map_err(|e| to_error("create the tree frame", e))?;
    let tree_view = native
        .Child()
        .and_then(|child| child.cast::<TreeView>())
        .map_err(|e| to_error("get the tree's TreeView", e))?;
    Ok((native, tree_view))
}

fn plain_surface() -> Result<(Border, TreeView)> {
    let tree_view = TreeView::new().map_err(|e| to_error("create the TreeView", e))?;
    let native = Border::new().map_err(|e| to_error("create the tree Border", e))?;
    let element = tree_view
        .cast::<UIElement>()
        .map_err(|e| to_error("convert the TreeView to UIElement", e))?;
    native
        .SetChild(&element)
        .map_err(|e| to_error("add to the tree Border", e))?;
    Ok((native, tree_view))
}

/// The `DataTemplate` applied to rows.
fn item_template() -> Result<DataTemplate> {
    XamlReader::Load(&HSTRING::from(ITEM_TEMPLATE_XAML))
        .and_then(|element| element.cast::<DataTemplate>())
        .map_err(|e| to_error("create the tree row template", e))
}
