//! The traits shared by widgets, and macros for creating the wrapper types.

use crate::sys;
use naui_core::{FileEntry, Sizing};

pub(crate) mod sealed {
    pub trait Sealed {}
}

/// A closure given to `on_file_drop`, as a trait object.
pub(crate) type FileDropFn = Box<dyn FnMut(&[FileEntry])>;

/// The common interface implemented by naui's widgets.
///
/// It cannot be implemented outside naui. Containers take children as
/// [`&dyn AsWidget`](AsWidget), which every `Widget` implements, so a `&dyn Widget`
/// can be passed as it is. To pass a type of your own, implement [`AsWidget`] for it.
pub trait Widget: AsWidget + sealed::Sealed + 'static {
    /// The wrapped backend widget. Used when putting it into a container.
    #[doc(hidden)]
    fn as_backend(&self) -> &dyn sys::Widget;
}

/// Something that can be put into a container: a naui widget, or a type of your
/// own that holds one.
///
/// Methods that put children into containers ([`Stack::append`](crate::Stack::append),
/// [`Window::set_child`](crate::Window::set_child) and others) take it as
/// `&dyn AsWidget`. Every naui widget implements it, returning itself. A type
/// made outside naui (a composition of widgets, a view in an app's own crate)
/// implements it by returning the widget it places: the outermost container it
/// built, usually. Then it can be passed as `stack.append(&tag)` rather than
/// handing out the widget inside at every call.
///
/// ```no_run
/// # use naui::{AsWidget, Orientation, Result, Ui, Widget};
/// /// A label with a button that removes it.
/// struct Tag {
///     root: naui::Stack,
///     remove: naui::Button,
/// }
///
/// impl Tag {
///     fn new(ui: &Ui, text: &str) -> Result<Tag> {
///         let root = ui.stack(Orientation::Horizontal)?;
///         let remove = ui.button("×")?;
///         root.append(&ui.label(text)?);
///         root.append(&remove);
///         Ok(Tag { root, remove })
///     }
/// }
///
/// impl AsWidget for Tag {
///     fn as_widget(&self) -> &dyn Widget {
///         &self.root
///     }
/// }
///
/// # fn build(ui: &Ui, stack: &naui::Stack) -> Result<()> {
/// let tag = Tag::new(ui, "Rust")?;
/// stack.append(&tag);
/// tag.set_tooltip(Some("A tag")); // reaches `tag.root`
/// # Ok(())
/// # }
/// ```
///
/// The container holds the widget returned, not the value implementing this:
/// it keeps a reference to the widget as it does when the widget is passed
/// directly, and the value can be dropped or moved afterwards. Return the same
/// widget each time, so that the methods below reach the widget that was placed.
///
/// The other methods are the operations every widget has, and by default reach the
/// widget returned by [`as_widget`](Self::as_widget). On naui's own widgets each
/// type's method of the same name is called, with the same result. They are
/// provided for types of your own, and for `&dyn Widget` values, which have no
/// methods of their own to call; override one only to do more than pass it on.
/// [`downgrade`](crate::Button::downgrade) is not among them, since it names the
/// widget's own type: keep a weak reference to the inner widget instead.
pub trait AsWidget {
    /// The widget placed in the container.
    fn as_widget(&self) -> &dyn Widget;

    /// Sets the size of the widget. Each call discards the previous setting. See
    /// [`Button::set_sizing`](crate::Button::set_sizing).
    fn set_sizing(&self, sizing: Sizing) {
        self.as_widget().set_sizing(sizing)
    }

    /// Whether the widget is shown. Shown by default. See
    /// [`Button::set_visible`](crate::Button::set_visible).
    fn set_visible(&self, visible: bool) {
        self.as_widget().set_visible(visible)
    }

    /// The visibility set with [`set_visible`](Self::set_visible). Stays `true` even
    /// if a parent is hidden.
    fn is_visible(&self) -> bool {
        self.as_widget().is_visible()
    }

    /// Moves keyboard focus into the widget. `true` if it moved. See
    /// [`Button::request_focus`](crate::Button::request_focus).
    fn request_focus(&self) -> bool {
        self.as_widget().request_focus()
    }

    /// The description shown when the pointer hovers over the widget. `None`
    /// removes it.
    fn set_tooltip(&self, text: Option<&str>) {
        self.as_widget().set_tooltip(text)
    }

