//! NSWindow のハンドル。

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use dispatch2::{DispatchQueue, MainThreadBound};
use naui_core::{CloseHandler, CloseResponse, Result, StateHandler, Theme, WindowState};
use objc2::rc::Retained;
use objc2::runtime::{NSObjectProtocol, ProtocolObject};
use objc2::{define_class, msg_send, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::NSWindowDelegate;
use objc2_app_kit::{
    NSAppearance, NSAppearanceCustomization, NSAppearanceNameAqua, NSAppearanceNameDarkAqua,
    NSApplication, NSScreen, NSSplitViewController, NSView, NSWindow, NSWindowCollectionBehavior,
    NSWindowStyleMask, NSWindowTitleVisibility,
};
use objc2_foundation::{NSNotification, NSObject, NSPoint, NSRect, NSSize, NSString};

use crate::menu_bar::MenuBar;
use crate::sidebar::Sidebar;
use crate::toolbar::Toolbar;
use crate::trampoline::ValueHandler;
use crate::widgets::Widget;

thread_local! {
    /// naui が作ったウィンドウ。作った順に並ぶ。
    ///
    /// `NSApplication` の `windows` には AppKit が内部で作る、画面に出ない
    /// ウィンドウも混ざる。[`Toast`](crate::Toast) の出し先を選ぶときに
    /// そちらを掴まないよう、naui のぶんだけ覚えておく。
    /// `Ui` がウィンドウを持ち続けるので、ここで保持しても寿命は変わらない。
    static WINDOWS: RefCell<Vec<Retained<NSWindow>>> = const { RefCell::new(Vec::new()) };
}

struct WindowInner {
    native: Retained<NSWindow>,
    /// ルートの子を保持し、トランポリンごと生かしておく。
    child: RefCell<Option<Box<dyn Widget>>>,
    /// 取り付けたツールバー。`NSWindow` の toolbar は強参照だが、
    /// naui 側のハンドル (トランポリンと通知先) もここで生かしておく。
    toolbar: RefCell<Option<Toolbar>>,
    /// 取り付けたメニューバー。`NSApplication` が `mainMenu` を強参照するが、
    /// naui 側のハンドル (トランポリンと通知先) もここで生かしておく。
    menu_bar: RefCell<Option<MenuBar>>,
    /// 取り付けたサイドバー。`contentViewController` として強参照されるが、
    /// naui 側のハンドル (データソースと通知先) もここで生かしておく。
    sidebar: RefCell<Option<Sidebar>>,
    on_close_request: CloseHandler,
    on_resize: ValueHandler<(f64, f64)>,
    /// `set_min_size` で頼まれた中身の下限。サイドバーやツールバーで
    /// タイトルバーと重なる分が変わるたびに、足し直して渡す。
    min_size: Cell<Option<(f64, f64)>>,
    /// `NSWindow` の delegate は弱参照なので、ここで生かしておく。
    delegate: RefCell<Option<Retained<WindowDelegate>>>,
    on_state_change: StateHandler,
    /// 一度でも [`Window::show`] したか。表示する前の `set_state` は覚えて
    /// おき、表示するときに当てる。
    shown: Cell<bool>,
    /// 表示する前に頼まれた状態。
    initial_state: Cell<WindowState>,
    /// 全画面へ入る・出るアニメーションの途中。途中で頼まれた状態は
    /// [`after_transition`](Self::after_transition) へ回す。
    in_transition: Cell<bool>,
    /// 全画面に入り終わっている。styleMask の `FullScreen` はアニメーションの
    /// 始まりで変わるので、終わりの通知で決めたこちらを `state` に使う。
    fullscreen: Cell<bool>,
    /// 全画面のアニメーションが終わってから当てる状態。全画面から最大化や
    /// 最小化へ移るときは、まず全画面を出て、出終わってから当てる。
    after_transition: Cell<Option<WindowState>>,
    /// `set_resizable` で頼まれた値。全画面の間は styleMask を触らず、
    /// 出たときに当て直す。
    resizable: Cell<bool>,
    /// `set_position` で位置を決めた。決めた後の `set_size` では中央へ寄せない。
    positioned: Cell<bool>,
    /// `zoom:` で最大化する前の枠 (AppKit の座標)。最大化の間の
    /// [`Window::position`] はこの左上を返す。`windowShouldZoom:toFrame:` で読む。
    zoom_frame: Cell<Option<NSRect>>,
    /// 全画面の間に [`Window::position`] が返す、全画面を出たときに戻る位置。
    /// 全画面へ入るときに読んでおく。
    fullscreen_return: Cell<Option<(f64, f64)>>,
    /// 最大化・全画面の間に [`Window::set_position`] で頼まれた位置。最大化は
    /// 元に戻した後、全画面は出たところで当てる。
    pending_position: Cell<Option<(f64, f64)>>,
}

define_class!(
    /// 利用者が閉じようとしたことと、大きさの変化を受け取る。
    ///
    /// `NSWindow::close` (naui の [`Window::close`]) は `windowShouldClose:`
    /// を通らないので、確認はタイトルバーのボタンや ⌘W のときだけ起きる。
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "NauiWindowDelegate"]
    #[ivars = Weak<WindowInner>]
    struct WindowDelegate;

    unsafe impl NSObjectProtocol for WindowDelegate {}

    unsafe impl NSWindowDelegate for WindowDelegate {
        #[unsafe(method(windowShouldClose:))]
        fn window_should_close(&self, _sender: &NSWindow) -> bool {
            self.ivars()
                .upgrade()
                .is_none_or(|inner| inner.on_close_request.ask() == CloseResponse::Close)
        }

        #[unsafe(method(windowDidResize:))]
        fn window_did_resize(&self, _notification: &NSNotification) {
            if let Some(inner) = self.ivars().upgrade() {
                let window = Window(inner.clone());
                let size = window.size();
                inner.on_resize.emit(size);
                // 最大化 (zoom) は大きさの変化としてだけ届く。
                window.report_state();
            }
        }

        #[unsafe(method(windowDidMiniaturize:))]
        fn window_did_miniaturize(&self, _notification: &NSNotification) {
            if let Some(inner) = self.ivars().upgrade() {
                Window(inner).report_state();
            }
        }

        #[unsafe(method(windowDidDeminiaturize:))]
        fn window_did_deminiaturize(&self, _notification: &NSNotification) {
            if let Some(inner) = self.ivars().upgrade() {
                Window(inner).report_state();
            }
        }

        #[unsafe(method(windowShouldZoom:toFrame:))]
        fn window_should_zoom(&self, _window: &NSWindow, _new_frame: NSRect) -> bool {
            self.ivars()
                .upgrade()
                .is_none_or(|inner| Window(inner).should_zoom())
        }

        #[unsafe(method(windowWillEnterFullScreen:))]
        fn window_will_enter_full_screen(&self, _notification: &NSNotification) {
            if let Some(inner) = self.ivars().upgrade() {
                let window = Window(inner.clone());
                // 最大化していなければ、前に頼まれた位置は使われずに残ったもの。
                if !inner.native.isZoomed() {
                    inner.pending_position.set(None);
                }
                inner.fullscreen_return.set(Some(window.normal_position()));
                inner.in_transition.set(true);
            }
        }

        #[unsafe(method(windowDidEnterFullScreen:))]
        fn window_did_enter_full_screen(&self, _notification: &NSNotification) {
            if let Some(inner) = self.ivars().upgrade() {
                Window(inner).finish_transition();
            }
        }

        #[unsafe(method(windowDidFailToEnterFullScreen:))]
        fn window_did_fail_to_enter_full_screen(&self, _window: &NSWindow) {
            if let Some(inner) = self.ivars().upgrade() {
                Window(inner).finish_transition();
            }
        }

        #[unsafe(method(windowWillExitFullScreen:))]
        fn window_will_exit_full_screen(&self, _notification: &NSNotification) {
            if let Some(inner) = self.ivars().upgrade() {
                inner.in_transition.set(true);
            }
        }

        #[unsafe(method(windowDidExitFullScreen:))]
        fn window_did_exit_full_screen(&self, _notification: &NSNotification) {
            if let Some(inner) = self.ivars().upgrade() {
                Window(inner).finish_transition();
            }
        }

        #[unsafe(method(windowDidFailToExitFullScreen:))]
        fn window_did_fail_to_exit_full_screen(&self, _window: &NSWindow) {
            if let Some(inner) = self.ivars().upgrade() {
                Window(inner).finish_transition();
            }
        }
    }
);

