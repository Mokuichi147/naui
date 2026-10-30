//! WinUI 3 Window handles.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::sync::Arc;

use naui_core::{Rect, Result, StateHandler, Theme, WindowPlacement, WindowState};
use naui_winui3::Microsoft::UI::Composition::ICompositionSupportsSystemBackdrop;
use naui_winui3::Microsoft::UI::Composition::SystemBackdrops::{
    MicaController, SystemBackdropConfiguration, SystemBackdropTheme,
};
use naui_winui3::Microsoft::UI::Windowing::{
    AppWindow, AppWindowChangedEventArgs, AppWindowPresenterKind, OverlappedPresenter,
    OverlappedPresenterState,
};
use naui_winui3::Microsoft::UI::Xaml::Controls::TextBlock;
use naui_winui3::Microsoft::UI::Xaml::Input::KeyEventHandler;
use naui_winui3::Microsoft::UI::Xaml::Markup::XamlReader;
use naui_winui3::Microsoft::UI::Xaml::Media::MicaBackdrop;
use naui_winui3::Microsoft::UI::Xaml::{
    Application, ApplicationTheme, Controls::Grid, ElementTheme, FrameworkElement,
    SizeChangedEventHandler, UIElement, Window as XamlWindow,
};
use windows::Foundation::TypedEventHandler;
use windows::System::VirtualKey;
use windows_core::{Interface, HSTRING};

use crate::menu_bar::MenuBar;
use crate::sidebar::Sidebar;
use crate::to_error;
use crate::toolbar::Toolbar;
use crate::ui_thread::UiThreadCell;
use crate::widgets::Widget;
use crate::Ui;

enum Backdrop {
    Controller {
        controller: MicaController,
        configuration: SystemBackdropConfiguration,
    },
    BuiltIn {
        _mica: MicaBackdrop,
    },
    /// When Mica could not be attached. The window is shown with the default
    /// background.
    None,
}

/// The width left free at the right end of the title bar. It is where the minimize,
/// maximize and close buttons go, and is the same value as the title bar XAML's
/// `Padding`.
const CAPTION_RESERVE: f64 = 140.0;

/// The title bar's height. This is the height of the minimize, maximize and close
/// buttons; even with `SetExtendsContentIntoTitleBar(true)`, the system draws just
/// the buttons at this height (`AppWindowTitleBar::PreferredHeightOption`, which
/// makes it 48, is not in the [`naui_winui3`] projection). The title text and the
/// toolbar also fit inside it, so the three line up at the same height.
pub(crate) const CAPTION_HEIGHT: f64 = 32.0;

/// The row of the window root's `Grid` that holds the menu bar's slot.
///
/// The row order is decided by the XAML of [`themed_content_root`]. The XAML lays out
/// its children in row order, so this number is also the child's position in the
/// root. Code that walks the root's children (where toasts are overlaid, for
/// example) uses this rather than writing the number directly.
const MENU_BAR_ROW: u32 = 1;

/// The row of the window root's `Grid` that holds the app's content and toasts.
const CONTENT_ROW: u32 = 2;

/// The title text size. 12, the same as WinUI 3's caption text
/// (`CaptionTextBlockStyle`). The body size of 14 would be large for the
/// [`CAPTION_HEIGHT`] band.
const TITLE_FONT_SIZE: f64 = 12.0;

struct WindowInner {
    native: XamlWindow,
    backdrop: Backdrop,
    child: RefCell<Option<Box<dyn Widget>>>,
    theme_root: RefCell<Option<UIElement>>,
    title_label: RefCell<Option<TextBlock>>,
    /// The container for the toolbar to the right of the title.
    toolbar_host: RefCell<Option<Grid>>,
    /// The container for the menu bar below the title bar.
    menu_bar_host: RefCell<Option<Grid>>,
    /// The layer holding the app's content and toasts ([`CONTENT_ROW`]). The first child
    /// is the content (the `NavigationView` if a sidebar is attached).
    content_layer: RefCell<Option<Grid>>,
    /// The candidate for the drag region. Without a toolbar, the whole title bar is
    /// used; with one, only the free space to the right of the toolbar (so the window
    /// does not move when dragging over the buttons).
    title_bar: RefCell<Option<UIElement>>,
    drag_area: RefCell<Option<UIElement>>,
    /// The attached toolbar. Kept alive along with its handlers.
    toolbar: RefCell<Option<Toolbar>>,
    /// The attached menu bar. Kept alive along with its handlers.
    menu_bar: RefCell<Option<MenuBar>>,
    /// The attached sidebar. Kept alive along with its handlers.
    sidebar: RefCell<Option<Sidebar>>,
    visible: RefCell<bool>,
    theme: Cell<Theme>,
    /// The content size (DIP) to apply on the first [`Window::show`]. It starts as the
    /// width and height given to `ui.window` and is replaced by `set_size` before
    /// showing. Emptied once shown.
    initial_size: Cell<Option<(f64, f64)>>,
    /// The content size requested before the content was laid out. Before layout the
    /// menu bar's height is unknown and the outer frame can only be estimated, so it is
    /// refitted once with the actual size when layout finishes.
    pending_fit: Cell<Option<(f64, f64)>>,
    /// The content size (DIP) last seen in the normal state. While maximized,
    /// minimized or in fullscreen, [`Window::placement`] returns it.
    normal_size: Cell<(f64, f64)>,
    /// The content size requested with [`Window::set_size`] while maximized,
    /// minimized or in fullscreen. Applied when the window is back to normal.
    pending_size: Cell<Option<(f64, f64)>>,
    wheel_subclass_installed: Cell<bool>,
    /// Whether the `KeyDown` that picks up shortcuts has been attached.
    menu_key_installed: Cell<bool>,
    /// The `on_key_down` handler. Called from the root's `PreviewKeyDown`.
    key_down: Rc<naui_core::KeyHandler>,
    /// The `on_file_drop` handler. Called from the root's `Drop` when no widget under
    /// the pointer takes the files.
    on_file_drop: Rc<crate::file_drop::FileDropHandler>,
    /// Runs the reading of dropped files (it completes asynchronously) on the UI
    /// thread.
    tasks: naui_core::Tasks,
    closing_token: Cell<Option<i64>>,
    on_close_request: naui_core::CloseHandler,
    on_resize: RefCell<Option<Box<ResizeCallback>>>,
    /// The size last passed to `on_resize`. Kept so it is not called twice for the same
    /// size.
    last_size: Cell<Option<(f64, f64)>>,
    /// The content minimum requested with `set_min_size`. It is converted to the outer
    /// frame's size and passed to the `OverlappedPresenter`, so it is passed again
    /// whenever the frame or band heights change.
    min_size: Cell<Option<(f64, f64)>>,
    on_state_change: StateHandler,
    /// The state requested before showing. Applied on the first [`Window::show`].
    /// Emptied once shown.
    initial_state: Cell<Option<WindowState>>,
    /// The `OverlappedPresenter` the window originally had. It is reattached when
    /// returning from fullscreen (`FullScreenPresenter`), so the minimum and resizable
    /// settings are preserved.
    overlapped: RefCell<Option<OverlappedPresenter>>,
    /// The value requested with [`Window::set_resizable`]. Before the first show,
    /// `AppWindow.Presenter` may be empty, so it is remembered and applied when the
    /// `OverlappedPresenter` is first obtained.
    resizable: Cell<bool>,
    /// The position [`Window::position`] returns while in fullscreen: where it goes back
    /// to when leaving (physical pixels). Read when entering fullscreen.
    fullscreen_return: Cell<Option<(i32, i32)>>,
    /// The position (physical pixels) requested with [`Window::set_position`] while in
    /// fullscreen. Applied on leaving fullscreen.
    position_after_fullscreen: Cell<Option<(i32, i32)>>,
    /// The `XamlRoot` whose `Changed` is subscribed to. Subscribed only once.
    scale_root: RefCell<Option<naui_winui3::Microsoft::UI::Xaml::XamlRoot>>,
    /// The last `RasterizationScale` seen. Kept so that the physical-pixel values (the
    /// minimum and the drag region) are passed again only when the scale changes.
    last_scale: Cell<f64>,
}

type ResizeCallback = dyn FnMut((f64, f64));

impl Drop for WindowInner {
    fn drop(&mut self) {
        if let Ok(child) = self.child.try_borrow() {
            if let Some(child) = child.as_ref() {
                crate::layout::clear_window_parent(&child.native_element(), &self.native);
            }
        }
    }
}

/// A top-level window.
#[derive(Clone)]
pub struct Window(Rc<WindowInner>);

