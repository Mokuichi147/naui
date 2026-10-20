//! `on_focus_change` (文字入力のウィジェット)。
//!
//! ウィンドウごとに `focus-widget` の変化を購読し、フォーカスのある
//! ウィジェットから親へたどって、最初に見つかった登録済みのウィジェットを
//! そのウィンドウのフォーカスの持ち主とする ([`FocusOwner`])。持ち主が
//! 変わったときだけ通知するので、入力欄の中の `GtkText` へ移るときのように
//! ウィジェットの中で移っても通知は出ない。
//!
//! `GtkEventControllerFocus` はウィンドウがアクティブでなくなったときにも
//! 出入りを知らせるが、`focus-widget` はウィンドウの中のフォーカスなので
//! 変わらない。ほかの環境とそろえるため、こちらを使う。
//!
//! 登録表はウィジェットを弱参照で持ち、ウィジェット (ウィンドウ) が
//! 解放されたときに通知先を表から外す。表に残しておくと、通知先と
//! それが捕まえている状態がいつまでも解放されない。

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use gtk::glib;
use gtk::prelude::*;
use naui_core::{FocusHandler, FocusOwner};

/// 弱参照の GObject と、それに結び付けた値の表。
type Table<K, V> = RefCell<Vec<(glib::WeakRef<K>, Rc<V>)>>;

thread_local! {
    static WIDGETS: Table<gtk::Widget, FocusHandler> = const { RefCell::new(Vec::new()) };
    /// ウィンドウごとのフォーカスの持ち主。
    static OWNERS: Table<gtk::Window, FocusOwner> = const { RefCell::new(Vec::new()) };
}

/// ウィジェットへ通知先を置く。呼ぶたびに置き換わる。
pub(crate) fn set_widget_handler(widget: &gtk::Widget, f: impl FnMut(bool) + 'static) {
    let handler = lookup(widget).unwrap_or_else(|| {
        let handler = Rc::new(FocusHandler::default());
        WIDGETS.with(|entries| {
            entries
                .borrow_mut()
                .push((widget.downgrade(), handler.clone()))
        });
        let weak = Rc::downgrade(&handler);
        widget.add_weak_ref_notify_local(move || {
            drop(remove_entry(&WIDGETS, &weak));
        });
        handler
    });
    // 前のクロージャを捨てるとき、捕まえていたウィジェットが解放されて
    // 表を触ることがある。表を借りていない所で置き換える。
    handler.set(f);
    adopt_current_focus(widget);
}

/// 値が `value` の項目を表から外して返す。返したものは表を借りていない所で
/// 捨てる (捨てると別のウィジェットが解放され、また表を触ることがある)。
///
/// GObject が解放されるときに呼ぶ。そのときにはもう弱参照は戻せないことが
/// あるので、値のほうで探す。
fn remove_entry<K: ObjectType, V: 'static>(
    table: &'static std::thread::LocalKey<Table<K, V>>,
    value: &Weak<V>,
) -> Option<(glib::WeakRef<K>, Rc<V>)> {
    // スレッドの後始末で表が先に消えていれば、外すものも無い。
    table
        .try_with(|entries| {
            let mut entries = entries.borrow_mut();
            let index = entries
                .iter()
                .position(|(_, v)| std::ptr::eq(Rc::as_ptr(v), value.as_ptr()))?;
            Some(entries.swap_remove(index))
        })
        .ok()
        .flatten()
}

/// フォーカスがすでに `widget` の中にあれば、通知せずに持ち主として覚える
/// (ウィンドウを出したときに最初の入力欄へ移るなど)。そうしないと、あとで
/// 出たときの `false` が届かない。
fn adopt_current_focus(widget: &gtk::Widget) {
    let Some(window) = widget.root().and_downcast::<gtk::Window>() else {
        return;
    };
    let Some(owner) = owner_for(&window) else {
        return;
    };
    let current = owner_of(GtkWindowExt::focus(&window));
    if current.is_some_and(|current| lookup(widget).is_some_and(|mine| Rc::ptr_eq(&current, &mine)))
    {
        owner.reset(lookup(widget));
    }
}

fn owner_for(window: &gtk::Window) -> Option<Rc<FocusOwner>> {
    OWNERS.with(|owners| {
        owners
            .borrow()
            .iter()
            .find(|(weak, _)| weak.upgrade().as_ref() == Some(window))
            .map(|(_, owner)| owner.clone())
    })
}

fn lookup(widget: &gtk::Widget) -> Option<Rc<FocusHandler>> {
    WIDGETS.with(|entries| {
        entries
            .borrow()
            .iter()
            .find(|(weak, _)| weak.upgrade().as_ref() == Some(widget))
            .map(|(_, handler)| handler.clone())
    })
}

/// `widget` を含む登録済みのウィジェット。
fn owner_of(widget: Option<gtk::Widget>) -> Option<Rc<FocusHandler>> {
    let mut current = widget;
    while let Some(widget) = current {
        if let Some(handler) = lookup(&widget) {
            return Some(handler);
        }
        current = widget.parent();
    }
    None
}

/// ウィンドウにフォーカスの見張りを付ける。ウィンドウを作るときに 1 度だけ呼ぶ。
pub(crate) fn install(window: &impl IsA<gtk::Window>) {
    let owner = Rc::new(FocusOwner::default());
    OWNERS.with(|owners| {
        owners
            .borrow_mut()
            .push((window.as_ref().downgrade(), owner.clone()))
    });
    let weak = Rc::downgrade(&owner);
    window.as_ref().add_weak_ref_notify_local(move || {
        drop(remove_entry(&OWNERS, &weak));
    });
    window.as_ref().connect_focus_widget_notify(move |window| {
        owner.move_to(owner_of(GtkWindowExt::focus(window)))
    });
}

/// 文字入力のウィジェットへ `on_focus_change` を足す。
macro_rules! impl_focus_change {
    ($t:ty) => {
        impl $t {
            /// フォーカスが入ったとき (`true`) と出たとき (`false`) の通知。
            ///
            /// ウィンドウの中でフォーカスが移ったときだけ呼ばれる。ウィンドウ
            /// 自体がアクティブでなくなっても呼ばれない。
            pub fn on_focus_change(&self, f: impl FnMut(bool) + 'static) {
                let widget = <$t as crate::widgets::Widget>::native_widget(self);
                crate::focus::set_widget_handler(&widget, f);
            }
        }
    };
}

pub(crate) use impl_focus_change;
