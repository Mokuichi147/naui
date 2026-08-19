//! アプリのクロージャを保持し、必要なときに 1 回だけ呼ぶための入れ物。
//!
//! GTK4 のシグナルハンドラは `Fn` (何度でも呼べる) を要求するが、naui の API は
//! `FnMut` を受ける。そこでシグナルは作った時点で 1 回だけつなぎ、アプリの
//! クロージャはここへ差し替える形で持つ。
//!
//! もう 1 つの役目は再入への備えで、**呼び出している間はクロージャを取り出して
//! おく**。通知の中から同じウィジェットを触られても `RefCell` が二重借用に
//! ならない (macOS バックエンドが AppKit の再入で行っているのと同じ)。

use std::cell::RefCell;

use naui_core::FileEntry;

/// クロージャ 1 つぶんの置き場。
type Slot<F> = RefCell<Option<Box<F>>>;

/// 取り出して呼び、差し替えられていなければ戻す。
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

/// 値を 1 つ受け取るコールバック。
pub(crate) struct Notifier<T>(Slot<dyn FnMut(T)>);

impl<T> Default for Notifier<T> {
    fn default() -> Self {
        Self(RefCell::new(None))
    }
}

impl<T> Notifier<T> {
    /// 以前のものを捨てて、新しいクロージャを持つ。
    pub(crate) fn set(&self, f: impl FnMut(T) + 'static) {
        *self.0.borrow_mut() = Some(Box::new(f));
    }

    /// 呼ぶ。持っていなければ何もしない。
    pub(crate) fn emit(&self, value: T) {
        emit(&self.0, value);
    }
}

/// 借用した値を受け取るコールバック。
///
/// [`Notifier`] を `&[usize]` などで使うには寿命の引数が要るので、
/// 受け取る型ごとに用意する。
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
    /// 選ばれた行のインデックスを受け取るコールバック。
    SelectionNotifier,
    &[usize]
);
borrowed_notifier!(
    /// 選ばれたファイルを受け取るコールバック。
    FileNotifier,
    &[FileEntry]
);
borrowed_notifier!(
    /// 入力された文字列を受け取るコールバック。
    TextNotifier,
    &str
);
