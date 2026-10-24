//! Handles wrapping real AppKit controls.
//!
//! Every handle is an `Rc<Inner>`, and `Inner` holds the Retained native object and
//! its trampolines. Cloning a handle still leaves one native object.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use naui_core::{Align, Orientation, Padding, TextColor, TextStyle};
use objc2::rc::{Allocated, Retained};
use objc2::runtime::NSObjectProtocol;
use objc2::{define_class, msg_send, sel, MainThreadMarker, MainThreadOnly, Message};
use objc2_app_kit::{
    NSAutoresizingMaskOptions, NSBorderType, NSButton, NSButtonType, NSColor,
    NSControlStateValueOff, NSControlStateValueOn, NSFont, NSFontTextStyle, NSFontTextStyleBody,
    NSFontTextStyleHeadline, NSFontTextStyleLargeTitle, NSFontTextStyleSubheadline,
    NSFontTextStyleTitle1, NSFontTextStyleTitle3, NSLayoutAttribute, NSLayoutConstraint,
    NSLineBreakMode, NSProgressIndicator, NSProgressIndicatorStyle, NSScrollView, NSSearchField,
    NSSecureTextField, NSSlider, NSStackView, NSStackViewDistribution, NSTextField, NSTextView,
    NSUserInterfaceLayoutOrientation, NSView, NSViewFrameDidChangeNotification,
};
use objc2_foundation::{
    NSArray, NSDictionary, NSEdgeInsets, NSNotificationCenter, NSPoint, NSRect, NSSize, NSString,
};

use crate::trampoline::{ActionTarget, FieldHandlers, FieldObserver, TextViewObserver};

/// The common interface implemented by naui's widgets.
pub trait Widget: 'static {
    /// The corresponding AppKit view. Public as a backend-specific escape hatch.
    fn native_view(&self) -> Retained<NSView>;

    #[doc(hidden)]
    fn boxed_clone(&self) -> Box<dyn Widget>;

    /// Called with the direction (`true` for vertical) when put into a `Stack`.
    ///
    /// Used only by nested `Stack`s. To settle that the outer one absorbs leftover
    /// space, a stack stiffens its own tail spacer when the direction is the same.
    /// Other widgets do nothing.
    #[doc(hidden)]
    fn nested_in_stack(&self, _vertical: bool) {}
}

macro_rules! impl_widget {
    ($t:ty) => {
        impl Widget for $t {
            fn native_view(&self) -> Retained<NSView> {
                let view: &NSView = self.0.native.as_ref();
                view.retain()
            }
            fn boxed_clone(&self) -> Box<dyn Widget> {
                Box::new(self.clone())
            }
        }

        crate::widgets::impl_sizing!($t);
    };
}

/// Operations every widget has (show/hide / focus / tooltip).
macro_rules! impl_interaction {
    ($t:ty) => {
        impl $t {
            /// Whether it is shown. Shown by default.
            ///
            /// A hidden widget takes no space in a `Stack`. In a `Grid`, its cell remains.
            pub fn set_visible(&self, visible: bool) {
                crate::interaction::set_visible(&<$t as Widget>::native_view(self), visible);
            }

            /// The visibility set with [`set_visible`](Self::set_visible). Stays `true` even
            /// if a parent is hidden.
            pub fn is_visible(&self) -> bool {
                crate::interaction::is_visible(&<$t as Widget>::native_view(self))
            }

            /// Moves keyboard focus here. `true` if it moved.
            ///
            /// If the widget itself cannot take focus (containers, for example), focus moves to
            /// the first thing inside that can. `false` before it is shown, while hidden, or
            /// while disabled.
            pub fn request_focus(&self) -> bool {
                crate::interaction::request_focus(&<$t as Widget>::native_view(self))
            }

            /// The description shown when the pointer hovers over it. `None` removes it.
            pub fn set_tooltip(&self, text: Option<&str>) {
                crate::interaction::set_tooltip(&<$t as Widget>::native_view(self), text);
            }

            /// The name reported to screen readers. `None` removes it (the visible text is
            /// used).
            ///
            /// Set it when there is no visible text, or the text does not fully convey the
            /// meaning, as with icon-only buttons.
            pub fn set_accessible_label(&self, text: Option<&str>) {
                crate::interaction::set_accessible_label(&<$t as Widget>::native_view(self), text);
            }

            /// Creates a weak reference. Capture it when touching this widget from its own
            /// notification closures (capturing a `clone()` creates a cycle and it is never
            /// freed).
            pub fn downgrade(&self) -> naui_core::WeakWidget<Self> {
                let weak = std::rc::Rc::downgrade(&self.0);
                naui_core::WeakWidget::new(move || weak.upgrade().map(Self))
            }
        }
    };
}

/// Adds only size specification to types that implement `Widget` by hand.
macro_rules! impl_sizing {
    ($t:ty) => {
        crate::widgets::impl_interaction!($t);

        impl $t {
            /// Sets the size. Each call discards the previous setting.
            ///
            /// The actual size is decided by Auto Layout, so what you pass here is only
            /// constraints such as "fix it" or "take the parent's leftover space".
            ///
            /// Call it **before** putting the widget into a container when using `Fill` on the
            /// cross axis or growing within a grid cell. AppKit places constraints and cell
            /// placement at the time of `append` / `attach`, so later changes are not applied.
            pub fn set_sizing(&self, sizing: naui_core::Sizing) {
                let view = <$t as Widget>::native_view(self);
                crate::layout::apply_sizing(&view, sizing);
            }
        }
    };
}

pub(crate) use {impl_interaction, impl_sizing, impl_widget};

// ------------------------------------------------------------------ Label

struct LabelInner {
    native: Retained<NSTextField>,
    mtm: MainThreadMarker,
    /// Whether it wraps.
    wraps: Cell<bool>,
    /// The relay that makes the wrapping width follow the frame. `None` while not
    /// wrapping.
    ///
    /// The relay is held with the same lifetime as the handle (the same rule as
    /// `Toast`).
    width_target: RefCell<Option<Retained<ActionTarget>>>,
}

