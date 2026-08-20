//! ツリー (`GtkListBox` を `GtkScrolledWindow` に載せたもの)。
//!
//! GTK4 の `GtkTreeExpander` は `GtkListView` + `GtkTreeListModel` と
//! 組み合わせてしか使えず、`GtkTreeView` は 4.10 で非推奨になっている。
//! そこで naui では、`List` と同じ `GtkListBox` の上に組み立てている。
//!
//! | 部分 | 作り |
//! | --- | --- |
//! | 枠と行 | `List` と同じ (`GtkScrolledWindow` + `GtkListBox`) |
//! | 段付け | 行の中身の左余白 |
//! | 開閉 | 行の左端に置いた `GtkButton` (`pan-end` / `pan-down` のアイコン) |
//!
//! 行は**全項目ぶんを作り置き**し、開閉では見え隠れ (`set_visible`) だけを
//! 切り替える。押された開閉ボタン自身が作り直しで消えないので、シグナルの
//! 最中に行を捨てずに済む。選択・キーボード操作・スクロールは
//! `GtkListBox` と `GtkScrolledWindow` が行う (見えない行は GTK4 が飛ばす)。

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use gtk::glib;
use gtk::prelude::*;
use naui_core::TreeItem;

use crate::bin::SizeBin;
use crate::callback::{ExpandNotifier, SelectionNotifier};
use crate::widgets::{impl_widget, without_signal, Widget};

/// 1 段ぶんの字下げ (論理ピクセル)。
const INDENT: i32 = 16;

/// 開閉のボタンと同じ幅。葉の行で、文字の左端をそろえるために空ける。
const TWISTY_WIDTH: i32 = 24;

/// 閉じているときのアイコン (Adwaita の標準アイコン)。
const ICON_COLLAPSED: &str = "pan-end-symbolic";
/// 開いているときのアイコン。
const ICON_EXPANDED: &str = "pan-down-symbolic";

struct TreeInner {
    native: gtk::ListBox,
    /// `GtkListBox` は自分でスクロールしないので、スクロール領域に載せる。
    _scroller: gtk::ScrolledWindow,
    bin: SizeBin,
    items: RefCell<Vec<TreeItem>>,
    /// 行にしてある項目のパス。深さ優先 (親が先) で、`GtkListBox` の行と
    /// 同じ並び。見えていない行もここに残る。
    rows: RefCell<Vec<Vec<usize>>>,
    /// 行に置いた開閉ボタン。葉の行には無い。アイコンの差し替えに使う。
    twisties: RefCell<Vec<Option<gtk::Button>>>,
    /// 開いた状態として覚えている項目。閉じた枝の中の分も残る。
    expanded: RefCell<HashSet<Vec<usize>>>,
    /// 選ばれている項目。選択なしは `None`。
    selected: RefCell<Option<Vec<usize>>>,
    on_select: SelectionNotifier,
    on_expand: ExpandNotifier,
    handler: RefCell<Option<glib::SignalHandlerId>>,
}

/// 入れ子の項目を開閉できる一覧。自分でスクロールする。
///
/// 項目は根からの子インデックスの並び (パス) で指す。`[0, 2]` は
/// 「1 番目の根の 3 番目の子」で、空のパスは「選択なし」を表す。
///
/// 高さは中身から決まらないので、[`Tree::set_sizing`] で指定しておく。
#[derive(Clone)]
pub struct Tree(Rc<TreeInner>);
impl_widget!(Tree);

