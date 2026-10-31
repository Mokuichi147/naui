//! Dialogs with arbitrary content (`AdwDialog`).
//!
//! libadwaita's dialog for arbitrary content is `AdwDialog`, which has no buttons of
//! its own. naui lays it out the way the GNOME guidelines describe: an
//! `AdwToolbarView` with an `AdwHeaderBar` on top showing the title, cancel at the
//! start of the header bar, the primary action (`.suggested-action`) at the end and
//! the secondary action beside it, and the content below with a margin.
//!
//! The header bar's close button is shown only when there is no cancel button, as
//! the way to close it with the pointer. Esc closes it as a cancel either way
//! (`AdwDialog`'s own). It follows its content's size (`follows-content-size`), so
//! content that grows while shown grows the dialog.
//!
//! As with `Dialog`, **the dialog is built each time it is opened**, and `Sheet`
//! itself holds only what to show.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use naui_core::{default_response, DialogButtons, DialogResponse, EnabledButtons, SHEET_MIN_WIDTH};

use crate::callback::Notifier;
use crate::widgets::Widget;

/// The margin around the content.
const MARGIN: i32 = 18;

/// The order the buttons are packed in: cancel at the start, then from the end the
/// primary action and the secondary one to its left.
const ORDER: [DialogResponse; 3] = [
    DialogResponse::Cancel,
    DialogResponse::Primary,
    DialogResponse::Secondary,
];

/// The dialog that is open now, with its parts.
struct Current {
    native: adw::Dialog,
    /// The box the content goes in.
    holder: adw::Bin,
    buttons: Vec<(DialogResponse, gtk::Button)>,
}

struct SheetInner {
    app: adw::Application,
    title: RefCell<String>,
    buttons: RefCell<DialogButtons>,
    enabled: Cell<EnabledButtons>,
    child: RefCell<Option<Box<dyn Widget>>>,
    on_response: Notifier<DialogResponse>,
    current: RefCell<Option<Current>>,
    open: Cell<bool>,
}

/// A modal dialog with arbitrary content (`AdwDialog`).
///
/// It is not a widget, so it does not go into containers (the same as `Window`).
#[derive(Clone)]
pub struct Sheet(Rc<SheetInner>);

impl Sheet {
    pub(crate) fn new(app: &adw::Application, title: &str) -> Self {
        Self(Rc::new(SheetInner {
            app: app.clone(),
            title: RefCell::new(title.to_string()),
            buttons: RefCell::new(DialogButtons::new()),
            enabled: Cell::new(EnabledButtons::default()),
            child: RefCell::new(None),
            on_response: Notifier::default(),
            current: RefCell::new(None),
            open: Cell::new(false),
        }))
    }

    /// The heading, shown in the header bar. An empty string shows none.
    pub fn set_title(&self, title: &str) {
        *self.0.title.borrow_mut() = title.to_string();
        if let Some(current) = self.0.current.borrow().as_ref() {
            current.native.set_title(title);
        }
    }

    pub fn title(&self) -> String {
        self.0.title.borrow().clone()
    }

    /// The content. Each call replaces it.
    pub fn set_child(&self, child: &dyn Widget) {
        let previous = self.0.child.replace(Some(child.boxed_clone()));
        // フォーカスを外すと on_focus_change がその場で呼ばれ、その中でシートを
        // 閉じたり中身を差し替えたりできる。借用を手放してから触る。
        let Some(holder) = self.holder() else {
            return;
        };
        if let Some(previous) = previous {
            crate::interaction::clear_focus(&previous.size_bin());
        }
        // 通知の中で閉じたか開き直したか、別の中身に差し替えていれば、そちらに任せる。
        let still_this = self
            .0
            .child
            .borrow()
            .as_ref()
            .is_some_and(|current| current.size_bin() == child.size_bin());
        if self.holder().as_ref() == Some(&holder) && still_this {
            place_child(&holder, child);
        }
    }

    /// The box the content goes in, of the dialog open now.
    fn holder(&self) -> Option<adw::Bin> {
        self.0
            .current
            .borrow()
            .as_ref()
            .map(|current| current.holder.clone())
    }

    /// The buttons to show. With none specified, only "OK" is shown. Takes effect
    /// the next time it is opened.
    pub fn set_buttons(&self, buttons: DialogButtons) {
        *self.0.buttons.borrow_mut() = buttons;
    }

    pub fn buttons(&self) -> DialogButtons {
        self.0.buttons.borrow().clone()
    }

    /// Enables or disables the button for `response`. The cancel button cannot be
    /// disabled.
    pub fn set_button_enabled(&self, response: DialogResponse, enabled: bool) {
        let mut state = self.0.enabled.get();
        state.set(response, enabled);
        self.0.enabled.set(state);
        // 押せなくしたボタンからはフォーカスが外れ、その通知の中でシートを
        // 閉じられる。借用を手放してから触る。
        let buttons = self
            .0
            .current
            .borrow()
            .as_ref()
            .map(|current| current.buttons.clone())
            .unwrap_or_default();
        for (role, button) in buttons {
            button.set_sensitive(state.get(role));
        }
    }

    pub fn is_button_enabled(&self, response: DialogResponse) -> bool {
        self.0.enabled.get().get(response)
    }

