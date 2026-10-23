//! A combo box (the DOM's `<select>`).

use std::cell::RefCell;
use std::rc::Rc;

use naui_core::Result;
use wasm_bindgen::JsCast;
use web_sys::{Document, Element, HtmlOptionElement, HtmlSelectElement};

use crate::widgets::{create, impl_widget, Listener, SelectionHandler, Widget};

struct ComboBoxInner {
    native: HtmlSelectElement,
    document: Document,
    on_select: SelectionHandler,
    /// The `change` subscription. Dropping it also unsubscribes.
    _listener: RefCell<Option<Listener>>,
}

/// A drop-down for choosing one item (`<select>`).
///
/// Unlike a list box, it has no `size` attribute and is shown as the browser's
/// standard drop-down.
#[derive(Clone)]
pub struct ComboBox(Rc<ComboBoxInner>);
impl_widget!(ComboBox, native);

impl ComboBox {
    pub(crate) fn new(document: &Document) -> Result<Self> {
        let native: HtmlSelectElement = create(document, "select")?.unchecked_into();
        // 項目を追加したときにも最初の行を暗黙選択させず、naui の初期状態を
        // 一貫して「未選択」にする。
        native.set_selected_index(-1);

        let inner = Rc::new(ComboBoxInner {
            native,
            document: document.clone(),
            on_select: SelectionHandler::default(),
            _listener: RefCell::new(None),
        });
        let listener = Listener::attach(inner.native.as_ref(), "change", {
            let weak = Rc::downgrade(&inner);
            move || {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                let index = inner.native.selected_index();
                if index >= 0 && (index as usize) < inner.native.length() as usize {
                    inner.on_select.emit(index as usize);
                }
            }
        })?;
        *inner._listener.borrow_mut() = Some(listener);
        Ok(Self(inner))
    }

    /// Rebuilds the items. Indices change meaning, so the selection is cleared. No
    /// notification is sent.
    pub fn set_items<S: AsRef<str>>(&self, items: &[S]) {
        while let Some(child) = self.0.native.first_child() {
            let _ = self.0.native.remove_child(&child);
        }
        for item in items {
            let Ok(option) = create(&self.0.document, "option") else {
                continue;
            };
            let option: HtmlOptionElement = option.unchecked_into();
            option.set_text_content(Some(item.as_ref()));
            let _ = self.0.native.append_child(&option);
        }
        // `<select>` は最初の `<option>` を自動選択するので、追加後に外す。
        self.0.native.set_selected_index(-1);
    }

    pub fn len(&self) -> usize {
        self.0.native.length() as usize
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn selected(&self) -> Option<usize> {
        let index = self.0.native.selected_index();
        (index >= 0 && (index as usize) < self.len()).then_some(index as usize)
    }

    /// Changes the selection without notifying, if the index is in range.
    pub fn set_selected(&self, index: usize) {
        let _ = self.write_selected(index);
    }

    /// Clears the selection without notifying.
    pub fn clear_selection(&self) {
        self.0.native.set_selected_index(-1);
    }

    /// Selects an item through the same path as a user's choice and notifies once.
    pub fn select(&self, index: usize) {
        if self.write_selected(index) {
            self.0.on_select.emit(index);
        }
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.0.native.set_disabled(!enabled);
    }

    /// Called with the index of the chosen item when the selection changes.
    pub fn on_select(&self, f: impl FnMut(usize) + 'static) {
        self.0.on_select.set(f);
    }

    fn write_selected(&self, index: usize) -> bool {
        if index >= self.len() {
            return false;
        }
        let Ok(index) = i32::try_from(index) else {
            return false;
        };
        self.0.native.set_selected_index(index);
        true
    }
}