/// A weak reference for referring to a `Window` from event handlers without holding
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

    pub(crate) fn new(
        title: &str,
        width: f64,
        height: f64,
        theme: Theme,
        tasks: naui_core::Tasks,
    ) -> Result<Self> {
        let native = XamlWindow::new().map_err(|e| to_error("create the Window", e))?;
        native
            .SetTitle(&HSTRING::from(title))
            .map_err(|e| to_error("set the Window title", e))?;
        let backdrop = create_backdrop(&native, theme);

        let this = Self(Rc::new(WindowInner {
            native,
            backdrop,
            child: RefCell::new(None),
            theme_root: RefCell::new(None),
            toolbar_host: RefCell::new(None),
            menu_bar_host: RefCell::new(None),
            content_layer: RefCell::new(None),
            title_bar: RefCell::new(None),
            drag_area: RefCell::new(None),
            toolbar: RefCell::new(None),
            menu_bar: RefCell::new(None),
            sidebar: RefCell::new(None),
            title_label: RefCell::new(None),
            visible: RefCell::new(false),
            theme: Cell::new(theme),
            initial_size: Cell::new(Some((width.max(0.0), height.max(0.0)))),
            pending_fit: Cell::new(None),
            normal_size: Cell::new((width.max(0.0), height.max(0.0))),
            pending_size: Cell::new(None),
            wheel_subclass_installed: Cell::new(false),
            menu_key_installed: Cell::new(false),
            key_down: Rc::default(),
            on_file_drop: Rc::default(),
            tasks,
            closing_token: Cell::new(None),
            on_close_request: naui_core::CloseHandler::default(),
            on_resize: RefCell::new(None),
            last_size: Cell::new(None),
            min_size: Cell::new(None),
            on_state_change: StateHandler::default(),
            initial_state: Cell::new(Some(WindowState::Normal)),
            overlapped: RefCell::new(None),
            resizable: Cell::new(true),
            fullscreen_return: Cell::new(None),
            position_after_fullscreen: Cell::new(None),
            scale_root: RefCell::new(None),
            last_scale: Cell::new(0.0),
        }));
        this.observe_size();
        this.observe_app_window();
        Ok(this)
    }

    fn observe_size(&self) {
        let state = UiThreadCell::new(Rc::downgrade(&self.0));
        let handler = TypedEventHandler::<
            windows_core::IInspectable,
            naui_winui3::Microsoft::UI::Xaml::WindowSizeChangedEventArgs,
        >::new(move |_, _| {
            if let Some(inner) = state.try_with_mut(|weak| weak.upgrade()).flatten() {
                // 中身の層があれば、並べ終わった後に層の `SizeChanged` が知らせる
                // (ここではまだ古い大きさのまま)。
                if inner.content_layer.borrow().is_none() {
                    Window(inner).content_size_changed();
                }
            }
            Ok(())
        });
        let _ = self.0.native.SizeChanged(&handler);
    }

    /// Picks up entering and leaving maximized, minimized and fullscreen through
    /// `AppWindow.Changed`.
    ///
    /// State changes show up in `DidPresenterChange` (fullscreen) or `DidSizeChange`
    /// (maximized, minimized), so the state is read again for either.
    fn observe_app_window(&self) {
        let Ok(app_window) = self.0.native.AppWindow() else {
            return;
        };
        let state = UiThreadCell::new(Rc::downgrade(&self.0));
        let handler =
            TypedEventHandler::<AppWindow, AppWindowChangedEventArgs>::new(move |_, _| {
                if let Some(inner) = state.try_with_mut(|weak| weak.upgrade()).flatten() {
                    Window(inner).report_state();
                }
                Ok(())
            });
        let _ = app_window.Changed(&handler);
    }

    /// The window's `OverlappedPresenter`. The first one read is remembered (while in
    /// fullscreen, `AppWindow.Presenter` is a `FullScreenPresenter`).
    fn overlapped_presenter(&self) -> Option<OverlappedPresenter> {
        if let Some(presenter) = self.0.overlapped.borrow().as_ref() {
            return Some(presenter.clone());
        }
        let presenter = self
            .0
            .native
            .AppWindow()
            .and_then(|app_window| app_window.Presenter())
            .and_then(|presenter| presenter.cast::<OverlappedPresenter>())
            .ok()?;
        *self.0.overlapped.borrow_mut() = Some(presenter.clone());
        // 取れる前に頼まれた `set_resizable` をここで当てる。
        apply_resizable(&presenter, self.0.resizable.get());
        Some(presenter)
    }

    /// Whether fullscreen (`FullScreenPresenter`) is attached.
    fn is_fullscreen(&self) -> bool {
        self.0
            .native
            .AppWindow()
            .and_then(|app_window| app_window.Presenter())
            .and_then(|presenter| presenter.Kind())
            .is_ok_and(|kind| kind == AppWindowPresenterKind::FullScreen)
    }

    /// Called when the content size may have changed. Passes the minimum again and, if
    /// the size changed, reports it to `on_resize`.
    fn content_size_changed(&self) {
        self.watch_rasterization_scale();
        self.apply_min_size();
        self.finish_pending_fit();
        let size = self.size();
        // 中身の層が無ければ `Bounds` がそのまま中身なので、並ぶのを待たない。
        let measured =
            self.0.content_layer.borrow().is_none() || self.laid_out_content_size().is_some();
        if self.0.initial_size.get().is_none()
            && self.native_state() == WindowState::Normal
            && measured
        {
            self.0.normal_size.set(size);
        }
        if self.0.last_size.replace(Some(size)) == Some(size) {
            return;
        }
        // 通知の中で差し替えられても二重借用にならないよう、取り出して呼ぶ。
        let taken = self.0.on_resize.borrow_mut().take();
        if let Some(mut f) = taken {
            f(size);
            let mut slot = self.0.on_resize.borrow_mut();
            if slot.is_none() {
                *slot = Some(f);
            }
        }
    }

    /// The size of the laid-out content layer. `None` if there is no layer yet or before
    /// layout.
    fn laid_out_content_size(&self) -> Option<(f64, f64)> {
        let layer = self.0.content_layer.borrow();
        let layer = layer.as_ref()?;
        let size = (layer.ActualWidth().ok()?, layer.ActualHeight().ok()?);
        (size.0 > 0.0 || size.1 > 0.0).then_some(size)
    }

    /// Converts a content size (DIP) into an outer frame size (physical pixels).
    ///
    /// Both `AppWindow` and the `OverlappedPresenter`'s minimum take the outer frame in
    /// physical pixels, so it is multiplied by `RasterizationScale` and the current
    /// difference between the outer frame and the content (the left and right borders,
    /// title bar and menu bar) is added. Before the content layer is laid out, the
    /// difference comes from the estimate of [`measured_size`](Self::measured_size), so
    /// the menu bar's share is missing.
    fn outer_size_for(
        &self,
        app_window: &naui_winui3::Microsoft::UI::Windowing::AppWindow,
        (width, height): (f64, f64),
    ) -> Option<(f64, f64)> {
        let outer = app_window.Size().ok()?;
        let (content_width, content_height) = self.measured_size();
        let scale = self.rasterization_scale();
        let extra_width = (f64::from(outer.Width) - content_width * scale).max(0.0);
        let extra_height = (f64::from(outer.Height) - content_height * scale).max(0.0);
        Some((
            width.max(0.0) * scale + extra_width,
            height.max(0.0) * scale + extra_height,
        ))
    }

    /// Changes the outer frame with `AppWindow.Resize` so that the content becomes
    /// `size` (DIP).
    fn resize_content(&self, size: (f64, f64)) {
        use windows::Graphics::SizeInt32;

        let Ok(app_window) = self.0.native.AppWindow() else {
            return;
        };
        let Some((width, height)) = self.outer_size_for(&app_window, size) else {
            return;
        };
        let _ = app_window.Resize(SizeInt32 {
            Width: width.round() as i32,
            Height: height.round() as i32,
        });
        // 並ぶ前の見積もりは、並べ終わってから実寸で合わせ直す。層が無ければ
        // 帯もメニューバーも無く、見積もり (`Bounds`) がそのまま中身なので要らない。
        if self.0.content_layer.borrow().is_some() && self.laid_out_content_size().is_none() {
            self.0.pending_fit.set(Some(size));
        }
    }

    /// Refits once, with the actual laid-out size, to the size requested before layout.
    ///
    /// If it was laid out maximized, minimized or in fullscreen (a state requested
    /// before showing), the refit waits until the window is back to normal, the same
    /// as a [`set_size`](Self::set_size) there.
    fn finish_pending_fit(&self) {
        let Some(target) = self.0.pending_fit.get() else {
            return;
        };
        let Some(size) = self.laid_out_content_size() else {
            return;
        };
        self.0.pending_fit.set(None);
        if self.native_state() != WindowState::Normal {
            if self.0.pending_size.get().is_none() {
                self.0.pending_size.set(Some(target));
            }
            return;
        }
        if (size.0 - target.0).abs() >= 0.5 || (size.1 - target.1).abs() >= 0.5 {
            self.resize_content(target);
        }
    }

    /// Converts the `set_min_size` minimum (content) into the outer frame's size and
    /// passes it.
    ///
    /// It is called every time the content layer is laid out, so it keeps up even when
    /// band heights change.
    fn apply_min_size(&self) {
        use windows::Foundation::{IReference, PropertyValue};

        let Some(min_size) = self.0.min_size.get() else {
            return;
        };
        let Ok(app_window) = self.0.native.AppWindow() else {
            return;
        };
        let Some(presenter) = self.overlapped_presenter() else {
            return;
        };
        let Some((width, height)) = self.outer_size_for(&app_window, min_size) else {
            return;
        };
        let value = |pixels: f64| {
            PropertyValue::CreateInt32(pixels.ceil() as i32)
                .and_then(|value| value.cast::<IReference<i32>>())
                .ok()
        };
        if let (Some(width), Some(height)) = (value(width), value(height)) {
            let _ = presenter.SetPreferredMinimumWidth(&width);
            let _ = presenter.SetPreferredMinimumHeight(&height);
        }
    }

    /// How many physical pixels one logical pixel (DIP) is.
    ///
    /// Before the content's `XamlRoot` is decided (before showing), it is derived from
    /// the window's DPI. [`run`](crate::run) makes the process Per-Monitor V2, but if
    /// the user's manifest chooses DPI unaware, DPI comes back as 96 and the
    /// `AppWindow`'s sizes are also virtualized to 96 DPI values, so either way it works
    /// out to 1.
    fn rasterization_scale(&self) -> f64 {
        self.0
            .theme_root
            .borrow()
            .as_ref()
            .and_then(|root| root.XamlRoot().ok())
            .and_then(|root| root.RasterizationScale().ok())
            .filter(|scale| *scale > 0.0)
            .or_else(|| self.window_dpi().map(|dpi| f64::from(dpi) / 96.0))
            .unwrap_or(1.0)
    }

    /// The value of `GetDpiForWindow`. `None` if the HWND cannot be obtained.
    fn window_dpi(&self) -> Option<u32> {
        use windows::Win32::UI::HiDpi::GetDpiForWindow;

        let hwnd = self.hwnd()?;
        // SAFETY: このウィンドウの HWND を渡すだけ。
        let dpi = unsafe { GetDpiForWindow(hwnd) };
        (dpi > 0).then_some(dpi)
    }

    /// This window's HWND. `None` if it cannot be obtained.
    pub(crate) fn hwnd(&self) -> Option<windows::Win32::Foundation::HWND> {
        let native = self.0.native.cast::<naui_winui3::IWindowNative>().ok()?;
        // SAFETY: HWND はこのウィンドウが生きている間有効で、UI スレッドから呼んでいる。
        unsafe { native.WindowHandle() }.ok()
    }

    /// Passes the physical-pixel values again when the display scale changes.
    ///
    /// The minimum ([`apply_min_size`](Self::apply_min_size)) and the drag region
    /// ([`apply_drag_rectangles`](Self::apply_drag_rectangles)) are fixed
    /// physical-pixel values, so they go wrong when the window moves to a monitor with
    /// a different scale or the scale is changed in the settings. The OS widens the
    /// outer frame to the new scale while the content's size in DIPs barely changes,
    /// so the content's `SizeChanged` alone does not notice. `XamlRoot.Changed`, which
    /// reports scale changes, is watched instead.
    ///
    /// The `XamlRoot` is only available once the content is laid out, so this is called
    /// from [`content_size_changed`](Self::content_size_changed) and subscribes only the
    /// first time it is available.
    fn watch_rasterization_scale(&self) {
        use naui_winui3::Microsoft::UI::Xaml::{XamlRoot, XamlRootChangedEventArgs};

        if self.0.scale_root.borrow().is_some() {
            return;
        }
        let root = self
            .0
            .theme_root
            .borrow()
            .as_ref()
            .and_then(|element| element.XamlRoot().ok());
        let Some(root) = root else {
            return;
        };
        self.0
            .last_scale
            .set(root.RasterizationScale().unwrap_or(0.0));
        let state = UiThreadCell::new(Rc::downgrade(&self.0));
        let handler =
            TypedEventHandler::<XamlRoot, XamlRootChangedEventArgs>::new(move |sender, _| {
                let Some(scale) = sender
                    .as_ref()
                    .and_then(|root| root.RasterizationScale().ok())
                else {
                    return Ok(());
                };
                let _ = state.try_with_mut(|weak| {
                    if let Some(inner) = weak.upgrade() {
                        if inner.last_scale.replace(scale) != scale {
                            Window(inner).scale_changed();
                        }
                    }
                });
                Ok(())
            });
        if root.Changed(&handler).is_ok() {
            *self.0.scale_root.borrow_mut() = Some(root);
        }
    }

    /// Passes the minimum and the drag region again at the new scale.
    ///
    /// `XamlRoot.Changed` can arrive before the OS has finished resizing the outer
    /// frame to the new scale with `WM_DPICHANGED`. So that
    /// [`outer_size_for`](Self::outer_size_for), which derives the frame thickness
    /// from the outer size, does not read the old frame, the computation waits for
    /// one round of the message loop.
    fn scale_changed(&self) {
        use naui_winui3::Microsoft::UI::Dispatching::{DispatcherQueue, DispatcherQueueHandler};

        let target = UiThreadCell::new(Rc::downgrade(&self.0));
        let enqueued = DispatcherQueue::GetForCurrentThread().and_then(|queue| {
            queue.TryEnqueue(&DispatcherQueueHandler::new(move || {
                let _ = target.try_with_mut(|weak| {
                    if let Some(inner) = weak.upgrade() {
                        let window = Window(inner);
                        window.apply_min_size();
                        window.apply_drag_area();
                    }
                });
                Ok(())
            }))
        });
        if !matches!(enqueued, Ok(true)) {
            // 積めなければ、その場で渡し直す。
            self.apply_min_size();
            self.apply_drag_area();
        }
    }

    /// Reports when the content layer is laid out again and its size changes.
    ///
    /// Besides the window's size, attaching or detaching the menu bar also changes the
    /// content. Both show up in the layer's `SizeChanged`, by which time the actual size
    /// is known. The layer is recreated on every `set_child`, so it is attached each
    /// time.
    fn watch_content_layer(&self) {
        let layer = self.0.content_layer.borrow().clone();
        let Some(layer) = layer else {
            return;
        };
        let state = UiThreadCell::new(Rc::downgrade(&self.0));
        let changed = SizeChangedEventHandler::new(move |_, _| {
            if let Some(inner) = state.try_with_mut(|weak| weak.upgrade()).flatten() {
                Window(inner).content_size_changed();
            }
            Ok(())
        });
        let _ = layer.SizeChanged(&changed);
    }

    /// Confirmation when the user tries to close the window.
    ///
    /// Returning [`CloseResponse::KeepOpen`](naui_core::CloseResponse::KeepOpen) keeps it
    /// open (to ask for confirmation when there are unsaved changes, for example). Not
    /// called for [`close`](Self::close) from code (`Window::Close` does not go through
    /// the `AppWindow`'s `Closing`).
    pub fn on_close_request(&self, f: impl FnMut() -> naui_core::CloseResponse + 'static) {
        self.0.on_close_request.set(f);
    }

    /// The size of the content area (width, height) in logical pixels (DIP).
    ///
    /// As on the other platforms, it excludes the title bar and menu bar. naui extends
    /// the content behind the title bar and draws the bands itself, so this returns the
    /// actual size of the layer children are placed in (the row below the bands), not
    /// `Window.Bounds`. The layer is laid out again in the layout after the size
    /// changes, so right after [`set_size`](Self::set_size) it still has the previous
    /// size (the same as GTK). The changed size arrives through
    /// [`on_resize`](Self::on_resize).
    ///
    /// Before it is on screen, this is the size given to `ui.window` or
    /// [`set_size`](Self::set_size) (the same as GTK).
    pub fn size(&self) -> (f64, f64) {
        self.0
            .initial_size
            .get()
            .unwrap_or_else(|| self.measured_size())
    }

    /// The current content size. Before layout, an estimate from `Bounds`.
    fn measured_size(&self) -> (f64, f64) {
        if let Some(size) = self.laid_out_content_size() {
            return size;
        }
        // 並べる前。中身を広げていなければ `Bounds` がそのまま中身。広げた後
        // なら、少なくともタイトルバーの帯の分は中身ではない。
        let caption = if self.0.title_bar.borrow().is_some() {
            CAPTION_HEIGHT
        } else {
            0.0
        };
        self.0.native.Bounds().map_or((0.0, 0.0), |bounds| {
            (
                f64::from(bounds.Width),
                (f64::from(bounds.Height) - caption).max(0.0),
            )
        })
    }

    /// Notified when the size changes. Receives the new [`size`](Self::size).
    pub fn on_resize(&self, f: impl FnMut((f64, f64)) + 'static) {
        *self.0.on_resize.borrow_mut() = Some(Box::new(f));
    }

    /// Called with the files dropped onto the window from Explorer or another app.
    ///
    /// A widget under the pointer with its own `on_file_drop` takes the drop
    /// instead. Text fields (`TextBox`) handle drags themselves, so drops onto them
    /// do not arrive. Files without a path (such as the inside of a zip shown by
    /// Explorer) are left out. The closure is called after the files have been read,
    /// a moment after the drop. Setting it again replaces the previous closure.
    pub fn on_file_drop(&self, f: impl FnMut(&[naui_core::FileEntry]) + 'static) {
        self.0.on_file_drop.set(f);
        self.ensure_drop_root();
    }

    /// Gives a window without a child an empty root to receive drops on.
    ///
    /// The receiver is normally attached to the root built by
    /// [`set_child`](Self::set_child), but a window without a child has no content
    /// at all. A later `set_child` replaces this root and attaches the receiver
    /// again.
    fn ensure_drop_root(&self) {
        if self.0.native.Content().is_ok() {
            return;
        }
        // 背景が無いと当たり判定に入らず、ドラッグのイベントが来ない。
        let root = XamlReader::Load(&HSTRING::from(
            r#"<Grid xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation" Background="Transparent"/>"#,
        ))
        .and_then(|root| root.cast::<UIElement>());
        let (Ok(root), Some(hwnd)) = (root, self.hwnd()) else {
            return;
        };
        if self.0.native.SetContent(&root).is_ok() {
            let _ = crate::file_drop::install(
                &root,
                hwnd,
                self.0.on_file_drop.clone(),
                self.0.tasks.clone(),
            );
        }
    }

    pub fn set_title(&self, title: &str) {
        let _ = self.0.native.SetTitle(&HSTRING::from(title));
        if let Some(label) = self.0.title_label.borrow().as_ref() {
            let _ = label.SetText(&HSTRING::from(title));
        }
    }

    pub fn title(&self) -> String {
        self.0
            .native
            .Title()
            .map(|s| s.to_string())
            .unwrap_or_default()
    }

    /// The minimum size the user can shrink it to (content width, height). The units
    /// are the same as [`size`](Self::size).
    ///
    /// Prevents the layout from breaking when it is made too small.
    ///
    /// It uses `OverlappedPresenter.PreferredMinimumWidth` / `Height`, so it has no
    /// effect with runtimes older than Windows App SDK 1.7.
    pub fn set_min_size(&self, width: f64, height: f64) {
        self.0.min_size.set(Some((width, height)));
        self.apply_min_size();
    }

    /// Changes the size of the content area. The units and extent are the same as
    /// [`size`](Self::size), excluding the title bar, menu bar and window frame (the
    /// same as the other platforms).
    ///
    /// `AppWindow.Resize` takes the outer frame in physical pixels, so it is multiplied
    /// by `RasterizationScale` and the current difference between the outer frame and
    /// the content is added. Before the content is laid out (right after showing, for
    /// example) the menu bar's height is unknown, so it is refitted once with the actual
    /// size when layout finishes. When called before it is on screen, it is shown at this
    /// size.
    ///
    /// While maximized, minimized or in fullscreen, the state is left as is and the
    /// size for returning to normal is decided, the same as
    /// [`set_position`](Self::set_position). It is applied when the window is back to
    /// normal.
    pub fn set_size(&self, width: f64, height: f64) {
        let size = (width.max(0.0), height.max(0.0));
        if self.0.initial_size.get().is_some() {
            self.0.initial_size.set(Some(size));
            return;
        }
        if self.native_state() != WindowState::Normal {
            self.0.pending_size.set(Some(size));
            return;
        }
        self.0.pending_size.set(None);
        self.0.pending_fit.set(None);
        self.resize_content(size);
    }

    /// The size and position to save, and the state to show it in next time.
    ///
    /// The size and position are those of the normal state, the same as
    /// [`position`](Self::position): while maximized, minimized or in fullscreen, the
    /// size is the content size last seen in the normal state (or the one requested
    /// with [`set_size`](Self::set_size) since). A minimized window reports the state
    /// it returns to (`GetWindowPlacement`'s `WPF_RESTORETOMAXIMIZED`).
    ///
    /// The position is divided by the primary monitor's scale, not this window's, so
    /// it points to the same place on screen whichever monitor the window is on when
    /// saved and when restored (the window is created on the primary monitor, and
    /// monitors with different DPIs would otherwise count it differently). On the
    /// primary monitor, or one with the same scale, it is the same as
    /// [`position`](Self::position).
    pub fn placement(&self) -> WindowPlacement {
        let normal = self.native_state() == WindowState::Normal;
        let size = match (self.0.initial_size.get(), self.0.pending_size.get()) {
            (Some(size), _) | (None, Some(size)) => size,
            (None, None) if normal => self.size(),
            (None, None) => self.0.normal_size.get(),
        };
        let scale = self.placement_scale();
        WindowPlacement {
            size,
            position: self
                .physical_position()
                .map(|(x, y)| (f64::from(x) / scale, f64::from(y) / scale)),
            state: self.state_to_restore(),
        }
    }

    /// The scale [`placement`](Self::placement) counts the position with: the primary
    /// monitor's (`GetDpiForMonitor`), or this window's if it cannot be read.
    fn placement_scale(&self) -> f64 {
        use windows::Win32::Foundation::POINT;
        use windows::Win32::Graphics::Gdi::{MonitorFromPoint, MONITOR_DEFAULTTOPRIMARY};

        // SAFETY: 主画面を引くだけ。(0, 0) は常に主画面の左上。
        let monitor = unsafe { MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY) };
        monitor_scale(monitor).unwrap_or_else(|| self.rasterization_scale())
    }

    /// [`state`](Self::state), with minimized replaced by the state it returns to.
    fn state_to_restore(&self) -> WindowState {
        use windows::Win32::UI::WindowsAndMessaging::WPF_RESTORETOMAXIMIZED;

        match self.state() {
            WindowState::Minimized => match self.placement_info() {
                Some((_, placement)) if placement.flags.0 & WPF_RESTORETOMAXIMIZED.0 != 0 => {
                    WindowState::Maximized
                }
                _ => WindowState::Normal,
            },
            state => state,
        }
    }

    /// Applies a saved [`placement`](Self::placement): the size, then the position, then
    /// the state.
    ///
    /// The position is used only if the top edge of the window would be on one of the
    /// monitors connected now (their work areas, `MONITORINFO.rcWork`). Otherwise, such
    /// as when the monitor it was on has been disconnected, it is placed as if no
    /// position had been given.
    pub fn set_placement(&self, placement: &WindowPlacement) {
        let (width, height) = placement.size;
        self.set_size(width, height);
        if let Some(position) = placement.position {
            let physical = to_physical(position, self.placement_scale());
            if is_on_some_monitor(physical, width) {
                self.set_physical_position(physical);
            }
        }
        self.set_state(placement.state);
    }

    /// The position (x, y) of the window's top-left corner, in logical pixels (DIP).
    ///
    /// Counted right and down from the top left of the primary screen (the same
    /// direction as the other platforms). It is `AppWindow.Position` (the outer frame in
    /// physical pixels) divided by `RasterizationScale`. The outer frame includes the
    /// invisible border Windows adds outside the edge (the resize handles), so it is a
    /// little up and left of the visible frame's top left. Across screens with different
    /// DPIs, it is the value divided by this window's DPI.
    ///
    /// While maximized, minimized or in fullscreen, it returns the position it will have
    /// when back to normal: `GetWindowPlacement`'s `rcNormalPosition` for maximized and
    /// minimized, and the position read before entering for fullscreen.
    pub fn position(&self) -> Option<(f64, f64)> {
        let scale = self.rasterization_scale();
        let (x, y) = self.physical_position()?;
        Some((f64::from(x) / scale, f64::from(y) / scale))
    }

    /// [`position`](Self::position) in physical pixels, before dividing by the scale.
    fn physical_position(&self) -> Option<(i32, i32)> {
        match self.native_state() {
            WindowState::Normal => {
                let position = self.0.native.AppWindow().ok()?.Position().ok()?;
                Some((position.X, position.Y))
            }
            WindowState::Fullscreen => {
                if let Some(position) = self.0.position_after_fullscreen.get() {
                    return Some(position);
                }
                if let Some(position) = self.0.fullscreen_return.get() {
                    return Some(position);
                }
                let position = self.0.native.AppWindow().ok()?.Position().ok()?;
                Some((position.X, position.Y))
            }
            WindowState::Maximized | WindowState::Minimized => {
                let (_, placement) = self.placement_info()?;
                let rect = placement.rcNormalPosition;
                Some((rect.left, rect.top))
            }
        }
    }

    /// Moves the window's top left to (x, y). The units and direction are the same as
    /// [`position`](Self::position).
    ///
    /// Uses `AppWindow.Move`. It may be called before it is on screen.
    ///
    /// While maximized, minimized or in fullscreen, the state is left as is and the
    /// position for returning to normal is decided. For maximized and minimized,
    /// `SetWindowPlacement` rewrites `rcNormalPosition`; for fullscreen it is moved on
    /// leaving.
    pub fn set_position(&self, x: f64, y: f64) {
        self.set_physical_position(to_physical((x, y), self.rasterization_scale()));
    }

    /// [`set_position`](Self::set_position) with a position already in physical pixels.
    fn set_physical_position(&self, physical: (i32, i32)) {
        match self.native_state() {
            WindowState::Normal => {
                let Ok(app_window) = self.0.native.AppWindow() else {
                    return;
                };
                let _ = app_window.Move(windows::Graphics::PointInt32 {
                    X: physical.0,
                    Y: physical.1,
                });
            }
            WindowState::Fullscreen => self.0.position_after_fullscreen.set(Some(physical)),
            WindowState::Maximized | WindowState::Minimized => {
                self.set_normal_position(physical);
            }
        }
    }

    /// Returns the value of `GetWindowPlacement` converted into screen coordinates
    /// (physical pixels).
    ///
    /// `rcNormalPosition` is in workspace coordinates (offset by the work area excluding
    /// the taskbar and the like), so the difference between the work area of the screen
    /// the window is on and that screen's top left is added.
    fn placement_info(
        &self,
    ) -> Option<(
        windows::Win32::Foundation::HWND,
        windows::Win32::UI::WindowsAndMessaging::WINDOWPLACEMENT,
    )> {
        use windows::Win32::UI::WindowsAndMessaging::{GetWindowPlacement, WINDOWPLACEMENT};

        let hwnd = self.hwnd()?;
        let mut placement = WINDOWPLACEMENT {
            length: std::mem::size_of::<WINDOWPLACEMENT>() as u32,
            ..Default::default()
        };
        // SAFETY: このウィンドウの HWND と、length を入れた構造体を渡す。
        unsafe { GetWindowPlacement(hwnd, &mut placement) }.ok()?;
        let (dx, dy) = workspace_offset(&placement.rcNormalPosition);
        let rect = &mut placement.rcNormalPosition;
        rect.left += dx;
        rect.right += dx;
        rect.top += dy;
        rect.bottom += dy;
        Some((hwnd, placement))
    }

    /// While staying maximized or minimized, decides the outer frame's top left
    /// (physical-pixel screen coordinates) for returning to normal. The size does not
    /// change.
    fn set_normal_position(&self, (x, y): (i32, i32)) {
        use windows::Win32::UI::WindowsAndMessaging::{
            SetWindowPlacement, SW_SHOWMINIMIZED, SW_SHOWMINNOACTIVE,
        };

        let Some((hwnd, mut placement)) = self.placement_info() else {
            return;
        };
        let rect = &mut placement.rcNormalPosition;
        let (width, height) = (rect.right - rect.left, rect.bottom - rect.top);
        *rect = windows::Win32::Foundation::RECT {
            left: x,
            top: y,
            right: x + width,
            bottom: y + height,
        };
        let (dx, dy) = workspace_offset(rect);
        rect.left -= dx;
        rect.right -= dx;
        rect.top -= dy;
        rect.bottom -= dy;
        // 最小化のままにするとき、SW_SHOWMINIMIZED だと前面へ出てしまう。
        if placement.showCmd == SW_SHOWMINIMIZED.0 as u32 {
            placement.showCmd = SW_SHOWMINNOACTIVE.0 as u32;
        }
        // SAFETY: このウィンドウの HWND と、GetWindowPlacement で埋めた構造体を渡す。
        let _ = unsafe { SetWindowPlacement(hwnd, &placement) };
    }

    /// The current display state.
    ///
    /// Fullscreen is decided by the kind of `AppWindow.Presenter`, the others by
    /// `OverlappedPresenter.State`. Before it is shown, it is the state requested with
    /// [`set_state`](Self::set_state) (the default is [`WindowState::Normal`]).
    pub fn state(&self) -> WindowState {
        if let Some(state) = self.0.initial_state.get() {
            return state;
        }
        self.native_state()
    }

    /// The current state of the WinUI window. The state requested before showing is not
    /// considered.
    fn native_state(&self) -> WindowState {
        if self.is_fullscreen() {
            return WindowState::Fullscreen;
        }
        match self
            .overlapped_presenter()
            .and_then(|presenter| presenter.State().ok())
        {
            Some(OverlappedPresenterState::Maximized) => WindowState::Maximized,
            Some(OverlappedPresenterState::Minimized) => WindowState::Minimized,
            _ => WindowState::Normal,
        }
    }

    /// Changes the display state. When called before it is shown, it is applied when
    /// shown.
    ///
    /// | Requested state | WinUI 3 |
    /// | --- | --- |
    /// | `Normal` | `OverlappedPresenter.Restore` |
    /// | `Maximized` | `OverlappedPresenter.Maximize` |
    /// | `Minimized` | `OverlappedPresenter.Minimize` |
    /// | `Fullscreen` | `AppWindow.SetPresenterByKind(FullScreen)` |
    ///
    /// When leaving fullscreen, the original `OverlappedPresenter` is reattached before
    /// applying it.
    pub fn set_state(&self, state: WindowState) {
        if self.0.initial_state.get().is_some() {
            self.0.initial_state.set(Some(state));
            return;
        }
        self.apply_state(state);
    }

    fn apply_state(&self, target: WindowState) {
        let Ok(app_window) = self.0.native.AppWindow() else {
            return;
        };
        if target == WindowState::Fullscreen {
            if !self.is_fullscreen() {
                self.0.fullscreen_return.set(self.physical_position());
                let _ = app_window.SetPresenterByKind(AppWindowPresenterKind::FullScreen);
            }
            return;
        }
        let Some(presenter) = self.overlapped_presenter() else {
            return;
        };
        if self.is_fullscreen() {
            let _ = app_window.SetPresenter(&presenter);
            self.0.fullscreen_return.set(None);
            // 全画面の間に頼まれた位置は、元の枠へ戻ったところで当てる
            // (最大化のまま戻ったときは、元に戻したときの位置になる)。
            if let Some(physical) = self.0.position_after_fullscreen.take() {
                self.set_physical_position(physical);
            }
        }
        let _ = match target {
            WindowState::Normal => {
                if presenter.State().ok() == Some(OverlappedPresenterState::Restored) {
                    Ok(())
                } else {
                    presenter.Restore()
                }
            }
            WindowState::Maximized => presenter.Maximize(),
            WindowState::Minimized => presenter.Minimize(),
            WindowState::Fullscreen => Ok(()),
        };
    }

    /// Notified when the display state changes. Receives the new
    /// [`state`](Self::state).
    ///
    /// Called both for user actions (title bar buttons, snapping to a screen edge and so
    /// on) and for [`set_state`](Self::set_state).
    pub fn on_state_change(&self, f: impl FnMut(WindowState) + 'static) {
        self.0.on_state_change.set(f);
    }

    /// Calls `on_state_change` if the current state differs from the one last reported.
    ///
    /// Once back to normal, applies a size requested with [`set_size`](Self::set_size)
    /// while not normal.
    fn report_state(&self) {
        if self.0.initial_state.get().is_some() {
            return;
        }
        self.0.on_state_change.update(self.state());
        if self.native_state() == WindowState::Normal {
            if let Some(size) = self.0.pending_size.take() {
                self.resize_content(size);
                // 中身はまだ戻る前の大きさで並んでいて、枠との差が通常の状態と
                // ずれる (最大化からは高さが 1 足りなかった) ので、並べ終わって
                // から実寸で合わせ直す。
                self.0.pending_fit.set(Some(size));
            }
        }
    }

    /// Whether the user can resize the window by dragging its edges. `true` by default.
    pub fn is_resizable(&self) -> bool {
        self.overlapped_presenter()
            .and_then(|presenter| presenter.IsResizable().ok())
            .unwrap_or(self.0.resizable.get())
    }

    /// Decides whether the user can resize the window.
    ///
    /// `OverlappedPresenter.IsResizable` and the maximize button (`IsMaximizable`) are
    /// switched together (on GTK and macOS too, the maximize button does not work for
    /// windows that cannot be resized). [`set_size`](Self::set_size) works either way.
    ///
    /// When the `OverlappedPresenter` cannot be obtained yet before the first show, the
    /// value is remembered and applied in [`show`](Self::show).
    pub fn set_resizable(&self, resizable: bool) {
        self.0.resizable.set(resizable);
        if let Some(presenter) = self.overlapped_presenter() {
            apply_resizable(&presenter, resizable);
        }
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
        self.0.key_down.set(f);
    }

    /// The widget placed at the root. Each call replaces it.
    ///
    /// With a sidebar attached, it is placed in the pane on its right
    /// (`NavigationView.Content`).
    pub fn set_child(&self, child: &dyn Widget) {
        // 根は作り直すが、サイドバー (`NavigationView`) は同じものを使い回す。
        // XAML の要素は親を 1 つしか持てないので、古い根の層から外しておく。
        let child_element = child.native_element();
        let previous = self
            .0
            .child
            .borrow()
            .as_ref()
            .map(|child| child.native_element());
        let Some(element) = self.host_content(&child_element) else {
            return;
        };
        self.take_content_head();
        let themed = themed_content_root(&element, &self.title());
        let (theme_root, title_bar, title_label, toolbar_host, menu_bar_host, drag_area, layer) =
            match themed {
                Ok(content) => (
                    content.root,
                    Some(content.title_bar),
                    Some(content.title_label),
                    Some(content.toolbar_host),
                    Some(content.menu_bar_host),
                    Some(content.drag_area),
                    Some(content.content),
                ),
                Err(error) => {
                    eprintln!("naui-windows: failed to create the themed window root: {error}");
                    (element.clone(), None, None, None, None, None, None)
                }
            };
        if self.0.native.SetContent(&theme_root).is_ok() {
            if let Some(previous) = previous {
                if previous != child_element {
                    crate::layout::clear_window_parent(&previous, &self.0.native);
                }
            }
            crate::layout::set_window_parent(&child_element, &self.0.native);
            if title_bar.is_some() {
                // Mica をタイトルバーまで連続させる。ドラッグ領域は
                // `apply_drag_area` が選ぶ。
                let _ = self.0.native.SetExtendsContentIntoTitleBar(true);
            }
            *self.0.title_bar.borrow_mut() = title_bar;
            *self.0.drag_area.borrow_mut() = drag_area;
            self.apply_drag_area();
            self.install_drag_area_updates();
            let _ = set_theme_on_element(&theme_root, self.0.theme.get());
            if let Some(label) = title_label.as_ref() {
                let _ = set_title_foreground(label, &theme_root, self.0.theme.get());
            }
            *self.0.theme_root.borrow_mut() = Some(theme_root);
            *self.0.title_label.borrow_mut() = title_label;
            *self.0.toolbar_host.borrow_mut() = toolbar_host;
            *self.0.menu_bar_host.borrow_mut() = menu_bar_host;
            *self.0.content_layer.borrow_mut() = layer;
            *self.0.child.borrow_mut() = Some(child.boxed_clone());
            // ルートを作り直したので、取り付けてあったものを載せ直す。
            // ショートカットの購読も、新しい根へ付け直す。
            self.mount_toolbar();
            self.mount_menu_bar();
            self.0.menu_key_installed.set(false);
            self.install_menu_shortcuts();
            // `on_key_down` の受け口と `on_focus_change` の見張りも新しい根へ付ける。
            if let Some(root) = self.0.theme_root.borrow().as_ref() {
                let _ = crate::keys::install(root, self.0.key_down.clone());
                let _ = crate::focus::install(root);
                if let Some(hwnd) = self.hwnd() {
                    let _ = crate::file_drop::install(
                        root,
                        hwnd,
                        self.0.on_file_drop.clone(),
                        self.0.tasks.clone(),
                    );
                }
            }
            if !self.0.wheel_subclass_installed.get() {
                self.0
                    .wheel_subclass_installed
                    .set(crate::layout::install_wheel_subclass(&self.0.native));
            }
            // タイトルバーの帯が中身の上に乗るので、中身の大きさが変わる。
            // 並べ終わったら層が知らせる。
            self.watch_content_layer();
        } else {
            // 新しい子を付けられなかったときは、外した元の中身を戻す。
            self.remount_content();
        }
    }

    /// The element placed in the content layer. With a sidebar attached, it puts the
    /// child in the right pane and returns the `NavigationView`.
    fn host_content(&self, element: &UIElement) -> Option<UIElement> {
        let sidebar = self.0.sidebar.borrow().clone();
        let Some(sidebar) = sidebar else {
            return Some(element.clone());
        };
        match sidebar.element() {
            Ok(navigation) => sidebar.set_content(Some(element)).then_some(navigation),
            Err(error) => {
                eprintln!("naui-windows: could not place the sidebar: {error}");
                Some(element.clone())
            }
        }
    }

    /// The sidebar attached to the left of the window. Each call replaces it.
    ///
    /// The head of the content layer is replaced with a `NavigationView`, and the child
    /// from [`set_child`](Self::set_child) moves to its right pane (`Content`). The
    /// title bar and menu bar stay above, so the sidebar starts below them (the same
    /// arrangement as Windows Settings).
    pub fn set_sidebar(&self, sidebar: &Sidebar) {
        self.clear_sidebar();
        *self.0.sidebar.borrow_mut() = Some(sidebar.clone());
        self.remount_content();
    }

    /// Removes the attached sidebar. Does nothing if none is attached.
    ///
    /// The child in the right pane goes back to being the window's content.
    pub fn clear_sidebar(&self) {
        let Some(old) = self.0.sidebar.borrow_mut().take() else {
            return;
        };
        // 先に層から外してから中身を手放す。XAML の要素は親を 1 つしか
        // 持てないので、子を層へ戻す前に `Content` から抜いておく。
        self.take_content_head();
        old.set_content(None);
        self.remount_content();
    }

    /// Removes the head of the content layer (the child or the `NavigationView`).
    fn take_content_head(&self) {
        let layer = self.0.content_layer.borrow().clone();
        if let Some(children) = layer.and_then(|layer| layer.Children().ok()) {
            if children.Size().unwrap_or(0) > 0 {
                let _ = children.RemoveAt(0);
            }
        }
    }

    /// Places the head of the content layer again for the current combination of child
    /// and sidebar.
    ///
    /// Does nothing if [`set_child`](Self::set_child) has not been called yet (it does
    /// the building).
    fn remount_content(&self) {
        let layer = self.0.content_layer.borrow().clone();
        let child = self
            .0
            .child
            .borrow()
            .as_ref()
            .map(|child| child.native_element());
        let (Some(layer), Some(child)) = (layer, child) else {
            return;
        };
        let Ok(children) = layer.Children() else {
            return;
        };
        self.take_content_head();
        if let Some(element) = self.host_content(&child) {
            let _ = children.InsertAt(0, &element);
        }
    }

    /// The toolbar attached to the top of the window. Each call replaces it.
    ///
    /// It is placed to the right of the title (the same position as macOS's `NSToolbar`
    /// and Linux's `AdwHeaderBar`). While attached, the drag region is split into two
    /// rectangles on either side of the buttons, so the window does not move when
    /// dragging over the buttons.
    pub fn set_toolbar(&self, toolbar: &Toolbar) {
        self.clear_toolbar();
        *self.0.toolbar.borrow_mut() = Some(toolbar.clone());
        self.mount_toolbar();
        // 項目がオーバーフローメニューへ出入りすると `CommandBar` の幅だけが
        // 変わる (置き場は `*` で変わらない)。ドラッグ領域はその幅から
        // 決めているので、帯そのものも見ておく。
        if let Ok(bar) = toolbar.native_command_bar().cast::<FrameworkElement>() {
            self.watch_for_drag_area(&bar);
        }
        self.apply_drag_area();
    }

    /// Removes the attached toolbar. Does nothing if none is attached.
    pub fn clear_toolbar(&self) {
        self.0.toolbar.borrow_mut().take();
        if let Some(host) = self.0.toolbar_host.borrow().as_ref() {
            let _ = host.Children().and_then(|children| children.Clear());
        }
        self.apply_drag_area();
    }

    /// The menu bar attached to the top of the window. Each call replaces it.
    ///
    /// A Windows menu bar is a band laid **below** the title bar rather than inside it,
    /// so it is placed between the title bar and the content.
    pub fn set_menu_bar(&self, menu_bar: &MenuBar) {
        self.clear_menu_bar();
        *self.0.menu_bar.borrow_mut() = Some(menu_bar.clone());
        self.mount_menu_bar();
        self.install_menu_shortcuts();
    }

    /// Removes the attached menu bar. Does nothing if none is attached.
    pub fn clear_menu_bar(&self) {
        if let Some(old) = self.0.menu_bar.borrow_mut().take() {
            old.close();
        }
        if let Some(host) = self.0.menu_bar_host.borrow().as_ref() {
            let _ = host.Children().and_then(|children| children.Clear());
        }
    }

    /// Puts the menu bar into its slot. Does nothing if either is missing.
    fn mount_menu_bar(&self) {
        let host = self.0.menu_bar_host.borrow();
        let menu_bar = self.0.menu_bar.borrow();
        let (Some(host), Some(menu_bar)) = (host.as_ref(), menu_bar.as_ref()) else {
            return;
        };
        let Ok(children) = host.Children() else {
            return;
        };
        let _ = children.Clear();
        let _ = children.Append(&menu_bar.mount());
    }

    /// Attaches the `KeyDown` that picks up shortcuts to the window root, exactly once.
    ///
    /// `KeyboardAccelerator` is not in the projection, so keys that bubble up to the root
    /// are inspected and passed to the [`MenuBar`]. Which menu bar is attached is checked
    /// at call time, so the subscription need not be reattached on every attach or
    /// detach.
    fn install_menu_shortcuts(&self) {
        if self.0.menu_key_installed.get() {
            return;
        }
        let root = self.0.theme_root.borrow().clone();
        let Some(root) = root else {
            return;
        };
        let weak = Arc::new(UiThreadCell::new(self.downgrade()));
        let handler = KeyEventHandler::new(move |_sender, args| {
            let Some(args) = args.as_ref() else {
                return Ok(());
            };
            let Some(window) = weak.try_with_mut(|weak| weak.upgrade()).flatten() else {
                return Ok(());
            };
            let menu_bar = window.0.menu_bar.borrow().clone();
            let Some(menu_bar) = menu_bar else {
                return Ok(());
            };
            let key = args.Key().unwrap_or(VirtualKey::None);
            if menu_bar.handle_key(key.0) {
                // 拾ったキーは、ほかのコントロールへは渡さない。
                let _ = args.SetHandled(true);
            }
            Ok(())
        });
        if root.KeyDown(&handler).is_ok() {
            self.0.menu_key_installed.set(true);
        }
    }

    /// Passes the window a drag region matching the current state.
    ///
    /// Passing an element to `SetTitleBar` would start window drags even over the
    /// interactive controls inside it. While a toolbar is attached, the region should be
    /// split into two — **the title text side** and **the free space right of the
    /// toolbar** — so `AppWindowTitleBar::SetDragRectangles`, which accepts several
    /// rectangles, is used. Only when that fails does it fall back to the `SetTitleBar`
    /// form.
    fn apply_drag_area(&self) {
        if self.apply_drag_rectangles().is_some() {
            return;
        }
        // 矩形を計算できないときの控え。ツールバーを付けているならボタンを
        // 含まない右の空きだけ、付けていなければタイトルバー全体を渡す。
        let has_toolbar = self.0.toolbar.borrow().is_some();
        let area = if has_toolbar {
            self.0.drag_area.borrow().clone()
        } else {
            self.0.title_bar.borrow().clone()
        };
        let Some(area) = area else {
            return;
        };
        let _ = self.0.native.SetTitleBar(&area);
    }

    /// Passes the part of the title bar that can be grabbed and moved, as rectangles.
    ///
    /// The coordinates passed are **physical pixels**, so the logical-pixel layout is
    /// multiplied by `RasterizationScale`. [`CAPTION_RESERVE`] at the right end is where
    /// the minimize, maximize and close buttons go, so it is left free.
    ///
    /// Before layout is decided (width 0), it gives up and returns `None`.
    /// [`install_drag_area_updates`](Self::install_drag_area_updates) calls it again
    /// whenever the size changes.
    fn apply_drag_rectangles(&self) -> Option<()> {
        let title_bar = self.0.title_bar.borrow().clone()?;
        let title_bar = title_bar.cast::<FrameworkElement>().ok()?;
        let width = title_bar.ActualWidth().ok()?;
        let height = title_bar.ActualHeight().ok()?;
        if width <= 0.0 || height <= 0.0 {
            return None;
        }
        let scale = title_bar
            .XamlRoot()
            .and_then(|root| root.RasterizationScale())
            .unwrap_or(1.0);
        if scale <= 0.0 {
            return None;
        }

        // ツールバーが載っているなら、その左右で 2 つに分ける。置き場は
        // 残りの幅いっぱい (`*`) なので、避けるのは置き場ではなく
        // **`CommandBar` が実際に使っている幅**。そうしないと右の空きが
        // 置き場に含まれてしまい、つかめる場所がタイトル文字の側だけに
        // なってしまう。
        let toolbar = self.0.toolbar.borrow().clone();
        let (skip_from, skip_to) = match toolbar {
            Some(toolbar) => {
                let host = self.0.toolbar_host.borrow().clone()?;
                let offset = f64::from(host.ActualOffset().ok()?.X);
                let bar = toolbar
                    .native_command_bar()
                    .cast::<FrameworkElement>()
                    .ok()?;
                (offset, offset + bar.ActualWidth().ok()?)
            }
            None => (width, width),
        };

        let right_edge = (width - CAPTION_RESERVE).max(0.0);
        let mut rects = Vec::with_capacity(2);
        for (from, to) in [(0.0, skip_from.min(right_edge)), (skip_to, right_edge)] {
            if to - from < 1.0 {
                continue;
            }
            rects.push(windows::Graphics::RectInt32 {
                X: (from * scale).round() as i32,
                Y: 0,
                Width: ((to - from) * scale).round() as i32,
                Height: (height * scale).round() as i32,
            });
        }
        if rects.is_empty() {
            return None;
        }

        self.0
            .native
            .AppWindow()
            .and_then(|window| window.TitleBar())
            .and_then(|bar| bar.SetDragRectangles(&rects))
            .ok()
    }

    /// Makes the drag region be recomputed whenever sizes change.
    ///
    /// The rectangles are fixed physical-pixel values, so they must be redone when any of
    /// the window width, the number of toolbar items or the display scale changes. The
    /// window width shows up in the sizes of the title bar and the slot. Changes in the
    /// number of items are watched by [`set_toolbar`](Self::set_toolbar), which adds the
    /// `CommandBar`. The scale barely shows up in DIP sizes, so
    /// [`watch_rasterization_scale`](Self::watch_rasterization_scale) watches it.
    fn install_drag_area_updates(&self) {
        let targets = [
            self.0
                .title_bar
                .borrow()
                .clone()
                .and_then(|element| element.cast::<FrameworkElement>().ok()),
            self.0
                .toolbar_host
                .borrow()
                .clone()
                .and_then(|host| host.cast::<FrameworkElement>().ok()),
        ];
        for target in targets.into_iter().flatten() {
            self.watch_for_drag_area(&target);
        }
    }

    /// Redoes the drag region when the element's size changes.
    fn watch_for_drag_area(&self, target: &FrameworkElement) {
        let state = UiThreadCell::new(Rc::downgrade(&self.0));
        let changed = SizeChangedEventHandler::new(move |_, _| {
            let _ = state.try_with_mut(|weak| {
                if let Some(inner) = weak.upgrade() {
                    Window(inner).apply_drag_area();
                }
            });
            Ok(())
        });
        let _ = target.SizeChanged(&changed);
    }

    /// Puts the toolbar back into its slot. Does nothing if the slot or the content does
    /// not exist yet.
    fn mount_toolbar(&self) {
        let host = self.0.toolbar_host.borrow();
        let toolbar = self.0.toolbar.borrow();
        let (Some(host), Some(toolbar)) = (host.as_ref(), toolbar.as_ref()) else {
            return;
        };
        let Ok(children) = host.Children() else {
            return;
        };
        let _ = children.Clear();
        let _ = children.Append(&toolbar.mount());
    }

    /// Switches this window's color theme.
    pub fn set_theme(&self, theme: Theme) -> Result<()> {
        self.0.theme.set(theme);
        let theme_root = self.0.theme_root.borrow();
        if let Some(theme_root) = theme_root.as_ref() {
            set_theme_on_element(theme_root, theme)?;
            if let Some(label) = self.0.title_label.borrow().as_ref() {
                set_title_foreground(label, theme_root, theme)?;
            }
        } else {
            let child = self.0.child.borrow();
            if let Some(child) = child.as_deref() {
                set_theme_on_element(&child.native_element(), theme)?;
            }
        }
        if let Backdrop::Controller { configuration, .. } = &self.0.backdrop {
            configuration
                .SetTheme(backdrop_theme(theme))
                .map_err(|e| to_error("set the Mica theme", e))?;
        }
        Ok(())
    }

    /// Shows it on screen and brings it to the front.
    ///
    /// On first show, the content is sized as given to `ui.window` (or to
    /// [`set_size`](Self::set_size) before showing), and the state requested with
    /// [`set_state`](Self::set_state) before showing is applied.
    pub fn show(&self) {
        if let Some(size) = self.0.initial_size.take() {
            // 最大化などで出すときは、通常の状態で並ぶ前にこれが戻る先の大きさになる。
            self.0.normal_size.set(size);
            self.resize_content(size);
        }
        if self.0.native.Activate().is_ok() {
            *self.0.visible.borrow_mut() = true;
            remember_owner(&self.0.native);
        }
        // 表示する前に取れなかった `OverlappedPresenter` を取り、覚えておいた
        // `set_resizable` を当てる。
        let _ = self.overlapped_presenter();
        if let Some(state) = self.0.initial_state.take() {
            // 最大化などの前の大きさで一度出してから当てるので、元へ戻したときは
            // `ui.window` の大きさになる。
            self.0.on_state_change.reset(WindowState::Normal);
            if state != WindowState::Normal {
                self.apply_state(state);
            }
        }
    }

    pub fn close(&self) {
        self.release_backdrop();
        if self.0.native.Close().is_ok() {
            *self.0.visible.borrow_mut() = false;
            if let Some(child) = self.0.child.borrow().as_ref() {
                crate::layout::clear_window_parent(&child.native_element(), &self.0.native);
            }
        }
    }

    pub fn is_visible(&self) -> bool {
        self.0.native.Visible().unwrap_or(*self.0.visible.borrow())
    }

    /// The real WinUI 3 window. A backend-specific escape hatch.
    pub fn native_window(&self) -> XamlWindow {
        self.0.native.clone()
    }

    /// Closes the Mica controller attached by naui itself.
    ///
    /// The controller holds on to the window's composition surface, so if the window is
    /// destroyed while it still does, the process crashes (STATUS_ACCESS_VIOLATION).
    /// There are three paths that wind things down, and every one of them goes through
    /// this.
    ///
    /// | Path | Where it is called |
    /// | --- | --- |
    /// | The user closes it | The `AppWindow`'s `Closing` → `clear_content_for_shutdown` |
    /// | `Window::close` | Here (`Window::Close` does not raise `Closing`) |
    /// | `Ui::quit` | For every window, before `Application::Exit` |
    ///
    /// The default Mica attached with `SetSystemBackdrop` belongs to WinUI, so it is not
    /// touched.
    pub(crate) fn release_backdrop(&self) {
        if let Backdrop::Controller { controller, .. } = &self.0.backdrop {
            let _ = controller.Close();
        }
    }

    pub(crate) fn clear_content_for_shutdown(&self) {
        self.release_backdrop();
        let _ = self.0.native.SetContent(None);
        // サイドバーは `Ui` が持ち続けるので、右の区画の子もここで手放す。
        if let Some(sidebar) = self.0.sidebar.borrow().as_ref() {
            sidebar.set_content(None);
        }
        if let Some(child) = self.0.child.borrow().as_ref() {
            crate::layout::clear_window_parent(&child.native_element(), &self.0.native);
        }
        *self.0.child.borrow_mut() = None;
        *self.0.theme_root.borrow_mut() = None;
        *self.0.content_layer.borrow_mut() = None;
        *self.0.title_label.borrow_mut() = None;
    }

    /// Removes the Content and child widgets before WinUI's XAML tree is destroyed.
    /// `Destroying` is too late, so it runs in the AppWindow's `Closing`.
    pub(crate) fn install_closing_handler(&self, state: &'static UiThreadCell<Option<Ui>>) {
        if self.0.closing_token.get().is_some() {
            return;
        }
        let Ok(app_window) = self.0.native.AppWindow() else {
            return;
        };
        let window = UiThreadCell::new(Rc::downgrade(&self.0));
        let handler = TypedEventHandler::<
            naui_winui3::Microsoft::UI::Windowing::AppWindow,
            naui_winui3::Microsoft::UI::Windowing::AppWindowClosingEventArgs,
        >::new(move |_sender, args| {
            // アプリが閉じないと答えたら、取り消して何も畳まない。
            let keep = window
                .try_with_mut(|weak| weak.upgrade())
                .flatten()
                .is_some_and(|inner| {
                    inner.on_close_request.ask() == naui_core::CloseResponse::KeepOpen
                });
            if keep {
                if let Some(args) = args.as_ref() {
                    let _ = args.SetCancel(true);
                }
                return Ok(());
            }
            state.with_mut(|slot| {
                if let Some(ui) = slot.take() {
                    // 画面が畳まれた後は、投函しても誰も取り出さない。
                    // 送信側へ失敗を返せるようにし、受信クロージャと future を解放する。
                    ui.0.tasks.shutdown();
                    ui.clear_windows_for_shutdown();
                }
            });
            Ok(())
        });
        if let Ok(token) = app_window.Closing(&handler) {
            self.0.closing_token.set(Some(token));
        }
    }
}