impl Tree {
    pub(crate) fn new() -> Self {
        let native = gtk::ListBox::new();
        native.set_selection_mode(gtk::SelectionMode::Single);
        let scroller = gtk::ScrolledWindow::new();
        scroller.set_policy(gtk::PolicyType::Automatic, gtk::PolicyType::Automatic);
        scroller.set_has_frame(true);
        scroller.set_child(Some(&native));

        let bin = SizeBin::wrap(&scroller);
        let inner = Rc::new(TreeInner {
            native,
            _scroller: scroller,
            bin,
            items: RefCell::new(Vec::new()),
            rows: RefCell::new(Vec::new()),
            twisties: RefCell::new(Vec::new()),
            expanded: RefCell::new(HashSet::new()),
            selected: RefCell::new(None),
            on_select: SelectionNotifier::default(),
            on_expand: ExpandNotifier::default(),
            handler: RefCell::new(None),
        });
        // 選択の通知は常時つないでおき、プログラムから変えるときだけ止める。
        let id = {
            let weak = Rc::downgrade(&inner);
            inner.native.connect_selected_rows_changed(move |_| {
                if let Some(inner) = weak.upgrade() {
                    Tree(inner).on_native_selection();
                }
            })
        };
        *inner.handler.borrow_mut() = Some(id);
        Self(inner)
    }

    /// 項目を作り直す。パスの意味が変わるため、選択は外れる。
    ///
    /// 開閉は [`TreeItem::expanded`] のとおりに戻る。
    pub fn set_items(&self, items: &[TreeItem]) {
        {
            let mut stored = self.0.items.borrow_mut();
            stored.clear();
            stored.extend_from_slice(items);
        }
        *self.0.selected.borrow_mut() = None;
        let mut expanded = HashSet::new();
        TreeItem::walk(items, |path, item| {
            if item.expanded && !item.is_leaf() {
                expanded.insert(path.to_vec());
            }
        });
        *self.0.expanded.borrow_mut() = expanded;
        self.rebuild();
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
        let actual = self.selected().unwrap_or_default();
        self.0.on_select.emit(&actual);
    }

    /// 選択が変わったときに、選ばれている項目のパスで呼ばれる。
    ///
    /// 選択が外れたときは空のパスで呼ばれる。
    pub fn on_select(&self, f: impl FnMut(&[usize]) + 'static) {
        self.0.on_select.set(f);
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
        self.0.on_expand.set(f);
    }

    // ------------------------------------------------------------ 組み立て

    /// 全項目ぶんの行を作る。見え隠れは [`Tree::paint`] が決める。
    fn rebuild(&self) {
        let mut paths = Vec::new();
        TreeItem::walk(&self.0.items.borrow(), |path, _| paths.push(path.to_vec()));

        let mut twisties = Vec::with_capacity(paths.len());
        without_signal(&self.0.native, &self.0.handler, || {
            while let Some(row) = self.0.native.first_child() {
                self.0.native.remove(&row);
            }
            for path in &paths {
                // パスは木から作っているので、行が作れないことはない。
                // それでも数がずれないよう、作れなければ場所だけ空けておく。
                match self.build_row(path) {
                    Some((row, twisty)) => {
                        self.0.native.append(&row);
                        twisties.push(twisty);
                    }
                    None => twisties.push(None),
                }
            }
            self.0.native.unselect_all();
        });
        *self.0.rows.borrow_mut() = paths;
        *self.0.twisties.borrow_mut() = twisties;
        self.paint();
    }

