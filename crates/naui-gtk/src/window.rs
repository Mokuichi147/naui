//! トップレベルウィンドウ (`AdwApplicationWindow`)。
//!
//! `AdwApplicationWindow` は `GtkApplicationWindow` と違い、**既定の
//! タイトルバーを持たない**。最小化・最大化・閉じるのボタンは
//! `AdwHeaderBar` が出すので、中身をそのまま入れるのではなく
//! `AdwToolbarView` の上段にヘッダーバーを、下段にアプリの中身を置く。
//!
//! 下段はさらに `AdwToastOverlay` で包む。[`Toast`](crate::Toast) はここへ
//! 足され、ヘッダーバーより下・アプリの中身の上へ重なる (GNOME の作法)。
//!
//! [`Sidebar`] を付けると、ウィンドウの中身は `GtkPaned` になり、上の
//! `AdwToolbarView` はその右の区画へ移る。左の区画はサイドバー自身の
//! ヘッダーバーと一覧を持つ。

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use adw::prelude::*;
use gtk::glib;
use naui_core::{CloseHandler, CloseResponse, Result, Theme};

use crate::callback::Notifier;

use crate::menu_bar::MenuBar;
use crate::sidebar::Sidebar;
use crate::toolbar::Toolbar;
use crate::widgets::Widget;

pub(crate) struct WindowInner {
    native: adw::ApplicationWindow,
    /// アプリの中身と、そこへ重なるトーストの入れ物。
    overlay: adw::ToastOverlay,
    /// 上段 (ヘッダーバーとメニューバー) と下段 (アプリの中身) の入れ物。
    view: adw::ToolbarView,
    /// タイトルと、最小化・最大化・閉じるのボタン。
    header: adw::HeaderBar,
    child: RefCell<Option<Box<dyn Widget>>>,
    /// ヘッダーバーへ差し込んだツールバー。通知先ごと生かしておく。
    toolbar: RefCell<Option<Toolbar>>,
    /// ヘッダーバーの下へ差し込んだメニューバー。通知先ごと生かしておく。
    menu_bar: RefCell<Option<MenuBar>>,
    /// 取り付けたサイドバー。通知先ごと生かしておく。
    sidebar: RefCell<Option<Sidebar>>,
    on_close_request: CloseHandler,
    on_resize: Notifier<(f64, f64)>,
    /// 最後に知らせた (または最初の) [`Window::size`]。同じ大きさでは呼ばない。
    last_size: Cell<(f64, f64)>,
    /// 描き終わるたびに中身の大きさを比べる、フレームクロックの `after-paint`。
    after_paint: RefCell<Option<(gtk::gdk::FrameClock, glib::SignalHandlerId)>>,
    /// naui の [`Window::close`] の最中。GTK4 はプログラムから閉じても
    /// `close-request` を出すので、そのときは確認しない。
    closing: Cell<bool>,
    /// 最初の [`Window::show`] で合わせる中身の大きさ。`ui.window` の
    /// 幅・高さで始まり、表示前の `set_size` で置き換わる。表示したら空になる。
    initial_size: Cell<Option<(f64, f64)>>,
    /// 合わせ直しの途中の中身の大きさ ([`Window::fit_after_layout`])。
    fit: Cell<Option<Fit>>,
    /// 合わせ直しのフレームごとの呼び出しが付いている。
    fitting: Cell<bool>,
}

/// 中身の大きさの合わせ直しの途中の状態。
#[derive(Clone, Copy)]
struct Fit {
    /// 頼まれた中身の大きさ。
    target: (f64, f64),
    /// 最後に既定の大きさを頼んでから回ったフレームの数。
    ticks: u32,
    /// 合わせ直しを始めたフレームの時刻 (マイクロ秒)。最初のフレームで決まる。
    started: Option<i64>,
}