/// Non-editable text (an NSTextField in label configuration).
#[derive(Clone)]
pub struct Label(Rc<LabelInner>);
impl_widget!(Label);

impl Label {
    pub(crate) fn new(mtm: MainThreadMarker, text: &str) -> Self {
        let native = NSTextField::labelWithString(&NSString::from_str(text), mtm);
        let this = Self(Rc::new(LabelInner {
            native,
            mtm,
            wraps: Cell::new(false),
            width_target: RefCell::new(None),
        }));
        this.set_wrap(false);
        this
    }

    pub fn text(&self) -> String {
        self.0.native.stringValue().to_string()
    }

    pub fn set_text(&self, text: &str) {
        self.0.native.setStringValue(&NSString::from_str(text));
        // 文字が変われば要る大きさも変わる (折り返していれば行数ごと)。
        // 親の連なりへ伝えないと、`Grid` の `Auto` 行が前の高さのまま残る。
        crate::layout::invalidate_ancestors(&self.0.native);
    }

    /// Whether the text can be selected and copied. Not selectable by default.
    ///
    /// Use it for text readers will want to copy, such as chat messages or error
    /// details. Unlike a text field, the text cannot be edited.
    pub fn set_selectable(&self, selectable: bool) {
        self.0.native.setSelectable(selectable);
    }

    pub fn is_selectable(&self) -> bool {
        self.0.native.isSelectable()
    }

    /// Whether long text wraps. Does not wrap by default.
    ///
    /// When not wrapping, it stays on one line and whatever does not fit is cut off
    /// with a trailing ellipsis (…) (`ByTruncatingTail` of `NSLineBreakMode`).
    ///
    /// The parent decides the wrapping width, so inside a `Stack` give it a width with
    /// [`set_sizing`](Self::set_sizing) (`Sizing::fill_width()` and the like).
    /// `NSTextField` can return a height only once its own width is decided, so naui
    /// watches frame changes and keeps `preferredMaxLayoutWidth` in step.
    pub fn set_wrap(&self, wrap: bool) {
        self.0.wraps.set(wrap);
        self.0.native.setUsesSingleLineMode(!wrap);
        self.0.native.setLineBreakMode(if wrap {
            NSLineBreakMode::ByWordWrapping
        } else {
            NSLineBreakMode::ByTruncatingTail
        });
        self.0
            .native
            .setMaximumNumberOfLines(if wrap { 0 } else { 1 });

        let center = NSNotificationCenter::defaultCenter();
        if let Some(previous) = self.0.width_target.borrow_mut().take() {
            unsafe {
                center.removeObserver_name_object(
                    &previous,
                    Some(NSViewFrameDidChangeNotification),
                    Some(&self.0.native),
                );
            }
        }
        if !wrap {
            self.0.native.setPreferredMaxLayoutWidth(0.0);
            return;
        }

        self.0.sync_preferred_width();
        self.0.native.setPostsFrameChangedNotifications(true);
        let target = ActionTarget::new(self.0.mtm, {
            let weak = Rc::downgrade(&self.0);
            move || {
                if let Some(inner) = weak.upgrade() {
                    inner.sync_preferred_width();
                }
            }
        });
        unsafe {
            center.addObserver_selector_name_object(
                &target,
                sel!(invoke:),
                Some(NSViewFrameDidChangeNotification),
                Some(&self.0.native),
            );
        }
        *self.0.width_target.borrow_mut() = Some(target);
    }

    /// The level of text size and weight. The default is [`TextStyle::Body`].
    ///
    /// The fonts AppKit provides for each level are used as is
    /// (`preferredFontForTextStyle:`). The OS decides the point sizes, so they follow
    /// the system's text size setting as is.
    pub fn set_style(&self, style: TextStyle) {
        self.0.native.setFont(Some(&preferred_font(style)));
        // フォントが変われば行の高さも変わる。折り返し中なら、いまの幅で
        // 測り直させる (frame が動かないときは通知が来ないため)。
        self.0.sync_preferred_width();
    }

    /// The text color role. The default is [`TextColor::Default`].
    ///
    /// AppKit's semantic colors (`labelColor` and so on) are used as is, so they follow
    /// light / dark switching and the accent color setting.
    pub fn set_color(&self, color: TextColor) {
        self.0.native.setTextColor(Some(&label_color(color)));
    }
}

/// The AppKit font for the level.
///
/// `NSFontTextStyle` was introduced in macOS 11, and the system decides the point
/// size and weight of each.
///
/// `Caption1` is not used for `Caption` because on macOS it is 10pt, only 0.77 times
/// the body text (13pt), which is hard to read as supplementary text.
/// `Subheadline` (11pt) is 0.85 times, matching the ratios on Windows (12 / 14px)
/// and the Web (`0.85em`).
fn preferred_font(style: TextStyle) -> Retained<NSFont> {
    // SAFETY: 定数はどれも AppKit が持つ `NSFontTextStyle`。options は
    // 型どおりの空辞書 (指定できるのは表示サイズの上限だけで、既定に任せる)。
    unsafe {
        let name: &NSFontTextStyle = match style {
            TextStyle::LargeTitle => NSFontTextStyleLargeTitle,
            TextStyle::Title => NSFontTextStyleTitle1,
            TextStyle::Subtitle => NSFontTextStyleTitle3,
            TextStyle::Heading => NSFontTextStyleHeadline,
            TextStyle::Body => NSFontTextStyleBody,
            TextStyle::Caption => NSFontTextStyleSubheadline,
        };
        NSFont::preferredFontForTextStyle_options(name, &NSDictionary::new())
    }
}

