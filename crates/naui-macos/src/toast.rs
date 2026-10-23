//! Toasts (AppKit).
//!
//! AppKit has no control equivalent to a toast. macOS's Notification Center shows
//! things **outside the app**, which is different from naui's toast ("shown briefly
//! over the screen you are looking at, then goes away by itself"), so it is built by
//! overlaying an `NSVisualEffectView` on the window's content (the same material as
//! `NSPopover`).
//!
//! It is shown in **the frontmost window**, placed on its `contentView`. An
//! `NSTimer` counts down until it disappears.
//!
//! ## Differences from other platforms
//!
//! On GTK4, `AdwToast` corresponds directly, but on macOS it is a view naui builds
//! as described above. The position (bottom center) and rounded corners are also
//! decided here.

use std::cell::RefCell;
use std::rc::Rc;

use naui_core::ToastSpec;
use objc2::rc::Retained;
use objc2::{sel, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSButton, NSColor, NSLayoutConstraint, NSShadow, NSStackView, NSTextField,
    NSUserInterfaceItemIdentification, NSUserInterfaceLayoutOrientation, NSView,
    NSVisualEffectBlendingMode, NSVisualEffectMaterial, NSVisualEffectState, NSVisualEffectView,
};
use objc2_foundation::{NSArray, NSEdgeInsets, NSPoint, NSRect, NSSize, NSString, NSTimer};

use crate::trampoline::ActionTarget;

/// The distance from the bottom edge.
const BOTTOM_MARGIN: f64 = 24.0;
/// The margin left at the left and right edges. When the window is narrow, the toast
/// shrinks instead.
const SIDE_MARGIN: f64 = 24.0;
/// The corner radius.
const CORNER_RADIUS: f64 = 10.0;
/// The shadow blur. A shadow is cast, as with `NSPopover`, to show that it is
/// layered on top.
const SHADOW_BLUR: f64 = 12.0;
/// The direction of the shadow (downward).
const SHADOW_OFFSET: NSSize = NSSize {
    width: 0.0,
    height: -3.0,
};
/// The name given to the overlaid view. Used to identify the toast from the native
/// side.
const IDENTIFIER: &str = "naui.toast";
/// The spacing between the text and the button.
const SPACING: f64 = 12.0;
/// The padding around the content.
const PADDING: NSEdgeInsets = NSEdgeInsets {
    top: 10.0,
    left: 16.0,
    bottom: 10.0,
    right: 16.0,
};

thread_local! {
    /// The toast showing now. Only one is shown at a time, and a new one replaces it.
    static CURRENT: RefCell<Option<Toast>> = const { RefCell::new(None) };
}

/// The slot for one closure.
///
/// It is taken out only for the duration of the call so that showing the toast again
/// from inside the notification does not double-borrow the `RefCell` (the same
/// design as `Dialog`).
#[derive(Clone, Default)]
struct Callback(Rc<RefCell<Option<Box<dyn FnMut()>>>>);

impl Callback {
    fn set(&self, f: impl FnMut() + 'static) {
        *self.0.borrow_mut() = Some(Box::new(f));
    }

    fn emit(&self) {
        let Some(mut f) = self.0.borrow_mut().take() else {
            return;
        };
        f();
        let mut slot = self.0.borrow_mut();
        if slot.is_none() {
            *slot = Some(f);
        }
    }
}

/// What is held only while it is shown.
struct Presentation {
    /// The view overlaid on the window. It casts a shadow, so it is separate from the
    /// `NSVisualEffectView` that rounds the corners and clips the content (doing both in
    /// the same view would clip away the shadow too).
    container: Retained<NSView>,
    /// The row laying out the text and the button horizontally.
    row: Retained<NSStackView>,
    /// The text being shown.
    label: Retained<NSTextField>,
    /// The action button. Absent if there is none.
    button: Option<Retained<NSButton>>,
    /// The timer for disappearing automatically. Absent if it never disappears.
    timer: Option<Retained<NSTimer>>,
}