struct ThemedContent {
    root: UIElement,
    title_bar: UIElement,
    title_label: TextBlock,
    /// The container for the toolbar placed to the right of the title.
    toolbar_host: Grid,
    /// The container for the menu bar placed below the title bar.
    menu_bar_host: Grid,
    /// The layer holding the app's content and toasts.
    content: Grid,
    /// The free space right of the toolbar. The drag region while a toolbar is attached.
    drag_area: UIElement,
}

fn backdrop_theme(theme: Theme) -> SystemBackdropTheme {
    match theme {
        Theme::System => SystemBackdropTheme::Default,
        Theme::Light => SystemBackdropTheme::Light,
        Theme::Dark => SystemBackdropTheme::Dark,
    }
}

/// Lays Mica as the window's background.
///
/// Mica is a "nice to have", so failure just means giving it up. Not being
/// translucent is far less of a problem than the window not appearing. With old
/// Windows App SDKs, or on Windows 10 without Mica, this becomes [`Backdrop::None`].
fn create_backdrop(native: &XamlWindow, theme: Theme) -> Backdrop {
    if MicaController::IsSupported().unwrap_or(false) {
        if let Ok(target) = native.cast::<ICompositionSupportsSystemBackdrop>() {
            if let Ok(controller) = MicaController::new() {
                if let Ok(configuration) = SystemBackdropConfiguration::new() {
                    let configured = configuration.SetIsInputActive(true).is_ok()
                        && configuration.SetTheme(backdrop_theme(theme)).is_ok()
                        && controller.AddSystemBackdropTarget(&target).unwrap_or(false)
                        && controller
                            .SetSystemBackdropConfiguration(&configuration)
                            .is_ok();
                    if configured {
                        return Backdrop::Controller {
                            controller,
                            configuration,
                        };
                    }
                }
            }
        }
    }

    let Ok(mica) = MicaBackdrop::new() else {
        return Backdrop::None;
    };
    match native.SetSystemBackdrop(&mica) {
        Ok(()) => Backdrop::BuiltIn { _mica: mica },
        Err(_) => Backdrop::None,
    }
}

