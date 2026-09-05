//! ツリー (WinUI 3)。
//!
//! WinUI 標準の `TreeView` をそのまま使う。項目は `TreeViewNode` に写して
//! 木のまま渡すので、段付け・開閉の山形・キーボード操作・選択の見た目は
//! すべて WinUI が持つ。
//!
//! | 部分 | 作り |
//! | --- | --- |
//! | 枠 | `List` と同じ色・角丸を持たせた `Border` |
//! | 行の中身 | `TreeViewNode.Content` に組み立てた要素を載せ、`ItemTemplate` で出す |
//! | 段付け・開閉 | `TreeView` (`Expanding` / `Collapsed` で通知を受ける) |
//! | 選択 | `TreeView` の `SelectedNode` (`SelectionChanged` で通知を受ける) |
//!
//! スクロールは `TreeView` が自分で持つ (`List` のように外側の `ScrollViewer`
//! へ預けることはできない。中の一覧は与えられた高さぶんしか並べないので、
//! 外へ預けると伸びずに切れてしまう)。ウィンドウ全体のホイール補助
//! (`crate::layout`) には、テンプレートの中にある `ScrollViewer` を
//! 見つけて登録する。
//!
//! naui の [`TreeItem::enabled`] は WinUI に対応するものが無い
//! (`TreeViewItem` を無効にすると、枝では開閉もできなくなる)。そこで
//! **文字を薄くしたうえで、行の中身に押下を受け止める覆いをかぶせる**。
//! 覆いがかかるのは開閉の山形より右だけなので、選べない枝も開閉はできる。

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;
use std::sync::Arc;

use naui_core::{Result, TreeItem};
use naui_winui3::Microsoft::UI::Dispatching::{DispatcherQueue, DispatcherQueueHandler};
use naui_winui3::Microsoft::UI::Xaml::Controls::{
    Border, Grid as XamlGrid, Orientation as XamlOrientation, ScrollViewer, StackPanel, TreeView,
    TreeViewCollapsedEventArgs, TreeViewExpandingEventArgs, TreeViewNode,
    TreeViewSelectionChangedEventArgs, TreeViewSelectionMode,
};
use naui_winui3::Microsoft::UI::Xaml::Input::PointerEventHandler;
use naui_winui3::Microsoft::UI::Xaml::Markup::XamlReader;
use naui_winui3::Microsoft::UI::Xaml::Media::VisualTreeHelper;
use naui_winui3::Microsoft::UI::Xaml::{
    DataTemplate, DependencyObject, RoutedEventHandler, UIElement,
};
use windows::Foundation::TypedEventHandler;
use windows_core::{IInspectable, Interface, HSTRING};

use crate::layout::ListScrollTarget;
use crate::list::{text_block, SelectionHandler};
use crate::to_error;
use crate::ui_thread::{HandlerCell, UiThreadCell};
use crate::widgets::{impl_widget, Widget};

/// テーマ付きの枠。色と角丸は `List` の枠と同じ (`crate::list`)。
const SURFACE_XAML: &str = r##"<Border
    xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
    Background="{ThemeResource ControlFillColorDefaultBrush}"
    BorderBrush="{ThemeResource ControlStrokeColorDefaultBrush}"
    BorderThickness="1"
    CornerRadius="{ThemeResource ControlCornerRadius}"
    Padding="4">
    <TreeView Background="Transparent"/>
</Border>"##;

/// 行に当てる `DataTemplate`。
///
/// 節に載せた要素 ([`TreeViewNode::Content`]) をそのまま出すだけ。これが
/// 無いと、WinUI は節そのものを文字にしようとして型名を並べてしまう。
const ITEM_TEMPLATE_XAML: &str = r##"<DataTemplate
    xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation">
    <ContentPresenter Content="{Binding Content}"/>
</DataTemplate>"##;

/// 選べない行の文字の濃さ。`List` の無効な行と同じ。
const DISABLED_OPACITY: f64 = 0.4;

/// 選べない行の中身を包む覆い。
///
/// 透明でも塗ってあれば当たり判定が出るので、行の幅いっぱいで押下を
/// 受け止められる。山形は `TreeViewItem` 側にあってこの覆いの外なので、
/// 選べない枝でも開閉はできる。
const BLOCKER_XAML: &str = r##"<Grid
    xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
    Background="Transparent"
    HorizontalAlignment="Stretch" VerticalAlignment="Stretch"/>"##;

