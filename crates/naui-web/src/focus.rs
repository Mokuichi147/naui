//! `on_focus_change` (text-input widgets).
//!
//! One `focusin` and one `focusout` are attached to the document, and walking up
//! the parents from where focus moved, the first registered widget found becomes
//! the focus owner ([`FocusOwner`]). Notifications go out only when the owner
//! changes, so moving from element to element within a widget does not notify.
//!
//! naui's Web windows are elements in the same document, so there is one owner per
//! document. **When the tab or the browser window itself loses focus, it is not
//! counted** (the element's `blur` fires, but `document.hasFocus()` becomes false,
//! and the `focus` on returning is for the same owner, so nothing is notified).
//! This is the same treatment as when a window becomes inactive on the other three
//! platforms.
//!
//! The registry holds elements and weak references to the handlers. The handler
//! itself is held by the widget ([`Focus`]), and the table entry is removed when
//! the widget is freed. Leaving it in the table would keep the element of a
//! discarded field from ever being freed.
//!
//! The document subscriptions are called re-entrantly. Calling `request_focus()` on
//! another field inside a notification makes `focus()` synchronously fire the next
//! `focusout` / `focusin` ([`Listener::attach_reentrant`]). [`FocusOwner`] queues
//! owner moves, so inner moves are not lost either.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use naui_core::{FocusHandler, FocusOwner};
use wasm_bindgen::JsCast;
use web_sys::{Element, FocusEvent, Node};

use crate::widgets::Listener;

thread_local! {
    static ENTRIES: RefCell<Vec<(Element, Weak<FocusHandler>)>> = const { RefCell::new(Vec::new()) };
    static OWNER: FocusOwner = FocusOwner::default();
    /// The document subscriptions. Attached at the first registration and kept from
    /// then on.
    static LISTENERS: RefCell<Vec<Listener>> = const { RefCell::new(Vec::new()) };
}

/// The handler held by a widget.
#[derive(Default)]
pub(crate) struct Focus {
    handler: Rc<FocusHandler>,
    registered: Cell<bool>,
}

impl Focus {
    /// Sets the handler. Each call replaces it.
    pub(crate) fn set(&self, element: &Element, f: impl FnMut(bool) + 'static) {
        self.handler.set(f);
        if self.registered.replace(true) {
            return;
        }
        ENTRIES.with(|entries| {
            entries
                .borrow_mut()
                .push((element.clone(), Rc::downgrade(&self.handler)))
        });
        ensure_listeners();
        // フォーカスがすでにこの中にあれば、通知せずに持ち主として覚える。
        // そうしないと、あとで出たときの `false` が届かない。
        let active = web_sys::window()
            .and_then(|window| window.document())
            .and_then(|document| document.active_element());
        if active.is_some_and(|active| element.contains(Some(&active))) {
            OWNER.with(|owner| owner.reset(Some(self.handler.clone())));
        }
    }
}

impl Drop for Focus {
    fn drop(&mut self) {
        if !self.registered.get() {
            return;
        }
        // 外す項目は要素と弱参照だけなので、捨てても表を触り直すことはない。
        // スレッドの後始末で表が先に消えていれば、外すものも無い。
        let _ = ENTRIES.try_with(|entries| {
            entries
                .borrow_mut()
                .retain(|(_, handler)| !std::ptr::eq(handler.as_ptr(), Rc::as_ptr(&self.handler)))
        });
    }
}

/// The number of elements in the registry. **For automated tests only.**
#[doc(hidden)]
pub fn registered_focus_elements() -> usize {
    ENTRIES.with(|entries| entries.borrow().len())
}

fn ensure_listeners() {
    LISTENERS.with(|listeners| {
        if !listeners.borrow().is_empty() {
            return;
        }
        let Some(document) = web_sys::window().and_then(|window| window.document()) else {
            return;
        };
        let focus_in = Listener::attach_reentrant(document.as_ref(), "focusin", |event| {
            let target = event.target().and_then(|t| t.dyn_into::<Node>().ok());
            OWNER.with(|owner| owner.move_to(owner_of(target)));
        });
        let focus_out = Listener::attach_reentrant(document.as_ref(), "focusout", |event| {
            // 移る先があるなら、続く `focusin` が数える。
            let next = event
                .dyn_ref::<FocusEvent>()
                .and_then(FocusEvent::related_target);
            if next.is_some() {
                return;
            }
            // タブやウィンドウごとフォーカスを失ったときは数えない。
            let has_focus = web_sys::window()
                .and_then(|window| window.document())
                .and_then(|document| document.has_focus().ok())
                .unwrap_or(false);
            if has_focus {
                OWNER.with(|owner| owner.move_to(None));
            }
        });
        let mut listeners = listeners.borrow_mut();
        listeners.extend(focus_in.ok());
        listeners.extend(focus_out.ok());
    });
}

/// The registered widget containing `node`.
fn owner_of(node: Option<Node>) -> Option<Rc<FocusHandler>> {
    let mut current = node;
    while let Some(node) = current {
        let found = ENTRIES.with(|entries| {
            entries
                .borrow()
                .iter()
                .find(|(element, _)| AsRef::<Node>::as_ref(element) == &node)
                .and_then(|(_, handler)| handler.upgrade())
        });
        if found.is_some() {
            return found;
        }
        current = node.parent_node();
    }
    None
}

/// Adds `on_focus_change` to text-input widgets. The `Inner` must hold a
/// `focus: Focus`.
macro_rules! impl_focus_change {
    ($t:ty) => {
        impl $t {
            /// Notified when focus enters (`true`) and leaves (`false`).
            ///
            /// Called only when focus moves within the page. Not called when the tab or the
            /// browser window itself loses focus.
            pub fn on_focus_change(&self, f: impl FnMut(bool) + 'static) {
                let element = <$t as crate::widgets::Widget>::native_element(self);
                self.0.focus.set(&element, f);
            }
        }
    };
}

pub(crate) use impl_focus_change;