    /// 行 1 つを組み立てる。枝なら開閉ボタンも返す。
    fn build_row(&self, path: &[usize]) -> Option<(gtk::ListBoxRow, Option<gtk::Button>)> {
        let items = self.0.items.borrow();
        let item = TreeItem::at(&items, path)?;
        let selectable = TreeItem::selectable(&items, path);

        let line = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        line.set_margin_top(4);
        line.set_margin_bottom(4);
        line.set_margin_start(6 + INDENT * (path.len().saturating_sub(1)) as i32);
        line.set_margin_end(10);

        // 左端は開閉のボタン。葉のときは同じ幅を空けて、文字の左端をそろえる。
        let twisty = if item.is_leaf() {
            let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            spacer.set_size_request(TWISTY_WIDTH, -1);
            line.append(&spacer);
            None
        } else {
            let twisty = gtk::Button::from_icon_name(ICON_COLLAPSED);
            twisty.set_has_frame(false);
            twisty.add_css_class("flat");
            twisty.set_valign(gtk::Align::Center);
            twisty.set_size_request(TWISTY_WIDTH, -1);
            let weak = Rc::downgrade(&self.0);
            let path = path.to_vec();
            twisty.connect_clicked(move |_| {
                if let Some(inner) = weak.upgrade() {
                    let tree = Tree(inner);
                    let expanded = tree.is_expanded(&path);
                    tree.toggle(&path, !expanded);
                }
            });
            line.append(&twisty);
            Some(twisty)
        };

        // 文字は `List` と同じ組み方 (補助があれば 2 行目に小さく出す)。
        let content = gtk::Box::new(gtk::Orientation::Vertical, 2);
        content.set_valign(gtk::Align::Center);
        let label = gtk::Label::new(Some(&item.label));
        label.set_xalign(0.0);
        content.append(&label);
        if let Some(detail) = &item.detail {
            let detail = gtk::Label::new(Some(detail));
            detail.set_xalign(0.0);
            detail.add_css_class("dim-label");
            detail.add_css_class("caption");
            content.append(&detail);
        }
        // 選べない項目は文字だけを淡くする。行ごと無効にすると、
        // 中の開閉ボタンまで押せなくなるため。
        content.set_sensitive(selectable);
        line.append(&content);

        let row = gtk::ListBoxRow::new();
        row.set_child(Some(&line));
        row.set_selectable(selectable);
        row.set_activatable(selectable);
        Some((row, twisty))
    }

    // --------------------------------------------------------- 状態の反映

    /// 開閉を行の見え隠れとアイコンへ写す。
    ///
    /// 見えるのは、祖先がすべて開いている行だけ。
    fn paint(&self) {
        let expanded = self.0.expanded.borrow();
        let rows = self.0.rows.borrow();
        let twisties = self.0.twisties.borrow();
        for (index, path) in rows.iter().enumerate() {
            let visible = path[..path.len() - 1]
                .iter()
                .enumerate()
                .all(|(depth, _)| expanded.contains(&path[..=depth]));
            if let Some(row) = self.0.native.row_at_index(index as i32) {
                row.set_visible(visible);
            }
            if let Some(Some(twisty)) = twisties.get(index) {
                twisty.set_icon_name(if expanded.contains(path) {
                    ICON_EXPANDED
                } else {
                    ICON_COLLAPSED
                });
            }
        }
    }

    /// 選択をネイティブへ写す。
    fn show_selection(&self) {
        let selected = self.0.selected.borrow().clone();
        let index = selected.and_then(|path| {
            self.0
                .rows
                .borrow()
                .iter()
                .position(|row| row.as_slice() == path.as_slice())
        });
        without_signal(&self.0.native, &self.0.handler, || {
            self.0.native.unselect_all();
            if let Some(index) = index {
                if let Some(row) = self.0.native.row_at_index(index as i32) {
                    self.0.native.select_row(Some(&row));
                }
            }
        });
    }

    /// 選択を覚えて書き込む (通知は起きない)。
    fn write_selected(&self, path: &[usize]) {
        let picked = TreeItem::selectable(&self.0.items.borrow(), path).then(|| path.to_vec());
        if let Some(path) = picked.as_deref() {
            // 見えていないと選んだことが分からないので、祖先を開く。
            self.write_expanded(path, true);
        }
        *self.0.selected.borrow_mut() = picked;
        self.show_selection();
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
            self.0.on_expand.emit(path, expanded);
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

    /// GTK 側で選択が変わったとき。
    fn on_native_selection(&self) {
        let picked = self.0.native.selected_row().and_then(|row| {
            let index = row.index().max(0) as usize;
            self.0.rows.borrow().get(index).cloned()
        });
        *self.0.selected.borrow_mut() = picked;
        let actual = self.selected().unwrap_or_default();
        self.0.on_select.emit(&actual);
    }
}