/// 既定の大きさを頼んでから、並び直した中身を測るまでのフレームの数。
/// 頼んだ次のフレームのレイアウトで並ぶので、その次のフレームで測る。
const FIT_SETTLE_TICKS: u32 = 2;
/// 頼んだ大きさにならないまま、同じ値を頼み直すまでのフレームの数。
const FIT_RETRY_TICKS: u32 = 6;
/// 合わせ直しを打ち切るまでの時間 (マイクロ秒)。利用者のリサイズと
/// 張り合い続けないよう、表示の直後だけに限る。
const FIT_LIMIT_US: i64 = 1_000_000;

/// トップレベルウィンドウ。
///
/// `run` に渡したコールバックの中で作る。フレームワーク
/// (`GtkApplication`) が参照を保持するので、戻り値を捨てても閉じられない。
#[derive(Clone)]
pub struct Window(Rc<WindowInner>);

/// ウィンドウを強く保持せずにイベントハンドラから参照するための弱参照。
#[derive(Clone)]
pub struct WeakWindow(Weak<WindowInner>);

impl WeakWindow {
    pub fn upgrade(&self) -> Option<Window> {
        self.0.upgrade().map(Window)
    }
}

impl Window {
    pub(crate) fn new(app: &adw::Application, title: &str, width: f64, height: f64) -> Self {
        // 既定の大きさはヘッダーバーを含むウィンドウ全体なので、中身の
        // 大きさから決め直す (下の `apply_default_size`)。
        let native = adw::ApplicationWindow::builder()
            .application(app)
            .title(title)
            .build();

        // ヘッダーバーは自分でタイトルを描かず、ウィンドウの `title` を映す。
        let header = adw::HeaderBar::new();
        let view = adw::ToolbarView::new();
        view.add_top_bar(&header);
        // GTK4 は既定でははみ出した中身を切り取らない。窓より中身が大きいとき
        // (縮めすぎたとき) に、ウィンドウの外へ描かれてしまうのを止める。
        view.set_overflow(gtk::Overflow::Hidden);
        // アプリの中身は、トーストを重ねられる入れ物ごしに置く。
        let overlay = adw::ToastOverlay::new();
        view.set_content(Some(&overlay));
        native.set_content(Some(&view));
        // `on_key_down` の受け口。通知先が無いあいだは素通りする。
        crate::keys::install(&native);

        let this = Self(Rc::new(WindowInner {
            native,
            overlay,
            view,
            header,
            child: RefCell::new(None),
            toolbar: RefCell::new(None),
            menu_bar: RefCell::new(None),
            sidebar: RefCell::new(None),
            on_close_request: CloseHandler::default(),
            on_resize: Notifier::default(),
            last_size: Cell::new((f64::from(to_px(width)), f64::from(to_px(height)))),
            after_paint: RefCell::new(None),
            closing: Cell::new(false),
            initial_size: Cell::new(None),
            fit: Cell::new(None),
            fitting: Cell::new(false),
        }));
        this.observe();
        this.apply_default_size((width.max(0.0), height.max(0.0)));
        this.0
            .initial_size
            .set(Some((width.max(0.0), height.max(0.0))));
        this
    }

    /// 中身を `size` にするよう、ウィンドウの既定の大きさを決める。
    ///
    /// GTK4 の既定の大きさはウィンドウ全体 (影は除く) で、ヘッダーバーと
    /// メニューバーの帯とウィンドウ自身の枠も含むので、それを足す。帯の高さは見積もりの
    /// ことがあるので、表示に関わるときは [`fit_after_layout`](Self::fit_after_layout)
    /// で並んだ後の実寸を確かめる。
    fn apply_default_size(&self, size: (f64, f64)) {
        let (width, height) = self.default_size_for(size);
        self.0.native.set_default_size(width, height);
    }

    /// 中身を `(width, height)` にするための、ウィンドウの既定の大きさ。
    fn default_size_for(&self, (width, height): (f64, f64)) -> (i32, i32) {
        let bars = self.top_bars_height();
        let (frame_width, frame_height) = self.frame_size();
        (
            to_px(width + frame_width),
            to_px(height + bars + frame_height),
        )
    }