/// The AppKit color for the role.
fn label_color(color: TextColor) -> Retained<NSColor> {
    match color {
        TextColor::Default => NSColor::labelColor(),
        TextColor::Secondary => NSColor::secondaryLabelColor(),
        TextColor::Accent => NSColor::controlAccentColor(),
        TextColor::Success => NSColor::systemGreenColor(),
        TextColor::Warning => NSColor::systemOrangeColor(),
        TextColor::Danger => NSColor::systemRedColor(),
    }
}

impl LabelInner {
    /// Matches the wrapping width to the current frame's width.
    ///
    /// It is written only when the value changes. Writing `preferredMaxLayoutWidth`
    /// invalidates the intrinsic size and triggers the next layout, so writing it every
    /// time would loop forever.
    fn sync_preferred_width(&self) {
        if !self.wraps.get() {
            return;
        }
        let width = self.native.frame().size.width;
        if width <= 0.0 {
            return;
        }
        if (self.native.preferredMaxLayoutWidth() - width).abs() < 0.5 {
            return;
        }
        self.native.setPreferredMaxLayoutWidth(width);
    }
}

// ----------------------------------------------------------------- Button

struct ButtonInner {
    native: Retained<NSButton>,
    /// The object holding the closure for clicks.
    target: RefCell<Option<Retained<ActionTarget>>>,
}

/// A push button (NSButton).
#[derive(Clone)]
pub struct Button(Rc<ButtonInner>);
impl_widget!(Button);

impl Button {
    pub(crate) fn new(mtm: MainThreadMarker, text: &str) -> Self {
        let native = unsafe {
            NSButton::buttonWithTitle_target_action(&NSString::from_str(text), None, None, mtm)
        };
        Self(Rc::new(ButtonInner {
            native,
            target: RefCell::new(None),
        }))
    }

    pub fn set_text(&self, text: &str) {
        self.0.native.setTitle(&NSString::from_str(text));
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.0.native.setEnabled(enabled);
    }

    /// Called on click. Setting it again removes the previous one.
    pub fn on_click(&self, f: impl FnMut() + 'static) {
        let mtm = MainThreadMarker::from(&*self.0.native);
        let target = ActionTarget::new(mtm, f);
        unsafe {
            self.0.native.setTarget(Some(&target));
            self.0.native.setAction(Some(sel!(invoke:)));
        }
        *self.0.target.borrow_mut() = Some(target);
    }

    /// Triggers a click (for tests and automation).
    pub fn click(&self) {
        unsafe { self.0.native.performClick(None) };
    }
}

// --------------------------------------------------------------- Checkbox

struct CheckboxInner {
    native: Retained<NSButton>,
    target: RefCell<Option<Retained<ActionTarget>>>,
}

/// A check box (an NSButton of the Switch type).
#[derive(Clone)]
pub struct Checkbox(Rc<CheckboxInner>);
impl_widget!(Checkbox);

impl Checkbox {
    pub(crate) fn new(mtm: MainThreadMarker, label: &str) -> Self {
        let native = unsafe {
            NSButton::checkboxWithTitle_target_action(&NSString::from_str(label), None, None, mtm)
        };
        native.setButtonType(NSButtonType::Switch);
        Self(Rc::new(CheckboxInner {
            native,
            target: RefCell::new(None),
        }))
    }

    pub fn is_checked(&self) -> bool {
        let state = self.0.native.state();
        state != NSControlStateValueOff
    }

    pub fn set_checked(&self, checked: bool) {
        let state = if checked {
            NSControlStateValueOn
        } else {
            NSControlStateValueOff
        };
        self.0.native.setState(state);
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.0.native.setEnabled(enabled);
    }

    /// Called with the new value when the state changes.
    pub fn on_toggle(&self, mut f: impl FnMut(bool) + 'static) {
        let mtm = MainThreadMarker::from(&*self.0.native);
        let native = self.0.native.clone();
        let target = ActionTarget::new(mtm, move || {
            let state = native.state();
            f(state != NSControlStateValueOff);
        });
        unsafe {
            self.0.native.setTarget(Some(&target));
            self.0.native.setAction(Some(sel!(invoke:)));
        }
        *self.0.target.borrow_mut() = Some(target);
    }

    /// Triggers a click (for tests and automation).
    pub fn click(&self) {
        unsafe { self.0.native.performClick(None) };
    }
}

// -------------------------------------------------------------- TextInput

struct TextInputInner {
    native: Retained<NSTextField>,
    handlers: Rc<FieldHandlers>,
    // NSTextField の delegate は weak なので、こちら側で生かしておく。
    _observer: Retained<FieldObserver>,
}

/// Single-line text input (NSTextField). Japanese input works through AppKit's IME
/// as is.
#[derive(Clone)]
pub struct TextInput(Rc<TextInputInner>);
crate::keys::impl_key_down!(TextInput);
crate::focus::impl_focus_change!(TextInput);
impl_widget!(TextInput);

impl TextInput {
    pub(crate) fn new(mtm: MainThreadMarker, text: &str) -> Self {
        let native = NSTextField::textFieldWithString(&NSString::from_str(text), mtm);
        let (handlers, observer) = observe_field(mtm, &native);
        Self(Rc::new(TextInputInner {
            native,
            handlers,
            _observer: observer,
        }))
    }

    pub fn text(&self) -> String {
        self.0.native.stringValue().to_string()
    }

    pub fn set_text(&self, text: &str) {
        self.0.native.setStringValue(&NSString::from_str(text));
    }

    pub fn set_placeholder(&self, text: &str) {
        self.0
            .native
            .setPlaceholderString(Some(&NSString::from_str(text)));
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.0.native.setEnabled(enabled);
    }

    /// Called with the current text every time a character is typed.
    pub fn on_change(&self, f: impl FnMut(&str) + 'static) {
        self.0.handlers.set_change(f);
    }

    /// Called with the current text when the user confirms with Enter.
    ///
    /// Not called for an Enter pressed while an IME is composing (which confirms the
    /// conversion). Not called while read-only either.
    pub fn on_submit(&self, mut f: impl FnMut(&str) + 'static) {
        let native = self.0.native.clone();
        self.0.handlers.set_submit(move |text| {
            if native.isEditable() {
                f(text);
            }
        });
    }

