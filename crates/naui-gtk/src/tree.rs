//! A tree (a `GtkListBox` in a `GtkScrolledWindow`).
//!
//! GTK4's `GtkTreeExpander` can only be used together with `GtkListView` +
//! `GtkTreeListModel`, and `GtkTreeView` was deprecated in 4.10. So naui builds it
//! on the same `GtkListBox` as `List`.
//!
//! | Part | Built from |
//! | --- | --- |
//! | Frame and rows | The same as `List` (`GtkScrolledWindow` + `GtkListBox`) |
//! | Indentation | Left margin of the row's content |
//! | Expand/collapse | A `GtkButton` at the left end of the row (`pan-end` / `pan-down` icons) |
//!
//! Rows are **built in advance for every item**, and expanding or collapsing only
//! toggles visibility (`set_visible`). The clicked expander button itself is never
//! destroyed by a rebuild, so rows need not be discarded in the middle of a signal.
//! Selection, keyboard navigation and scrolling are done by `GtkListBox` and
//! `GtkScrolledWindow` (GTK4 skips invisible rows).

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use gtk::glib;
use gtk::prelude::*;
use naui_core::{Sizing, TreeItem};

use crate::bin::SizeBin;
use crate::callback::{ExpandNotifier, SelectionNotifier};
use crate::widgets::{impl_widget, without_signal, Widget};

/// The indentation per level (logical pixels).
const INDENT: i32 = 16;

/// The same width as the expander button. Left empty in leaf rows to align the left
/// edge of the text.
const TWISTY_WIDTH: i32 = 24;

/// The icon when collapsed (a standard Adwaita icon).
const ICON_COLLAPSED: &str = "pan-end-symbolic";
/// The icon when expanded.
const ICON_EXPANDED: &str = "pan-down-symbolic";

struct TreeInner {
    native: gtk::ListBox,
    /// `GtkListBox` does not scroll by itself, so it is placed in a scroll area.
    _scroller: gtk::ScrolledWindow,
    bin: SizeBin,
    items: RefCell<Vec<TreeItem>>,
    /// The paths of the items made into rows. Depth-first (parents first), in the same
    /// order as the `GtkListBox` rows. Invisible rows stay here too.
    rows: RefCell<Vec<Vec<usize>>>,
    /// The expander buttons placed in rows. Leaf rows have none. Used to swap icons.
    twisties: RefCell<Vec<Option<gtk::Button>>>,
    /// The items remembered as expanded. Those inside collapsed branches remain too.
    expanded: RefCell<HashSet<Vec<usize>>>,
    /// The selected item. `None` for no selection.
    selected: RefCell<Option<Vec<usize>>>,
    on_select: SelectionNotifier,
    on_expand: ExpandNotifier,
    handler: RefCell<Option<glib::SignalHandlerId>>,
}

/// A list whose nested items can be expanded and collapsed. It scrolls by itself.
///
/// Items are referred to by the sequence of child indices from the root (a path).
/// `[0, 2]` is "the third child of the first root", and an empty path means "no
/// selection".
///
/// Its height is not determined by its contents, so set it with
/// [`Tree::set_sizing`].
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

    /// Rebuilds the items. Paths change meaning, so the selection is cleared.
    ///
    /// Expansion returns to what [`TreeItem::expanded`] says.
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
    /// already visible nothing moves. Does nothing for a path that does not exist.
    pub fn scroll_to_item(&self, path: &[usize]) {
        if TreeItem::at(&self.0.items.borrow(), path).is_none() {
            return;
        }
        self.write_expanded(&path[..path.len().saturating_sub(1)], true);
        let index = self.0.rows.borrow().iter().position(|row| row == path);
        let row = index
            .and_then(|index| i32::try_from(index).ok())
            .and_then(|index| self.0.native.row_at_index(index));
        if let Some(row) = row {
            crate::list::reveal_widget(&self.0._scroller, &row, true);
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
        let actual = self.selected().unwrap_or_default();
        self.0.on_select.emit(&actual);
    }

    /// Called with the path of the selected item when the selection changes.
    ///
    /// When the selection is cleared, it is called with an empty path.
    pub fn on_select(&self, f: impl FnMut(&[usize]) + 'static) {
        self.0.on_select.set(f);
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
        self.0.on_expand.set(f);
    }

    // ------------------------------------------------------------ 組み立て

    /// Builds rows for every item. [`Tree::paint`] decides which are visible.
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

    /// Builds one row. For branches, also returns the expander button.
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
        let content = crate::list::item_content(&item.label, item.detail.as_deref());
        let content = content.size_bin();
        // 開閉ボタンの右はぜんぶ文字の領域。`SizeBin` の既定は中身の大きさに
        // 合わせるので、そのままでは行の残り幅を使わず、長い文字が切られない。
        content.apply_sizing(Sizing::fill_width());
        // 開閉ボタンと縦中央でそろえる。`apply_sizing` は縦の寄せ方も書くので、
        // そのあとに指定する。
        content.set_valign(gtk::Align::Center);
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

    /// Applies the expansion state to row visibility and icons.
    ///
    /// Only rows whose ancestors are all expanded are visible.
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

    /// Applies the selection to the native side.
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

    /// Remembers and writes the selection (no notification).
    fn write_selected(&self, path: &[usize]) {
        let picked = TreeItem::selectable(&self.0.items.borrow(), path).then(|| path.to_vec());
        if let Some(path) = picked.as_deref() {
            // 見えていないと選んだことが分からないので、祖先を開く。
            // 葉には開閉が無いので、開くのは親から上だけ。
            self.write_expanded(&path[..path.len().saturating_sub(1)], true);
        }
        *self.0.selected.borrow_mut() = picked;
        self.show_selection();
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
            self.0.on_expand.emit(path, expanded);
        }
    }

    /// Expands or collapses every branch at once (no notification).
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

    /// When the selection changes on the GTK side.
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