    /// ウィンドウ自身の CSS の枠 (padding と border) の幅と高さ。
    ///
    /// 既定の大きさはウィンドウの border box で、中身はその内側に並ぶ。
    /// コンポジタが無い X11 では、影の代わりに `solid-csd` の padding が
    /// 付く (Adwaita では上下左右 5px)。このクラスは realize で付くので、
    /// 並ぶ前は realize してあれば CSS から読み、それより前は 0 とする。
    fn frame_size(&self) -> (f64, f64) {
        let native = &self.0.native;
        let (width, height) = (native.width(), native.height());
        if width <= 0 || height <= 0 {
            if !native.is_realized() {
                return (0.0, 0.0);
            }
            // CSS の枠を読む API は GtkStyleContext にしか無い。
            #[allow(deprecated)]
            let (padding, border) = {
                let style = native.style_context();
                (style.padding(), style.border())
            };
            let sum = |a: i16, b: i16, c: i16, d: i16| {
                f64::from(i32::from(a) + i32::from(b) + i32::from(c) + i32::from(d)).max(0.0)
            };
            return (
                sum(
                    padding.left(),
                    padding.right(),
                    border.left(),
                    border.right(),
                ),
                sum(
                    padding.top(),
                    padding.bottom(),
                    border.top(),
                    border.bottom(),
                ),
            );
        }
        native.compute_bounds(native).map_or((0.0, 0.0), |bounds| {
            (
                (f64::from(bounds.width()) - f64::from(width)).max(0.0),
                (f64::from(bounds.height()) - f64::from(height)).max(0.0),
            )
        })
    }

    /// 上段の帯 (ヘッダーバーとメニューバー) の高さ。
    ///
    /// 並べ終わっていれば、ウィンドウの中身と `AdwToastOverlay` の高さの差。
    /// 表示中に付けたばかりのメニューバーはまだ並んでいないので、測った
    /// 高さを足す (外した直後とツールバーでヘッダーバーの高さが変わった
    /// 直後は、並び直すまで前の高さのまま)。並ぶ前は帯を 1 本ずつ測った
    /// 見積もり (CSS が当たる前は低めに出る)。
    fn top_bars_height(&self) -> f64 {
        let content = self
            .0
            .native
            .content()
            .map_or(0, |content| content.height());
        let overlay = self.0.overlay.height();
        let menu_bar: Option<gtk::Widget> = self
            .0
            .menu_bar
            .borrow()
            .as_ref()
            .map(|menu_bar| menu_bar.mount().upcast());
        if content > 0 && overlay > 0 {
            let unplaced = menu_bar
                .filter(|bar| bar.height() <= 0)
                .map_or(0, |bar| bar.measure(gtk::Orientation::Vertical, -1).1);
            return f64::from((content - overlay).max(0) + unplaced.max(0));
        }
        let header: gtk::Widget = self.0.header.clone().upcast();
        let bars = std::iter::once(header)
            .chain(menu_bar)
            .map(|bar| bar.measure(gtk::Orientation::Vertical, -1).1)
            .sum::<i32>();
        f64::from(bars.max(0))
    }

    /// 中身が `target` になるまで、並んだ後の実寸で合わせ直す。
    ///
    /// 既定の大きさに足した帯の高さは、並ぶ前や帯を付け外した直後には
    /// 見積もりなので外れることがある。また Wayland では、表示の直後に
    /// 頼んだ大きさが、後から届くコンポジタの configure (前の大きさのまま)
    /// で上書きされることがある。GTK4 には大きさが決まったことの通知が
    /// 無いので、フレームごとに測り、帯の高さが違えば頼み直し、頼んだ
    /// 大きさにならなければしばらく待って同じ値を頼み直す。
    /// [`FIT_LIMIT_US`] で打ち切る。
    fn fit_after_layout(&self, target: (f64, f64)) {
        self.0.fit.set(Some(Fit {
            target,
            ticks: 0,
            started: None,
        }));
        // 呼び出しが付いていれば、それが新しい大きさを拾う。
        if self.0.fitting.replace(true) {
            return;
        }
        let weak = Rc::downgrade(&self.0);
        self.0.native.add_tick_callback(move |_, clock| {
            let Some(inner) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            let window = Window(inner);
            if window.step_fit(clock.frame_time()) {
                glib::ControlFlow::Continue
            } else {
                window.0.fitting.set(false);
                glib::ControlFlow::Break
            }
        });
    }

