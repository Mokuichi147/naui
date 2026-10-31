//! A modal dialog (`AdwAlertDialog`).
//!
//! naui's shape of title, body text, optional widget (`extra_child`) and buttons
//! with roles maps directly onto `AdwAlertDialog`.
//!
//! Buttons cannot be removed once added to an `AdwAlertDialog`, so **the dialog is
//! built each time it is opened**. `Dialog` itself holds only "what to show", such
//! as the title and body text.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use naui_core::{DialogButtons, DialogResponse};

use crate::callback::Notifier;
use crate::widgets::Widget;

/// The mapping between roles and `AdwAlertDialog` response IDs.
const PRIMARY: &str = "primary";
const SECONDARY: &str = "secondary";
const CANCEL: &str = "cancel";

/// The order following the GNOME guidelines: cancel on the left, the primary action
/// on the right.
const ORDER: [DialogResponse; 3] = [
    DialogResponse::Cancel,
    DialogResponse::Secondary,
    DialogResponse::Primary,
];

fn response_id(response: DialogResponse) -> &'static str {
    match response {
        DialogResponse::Primary => PRIMARY,
        DialogResponse::Secondary => SECONDARY,
        DialogResponse::Cancel => CANCEL,
    }
}

fn from_response_id(id: &str) -> DialogResponse {
    match id {
        PRIMARY => DialogResponse::Primary,
        SECONDARY => DialogResponse::Secondary,
        // Esc や外側の操作で閉じたときもここへ来る。
        _ => DialogResponse::Cancel,
    }
}

struct DialogInner {
    app: adw::Application,
    title: RefCell<String>,
    message: RefCell<String>,
    buttons: RefCell<DialogButtons>,
    child: RefCell<Option<Box<dyn Widget>>>,
    on_response: Notifier<DialogResponse>,
    /// The real dialog, held only while it is open.
    current: RefCell<Option<adw::AlertDialog>>,
    open: Cell<bool>,
}

/// A modal dialog.
///
/// It is not a widget, so it does not go into containers (the same as `Window`).
#[derive(Clone)]
pub struct Dialog(Rc<DialogInner>);

impl Dialog {
    pub(crate) fn new(app: &adw::Application, title: &str) -> Self {
        Self(Rc::new(DialogInner {
            app: app.clone(),
            title: RefCell::new(title.to_string()),
            message: RefCell::new(String::new()),
            buttons: RefCell::new(DialogButtons::new()),
            child: RefCell::new(None),
            on_response: Notifier::default(),
            current: RefCell::new(None),
            open: Cell::new(false),
        }))
    }

    pub fn set_title(&self, title: &str) {
        *self.0.title.borrow_mut() = title.to_string();
        if let Some(native) = self.0.current.borrow().as_ref() {
            native.set_heading(Some(title));
        }
    }

    pub fn title(&self) -> String {
        self.0.title.borrow().clone()
    }

    pub fn set_message(&self, message: &str) {
        *self.0.message.borrow_mut() = message.to_string();
        if let Some(native) = self.0.current.borrow().as_ref() {
            native.set_body(message);
        }
    }

    pub fn message(&self) -> String {
        self.0.message.borrow().clone()
    }

    /// The widget placed below the body text.
    pub fn set_child(&self, child: &dyn Widget) {
        *self.0.child.borrow_mut() = Some(child.boxed_clone());
    }

    /// The buttons to show. With none specified, only "OK" is shown.
    pub fn set_buttons(&self, buttons: DialogButtons) {
        *self.0.buttons.borrow_mut() = buttons;
    }

    pub fn buttons(&self) -> DialogButtons {
        self.0.buttons.borrow().clone()
    }

    /// Called with the reason when the dialog closes.
    pub fn on_response(&self, f: impl FnMut(DialogResponse) + 'static) {
        self.0.on_response.set(f);
    }