    /// Whether it is read-only. Editable by default.
    ///
    /// While read-only, the text can still be selected and copied, and it still takes
    /// focus (unlike `set_enabled(false)`, it is not dimmed). Use it for fields that
    /// show logs or generated output.
    pub fn set_read_only(&self, read_only: bool) {
        self.0.native.setEditable(!read_only);
        self.0.native.setSelectable(true);
    }

    pub fn is_read_only(&self) -> bool {
        !self.0.native.isEditable()
    }
}

// ----------------------------------------------------------- PasswordInput

struct PasswordInputInner {
    native: Retained<NSSecureTextField>,
    handlers: Rc<FieldHandlers>,
    // NSSecureTextField の delegate は weak なので、こちら側で生かしておく。
    _observer: Retained<FieldObserver>,
}

/// Password input (`NSSecureTextField`).
///
/// The API is the same as [`TextInput`]; the only difference is that **typed
/// characters are masked**. AppKit has no mechanism to temporarily reveal the text,
/// so there is none.
#[derive(Clone)]
pub struct PasswordInput(Rc<PasswordInputInner>);
crate::keys::impl_key_down!(PasswordInput);
crate::focus::impl_focus_change!(PasswordInput);
impl_widget!(PasswordInput);

impl PasswordInput {
    pub(crate) fn new(mtm: MainThreadMarker) -> Self {
        let native = NSSecureTextField::new(mtm);
        let (handlers, observer) = observe_field(mtm, &native);
        Self(Rc::new(PasswordInputInner {
            native,
            handlers,
            _observer: observer,
        }))
    }

    /// The text currently entered.
    pub fn text(&self) -> String {
        self.0.native.stringValue().to_string()
    }

    /// Replaces the text. `on_change` is not called.
    pub fn set_text(&self, text: &str) {
        self.0.native.setStringValue(&NSString::from_str(text));
    }

    pub fn set_placeholder(&self, text: &str) {
        self.0
            .native
            .setPlaceholderString(Some(&NSString::from_str(text)));
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.0.native.setEnabled(enabled);
    }

    /// Called with the current text every time a character is typed.
    pub fn on_change(&self, f: impl FnMut(&str) + 'static) {
        self.0.handlers.set_change(f);
    }

    /// Called with the current text when the user confirms with Enter.
    pub fn on_submit(&self, f: impl FnMut(&str) + 'static) {
        self.0.handlers.set_submit(f);
    }
}

// ------------------------------------------------------------- SearchInput

struct SearchInputInner {
    native: Retained<NSSearchField>,
    handlers: Rc<FieldHandlers>,
    // NSSearchField の delegate は weak なので、こちら側で生かしておく。
    _observer: Retained<FieldObserver>,
}

/// A search field (`NSSearchField`).
///
/// The magnifying glass and the clear button (✕) that appears once you start
/// typing are shown by AppKit. [`on_change`](SearchInput::on_change) is called on
/// every keystroke, and [`on_search`](SearchInput::on_search) when the user
/// confirms with Enter.
#[derive(Clone)]
pub struct SearchInput(Rc<SearchInputInner>);
crate::keys::impl_key_down!(SearchInput);
crate::focus::impl_focus_change!(SearchInput);
impl_widget!(SearchInput);

impl SearchInput {
    pub(crate) fn new(mtm: MainThreadMarker) -> Self {
        let native = NSSearchField::new(mtm);
        let (handlers, observer) = observe_field(mtm, &native);
        Self(Rc::new(SearchInputInner {
            native,
            handlers,
            _observer: observer,
        }))
    }

    /// The text currently entered.
    pub fn text(&self) -> String {
        self.0.native.stringValue().to_string()
    }

    /// Replaces the text. `on_change` is not called.
    pub fn set_text(&self, text: &str) {
        self.0.native.setStringValue(&NSString::from_str(text));
    }

    pub fn set_placeholder(&self, text: &str) {
        self.0
            .native
            .setPlaceholderString(Some(&NSString::from_str(text)));
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.0.native.setEnabled(enabled);
    }

    /// Called with the current text every time a character is typed.
    pub fn on_change(&self, f: impl FnMut(&str) + 'static) {
        self.0.handlers.set_change(f);
    }

    /// Called with the current text when the user confirms with Enter.
    pub fn on_search(&self, f: impl FnMut(&str) + 'static) {
        self.0.handlers.set_submit(f);
    }
}

/// Sets a delegate on a single-line field.
///
/// It is shared by the two notifications for typing and confirming, so it is set
/// exactly once at creation. AppKit delegates are weak, so the caller holds the
/// returned delegate.
pub(crate) fn observe_field(
    mtm: MainThreadMarker,
    field: &NSTextField,
) -> (Rc<FieldHandlers>, Retained<FieldObserver>) {
    let handlers = Rc::new(FieldHandlers::default());
    let observer = FieldObserver::new(mtm, handlers.clone());
    unsafe { field.setDelegate(Some(objc2::runtime::ProtocolObject::from_ref(&*observer))) };
    (handlers, observer)
}

// --------------------------------------------------------------- TextArea

// プレースホルダー用のラベル。
//
// NSTextView の上に重ねるため、そのままでは文字の載っている場所を
// クリックしてもキャレットが立たない。`hitTest:` で nil を返すと、
// AppKit はこのビューを飛ばして下の NSTextView へ当たり判定を渡す。
define_class!(
    #[unsafe(super(NSTextField))]
    #[thread_kind = MainThreadOnly]
    #[name = "NauiPlaceholderLabel"]
    /// A label that passes clicks through to the view below.
    struct PlaceholderLabel;

    unsafe impl NSObjectProtocol for PlaceholderLabel {}

    impl PlaceholderLabel {
        #[unsafe(method_id(hitTest:))]
        fn hit_test(&self, _point: NSPoint) -> Option<Retained<NSView>> {
            None
        }
    }
);