    /// 合わせ直しを 1 フレーム進める。続けるなら `true`。
    fn step_fit(&self, now: i64) -> bool {
        let Some(mut fit) = self.0.fit.take() else {
            return false;
        };
        let native = &self.0.native;
        let started = *fit.started.get_or_insert(now);
        // 最大化・全画面の間はウィンドウの大きさを頼めない。
        if now - started > FIT_LIMIT_US || native.is_maximized() || native.is_fullscreen() {
            return false;
        }
        fit.ticks += 1;
        if fit.ticks < FIT_SETTLE_TICKS || self.0.overlay.height() <= 0 {
            self.0.fit.set(Some(fit));
            return true;
        }
        let (width, height) = (to_px(fit.target.0), to_px(fit.target.1));
        if self.size() == (f64::from(width), f64::from(height)) {
            return false;
        }
        let want = self.default_size_for(fit.target);
        if native.default_size() != want || fit.ticks >= FIT_RETRY_TICKS {
            native.set_default_size(want.0, want.1);
            fit.ticks = 0;
        }
        self.0.fit.set(Some(fit));
        true
    }

    fn observe(&self) {
        let weak = Rc::downgrade(&self.0);
        self.0.native.connect_close_request(move |_| {
            let Some(inner) = weak.upgrade() else {
                return glib::Propagation::Proceed;
            };
            if inner.closing.get() || inner.on_close_request.ask() == CloseResponse::Close {
                glib::Propagation::Proceed
            } else {
                glib::Propagation::Stop
            }
        });
        // 中身の大きさが配られるのはレイアウトの中なので、描き終わった後に
        // 比べる。利用者のドラッグ・最大化・画面端へのスナップ・`set_size`・
        // メニューバーやサイドバーの付け外しのどれで変わっても届く。
        // (`default-width` は最大化やスナップでは変わらず、変わった直後には
        // まだ中身へ配られていないので、それを合図にはしない。)
        let weak = Rc::downgrade(&self.0);
        self.0.native.connect_realize(move |native| {
            let Some(clock) = native.frame_clock() else {
                return;
            };
            let weak = weak.clone();
            let Some(inner) = weak.upgrade() else {
                return;
            };
            let handler = clock.connect_after_paint(move |_| {
                if let Some(inner) = weak.upgrade() {
                    Window(inner).report_size();
                }
            });
            if let Some((old, id)) = inner.after_paint.replace(Some((clock, handler))) {
                old.disconnect(id);
            }
        });
        let weak = Rc::downgrade(&self.0);
        self.0.native.connect_unrealize(move |_| {
            if let Some((clock, id)) = weak.upgrade().and_then(|i| i.after_paint.take()) {
                clock.disconnect(id);
            }
        });
        // 画面に出ていないウィンドウは描かれないので、`set_size` で指定した
        // 大きさ (= `size`) をループを 1 周させてから知らせる。出ているときは
        // まだ前の大きさを返すので、ここでは何も起きず、描いた後に届く。
        for property in ["default-width", "default-height"] {
            let weak = Rc::downgrade(&self.0);
            self.0
                .native
                .connect_notify_local(Some(property), move |_, _| {
                    let weak = weak.clone();
                    glib::idle_add_local_once(move || {
                        if let Some(inner) = weak.upgrade() {
                            Window(inner).report_size();
                        }
                    });
                });
        }
    }

    /// いまの [`size`](Self::size) が前に知らせたものと違えば `on_resize` を呼ぶ。
    fn report_size(&self) {
        let size = self.size();
        if self.0.last_size.replace(size) != size {
            self.0.on_resize.emit(size);
        }
    }

