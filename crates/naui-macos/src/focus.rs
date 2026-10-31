//! `on_focus_change` (text-input widgets).
//!
//! AppKit has no notification that focus moved. Every focus move goes through the
//! window's `makeFirstResponder:`, so naui's windows are created from an `NSWindow`
//! subclass that overrides it; after the move, it walks up the parent views from the
//! first responder, and the first registered widget found becomes that window's
//! focus owner ([`FocusOwner`]).
//!
//! Editing in text fields is done by the field editor (a shared `NSTextView`), but
//! the field editor is placed inside the `NSTextField` being edited, so walking up
//! the parents reaches the field (the same as `on_key_down`).
//!
//! The handler is held as an associated object of the view ([`HandlerBox`]), and
//! there is no registry. When the view is freed, the associated object is freed
//! too, and the handler and the state it captures are discarded together (a table
//! would keep them until the next registration).

use std::cell::Cell;
use std::ffi::c_void;
use std::rc::Rc;

use naui_core::{FocusHandler, FocusOwner};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObjectProtocol};
use objc2::{define_class, ffi, msg_send, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSBackingStoreType, NSResponder, NSView, NSWindow, NSWindowStyleMask};
use objc2_foundation::{NSObject, NSRect};

/// The associated object's key. Its contents are unused; only its address is.
static HANDLER_KEY: u8 = 0;

define_class!(
    /// Attached to the view as an associated object, giving the handler the same
    /// lifetime as the view.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "NauiFocusHandlerBox"]
    #[ivars = Rc<FocusHandler>]
    struct HandlerBox;

    unsafe impl NSObjectProtocol for HandlerBox {}
);

/// Sets the handler on the widget's view. Each call replaces it.
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

/// If focus is already inside `view`, records it as the owner without notifying
/// (such as focus moving to the first field when the window is shown). Otherwise
/// the `false` for leaving later would never arrive.
fn adopt_current_focus(view: &NSView) {
    let Some(window) = view.window() else {
        return;
    };
    let mine = lookup(view);
    let current = owner_of(window.firstResponder());
    if let (Some(mine), Some(current)) = (&mine, &current) {
        if Rc::ptr_eq(mine, current) {
            if let Ok(window) = window.clone().downcast::<FocusWindow>() {
                window.ivars().owner.reset(Some(current.clone()));
            } else if let Ok(sheet) = window.downcast::<crate::sheet::SheetWindow>() {
                sheet.focus_state().owner.reset(Some(current.clone()));
            }
        }
    }
}

/// The handler set on `view`.
fn lookup(view: &NSView) -> Option<Rc<FocusHandler>> {
    let holder = unsafe {
        ffi::objc_getAssociatedObject(view as *const NSView as *const AnyObject, handler_key())
    };
    // この鍵で付けるのは `set_view_handler` だけで、付けるのは必ず `HandlerBox`。
    let holder = unsafe { holder.cast::<HandlerBox>().as_ref() }?;
    Some(holder.ivars().clone())
}

/// The registered widget containing `responder`. If it is not a view (the window
/// itself, for example), it is contained in none.
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
pub(crate) struct FocusState {
    owner: FocusOwner,
    /// The depth of nested `makeFirstResponder:` calls.
    depth: Cell<u32>,
}

impl FocusState {
    /// The body of an overridden `makeFirstResponder:`. `call_super` makes the move
    /// itself, and `window` is the window it happens in.
    ///
    /// When moving into a text field, AppKit calls `makeFirstResponder:` again from
    /// inside this one (to end editing in the current field it first moves to the
    /// window itself, then to the field editor). Intermediate states are not counted;
    /// counting happens when the outermost call finishes.
    pub(crate) fn track_move(&self, window: &NSWindow, call_super: impl FnOnce() -> bool) -> bool {
        self.depth.set(self.depth.get() + 1);
        let moved = call_super();
        self.depth.set(self.depth.get() - 1);
        if self.depth.get() == 0 {
            // 移せなかったときも、入れ子の移動で持ち主が変わっていることが
            // あるので、いまの first responder から数え直す。
            self.owner.move_to(owner_of(window.firstResponder()));
        }
        moved
    }
}

define_class!(
    /// An `NSWindow` that watches focus moves.
    #[unsafe(super(NSWindow, NSResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "NauiWindow"]
    #[ivars = FocusState]
    struct FocusWindow;

    unsafe impl NSObjectProtocol for FocusWindow {}

    impl FocusWindow {
        #[unsafe(method(makeFirstResponder:))]
        fn make_first_responder(&self, responder: Option<&NSResponder>) -> bool {
            self.ivars().track_move(self, || unsafe {
                msg_send![super(self), makeFirstResponder: responder]
            })
        }
    }
);

/// Creates a window that watches focus moves.
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

/// Adds `on_focus_change` to text-input widgets.
macro_rules! impl_focus_change {
    ($t:ty) => {
        impl $t {
            /// Notified when focus enters (`true`) and leaves (`false`).
            ///
            /// Called only when focus moves within the window. Not called when the window
            /// itself becomes inactive.
            pub fn on_focus_change(&self, f: impl FnMut(bool) + 'static) {
                let view = <$t as crate::widgets::Widget>::native_view(self);
                crate::focus::set_view_handler(&view, f);
            }
        }
    };
}

pub(crate) use impl_focus_change;
