use crate::sys;
use crate::Widget;
use crate::{MenuBar, Sidebar, Toolbar};
use naui_core::{CloseResponse, EventResponse, KeyEvent, Result, Theme, WindowState};

/// ウィンドウを強く保持せずにイベントハンドラから参照するための弱参照。
#[derive(Clone)]
pub struct WeakWindow(pub(crate) sys::WeakWindow);

impl_backend!(WeakWindow);

impl WeakWindow {
    #[inline]
    pub fn upgrade(&self) -> Option<Window> {
        self.0.upgrade().map(Window)
    }
}

/// トップレベルウィンドウ。
///
/// `run` に渡したコールバックの中で作る。フレームワーク
/// (`GtkApplication`) が参照を保持するので、戻り値を捨てても閉じられない。
#[derive(Clone)]
pub struct Window(pub(crate) sys::Window);

impl_backend!(Window);

impl Window {
    /// 利用者がウィンドウを閉じようとしたときの確認。
    ///
    /// [`CloseResponse::KeepOpen`] を返すと閉じない (保存していない変更が
    /// あるときに確認のダイアログを出す、など)。プログラムからの
    /// [`close`](Self::close) では呼ばれない。
    #[inline]
    pub fn on_close_request(&self, f: impl FnMut() -> CloseResponse + 'static) {
        self.0.on_close_request(f)
    }

    /// 中身の領域の大きさ (幅, 高さ)。単位は論理ピクセル。ヘッダーバーと
    /// メニューバーは含まない。サイドバーを付けているときは、ほかの環境と
    /// 同じくサイドバーの幅も含む。
    ///
    /// 画面に出る前は、`ui.window` か [`set_size`](Self::set_size) で指定した大きさ。
    #[inline]
    pub fn size(&self) -> (f64, f64) {
        self.0.size()
    }

    /// ウィンドウの左上の位置。**Linux では取れないので、いつも `None`。**
    ///
    /// GTK4 にはウィンドウの位置を読む API も動かす API も無い。Wayland では
    /// アプリがウィンドウの位置を知る仕組みそのものが無く (置き場所は
    /// コンポジタが決める)、GTK4 はそれに合わせて X11 でも外した。
    #[inline]
    pub fn position(&self) -> Option<(f64, f64)> {
        self.0.position()
    }

    /// ウィンドウを動かす。**Linux では何もしない** ([`position`](Self::position) を参照)。
    #[inline]
    pub fn set_position(&self, x: f64, y: f64) {
        self.0.set_position(x, y)
    }

    /// いまの表示の状態。
    ///
    /// コンポジタから届いた面の状態 (`GdkToplevelState`) を読む。表示する前と、
    /// 表示してからコンポジタの最初の状態が届くまでは、[`set_state`](Self::set_state)
    /// で頼んだ状態 (既定は [`WindowState::Normal`])。
    ///
    /// **Wayland では最小化を知る仕組みが無い**ので、最小化しても
    /// [`WindowState::Minimized`] にはならない (しまう前の状態を返す)。
    #[inline]
    pub fn state(&self) -> WindowState {
        self.0.state()
    }

    /// 表示の状態を変える。表示する前に呼んだときは、表示するときに当てる。
    ///
    /// | 頼んだ状態 | GTK4 |
    /// | --- | --- |
    /// | `Normal` | `unfullscreen` / `unmaximize` / `unminimize` と `present` |
    /// | `Maximized` | `maximize` |
    /// | `Minimized` | `minimize` |
    /// | `Fullscreen` | `fullscreen` |
    ///
    /// どれもコンポジタへの頼みごとで、`state` が変わるのはコンポジタが
    /// 応えた後になる (変わったら [`on_state_change`](Self::on_state_change)
    /// が呼ばれる)。全画面からほかの状態へ移るときは、全画面を出終わって
    /// から当てる。最小化から戻すときは、`unminimize` に加えて `present` で
    /// 前面へ出すよう頼む (X11 のウィンドウマネージャは `unminimize` だけでは
    /// 戻さないことがある)。**Wayland では最小化から戻せるとは限らない**
    /// (戻す仕組みが無く、`present` に応えるかはコンポジタ次第)。
    ///
    /// コンポジタが応える前に続けて頼んだときは、まだ応えていない頼みを
    /// 取り消してから次を頼むので、最後に頼んだ状態になる。
    ///
    /// 表示する前の最小化だけは、出す前に頼んでもウィンドウマネージャが
    /// 応えないので、表示してコンポジタから最初の状態が届いてから当てる
    /// (一瞬だけ画面に出る)。
    #[inline]
    pub fn set_state(&self, state: WindowState) {
        self.0.set_state(state)
    }

    /// 表示の状態が変わったときの通知。変わった後の [`state`](Self::state) が届く。
    ///
    /// 利用者の操作 (ヘッダーバーのボタン、画面端へのスナップ、など) でも
    /// [`set_state`](Self::set_state) でも呼ばれる。
    #[inline]
    pub fn on_state_change(&self, f: impl FnMut(WindowState) + 'static) {
        self.0.on_state_change(f)
    }

    /// 利用者がウィンドウの縁をつかんで大きさを変えられるか。既定は `true`。
    #[inline]
    pub fn is_resizable(&self) -> bool {
        self.0.is_resizable()
    }

