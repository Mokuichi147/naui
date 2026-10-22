//! ウィンドウの通知で受け渡す値と、通知先の入れ物。

/// [`Window::on_close_request`] からの返事。
///
/// [`Window::on_close_request`]: ../../naui/struct.Window.html#method.on_close_request
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CloseResponse {
    /// そのまま閉じる。
    #[default]
    Close,
    /// 閉じない (保存の確認を出すなど、アプリが後で決める)。
    KeepOpen,
}

/// `on_close_request` の通知先を 1 つ持つ。各バックエンドで共有する。
///
/// 呼んでいる間はクロージャを取り出しておくので、通知の中から同じ
/// ウィンドウを操作したり、通知先を差し替えたりしてもよい。通知先が
/// 無いとき (と、入れ子で呼ばれたとき) は閉じる。
#[derive(Default)]
pub struct CloseHandler(std::cell::RefCell<crate::Slot<CloseCallback>>);

type CloseCallback = dyn FnMut() -> CloseResponse;

impl CloseHandler {
    pub fn set(&self, f: impl FnMut() -> CloseResponse + 'static) {
        *self.0.borrow_mut() = Some(Box::new(f));
    }

    pub fn ask(&self) -> CloseResponse {
        let Some(mut f) = self.0.borrow_mut().take() else {
            return CloseResponse::Close;
        };
        let response = f();
        let mut slot = self.0.borrow_mut();
        if slot.is_none() {
            *slot = Some(f);
        }
        response
    }
}

/// ウィンドウの表示の状態 (`Window::state` / `Window::set_state`)。
///
/// 環境によっては重なり得る (最大化したまま最小化した、など) が、naui は
/// 利用者から見えているほうを 1 つだけ返す。重なったときは
/// 最小化 → 全画面 → 最大化 の順に優先する。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum WindowState {
    /// 最大化・最小化・全画面のどれでもない。利用者が動かした位置と大きさのまま。
    #[default]
    Normal,
    /// 画面 (メニューバーやタスクバーを除く) いっぱいに広げている。
    Maximized,
    /// Dock やタスクバーへしまっている。
    Minimized,
    /// タイトルバーも出さずに画面全体を使っている。
    Fullscreen,
}

impl WindowState {
    /// 重なった状態から、利用者から見えているほうを選ぶ。
    pub fn from_flags(minimized: bool, fullscreen: bool, maximized: bool) -> Self {
        if minimized {
            Self::Minimized
        } else if fullscreen {
            Self::Fullscreen
        } else if maximized {
            Self::Maximized
        } else {
            Self::Normal
        }
    }
}

/// `on_state_change` の通知先と、最後に知らせた状態。各バックエンドで共有する。
///
/// ネイティブの通知は 1 回の変化で何度も届くことがある (大きさの変化と
/// 状態の変化が別々に来る、など) ので、前に知らせた状態と比べて、
/// 変わったときだけ呼ぶ。呼んでいる間はクロージャを取り出しておくので、
/// 通知の中から同じウィンドウを操作してもよい。
///
/// 通知の中で状態がさらに変わったとき (通知の中から `set_state` した、
/// など) は、その場では呼ばずに覚えておき、通知先が戻ってから最後の状態を
/// 知らせる。途中で元の状態へ戻っていれば知らせない。
#[derive(Default)]
pub struct StateHandler {
    /// 最後に届いた状態。
    last: std::cell::Cell<WindowState>,
    /// 最後に通知先へ渡した (か、`reset` で決めた) 状態。
    delivered: std::cell::Cell<WindowState>,
    /// 通知先を呼んでいる最中。
    notifying: std::cell::Cell<bool>,
    callback: std::cell::RefCell<crate::Slot<StateCallback>>,
}

type StateCallback = dyn FnMut(WindowState);

impl StateHandler {
    pub fn set(&self, f: impl FnMut(WindowState) + 'static) {
        *self.callback.borrow_mut() = Some(Box::new(f));
    }

    /// 最後に届いた状態。
    pub fn last(&self) -> WindowState {
        self.last.get()
    }

    /// 知らせずに、いまの状態として覚える (表示する前の状態など)。
    pub fn reset(&self, state: WindowState) {
        self.last.set(state);
        self.delivered.set(state);
    }

