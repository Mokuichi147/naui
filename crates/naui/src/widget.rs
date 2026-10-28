//! The trait shared by widgets, and macros for creating the wrapper types.

use crate::sys;

pub(crate) mod sealed {
    pub trait Sealed {}
}

/// The common interface implemented by naui's widgets.
///
/// Methods that put children into containers ([`Stack::append`](crate::Stack::append)
/// and others) take it as `&dyn Widget`. It cannot be implemented outside naui.
pub trait Widget: sealed::Sealed + 'static {
    /// The wrapped backend widget. Used when putting it into a container.
    #[doc(hidden)]
    fn as_backend(&self) -> &dyn sys::Widget;
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

/// The shared part of widgets: the [`Widget`] implementation and the eight methods
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
            /// URL that stays valid until the next drop onto the same widget. See
            /// [Receiving dropped files](crate::guide#receiving-dropped-files).
            ///
            /// [`FileEntry::source`]: naui_core::FileEntry::source
            #[inline]
            pub fn on_file_drop(&self, f: impl FnMut(&[naui_core::FileEntry]) + 'static) {
                self.0.on_file_drop(f)
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
