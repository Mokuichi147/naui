//! `on_focus_change` (文字入力のウィジェット)。
//!
//! 文書に `focusin` と `focusout` を 1 つずつ付け、フォーカスの移った先から
//! 親へたどって、最初に見つかった登録済みのウィジェットをフォーカスの
//! 持ち主とする ([`FocusOwner`])。持ち主が変わったときだけ通知するので、
//! ウィジェットの中で要素から要素へ移っても通知は出ない。
//!
//! naui の Web のウィンドウは同じ文書の中の要素なので、持ち主は文書に 1 つ。
//! **タブやブラウザのウィンドウ自体がフォーカスを失ったときは数えない**
//! (要素の `blur` は出るが、`document.hasFocus()` が偽になる。戻ったときの
//! `focus` は同じ持ち主なので通知しない)。ほかの 3 環境でウィンドウが
//! アクティブでなくなったときと同じ扱いになる。
//!
//! 登録表は要素と、通知先の弱参照を持つ。通知先そのものはウィジェットが
//! 持ち ([`Focus`])、ウィジェットが解放されるときに表の項目も外す。表に
//! 残しておくと、捨てた欄の要素がいつまでも解放されない。
//!
//! 文書の購読は入れ子で呼ばれる。通知の中で別の欄へ `request_focus()` すると、
//! `focus()` が同期で次の `focusout` / `focusin` を出すため
//! ([`Listener::attach_reentrant`])。持ち主の移動は [`FocusOwner`] が順番待ちに
//! するので、内側の移動も取りこぼさない。

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use naui_core::{FocusHandler, FocusOwner};
use wasm_bindgen::JsCast;
use web_sys::{Element, FocusEvent, Node};

use crate::widgets::Listener;

thread_local! {
    static ENTRIES: RefCell<Vec<(Element, Weak<FocusHandler>)>> = const { RefCell::new(Vec::new()) };
    static OWNER: FocusOwner = FocusOwner::default();
    /// 文書の購読。最初の登録のときに付け、そのまま持ち続ける。
    static LISTENERS: RefCell<Vec<Listener>> = const { RefCell::new(Vec::new()) };
}

/// ウィジェットに持たせる通知先。
#[derive(Default)]
pub(crate) struct Focus {
    handler: Rc<FocusHandler>,
    registered: Cell<bool>,
}

impl Focus {
    /// 通知先を置く。呼ぶたびに置き換わる。
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

/// 登録表に載っている要素の数。**自動テスト専用**。
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

/// `node` を含む登録済みのウィジェット。
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

/// 文字入力のウィジェットへ `on_focus_change` を足す。`Inner` に
/// `focus: Focus` を持たせておくこと。
macro_rules! impl_focus_change {
    ($t:ty) => {
        impl $t {
            /// フォーカスが入ったとき (`true`) と出たとき (`false`) の通知。
            ///
            /// ページの中でフォーカスが移ったときだけ呼ばれる。タブや
            /// ブラウザのウィンドウ自体がフォーカスを失っても呼ばれない。
            pub fn on_focus_change(&self, f: impl FnMut(bool) + 'static) {
                let element = <$t as crate::widgets::Widget>::native_element(self);
                self.0.focus.set(&element, f);
            }
        }
    };
}

pub(crate) use impl_focus_change;
