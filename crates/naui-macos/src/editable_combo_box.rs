//! A combo box that accepts free input (AppKit's `NSComboBox`).
//!
//! [`ComboBox`](crate::ComboBox) is an `NSPopUpButton`, so it can only choose from
//! the options. This one uses `NSComboBox`, which accepts typing, and handles its
//! value as a **string**. The suggestions are only an input aid; strings that match
//! none still become the value as is.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{MainThreadMarker, MainThreadOnly, Message};
use objc2_app_kit::{NSComboBox, NSView};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};

use crate::trampoline::{ComboObserver, TextHandler};
use crate::widgets::{impl_widget, Widget};

/// The number of suggestions visible at once when the list opens. A little more
/// than AppKit's default (5).
const VISIBLE_ITEMS: isize = 8;

struct EditableComboBoxInner {
    native: Retained<NSComboBox>,
    /// A copy of the suggestions. Used to check matches for `selected` and to write for
    /// `set_selected`.
    items: RefCell<Vec<String>>,
    on_change: TextHandler,
    /// The string last notified. AppKit reports "a suggestion was chosen" and "the
    /// field was rewritten" separately, so duplicate notifications of the same value
    /// are dropped here.
    last: RefCell<String>,
    /// Stops notifications only while rewriting from code.
    silent: Cell<bool>,
    /// AppKit delegates are weak, so it is kept alive on the handle side.
    observer: RefCell<Option<Retained<ComboObserver>>>,
}

/// A field where the user can either choose from suggestions or type freely
/// (`NSComboBox`).
///
/// The value is a string, empty right after creation.
#[derive(Clone)]
pub struct EditableComboBox(Rc<EditableComboBoxInner>);
crate::keys::impl_key_down!(EditableComboBox);
crate::focus::impl_focus_change!(EditableComboBox);
impl_widget!(EditableComboBox);

impl EditableComboBox {
    pub(crate) fn new(mtm: MainThreadMarker) -> Self {
        let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(0.0, 0.0));
        let native = NSComboBox::initWithFrame(NSComboBox::alloc(mtm), frame);
        native.setNumberOfVisibleItems(VISIBLE_ITEMS);
        // 打ちかけの文字を候補で補完する。打ち切れば候補外の文字列も残せる。
        native.setCompletes(true);

        let inner = Rc::new(EditableComboBoxInner {
            native,
            items: RefCell::new(Vec::new()),
            on_change: TextHandler::default(),
            last: RefCell::new(String::new()),
            silent: Cell::new(false),
            observer: RefCell::new(None),
        });

        let observer = ComboObserver::new(mtm, {
            let weak = Rc::downgrade(&inner);
            move |from_list| {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                EditableComboBox(inner).handle(from_list);
            }
        });
        unsafe {
            inner
                .native
                .setDelegate(Some(ProtocolObject::from_ref(&*observer)))
        };
        *inner.observer.borrow_mut() = Some(observer);

        Self(inner)
    }

    /// Rebuilds the suggestions. **The entered text does not change**, and no
    /// notification is sent.
    pub fn set_items<S: AsRef<str>>(&self, items: &[S]) {
        *self.0.items.borrow_mut() = items.iter().map(|s| s.as_ref().to_string()).collect();
        self.without_notifying(|this| {
            this.0.native.removeAllItems();
            for item in items {
                unsafe {
                    this.0
                        .native
                        .addItemWithObjectValue(&NSString::from_str(item.as_ref()))
                };
            }
            // 候補が入れ替わると、今の文字列と一致する候補も変わる。
            this.sync_selection();
        });
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
        self.0.native.stringValue().to_string()
    }

    /// Replaces the text from code. `on_change` is not called.
    pub fn set_text(&self, text: &str) {
        self.without_notifying(|this| {
            this.write_text(text);
            this.sync_selection();
        });
        *self.0.last.borrow_mut() = text.to_string();
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
        self.0
            .native
            .setPlaceholderString(Some(&NSString::from_str(text)));
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.0.native.setEnabled(enabled);
    }

    /// Called with the current contents every time the text changes. Called for both
    /// typing and choosing a suggestion. Setting it again removes the previous one.
    pub fn on_change(&self, f: impl FnMut(&str) + 'static) {
        self.0.on_change.set(f);
    }

    /// The real AppKit control. Public as a backend-specific escape hatch.
    pub fn native_combo_box(&self) -> Retained<NSComboBox> {
        self.0.native.clone()
    }

    /// Merges the two kinds of notifications from AppKit into a single `on_change`.
    fn handle(&self, from_list: bool) {
        if self.0.silent.get() {
            return;
        }
        if from_list {
            // 候補を押した直後、テキスト欄はまだ書き換わっていない。
            // 先に控えから書き入れて、通知の最中でも `text()` が読めるようにする。
            let index = self.0.native.indexOfSelectedItem();
            let picked = usize::try_from(index)
                .ok()
                .and_then(|index| self.0.items.borrow().get(index).cloned());
            if let Some(text) = picked {
                self.without_notifying(|this| this.write_text(&text));
            }
        }
        let text = self.text();
        if *self.0.last.borrow() == text {
            return;
        }
        *self.0.last.borrow_mut() = text.clone();
        self.0.on_change.emit(&text);
    }

    /// Writes only the field's text (`setStringValue:` sends no notification).
    fn write_text(&self, text: &str) {
        self.0.native.setStringValue(&NSString::from_str(text));
    }

    /// Matches the list's selection to the current text. The text itself is kept.
    fn sync_selection(&self) {
        let previous = self.0.native.indexOfSelectedItem();
        match self.selected() {
            Some(index) => self.0.native.selectItemAtIndex(index as isize),
            // 候補と一致しなくなったら、一覧の選択も外す。
            // macOS 27 では選択を外すと欄の文字列まで空になるので、控えて書き戻す。
            None if previous >= 0 => {
                let text = self.0.native.stringValue();
                self.without_notifying(|this| {
                    this.0.native.deselectItemAtIndex(previous);
                    this.0.native.setStringValue(&text);
                });
            }
            None => {}
        }
    }

    /// Operates with notifications from AppKit stopped.
    fn without_notifying<R>(&self, f: impl FnOnce(&Self) -> R) -> R {
        let previous = self.0.silent.replace(true);
        let result = f(self);
        self.0.silent.set(previous);
        result
    }
}
