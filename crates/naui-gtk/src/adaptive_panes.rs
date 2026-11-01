//! A list and its detail, side by side or one at a time (`AdwNavigationSplitView`).
//!
//! libadwaita has the control for this: side by side it shows the sidebar page and
//! the content page, and collapsed it becomes an `AdwNavigationView`, sliding
//! between them. Going back by Escape, Alt+←, the mouse's back button and a swipe
//! is its own, and changes `show-content`, which naui follows.
//!
//! naui decides two things:
//!
//! - **When it collapses.** libadwaita leaves that to an `AdwBreakpoint`, but a
//!   breakpoint needs an `AdwBreakpointBin` (or the window) that takes away the
//!   minimum size of what it holds, the same reason `Stack::set_breakpoint` and
//!   `Sidebar`'s automatic collapsing do not use it. So naui compares the width
//!   after each paint with naui-core's [`PanesArrangement`] and switches on the next
//!   main loop iteration (not while GTK is laying out).
//! - **A back button.** `AdwNavigationSplitView` draws none: apps put an
//!   `AdwHeaderBar` in each page, which shows it. naui's widgets live inside the
//!   window's content rather than its title bar, so a flat button with the same
//!   icon (`go-previous-symbolic`) sits above the detail while only the detail is
//!   shown.
//!
//! The list's width is fixed by setting the sidebar's smallest and largest widths
//! to the same value, in pixels.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use gtk::glib;
use naui_core::{ui_strings, Pane, PanesArrangement, DEFAULT_LIST_PANE_WIDTH};

use crate::bin::SizeBin;
use crate::callback::Notifier;
use crate::widgets::{impl_widget, without_signal, Widget};

/// The margin around the back button.
const BACK_MARGIN: i32 = 6;

struct AdaptivePanesInner {
    native: adw::NavigationSplitView,
    bin: SizeBin,
    /// The containers the app's widgets are put into, inside each page.
    list_pane: SizeBin,
    detail_pane: SizeBin,
    back: gtk::Button,
    /// Holds the panes' handles, keeping them alive along with their callbacks.
    list: RefCell<Option<Box<dyn Widget>>>,
    detail: RefCell<Option<Box<dyn Widget>>>,
    list_width: Cell<f64>,
    /// The breakpoint, the last width, and the pane shown while only one is.
    arrangement: Cell<PanesArrangement>,
    /// Compares the width after each paint.
    after_paint: RefCell<Option<(gtk::gdk::FrameClock, glib::SignalHandlerId)>>,
    /// Watches for being realized, to connect `after_paint` to the frame clock.
    realize_handlers: RefCell<Vec<glib::SignalHandlerId>>,
    /// Applying the collapsed state `arrangement` decided is queued on the main loop.
    switch_scheduled: Cell<bool>,
    /// `notify::show-content`, blocked while naui changes it.
    show_content_handler: RefCell<Option<glib::SignalHandlerId>>,
    on_back: Notifier<()>,
}

impl Drop for AdaptivePanesInner {
    fn drop(&mut self) {
        if let Some((clock, id)) = self.after_paint.get_mut().take() {
            clock.disconnect(id);
        }
        for id in self.realize_handlers.get_mut().drain(..) {
            self.native.disconnect(id);
        }
    }
}

/// A list and its detail, side by side when wide and one at a time when narrow.
#[derive(Clone)]
pub struct AdaptivePanes(Rc<AdaptivePanesInner>);
impl_widget!(AdaptivePanes);

