//! AppKit の target/action とデリゲートを Rust のクロージャへ中継する。
//!
//! Objective-C 側はセレクタしか受け取れないので、クロージャを ivar に持つ
//! 小さなクラスを 2 つだけ定義し、すべてのコントロールで使い回す。

use std::cell::RefCell;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol};
use objc2::{define_class, msg_send, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSControl, NSControlTextEditingDelegate, NSTextFieldDelegate};
use objc2_foundation::NSNotification;

type Callback = RefCell<Box<dyn FnMut()>>;

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "MiuiActionTarget"]
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
    #[name = "MiuiTextObserver"]
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
