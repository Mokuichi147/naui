//! `on_focus_change` (文字入力のウィジェット)。
//!
//! AppKit にはフォーカスが移ったことを知らせる通知が無い。フォーカスの移動は
//! どれもウィンドウの `makeFirstResponder:` を通るので、naui のウィンドウを
//! それを上書きした `NSWindow` の派生クラスで作り、移ったあとの first
//! responder から親のビューへたどって、最初に見つかった登録済みのウィジェットを
//! そのウィンドウのフォーカスの持ち主とする ([`FocusOwner`])。
//!
//! 入力欄の編集はフィールドエディタ (共有の `NSTextView`) が担うが、
//! フィールドエディタは編集中の `NSTextField` の中に置かれるので、親を
//! たどれば入力欄に着く (`on_key_down` と同じ)。
//!
//! 通知先はビューの関連オブジェクト ([`HandlerBox`]) として持たせ、登録表は
//! 置かない。ビューが解放されれば関連オブジェクトも解放され、通知先と
//! それが捕まえている状態も一緒に捨てられる (表に持たせると、次に
//! 登録されるまで残ってしまう)。

use std::cell::Cell;
use std::ffi::c_void;
use std::rc::Rc;

use naui_core::{FocusHandler, FocusOwner};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObjectProtocol};
use objc2::{define_class, ffi, msg_send, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSBackingStoreType, NSResponder, NSView, NSWindow, NSWindowStyleMask};
use objc2_foundation::{NSObject, NSRect};

/// 関連オブジェクトの鍵。中身は使わず、番地だけを使う。
static HANDLER_KEY: u8 = 0;

define_class!(
    /// ビューに関連オブジェクトとして付け、通知先をビューと同じ寿命にする。
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "NauiFocusHandlerBox"]
    #[ivars = Rc<FocusHandler>]
    struct HandlerBox;

    unsafe impl NSObjectProtocol for HandlerBox {}
);

/// ウィジェットのビューへ通知先を置く。呼ぶたびに置き換わる。
pub(crate) fn set_view_handler(view: &NSView, f: impl FnMut(bool) + 'static) {
    let handler = lookup(view).unwrap_or_else(|| {
        let mtm = MainThreadMarker::from(view);
        let handler = Rc::new(FocusHandler::default());
        let holder = HandlerBox::alloc(mtm).set_ivars(handler.clone());
        let holder: Retained<HandlerBox> = unsafe { msg_send![super(holder), init] };
        // 関連オブジェクトは、ビューの解放のときに手放される。
        unsafe {
            ffi::objc_setAssociatedObject(
                view as *const NSView as *mut AnyObject,
                handler_key(),
                Retained::as_ptr(&holder) as *mut AnyObject,
                ffi::OBJC_ASSOCIATION_RETAIN_NONATOMIC,
            );
        }
        handler
    });
    handler.set(f);
    adopt_current_focus(view);
}

fn handler_key() -> *const c_void {
    std::ptr::addr_of!(HANDLER_KEY).cast()
}

/// フォーカスがすでに `view` の中にあれば、通知せずに持ち主として覚える
/// (ウィンドウを出したときに最初の入力欄へ移るなど)。そうしないと、あとで
/// 出たときの `false` が届かない。
fn adopt_current_focus(view: &NSView) {
    let Some(window) = view.window() else {
        return;
    };
    let Ok(window) = window.downcast::<FocusWindow>() else {
        return;
    };
    let mine = lookup(view);
    let current = owner_of(window.firstResponder());
    if let (Some(mine), Some(current)) = (&mine, &current) {
        if Rc::ptr_eq(mine, current) {
            window.ivars().owner.reset(Some(current.clone()));
        }
    }
}

/// `view` に置いてある通知先。
fn lookup(view: &NSView) -> Option<Rc<FocusHandler>> {
    let holder = unsafe {
        ffi::objc_getAssociatedObject(view as *const NSView as *const AnyObject, handler_key())
    };
    // この鍵で付けるのは `set_view_handler` だけで、付けるのは必ず `HandlerBox`。
    let holder = unsafe { holder.cast::<HandlerBox>().as_ref() }?;
    Some(holder.ivars().clone())
}

/// `responder` を含む登録済みのウィジェット。ビューでなければ (ウィンドウ自身
/// など) どれにも含まれない。
fn owner_of(responder: Option<Retained<NSResponder>>) -> Option<Rc<FocusHandler>> {
    let mut view = responder.and_then(|responder| responder.downcast::<NSView>().ok());
    while let Some(current) = view {
        if let Some(handler) = lookup(&current) {
            return Some(handler);
        }
        view = unsafe { current.superview() };
    }
    None
}

#[derive(Default)]
struct FocusState {
    owner: FocusOwner,
    /// 入れ子になっている `makeFirstResponder:` の深さ。
    depth: Cell<u32>,
}

define_class!(
    /// フォーカスの移動を見張る `NSWindow`。
    #[unsafe(super(NSWindow, NSResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "NauiWindow"]
    #[ivars = FocusState]
    struct FocusWindow;

    unsafe impl NSObjectProtocol for FocusWindow {}

    impl FocusWindow {
        /// 入力欄へ移すときは、AppKit がこの中からさらに
        /// `makeFirstResponder:` を呼ぶ (編集中の欄を閉じるためにいったん
        /// ウィンドウ自身へ移し、続けてフィールドエディタへ移す)。途中の
        /// 状態は数えず、いちばん外側の呼び出しが終わったところで数える。
        #[unsafe(method(makeFirstResponder:))]
        fn make_first_responder(&self, responder: Option<&NSResponder>) -> bool {
            let state = self.ivars();
            state.depth.set(state.depth.get() + 1);
            let moved: bool = unsafe { msg_send![super(self), makeFirstResponder: responder] };
            state.depth.set(state.depth.get() - 1);
            if state.depth.get() == 0 {
                // 移せなかったときも、入れ子の移動で持ち主が変わっていることが
                // あるので、いまの first responder から数え直す。
                state.owner.move_to(owner_of(self.firstResponder()));
            }
            moved
        }
    }
);

/// フォーカスの移動を見張るウィンドウを作る。
pub(crate) fn new_window(
    mtm: MainThreadMarker,
    frame: NSRect,
    style: NSWindowStyleMask,
) -> Retained<NSWindow> {
    let this = FocusWindow::alloc(mtm).set_ivars(FocusState::default());
    let this: Retained<FocusWindow> = unsafe {
        msg_send![
            super(this),
            initWithContentRect: frame,
            styleMask: style,
            backing: NSBackingStoreType::Buffered,
            defer: false,
        ]
    };
    Retained::into_super(this)
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
                let view = <$t as crate::widgets::Widget>::native_view(self);
                crate::focus::set_view_handler(&view, f);
            }
        }
    };
}

pub(crate) use impl_focus_change;
