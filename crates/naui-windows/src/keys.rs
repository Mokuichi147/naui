//! `on_key_down` (文字入力のウィジェットと `Window`)。
//!
//! ウィンドウの根に `PreviewKeyDown` を 1 つ付ける。トンネル (根から葉へ)
//! で届くので、キーが入力欄へ届く前に受け取れる。キーの出どころ
//! (`OriginalSource`) から親へたどり、最初に見つかった登録済みのウィジェット、
//! 続けてウィンドウの順に通知する。`Handled` なら `Handled` を立てるので、
//! 入力欄 (改行の挿入など) にも届かない。
//!
//! 変換中のキーは IME のものなので届けない。IME が処理するキーは
//! `VK_PROCESSKEY` (229) で来るほか、`TextBox` の
//! `TextCompositionStarted` / `TextCompositionEnded` の間も変換中とみなす。
//!
//! 登録表は要素を弱参照で持つ。要素が解放されれば通知先も表から外れる。

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;
use std::sync::Arc;

use naui_core::{EventResponse, Key, KeyEvent, KeyHandler, Modifiers};
use naui_winui3::Microsoft::UI::Xaml::Controls::{
    TextBox, TextCompositionEndedEventArgs, TextCompositionStartedEventArgs,
};
use naui_winui3::Microsoft::UI::Xaml::Input::{KeyEventHandler, KeyRoutedEventArgs};
use naui_winui3::Microsoft::UI::Xaml::Media::VisualTreeHelper;
use naui_winui3::Microsoft::UI::Xaml::{DependencyObject, UIElement};
use windows::Foundation::TypedEventHandler;
use windows::System::VirtualKey;
use windows_core::{IUnknown, Interface, Weak};

use crate::ui_thread::UiThreadCell;

/// IME が処理しているキー。`Windows.System.VirtualKey` には名前が無い。
const VK_PROCESSKEY: i32 = 229;

type Entries = RefCell<Vec<(Weak<UIElement>, Rc<KeyHandler>)>>;

thread_local! {
    static WIDGETS: Entries = const { RefCell::new(Vec::new()) };
    /// 変換の始まりと終わりを購読済みの `TextBox`。
    static WATCHED: RefCell<Vec<Weak<TextBox>>> = const { RefCell::new(Vec::new()) };
    /// いま変換中の `TextBox`。
    static COMPOSING: RefCell<Vec<Weak<TextBox>>> = const { RefCell::new(Vec::new()) };
}

/// 同じ COM オブジェクトか (インターフェースが違っても `IUnknown` は同じ)。
fn same<A: Interface, B: Interface>(a: &A, b: &B) -> bool {
    match (a.cast::<IUnknown>(), b.cast::<IUnknown>()) {
        (Ok(a), Ok(b)) => a.as_raw() == b.as_raw(),
        _ => false,
    }
}

/// ウィジェットの要素へ通知先を置く。呼ぶたびに置き換わる。
pub(crate) fn set_widget_handler(
    element: &UIElement,
    f: impl FnMut(&KeyEvent) -> EventResponse + 'static,
) {
    WIDGETS.with(|entries| {
        let mut entries = entries.borrow_mut();
        entries.retain(|(weak, _)| weak.upgrade().is_some());
        if let Some((_, handler)) = entries
            .iter()
            .find(|(weak, _)| weak.upgrade().is_some_and(|other| same(&other, element)))
        {
            handler.set(f);
            return;
        }
        let Ok(weak) = element.downgrade() else {
            return;
        };
        let handler = Rc::new(KeyHandler::default());
        handler.set(f);
        entries.push((weak, handler));
    });
}

fn lookup(object: &DependencyObject) -> Option<Rc<KeyHandler>> {
    WIDGETS.with(|entries| {
        entries
            .borrow()
            .iter()
            .find(|(weak, _)| weak.upgrade().is_some_and(|other| same(&other, object)))
            .map(|(_, handler)| handler.clone())
    })
}

/// ウィンドウの根にキーの受け口を付ける。`window` はウィンドウの通知先。
pub(crate) fn install(root: &UIElement, window: Rc<KeyHandler>) -> windows_core::Result<()> {
    // 押しっぱなしの繰り返しを見分けるため、押されているキーを覚える。
    let pressed = Arc::new(UiThreadCell::new(HashSet::<i32>::new()));
    let window = Arc::new(UiThreadCell::new(window));
    let down = KeyEventHandler::new({
        let pressed = pressed.clone();
        move |_, args| {
            let Some(args) = args.as_ref() else {
                return Ok(());
            };
            let key = args.Key().unwrap_or(VirtualKey::None);
            let repeat = pressed
                .try_with_mut(|pressed| !pressed.insert(key.0))
                .unwrap_or(false);
            let Some(window) = window.try_with_mut(|window| window.clone()) else {
                return Ok(());
            };
            if dispatch(args, key, repeat, &window) == EventResponse::Handled {
                let _ = args.SetHandled(true);
            }
            Ok(())
        }
    });
    root.PreviewKeyDown(&down)?;
    let up = KeyEventHandler::new(move |_, args| {
        if let Some(args) = args.as_ref() {
            let key = args.Key().unwrap_or(VirtualKey::None);
            let _ = pressed.try_with_mut(|pressed| pressed.remove(&key.0));
        }
        Ok(())
    });
    root.PreviewKeyUp(&up)?;
    Ok(())
}

