//! A top-level window (`AdwApplicationWindow`).
//!
//! Unlike `GtkApplicationWindow`, `AdwApplicationWindow` **has no default title
//! bar**. The minimize, maximize and close buttons are provided by `AdwHeaderBar`,
//! so instead of inserting the content directly, the header bar goes in the top row
//! of an `AdwToolbarView` and the app's content in the bottom row.
//!
//! The bottom row is further wrapped in an `AdwToastOverlay`. A
//! [`Toast`](crate::Toast) is added here and overlaps below the header bar, on top
//! of the app's content (the GNOME convention).
//!
//! When a [`Sidebar`] is attached, the window's content becomes a `GtkPaned`, and
//! the `AdwToolbarView` above moves to its right pane. The left pane holds the
//! sidebar's own header bar and list.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use adw::prelude::*;
use gtk::glib;
use naui_core::{CloseHandler, CloseResponse, Result, StateHandler, Theme, WindowState};

use crate::callback::Notifier;

use crate::menu_bar::MenuBar;
use crate::sidebar::Sidebar;
use crate::toolbar::Toolbar;
use crate::widgets::Widget;

pub(crate) struct WindowInner {
    native: adw::ApplicationWindow,
    /// The container for the app's content and the toasts that overlap it.
    overlay: adw::ToastOverlay,
    /// The container for the top row (header bar and menu bar) and the bottom row (the
    /// app's content).
    view: adw::ToolbarView,
    /// The title and the minimize, maximize and close buttons.
    header: adw::HeaderBar,
    child: RefCell<Option<Box<dyn Widget>>>,
    /// The toolbar inserted into the header bar. Kept alive along with its handlers.
    toolbar: RefCell<Option<Toolbar>>,
    /// The menu bar inserted below the header bar. Kept alive along with its handlers.
    menu_bar: RefCell<Option<MenuBar>>,
    /// The attached sidebar. Kept alive along with its handlers.
    sidebar: RefCell<Option<Sidebar>>,
    on_close_request: CloseHandler,
    on_resize: Notifier<(f64, f64)>,
    /// The [`Window::size`] last reported (or the initial one). Not called for the same
    /// size.
    last_size: Cell<(f64, f64)>,
    /// The frame clock's `after-paint`, which compares the content size after each
    /// paint.
    after_paint: RefCell<Option<(gtk::gdk::FrameClock, glib::SignalHandlerId)>>,
    /// In the middle of naui's [`Window::close`]. GTK4 emits `close-request` even when
    /// closed from code, so no confirmation is asked then.
    closing: Cell<bool>,
    /// The content size to apply on the first [`Window::show`]. It starts as the width
    /// and height given to `ui.window` and is replaced by `set_size` before showing.
    /// Emptied once shown.
    initial_size: Cell<Option<(f64, f64)>>,
    /// The content size being refitted ([`Window::fit_after_layout`]).
    fit: Cell<Option<Fit>>,
    /// A per-frame callback for refitting is attached.
    fitting: Cell<bool>,
    on_state_change: StateHandler,
    /// Whether [`Window::show`] has ever been called. `set_state` before showing is
    /// remembered and applied when shown.
    shown: Cell<bool>,
    /// The state requested before showing.
    initial_state: Cell<WindowState>,
    /// The state requested before showing, for which the compositor's first state has
    /// not arrived yet. [`Window::state`] returns this meanwhile (on Wayland the surface
    /// state right after showing is empty, so reading it would look like it went back
    /// to `Normal`). Minimizing is the exception: no compositor honors it when
    /// requested before showing, so it is applied after the first state arrives.
    pending_state: Cell<Option<WindowState>>,
    /// Minimization was requested with [`Window::set_state`] and restoring has not
    /// been requested yet. On Wayland, minimization does not show up in `state`, so
    /// this is the clue used when restoring.
    minimize_requested: Cell<bool>,
    /// Maximization was requested, has not shown up in the compositor's state yet, and
    /// restoring has not been requested either. `is_maximized` does not change until
    /// the compositor responds, so this is the clue for withdrawing it when another
    /// request comes before the response.
    maximize_requested: Cell<bool>,
    /// Fullscreen was requested, has not shown up in the compositor's state yet, and
    /// leaving it has not been requested either.
    fullscreen_requested: Cell<bool>,
    /// The state to apply after leaving fullscreen has finished. Some compositors
    /// ignore a maximize request while fullscreen, so fullscreen is left first.
    after_fullscreen: Cell<Option<WindowState>>,
    /// The `notify::state` of the `GdkToplevel` watched for state changes. The surface
    /// is recreated on every realize, so it is reattached each time.
    state_handler: RefCell<Option<(gtk::gdk::Surface, glib::SignalHandlerId)>>,
}

