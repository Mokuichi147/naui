//! 入り切りのスイッチ (`GtkSwitch` + ラベルの `GtkLabel`)。
//!
//! `GtkSwitch` は入り切りの部分だけを持ち、文字は持たない (GNOME の設定に
//! 並ぶスイッチと同じ)。ラベルはとなりへ `GtkLabel` を並べて添える。

use std::cell::RefCell;
use std::rc::Rc;

use gtk::glib;
use gtk::prelude::*;

use crate::bin::SizeBin;
use crate::callback::Notifier;
use crate::widgets::{impl_widget, without_signal, Widget};

/// スイッチとラベルのすき間 (論理ピクセル)。
const SPACING: i32 = 8;

struct ToggleInner {
    native: gtk::Box,
    switch: gtk::Switch,
    bin: SizeBin,
    on_toggle: Notifier<bool>,
    handler: RefCell<Option<glib::SignalHandlerId>>,
}

/// 入り切りを切り替えるスイッチ (`GtkSwitch`)。
#[derive(Clone)]
pub struct Toggle(Rc<ToggleInner>);
impl_widget!(Toggle);

impl Toggle {
    pub(crate) fn new(label: &str) -> Self {
        let native = gtk::Box::new(gtk::Orientation::Horizontal, SPACING);

        let switch = gtk::Switch::new();
        // スイッチは自分の大きさのままでいる (行いっぱいに伸びない)。
        switch.set_valign(gtk::Align::Center);
        switch.set_halign(gtk::Align::Start);

        let text = gtk::Label::new(Some(label));
        text.set_xalign(0.0);

        native.append(&switch);
        native.append(&text);
        let bin = SizeBin::wrap(&native);

        let inner = Rc::new(ToggleInner {
            native,
            switch,
            bin,
            on_toggle: Notifier::default(),
            handler: RefCell::new(None),
        });
        let id = {
            let weak = Rc::downgrade(&inner);
            inner.switch.connect_active_notify(move |switch| {
                if let Some(inner) = weak.upgrade() {
                    inner.on_toggle.emit(switch.is_active());
                }
            })
        };
        *inner.handler.borrow_mut() = Some(id);
        Self(inner)
    }

    /// 入っているかどうか。
    pub fn is_on(&self) -> bool {
        self.0.switch.is_active()
    }

    /// プログラムから切り替える。`on_toggle` は呼ばれない。
    pub fn set_on(&self, on: bool) {
        without_signal(&self.0.switch, &self.0.handler, || {
            self.0.switch.set_active(on);
        });
    }

    pub fn set_enabled(&self, enabled: bool) {
        // 箱ごと切ると、スイッチもラベルも同じ見た目になる。
        self.0.native.set_sensitive(enabled);
    }

    /// 利用者が切り替えるたびに、切り替えた後の状態で呼ばれる。
    pub fn on_toggle(&self, f: impl FnMut(bool) + 'static) {
        self.0.on_toggle.set(f);
    }

    /// スイッチ本体。バックエンド固有の脱出口として公開している。
    pub fn native_switch(&self) -> gtk::Switch {
        self.0.switch.clone()
    }
}
