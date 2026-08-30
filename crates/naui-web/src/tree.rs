//! ツリー (DOM)。
//!
//! ブラウザには「ツリー」の標準コントロールが無いので、WAI-ARIA の
//! `role="tree"` に沿って組み立てる。
//!
//! | 部分 | 要素 |
//! | --- | --- |
//! | 全体 | `<ul role="tree" tabindex="0">` |
//! | 項目 | `<li role="treeitem" aria-expanded aria-selected>` |
//! | 子の並び | `<ul role="group">` (親の中に入れ子で置く) |
//!
//! 入れ子の `<ul>` をそのまま作り、開閉は `display` の切り替えで行う。
//! 閉じた枝の中の開閉がそのまま残る (開き直すと元どおりに出る) のは
//! この作りによるもので、macOS の `NSOutlineView` と同じ動きになる。
//!
//! 枠と選択の色はブラウザが持つシステム色 (`Field` / `SelectedItem` /
//! `Highlight` / `GrayText`) をそのまま使い、naui は配色を決めない。

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;

use naui_core::{Result, TreeItem};
use wasm_bindgen::JsCast;
use web_sys::{Document, Element, HtmlElement, KeyboardEvent};

use crate::widgets::{create, impl_widget, Listener, Widget};

/// 1 段ぶんの字下げ (px)。
const INDENT: f64 = 16.0;