/// The state of an in-progress refit of the content size.
#[derive(Clone, Copy)]
struct Fit {
    /// The content size requested.
    target: (f64, f64),
    /// The number of frames since the default size was last requested.
    ticks: u32,
    /// The time (microseconds) of the frame the refit started at. Set on the first
    /// frame.
    started: Option<i64>,
}

/// The number of frames from requesting the default size until measuring the
/// rearranged content. It is laid out in the frame after the request, so it is
/// measured in the frame after that.
const FIT_SETTLE_TICKS: u32 = 2;
/// The number of frames after which the same value is requested again if the
/// requested size has not been reached.
const FIT_RETRY_TICKS: u32 = 6;
/// How long (microseconds) before giving up on refitting. Limited to right after
/// showing so that it does not keep fighting the user's resizing.
const FIT_LIMIT_US: i64 = 1_000_000;

/// A top-level window.
///
/// Create it inside the callback passed to `run`. The framework (`GtkApplication`)
/// keeps a reference to it, so dropping the return value does not close it.
#[derive(Clone)]
pub struct Window(Rc<WindowInner>);

/// A weak reference for referring to a window from event handlers without holding
/// it strongly.
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
        // `on_focus_change` の見張り。
        crate::focus::install(&native);

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
            on_state_change: StateHandler::default(),
            shown: Cell::new(false),
            initial_state: Cell::new(WindowState::Normal),
            pending_state: Cell::new(None),
            minimize_requested: Cell::new(false),
            maximize_requested: Cell::new(false),
            fullscreen_requested: Cell::new(false),
            after_fullscreen: Cell::new(None),
            state_handler: RefCell::new(None),
        }));
        this.observe();
        this.apply_default_size((width.max(0.0), height.max(0.0)));
        this.0
            .initial_size
            .set(Some((width.max(0.0), height.max(0.0))));
        this
    }

    /// Sets the window's default size so that the content becomes `size`.
    ///
    /// GTK4's default size is for the whole window (excluding the shadow), including
    /// the header bar and menu bar strips and the window's own border, so those are
    /// added. The strip heights may be estimates, so when it affects what is shown,
    /// the actual size after layout is checked with
    /// [`fit_after_layout`](Self::fit_after_layout).
    fn apply_default_size(&self, size: (f64, f64)) {
        let (width, height) = self.default_size_for(size);
        self.0.native.set_default_size(width, height);
    }

    /// The window's default size needed to make the content `(width, height)`.
    fn default_size_for(&self, (width, height): (f64, f64)) -> (i32, i32) {
        let bars = self.top_bars_height();
        let (frame_width, frame_height) = self.frame_size();
        (
            to_px(width + frame_width),
            to_px(height + bars + frame_height),
        )
    }

    /// The width and height of the window's own CSS frame (padding and border).
    ///
    /// The default size is the window's border box, and the content is laid out inside
    /// it. On X11 without a compositor, `solid-csd` padding is added instead of a
    /// shadow (5px on every side in Adwaita). This class is added on realize, so
    /// before layout it is read from CSS if realized, and taken as 0 before that.
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

    /// The height of the top strips (header bar and menu bar).
    ///
    /// Once laid out, it is the difference between the heights of the window's content
    /// and the `AdwToastOverlay`. A menu bar just attached while shown is not laid out
    /// yet, so its measured height is added (right after detaching, or right after the
    /// toolbar changes the header bar's height, the old height remains until it is
    /// laid out again). Before layout, it is an estimate from measuring each strip
    /// (which comes out low before CSS applies).
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

    /// Refits using the actual size after layout until the content becomes `target`.
    ///
    /// The strip heights added to the default size are estimates before layout and
    /// right after strips are attached or detached, so they can be off. Also, on
    /// Wayland, a size requested right after showing can be overwritten by a
    /// compositor configure that arrives later (with the old size). GTK4 has no
    /// notification that the size has settled, so it measures every frame, requests
    /// again if the strip heights differ, and if the requested size is not reached,
    /// waits a while and requests the same value again. It gives up after
    /// [`FIT_LIMIT_US`].
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

    /// Advances the refit by one frame. `true` to continue.
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
            let handler = clock.connect_after_paint({
                let weak = weak.clone();
                move |_| {
                    if let Some(inner) = weak.upgrade() {
                        Window(inner).report_size();
                    }
                }
            });
            if let Some((old, id)) = inner.after_paint.replace(Some((clock, handler))) {
                old.disconnect(id);
            }
            // 最大化・最小化・全画面は、どれもコンポジタから届く面の状態に出る。
            if let Some(surface) = native.surface() {
                let weak = weak.clone();
                let handler = surface.connect_notify_local(Some("state"), move |_, _| {
                    if let Some(inner) = weak.upgrade() {
                        Window(inner).report_state();
                    }
                });
                if let Some((old, id)) = inner.state_handler.replace(Some((surface, handler))) {
                    old.disconnect(id);
                }
            }
        });
        let weak = Rc::downgrade(&self.0);
        self.0.native.connect_unrealize(move |_| {
            let Some(inner) = weak.upgrade() else {
                return;
            };
            if let Some((clock, id)) = inner.after_paint.take() {
                clock.disconnect(id);
            }
            if let Some((surface, id)) = inner.state_handler.take() {
                surface.disconnect(id);
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

    /// Calls `on_resize` if the current [`size`](Self::size) differs from the one last
    /// reported.
    fn report_size(&self) {
        let size = self.size();
        if self.0.last_size.replace(size) != size {
            self.0.on_resize.emit(size);
        }
    }

    /// Confirmation when the user tries to close the window.
    ///
    /// Returning [`CloseResponse::KeepOpen`] keeps it open (to ask for confirmation
    /// when there are unsaved changes, for example). Not called for
    /// [`close`](Self::close) from code.
    pub fn on_close_request(&self, f: impl FnMut() -> CloseResponse + 'static) {
        self.0.on_close_request.set(f);
    }

    /// The size of the content area (width, height) in logical pixels, excluding the
    /// header bar and menu bar. With a sidebar attached, it includes the sidebar's
    /// width, as on the other platforms.
    ///
    /// Before it is on screen, this is the size given to `ui.window` or
    /// [`set_size`](Self::set_size).
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

    /// The position of the window's top-left corner. **It cannot be obtained on Linux,
    /// so this is always `None`.**
    ///
    /// GTK4 has no API to read or move a window's position. On Wayland there is no
    /// mechanism at all for an app to know its window's position (the compositor
    /// decides placement), and GTK4 dropped it on X11 too to match.
    pub fn position(&self) -> Option<(f64, f64)> {
        None
    }

    /// Moves the window. **Does nothing on Linux** (see [`position`](Self::position)).
    pub fn set_position(&self, x: f64, y: f64) {
        let _ = (x, y);
    }

    /// The current display state.
    ///
    /// Reads the surface state delivered by the compositor (`GdkToplevelState`).
    /// Before it is shown, and after showing until the compositor's first state
    /// arrives, it is the state requested with [`set_state`](Self::set_state) (the
    /// default is [`WindowState::Normal`]).
    ///
    /// **Wayland has no way to know about minimization**, so minimizing does not make
    /// it [`WindowState::Minimized`] (it returns the state from before it was put
    /// away).
    pub fn state(&self) -> WindowState {
        if !self.0.shown.get() {
            return self.0.initial_state.get();
        }
        let native = &self.0.native;
        match self.toplevel() {
            Some(toplevel) => {
                use gtk::gdk::ToplevelState;
                let state = toplevel.state();
                // 何も届いていない面の状態は空。そこで読むと頼んだ状態が
                // `Normal` に戻って見えるので、頼んだ状態のままにしておく。
                if state.is_empty() {
                    if let Some(pending) = self.0.pending_state.get() {
                        return pending;
                    }
                }
                WindowState::from_flags(
                    state.contains(ToplevelState::MINIMIZED),
                    state.contains(ToplevelState::FULLSCREEN),
                    state.contains(ToplevelState::MAXIMIZED),
                )
            }
            None => WindowState::from_flags(false, native.is_fullscreen(), native.is_maximized()),
        }
    }

    /// The window's surface (`GdkToplevel`), if realized.
    fn toplevel(&self) -> Option<gtk::gdk::Toplevel> {
        self.0
            .native
            .surface()
            .and_then(|surface| surface.dynamic_cast::<gtk::gdk::Toplevel>().ok())
    }

    /// Changes the display state. When called before it is shown, it is applied when
    /// shown.
    ///
    /// | Requested state | GTK4 |
    /// | --- | --- |
    /// | `Normal` | `unfullscreen` / `unmaximize` / `unminimize` and `present` |
    /// | `Maximized` | `maximize` |
    /// | `Minimized` | `minimize` |
    /// | `Fullscreen` | `fullscreen` |
    ///
    /// All of these are requests to the compositor, and `state` changes only after the
    /// compositor responds ([`on_state_change`](Self::on_state_change) is called when
    /// it changes). When moving from fullscreen to another state, it is applied after
    /// leaving fullscreen has finished. When restoring from minimized, `present` is
    /// requested in addition to `unminimize` to bring it to the front (X11 window
    /// managers sometimes do not restore it with `unminimize` alone). **On Wayland,
    /// restoring from minimized is not guaranteed** (there is no mechanism for it, and
    /// whether `present` is honored is up to the compositor).
    ///
    /// If you request again before the compositor responds, the unanswered request is
    /// withdrawn before the next one, so you end up in the last state requested.
    ///
    /// Minimizing before showing is the one exception: window managers ignore it when
    /// requested before showing, so it is applied after the window is shown and the
    /// compositor's first state arrives (it appears on screen for a moment).
    pub fn set_state(&self, state: WindowState) {
        if !self.0.shown.get() {
            self.0.initial_state.set(state);
            return;
        }
        // 表示する前の頼みより、後から頼まれた状態を優先する。
        self.0.pending_state.set(None);
        self.apply_state(state);
    }

    fn apply_state(&self, target: WindowState) {
        let native = &self.0.native;
        if target == WindowState::Minimized {
            self.0.minimize_requested.set(true);
        } else if self.0.minimize_requested.replace(false) || self.state() == WindowState::Minimized
        {
            native.unminimize();
            native.present();
        }
        if target != WindowState::Fullscreen {
            if native.is_fullscreen() {
                self.0.after_fullscreen.set(Some(target));
                self.0.fullscreen_requested.set(false);
                native.unfullscreen();
                return;
            }
            // 頼んだ全画面にコンポジタがまだ応えていなければ、応える前に
            // 取り消す (頼みは順に届くので、出終わりを待たずに次を頼める)。
            if self.0.fullscreen_requested.replace(false) {
                native.unfullscreen();
            }
        }
        self.0.after_fullscreen.set(None);
        match target {
            WindowState::Normal => {
                if self.0.maximize_requested.replace(false) || native.is_maximized() {
                    native.unmaximize();
                }
            }
            WindowState::Maximized => {
                self.0.maximize_requested.set(true);
                native.maximize();
            }
            WindowState::Minimized => native.minimize(),
            WindowState::Fullscreen => {
                self.0.fullscreen_requested.set(true);
                native.fullscreen();
            }
        }
    }

    /// Notified when the display state changes. Receives the new
    /// [`state`](Self::state).
    ///
    /// Called both for user actions (header bar buttons, snapping to a screen edge and
    /// so on) and for [`set_state`](Self::set_state).
    pub fn on_state_change(&self, f: impl FnMut(WindowState) + 'static) {
        self.0.on_state_change.set(f);
    }

    /// Calls `on_state_change` if the current state differs from the one last
    /// reported.
    ///
    /// When the compositor's first state arrives, a minimize requested before showing
    /// is applied. When leaving fullscreen has finished, the state requested after it
    /// is applied.
    fn report_state(&self) {
        if !self.0.shown.get() {
            return;
        }
        let arrived = self
            .toplevel()
            .is_some_and(|toplevel| !toplevel.state().is_empty());
        if arrived && self.0.pending_state.take() == Some(WindowState::Minimized) {
            self.apply_state(WindowState::Minimized);
        }
        // コンポジタが応えた頼みは、ここからは面の状態で分かる。
        if let Some(toplevel) = self.toplevel() {
            use gtk::gdk::ToplevelState;
            let flags = toplevel.state();
            if flags.contains(ToplevelState::MAXIMIZED) {
                self.0.maximize_requested.set(false);
            }
            if flags.contains(ToplevelState::FULLSCREEN) {
                self.0.fullscreen_requested.set(false);
            }
        }
        let state = self.state();
        self.0.on_state_change.update(state);
        if state != WindowState::Fullscreen {
            if let Some(next) = self.0.after_fullscreen.take() {
                self.apply_state(next);
            }
        }
    }

    /// Whether the user can resize the window by dragging its edges. `true` by default.
    pub fn is_resizable(&self) -> bool {
        self.0.native.is_resizable()
    }

    /// Decides whether the user can resize the window.
    ///
    /// Uses `gtk_window_set_resizable`. Turning it off also removes the header bar's
    /// maximize button. [`set_size`](Self::set_size) works either way.
    pub fn set_resizable(&self, resizable: bool) {
        self.0.native.set_resizable(resizable);
    }

    /// Notified when the size changes. Receives the new [`size`](Self::size).
    pub fn on_resize(&self, f: impl FnMut((f64, f64)) + 'static) {
        self.0.on_resize.set(f);
    }

    /// The corresponding GTK4 window. Public as a backend-specific escape hatch.
    pub fn native_window(&self) -> adw::ApplicationWindow {
        self.0.native.clone()
    }

    /// The header bar with the title and the minimize, maximize and close buttons.
    ///
    /// Public as a backend-specific escape hatch.
    pub fn native_header_bar(&self) -> adw::HeaderBar {
        self.0.header.clone()
    }

    /// The `AdwToastOverlay` toasts overlap in.
    ///
    /// Public as a backend-specific escape hatch.
    pub fn native_toast_overlay(&self) -> adw::ToastOverlay {
        self.0.overlay.clone()
    }

    pub fn downgrade(&self) -> WeakWindow {
        WeakWindow(Rc::downgrade(&self.0))
    }

    /// Notified of keys pressed inside this window.
    ///
    /// Keys for which the focused widget's `on_key_down` returned `Continue` arrive
    /// here. Returning `Handled` keeps the key from reaching the widget. Keys pressed
    /// while an IME is composing are not delivered.
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

    /// The minimum size the user can shrink it to (content width, height). The units
    /// are the same as [`set_size`](Self::set_size).
    ///
    /// Prevents the layout from breaking when it is made too small.
    ///
    /// A GTK4 window never shrinks below its content's minimum size, so the minimum is
    /// set on the content container (the header bar is added on top of that). While a
    /// sidebar is attached, the minimum width applies to the pane right of the sidebar
    /// (it stops at a width larger by the sidebar's width).
    pub fn set_min_size(&self, width: f64, height: f64) {
        self.0
            .overlay
            .set_size_request(to_px(width.max(0.0)), to_px(height.max(0.0)));
    }

    /// Changes the size of the content area. The units and extent are the same as
    /// [`size`](Self::size), excluding the header bar and menu bar (the same as the
    /// other platforms).
    ///
    /// GTK4's `set_default_size` takes the size of the whole window, so the heights of
    /// the header bar and menu bar strips are added. When called before it is on
    /// screen, it is shown at this size.
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

    /// Replaces the window's content.
    pub fn set_child(&self, child: &dyn Widget) {
        let bin = child.size_bin();
        let previous = self.0.child.borrow().as_ref().map(|child| child.size_bin());
        if let Some(previous) = previous {
            if previous != bin {
                crate::interaction::clear_focus(&previous);
            }
        }
        // ウィンドウの中身は、他のバックエンドと同じく窓いっぱいに広がる。
        bin.fill_parent();
        // ヘッダーバーの下が、アプリの中身の置き場になる。
        // 直接ではなく、トーストを重ねる入れ物ごしに入れる。
        self.0.overlay.set_child(Some(&bin));
        *self.0.child.borrow_mut() = Some(child.boxed_clone());
    }

    /// The sidebar attached to the left of the window. Each call replaces it.
    ///
    /// The window's content is replaced with a `GtkPaned`, and the previous content
    /// (header bar, menu bar, child) moves to the pane on its right.
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

    /// Removes the attached sidebar. Does nothing if none is attached.
    ///
    /// The content of the right pane goes back to being the window's content.
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

    /// Reorders the left side of the header bar as: the sidebar button's slot, then the
    /// toolbar.
    ///
    /// The slot holds the sidebar button only while the sidebar is collapsed.
    /// `pack_start` keeps appending, so whatever was attached first is removed before
    /// reordering (the slot is always at the left end).
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

    /// The toolbar attached to the top of the window. Each call replaces it.
    ///
    /// Following GNOME conventions, items are laid out on the left side of the header
    /// bar.
    pub fn set_toolbar(&self, toolbar: &Toolbar) {
        self.clear_toolbar();
        *self.0.toolbar.borrow_mut() = Some(toolbar.clone());
        self.mount_header_start();
    }

    /// Removes the attached toolbar. Does nothing if none is attached.
    pub fn clear_toolbar(&self) {
        if let Some(old) = self.0.toolbar.borrow_mut().take() {
            self.0.header.remove(&old.mount());
        }
    }

    /// The menu bar attached to the top of the window. Each call replaces it.
    ///
    /// The `GtkPopoverMenuBar` goes below the header bar. Item actions are added as a
    /// group of window actions, so accelerators (shortcuts) registered with the
    /// `GtkApplication` work without opening the menu.
    pub fn set_menu_bar(&self, menu_bar: &MenuBar) {
        self.clear_menu_bar();
        self.0.view.add_top_bar(&menu_bar.mount());
        self.0
            .native
            .insert_action_group(&menu_bar.group(), Some(&menu_bar.action_group()));
        menu_bar.attach();
        *self.0.menu_bar.borrow_mut() = Some(menu_bar.clone());
    }

    /// Removes the attached menu bar. Does nothing if none is attached.
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

    /// Shows it on screen and brings it to the front.
    ///
    /// On first show, the content is sized as given to `ui.window` (or to
    /// [`set_size`](Self::set_size) before showing), and the state requested with
    /// [`set_state`](Self::set_state) before showing is applied.
    pub fn show(&self) {
        if !self.0.shown.replace(true) {
            // GTK4 は表示する前に頼んだ最大化・全画面を、出すときに当てる。
            // 最小化は、コンポジタから最初の状態が届いたら `report_state` が当てる。
            let state = self.0.initial_state.get();
            if state != WindowState::Normal {
                self.0.pending_state.set(Some(state));
                if state != WindowState::Minimized {
                    self.apply_state(state);
                }
            }
        }
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

    /// Applies a color theme to this window.
    ///
    /// libadwaita has one theme for the whole app, so in practice it applies to the
    /// whole app.
    pub fn set_theme(&self, theme: Theme) -> Result<()> {
        crate::apply_theme(theme);
        Ok(())
    }
}

/// The `AdwToastOverlay` in `window`. `None` if it is not a window naui created.
///
/// [`Toast`](crate::Toast) is shown in "the frontmost window", so starting from a
/// window found through the `GtkApplication`, it descends the structure naui built
/// (`AdwApplicationWindow` → `AdwToolbarView` → `AdwToastOverlay`). With a sidebar
/// attached, the right pane of a `GtkPaned` sits in between.
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