    /// The name reported to screen readers. `None` removes it (the visible text is
    /// used). See [`Button::set_accessible_label`](crate::Button::set_accessible_label).
    fn set_accessible_label(&self, text: Option<&str>) {
        self.as_widget().set_accessible_label(text)
    }

    /// Called with the files dropped onto the widget from the OS. See
    /// [`Button::on_file_drop`](crate::Button::on_file_drop).
    ///
    /// Not callable on a `&dyn Widget` or `&dyn AsWidget`; call it on the widget
    /// or on your own type.
    fn on_file_drop(&self, f: impl FnMut(&[FileEntry]) + 'static)
    where
        Self: Sized,
    {
        self.on_file_drop_boxed(Box::new(f))
    }

    /// [`on_file_drop`](Self::on_file_drop) through a trait object.
    #[doc(hidden)]
    fn on_file_drop_boxed(&self, f: FileDropFn) {
        self.as_widget().on_file_drop_boxed(f)
    }

    /// Drops `files` onto the widget the way the user drops files from the OS. See
    /// [`Button::drop_files`](crate::Button::drop_files).
    fn drop_files(&self, files: &[FileEntry]) {
        self.as_widget().drop_files(files)
    }
}

/// The shared part of types that are not widgets (`Window`, `ListRow` and others).
macro_rules! impl_backend {
    ($t:ident) => {
        impl $t {
            /// The wrapped backend type. Native operations that naui does not offer
            /// (`native_*`) are called through this.
            ///
            /// The type differs per backend being built (the type of the same name in
            /// `naui::macos` / `naui::windows` / `naui::gtk` / `naui::web`).
            #[inline]
            pub fn backend(&self) -> &crate::sys::$t {
                &self.0
            }
        }
    };
}

/// The shared part of widgets: the [`Widget`] implementation and the ten methods
/// every widget has.
macro_rules! impl_widget {
    ($t:ident) => {
        impl_backend!($t);

        impl crate::widget::sealed::Sealed for $t {}

        impl crate::Widget for $t {
            #[inline]
            fn as_backend(&self) -> &dyn crate::sys::Widget {
                &self.0
            }
        }

        // 既定のメソッドは as_widget() へ転送するので、ここで同じ名前の
        // メソッドへ向け直す (向け直さないと自分へ転送し続ける)。
        impl crate::AsWidget for $t {
            #[inline]
            fn as_widget(&self) -> &dyn crate::Widget {
                self
            }
            #[inline]
            fn set_sizing(&self, sizing: naui_core::Sizing) {
                $t::set_sizing(self, sizing)
            }
            #[inline]
            fn set_visible(&self, visible: bool) {
                $t::set_visible(self, visible)
            }
            #[inline]
            fn is_visible(&self) -> bool {
                $t::is_visible(self)
            }
            #[inline]
            fn request_focus(&self) -> bool {
                $t::request_focus(self)
            }
            #[inline]
            fn set_tooltip(&self, text: Option<&str>) {
                $t::set_tooltip(self, text)
            }
            #[inline]
            fn set_accessible_label(&self, text: Option<&str>) {
                $t::set_accessible_label(self, text)
            }
            #[inline]
            fn on_file_drop(&self, f: impl FnMut(&[naui_core::FileEntry]) + 'static) {
                $t::on_file_drop(self, f)
            }
            #[inline]
            fn on_file_drop_boxed(&self, f: crate::widget::FileDropFn) {
                $t::on_file_drop(self, f)
            }
            #[inline]
            fn drop_files(&self, files: &[naui_core::FileEntry]) {
                $t::drop_files(self, files)
            }
        }

        impl $t {
            /// Sets the size. Each call discards the previous setting.
            ///
            /// The actual size is decided by GTK4's layout, so what you pass here is only
            /// "at least this much", "take the leftover space" and "stop at this maximum".
            ///
            /// It can be called before or after putting the widget into a container, with
            /// the same result (see
            /// [changing widgets after adding](crate::guide#changing-and-moving-widgets-after-adding)).
            #[inline]
            pub fn set_sizing(&self, sizing: naui_core::Sizing) {
                self.0.set_sizing(sizing)
            }

            /// Whether it is shown. Shown by default.
            ///
            /// A hidden widget takes no space in a `Stack`. In a `Grid`, its cell remains.
            #[inline]
            pub fn set_visible(&self, visible: bool) {
                self.0.set_visible(visible)
            }

            /// The visibility set with [`set_visible`](Self::set_visible). Stays `true` even
            /// if a parent is hidden.
            #[inline]
            pub fn is_visible(&self) -> bool {
                self.0.is_visible()
            }

            /// Moves keyboard focus here. `true` if it moved.
            ///
            /// If the widget itself cannot take focus (containers, for example), focus moves to
            /// the first thing inside that can. `false` before it is shown, while hidden, or
            /// while disabled.
            #[inline]
            pub fn request_focus(&self) -> bool {
                self.0.request_focus()
            }

            /// The description shown when the pointer hovers over it. `None` removes it.
            #[inline]
            pub fn set_tooltip(&self, text: Option<&str>) {
                self.0.set_tooltip(text)
            }

            /// The name reported to screen readers. `None` removes it (the visible text is
            /// used).
            ///
            /// Set it when there is no visible text, or the text does not fully convey the
            /// meaning, as with icon-only buttons.
            #[inline]
            pub fn set_accessible_label(&self, text: Option<&str>) {
                self.0.set_accessible_label(text)
            }

            /// Called with the files dropped onto this widget from the OS (Finder,
            /// Explorer, a file manager).
            ///
            /// When widgets with this closure are nested, the innermost one under the
            /// pointer takes the drop, and the window's
            /// [`on_file_drop`](crate::Window::on_file_drop) gets only drops no widget
            /// takes. Setting it again replaces the previous closure.
            ///
            /// On the Web the entries carry no path; [`FileEntry::source`] is a `blob:`
            /// URL that stays valid until the next drop onto the same widget, or until
            /// the widget is dropped. See
            /// [Receiving dropped files](crate::guide#receiving-dropped-files).
            ///
            /// [`FileEntry::source`]: naui_core::FileEntry::source
            #[inline]
            pub fn on_file_drop(&self, f: impl FnMut(&[naui_core::FileEntry]) + 'static) {
                self.0.on_file_drop(f)
            }

            /// Drops `files` onto this widget the way the user drops files from the
            /// OS. The drop goes where a real one would: to this widget's
            /// [`on_file_drop`](Self::on_file_drop), or else to the nearest enclosing
            /// widget that has one, or else to its window's
            /// [`on_file_drop`](crate::Window::on_file_drop). Nothing happens if
            /// `files` is empty or nothing takes the drop.
            ///
            /// Natively the entries arrive as given. On the Web they arrive as the
            /// browser delivers dropped files: an empty file named after each entry,
            /// with a `blob:` URL and no path.
            ///
            /// For tests and automation. See
            /// [Testing apps](crate::guide#testing-apps).
            #[inline]
            pub fn drop_files(&self, files: &[naui_core::FileEntry]) {
                self.0.drop_files(files)
            }

            /// Runs `f` when `shortcut` is pressed while keyboard focus is inside this
            /// widget (on it or on anything it contains). Giving the same shortcut
            /// again replaces the closure; [`remove_shortcut`](Self::remove_shortcut)
            /// takes it away.
            ///
            /// The focused widget's `on_key_down` comes first, then the shortcuts of
            /// the widgets around the focus from the inside out, then the window's
            /// `on_key_down` and [shortcuts](crate::Window::on_shortcut), then the
            /// menu bar. A shortcut that fires takes the key, so the focused control
            /// does not get it. Invalid shortcuts ([`Shortcut::is_valid`]) never fire.
            /// See [Keyboard shortcuts](crate::guide#keyboard-shortcuts).
            ///
            /// [`Shortcut::is_valid`]: naui_core::Shortcut::is_valid
            #[inline]
            pub fn on_shortcut(&self, shortcut: naui_core::Shortcut, f: impl FnMut() + 'static) {
                self.0.on_shortcut(shortcut, f)
            }

            /// Removes a shortcut given with [`on_shortcut`](Self::on_shortcut).
            /// Nothing happens if it was not given.
            #[inline]
            pub fn remove_shortcut(&self, shortcut: naui_core::Shortcut) {
                self.0.remove_shortcut(shortcut)
            }

            /// Creates a weak reference. Capture it when touching this widget from its own
            /// notification closures (capturing a `clone()` creates a cycle and it is never
            /// freed).
            #[inline]
            pub fn downgrade(&self) -> naui_core::WeakWidget<Self> {
                let weak = self.0.downgrade();
                naui_core::WeakWidget::new(move || weak.upgrade().map(Self))
            }
        }
    };
}