    /// 利用者がウィンドウを閉じようとしたときの確認。
    ///
    /// [`CloseResponse::KeepOpen`] を返すと閉じない (保存していない変更が
    /// あるときに確認のダイアログを出す、など)。プログラムからの
    /// [`close`](Self::close) では呼ばれない。
    pub fn on_close_request(&self, f: impl FnMut() -> CloseResponse + 'static) {
        self.0.on_close_request.set(f);
    }

    /// 中身の領域の大きさ (幅, 高さ)。単位は論理ピクセル。ヘッダーバーと
    /// メニューバーは含まない。サイドバーを付けているときは、ほかの環境と
    /// 同じくサイドバーの幅も含む。
    ///
    /// 画面に出る前は、`ui.window` か [`set_size`](Self::set_size) で指定した大きさ。
    pub fn size(&self) -> (f64, f64) {
        if let Some(size) = self.0.initial_size.get() {
            return size;
        }
        let width = self.0.native.content().map_or(0, |content| content.width());
        let height = self.0.overlay.height();
        if width > 0 && height > 0 {
            return (f64::from(width), f64::from(height));
        }
        let (width, height) = self.0.native.default_size();
        let bars = self.top_bars_height();
        let (frame_width, frame_height) = self.frame_size();
        (
            (f64::from(width.max(0)) - frame_width).max(0.0),
            (f64::from(height.max(0)) - bars - frame_height).max(0.0),
        )
    }

    /// 大きさが変わったときの通知。変わった後の [`size`](Self::size) が届く。
    pub fn on_resize(&self, f: impl FnMut((f64, f64)) + 'static) {
        self.0.on_resize.set(f);
    }

    /// 対応する GTK4 のウィンドウ。バックエンド固有の脱出口として公開している。
    pub fn native_window(&self) -> adw::ApplicationWindow {
        self.0.native.clone()
    }

    /// タイトルと最小化・最大化・閉じるのボタンを持つヘッダーバー。
    ///
    /// バックエンド固有の脱出口として公開している。
    pub fn native_header_bar(&self) -> adw::HeaderBar {
        self.0.header.clone()
    }

    /// トーストが重なる `AdwToastOverlay`。
    ///
    /// バックエンド固有の脱出口として公開している。
    pub fn native_toast_overlay(&self) -> adw::ToastOverlay {
        self.0.overlay.clone()
    }

    pub fn downgrade(&self) -> WeakWindow {
        WeakWindow(Rc::downgrade(&self.0))
    }

    /// このウィンドウの中で押されたキーの通知。
    ///
    /// フォーカスのあるウィジェットの `on_key_down` が `Continue` を返した
    /// キーが届く。`Handled` を返すと、そのキーはウィジェットへ渡らない。
    /// IME で変換している間のキーは届かない。
    pub fn on_key_down(
        &self,
        f: impl FnMut(&naui_core::KeyEvent) -> naui_core::EventResponse + 'static,
    ) {
        crate::keys::set_window_handler(self.0.native.upcast_ref(), f);
    }

    pub fn set_title(&self, title: &str) {
        self.0.native.set_title(Some(title));
    }

    pub fn title(&self) -> String {
        self.0
            .native
            .title()
            .map(|t| t.to_string())
            .unwrap_or_default()
    }

    /// 利用者が縮められる下限 (中身の幅, 高さ)。単位は [`set_size`](Self::set_size) と同じ。
    ///
    /// 小さくしすぎてレイアウトが崩れるのを防ぐ。
    ///
    /// GTK4 のウィンドウは中身の最小の大きさより縮まないので、中身の入れ物に
    /// 最小の大きさを付ける (ヘッダーバーの分はその上に足される)。
    /// サイドバーを付けている間は、幅の下限はサイドバーの右の区画に効く
    /// (サイドバーの幅だけ広いところで止まる)。
    pub fn set_min_size(&self, width: f64, height: f64) {
        self.0
            .overlay
            .set_size_request(to_px(width.max(0.0)), to_px(height.max(0.0)));
    }

