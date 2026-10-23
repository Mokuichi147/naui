//! A collapsible section (`GtkExpander`).
//!
//! GTK4 has a control for the same job (`GtkExpander`), so it is used as is. The
//! heading's triangle and expanding and collapsing on click are done by GTK4. When
//! collapsed, GTK4 removes the content from its internal box (keeping a reference),
//! so no space is left either.

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
    /// Holds the content's handle, keeping it alive along with its callbacks.
    child: RefCell<Option<Box<dyn Widget>>>,
    on_toggle: Notifier<bool>,
    handler: RefCell<Option<glib::SignalHandlerId>>,
}

/// A container whose contents are shown and hidden by clicking its heading
/// (`GtkExpander`).
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

    /// The heading text.
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

    /// Whether it is expanded.
    pub fn is_expanded(&self) -> bool {
        self.0.native.is_expanded()
    }

    /// Expands or collapses it from code. `on_toggle` is not called.
    pub fn set_expanded(&self, expanded: bool) {
        without_signal(&self.0.native, &self.0.handler, || {
            self.0.native.set_expanded(expanded);
        });
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.0.native.set_sensitive(enabled);
    }

    /// The contents to collapse. Each call replaces them.
    pub fn set_child(&self, child: &dyn Widget) {
        let bin = child.size_bin();
        // 中身は展開されたぶんの幅いっぱいに置く (`Scroll` と同じ扱い)。
        bin.fill_parent();
        self.0.native.set_child(Some(&bin));
        *self.0.child.borrow_mut() = Some(child.boxed_clone());
    }

    /// Called with the new state every time the user expands or collapses it.
    pub fn on_toggle(&self, f: impl FnMut(bool) + 'static) {
        self.0.on_toggle.set(f);
    }
}
