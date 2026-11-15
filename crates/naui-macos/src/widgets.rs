//! Handles wrapping real AppKit controls.
//!
//! Every handle is an `Rc<Inner>`, and `Inner` holds the Retained native object and
//! its trampolines. Cloning a handle still leaves one native object.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use dispatch2::{DispatchQueue, MainThreadBound};
use naui_core::{
    Align, Breakpoint, ButtonRole, EventResponse, LinkHandler, Orientation, Padding, RichText,
    StackArrangement, TextColor, TextRun, TextStyle,
};
use objc2::rc::{Allocated, Retained};
use objc2::runtime::{AnyObject, Bool, NSObject, NSObjectProtocol};
use objc2::{
    define_class, msg_send, sel, AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, Message,
};
use objc2_app_kit::{
    NSAutoresizingMaskOptions, NSBorderType, NSButton, NSButtonType, NSColor, NSControl,
    NSControlStateValueOff, NSControlStateValueOn, NSFont, NSFontAttributeName,
    NSFontDescriptorSymbolicTraits, NSFontTextStyle, NSFontTextStyleBody, NSFontTextStyleHeadline,
    NSFontTextStyleLargeTitle, NSFontTextStyleSubheadline, NSFontTextStyleTitle1,
    NSFontTextStyleTitle3, NSForegroundColorAttributeName, NSLayoutAttribute, NSLayoutConstraint,
    NSLayoutGuide, NSLineBreakMode, NSLinkAttributeName, NSProgressIndicator,
    NSProgressIndicatorStyle, NSResponder, NSScrollView, NSSearchField, NSSecureTextField,
    NSSlider, NSStackView, NSStackViewDistribution, NSTextAlignment, NSTextField, NSTextView,
    NSTintProminence, NSUnderlineStyle, NSUnderlineStyleAttributeName,
    NSUserInterfaceLayoutOrientation, NSView, NSViewFrameDidChangeNotification,
};
use objc2_foundation::{
    NSArray, NSAttributedString, NSAttributedStringKey, NSDictionary, NSEdgeInsets,
    NSMutableAttributedString, NSNotificationCenter, NSNumber, NSPoint, NSRect, NSSize, NSString,
    NSUInteger, NSURL,
};

use crate::trampoline::{ActionTarget, FieldHandlers, FieldObserver, SharedSlot, TextViewObserver};

/// The common interface implemented by naui's widgets.
pub trait Widget: 'static {
    /// The corresponding AppKit view. Public as a backend-specific escape hatch.
    fn native_view(&self) -> Retained<NSView>;

    #[doc(hidden)]
    fn boxed_clone(&self) -> Box<dyn Widget>;

    /// Called with the direction (`true` for vertical) when put into a `Stack`, and
    /// again when that stack changes direction.
    ///
    /// Used only by nested `Stack`s. To settle that the outer one absorbs leftover
    /// space, a stack stiffens its own tail spacer when the direction is the same.
    /// Other widgets do nothing.
    #[doc(hidden)]
    fn nested_in_stack(&self, _vertical: bool) {}

    /// Called when taken out of a `Stack`, undoing [`nested_in_stack`](Self::nested_in_stack).
    ///
    /// A `Stack` moved into a `Grid` or a `Scroll` absorbs leftover space again, as a
    /// stack that was never nested does.
    #[doc(hidden)]
    fn left_stack(&self) {}
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

            /// Called with the files dropped onto this widget from Finder or another app.
            ///
            /// When widgets with this closure are nested, the innermost one under the
            /// pointer takes the drop, and the window's
            /// [`on_file_drop`](crate::Window::on_file_drop) gets only drops no widget
            /// takes. Text fields take dropped files as text themselves, so drops onto
            /// them do not arrive. Setting it again replaces the previous closure.
            pub fn on_file_drop(&self, f: impl FnMut(&[naui_core::FileEntry]) + 'static) {
                crate::file_drop::set_handler(&<$t as Widget>::native_view(self), f);
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
            /// It can be called before or after putting the widget into a container. Called
            /// after, the parent also redoes its side (the cross-axis `Fill` constraints of a
            /// `Stack`, the placement within a `Grid` cell).
            pub fn set_sizing(&self, sizing: naui_core::Sizing) {
                let view = <$t as Widget>::native_view(self);
                crate::layout::apply_sizing(&view, sizing);
            }
        }
    };
}

pub(crate) use {impl_interaction, impl_sizing, impl_widget};

// ------------------------------------------------------------------ Label

define_class!(
    #[unsafe(super(NSTextField, NSControl, NSView, NSResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "NauiLabel"]
    #[ivars = LinkHandler]
    /// A label that hears about clicks on its links.
    ///
    /// While a selectable text field is being clicked, the field editor is what shows
    /// the text, and the field is that editor's delegate. So a link's click arrives
    /// here as `NSTextViewDelegate`'s method.
    struct NauiLabel;

    unsafe impl NSObjectProtocol for NauiLabel {}

    impl NauiLabel {
        #[unsafe(method(textView:clickedOnLink:atIndex:))]
        fn clicked_on_link(
            &self,
            _text_view: Option<&NSTextView>,
            link: &AnyObject,
            _index: NSUInteger,
        ) -> Bool {
            let href = match (link.downcast_ref::<NSString>(), link.downcast_ref::<NSURL>()) {
                (Some(text), _) => text.to_string(),
                (None, Some(url)) => url
                    .absoluteString()
                    .map(|text| text.to_string())
                    .unwrap_or_default(),
                (None, None) => return Bool::NO,
            };
            if self.ivars().emit(&href) == EventResponse::Continue && !href.is_empty() {
                if let Err(error) = crate::open_url(&href) {
                    eprintln!("naui-macos: could not open the link: {error}");
                }
            }
            Bool::YES
        }
    }
);