impl AdaptivePanes {
    pub(crate) fn new() -> Self {
        let native = adw::NavigationSplitView::new();
        use_pixels(&native);
        native.set_min_sidebar_width(DEFAULT_LIST_PANE_WIDTH);
        native.set_max_sidebar_width(DEFAULT_LIST_PANE_WIDTH);

        let list_pane = pane_container();
        let detail_pane = pane_container();

        let back = gtk::Button::new();
        let content = adw::ButtonContent::new();
        content.set_icon_name("go-previous-symbolic");
        content.set_label(ui_strings().back);
        back.set_child(Some(&content));
        back.add_css_class("flat");
        back.set_halign(gtk::Align::Start);
        back.set_margin_start(BACK_MARGIN);
        back.set_margin_top(BACK_MARGIN);
        back.set_margin_bottom(BACK_MARGIN);
        back.set_visible(false);

        let detail_side = gtk::Box::new(gtk::Orientation::Vertical, 0);
        detail_side.append(&back);
        detail_side.append(&detail_pane);

        // 見出しは画面に出ない (見出し帯を置かないため) が、ページの名前として
        // 支援技術へ渡る。libadwaita は見出しの無いページを警告する。
        let strings = ui_strings();
        let list_page = adw::NavigationPage::new(&list_pane, strings.list_pane);
        let detail_page = adw::NavigationPage::new(&detail_side, strings.detail_pane);
        native.set_sidebar(Some(&list_page));
        native.set_content(Some(&detail_page));

        let bin = SizeBin::wrap(&native);
        let this = Self(Rc::new(AdaptivePanesInner {
            native,
            bin,
            list_pane,
            detail_pane,
            back,
            list: RefCell::new(None),
            detail: RefCell::new(None),
            list_width: Cell::new(DEFAULT_LIST_PANE_WIDTH),
            arrangement: Cell::new(PanesArrangement::default()),
            after_paint: RefCell::new(None),
            realize_handlers: RefCell::new(Vec::new()),
            switch_scheduled: Cell::new(false),
            show_content_handler: RefCell::new(None),
            on_back: Notifier::default(),
        }));

        {
            let weak = Rc::downgrade(&this.0);
            this.0.back.connect_clicked(move |_| {
                if let Some(inner) = weak.upgrade() {
                    AdaptivePanes(inner).go_back();
                }
            });
        }
        // Escape やスワイプなど、libadwaita 自身の「戻る」。
        let id = {
            let weak = Rc::downgrade(&this.0);
            this.0.native.connect_show_content_notify(move |native| {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                if native.shows_content() {
                    return;
                }
                let mut arrangement = inner.arrangement.get();
                if arrangement.show(Pane::List) {
                    inner.arrangement.set(arrangement);
                    inner.apply();
                    inner.on_back.emit(());
                }
            })
        };
        *this.0.show_content_handler.borrow_mut() = Some(id);
        this.watch_width();
        this
    }

    /// The list pane. Each call replaces it.
    pub fn set_list(&self, child: &dyn Widget) {
        put(&self.0.list_pane, &self.0.list, child);
    }

    /// The detail pane. Each call replaces it.
    pub fn set_detail(&self, child: &dyn Widget) {
        put(&self.0.detail_pane, &self.0.detail, child);
    }

    /// The narrowest width at which both panes are shown.
    pub fn set_breakpoint(&self, width: f64) {
        let mut arrangement = self.0.arrangement.get();
        arrangement.resized(f64::from(self.0.native.width()));
        arrangement.set_breakpoint(width);
        self.0.arrangement.set(arrangement);
        self.0.apply();
    }

    pub fn breakpoint(&self) -> f64 {
        self.0.arrangement.get().breakpoint()
    }

    /// The list pane's width while both panes are shown.
    pub fn set_list_width(&self, width: f64) {
        let width = width.max(0.0);
        self.0.list_width.set(width);
        // 最小を先に変えると、新しい最小が古い最大を越えたときに GTK が警告する。
        if width > self.0.native.max_sidebar_width() {
            self.0.native.set_max_sidebar_width(width);
            self.0.native.set_min_sidebar_width(width);
        } else {
            self.0.native.set_min_sidebar_width(width);
            self.0.native.set_max_sidebar_width(width);
        }
    }

    pub fn list_width(&self) -> f64 {
        self.0.list_width.get()
    }

    /// Whether only one pane is shown.
    pub fn is_collapsed(&self) -> bool {
        self.0.arrangement.get().is_collapsed()
    }

    /// The pane shown while only one is.
    pub fn visible_pane(&self) -> Pane {
        self.0.arrangement.get().visible()
    }

    /// Chooses the pane shown while only one is. `on_back` is not called.
    pub fn show(&self, pane: Pane) {
        let mut arrangement = self.0.arrangement.get();
        if arrangement.show(pane) {
            self.0.arrangement.set(arrangement);
            self.0.apply();
        }
    }

    /// Goes back to the list as if the user had pressed the back button. Calls
    /// `on_back`, even when the list is already shown.
    pub fn go_back(&self) {
        self.show(Pane::List);
        self.0.on_back.emit(());
    }

    /// Called when the user goes back from the detail to the list.
    pub fn on_back(&self, mut f: impl FnMut() + 'static) {
        self.0.on_back.set(move |()| f());
    }

    /// The corresponding `AdwNavigationSplitView`. Public as a backend-specific
    /// escape hatch.
    pub fn native_split_view(&self) -> adw::NavigationSplitView {
        self.0.native.clone()
    }