thread_local! {
    /// `aria-activedescendant` から項目を指すための、ツリーごとの通し番号。
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

/// 選択が変わったことの通知先。選ばれている項目のパスで呼ぶ
/// (選択が外れたときは空のパス)。
#[derive(Clone, Default)]
struct SelectionHandler(Rc<RefCell<Option<Box<dyn FnMut(&[usize])>>>>);

impl SelectionHandler {
    fn set(&self, f: impl FnMut(&[usize]) + 'static) {
        *self.0.borrow_mut() = Some(Box::new(f));
    }

    /// 呼び出しの間だけクロージャを取り出すので、通知の中から
    /// ツリーを操作しても二重借用にならない。
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

/// 開閉が変わったことの通知先。
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

/// 画面に出ている項目 1 つぶん。並びは深さ優先 (親が先)。
struct Node {
    path: Vec<usize>,
    /// `role="treeitem"`。開閉と選択の状態はここに載る。
    item: HtmlElement,
    /// 文字と開閉の印を載せた 1 行。押すと選ぶ。
    row: HtmlElement,
    /// 子の `role="group"`。葉なら `None`。
    group: Option<HtmlElement>,
    /// 開閉の印。葉なら `None`。
    twisty: Option<HtmlElement>,
}

struct TreeInner {
    /// 外から見える `role="tree"`。項目を作り直しても、この要素は変わらない。
    root: HtmlElement,
    document: Document,
    id: u32,
    items: RefCell<Vec<TreeItem>>,
    nodes: RefCell<Vec<Node>>,
    /// 開いた状態として覚えている項目。閉じた枝の中の分も残る。
    expanded: RefCell<HashSet<Vec<usize>>>,
    /// 選ばれている項目。選択なしは `None`。
    selected: RefCell<Option<Vec<usize>>>,
    /// キーボードでいま指している項目。
    active: RefCell<Option<Vec<usize>>>,
    handler: SelectionHandler,
    expand: ExpandHandler,
    /// 行と開閉の印の購読。作り直すと外れる。
    listeners: RefCell<Vec<Listener>>,
    /// ツリー全体のキー操作の購読。
    _keys: RefCell<Option<Listener>>,
}

/// 入れ子の項目を開閉できる一覧 (`role="tree"`)。
///
/// 項目は根からの子インデックスの並び (パス) で指す。`[0, 2]` は
/// 「1 番目の根の 3 番目の子」で、空のパスは「選択なし」を表す。
///
/// 高さは中身から決まらないので、`set_sizing` で指定する。
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
        style(&root, "border", "1px solid");
        style(&root, "border-color", "ButtonBorder");
        style(&root, "background-color", "Field");
        style(&root, "color", "FieldText");

        let this = Self(Rc::new(TreeInner {
            root,
            document: doc.clone(),
            id: next_tree_id(),
            items: RefCell::new(Vec::new()),
            nodes: RefCell::new(Vec::new()),
            expanded: RefCell::new(HashSet::new()),
            selected: RefCell::new(None),
            active: RefCell::new(None),
            handler: SelectionHandler::default(),
            expand: ExpandHandler::default(),
            listeners: RefCell::new(Vec::new()),
            _keys: RefCell::new(None),
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

    /// 項目を作り直す。パスの意味が変わるため、選択は外れる。
    ///
    /// 開閉は [`TreeItem::expanded`] のとおりに戻る。
    pub fn set_items(&self, items: &[TreeItem]) {
        *self.0.items.borrow_mut() = items.to_vec();
        *self.0.selected.borrow_mut() = None;
        *self.0.active.borrow_mut() = None;
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
        // ブラウザはプログラムからの変更でイベントを出さないため、
        // ここで 1 回だけ通知する。
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

    /// 中身の `role="tree"` の要素。バックエンド固有の脱出口として公開している。
    pub fn native_tree(&self) -> HtmlElement {
        self.0.root.clone()
    }

    // ------------------------------------------------------------ 組み立て

    /// 項目の入れ子を DOM へ作り直す。
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

    /// 同じ深さの項目を 1 段ぶん組み立てる。
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

    /// 項目 1 つぶんの要素を作る。
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
            "padding-left",
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
            listeners.push(Listener::attach(row.as_ref(), "click", {
                let weak = Rc::downgrade(&self.0);
                let path = path.to_vec();
                move || {
                    if let Some(inner) = weak.upgrade() {
                        Tree(inner).on_row_clicked(&path);
                    }
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

    /// 開閉と選択を、いまの状態どおりに描き直す。
    fn paint(&self) {
        let expanded = self.0.expanded.borrow();
        let selected = self.0.selected.borrow();
        for node in self.0.nodes.borrow().iter() {
            let open = expanded.contains(&node.path);
            if let Some(group) = &node.group {
                style(group, "display", if open { "block" } else { "none" });
            }
            if let Some(twisty) = &node.twisty {
                twisty.set_text_content(Some(if open { "▾" } else { "▸" }));
                let _ = node
                    .item
                    .set_attribute("aria-expanded", if open { "true" } else { "false" });
            }

            let picked = selected.as_deref() == Some(node.path.as_slice());
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

    /// 選択を覚えて描き直す (通知は起きない)。
    fn write_selected(&self, path: &[usize]) {
        let picked = TreeItem::selectable(&self.0.items.borrow(), path).then(|| path.to_vec());
        if let Some(path) = picked.as_deref() {
            // 見えていないと選んだことが分からないので、祖先を開く。
            // 葉には開閉が無いので、開くのは親から上だけ。
            self.write_expanded(&path[..path.len().saturating_sub(1)], true);
        }
        *self.0.selected.borrow_mut() = picked.clone();
        if picked.is_some() {
            *self.0.active.borrow_mut() = picked;
        }
        self.paint();
        self.reveal_active();
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

    /// 行が押されたとき。
    fn on_row_clicked(&self, path: &[usize]) {
        self.write_selected(path);
        let actual = self.selected().unwrap_or_default();
        self.0.handler.emit(&actual);
    }

    /// キー操作。処理したら `true` を返す (ブラウザの既定動作を止める)。
    ///
    /// WAI-ARIA の Tree View パターンに合わせ、↑↓ で行を移動し、
    /// → で開く / 子へ、← で閉じる / 親へ動く。
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

        match event.key().as_str() {
            "ArrowDown" => {
                let next = at.map_or(0, |at| (at + 1).min(rows.len() - 1));
                self.focus(&rows[next]);
            }
            "ArrowUp" => {
                let next = at.map_or(rows.len() - 1, |at| at.saturating_sub(1));
                self.focus(&rows[next]);
            }
            "Home" => self.focus(&rows[0]),
            "End" => self.focus(&rows[rows.len() - 1]),
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
                    self.focus(&child);
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
                    self.focus(&path[..path.len() - 1]);
                } else {
                    return false;
                }
            }
            "Enter" | " " => {
                let Some(path) = current else {
                    return false;
                };
                self.on_row_clicked(&path);
            }
            _ => return false,
        }
        true
    }

    /// キーボードで指す項目を移して、選べるならそのまま選ぶ。
    fn focus(&self, path: &[usize]) {
        *self.0.active.borrow_mut() = Some(path.to_vec());
        self.reveal_active();
        // ツリーは単一選択なので、カーソルの移動がそのまま選択になる
        // (`NSOutlineView` の ↑↓ と同じ)。選べない項目では選択を動かさない。
        if TreeItem::selectable(&self.0.items.borrow(), path) {
            self.on_row_clicked(path);
        }
    }

    /// いま見えている項目のパスを上から順に返す。
    fn visible_paths(&self) -> Vec<Vec<usize>> {
        let expanded = self.0.expanded.borrow();
        TreeItem::visible(&self.0.items.borrow(), |path| expanded.contains(path))
    }

    /// キーボードで指している項目を、スクロール領域の中へ入れる。
    fn reveal_active(&self) {
        let Some(active) = self.0.active.borrow().clone() else {
            return;
        };
        let _ = self
            .0
            .root
            .set_attribute("aria-activedescendant", &self.item_id(&active));
        let nodes = self.0.nodes.borrow();
        let Some(node) = nodes.iter().find(|node| node.path == active) else {
            return;
        };
        let top = node.row.offset_top();
        let bottom = top + node.row.offset_height();
        let view_top = self.0.root.scroll_top();
        let view_bottom = view_top + self.0.root.client_height();
        if top < view_top {
            self.0.root.set_scroll_top(top);
        } else if bottom > view_bottom {
            self.0
                .root
                .set_scroll_top(bottom - self.0.root.client_height());
        }
    }
}
