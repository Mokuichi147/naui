//! Relays AppKit target/action and delegates to Rust closures.
//!
//! The Objective-C side can only receive selectors, so only small classes holding
//! closures in ivars are defined, and they are reused for every control.

use std::cell::RefCell;
use std::rc::Rc;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol, Sel};
use objc2::{define_class, msg_send, sel, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSComboBoxDelegate, NSControl, NSControlTextEditingDelegate, NSSearchFieldDelegate, NSTabView,
    NSTabViewDelegate, NSTabViewItem, NSTextDelegate, NSTextFieldDelegate, NSTextView,
    NSTextViewDelegate,
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

/// The two handlers of a single-line text field.
///
/// `on_change` on every keystroke and the notification on confirming with Enter
/// (`on_submit`, `on_search` for search fields) are received by one delegate, so
/// both are held together. Taking the closure out only for the duration of the call
/// is the same as [`ValueHandler`] (touching the same field inside the notification
/// does not double-borrow).
#[derive(Default)]
pub(crate) struct FieldHandlers {
    change: TextSlot,
    submit: TextSlot,
}

/// The slot for a closure that receives a string. May be empty.
type TextSlot = RefCell<Option<Box<dyn FnMut(&str)>>>;

impl FieldHandlers {
    pub(crate) fn set_change(&self, f: impl FnMut(&str) + 'static) {
        *self.change.borrow_mut() = Some(Box::new(f));
    }

    pub(crate) fn set_submit(&self, f: impl FnMut(&str) + 'static) {
        *self.submit.borrow_mut() = Some(Box::new(f));
    }

    fn emit(slot: &TextSlot, text: &str) {
        let Some(mut f) = slot.borrow_mut().take() else {
            return;
        };
        f(text);
        // 呼び出し中に差し替えられていたら、新しいほうを残す。
        let mut slot = slot.borrow_mut();
        if slot.is_none() {
            *slot = Some(f);
        }
    }
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "NauiFieldObserver"]
    #[ivars = Rc<FieldHandlers>]
    /// The delegate for `NSTextField` (and its subclasses `NSSecureTextField` /
    /// `NSSearchField`).
    pub(crate) struct FieldObserver;

    unsafe impl NSObjectProtocol for FieldObserver {}

    unsafe impl NSControlTextEditingDelegate for FieldObserver {
        #[unsafe(method(controlTextDidChange:))]
        fn control_text_did_change(&self, notification: &NSNotification) {
            let Some(object) = notification.object() else {
                return;
            };
            let Ok(control) = object.downcast::<NSControl>() else {
                return;
            };
            let text = control.stringValue().to_string();
            FieldHandlers::emit(&self.ivars().change, &text);
        }

        // Enter だけを拾うため、コントロールの action ではなく編集中の
        // コマンドを見る。action は欄を離れたときや検索欄の取り消しボタン
        // (✕) でも飛ぶので、「確定したとき」という意味からずれる。
        //
        // IME で変換している間の Enter は IME が受け取るので、ここへは来ない。
        #[unsafe(method(control:textView:doCommandBySelector:))]
        fn do_command_by_selector(
            &self,
            control: &NSControl,
            _text_view: &NSTextView,
            command: Sel,
        ) -> bool {
            if command == sel!(insertNewline:) {
                let text = control.stringValue().to_string();
                FieldHandlers::emit(&self.ivars().submit, &text);
            }
            // 既定の動作 (編集の確定) は AppKit へ任せる。
            false
        }
    }

    unsafe impl NSTextFieldDelegate for FieldObserver {}

    unsafe impl NSSearchFieldDelegate for FieldObserver {}
);

