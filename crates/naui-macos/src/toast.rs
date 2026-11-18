//! Toasts (AppKit).
//!
//! AppKit has no control equivalent to a toast. macOS's Notification Center shows
//! things **outside the app**, which is different from naui's toast ("shown briefly
//! over the screen you are looking at, then goes away by itself"), so it is built by
//! overlaying an `NSVisualEffectView` on the window's content (the same material as
//! `NSPopover`).
//!
//! It is shown in **the frontmost window** at the time it comes up, placed on its
//! `contentView`. An `NSTimer` counts down until it disappears. Which toast is on
//! screen and which wait is decided by [`naui_core::ToastQueue`], shared with the
//! other backends.
//!
//! ## Differences from other platforms
//!
//! On GTK4, `AdwToast` corresponds directly, but on macOS it is a view naui builds
//! as described above. The position (six places along the top and bottom edges,
//! inside the safe area so that a toolbar does not cover it), the row of action
//! buttons and the rounded corners are also decided here.

use std::cell::RefCell;
use std::rc::Rc;

use naui_core::{Align, ToastPosition, ToastPriority, ToastQueue, ToastSlot, ToastSpec};
use objc2::rc::Retained;
use objc2::{sel, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSButton, NSColor, NSLayoutConstraint, NSShadow, NSStackView, NSTextField,
    NSUserInterfaceItemIdentification, NSUserInterfaceLayoutOrientation, NSView,
    NSVisualEffectBlendingMode, NSVisualEffectMaterial, NSVisualEffectState, NSVisualEffectView,
};
use objc2_foundation::{NSArray, NSEdgeInsets, NSPoint, NSRect, NSSize, NSString, NSTimer};

use crate::trampoline::{ActionTarget, SharedSlot};

/// The distance from the top or bottom edge.
const EDGE_MARGIN: f64 = 24.0;
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
/// The spacing between the text and the buttons.
const SPACING: f64 = 12.0;
/// The padding around the content.
const PADDING: NSEdgeInsets = NSEdgeInsets {
    top: 10.0,
    left: 16.0,
    bottom: 10.0,
    right: 16.0,
};

thread_local! {
    /// The toast on screen and the ones waiting their turn.
    static QUEUE: RefCell<ToastQueue<ToastInner>> = const { RefCell::new(ToastQueue::new()) };
}

/// The slot for one closure.
///
/// It is taken out only for the duration of the call so that showing the toast again
/// from inside the notification does not double-borrow the `RefCell` (the same
/// design as `Dialog`).
struct Callback<A>(SharedSlot<dyn FnMut(A)>);

impl<A> Default for Callback<A> {
    fn default() -> Self {
        Self(Rc::default())
    }
}

impl<A> Callback<A> {
    fn set(&self, f: impl FnMut(A) + 'static) {
        *self.0.borrow_mut() = Some(Box::new(f));
    }

    fn emit(&self, arg: A) {
        let Some(mut f) = self.0.borrow_mut().take() else {
            return;
        };
        f(arg);
        let mut slot = self.0.borrow_mut();
        if slot.is_none() {
            *slot = Some(f);
        }
    }
}

/// What is held only while it is on screen.
struct Presentation {
    /// The view overlaid on the window. It casts a shadow, so it is separate from the
    /// `NSVisualEffectView` that rounds the corners and clips the content (doing both in
    /// the same view would clip away the shadow too).
    container: Retained<NSView>,
    /// The row laying out the text and the buttons horizontally.
    row: Retained<NSStackView>,
    /// The text being shown.
    label: Retained<NSTextField>,
    /// The action buttons, in order.
    buttons: Vec<Retained<NSButton>>,
    /// The timer for disappearing automatically. Absent if it never disappears.
    timer: Option<Retained<NSTimer>>,
}

struct ToastInner {
    mtm: MainThreadMarker,
    spec: RefCell<ToastSpec>,
    shown: RefCell<Option<Presentation>>,
    /// The relays for the action buttons (one per index) and the timer. **Kept alive
    /// as long as the toast**, and the button relays only ever grow.
    ///
    /// `NSControl` targets are weak references, so something must hold them. They are
    /// not recreated every time it is shown, nor dropped when the buttons are
    /// replaced, so that dismissing it or changing the buttons from inside a relay (a
    /// button was clicked, or time ran out) **does not free the relay that is
    /// currently running**.
    action_targets: RefCell<Vec<Retained<ActionTarget>>>,
    timer_target: RefCell<Option<Retained<ActionTarget>>>,
    on_action: Callback<usize>,
    on_dismiss: Callback<()>,
}

