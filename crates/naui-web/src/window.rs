//! Browsers have no OS windows, so a block element directly under `<body>` is
//! treated as a window. The title is reflected in `document.title`.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use naui_core::{Error, Result, StateHandler, Theme, WindowPlacement, WindowState};
use wasm_bindgen::JsCast;
use web_sys::{Document, Element, HtmlElement};

use crate::apply_theme;
use crate::menu_bar::MenuBar;
use crate::sidebar::Sidebar;
use crate::to_error;
use crate::toolbar::Toolbar;
use crate::widgets::{create, Listener, Widget};

/// The marker put on window elements.
const WINDOW_ATTRIBUTE: &str = "data-naui-window";

/// The selector for finding window elements with `closest`.
pub(crate) const WINDOW_SELECTOR: &str = "[data-naui-window]";

struct WindowInner {
    element: HtmlElement,
    document: Document,
    title: RefCell<String>,
    child: RefCell<Option<Box<dyn Widget>>>,
    /// The toolbar inserted at the top. Kept alive along with its handlers.
    toolbar: RefCell<Option<Toolbar>>,
    /// The menu bar inserted at the top. Kept alive along with its handlers.
    menu_bar: RefCell<Option<MenuBar>>,
    /// The attached sidebar. Kept alive along with its handlers.
    sidebar: RefCell<Option<Sidebar>>,
    /// The toolbar row. Lays out the sidebar's collapse button and the toolbar side by
    /// side.
    toolbar_row: HtmlElement,
    key_down: crate::keys::KeyDown,
    on_resize: Rc<crate::widgets::ValueHandler<(f64, f64)>>,
    /// The subscription to size changes. Dropping it also unsubscribes.
    resize_observer: RefCell<Option<web_sys::ResizeObserver>>,
    resize_callback: RefCell<Option<ObserverCallback>>,
    /// The content size requested with `ui.window` or `set_size`.
    size: Cell<(f64, f64)>,
    /// The content minimum requested with `set_min_size`.
    min_size: Cell<Option<(f64, f64)>>,
    /// The subscription to height changes of the menu bar and toolbar rows. When the
    /// rows' heights change, the window element's height setting is re-added.
    bars_observer: RefCell<Option<web_sys::ResizeObserver>>,
    bars_callback: RefCell<Option<ObserverCallback>>,
    on_state_change: StateHandler,
    /// The document's `fullscreenchange` subscription. Reports entering and leaving
    /// fullscreen.
    fullscreen_change: RefCell<Option<Listener>>,
    /// The direction last requested with `set_state` (`true` for fullscreen). `Some`
    /// only while not yet applied; `None` once applied (or refused).
    wanted: Cell<Option<bool>>,
    /// The direction of the `requestFullscreen` (`true`) / `exitFullscreen` (`false`)
    /// being waited on now. `None` if nothing is being waited on.
    in_flight: Cell<Option<bool>>,
    /// The receiver for when the awaited Promise settles (fulfilled or rejected).
    /// Without it, rejections show in the console as "unhandled rejections". It is
    /// reused, so it is kept alive here.
    settled: RefCell<Option<ObserverCallback>>,
    /// The value requested with `set_resizable`. It has no effect on the Web and is just
    /// remembered.
    resizable: Cell<bool>,
    /// `on_file_drop`.
    file_drop: crate::file_drop::FileDrop,
}

/// The handler passed to `ResizeObserver`.
type ObserverCallback = wasm_bindgen::closure::Closure<dyn FnMut(wasm_bindgen::JsValue)>;

impl Drop for WindowInner {
    fn drop(&mut self) {
        if let Some(observer) = self.resize_observer.borrow_mut().take() {
            observer.disconnect();
        }
        if let Some(observer) = self.bars_observer.borrow_mut().take() {
            observer.disconnect();
        }
    }
}

