//! General-purpose dialogs (AppKit).
//!
//! AppKit's modal with "a title, body text, buttons and an optional view" is
//! **`NSAlert`**, and AppKit handles the layout, the look of the default button and
//! the Esc / Return assignments. naui only copies the settings onto it and maps the
//! clicked button back to a role.
//!
//! The optional widget goes into `NSAlert`'s `accessoryView`.
//!
//! ## Differences from other platforms
//!
//! `NSAlert`'s [`runModal`](NSAlert::runModal) is **app-modal** and does not return
//! until it is closed. So [`Dialog::open`] does not return until it is closed, and
//! `on_response` is called inside `open()` (on the Web / Windows, `open()` returns
//! right away and the notification arrives later). The same property as
//! `FilePicker`.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use naui_core::{DialogButtons, DialogResponse};
use objc2::rc::Retained;
use objc2::MainThreadMarker;
use objc2_app_kit::{
    NSAlert, NSAlertFirstButtonReturn, NSApplication, NSModalResponse, NSModalResponseAbort, NSView,
};
use objc2_foundation::{NSSize, NSString};

use crate::widgets::Widget;

/// The order in which buttons are added to the `NSAlert`.
///
/// `NSAlert` places **earlier buttons further right** and makes the first one the
/// default. Adding them in this order gives the macOS order of "secondary, cancel,
/// primary" from the left.
const BUTTON_ORDER: [DialogResponse; 3] = [
    DialogResponse::Primary,
    DialogResponse::Cancel,
    DialogResponse::Secondary,
];

/// The Esc key. Assigned to the cancel button.
const ESCAPE_KEY: &str = "\u{1b}";

/// The handler notified on close. Shares a single replaceable closure.
///
/// So that usage such as opening the dialog again from inside the notification does
/// not double-borrow, the closure is taken out only for the duration of the call
/// (the same design as `FilePicker`'s notification).
#[derive(Clone, Default)]
struct ResponseHandler(Rc<RefCell<Option<Box<dyn FnMut(DialogResponse)>>>>);

impl ResponseHandler {
    fn set(&self, f: impl FnMut(DialogResponse) + 'static) {
        *self.0.borrow_mut() = Some(Box::new(f));
    }

    fn emit(&self, response: DialogResponse) {
        let Some(mut f) = self.0.borrow_mut().take() else {
            return;
        };
        f(response);
        let mut slot = self.0.borrow_mut();
        if slot.is_none() {
            *slot = Some(f);
        }
    }
}

struct DialogInner {
    mtm: MainThreadMarker,
    title: RefCell<String>,
    message: RefCell<String>,
    /// The content widget. Kept alive along with its trampolines.
    child: RefCell<Option<Box<dyn Widget>>>,
    buttons: RefCell<DialogButtons>,
    on_response: ResponseHandler,
    open: Cell<bool>,
    /// Whether it was closed with [`Dialog::close`]. A marker so that the reason for
    /// closing is not notified.
    closed_by_app: Cell<bool>,
}

/// A modal dialog (NSAlert).
#[derive(Clone)]
pub struct Dialog(Rc<DialogInner>);

impl Dialog {
    pub(crate) fn new(mtm: MainThreadMarker, title: &str) -> Self {
        Self(Rc::new(DialogInner {
            mtm,
            title: RefCell::new(title.to_string()),
            message: RefCell::new(String::new()),
            child: RefCell::new(None),
            buttons: RefCell::new(DialogButtons::new()),
            on_response: ResponseHandler::default(),
            open: Cell::new(false),
            closed_by_app: Cell::new(false),
        }))
    }

    pub fn set_title(&self, title: &str) {
        *self.0.title.borrow_mut() = title.to_string();
    }

    pub fn title(&self) -> String {
        self.0.title.borrow().clone()
    }

    /// The body text shown below the title. Not shown if empty.
    pub fn set_message(&self, message: &str) {
        *self.0.message.borrow_mut() = message.to_string();
    }

    pub fn message(&self) -> String {
        self.0.message.borrow().clone()
    }

    /// The widget placed between the body text and the buttons. Each call replaces it.
    pub fn set_child(&self, child: &dyn Widget) {
        *self.0.child.borrow_mut() = Some(child.boxed_clone());
    }

    /// The buttons to show. By default there are none, in which case only "OK" is
    /// shown.
    pub fn set_buttons(&self, buttons: DialogButtons) {
        *self.0.buttons.borrow_mut() = buttons;
    }

    pub fn buttons(&self) -> DialogButtons {
        self.0.buttons.borrow().clone()
    }

    /// Called with the reason when it closes. Setting it again removes the previous
    /// one.
    ///
    /// Not called when it is closed with [`Dialog::close`].
    pub fn on_response(&self, f: impl FnMut(DialogResponse) + 'static) {
        self.0.on_response.set(f);
    }

    /// Shows the dialog.
    ///
    /// **`NSAlert` is app-modal, so this does not return until it is closed.**
    /// `on_response` is called inside this call. Does nothing if it is already shown.
    pub fn open(&self) {
        if self.0.open.get() {
            return;
        }
        let (alert, roles) = self.build();
        self.0.open.set(true);
        self.0.closed_by_app.set(false);
        let code = alert.runModal();
        self.0.open.set(false);
        if self.0.closed_by_app.replace(false) {
            return; // アプリ側から閉じた。通知はしない。
        }
        let Some(response) = response_for(&roles, code) else {
            return;
        };
        self.0.on_response.emit(response);
    }

