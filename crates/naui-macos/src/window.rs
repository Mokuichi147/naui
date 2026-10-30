//! NSWindow handles.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use dispatch2::{DispatchQueue, MainThreadBound};
use naui_core::{
    CloseHandler, CloseResponse, Rect, Result, StateHandler, Theme, WindowPlacement, WindowState,
};
use objc2::rc::Retained;
use objc2::runtime::{NSObjectProtocol, ProtocolObject};
use objc2::{define_class, msg_send, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSAppearance, NSAppearanceCustomization, NSAppearanceNameAqua, NSAppearanceNameDarkAqua,
    NSApplication, NSScreen, NSSplitViewController, NSView, NSWindow, NSWindowCollectionBehavior,
    NSWindowStyleMask, NSWindowTitleVisibility,
};
use objc2_app_kit::{NSDragOperation, NSDraggingDestination, NSDraggingInfo, NSWindowDelegate};
use objc2_foundation::{NSNotification, NSObject, NSPoint, NSRect, NSSize, NSString};

use crate::file_drop::FileDropHandler;
use crate::menu_bar::MenuBar;
use crate::sidebar::Sidebar;
use crate::toolbar::Toolbar;
use crate::trampoline::ValueHandler;
use crate::widgets::Widget;

thread_local! {
    /// The windows naui created, in the order they were created.
    static WINDOWS: RefCell<Vec<Retained<NSWindow>>> = const { RefCell::new(Vec::new()) };
}

struct WindowInner {
    native: Retained<NSWindow>,
    /// Holds the root child, keeping it alive along with its trampolines.
    child: RefCell<Option<Box<dyn Widget>>>,
    /// The attached toolbar. `NSWindow`'s toolbar is a strong reference, but naui's
    /// handles (trampolines and handlers) are also kept alive here.
    toolbar: RefCell<Option<Toolbar>>,
    /// The attached menu bar. `NSApplication` holds `mainMenu` strongly, but naui's
    /// handles (trampolines and handlers) are also kept alive here.
    menu_bar: RefCell<Option<MenuBar>>,
    /// The attached sidebar. It is held strongly as the `contentViewController`, but
    /// naui's handles (data source and handlers) are also kept alive here.
    sidebar: RefCell<Option<Sidebar>>,
    on_close_request: CloseHandler,
    on_resize: ValueHandler<(f64, f64)>,
    /// Takes files dropped where no widget under the pointer takes them.
    on_file_drop: Rc<FileDropHandler>,
    /// The content minimum requested with `set_min_size`. Re-added and passed again
    /// whenever the part overlapping the title bar changes because of a sidebar or
    /// toolbar.
    min_size: Cell<Option<(f64, f64)>>,
    /// `NSWindow`'s delegate is a weak reference, so it is kept alive here.
    delegate: RefCell<Option<Retained<WindowDelegate>>>,
    on_state_change: StateHandler,
    /// Whether [`Window::show`] has ever been called. `set_state` before showing is
    /// remembered and applied when shown.
    shown: Cell<bool>,
    /// The state requested before showing.
    initial_state: Cell<WindowState>,
    /// In the middle of the animation entering or leaving fullscreen. States requested
    /// meanwhile are passed to [`after_transition`](Self::after_transition).
    in_transition: Cell<bool>,
    /// Finished entering fullscreen. The styleMask's `FullScreen` changes at the start
    /// of the animation, so `state` uses this one, decided by the notification at the
    /// end.
    fullscreen: Cell<bool>,
    /// The state to apply after the fullscreen animation ends. When moving from
    /// fullscreen to maximized or minimized, fullscreen is left first, and the state is
    /// applied once that finishes.
    after_transition: Cell<Option<WindowState>>,
    /// The value requested with `set_resizable`. The styleMask is not touched while in
    /// fullscreen; it is applied again on leaving.
    resizable: Cell<bool>,
    /// The position was decided with `set_position`. Later `set_size` calls do not
    /// recenter it.
    positioned: Cell<bool>,
    /// The frame before maximizing with `zoom:` (in AppKit coordinates). While
    /// maximized, [`Window::position`] returns its top left. Read in
    /// `windowShouldZoom:toFrame:`.
    zoom_frame: Cell<Option<NSRect>>,
    /// The position [`Window::position`] returns while in fullscreen: where it goes back
    /// to when leaving. Read when entering fullscreen, and cleared after moving there on
    /// the iteration after leaving finishes.
    fullscreen_return: Cell<Option<(f64, f64)>>,
    /// The content size to go back to when leaving fullscreen, read along with
    /// [`fullscreen_return`](Self::fullscreen_return).
    fullscreen_return_size: Cell<Option<(f64, f64)>>,
    /// The position requested with [`Window::set_position`] while maximized or in
    /// fullscreen. Applied after restoring from maximized, or on the iteration after
    /// leaving fullscreen finishes.
    pending_position: Cell<Option<(f64, f64)>>,
    /// The content size requested with [`Window::set_size`] while maximized or in
    /// fullscreen. Applied along with [`pending_position`](Self::pending_position).
    pending_size: Cell<Option<(f64, f64)>>,
}

