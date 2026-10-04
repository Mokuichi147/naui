//! `on_key_down` (文字入力のウィジェットと `Window`)。
//!
//! ウィンドウごとに捕捉フェーズの `GtkEventControllerKey` を 1 つ付け、
//! キーが入力欄 (と IME) へ届く前に受け取る。フォーカスのあるウィジェット
//! から親へたどり、最初に見つかった登録済みのウィジェット、続けて
//! ウィンドウの順に通知する。`Handled` なら伝わりを止めるので、入力欄にも
//! 届かない。
//!
//! 変換中のキーは IME のものなので届けない。GTK4 には変換中かを尋ねる
//! 手段が無いので、`GtkText` / `GtkTextView` の `preedit-changed` を見て
//! 覚えておく。
//!
//! 登録表はウィジェットを弱参照で持つ。ウィジェットが解放されれば
//! 通知先も表から外れる。

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use gtk::gdk;
use gtk::glib;
use gtk::glib::translate::IntoGlib;
use gtk::prelude::*;
use naui_core::{EventResponse, Key, KeyEvent, KeyHandler, Modifiers};

type Entries = RefCell<Vec<(glib::WeakRef<gtk::Widget>, Rc<KeyHandler>)>>;

thread_local! {
    static WIDGETS: Entries = const { RefCell::new(Vec::new()) };
    static WINDOWS: Entries = const { RefCell::new(Vec::new()) };
    /// `preedit-changed` を購読済みの文字入力。
    static WATCHED: RefCell<Vec<glib::WeakRef<gtk::Widget>>> = const { RefCell::new(Vec::new()) };
    /// いま変換中の文字入力。
    static COMPOSING: RefCell<Vec<glib::WeakRef<gtk::Widget>>> = const { RefCell::new(Vec::new()) };
}

/// ウィジェットへ通知先を置く。呼ぶたびに置き換わる。
pub(crate) fn set_widget_handler(
    widget: &gtk::Widget,
    f: impl FnMut(&KeyEvent) -> EventResponse + 'static,
) {
    WIDGETS.with(|entries| handler_for(&mut entries.borrow_mut(), widget).set(f));
}

/// ウィンドウへ通知先を置く。呼ぶたびに置き換わる。
pub(crate) fn set_window_handler(
    window: &gtk::Widget,
    f: impl FnMut(&KeyEvent) -> EventResponse + 'static,
) {
    WINDOWS.with(|entries| handler_for(&mut entries.borrow_mut(), window).set(f));
}

fn handler_for(
    entries: &mut Vec<(glib::WeakRef<gtk::Widget>, Rc<KeyHandler>)>,
    widget: &gtk::Widget,
) -> Rc<KeyHandler> {
    entries.retain(|(weak, _)| weak.upgrade().is_some());
    if let Some((_, handler)) = entries
        .iter()
        .find(|(weak, _)| weak.upgrade().as_ref() == Some(widget))
    {
        return handler.clone();
    }
    let handler = Rc::new(KeyHandler::default());
    entries.push((widget.downgrade(), handler.clone()));
    handler
}

fn lookup(
    entries: &'static std::thread::LocalKey<Entries>,
    widget: &gtk::Widget,
) -> Option<Rc<KeyHandler>> {
    entries.with(|entries| {
        entries
            .borrow()
            .iter()
            .find(|(weak, _)| weak.upgrade().as_ref() == Some(widget))
            .map(|(_, handler)| handler.clone())
    })
}

/// ウィンドウにキーの受け口を付ける。ウィンドウを作るときに 1 度だけ呼ぶ。
pub(crate) fn install(window: &impl IsA<gtk::Window>) {
    let controller = gtk::EventControllerKey::new();
    controller.set_propagation_phase(gtk::PropagationPhase::Capture);
    // 押しっぱなしの繰り返しを見分けるため、押されているキーを覚える。
    let pressed: Rc<RefCell<HashSet<u32>>> = Rc::default();
    let window_widget: glib::WeakRef<gtk::Widget> =
        window.as_ref().upcast_ref::<gtk::Widget>().downgrade();
    controller.connect_key_pressed({
        let pressed = pressed.clone();
        move |_, keyval, keycode, state| {
            let repeat = !pressed.borrow_mut().insert(keycode);
            let Some(window) = window_widget.upgrade() else {
                return glib::Propagation::Proceed;
            };
            let event = KeyEvent::new(key_of(keyval), modifiers_of(state)).repeat(repeat);
            match dispatch(&window, &event) {
                EventResponse::Handled => glib::Propagation::Stop,
                EventResponse::Continue => glib::Propagation::Proceed,
            }
        }
    });
    controller.connect_key_released(move |_, _, keycode, _| {
        pressed.borrow_mut().remove(&keycode);
    });
    window.as_ref().add_controller(controller);
}

