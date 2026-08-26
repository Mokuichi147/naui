//! AppKit の target/action とデリゲートを Rust のクロージャへ中継する。
//!
//! Objective-C 側はセレクタしか受け取れないので、クロージャを ivar に持つ
//! 小さなクラスだけを定義し、すべてのコントロールで使い回す。

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

/// 検索欄の 2 つの通知先。
///
/// 打つたびの `on_change` と、Enter で確定したときの `on_search` を 1 つの
/// デリゲートで受けるため、両方をまとめて持つ。呼び出しの間だけクロージャを
/// 取り出すのは [`ValueHandler`] と同じ (通知の中で同じ欄を触っても
/// 二重借用にならない)。
#[derive(Default)]
pub(crate) struct SearchHandlers {
    change: RefCell<Option<Box<dyn FnMut(&str)>>>,
    search: RefCell<Option<Box<dyn FnMut(&str)>>>,
}

impl SearchHandlers {
    pub(crate) fn set_change(&self, f: impl FnMut(&str) + 'static) {
        *self.change.borrow_mut() = Some(Box::new(f));
    }

    pub(crate) fn set_search(&self, f: impl FnMut(&str) + 'static) {
        *self.search.borrow_mut() = Some(Box::new(f));
    }

    fn emit(slot: &RefCell<Option<Box<dyn FnMut(&str)>>>, text: &str) {
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
    #[name = "NauiSearchObserver"]
    #[ivars = Rc<SearchHandlers>]
    pub(crate) struct SearchObserver;

    unsafe impl NSObjectProtocol for SearchObserver {}

    unsafe impl NSControlTextEditingDelegate for SearchObserver {
        #[unsafe(method(controlTextDidChange:))]
        fn control_text_did_change(&self, notification: &NSNotification) {
            let Some(object) = notification.object() else {
                return;
            };
            let Ok(control) = object.downcast::<NSControl>() else {
                return;
            };
            let text = control.stringValue().to_string();
            SearchHandlers::emit(&self.ivars().change, &text);
        }

        // Enter だけを拾うため、NSSearchField の action ではなく編集中の
        // コマンドを見る。action は取り消しボタン (✕) でも飛ぶので、
        // 「確定したとき」という意味からずれる。
        #[unsafe(method(control:textView:doCommandBySelector:))]
        fn do_command_by_selector(
            &self,
            control: &NSControl,
            _text_view: &NSTextView,
            command: Sel,
        ) -> bool {
            if command == sel!(insertNewline:) {
                let text = control.stringValue().to_string();
                SearchHandlers::emit(&self.ivars().search, &text);
            }
            // 既定の動作 (編集の確定) は AppKit へ任せる。
            false
        }
    }

    unsafe impl NSTextFieldDelegate for SearchObserver {}

    unsafe impl NSSearchFieldDelegate for SearchObserver {}
);

impl SearchObserver {
    pub(crate) fn new(mtm: MainThreadMarker, handlers: Rc<SearchHandlers>) -> Retained<Self> {
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

/// コンボボックスの通知を中継するクロージャ。
///
/// 引数は「候補の一覧から選ばれたか」で、`false` は打鍵による変更。
/// 呼び出しの間だけ取り出すのは、通知の中で候補を選び直されても
/// (`selectItemAtIndex:` は選択の通知を出す) 二重借用にならないようにするため。
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

/// 取り出して呼び、差し替えられていなければ戻す ([`SelectHandler::emit`] と同じ形)。
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

/// 文字列を受け取る通知先。
///
/// [`SelectHandler`] と同じ再入対応を `&str` で行う。`ValueHandler<T>` は
/// 借用した値を受け取れない (寿命の引数が要る) ため、別に用意している。
#[derive(Clone, Default)]
pub(crate) struct TextHandler(Rc<RefCell<Option<Box<dyn FnMut(&str)>>>>);

impl TextHandler {
    pub(crate) fn set(&self, f: impl FnMut(&str) + 'static) {
        *self.0.borrow_mut() = Some(Box::new(f));
    }

    /// 通知する。まだ設定されていなければ何もしない。
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