/// 開閉した枝の道筋と、開いたかどうかを受け取る通知。
type ExpandCallback = dyn FnMut(&[usize], bool);

/// 開閉が変わったことの通知先。
///
/// WinRT のデリゲートは `Send + Sync` を要求するため `UiThreadCell` に載せる。
/// 呼び出しの間だけクロージャを取り出すので、通知の中から同じツリーを
/// 操作しても二重借用にならない。
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
    /// ホイール補助への登録。テンプレートの `ScrollViewer` が現れるまで
    /// できないので、`Loaded` を待って入れる。
    wheel: RefCell<Option<Rc<ListScrollTarget>>>,
    items: RefCell<Vec<TreeItem>>,
    /// 項目に対応する節。パスの深さ優先 (親が先) 順、つまりパスの辞書順に
    /// 並ぶので、パスからの引きは二分探索でよい。
    nodes: RefCell<Vec<(Vec<usize>, TreeViewNode)>>,
    /// 開いた状態として覚えている項目。閉じた枝の中の分も残る。
    ///
    /// WinUI 側にも同じ状態があるが、こちらは**通知を出すかどうかの判断**に
    /// 使う。自分で書き換えた分と、利用者の操作で届いた分を突き合わせると、
    /// 同じ開閉で二重に通知するのを防げる。
    expanded: RefCell<HashSet<Vec<usize>>>,
    /// 選ばれている項目。選択なしは `None`。
    selected: RefCell<Option<Vec<usize>>>,
    handler: SelectionHandler,
    expand: ExpandHandler,
    /// プログラムから選択や開閉を変えている間だけ通知を止める。
    silent: Rc<Cell<bool>>,
    /// ウィンドウ全体のホイール補助がこの ScrollViewer を選ぶための状態。
    hovered: Arc<UiThreadCell<usize>>,
}

/// 入れ子の項目を開閉できる一覧 (TreeView)。
///
/// 項目は根からの子インデックスの並び (パス) で指す。`[0, 2]` は
/// 「1 番目の根の 3 番目の子」で、空のパスは「選択なし」を表す。
///
/// 高さは行数に関係なく固定したいときに `set_sizing` で指定する。
#[derive(Clone)]
pub struct Tree(Rc<TreeInner>);
impl_widget!(Tree, native);

impl Tree {
    pub(crate) fn new() -> Result<Self> {
        let (native, tree_view) = build_surface()?;
        tree_view
            .SetSelectionMode(TreeViewSelectionMode::Single)
            .map_err(|e| to_error("ツリーの選択方法の設定", e))?;
        match item_template() {
            Ok(template) => tree_view
                .SetItemTemplate(&template)
                .map_err(|e| to_error("ツリーの行のテンプレートの設定", e))?,
            // 読めなくても動きは保てるので、見た目を諦めて先へ進む。
            Err(error) => eprintln!("naui-windows: ツリーの行のテンプレートの生成に失敗: {error}"),
        }
        let hovered = Arc::new(UiThreadCell::new(0));

        let this = Self(Rc::new(TreeInner {
            native,
            tree_view,
            wheel: RefCell::new(None),
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

    /// 項目を作り直す。パスの意味が変わるため、選択は外れる。
    ///
    /// 開閉は [`TreeItem::expanded`] のとおりに戻る。
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
            eprintln!("naui-windows: ツリーの組み立てに失敗: {error}");
        }
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
        self.0.selected.borrow().clone()
    }

    /// 通知せずに 1 項目を選ぶ。
    ///
    /// 選べない項目や無いパスを渡すと、選択は外れる。閉じた枝の中にある
    /// 項目は、見えるように祖先を開いてから選ぶ。
    pub fn set_selected(&self, path: &[usize]) {
        self.write_selected(path);
    }

    /// 通知せずに選択を外す。
    pub fn clear_selection(&self) {
        self.write_selected(&[]);
    }

    /// ユーザーが選んだのと同じ経路で 1 項目を選ぶ (通知あり)。
    pub fn select(&self, path: &[usize]) {
        self.write_selected(path);
        // 同じ行を選び直すと `SelectionChanged` は起きないため、
        // 通知の回数をそろえてここで 1 回だけ出す。
        let actual = self.selected().unwrap_or_default();
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
        self.write_expanded(path, expanded);
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
        self.set_all(true);
    }

    /// 通知せずにすべての枝を閉じる。
    pub fn collapse_all(&self) {
        self.set_all(false);
    }

    /// 開閉が変わったときに、その項目のパスと変わった後の状態で呼ばれる。
    pub fn on_expand(&self, f: impl FnMut(&[usize], bool) + 'static) {
        self.0.expand.set(f);
    }

    /// 中身の `TreeView`。バックエンド固有の脱出口として公開している。
    pub fn native_tree_view(&self) -> TreeView {
        self.0.tree_view.clone()
    }

    // -------------------------------------------------------------- 購読

    /// 選択の購読。
    ///
    /// ハンドルを強く持つと購読との間で循環するため、弱参照にする。
    /// 通知の中から選択を書き換えると `SelectionChanged` がその場でもう一度
    /// 起きるため、`with_mut` では二重借用の panic が WinRT の境界を越えて
    /// クラッシュになる。再入を取りこぼしとして扱える `try_with_mut` を使う
    /// (再入時は `silent` が立っていて、どのみち捨てる通知になる)。
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
            .map_err(|e| to_error("ツリーの購読", e))?;
        Ok(())
    }

    /// 開閉の購読。開くときと閉じるときで別のイベントが来る。
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
            .map_err(|e| to_error("ツリーの開閉の購読", e))?;

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
            .map_err(|e| to_error("ツリーの開閉の購読", e))?;
        Ok(())
    }

