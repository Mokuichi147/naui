//! 選択できる行の一覧 (`GtkListBox` を `GtkScrolledWindow` に載せたもの)。
//!
//! 文字だけの [`ListItem`] も任意内容の [`ListRow`] も同じ行モデルへ正規化し、
//! 同じ経路で `GtkListBoxRow` を組み立てる。選択と行 activation は
//! Rust のクロージャへ渡す。

use std::cell::RefCell;
use std::rc::Rc;

use gtk::glib;
use gtk::prelude::*;
use naui_core::{ListItem, SelectionMode};

use crate::bin::SizeBin;
use crate::callback::SelectionNotifier;
use crate::widgets::{impl_widget, without_signal, Widget};

/// 行がクリックされたことの通知先。
///
/// 呼び出し中に同じ行のコールバックを差し替えても二重借用しない。
#[derive(Clone, Default)]
struct ActivationHandler(Rc<RefCell<Option<Box<dyn FnMut()>>>>);

impl ActivationHandler {
    fn set(&self, f: impl FnMut() + 'static) {
        *self.0.borrow_mut() = Some(Box::new(f));
    }

    fn is_some(&self) -> bool {
        self.0.borrow().is_some()
    }

    fn emit(&self) {
        let Some(mut f) = self.0.borrow_mut().take() else {
            return;
        };
        f();
        let mut slot = self.0.borrow_mut();
        if slot.is_none() {
            *slot = Some(f);
        }
    }
}

/// 1 行の内容。通常の文字行も任意ウィジェット行も同じ型へ正規化する。
enum ListRowContent {
    Item(ListItem),
    Custom(Box<dyn Widget>),
}

impl Clone for ListRowContent {
    fn clone(&self) -> Self {
        match self {
            Self::Item(item) => Self::Item(item.clone()),
            Self::Custom(content) => Self::Custom(content.boxed_clone()),
        }
    }
}

/// リストへ載せる 1 行。
///
/// [`ListItem`] は文字列だけで済む一覧向けの簡便 API であり、設定画面のように
/// アイコン、複数のラベル、チェックボックス、末尾のボタンなどを組み合わせる
/// 行は `Grid` / `Stack` で作って `ListRow` に包む。
///
/// ```no_run
/// # use naui_gtk::{ListRow, Widget};
/// # fn row(content: &dyn Widget) {
/// let row = ListRow::new(content).selectable(false);
/// row.on_activate(|| println!("行がクリックされました"));
/// # let _ = row;
/// # }
/// ```
pub struct ListRow {
    content: ListRowContent,
    selectable: bool,
    activation: ActivationHandler,
}

impl Clone for ListRow {
    fn clone(&self) -> Self {
        Self {
            content: self.content.clone(),
            selectable: self.selectable,
            activation: self.activation.clone(),
        }
    }
}

impl ListRow {
    /// ウィジェットを 1 行の内容として使う。既定では行全体も選択できる。
    pub fn new(content: &dyn Widget) -> Self {
        Self {
            content: ListRowContent::Custom(content.boxed_clone()),
            selectable: true,
            activation: ActivationHandler::default(),
        }
    }

    /// `ListItem` を通常の文字行へ変換する。`List::set_items` の正規化経路。
    fn from_item(item: ListItem) -> Self {
        let selectable = item.enabled;
        Self {
            content: ListRowContent::Item(item),
            selectable,
            activation: ActivationHandler::default(),
        }
    }

    /// 行内のコントロールだけを操作する行では `false` にする。
    pub fn selectable(mut self, selectable: bool) -> Self {
        self.selectable = selectable;
        self
    }

    pub fn is_selectable(&self) -> bool {
        self.selectable
    }

    /// 行内のラベル・アイコン・余白がクリックされたときに呼ぶ処理。
    ///
    /// チェックボックス、ボタン、入力欄などのコントロールを直接押した場合は
    /// (それぞれが自分でクリックを受け取るため) 呼ばれず、同じ操作が二重に
    /// 発火しない。
    ///
    /// `GtkListBoxRow` を押せるようにするかどうかは行を組み立てるときに
    /// 決まるので、[`List::set_rows`] へ渡す前に設定する。
    pub fn on_activate(&self, f: impl FnMut() + 'static) {
        self.activation.set(f);
    }
}