    /// Opens it. Does nothing if it is already open.
    pub fn open(&self) {
        if self.0.open.get() {
            return;
        }
        let native =
            adw::AlertDialog::new(Some(&self.0.title.borrow()), Some(&self.0.message.borrow()));

        let buttons = self.0.buttons.borrow().resolved();
        for (role, label) in buttons.in_order(&ORDER) {
            let id = response_id(role);
            native.add_response(id, &label);
            if role == DialogResponse::Primary {
                native.set_response_appearance(id, adw::ResponseAppearance::Suggested);
                native.set_default_response(Some(id));
            }
        }
        // Esc や外側を押して閉じたときは、取り消し扱いにする。
        native.set_close_response(CANCEL);

        if let Some(child) = self.0.child.borrow().as_ref() {
            let bin = child.size_bin();
            bin.fill_parent();
            native.set_extra_child(Some(&bin));
        }

        {
            let weak = Rc::downgrade(&self.0);
            native.connect_response(None, move |_, id| {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                // 1 回開くごとに応答は 1 回だけ。閉じたあとに来たものは捨てる。
                if !inner.open.get() {
                    return;
                }
                Dialog(inner.clone()).finish();
                inner.on_response.emit(from_response_id(id));
            });
        }

        self.0.open.set(true);
        *self.0.current.borrow_mut() = Some(native.clone());
        native.present(self.parent().as_ref());
    }

    /// The window the dialog is placed on.
    ///
    /// Sometimes no window has focus yet (right after startup, for example), in which
    /// case the first window the app holds is used.
    fn parent(&self) -> Option<gtk::Window> {
        self.0
            .app
            .active_window()
            .or_else(|| self.0.app.windows().into_iter().next())
    }

    /// Closes it. Does nothing if it is not open.
    pub fn close(&self) {
        let native = self.0.current.borrow().clone();
        let Some(native) = native else {
            return;
        };
        // 閉じられれば `AdwAlertDialog` が取り消しの応答を出すので、
        // 後始末はそちらの経路で済む。
        close_native(&native);
        // ウィンドウが無いなど、ネイティブが閉じられなかったときの受け皿。
        // naui の `close` は「必ず閉じて取り消しを知らせる」ものとして扱う。
        if self.0.open.get() {
            self.finish();
            self.0.on_response.emit(DialogResponse::Cancel);
        }
    }

    pub fn is_open(&self) -> bool {
        self.0.open.get()
    }

    /// The `AdwAlertDialog` that is open now. `None` if none is open.
    ///
    /// Public as a backend-specific escape hatch.
    pub fn native_dialog(&self) -> Option<adw::AlertDialog> {
        self.0.current.borrow().clone()
    }

    /// Cleanup after closing. Keeps the content widget usable the next time it opens.
    fn finish(&self) {
        self.0.open.set(false);
        if let Some(native) = self.0.current.borrow_mut().take() {
            native.set_extra_child(None::<&gtk::Widget>);
        }
    }
}

/// Closes `native`, even before it has opened.
///
/// libadwaita 1.5 opens the sheet inside `AdwDialog` on its second frame after it
/// is mapped, and closing it before then does nothing: the dialog would open
/// afterwards and stay, though naui has already let it go. If `closed` does not
/// come, keep it hidden and out of reach of the pointer, and close it again on each
/// frame until it does. 1.6 and later close it on the first try.
fn close_native(native: &impl IsA<adw::Dialog>) {
    let native = native.as_ref();
    let closed = Rc::new(Cell::new(false));
    native.connect_closed({
        let closed = closed.clone();
        move |_| closed.set(true)
    });
    if !native.close() {
        native.force_close();
    }
    if closed.get() {
        return;
    }
    native.set_opacity(0.0);
    native.set_can_target(false);
    native.add_tick_callback(move |native, _| {
        if !closed.get() {
            native.force_close();
        }
        if closed.get() {
            gtk::glib::ControlFlow::Break
        } else {
            gtk::glib::ControlFlow::Continue
        }
    });
}
