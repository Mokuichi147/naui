use crate::sys;
use crate::Widget;
use crate::{MenuBar, Sidebar, Toolbar};
use naui_core::{
    CloseResponse, EventResponse, FileEntry, KeyEvent, Result, Theme, WindowPlacement, WindowState,
};

/// A weak reference for referring to a window from event handlers without holding
/// it strongly.
#[derive(Clone)]
pub struct WeakWindow(pub(crate) sys::WeakWindow);

impl_backend!(WeakWindow);

impl WeakWindow {
    #[inline]
    pub fn upgrade(&self) -> Option<Window> {
        self.0.upgrade().map(Window)
    }
}

/// A top-level window.
///
/// Create it inside the callback passed to `run`. The framework (`GtkApplication`)
/// keeps a reference to it, so dropping the return value does not close it.
#[derive(Clone)]
pub struct Window(pub(crate) sys::Window);

impl_backend!(Window);

impl Window {
    /// Confirmation when the user tries to close the window.
    ///
    /// Returning [`CloseResponse::KeepOpen`] keeps it open (to ask for confirmation
    /// when there are unsaved changes, for example). Not called for
    /// [`close`](Self::close) from code.
    #[inline]
    pub fn on_close_request(&self, f: impl FnMut() -> CloseResponse + 'static) {
        self.0.on_close_request(f)
    }

    /// The size of the content area (width, height) in logical pixels, excluding the
    /// header bar and menu bar. With a sidebar attached, it includes the sidebar's
    /// width, as on the other platforms.
    ///
    /// Before it is on screen, this is the size given to `ui.window` or
    /// [`set_size`](Self::set_size).
    #[inline]
    pub fn size(&self) -> (f64, f64) {
        self.0.size()
    }

    /// The position of the window's top-left corner. **It cannot be obtained on Linux,
    /// so this is always `None`.**
    ///
    /// GTK4 has no API to read or move a window's position. On Wayland there is no
    /// mechanism at all for an app to know its window's position (the compositor
    /// decides placement), and GTK4 dropped it on X11 too to match.
    #[inline]
    pub fn position(&self) -> Option<(f64, f64)> {
        self.0.position()
    }

