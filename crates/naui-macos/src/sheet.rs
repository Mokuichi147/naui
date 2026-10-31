//! Dialogs with arbitrary content (an AppKit sheet).
//!
//! AppKit's modal for arbitrary content is a window shown as a **sheet**
//! (`beginSheet:completionHandler:`): it slides out of the parent window's title
//! bar, blocks only that window, and has no title bar of its own. naui builds the
//! window's content: the title as a bold label (the same look as `NSAlert`'s
//! heading, since a sheet shows no window title), the content widget (in the same
//! [`HostedChild`] as popovers) and a row of `NSButton`s along the bottom.
//!
//! The buttons follow the macOS layout: the primary action at the right end with
//! the Return key equivalent (which draws it as the default button), cancel to its
//! left with Esc, and the secondary action at the left end. Esc also reaches the
//! window as `cancelOperation:` when no button takes it, which closes the sheet as a
//! cancel.
//!
//! ## Differences from other platforms
//!
//! Unlike `Dialog` (`NSAlert`, app-modal), [`Sheet::open`] returns right away and
//! the response arrives later, as on the other platforms.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use naui_core::{default_response, DialogButtons, DialogResponse, EnabledButtons, SHEET_MIN_WIDTH};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObjectProtocol};
use objc2::{
    define_class, msg_send, sel, ClassType, DefinedClass, MainThreadMarker, MainThreadOnly,
};
use objc2_app_kit::{
    NSApplication, NSBackingStoreType, NSButton, NSFont, NSLayoutConstraint, NSResponder,
    NSStackView, NSStackViewGravity, NSTextField, NSUserInterfaceLayoutOrientation, NSView,
    NSViewController, NSWindow, NSWindowStyleMask,
};
use objc2_foundation::{NSArray, NSObject, NSPoint, NSRect, NSSize, NSString};

use crate::focus::FocusState;
use crate::hosted::HostedChild;
use crate::trampoline::{ActionTarget, ValueHandler};
use crate::widgets::Widget;

/// The margin between the sheet's edge and its contents (the HIG's window margin).
const MARGIN: f64 = 20.0;
/// The space between the title and the content.
const TITLE_SPACING: f64 = 12.0;
/// The space between the buttons.
const BUTTON_SPACING: f64 = 12.0;

/// The Return key.
const RETURN_KEY: &str = "\r";
/// The Esc key.
const ESCAPE_KEY: &str = "\u{1b}";

/// The state of a sheet's window.
pub(crate) struct SheetState {
    focus: FocusState,
    /// Called on `cancelOperation:` (Esc). Called without a borrow, since it closes
    /// the sheet and may open it again.
    cancel: Box<dyn Fn()>,
}

define_class!(
    /// A sheet's window. It tracks focus like naui's other windows (so
    /// `on_focus_change` works on the content), and closes on Esc.
    #[unsafe(super(NSWindow, NSResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "NauiSheetWindow"]
    #[ivars = SheetState]
    pub(crate) struct SheetWindow;

    unsafe impl NSObjectProtocol for SheetWindow {}

    impl SheetWindow {
        #[unsafe(method(makeFirstResponder:))]
        fn make_first_responder(&self, responder: Option<&NSResponder>) -> bool {
            self.ivars().focus.track_move(self, || unsafe {
                msg_send![super(self), makeFirstResponder: responder]
            })
        }

        #[unsafe(method(cancelOperation:))]
        fn cancel_operation(&self, _sender: Option<&AnyObject>) {
            (self.ivars().cancel)();
        }
    }
);

impl SheetWindow {
    fn new(mtm: MainThreadMarker, cancel: impl Fn() + 'static) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(SheetState {
            focus: FocusState::default(),
            cancel: Box::new(cancel),
        });
        unsafe {
            msg_send![
                super(this),
                initWithContentRect: NSRect::new(NSPoint::ZERO, NSSize::new(320.0, 120.0)),
                styleMask: NSWindowStyleMask::Titled,
                backing: NSBackingStoreType::Buffered,
                defer: false,
            ]
        }
    }

    /// The focus state, for `on_focus_change` on the content.
    pub(crate) fn focus_state(&self) -> &FocusState {
        &self.ivars().focus
    }
}

