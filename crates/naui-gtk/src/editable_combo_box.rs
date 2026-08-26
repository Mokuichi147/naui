//! 自由に入力できるコンボボックス (`GtkEntry` + `GtkMenuButton` の `GtkListBox`)。
//!
//! GTK4 には「打ち込める 1 つのコントロール」が無い。`GtkDropDown` は候補から
//! しか選べず、入力欄を持つ `GtkComboBoxText` と `GtkEntryCompletion` は
//! GTK 4.10 で非推奨になり、置き換え先も用意されていない。そこで
//! `GtkEntry` と、候補の一覧を出す `GtkMenuButton` を `.linked` の `GtkBox` へ
//! 並べて組む (`DatePicker` がカレンダーを出しているのと同じ形)。
//!
//! 描くのは GTK4 で、naui が作るのは並びだけ。候補を押すと入力欄へ書き入れて
//! ポップオーバーを閉じる。

use std::cell::RefCell;
use std::rc::Rc;

use gtk::glib;
use gtk::prelude::*;

use crate::bin::SizeBin;
use crate::callback::TextNotifier;
use crate::widgets::{impl_widget, without_signal, Widget};

/// 候補の一覧に出す高さの上限 (論理ピクセル)。これを超えるとスクロールする。
const MAX_LIST_HEIGHT: i32 = 300;

struct EditableComboBoxInner {
    native: gtk::Box,
    entry: gtk::Entry,
    button: gtk::MenuButton,
    popover: gtk::Popover,
    list: gtk::ListBox,
    bin: SizeBin,
    /// 候補の控え。`selected` の一致判定と、押された行の取り出しに使う。
    items: RefCell<Vec<String>>,
    on_change: TextNotifier,
    /// 入力欄の `changed`。プログラムから書くときに止める。
    handler: RefCell<Option<glib::SignalHandlerId>>,
}

/// 候補から選ぶことも、自由に打ち込むこともできる入力欄。
///
/// 値は文字列で、作った直後は空。
#[derive(Clone)]
pub struct EditableComboBox(Rc<EditableComboBoxInner>);
impl_widget!(EditableComboBox);

impl EditableComboBox {
    pub(crate) fn new() -> Self {
        let native = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        // 入力欄とボタンを 1 つのコントロールに見せる GTK4 の作法。
        native.add_css_class("linked");

        let entry = gtk::Entry::new();
        entry.set_hexpand(true);

        let list = gtk::ListBox::new();
        // 押した行は `row-activated` で受けるので、選択の色は残さない。
        list.set_selection_mode(gtk::SelectionMode::None);

        let scroll = gtk::ScrolledWindow::new();
        scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        scroll.set_propagate_natural_height(true);
        scroll.set_max_content_height(MAX_LIST_HEIGHT);
        scroll.set_child(Some(&list));

        let popover = gtk::Popover::new();
        popover.set_position(gtk::PositionType::Bottom);
        popover.set_child(Some(&scroll));

        let button = gtk::MenuButton::new();
        button.set_icon_name("pan-down-symbolic");
        button.set_popover(Some(&popover));

        native.append(&entry);
        native.append(&button);
        let bin = SizeBin::wrap(&native);

        let inner = Rc::new(EditableComboBoxInner {
            native,
            entry,
            button,
            popover,
            list,
            bin,
            items: RefCell::new(Vec::new()),
            on_change: TextNotifier::default(),
            handler: RefCell::new(None),
        });

        let id = {
            let weak = Rc::downgrade(&inner);
            inner.entry.connect_changed(move |entry| {
                if let Some(inner) = weak.upgrade() {
                    inner.on_change.emit(entry.text().as_str());
                }
            })
        };
        *inner.handler.borrow_mut() = Some(id);

        {
            let weak = Rc::downgrade(&inner);
            inner.list.connect_row_activated(move |_list, row| {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                let index = usize::try_from(row.index()).unwrap_or(usize::MAX);
                EditableComboBox(inner).pick(index);
            });
        }

        // ポップオーバーは押したボタンの幅に合わせて開くので、そのままでは
        // 矢印ぶんの幅しか無い。画面に出るときに入力欄を含めた幅へ広げる
        // (`show` シグナルは GTK 4.10 で非推奨なので `map` を使う)。
        {
            let weak = Rc::downgrade(&inner);
            inner.popover.connect_map(move |popover| {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                popover.set_size_request(inner.native.width(), -1);
            });
        }

        Self(inner)
    }

