//! 選択できる行の一覧 (`GtkListBox` を `GtkScrolledWindow` に載せたもの)。

use std::cell::RefCell;
use std::rc::Rc;

use gtk::glib;
use gtk::prelude::*;
use naui_core::{ListItem, SelectionMode};

use crate::bin::SizeBin;
use crate::callback::SelectionNotifier;
use crate::widgets::{impl_widget, without_signal, Widget};

struct ListInner {
    native: gtk::ListBox,
    /// `GtkListBox` は自分でスクロールしないので、スクロール領域に載せる。
    _scroller: gtk::ScrolledWindow,
    bin: SizeBin,
    items: RefCell<Vec<ListItem>>,
    on_select: SelectionNotifier,
    handler: RefCell<Option<glib::SignalHandlerId>>,
}

/// 選択できる行の一覧。自分でスクロールする。
///
/// 高さは中身から決まらないので、[`List::set_sizing`] で指定しておく。
#[derive(Clone)]
pub struct List(Rc<ListInner>);
impl_widget!(List);

impl List {
    pub(crate) fn new() -> Self {
        let native = gtk::ListBox::new();
        native.set_selection_mode(gtk::SelectionMode::Single);
        native.add_css_class("boxed-list");

        let scroller = gtk::ScrolledWindow::new();
        scroller.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        scroller.set_has_frame(true);
        scroller.set_child(Some(&native));

        let bin = SizeBin::wrap(&scroller);
        let inner = Rc::new(ListInner {
            native,
            _scroller: scroller,
            bin,
            items: RefCell::new(Vec::new()),
            on_select: SelectionNotifier::default(),
            handler: RefCell::new(None),
        });
        // 選択の通知は常時つないでおき、プログラムから変えるときだけ止める。
        let id = {
            let weak = Rc::downgrade(&inner);
            inner.native.connect_selected_rows_changed(move |_| {
                if let Some(inner) = weak.upgrade() {
                    let list = List(inner);
                    let selection = list.selection();
                    list.0.on_select.emit(&selection);
                }
            })
        };
        *inner.handler.borrow_mut() = Some(id);
        Self(inner)
    }

    /// 行を作り直す。インデックスの意味が変わるため、選択は外れる。
    pub fn set_items(&self, items: &[ListItem]) {
        without_signal(&self.0.native, &self.0.handler, || {
            while let Some(row) = self.0.native.first_child() {
                self.0.native.remove(&row);
            }
            for item in items {
                self.0.native.append(&build_row(item));
            }
            self.0.native.unselect_all();
        });
        let mut stored = self.0.items.borrow_mut();
        stored.clear();
        stored.extend_from_slice(items);
    }

    pub fn len(&self) -> usize {
        self.0.items.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn set_selection_mode(&self, mode: SelectionMode) {
        self.0.native.set_selection_mode(if mode.is_multiple() {
            // `Multiple` は「⌘ / Ctrl や Shift を押しながら選ぶ」形。
            gtk::SelectionMode::Multiple
        } else {
            gtk::SelectionMode::Single
        });
    }

    pub fn selection_mode(&self) -> SelectionMode {
        match self.0.native.selection_mode() {
            gtk::SelectionMode::Multiple => SelectionMode::Multiple,
            _ => SelectionMode::Single,
        }
    }

    /// 選ばれている先頭の行。
    pub fn selected(&self) -> Option<usize> {
        self.selection().first().copied()
    }

    /// 選ばれている行を昇順で返す。
    pub fn selection(&self) -> Vec<usize> {
        let mut indices: Vec<usize> = self
            .0
            .native
            .selected_rows()
            .iter()
            .map(|row| row.index().max(0) as usize)
            .collect();
        indices.sort_unstable();
        indices
    }

    /// 通知せずに 1 行だけ選ぶ。
    pub fn set_selected(&self, index: usize) {
        self.set_selection(&[index]);
    }

    /// 通知せずに選択を置き換える。
    pub fn set_selection(&self, indices: &[usize]) {
        let picked = self.normalize(indices);
        without_signal(&self.0.native, &self.0.handler, || self.show(&picked));
    }

    /// 通知せずに選択をすべて外す。
    pub fn clear_selection(&self) {
        without_signal(&self.0.native, &self.0.handler, || {
            self.0.native.unselect_all();
        });
    }

    /// ユーザーが選んだのと同じ経路で 1 行を選ぶ (通知あり)。
    pub fn select(&self, index: usize) {
        self.select_many(&[index]);
    }

    /// ユーザーが選んだのと同じ経路で選択を置き換える (通知あり)。
    pub fn select_many(&self, indices: &[usize]) {
        let picked = self.normalize(indices);
        without_signal(&self.0.native, &self.0.handler, || self.show(&picked));
        self.0.on_select.emit(&picked);
    }

    /// 選択が変わるたびに、選ばれている行のインデックスで呼ばれる。
    pub fn on_select(&self, f: impl FnMut(&[usize]) + 'static) {
        self.0.on_select.set(f);
    }

    /// 指定された選択を、この一覧で意味を持つ形にそろえる。
    fn normalize(&self, indices: &[usize]) -> Vec<usize> {
        let items = self.0.items.borrow();
        self.selection_mode().normalize(&items, indices)
    }

    /// 選択をネイティブへ写す。
    fn show(&self, picked: &[usize]) {
        self.0.native.unselect_all();
        for &index in picked {
            if let Some(row) = self.0.native.row_at_index(index as i32) {
                self.0.native.select_row(Some(&row));
            }
        }
    }
}

/// 1 行を組み立てる。補助の文字列があれば 2 行目に小さく出す。
fn build_row(item: &ListItem) -> gtk::ListBoxRow {
    let content = gtk::Box::new(gtk::Orientation::Vertical, 2);
    content.set_margin_top(6);
    content.set_margin_bottom(6);
    content.set_margin_start(10);
    content.set_margin_end(10);

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

    let row = gtk::ListBoxRow::new();
    row.set_child(Some(&content));
    // 選べない行は、選択もクリックの反応もしない。
    row.set_selectable(item.enabled);
    row.set_activatable(item.enabled);
    row.set_sensitive(item.enabled);
    row
}