struct ListInner {
    native: gtk::ListBox,
    /// `GtkListBox` は自分でスクロールしないので、スクロール領域に載せる。
    _scroller: gtk::ScrolledWindow,
    bin: SizeBin,
    rows: RefCell<Vec<ListRow>>,
    on_select: SelectionNotifier,
    handler: RefCell<Option<glib::SignalHandlerId>>,
}

/// 選択できる行の一覧。自分でスクロールする。
///
/// 高さを指定しないときは全行の高さに追従し、固定高さや `Fill` では
/// はみ出した分をスクロールする。
#[derive(Clone)]
pub struct List(Rc<ListInner>);
impl_widget!(List);

impl List {
    pub(crate) fn new() -> Self {
        let native = gtk::ListBox::new();
        native.set_selection_mode(gtk::SelectionMode::Single);
        // libadwaita の `boxed-list` (角丸のカード) は付けない。スクロール領域の
        // 枠と二重になって、角丸の中身に四角い枠が付いて見えるため。枠のほうを
        // 残すのは、一覧の高さが指定で決まる (行が少なくても場所を取る) ので、
        // どこまでが一覧なのかが分かるようにするため。macOS の `NSTableView` /
        // Windows の `ListBox` / Web の `<select size>` とも同じ見え方になる。
        let scroller = gtk::ScrolledWindow::new();
        scroller.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        scroller.set_has_frame(true);
        scroller.set_propagate_natural_height(true);
        scroller.set_child(Some(&native));

        let bin = SizeBin::wrap(&scroller);
        let inner = Rc::new(ListInner {
            native,
            _scroller: scroller,
            bin,
            rows: RefCell::new(Vec::new()),
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
        let rows: Vec<ListRow> = items.iter().cloned().map(ListRow::from_item).collect();
        self.set_rows(&rows);
    }

    /// 行を作り直す。通常行も任意内容の行もこの経路で描画する。
    ///
    /// 行内のコントロールは通常どおりそれぞれのコールバックを持てる。
    pub fn set_rows(&self, rows: &[ListRow]) {
        without_signal(&self.0.native, &self.0.handler, || {
            while let Some(row) = self.0.native.first_child() {
                self.0.native.remove(&row);
            }
            for row in rows {
                self.0.native.append(&build_row(row));
            }
            self.0.native.unselect_all();
        });
        *self.0.rows.borrow_mut() = rows.to_vec();
    }

    pub fn len(&self) -> usize {
        self.0.rows.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn set_selection_mode(&self, mode: SelectionMode) {
        let multiple = mode.is_multiple();
        self.0.native.set_selection_mode(if multiple {
            // `Multiple` は「⌘ / Ctrl や Shift を押しながら選ぶ」形。
            gtk::SelectionMode::Multiple
        } else {
            gtk::SelectionMode::Single
        });
        // `GtkListBox` は「1 クリックで確定」(既定) の間、クリックに付いている
        // Ctrl / Shift を読まず、必ず「その行だけを選ぶ」に倒す。複数選択では
        // これを切らないと、行を足すことも外すこともできない。
        self.0.native.set_activate_on_single_click(!multiple);
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
        let rows = self.0.rows.borrow();
        self.selection_mode().normalize_by(indices, |index| {
            rows.get(index).is_some_and(ListRow::is_selectable)
        })
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

/// 1 行を組み立てる。`ListItem` も任意内容も同じ経路を通る。
fn build_row(item: &ListRow) -> gtk::ListBoxRow {
    let content: gtk::Widget = match &item.content {
        ListRowContent::Item(item) => {
            let content = gtk::Box::new(gtk::Orientation::Vertical, 2);
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
            content.upcast()
        }
        ListRowContent::Custom(content) => content.size_bin().upcast(),
    };
    content.set_margin_top(6);
    content.set_margin_bottom(6);
    content.set_margin_start(10);
    content.set_margin_end(10);

    let row = gtk::ListBoxRow::new();
    row.set_child(Some(&content));
    row.set_selectable(item.selectable);
    row.set_activatable(item.selectable || item.activation.is_some());
    if matches!(&item.content, ListRowContent::Item(item) if !item.enabled) {
        row.set_sensitive(false);
    }
    let activation = item.activation.clone();
    row.connect_activate(move |_| activation.emit());
    row
}