impl NauiLabel {
    fn new(mtm: MainThreadMarker, text: &str) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(LinkHandler::default());
        let this: Retained<Self> = unsafe { msg_send![super(this), init] };
        // NSTextField::labelWithString と同じ構成を手で作る。あちらは ivars を
        // 初期化する機会がない。
        this.setEditable(false);
        this.setSelectable(false);
        this.setBezeled(false);
        this.setDrawsBackground(false);
        // `init` で作った欄の揃えは左 (Left) のまま。`labelWithString` などと同じ
        // 自然な揃え (Natural) にし、右から左のアプリでは右へ寄せる。
        this.setAlignment(NSTextAlignment::Natural);
        this.setTextColor(Some(&NSColor::labelColor()));
        this.setFont(Some(&NSFont::systemFontOfSize(NSFont::systemFontSize())));
        this.setTranslatesAutoresizingMaskIntoConstraints(false);
        this.setStringValue(&NSString::from_str(text));
        this
    }
}

struct LabelInner {
    native: Retained<NauiLabel>,
    mtm: MainThreadMarker,
    /// Whether it wraps.
    wraps: Cell<bool>,
    /// The relay that makes the wrapping width follow the frame. `None` while not
    /// wrapping.
    ///
    /// The relay is held with the same lifetime as the handle (the same rule as
    /// `Toast`).
    width_target: RefCell<Option<Retained<ActionTarget>>>,
    /// The formatted content being shown. `None` while showing plain text.
    rich: RefCell<Option<RichText>>,
    /// What `set_selectable` asked for. AppKit's own flag also goes up for links.
    selectable: Cell<bool>,
    /// The font from `set_style`. Kept here because the field's own font follows the
    /// first run of an attributed string.
    font: RefCell<Retained<NSFont>>,
    /// The role from `set_color`, for the same reason.
    color: Cell<TextColor>,
}

/// Non-editable text (an NSTextField in label configuration).
#[derive(Clone)]
pub struct Label(Rc<LabelInner>);
impl_widget!(Label);

impl Label {
    pub(crate) fn new(mtm: MainThreadMarker, text: &str) -> Self {
        let native = NauiLabel::new(mtm, text);
        let font = native
            .font()
            .unwrap_or_else(|| NSFont::systemFontOfSize(NSFont::systemFontSize()));
        let this = Self(Rc::new(LabelInner {
            native,
            mtm,
            wraps: Cell::new(false),
            width_target: RefCell::new(None),
            rich: RefCell::new(None),
            selectable: Cell::new(false),
            font: RefCell::new(font),
            color: Cell::new(TextColor::default()),
        }));
        this.set_wrap(false);
        this
    }

    pub fn text(&self) -> String {
        self.0.native.stringValue().to_string()
    }

    pub fn set_text(&self, text: &str) {
        // 表示中と同じ値なら何もしない。書き直すと編集中のカーソルが動き、
        // 変換中の文字が確定してしまう (render が毎回同じ値を渡すため)。
        // 飾りが付いているときは、文字が同じでも飾りを外す必要がある。
        if self.0.rich.borrow().is_none() && self.text() == text {
            return;
        }
        self.0.rich.borrow_mut().take();
        self.0.sync_link_support();
        self.0.native.setStringValue(&NSString::from_str(text));
        // 飾り付きの文字を入れている間に、欄のフォントと色は最初の断片のものへ
        // 変わっている。
        self.0.native.setFont(Some(&self.0.font.borrow()));
        self.0
            .native
            .setTextColor(Some(&label_color(self.0.color.get())));
        // 文字が変われば要る大きさも変わる (折り返していれば行数ごと)。
        // 親の連なりへ伝えないと、`Grid` の `Auto` 行が前の高さのまま残る。
        crate::layout::invalidate_ancestors(&self.0.native);
    }

    /// Shows text that mixes bold, colors and links (`NSAttributedString`).
    ///
    /// Setting the content already shown does nothing, so the selection and the
    /// keyboard focus survive, and `render` can call it every time.
    /// [`text`](Self::text) returns the text without its formatting, and
    /// [`set_text`](Self::set_text) goes back to plain text.
    ///
    /// AppKit reacts to links only in a selectable field, so **a label holding links
    /// can be selected whatever [`set_selectable`](Self::set_selectable) says**
    /// (`allowsEditingTextAttributes` is also needed, and does not make it editable).
    /// Clicking a link gives the label the keyboard focus, and the pointer over a link
    /// stays an I-beam rather than a pointing hand (the field's cursor rect covers the
    /// whole label, and the field editor's `linkTextAttributes` names no cursor).
    pub fn set_rich_text(&self, text: &RichText) {
        if text.is_plain() {
            self.set_text(&text.plain_text());
            return;
        }
        if self.0.rich.borrow().as_ref() == Some(text) {
            return;
        }
        *self.0.rich.borrow_mut() = Some(text.clone());
        self.0.sync_link_support();
        self.0.apply_rich_text();
        crate::layout::invalidate_ancestors(&self.0.native);
    }

    /// Called with the `href` when a link in the text is clicked. Calling it again
    /// removes the previous one.
    ///
    /// Answering [`EventResponse::Continue`] then opens the `href` with the default
    /// handler; [`EventResponse::Handled`] keeps it from being opened, for links that
    /// act inside the app.
    pub fn on_link(&self, f: impl FnMut(&str) -> EventResponse + 'static) {
        self.0.native.ivars().set(f);
    }

