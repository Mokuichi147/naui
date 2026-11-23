//! `on_key_down` and `on_shortcut` (widgets and `Window`).
//!
//! One `PreviewKeyDown` is attached to the window's root. It is delivered by
//! tunneling (root to leaf), so keys can be received before they reach the field.
//! Walking up the parents from the key's source (`OriginalSource`), it collects the
//! widgets that have an `on_key_down` or shortcuts and delivers the key in naui's
//! order ([`naui_core::deliver_key`]): the innermost `on_key_down`, the widgets'
//! shortcuts from the inside out, the window's `on_key_down`, then the window's
//! shortcuts. With `Handled`, `Handled` is set, so the key does not reach the field
//! either (inserting a newline, for example), nor the menu bar, whose shortcuts are
//! picked up from the root's bubbling `KeyDown`.
//!
//! WinUI's `KeyboardAccelerator` is not used: it is not in naui's projection, and
//! its invocation order relative to the focused control and the root's
//! `PreviewKeyDown` would differ from the other platforms.
//!
//! Modifier keys pressed on their own are not delivered (the same as macOS).
//!
//! Keys during composition belong to the IME and are not delivered. Keys the IME
//! processes arrive as `VK_PROCESSKEY` (229), and the span between a `TextBox`'s
//! `TextCompositionStarted` / `TextCompositionEnded` is also treated as composing.
//!
//! The registry holds elements by weak reference. When an element is freed, its
//! handler is removed from the table too. On the shutdown path, the handlers are
//! released with [`release_handlers`] (see [`Registry`]).

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;
use std::sync::Arc;

use naui_core::{
    EventResponse, Key, KeyEvent, KeyHandler, KeyTarget, Modifiers, Shortcut, ShortcutHandlers,
};
use naui_winui3::Microsoft::UI::Xaml::Controls::{
    Control, TextBox, TextCompositionEndedEventArgs, TextCompositionStartedEventArgs,
};
use naui_winui3::Microsoft::UI::Xaml::Input::{KeyEventHandler, KeyRoutedEventArgs};
use naui_winui3::Microsoft::UI::Xaml::Media::VisualTreeHelper;
use naui_winui3::Microsoft::UI::Xaml::{DependencyObject, FocusState, UIElement};
use windows::Foundation::TypedEventHandler;
use windows::System::VirtualKey;
use windows_core::{IUnknown, Interface, Weak};

use crate::ui_thread::{Registry, UiThreadCell};

/// A key the IME is processing. `Windows.System.VirtualKey` has no name for it.
const VK_PROCESSKEY: i32 = 229;

type Entries<H> = Registry<(Weak<UIElement>, Rc<H>)>;

thread_local! {
    static WIDGETS: Entries<KeyHandler> = const { Registry::new() };
    static WIDGET_SHORTCUTS: Entries<ShortcutHandlers> = const { Registry::new() };
    /// The `TextBox`es whose composition start and end are already subscribed.
    static WATCHED: RefCell<Vec<Weak<TextBox>>> = const { RefCell::new(Vec::new()) };
    /// The `TextBox`es currently composing.
    static COMPOSING: RefCell<Vec<Weak<TextBox>>> = const { RefCell::new(Vec::new()) };
}

/// Whether they are the same COM object (`IUnknown` is the same even if the
/// interfaces differ).
pub(crate) fn same<A: Interface, B: Interface>(a: &A, b: &B) -> bool {
    match (a.cast::<IUnknown>(), b.cast::<IUnknown>()) {
        (Ok(a), Ok(b)) => a.as_raw() == b.as_raw(),
        _ => false,
    }
}

/// Sets the handler on the widget's element. Each call replaces it.
pub(crate) fn set_widget_handler(
    element: &UIElement,
    f: impl FnMut(&KeyEvent) -> EventResponse + 'static,
) {
    if let Some(handler) = WIDGETS.with(|entries| handler_for(entries, element)) {
        handler.set(f);
    }
}

/// Gives the widget's element a shortcut, replacing the closure it had.
pub(crate) fn set_widget_shortcut(
    element: &UIElement,
    shortcut: Shortcut,
    f: impl FnMut() + 'static,
) {
    if let Some(handlers) = WIDGET_SHORTCUTS.with(|entries| handler_for(entries, element)) {
        handlers.set(shortcut, f);
    }
}

/// Removes a shortcut from the widget's element.
pub(crate) fn remove_widget_shortcut(element: &UIElement, shortcut: Shortcut) {
    if let Some(handlers) = lookup(&WIDGET_SHORTCUTS, element) {
        handlers.remove(shortcut);
    }
}

/// The handler for `element`, created if absent. Entries for elements that are gone
/// are cleaned up here too. `None` if the element cannot be held weakly.
fn handler_for<H: Default>(entries: &Entries<H>, element: &UIElement) -> Option<Rc<H>> {
    let mut entries = entries.borrow_mut();
    entries.retain(|(weak, _)| weak.upgrade().is_some());
    if let Some((_, handler)) = entries
        .iter()
        .find(|(weak, _)| weak.upgrade().is_some_and(|other| same(&other, element)))
    {
        return Some(handler.clone());
    }
    let weak = element.downgrade().ok()?;
    let handler = Rc::new(H::default());
    entries.push((weak, handler.clone()));
    Some(handler)
}

/// Releases every handler. Called on the shutdown path, before XAML's cleanup.
pub(crate) fn release_handlers() {
    // 借用の外で捨てる。
    drop(WIDGETS.with(Registry::take));
    drop(WIDGET_SHORTCUTS.with(Registry::take));
}

