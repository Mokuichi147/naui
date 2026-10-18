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
//! 登録表はウィジェットを弱参照で持つ。ウィジェットが解放されれば
//! 通知先も表から外れる。

use std::cell::RefCell;
use std::rc::Rc;

use gtk::glib;
use gtk::prelude::*;
use naui_core::{FocusHandler, FocusOwner};

type Entries = RefCell<Vec<(glib::WeakRef<gtk::Widget>, Rc<FocusHandler>)>>;

thread_local! {
    static WIDGETS: Entries = const { RefCell::new(Vec::new()) };
    /// ウィンドウごとのフォーカスの持ち主。
    static OWNERS: RefCell<Vec<(glib::WeakRef<gtk::Window>, Rc<FocusOwner>)>> =
        const { RefCell::new(Vec::new()) };
}

/// ウィジェットへ通知先を置く。呼ぶたびに置き換わる。
pub(crate) fn set_widget_handler(widget: &gtk::Widget, f: impl FnMut(bool) + 'static) {
    WIDGETS.with(|entries| {
        let mut entries = entries.borrow_mut();
        entries.retain(|(weak, _)| weak.upgrade().is_some());
        if let Some((_, handler)) = entries
            .iter()
            .find(|(weak, _)| weak.upgrade().as_ref() == Some(widget))
        {
            handler.set(f);
            return;
        }
        let handler = Rc::new(FocusHandler::default());
        handler.set(f);
        entries.push((widget.downgrade(), handler));
    });
    adopt_current_focus(widget);
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
        let mut owners = owners.borrow_mut();
        owners.retain(|(weak, _)| weak.upgrade().is_some());
        owners.push((window.as_ref().downgrade(), owner.clone()));
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