impl PlaceholderLabel {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this: Allocated<Self> = Self::alloc(mtm);
        let this: Retained<Self> = unsafe { msg_send![this, init] };
        // NSTextField::labelWithString と同じ構成を手で作る。
        this.setEditable(false);
        this.setSelectable(false);
        this.setBezeled(false);
        this.setDrawsBackground(false);
        this.setTextColor(Some(&NSColor::placeholderTextColor()));
        this.setFont(Some(&NSFont::systemFontOfSize(NSFont::systemFontSize())));
        this.setTranslatesAutoresizingMaskIntoConstraints(false);
        this
    }
}

/// A single replaceable "text changed" handler.
///
/// naui also needs to subscribe to show and hide the placeholder, so the delegate
/// is attached from creation and the app's closure goes here. So that calling
/// `on_change` again mid-notification does not double-borrow, it is taken out only
/// for the duration of the call (the same shape as
/// [`crate::trampoline::SelectHandler`]).
#[derive(Clone, Default)]
struct TextHandler(Rc<RefCell<Option<Box<dyn FnMut(&str)>>>>);

impl TextHandler {
    fn set(&self, f: impl FnMut(&str) + 'static) {
        *self.0.borrow_mut() = Some(Box::new(f));
    }

    fn emit(&self, text: &str) {
        let Some(mut f) = self.0.borrow_mut().take() else {
            return;
        };
        f(text);
        let mut slot = self.0.borrow_mut();
        if slot.is_none() {
            *slot = Some(f);
        }
    }
}

struct TextAreaInner {
    /// The view visible from outside. Multi-line input is one widget together with this
    /// scroll view.
    scroll: Retained<NSScrollView>,
    text_view: Retained<NSTextView>,
    placeholder: Retained<PlaceholderLabel>,
    handler: TextHandler,
    /// The delegate is a weak reference, so it is held.
    _observer: Retained<TextViewObserver>,
}

/// Multi-line text input (an NSTextView in an NSScrollView).
///
/// It handles text including newlines as is, and wrapping, scrolling, IME and undo
/// are done by AppKit. **A scroll view has no height that fits its contents**, so
/// set the height with `set_sizing` (the same as [`crate::Scroll`] and
/// [`crate::List`]).
#[derive(Clone)]
pub struct TextArea(Rc<TextAreaInner>);
crate::keys::impl_key_down!(TextArea);
crate::focus::impl_focus_change!(TextArea);

impl Widget for TextArea {
    fn native_view(&self) -> Retained<NSView> {
        let view: &NSView = self.0.scroll.as_ref();
        view.retain()
    }
    fn boxed_clone(&self) -> Box<dyn Widget> {
        Box::new(self.clone())
    }
}

crate::widgets::impl_sizing!(TextArea);

impl TextArea {
    pub(crate) fn new(mtm: MainThreadMarker, text: &str) -> Self {
        let scroll = NSScrollView::new(mtm);
        scroll.setHasVerticalScroller(true);
        // 1 行入力 (NSTextField) と同じ、へこんだ枠にする。
        scroll.setBorderType(NSBorderType::BezelBorder);

        let text_view = NSTextView::new(mtm);
        // NSScrollView の中の NSTextView は、Auto Layout ではなく
        // autoresizing で幅を追わせるのが AppKit の標準の組み方。
        // 高さは中身に合わせて伸び、はみ出した分をスクロールが担う。
        let content = scroll.contentSize();
        text_view.setFrame(NSRect::new(NSPoint::new(0.0, 0.0), content));
        text_view.setMinSize(NSSize::new(0.0, 0.0));
        text_view.setMaxSize(NSSize::new(f64::MAX, f64::MAX));
        text_view.setVerticallyResizable(true);
        text_view.setHorizontallyResizable(false);
        text_view.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable);
        if let Some(container) = unsafe { text_view.textContainer() } {
            // 幅はテキストビューに追従させ、高さは無制限にする。
            // これで横は折り返し、縦はスクロールになる。
            container.setContainerSize(NSSize::new(content.width, f64::MAX));
            container.setWidthTracksTextView(true);
        }
        // 書式付きテキストは扱わない。貼り付けも書式を落として素の文字にする。
        text_view.setRichText(false);
        text_view.setAllowsUndo(true);
        text_view.setFont(Some(&NSFont::systemFontOfSize(NSFont::systemFontSize())));
        text_view.setString(&NSString::from_str(text));
        scroll.setDocumentView(Some(&text_view));

        // NSTextView にプレースホルダーは無いので、薄い文字のラベルを重ねる。
        let placeholder = PlaceholderLabel::new(mtm);
        placeholder.setHidden(!text.is_empty());
        let view: &NSView = text_view.as_ref();
        view.addSubview(&placeholder);
        // 文字の描き始めは「テキストの余白 + 行の余白」の内側。
        let inset = text_view.textContainerInset();
        let padding = unsafe { text_view.textContainer() }
            .map(|container| container.lineFragmentPadding())
            .unwrap_or(0.0);
        let constraints = [
            placeholder
                .leadingAnchor()
                .constraintEqualToAnchor_constant(&view.leadingAnchor(), inset.width + padding),
            placeholder
                .topAnchor()
                .constraintEqualToAnchor_constant(&view.topAnchor(), inset.height),
            placeholder
                .trailingAnchor()
                .constraintLessThanOrEqualToAnchor_constant(
                    &view.trailingAnchor(),
                    -(inset.width + padding),
                ),
        ];
        NSLayoutConstraint::activateConstraints(&NSArray::from_retained_slice(&constraints));

