//! `on_key_down` (文字入力のウィジェットと `Window`)。
//!
//! アプリ全体で `NSEvent` のローカルモニターを 1 つ置き、`keyDown` を
//! **AppKit が配送する前に**受け取る。キーが来たウィンドウの first responder
//! から親のビューへたどり、最初に見つかった登録済みのウィジェット、続けて
//! ウィンドウの順に通知する。`Handled` ならモニターがイベントを捨てるので、
//! 入力欄 (改行の挿入など) にもメニューのショートカットにも届かない。
//!
//! 入力欄の編集はフィールドエディタ (共有の `NSTextView`) が担うが、
//! フィールドエディタは編集中の `NSTextField` の中に置かれるので、親を
//! たどれば入力欄に着く。
//!
//! 変換中 (first responder が `hasMarkedText`) のキーは IME のものなので
//! 届けない。
//!
//! 登録表はビューとウィンドウを弱参照で持つ。ウィジェットのハンドルが
//! すべて落ちてビューが解放されれば、通知先も表から外れる。

use std::cell::RefCell;
use std::ptr::NonNull;
use std::rc::Rc;

use block2::RcBlock;
use naui_core::{EventResponse, Key, KeyEvent, KeyHandler, Modifiers};
use objc2::rc::{Retained, Weak};
use objc2::runtime::{AnyObject, NSObjectProtocol};
use objc2::{msg_send, sel, MainThreadMarker};
use objc2_app_kit::{NSEvent, NSEventMask, NSEventModifierFlags, NSResponder, NSView, NSWindow};

type Entries<T> = RefCell<Vec<(Weak<T>, Rc<KeyHandler>)>>;

thread_local! {
    static VIEWS: Entries<NSView> = const { RefCell::new(Vec::new()) };
    static WINDOWS: Entries<NSWindow> = const { RefCell::new(Vec::new()) };
    /// モニターの戻り値。持っている間だけモニターが生きる。
    static MONITOR: RefCell<Option<Retained<AnyObject>>> = const { RefCell::new(None) };
}

/// ウィジェットのビューへ通知先を置く。呼ぶたびに置き換わる。
pub(crate) fn set_view_handler(view: &NSView, f: impl FnMut(&KeyEvent) -> EventResponse + 'static) {
    VIEWS.with(|views| handler_for(&mut views.borrow_mut(), view).set(f));
    ensure_monitor();
}

/// ウィンドウへ通知先を置く。呼ぶたびに置き換わる。
pub(crate) fn set_window_handler(
    window: &NSWindow,
    f: impl FnMut(&KeyEvent) -> EventResponse + 'static,
) {
    WINDOWS.with(|windows| handler_for(&mut windows.borrow_mut(), window).set(f));
    ensure_monitor();
}

/// `target` の通知先。無ければ作る。消えたものの分もここで片づける。
fn handler_for<T: objc2::Message>(
    entries: &mut Vec<(Weak<T>, Rc<KeyHandler>)>,
    target: &T,
) -> Rc<KeyHandler> {
    entries.retain(|(weak, _)| weak.load().is_some());
    if let Some((_, handler)) = entries.iter().find(|(weak, _)| {
        weak.load()
            .is_some_and(|other| std::ptr::eq(&*other, target))
    }) {
        return handler.clone();
    }
    let handler = Rc::new(KeyHandler::default());
    entries.push((Weak::from(target), handler.clone()));
    handler
}

fn lookup<T: objc2::Message>(
    entries: &'static std::thread::LocalKey<Entries<T>>,
    target: &T,
) -> Option<Rc<KeyHandler>> {
    entries.with(|entries| {
        entries
            .borrow()
            .iter()
            .find(|(weak, _)| {
                weak.load()
                    .is_some_and(|other| std::ptr::eq(&*other, target))
            })
            .map(|(_, handler)| handler.clone())
    })
}

fn ensure_monitor() {
    MONITOR.with(|monitor| {
        if monitor.borrow().is_some() {
            return;
        }
        let block = RcBlock::new(|event: NonNull<NSEvent>| -> *mut NSEvent {
            // SAFETY: AppKit が渡す、生きている NSEvent。
            let event_ref = unsafe { event.as_ref() };
            match dispatch(event_ref) {
                EventResponse::Handled => std::ptr::null_mut(),
                EventResponse::Continue => event.as_ptr(),
            }
        });
        let token = unsafe {
            NSEvent::addLocalMonitorForEventsMatchingMask_handler(NSEventMask::KeyDown, &block)
        };
        *monitor.borrow_mut() = token;
    });
}