impl WindowDelegate {
    fn new(mtm: MainThreadMarker, inner: Weak<WindowInner>) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(inner);
        unsafe { msg_send![super(this), init] }
    }
}

/// トップレベルウィンドウ (NSWindow)。
#[derive(Clone)]
pub struct Window(Rc<WindowInner>);

/// ウィンドウを強く保持せずにイベントハンドラから参照するための弱参照。
#[derive(Clone)]
pub struct WeakWindow(Weak<WindowInner>);

impl WeakWindow {
    /// ウィンドウがまだ生きていれば強参照へ戻す。
    pub fn upgrade(&self) -> Option<Window> {
        self.0.upgrade().map(Window)
    }
}

impl Window {
    /// イベントハンドラなどへ渡しても所有権循環を作らない参照を返す。
    pub fn downgrade(&self) -> WeakWindow {
        WeakWindow(Rc::downgrade(&self.0))
    }

    pub(crate) fn new(mtm: MainThreadMarker, title: &str, width: f64, height: f64) -> Self {
        let native = crate::focus::new_window(
            mtm,
            NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(width, height)),
            NSWindowStyleMask::Titled
                | NSWindowStyleMask::Closable
                | NSWindowStyleMask::Miniaturizable
                | NSWindowStyleMask::Resizable,
        );
        native.setTitle(&NSString::from_str(title));
        // Rust 側が Retained を持つので、閉じたときの自動解放は切る
        // (切らないと二重解放になる)。
        unsafe { native.setReleasedWhenClosed(false) };
        // `toggleFullScreen:` で全画面にできるよう、緑のボタンと同じ扱いの
        // ウィンドウにする (大きさを変えられるウィンドウの既定と同じ)。
        native.setCollectionBehavior(
            native.collectionBehavior() | NSWindowCollectionBehavior::FullScreenPrimary,
        );
        native.center();