fn set_title_foreground(label: &TextBlock, root: &UIElement, requested: Theme) -> Result<()> {
    let theme = effective_theme(root, requested);
    // 色は決め打ちにせず、WinUI 3 のテーマリソースから引く。タイトルバーの
    // 文字は本文と同じ `TextFillColorPrimaryBrush` で、明暗それぞれの値は
    // `XamlControlsResources` が持つ。要素の `RequestedTheme` を先に決めて
    // おくと、その場でどちらの値を引くかが定まる。
    let theme_name = match theme {
        Theme::Light => "Light",
        Theme::Dark | Theme::System => "Dark",
    };
    let brush = XamlReader::Load(&HSTRING::from(format!(
        r##"<TextBlock xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
            RequestedTheme="{theme_name}"
            Foreground="{{ThemeResource TextFillColorPrimaryBrush}}"/>"##
    )))
    .map_err(|e| to_error("create the title text brush", e))?
    .cast::<TextBlock>()
    .map_err(|e| to_error("convert to the title TextBlock", e))?
    .Foreground()
    .map_err(|e| to_error("get the title text brush", e))?;
    label
        .SetForeground(&brush)
        .map_err(|e| to_error("set the title text color", e))
}

fn effective_theme(root: &UIElement, requested: Theme) -> Theme {
    if requested != Theme::System {
        return requested;
    }
    Application::Current()
        .ok()
        .and_then(|app| app.RequestedTheme().ok())
        .map(|theme| {
            if theme == ApplicationTheme::Light {
                Theme::Light
            } else {
                Theme::Dark
            }
        })
        .or_else(|| actual_theme(root).ok())
        .unwrap_or(Theme::Dark)
}

