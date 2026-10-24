//! WinRT delegates require `Send + Sync`, but XAML events are always called on the
//! UI thread. So a cell that "panics if touched from anything other than the UI
//! thread" is provided, letting Rust closures that assume a single thread be placed
//! in them.
//!
//! A Windows-only receptacle needed to keep naui's public API in the same shape
//! (`impl FnMut() + 'static`) across all four backends.

use std::cell::RefCell;
use std::thread::ThreadId;

use crate::Slot;

/// The slot for one notification, placed in a WinRT delegate.
///
/// Delegates require `Send + Sync`, so it is wrapped in a [`UiThreadCell`], and
/// references survive even after unsubscribing, so it is counted with an `Arc`. The
/// function type inside is written by the user (as in
/// `HandlerCell<dyn FnMut(&str)>`).
pub(crate) type HandlerCell<F> = std::sync::Arc<UiThreadCell<Slot<F>>>;

/// A handler that receives one string.
///
/// WinRT delegates require `Send + Sync`, so it is placed in a [`UiThreadCell`].
/// The closure is taken out only for the duration of the call, so operating on the
/// same field from inside the notification does not double-borrow.
#[derive(Clone)]
pub(crate) struct TextHandler(HandlerCell<dyn FnMut(&str)>);

impl TextHandler {
    pub(crate) fn new() -> Self {
        Self(std::sync::Arc::new(UiThreadCell::new(None)))
    }

    pub(crate) fn set(&self, f: impl FnMut(&str) + 'static) {
        self.0.with_mut(|slot| *slot = Some(Box::new(f)));
    }

    pub(crate) fn emit(&self, text: &str) {
        let Some(Some(mut f)) = self.0.try_with_mut(|slot| slot.take()) else {
            return;
        };
        f(text);
        let _ = self.0.try_with_mut(|slot| {
            if slot.is_none() {
                *slot = Some(f);
            }
        });
    }
}

/// A cell whose contents can be touched only on the thread that created it.
pub(crate) struct UiThreadCell<T> {
    owner: ThreadId,
    value: RefCell<T>,
}

// SAFETY: `owner` と異なるスレッドからは `with_mut` が panic するため、
// 中身が別スレッドへ渡ることはない。WinRT のデリゲートに載せるためだけに
// Send/Sync を主張している。
unsafe impl<T> Send for UiThreadCell<T> {}
unsafe impl<T> Sync for UiThreadCell<T> {}

impl<T> UiThreadCell<T> {
    pub(crate) fn new(value: T) -> Self {
        Self {
            owner: std::thread::current().id(),
            value: RefCell::new(value),
        }
    }

    /// Borrows the contents mutably from the UI thread. Panics on any other thread.
    pub(crate) fn with_mut<R>(&self, f: impl FnOnce(&mut T) -> R) -> R {
        assert_eq!(
            std::thread::current().id(),
            self.owner,
            "naui: UI スレッド以外から UI を操作しました"
        );
        f(&mut self.value.borrow_mut())
    }

    /// Touches the contents safely from a WinRT callback. Returns `None` when on the
    /// wrong thread, or when it is already borrowed because of re-entrancy.
    ///
    /// Unwinding a panic out of a WinRT delegate crosses the ABI boundary and becomes an
    /// access violation on the Windows side, so use this one in event handling.
    pub(crate) fn try_with_mut<R>(&self, f: impl FnOnce(&mut T) -> R) -> Option<R> {
        if std::thread::current().id() != self.owner {
            return None;
        }
        let mut value = self.value.try_borrow_mut().ok()?;
        Some(f(&mut value))
    }

    /// For the one-time handoff of state at startup. Use it only for state moving
    /// between `Application::Start` and `OnLaunched`, not for UI objects. The caller
    /// guarantees there is no concurrent access.
    pub(crate) fn with_mut_cross_thread<R>(&self, f: impl FnOnce(&mut T) -> R) -> R {
        // SAFETY: この経路は起動時の直列な hand-off 専用で、通常の UI
        // イベントから同時にアクセスされないことを呼び出し側が保証する。
        unsafe { f(&mut *self.value.as_ptr()) }
    }
}

/// A per-widget registry placed in the UI thread's `thread_local!`.
///
/// It holds notification closures (and the widgets they capture). When the
/// `thread_local!` is destroyed at the end of the process, the contents are not
/// dropped but left leaked. Dropping them there would make the widgets' `Drop` touch
/// other `thread_local!`s that are already destroyed and panic, crashing the process
/// (`STATUS_STACK_BUFFER_OVERRUN`). If XAML has already been wound down, releasing
/// COM objects is risky too.
///
/// When you want the closures' `Drop` to run, [`take`](Self::take) them and drop
/// them on the shutdown path (before XAML's cleanup).
pub(crate) struct Registry<T>(RefCell<Vec<T>>);

impl<T> Registry<T> {
    pub(crate) const fn new() -> Self {
        Self(RefCell::new(Vec::new()))
    }

    /// Takes out all the contents. The table comes back empty, so they can be dropped
    /// outside the borrow.
    pub(crate) fn take(&self) -> Vec<T> {
        std::mem::take(&mut *self.0.borrow_mut())
    }
}

impl<T> std::ops::Deref for Registry<T> {
    type Target = RefCell<Vec<T>>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> Drop for Registry<T> {
    fn drop(&mut self) {
        std::mem::forget(std::mem::take(self.0.get_mut()));
    }
}