struct ToastInner {
    mtm: MainThreadMarker,
    spec: RefCell<ToastSpec>,
    shown: RefCell<Option<Presentation>>,
    /// The relay for the action button and the timer. **Kept alive as long as the
    /// toast.**
    ///
    /// `NSControl` targets are weak references, so something must hold them. They are
    /// not recreated every time it is shown so that dismissing it from inside the relay
    /// (the button was clicked, or time ran out) **does not free the relay that is
    /// currently running**.
    action_target: RefCell<Option<Retained<ActionTarget>>>,
    timer_target: RefCell<Option<Retained<ActionTarget>>>,
    on_action: Callback,
    on_dismiss: Callback,
}

/// A transient notification (an overlaid `NSVisualEffectView`).
///
/// It is not a widget, so it does not go into containers (the same as `Dialog`).
#[derive(Clone)]
pub struct Toast(Rc<ToastInner>);

impl Toast {
    pub(crate) fn new(mtm: MainThreadMarker, message: &str) -> Self {
        let this = Self(Rc::new(ToastInner {
            mtm,
            spec: RefCell::new(ToastSpec::new(message)),
            shown: RefCell::new(None),
            action_target: RefCell::new(None),
            timer_target: RefCell::new(None),
            on_action: Callback::default(),
            on_dismiss: Callback::default(),
        }));
        this.install_targets();
        this
    }

    /// Creates the relays for the action button and the timer. They are reused from then
    /// on.
    fn install_targets(&self) {
        let mtm = self.0.mtm;
        let for_action = Rc::downgrade(&self.0);
        *self.0.action_target.borrow_mut() = Some(ActionTarget::new(mtm, move || {
            if let Some(inner) = for_action.upgrade() {
                Toast(inner).finish(true);
            }
        }));
        let for_timer = Rc::downgrade(&self.0);
        *self.0.timer_target.borrow_mut() = Some(ActionTarget::new(mtm, move || {
            if let Some(inner) = for_timer.upgrade() {
                Toast(inner).finish(false);
            }
        }));
    }

    /// The string to show. Calling it while shown updates it on the spot (the time until
    /// it disappears is not restarted).
    pub fn set_message(&self, message: &str) {
        self.0.spec.borrow_mut().set_message(message);
        if let Some(shown) = self.0.shown.borrow().as_ref() {
            shown.label.setStringValue(&NSString::from_str(message));
        }
    }

    pub fn message(&self) -> String {
        self.0.spec.borrow().message().to_string()
    }

    /// The action button's label. **Passing an empty string removes the button.**
    ///
    /// Calling it while shown adds or removes it on the spot.
    pub fn set_action(&self, label: &str) {
        self.0.spec.borrow_mut().set_action(label);
        let action = self.0.spec.borrow().action().map(str::to_string);
        if let Some(shown) = self.0.shown.borrow_mut().as_mut() {
            self.apply_action(shown, action.as_deref());
        }
    }

    /// The action button's label. The empty string if there is no button.
    pub fn action(&self) -> String {
        self.0
            .spec
            .borrow()
            .action()
            .unwrap_or_default()
            .to_string()
    }

    /// Seconds until it disappears automatically. **Passing 0 means it never
    /// disappears automatically.**
    ///
    /// Takes effect from the next [`show`](Self::show).
    pub fn set_timeout(&self, seconds: f64) {
        self.0.spec.borrow_mut().set_timeout(seconds);
    }

    pub fn timeout(&self) -> f64 {
        self.0.spec.borrow().timeout()
    }

    /// The current settings.
    pub fn spec(&self) -> ToastSpec {
        self.0.spec.borrow().clone()
    }

    /// Called when the action button is clicked. Setting it again removes the previous
    /// one.
    ///
    /// Clicking it makes the toast disappear, so `on_dismiss` is called next.
    pub fn on_action(&self, f: impl FnMut() + 'static) {
        self.0.on_action.set(f);
    }

    /// Called when it disappears. Setting it again removes the previous one.
    ///
    /// It is called **when it times out** and **when it is closed by the action
    /// button**. It is not called when it is closed with [`dismiss`](Self::dismiss) or
    /// replaced by another toast (the same rule as
    /// [`Dialog::close`](crate::Dialog::close): the app's own actions are not
    /// notified).
    pub fn on_dismiss(&self, f: impl FnMut() + 'static) {
        self.0.on_dismiss.set(f);
    }

