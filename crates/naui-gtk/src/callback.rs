//! A container that holds an app closure and calls it once when needed.
//!
//! GTK4 signal handlers require `Fn` (callable any number of times), but naui's API
//! takes `FnMut`. So the signal is connected exactly once at creation, and the
//! app's closure is kept here, swapped in and out.
//!
//! Its other role is guarding against re-entrancy: **the closure is taken out while
//! it is being called**. Even if the same widget is touched from inside the
//! notification, the `RefCell` is not double-borrowed (the same as what the macOS
//! backend does for AppKit re-entrancy).

use std::cell::RefCell;

use naui_core::{Error, FileEntry, SortOrder};

/// The slot for one closure.
type Slot<F> = RefCell<Option<Box<F>>>;

/// Takes it out, calls it, and puts it back unless it was replaced.
fn emit<F: ?Sized + FnMut(T), T>(slot: &Slot<F>, value: T) {
    let Some(mut f) = slot.borrow_mut().take() else {
        return;
    };
    f(value);
    let mut slot = slot.borrow_mut();
    // 呼び出しの中で差し替えられていたら、新しいほうを残す。
    if slot.is_none() {
        *slot = Some(f);
    }
}

/// A callback that receives one value.
pub(crate) struct Notifier<T>(Slot<dyn FnMut(T)>);

impl<T> Default for Notifier<T> {
    fn default() -> Self {
        Self(RefCell::new(None))
    }
}

impl<T> Notifier<T> {
    /// Discards the previous closure and holds the new one.
    pub(crate) fn set(&self, f: impl FnMut(T) + 'static) {
        *self.0.borrow_mut() = Some(Box::new(f));
    }

    /// Calls it. Does nothing if none is held.
    pub(crate) fn emit(&self, value: T) {
        emit(&self.0, value);
    }

    /// Whether a closure is held.
    pub(crate) fn is_set(&self) -> bool {
        self.0.borrow().is_some()
    }
}

/// A callback that receives a borrowed value.
///
/// Using [`Notifier`] with `&[usize]` and the like needs a lifetime parameter, so
/// one is provided per received type.
macro_rules! borrowed_notifier {
    ($(#[$meta:meta])* $name:ident, $arg:ty) => {
        $(#[$meta])*
        #[derive(Default)]
        pub(crate) struct $name(Slot<dyn FnMut($arg)>);

        impl $name {
            pub(crate) fn set(&self, f: impl FnMut($arg) + 'static) {
                *self.0.borrow_mut() = Some(Box::new(f));
            }

            pub(crate) fn emit(&self, value: $arg) {
                emit(&self.0, value);
            }
        }
    };
}

borrowed_notifier!(
    /// A callback that receives the indices of the selected rows.
    SelectionNotifier,
    &[usize]
);
borrowed_notifier!(
    /// A callback that receives the picked files.
    FileNotifier,
    &[FileEntry]
);
borrowed_notifier!(
    /// A callback that receives where the contents were written.
    SavedNotifier,
    &FileEntry
);
borrowed_notifier!(
    /// A callback that receives the details of a failure.
    ErrorNotifier,
    &Error
);
borrowed_notifier!(
    /// A callback that receives the entered text.
    TextNotifier,
    &str
);

/// A callback that receives the column whose sort changed and its direction.
///
/// It receives two values, so it is written by hand instead of with
/// [`borrowed_notifier!`].
#[derive(Default)]
pub(crate) struct SortNotifier(Slot<dyn FnMut(usize, SortOrder)>);

impl SortNotifier {
    pub(crate) fn set(&self, f: impl FnMut(usize, SortOrder) + 'static) {
        *self.0.borrow_mut() = Some(Box::new(f));
    }

    /// Takes out the closure only for the duration of the call (the same shape as
    /// [`emit`]).
    pub(crate) fn emit(&self, column: usize, order: SortOrder) {
        let Some(mut f) = self.0.borrow_mut().take() else {
            return;
        };
        f(column, order);
        let mut slot = self.0.borrow_mut();
        if slot.is_none() {
            *slot = Some(f);
        }
    }
}

/// A callback that receives the heading and item indices of the clicked item.
///
/// It receives two values, so it is written by hand instead of with
/// [`borrowed_notifier!`].
#[derive(Default)]
pub(crate) struct ActivateNotifier(Slot<dyn FnMut(usize, usize)>);

impl ActivateNotifier {
    pub(crate) fn set(&self, f: impl FnMut(usize, usize) + 'static) {
        *self.0.borrow_mut() = Some(Box::new(f));
    }

    /// Takes out the closure only for the duration of the call (the same shape as
    /// [`emit`]).
    pub(crate) fn emit(&self, menu: usize, item: usize) {
        let Some(mut f) = self.0.borrow_mut().take() else {
            return;
        };
        f(menu, item);
        let mut slot = self.0.borrow_mut();
        if slot.is_none() {
            *slot = Some(f);
        }
    }
}

/// A callback that receives the path of the item whose expansion changed and its
/// new state.
///
/// It receives two values, so it is written by hand instead of with
/// [`borrowed_notifier!`].
#[derive(Default)]
pub(crate) struct ExpandNotifier(Slot<dyn FnMut(&[usize], bool)>);

impl ExpandNotifier {
    pub(crate) fn set(&self, f: impl FnMut(&[usize], bool) + 'static) {
        *self.0.borrow_mut() = Some(Box::new(f));
    }

    /// Takes out the closure only for the duration of the call (the same shape as
    /// [`emit`]).
    pub(crate) fn emit(&self, path: &[usize], expanded: bool) {
        let Some(mut f) = self.0.borrow_mut().take() else {
            return;
        };
        f(path, expanded);
        let mut slot = self.0.borrow_mut();
        if slot.is_none() {
            *slot = Some(f);
        }
    }
}