    /// 中身の領域の大きさを変える。単位と範囲は [`size`](Self::size) と同じで、
    /// ヘッダーバーとメニューバーは含まない (ほかの環境と同じ)。
    ///
    /// GTK4 の `set_default_size` はウィンドウ全体の大きさを取るので、
    /// ヘッダーバーとメニューバーの帯の高さを足して渡す。画面に出る前に
    /// 呼んだときは、表示するときにこの大きさで出す。
    pub fn set_size(&self, width: f64, height: f64) {
        let size = (width.max(0.0), height.max(0.0));
        self.apply_default_size(size);
        if self.0.initial_size.get().is_some() {
            // 表示するときに `show` が合わせる。
            self.0.initial_size.set(Some(size));
        } else {
            // 帯を付け外した直後は、足した帯の高さが前のままなので、
            // 並び直した後の実寸で確かめる。
            self.fit_after_layout(size);
        }
    }

    /// ウィンドウの中身を差し替える。
    pub fn set_child(&self, child: &dyn Widget) {
        let bin = child.size_bin();
        // ウィンドウの中身は、他のバックエンドと同じく窓いっぱいに広がる。
        bin.fill_parent();
        // ヘッダーバーの下が、アプリの中身の置き場になる。
        // 直接ではなく、トーストを重ねる入れ物ごしに入れる。
        self.0.overlay.set_child(Some(&bin));
        *self.0.child.borrow_mut() = Some(child.boxed_clone());
    }

    /// ウィンドウの左に付けるサイドバー。呼ぶたびに置き換わる。
    ///
    /// ウィンドウの中身を `GtkPaned` へ差し替え、これまでの中身
    /// (ヘッダーバー・メニューバー・子) はその右の区画へ移す。
    pub fn set_sidebar(&self, sidebar: &Sidebar) {
        self.clear_sidebar();
        let paned = sidebar.native_paned();
        self.0.native.set_content(None::<&gtk::Widget>);
        paned.set_end_child(Some(&self.0.view));
        self.0.native.set_content(Some(&paned));
        *self.0.sidebar.borrow_mut() = Some(sidebar.clone());
        self.mount_header_start();
        sidebar.set_content_header(Some(&self.0.header));
    }

    /// 取り付けたサイドバーを外す。付いていなければ何もしない。
    ///
    /// 右の区画にあった中身は、ウィンドウの中身へ戻る。
    pub fn clear_sidebar(&self) {
        let Some(old) = self.0.sidebar.borrow_mut().take() else {
            return;
        };
        let paned = old.native_paned();
        self.0.native.set_content(None::<&gtk::Widget>);
        paned.set_end_child(None::<&gtk::Widget>);
        self.0.native.set_content(Some(&self.0.view));
        let slot = old.content_slot();
        if slot.parent().is_some() {
            self.0.header.remove(&slot);
        }
        old.set_content_header(None);
    }

    /// ヘッダーバーの左側を、サイドバーボタンの置き場 → ツールバーの順に
    /// 並べ直す。
    ///
    /// 置き場にはサイドバーを閉じている間だけサイドバーボタンが入る。
    /// `pack_start` は後ろへ足していくので、先に付いていたほうを外してから
    /// 並べる (置き場はいつも左端)。
    fn mount_header_start(&self) {
        let sidebar = self.0.sidebar.borrow().clone();
        let toolbar = self.0.toolbar.borrow().clone();
        if let Some(sidebar) = &sidebar {
            let slot = sidebar.content_slot();
            if slot.parent().is_some() {
                self.0.header.remove(&slot);
            }
        }
        if let Some(toolbar) = &toolbar {
            let mount = toolbar.mount();
            if mount.parent().is_some() {
                self.0.header.remove(&mount);
            }
        }
        if let Some(sidebar) = &sidebar {
            self.0.header.pack_start(&sidebar.content_slot());
        }
        if let Some(toolbar) = &toolbar {
            self.0.header.pack_start(&toolbar.mount());
        }
    }

    /// ウィンドウの上端に付けるツールバー。呼ぶたびに置き換わる。
    ///
    /// GNOME の作法どおり、項目はヘッダーバーの左側へ並ぶ。
    pub fn set_toolbar(&self, toolbar: &Toolbar) {
        self.clear_toolbar();
        *self.0.toolbar.borrow_mut() = Some(toolbar.clone());
        self.mount_header_start();
    }

