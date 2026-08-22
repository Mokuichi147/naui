//! AppKit の target/action とデリゲートを Rust のクロージャへ中継する。
//!
//! Objective-C 側はセレクタしか受け取れないので、クロージャを ivar に持つ
//! 小さなクラスだけを定義し、すべてのコントロールで使い回す。

use std::cell::RefCell;
use std::rc::Rc;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol};
use objc2::{define_class, msg_send, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSControl, NSControlTextEditingDelegate, NSTabView, NSTabViewDelegate, NSTabViewItem,
    NSTextDelegate, NSTextFieldDelegate, NSTextView, NSTextViewDelegate,
};
use objc2_foundation::NSNotification;

type Callback = RefCell<Box<dyn FnMut()>>;

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "NauiActionTarget"]
    #[ivars = Callback]
    pub(crate) struct ActionTarget;

    unsafe impl NSObjectProtocol for ActionTarget {}

    impl ActionTarget {
        #[unsafe(method(invoke:))]
        fn invoke(&self, _sender: Option<&AnyObject>) {
            // クロージャ内で同じコントロールを触っても再入しないよう、
            // borrow は呼び出しの間だけに閉じる。
            let mut cb = self.ivars().borrow_mut();
            (cb)();
        }
    }
);

impl ActionTarget {
    pub(crate) fn new(mtm: MainThreadMarker, f: impl FnMut() + 'static) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(RefCell::new(Box::new(f) as Box<dyn FnMut()>));
        unsafe { msg_send![super(this), init] }
    }
}

type TextCallback = RefCell<Box<dyn FnMut(&str)>>;

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "NauiTextObserver"]
    #[ivars = TextCallback]
    pub(crate) struct TextObserver;

    unsafe impl NSObjectProtocol for TextObserver {}

    unsafe impl NSControlTextEditingDelegate for TextObserver {
        #[unsafe(method(controlTextDidChange:))]
        fn control_text_did_change(&self, notification: &NSNotification) {
            let Some(object) = notification.object() else {
                return;
            };
            let Ok(control) = object.downcast::<NSControl>() else {
                return;
            };
            let text = control.stringValue().to_string();
            let mut cb = self.ivars().borrow_mut();
            (cb)(&text);
        }
    }

    unsafe impl NSTextFieldDelegate for TextObserver {}
);

impl TextObserver {
    pub(crate) fn new(mtm: MainThreadMarker, f: impl FnMut(&str) + 'static) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(RefCell::new(Box::new(f) as Box<dyn FnMut(&str)>));
        unsafe { msg_send![super(this), init] }
    }
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "NauiTextViewObserver"]
    #[ivars = TextCallback]
    pub(crate) struct TextViewObserver;

    unsafe impl NSObjectProtocol for TextViewObserver {}

    // NSTextView は NSControl ではないので、`controlTextDidChange:` は来ない。
    // 複数行の入力は NSText の `textDidChange:` で通知される。
    unsafe impl NSTextDelegate for TextViewObserver {
        #[unsafe(method(textDidChange:))]
        fn text_did_change(&self, notification: &NSNotification) {
            let Some(object) = notification.object() else {
                return;
            };
            let Ok(view) = object.downcast::<NSTextView>() else {
                return;
            };
            let text = view.string().to_string();
            let mut cb = self.ivars().borrow_mut();
            (cb)(&text);
        }
    }

    unsafe impl NSTextViewDelegate for TextViewObserver {}
);

impl TextViewObserver {
    pub(crate) fn new(mtm: MainThreadMarker, f: impl FnMut(&str) + 'static) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(RefCell::new(Box::new(f) as Box<dyn FnMut(&str)>));
        unsafe { msg_send![super(this), init] }
    }
}

/// ナビゲーション系ウィジェットの「選択された」通知先。
///
/// 差し替え可能な 1 本のクロージャを共有で持つ。タブを選んだコールバックの
/// 中からナビバーの選択を変える、といった使い方をしても二重借用にならないよう、
/// 呼び出しの間だけクロージャを取り出す。
#[derive(Clone, Default)]
pub(crate) struct SelectHandler(Rc<RefCell<Option<Box<dyn FnMut(usize)>>>>);

impl SelectHandler {
    pub(crate) fn set(&self, f: impl FnMut(usize) + 'static) {
        *self.0.borrow_mut() = Some(Box::new(f));
    }

    /// 選択を通知する。まだ設定されていなければ何もしない。
    pub(crate) fn emit(&self, index: usize) {
        let Some(mut f) = self.0.borrow_mut().take() else {
            return;
        };
        f(index);
        // 呼び出し中に差し替えられていたら、新しいほうを残す。
        let mut slot = self.0.borrow_mut();
        if slot.is_none() {
            *slot = Some(f);
        }
    }
}

/// 値を 1 つ受け取る通知先。
///
/// [`SelectHandler`] と同じ再入対応を、インデックス以外の値でも使うためのもの
/// (日付ピッカーの `on_change` など)。`SelectHandler` は `define_class!` の
/// ivars に載せる都合で型を固定しているので、別に用意している。
pub(crate) struct ValueHandler<T>(RefCell<Option<Box<dyn FnMut(T)>>>);

impl<T> Default for ValueHandler<T> {
    fn default() -> Self {
        Self(RefCell::new(None))
    }
}

impl<T> ValueHandler<T> {
    pub(crate) fn set(&self, f: impl FnMut(T) + 'static) {
        *self.0.borrow_mut() = Some(Box::new(f));
    }

    /// 通知する。まだ設定されていなければ何もしない。
    pub(crate) fn emit(&self, value: T) {
        let Some(mut f) = self.0.borrow_mut().take() else {
            return;
        };
        f(value);
        // 呼び出し中に差し替えられていたら、新しいほうを残す。
        let mut slot = self.0.borrow_mut();
        if slot.is_none() {
            *slot = Some(f);
        }
    }
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "NauiTabObserver"]
    #[ivars = SelectHandler]
    pub(crate) struct TabObserver;

    unsafe impl NSObjectProtocol for TabObserver {}

    unsafe impl NSTabViewDelegate for TabObserver {
        #[unsafe(method(tabView:didSelectTabViewItem:))]
        fn tab_view_did_select(&self, tab_view: &NSTabView, item: Option<&NSTabViewItem>) {
            let Some(item) = item else {
                return;
            };
            let index = tab_view.indexOfTabViewItem(item);
            if index < 0 {
                return;
            }
            self.ivars().emit(index as usize);
        }
    }
);

impl TabObserver {
    pub(crate) fn new(mtm: MainThreadMarker, handler: SelectHandler) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(handler);
        unsafe { msg_send![super(this), init] }
    }
}