        WINDOWS.with(|slot| slot.borrow_mut().push(native.clone()));

        let this = Self(Rc::new(WindowInner {
            native,
            child: RefCell::new(None),
            toolbar: RefCell::new(None),
            menu_bar: RefCell::new(None),
            sidebar: RefCell::new(None),
            on_close_request: CloseHandler::default(),
            on_resize: ValueHandler::default(),
            min_size: Cell::new(None),
            delegate: RefCell::new(None),
            on_state_change: StateHandler::default(),
            shown: Cell::new(false),
            initial_state: Cell::new(WindowState::Normal),
            in_transition: Cell::new(false),
            fullscreen: Cell::new(false),
            after_transition: Cell::new(None),
            resizable: Cell::new(true),
            positioned: Cell::new(false),
            zoom_frame: Cell::new(None),
            fullscreen_return: Cell::new(None),
            pending_position: Cell::new(None),
        }));
        let delegate = WindowDelegate::new(mtm, Rc::downgrade(&this.0));
        this.0
            .native
            .setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
        *this.0.delegate.borrow_mut() = Some(delegate);
        this
    }

    /// 利用者がウィンドウを閉じようとしたときの確認。
    ///
    /// [`CloseResponse::KeepOpen`] を返すと閉じない (保存していない変更が
    /// あるときに確認のダイアログを出す、など)。プログラムからの
    /// [`close`](Self::close) では呼ばれない。
    pub fn on_close_request(&self, f: impl FnMut() -> CloseResponse + 'static) {
        self.0.on_close_request.set(f);
    }

    /// 中身の領域の大きさ (幅, 高さ)。単位は論理ピクセル。タイトルバーとツールバーは含まない。
    ///
    /// サイドバーを付けている間は `fullSizeContentView` で `contentView` が
    /// タイトルバーの下まで伸びるので、`contentView` ではなく、タイトルバーと
    /// ツールバーに隠れない `contentLayoutRect` の大きさを返す。
    pub fn size(&self) -> (f64, f64) {
        let size = self.0.native.contentLayoutRect().size;
        (size.width, size.height)
    }

    /// `contentView` のうちタイトルバーとツールバーに重なる分 (幅, 高さ)。
    ///
    /// `setContentSize` などは `contentView` の大きさを取るので、中身の大きさに
    /// これを足して渡す。`fullSizeContentView` でなければ 0。
    fn title_bar_overlap(&self) -> (f64, f64) {
        let native = &self.0.native;
        let content = native.contentRectForFrameRect(native.frame()).size;
        let layout = native.contentLayoutRect().size;
        (
            (content.width - layout.width).max(0.0),
            (content.height - layout.height).max(0.0),
        )
    }

    /// `change` の前後で中身の大きさ ([`size`](Self::size)) を保つ。上端は動かさない。
    ///
    /// ツールバーやサイドバーを付け外しすると、AppKit は中身の大きさを保たない。
    /// ツールバーを付けると中身がタイトルバーの高さだけ伸び (macOS 27 で確認)、
    /// サイドバーは枠を保つので、タイトルバーの下に隠れる分だけ中身が縮む。
    /// `ui.window` や `set_size` で決めた大きさのままにするため、変わった分を戻す。
    /// 最大化と全画面の間は画面に合わせた大きさなので、そのままにする。
    fn keeping_size(&self, change: impl FnOnce()) {
        let (width, height) = self.size();
        change();
        let native = &self.0.native;
        if self.size() == (width, height)
            || native.isZoomed()
            || native.styleMask().contains(NSWindowStyleMask::FullScreen)
        {
            return;
        }
        let (extra_width, extra_height) = self.title_bar_overlap();
        let frame = native.frame();
        let mut target = native.frameRectForContentRect(NSRect::new(
            frame.origin,
            NSSize::new(width + extra_width, height + extra_height),
        ));
        // AppKit の座標は下から上へ数えるので、上端をそろえるには下端をずらす。
        target.origin.y = frame.origin.y + frame.size.height - target.size.height;
        native.setFrame_display(target, true);
    }

    /// `set_min_size` の下限を、タイトルバーと重なる分を足して渡す。
    fn apply_min_size(&self) {
        let Some((width, height)) = self.0.min_size.get() else {
            return;
        };
        let (extra_width, extra_height) = self.title_bar_overlap();
        self.0.native.setContentMinSize(NSSize::new(
            width.max(0.0) + extra_width,
            height.max(0.0) + extra_height,
        ));
    }

    /// 大きさが変わったときの通知。変わった後の [`size`](Self::size) が届く。
    pub fn on_resize(&self, f: impl FnMut((f64, f64)) + 'static) {
        self.0.on_resize.set(f);
    }

    /// このウィンドウの中で押されたキーの通知。
    ///
    /// フォーカスのあるウィジェットの `on_key_down` が `Continue` を返した
    /// キーが届く。`Handled` を返すと、そのキーは AppKit へ渡らない。
    /// IME で変換している間のキーは届かない。
    pub fn on_key_down(
        &self,
        f: impl FnMut(&naui_core::KeyEvent) -> naui_core::EventResponse + 'static,
    ) {
        crate::keys::set_window_handler(&self.native_window(), f);
    }

    pub fn set_title(&self, title: &str) {
        self.0.native.setTitle(&NSString::from_str(title));
    }

    pub fn title(&self) -> String {
        self.0.native.title().to_string()
    }

    /// 利用者が縮められる下限 (中身の幅, 高さ)。単位は [`set_size`](Self::set_size) と同じ。
    ///
    /// 小さくしすぎてレイアウトが崩れるのを防ぐ。
    pub fn set_min_size(&self, width: f64, height: f64) {
        self.0.min_size.set(Some((width, height)));
        self.apply_min_size();
    }

    /// 中身の領域の大きさを変える。単位と範囲は [`size`](Self::size) と同じで、
    /// タイトルバーとツールバーは含まない (ほかの環境と同じ)。
    ///
    /// `setContentSize` を使う。サイドバーを付けている間は `contentView` が
    /// タイトルバーの下まで伸びているので、重なる分を足して渡す。
    pub fn set_size(&self, width: f64, height: f64) {
        let (extra_width, extra_height) = self.title_bar_overlap();
        self.0.native.setContentSize(NSSize::new(
            width.max(0.0) + extra_width,
            height.max(0.0) + extra_height,
        ));
        if !self.0.positioned.get() {
            self.0.native.center();
        }
    }

    /// ウィンドウの左上の位置 (x, y)。単位は論理ピクセル (ポイント)。
    ///
    /// 主画面 (メニューバーのある画面) の左上を原点に、右と下へ数える
    /// (ほかの環境と同じ向き)。AppKit の座標は主画面の左下から上へ数える
    /// ので、主画面の高さから引いて向きを変える。タイトルバーを含む
    /// ウィンドウの枠の左上で、`None` にはならない。
    ///
    /// 最大化・最小化・全画面の間は、通常の状態へ戻ったときの位置を返す。
    /// 最小化の間も `frame` は元の枠のまま。最大化は `zoom:` する前の枠、
    /// 全画面は入る前に読んでおいた位置。
    pub fn position(&self) -> Option<(f64, f64)> {
        if self.0.fullscreen.get() || self.0.in_transition.get() {
            if let Some(position) = self
                .0
                .pending_position
                .get()
                .or(self.0.fullscreen_return.get())
            {
                return Some(position);
            }
        }
        Some(self.normal_position())
    }

    /// 全画面を除いて、通常の状態へ戻ったときの位置。
    fn normal_position(&self) -> (f64, f64) {
        if let Some(frame) = self.zoom_frame() {
            return self
                .0
                .pending_position
                .get()
                .unwrap_or_else(|| self.top_left(frame));
        }
        self.top_left(self.0.native.frame())
    }

    /// `zoom:` で最大化している間の、最大化する前の枠。
    ///
    /// 利用者が大きさを画面いっぱいに合わせたときなど、`zoom:` を通らずに
    /// `isZoomed` になったときは `None` (見えている枠をそのまま扱う)。
    fn zoom_frame(&self) -> Option<NSRect> {
        self.0.zoom_frame.get().filter(|_| self.0.native.isZoomed())
    }

    /// AppKit の枠を、主画面の左上から数えた左上の位置へ直す。
    fn top_left(&self, frame: NSRect) -> (f64, f64) {
        (
            frame.origin.x,
            self.primary_screen_height() - (frame.origin.y + frame.size.height),
        )
    }

    /// ウィンドウの左上を (x, y) へ動かす。単位と向きは [`position`](Self::position) と同じ。
    ///
    /// `setFrameTopLeftPoint:` を使う。位置を決めた後の
    /// [`set_size`](Self::set_size) は、ウィンドウを画面の中央へ寄せ直さない。
    /// 画面の外を指すと、表示するときに AppKit がタイトルバーの見える
    /// ところまで戻すことがある。
    ///
    /// 最大化・最小化・全画面の間は状態をそのままにして、通常の状態へ戻った
    /// ときの位置を決める。最小化の間は `frame` を動かせば戻る先になり、
    /// 最大化は元に戻した後に、全画面は出たところで動かす。
    pub fn set_position(&self, x: f64, y: f64) {
        self.0.positioned.set(true);
        if self.0.fullscreen.get() || self.0.in_transition.get() || self.zoom_frame().is_some() {
            self.0.pending_position.set(Some((x, y)));
            return;
        }
        self.0.pending_position.set(None);
        self.0
            .native
            .setFrameTopLeftPoint(NSPoint::new(x, self.primary_screen_height() - y));
    }

    /// `zoom:` (緑のボタンやタイトルバーのダブルクリックも含む) で最大化する・
    /// 元に戻す直前に呼ばれる。
    ///
    /// 最大化するときは、元に戻す先として今の枠を覚える。元に戻すときに
    /// 最大化の間に頼まれた位置があれば、AppKit が元の枠へ戻し終えてから
    /// その位置へ動かす。`false` を返して AppKit の代わりに枠を動かすと、
    /// AppKit は最大化したままだと覚えていて、次に全画面を出たときに最大化の
    /// 枠へ戻してしまう (macOS 15 で確認)。`zoom:` はアニメーションが終わって
    /// から戻るので、メインキューの次の周回で動かす。
    fn should_zoom(&self) -> bool {
        let native = &self.0.native;
        if !native.isZoomed() {
            self.0.zoom_frame.set(Some(native.frame()));
            self.0.pending_position.set(None);
            return true;
        }
        self.0.zoom_frame.set(None);
        if self.0.pending_position.get().is_some() {
            let mtm = MainThreadMarker::from(&**native);
            let weak = MainThreadBound::new(Rc::downgrade(&self.0), mtm);
            DispatchQueue::main().exec_async(move || {
                // メインキューの仕事はメインスレッドで走る。
                let mtm = MainThreadMarker::new().expect("メインキューの外で呼ばれた");
                if let Some(inner) = weak.into_inner(mtm).upgrade() {
                    Window(inner).apply_pending_position();
                }
            });
        }
        true
    }

    /// 最大化・全画面の間に頼まれた位置を、通常の枠へ戻っていれば当てる。
    fn apply_pending_position(&self) {
        if self.0.fullscreen.get() || self.0.in_transition.get() || self.0.native.isZoomed() {
            return;
        }
        if let Some((x, y)) = self.0.pending_position.take() {
            self.set_position(x, y);
        }
    }

    /// 主画面の高さ。画面がつながっていなければ 0 (位置は AppKit の値のまま)。
    fn primary_screen_height(&self) -> f64 {
        let mtm = MainThreadMarker::from(&*self.0.native);
        NSScreen::screens(mtm)
            .firstObject()
            .map_or(0.0, |screen| screen.frame().size.height)
    }

    /// いまの表示の状態。
    ///
    /// 表示する前は、[`set_state`](Self::set_state) で頼んだ状態 (既定は
    /// [`WindowState::Normal`])。全画面へ入る・出る途中は、まだ前の状態を返す。
    pub fn state(&self) -> WindowState {
        if !self.0.shown.get() {
            return self.0.initial_state.get();
        }
        let native = &self.0.native;
        WindowState::from_flags(
            native.isMiniaturized(),
            self.0.fullscreen.get(),
            native.isZoomed(),
        )
    }

    /// 表示の状態を変える。表示する前に呼んだときは、表示するときに当てる。
    ///
    /// | 頼んだ状態 | AppKit |
    /// | --- | --- |
    /// | `Normal` | 最小化・全画面・zoom をすべて解く |
    /// | `Maximized` | `zoom:` (緑のボタンを Option を押しながら押したときと同じ) |
    /// | `Minimized` | `miniaturize:` |
    /// | `Fullscreen` | `toggleFullScreen:` |
    ///
    /// 全画面へ入る・出るのはアニメーションの後なので、`state` が変わるのも
    /// その後になる (変わったら [`on_state_change`](Self::on_state_change) が
    /// 呼ばれる)。全画面から最大化や最小化へ移るときは、全画面を出終わって
    /// から当てる。
    pub fn set_state(&self, state: WindowState) {
        if !self.0.shown.get() {
            self.0.initial_state.set(state);
            return;
        }
        self.apply_state(state);
    }

    fn apply_state(&self, target: WindowState) {
        if self.0.in_transition.get() {
            self.0.after_transition.set(Some(target));
            return;
        }
        let native = &self.0.native;
        if native.isMiniaturized() && target != WindowState::Minimized {
            native.deminiaturize(None);
        }
        let fullscreen = native.styleMask().contains(NSWindowStyleMask::FullScreen);
        match target {
            WindowState::Fullscreen => {
                if !fullscreen {
                    native.toggleFullScreen(None);
                }
            }
            // 全画面のままでは zoom も最小化もできないので、先に出る。
            _ if fullscreen => {
                self.0.after_transition.set(Some(target));
                native.toggleFullScreen(None);
            }
            WindowState::Maximized => {
                if !native.isZoomed() {
                    native.zoom(None);
                }
            }
            WindowState::Normal => {
                if native.isZoomed() {
                    native.zoom(None);
                    // `zoom:` はアニメーションの後に戻るので、ここで当てておけば
                    // `set_state` から戻ったときには頼まれた位置になっている。
                    self.apply_pending_position();
                }
            }
            WindowState::Minimized => native.miniaturize(None),
        }
        self.report_state();
    }

    /// 全画面へ入る・出るアニメーションが終わった (か失敗した)。
    ///
    /// 続けて当てる状態があれば、AppKit が通知を配り終えた後 (メインキューの
    /// 次の周回) に当てる。通知の中から `toggleFullScreen:` を呼ぶと無視される。
    fn finish_transition(&self) {
        let native = &self.0.native;
        self.0.in_transition.set(false);
        self.0
            .fullscreen
            .set(native.styleMask().contains(NSWindowStyleMask::FullScreen));
        self.apply_resizable();
        if !self.0.fullscreen.get() {
            self.0.fullscreen_return.set(None);
            // 全画面の間に頼まれた位置は、元の枠へ戻ったところで当てる
            // (最大化のまま戻ったときは、元に戻したときの位置になる)。
            self.apply_pending_position();
        }
        self.report_state();
        if self.0.after_transition.get().is_none() {
            return;
        }
        let mtm = MainThreadMarker::from(&**native);
        let weak = MainThreadBound::new(Rc::downgrade(&self.0), mtm);
        DispatchQueue::main().exec_async(move || {
            // メインキューの仕事はメインスレッドで走る。
            let mtm = MainThreadMarker::new().expect("メインキューの外で呼ばれた");
            let Some(inner) = weak.into_inner(mtm).upgrade() else {
                return;
            };
            if let Some(next) = inner.after_transition.take() {
                Window(inner).apply_state(next);
            }
        });
    }

    /// 表示の状態が変わったときの通知。変わった後の [`state`](Self::state) が届く。
    ///
    /// 利用者の操作 (緑のボタン、Dock へしまう、など) でも
    /// [`set_state`](Self::set_state) でも呼ばれる。
    pub fn on_state_change(&self, f: impl FnMut(WindowState) + 'static) {
        self.0.on_state_change.set(f);
    }

    /// いまの状態が前に知らせたものと違えば `on_state_change` を呼ぶ。
    ///
    /// 全画面へ入る・出る途中は、枠が画面いっぱいに広がって zoom と見分けが
    /// 付かないので知らせない (終わったときに知らせる)。
    fn report_state(&self) {
        if self.0.shown.get() && !self.0.in_transition.get() {
            self.0.on_state_change.update(self.state());
        }
    }

    /// 利用者がウィンドウの縁をつかんで大きさを変えられるか。既定は `true`。
    pub fn is_resizable(&self) -> bool {
        self.0.resizable.get()
    }

    /// 利用者が大きさを変えられるかどうかを決める。
    ///
    /// styleMask の `Resizable` を付け外しする。外すと緑のボタンでの zoom も
    /// できなくなる。[`set_size`](Self::set_size) はどちらでも効く。
    /// 全画面の間は styleMask を変えられないので、全画面を出たときに当てる。
    pub fn set_resizable(&self, resizable: bool) {
        self.0.resizable.set(resizable);
        self.apply_resizable();
    }

    fn apply_resizable(&self) {
        let native = &self.0.native;
        let mask = native.styleMask();
        if self.0.in_transition.get() || mask.contains(NSWindowStyleMask::FullScreen) {
            return;
        }
        let mask = if self.0.resizable.get() {
            mask | NSWindowStyleMask::Resizable
        } else {
            mask & !NSWindowStyleMask::Resizable
        };
        native.setStyleMask(mask);
    }

    /// ルートに置くウィジェット。呼ぶたびに置き換わる。
    ///
    /// サイドバーを付けているときは、その右の区画に置かれる。
    pub fn set_child(&self, child: &dyn Widget) {
        let view = child.native_view();
        let sidebar = self.0.sidebar.borrow().clone();
        match sidebar {
            Some(sidebar) => sidebar.set_content(Some(view)),
            None => self.0.native.setContentView(Some(&view)),
        }
        *self.0.child.borrow_mut() = Some(child.boxed_clone());
    }

    /// ウィンドウの左に付けるサイドバー。呼ぶたびに置き換わる。
    ///
    /// ウィンドウの `contentViewController` を `NSSplitViewController` へ
    /// 差し替え、[`set_child`](Self::set_child) の子はその右の区画へ移す。
    /// サイドバーをタイトルバーの下まで伸ばすため (「システム設定」と同じ形)、
    /// 付けている間はウィンドウに `fullSizeContentView` を足す。子の上端は
    /// タイトルバーを避けるので、中身の見え方は変わらない。
    ///
    /// 中身の大きさ ([`size`](Self::size)) は変わらず、サイドバーの幅も
    /// その中に含む (ウィンドウの枠のほうが変わる)。
    pub fn set_sidebar(&self, sidebar: &Sidebar) {
        self.keeping_size(|| self.attach_sidebar(sidebar));
    }

    fn attach_sidebar(&self, sidebar: &Sidebar) {
        self.detach_sidebar();
        let native = &self.0.native;
        // `contentViewController` を差し替えると、ウィンドウがその大きさへ
        // 合わせて縮むことがある。置く前の枠へ戻す。
        let frame = native.frame();
        let child = self
            .0
            .child
            .borrow()
            .as_ref()
            .map(|child| child.native_view());
        if let Some(view) = &child {
            view.removeFromSuperview();
        }
        native.setStyleMask(native.styleMask() | NSWindowStyleMask::FullSizeContentView);
        native.setContentViewController(Some(&sidebar.native_split_view_controller()));
        native.setFrame_display(frame, true);
        sidebar.set_content(child);
        sidebar.apply_width();
        *self.0.sidebar.borrow_mut() = Some(sidebar.clone());
        self.apply_toolbar();
    }

    /// 取り付けたサイドバーを外す。付いていなければ何もしない。
    ///
    /// 右の区画にあった子は、ウィンドウの中身へ戻る。中身の大きさは変わらない。
    pub fn clear_sidebar(&self) {
        self.keeping_size(|| self.detach_sidebar());
    }

    fn detach_sidebar(&self) {
        let Some(sidebar) = self.0.sidebar.borrow_mut().take() else {
            return;
        };
        let native = &self.0.native;
        let frame = native.frame();
        let child = sidebar.take_content();
        native.setContentViewController(None);
        native.setStyleMask(native.styleMask() & !NSWindowStyleMask::FullSizeContentView);
        let view = child.unwrap_or_else(|| NSView::new(MainThreadMarker::from(&**native)));
        // 右の区画では制約で置いていた。ウィンドウの中身は枠で置かれる。
        view.setTranslatesAutoresizingMaskIntoConstraints(true);
        native.setContentView(Some(&view));
        // ボタンだけのツールバーを外すとウィンドウの高さが変わるので、
        // ツールバーを決め直してから元の枠へ戻す。
        self.apply_toolbar();
        native.setFrame_display(frame, true);
    }

    /// ウィンドウの上端に付けるツールバー。呼ぶたびに置き換わる。
    ///
    /// AppKit ではタイトルバーと一体で表示され、項目が入りきらないときは
    /// AppKit が送り出しのメニューを出す。
    ///
    /// **タイトル文字は隠れる。** ツールバーのあるウィンドウでタイトルを
    /// 出さないのが macOS の作法で、出したままだとタイトルが先頭を占め、
    /// 項目が右端へ押しやられてしまう。[`set_title`](Self::set_title) で
    /// 設定した文字はウィンドウのタイトルとして残り (ウィンドウメニューや
    /// Mission Control には出る)、[`title`](Self::title) も返し続ける。
    ///
    /// サイドバーを付けているときは、先頭に AppKit 標準のサイドバーボタンが
    /// 入る (naui の項目のインデックスには数えない)。
    ///
    /// 中身の大きさ ([`size`](Self::size)) は変わらず、ツールバーの分だけ
    /// ウィンドウの枠が伸びる。
    pub fn set_toolbar(&self, toolbar: &Toolbar) {
        let old = self.0.toolbar.borrow_mut().replace(toolbar.clone());
        self.keeping_size(|| self.apply_toolbar());
        // 前のツールバーは、ウィンドウから外れてから元の並びへ戻す。
        if let Some(old) = old.filter(|old| !old.is_same(toolbar)) {
            old.set_sidebar_controls(false);
        }
    }

    /// 取り付けたツールバーを外す。付いていなければ何もしない。
    ///
    /// 隠していたタイトル文字も出し直す。
    ///
    /// サイドバーを付けているときは、サイドバーボタンだけのツールバーが残る。
    /// 中身の大きさは変わらない。
    pub fn clear_toolbar(&self) {
        let old = self.0.toolbar.borrow_mut().take();
        self.keeping_size(|| self.apply_toolbar());
        if let Some(old) = old {
            old.set_sidebar_controls(false);
        }
    }

    /// ツールバーとサイドバーの組み合わせから、ウィンドウのツールバーを決める。
    ///
    /// | アプリのツールバー | サイドバー | ウィンドウに付くもの |
    /// | --- | --- | --- |
    /// | あり | あり | アプリのツールバー (先頭にサイドバーボタン) |
    /// | あり | なし | アプリのツールバー |
    /// | なし | あり | サイドバーボタンだけのツールバー (タイトルは出したまま) |
    /// | なし | なし | 無し |
    ///
    /// サイドバー用の項目を外すのは、ツールバーをウィンドウから外している
    /// 間に行い、入れるのはウィンドウに付けてから行う
    /// (`NSToolbarSidebarTrackingSeparatorItemIdentifier` は、付いていない
    /// ツールバーには入らない)。
    fn apply_toolbar(&self) {
        let toolbar = self.0.toolbar.borrow().clone();
        let sidebar = self.0.sidebar.borrow().clone();
        self.0.native.setToolbar(None);
        if let Some(toolbar) = &toolbar {
            toolbar.set_sidebar_controls(sidebar.is_some());
        }
        let (chosen, visibility) = match (&toolbar, &sidebar) {
            (Some(toolbar), _) => (Some(toolbar.clone()), NSWindowTitleVisibility::Hidden),
            (None, Some(sidebar)) => (
                Some(sidebar.controls_toolbar()),
                NSWindowTitleVisibility::Visible,
            ),
            (None, None) => (None, NSWindowTitleVisibility::Visible),
        };
        self.0
            .native
            .setToolbar(chosen.as_ref().map(|t| t.native_toolbar()).as_deref());
        self.0.native.setTitleVisibility(visibility);
        // ウィンドウに付いてからでないとサイドバー用の区切りが入らないので、
        // 付けたあとでもう一度そろえる。
        if let Some(chosen) = &chosen {
            chosen.set_sidebar_controls(sidebar.is_some());
        }
        // サイドバーとツールバーで、タイトルバーと重なる分が変わる。
        self.apply_min_size();
    }

    /// 画面上端に出すメニューバー。呼ぶたびに置き換わる。
    ///
    /// **macOS のメニューバーはアプリに 1 つ**なので、
    /// `NSApplication.mainMenu` を差し替える。どのウィンドウから呼んでも
    /// アプリ全体に効き、ウィンドウが前面かどうかでは変わらない
    /// (ほかの 3 環境に合わせてウィンドウの API にしてある)。
    pub fn set_menu_bar(&self, menu_bar: &MenuBar) {
        menu_bar.install();
        *self.0.menu_bar.borrow_mut() = Some(menu_bar.clone());
    }

    /// 取り付けたメニューバーを外す。付いていなければ何もしない。
    ///
    /// メニューが 1 つも無いと ⌘C / ⌘V が配送されなくなるため、naui の
    /// 既定のメニュー (アプリメニューと編集メニュー) へ戻す。
    pub fn clear_menu_bar(&self) {
        if let Some(old) = self.0.menu_bar.borrow_mut().take() {
            old.uninstall();
        }
    }

    /// 画面に出して前面へ持ってくる。
    ///
    /// 最初の表示では、表示する前に [`set_state`](Self::set_state) で頼んだ
    /// 状態にする。
    pub fn show(&self) {
        self.0.native.makeKeyAndOrderFront(None);
        if !self.0.shown.replace(true) {
            self.0.on_state_change.reset(WindowState::Normal);
            let state = self.0.initial_state.get();
            if state != WindowState::Normal {
                self.apply_state(state);
            }
        }
    }

    pub fn close(&self) {
        self.0.native.close();
    }

    pub fn is_visible(&self) -> bool {
        self.0.native.isVisible()
    }

    /// このウィンドウの配色テーマを切り替える。
    pub fn set_theme(&self, theme: Theme) -> Result<()> {
        let appearance = match theme {
            Theme::System => None,
            Theme::Light => unsafe { NSAppearance::appearanceNamed(NSAppearanceNameAqua) },
            Theme::Dark => unsafe { NSAppearance::appearanceNamed(NSAppearanceNameDarkAqua) },
        };
        self.0.native.setAppearance(appearance.as_deref());
        Ok(())
    }

    /// AppKit の実ウィンドウ。バックエンド固有の脱出口。
    pub fn native_window(&self) -> Retained<NSWindow> {
        self.0.native.clone()
    }
}

