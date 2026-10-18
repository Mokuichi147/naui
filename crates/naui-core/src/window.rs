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
#[derive(Default)]
pub struct StateHandler {
    last: std::cell::Cell<WindowState>,
    callback: std::cell::RefCell<crate::Slot<StateCallback>>,
}

type StateCallback = dyn FnMut(WindowState);

impl StateHandler {
    pub fn set(&self, f: impl FnMut(WindowState) + 'static) {
        *self.callback.borrow_mut() = Some(Box::new(f));
    }

    /// 最後に知らせた状態。
    pub fn last(&self) -> WindowState {
        self.last.get()
    }

    /// 知らせずに、いまの状態として覚える (表示する前の状態など)。
    pub fn reset(&self, state: WindowState) {
        self.last.set(state);
    }

    /// `state` が前に知らせたものと違えば通知先を呼ぶ。
    pub fn update(&self, state: WindowState) {
        if self.last.replace(state) == state {
            return;
        }
        let Some(mut f) = self.callback.borrow_mut().take() else {
            return;
        };
        f(state);
        let mut slot = self.callback.borrow_mut();
        if slot.is_none() {
            *slot = Some(f);
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
}
