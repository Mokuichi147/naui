use crate::sys;
use naui_core::ToastSpec;

/// 一時的な通知 (`AdwToast`)。
///
/// ウィジェットではないので、コンテナへは入れない (`Dialog` と同じ)。
#[derive(Clone)]
pub struct Toast(pub(crate) sys::Toast);

impl_backend!(Toast);

impl Toast {
    /// 出す文字列。出している間に呼ぶと、その場で書き換わる。
    #[inline]
    pub fn set_message(&self, message: &str) {
        self.0.set_message(message)
    }

    #[inline]
    pub fn message(&self) -> String {
        self.0.message()
    }

    /// 操作ボタンの文字列。**空文字列を渡すとボタンを外す。**
    #[inline]
    pub fn set_action(&self, label: &str) {
        self.0.set_action(label)
    }

    /// 操作ボタンの文字列。置いていなければ空文字列。
    #[inline]
    pub fn action(&self) -> String {
        self.0.action()
    }

    /// 自動で消えるまでの秒数。**0 を渡すと自動では消えない。**
    ///
    /// 次に [`show`](Self::show) したときから効く。
    #[inline]
    pub fn set_timeout(&self, seconds: f64) {
        self.0.set_timeout(seconds)
    }

    #[inline]
    pub fn timeout(&self) -> f64 {
        self.0.timeout()
    }

    /// いまの設定。
    #[inline]
    pub fn spec(&self) -> ToastSpec {
        self.0.spec()
    }

    /// 操作ボタンが押されたときに呼ばれる。設定し直すと以前のものは外れる。
    ///
    /// 押されるとトーストは消えるので、続けて `on_dismiss` も呼ばれる。
    #[inline]
    pub fn on_action(&self, f: impl FnMut() + 'static) {
        self.0.on_action(f)
    }

    /// 消えたときに呼ばれる。設定し直すと以前のものは外れる。
    ///
    /// 呼ばれるのは**時間で消えたとき**と**操作ボタンで消えたとき**。
    /// [`dismiss`](Self::dismiss) で消したときと、別のトーストに
    /// 置き換えられたときは呼ばれない (アプリ自身の操作は通知しない、
    /// という [`Dialog::close`](crate::Dialog::close) と同じ決まり)。
    #[inline]
    pub fn on_dismiss(&self, f: impl FnMut() + 'static) {
        self.0.on_dismiss(f)
    }

    /// トーストを出す。
    ///
    /// **同時に出るのは 1 つ**で、ほかのトーストが出ていれば置き換える
    /// (置き換えられたほうの `on_dismiss` は呼ばれない)。
    /// 出せるウィンドウがまだ無いときは何もしない。
    ///
    /// `AdwToastOverlay` は足されたトーストを順番待ちさせる作りだが、
    /// naui は 4 環境で同じにそろえるため、前のものを消してから足す。
    #[inline]
    pub fn show(&self) {
        self.0.show()
    }

    /// 出しているトーストを消す。`on_dismiss` は呼ばれない。
    #[inline]
    pub fn dismiss(&self) {
        self.0.dismiss()
    }

    /// いま出ているか。
    #[inline]
    pub fn is_visible(&self) -> bool {
        self.0.is_visible()
    }
}