    /// 利用者が大きさを変えられるかどうかを決める。
    ///
    /// `gtk_window_set_resizable` を使う。外すとヘッダーバーの最大化の
    /// ボタンも消える。[`set_size`](Self::set_size) はどちらでも効く。
    #[inline]
    pub fn set_resizable(&self, resizable: bool) {
        self.0.set_resizable(resizable)
    }

    /// 大きさが変わったときの通知。変わった後の [`size`](Self::size) が届く。
    #[inline]
    pub fn on_resize(&self, f: impl FnMut((f64, f64)) + 'static) {
        self.0.on_resize(f)
    }

    #[inline]
    pub fn downgrade(&self) -> WeakWindow {
        WeakWindow(self.0.downgrade())
    }

    /// このウィンドウの中で押されたキーの通知。
    ///
    /// フォーカスのあるウィジェットの `on_key_down` が `Continue` を返した
    /// キーが届く。`Handled` を返すと、そのキーはウィジェットへ渡らない。
    /// IME で変換している間のキーは届かない。
    #[inline]
    pub fn on_key_down(&self, f: impl FnMut(&KeyEvent) -> EventResponse + 'static) {
        self.0.on_key_down(f)
    }

    #[inline]
    pub fn set_title(&self, title: &str) {
        self.0.set_title(title)
    }

    #[inline]
    pub fn title(&self) -> String {
        self.0.title()
    }

    /// 利用者が縮められる下限 (中身の幅, 高さ)。単位は [`set_size`](Self::set_size) と同じ。
    ///
    /// 小さくしすぎてレイアウトが崩れるのを防ぐ。
    ///
    /// GTK4 のウィンドウは中身の最小の大きさより縮まないので、中身の入れ物に
    /// 最小の大きさを付ける (ヘッダーバーの分はその上に足される)。
    /// サイドバーを付けている間は、幅の下限はサイドバーの右の区画に効く
    /// (サイドバーの幅だけ広いところで止まる)。
    #[inline]
    pub fn set_min_size(&self, width: f64, height: f64) {
        self.0.set_min_size(width, height)
    }

    /// 中身の領域の大きさを変える。単位と範囲は [`size`](Self::size) と同じで、
    /// ヘッダーバーとメニューバーは含まない (ほかの環境と同じ)。
    ///
    /// GTK4 の `set_default_size` はウィンドウ全体の大きさを取るので、
    /// ヘッダーバーとメニューバーの帯の高さを足して渡す。画面に出る前に
    /// 呼んだときは、表示するときにこの大きさで出す。
    #[inline]
    pub fn set_size(&self, width: f64, height: f64) {
        self.0.set_size(width, height)
    }

    /// ウィンドウの中身を差し替える。
    #[inline]
    pub fn set_child(&self, child: &dyn Widget) {
        self.0.set_child(child.as_backend())
    }

    /// ウィンドウの左に付けるサイドバー。呼ぶたびに置き換わる。
    ///
    /// ウィンドウの中身を `GtkPaned` へ差し替え、これまでの中身
    /// (ヘッダーバー・メニューバー・子) はその右の区画へ移す。
    #[inline]
    pub fn set_sidebar(&self, sidebar: &Sidebar) {
        self.0.set_sidebar(&sidebar.0)
    }

    /// 取り付けたサイドバーを外す。付いていなければ何もしない。
    ///
    /// 右の区画にあった中身は、ウィンドウの中身へ戻る。
    #[inline]
    pub fn clear_sidebar(&self) {
        self.0.clear_sidebar()
    }

    /// ウィンドウの上端に付けるツールバー。呼ぶたびに置き換わる。
    ///
    /// GNOME の作法どおり、項目はヘッダーバーの左側へ並ぶ。
    #[inline]
    pub fn set_toolbar(&self, toolbar: &Toolbar) {
        self.0.set_toolbar(&toolbar.0)
    }

    /// 取り付けたツールバーを外す。付いていなければ何もしない。
    #[inline]
    pub fn clear_toolbar(&self) {
        self.0.clear_toolbar()
    }

    /// ウィンドウの上端に付けるメニューバー。呼ぶたびに置き換わる。
    ///
    /// `GtkPopoverMenuBar` はヘッダーバーの下へ入る。項目の操作は
    /// ウィンドウの操作の組として入れるので、`GtkApplication` へ登録した
    /// アクセラレータ (ショートカット) はメニューを開かなくても効く。
    #[inline]
    pub fn set_menu_bar(&self, menu_bar: &MenuBar) {
        self.0.set_menu_bar(&menu_bar.0)
    }

    /// 取り付けたメニューバーを外す。付いていなければ何もしない。
    #[inline]
    pub fn clear_menu_bar(&self) {
        self.0.clear_menu_bar()
    }

    /// 画面に出して前面へ持ってくる。
    ///
    /// 最初の表示では、中身を `ui.window` (か表示前の [`set_size`](Self::set_size))
    /// の大きさにし、表示する前に [`set_state`](Self::set_state) で頼んだ状態にする。
    #[inline]
    pub fn show(&self) {
        self.0.show()
    }

    #[inline]
    pub fn close(&self) {
        self.0.close()
    }

    #[inline]
    pub fn is_visible(&self) -> bool {
        self.0.is_visible()
    }

    /// このウィンドウに配色テーマを適用する。
    ///
    /// libadwaita のテーマはアプリ全体で 1 つなので、実際にはアプリ全体に効く。
    #[inline]
    pub fn set_theme(&self, theme: Theme) -> Result<()> {
        self.0.set_theme(theme)
    }
}