impl ToastSlot for ToastInner {
    fn present(this: &Rc<Self>) -> bool {
        let Some(host) = host_view(this.mtm) else {
            return false;
        };
        let toast = Toast(this.clone());
        let presentation = toast.build(&host);
        *this.shown.borrow_mut() = Some(presentation);
        true
    }

    fn withdraw(this: &Rc<Self>) {
        let Some(shown) = this.shown.borrow_mut().take() else {
            return;
        };
        if let Some(timer) = shown.timer {
            timer.invalidate();
        }
        shown.container.removeFromSuperview();
    }

    fn is_stranded(this: &Rc<Self>) -> bool {
        let shown = this.shown.borrow();
        let Some(shown) = shown.as_ref() else {
            return false;
        };
        let Some(window) = shown.container.window() else {
            return true;
        };
        // 表示する前の窓にも出せるので、見えていないだけでは決めない。
        // 閉じた窓は見えておらず、次に出す先でもなくなっている。
        !window.isVisible()
            && crate::window::frontmost(this.mtm)
                .is_none_or(|front| !std::ptr::eq(&*front, &*window))
    }
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
            action_targets: RefCell::new(Vec::new()),
            timer_target: RefCell::new(None),
            on_action: Callback::default(),
            on_dismiss: Callback::default(),
        }));
        let for_timer = Rc::downgrade(&this.0);
        *this.0.timer_target.borrow_mut() = Some(ActionTarget::new(mtm, move || {
            if let Some(inner) = for_timer.upgrade() {
                Toast(inner).finish(None);
            }
        }));
        this
    }

    /// The relay for the button at `index`, created on first use.
    fn action_target(&self, index: usize) -> Retained<ActionTarget> {
        let mut targets = self.0.action_targets.borrow_mut();
        while targets.len() <= index {
            let i = targets.len();
            let weak = Rc::downgrade(&self.0);
            targets.push(ActionTarget::new(self.0.mtm, move || {
                if let Some(inner) = weak.upgrade() {
                    Toast(inner).finish(Some(i));
                }
            }));
        }
        targets[index].clone()
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

    /// The action buttons' labels, laid out in this order. **Passing an empty list
    /// removes the buttons.**
    ///
    /// Calling it while shown replaces them on the spot.
    pub fn set_actions<S: AsRef<str>>(&self, labels: &[S]) {
        self.0.spec.borrow_mut().set_actions(labels);
        let labels = self.0.spec.borrow().actions().to_vec();
        if let Some(shown) = self.0.shown.borrow_mut().as_mut() {
            self.apply_actions(shown, &labels);
        }
    }

    /// The action buttons' labels. Empty if there are none.
    pub fn actions(&self) -> Vec<String> {
        self.0.spec.borrow().actions().to_vec()
    }

    /// Seconds until it disappears automatically. **Passing 0 means it never
    /// disappears automatically.**
    ///
    /// Takes effect from the next time it comes up.
    pub fn set_timeout(&self, seconds: f64) {
        self.0.spec.borrow_mut().set_timeout(seconds);
    }

    pub fn timeout(&self) -> f64 {
        self.0.spec.borrow().timeout()
    }

    /// How it takes its turn when shown. Takes effect from the next
    /// [`show`](Self::show).
    pub fn set_priority(&self, priority: ToastPriority) {
        self.0.spec.borrow_mut().set_priority(priority);
    }

    pub fn priority(&self) -> ToastPriority {
        self.0.spec.borrow().priority()
    }

    /// Where in the window it appears. Takes effect from the next time it comes up.
    pub fn set_position(&self, position: ToastPosition) {
        self.0.spec.borrow_mut().set_position(position);
    }

    pub fn position(&self) -> ToastPosition {
        self.0.spec.borrow().position()
    }

    /// The current settings.
    pub fn spec(&self) -> ToastSpec {
        self.0.spec.borrow().clone()
    }

    /// Called with the index of the clicked action button. Setting it again removes
    /// the previous one.
    ///
    /// Clicking it makes the toast disappear, so `on_dismiss` is called next.
    pub fn on_action(&self, f: impl FnMut(usize) + 'static) {
        self.0.on_action.set(f);
    }

    /// Called when it disappears. Setting it again removes the previous one.
    ///
    /// It is called **when it times out** and **when it is closed by an action
    /// button**. It is not called when it is closed with [`dismiss`](Self::dismiss)
    /// or put back in the queue by a `High` toast (the same rule as
    /// [`Dialog::close`](crate::Dialog::close): the app's own actions are not
    /// notified).
    pub fn on_dismiss(&self, mut f: impl FnMut() + 'static) {
        self.0.on_dismiss.set(move |()| f());
    }

    /// Shows the toast: on screen at once, or at the back of the queue (see
    /// [`naui_core::ToastQueue`]).
    pub fn show(&self) {
        let priority = self.0.spec.borrow().priority();
        ToastQueue::show(&QUEUE, &self.0, priority);
    }

    /// Closes the toast, or takes it out of the queue. `on_dismiss` is not called.
    pub fn dismiss(&self) {
        ToastQueue::remove(&QUEUE, &self.0);
    }

    /// Whether it is on screen now.
    pub fn is_visible(&self) -> bool {
        self.0.shown.borrow().is_some()
    }

    /// Whether it is waiting its turn in the queue.
    pub fn is_queued(&self) -> bool {
        ToastQueue::is_waiting_in(&QUEUE, &self.0)
    }

    /// The overlaid view. `None` if it is not on screen.
    ///
    /// Public as a backend-specific escape hatch.
    pub fn native_view(&self) -> Option<Retained<NSView>> {
        self.0
            .shown
            .borrow()
            .as_ref()
            .map(|shown| shown.container.clone())
    }

    /// Replaces the action buttons.
    fn apply_actions(&self, shown: &mut Presentation, labels: &[String]) {
        for button in shown.buttons.drain(..) {
            shown.row.removeArrangedSubview(&button);
            button.removeFromSuperview();
        }
        for (index, label) in labels.iter().enumerate() {
            let button = self.action_button(index, label);
            shown.row.addArrangedSubview(&button);
            shown.buttons.push(button);
        }
    }

    /// A button that dismisses the toast through `on_action(index)` when clicked.
    fn action_button(&self, index: usize, label: &str) -> Retained<NSButton> {
        let button = unsafe {
            NSButton::buttonWithTitle_target_action(
                &NSString::from_str(label),
                None,
                None,
                self.0.mtm,
            )
        };
        let target = self.action_target(index);
        unsafe {
            button.setTarget(Some(&target));
            button.setAction(Some(sel!(invoke:)));
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

        let mut presentation = Presentation {
            container,
            row,
            label,
            buttons: Vec::new(),
            timer: None,
        };
        self.apply_actions(&mut presentation, spec.actions());
        let Presentation { container, row, .. } = &presentation;

        host.addSubview(container);
        // 上端はツールバーの下から数える (全面の中身でも隠れないように)。
        let safe = host.safeAreaLayoutGuide();
        let position = spec.position();
        let vertical = if position.is_top() {
            container
                .topAnchor()
                .constraintEqualToAnchor_constant(&safe.topAnchor(), EDGE_MARGIN)
        } else {
            container
                .bottomAnchor()
                .constraintEqualToAnchor_constant(&safe.bottomAnchor(), -EDGE_MARGIN)
        };
        // 始まり・終わりは leading / trailing なので、右から左の配置では入れ替わる。
        let horizontal = match position.align() {
            Align::Start => container
                .leadingAnchor()
                .constraintEqualToAnchor_constant(&safe.leadingAnchor(), SIDE_MARGIN),
            Align::End => container
                .trailingAnchor()
                .constraintEqualToAnchor_constant(&safe.trailingAnchor(), -SIDE_MARGIN),
            _ => container
                .centerXAnchor()
                .constraintEqualToAnchor(&safe.centerXAnchor()),
        };
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
            vertical,
            horizontal,
            // 文字が長くてもウィンドウからはみ出さない。
            container
                .widthAnchor()
                .constraintLessThanOrEqualToAnchor_constant(
                    &safe.widthAnchor(),
                    -SIDE_MARGIN * 2.0,
                ),
        ]));

        presentation.timer = self.0.timer_target.borrow().as_ref().and_then(|target| {
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
        presentation
    }

    /// Tells the app it disappeared by itself. `action` is the index of the action
    /// button that closed it, if one did.
    ///
    /// The next toast in the queue comes up before the app is told.
    fn finish(&self, action: Option<usize>) {
        if !self.is_visible() {
            return;
        }
        ToastQueue::remove(&QUEUE, &self.0);
        if let Some(index) = action {
            self.0.on_action.emit(index);
        }
        self.0.on_dismiss.emit(());
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
