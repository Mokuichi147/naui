//! WinRT のデリゲートは `Send + Sync` を要求するが、XAML のイベントは
//! 必ず UI スレッドで呼ばれる。そこで「UI スレッド以外から触れたら panic する」
//! セルを用意し、シングルスレッド前提の Rust クロージャを載せられるようにする。
//!
//! naui の公開 API を 4 バックエンドで同じ形 (`impl FnMut() + 'static`) に
//! 保つために必要な、Windows だけの受け皿。

use std::cell::RefCell;
use std::thread::ThreadId;

use crate::Slot;

/// WinRT のデリゲートへ載せる、通知 1 つぶんの置き場。
///
/// デリゲートは `Send + Sync` を要求するので [`UiThreadCell`] で包み、
/// 購読を解いたあとも生き残る参照があるので `Arc` で数える。中身の関数型は
/// 使う側が書く (`HandlerCell<dyn FnMut(&str)>` のように)。
pub(crate) type HandlerCell<F> = std::sync::Arc<UiThreadCell<Slot<F>>>;

/// 生成したスレッドでのみ中身に触れるセル。
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

    /// UI スレッドから中身を可変で借りる。別スレッドなら panic する。
    pub(crate) fn with_mut<R>(&self, f: impl FnOnce(&mut T) -> R) -> R {
        assert_eq!(
            std::thread::current().id(),
            self.owner,
            "naui: UI スレッド以外から UI を操作しました"
        );
        f(&mut self.value.borrow_mut())
    }

    /// WinRT コールバックから安全に中身へ触れる。UI スレッド違い、または
    /// 再入で既に借用されている場合は `None` を返す。
    ///
    /// WinRT のデリゲートから panic を巻き戻すと、ABI の境界を越えて
    /// Windows 側のアクセス違反になるため、イベント処理ではこちらを使う。
    pub(crate) fn try_with_mut<R>(&self, f: impl FnOnce(&mut T) -> R) -> Option<R> {
        if std::thread::current().id() != self.owner {
            return None;
        }
        let mut value = self.value.try_borrow_mut().ok()?;
        Some(f(&mut value))
    }
}