/// キーの出どころ → ウィンドウの順に渡す。
fn dispatch(
    args: &KeyRoutedEventArgs,
    key: VirtualKey,
    repeat: bool,
    window: &KeyHandler,
) -> EventResponse {
    if key.0 == VK_PROCESSKEY {
        return EventResponse::Continue;
    }
    let source = args
        .OriginalSource()
        .ok()
        .and_then(|source| source.cast::<DependencyObject>().ok());
    if let Some(text_box) = source.as_ref().and_then(|s| s.cast::<TextBox>().ok()) {
        watch_composition(&text_box);
        if is_composing(&text_box) {
            return EventResponse::Continue;
        }
    }
    let event = KeyEvent::new(key_of(key), modifiers()).repeat(repeat);

    let mut current = source;
    while let Some(object) = current {
        if let Some(handler) = lookup(&object) {
            if handler.emit(&event) == EventResponse::Handled {
                return EventResponse::Handled;
            }
            break;
        }
        current = VisualTreeHelper::GetParent(&object).ok();
    }
    window.emit(&event)
}

/// `TextBox` の変換の始まりと終わりを覚える。
fn watch_composition(text_box: &TextBox) {
    let already = WATCHED.with(|watched| {
        let mut watched = watched.borrow_mut();
        watched.retain(|weak| weak.upgrade().is_some());
        if watched
            .iter()
            .any(|weak| weak.upgrade().is_some_and(|other| same(&other, text_box)))
        {
            return true;
        }
        if let Ok(weak) = text_box.downgrade() {
            watched.push(weak);
        }
        false
    });
    if already {
        return;
    }
    let started =
        TypedEventHandler::<TextBox, TextCompositionStartedEventArgs>::new(|sender, _| {
            if let Some(text_box) = sender.as_ref() {
                set_composing(text_box, true);
            }
            Ok(())
        });
    let ended = TypedEventHandler::<TextBox, TextCompositionEndedEventArgs>::new(|sender, _| {
        if let Some(text_box) = sender.as_ref() {
            set_composing(text_box, false);
        }
        Ok(())
    });
    let _ = text_box.TextCompositionStarted(&started);
    let _ = text_box.TextCompositionEnded(&ended);
}

fn set_composing(text_box: &TextBox, composing: bool) {
    COMPOSING.with(|list| {
        let mut list = list.borrow_mut();
        list.retain(|weak| weak.upgrade().is_some_and(|other| !same(&other, text_box)));
        if composing {
            if let Ok(weak) = text_box.downgrade() {
                list.push(weak);
            }
        }
    });
}

fn is_composing(text_box: &TextBox) -> bool {
    COMPOSING.with(|list| {
        list.borrow()
            .iter()
            .any(|weak| weak.upgrade().is_some_and(|other| same(&other, text_box)))
    })
}

fn modifiers() -> Modifiers {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetKeyState, VIRTUAL_KEY, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
    };
    // 最上位のビットが立っていれば、そのキーは押されている。
    let down = |key: VIRTUAL_KEY| unsafe { GetKeyState(i32::from(key.0)) } < 0;
    Modifiers {
        shift: down(VK_SHIFT),
        control: down(VK_CONTROL),
        alt: down(VK_MENU),
        meta: down(VK_LWIN) || down(VK_RWIN),
    }
}

/// 仮想キーを naui のキーへ写す。
fn key_of(key: VirtualKey) -> Key {
    let code = key.0;
    match code {
        13 => Key::Enter,
        27 => Key::Escape,
        9 => Key::Tab,
        8 => Key::Backspace,
        46 => Key::Delete,
        32 => Key::Space,
        38 => Key::ArrowUp,
        40 => Key::ArrowDown,
        37 => Key::ArrowLeft,
        39 => Key::ArrowRight,
        36 => Key::Home,
        35 => Key::End,
        33 => Key::PageUp,
        34 => Key::PageDown,
        112..=135 => Key::F((code - 111) as u8),
        // 英字 (VK_A..VK_Z) と数字 (VK_0..VK_9、テンキー)。
        65..=90 | 48..=57 => char::from_u32(code as u32).map_or(Key::Other, Key::character),
        96..=105 => char::from_u32((code - 96 + 48) as u32).map_or(Key::Other, Key::character),
        _ => character_of(code),
    }
}

/// 記号のキー (VK_OEM_*) は、いまのキー配列で ⇧ を押さないときの文字で読む。
fn character_of(code: i32) -> Key {
    use windows::Win32::UI::Input::KeyboardAndMouse::{MapVirtualKeyW, MAPVK_VK_TO_CHAR};
    let Ok(code) = u32::try_from(code) else {
        return Key::Other;
    };
    // 上位ビットはデッドキーの印なので落とす。
    let mapped = unsafe { MapVirtualKeyW(code, MAPVK_VK_TO_CHAR) } & 0x7FFF_FFFF;
    char::from_u32(mapped)
        .filter(|c| *c != '\0')
        .map_or(Key::Other, Key::character)
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
                let element = <$t as crate::widgets::Widget>::native_element(self);
                crate::keys::set_widget_handler(&element, f);
            }
        }
    };
}

pub(crate) use impl_key_down;