        // デリゲートは常に付けておく。プレースホルダーの出し入れが
        // アプリのクロージャの有無に左右されないようにするため。
        let handler = TextHandler::default();
        let observer = TextViewObserver::new(mtm, {
            let placeholder = placeholder.clone();
            let handler = handler.clone();
            move |text: &str| {
                placeholder.setHidden(!text.is_empty());
                handler.emit(text);
            }
        });
        text_view.setDelegate(Some(objc2::runtime::ProtocolObject::from_ref(&*observer)));

        Self(Rc::new(TextAreaInner {
            scroll,
            text_view,
            placeholder,
            handler,
            _observer: observer,
        }))
    }

    /// The current text. Newlines are included as is.
    pub fn text(&self) -> String {
        self.0.text_view.string().to_string()
    }

    /// Replaces the text. `on_change` is not called.
    pub fn set_text(&self, text: &str) {
        self.0.text_view.setString(&NSString::from_str(text));
        // setString: は textDidChange: を出さないので、ここで合わせる。
        self.0.placeholder.setHidden(!text.is_empty());
    }

    /// Text shown faintly when nothing has been entered.
    pub fn set_placeholder(&self, text: &str) {
        self.0.placeholder.setStringValue(&NSString::from_str(text));
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.0.text_view.setEditable(enabled);
        // 編集できないことが見て分かるよう、無効なコントロールの色にする。
        // 戻すときは NSTextView の既定である textColor へ (labelColor ではない)。
        let color = if enabled {
            NSColor::textColor()
        } else {
            NSColor::disabledControlTextColor()
        };
        self.0.text_view.setTextColor(Some(&color));
    }

    /// Called with the current text every time a character is typed.
    ///
    /// Also called for newlines. Not called for `set_text`.
    pub fn on_change(&self, f: impl FnMut(&str) + 'static) {
        self.0.handler.set(f);
    }

    /// The inner `NSTextView`. Public as a backend-specific escape hatch.
    pub fn native_text_view(&self) -> Retained<NSTextView> {
        self.0.text_view.clone()
    }

    /// Whether it is read-only. Editable by default.
    ///
    /// While read-only, the text can still be selected and copied, and it still takes
    /// focus (unlike `set_enabled(false)`, it is not dimmed). Use it for fields that
    /// show logs or generated output.
    pub fn set_read_only(&self, read_only: bool) {
        self.0.text_view.setEditable(!read_only);
        self.0.text_view.setSelectable(true);
    }

    pub fn is_read_only(&self) -> bool {
        !self.0.text_view.isEditable()
    }
}

// ----------------------------------------------------------------- Slider

struct SliderInner {
    native: Retained<NSSlider>,
    target: RefCell<Option<Retained<ActionTarget>>>,
}

/// A slider (NSSlider).
#[derive(Clone)]
pub struct Slider(Rc<SliderInner>);
impl_widget!(Slider);

impl Slider {
    pub(crate) fn new(mtm: MainThreadMarker, min: f64, max: f64) -> Self {
        let native = NSSlider::new(mtm);
        {
            native.setMinValue(min);
            native.setMaxValue(max);
            native.setContinuous(true);
        }
        Self(Rc::new(SliderInner {
            native,
            target: RefCell::new(None),
        }))
    }

    pub fn value(&self) -> f64 {
        self.0.native.doubleValue()
    }

    pub fn set_value(&self, value: f64) {
        self.0.native.setDoubleValue(value);
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.0.native.setEnabled(enabled);
    }

    /// Called with the value every time the knob moves.
    pub fn on_change(&self, mut f: impl FnMut(f64) + 'static) {
        let mtm = MainThreadMarker::from(&*self.0.native);
        let native = self.0.native.clone();
        let target = ActionTarget::new(mtm, move || {
            f(native.doubleValue());
        });
        unsafe {
            self.0.native.setTarget(Some(&target));
            self.0.native.setAction(Some(sel!(invoke:)));
        }
        *self.0.target.borrow_mut() = Some(target);
    }
}

// ------------------------------------------------------------ ProgressBar

struct ProgressInner {
    native: Retained<NSProgressIndicator>,
}

/// A progress bar (NSProgressIndicator).
#[derive(Clone)]
pub struct ProgressBar(Rc<ProgressInner>);
impl_widget!(ProgressBar);

impl ProgressBar {
    pub(crate) fn new(mtm: MainThreadMarker) -> Self {
        let native = NSProgressIndicator::new(mtm);
        {
            native.setStyle(NSProgressIndicatorStyle::Bar);
            native.setIndeterminate(false);
            native.setMinValue(0.0);
            native.setMaxValue(1.0);
        }
        Self(Rc::new(ProgressInner { native }))
    }

    /// 0.0..=1.0.
    pub fn set_value(&self, value: f64) {
        self.0.native.setDoubleValue(value.clamp(0.0, 1.0));
    }

    pub fn value(&self) -> f64 {
        self.0.native.doubleValue()
    }

    /// Whether to show work whose progress is unknown (indeterminate progress).
    ///
    /// While `true`, motion shows "working" instead of a value. Turning it off returns
    /// to showing the value set with [`set_value`](Self::set_value) (the value is
    /// remembered).
    pub fn set_indeterminate(&self, indeterminate: bool) {
        let native = &self.0.native;
        if native.isIndeterminate() == indeterminate {
            return;
        }
        native.setIndeterminate(indeterminate);
        if indeterminate {
            unsafe { native.startAnimation(None) };
        } else {
            unsafe { native.stopAnimation(None) };
        }
    }

    pub fn is_indeterminate(&self) -> bool {
        self.0.native.isIndeterminate()
    }
}

// ------------------------------------------------------------------ Stack

