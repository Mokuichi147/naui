//! A place for a `GtkWidget` made outside naui (a widget subclass of naui's own).
//!
//! The content control is `NauiNativeHost`, a widget with at most one child: the
//! hosted widget. It measures as its child does and gives the child its whole
//! allocation, so a widget that knows its size (a control) makes the host that
//! size, and one that does not (a `GtkGLArea`, a `GtkPicture` showing frames a GPU
//! renders) gets the size naui's layout gives. Like every naui widget it sits in a
//! [`SizeBin`](crate::SizeBin), which applies naui's size settings.
//!
//! GTK4 has no native child windows, so a GPU surface cannot be placed as one. The
//! hosted widget can be anything GTK draws itself: a `GtkGLArea`, or a `GtkPicture`
//! of `GdkDmabufTexture`s (GTK 4.14) or memory textures. Which of them a drawing
//! crate uses is its own choice; the host only places the widget.
//!
//! The size is reported from `size_allocate`, the scale from `notify::scale-factor`
//! (GTK's whole-number factor), and joining or leaving a window from the `root` /
//! `unroot` virtual functions.

use std::rc::Rc;

use gtk::glib;
use gtk::prelude::*;
use naui_core::HostSize;

use crate::bin::SizeBin;
use crate::widgets::{impl_widget, Widget};

// glib のサブクラス化マクロが `unsafe impl` を作るため、この中だけ許可する。
#[allow(unsafe_code)]
mod imp {
    use std::cell::RefCell;

    use gtk::glib;
    use gtk::prelude::*;
    use gtk::subclass::prelude::*;
    use naui_core::{HostEvents, HostSize};

