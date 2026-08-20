//! 選択肢を並べて 1 つだけ選ばせるラジオグループ (`GtkCheckButton` の組)。
//!
//! GTK4 では `GtkRadioButton` が廃止され、`GtkCheckButton` を
//! `gtk_check_button_set_group` で束ねたものがラジオになる。束ねたボタンを
//! `GtkBox` へ並べることで、ほかのラジオグループと混ざらない。

use std::cell::RefCell;
use std::rc::Rc;

use gtk::glib;
use gtk::prelude::*;
use naui_core::Orientation;

use crate::bin::SizeBin;
use crate::callback::Notifier;
use crate::widgets::{impl_widget, Widget};

struct RadioGroupInner {
    native: gtk::Box,
    bin: SizeBin,
    buttons: RefCell<Vec<gtk::CheckButton>>,
    /// ボタンと同じ順に並ぶ `toggled` の購読。プログラムからの変更で止める。
    handlers: RefCell<Vec<glib::SignalHandlerId>>,
    on_select: Notifier<usize>,
}

/// 選択肢を並べて 1 つだけ選ばせるラジオグループ。
///
/// 作った直後と [`set_items`](Self::set_items) の直後は、何も選ばれていない。
#[derive(Clone)]
pub struct RadioGroup(Rc<RadioGroupInner>);
impl_widget!(RadioGroup);

impl RadioGroup {
    pub(crate) fn new() -> Self {
        let native = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let bin = SizeBin::wrap(&native);
        Self(Rc::new(RadioGroupInner {
            native,
            bin,
            buttons: RefCell::new(Vec::new()),
            handlers: RefCell::new(Vec::new()),
            on_select: Notifier::default(),
        }))
    }

    /// 選択肢を作り直し、選択を外す。選択通知は発生しない。
    pub fn set_items<S: AsRef<str>>(&self, items: &[S]) {
        for button in self.0.buttons.borrow_mut().drain(..) {
            // グループから外してから捨てないと、GTK4 が残りの組を触りにいく。
            button.set_group(gtk::CheckButton::NONE);
            self.0.native.remove(&button);
        }
        self.0.handlers.borrow_mut().clear();

        let mut buttons: Vec<gtk::CheckButton> = Vec::with_capacity(items.len());
        for item in items {
            let button = gtk::CheckButton::with_label(item.as_ref());
            crate::indicator::watch(&button);
            if let Some(first) = buttons.first() {
                button.set_group(Some(first));
            }
            self.0.native.append(&button);
            buttons.push(button);
        }

        // 購読はボタンをすべて並べ終えてからつなぐ。組み立ての最中に出る
        // `toggled` を通知として拾わないため。
        let handlers = buttons
            .iter()
            .enumerate()
            .map(|(index, button)| {
                let weak = Rc::downgrade(&self.0);
                button.connect_toggled(move |button| {
                    // 組の中では「外れた側」でも `toggled` が出る。点いたほうだけ通知する。
                    if !button.is_active() {
                        return;
                    }
                    if let Some(inner) = weak.upgrade() {
                        inner.on_select.emit(index);
                    }
                })
            })
            .collect();

        *self.0.buttons.borrow_mut() = buttons;
        *self.0.handlers.borrow_mut() = handlers;
    }

    /// 選択肢の数。
    pub fn len(&self) -> usize {
        self.0.buttons.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// 現在選ばれている選択肢。未選択なら `None`。
    pub fn selected(&self) -> Option<usize> {
        self.0
            .buttons
            .borrow()
            .iter()
            .position(|button| button.is_active())
    }

    /// 範囲内の選択肢を通知せずに選ぶ。範囲外なら何もしない。
    pub fn set_selected(&self, index: usize) {
        if index < self.len() {
            self.without_signals(|buttons| buttons[index].set_active(true));
        }
    }

    /// 選択を通知せずに外す。
    pub fn clear_selection(&self) {
        self.without_signals(|buttons| {
            for button in buttons {
                button.set_active(false);
            }
        });
    }

    /// ユーザーが選んだのと同じように、範囲内の選択肢を選んで通知する。
    pub fn select(&self, index: usize) {
        if index < self.len() {
            self.set_selected(index);
            self.0.on_select.emit(index);
        }
    }

    /// 選択肢の並ぶ向き。既定は縦。
    pub fn set_orientation(&self, orientation: Orientation) {
        self.0.native.set_orientation(if orientation.is_vertical() {
            gtk::Orientation::Vertical
        } else {
            gtk::Orientation::Horizontal
        });
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.0.native.set_sensitive(enabled);
    }

    /// 選択肢が選ばれたときに、そのインデックスで呼ばれる。
    /// 設定し直すと以前のコールバックは外れる。
    pub fn on_select(&self, f: impl FnMut(usize) + 'static) {
        self.0.on_select.set(f);
    }

    /// 組にした `GtkCheckButton`。バックエンド固有の脱出口として公開している。
    pub fn native_buttons(&self) -> Vec<gtk::CheckButton> {
        self.0.buttons.borrow().clone()
    }

    /// 組にしたボタンすべての `toggled` を止めたまま操作する。
    ///
    /// 組の中では 1 つ点けると別の 1 つが外れるので、片方だけ止めても足りない。
    fn without_signals<T>(&self, f: impl FnOnce(&[gtk::CheckButton]) -> T) -> T {
        let buttons = self.0.buttons.borrow();
        let handlers = self.0.handlers.borrow();
        for (button, handler) in buttons.iter().zip(handlers.iter()) {
            button.block_signal(handler);
        }
        let out = f(&buttons);
        for (button, handler) in buttons.iter().zip(handlers.iter()) {
            button.unblock_signal(handler);
        }
        out
    }
}