/// The equivalent of a window on the page.
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

    pub(crate) fn new(document: &Document, title: &str, width: f64, height: f64) -> Result<Self> {
        let element: HtmlElement = create(document, "div")?.unchecked_into();
        // メニューバーが「キーがどのウィンドウから上がってきたか」を見分ける印。
        let _ = element.set_attribute(WINDOW_ATTRIBUTE, "");
        let style = element.style();
        // 幅と高さは `apply_size` が付ける。
        let _ = style.set_property("margin", "0 auto");
        let _ = style.set_property("box-sizing", "border-box");
        // 中身がウィンドウの高さいっぱいに広がれるようにする。
        let _ = style.set_property("display", "flex");
        let _ = style.set_property("flex-direction", "column");
        crate::layout::mark_parent(
            &element,
            crate::layout::ParentLayout::Flex(naui_core::Orientation::Vertical),
        );

        let body = document
            .body()
            .ok_or_else(|| Error::new("get body", "there is no body"))?;
        body.append_child(&element)
            .map_err(|e| to_error("add the window", e))?;

        // ツールバーの行。サイドバーの開閉ボタン (あれば) とツールバーを並べる。
        // macOS でサイドバーボタンがツールバーの先頭に入るのと同じ並び。
        let toolbar_row: HtmlElement = create(document, "div")?.unchecked_into();
        let row_style = toolbar_row.style();
        let _ = row_style.set_property("display", "flex");
        let _ = row_style.set_property("flex-direction", "row");
        let _ = row_style.set_property("align-items", "center");
        let _ = row_style.set_property("gap", "6px");
        let _ = row_style.set_property("flex-shrink", "0");

        let this = Self(Rc::new(WindowInner {
            element,
            document: document.clone(),
            title: RefCell::new(String::new()),
            child: RefCell::new(None),
            toolbar: RefCell::new(None),
            menu_bar: RefCell::new(None),
            sidebar: RefCell::new(None),
            toolbar_row,
            key_down: crate::keys::KeyDown::default(),
            on_resize: Rc::default(),
            resize_observer: RefCell::new(None),
            resize_callback: RefCell::new(None),
            size: Cell::new((width.max(0.0), height.max(0.0))),
            min_size: Cell::new(None),
            bars_observer: RefCell::new(None),
            bars_callback: RefCell::new(None),
            on_state_change: StateHandler::default(),
            fullscreen_change: RefCell::new(None),
            wanted: Cell::new(None),
            in_flight: Cell::new(None),
            settled: RefCell::new(None),
            resizable: Cell::new(true),
            file_drop: Default::default(),
        }));
        this.observe_fullscreen();
        this.set_title(title);
        this.observe_bars();
        this.apply_size();
        Ok(this)
    }

    /// The total height of the menu bar and toolbar rows. Rows that are not attached
    /// are not counted.
    fn bars_height(&self) -> f64 {
        let menu_bar = self.0.menu_bar.borrow().as_ref().map(|m| m.mount());
        std::iter::once(self.0.toolbar_row.clone())
            .chain(menu_bar)
            .filter(|bar| bar.parent_element().as_ref() == Some(self.0.element.as_ref()))
            .map(|bar| f64::from(bar.offset_height()))
            .sum()
    }

    /// Maps the requested content size and minimum onto the window element's CSS.
    ///
    /// The user resizes a desktop window; on the Web the browser window plays that
    /// part. So the height follows the browser's visible area (`100dvh`), less what
    /// the page itself takes around it ([`page_inset`]), and the width is the
    /// requested width as `max-width` (shrinks on narrow screens). The requested
    /// height is not used. The minimum becomes `min-width` / `min-height`; the window
    /// element also contains the menu bar and toolbar rows, so their heights are
    /// added to the minimum height.
    fn apply_size(&self) {
        let (width, _) = self.0.size.get();
        let min = self.0.min_size.get();
        let bars = self.bars_height();
        let style = self.0.element.style();
        let _ = style.set_property("max-width", &format!("{width}px"));
        // `dvh` の無いブラウザでは後の指定が無視され、`vh` が残る。
        let inset = page_inset(&self.0.document);
        for unit in ["vh", "dvh"] {
            let height = if inset > 0.0 {
                format!("calc(100{unit} - {inset}px)")
            } else {
                format!("100{unit}")
            };
            let _ = style.set_property("height", &height);
        }
        match min {
            Some((min_width, min_height)) => {
                let _ = style.set_property("min-width", &format!("{min_width}px"));
                let _ = style.set_property("min-height", &format!("{}px", min_height + bars));
            }
            None => {
                let _ = style.remove_property("min-width");
                let _ = style.remove_property("min-height");
            }
        }
    }

    /// Reports entering and leaving fullscreen (including the user leaving with Esc) to
    /// `on_state_change`.
    fn observe_fullscreen(&self) {
        let weak = Rc::downgrade(&self.0);
        let listener = Listener::attach(self.0.document.as_ref(), "fullscreenchange", move || {
            if let Some(inner) = weak.upgrade() {
                let state = Window(inner.clone()).state();
                inner.on_state_change.update(state);
            }
        });
        *self.0.fullscreen_change.borrow_mut() = listener.ok();
        let weak = Rc::downgrade(&self.0);
        let settled = wasm_bindgen::closure::Closure::<dyn FnMut(wasm_bindgen::JsValue)>::new(
            move |_: wasm_bindgen::JsValue| {
                if let Some(inner) = weak.upgrade() {
                    Window(inner).finish_fullscreen_request();
                }
            },
        );
        *self.0.settled.borrow_mut() = Some(settled);
    }

    /// Re-adds the height when the rows' heights change (from attaching or detaching,
    /// or from fonts loading).
    fn observe_bars(&self) {
        let weak = Rc::downgrade(&self.0);
        let callback = wasm_bindgen::closure::Closure::<dyn FnMut(wasm_bindgen::JsValue)>::new(
            move |_entries: wasm_bindgen::JsValue| {
                if let Some(inner) = weak.upgrade() {
                    Window(inner).apply_size();
                }
            },
        );
        let Ok(observer) = web_sys::ResizeObserver::new(callback.as_ref().unchecked_ref()) else {
            return;
        };
        observer.observe(self.0.toolbar_row.as_ref());
        *self.0.bars_observer.borrow_mut() = Some(observer);
        *self.0.bars_callback.borrow_mut() = Some(callback);
    }

    /// Confirmation when the user tries to close the window.
    ///
    /// **It is never called on the Web.** Browsers have no user action for closing a
    /// window inside the page, and a custom confirmation cannot be shown when the tab
    /// closes (`beforeunload` can only show the browser's own wording). It is accepted
    /// anyway so that the same code can be written for all four platforms.
    pub fn on_close_request(&self, f: impl FnMut() -> naui_core::CloseResponse + 'static) {
        drop(f);
    }

    /// The size of the content area (width, height) in CSS pixels.
    ///
    /// As on the other platforms, it excludes the menu bar and toolbar rows (their
    /// heights are subtracted from the window element's size).
    pub fn size(&self) -> (f64, f64) {
        (
            f64::from(self.0.element.client_width()),
            (f64::from(self.0.element.client_height()) - self.bars_height()).max(0.0),
        )
    }

    /// Called with the files dropped onto the window from the OS.
    ///
    /// A widget under the pointer with its own `on_file_drop` takes the drop
    /// instead. As with widgets, the entries carry `blob:` URLs instead of paths,
    /// valid until the next drop onto the window, or until the window is dropped.
    /// Setting it again replaces the previous closure.
    pub fn on_file_drop(&self, f: impl FnMut(&[naui_core::FileEntry]) + 'static) {
        self.0.file_drop.set(&self.0.element, f);
    }

    /// Notified when the size changes. Receives the new [`size`](Self::size).
    ///
    /// It is picked up with `ResizeObserver`, so it also arrives once right after the
    /// window is placed in the document.
    pub fn on_resize(&self, f: impl FnMut((f64, f64)) + 'static) {
        self.0.on_resize.set(f);
        if self.0.resize_observer.borrow().is_some() {
            return;
        }
        let weak = Rc::downgrade(&self.0);
        let callback = wasm_bindgen::closure::Closure::<dyn FnMut(wasm_bindgen::JsValue)>::new(
            move |_entries: wasm_bindgen::JsValue| {
                if let Some(inner) = weak.upgrade() {
                    let size = Window(inner.clone()).size();
                    inner.on_resize.emit(size);
                }
            },
        );
        let Ok(observer) = web_sys::ResizeObserver::new(callback.as_ref().unchecked_ref()) else {
            return;
        };
        observer.observe(self.0.element.as_ref());
        *self.0.resize_observer.borrow_mut() = Some(observer);
        *self.0.resize_callback.borrow_mut() = Some(callback);
    }

    /// The position of the window's top-left corner. **It cannot be obtained on the
    /// Web, so this is always `None`.**
    ///
    /// Windows on the page are just laid out inside the browser window and have no
    /// position on the screen (`moveTo`, which moves the browser window, works only on
    /// windows you opened yourself).
    pub fn position(&self) -> Option<(f64, f64)> {
        None
    }

    /// Moves the window. **Does nothing on the Web** (see [`position`](Self::position)).
    pub fn set_position(&self, x: f64, y: f64) {
        let _ = (x, y);
    }

    /// The size and state to save, and to show it in next time. **The position is
    /// always `None`** (see [`position`](Self::position)).
    ///
    /// The size is the one requested with `ui.window` or [`set_size`](Self::set_size),
    /// not the measured [`size`](Self::size). The browser decides the actual size, so
    /// saving the measured one and passing it back would turn a narrow browser window
    /// into a lasting `max-width`.
    pub fn placement(&self) -> WindowPlacement {
        WindowPlacement {
            size: self.0.size.get(),
            position: None,
            state: self.state(),
        }
    }

    /// Applies a saved [`placement`](Self::placement): the size, then the state. **The
    /// position is ignored.**
    ///
    /// Browsers refuse fullscreen outside a user action, so a saved
    /// [`WindowState::Fullscreen`] applied at startup leaves the page as it is (see
    /// [`set_state`](Self::set_state)).
    pub fn set_placement(&self, placement: &WindowPlacement) {
        let (width, height) = placement.size;
        self.set_size(width, height);
        self.set_state(placement.state);
    }

    /// The current display state. On the Web, only [`WindowState::Normal`] or
    /// [`WindowState::Fullscreen`].
    ///
    /// `Fullscreen` when the whole page (`document.documentElement`) is fullscreen.
    /// When only another element, such as a video, is fullscreen, it stays `Normal`.
    pub fn state(&self) -> WindowState {
        let page: Option<Element> = self.0.document.document_element();
        if page.is_some() && self.0.document.fullscreen_element() == page {
            WindowState::Fullscreen
        } else {
            WindowState::Normal
        }
    }

    /// Changes the display state. **The Web has only fullscreen**, using the Fullscreen
    /// API.
    ///
    /// | Requested state | Web |
    /// | --- | --- |
    /// | `Fullscreen` | `document.documentElement.requestFullscreen()` |
    /// | `Normal` | `document.exitFullscreen()` if in fullscreen |
    /// | `Maximized` / `Minimized` | Only leaves fullscreen if in it (a page has no way to maximize or minimize the browser window) |
    ///
    /// Browsers **allow fullscreen only from inside a user action (a click or key)**,
    /// so call it from a button's `on_click` or similar. Calling it at startup or from a
    /// timer is refused, and the state does not change. Becoming fullscreen is
    /// asynchronous, so receive the change through
    /// [`on_state_change`](Self::on_state_change).
    ///
    /// The whole page becomes fullscreen, so with several windows they all go into the
    /// fullscreen view (toasts and dialogs appear too).
    ///
    /// When another state is requested while entering or leaving fullscreen, it
    /// switches again to the last requested one after the pending switch finishes (in
    /// the middle, `fullscreenElement` has not changed yet, so it cannot be decided on
    /// the spot).
    pub fn set_state(&self, state: WindowState) {
        self.0.wanted.set(Some(state == WindowState::Fullscreen));
        self.apply_wanted();
    }

    /// If the requested direction differs from the current state, requests a switch.
    ///
    /// It does not request while waiting for a switch (it is called again when that
    /// finishes).
    fn apply_wanted(&self) {
        if self.0.in_flight.get().is_some() {
            return;
        }
        let Some(wanted) = self.0.wanted.get() else {
            return;
        };
        if wanted == (self.state() == WindowState::Fullscreen) {
            self.0.wanted.set(None);
            return;
        }
        let promise = if wanted {
            self.0
                .document
                .document_element()
                .and_then(|page| call_promise(page.as_ref(), "requestFullscreen"))
        } else {
            call_promise(self.0.document.as_ref(), "exitFullscreen")
        };
        let settled = self.0.settled.borrow();
        let (Some(promise), Some(settled)) = (promise, settled.as_ref()) else {
            // Fullscreen API の無いブラウザ。頼みは捨てる。
            self.0.wanted.set(None);
            return;
        };
        self.0.in_flight.set(Some(wanted));
        let _ = promise.then2(settled, settled);
    }

    /// The awaited switch has finished (fulfilled or rejected).
    ///
    /// If the same direction as the one that finished is still requested, it counts as
    /// answered (so a refused request is not retried forever). If the opposite direction
    /// was requested meanwhile, it compares with the current state and switches again.
    fn finish_fullscreen_request(&self) {
        let done = self.0.in_flight.take();
        if done.is_some() && self.0.wanted.get() == done {
            self.0.wanted.set(None);
        }
        self.apply_wanted();
    }

    /// Notified when the display state changes. Receives the new
    /// [`state`](Self::state).
    ///
    /// Called on entering and leaving fullscreen (`fullscreenchange`). Also arrives when
    /// the user leaves fullscreen with Esc.
    pub fn on_state_change(&self, f: impl FnMut(WindowState) + 'static) {
        self.0.on_state_change.set(f);
    }

    /// Whether the user can resize it. Returns the value requested with
    /// [`set_resizable`](Self::set_resizable) (`true` by default).
    pub fn is_resizable(&self) -> bool {
        self.0.resizable.get()
    }

    /// Whether the user can resize it. **It has no effect on the Web** (the value is
    /// just remembered).
    ///
    /// Windows on the page have no edges for the user to grab, and a page cannot stop
    /// the browser window from being resized. It is accepted anyway so that the same
    /// code can be written for all four platforms.
    pub fn set_resizable(&self, resizable: bool) {
        self.0.resizable.set(resizable);
    }

    /// Notified of keys pressed inside this window.
    ///
    /// Keys for which the focused widget's `on_key_down` returned `Continue` arrive
    /// here. Returning `Handled` prevents the key's default action. Keys pressed while
    /// an IME is composing are not delivered.
    pub fn on_key_down(
        &self,
        f: impl FnMut(&naui_core::KeyEvent) -> naui_core::EventResponse + 'static,
    ) {
        let document: &web_sys::EventTarget = self.0.document.as_ref();
        let _ = self
            .0
            .key_down
            .set_on_window(document, self.0.element.as_ref(), f);
    }

    /// The minimum size the user can shrink it to (content width, height). The units
    /// are the same as [`set_size`](Self::set_size).
    ///
    /// Prevents the layout from breaking when it is made too small.
    ///
    /// On the Web, `min-width` / `min-height` are put on the window element (the size of
    /// the browser window itself cannot be decided, so when it is smaller, the page
    /// scrolls). The heights of the menu bar and toolbar rows are added to the height.
    pub fn set_min_size(&self, width: f64, height: f64) {
        self.0.min_size.set(Some((width.max(0.0), height.max(0.0))));
        self.apply_size();
    }

    pub fn set_title(&self, title: &str) {
        *self.0.title.borrow_mut() = title.to_string();
        self.0.document.set_title(title);
    }

    pub fn title(&self) -> String {
        self.0.title.borrow().clone()
    }

    /// Changes the size of the content area. The units and extent are the same as
    /// [`size`](Self::size), excluding the menu bar and toolbar rows (the same as the
    /// other platforms).
    ///
    /// The size of the browser window itself cannot be decided, so only the width is
    /// used, as the window element's `max-width` (a maximum width that shrinks on
    /// narrow screens). The height follows the browser's visible area, as a desktop
    /// window follows the size the user gives it.
    pub fn set_size(&self, width: f64, height: f64) {
        self.0.size.set((width.max(0.0), height.max(0.0)));
        self.apply_size();
    }

    /// The widget placed at the root. Each call replaces it.
    ///
    /// The root fills the window (the same as AppKit's contentView). With a sidebar
    /// attached, it is placed on the content side to the sidebar's right.
    pub fn set_child(&self, child: &dyn Widget) {
        *self.0.child.borrow_mut() = Some(child.boxed_clone());
        self.mount_body();
    }

    /// The sidebar attached to the left of the window. Each call replaces it.
    ///
    /// Browsers have no sidebar control, so the inside of the window element is
    /// rearranged into "sidebar (`<aside>`) and content" side by side, and the child
    /// from [`set_child`](Self::set_child) moves to the content side.
    pub fn set_sidebar(&self, sidebar: &Sidebar) {
        *self.0.sidebar.borrow_mut() = Some(sidebar.clone());
        self.mount_body();
    }

    /// Removes the attached sidebar. Does nothing if none is attached.
    ///
    /// The child on the content side goes back to being the window's content.
    pub fn clear_sidebar(&self) {
        let old = self.0.sidebar.borrow_mut().take();
        if let Some(old) = old {
            old.set_content(None);
            old.mount().remove();
            self.mount_body();
        }
    }

    /// Rebuilds the contents of the window element.
    ///
    /// Places the child (and the sidebar), and inserts the toolbar and menu bar at the
    /// start.
    fn mount_body(&self) {
        self.0.element.set_inner_html("");
        let child = self
            .0
            .child
            .borrow()
            .as_ref()
            .map(|child| child.native_element());
        let sidebar = self.0.sidebar.borrow().clone();
        let body = match &sidebar {
            Some(sidebar) => {
                sidebar.set_content(child.as_ref());
                Some(sidebar.mount().unchecked_into::<Element>())
            }
            None => child,
        };
        if let Some(body) = body {
            if self.0.element.append_child(&body).is_ok() {
                crate::layout::fill_parent(&body);
                crate::layout::apply_child_layout(
                    &body,
                    crate::layout::ParentLayout::Flex(naui_core::Orientation::Vertical),
                );
            }
        }
        // 中身を入れ替えると差し込んだ要素も消えるので、付け直す。
        // メニューバーはツールバーより上に来る (OS のメニューと同じ順)。
        self.mount_toolbar();
        self.mount_menu_bar();
    }

    /// The toolbar attached to the top of the window. Each call replaces it.
    ///
    /// Browsers have no title bar, so it is placed at the start of the window element.
    pub fn set_toolbar(&self, toolbar: &Toolbar) {
        self.clear_toolbar();
        *self.0.toolbar.borrow_mut() = Some(toolbar.clone());
        self.mount_toolbar();
    }

    /// Removes the attached toolbar. Does nothing if none is attached.
    pub fn clear_toolbar(&self) {
        if let Some(old) = self.0.toolbar.borrow_mut().take() {
            old.mount().remove();
        }
        self.mount_toolbar();
    }

    /// Rebuilds the toolbar row and places it at the start of the window.
    ///
    /// The order is [sidebar collapse button][toolbar]. If neither exists, the whole row
    /// is removed. Even without a toolbar, if there is a sidebar it becomes a row with
    /// just the button (the same as macOS's button-only toolbar).
    fn mount_toolbar(&self) {
        let row = &self.0.toolbar_row;
        row.set_inner_html("");
        if let Some(sidebar) = self.0.sidebar.borrow().as_ref() {
            let _ = row.append_child(&sidebar.toggle());
        }
        if let Some(toolbar) = self.0.toolbar.borrow().as_ref() {
            let _ = row.append_child(&toolbar.mount());
        }
        if row.child_element_count() == 0 {
            row.remove();
            return;
        }
        // メニューバーが付いていれば、その直下 (OS のメニューと同じ順)。
        let menu_bar = self.0.menu_bar.borrow().as_ref().map(|m| m.mount());
        match menu_bar.filter(|m| m.parent_element().as_ref() == Some(self.0.element.as_ref())) {
            Some(menu_bar) => {
                let _ = self
                    .0
                    .element
                    .insert_before(row, menu_bar.next_sibling().as_ref());
            }
            None => self.mount_first(row),
        }
    }

    /// The menu bar attached to the top of the window. Each call replaces it.
    ///
    /// Browsers have no OS menu bar, so it is placed at the start of the window element
    /// (above the toolbar).
    pub fn set_menu_bar(&self, menu_bar: &MenuBar) {
        self.clear_menu_bar();
        *self.0.menu_bar.borrow_mut() = Some(menu_bar.clone());
        self.mount_menu_bar();
        if let Some(observer) = self.0.bars_observer.borrow().as_ref() {
            observer.observe(menu_bar.mount().as_ref());
        }
        // ショートカットの購読は、取り付けている間だけ張る。
        menu_bar.attach(self.0.element.as_ref());
    }

    /// Removes the attached menu bar. Does nothing if none is attached.
    ///
    /// The removed menu bar no longer reacts to shortcuts either.
    pub fn clear_menu_bar(&self) {
        let old = self.0.menu_bar.borrow_mut().take();
        if let Some(old) = old {
            old.detach();
            if let Some(observer) = self.0.bars_observer.borrow().as_ref() {
                observer.unobserve(old.mount().as_ref());
            }
            old.mount().remove();
            self.apply_size();
        }
    }

    /// Places the menu bar again at the start of the window.
    fn mount_menu_bar(&self) {
        let menu_bar = self.0.menu_bar.borrow();
        let Some(menu_bar) = menu_bar.as_ref() else {
            return;
        };
        self.mount_first(&menu_bar.mount());
    }

    /// Inserts at the start of the window element.
    fn mount_first(&self, mount: &HtmlElement) {
        let first = self.0.element.first_element_child();
        let _ = self
            .0
            .element
            .insert_before(mount, first.as_ref().map(|e| e.as_ref()));
    }

    /// Shows it. On the Web it is shown from the start, so this restores it if it was
    /// hidden.
    pub fn show(&self) {
        // 中身を縦に積む flex コンテナへ戻す (`none` からの復帰)。
        let _ = self.0.element.style().set_property("display", "flex");
    }

    pub fn close(&self) {
        // 開いたままのメニューを残さない (隠れたウィンドウの Esc を拾わない)。
        let menu_bar = self.0.menu_bar.borrow().clone();
        if let Some(menu_bar) = menu_bar {
            menu_bar.close();
        }
        let _ = self.0.element.style().set_property("display", "none");
    }

    pub fn is_visible(&self) -> bool {
        self.0
            .element
            .style()
            .get_property_value("display")
            .map(|v| v != "none")
            .unwrap_or(true)
    }

    /// Switches this window's color theme.
    pub fn set_theme(&self, theme: Theme) -> Result<()> {
        apply_theme(&self.0.document, theme)
    }

    /// The DOM element. A backend-specific escape hatch.
    pub fn native_element(&self) -> Element {
        self.0.element.clone().unchecked_into()
    }
}