/// A button in the row along the bottom, with the target relaying its clicks.
struct SheetButton {
    role: DialogResponse,
    native: Retained<NSButton>,
    /// `NSButton`'s target is weak, so it is held here.
    _target: Retained<ActionTarget>,
}

struct SheetInner {
    mtm: MainThreadMarker,
    window: Retained<SheetWindow>,
    title: Retained<NSTextField>,
    /// The two places the content can start: below the title, or at the top when
    /// there is no title.
    below_title: Retained<NSLayoutConstraint>,
    at_top: Retained<NSLayoutConstraint>,
    content: HostedChild,
    /// The row of buttons along the bottom.
    row: Retained<NSStackView>,
    button_views: RefCell<Vec<SheetButton>>,
    buttons: RefCell<DialogButtons>,
    enabled: Cell<EnabledButtons>,
    /// The window it is shown on. Held while open, to end the sheet on it.
    parent: RefCell<Option<Retained<NSWindow>>>,
    open: Cell<bool>,
    on_response: ValueHandler<DialogResponse>,
}

/// A modal dialog with arbitrary content (a sheet).
///
/// It is not a widget, so it does not go into containers (the same as `Window`).
/// [`crate::Ui`] holds the sheets it creates, so dropping the return value does not
/// stop notifications.
#[derive(Clone)]
pub struct Sheet(Rc<SheetInner>);

impl Sheet {
    pub(crate) fn new(mtm: MainThreadMarker, title: &str) -> Self {
        let inner = Rc::new_cyclic(|weak: &Weak<SheetInner>| {
            let window = SheetWindow::new(mtm, {
                let weak = weak.clone();
                move || {
                    if let Some(inner) = weak.upgrade() {
                        Sheet(inner).respond(DialogResponse::Cancel);
                    }
                }
            });
            // 閉じても解放させない (Rust 側が持ち続ける)。
            unsafe { window.setReleasedWhenClosed(false) };
            let root = NSView::new(mtm);

            let title_label = NSTextField::labelWithString(&NSString::new(), mtm);
            // `NSAlert` の見出しと同じ太字にする。
            title_label.setFont(Some(
                &NSFont::boldSystemFontOfSize(NSFont::systemFontSize()),
            ));
            let content = HostedChild::new(mtm, 0.0);
            let row = NSStackView::new(mtm);
            row.setOrientation(NSUserInterfaceLayoutOrientation::Horizontal);
            row.setSpacing(BUTTON_SPACING);
            for view in [
                &**title_label as &NSView,
                &**content.container(),
                &**row as &NSView,
            ] {
                view.setTranslatesAutoresizingMaskIntoConstraints(false);
                root.addSubview(view);
            }

            let container = content.container();
            let below_title = container
                .topAnchor()
                .constraintEqualToAnchor_constant(&title_label.bottomAnchor(), TITLE_SPACING);
            let at_top = container
                .topAnchor()
                .constraintEqualToAnchor_constant(&root.topAnchor(), MARGIN);
            // 中身が無いときは高さを持たない。
            let empty_height = container.heightAnchor().constraintEqualToConstant(0.0);
            empty_height.setPriority(1.0);
            let constraints = [
                title_label
                    .topAnchor()
                    .constraintEqualToAnchor_constant(&root.topAnchor(), MARGIN),
                title_label
                    .leadingAnchor()
                    .constraintEqualToAnchor_constant(&root.leadingAnchor(), MARGIN),
                root.trailingAnchor()
                    .constraintGreaterThanOrEqualToAnchor_constant(
                        &title_label.trailingAnchor(),
                        MARGIN,
                    ),
                container
                    .leadingAnchor()
                    .constraintEqualToAnchor_constant(&root.leadingAnchor(), MARGIN),
                root.trailingAnchor()
                    .constraintEqualToAnchor_constant(&container.trailingAnchor(), MARGIN),
                empty_height,
                row.topAnchor()
                    .constraintEqualToAnchor_constant(&container.bottomAnchor(), MARGIN),
                row.leadingAnchor()
                    .constraintEqualToAnchor_constant(&root.leadingAnchor(), MARGIN),
                root.trailingAnchor()
                    .constraintEqualToAnchor_constant(&row.trailingAnchor(), MARGIN),
                root.bottomAnchor()
                    .constraintEqualToAnchor_constant(&row.bottomAnchor(), MARGIN),
                root.widthAnchor()
                    .constraintGreaterThanOrEqualToConstant(SHEET_MIN_WIDTH),
            ];
            NSLayoutConstraint::activateConstraints(&NSArray::from_retained_slice(&constraints));

            // ウィンドウの大きさを中身の制約に従わせる (中身が伸びればシートも伸びる)。
            let controller = NSViewController::new(mtm);
            controller.setView(&root);
            window.setContentViewController(Some(&controller));

            SheetInner {
                mtm,
                window,
                title: title_label,
                below_title,
                at_top,
                content,
                row,
                button_views: RefCell::new(Vec::new()),
                buttons: RefCell::new(DialogButtons::new()),
                enabled: Cell::new(EnabledButtons::default()),
                parent: RefCell::new(None),
                open: Cell::new(false),
                on_response: ValueHandler::default(),
            }
        });
        let this = Self(inner);
        this.set_title(title);
        this.rebuild_buttons();
        this
    }

