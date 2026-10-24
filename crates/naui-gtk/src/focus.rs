//! `on_focus_change` (text-input widgets).
//!
//! Each window's `focus-widget` changes are watched, and walking up the parents
//! from the focused widget, the first registered widget found becomes that window's
//! focus owner ([`FocusOwner`]). Notifications go out only when the owner changes,
//! so moving within a widget, such as into the `GtkText` inside an entry, does not
//! notify.
//!
//! `GtkEventControllerFocus` also reports entering and leaving when the window
//! becomes inactive, but `focus-widget` is the focus inside the window and does not
//! change. This one is used to match the other platforms.
//!
//! The registry holds widgets by weak reference, and removes the handler from the
//! table when the widget (window) is freed. Leaving it in the table would keep the
//! handler and the state it captures alive forever.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use gtk::glib;
use gtk::prelude::*;
use naui_core::{FocusHandler, FocusOwner};

/// A table of weakly referenced GObjects and the values tied to them.
type Table<K, V> = RefCell<Vec<(glib::WeakRef<K>, Rc<V>)>>;

thread_local! {
    static WIDGETS: Table<gtk::Widget, FocusHandler> = const { RefCell::new(Vec::new()) };
    /// The focus owner for each window.
    static OWNERS: Table<gtk::Window, FocusOwner> = const { RefCell::new(Vec::new()) };
}

/// Sets the handler on the widget. Each call replaces it.
pub(crate) fn set_widget_handler(widget: &gtk::Widget, f: impl FnMut(bool) + 'static) {
    let handler = lookup(widget).unwrap_or_else(|| {
        let handler = Rc::new(FocusHandler::default());
        WIDGETS.with(|entries| {
            entries
                .borrow_mut()
                .push((widget.downgrade(), handler.clone()))
        });
        let weak = Rc::downgrade(&handler);
        widget.add_weak_ref_notify_local(move || {
            drop(remove_entry(&WIDGETS, &weak));
        });
        handler
    });
    // 前のクロージャを捨てるとき、捕まえていたウィジェットが解放されて
    // 表を触ることがある。表を借りていない所で置き換える。
    handler.set(f);
    adopt_current_focus(widget);
}

/// Removes the entry whose value is `value` from the table and returns it. Drop
/// what is returned somewhere the table is not borrowed (dropping it can free
/// another widget, which may touch the table again).
///
/// Called when the GObject is freed. By then the weak reference may no longer be
/// upgradable, so the entry is found by its value.
fn remove_entry<K: ObjectType, V: 'static>(
    table: &'static std::thread::LocalKey<Table<K, V>>,
    value: &Weak<V>,
) -> Option<(glib::WeakRef<K>, Rc<V>)> {
    // スレッドの後始末で表が先に消えていれば、外すものも無い。
    table
        .try_with(|entries| {
            let mut entries = entries.borrow_mut();
            let index = entries
                .iter()
                .position(|(_, v)| std::ptr::eq(Rc::as_ptr(v), value.as_ptr()))?;
            Some(entries.swap_remove(index))
        })
        .ok()
        .flatten()
}

/// If focus is already inside `widget`, records it as the owner without notifying
/// (such as focus moving to the first field when the window is shown). Otherwise
/// the `false` for leaving later would never arrive.
fn adopt_current_focus(widget: &gtk::Widget) {
    let Some(window) = widget.root().and_downcast::<gtk::Window>() else {
        return;
    };
    let Some(owner) = owner_for(&window) else {
        return;
    };
    let current = owner_of(GtkWindowExt::focus(&window));
    if current.is_some_and(|current| lookup(widget).is_some_and(|mine| Rc::ptr_eq(&current, &mine)))
    {
        owner.reset(lookup(widget));
    }
}

fn owner_for(window: &gtk::Window) -> Option<Rc<FocusOwner>> {
    OWNERS.with(|owners| {
        owners
            .borrow()
            .iter()
            .find(|(weak, _)| weak.upgrade().as_ref() == Some(window))
            .map(|(_, owner)| owner.clone())
    })
}

fn lookup(widget: &gtk::Widget) -> Option<Rc<FocusHandler>> {
    WIDGETS.with(|entries| {
        entries
            .borrow()
            .iter()
            .find(|(weak, _)| weak.upgrade().as_ref() == Some(widget))
            .map(|(_, handler)| handler.clone())
    })
}

/// The registered widget containing `widget`.
fn owner_of(widget: Option<gtk::Widget>) -> Option<Rc<FocusHandler>> {
    let mut current = widget;
    while let Some(widget) = current {
        if let Some(handler) = lookup(&widget) {
            return Some(handler);
        }
        current = widget.parent();
    }
    None
}

/// Attaches the focus watcher to a window. Called exactly once when the window is
/// created.
pub(crate) fn install(window: &impl IsA<gtk::Window>) {
    let owner = Rc::new(FocusOwner::default());
    OWNERS.with(|owners| {
        owners
            .borrow_mut()
            .push((window.as_ref().downgrade(), owner.clone()))
    });
    let weak = Rc::downgrade(&owner);
    window.as_ref().add_weak_ref_notify_local(move || {
        drop(remove_entry(&OWNERS, &weak));
    });
    window.as_ref().connect_focus_widget_notify(move |window| {
        owner.move_to(owner_of(GtkWindowExt::focus(window)))
    });
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
                let widget = <$t as crate::widgets::Widget>::native_widget(self);
                crate::focus::set_widget_handler(&widget, f);
            }
        }
    };
}

pub(crate) use impl_focus_change;