struct StackInner {
    native: Retained<NSStackView>,
    /// A view at the end that takes the leftover space, so that Auto children do not.
    tail_spacer: Retained<NSView>,
    tail_spacer_active: Cell<bool>,
    /// The constraint "keep the tail spacer at 0 on the main axis". Its priority decides
    /// "whether it may absorb leftover space" (strong only when nested).
    tail_zero: Retained<NSLayoutConstraint>,
    /// Whether it is inside a `Stack` of the same direction.
    nested: Cell<bool>,
    /// Whether `Fill` was specified on the main axis. If so, it may grow even when
    /// nested.
    fill_main: Cell<bool>,
    /// Holds the children's handles, keeping them alive along with their trampolines.
    children: RefCell<Vec<Box<dyn Widget>>>,
    /// The constraints tying children with `Fill` on the cross axis to the stack's width
    /// / height. Kept because the constants change when the padding changes.
    fill_constraints: RefCell<Vec<Retained<NSLayoutConstraint>>>,
    padding: Cell<Padding>,
    spacing: Cell<f64>,
}

/// A container that lays out its children vertically / horizontally (NSStackView).
#[derive(Clone)]
pub struct Stack(Rc<StackInner>);

impl_interaction!(Stack);

impl Widget for Stack {
    fn native_view(&self) -> Retained<NSView> {
        let view: &NSView = self.0.native.as_ref();
        view.retain()
    }

    fn boxed_clone(&self) -> Box<dyn Widget> {
        Box::new(self.clone())
    }

    /// When put into a `Stack` of the same direction, makes its tail spacer shrink
    /// strongly.
    ///
    /// Otherwise, either the outer or the inner one growing would be an equally valid
    /// Auto Layout solution, and the inner one might grow, leaving a large gap in the
    /// middle of a section ([`crate::layout::NESTED_TAIL_PRIORITY`]).
    fn nested_in_stack(&self, vertical: bool) {
        if self.is_vertical() == vertical {
            self.0.nested.set(true);
            self.update_tail_priority();
        }
    }
}

impl Stack {
    /// Sets the size. Each call discards the previous setting.
    ///
    /// A `Stack` with `Fill` specified on the main axis may take leftover space even
    /// when nested (it is the one saying "I want to grow").
    pub fn set_sizing(&self, sizing: naui_core::Sizing) {
        let view = <Self as Widget>::native_view(self);
        crate::layout::apply_sizing(&view, sizing);
        let fill_main = if self.is_vertical() {
            sizing.height.is_fill()
        } else {
            sizing.width.is_fill()
        };
        self.0.fill_main.set(fill_main);
        self.update_tail_priority();
    }

    /// Decides again from the current state whether the tail spacer may absorb leftover
    /// space.
    fn update_tail_priority(&self) {
        let hold = self.0.nested.get() && !self.0.fill_main.get();
        self.0.tail_zero.setPriority(if hold {
            crate::layout::NESTED_TAIL_PRIORITY
        } else {
            crate::layout::TAIL_PRIORITY
        });
    }

    pub(crate) fn new(mtm: MainThreadMarker, orientation: Orientation) -> Self {
        let native = NSStackView::new(mtm);
        {
            native.setOrientation(if orientation.is_vertical() {
                NSUserInterfaceLayoutOrientation::Vertical
            } else {
                NSUserInterfaceLayoutOrientation::Horizontal
            });
            // `Fill` は AppKit が主軸の hugging priority を必須扱いにし、
            // Auto の子のどれかへ余りを配ってしまう。GravityAreas は
            // 子の hugging priority に従うため、Fill / Spacer だけが余りを受ける。
            native.setDistribution(NSStackViewDistribution::GravityAreas);
            native.setAlignment(if orientation.is_vertical() {
                NSLayoutAttribute::CenterX
            } else {
                NSLayoutAttribute::CenterY
            });
        }
        let spacing = native.spacing();
        let tail_spacer = NSView::new(mtm);
        crate::layout::prepare_child(&tail_spacer);
        // 明示的な `Fill` / `Spacer` よりは優先度を上げ、Auto の子よりは
        // 下げる。これで、指定が無いときだけ末尾の受け皿が余りを吸う。
        for horizontal in [true, false] {
            let orientation = if horizontal {
                objc2_app_kit::NSLayoutConstraintOrientation::Horizontal
            } else {
                objc2_app_kit::NSLayoutConstraintOrientation::Vertical
            };
            tail_spacer.setContentHuggingPriority_forOrientation(2.0, orientation);
            tail_spacer.setContentCompressionResistancePriority_forOrientation(1.0, orientation);
        }
        native.addArrangedSubview(&tail_spacer);
        // 受け皿は中身を持たないので、放っておくと何の抵抗もなく伸びる。
        // 「主軸で 0 にしたい」を弱い優先度で張っておき、入れ子になったときは
        // `nested_in_stack` がこれを強める。
        let tail_zero =
            crate::layout::tail_zero_constraint(&tail_spacer, orientation.is_vertical());
        tail_zero.setActive(true);
        Self(Rc::new(StackInner {
            native,
            tail_spacer,
            tail_spacer_active: Cell::new(true),
            tail_zero,
            nested: Cell::new(false),
            fill_main: Cell::new(false),
            children: RefCell::new(Vec::new()),
            fill_constraints: RefCell::new(Vec::new()),
            padding: Cell::new(Padding::ZERO),
            spacing: Cell::new(spacing),
        }))
    }

    pub fn set_spacing(&self, spacing: f64) {
        self.0.spacing.set(spacing);
        self.0.native.setSpacing(spacing);
        if let Some(last) = self.0.children.borrow().last() {
            let view = last.native_view();
            self.0.native.setCustomSpacing_afterView(0.0, &view);
        }
        self.invalidate_natural_size();
    }

    pub fn set_padding(&self, padding: Padding) {
        self.0.padding.set(padding);
        self.0.native.setEdgeInsets(NSEdgeInsets {
            top: padding.top,
            left: padding.left,
            bottom: padding.bottom,
            right: padding.right,
        });
        // 交差軸いっぱいに広げている子は、余白のぶんだけ狭くなる。
        let inset = self.cross_inset();
        for constraint in self.0.fill_constraints.borrow().iter() {
            constraint.setConstant(-inset);
        }
        self.invalidate_natural_size();
    }

