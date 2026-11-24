use naui_core::HostSize;

/// A place for a native view made outside naui, among ordinary widgets.
///
/// It is for crates that make a view naui does not have: a surface a GPU draws
/// into, a web view, a control of the platform's own. naui places it, sizes it,
/// shows and hides it, reports its size and the display scale
/// ([`on_resize`](Self::on_resize)) and when it joins or leaves a window
/// ([`on_attach`](Self::on_attach)). What the view draws, and any GPU setup or
/// loop that drives it, is left to the crate that made it.
///
/// The view is put in through the backend, since its type is the platform's own:
///
/// | Platform | Call | Takes |
/// | --- | --- | --- |
/// | macOS | `host.backend().set_hosted_view(Some(&view))` | `&NSView` |
/// | Linux | `host.backend().set_hosted_widget(Some(&widget))` | `&impl IsA<gtk::Widget>` |
/// | Windows | `host.backend().set_hosted_element(Some(&element))` | `&UIElement` (`naui-winui3`) |
/// | Web | `host.backend().set_hosted_element(Some(&element))` | `&web_sys::Element` |
///
/// ```ignore
/// let host = ui.native_host()?;
/// host.set_sizing(naui::Sizing::fill());
/// #[cfg(target_os = "macos")]
/// host.backend().set_hosted_view(Some(&my_ns_view));
/// #[cfg(target_arch = "wasm32")]
/// host.backend().set_hosted_element(Some(&my_canvas_element));
/// host.on_resize(|size| {
///     let (width, height) = size.physical_size(); // resize the GPU surface here
/// });
/// stack.append(&host);
/// ```
///
/// The hosted view fills the host. Where the view knows its own size (a control),
/// the host takes that size, as other widgets take their content's; where it does
/// not (a GPU surface), set the size with `set_sizing`, as for a [`Canvas`].
///
/// naui holds the hosted view until it is replaced or the host is dropped, and then
/// takes it out of the host. See
/// [Hosting native views](crate::guide#hosting-native-views).
///
/// [`Canvas`]: crate::Canvas
#[derive(Clone)]
pub struct NativeHost(pub(crate) crate::sys::NativeHost);

impl_widget!(NativeHost);

impl NativeHost {
    /// The current size (width, height) in logical pixels. 0 if not laid out yet.
    #[inline]
    pub fn size(&self) -> (f64, f64) {
        self.0.size()
    }

    /// Physical pixels per logical pixel on the display the host is shown on (2.0
    /// on a typical Retina or 200% display).
    ///
    /// Before the host is in a window it is a guess: the main display's on macOS,
    /// the display's on Linux, `devicePixelRatio` on the Web and 1.0 on Windows.
    /// On Linux it is GTK's whole-number scale factor (a fractional scale of 125%
    /// reads 2).
    #[inline]
    pub fn scale_factor(&self) -> f64 {
        self.0.scale_factor()
    }

    /// Called with the size and scale ([`HostSize`]) whenever either changes: after
    /// the host is laid out for the first time, when it is resized, and when it
    /// moves to a display with another scale. Setting it again replaces the
    /// previous closure. It is not called on the spot when set; read
    /// [`size`](Self::size) and [`scale_factor`](Self::scale_factor) for the
    /// current values.
    ///
    /// The first call has a size with some area: the 0 × 0 a host has before its
    /// first layout is not reported. Later the size can become 0 (the host hidden
    /// on the Web, for example), and that is reported like any change.
    ///
    /// It runs inside the platform's layout pass on macOS and Linux, and after it on
    /// Windows and the Web, so resize a GPU surface here, but change the layout of
    /// other widgets later (with [`Ui::tasks`](crate::Ui::tasks), for example).
    #[inline]
    pub fn on_resize(&self, f: impl FnMut(HostSize) + 'static) {
        self.0.on_resize(f)
    }

    /// Called with `true` when the host joins a window and `false` when it leaves
    /// one (it, or a container it is in, is taken out of the window, or the
    /// window's content is replaced). Setting it again replaces the previous
    /// closure. Closing a window does not take its content out, so it does not
    /// count as leaving on macOS, Linux and the Web. On Windows XAML unloads the
    /// content of a window when it closes, so closing it reports `false`.
    ///
    /// When it counts as joined differs: on macOS and Linux once it is in the
    /// window's widget tree, shown or not; on Windows when XAML loads it, which is
    /// once the window is shown; on the Web once it is in the page. See
    /// [Hosting native views](crate::guide#hosting-native-views).
    #[inline]
    pub fn on_attach(&self, f: impl FnMut(bool) + 'static) {
        self.0.on_attach(f)
    }

    /// Whether the host is in a window now, by the same rule as
    /// [`on_attach`](Self::on_attach).
    #[inline]
    pub fn is_attached(&self) -> bool {
        self.0.is_attached()
    }
}
