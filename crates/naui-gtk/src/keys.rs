//! `on_key_down` and `on_shortcut` (widgets and `Window`).
//!
//! Each window gets one capture-phase `GtkEventControllerKey`, which receives keys
//! before they reach the field (and the IME). It walks up the parents from the
//! focused widget, collects the widgets that have an `on_key_down` or shortcuts,
//! and delivers the key in naui's order ([`naui_core::deliver_key`]): the innermost
//! `on_key_down`, the widgets' shortcuts from the inside out, the window's
//! `on_key_down`, then the window's shortcuts. With `Handled`, propagation stops, so
//! the key does not reach the field either.
//!
//! The menu bar's accelerators are the `GtkApplication`'s, which GTK handles in a
//! capture-phase `GtkShortcutController` the window adds when it is created. A
//! widget's controllers run newest first, so naui's controller, added after it,
//! comes before the menu bar. Shortcuts are not given to GTK as
//! `GtkShortcutController`s for the same reason: one on a widget would run after the
//! window's capture phase, so the menu bar would come first.
//!
//! `Dialog` and `Sheet` open in an `AdwDialog` inside the window. Keys pressed in
//! one reach the widgets in it but not the window, as on the other platforms, where
//! those are windows of their own.
//!
//! Modifier keys pressed on their own are not delivered (the same as macOS).
//!
//! Keys during composition belong to the IME and are not delivered. GTK4 has no way
//! to ask whether composition is in progress, so `preedit-changed` on `GtkText` /
//! `GtkTextView` is watched and remembered.
//!
//! The registry holds widgets by weak reference. When a widget is freed, its
//! handlers are removed from the table too.

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use gtk::gdk;
use gtk::glib;
use gtk::glib::translate::IntoGlib;
use gtk::prelude::*;
use naui_core::{
    EventResponse, Key, KeyEvent, KeyHandler, KeyTarget, Modifiers, Shortcut, ShortcutHandlers,
};

type Entries<H> = RefCell<Vec<(glib::WeakRef<gtk::Widget>, Rc<H>)>>;

thread_local! {
    static WIDGETS: Entries<KeyHandler> = const { RefCell::new(Vec::new()) };
    static WINDOWS: Entries<KeyHandler> = const { RefCell::new(Vec::new()) };
    static WIDGET_SHORTCUTS: Entries<ShortcutHandlers> = const { RefCell::new(Vec::new()) };
    static WINDOW_SHORTCUTS: Entries<ShortcutHandlers> = const { RefCell::new(Vec::new()) };
    /// The text inputs whose `preedit-changed` is already subscribed.
    static WATCHED: RefCell<Vec<glib::WeakRef<gtk::Widget>>> = const { RefCell::new(Vec::new()) };
    /// The text inputs currently composing.
    static COMPOSING: RefCell<Vec<glib::WeakRef<gtk::Widget>>> = const { RefCell::new(Vec::new()) };
}

/// Sets the handler on the widget. Each call replaces it.
pub(crate) fn set_widget_handler(
    widget: &gtk::Widget,
    f: impl FnMut(&KeyEvent) -> EventResponse + 'static,
) {
    WIDGETS.with(|entries| handler_for(&mut entries.borrow_mut(), widget).set(f));
}

/// Sets the handler on the window. Each call replaces it.
pub(crate) fn set_window_handler(
    window: &gtk::Widget,
    f: impl FnMut(&KeyEvent) -> EventResponse + 'static,
) {
    WINDOWS.with(|entries| handler_for(&mut entries.borrow_mut(), window).set(f));
}

/// Gives the widget a shortcut, replacing the closure it had.
pub(crate) fn set_widget_shortcut(
    widget: &gtk::Widget,
    shortcut: Shortcut,
    f: impl FnMut() + 'static,
) {
    WIDGET_SHORTCUTS
        .with(|entries| handler_for(&mut entries.borrow_mut(), widget).set(shortcut, f));
}

/// Removes a shortcut from the widget.
pub(crate) fn remove_widget_shortcut(widget: &gtk::Widget, shortcut: Shortcut) {
    if let Some(handlers) = lookup(&WIDGET_SHORTCUTS, widget) {
        handlers.remove(shortcut);
    }
}

/// Gives the window a shortcut, replacing the closure it had.
pub(crate) fn set_window_shortcut(
    window: &gtk::Widget,
    shortcut: Shortcut,
    f: impl FnMut() + 'static,
) {
    WINDOW_SHORTCUTS
        .with(|entries| handler_for(&mut entries.borrow_mut(), window).set(shortcut, f));
}

/// Removes a shortcut from the window.
pub(crate) fn remove_window_shortcut(window: &gtk::Widget, shortcut: Shortcut) {
    if let Some(handlers) = lookup(&WINDOW_SHORTCUTS, window) {
        handlers.remove(shortcut);
    }
}