    /// テンプレートの中の `ScrollViewer` を、ホイール補助の行き先として登録する。
    ///
    /// `TreeView` の中身は `Loaded` まで組み上がらないので、そこまで待つ。
    /// 見つからなければ登録しないだけで、コントロール自身のスクロールは動く。
    fn install_wheel_target(&self) -> Result<()> {
        let state = UiThreadCell::new(Rc::downgrade(&self.0));
        let loaded = RoutedEventHandler::new(move |_, _| {
            let _ = state.try_with_mut(|weak| {
                if let Some(inner) = weak.upgrade() {
                    Tree(inner).register_wheel_target();
                }
            });
            Ok(())
        });
        self.0
            .tree_view
            .Loaded(&loaded)
            .map_err(|e| to_error("ツリーの表示の購読", e))?;
        Ok(())
    }

    /// ホイール補助への登録を 1 回だけ行う。
    fn register_wheel_target(&self) {
        if self.0.wheel.borrow().is_some() {
            return;
        }
        let Some(scroll) = scroll_viewer_within(&self.0.tree_view) else {
            return;
        };
        let target = crate::layout::register_list_scroll(scroll, self.0.hovered.clone());
        *self.0.wheel.borrow_mut() = Some(target);
    }

    /// ホイールの行き先を選ばせるためのホバー状態 (`List` と同じ)。
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
            .map_err(|e| to_error("ツリーのポインター購読", e))?;
        self.0
            .native
            .PointerExited(&exited)
            .map_err(|e| to_error("ツリーのポインター購読", e))?;
        self.0
            .native
            .PointerMoved(&moved)
            .map_err(|e| to_error("ツリーのポインター購読", e))?;
        Ok(())
    }

    // ------------------------------------------------------------ 組み立て

    /// 項目を `TreeViewNode` の木へ写す。
    fn rebuild(&self) -> Result<()> {
        let mut nodes = Vec::new();
        {
            let items = self.0.items.borrow();
            self.without_notifying(|this| -> Result<()> {
                this.0
                    .tree_view
                    .RootNodes()
                    .and_then(|roots| roots.Clear())
                    .map_err(|e| to_error("ツリーの消去", e))?;
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

    /// パスの指す節。無ければ `None`。
    fn node_at(&self, path: &[usize]) -> Option<TreeViewNode> {
        let nodes = self.0.nodes.borrow();
        let index = nodes
            .binary_search_by(|(candidate, _)| candidate.as_slice().cmp(path))
            .ok()?;
        nodes.get(index).map(|(_, node)| node.clone())
    }

    /// その節を指すパス。作り直しの途中で届いた通知など、こちらの表に
    /// 無い節なら `None`。
    fn path_of(&self, node: &TreeViewNode) -> Option<Vec<usize>> {
        self.0
            .nodes
            .borrow()
            .iter()
            .find(|(_, candidate)| candidate == node)
            .map(|(path, _)| path.clone())
    }

    // --------------------------------------------------------- 状態の反映

    /// 選択を覚えて書き込む (通知は起きない)。
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

    /// WinUI 側の選択だけを書き換える (通知は止める)。
    ///
    /// 閉じた枝の中にある節は渡さない。WinUI の `SelectedNode` は見えている
    /// 節しか扱えず、隠れている節を渡すと内側で再帰してスタックを溢れさせる。
    /// naui 側の記憶は別に持っているので、開き直したときに
    /// [`Tree::restore_selection`] が書き戻す。
    fn write_native_selection(&self, path: Option<&[usize]>) {
        let node = path
            .filter(|path| self.is_visible(path))
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

    /// 開閉を覚えて反映する (通知は起きない)。開くときは祖先もまとめて開く。
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

    /// WinUI 側の開閉だけを書き換える (通知は止める)。
    fn write_native_expanded(&self, path: &[usize], expanded: bool) {
        let Some(node) = self.node_at(path) else {
            return;
        };
        self.without_notifying(|_| {
            let _ = node.SetIsExpanded(expanded);
        });
    }

    /// 開閉を変えて 1 回だけ通知する。
    fn toggle(&self, path: &[usize], expanded: bool) {
        let before = self.is_expanded(path);
        self.write_expanded(path, expanded);
        if self.is_expanded(path) != before {
            self.0.expand.emit(path, expanded);
        }
    }

    /// すべての枝をまとめて開閉する (通知なし)。
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

    /// 覚えている選択を WinUI 側へ書き戻す (通知は起きない)。
    ///
    /// WinUI は枝を閉じると、その中にある選択を捨てる。naui では
    /// 「隠れているだけ」として覚えたままにするので、開き直したときに
    /// ここで選択の見た目を戻す。
    ///
    /// **食い違っているときだけ書く。** 選択の書き込みは焦点とスクロール位置
    /// まで動かすので、同じ選択を書き直すと、関係のない枝を開いただけで
    /// 選ばれている行へ画面が飛んでしまう。
    fn restore_selection(&self) {
        // 見えていない選択は、WinUI 側では「選択なし」として表す。
        let wanted = self
            .0
            .selected
            .borrow()
            .clone()
            .filter(|path| self.is_visible(path));
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

    /// 覚えている選択を、いまのイベントから抜けてから書き戻す。
    ///
    /// 選択のイベントの中で選択を書き換えると、WinUI が押された行を選び直して
    /// またイベントを出す、という往復が止まらずスタックを溢れさせる。
    /// `DispatcherQueue` に積んで、イベントの外で書き戻す。
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

    /// そのパスが今見えているか (祖先がすべて開いているか)。
    fn is_visible(&self, path: &[usize]) -> bool {
        let expanded = self.0.expanded.borrow();
        (1..path.len()).all(|depth| expanded.contains(&path[..depth]))
    }

    /// WinUI 側で選択が変わったとき。
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
                .is_some_and(|path| !self.is_visible(path))
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

    /// WinUI 側で開閉が変わったとき。
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

    /// WinUI からの通知を止めたまま操作する。
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

/// 項目を `TreeViewNode` に写して木へ足す。
///
/// 深さ優先 (親が先) にたどるので、`out` はパスの辞書順に並ぶ。`enabled` は
/// 祖先がすべて有効かどうかで、[`TreeItem::selectable`] と同じ判定になる。
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
    .map_err(|e| to_error("ツリーの子の取得", e))?;
    for (index, item) in items.iter().enumerate() {
        path.push(index);
        let selectable = enabled && item.enabled;
        let node = TreeViewNode::new().map_err(|e| to_error("TreeViewNode の生成", e))?;
        let content = row_content(item, selectable)?
            .cast::<IInspectable>()
            .map_err(|e| to_error("行の要素化", e))?;
        node.SetContent(&content)
            .map_err(|e| to_error("行の内容設定", e))?;
        target
            .Append(&node)
            .map_err(|e| to_error("ツリーへの追加", e))?;
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

/// 行の中身を組み立てる。文字は `List` と同じ組み方 (補助があれば 2 行)。
fn row_content(item: &TreeItem, selectable: bool) -> Result<UIElement> {
    let title = text_block(&item.label, false)?;
    let content: UIElement = match &item.detail {
        None => title
            .cast::<UIElement>()
            .map_err(|e| to_error("行の要素化", e))?,
        Some(detail) => {
            let stack = StackPanel::new().map_err(|e| to_error("行の StackPanel の生成", e))?;
            stack
                .SetOrientation(XamlOrientation::Vertical)
                .map_err(|e| to_error("行の向き設定", e))?;
            let children = stack
                .Children()
                .map_err(|e| to_error("行の中身の取得", e))?;
            children
                .Append(
                    &title
                        .cast::<UIElement>()
                        .map_err(|e| to_error("行の要素化", e))?,
                )
                .map_err(|e| to_error("行への追加", e))?;
            children
                .Append(
                    &text_block(detail, true)?
                        .cast::<UIElement>()
                        .map_err(|e| to_error("行の要素化", e))?,
                )
                .map_err(|e| to_error("行への追加", e))?;
            stack
                .cast::<UIElement>()
                .map_err(|e| to_error("行の要素化", e))?
        }
    };
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
        .map_err(|e| to_error("行の覆いへの追加", e))?;
    blocker
        .cast::<UIElement>()
        .map_err(|e| to_error("行の要素化", e))
}

/// 選べない行を包む覆いを作る。
///
/// 押下をここで握りつぶすので、`TreeViewItem` まで届かず行が選ばれない。
/// WinUI に選ばせてから選択を戻すやりかたでは、戻すあいだに選択と焦点が
/// 目に見えて動いてしまう。
fn pointer_blocker() -> Result<XamlGrid> {
    let blocker = XamlReader::Load(&HSTRING::from(BLOCKER_XAML))
        .and_then(|element| element.cast::<XamlGrid>())
        .map_err(|e| to_error("行の覆いの生成", e))?;
    let swallow = PointerEventHandler::new(|_, args| {
        if let Some(args) = args.as_ref() {
            let _ = args.SetHandled(true);
        }
        Ok(())
    });
    // 押した / 離したの両方を止める。`TreeViewItem` はどちらでも選ぶ。
    blocker
        .PointerPressed(&swallow)
        .map_err(|e| to_error("行の覆いの購読", e))?;
    blocker
        .PointerReleased(&swallow)
        .map_err(|e| to_error("行の覆いの購読", e))?;
    Ok(blocker)
}

/// テーマ付きの枠を読み込む。読めなければ素の `Border` + `TreeView` に戻す。
fn build_surface() -> Result<(Border, TreeView)> {
    match load_surface() {
        Ok(surface) => Ok(surface),
        Err(error) => {
            eprintln!("naui-windows: ツリーのテーマ付き枠の生成に失敗: {error}");
            plain_surface()
        }
    }
}

fn load_surface() -> Result<(Border, TreeView)> {
    let native = XamlReader::Load(&HSTRING::from(SURFACE_XAML))
        .and_then(|element| element.cast::<Border>())
        .map_err(|e| to_error("ツリーの枠の生成", e))?;
    let tree_view = native
        .Child()
        .and_then(|child| child.cast::<TreeView>())
        .map_err(|e| to_error("ツリーの TreeView の取得", e))?;
    Ok((native, tree_view))
}

fn plain_surface() -> Result<(Border, TreeView)> {
    let tree_view = TreeView::new().map_err(|e| to_error("TreeView の生成", e))?;
    let native = Border::new().map_err(|e| to_error("ツリーの Border の生成", e))?;
    let element = tree_view
        .cast::<UIElement>()
        .map_err(|e| to_error("TreeView の要素化", e))?;
    native
        .SetChild(&element)
        .map_err(|e| to_error("ツリーの Border への追加", e))?;
    Ok((native, tree_view))
}

/// その要素の中にある最初の `ScrollViewer` を、深さ優先で探す。
///
/// `TreeView` は `TreeViewList` を持ち、その中に `ScrollViewer` がある。
/// 段数はテンプレート次第なので、名前ではなく型で探す。
fn scroll_viewer_within(root: &TreeView) -> Option<ScrollViewer> {
    let root = root.cast::<DependencyObject>().ok()?;
    let mut queue = vec![root];
    while let Some(element) = queue.pop() {
        if let Ok(scroll) = element.cast::<ScrollViewer>() {
            return Some(scroll);
        }
        let count = VisualTreeHelper::GetChildrenCount(&element).unwrap_or(0);
        for index in (0..count).rev() {
            if let Ok(child) = VisualTreeHelper::GetChild(&element, index) {
                queue.push(child);
            }
        }
    }
    None
}

/// 行に当てる `DataTemplate`。
fn item_template() -> Result<DataTemplate> {
    XamlReader::Load(&HSTRING::from(ITEM_TEMPLATE_XAML))
        .and_then(|element| element.cast::<DataTemplate>())
        .map_err(|e| to_error("ツリーの行のテンプレートの生成", e))
}
