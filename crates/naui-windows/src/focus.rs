//! `on_focus_change` (text-input widgets).
//!
//! One `GotFocus` is attached to the window's root. `GotFocus` is a bubbling routed
//! event, so it reaches the root whichever element gains focus. Walking up the
//! parents from its source (`OriginalSource`), the first registered widget found
//! becomes that window's focus owner ([`FocusOwner`]). Notifications go out only
//! when the owner changes, so moving within a widget, such as into the input field
//! inside a `NumberBox`, does not notify.
//!
//! WinUI does not change the focused element when the window becomes inactive, so
//! nothing is notified then (the same as the other platforms).
//!
//! The registry holds elements and weak references to the handlers. The handler
//! itself is held by the widget ([`Focus`]), and the table entry is removed when the
//! widget is freed. If the table held handlers strongly, a discarded field's handler
//! and the state it captures would stay until the next registration. It has the
//! same lifetime as the widget's other handlers, so on the shutdown path too it is
//! released together with the widget tree.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak as RcWeak};
use std::sync::Arc;

use naui_core::{FocusHandler, FocusOwner};
use naui_winui3::Microsoft::UI::Xaml::Media::VisualTreeHelper;
use naui_winui3::Microsoft::UI::Xaml::{DependencyObject, RoutedEventHandler, UIElement};
use windows_core::{Interface, Weak};

use crate::keys::same;
use crate::ui_thread::{Registry, UiThreadCell};

/// The watcher for one window root.
#[derive(Default)]
struct Tracker {
    owner: FocusOwner,
    /// The element that last gained focus. Used, when a handler is set, to check whether
    /// focus is already inside it.
    focused: RefCell<Option<Weak<DependencyObject>>>,
}

type Entries = Registry<(Weak<UIElement>, RcWeak<FocusHandler>)>;

thread_local! {
    static WIDGETS: Entries = const { Registry::new() };
    /// The live watchers. The watchers themselves are held by the root's subscription.
    static TRACKERS: RefCell<Vec<RcWeak<Tracker>>> = const { RefCell::new(Vec::new()) };
}

/// The handler held by a widget.
#[derive(Default)]
pub(crate) struct Focus {
    handler: Rc<FocusHandler>,
    registered: Cell<bool>,
}

impl Focus {
    /// Sets the handler. Each call replaces it.
    pub(crate) fn set(&self, element: &UIElement, f: impl FnMut(bool) + 'static) {
        self.handler.set(f);
        if self.registered.get() {
            return;
        }
        let Ok(weak) = element.downgrade() else {
            return;
        };
        self.registered.set(true);
        WIDGETS.with(|entries| {
            entries
                .borrow_mut()
                .push((weak, Rc::downgrade(&self.handler)))
        });
        adopt_current_focus(&self.handler);
    }
}

impl Drop for Focus {
    fn drop(&mut self) {
        if !self.registered.get() {
            return;
        }
        // 外す項目は弱参照だけなので、捨てても表を触り直すことはない。
        // スレッドの後始末で表が先に消えていれば、外すものも無い。
        let _ = WIDGETS.try_with(|entries| {
            entries
                .borrow_mut()
                .retain(|(_, handler)| !std::ptr::eq(handler.as_ptr(), Rc::as_ptr(&self.handler)))
        });
    }
}

/// If focus is already inside a newly registered widget, records it as the owner
/// without notifying (such as focus moving to the first field when the window is
/// shown). Otherwise the `false` for leaving later would never arrive.
fn adopt_current_focus(handler: &Rc<FocusHandler>) {
    let trackers: Vec<Rc<Tracker>> = TRACKERS.with(|trackers| {
        let mut trackers = trackers.borrow_mut();
        trackers.retain(|weak| weak.strong_count() > 0);
        trackers.iter().filter_map(RcWeak::upgrade).collect()
    });
    for tracker in trackers {
        let focused = tracker.focused.borrow().as_ref().and_then(Weak::upgrade);
        if owner_of(focused).is_some_and(|owner| Rc::ptr_eq(&owner, handler)) {
            tracker.owner.reset(Some(handler.clone()));
        }
    }
}

/// Releases every handler. Called on the shutdown path, before XAML's cleanup.
///
/// The handlers themselves are held by the widgets, but the owners remembered by the
/// watchers also hold handlers, so they are removed. The weak references to elements
/// remaining in the table are discarded too.
pub(crate) fn release_handlers() {
    let trackers: Vec<Rc<Tracker>> = TRACKERS.with(|trackers| {
        trackers
            .borrow()
            .iter()
            .filter_map(RcWeak::upgrade)
            .collect()
    });
    for tracker in trackers {
        tracker.owner.reset(None);
    }
    // 借用の外で捨てる。
    drop(WIDGETS.with(Registry::take));
}

fn lookup(object: &DependencyObject) -> Option<Rc<FocusHandler>> {
    WIDGETS.with(|entries| {
        entries
            .borrow()
            .iter()
            .find(|(weak, _)| weak.upgrade().is_some_and(|other| same(&other, object)))
            .and_then(|(_, handler)| handler.upgrade())
    })
}

/// The registered widget containing `object`.
fn owner_of(object: Option<DependencyObject>) -> Option<Rc<FocusHandler>> {
    let mut current = object;
    while let Some(object) = current {
        if let Some(handler) = lookup(&object) {
            return Some(handler);
        }
        current = VisualTreeHelper::GetParent(&object).ok();
    }
    None
}

/// Attaches the focus watcher to a window root. Called every time the root is
/// recreated.
pub(crate) fn install(root: &UIElement) -> windows_core::Result<()> {
    let tracker = Rc::new(Tracker::default());
    TRACKERS.with(|trackers| trackers.borrow_mut().push(Rc::downgrade(&tracker)));
    let tracker = Arc::new(UiThreadCell::new(tracker));
    let handler = RoutedEventHandler::new(move |_, args| {
        let Some(tracker) = tracker.try_with_mut(|tracker| tracker.clone()) else {
            return Ok(());
        };
        let source = args
            .as_ref()
            .and_then(|args| args.OriginalSource().ok())
            .and_then(|source| source.cast::<DependencyObject>().ok());
        *tracker.focused.borrow_mut() = source.as_ref().and_then(|s| s.downgrade().ok());
        // 通知の中で要素を操作してもよいよう、借用の外で知らせる。
        tracker.owner.move_to(owner_of(source));
        Ok(())
    });
    root.GotFocus(&handler)?;
    Ok(())
}

/// Adds `on_focus_change` to text-input widgets. The `Inner` must hold a
/// `focus: Focus`.
macro_rules! impl_focus_change {
    ($t:ty) => {
        impl $t {
            /// Notified when focus enters (`true`) and leaves (`false`).
            ///
            /// Called only when focus moves within the window. Not called when the window
            /// itself becomes inactive.
            pub fn on_focus_change(&self, f: impl FnMut(bool) + 'static) {
                let element = <$t as crate::widgets::Widget>::native_element(self);
                self.0.focus.set(&element, f);
            }
        }
    };
}

pub(crate) use impl_focus_change;
