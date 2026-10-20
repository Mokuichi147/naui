//! ウィジェットの弱参照。

use std::rc::Rc;

/// ウィジェットを強く保持せずに通知のクロージャから参照するための弱参照。
///
/// 各バックエンドのウィジェットの `downgrade()` が返す。ウィジェットの通知
/// (`on_change` など) のクロージャはウィジェットの中に持たれるので、
/// クロージャにウィジェット自身を `clone()` で捕まえると循環して解放されない。
/// 代わりにこれを捕まえ、使うときに [`upgrade`](Self::upgrade) する。
///
/// ```ignore
/// let weak = input.downgrade();
/// input.on_focus_change(move |focused| {
///     if let Some(input) = weak.upgrade() {
///         if !focused {
///             input.set_text(input.text().trim());
///         }
///     }
/// });
/// ```
pub struct WeakWidget<W>(Rc<dyn Fn() -> Option<W>>);

impl<W> WeakWidget<W> {
    /// バックエンドが作る。`upgrade` は中身がまだあればウィジェットを返す。
    #[doc(hidden)]
    pub fn new(upgrade: impl Fn() -> Option<W> + 'static) -> Self {
        Self(Rc::new(upgrade))
    }

    /// ウィジェットがまだあれば、それを指すハンドルを返す。
    ///
    /// ハンドルがすべて捨てられ、親のコンテナからも外れて解放された後は `None`。
    pub fn upgrade(&self) -> Option<W> {
        (self.0)()
    }
}

impl<W> Clone for WeakWidget<W> {
    fn clone(&self) -> Self {
        Self(Rc::clone(&self.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone)]
    struct Probe(Rc<()>);

    #[test]
    fn upgrade_returns_none_after_drop() {
        let probe = Probe(Rc::new(()));
        let inner = Rc::downgrade(&probe.0);
        let weak = WeakWidget::new(move || inner.upgrade().map(Probe));
        let copy = weak.clone();
        assert!(weak.upgrade().is_some());
        // 弱参照そのものは中身を保持しない。
        assert_eq!(Rc::strong_count(&probe.0), 1);
        drop(probe);
        assert!(weak.upgrade().is_none());
        assert!(copy.upgrade().is_none());
    }
}