    /// The total padding taken in the cross-axis direction.
    fn cross_inset(&self) -> f64 {
        let padding = self.0.padding.get();
        if self.is_vertical() {
            padding.left + padding.right
        } else {
            padding.top + padding.bottom
        }
    }

    fn is_vertical(&self) -> bool {
        self.0.native.orientation() == NSUserInterfaceLayoutOrientation::Vertical
    }

    /// NSStackView does not expose an intrinsic size itself, so changes to children or
    /// padding are reported explicitly to a Grid's Auto rows.
    fn invalidate_natural_size(&self) {
        crate::layout::invalidate_ancestors(&self.0.native);
    }

    pub fn set_align(&self, align: Align) {
        let vertical = self.is_vertical();
        let attr = match (align, vertical) {
            (Align::Fill, _) => NSLayoutAttribute::NotAnAttribute,
            (Align::Start, true) => NSLayoutAttribute::Leading,
            (Align::Center, true) => NSLayoutAttribute::CenterX,
            (Align::End, true) => NSLayoutAttribute::Trailing,
            (Align::Start, false) => NSLayoutAttribute::Top,
            (Align::Center, false) => NSLayoutAttribute::CenterY,
            (Align::End, false) => NSLayoutAttribute::Bottom,
        };
        self.0.native.setAlignment(attr);
    }

    /// Appends a child at the end.
    ///
    /// If the child specifies [`naui_core::Length::Fill`] on the cross axis, it is
    /// stretched to the stack's width (for vertical stacks) or height. `Fill` on the
    /// main axis is handled by NSStackView through the hugging priority.
    pub fn append(&self, child: &dyn Widget) {
        let view = child.native_view();
        crate::layout::prepare_child(&view);
        let index = self.0.children.borrow().len();
        let vertical = self.is_vertical();
        // 入れ子の `Stack` は、余りを吸うのを外側へ譲る。
        child.nested_in_stack(vertical);
        let wants_main_fill = crate::layout::wants_fill(&view, !vertical);
        if !wants_main_fill {
            crate::layout::keep_auto_size(&view, !vertical);
        }
        if wants_main_fill && self.0.tail_spacer_active.replace(false) {
            self.0.tail_zero.setActive(false);
            self.0.native.removeArrangedSubview(&self.0.tail_spacer);
            self.0.tail_spacer.removeFromSuperview();
        }
        if let Some(last) = self.0.children.borrow().last() {
            let previous = last.native_view();
            self.0
                .native
                .setCustomSpacing_afterView(self.0.spacing.get(), &previous);
        }
        self.0
            .native
            .insertArrangedSubview_atIndex(&view, index as isize);
        if self.0.tail_spacer_active.get() {
            self.0.native.setCustomSpacing_afterView(0.0, &view);
        }

        // 縦並びの交差軸は横方向。
        let cross_is_horizontal = vertical;
        if crate::layout::wants_fill(&view, cross_is_horizontal) {
            let inset = self.cross_inset();
            let (child, parent) = if vertical {
                (view.widthAnchor(), self.0.native.widthAnchor())
            } else {
                (view.heightAnchor(), self.0.native.heightAnchor())
            };
            // 「はみ出さない」は必須、「親に合わせる」は 1 段下。上限
            // (`max_width` など) を付けた子は、上限のほうが勝つ。
            let at_most = child.constraintLessThanOrEqualToAnchor_constant(&parent, -inset);
            let equal = child.constraintEqualToAnchor_constant(&parent, -inset);
            equal.setPriority(crate::layout::CROSS_FILL_PRIORITY);
            for constraint in [&at_most, &equal] {
                constraint.setActive(true);
            }
            let mut constraints = self.0.fill_constraints.borrow_mut();
            constraints.push(at_most);
            constraints.push(equal);
        }

        self.0.children.borrow_mut().push(child.boxed_clone());
        self.invalidate_natural_size();
    }

    /// Inserts a child at the given position. If `index` is at least the current
    /// count, it is appended at the end.
    pub fn insert(&self, index: usize, child: &dyn Widget) {
        let mut children = self.take_children();
        let index = index.min(children.len());
        children.insert(index, child.boxed_clone());
        self.refill(children);
    }

    /// Removes the child at the given position. Does nothing if out of range.
    pub fn remove(&self, index: usize) {
        let mut children = self.take_children();
        if index >= children.len() {
            self.refill(children);
            return;
        }
        children.remove(index);
        self.refill(children);
    }

    /// Removes every child.
    pub fn clear(&self) {
        drop(self.take_children());
        self.invalidate_natural_size();
    }

    /// Removes all the children laid out and returns them.
    ///
    /// [`append`](Stack::append) places the tail spacer, spacing and `Fill` constraints
    /// against "the sequence at that time", so operations that change the order remove
    /// and re-stack.
    fn take_children(&self) -> Vec<Box<dyn Widget>> {
        let children = std::mem::take(&mut *self.0.children.borrow_mut());
        for child in &children {
            let view = child.native_view();
            self.0.native.removeArrangedSubview(&view);
            view.removeFromSuperview();
        }
        {
            let mut constraints = self.0.fill_constraints.borrow_mut();
            for constraint in constraints.iter() {
                constraint.setActive(false);
            }
            constraints.clear();
        }
        // 主軸に `Fill` の子がいて外していた受け皿を、空の状態へ戻す。
        if !self.0.tail_spacer_active.replace(true) {
            self.0.native.addArrangedSubview(&self.0.tail_spacer);
            self.0.tail_zero.setActive(true);
        }
        children
    }

    /// Re-stacks the children removed with [`take_children`](Stack::take_children).
    fn refill(&self, children: Vec<Box<dyn Widget>>) {
        for child in &children {
            self.append(&**child);
        }
        self.invalidate_natural_size();
    }

    /// The number of children added.
    pub fn len(&self) -> usize {
        self.0.children.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
