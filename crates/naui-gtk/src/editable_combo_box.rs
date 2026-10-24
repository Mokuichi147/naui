//! A combo box that accepts free input (`GtkEntry` + a `GtkListBox` in a
//! `GtkMenuButton`).
//!
//! GTK4 has no "single control you can type into". `GtkDropDown` only allows
//! choosing from the options, and `GtkComboBoxText` and `GtkEntryCompletion`, which
//! have an entry, were deprecated in GTK 4.10 with no replacement. So a `GtkEntry`
//! and a `GtkMenuButton` that shows the list of suggestions are put side by side in
//! a `.linked` `GtkBox` (the same shape as the calendar shown by `DatePicker`).
//!
//! GTK4 does the drawing; naui only arranges them. Clicking a suggestion writes it
//! into the entry and closes the popover.

use std::cell::RefCell;
use std::rc::Rc;

use gtk::glib;
use gtk::prelude::*;

use crate::bin::SizeBin;
use crate::callback::TextNotifier;
use crate::widgets::{impl_widget, without_signal, Widget};

/// The maximum height of the suggestion list (logical pixels). Beyond this it
/// scrolls.
const MAX_LIST_HEIGHT: i32 = 300;

struct EditableComboBoxInner {
    native: gtk::Box,
    entry: gtk::Entry,
    button: gtk::MenuButton,
    popover: gtk::Popover,
    list: gtk::ListBox,
    bin: SizeBin,
    /// A copy of the suggestions. Used to check matches for `selected` and to look up
    /// the clicked row.
    items: RefCell<Vec<String>>,
    on_change: TextNotifier,
    /// The entry's `changed`. Blocked while writing from code.
    handler: RefCell<Option<glib::SignalHandlerId>>,
}

/// A field where the user can either choose from suggestions or type freely.
///
/// The value is a string, empty right after creation.
#[derive(Clone)]
pub struct EditableComboBox(Rc<EditableComboBoxInner>);
crate::keys::impl_key_down!(EditableComboBox);
crate::focus::impl_focus_change!(EditableComboBox);
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

    /// Rebuilds the suggestions. **The entered text does not change**, and no
    /// notification is sent.
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

    /// The number of suggestions.
    pub fn len(&self) -> usize {
        self.0.items.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The entered text.
    pub fn text(&self) -> String {
        self.0.entry.text().to_string()
    }

    /// Replaces the text from code. `on_change` is not called.
    pub fn set_text(&self, text: &str) {
        without_signal(&self.0.entry, &self.0.handler, || {
            self.0.entry.set_text(text);
        });
    }

    /// The position of the suggestion that **exactly matches** the entered text.
    ///
    /// `None` if the typed text matches no suggestion.
    pub fn selected(&self) -> Option<usize> {
        let text = self.text();
        self.0.items.borrow().iter().position(|item| *item == text)
    }

    /// Chooses an in-range suggestion without notifying. Does nothing if out of range.
    pub fn set_selected(&self, index: usize) {
        let Some(text) = self.0.items.borrow().get(index).cloned() else {
            return;
        };
        self.set_text(&text);
    }

    /// Empties the text without notifying.
    pub fn clear(&self) {
        self.set_text("");
    }

    /// Chooses an in-range suggestion and notifies, as if the user had chosen it.
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

    /// Called with the current contents every time the text changes. Called for both
    /// typing and choosing a suggestion. Setting it again removes the previous one.
    pub fn on_change(&self, f: impl FnMut(&str) + 'static) {
        self.0.on_change.set(f);
    }

    /// The entry's `GtkEntry`. Public as a backend-specific escape hatch.
    pub fn native_entry(&self) -> gtk::Entry {
        self.0.entry.clone()
    }

    /// The button that shows the suggestion list. Public as a backend-specific escape
    /// hatch.
    pub fn native_button(&self) -> gtk::MenuButton {
        self.0.button.clone()
    }

    /// The `GtkListBox` holding the suggestions. Public as a backend-specific escape
    /// hatch.
    pub fn native_list(&self) -> gtk::ListBox {
        self.0.list.clone()
    }

    /// Handles a click on a list row: writes it in, closes and notifies, once each.
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