    /// The back button. Public as a backend-specific escape hatch.
    pub fn native_back_button(&self) -> gtk::Button {
        self.0.back.clone()
    }

    /// Starts comparing the width after each paint. Reconnects whenever the split
    /// view is realized again, since the frame clock changes with the window.
    fn watch_width(&self) {
        let weak = Rc::downgrade(&self.0);
        let realize = self.0.native.connect_realize(move |_| {
            if let Some(inner) = weak.upgrade() {
                AdaptivePanes(inner).connect_clock();
            }
        });
        let weak = Rc::downgrade(&self.0);
        let unrealize = self.0.native.connect_unrealize(move |_| {
            if let Some((clock, id)) = weak.upgrade().and_then(|inner| inner.after_paint.take()) {
                clock.disconnect(id);
            }
        });
        self.0
            .realize_handlers
            .borrow_mut()
            .extend([realize, unrealize]);
    }

    /// Connects `check_width` to the frame clock of the window the split view is in.
    fn connect_clock(&self) {
        let Some(clock) = self.0.native.frame_clock() else {
            return;
        };
        let weak = Rc::downgrade(&self.0);
        let handler = clock.connect_after_paint(move |_| {
            if let Some(inner) = weak.upgrade() {
                AdaptivePanes(inner).check_width();
            }
        });
        if let Some((old, id)) = self.0.after_paint.replace(Some((clock, handler))) {
            old.disconnect(id);
        }
        // つないだ直後の幅で決められるよう、描き直しを頼む。
        self.0.native.queue_draw();
    }

    /// Compares the width after a paint. If it crossed the breakpoint, switches on the
    /// next main loop iteration.
    fn check_width(&self) {
        let mut arrangement = self.0.arrangement.get();
        let changed = arrangement.resized(f64::from(self.0.native.width()));
        self.0.arrangement.set(arrangement);
        if !changed || self.0.switch_scheduled.replace(true) {
            return;
        }
        let weak = Rc::downgrade(&self.0);
        glib::idle_add_local_once(move || {
            if let Some(inner) = weak.upgrade() {
                inner.switch_scheduled.set(false);
                // 予約のあとに幅が戻っていれば、いまの並びのままになる。
                inner.apply();
            }
        });
    }
}

impl AdaptivePanesInner {
    /// Makes the split view match `arrangement`.
    fn apply(&self) {
        let arrangement = self.arrangement.get();
        let native = &self.native;
        let show_content = arrangement.visible() == Pane::Detail;
        without_signal(native, &self.show_content_handler, || {
            if native.is_collapsed() != arrangement.is_collapsed() {
                native.set_collapsed(arrangement.is_collapsed());
            }
            if native.shows_content() != show_content {
                native.set_show_content(show_content);
            }
        });
        self.back.set_visible(arrangement.shows_back());
    }
}

/// Makes the sidebar widths logical pixels, as naui's other sizes are.
///
/// From libadwaita 1.7 they default to `sp`, which follows the text scale (and on
/// a display GDK reports at 72 dpi, comes out at three quarters). naui builds
/// against 1.5, so the property is set by name where the running libadwaita has it.
fn use_pixels(native: &adw::NavigationSplitView) {
    const PROPERTY: &str = "sidebar-width-unit";
    let Some(pspec) = native.find_property(PROPERTY) else {
        return;
    };
    // AdwLengthUnit の ADW_LENGTH_UNIT_PX。
    let Some(px) =
        glib::EnumClass::with_type(pspec.value_type()).and_then(|class| class.to_value(0))
    else {
        return;
    };
    native.set_property_from_value(PROPERTY, &px);
}

/// Puts `child` into a pane, replacing what was there.
fn put(pane: &SizeBin, slot: &RefCell<Option<Box<dyn Widget>>>, child: &dyn Widget) {
    while let Some(previous) = pane.first_child() {
        crate::interaction::clear_focus(&previous);
        previous.unparent();
    }
    let bin = child.size_bin();
    // 区画の中身は、配られた場所いっぱいに置く (`SplitView` と同じ扱い)。
    bin.fill_parent();
    bin.set_parent(pane);
    *slot.borrow_mut() = Some(child.boxed_clone());
}

/// The container for one pane, filling the page.
fn pane_container() -> SizeBin {
    let container: SizeBin = glib::Object::new();
    container.set_halign(gtk::Align::Fill);
    container.set_valign(gtk::Align::Fill);
    container.set_hexpand(true);
    container.set_vexpand(true);
    container
}