/// 届いたキーを、フォーカスのあるウィジェット → ウィンドウの順に渡す。
pub(crate) fn dispatch(event: &NSEvent) -> EventResponse {
    // モニターはメインスレッドで呼ばれる。
    let Some(mtm) = MainThreadMarker::new() else {
        return EventResponse::Continue;
    };
    let Some(window) = event.window(mtm) else {
        return EventResponse::Continue;
    };
    let responder = window.firstResponder();
    if responder.as_deref().is_some_and(is_composing) {
        return EventResponse::Continue;
    }
    let key_event = to_key_event(event);

    let mut view = responder.and_then(|responder| responder.downcast::<NSView>().ok());
    while let Some(current) = view {
        if let Some(handler) = lookup(&VIEWS, &current) {
            if handler.emit(&key_event) == EventResponse::Handled {
                return EventResponse::Handled;
            }
            break;
        }
        view = unsafe { current.superview() };
    }
    lookup(&WINDOWS, &window).map_or(EventResponse::Continue, |handler| handler.emit(&key_event))
}

/// IME で変換している最中か (`NSTextInputClient` の `hasMarkedText`)。
fn is_composing(responder: &NSResponder) -> bool {
    if !responder.respondsToSelector(sel!(hasMarkedText)) {
        return false;
    }
    unsafe { msg_send![responder, hasMarkedText] }
}

/// AppKit のキーを naui のキーへ写す。
fn to_key_event(event: &NSEvent) -> KeyEvent {
    let flags = event.modifierFlags();
    let modifiers = Modifiers {
        shift: flags.contains(NSEventModifierFlags::Shift),
        control: flags.contains(NSEventModifierFlags::Control),
        alt: flags.contains(NSEventModifierFlags::Option),
        meta: flags.contains(NSEventModifierFlags::Command),
    };
    KeyEvent::new(key_of(event), modifiers).repeat(event.isARepeat())
}

/// 仮想キーコード (`kVK_*`、配列に依らない) で名前の付いたキーを、それ
/// 以外は修飾キーを除いた文字で読む。
fn key_of(event: &NSEvent) -> Key {
    match event.keyCode() {
        36 | 76 => return Key::Enter, // Return / テンキーの Enter
        53 => return Key::Escape,
        48 => return Key::Tab,
        51 => return Key::Backspace,
        117 => return Key::Delete,
        49 => return Key::Space,
        126 => return Key::ArrowUp,
        125 => return Key::ArrowDown,
        123 => return Key::ArrowLeft,
        124 => return Key::ArrowRight,
        115 => return Key::Home,
        119 => return Key::End,
        116 => return Key::PageUp,
        121 => return Key::PageDown,
        code => {
            if let Some(number) = function_key(code) {
                return Key::F(number);
            }
        }
    }
    event
        .charactersIgnoringModifiers()
        .and_then(|text| text.to_string().chars().next())
        .map_or(Key::Other, Key::character)
}

fn function_key(code: u16) -> Option<u8> {
    const CODES: [u16; 12] = [122, 120, 99, 118, 96, 97, 98, 100, 101, 109, 103, 111];
    CODES
        .iter()
        .position(|&c| c == code)
        .map(|index| index as u8 + 1)
}

/// 文字入力のウィジェットへ `on_key_down` を足す。
macro_rules! impl_key_down {
    ($t:ty) => {
        impl $t {
            /// キーが押されたときの通知。`Handled` を返すと、そのキーは
            /// 入力欄にもウィンドウにも渡らない (改行や確定を止められる)。
            ///
            /// IME で変換している間のキーは届かない。
            pub fn on_key_down(
                &self,
                f: impl FnMut(&naui_core::KeyEvent) -> naui_core::EventResponse + 'static,
            ) {
                let view = <$t as crate::widgets::Widget>::native_view(self);
                crate::keys::set_view_handler(&view, f);
            }
        }
    };
}

pub(crate) use impl_key_down;