    #[derive(Default)]
    pub struct NativeHostWidget {
        pub(super) events: HostEvents,
        /// The hosted widget. Also the only child while it is still there.
        pub(super) hosted: RefCell<Option<gtk::Widget>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for NativeHostWidget {
        const NAME: &'static str = "NauiNativeHost";
        type Type = super::NativeHostWidget;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for NativeHostWidget {
        fn constructed(&self) {
            self.parent_constructed();
            self.obj().connect_scale_factor_notify(|widget| {
                widget.imp().report_size();
            });
        }

        fn dispose(&self) {
            while let Some(child) = self.obj().first_child() {
                child.unparent();
            }
        }
    }

    impl NativeHostWidget {
        pub(super) fn current_size(&self) -> HostSize {
            let widget = self.obj();
            HostSize::new(
                f64::from(widget.width()),
                f64::from(widget.height()),
                f64::from(widget.scale_factor()),
            )
        }

        fn report_size(&self) {
            self.events.report_size(self.current_size());
        }
    }

    impl WidgetImpl for NativeHostWidget {
        fn request_mode(&self) -> gtk::SizeRequestMode {
            self.obj()
                .first_child()
                .map_or(gtk::SizeRequestMode::ConstantSize, |child| {
                    child.request_mode()
                })
        }

        fn measure(&self, orientation: gtk::Orientation, for_size: i32) -> (i32, i32, i32, i32) {
            match self.obj().first_child() {
                Some(child) if child.should_layout() => child.measure(orientation, for_size),
                _ => (0, 0, -1, -1),
            }
        }

        fn size_allocate(&self, width: i32, height: i32, baseline: i32) {
            if let Some(child) = self.obj().first_child() {
                if child.should_layout() {
                    child.allocate(width, height, baseline, None);
                }
            }
            // `width()` / `height()` はこの呼び出しの中ではまだ前の値のことがある。
            self.events.report_size(HostSize::new(
                f64::from(width),
                f64::from(height),
                f64::from(self.obj().scale_factor()),
            ));
        }

        fn root(&self) {
            self.parent_root();
            self.events.report_attached(true);
            self.report_size();
        }

        fn unroot(&self) {
            self.events.report_attached(false);
            self.parent_unroot();
        }
    }
}

glib::wrapper! {
    /// The content control of [`NativeHost`]: holds the hosted widget and reports
    /// its size and window.
    pub struct NativeHostWidget(ObjectSubclass<imp::NativeHostWidget>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl NativeHostWidget {
    fn imp(&self) -> &imp::NativeHostWidget {
        gtk::subclass::prelude::ObjectSubclassIsExt::imp(self)
    }
}

struct NativeHostInner {
    native: NativeHostWidget,
    bin: SizeBin,
}

impl Drop for NativeHostInner {
    /// Lets go of the hosted widget, taking it out of the host if it is still there.
    fn drop(&mut self) {
        if let Some(widget) = self.native.imp().hosted.take() {
            if widget.parent().as_ref() == Some(self.native.upcast_ref()) {
                widget.unparent();
            }
        }
    }
}

/// A place for a `GtkWidget` made outside naui.
///
/// Put the widget in with [`set_hosted_widget`](Self::set_hosted_widget). naui
/// places, sizes and shows the host around it, and reports its size and scale
/// ([`on_resize`](Self::on_resize)) and when it joins or leaves a window
/// ([`on_attach`](Self::on_attach)); what the widget draws is up to whoever made it.
#[derive(Clone)]
pub struct NativeHost(Rc<NativeHostInner>);
impl_widget!(NativeHost);

impl NativeHost {
    pub(crate) fn new() -> Self {
        let native: NativeHostWidget = glib::Object::new();
        let bin = SizeBin::wrap(&native);
        Self(Rc::new(NativeHostInner { native, bin }))
    }

    /// Puts `widget` in the host, replacing the widget there before (which is taken
    /// out of the host and let go). `None` empties the host.
    ///
    /// Returns `false`, changing nothing, when `widget` already has a parent other
    /// than the host: GTK4 gives a widget one parent, and only that parent can take
    /// it out. naui keeps a reference to `widget` until it is replaced or the host is
    /// dropped; when either happens, the widget is taken out of the host.
    pub fn set_hosted_widget(&self, widget: Option<&impl IsA<gtk::Widget>>) -> bool {
        let host: &gtk::Widget = self.0.native.upcast_ref();
        let widget = widget.map(|widget| widget.as_ref().clone());
        if let Some(widget) = &widget {
            match widget.parent() {
                Some(parent) if &parent == host => {
                    // 同じものを渡し直したときは何もしない。
                    if self.0.native.imp().hosted.borrow().as_ref() == Some(widget) {
                        return true;
                    }
                }
                Some(_) => return false,
                None => {}
            }
        }
        let previous = self.0.native.imp().hosted.replace(widget.clone());
        if let Some(previous) = previous {
            if previous.parent().as_ref() == Some(host) {
                previous.unparent();
            }
        }
        if let Some(widget) = widget {
            widget.set_parent(host);
        }
        self.0.native.queue_resize();
        true
    }

    /// The widget put in with [`set_hosted_widget`](Self::set_hosted_widget), if any.
    pub fn hosted_widget(&self) -> Option<gtk::Widget> {
        self.0.native.imp().hosted.borrow().clone()
    }

    /// The current size (width, height) in logical pixels. 0 if not laid out yet.
    pub fn size(&self) -> (f64, f64) {
        let size = self.0.native.imp().current_size();
        (size.width, size.height)
    }

    /// GTK's scale factor for the host (`gtk_widget_get_scale_factor`), a whole
    /// number: a fractional scale such as 125% reads 2.
    pub fn scale_factor(&self) -> f64 {
        f64::from(self.0.native.scale_factor())
    }

    /// Called with the new size and scale when either changes (from `size_allocate`
    /// and `notify::scale-factor`). It runs inside GTK's layout pass.
    pub fn on_resize(&self, f: impl FnMut(HostSize) + 'static) {
        self.0.native.imp().events.on_resize(f);
    }

    /// Called with `true` when the host joins a window's widget tree and `false`
    /// when it leaves it (the `root` / `unroot` virtual functions), whether or not
    /// the window is shown.
    pub fn on_attach(&self, f: impl FnMut(bool) + 'static) {
        self.0.native.imp().events.on_attach(f);
    }

    /// Whether the host is in a window's widget tree now.
    pub fn is_attached(&self) -> bool {
        self.0.native.root().is_some()
    }
}