    /// The heading, shown in bold at the top. An empty string shows none.
    pub fn set_title(&self, title: &str) {
        let text = NSString::from_str(title);
        self.0.title.setStringValue(&text);
        // シートにはタイトルバーが出ないが、読み上げのために窓の名前にも入れる。
        self.0.window.setTitle(&text);
        let shown = !title.is_empty();
        self.0.title.setHidden(!shown);
        // 外す側を先に下ろして、両方が効く瞬間を作らない。
        if shown {
            self.0.at_top.setActive(false);
            self.0.below_title.setActive(true);
        } else {
            self.0.below_title.setActive(false);
            self.0.at_top.setActive(true);
        }
    }

    pub fn title(&self) -> String {
        self.0.title.stringValue().to_string()
    }

    /// The content. Each call replaces it.
    pub fn set_child(&self, child: &dyn Widget) {
        self.0.content.set(child);
    }

    /// The buttons to show. With none specified, only "OK" is shown.
    pub fn set_buttons(&self, buttons: DialogButtons) {
        *self.0.buttons.borrow_mut() = buttons;
        self.rebuild_buttons();
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
        for button in self.0.button_views.borrow().iter() {
            button.native.setEnabled(state.get(button.role));
        }
    }

    pub fn is_button_enabled(&self, response: DialogResponse) -> bool {
        self.0.enabled.get().get(response)
    }

