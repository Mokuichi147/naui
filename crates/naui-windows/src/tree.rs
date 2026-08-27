//! ツリー (WinUI 3)。
//!
//! `TreeView` は `winio-winui3` 0.4.5 のバインディングに含まれていないため、
//! `List` と同じ `ScrollViewer` + `ListBox` の上に組み立てている。
//!
//! | 部分 | 作り |
//! | --- | --- |
//! | 枠と行の見た目 | `List` と同じ (`crate::list` のテーマ付き枠と行スタイル) |
//! | 段付け | 行の中身に左余白を付ける |
//! | 開閉 | 行の左端に置いた `Button` (Segoe Fluent Icons の山形) |
//!
//! 行は**全項目ぶんを作り置き**し、開閉では見え隠れ (`Visibility`) だけを
//! 切り替える。押された開閉ボタン自身が作り直しで消えないので、Click の
//! 最中に行を捨てずに済む。選択・キーボード操作・スクロールは
//! `ListBox` と `ScrollViewer` が行う。

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;
use std::sync::Arc;

use naui_core::{Result, TreeItem};
use windows::Foundation::PropertyValue;
use windows_core::{IInspectable, Interface, HSTRING};
use winui3::Microsoft::UI::Xaml::Controls::{
    Button, ListBox as XamlListBox, ListBoxItem, Orientation as XamlOrientation,
    ScrollBarVisibility, ScrollViewer, SelectionChangedEventHandler,
    SelectionMode as XamlSelectionMode, StackPanel, TextBlock,
};
use winui3::Microsoft::UI::Xaml::Input::PointerEventHandler;
use winui3::Microsoft::UI::Xaml::Markup::XamlReader;
use winui3::Microsoft::UI::Xaml::{RoutedEventHandler, Style, Thickness, UIElement, Visibility};

use crate::layout::ListScrollTarget;
use crate::list::{build_surface, row_style, text_block, SelectionHandler};
use crate::to_error;
use crate::ui_thread::UiThreadCell;
use crate::widgets::{impl_widget, Widget};

/// 1 段ぶんの字下げ (論理ピクセル)。
const INDENT: f64 = 16.0;

/// 開閉のボタン。山形は [`set_twisty_glyph`] が入れる。
///
/// `List` の行と同じく `{ThemeResource}` で色を引くので、ライト / ダークの
/// 切り替えにそのまま追従する。
const TWISTY_XAML: &str = r##"<Button
    xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
    Background="Transparent" BorderThickness="0" Padding="0"
    Width="20" Height="20" MinWidth="20" MinHeight="20"
    VerticalAlignment="Center" FontFamily="Segoe Fluent Icons" FontSize="10"
    Foreground="{ThemeResource TextFillColorSecondaryBrush}"/>"##;

/// 閉じているときの山形 (Segoe Fluent Icons の ChevronRight)。
const GLYPH_COLLAPSED: &str = "\u{E76C}";
/// 開いているときの山形 (ChevronDown)。
const GLYPH_EXPANDED: &str = "\u{E70D}";

/// 開閉のボタンと同じ幅。葉の行で、文字の左端をそろえるために空ける。
const TWISTY_WIDTH: f64 = 20.0;

/// 開閉が変わったことの通知先。
///
/// WinRT のデリゲートは `Send + Sync` を要求するため `UiThreadCell` に載せる。
/// 呼び出しの間だけクロージャを取り出すので、通知の中から同じツリーを
/// 操作しても二重借用にならない。
#[derive(Clone)]
struct ExpandHandler(Arc<UiThreadCell<Option<Box<dyn FnMut(&[usize], bool)>>>>);

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
    native: ScrollViewer,
    list_box: XamlListBox,
    _wheel: Rc<ListScrollTarget>,
    items: RefCell<Vec<TreeItem>>,
    /// 行にしてある項目のパス。深さ優先 (親が先) で、`ListBox` の行と
    /// 同じ並び。見えていない行もここに残る。
    rows: RefCell<Vec<Vec<usize>>>,
    /// 行そのもの。選択と見え隠れの読み書きはここを通す。
    row_items: RefCell<Vec<ListBoxItem>>,
    /// 行に置いた開閉ボタン。葉の行には無い。山形の差し替えに使う。
    twisties: RefCell<Vec<Option<Button>>>,
    /// 開いた状態として覚えている項目。閉じた枝の中の分も残る。
    expanded: RefCell<HashSet<Vec<usize>>>,
    /// 選ばれている項目。選択なしは `None`。
    selected: RefCell<Option<Vec<usize>>>,
    handler: SelectionHandler,
    expand: ExpandHandler,
    /// プログラムから選択を変えている間だけ通知を止める。
    silent: Rc<Cell<bool>>,
    /// ウィンドウ全体のホイール補助がこの ScrollViewer を選ぶための状態。
    hovered: Arc<UiThreadCell<usize>>,
    /// 行に当てる見た目。読めなかったときだけ `None`。
    row_style: Option<Style>,
}