fn handler_for<H: Default>(
    entries: &mut Vec<(glib::WeakRef<gtk::Widget>, Rc<H>)>,
    widget: &gtk::Widget,
) -> Rc<H> {
    entries.retain(|(weak, _)| weak.upgrade().is_some());
    if let Some((_, handler)) = entries
        .iter()
        .find(|(weak, _)| weak.upgrade().as_ref() == Some(widget))
    {
        return handler.clone();
    }
    let handler = Rc::new(H::default());
    entries.push((widget.downgrade(), handler.clone()));
    handler
}

fn lookup<H>(
    entries: &'static std::thread::LocalKey<Entries<H>>,
    widget: &gtk::Widget,
) -> Option<Rc<H>> {
    entries.with(|entries| {
        entries
            .borrow()
            .iter()
            .find(|(weak, _)| weak.upgrade().as_ref() == Some(widget))
            .map(|(_, handler)| handler.clone())
    })
}

/// What `widget` has for keys: its `on_key_down` and its shortcuts (if any are
/// given).
fn target_of(
    key_downs: &'static std::thread::LocalKey<Entries<KeyHandler>>,
    shortcuts: &'static std::thread::LocalKey<Entries<ShortcutHandlers>>,
    widget: &gtk::Widget,
) -> KeyTarget {
    KeyTarget {
        key_down: lookup(key_downs, widget),
        shortcuts: lookup(shortcuts, widget).filter(|s| !s.is_empty()),
    }
}

/// Attaches the key receiver to a window. Called exactly once when the window is
/// created.
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
            if is_modifier(keyval) {
                return glib::Propagation::Proceed;
            }
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

/// Delivers `event` to the widgets around the window's focus and to the window, in
/// naui's order. Used for real keys and by `Window::send_key`.
pub(crate) fn dispatch(window: &gtk::Widget, event: &KeyEvent) -> EventResponse {
    let focus = window
        .downcast_ref::<gtk::Window>()
        .and_then(gtk::prelude::GtkWindowExt::focus);
    if let Some(focus) = &focus {
        watch_preedit(focus);
        if is_composing(focus) {
            return EventResponse::Continue;
        }
    }
    let mut widgets = Vec::new();
    let mut in_dialog = false;
    let mut current = focus;
    while let Some(widget) = current {
        if &widget == window {
            break;
        }
        let target = target_of(&WIDGETS, &WIDGET_SHORTCUTS, &widget);
        if !target.is_empty() {
            widgets.push(target);
        }
        // `Dialog` と `Sheet` は窓の中の `AdwDialog` に出る。ほかの環境では別の窓
        // なので、その中のキーは窓へ渡さない。
        if widget.is::<adw::Dialog>() {
            in_dialog = true;
            break;
        }
        current = widget.parent();
    }
    let window = if in_dialog {
        KeyTarget::default()
    } else {
        target_of(&WINDOWS, &WINDOW_SHORTCUTS, window)
    };
    naui_core::deliver_key(event, &widgets, &window)
}

/// Remembers when composition starts and ends in a text input (`GtkText` /
/// `GtkTextView`).
///
/// Subscriptions are made as widgets receive focus, so the very key that starts
/// composition may still be delivered (if it arrives before the subscription).
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

/// The modifier keys in an event's state (keys, pointer and wheel alike).
pub(crate) fn modifiers_of(state: gdk::ModifierType) -> Modifiers {
    Modifiers {
        shift: state.contains(gdk::ModifierType::SHIFT_MASK),
        control: state.contains(gdk::ModifierType::CONTROL_MASK),
        alt: state.contains(gdk::ModifierType::ALT_MASK),
        meta: state.intersects(gdk::ModifierType::SUPER_MASK | gdk::ModifierType::META_MASK),
    }
}

/// ⇧, Ctrl, Alt, Meta, Super, Hyper (left and right) and AltGr.
fn is_modifier(keyval: gdk::Key) -> bool {
    use gdk::Key as K;
    matches!(
        keyval,
        K::Shift_L
            | K::Shift_R
            | K::Control_L
            | K::Control_R
            | K::Alt_L
            | K::Alt_R
            | K::Meta_L
            | K::Meta_R
            | K::Super_L
            | K::Super_R
            | K::Hyper_L
            | K::Hyper_R
            | K::ISO_Level3_Shift
    )
}

/// Maps a GDK key value to a naui key.
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
                let widget = <$t as crate::widgets::Widget>::native_widget(self);
                crate::keys::set_widget_handler(&widget, f);
            }
        }
    };
}

pub(crate) use impl_key_down;