/// フォーカスのあるウィジェット → ウィンドウの順に渡す。
fn dispatch(window: &gtk::Widget, event: &KeyEvent) -> EventResponse {
    let focus = window
        .downcast_ref::<gtk::Window>()
        .and_then(gtk::prelude::GtkWindowExt::focus);
    if let Some(focus) = &focus {
        watch_preedit(focus);
        if is_composing(focus) {
            return EventResponse::Continue;
        }
    }
    let mut current = focus;
    while let Some(widget) = current {
        if &widget == window {
            break;
        }
        if let Some(handler) = lookup(&WIDGETS, &widget) {
            if handler.emit(event) == EventResponse::Handled {
                return EventResponse::Handled;
            }
            break;
        }
        current = widget.parent();
    }
    lookup(&WINDOWS, window).map_or(EventResponse::Continue, |handler| handler.emit(event))
}

/// 文字入力 (`GtkText` / `GtkTextView`) の変換の始まりと終わりを覚える。
///
/// フォーカスを受けたものから順に購読するので、変換を始めるキーそのものは
/// (購読より前に来ると) 届くことがある。
fn watch_preedit(widget: &gtk::Widget) {
    let already = WATCHED.with(|watched| {
        let mut watched = watched.borrow_mut();
        watched.retain(|weak| weak.upgrade().is_some());
        if watched
            .iter()
            .any(|weak| weak.upgrade().as_ref() == Some(widget))
        {
            return true;
        }
        watched.push(widget.downgrade());
        false
    });
    if already {
        return;
    }
    let update = |widget: &gtk::Widget, preedit: &str| {
        COMPOSING.with(|composing| {
            let mut composing = composing.borrow_mut();
            composing.retain(|weak| weak.upgrade().is_some_and(|other| &other != widget));
            if !preedit.is_empty() {
                composing.push(widget.downgrade());
            }
        });
    };
    if let Some(text) = widget.downcast_ref::<gtk::Text>() {
        text.connect_preedit_changed(move |text, preedit| update(text.upcast_ref(), preedit));
    } else if let Some(view) = widget.downcast_ref::<gtk::TextView>() {
        view.connect_preedit_changed(move |view, preedit| update(view.upcast_ref(), preedit));
    }
}

fn is_composing(widget: &gtk::Widget) -> bool {
    COMPOSING.with(|composing| {
        composing
            .borrow()
            .iter()
            .any(|weak| weak.upgrade().as_ref() == Some(widget))
    })
}

fn modifiers_of(state: gdk::ModifierType) -> Modifiers {
    Modifiers {
        shift: state.contains(gdk::ModifierType::SHIFT_MASK),
        control: state.contains(gdk::ModifierType::CONTROL_MASK),
        alt: state.contains(gdk::ModifierType::ALT_MASK),
        meta: state.intersects(gdk::ModifierType::SUPER_MASK | gdk::ModifierType::META_MASK),
    }
}

/// GDK のキー値を naui のキーへ写す。
fn key_of(keyval: gdk::Key) -> Key {
    use gdk::Key as K;
    match keyval {
        K::Return | K::KP_Enter | K::ISO_Enter => Key::Enter,
        K::Escape => Key::Escape,
        K::Tab | K::ISO_Left_Tab | K::KP_Tab => Key::Tab,
        K::BackSpace => Key::Backspace,
        K::Delete | K::KP_Delete => Key::Delete,
        K::space | K::KP_Space => Key::Space,
        K::Up | K::KP_Up => Key::ArrowUp,
        K::Down | K::KP_Down => Key::ArrowDown,
        K::Left | K::KP_Left => Key::ArrowLeft,
        K::Right | K::KP_Right => Key::ArrowRight,
        K::Home | K::KP_Home => Key::Home,
        K::End | K::KP_End => Key::End,
        K::Page_Up | K::KP_Page_Up => Key::PageUp,
        K::Page_Down | K::KP_Page_Down => Key::PageDown,
        other => function_key(other)
            .or_else(|| other.to_unicode().map(Key::character))
            .unwrap_or(Key::Other),
    }
}

fn function_key(keyval: gdk::Key) -> Option<Key> {
    let first = gdk::Key::F1.into_glib();
    let value = keyval.into_glib();
    (first..first + 24)
        .contains(&value)
        .then(|| Key::F((value - first + 1) as u8))
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
                let widget = <$t as crate::widgets::Widget>::native_widget(self);
                crate::keys::set_widget_handler(&widget, f);
            }
        }
    };
}

pub(crate) use impl_key_down;