    /// Lays out the buttons again: the secondary action on the left, cancel and the
    /// primary action on the right.
    fn rebuild_buttons(&self) {
        let row = &self.0.row;
        for button in self.0.button_views.borrow_mut().drain(..) {
            row.removeView(&button.native);
        }
        // 窓は既定のボタンを 1 つだけ覚えていて、それがある間は新しく Return を
        // 持たせたボタンからキーを外してしまう。外したボタンのものを忘れさせる。
        self.0.window.setDefaultButtonCell(None);
        let buttons = self.0.buttons.borrow().resolved();
        let default = default_response(&buttons);
        let enabled = self.0.enabled.get();
        let mut views = Vec::new();
        for (role, label) in buttons.in_order(&[
            DialogResponse::Secondary,
            DialogResponse::Cancel,
            DialogResponse::Primary,
        ]) {
            let target = ActionTarget::new(self.0.mtm, {
                let weak = Rc::downgrade(&self.0);
                move || {
                    if let Some(inner) = weak.upgrade() {
                        Sheet(inner).respond(role);
                    }
                }
            });
            let native = unsafe {
                NSButton::buttonWithTitle_target_action(
                    &NSString::from_str(&label),
                    Some(&target),
                    Some(sel!(invoke:)),
                    self.0.mtm,
                )
            };
            // Return を持つボタンは AppKit が既定のボタンとして描く。
            let key = if Some(role) == default {
                RETURN_KEY
            } else if role == DialogResponse::Cancel {
                ESCAPE_KEY
            } else {
                ""
            };
            native.setEnabled(enabled.get(role));
            let gravity = if role == DialogResponse::Secondary {
                NSStackViewGravity::Leading
            } else {
                NSStackViewGravity::Trailing
            };
            row.addView_inGravity(&native, gravity);
            native.setKeyEquivalent(&NSString::from_str(key));
            views.push(SheetButton {
                role,
                native,
                _target: target,
            });
        }
        *self.0.button_views.borrow_mut() = views;
    }

    /// Called with the reason when the user closes it. Setting it again replaces the
    /// previous closure.
    ///
    /// Not called when it is closed with [`Sheet::close`].
    pub fn on_response(&self, f: impl FnMut(DialogResponse) + 'static) {
        self.0.on_response.set(f);
    }

    /// Shows it as a sheet on the frontmost window. It returns right away.
    ///
    /// Does nothing if it is already open, or while no window is shown.
    pub fn open(&self) {
        if self.0.open.get() {
            return;
        }
        let Some(parent) = parent_window(self.0.mtm) else {
            return;
        };
        self.0.content.apply_fallback_width();
        self.0.open.set(true);
        *self.0.parent.borrow_mut() = Some(parent.clone());
        // 閉じたことは自分で知っているので、完了の通知は受けない。
        parent.beginSheet_completionHandler(&self.0.window, None);
    }

    /// Closes it. `on_response` is not called. Does nothing if it is not open.
    pub fn close(&self) {
        if !self.0.open.replace(false) {
            return;
        }
        self.end_sheet();
    }

    /// Closes it and notifies `response`, as a click on that button does. Does
    /// nothing if it is not open.
    fn respond(&self, response: DialogResponse) {
        if !self.0.open.replace(false) {
            return;
        }
        self.end_sheet();
        self.0.on_response.emit(response);
    }

    fn end_sheet(&self) {
        let parent = self.0.parent.borrow_mut().take();
        if let Some(parent) = parent {
            parent.endSheet(&self.0.window);
        }
    }

    /// Whether it is open.
    pub fn is_open(&self) -> bool {
        self.0.open.get()
    }

    /// The sheet's window. Public as a backend-specific escape hatch.
    pub fn native_window(&self) -> Retained<NSWindow> {
        Retained::into_super(self.0.window.clone())
    }
}

/// The window to show a sheet on: the key window (a sheet on a sheet is fine), or
/// failing that the main window, or failing that the first window shown on screen.
///
/// Windows that can never be main, such as a popover's, are skipped unless they
/// are one of naui's sheets.
fn parent_window(mtm: MainThreadMarker) -> Option<Retained<NSWindow>> {
    let app = NSApplication::sharedApplication(mtm);
    let usable = |window: &NSWindow| {
        window.isVisible()
            && (window.canBecomeMainWindow() || window.isKindOfClass(SheetWindow::class()))
    };
    app.keyWindow()
        .filter(|window| usable(window))
        .or_else(|| app.mainWindow().filter(|window| usable(window)))
        .or_else(|| app.windows().iter().find(|window| usable(window)))
}