    /// Moves the window. **Does nothing on Linux** (see [`position`](Self::position)).
    #[inline]
    pub fn set_position(&self, x: f64, y: f64) {
        self.0.set_position(x, y)
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
    #[inline]
    pub fn state(&self) -> WindowState {
        self.0.state()
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
    #[inline]
    pub fn set_state(&self, state: WindowState) {
        self.0.set_state(state)
    }

    /// Notified when the display state changes. Receives the new
    /// [`state`](Self::state).
    ///
    /// Called both for user actions (header bar buttons, snapping to a screen edge and
    /// so on) and for [`set_state`](Self::set_state).
    #[inline]
    pub fn on_state_change(&self, f: impl FnMut(WindowState) + 'static) {
        self.0.on_state_change(f)
    }

    /// The size, position and state to save when the app quits, for
    /// [`set_placement`](Self::set_placement) the next time it starts.
    ///
    /// The size and position are those of the normal state: while maximized or in
    /// fullscreen, they are where the window returns to (the same as
    /// [`position`](Self::position)). A minimized window reports the state it
    /// returns to, never [`WindowState::Minimized`]. **On Linux and the Web the
    /// position is `None`.** On Linux, the size is the last one seen in the normal
    /// state (not while maximized, fullscreen or tiled to a screen edge). On the Web,
    /// it is the size requested with [`set_size`](Self::set_size), because the
    /// browser decides the actual size.
    ///
    /// See [Saving and restoring the window](crate::guide#saving-and-restoring-the-window).
    #[inline]
    pub fn placement(&self) -> WindowPlacement {
        self.0.placement()
    }

    /// Applies a saved [`placement`](Self::placement): the size, then the position,
    /// then the state. Call it before [`show`](Self::show) so the window appears
    /// where it was.
    ///
    /// The position is used only if the window's top edge would be on one of the
    /// screens connected now; a position on a screen that has been disconnected is
    /// skipped. **Linux and the Web restore only the size and state**, and browsers
    /// refuse fullscreen outside a user action, so on the Web only the size applies
    /// at startup.
    #[inline]
    pub fn set_placement(&self, placement: &WindowPlacement) {
        self.0.set_placement(placement)
    }

    /// Whether the user can resize the window by dragging its edges. `true` by default.
    #[inline]
    pub fn is_resizable(&self) -> bool {
        self.0.is_resizable()
    }

    /// Decides whether the user can resize the window.
    ///
    /// Uses `gtk_window_set_resizable`. Turning it off also removes the header bar's
    /// maximize button. [`set_size`](Self::set_size) works either way.
    #[inline]
    pub fn set_resizable(&self, resizable: bool) {
        self.0.set_resizable(resizable)
    }

    /// Notified when the size changes. Receives the new [`size`](Self::size).
    #[inline]
    pub fn on_resize(&self, f: impl FnMut((f64, f64)) + 'static) {
        self.0.on_resize(f)
    }

    /// Called with the files dropped onto the window from the OS (Finder, Explorer,
    /// a file manager).
    ///
    /// A widget under the pointer with its own `on_file_drop` takes the drop instead,
    /// so this gets only the drops no widget takes. Setting it again replaces the
    /// previous closure.
    ///
    /// On the Web the entries carry no path; [`FileEntry::source`] is a `blob:` URL
    /// that stays valid until the next drop onto the same window, or until the
    /// window is dropped. See
    /// [Receiving dropped files](crate::guide#receiving-dropped-files).
    #[inline]
    pub fn on_file_drop(&self, f: impl FnMut(&[FileEntry]) + 'static) {
        self.0.on_file_drop(f)
    }

    #[inline]
    pub fn downgrade(&self) -> WeakWindow {
        WeakWindow(self.0.downgrade())
    }

    /// Notified of keys pressed inside this window.
    ///
    /// Keys for which the focused widget's `on_key_down` returned `Continue` arrive
    /// here. Returning `Handled` keeps the key from reaching the widget. Keys pressed
    /// while an IME is composing are not delivered.
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

    /// The minimum size the user can shrink it to (content width, height). The units
    /// are the same as [`set_size`](Self::set_size).
    ///
    /// Prevents the layout from breaking when it is made too small.
    ///
    /// A GTK4 window never shrinks below its content's minimum size, so the minimum is
    /// set on the content container (the header bar is added on top of that). While a
    /// sidebar is attached, the minimum width applies to the pane right of the sidebar
    /// (it stops at a width larger by the sidebar's width).
    #[inline]
    pub fn set_min_size(&self, width: f64, height: f64) {
        self.0.set_min_size(width, height)
    }

    /// Changes the size of the content area. The units and extent are the same as
    /// [`size`](Self::size), excluding the header bar and menu bar (the same as the
    /// other platforms).
    ///
    /// GTK4's `set_default_size` takes the size of the whole window, so the heights of
    /// the header bar and menu bar strips are added. When called before it is on
    /// screen, it is shown at this size.
    ///
    /// While maximized or in fullscreen (on Windows also while minimized), the window
    /// keeps its frame and this becomes the size it returns to, the same as
    /// [`set_position`](Self::set_position) does with the position. On the Web only
    /// the width is used, as the window's `max-width`.
    #[inline]
    pub fn set_size(&self, width: f64, height: f64) {
        self.0.set_size(width, height)
    }

    /// Replaces the window's content.
    #[inline]
    pub fn set_child(&self, child: &dyn Widget) {
        self.0.set_child(child.as_backend())
    }

    /// The sidebar attached to the left of the window. Each call replaces it.
    ///
    /// The window's content is replaced with a `GtkPaned`, and the previous content
    /// (header bar, menu bar, child) moves to the pane on its right.
    #[inline]
    pub fn set_sidebar(&self, sidebar: &Sidebar) {
        self.0.set_sidebar(&sidebar.0)
    }

    /// Removes the attached sidebar. Does nothing if none is attached.
    ///
    /// The content of the right pane goes back to being the window's content.
    #[inline]
    pub fn clear_sidebar(&self) {
        self.0.clear_sidebar()
    }

    /// The toolbar attached to the top of the window. Each call replaces it.
    ///
    /// Following GNOME conventions, items are laid out on the left side of the header
    /// bar.
    #[inline]
    pub fn set_toolbar(&self, toolbar: &Toolbar) {
        self.0.set_toolbar(&toolbar.0)
    }

    /// Removes the attached toolbar. Does nothing if none is attached.
    #[inline]
    pub fn clear_toolbar(&self) {
        self.0.clear_toolbar()
    }

    /// The menu bar attached to the top of the window. Each call replaces it.
    ///
    /// The `GtkPopoverMenuBar` goes below the header bar. Item actions are added as a
    /// group of window actions, so accelerators (shortcuts) registered with the
    /// `GtkApplication` work without opening the menu.
    #[inline]
    pub fn set_menu_bar(&self, menu_bar: &MenuBar) {
        self.0.set_menu_bar(&menu_bar.0)
    }

    /// Removes the attached menu bar. Does nothing if none is attached.
    #[inline]
    pub fn clear_menu_bar(&self) {
        self.0.clear_menu_bar()
    }

    /// Shows it on screen and brings it to the front.
    ///
    /// On first show, the content is sized as given to `ui.window` (or to
    /// [`set_size`](Self::set_size) before showing), and the state requested with
    /// [`set_state`](Self::set_state) before showing is applied.
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

    /// Applies a color theme to this window.
    ///
    /// libadwaita has one theme for the whole app, so in practice it applies to the
    /// whole app.
    #[inline]
    pub fn set_theme(&self, theme: Theme) -> Result<()> {
        self.0.set_theme(theme)
    }
}
