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
use gtk::glib;
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
    /// Marks the content as this dialog's (see [`crate::handover`]).
    owner: crate::handover::Owner,
    /// The answer [`Dialog::respond`] keeps for the next time it opens.
    pending: Cell<Option<DialogResponse>>,
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
            owner: crate::handover::Owner::new(),
            pending: Cell::new(None),
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
    ///
    /// A widget still in another container is refused, and the previous one stays.
    /// It is placed each time the dialog opens, so if the app puts it in another
    /// container in the meantime, the dialog opens without it and leaves it there.
    pub fn set_child(&self, child: &dyn Widget) {
        let bin = child.size_bin();
        let in_dialog = self
            .0
            .current
            .borrow()
            .as_ref()
            .is_some_and(|native| bin.is_ancestor(native));
        if self.0.owner.claim(&bin, in_dialog, "Dialog::set_child") {
            *self.0.child.borrow_mut() = Some(child.boxed_clone());
        }
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
            if self.0.owner.may_place(&bin) {
                bin.fill_parent();
                native.set_extra_child(Some(&bin));
            }
        }

        {
            let weak = Rc::downgrade(&self.0);
            native.connect_response(None, move |native, id| {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                // 1 回開くごとに応答は 1 回だけ。閉じたあとに来たものは捨てる。
                // 開き直していれば、前のダイアログが遅れて閉じたときの応答も
                // 捨てる (libadwaita 1.5 は開く前の close を後回しにする)。
                let mine = inner.current.borrow().as_ref() == Some(native);
                if !inner.open.get() || !mine {
                    return;
                }
                Dialog(inner.clone()).finish();
                inner.on_response.emit(from_response_id(id));
            });
        }

        self.0.open.set(true);
        *self.0.current.borrow_mut() = Some(native.clone());
        native.present(self.parent().as_ref());
        if let Some(response) = self.0.pending.take() {
            let weak = Rc::downgrade(&self.0);
            when_shown(&native, move || {
                if let Some(inner) = weak.upgrade() {
                    Dialog(inner).respond(response);
                }
            });
        }
    }

    /// Answers it the way the user does, by clicking its button for `response`
    /// (`clicked` on the `AdwAlertDialog`'s button), so `on_response` is called with
    /// it (for tests and automation). Nothing happens if it has no such button.
    ///
    /// If it is not open, the answer is kept and given right after it next opens.
    pub fn respond(&self, response: DialogResponse) {
        let native = self.0.current.borrow().clone();
        let Some(native) = native.filter(|_| self.0.open.get()) else {
            self.0.pending.set(Some(response));
            return;
        };
        // `AdwAlertDialog` は応答のボタンを外へ出さないので、ラベルで探す。
        let buttons = self.0.buttons.borrow().resolved();
        let Some(label) = buttons.label(response) else {
            return;
        };
        if let Some(button) = find_button(native.upcast_ref(), label) {
            if button.is_sensitive() {
                button.emit_clicked();
            }
        }
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

/// The first `GtkButton` labelled `label` in `root`'s widget tree.
pub(crate) fn find_button(root: &gtk::Widget, label: &str) -> Option<gtk::Button> {
    let mut child = root.first_child();
    while let Some(widget) = child {
        if let Some(button) = widget.downcast_ref::<gtk::Button>() {
            if button.label().is_some_and(|text| text == label) {
                return Some(button.clone());
            }
        }
        if let Some(found) = find_button(&widget, label) {
            return Some(found);
        }
        child = widget.next_sibling();
    }
    None
}

/// Calls `f` once `native` is on screen, as a user can click it only once it is
/// shown: after it is mapped and two frames have been drawn, on the main loop.
///
/// libadwaita 1.5 opens the sheet inside `AdwDialog` on its second frame after it
/// is mapped and ignores closing before then (see [`close_native`]), so an answer
/// given earlier could leave it on screen.
pub(crate) fn when_shown(native: &impl IsA<gtk::Widget>, f: impl FnOnce() + 'static) {
    /// The frames to wait for after mapping.
    const FRAMES: u32 = 2;

    let f = Rc::new(RefCell::new(Some(f)));
    let after_frames = move |native: &gtk::Widget| {
        let f = f.clone();
        let frames = Cell::new(0);
        native.add_tick_callback(move |_, _| {
            frames.set(frames.get() + 1);
            if frames.get() < FRAMES {
                return glib::ControlFlow::Continue;
            }
            let f = f.clone();
            glib::idle_add_local_once(move || {
                if let Some(f) = f.borrow_mut().take() {
                    f();
                }
            });
            glib::ControlFlow::Break
        });
    };
    let native = native.as_ref();
    if native.is_mapped() {
        after_frames(native);
        return;
    }
    let handler: Rc<RefCell<Option<glib::SignalHandlerId>>> = Rc::default();
    let id = native.connect_map({
        let handler = handler.clone();
        move |native| {
            if let Some(id) = handler.borrow_mut().take() {
                native.disconnect(id);
            }
            after_frames(native);
        }
    });
    *handler.borrow_mut() = Some(id);
}

/// Closes `native`, even before it has opened.
///
/// libadwaita 1.5 opens the sheet inside `AdwDialog` on its second frame after it
/// is mapped, and closing it before then does nothing: the dialog would open
/// afterwards and stay, though naui has already let it go. If `closed` does not
/// come, keep it hidden and out of reach of the pointer, and close it again on each
/// frame until it does. 1.6 and later close it on the first try.
pub(crate) fn close_native(native: &impl IsA<adw::Dialog>) {
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

/// The shown window to place a sheet on: the active one, or failing that the first
/// shown window the app holds.
pub(crate) fn parent_window(app: &adw::Application) -> Option<gtk::Window> {
    app.active_window()
        .filter(|window| window.is_visible())
        .or_else(|| app.windows().into_iter().find(|window| window.is_visible()))
}