fn themed_content_root(element: &UIElement, title: &str) -> Result<ThemedContent> {
    // 帯の高さは `CAPTION_HEIGHT`。システムが描く最小化・最大化・閉じるの
    // ボタンがこの高さなので、タイトル文字もツールバーもその中へ収まる。
    // 帯と中身の間は空けない。空きの取り方は中身の側 (ウィジェットの
    // パディング) に任せる。
    //
    // ツールバーの置き場 (2 列目) は `*`。`Auto` だと `CommandBar` が
    // 幅の制限なしで測られ、項目が入りきらなくなってもオーバーフロー
    // メニューへ移らず、キャプションボタンの上へはみ出してしまう。残りの
    // 幅を渡しておけば `CommandBar` が自分で項目をメニューへ送る。
    // `CommandBar` は左寄せなので、余った分はそのまま右の空きになる
    // ([`Window::apply_drag_rectangles`] がそこをドラッグ領域にする)。
    // 3 列目はドラッグ領域の控え。矩形を渡せなかったときだけ使う。
    //
    // メニューバーの置き場 (2 行目) は `Auto`。付いていなければ中身が無く、
    // 高さ 0 になるので、タイトルバーと中身がそのまま隣り合う。左の余白は、
    // 見出しのボタンが持つ内側の余白とあわせてタイトル文字の左端にそろう。
    let root = XamlReader::Load(&HSTRING::from(format!(
        r##"<Grid xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
            Background="Transparent">
            <Grid.RowDefinitions>
                <RowDefinition Height="{CAPTION_HEIGHT}"/>
                <RowDefinition Height="Auto"/>
                <RowDefinition Height="*"/>
            </Grid.RowDefinitions>
            <Grid Grid.Row="0" Background="Transparent"
                Padding="16,0,{CAPTION_RESERVE},0">
                <Grid.ColumnDefinitions>
                    <ColumnDefinition Width="Auto"/>
                    <ColumnDefinition Width="*"/>
                    <ColumnDefinition Width="Auto" MinWidth="48"/>
                </Grid.ColumnDefinitions>
                <TextBlock Grid.Column="0" FontSize="{TITLE_FONT_SIZE}"
                    VerticalAlignment="Center"/>
                <Grid Grid.Column="1" Background="Transparent" Margin="12,0,0,0"/>
                <Grid Grid.Column="2" Background="Transparent"/>
            </Grid>
            <Grid Grid.Row="{MENU_BAR_ROW}" Background="Transparent" Padding="6,0,0,0"/>
            <Grid Grid.Row="{CONTENT_ROW}" Background="Transparent"/>
        </Grid>"##,
    )))
    .map_err(|e| to_error("create the themed background element", e))?
    .cast::<Grid>()
    .map_err(|e| to_error("convert to the themed background element", e))?;
    let children = root
        .Children()
        .map_err(|e| to_error("get the themed background element children", e))?;
    let title_bar = children
        .GetAt(0)
        .map_err(|e| to_error("get the title bar", e))?
        .cast::<Grid>()
        .map_err(|e| to_error("convert to the title bar", e))?;
    let title_label = title_bar
        .Children()
        .map_err(|e| to_error("get the title label children", e))?
        .GetAt(0)
        .map_err(|e| to_error("get the title label", e))?
        .cast::<TextBlock>()
        .map_err(|e| to_error("convert to the title label", e))?;
    title_label
        .SetText(&HSTRING::from(title))
        .map_err(|e| to_error("set the title label", e))?;

    let toolbar_host = title_bar
        .Children()
        .map_err(|e| to_error("get the toolbar slot children", e))?
        .GetAt(1)
        .map_err(|e| to_error("get the toolbar slot", e))?
        .cast::<Grid>()
        .map_err(|e| to_error("convert to the toolbar slot", e))?;
    let drag_area = title_bar
        .Children()
        .map_err(|e| to_error("get the drag area children", e))?
        .GetAt(2)
        .map_err(|e| to_error("get the drag area", e))?;
    let menu_bar_host = children
        .GetAt(MENU_BAR_ROW)
        .map_err(|e| to_error("get the menu bar slot", e))?
        .cast::<Grid>()
        .map_err(|e| to_error("convert to the menu bar slot", e))?;
    let content = children
        .GetAt(CONTENT_ROW)
        .map_err(|e| to_error("get the content layer", e))?
        .cast::<Grid>()
        .map_err(|e| to_error("convert to the content layer", e))?;
    content
        .Children()
        .map_err(|e| to_error("get the themed background element children", e))?
        .Append(element)
        .map_err(|e| to_error("add to the themed background element", e))?;
    let layer = content.clone();
    Ok(ThemedContent {
        root: root
            .cast::<UIElement>()
            .map_err(|e| to_error("convert to the themed background element", e))?,
        title_bar: title_bar
            .cast::<UIElement>()
            .map_err(|e| to_error("convert to the title bar", e))?,
        title_label,
        toolbar_host,
        menu_bar_host,
        content: layer,
        drag_area,
    })
}

