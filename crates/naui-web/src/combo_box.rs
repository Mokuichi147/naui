//! コンボボックス (DOM の `<select>`)。

use std::cell::RefCell;
use std::rc::Rc;

use naui_core::Result;
use wasm_bindgen::JsCast;
use web_sys::{Document, Element, HtmlOptionElement, HtmlSelectElement};

use crate::widgets::{create, impl_widget, Listener, Widget};

type SelectCallback = Box<dyn FnMut(usize)>;

/// 選択された項目の通知先。
///
/// 呼び出している間だけクロージャを取り出すため、通知の中から同じ
/// コンボボックスを操作しても `RefCell` の二重借用にならない。通知中に
/// `on_select` を呼び直した場合は、新しいクロージャを残す。
#[derive(Default)]
struct SelectionHandler(RefCell<Option<SelectCallback>>);

impl SelectionHandler {
    fn set(&self, f: impl FnMut(usize) + 'static) {
        *self.0.borrow_mut() = Some(Box::new(f));
    }

    fn emit(&self, index: usize) {
        let Some(mut f) = self.0.borrow_mut().take() else {
            return;
        };
        f(index);
        let mut slot = self.0.borrow_mut();
        if slot.is_none() {
            *slot = Some(f);
        }
    }
}

struct ComboBoxInner {
    native: HtmlSelectElement,
    document: Document,
    on_select: SelectionHandler,
    /// `change` の購読。落とすと購読も外れる。
    _listener: RefCell<Option<Listener>>,
}

/// 1 項目を選ぶドロップダウン (`<select>`)。
///
/// リストボックスとは異なり `size` 属性を付けず、ブラウザ標準の
/// ドロップダウンとして表示する。
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

    /// 項目を作り直す。インデックスの意味が変わるため、選択は外れる。
    /// 通知は行わない。
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

    /// 範囲内なら、通知せずに選択を変える。
    pub fn set_selected(&self, index: usize) {
        let _ = self.write_selected(index);
    }

    /// 通知せずに選択を外す。
    pub fn clear_selection(&self) {
        self.0.native.set_selected_index(-1);
    }

    /// ユーザーが選んだのと同じ経路で項目を選び、1 回通知する。
    pub fn select(&self, index: usize) {
        if self.write_selected(index) {
            self.0.on_select.emit(index);
        }
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.0.native.set_disabled(!enabled);
    }

    /// 選択が変わったときに、選ばれた項目のインデックスで呼ばれる。
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