    /// 候補を作り直す。**入力されている文字列は変わらず**、通知も出ない。
    pub fn set_items<S: AsRef<str>>(&self, items: &[S]) {
        while let Some(row) = self.0.list.first_child() {
            self.0.list.remove(&row);
        }
        for item in items {
            let label = gtk::Label::new(Some(item.as_ref()));
            label.set_xalign(0.0);
            let row = gtk::ListBoxRow::new();
            row.set_child(Some(&label));
            self.0.list.append(&row);
        }
        *self.0.items.borrow_mut() = items.iter().map(|s| s.as_ref().to_string()).collect();
    }

    /// 候補の数。
    pub fn len(&self) -> usize {
        self.0.items.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// 入力されている文字列。
    pub fn text(&self) -> String {
        self.0.entry.text().to_string()
    }

    /// プログラムから文字列を差し替える。`on_change` は呼ばれない。
    pub fn set_text(&self, text: &str) {
        without_signal(&self.0.entry, &self.0.handler, || {
            self.0.entry.set_text(text);
        });
    }

    /// 入力されている文字列と**そのまま一致する**候補の位置。
    ///
    /// 打ち込まれた文字列がどの候補とも一致しなければ `None`。
    pub fn selected(&self) -> Option<usize> {
        let text = self.text();
        self.0.items.borrow().iter().position(|item| *item == text)
    }

    /// 範囲内の候補を通知せずに選ぶ。範囲外なら何もしない。
    pub fn set_selected(&self, index: usize) {
        let Some(text) = self.0.items.borrow().get(index).cloned() else {
            return;
        };
        self.set_text(&text);
    }

    /// 通知せずに文字列を空にする。
    pub fn clear(&self) {
        self.set_text("");
    }

    /// 利用者が候補を選んだのと同じように、範囲内の候補を選んで通知する。
    pub fn select(&self, index: usize) {
        let Some(text) = self.0.items.borrow().get(index).cloned() else {
            return;
        };
        self.set_text(&text);
        self.0.on_change.emit(&text);
    }

    pub fn set_placeholder(&self, text: &str) {
        self.0.entry.set_placeholder_text(Some(text));
    }

    pub fn set_enabled(&self, enabled: bool) {
        // 箱ごと切ると、入力欄も矢印のボタンも同じ見た目になる。
        self.0.native.set_sensitive(enabled);
    }

    /// 文字列が変わるたびに、その時点の中身で呼ばれる。
    /// 打鍵と候補の選択のどちらでも呼ばれる。設定し直すと以前のものは外れる。
    pub fn on_change(&self, f: impl FnMut(&str) + 'static) {
        self.0.on_change.set(f);
    }

    /// 入力欄の `GtkEntry`。バックエンド固有の脱出口として公開している。
    pub fn native_entry(&self) -> gtk::Entry {
        self.0.entry.clone()
    }

    /// 候補の一覧を出すボタン。バックエンド固有の脱出口として公開している。
    pub fn native_button(&self) -> gtk::MenuButton {
        self.0.button.clone()
    }

    /// 候補が並ぶ `GtkListBox`。バックエンド固有の脱出口として公開している。
    pub fn native_list(&self) -> gtk::ListBox {
        self.0.list.clone()
    }

    /// 一覧の行が押されたときの処理。書き入れ・閉じる・通知を 1 回ずつ行う。
    fn pick(&self, index: usize) {
        let Some(text) = self.0.items.borrow().get(index).cloned() else {
            return;
        };
        // `changed` は止めて書き、通知は下で 1 回だけ出す。同じ文字列を
        // 選び直したときにも通知が届くようにするため。
        self.set_text(&text);
        self.0.popover.popdown();
        self.0.on_change.emit(&text);
    }
}
