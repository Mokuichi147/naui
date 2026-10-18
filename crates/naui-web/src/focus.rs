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
//! 持つので、ウィジェットが解放されれば表からも外れる。

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
            let mut entries = entries.borrow_mut();
            entries.retain(|(_, handler)| handler.strong_count() > 0);
            entries.push((element.clone(), Rc::downgrade(&self.handler)));
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

fn ensure_listeners() {
    LISTENERS.with(|listeners| {
        if !listeners.borrow().is_empty() {
            return;
        }
        let Some(document) = web_sys::window().and_then(|window| window.document()) else {
            return;
        };
        let focus_in = Listener::attach_event(document.as_ref(), "focusin", |event| {
            let target = event.target().and_then(|t| t.dyn_into::<Node>().ok());
            OWNER.with(|owner| owner.move_to(owner_of(target)));
        });
        let focus_out = Listener::attach_event(document.as_ref(), "focusout", |event| {
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