    /// Closes the dialog being shown. `on_response` is not called.
    pub fn close(&self) {
        if !self.0.open.get() {
            return;
        }
        self.0.closed_by_app.set(true);
        NSApplication::sharedApplication(self.0.mtm).abortModal();
    }

    /// Whether it is showing now.
    pub fn is_open(&self) -> bool {
        self.0.open.get()
    }

    /// Builds and returns an `NSAlert` reflecting the current settings. **It is not
    /// shown yet.**
    ///
    /// A backend-specific escape hatch. AppKit-specific uses, such as showing it as a
    /// sheet (`beginSheetModalForWindow:completionHandler:`) or changing the icon or
    /// `alertStyle`, start from here.
    pub fn native_alert(&self) -> Retained<NSAlert> {
        self.build().0
    }

    /// Creates the `NSAlert` and the sequence of roles of the added buttons.
    ///
    /// Response codes come back in the order the buttons were added, so that order is
    /// remembered.
    fn build(&self) -> (Retained<NSAlert>, Vec<DialogResponse>) {
        let alert = NSAlert::new(self.0.mtm);
        alert.setMessageText(&NSString::from_str(&self.0.title.borrow()));
        alert.setInformativeText(&NSString::from_str(&self.0.message.borrow()));

        if let Some(child) = self.0.child.borrow().as_deref() {
            let view = child.native_view();
            // `NSAlert` は accessoryView の **frame** を見て場所を空ける。
            // 制約に任せる (translatesAutoresizingMaskIntoConstraints = false)
            // ままだと、frame を入れても潰れてしまうので戻しておく。
            view.setTranslatesAutoresizingMaskIntoConstraints(true);
            view.setFrameSize(accessory_size(&view));
            alert.setAccessoryView(Some(&view));
        }

        let buttons = self.0.buttons.borrow().resolved();
        let mut roles = Vec::new();
        for (role, label) in buttons.in_order(&BUTTON_ORDER) {
            let button = alert.addButtonWithTitle(&NSString::from_str(&label));
            if role == DialogResponse::Cancel {
                // 取り消しは Esc で閉じられるようにする。文字列が
                // "Cancel" のときは AppKit も同じことをするが、
                // 日本語のラベルでも同じにそろえる。
                button.setKeyEquivalent(&NSString::from_str(ESCAPE_KEY));
            }
            roles.push(role);
        }
        (alert, roles)
    }
}

/// The width given to content that cannot decide its own width. About the same as
/// `NSAlert`'s body text area.
const ACCESSORY_WIDTH: f64 = 260.0;
/// The height given to content that cannot decide its own height (one line).
const ACCESSORY_HEIGHT: f64 = 22.0;

/// The size of the frame put into the accessoryView.
///
/// The size from the constraints (`fittingSize`) is used, but widgets with nothing
/// to decide their width (`NSTextField` and the like) return 0. Only then is the
/// default used. A size specified with `set_sizing` takes precedence.
fn accessory_size(view: &NSView) -> NSSize {
    let fitting = view.fittingSize();
    NSSize::new(
        if fitting.width > 0.0 {
            fitting.width
        } else {
            ACCESSORY_WIDTH
        },
        if fitting.height > 0.0 {
            fitting.height
        } else {
            ACCESSORY_HEIGHT
        },
    )
}

/// Maps the return value of `runModal` back to the role of the clicked button.
///
/// `NSAlert` numbers buttons from `NSAlertFirstButtonReturn` in the order they were
/// added. An abort by [`Dialog::close`] (`NSModalResponseAbort`) and out-of-range
/// values give `None`, meaning "not clicked".
fn response_for(roles: &[DialogResponse], code: NSModalResponse) -> Option<DialogResponse> {
    if code == NSModalResponseAbort {
        return None;
    }
    let index = code.checked_sub(NSAlertFirstButtonReturn)?;
    let index = usize::try_from(index).ok()?;
    roles.get(index).copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn response_follows_the_order_buttons_were_added() {
        let roles = [
            DialogResponse::Primary,
            DialogResponse::Cancel,
            DialogResponse::Secondary,
        ];
        assert_eq!(
            response_for(&roles, NSAlertFirstButtonReturn),
            Some(DialogResponse::Primary)
        );
        assert_eq!(
            response_for(&roles, NSAlertFirstButtonReturn + 1),
            Some(DialogResponse::Cancel)
        );
        assert_eq!(
            response_for(&roles, NSAlertFirstButtonReturn + 2),
            Some(DialogResponse::Secondary)
        );
    }

    #[test]
    fn aborting_and_unknown_codes_do_not_map_to_a_button() {
        let roles = [DialogResponse::Cancel];
        assert_eq!(response_for(&roles, NSModalResponseAbort), None);
        assert_eq!(response_for(&roles, NSAlertFirstButtonReturn + 1), None);
        assert_eq!(response_for(&roles, 0), None);
    }

    #[test]
    fn button_order_puts_the_default_button_first() {
        // 先に足したものが右端かつ既定のボタンになる。
        assert_eq!(BUTTON_ORDER[0], DialogResponse::Primary);
    }
}