/// 入れ子の項目を開閉できる一覧 (ListBox の組み立て)。
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
        let (native, list_box) = build_surface()?;
        list_box
            .SetSelectionMode(XamlSelectionMode::Single)
            .map_err(|e| to_error("ツリーの選択方法の設定", e))?;
        // スクロールは外側の ScrollViewer に任せる (`List` と同じ)。
        let _ = ScrollViewer::SetHorizontalScrollBarVisibility2(
            &list_box,
            ScrollBarVisibility::Disabled,
        );
        let _ =
            ScrollViewer::SetVerticalScrollBarVisibility2(&list_box, ScrollBarVisibility::Disabled);
        let _ = native.SetHorizontalScrollBarVisibility(ScrollBarVisibility::Auto);
        let _ = native.SetVerticalScrollBarVisibility(ScrollBarVisibility::Auto);
        let hovered = Arc::new(UiThreadCell::new(0));
        let wheel = crate::layout::register_list_scroll(native.clone(), hovered.clone());

        let this = Self(Rc::new(TreeInner {
            native,
            list_box,
            _wheel: wheel,
            items: RefCell::new(Vec::new()),
            rows: RefCell::new(Vec::new()),
            row_items: RefCell::new(Vec::new()),
            twisties: RefCell::new(Vec::new()),
            expanded: RefCell::new(HashSet::new()),
            selected: RefCell::new(None),
            handler: SelectionHandler::new(),
            expand: ExpandHandler::new(),
            silent: Rc::new(Cell::new(false)),
            hovered,
            row_style: row_style(),
        }));

        // ハンドルを強く持つと購読との間で循環するため、弱参照にする。
        let state = UiThreadCell::new(Rc::downgrade(&this.0));
        let handler = SelectionChangedEventHandler::new(move |_sender, _args| {
            state.with_mut(|weak| {
                if let Some(inner) = weak.upgrade() {
                    let tree = Tree(inner);
                    if !tree.0.silent.get() {
                        tree.on_native_selection();
                    }
                }
            });
            Ok(())
        });
        this.0
            .list_box
            .SelectionChanged(&handler)
            .map_err(|e| to_error("ツリーの購読", e))?;

        // ホイールの行き先を選ばせるためのホバー状態 (`List` と同じ)。
        let entered_state = this.0.hovered.clone();
        let entered = PointerEventHandler::new(move |_, _| {
            entered_state.with_mut(|hovered| *hovered = hovered.saturating_add(1));
            Ok(())
        });
        let exited_state = this.0.hovered.clone();
        let exited = PointerEventHandler::new(move |_, _| {
            exited_state.with_mut(|hovered| {
                if *hovered > 0 {
                    *hovered -= 1;
                }
            });
            Ok(())
        });
        let moved_state = this.0.hovered.clone();
        let moved = PointerEventHandler::new(move |_, _| {
            moved_state.with_mut(|hovered| {
                if *hovered == 0 {
                    *hovered = 1;
                }
            });
            Ok(())
        });
        this.0
            .native
            .PointerEntered(&entered)
            .map_err(|e| to_error("ツリーのポインター購読", e))?;
        this.0
            .native
            .PointerExited(&exited)
            .map_err(|e| to_error("ツリーのポインター購読", e))?;
        this.0
            .native
            .PointerMoved(&moved)
            .map_err(|e| to_error("ツリーのポインター購読", e))?;
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
        let _ = self.rebuild();
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

    /// 中身の `ListBox`。バックエンド固有の脱出口として公開している。
    pub fn native_list_box(&self) -> XamlListBox {
        self.0.list_box.clone()
    }

    // ------------------------------------------------------------ 組み立て

    /// 全項目ぶんの行を作る。見え隠れは [`Tree::paint`] が決める。
    fn rebuild(&self) -> Result<()> {
        let mut paths = Vec::new();
        TreeItem::walk(&self.0.items.borrow(), |path, _| paths.push(path.to_vec()));

        let children = self
            .0
            .list_box
            .Items()
            .map_err(|e| to_error("行の取得", e))?;
        self.without_notifying(|_| children.Clear())
            .map_err(|e| to_error("行の消去", e))?;
        self.0.row_items.borrow_mut().clear();
        self.0.twisties.borrow_mut().clear();

        let mut row_items = Vec::with_capacity(paths.len());
        let mut twisties = Vec::with_capacity(paths.len());
        for path in &paths {
            let (row, twisty) = self.build_row(path)?;
            let element = row
                .cast::<IInspectable>()
                .map_err(|e| to_error("行の要素化", e))?;
            self.without_notifying(|_| children.Append(&element))
                .map_err(|e| to_error("行の追加", e))?;
            row_items.push(row);
            twisties.push(twisty);
        }
        *self.0.rows.borrow_mut() = paths;
        *self.0.row_items.borrow_mut() = row_items;
        *self.0.twisties.borrow_mut() = twisties;
        self.paint();
        Ok(())
    }

    /// 行 1 つを組み立てる。
    fn build_row(&self, path: &[usize]) -> Result<(ListBoxItem, Option<Button>)> {
        let items = self.0.items.borrow();
        let item = TreeItem::at(&items, path)
            .ok_or_else(|| naui_core::Error::new("ツリーの行の組み立て", "項目が見つかりません"))?;
        let selectable = TreeItem::selectable(&items, path);
        let branch = !item.is_leaf();

        let row = ListBoxItem::new().map_err(|e| to_error("ListBoxItem の生成", e))?;
        if let Some(style) = self.0.row_style.as_ref() {
            let _ = row.SetStyle(style);
        }

        let line = StackPanel::new().map_err(|e| to_error("行の StackPanel の生成", e))?;
        line.SetOrientation(XamlOrientation::Horizontal)
            .map_err(|e| to_error("行の向き設定", e))?;
        // 深さは左余白で表す。
        let _ = line.SetMargin(Thickness {
            Left: INDENT * (path.len().saturating_sub(1)) as f64,
            Top: 0.0,
            Right: 0.0,
            Bottom: 0.0,
        });
        let line_children = line.Children().map_err(|e| to_error("行の中身の取得", e))?;

        // 左端は開閉のボタン。葉のときは同じ幅を空けて、文字の左端をそろえる。
        let mut twisty_button_handle = None;
        let head: UIElement = match branch {
            true => {
                let twisty = twisty_button()?;
                // ハンドルを強く持つと購読との間で循環するため、弱参照にする。
                let state = UiThreadCell::new((Rc::downgrade(&self.0), path.to_vec()));
                let click = RoutedEventHandler::new(move |_, _| {
                    state.with_mut(|(weak, path)| {
                        if let Some(inner) = weak.upgrade() {
                            let tree = Tree(inner);
                            let expanded = tree.is_expanded(path);
                            tree.toggle(path, !expanded);
                        }
                    });
                    Ok(())
                });
                twisty
                    .Click(&click)
                    .map_err(|e| to_error("開閉ボタンの購読", e))?;
                let element = twisty
                    .cast::<UIElement>()
                    .map_err(|e| to_error("開閉ボタンの要素化", e))?;
                twisty_button_handle = Some(twisty);
                element
            }
            false => {
                let spacer = TextBlock::new().map_err(|e| to_error("行の余白の生成", e))?;
                let _ = spacer.SetWidth(TWISTY_WIDTH);
                spacer
                    .cast::<UIElement>()
                    .map_err(|e| to_error("行の余白の要素化", e))?
            }
        };
        line_children
            .Append(&head)
            .map_err(|e| to_error("行への追加", e))?;

        // 文字は `List` と同じ組み方 (補助があれば 2 行)。
        let title = text_block(&item.label, false)?;
        let text: UIElement = match &item.detail {
            None => title
                .cast::<UIElement>()
                .map_err(|e| to_error("行の要素化", e))?,
            Some(detail) => {
                let stack = StackPanel::new().map_err(|e| to_error("行の StackPanel の生成", e))?;
                stack
                    .SetOrientation(XamlOrientation::Vertical)
                    .map_err(|e| to_error("行の向き設定", e))?;
                let stack_children = stack
                    .Children()
                    .map_err(|e| to_error("行の中身の取得", e))?;
                stack_children
                    .Append(
                        &title
                            .cast::<UIElement>()
                            .map_err(|e| to_error("行の要素化", e))?,
                    )
                    .map_err(|e| to_error("行への追加", e))?;
                stack_children
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
        if !selectable && branch {
            // 枝は開閉できる必要があるため、行全体を無効にできない。
            // その代わり文字だけを無効な行と同じ濃さにする。
            let _ = text.SetOpacity(0.4);
        }
        line_children
            .Append(&text)
            .map_err(|e| to_error("行への追加", e))?;

        row.SetContent(&line)
            .map_err(|e| to_error("行への内容設定", e))?;
        // 選べない枝も開閉ボタンは押せるよう、枝の行は有効のままにする。
        // 選択イベント側で選択不可の行を弾くので、選択可能性は保たれる。
        let _ = row.SetIsEnabled(selectable || branch);
        Ok((row, twisty_button_handle))
    }

    // --------------------------------------------------------- 状態の反映

    /// 開閉を行の見え隠れと山形へ写す。
    ///
    /// 見えるのは、祖先がすべて開いている行だけ。
    fn paint(&self) {
        {
            let expanded = self.0.expanded.borrow();
            let rows = self.0.rows.borrow();
            let row_items = self.0.row_items.borrow();
            let twisties = self.0.twisties.borrow();
            for (index, path) in rows.iter().enumerate() {
                let visible = path[..path.len() - 1]
                    .iter()
                    .enumerate()
                    .all(|(depth, _)| expanded.contains(&path[..=depth]));
                if let Some(row) = row_items.get(index) {
                    let _ = row.SetVisibility(match visible {
                        true => Visibility::Visible,
                        false => Visibility::Collapsed,
                    });
                }
                if let Some(Some(twisty)) = twisties.get(index) {
                    let _ = set_twisty_glyph(twisty, expanded.contains(path));
                }
            }
        }
        self.paint_selection();
    }

    /// 選択を行へ書き込む (通知は起きない)。
    fn paint_selection(&self) {
        let selected = self.0.selected.borrow().clone();
        let rows = self.0.rows.borrow();
        self.without_notifying(|this| {
            for (index, row) in this.0.row_items.borrow().iter().enumerate() {
                let picked = selected.as_deref() == rows.get(index).map(|path| path.as_slice());
                let _ = row.SetIsSelected(picked);
            }
        });
    }

    /// 選択を覚えて書き込む (通知は起きない)。
    fn write_selected(&self, path: &[usize]) {
        let picked = TreeItem::selectable(&self.0.items.borrow(), path).then(|| path.to_vec());
        if let Some(path) = picked.as_deref() {
            // 見えていないと選んだことが分からないので、祖先を開く。
            // 葉には開閉が無いので、開くのは親から上だけ。
            self.write_expanded(&path[..path.len().saturating_sub(1)], true);
        }
        *self.0.selected.borrow_mut() = picked;
        self.paint_selection();
    }

    /// 開閉を覚えて描き直す (通知は起きない)。開くときは祖先もまとめて開く。
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
        {
            let mut set = self.0.expanded.borrow_mut();
            set.clear();
            if expanded {
                TreeItem::walk(&self.0.items.borrow(), |path, item| {
                    if !item.is_leaf() {
                        set.insert(path.to_vec());
                    }
                });
            }
        }
        self.paint();
    }

    /// WinUI 側で選択が変わったとき。
    fn on_native_selection(&self) {
        let picked = self
            .0
            .row_items
            .borrow()
            .iter()
            .position(|row| row.IsSelected().unwrap_or(false))
            .and_then(|index| self.0.rows.borrow().get(index).cloned());

        // 開閉のために選択可能にしている枝は、ListBox から見ると選択できて
        // しまう。無効な行が選ばれた場合は、直前の選択を描き戻して選択状態を
        // 変えない (選択が無い場合はそのまま全解除する)。
        if picked
            .as_ref()
            .is_some_and(|path| !TreeItem::selectable(&self.0.items.borrow(), path))
        {
            self.paint_selection();
            return;
        }
        *self.0.selected.borrow_mut() = picked;
        let actual = self.selected().unwrap_or_default();
        self.0.handler.emit(&actual);
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

/// 開閉のボタンを作る。テーマリソースが引けない環境では、素のボタンに落とす。
///
/// 山形は [`set_twisty_glyph`] があとから入れる。
fn twisty_button() -> Result<Button> {
    match XamlReader::Load(&HSTRING::from(TWISTY_XAML)).and_then(|element| element.cast::<Button>())
    {
        Ok(button) => Ok(button),
        Err(error) => {
            eprintln!("naui-windows: ツリーの開閉ボタンの生成に失敗: {error}");
            let button = Button::new().map_err(|e| to_error("開閉ボタンの生成", e))?;
            let _ = button.SetWidth(TWISTY_WIDTH);
            Ok(button)
        }
    }
}

/// 開閉のボタンに山形を入れる。開いていれば下向き、閉じていれば右向き。
fn set_twisty_glyph(button: &Button, expanded: bool) -> Result<()> {
    let glyph = match expanded {
        true => GLYPH_EXPANDED,
        false => GLYPH_COLLAPSED,
    };
    let content = PropertyValue::CreateString(&HSTRING::from(glyph))
        .map_err(|e| to_error("開閉ボタンの山形の生成", e))?;
    button
        .SetContent(&content)
        .map_err(|e| to_error("開閉ボタンの内容設定", e))
}