    /// Shows the toast.
    ///
    /// **Only one is shown at a time**; if another toast is showing, it is replaced
    /// (the replaced one's `on_dismiss` is not called). Does nothing if there is no
    /// window to show it in yet.
    pub fn show(&self) {
        let Some(host) = host_view(self.0.mtm) else {
            return;
        };
        // 自分自身を出し直すときも、いったん片付けてから組み立て直す。
        if let Some(previous) = CURRENT.with(|slot| slot.borrow_mut().take()) {
            previous.take_down();
        }
        let presentation = self.build(&host);
        *self.0.shown.borrow_mut() = Some(presentation);
        CURRENT.with(|slot| *slot.borrow_mut() = Some(self.clone()));
    }

    /// Closes the toast being shown. `on_dismiss` is not called.
    pub fn dismiss(&self) {
        if !self.is_visible() {
            return;
        }
        self.forget_current();
        self.take_down();
    }

    /// Whether it is showing now.
    pub fn is_visible(&self) -> bool {
        self.0.shown.borrow().is_some()
    }

    /// The overlaid view. `None` if it is not shown.
    ///
    /// Public as a backend-specific escape hatch.
    pub fn native_view(&self) -> Option<Retained<NSView>> {
        self.0
            .shown
            .borrow()
            .as_ref()
            .map(|shown| shown.container.clone())
    }

    /// Reattaches the action button. Removes it if `label` is `None`.
    fn apply_action(&self, shown: &mut Presentation, label: Option<&str>) {
        if let Some(button) = shown.button.take() {
            shown.row.removeArrangedSubview(&button);
            button.removeFromSuperview();
        }
        let Some(label) = label else {
            return;
        };
        let button = self.action_button(label);
        shown.row.addArrangedSubview(&button);
        shown.button = Some(button);
    }

    /// A button that dismisses the toast through `on_action` when clicked.
    fn action_button(&self, label: &str) -> Retained<NSButton> {
        let button = unsafe {
            NSButton::buttonWithTitle_target_action(
                &NSString::from_str(label),
                None,
                None,
                self.0.mtm,
            )
        };
        if let Some(target) = self.0.action_target.borrow().as_ref() {
            unsafe {
                button.setTarget(Some(target));
                button.setAction(Some(sel!(invoke:)));
            }
        }
        button
    }

    /// Builds the view, overlays it on `host`, and starts counting down.
    fn build(&self, host: &NSView) -> Presentation {
        let mtm = self.0.mtm;
        let spec = self.0.spec.borrow().clone();
        // 影を落とす外側。角丸で中身を切り取るビューと分けるのは、
        // 切り取りが影まで消してしまうため。
        let container = NSView::new(mtm);
        crate::layout::prepare_child(&container);
        container.setIdentifier(Some(&NSString::from_str(IDENTIFIER)));
        container.setWantsLayer(true);
        let shadow = NSShadow::new();
        shadow.setShadowBlurRadius(SHADOW_BLUR);
        shadow.setShadowOffset(SHADOW_OFFSET);
        let shadow_color = NSColor::shadowColor();
        shadow.setShadowColor(Some(&shadow_color));
        container.setShadow(Some(&shadow));

        // 背景。`NSPopover` と同じ材質にして、OS のテーマへ追従させる。
        let surface = NSVisualEffectView::initWithFrame(
            NSVisualEffectView::alloc(mtm),
            NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(0.0, 0.0)),
        );
        surface.setMaterial(NSVisualEffectMaterial::Popover);
        // ウィンドウの中身の上に重ねるので、背後ではなく手前を混ぜる。
        surface.setBlendingMode(NSVisualEffectBlendingMode::WithinWindow);
        // ウィンドウが背面にあっても薄くならないようにする。
        surface.setState(NSVisualEffectState::Active);
        surface.setWantsLayer(true);
        if let Some(layer) = surface.layer() {
            layer.setCornerRadius(CORNER_RADIUS);
            layer.setMasksToBounds(true);
        }
        crate::layout::prepare_child(&surface);
        container.addSubview(&surface);