thread_local! {
    /// The HWND of the window last shown. Used as the parent of modal dialogs.
    ///
    /// WinUI 3 has no API for finding the window an XAML element is on, so it is
    /// remembered on every show.
    static OWNER_HWND: Cell<isize> = const { Cell::new(0) };
    /// The window last shown. Used for the `XamlRoot` on which `ContentDialog`s are
    /// shown.
    static OWNER_WINDOW: RefCell<Option<XamlWindow>> = const { RefCell::new(None) };
}

/// Converts a position in logical pixels into physical pixels at `scale`.
fn to_physical((x, y): (f64, f64), scale: f64) -> (i32, i32) {
    ((x * scale).round() as i32, (y * scale).round() as i32)
}

/// How many physical pixels one logical pixel (DIP) is on `monitor`
/// (`GetDpiForMonitor`). `None` if it cannot be read.
fn monitor_scale(monitor: windows::Win32::Graphics::Gdi::HMONITOR) -> Option<f64> {
    use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};

    let (mut dpi_x, mut dpi_y) = (0, 0);
    // SAFETY: 画面と、書き込み先の u32 を渡すだけ。
    unsafe { GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y) }.ok()?;
    (dpi_x > 0).then(|| f64::from(dpi_x) / 96.0)
}

/// Whether a window `width` (DIP) wide with its outer frame's top left at `physical`
/// would show enough of its top edge on one of the connected monitors' work areas for
/// the user to drag it ([`naui_core::is_on_screen`]).
///
/// The window takes the scale of the monitor it is on, so each monitor is checked in
/// its own logical pixels: the position and work area divided by that monitor's
/// scale, against the width as is.
fn is_on_some_monitor(physical: (i32, i32), width: f64) -> bool {
    is_on_monitors(physical, width, &monitor_work_areas())
}