    /// `state` が前に知らせたものと違えば通知先を呼ぶ。
    ///
    /// 通知先の中から呼ばれたときは覚えるだけで、外側の呼び出しが
    /// 通知先から戻った後に知らせる。
    pub fn update(&self, state: WindowState) {
        self.last.set(state);
        if self.notifying.replace(true) {
            return;
        }
        // 通知先が panic しても、次の通知を止めないように戻す。
        struct Notifying<'a>(&'a std::cell::Cell<bool>);
        impl Drop for Notifying<'_> {
            fn drop(&mut self) {
                self.0.set(false);
            }
        }
        let _notifying = Notifying(&self.notifying);
        while self.delivered.get() != self.last.get() {
            let next = self.last.get();
            self.delivered.set(next);
            let Some(mut f) = self.callback.borrow_mut().take() else {
                continue;
            };
            f(next);
            let mut slot = self.callback.borrow_mut();
            if slot.is_none() {
                *slot = Some(f);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    #[test]
    fn picks_the_state_the_user_sees() {
        assert_eq!(
            WindowState::from_flags(false, false, false),
            WindowState::Normal
        );
        assert_eq!(
            WindowState::from_flags(false, false, true),
            WindowState::Maximized
        );
        assert_eq!(
            WindowState::from_flags(false, true, true),
            WindowState::Fullscreen
        );
        assert_eq!(
            WindowState::from_flags(true, true, true),
            WindowState::Minimized
        );
    }

    #[test]
    fn notifies_only_changes() {
        let handler = StateHandler::default();
        let seen = Rc::new(RefCell::new(Vec::new()));
        handler.set({
            let seen = seen.clone();
            move |state| seen.borrow_mut().push(state)
        });
        handler.update(WindowState::Normal);
        handler.update(WindowState::Maximized);
        handler.update(WindowState::Maximized);
        handler.reset(WindowState::Fullscreen);
        handler.update(WindowState::Fullscreen);
        handler.update(WindowState::Normal);
        assert_eq!(
            seen.borrow().as_slice(),
            [WindowState::Maximized, WindowState::Normal]
        );
        assert_eq!(handler.last(), WindowState::Normal);
    }

    /// 通知の中で状態が変わったら、戻った後に知らせる (失わない)。
    #[test]
    fn notifies_changes_made_inside_the_callback() {
        let handler = Rc::new(StateHandler::default());
        let seen = Rc::new(RefCell::new(Vec::new()));
        handler.set({
            let seen = seen.clone();
            let handler = Rc::downgrade(&handler);
            move |state| {
                seen.borrow_mut().push(state);
                if state == WindowState::Maximized {
                    if let Some(handler) = handler.upgrade() {
                        handler.update(WindowState::Normal);
                        assert_eq!(seen.borrow().len(), 1, "通知の中では呼ばない");
                    }
                }
            }
        });
        handler.update(WindowState::Maximized);
        assert_eq!(
            seen.borrow().as_slice(),
            [WindowState::Maximized, WindowState::Normal]
        );
        assert_eq!(handler.last(), WindowState::Normal);
    }

    /// 通知の中で変わって元へ戻ったときは、知らせ直さない。
    #[test]
    fn skips_changes_undone_inside_the_callback() {
        let handler = Rc::new(StateHandler::default());
        let seen = Rc::new(RefCell::new(Vec::new()));
        handler.set({
            let seen = seen.clone();
            let handler = Rc::downgrade(&handler);
            move |state| {
                seen.borrow_mut().push(state);
                if let Some(handler) = handler.upgrade() {
                    handler.update(WindowState::Normal);
                    handler.update(state);
                }
            }
        });
        handler.update(WindowState::Fullscreen);
        assert_eq!(seen.borrow().as_slice(), [WindowState::Fullscreen]);
    }

    /// 通知先が panic しても、次の変化は知らせる。
    #[test]
    fn keeps_notifying_after_a_panic() {
        let handler = StateHandler::default();
        handler.set(|state| {
            if state == WindowState::Maximized {
                panic!("通知先の失敗");
            }
        });
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            handler.update(WindowState::Maximized)
        }));
        assert!(result.is_err());
        let seen = Rc::new(RefCell::new(Vec::new()));
        handler.set({
            let seen = seen.clone();
            move |state| seen.borrow_mut().push(state)
        });
        handler.update(WindowState::Normal);
        assert_eq!(seen.borrow().as_slice(), [WindowState::Normal]);
    }
}