    /// Called with the reason when the user closes it. Setting it again replaces the
    /// previous closure.
    ///
    /// Not called when it is closed with [`Sheet::close`].
    pub fn on_response(&self, f: impl FnMut(DialogResponse) + 'static) {
        self.0.on_response.set(f);
    }

    /// Shows it over the active window. It returns right away.
    ///
    /// Does nothing if it is already open, or while no window is shown.
    pub fn open(&self) {
        if self.0.open.get() {
            return;
        }
        let Some(parent) = crate::dialog::parent_window(&self.0.app) else {
            return;
        };
        let current = self.build();
        let native = current.native.clone();
        self.0.open.set(true);
        *self.0.current.borrow_mut() = Some(current);
        native.present(Some(&parent));
    }

    fn build(&self) -> Current {
        let native = adw::Dialog::new();
        native.set_title(&self.0.title.borrow());
        // 中身が伸びれば (Expander を開くなど) ダイアログも伸びる。
        native.set_follows_content_size(true);

        let header = adw::HeaderBar::new();
        let buttons = self.0.buttons.borrow().resolved();
        // 取り消しがあれば、閉じるボタンはそれと重なるので出さない。
        header.set_show_end_title_buttons(buttons.label(DialogResponse::Cancel).is_none());
        let default = default_response(&buttons);
        let enabled = self.0.enabled.get();
        let mut views = Vec::new();
        for (role, label) in buttons.in_order(&ORDER) {
            let button = gtk::Button::with_label(&label);
            if role == DialogResponse::Primary {
                button.add_css_class("suggested-action");
            }
            button.set_sensitive(enabled.get(role));
            let weak = Rc::downgrade(&self.0);
            button.connect_clicked(move |_| {
                if let Some(inner) = weak.upgrade() {
                    Sheet(inner).respond(role);
                }
            });
            if role == DialogResponse::Cancel {
                header.pack_start(&button);
            } else {
                header.pack_end(&button);
            }
            if Some(role) == default {
                native.set_default_widget(Some(&button));
            }
            views.push((role, button));
        }

        let holder = adw::Bin::new();
        holder.set_margin_start(MARGIN);
        holder.set_margin_end(MARGIN);
        holder.set_margin_top(MARGIN);
        holder.set_margin_bottom(MARGIN);
        // 前のダイアログから外すとフォーカスの通知が走り、その中で中身を
        // 差し替えられる。借用を手放してから触る。
        let child = self
            .0
            .child
            .borrow()
            .as_ref()
            .map(|child| child.boxed_clone());
        if let Some(child) = child {
            place_child(&holder, child.as_ref());
        }
        let toolbar = adw::ToolbarView::new();
        toolbar.set_width_request(SHEET_MIN_WIDTH as i32);
        toolbar.add_top_bar(&header);
        toolbar.set_content(Some(&holder));
        native.set_child(Some(&toolbar));

        let weak = Rc::downgrade(&self.0);
        let closed_holder = holder.clone();
        native.connect_closed(move |native| {
            // 閉じ終えたら中身を手放し、ほかの場所でも使えるようにする
            // (開き直していれば、もう新しいほうへ移っている)。
            closed_holder.set_child(None::<&gtk::Widget>);
            let Some(inner) = weak.upgrade() else {
                return;
            };
            // naui が閉じたもの (ボタン・`close`) は、もう `current` に無い。
            let mine = inner
                .current
                .borrow()
                .as_ref()
                .is_some_and(|current| &current.native == native);
            if mine && inner.open.replace(false) {
                inner.current.borrow_mut().take();
                inner.on_response.emit(DialogResponse::Cancel);
            }
        });

        Current {
            native,
            holder,
            buttons: views,
        }
    }

    /// Closes it. `on_response` is not called. Does nothing if it is not open.
    pub fn close(&self) {
        if !self.0.open.replace(false) {
            return;
        }
        // `force_close` はその場で `closed` を出すので、借用を先に手放す。
        let current = self.0.current.borrow_mut().take();
        if let Some(current) = current {
            current.native.force_close();
        }
    }

    /// Closes it and notifies `response`, as a click on that button does.
    fn respond(&self, response: DialogResponse) {
        if !self.0.open.replace(false) {
            return;
        }
        let current = self.0.current.borrow_mut().take();
        if let Some(current) = current {
            current.native.force_close();
        }
        self.0.on_response.emit(response);
    }

    /// Whether it is open.
    pub fn is_open(&self) -> bool {
        self.0.open.get()
    }

    /// The `AdwDialog` that is open now. `None` if none is open.
    ///
    /// Public as a backend-specific escape hatch.
    pub fn native_dialog(&self) -> Option<adw::Dialog> {
        self.0
            .current
            .borrow()
            .as_ref()
            .map(|current| current.native.clone())
    }
}

/// Puts the content into `holder`, taking it out of the previous dialog first: a
/// dialog closed a moment ago may still hold it while it animates out.
fn place_child(holder: &adw::Bin, child: &dyn Widget) {
    let bin = child.size_bin();
    if let Some(previous) = bin.parent().and_then(|p| p.downcast::<adw::Bin>().ok()) {
        if &previous != holder {
            previous.set_child(None::<&gtk::Widget>);
        }
    }
    bin.fill_parent();
    holder.set_child(Some(&bin));
}