/// [`is_on_some_monitor`] against the given work areas (physical pixels) and scales.
fn is_on_monitors(
    physical: (i32, i32),
    width: f64,
    monitors: &[(windows::Win32::Foundation::RECT, f64)],
) -> bool {
    monitors.iter().any(|&(area, scale)| {
        let logical = |value: i32| f64::from(value) / scale;
        let screen = Rect::new(
            logical(area.left),
            logical(area.top),
            logical(area.right - area.left),
            logical(area.bottom - area.top),
        );
        naui_core::is_on_screen((logical(physical.0), logical(physical.1)), width, &[screen])
    })
}

/// The work areas of the connected monitors (excluding the taskbar,
/// `MONITORINFO.rcWork`) in physical pixels, each with its monitor's scale (1 if it
/// cannot be read).
fn monitor_work_areas() -> Vec<(windows::Win32::Foundation::RECT, f64)> {
    use windows::Win32::Foundation::{LPARAM, RECT};
    use windows::Win32::Graphics::Gdi::{
        EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO,
    };
    use windows_core::BOOL;

    unsafe extern "system" fn collect(
        monitor: HMONITOR,
        _: HDC,
        _: *mut RECT,
        data: LPARAM,
    ) -> BOOL {
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        let scale = monitor_scale(monitor).unwrap_or(1.0);
        // SAFETY: data は下の呼び出しが渡した Vec を指し、列挙の間だけ使う。
        // 画面は EnumDisplayMonitors が渡したもの。
        unsafe {
            if GetMonitorInfoW(monitor, &mut info).as_bool() {
                (*(data.0 as *mut Vec<(RECT, f64)>)).push((info.rcWork, scale));
            }
        }
        true.into()
    }

    let mut areas: Vec<(RECT, f64)> = Vec::new();
    // SAFETY: 列挙は戻る前に終わるので、areas への指し先はその間だけ使われる。
    let _ = unsafe {
        EnumDisplayMonitors(
            None,
            None,
            Some(collect),
            LPARAM(&mut areas as *mut Vec<(RECT, f64)> as isize),
        )
    };
    areas
}