fn lookup<H, T: Interface>(
    entries: &'static std::thread::LocalKey<Entries<H>>,
    object: &T,
) -> Option<Rc<H>> {
    entries.with(|entries| {
        entries
            .borrow()
            .iter()
            .find(|(weak, _)| weak.upgrade().is_some_and(|other| same(&other, object)))
            .map(|(_, handler)| handler.clone())
    })
}

/// Attaches the key receiver to the window's root. `window` is what the window has
/// for keys (its `on_key_down` and its shortcuts).
pub(crate) fn install(root: &UIElement, window: KeyTarget) -> windows_core::Result<()> {
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

/// Delivers an arriving key from its source, in naui's order.
fn dispatch(
    args: &KeyRoutedEventArgs,
    key: VirtualKey,
    repeat: bool,
    window: &KeyTarget,
) -> EventResponse {
    if key.0 == VK_PROCESSKEY || is_modifier(key.0) {
        return EventResponse::Continue;
    }
    let source = args
        .OriginalSource()
        .ok()
        .and_then(|source| source.cast::<DependencyObject>().ok());
    let event = KeyEvent::new(key_of(key), modifiers()).repeat(repeat);
    deliver(source, &event, window)
}

/// Delivers `event` to the widgets from `source` outward and to the window, in
/// naui's order. Nothing is delivered while `source` is a composing `TextBox`.
fn deliver(
    source: Option<DependencyObject>,
    event: &KeyEvent,
    window: &KeyTarget,
) -> EventResponse {
    if let Some(text_box) = source.as_ref().and_then(|s| s.cast::<TextBox>().ok()) {
        watch_composition(&text_box);
        if is_composing(&text_box) {
            return EventResponse::Continue;
        }
    }
    let mut widgets = Vec::new();
    let mut current = source;
    while let Some(object) = current {
        let target = KeyTarget {
            key_down: lookup(&WIDGETS, &object),
            shortcuts: lookup(&WIDGET_SHORTCUTS, &object).filter(|s| !s.is_empty()),
        };
        if !target.is_empty() {
            widgets.push(target);
        }
        current = VisualTreeHelper::GetParent(&object).ok();
    }
    naui_core::deliver_key(event, &widgets, window)
}

/// Delivers `event` as if pressed with focus where it is now under `root`, for
/// `Window::send_key`. Starts from the root when nothing under it has focus.
pub(crate) fn send(root: &UIElement, event: &KeyEvent, window: &KeyTarget) -> EventResponse {
    let root = root.cast::<DependencyObject>().ok();
    let focused = root.as_ref().and_then(focused_within);
    deliver(focused.or(root), event, window)
}

/// The control under `object` that has keyboard focus.
fn focused_within(object: &DependencyObject) -> Option<DependencyObject> {
    let focused = object
        .cast::<Control>()
        .and_then(|control| control.FocusState())
        .is_ok_and(|state| state != FocusState::Unfocused);
    if focused {
        return Some(object.clone());
    }
    let count = VisualTreeHelper::GetChildrenCount(object).unwrap_or(0);
    (0..count)
        .filter_map(|index| VisualTreeHelper::GetChild(object, index).ok())
        .find_map(|child| focused_within(&child))
}

/// Calls `f` when Enter is pressed in a field (the element's `PreviewKeyDown`).
///
/// `TextBox` / `PasswordBox` have no event that reports only confirmation, so it is
/// picked up from keys. So as not to miss it when the control handles the key
/// itself and sets `Handled`, it is received on the tunneling side, which arrives
/// before the control. Keys for which `on_key_down` returned `Handled` have
/// `Handled` set in the window root's `PreviewKeyDown`, which arrives even earlier,
/// so they never reach here. Enters that confirm a conversion (those arriving as
/// `VK_PROCESSKEY`, or during a `TextBox`'s composition) do not count either.
///
/// `f` stays in the element's delegate, so hold the widget by weak reference.
pub(crate) fn on_enter(element: &UIElement, f: impl FnMut() + 'static) -> windows_core::Result<()> {
    let f = UiThreadCell::new(f);
    let handler = KeyEventHandler::new(move |_, args| {
        let Some(args) = args.as_ref() else {
            return Ok(());
        };
        if args.Key().unwrap_or(VirtualKey::None) != VirtualKey::Enter {
            return Ok(());
        }
        let text_box = args
            .OriginalSource()
            .ok()
            .and_then(|source| source.cast::<TextBox>().ok());
        if text_box.is_some_and(|text_box| is_composing(&text_box)) {
            return Ok(());
        }
        let _ = f.try_with_mut(|f| f());
        Ok(())
    });
    element.PreviewKeyDown(&handler)?;
    Ok(())
}

/// Remembers when a `TextBox`'s composition starts and ends.
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

pub(crate) fn modifiers() -> Modifiers {
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

/// ⇧, Ctrl, Alt and the Windows key (including left/right variants).
fn is_modifier(code: i32) -> bool {
    matches!(code, 16..=18 | 91 | 92 | 160..=165)
}

/// Maps a virtual key to a naui key.
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

/// Symbol keys (VK_OEM_*) are read as the character they give without ⇧ in the
/// current keyboard layout.
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

/// Adds `on_key_down` to text-input widgets.
macro_rules! impl_key_down {
    ($t:ty) => {
        impl $t {
            /// Notified when a key is pressed. Returning `Handled` keeps the key from reaching
            /// the field and the window (so you can stop newlines or confirmation).
            ///
            /// Keys pressed while an IME is composing are not delivered.
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