impl FieldObserver {
    pub(crate) fn new(mtm: MainThreadMarker, handlers: Rc<FieldHandlers>) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(handlers);
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

/// The closure that relays combo box notifications.
///
/// The argument is "whether it was chosen from the suggestion list"; `false` means a
/// change by typing. It is taken out only for the duration of the call so that
/// reselecting a suggestion inside the notification (`selectItemAtIndex:` sends a
/// selection notification) does not double-borrow.
type ComboCallback = RefCell<Option<Box<dyn FnMut(bool)>>>;

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "NauiComboObserver"]
    #[ivars = ComboCallback]
    pub(crate) struct ComboObserver;

    unsafe impl NSObjectProtocol for ComboObserver {}

    unsafe impl NSControlTextEditingDelegate for ComboObserver {
        #[unsafe(method(controlTextDidChange:))]
        fn control_text_did_change(&self, _notification: &NSNotification) {
            invoke_combo(self.ivars(), false);
        }
    }

    unsafe impl NSTextFieldDelegate for ComboObserver {}

    unsafe impl NSComboBoxDelegate for ComboObserver {
        #[unsafe(method(comboBoxSelectionDidChange:))]
        fn combo_box_selection_did_change(&self, _notification: &NSNotification) {
            invoke_combo(self.ivars(), true);
        }
    }
);

/// Takes it out, calls it, and puts it back unless it was replaced (the same shape
/// as [`SelectHandler::emit`]).
fn invoke_combo(slot: &ComboCallback, from_list: bool) {
    let Some(mut f) = slot.borrow_mut().take() else {
        return;
    };
    f(from_list);
    let mut slot = slot.borrow_mut();
    if slot.is_none() {
        *slot = Some(f);
    }
}

impl ComboObserver {
    pub(crate) fn new(mtm: MainThreadMarker, f: impl FnMut(bool) + 'static) -> Retained<Self> {
        let this =
            Self::alloc(mtm).set_ivars(RefCell::new(Some(Box::new(f) as Box<dyn FnMut(bool)>)));
        unsafe { msg_send![super(this), init] }
    }
}

/// A handler that receives a string.
///
/// The same re-entrancy handling as [`SelectHandler`], with `&str`. `ValueHandler<T>`
/// cannot receive borrowed values (it would need a lifetime parameter), so this is
/// provided separately.
#[derive(Clone, Default)]
pub(crate) struct TextHandler(Rc<RefCell<Option<Box<dyn FnMut(&str)>>>>);

impl TextHandler {
    pub(crate) fn set(&self, f: impl FnMut(&str) + 'static) {
        *self.0.borrow_mut() = Some(Box::new(f));
    }

    /// Notifies. Does nothing if not set yet.
    pub(crate) fn emit(&self, text: &str) {
        let Some(mut f) = self.0.borrow_mut().take() else {
            return;
        };
        f(text);
        // 呼び出し中に差し替えられていたら、新しいほうを残す。
        let mut slot = self.0.borrow_mut();
        if slot.is_none() {
            *slot = Some(f);
        }
    }
}

/// The "selected" handler of navigation widgets.
///
/// It shares a single replaceable closure. So that usage such as changing the nav
/// bar's selection from inside a tab selection callback does not double-borrow, the
/// closure is taken out only for the duration of the call.
#[derive(Clone, Default)]
pub(crate) struct SelectHandler(Rc<RefCell<Option<Box<dyn FnMut(usize)>>>>);

impl SelectHandler {
    pub(crate) fn set(&self, f: impl FnMut(usize) + 'static) {
        *self.0.borrow_mut() = Some(Box::new(f));
    }

    /// Whether a closure is set.
    pub(crate) fn is_set(&self) -> bool {
        self.0.borrow().is_some()
    }

    /// Notifies the selection. Does nothing if not set yet.
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

/// A handler that receives one value.
///
/// For using the same re-entrancy handling as [`SelectHandler`] with values other
/// than indices (the date picker's `on_change`, for example). `SelectHandler` has a
/// fixed type because it is placed in the ivars of `define_class!`, so this is
/// provided separately.
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

    /// Whether a closure is set.
    pub(crate) fn is_set(&self) -> bool {
        self.0.borrow().is_some()
    }

    /// Notifies. Does nothing if not set yet.
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
