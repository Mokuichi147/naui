//! ウィンドウの通知で受け渡す値。

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
