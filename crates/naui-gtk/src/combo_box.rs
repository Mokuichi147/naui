//! コンボボックス (`GtkDropDown` + `GtkStringList`)。

use std::cell::RefCell;
use std::rc::Rc;

use gtk::glib;
use gtk::prelude::*;

use crate::bin::SizeBin;
use crate::callback::Notifier;
use crate::widgets::{impl_widget, without_signal, Widget};

struct ComboBoxInner {
    native: gtk::DropDown,
    model: gtk::StringList,
    bin: SizeBin,
    on_select: Notifier<usize>,
    handler: RefCell<Option<glib::SignalHandlerId>>,
}

/// 1 項目を選ぶドロップダウン。
#[derive(Clone)]
pub struct ComboBox(Rc<ComboBoxInner>);
impl_widget!(ComboBox);

impl ComboBox {
    pub(crate) fn new() -> Self {
        let model = gtk::StringList::new(&[]);
        let native = gtk::DropDown::builder().model(&model).build();
        native.set_selected(gtk::INVALID_LIST_POSITION);
        let bin = SizeBin::wrap(&native);
        let inner = Rc::new(ComboBoxInner {
            native,
            model,
            bin,
            on_select: Notifier::default(),
            handler: RefCell::new(None),
        });

        let id = {
            let weak = Rc::downgrade(&inner);
            inner.native.connect_selected_notify(move |native| {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                let index = native.selected();
                if index != gtk::INVALID_LIST_POSITION && index < inner.model.n_items() {
                    inner.on_select.emit(index as usize);
                }
            })
        };
        *inner.handler.borrow_mut() = Some(id);
        Self(inner)
    }

    /// 項目を作り直す。インデックスの意味が変わるため、選択は外れる。
    /// 通知は行わない。
    pub fn set_items<S: AsRef<str>>(&self, items: &[S]) {
        let additions: Vec<&str> = items.iter().map(AsRef::as_ref).collect();
        without_signal(&self.0.native, &self.0.handler, || {
            self.0.model.splice(0, self.0.model.n_items(), &additions);
            self.0.native.set_selected(gtk::INVALID_LIST_POSITION);
        });
    }

    pub fn len(&self) -> usize {
        self.0.model.n_items() as usize
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn selected(&self) -> Option<usize> {
        let index = self.0.native.selected();
        (index != gtk::INVALID_LIST_POSITION && index < self.0.model.n_items())
            .then_some(index as usize)
    }

    /// 範囲内なら、通知せずに選択を変える。
    pub fn set_selected(&self, index: usize) {
        if index < self.len() {
            without_signal(&self.0.native, &self.0.handler, || {
                self.0.native.set_selected(index as u32);
            });
        }
    }

    /// 通知せずに選択を外す。
    pub fn clear_selection(&self) {
        without_signal(&self.0.native, &self.0.handler, || {
            self.0.native.set_selected(gtk::INVALID_LIST_POSITION);
        });
    }

    /// ユーザーが選んだのと同じ経路で項目を選び、1 回通知する。
    pub fn select(&self, index: usize) {
        if index < self.len() {
            self.set_selected(index);
            self.0.on_select.emit(index);
        }
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.0.native.set_sensitive(enabled);
    }

    /// 選択が変わったときに、選ばれた項目のインデックスで呼ばれる。
    pub fn on_select(&self, f: impl FnMut(usize) + 'static) {
        self.0.on_select.set(f);
    }
}