/// Calls `target`'s method `name` without arguments and returns the Promise it
/// returned.
///
/// web-sys's `request_fullscreen` / `exit_fullscreen` are the old forms that do not
/// return a Promise, so refusals cannot be received. `None` in browsers without the
/// method (`requestFullscreen` on iPhone Safari, for example).
fn call_promise(target: &wasm_bindgen::JsValue, name: &str) -> Option<js_sys::Promise> {
    let method = js_sys::Reflect::get(target, &wasm_bindgen::JsValue::from_str(name))
        .ok()?
        .dyn_into::<js_sys::Function>()
        .ok()?;
    method
        .call0(target)
        .ok()?
        .dyn_into::<js_sys::Promise>()
        .ok()
}

/// The vertical space the page takes around its windows: the margins, borders and
/// padding of `<html>` and `<body>` (a page without a style sheet has an 8 px margin
/// on `<body>`).
///
/// Without subtracting it, a window as tall as the browser's visible area would
/// stick out at the bottom by that much, and the whole page would scroll even when
/// the content fits in the window. Read when the size is applied, so a page whose
/// own style changes these later gets them on the next `set_size`.
fn page_inset(document: &Document) -> f64 {
    let Some(window) = web_sys::window() else {
        return 0.0;
    };
    let elements = [
        document.document_element(),
        document.body().map(|body| body.unchecked_into::<Element>()),
    ];
    let properties = [
        "margin-top",
        "margin-bottom",
        "border-top-width",
        "border-bottom-width",
        "padding-top",
        "padding-bottom",
    ];
    elements
        .into_iter()
        .flatten()
        .filter_map(|element| window.get_computed_style(&element).ok().flatten())
        .flat_map(|style| {
            properties.map(|property| {
                style
                    .get_property_value(property)
                    .ok()
                    .and_then(|value| value.trim_end_matches("px").parse::<f64>().ok())
                    .unwrap_or(0.0)
            })
        })
        .sum()
}