define_class!(
    /// Receives the user's attempts to close and size changes.
    ///
    /// `NSWindow::close` (naui's [`Window::close`]) does not go through
    /// `windowShouldClose:`, so confirmation happens only for the title bar button and
    /// ⌘W.
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
                    inner.pending_size.set(None);
                }
                inner.fullscreen_return.set(Some(window.normal_position()));
                inner
                    .fullscreen_return_size
                    .set(Some(window.normal_size()));
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

    // `NSWindow` passes the messages for the file drags it registered for to its
    // delegate (see `file_drop`).
    unsafe impl NSDraggingDestination for WindowDelegate {
        #[unsafe(method(draggingEntered:))]
        fn dragging_entered(&self, sender: &ProtocolObject<dyn NSDraggingInfo>) -> NSDragOperation {
            self.drag_operation(sender)
        }

        #[unsafe(method(draggingUpdated:))]
        fn dragging_updated(&self, sender: &ProtocolObject<dyn NSDraggingInfo>) -> NSDragOperation {
            self.drag_operation(sender)
        }

        #[unsafe(method(performDragOperation:))]
        fn perform_drag_operation(&self, sender: &ProtocolObject<dyn NSDraggingInfo>) -> bool {
            self.ivars().upgrade().is_some_and(|inner| {
                crate::file_drop::perform(&inner.native, sender, &inner.on_file_drop)
            })
        }
    }
);

impl WindowDelegate {
    /// Answers whether the files being dragged would be taken at the pointer.
    fn drag_operation(&self, sender: &ProtocolObject<dyn NSDraggingInfo>) -> NSDragOperation {
        self.ivars()
            .upgrade()
            .map_or(NSDragOperation::None, |inner| {
                crate::file_drop::operation(&inner.native, sender, &inner.on_file_drop)
            })
    }

    fn new(mtm: MainThreadMarker, inner: Weak<WindowInner>) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(inner);
        unsafe { msg_send![super(this), init] }
    }
}

/// A top-level window (NSWindow).
#[derive(Clone)]
pub struct Window(Rc<WindowInner>);

/// A weak reference for referring to a window from event handlers without holding
/// it strongly.
#[derive(Clone)]
pub struct WeakWindow(Weak<WindowInner>);

impl WeakWindow {
    /// Turns it back into a strong reference if the window is still alive.
    pub fn upgrade(&self) -> Option<Window> {
        self.0.upgrade().map(Window)
    }
}