    /// Whether the text can be selected and copied. Not selectable by default.
    ///
    /// Use it for text readers will want to copy, such as chat messages or error
    /// details. Unlike a text field, the text cannot be edited.
    pub fn set_selectable(&self, selectable: bool) {
        self.0.selectable.set(selectable);
        self.0.sync_link_support();
    }

    pub fn is_selectable(&self) -> bool {
        self.0.selectable.get()
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
        let font = preferred_font(style);
        self.0.native.setFont(Some(&font));
        *self.0.font.borrow_mut() = font;
        self.0.apply_rich_text();
        // フォントが変われば行の高さも変わる。折り返し中なら、いまの幅で
        // 測り直させる (frame が動かないときは通知が来ないため)。
        self.0.sync_preferred_width();
    }

    /// The text color role. The default is [`TextColor::Default`].
    ///
    /// AppKit's semantic colors (`labelColor` and so on) are used as is, so they follow
    /// light / dark switching and the accent color setting.
    pub fn set_color(&self, color: TextColor) {
        self.0.color.set(color);
        self.0.native.setTextColor(Some(&label_color(color)));
        self.0.apply_rich_text();
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

/// The bold face of `font`. `font` itself if it has none.
fn bold_font(font: &NSFont) -> Retained<NSFont> {
    let descriptor = font.fontDescriptor();
    let traits = descriptor.symbolicTraits() | NSFontDescriptorSymbolicTraits::TraitBold;
    let bold = descriptor.fontDescriptorWithSymbolicTraits(traits);
    NSFont::fontWithDescriptor_size(&bold, font.pointSize()).unwrap_or_else(|| font.retain())
}

impl LabelInner {
    /// Writes the formatted content to the field. Does nothing while showing plain
    /// text. An attributed string fixes its font and colors when it is made, so this
    /// also runs when the label's own font or color changes.
    fn apply_rich_text(&self) {
        let rich = self.rich.borrow();
        let Some(rich) = rich.as_ref() else {
            return;
        };
        let whole = NSMutableAttributedString::new();
        for run in rich.runs() {
            whole.appendAttributedString(&self.attributed_run(run));
        }
        self.native.setAttributedStringValue(&whole);
        // 文字の大きさが変わるので、折り返し中ならいまの幅で測り直させる。
        self.sync_preferred_width();
    }

    fn attributed_run(&self, run: &TextRun) -> Retained<NSAttributedString> {
        let base = self.font.borrow();
        let font = if run.is_bold() {
            bold_font(&base)
        } else {
            base.retain()
        };
        let color = match (run.href(), run.text_color()) {
            (Some(_), _) => NSColor::linkColor(),
            (None, TextColor::Default) => label_color(self.color.get()),
            (None, role) => label_color(role),
        };
        // リンクの見た目は欄の編集中だけ AppKit が付ける。それ以外の間も同じに
        // 見えるよう、色と下線を自分で入れる。
        let underline = NSNumber::new_isize(NSUnderlineStyle::Single.0);
        let href = run.href().map(NSString::from_str);
        // SAFETY: どれも AppKit が持つ属性名の定数で、値の型は各属性の約束どおり
        // (NSLinkAttributeName は NSString を受ける)。
        let attributes = unsafe {
            let mut keys: Vec<&NSAttributedStringKey> =
                vec![NSFontAttributeName, NSForegroundColorAttributeName];
            let mut values: Vec<&AnyObject> = vec![font.as_ref(), color.as_ref()];
            if let Some(href) = &href {
                keys.extend([NSLinkAttributeName, NSUnderlineStyleAttributeName]);
                values.extend::<[&AnyObject; 2]>([href.as_ref(), underline.as_ref()]);
            }
            NSDictionary::from_slices(&keys, &values)
        };
        // SAFETY: 属性の値の型は上で確かめたとおり。
        unsafe {
            NSAttributedString::initWithString_attributes(
                NSAttributedString::alloc(),
                &NSString::from_str(run.text()),
                Some(&attributes),
            )
        }
    }

    /// Puts AppKit's selectable flags in line with the content. A field reacts to its
    /// links only while both are up.
    fn sync_link_support(&self) {
        let links = self.rich.borrow().as_ref().is_some_and(RichText::has_links);
        self.native.setAllowsEditingTextAttributes(links);
        self.native.setSelectable(links || self.selectable.get());
    }

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

    /// The button's role. The default is [`ButtonRole::Normal`].
    ///
    /// - `Primary`: `tintProminence = .primary` on macOS 26 and later, and
    ///   `bezelColor = controlAccentColor` before that. Both draw the accent color
    ///   like the window's default button, but the Return key equivalent is not set,
    ///   so Return does not activate it.
    /// - `Destructive`: `hasDestructiveAction`. On macOS 26 and later it also gets
    ///   the look AppKit gives destructive buttons in alerts (red text on a faint red
    ///   bezel: `bezelColor = systemRedColor` with `tintProminence = .secondary`).
    ///   Before that AppKit has no such look, so it looks like an ordinary button.
    pub fn set_role(&self, role: ButtonRole) {
        let native = &self.0.native;
        // macOS 26 で入った API。それより前の AppKit には無い。
        let has_prominence = native.respondsToSelector(sel!(setTintProminence:));
        let (bezel, prominence) = match role {
            ButtonRole::Normal => (None, NSTintProminence::Automatic),
            ButtonRole::Primary if has_prominence => (None, NSTintProminence::Primary),
            ButtonRole::Primary => (
                Some(NSColor::controlAccentColor()),
                NSTintProminence::Automatic,
            ),
            ButtonRole::Destructive if has_prominence => {
                (Some(NSColor::systemRedColor()), NSTintProminence::Secondary)
            }
            ButtonRole::Destructive => (None, NSTintProminence::Automatic),
        };
        native.setHasDestructiveAction(role == ButtonRole::Destructive);
        native.setBezelColor(bezel.as_deref());
        if has_prominence {
            native.setTintProminence(prominence);
        }
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
        // 表示中と同じ値なら何もしない。書き直すと編集中のカーソルが動き、
        // 変換中の文字が確定してしまう (render が毎回同じ値を渡すため)。
        if self.text() == text {
            return;
        }
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
        // `new` で作った欄の揃えは左 (Left) のまま。`labelWithString` などと同じ
        // 自然な揃え (Natural) にし、右から左のアプリでは右へ寄せる。
        native.setAlignment(NSTextAlignment::Natural);
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
        // 表示中と同じ値なら何もしない。書き直すと編集中のカーソルが動き、
        // 変換中の文字が確定してしまう (render が毎回同じ値を渡すため)。
        if self.text() == text {
            return;
        }
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
        // 揃えは自然な揃えにする (PasswordInput と同じ理由)。
        native.setAlignment(NSTextAlignment::Natural);
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
        // 表示中と同じ値なら何もしない。書き直すと編集中のカーソルが動き、
        // 変換中の文字が確定してしまう (render が毎回同じ値を渡すため)。
        if self.text() == text {
            return;
        }
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
        this.setAlignment(NSTextAlignment::Natural);
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
struct TextHandler(SharedSlot<dyn FnMut(&str)>);

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
        // 表示中と同じ値なら何もしない。書き直すと編集中のカーソルが動き、
        // 変換中の文字が確定してしまう (render が毎回同じ値を渡すため)。
        if self.text() == text {
            return;
        }
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

// ---------------------------------------------------------------- Spinner

struct SpinnerInner {
    native: Retained<NSProgressIndicator>,
    /// NSProgressIndicator has no getter for whether it is animating, so it is kept.
    spinning: Cell<bool>,
}

/// A spinning indicator (NSProgressIndicator, spinning style).
#[derive(Clone)]
pub struct Spinner(Rc<SpinnerInner>);
impl_widget!(Spinner);

impl Spinner {
    pub(crate) fn new(mtm: MainThreadMarker) -> Self {
        let native = NSProgressIndicator::new(mtm);
        native.setStyle(NSProgressIndicatorStyle::Spinning);
        native.setIndeterminate(true);
        // 止めている間は描かない (WinUI の ProgressRing・GTK と同じ)。場所は残る。
        native.setDisplayedWhenStopped(false);
        unsafe { native.startAnimation(None) };
        Self(Rc::new(SpinnerInner {
            native,
            spinning: Cell::new(true),
        }))
    }

    /// Whether it spins. Spins from creation.
    ///
    /// While stopped it is not drawn but keeps its space (`displayedWhenStopped` is
    /// off). Use `set_visible(false)` to give the space up as well.
    pub fn set_spinning(&self, spinning: bool) {
        if self.0.spinning.replace(spinning) == spinning {
            return;
        }
        if spinning {
            unsafe { self.0.native.startAnimation(None) };
        } else {
            unsafe { self.0.native.stopAnimation(None) };
        }
    }

    pub fn is_spinning(&self) -> bool {
        self.0.spinning.get()
    }
}

// ------------------------------------------------------------------ Stack

struct StackInner {
    native: Retained<NSStackView>,
    /// A view at the end that takes the leftover space, so that Auto children do not.
    tail_spacer: Retained<NSView>,
    tail_spacer_active: Cell<bool>,
    /// The constraint "keep the tail spacer at 0 on the main axis". Its priority decides
    /// "whether it may absorb leftover space" (strong only when nested). Made again
    /// when the orientation changes.
    tail_zero: RefCell<Retained<NSLayoutConstraint>>,
    /// The direction of the `Stack` it is in (`true` for vertical), if any. Nested in
    /// one of the same direction, it leaves the leftover space to the outer one.
    parent_vertical: Cell<Option<bool>>,
    /// Its own size specification. `Fill` on the main axis lets it grow even when
    /// nested; which axis that is depends on the orientation.
    sizing: Cell<naui_core::Sizing>,
    /// The cross-axis alignment, applied again when the orientation changes.
    align: Cell<Align>,
    /// Holds the children's handles, keeping them alive along with their trampolines.
    children: RefCell<Vec<Box<dyn Widget>>>,
    /// The area inside the padding. Children are aligned to it on the cross axis.
    ///
    /// `NSStackView` keeps its children inside `edgeInsets` on the cross axis only at
    /// its hugging priority (250); its required edge constraints are `>= 0`. Its own
    /// alignment does not follow the insets either: `CenterX` / `CenterY` centers on
    /// the whole view, and `Trailing` / `Bottom` adds the inset outward
    /// (`child.right == stack.right + inset`). So the native alignment is always
    /// `NotAnAttribute`, and the alignment is placed against this guide instead.
    padding_guide: Retained<NSLayoutGuide>,
    /// The guide's leading, trailing, top and bottom distances from the stack. Their
    /// constants follow the padding.
    padding_constraints: [Retained<NSLayoutConstraint>; 4],
    /// The cross-axis constraints placed for each child (alignment, and `Fill`), looked
    /// up by the child's view (its address). Made again when the alignment, the
    /// orientation or the child's specification changes.
    cross_constraints: RefCell<HashMap<usize, Vec<Retained<NSLayoutConstraint>>>>,
    padding: Cell<Padding>,
    spacing: Cell<f64>,
    /// The orientation the app gave and the breakpoint, which decide the orientation
    /// the children are laid out in.
    arrangement: Cell<StackArrangement>,
    /// Watches the frame while there is a breakpoint.
    frame_target: RefCell<Option<Retained<ActionTarget>>>,
    /// Switching to the orientation `arrangement` decided is queued on the main queue.
    switch_scheduled: Cell<bool>,
}

impl Drop for StackInner {
    fn drop(&mut self) {
        crate::layout::unregister_container(&self.native);
        if let Some(target) = self.frame_target.get_mut().take() {
            unsafe { NSNotificationCenter::defaultCenter().removeObserver(&target) };
        }
    }
}

fn view_key(view: &NSView) -> usize {
    view as *const NSView as usize
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
        self.0.parent_vertical.set(Some(vertical));
        self.update_tail_priority();
    }

    fn left_stack(&self) {
        self.0.parent_vertical.set(None);
        self.update_tail_priority();
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
        self.0.sizing.set(sizing);
        self.update_tail_priority();
    }

    /// Decides again from the current state whether the tail spacer may absorb leftover
    /// space.
    fn update_tail_priority(&self) {
        let vertical = self.is_vertical();
        let nested = self.0.parent_vertical.get() == Some(vertical);
        let sizing = self.0.sizing.get();
        let fill_main = if vertical {
            sizing.height.is_fill()
        } else {
            sizing.width.is_fill()
        };
        let hold = nested && !fill_main;
        self.0.tail_zero.borrow().setPriority(if hold {
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
            // 交差軸の揃えは `padding_guide` へ自前で張る (理由はその説明に)。
            native.setAlignment(NSLayoutAttribute::NotAnAttribute);
        }
        let padding_guide = NSLayoutGuide::new();
        native.addLayoutGuide(&padding_guide);
        let padding_constraints = [
            padding_guide
                .leadingAnchor()
                .constraintEqualToAnchor(&native.leadingAnchor()),
            native
                .trailingAnchor()
                .constraintEqualToAnchor(&padding_guide.trailingAnchor()),
            padding_guide
                .topAnchor()
                .constraintEqualToAnchor(&native.topAnchor()),
            native
                .bottomAnchor()
                .constraintEqualToAnchor(&padding_guide.bottomAnchor()),
        ];
        // 余白の合計より小さくされても衝突しないよう、末尾と下だけ必須から
        // 1 段下げる (通常の大きさでは必ず満たされる)。
        padding_constraints[1].setPriority(crate::layout::CROSS_FILL_PRIORITY);
        padding_constraints[3].setPriority(crate::layout::CROSS_FILL_PRIORITY);
        for constraint in &padding_constraints {
            constraint.setActive(true);
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
        let this = Self(Rc::new(StackInner {
            native,
            tail_spacer,
            tail_spacer_active: Cell::new(true),
            tail_zero: RefCell::new(tail_zero),
            parent_vertical: Cell::new(None),
            sizing: Cell::new(naui_core::Sizing::AUTO),
            align: Cell::new(Align::default()),
            children: RefCell::new(Vec::new()),
            padding_guide,
            padding_constraints,
            cross_constraints: RefCell::new(HashMap::new()),
            padding: Cell::new(Padding::ZERO),
            spacing: Cell::new(spacing),
            arrangement: Cell::new(StackArrangement::new(orientation)),
            frame_target: RefCell::new(None),
            switch_scheduled: Cell::new(false),
        }));
        let view: &NSView = this.0.native.as_ref();
        let weak = Rc::downgrade(&this.0);
        crate::layout::register_container(view, move |child| {
            if let Some(inner) = weak.upgrade() {
                Stack(inner).refresh_child(child);
            }
        });
        this
    }

    /// Switches between laying out vertically and horizontally
    /// (`NSStackView.orientation`).
    ///
    /// The children stay in the view hierarchy, so a field being typed into keeps its
    /// focus and IME composition. What depends on the direction is placed again: the
    /// cross-axis alignment and `Fill` constraints, the main-axis hugging, the tail
    /// spacer, and whether nested `Stack`s leave the leftover space to their parent.
    ///
    /// With a [`set_breakpoint`](Self::set_breakpoint), this is the orientation used
    /// below the breakpoint's width.
    pub fn set_orientation(&self, orientation: Orientation) {
        let mut arrangement = self.0.arrangement.get();
        arrangement.set_base(orientation);
        self.0.arrangement.set(arrangement);
        self.arrange(arrangement.orientation());
    }

    /// The orientation given at creation or with
    /// [`set_orientation`](Self::set_orientation).
    pub fn orientation(&self) -> Orientation {
        self.0.arrangement.get().base()
    }

    /// Declares the width from which the children are laid out in another
    /// orientation. `None` removes it.
    ///
    /// AppKit has no container that switches by size, so naui watches the
    /// `NSStackView`'s frame (`NSViewFrameDidChangeNotification`) and, when the width
    /// crosses the breakpoint, switches on the next iteration of the main queue rather
    /// than in the middle of the layout pass that moved the frame.
    pub fn set_breakpoint(&self, breakpoint: Option<Breakpoint>) {
        let mut arrangement = self.0.arrangement.get();
        arrangement.resized(self.0.native.frame().size.width);
        arrangement.set_breakpoint(breakpoint);
        self.0.arrangement.set(arrangement);
        if breakpoint.is_some() {
            self.watch_frame();
        }
        self.arrange(arrangement.orientation());
    }

    pub fn breakpoint(&self) -> Option<Breakpoint> {
        self.0.arrangement.get().breakpoint()
    }

    /// The orientation the children are laid out in now.
    pub fn current_orientation(&self) -> Orientation {
        if self.is_vertical() {
            Orientation::Vertical
        } else {
            Orientation::Horizontal
        }
    }

    /// Starts telling `arrangement` the width after each change of the frame.
    fn watch_frame(&self) {
        if self.0.frame_target.borrow().is_some() {
            return;
        }
        let mtm = MainThreadMarker::from(&*self.0.native);
        let target = ActionTarget::new(mtm, {
            let weak = Rc::downgrade(&self.0);
            move || {
                if let Some(inner) = weak.upgrade() {
                    Stack(inner).frame_changed();
                }
            }
        });
        self.0.native.setPostsFrameChangedNotifications(true);
        unsafe {
            NSNotificationCenter::defaultCenter().addObserver_selector_name_object(
                &target,
                sel!(invoke:),
                Some(NSViewFrameDidChangeNotification),
                Some(&self.0.native),
            );
        }
        *self.0.frame_target.borrow_mut() = Some(target);
    }

    /// The frame changed, in the middle of a layout pass. If the width crossed the
    /// breakpoint, switches once the pass is over.
    fn frame_changed(&self) {
        let mut arrangement = self.0.arrangement.get();
        let changed = arrangement.resized(self.0.native.frame().size.width);
        self.0.arrangement.set(arrangement);
        if !changed || self.0.switch_scheduled.replace(true) {
            return;
        }
        let mtm = MainThreadMarker::from(&*self.0.native);
        let weak = MainThreadBound::new(Rc::downgrade(&self.0), mtm);
        DispatchQueue::main().exec_async(move || {
            // メインキューの仕事はメインスレッドで走る。
            let mtm = MainThreadMarker::new().expect("called outside the main queue");
            if let Some(inner) = weak.into_inner(mtm).upgrade() {
                inner.switch_scheduled.set(false);
                let stack = Stack(inner);
                // 予約のあとに幅が戻っていれば、いまの向きのままになる。
                stack.arrange(stack.0.arrangement.get().orientation());
            }
        });
    }

    /// Lays the children out in `orientation`.
    fn arrange(&self, orientation: Orientation) {
        let vertical = orientation.is_vertical();
        if self.is_vertical() == vertical {
            return;
        }
        // 前の主軸で Auto の子に付けた強い hugging を外し、最初から新しい
        // 向きで入れた子と同じ優先度にする。
        for child in self.held_children() {
            crate::layout::release_auto_size(&child.native_view(), vertical);
        }
        self.0.native.setOrientation(if vertical {
            NSUserInterfaceLayoutOrientation::Vertical
        } else {
            NSUserInterfaceLayoutOrientation::Horizontal
        });

        // 受け皿を 0 に保つ制約は軸ごとのものなので、作り直す。
        let tail_zero = crate::layout::tail_zero_constraint(&self.0.tail_spacer, vertical);
        let previous = self.0.tail_zero.replace(tail_zero);
        previous.setActive(false);
        self.update_tail_priority();
        if self.0.tail_spacer_active.get() {
            self.0.tail_zero.borrow().setActive(true);
        }

        // 子の側は、入れたときと同じ手順で新しい向きに合わせる。
        for child in self.held_children() {
            child.nested_in_stack(vertical);
            self.apply_child_layout(&child.native_view());
        }
        self.update_tail();
        self.invalidate_natural_size();
    }

    pub fn set_spacing(&self, spacing: f64) {
        self.0.spacing.set(spacing);
        self.0.native.setSpacing(spacing);
        // 子どうしの間隔は子ごとに張っているので、積んだあとの変更も全部へ写す。
        for index in 0..self.len() {
            self.update_spacing_after(index);
        }
        self.invalidate_natural_size();
    }

    pub fn set_padding(&self, padding: Padding) {
        self.0.padding.set(padding);
        // NSStackView の edgeInsets の左右は、右から左のアプリでは入れ替わる
        // (left が先頭側)。start と end をそのまま渡せば反転も付いてくる。
        self.0.native.setEdgeInsets(NSEdgeInsets {
            top: padding.top,
            left: padding.start,
            bottom: padding.bottom,
            right: padding.end,
        });
        // 子は余白の内側のガイドへ揃えてあるので、ガイドを動かせば追従する。
        // ガイドは leading / trailing に張ってあるので、start は先頭側に付き、
        // 右から左のアプリでは右になる。
        let [leading, trailing, top, bottom] = &self.0.padding_constraints;
        leading.setConstant(padding.start);
        trailing.setConstant(padding.end);
        top.setConstant(padding.top);
        bottom.setConstant(padding.bottom);
        self.invalidate_natural_size();
    }

    fn is_vertical(&self) -> bool {
        self.0.native.orientation() == NSUserInterfaceLayoutOrientation::Vertical
    }

    /// NSStackView does not expose an intrinsic size itself, so changes to children or
    /// padding are reported explicitly to a Grid's Auto rows.
    fn invalidate_natural_size(&self) {
        crate::layout::invalidate_ancestors(&self.0.native);
    }

    /// Aligns the children on the cross axis, inside the padding.
    ///
    /// `NSStackView.alignment` stays `NotAnAttribute`; the alignment is placed per
    /// child against the area inside the padding (see `padding_guide`).
    pub fn set_align(&self, align: Align) {
        if self.0.align.replace(align) == align {
            return;
        }
        for child in self.0.children.borrow().iter() {
            self.place_cross(&child.native_view());
        }
    }

    /// Appends a child at the end.
    ///
    /// If the child specifies [`naui_core::Length::Fill`] on the cross axis, it is
    /// stretched to the stack's width (for vertical stacks) or height. `Fill` on the
    /// main axis is handled by NSStackView through the hugging priority. Specifying
    /// it after appending works too.
    pub fn append(&self, child: &dyn Widget) {
        self.mount(self.len(), child);
    }

    /// Inserts a child at the given position. If `index` is at least the current
    /// count, it is appended at the end.
    ///
    /// The other children stay in the view hierarchy, so a field being typed into
    /// keeps its focus and IME composition.
    pub fn insert(&self, index: usize, child: &dyn Widget) {
        self.mount(index.min(self.len()), child);
    }

    /// Removes the child at the given position. Does nothing if out of range.
    ///
    /// Only that child leaves the view hierarchy; the others keep their focus and IME
    /// composition.
    pub fn remove(&self, index: usize) {
        if index >= self.len() {
            return;
        }
        let child = self.0.children.borrow_mut().remove(index);
        self.unmount(&*child);
        self.update_tail();
        // 末尾が変わったら、新しい末尾と受け皿の間を詰め直す。
        self.update_spacing_before(index);
        self.invalidate_natural_size();
    }

    /// Removes every child.
    pub fn clear(&self) {
        let children = std::mem::take(&mut *self.0.children.borrow_mut());
        for child in &children {
            self.unmount(&**child);
        }
        self.update_tail();
        self.invalidate_natural_size();
    }

    /// Places a child at `index` and sets up the constraints its specification needs.
    fn mount(&self, index: usize, child: &dyn Widget) {
        let view = child.native_view();
        // 移っていった子が戻ってきたら、残っていた古い記録は捨てる。
        let index = self.forget_moved(&view, index);
        crate::layout::prepare_child(&view);
        // 入れ子の `Stack` は、余りを吸うのを外側へ譲る。
        child.nested_in_stack(self.is_vertical());
        // 受け皿は常に末尾にいるので、子の数以下の位置はその手前になる。
        // ネイティブの並びには移っていった子がいないので、その分を詰める。
        let native_index = self.0.children.borrow()[..index]
            .iter()
            .filter(|child| self.holds(&child.native_view()))
            .count();
        self.0
            .native
            .insertArrangedSubview_atIndex(&view, native_index as isize);
        self.0
            .children
            .borrow_mut()
            .insert(index, child.boxed_clone());
        self.apply_child_layout(&view);
        self.update_tail();
        self.update_spacing_before(index);
        self.update_spacing_after(index);
        self.invalidate_natural_size();
    }

    /// Takes a child out of the view hierarchy, along with the constraints placed for
    /// it.
    ///
    /// A child that has moved to another parent is left where it is: only this
    /// stack's record of it goes.
    fn unmount(&self, child: &dyn Widget) {
        let view = child.native_view();
        let held = self.holds(&view);
        if held {
            self.0.native.removeArrangedSubview(&view);
            view.removeFromSuperview();
        }
        let removed = self
            .0
            .cross_constraints
            .borrow_mut()
            .remove(&view_key(&view));
        for constraint in removed.iter().flatten() {
            constraint.setActive(false);
        }
        // 入れ子の扱いは、もう新しい親のもの。
        if held {
            child.left_stack();
        }
    }

    /// Redoes the constraints of a child whose specification changed.
    ///
    /// Called from [`crate::layout::apply_sizing`] through
    /// [`crate::layout::register_container`]. The child stays in the view hierarchy.
    fn refresh_child(&self, view: &NSView) {
        let placed = self
            .0
            .children
            .borrow()
            .iter()
            .any(|child| std::ptr::eq::<NSView>(&*child.native_view(), view));
        if !placed {
            return;
        }
        self.apply_child_layout(view);
        self.update_tail();
        self.invalidate_natural_size();
    }

    /// Decides the main-axis hugging and the cross-axis constraints from the child's
    /// specification.
    fn apply_child_layout(&self, view: &NSView) {
        // 移っていった子へ張ると、共通の祖先のない制約になり例外が出る。
        if !self.holds(view) {
            return;
        }
        let vertical = self.is_vertical();
        if !crate::layout::wants_fill(view, !vertical) {
            crate::layout::keep_auto_size(view, !vertical);
        }
        self.place_cross(view);
    }

    /// Places a child on the cross axis inside the padding: the alignment, and for
    /// `Fill`, the size of the area inside the padding.
    fn place_cross(&self, view: &NSView) {
        let previous = self
            .0
            .cross_constraints
            .borrow_mut()
            .remove(&view_key(view));
        for constraint in previous.iter().flatten() {
            constraint.setActive(false);
        }

        // 外さずに別の親へ入れられた子は、もうこの Stack の中にいない。共通の
        // 祖先のない制約は有効にできない (例外になる) ので、張らずにおく。
        // superview は読むだけ。
        let parent = unsafe { view.superview() };
        let inside =
            parent.is_some_and(|parent| std::ptr::eq::<NSView>(&*parent, &**self.0.native));
        if !inside {
            return;
        }
        let vertical = self.is_vertical();
        let guide = &self.0.padding_guide;
        let mut constraints = Vec::new();
        // 縦並びの交差軸は横方向。
        let align = match (self.0.align.get(), vertical) {
            (Align::Fill, _) => None,
            (Align::Start, true) => Some(
                view.leadingAnchor()
                    .constraintEqualToAnchor(&guide.leadingAnchor()),
            ),
            (Align::Center, true) => Some(
                view.centerXAnchor()
                    .constraintEqualToAnchor(&guide.centerXAnchor()),
            ),
            (Align::End, true) => Some(
                view.trailingAnchor()
                    .constraintEqualToAnchor(&guide.trailingAnchor()),
            ),
            (Align::Start, false) => {
                Some(view.topAnchor().constraintEqualToAnchor(&guide.topAnchor()))
            }
            (Align::Center, false) => Some(
                view.centerYAnchor()
                    .constraintEqualToAnchor(&guide.centerYAnchor()),
            ),
            (Align::End, false) => Some(
                view.bottomAnchor()
                    .constraintEqualToAnchor(&guide.bottomAnchor()),
            ),
        };
        if let Some(align) = align {
            align.setPriority(crate::layout::CROSS_ALIGN_PRIORITY);
            constraints.push(align);
        }
        if crate::layout::wants_fill(view, vertical) {
            let (child, inside) = if vertical {
                (view.widthAnchor(), guide.widthAnchor())
            } else {
                (view.heightAnchor(), guide.heightAnchor())
            };
            // 「はみ出さない」は必須、「余白の内側に合わせる」は 1 段下。上限
            // (`max_width` など) を付けた子は、上限のほうが勝つ。
            let at_most = child.constraintLessThanOrEqualToAnchor(&inside);
            let equal = child.constraintEqualToAnchor(&inside);
            equal.setPriority(crate::layout::CROSS_FILL_PRIORITY);
            constraints.extend([at_most, equal]);
        }
        for constraint in &constraints {
            constraint.setActive(true);
        }
        self.0
            .cross_constraints
            .borrow_mut()
            .insert(view_key(view), constraints);
    }

    /// Places the tail spacer when no child has `Fill` on the main axis, and removes it
    /// when one does.
    ///
    /// With the tail spacer present, it would compete with the `Fill` children for the
    /// leftover space.
    fn update_tail(&self) {
        let vertical = self.is_vertical();
        let wants_tail = !self
            .held_children()
            .iter()
            .any(|child| crate::layout::wants_fill(&child.native_view(), !vertical));
        if self.0.tail_spacer_active.replace(wants_tail) == wants_tail {
            return;
        }
        if wants_tail {
            self.0.native.addArrangedSubview(&self.0.tail_spacer);
            self.0.tail_zero.borrow().setActive(true);
            if let Some(last) = self.len().checked_sub(1) {
                self.update_spacing_after(last);
            }
        } else {
            self.0.tail_zero.borrow().setActive(false);
            self.0.native.removeArrangedSubview(&self.0.tail_spacer);
            self.0.tail_spacer.removeFromSuperview();
        }
    }

    /// Decides the spacing after the `index`-th child.
    ///
    /// Between children it is the specified spacing; between the last child and the
    /// tail spacer it is 0 (spacing in front of the tail spacer would add that much
    /// blank space).
    ///
    /// Children that have moved to another parent are skipped: they are not in the
    /// native arrangement, and `setCustomSpacing:afterView:` raises an exception for
    /// a view that is not arranged.
    fn update_spacing_after(&self, index: usize) {
        let children = self.0.children.borrow();
        let Some(child) = children.get(index) else {
            return;
        };
        let view = child.native_view();
        if !self.holds(&view) {
            return;
        }
        let followed = children[index + 1..]
            .iter()
            .any(|next| self.holds(&next.native_view()));
        let spacing = if followed { self.0.spacing.get() } else { 0.0 };
        self.0.native.setCustomSpacing_afterView(spacing, &view);
    }

    /// Decides the spacing after the nearest child in front of `index` that is still
    /// in this stack (records of children that moved away are skipped).
    fn update_spacing_before(&self, index: usize) {
        let previous = self.0.children.borrow()[..index.min(self.len())]
            .iter()
            .rposition(|child| self.holds(&child.native_view()));
        if let Some(previous) = previous {
            self.update_spacing_after(previous);
        }
    }

    /// Drops the records of `view` left from before it moved to another parent, and
    /// returns `index` shifted by the records dropped in front of it.
    fn forget_moved(&self, view: &NSView, index: usize) -> usize {
        if self.holds(view) {
            return index;
        }
        let mut children = self.0.children.borrow_mut();
        let mut shifted = index;
        let mut position = 0;
        children.retain(|child| {
            let stale = std::ptr::eq::<NSView>(&*child.native_view(), view);
            if stale && position < index {
                shifted -= 1;
            }
            position += 1;
            !stale
        });
        drop(children);
        let removed = self
            .0
            .cross_constraints
            .borrow_mut()
            .remove(&view_key(view));
        for constraint in removed.iter().flatten() {
            constraint.setActive(false);
        }
        shifted
    }

    /// Whether `view` is still in this stack (see [`crate::layout::is_placed_in`]).
    fn holds(&self, view: &NSView) -> bool {
        crate::layout::is_placed_in(view, &self.0.native)
    }

    /// The children still in this stack (see [`holds`](Self::holds)), cloned so that
    /// callbacks may touch `children` while they are walked.
    fn held_children(&self) -> Vec<Box<dyn Widget>> {
        self.0
            .children
            .borrow()
            .iter()
            .filter(|child| self.holds(&child.native_view()))
            .map(|child| child.boxed_clone())
            .collect()
    }

    /// The number of children added.
    pub fn len(&self) -> usize {
        self.0.children.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