        let row = NSStackView::new(mtm);
        // 制約で container につなぐので、frame から作られる制約は外す。
        // 付けたままだと 0 × 0 の frame がそのまま大きさになり、
        // トーストが見えないまま重なる。
        crate::layout::prepare_child(row.as_ref());
        row.setOrientation(NSUserInterfaceLayoutOrientation::Horizontal);
        row.setSpacing(SPACING);
        row.setEdgeInsets(PADDING);
        surface.addSubview(&row);

        let label = NSTextField::labelWithString(&NSString::from_str(spec.message()), mtm);
        row.addArrangedSubview(&label);

        let button = spec.action().map(|action| {
            let button = self.action_button(action);
            row.addArrangedSubview(&button);
            button
        });

        host.addSubview(&container);
        NSLayoutConstraint::activateConstraints(&NSArray::from_retained_slice(&[
            surface
                .leadingAnchor()
                .constraintEqualToAnchor(&container.leadingAnchor()),
            surface
                .trailingAnchor()
                .constraintEqualToAnchor(&container.trailingAnchor()),
            surface
                .topAnchor()
                .constraintEqualToAnchor(&container.topAnchor()),
            surface
                .bottomAnchor()
                .constraintEqualToAnchor(&container.bottomAnchor()),
            row.leadingAnchor()
                .constraintEqualToAnchor(&surface.leadingAnchor()),
            row.trailingAnchor()
                .constraintEqualToAnchor(&surface.trailingAnchor()),
            row.topAnchor()
                .constraintEqualToAnchor(&surface.topAnchor()),
            row.bottomAnchor()
                .constraintEqualToAnchor(&surface.bottomAnchor()),
            container
                .centerXAnchor()
                .constraintEqualToAnchor(&host.centerXAnchor()),
            container
                .bottomAnchor()
                .constraintEqualToAnchor_constant(&host.bottomAnchor(), -BOTTOM_MARGIN),
            // 文字が長くてもウィンドウからはみ出さない。
            container
                .widthAnchor()
                .constraintLessThanOrEqualToAnchor_constant(
                    &host.widthAnchor(),
                    -SIDE_MARGIN * 2.0,
                ),
        ]));

        let timer = self.0.timer_target.borrow().as_ref().and_then(|target| {
            spec.timeout_millis().map(|_| unsafe {
                NSTimer::scheduledTimerWithTimeInterval_target_selector_userInfo_repeats(
                    spec.timeout(),
                    target,
                    sel!(invoke:),
                    None,
                    false,
                )
            })
        });

        Presentation {
            container,
            row,
            label,
            button,
            timer,
        }
    }

    /// Tells the app it disappeared. `action` is whether the action button closed it.
    fn finish(&self, action: bool) {
        if !self.is_visible() {
            return;
        }
        self.forget_current();
        self.take_down();
        if action {
            self.0.on_action.emit();
        }
        self.0.on_dismiss.emit();
    }

    /// Removes the view and stops the timer. No notification.
    fn take_down(&self) {
        let Some(shown) = self.0.shown.borrow_mut().take() else {
            return;
        };
        if let Some(timer) = shown.timer {
            timer.invalidate();
        }
        shown.container.removeFromSuperview();
    }

    /// If "the toast showing now" is this one, vacates that slot.
    fn forget_current(&self) {
        CURRENT.with(|slot| {
            let mine = slot
                .borrow()
                .as_ref()
                .is_some_and(|current| Rc::ptr_eq(&current.0, &self.0));
            if mine {
                slot.borrow_mut().take();
            }
        });
    }
}

/// The view the toast is placed on. `None` if there is no window yet.
///
/// It goes to the frontmost of the windows naui created
/// ([`crate::window::frontmost`]). `NSApplication`'s `windows` is not looked at
/// directly, so as not to grab windows AppKit creates internally.
///
/// In a window with a sidebar, it is overlaid on the right pane (the content side),
/// the same position as Linux's `AdwToastOverlay`, which is on the content side.
fn host_view(mtm: MainThreadMarker) -> Option<Retained<NSView>> {
    crate::window::overlay_host(&*crate::window::frontmost(mtm)?)
}