impl Window {
    /// Returns a reference that creates no ownership cycle when passed to event
    /// handlers and the like.
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
            on_file_drop: Rc::default(),
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
            fullscreen_return_size: Cell::new(None),
            pending_position: Cell::new(None),
            pending_size: Cell::new(None),
        }));
        let delegate = WindowDelegate::new(mtm, Rc::downgrade(&this.0));
        this.0
            .native
            .setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
        *this.0.delegate.borrow_mut() = Some(delegate);
        crate::file_drop::register_window(&this.0.native);
        this
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
    /// title bar and toolbar.
    ///
    /// While a sidebar is attached, `fullSizeContentView` extends the `contentView`
    /// under the title bar, so the size of `contentLayoutRect`, which is not hidden by
    /// the title bar and toolbar, is returned instead of the `contentView`'s.
    pub fn size(&self) -> (f64, f64) {
        let size = self.0.native.contentLayoutRect().size;
        (size.width, size.height)
    }

    /// The part of the `contentView` overlapping the title bar and toolbar (width,
    /// height).
    ///
    /// `setContentSize` and the like take the `contentView`'s size, so this is added to
    /// the content size before passing it. 0 when not `fullSizeContentView`.
    fn title_bar_overlap(&self) -> (f64, f64) {
        let native = &self.0.native;
        let content = native.contentRectForFrameRect(native.frame()).size;
        let layout = native.contentLayoutRect().size;
        (
            (content.width - layout.width).max(0.0),
            (content.height - layout.height).max(0.0),
        )
    }

    /// Keeps the content size ([`size`](Self::size)) the same before and after
    /// `change`. The top edge does not move.
    ///
    /// Attaching or detaching a toolbar or sidebar does not keep the content size in
    /// AppKit. Attaching a toolbar grows the content by the title bar's height
    /// (confirmed on macOS 27), and a sidebar keeps the frame, so the content shrinks by
    /// the part hidden under the title bar. To keep the size decided with `ui.window`
    /// or `set_size`, the difference is undone. While maximized or in fullscreen the
    /// size matches the screen, so it is left as is.
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

    /// Passes the `set_min_size` minimum with the title bar overlap added.
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

    /// Notified when the size changes. Receives the new [`size`](Self::size).
    pub fn on_resize(&self, f: impl FnMut((f64, f64)) + 'static) {
        self.0.on_resize.set(f);
    }

    /// Called with the files dropped onto the window from Finder or another app.
    ///
    /// A widget under the pointer with its own `on_file_drop` takes the drop
    /// instead. Text fields take dropped files as text themselves, so drops onto them
    /// do not arrive. Setting it again replaces the previous closure.
    pub fn on_file_drop(&self, f: impl FnMut(&[naui_core::FileEntry]) + 'static) {
        self.0.on_file_drop.set(f);
    }

    /// Notified of keys pressed inside this window.
    ///
    /// Keys for which the focused widget's `on_key_down` returned `Continue` arrive
    /// here. Returning `Handled` keeps the key from reaching AppKit. Keys pressed while
    /// an IME is composing are not delivered.
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

    /// The minimum size the user can shrink it to (content width, height). The units
    /// are the same as [`set_size`](Self::set_size).
    ///
    /// Prevents the layout from breaking when it is made too small.
    pub fn set_min_size(&self, width: f64, height: f64) {
        self.0.min_size.set(Some((width, height)));
        self.apply_min_size();
    }

    /// Changes the size of the content area. The units and extent are the same as
    /// [`size`](Self::size), excluding the title bar and toolbar (the same as the other
    /// platforms).
    ///
    /// Uses `setContentSize`. While a sidebar is attached, the `contentView` extends
    /// under the title bar, so the overlap is added before passing it.
    ///
    /// While maximized or in fullscreen, the state is left as is and the size for
    /// returning to normal is decided, the same as
    /// [`set_position`](Self::set_position). While minimized, the frame changes and
    /// shows when it comes back.
    pub fn set_size(&self, width: f64, height: f64) {
        if self.in_fullscreen() || self.zoom_frame().is_some() {
            self.0
                .pending_size
                .set(Some((width.max(0.0), height.max(0.0))));
            return;
        }
        self.0.pending_size.set(None);
        // 狭すぎるとサイドバーと中身の最小に合わせて広がるので、先に畳む。
        if let Some(sidebar) = self.0.sidebar.borrow().clone() {
            sidebar.make_room(width);
        }
        let (extra_width, extra_height) = self.title_bar_overlap();
        self.0.native.setContentSize(NSSize::new(
            width.max(0.0) + extra_width,
            height.max(0.0) + extra_height,
        ));
        if !self.0.positioned.get() {
            self.0.native.center();
        }
    }

    /// The position (x, y) of the window's top-left corner, in logical pixels (points).
    ///
    /// Counted right and down from the top left of the primary screen (the screen with
    /// the menu bar), the same direction as the other platforms. AppKit's coordinates
    /// count up from the bottom left of the primary screen, so the direction is flipped
    /// by subtracting from the primary screen's height. It is the top left of the
    /// window frame including the title bar, and is never `None`.
    ///
    /// While maximized, minimized or in fullscreen, it returns the position it will have
    /// when back to normal. While minimized, `frame` stays the original frame. For
    /// maximized it is the frame before `zoom:`, and for fullscreen the position read
    /// before entering.
    pub fn position(&self) -> Option<(f64, f64)> {
        if self.in_fullscreen() {
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

    /// In fullscreen, or after leaving it until moved to the return position.
    fn in_fullscreen(&self) -> bool {
        self.0.fullscreen.get()
            || self.0.in_transition.get()
            || self.0.fullscreen_return.get().is_some()
    }

    /// The position when back to normal, excluding fullscreen.
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

    /// The content size when back to normal, excluding fullscreen (see
    /// [`normal_position`](Self::normal_position)).
    fn normal_size(&self) -> (f64, f64) {
        let Some(frame) = self.zoom_frame() else {
            return self.size();
        };
        if let Some(size) = self.0.pending_size.get() {
            return size;
        }
        let content = self.0.native.contentRectForFrameRect(frame).size;
        let (extra_width, extra_height) = self.title_bar_overlap();
        (
            (content.width - extra_width).max(0.0),
            (content.height - extra_height).max(0.0),
        )
    }

    /// The size and position to save, and the state to show it in next time.
    ///
    /// The size and position are those of the normal state, the same as
    /// [`position`](Self::position): while maximized it is the frame before `zoom:`,
    /// and in fullscreen the frame read before entering. A minimized window reports
    /// the state it returns to (maximized if it was zoomed).
    pub fn placement(&self) -> WindowPlacement {
        let size = if self.in_fullscreen() {
            self.0
                .pending_size
                .get()
                .or(self.0.fullscreen_return_size.get())
                .unwrap_or_else(|| self.normal_size())
        } else {
            self.normal_size()
        };
        let state = if self.0.shown.get() {
            WindowState::from_flags(false, self.0.fullscreen.get(), self.0.native.isZoomed())
        } else {
            match self.0.initial_state.get() {
                WindowState::Minimized => WindowState::Normal,
                state => state,
            }
        };
        WindowPlacement {
            size,
            position: self.position(),
            state,
        }
    }

    /// Applies a saved [`placement`](Self::placement): the size, then the position, then
    /// the state.
    ///
    /// The position is used only if the top edge of the window would be on one of the
    /// screens connected now (`NSScreen`'s `visibleFrame`). Otherwise, such as when the
    /// screen it was on has been disconnected, it is placed as if no position had been
    /// given (centered unless [`set_position`](Self::set_position) was called before).
    pub fn set_placement(&self, placement: &WindowPlacement) {
        let (width, height) = placement.size;
        self.set_size(width, height);
        if let Some(position) = placement.position {
            if naui_core::is_on_screen(position, width, &self.screen_work_areas()) {
                self.set_position(position.0, position.1);
            }
        }
        self.set_state(placement.state);
    }

    /// The work areas of the connected screens (`visibleFrame`, excluding the menu bar
    /// and Dock), in the coordinates of [`position`](Self::position).
    fn screen_work_areas(&self) -> Vec<Rect> {
        let mtm = MainThreadMarker::from(&*self.0.native);
        let primary_height = self.primary_screen_height();
        NSScreen::screens(mtm)
            .iter()
            .map(|screen| {
                let frame = screen.visibleFrame();
                Rect::new(
                    frame.origin.x,
                    primary_height - (frame.origin.y + frame.size.height),
                    frame.size.width,
                    frame.size.height,
                )
            })
            .collect()
    }

    /// While maximized with `zoom:`, the frame before maximizing.
    ///
    /// `None` when it became `isZoomed` without going through `zoom:`, such as when the
    /// user resized it to fill the screen (the visible frame is used as is).
    fn zoom_frame(&self) -> Option<NSRect> {
        self.0.zoom_frame.get().filter(|_| self.0.native.isZoomed())
    }

    /// Converts an AppKit frame into a top-left position counted from the primary
    /// screen's top left.
    fn top_left(&self, frame: NSRect) -> (f64, f64) {
        (
            frame.origin.x,
            self.primary_screen_height() - (frame.origin.y + frame.size.height),
        )
    }

    /// Moves the window's top left to (x, y). The units and direction are the same as
    /// [`position`](Self::position).
    ///
    /// Uses `setFrameTopLeftPoint:`. Once the position is decided,
    /// [`set_size`](Self::set_size) does not recenter the window on the screen. If it
    /// points off screen, AppKit may move it back on show until the title bar is
    /// visible.
    ///
    /// While maximized, minimized or in fullscreen, the state is left as is and the
    /// position for returning to normal is decided. While minimized, moving `frame`
    /// makes it the return position; for maximized it is moved after restoring, and for
    /// fullscreen on leaving.
    pub fn set_position(&self, x: f64, y: f64) {
        self.0.positioned.set(true);
        if self.in_fullscreen() || self.zoom_frame().is_some() {
            self.0.pending_position.set(Some((x, y)));
            return;
        }
        self.0.pending_position.set(None);
        self.0
            .native
            .setFrameTopLeftPoint(NSPoint::new(x, self.primary_screen_height() - y));
    }

    /// Called right before maximizing or restoring with `zoom:` (including the green
    /// button and double-clicking the title bar).
    ///
    /// When maximizing, the current frame is remembered as the restore target. When
    /// restoring, if a position was requested while maximized, it moves there after
    /// AppKit has finished restoring the original frame. Returning `false` and moving
    /// the frame instead of AppKit leaves AppKit believing it is still maximized, and
    /// the next time fullscreen is left it goes back to the maximized frame (confirmed
    /// on macOS 15). `zoom:` returns after its animation ends, so it moves on the next
    /// iteration of the main queue.
    fn should_zoom(&self) -> bool {
        let native = &self.0.native;
        if !native.isZoomed() {
            self.0.zoom_frame.set(Some(native.frame()));
            self.0.pending_position.set(None);
            self.0.pending_size.set(None);
            return true;
        }
        self.0.zoom_frame.set(None);
        if self.0.pending_position.get().is_some() || self.0.pending_size.get().is_some() {
            self.on_next_turn(|window| window.apply_pending_frame());
        }
        true
    }

    /// Applies a size and position requested while maximized or in fullscreen, once it
    /// is back to a normal frame.
    fn apply_pending_frame(&self) {
        if self.in_fullscreen() || self.0.native.isZoomed() {
            return;
        }
        if let Some((width, height)) = self.0.pending_size.take() {
            self.set_size(width, height);
        }
        if let Some((x, y)) = self.0.pending_position.take() {
            self.set_position(x, y);
        }
    }

    /// On the iteration after leaving fullscreen finishes, moves to the size and
    /// position promised during fullscreen.
    ///
    /// If none was requested, it goes back to the size and position before entering.
    /// AppKit should restore the frame from before entering, but on macOS 15 it
    /// sometimes forgets moves made with `setFrameTopLeftPoint:` and restores the frame
    /// from when it was shown. Moving inside the notification that leaving finished
    /// gets undone by AppKit, so it moves on the next iteration. If it came back to the
    /// maximized frame, the size and position are kept as the ones for restoring.
    fn restore_after_fullscreen(&self) {
        if self.0.fullscreen.get() || self.0.in_transition.get() {
            return;
        }
        let Some(return_position) = self.0.fullscreen_return.take() else {
            return;
        };
        let return_size = self.0.fullscreen_return_size.take();
        if let Some((width, height)) = self.0.pending_size.take().or(return_size) {
            if self.size() != (width, height) {
                self.set_size(width, height);
            }
        }
        let (x, y) = self.0.pending_position.take().unwrap_or(return_position);
        self.set_position(x, y);
    }

    /// Calls `f` after AppKit has finished delivering notifications (on the next
    /// iteration of the main queue).
    ///
    /// Changing the state or moving the frame from inside fullscreen or `zoom:`
    /// notifications is sometimes ignored or overwritten by AppKit, so it is deferred
    /// here.
    fn on_next_turn(&self, f: fn(&Window)) {
        let mtm = MainThreadMarker::from(&*self.0.native);
        let weak = MainThreadBound::new(Rc::downgrade(&self.0), mtm);
        DispatchQueue::main().exec_async(move || {
            // メインキューの仕事はメインスレッドで走る。
            let mtm = MainThreadMarker::new().expect("メインキューの外で呼ばれた");
            if let Some(inner) = weak.into_inner(mtm).upgrade() {
                f(&Window(inner));
            }
        });
    }

    /// The height of the primary screen. 0 if no screen is connected (positions stay as
    /// AppKit's values).
    fn primary_screen_height(&self) -> f64 {
        let mtm = MainThreadMarker::from(&*self.0.native);
        NSScreen::screens(mtm)
            .firstObject()
            .map_or(0.0, |screen| screen.frame().size.height)
    }

    /// The current display state.
    ///
    /// Before it is shown, it is the state requested with [`set_state`](Self::set_state)
    /// (the default is [`WindowState::Normal`]). While entering or leaving fullscreen,
    /// it still returns the previous state.
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

    /// Changes the display state. When called before it is shown, it is applied when
    /// shown.
    ///
    /// | Requested state | AppKit |
    /// | --- | --- |
    /// | `Normal` | Undoes minimize, fullscreen and zoom |
    /// | `Maximized` | `zoom:` (the same as clicking the green button with Option held) |
    /// | `Minimized` | `miniaturize:` |
    /// | `Fullscreen` | `toggleFullScreen:` |
    ///
    /// Entering and leaving fullscreen happen after an animation, so `state` changes
    /// after it too ([`on_state_change`](Self::on_state_change) is called when it
    /// changes). When moving from fullscreen to maximized or minimized, it is applied
    /// after leaving fullscreen has finished.
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
                    self.apply_pending_frame();
                }
            }
            WindowState::Minimized => native.miniaturize(None),
        }
        self.report_state();
    }

    /// The animation entering or leaving fullscreen has ended (or failed).
    ///
    /// Moving to the return position and applying the next state are deferred until
    /// AppKit has finished delivering notifications (the next iteration of the main
    /// queue). `toggleFullScreen:` called from inside the notification is ignored.
    fn finish_transition(&self) {
        let native = &self.0.native;
        self.0.in_transition.set(false);
        self.0
            .fullscreen
            .set(native.styleMask().contains(NSWindowStyleMask::FullScreen));
        self.apply_resizable();
        if !self.0.fullscreen.get() {
            // 続けて当てる状態より先に動かす (最大化するなら、動かした枠が
            // 元に戻す先になる)。
            self.on_next_turn(|window| window.restore_after_fullscreen());
        }
        self.report_state();
        if self.0.after_transition.get().is_none() {
            return;
        }
        self.on_next_turn(|window| {
            if let Some(next) = window.0.after_transition.take() {
                window.apply_state(next);
            }
        });
    }

    /// Notified when the display state changes. Receives the new
    /// [`state`](Self::state).
    ///
    /// Called both for user actions (the green button, putting it away in the Dock and
    /// so on) and for [`set_state`](Self::set_state).
    pub fn on_state_change(&self, f: impl FnMut(WindowState) + 'static) {
        self.0.on_state_change.set(f);
    }

    /// Calls `on_state_change` if the current state differs from the one last reported.
    ///
    /// While entering or leaving fullscreen, the frame expands to fill the screen and
    /// cannot be told apart from zoom, so nothing is reported (it is reported when it
    /// ends).
    fn report_state(&self) {
        if self.0.shown.get() && !self.0.in_transition.get() {
            self.0.on_state_change.update(self.state());
        }
    }

    /// Whether the user can resize the window by dragging its edges. `true` by default.
    pub fn is_resizable(&self) -> bool {
        self.0.resizable.get()
    }

    /// Decides whether the user can resize the window.
    ///
    /// Adds or removes `Resizable` in the styleMask. Removing it also disables zooming
    /// with the green button. [`set_size`](Self::set_size) works either way. The
    /// styleMask cannot be changed in fullscreen, so it is applied on leaving
    /// fullscreen.
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

    /// The widget placed at the root. Each call replaces it.
    ///
    /// With a sidebar attached, it is placed in the pane on its right.
    pub fn set_child(&self, child: &dyn Widget) {
        let view = child.native_view();
        let sidebar = self.0.sidebar.borrow().clone();
        match sidebar {
            Some(sidebar) => sidebar.set_content(Some(view)),
            None => self.0.native.setContentView(Some(&view)),
        }
        *self.0.child.borrow_mut() = Some(child.boxed_clone());
    }

    /// The sidebar attached to the left of the window. Each call replaces it.
    ///
    /// The window's `contentViewController` is replaced with an
    /// `NSSplitViewController`, and the child from [`set_child`](Self::set_child) moves
    /// to its right pane. To extend the sidebar under the title bar (the same shape as
    /// System Settings), `fullSizeContentView` is added to the window while it is
    /// attached. The child's top avoids the title bar, so the content looks the same.
    ///
    /// The content size ([`size`](Self::size)) does not change, and includes the
    /// sidebar's width (the window frame changes instead).
    pub fn set_sidebar(&self, sidebar: &Sidebar) {
        self.keeping_size(|| self.attach_sidebar(sidebar));
    }

    fn attach_sidebar(&self, sidebar: &Sidebar) {
        let (width, _) = self.size();
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
        // 狭いウィンドウは、両方の区画の最小に合わせて広がらないよう畳む。
        // 区画がこのウィンドウへ移ってから畳む (前に付いていた閉じたウィンドウの
        // 中で畳むと、AppKit がキーの巡回をたどって抜けなくなる)。
        sidebar.make_room(width);
        native.setFrame_display(frame, true);
        sidebar.set_content(child);
        sidebar.apply_width();
        *self.0.sidebar.borrow_mut() = Some(sidebar.clone());
        self.apply_toolbar();
    }

    /// Removes the attached sidebar. Does nothing if none is attached.
    ///
    /// The child in the right pane goes back to being the window's content. The content
    /// size does not change.
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

    /// The toolbar attached to the top of the window. Each call replaces it.
    ///
    /// In AppKit it is shown merged with the title bar, and when items do not fit,
    /// AppKit shows an overflow menu.
    ///
    /// **The title text is hidden.** Not showing the title in a window with a toolbar is
    /// the macOS convention; if it stayed, the title would take the leading space and
    /// push the items to the right edge. The text set with
    /// [`set_title`](Self::set_title) remains as the window's title (shown in the Window
    /// menu and Mission Control), and [`title`](Self::title) keeps returning it.
    ///
    /// With a sidebar attached, AppKit's standard sidebar button goes first (not
    /// counted in naui's item indices).
    ///
    /// The content size ([`size`](Self::size)) does not change; the window frame grows
    /// by the toolbar.
    pub fn set_toolbar(&self, toolbar: &Toolbar) {
        let old = self.0.toolbar.borrow_mut().replace(toolbar.clone());
        self.keeping_size(|| self.apply_toolbar());
        // 前のツールバーは、ウィンドウから外れてから元の並びへ戻す。
        if let Some(old) = old.filter(|old| !old.is_same(toolbar)) {
            old.set_sidebar_controls(false);
        }
    }

    /// Removes the attached toolbar. Does nothing if none is attached.
    ///
    /// The hidden title text is shown again.
    ///
    /// With a sidebar attached, a toolbar with just the sidebar button remains. The
    /// content size does not change.
    pub fn clear_toolbar(&self) {
        let old = self.0.toolbar.borrow_mut().take();
        self.keeping_size(|| self.apply_toolbar());
        if let Some(old) = old {
            old.set_sidebar_controls(false);
        }
    }

    /// Decides the window's toolbar from the combination of toolbar and sidebar.
    ///
    /// | App toolbar | Sidebar | Attached to the window |
    /// | --- | --- | --- |
    /// | Yes | Yes | The app's toolbar (with the sidebar button first) |
    /// | Yes | No | The app's toolbar |
    /// | No | Yes | A toolbar with just the sidebar button (the title stays shown) |
    /// | No | No | Nothing |
    ///
    /// Sidebar items are removed while the toolbar is detached from the window, and
    /// inserted after it is attached
    /// (`NSToolbarSidebarTrackingSeparatorItemIdentifier` does not go into a toolbar
    /// that is not attached).
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

    /// The menu bar shown at the top of the screen. Each call replaces it.
    ///
    /// **macOS has one menu bar per app**, so `NSApplication.mainMenu` is replaced.
    /// Whichever window it is called from, it applies to the whole app, regardless of
    /// whether the window is frontmost (it is a window API to match the other three
    /// platforms).
    pub fn set_menu_bar(&self, menu_bar: &MenuBar) {
        menu_bar.install();
        *self.0.menu_bar.borrow_mut() = Some(menu_bar.clone());
    }

    /// Removes the attached menu bar. Does nothing if none is attached.
    ///
    /// Without any menu, ⌘C / ⌘V would no longer be dispatched, so it goes back to
    /// naui's default menu (the app menu and Edit menu).
    pub fn clear_menu_bar(&self) {
        if let Some(old) = self.0.menu_bar.borrow_mut().take() {
            old.uninstall();
        }
    }

    /// Shows it on screen and brings it to the front.
    ///
    /// On first show, the state requested with [`set_state`](Self::set_state) before
    /// showing is applied.
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

    /// Switches this window's color theme.
    pub fn set_theme(&self, theme: Theme) -> Result<()> {
        let appearance = match theme {
            Theme::System => None,
            Theme::Light => unsafe { NSAppearance::appearanceNamed(NSAppearanceNameAqua) },
            Theme::Dark => unsafe { NSAppearance::appearanceNamed(NSAppearanceNameDarkAqua) },
        };
        self.0.native.setAppearance(appearance.as_deref());
        Ok(())
    }

    /// The real AppKit window. A backend-specific escape hatch.
    pub fn native_window(&self) -> Retained<NSWindow> {
        self.0.native.clone()
    }
}

/// The view overlaid on the window's content (where toasts are placed).
///
/// In a window with naui's sidebar, the `contentViewController` is an
/// `NSSplitViewController`, so its last pane (the content side) is returned. Adding
/// to the `contentView` itself (an `NSSplitView`) would lay it out as one of the
/// panes.
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

/// The frontmost of the windows naui created. `None` if there are none.
///
/// The window with focus is used; when none has focus yet (right after startup, or
/// in automated tests), **the last one created** is used. The same idea as how
/// `Dialog` picks its parent.
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