    /// 取り付けたツールバーを外す。付いていなければ何もしない。
    pub fn clear_toolbar(&self) {
        if let Some(old) = self.0.toolbar.borrow_mut().take() {
            self.0.header.remove(&old.mount());
        }
    }

    /// ウィンドウの上端に付けるメニューバー。呼ぶたびに置き換わる。
    ///
    /// `GtkPopoverMenuBar` はヘッダーバーの下へ入る。項目の操作は
    /// ウィンドウの操作の組として入れるので、`GtkApplication` へ登録した
    /// アクセラレータ (ショートカット) はメニューを開かなくても効く。
    pub fn set_menu_bar(&self, menu_bar: &MenuBar) {
        self.clear_menu_bar();
        self.0.view.add_top_bar(&menu_bar.mount());
        self.0
            .native
            .insert_action_group(&menu_bar.group(), Some(&menu_bar.action_group()));
        menu_bar.attach();
        *self.0.menu_bar.borrow_mut() = Some(menu_bar.clone());
    }

    /// 取り付けたメニューバーを外す。付いていなければ何もしない。
    pub fn clear_menu_bar(&self) {
        let old = self.0.menu_bar.borrow_mut().take();
        if let Some(old) = old {
            self.0.view.remove(&old.mount());
            self.0
                .native
                .insert_action_group(&old.group(), None::<&gtk::gio::SimpleActionGroup>);
            // アプリ全体のアクセラレータの表からも外す。
            old.detach();
        }
    }

    /// 画面に出して前面へ持ってくる。
    ///
    /// 最初の表示では、中身を `ui.window` (か表示前の [`set_size`](Self::set_size))
    /// の大きさにする。
    pub fn show(&self) {
        if let Some(size) = self.0.initial_size.take() {
            // メニューバーは表示の前に付くことが多いので、ここで測り直す。
            // ウィンドウの枠のクラス (`solid-csd` など) は realize で付くので
            // 先に realize しておく。並ぶ前の見積もりなので、並んだ後に
            // 合わせ直す。
            WidgetExt::realize(&self.0.native);
            self.apply_default_size(size);
            self.fit_after_layout(size);
        }
        self.0.native.present();
    }

    pub fn close(&self) {
        self.0.closing.set(true);
        self.0.native.close();
        self.0.closing.set(false);
    }

    pub fn is_visible(&self) -> bool {
        WidgetExt::is_visible(&self.0.native)
    }

    /// このウィンドウに配色テーマを適用する。
    ///
    /// libadwaita のテーマはアプリ全体で 1 つなので、実際にはアプリ全体に効く。
    pub fn set_theme(&self, theme: Theme) -> Result<()> {
        crate::apply_theme(theme);
        Ok(())
    }
}

/// `window` に載っている `AdwToastOverlay`。naui が作ったウィンドウでなければ
/// `None`。
///
/// [`Toast`](crate::Toast) は「いちばん手前のウィンドウ」へ出すので、
/// `GtkApplication` からたどったウィンドウを、naui が組んだ構造
/// (`AdwApplicationWindow` → `AdwToolbarView` → `AdwToastOverlay`) に沿って
/// 下りる。サイドバーを付けていれば、間に `GtkPaned` の右の区画が挟まる。
pub(crate) fn toast_overlay(window: &gtk::Window) -> Option<adw::ToastOverlay> {
    let window = window.clone().downcast::<adw::ApplicationWindow>().ok()?;
    let mut content = window.content()?;
    if let Some(paned) = content.downcast_ref::<gtk::Paned>() {
        content = paned.end_child()?;
    }
    let view = content.downcast::<adw::ToolbarView>().ok()?;
    view.content()?.downcast::<adw::ToastOverlay>().ok()
}

fn to_px(value: f64) -> i32 {
    value.round().clamp(1.0, i32::MAX as f64) as i32
}
