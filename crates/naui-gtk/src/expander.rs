//! 折りたたみ (`GtkExpander`)。
//!
//! GTK4 に同じ役目のコントロール (`GtkExpander`) があるので、そのまま使う。
//! 見出しの三角も、押したときの開閉も GTK4 が行う。たたむと、GTK4 は中身を
//! 内部の箱から外す (参照は持ったまま) ので、場所も空かない。

use std::cell::RefCell;
use std::rc::Rc;

use gtk::glib;
use gtk::prelude::*;

use crate::bin::SizeBin;
use crate::callback::Notifier;
use crate::widgets::{impl_widget, without_signal, Widget};

struct ExpanderInner {
    native: gtk::Expander,
    bin: SizeBin,
    /// 中身のハンドルを保持し、コールバックごと生かしておく。
    child: RefCell<Option<Box<dyn Widget>>>,
    on_toggle: Notifier<bool>,
    handler: RefCell<Option<glib::SignalHandlerId>>,
}

/// 見出しを押して中身を出し入れするコンテナ (`GtkExpander`)。
#[derive(Clone)]
pub struct Expander(Rc<ExpanderInner>);
impl_widget!(Expander);

impl Expander {
    pub(crate) fn new(text: &str) -> Self {
        let native = gtk::Expander::new(Some(text));
        let bin = SizeBin::wrap(&native);
        let inner = Rc::new(ExpanderInner {
            native,
            bin,
            child: RefCell::new(None),
            on_toggle: Notifier::default(),
            handler: RefCell::new(None),
        });
        let id = {
            let weak = Rc::downgrade(&inner);
            inner.native.connect_expanded_notify(move |native| {
                if let Some(inner) = weak.upgrade() {
                    inner.on_toggle.emit(native.is_expanded());
                }
            })
        };
        *inner.handler.borrow_mut() = Some(id);
        Self(inner)
    }

    /// 見出しの文字。
    pub fn text(&self) -> String {
        self.0
            .native
            .label()
            .map(|label| label.to_string())
            .unwrap_or_default()
    }

    pub fn set_text(&self, text: &str) {
        self.0.native.set_label(Some(text));
    }

    /// 開いているかどうか。
    pub fn is_expanded(&self) -> bool {
        self.0.native.is_expanded()
    }

    /// プログラムから開閉する。`on_toggle` は呼ばれない。
    pub fn set_expanded(&self, expanded: bool) {
        without_signal(&self.0.native, &self.0.handler, || {
            self.0.native.set_expanded(expanded);
        });
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.0.native.set_sensitive(enabled);
    }

    /// 折りたたむ中身。呼ぶたびに置き換わる。
    pub fn set_child(&self, child: &dyn Widget) {
        let bin = child.size_bin();
        // 中身は展開されたぶんの幅いっぱいに置く (`Scroll` と同じ扱い)。
        bin.fill_parent();
        self.0.native.set_child(Some(&bin));
        *self.0.child.borrow_mut() = Some(child.boxed_clone());
    }

    /// 利用者が開閉するたびに、変わった後の状態で呼ばれる。
    pub fn on_toggle(&self, f: impl FnMut(bool) + 'static) {
        self.0.on_toggle.set(f);
    }
}
