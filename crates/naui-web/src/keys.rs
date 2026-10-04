//! `on_key_down` (文字入力のウィジェットと `Window`)。
//!
//! ウィジェットは自分の要素に**捕捉フェーズ**で `keydown` を付ける。対象の
//! 要素の上では捕捉の購読が通常の購読より先に呼ばれるので、naui 自身の
//! `keydown` (検索入力の Enter など) より前にアプリが判断できる。
//! `Handled` なら既定動作を止め、ほかの購読へも親へも伝えない。
//!
//! ウィンドウは `document` で受け、キーが上がってきた元のウィンドウ
//! (どのウィンドウにも属さないところからなら、どれでも) だけが受け取る
//! (メニューバーのショートカットと同じ判定)。
//!
//! 変換中のキー (`isComposing`、Safari が変換中に付ける `keyCode` 229) は
//! IME のものなので届けない。

use std::cell::RefCell;
use std::rc::Rc;

use naui_core::{EventResponse, Key, KeyEvent, KeyHandler, Modifiers, Result};
use wasm_bindgen::JsCast;
use web_sys::{Element, EventTarget, KeyboardEvent};

use crate::widgets::Listener;

/// IME が処理しているキーに付く `keyCode`。
const IME_PROCESS_KEY_CODE: u32 = 229;

/// `on_key_down` の通知先と、その購読。購読は最初に通知先を置いたときに付ける。
#[derive(Default)]
pub(crate) struct KeyDown {
    handler: Rc<KeyHandler>,
    listener: RefCell<Option<Listener>>,
}

impl KeyDown {
    /// ウィジェットの要素で受ける。
    pub(crate) fn set_on_element(
        &self,
        target: &EventTarget,
        f: impl FnMut(&KeyEvent) -> EventResponse + 'static,
    ) {
        self.handler.set(f);
        if self.listener.borrow().is_some() {
            return;
        }
        let handler = self.handler.clone();
        let listener = Listener::attach_capture(target, "keydown", move |event| {
            let Some(key) = event.dyn_ref::<KeyboardEvent>() else {
                return;
            };
            if let Some(key_event) = to_key_event(key) {
                if handler.emit(&key_event) == EventResponse::Handled {
                    event.prevent_default();
                    event.stop_immediate_propagation();
                }
            }
        });
        *self.listener.borrow_mut() = listener.ok();
    }

    /// ウィンドウとして `document` で受ける。`window` はそのウィンドウの要素。
    pub(crate) fn set_on_window(
        &self,
        document: &EventTarget,
        window: &Element,
        f: impl FnMut(&KeyEvent) -> EventResponse + 'static,
    ) -> Result<()> {
        self.handler.set(f);
        if self.listener.borrow().is_some() {
            return Ok(());
        }
        let handler = self.handler.clone();
        let window = window.clone();
        let listener = Listener::attach_event(document, "keydown", move |event| {
            if !comes_from(&event, &window) {
                return;
            }
            let Some(key) = event.dyn_ref::<KeyboardEvent>() else {
                return;
            };
            if let Some(key_event) = to_key_event(key) {
                if handler.emit(&key_event) == EventResponse::Handled {
                    event.prevent_default();
                    event.stop_immediate_propagation();
                }
            }
        })?;
        *self.listener.borrow_mut() = Some(listener);
        Ok(())
    }
}

/// キーが `window` から上がってきたか。どのウィンドウにも属さないところ
/// (`<body>` にフォーカスがあるときなど) からのキーは、まだ誰も既定動作を
/// 止めていなければ受け取る。
fn comes_from(event: &web_sys::Event, window: &Element) -> bool {
    if !window.is_connected() {
        return false;
    }
    let owner = event
        .target()
        .and_then(|target| target.dyn_into::<Element>().ok())
        .and_then(|element| {
            element
                .closest(crate::window::WINDOW_SELECTOR)
                .ok()
                .flatten()
        });
    match owner {
        Some(owner) => &owner == window,
        None => !event.default_prevented(),
    }
}

/// ブラウザのキーを naui のキーへ写す。変換中のキーは `None`。
fn to_key_event(event: &KeyboardEvent) -> Option<KeyEvent> {
    if event.is_composing() || event.key_code() == IME_PROCESS_KEY_CODE {
        return None;
    }
    let modifiers = Modifiers {
        shift: event.shift_key(),
        control: event.ctrl_key(),
        alt: event.alt_key(),
        meta: event.meta_key(),
    };
    Some(KeyEvent::new(Key::from_web_key(&event.key()), modifiers).repeat(event.repeat()))
}

/// 文字入力のウィジェットへ `on_key_down` を足す。`Inner` に `key_down: KeyDown`
/// を持たせておくこと。
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
                let element = <$t as crate::widgets::Widget>::native_element(self);
                self.0.key_down.set_on_element(element.as_ref(), f);
            }
        }
    };
}

pub(crate) use impl_key_down;