/// Remembers the window to use as the parent of modal dialogs.
fn remember_owner(window: &XamlWindow) {
    OWNER_WINDOW.with(|slot| *slot.borrow_mut() = Some(window.clone()));
    let Ok(native) = window.cast::<naui_winui3::IWindowNative>() else {
        return;
    };
    if let Ok(hwnd) = unsafe { native.WindowHandle() } {
        OWNER_HWND.with(|slot| slot.set(hwnd.0 as isize));
    }
}

/// The base on which `ContentDialog`s are shown. `None` if no window has been shown
/// yet.
///
/// The `XamlRoot` is decided only after the window is shown, so it is obtained again
/// from the remembered window on every call.
pub(crate) fn owner_xaml_root() -> Option<naui_winui3::Microsoft::UI::Xaml::XamlRoot> {
    OWNER_WINDOW.with(|slot| {
        let window = slot.borrow();
        let content = window.as_ref()?.Content().ok()?;
        content.XamlRoot().ok()
    })
}

/// The layer toasts are overlaid on. `None` if no window has been shown yet.
///
/// It is the content row ([`CONTENT_ROW`]) created by [`themed_content_root`]; a
/// `Grid` places its children in stacking order, so a toast added later appears
/// above the content.
pub(crate) fn owner_content_layer() -> Option<Grid> {
    OWNER_WINDOW.with(|slot| {
        let window = slot.borrow();
        let root = window.as_ref()?.Content().ok()?.cast::<Grid>().ok()?;
        root.Children()
            .ok()?
            .GetAt(CONTENT_ROW)
            .ok()?
            .cast::<Grid>()
            .ok()
    })
}

/// The window to use as the parent of modal dialogs. `None` if nothing has been
/// shown yet.
pub(crate) fn owner_hwnd() -> Option<windows::Win32::Foundation::HWND> {
    let raw = OWNER_HWND.with(|slot| slot.get());
    if raw == 0 {
        return None;
    }
    Some(windows::Win32::Foundation::HWND(
        raw as *mut std::ffi::c_void,
    ))
}

/// Switches the resize handles and the maximize button together.
fn apply_resizable(presenter: &OverlappedPresenter, resizable: bool) {
    let _ = presenter.SetIsResizable(resizable);
    let _ = presenter.SetIsMaximizable(resizable);
}

/// For the screen `rect` (screen coordinates) is on, the amount added when
/// converting workspace coordinates to screen coordinates (the work area's top left
/// − the screen's top left).
///
/// `WINDOWPLACEMENT` coordinates are workspace coordinates for windows that are not
/// `WS_EX_TOOLWINDOW`. (0, 0) if the taskbar is at the bottom or right.
fn workspace_offset(rect: &windows::Win32::Foundation::RECT) -> (i32, i32) {
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromRect, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    };

    // SAFETY: 読むだけの RECT を渡す。
    let monitor = unsafe { MonitorFromRect(rect, MONITOR_DEFAULTTONEAREST) };
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    // SAFETY: MonitorFromRect の返した画面と、cbSize を入れた構造体を渡す。
    if !unsafe { GetMonitorInfoW(monitor, &mut info) }.as_bool() {
        return (0, 0);
    }
    (
        info.rcWork.left - info.rcMonitor.left,
        info.rcWork.top - info.rcMonitor.top,
    )
}

/// Maps naui's theme onto XAML's `ElementTheme`.
fn element_theme(theme: Theme) -> ElementTheme {
    match theme {
        Theme::System => ElementTheme::Default,
        Theme::Light => ElementTheme::Light,
        Theme::Dark => ElementTheme::Dark,
    }
}

pub(crate) fn set_theme_on_element(element: &UIElement, theme: Theme) -> Result<()> {
    element
        .cast::<FrameworkElement>()
        .map_err(|e| to_error("convert to the themed element", e))?
        .SetRequestedTheme(element_theme(theme))
        .map_err(|e| to_error("set the theme", e))
}

fn actual_theme(element: &UIElement) -> Result<Theme> {
    let actual = element
        .cast::<FrameworkElement>()
        .map_err(|e| to_error("convert to the element for the actual theme", e))?
        .ActualTheme()
        .map_err(|e| to_error("get the actual theme", e))?;
    // `ActualTheme` が `Default` を返すことは無いが、念のため暗いほうへ寄せる。
    Ok(if actual == ElementTheme::Light {
        Theme::Light
    } else {
        Theme::Dark
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Foundation::RECT;

    fn area(left: i32, top: i32, right: i32, bottom: i32) -> RECT {
        RECT {
            left,
            top,
            right,
            bottom,
        }
    }

    #[test]
    fn on_monitor_check_counts_the_width_with_each_monitors_scale() {
        // 主画面 100% の左に、物理 [-3840, 0] の 200% の画面。
        let monitors = [
            (area(0, 0, 1920, 1040), 1.0),
            (area(-3840, 0, 0, 2120), 2.0),
        ];
        // 幅 800 DIP は左の画面では物理 1600 なので、-5000 から -3400 まで届く。
        assert!(is_on_monitors((-5000, 100), 800.0, &monitors));
        // 物理 1600 でも届かないところは画面の外。
        assert!(!is_on_monitors((-5500, 100), 800.0, &monitors));
        // 主画面の右へはみ出た位置は、どちらの画面にも無い。
        assert!(!is_on_monitors((1950, 100), 800.0, &monitors));
        assert!(is_on_monitors((1800, 100), 800.0, &monitors));
    }
}