/// ウィンドウの中身へ重ねるビュー (トーストの載せ先)。
///
/// naui のサイドバーを付けたウィンドウでは、`contentViewController` が
/// `NSSplitViewController` になっているので、その最後の区画 (中身の側) を
/// 返す。`contentView` そのもの (`NSSplitView`) へ足すと、区画の 1 つとして
/// 並べられてしまう。
pub(crate) fn overlay_host(window: &NSWindow) -> Option<Retained<NSView>> {
    let mtm = MainThreadMarker::from(window);
    let split = window
        .contentViewController()
        .and_then(|controller| controller.downcast::<NSSplitViewController>().ok());
    match split {
        Some(split) => split
            .splitViewItems()
            .lastObject()
            .map(|item| item.viewController(mtm).view()),
        None => window.contentView(),
    }
}

/// naui が作ったウィンドウのうち、いちばん手前のもの。1 つも無ければ `None`。
///
/// 焦点のあるウィンドウを使い、まだどれにも当たっていないとき (起動直後や
/// 自動テスト) は**最後に作ったもの**にする。`Dialog` が親を選ぶのと
/// 同じ考え方。
pub(crate) fn frontmost(mtm: MainThreadMarker) -> Option<Retained<NSWindow>> {
    let app = NSApplication::sharedApplication(mtm);
    WINDOWS.with(|slot| {
        let windows = slot.borrow();
        let ours = |window: &NSWindow| {
            windows
                .iter()
                .any(|ours| std::ptr::eq(&**ours as *const NSWindow, window as *const NSWindow))
        };
        app.keyWindow()
            .filter(|window| ours(window))
            .or_else(|| app.mainWindow().filter(|window| ours(window)))
            .or_else(|| windows.last().cloned())
    })
}
