//! Handles wrapping real GTK4 controls.
//!
//! Every handle is an `Rc<Inner>`, and `Inner` holds the GTK4 object and the app's
//! closures. Cloning a handle still leaves one native object.
//!
//! Signals are connected **exactly once at creation**, and the app's closures are
//! held in a [`Notifier`], swapped in and out. GTK4 signal handlers require `Fn`
//! (callable any number of times), while naui's API takes `FnMut`.
//!
//! Drawing, layout, IME, accessibility and following the theme are all done by
//! GTK4.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::glib;
use gtk::pango;
use gtk::prelude::*;
use naui_core::{Align, Breakpoint, Orientation, Padding, StackArrangement, TextColor, TextStyle};

use crate::bin::{apply_padding, SizeBin};
use crate::callback::{Notifier, TextNotifier};

/// The common interface implemented by naui's widgets.
pub trait Widget: 'static {
    /// The corresponding GTK4 widget. Public as a backend-specific escape hatch.
    ///
    /// What is returned is **the content control**; what goes into containers is the
    /// [`SizeBin`] wrapping it.
    fn native_widget(&self) -> gtk::Widget;

    #[doc(hidden)]
    fn size_bin(&self) -> SizeBin;

    #[doc(hidden)]
    fn boxed_clone(&self) -> Box<dyn Widget>;
}

macro_rules! impl_widget {
    ($t:ty) => {
        impl Widget for $t {
            fn native_widget(&self) -> gtk::Widget {
                self.0.native.clone().upcast()
            }
            fn size_bin(&self) -> SizeBin {
                self.0.bin.clone()
            }
            fn boxed_clone(&self) -> Box<dyn Widget> {
                Box::new(self.clone())
            }
        }

        crate::widgets::impl_sizing!($t);
    };
}

/// Adds only size specification to types that implement `Widget` by hand.
macro_rules! impl_sizing {
    ($t:ty) => {
        impl $t {
            /// Sets the size. Each call discards the previous setting.
            ///
            /// The actual size is decided by GTK4's layout, so what you pass here is only
            /// "at least this much", "take the leftover space" and "stop at this maximum".
            ///
            /// It can be called before or after putting the widget into a container. What
            /// the container decides ([`Stack::set_align`](crate::Stack::set_align), a
            /// `Grid` column's width) is kept either way.
            pub fn set_sizing(&self, sizing: naui_core::Sizing) {
                <$t as Widget>::size_bin(self).apply_sizing(sizing);
            }

            /// Whether it is shown. Shown by default.
            ///
            /// A hidden widget takes no space in a `Stack`. In a `Grid`, its cell remains.
            pub fn set_visible(&self, visible: bool) {
                crate::interaction::set_visible(&<$t as Widget>::size_bin(self), visible);
            }

            /// The visibility set with [`set_visible`](Self::set_visible). Stays `true` even
            /// if a parent is hidden.
            pub fn is_visible(&self) -> bool {
                crate::interaction::is_visible(&<$t as Widget>::size_bin(self))
            }

            /// Moves keyboard focus here. `true` if it moved.
            ///
            /// If the widget itself cannot take focus (containers, for example), focus moves to
            /// the first thing inside that can. `false` before it is shown, while hidden, or
            /// while disabled.
            pub fn request_focus(&self) -> bool {
                crate::interaction::request_focus(&<$t as Widget>::native_widget(self))
            }

            /// The description shown when the pointer hovers over it. `None` removes it.
            pub fn set_tooltip(&self, text: Option<&str>) {
                crate::interaction::set_tooltip(&<$t as Widget>::native_widget(self), text);
            }

            /// The name reported to screen readers. `None` removes it (the visible text is
            /// used).
            ///
            /// Set it when there is no visible text, or the text does not fully convey the
            /// meaning, as with icon-only buttons.
            pub fn set_accessible_label(&self, text: Option<&str>) {
                crate::interaction::set_accessible_label(
                    &<$t as Widget>::native_widget(self),
                    text,
                );
            }

            /// Called with the files dropped onto this widget from a file manager or
            /// another app.
            ///
            /// When widgets with this closure are nested, the innermost one under the
            /// pointer takes the drop, and the window's
            /// [`on_file_drop`](crate::Window::on_file_drop) gets only drops no widget
            /// takes. Text fields take dropped files as text themselves, so drops onto
            /// them do not arrive. Only files with a local path are passed. Setting it
            /// again replaces the previous closure.
            pub fn on_file_drop(&self, f: impl FnMut(&[naui_core::FileEntry]) + 'static) {
                crate::file_drop::set_handler(&<$t as Widget>::size_bin(self), f);
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

pub(crate) use {impl_sizing, impl_widget};

/// Rewrites a value with the signal blocked.
///
/// GTK4 emits `changed` even when a value is changed from code, but in naui
/// `on_change` is called **only for user actions** (the same as macOS / Web).
pub(crate) fn without_signal<T>(
    object: &impl IsA<glib::Object>,
    handler: &RefCell<Option<glib::SignalHandlerId>>,
    f: impl FnOnce() -> T,
) -> T {
    let object = object.as_ref();
    let handler = handler.borrow();
    if let Some(id) = handler.as_ref() {
        object.block_signal(id);
    }
    let out = f();
    if let Some(id) = handler.as_ref() {
        object.unblock_signal(id);
    }
    out
}

// ------------------------------------------------------------------ Label

struct LabelInner {
    native: gtk::Label,
    bin: SizeBin,
}

/// Non-editable text (`GtkLabel`).
#[derive(Clone)]
pub struct Label(Rc<LabelInner>);
impl_widget!(Label);

impl Label {
    pub(crate) fn new(text: &str) -> Self {
        let native = gtk::Label::new(Some(text));
        // GtkLabel の既定は中央ぞろえだが、naui のラベルは他の環境と同じく左詰め。
        native.set_xalign(0.0);
        let bin = SizeBin::wrap(&native);
        let this = Self(Rc::new(LabelInner { native, bin }));
        this.set_wrap(false);
        this
    }

    pub fn text(&self) -> String {
        self.0.native.text().to_string()
    }

    pub fn set_text(&self, text: &str) {
        self.0.native.set_text(text);
    }

    /// Whether the text can be selected and copied. Not selectable by default.
    ///
    /// Use it for text readers will want to copy, such as chat messages or error
    /// details. Unlike a text field, the text cannot be edited.
    pub fn set_selectable(&self, selectable: bool) {
        self.0.native.set_selectable(selectable);
    }

    pub fn is_selectable(&self) -> bool {
        self.0.native.is_selectable()
    }

    /// Whether long text wraps. Does not wrap by default.
    ///
    /// When not wrapping, it stays on one line and whatever does not fit is cut off
    /// with a trailing ellipsis (…) (`End` of `PangoEllipsizeMode`). **The ellipsis also
    /// makes the `GtkLabel`'s minimum width smaller**, so putting it into a narrow
    /// container no longer pushes the whole container wider.
    pub fn set_wrap(&self, wrap: bool) {
        self.0.native.set_wrap(wrap);
        self.0.native.set_wrap_mode(pango::WrapMode::WordChar);
        self.0.native.set_ellipsize(if wrap {
            pango::EllipsizeMode::None
        } else {
            pango::EllipsizeMode::End
        });
    }

    /// The level of text size and weight. The default is [`TextStyle::Body`].
    ///
    /// It only applies libadwaita's style classes (`.title-1` and so on), so the theme
    /// decides the point size and weight. It also follows the desktop's font settings
    /// as is.
    pub fn set_style(&self, style: TextStyle) {
        // 呼ぶたびに以前の段階は外す。付けっぱなしにすると、あとから
        // 当てたクラスとどちらが勝つかがテーマの並び順で決まってしまう。
        for candidate in TextStyle::ALL {
            if let Some(class) = candidate.style_class() {
                self.0.native.remove_css_class(class);
            }
        }
        if let Some(class) = style.style_class() {
            self.0.native.add_css_class(class);
        }
    }

    /// The text color role. The default is [`TextColor::Default`].
    ///
    /// The theme owns the colors themselves (`.accent` is the desktop's accent color).
    /// The theme also follows light / dark switching.
    pub fn set_color(&self, color: TextColor) {
        for candidate in TextColor::ALL {
            if let Some(class) = candidate.style_class() {
                self.0.native.remove_css_class(class);
            }
        }
        if let Some(class) = color.style_class() {
            self.0.native.add_css_class(class);
        }
    }
}

// ----------------------------------------------------------------- Button

struct ButtonInner {
    native: gtk::Button,
    bin: SizeBin,
    on_click: Notifier<()>,
}

/// A push button (`GtkButton`).
#[derive(Clone)]
pub struct Button(Rc<ButtonInner>);
impl_widget!(Button);

impl Button {
    pub(crate) fn new(text: &str) -> Self {
        let native = gtk::Button::with_label(text);
        let bin = SizeBin::wrap(&native);
        let inner = Rc::new(ButtonInner {
            native,
            bin,
            on_click: Notifier::default(),
        });
        let weak = Rc::downgrade(&inner);
        inner.native.connect_clicked(move |_| {
            if let Some(inner) = weak.upgrade() {
                inner.on_click.emit(());
            }
        });
        Self(inner)
    }

    pub fn set_text(&self, text: &str) {
        self.0.native.set_label(text);
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.0.native.set_sensitive(enabled);
    }

    /// Called on every click. Calling it again removes the previous one.
    pub fn on_click(&self, mut f: impl FnMut() + 'static) {
        self.0.on_click.set(move |()| f());
    }
}

// --------------------------------------------------------------- Checkbox

struct CheckboxInner {
    native: gtk::CheckButton,
    bin: SizeBin,
    on_toggle: Notifier<bool>,
    handler: RefCell<Option<glib::SignalHandlerId>>,
}

/// A check box (`GtkCheckButton`).
#[derive(Clone)]
pub struct Checkbox(Rc<CheckboxInner>);
impl_widget!(Checkbox);

impl Checkbox {
    pub(crate) fn new(label: &str) -> Self {
        let native = gtk::CheckButton::with_label(label);
        crate::indicator::watch(&native);
        let bin = SizeBin::wrap(&native);
        let inner = Rc::new(CheckboxInner {
            native,
            bin,
            on_toggle: Notifier::default(),
            handler: RefCell::new(None),
        });
        let id = {
            let weak = Rc::downgrade(&inner);
            inner.native.connect_toggled(move |native| {
                if let Some(inner) = weak.upgrade() {
                    inner.on_toggle.emit(native.is_active());
                }
            })
        };
        *inner.handler.borrow_mut() = Some(id);
        Self(inner)
    }

    pub fn is_checked(&self) -> bool {
        self.0.native.is_active()
    }

    /// Changes the state from code. `on_toggle` is not called.
    pub fn set_checked(&self, checked: bool) {
        without_signal(&self.0.native, &self.0.handler, || {
            self.0.native.set_active(checked);
        });
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.0.native.set_sensitive(enabled);
    }

    /// Called with the new state every time the user toggles it.
    pub fn on_toggle(&self, f: impl FnMut(bool) + 'static) {
        self.0.on_toggle.set(f);
    }
}

// -------------------------------------------------------------- TextInput

struct TextInputInner {
    native: gtk::Entry,
    bin: SizeBin,
    on_change: TextNotifier,
    on_submit: TextNotifier,
    handler: RefCell<Option<glib::SignalHandlerId>>,
}

/// Single-line text input (`GtkEntry`).
#[derive(Clone)]
pub struct TextInput(Rc<TextInputInner>);
crate::keys::impl_key_down!(TextInput);
crate::focus::impl_focus_change!(TextInput);
impl_widget!(TextInput);

impl TextInput {
    pub(crate) fn new(text: &str) -> Self {
        let native = gtk::Entry::new();
        native.set_text(text);
        let bin = SizeBin::wrap(&native);
        let inner = Rc::new(TextInputInner {
            native,
            bin,
            on_change: TextNotifier::default(),
            on_submit: TextNotifier::default(),
            handler: RefCell::new(None),
        });
        let id = {
            let weak = Rc::downgrade(&inner);
            inner.native.connect_changed(move |native| {
                if let Some(inner) = weak.upgrade() {
                    inner.on_change.emit(native.text().as_str());
                }
            })
        };
        *inner.handler.borrow_mut() = Some(id);
        {
            let weak = Rc::downgrade(&inner);
            inner.native.connect_activate(move |native| {
                // 読み取り専用の間は書き換えられないので、確定もしない。
                if let Some(inner) = weak.upgrade().filter(|_| native.is_editable()) {
                    inner.on_submit.emit(native.text().as_str());
                }
            });
        }
        Self(inner)
    }

    pub fn text(&self) -> String {
        self.0.native.text().to_string()
    }

    /// Replaces the contents from code. `on_change` is not called.
    pub fn set_text(&self, text: &str) {
        without_signal(&self.0.native, &self.0.handler, || {
            self.0.native.set_text(text);
        });
    }

    pub fn set_placeholder(&self, text: &str) {
        self.0.native.set_placeholder_text(Some(text));
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.0.native.set_sensitive(enabled);
    }

    /// Called with the current contents every time the user types.
    pub fn on_change(&self, f: impl FnMut(&str) + 'static) {
        self.0.on_change.set(f);
    }

    /// Called with the current contents when the user confirms with Enter (the
    /// `activate` signal).
    ///
    /// Not called for an Enter pressed while an IME is composing (which confirms the
    /// conversion). Not called while read-only either.
    pub fn on_submit(&self, f: impl FnMut(&str) + 'static) {
        self.0.on_submit.set(f);
    }

    /// Whether it is read-only. Editable by default.
    ///
    /// While read-only, the text can still be selected and copied, and it still takes
    /// focus (unlike `set_enabled(false)`, it is not dimmed). Use it for fields that
    /// show logs or generated output.
    pub fn set_read_only(&self, read_only: bool) {
        self.0.native.set_editable(!read_only);
    }

    pub fn is_read_only(&self) -> bool {
        !self.0.native.is_editable()
    }
}

// ----------------------------------------------------------- PasswordInput

struct PasswordInputInner {
    native: gtk::PasswordEntry,
    bin: SizeBin,
    on_change: TextNotifier,
    on_submit: TextNotifier,
    handler: RefCell<Option<glib::SignalHandlerId>>,
}

/// Password input (`GtkPasswordEntry`).
///
/// The API is the same as [`TextInput`]; the only difference is that **typed
/// characters are masked**. `GtkPasswordEntry` has a button that temporarily
/// reveals the text (the peek icon), but it is not shown because it is not part of
/// what all four platforms share.
#[derive(Clone)]
pub struct PasswordInput(Rc<PasswordInputInner>);
crate::keys::impl_key_down!(PasswordInput);
crate::focus::impl_focus_change!(PasswordInput);
impl_widget!(PasswordInput);

impl PasswordInput {
    pub(crate) fn new() -> Self {
        let native = gtk::PasswordEntry::new();
        let bin = SizeBin::wrap(&native);
        let inner = Rc::new(PasswordInputInner {
            native,
            bin,
            on_change: TextNotifier::default(),
            on_submit: TextNotifier::default(),
            handler: RefCell::new(None),
        });
        let id = {
            let weak = Rc::downgrade(&inner);
            inner.native.connect_changed(move |native| {
                if let Some(inner) = weak.upgrade() {
                    inner.on_change.emit(native.text().as_str());
                }
            })
        };
        *inner.handler.borrow_mut() = Some(id);
        {
            let weak = Rc::downgrade(&inner);
            inner.native.connect_activate(move |native| {
                if let Some(inner) = weak.upgrade() {
                    inner.on_submit.emit(native.text().as_str());
                }
            });
        }
        Self(inner)
    }

    /// The text currently entered.
    pub fn text(&self) -> String {
        self.0.native.text().to_string()
    }

    /// Replaces the contents from code. `on_change` is not called.
    pub fn set_text(&self, text: &str) {
        without_signal(&self.0.native, &self.0.handler, || {
            self.0.native.set_text(text);
        });
    }

    pub fn set_placeholder(&self, text: &str) {
        self.0.native.set_placeholder_text(Some(text));
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.0.native.set_sensitive(enabled);
    }

    /// Called with the current contents every time the user types.
    pub fn on_change(&self, f: impl FnMut(&str) + 'static) {
        self.0.on_change.set(f);
    }

    /// Called with the current contents when the user confirms with Enter (the
    /// `activate` signal).
    pub fn on_submit(&self, f: impl FnMut(&str) + 'static) {
        self.0.on_submit.set(f);
    }
}

// ------------------------------------------------------------- SearchInput

struct SearchInputInner {
    native: gtk::SearchEntry,
    bin: SizeBin,
    on_change: TextNotifier,
    on_search: TextNotifier,
    handler: RefCell<Option<glib::SignalHandlerId>>,
}

/// A search field (`GtkSearchEntry`).
///
/// The magnifying glass and the clear button (✕) that appears once you start
/// typing are shown by GTK. `on_change` is called on every keystroke (the
/// `changed` signal), and `on_search` when the user confirms with Enter (the
/// `activate` signal). GtkSearchEntry also has `search-changed`, which batches
/// keystrokes, but its delay would put it out of step with the other platforms, so
/// it is not used.
#[derive(Clone)]
pub struct SearchInput(Rc<SearchInputInner>);
crate::keys::impl_key_down!(SearchInput);
crate::focus::impl_focus_change!(SearchInput);
impl_widget!(SearchInput);

impl SearchInput {
    pub(crate) fn new() -> Self {
        let native = gtk::SearchEntry::new();
        let bin = SizeBin::wrap(&native);
        let inner = Rc::new(SearchInputInner {
            native,
            bin,
            on_change: TextNotifier::default(),
            on_search: TextNotifier::default(),
            handler: RefCell::new(None),
        });
        let id = {
            let weak = Rc::downgrade(&inner);
            inner.native.connect_changed(move |native| {
                if let Some(inner) = weak.upgrade() {
                    inner.on_change.emit(native.text().as_str());
                }
            })
        };
        *inner.handler.borrow_mut() = Some(id);
        {
            let weak = Rc::downgrade(&inner);
            inner.native.connect_activate(move |native| {
                if let Some(inner) = weak.upgrade() {
                    inner.on_search.emit(native.text().as_str());
                }
            });
        }
        Self(inner)
    }

    /// The text currently entered.
    pub fn text(&self) -> String {
        self.0.native.text().to_string()
    }

    /// Replaces the contents from code. `on_change` is not called.
    pub fn set_text(&self, text: &str) {
        without_signal(&self.0.native, &self.0.handler, || {
            self.0.native.set_text(text);
        });
    }

    pub fn set_placeholder(&self, text: &str) {
        self.0.native.set_placeholder_text(Some(text));
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.0.native.set_sensitive(enabled);
    }

    /// Called with the current contents every time the user types.
    pub fn on_change(&self, f: impl FnMut(&str) + 'static) {
        self.0.on_change.set(f);
    }

    /// Called with the current contents when the user confirms with Enter.
    pub fn on_search(&self, f: impl FnMut(&str) + 'static) {
        self.0.on_search.set(f);
    }
}

// --------------------------------------------------------------- TextArea

struct TextAreaInner {
    native: gtk::TextView,
    /// `GtkTextView` does not scroll by itself, so it is placed in a scroll area.
    _scroller: gtk::ScrolledWindow,
    /// `GtkTextView` has no placeholder, so this label is overlaid only while it is
    /// empty.
    placeholder: gtk::Label,
    bin: SizeBin,
    on_change: TextNotifier,
    handler: RefCell<Option<glib::SignalHandlerId>>,
}

/// A field for entering text that includes newlines (`GtkTextView`).
///
/// Like a scroll view, its height does not follow its contents, so set it with
/// [`TextArea::set_sizing`].
#[derive(Clone)]
pub struct TextArea(Rc<TextAreaInner>);
crate::keys::impl_key_down!(TextArea);
crate::focus::impl_focus_change!(TextArea);
impl_widget!(TextArea);

impl TextArea {
    pub(crate) fn new(text: &str) -> Self {
        let native = gtk::TextView::new();
        native.set_wrap_mode(gtk::WrapMode::WordChar);
        native.buffer().set_text(text);

        let scroller = gtk::ScrolledWindow::new();
        scroller.set_policy(gtk::PolicyType::Automatic, gtk::PolicyType::Automatic);
        scroller.set_has_frame(true);
        scroller.set_child(Some(&native));

        // placeholder は入力の邪魔にならないよう、クリックを受けない。
        let placeholder = gtk::Label::new(None);
        placeholder.set_halign(gtk::Align::Start);
        placeholder.set_valign(gtk::Align::Start);
        placeholder.set_margin_top(3);
        placeholder.set_margin_start(3);
        placeholder.set_can_target(false);
        placeholder.set_visible(false);
        placeholder.add_css_class("dim-label");

        let overlay = gtk::Overlay::new();
        overlay.set_child(Some(&scroller));
        overlay.add_overlay(&placeholder);

        let bin = SizeBin::wrap(&overlay);
        let inner = Rc::new(TextAreaInner {
            native,
            _scroller: scroller,
            placeholder,
            bin,
            on_change: TextNotifier::default(),
            handler: RefCell::new(None),
        });

        // placeholder の出し入れは、アプリのクロージャとは別に常時つないでおく。
        // `set_text` で通知を止めても、表示だけは正しく追従する。
        {
            let placeholder = inner.placeholder.clone();
            inner.native.buffer().connect_changed(move |buffer| {
                placeholder.set_visible(is_empty(buffer) && !placeholder.text().is_empty());
            });
        }
        let id = {
            let weak = Rc::downgrade(&inner);
            inner.native.buffer().connect_changed(move |buffer| {
                if let Some(inner) = weak.upgrade() {
                    inner.on_change.emit(&buffer_text(buffer));
                }
            })
        };
        *inner.handler.borrow_mut() = Some(id);
        Self(inner)
    }

    pub fn text(&self) -> String {
        buffer_text(&self.0.native.buffer())
    }

    /// Replaces the contents from code. `on_change` is not called.
    pub fn set_text(&self, text: &str) {
        let buffer = self.0.native.buffer();
        without_signal(&buffer, &self.0.handler, || buffer.set_text(text));
    }

    pub fn set_placeholder(&self, text: &str) {
        self.0.placeholder.set_text(text);
        let empty = is_empty(&self.0.native.buffer());
        self.0.placeholder.set_visible(empty && !text.is_empty());
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.0.native.set_sensitive(enabled);
    }

    /// Called with the current contents every time the user types.
    pub fn on_change(&self, f: impl FnMut(&str) + 'static) {
        self.0.on_change.set(f);
    }

    /// Whether it is read-only. Editable by default.
    ///
    /// While read-only, the text can still be selected and copied, and it still takes
    /// focus (unlike `set_enabled(false)`, it is not dimmed). Use it for fields that
    /// show logs or generated output.
    pub fn set_read_only(&self, read_only: bool) {
        self.0.native.set_editable(!read_only);
        // 書き換えられない欄でカーソルが点滅すると、打てるように見える。
        self.0.native.set_cursor_visible(!read_only);
    }

    pub fn is_read_only(&self) -> bool {
        !self.0.native.is_editable()
    }
}

fn buffer_text(buffer: &gtk::TextBuffer) -> String {
    buffer
        .text(&buffer.start_iter(), &buffer.end_iter(), false)
        .to_string()
}

fn is_empty(buffer: &gtk::TextBuffer) -> bool {
    buffer.char_count() == 0
}

// ----------------------------------------------------------------- Slider

struct SliderInner {
    native: gtk::Scale,
    bin: SizeBin,
    on_change: Notifier<f64>,
    handler: RefCell<Option<glib::SignalHandlerId>>,
}

/// A slider (`GtkScale`).
#[derive(Clone)]
pub struct Slider(Rc<SliderInner>);
impl_widget!(Slider);

impl Slider {
    pub(crate) fn new(min: f64, max: f64) -> Self {
        // 刻みが 0 だと GtkScale が値を動かせないので、幅から決める。
        let step = if max > min { (max - min) / 100.0 } else { 1.0 };
        let native = gtk::Scale::with_range(gtk::Orientation::Horizontal, min, max, step);
        // 値の数字は naui の API に無いので出さない (NSSlider / `<input>` と同じ)。
        native.set_draw_value(false);
        // GtkScale の自然な幅はつまみ 1 個ぶんしかなく、そのままでは操作できない。
        native.set_width_request(160);
        let bin = SizeBin::wrap(&native);
        let inner = Rc::new(SliderInner {
            native,
            bin,
            on_change: Notifier::default(),
            handler: RefCell::new(None),
        });
        let id = {
            let weak = Rc::downgrade(&inner);
            inner.native.connect_value_changed(move |native| {
                if let Some(inner) = weak.upgrade() {
                    inner.on_change.emit(native.value());
                }
            })
        };
        *inner.handler.borrow_mut() = Some(id);
        Self(inner)
    }

    pub fn value(&self) -> f64 {
        self.0.native.value()
    }

    /// Changes the value from code. `on_change` is not called.
    pub fn set_value(&self, value: f64) {
        without_signal(&self.0.native, &self.0.handler, || {
            self.0.native.set_value(value);
        });
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.0.native.set_sensitive(enabled);
    }

    /// Called with the value every time the knob moves.
    pub fn on_change(&self, f: impl FnMut(f64) + 'static) {
        self.0.on_change.set(f);
    }
}

// ------------------------------------------------------------ ProgressBar

struct ProgressInner {
    native: gtk::ProgressBar,
    bin: SizeBin,
    /// The value that was set. While indeterminate, `pulse` moves `fraction`, so it is
    /// kept separately.
    value: Cell<f64>,
    /// The timer that keeps calling `pulse` while indeterminate.
    pulse: RefCell<Option<glib::SourceId>>,
}

impl Drop for ProgressInner {
    fn drop(&mut self) {
        if let Some(source) = self.pulse.borrow_mut().take() {
            source.remove();
        }
    }
}

/// The interval for animating indeterminate progress. Matches the interval at which
/// GNOME apps call `pulse`.
const PULSE_INTERVAL: std::time::Duration = std::time::Duration::from_millis(100);

/// A progress bar (`GtkProgressBar`).
#[derive(Clone)]
pub struct ProgressBar(Rc<ProgressInner>);
impl_widget!(ProgressBar);

impl ProgressBar {
    pub(crate) fn new() -> Self {
        let native = gtk::ProgressBar::new();
        native.set_fraction(0.0);
        let bin = SizeBin::wrap(&native);
        Self(Rc::new(ProgressInner {
            native,
            bin,
            value: Cell::new(0.0),
            pulse: RefCell::new(None),
        }))
    }

    /// 0.0..=1.0.
    pub fn set_value(&self, value: f64) {
        let value = value.clamp(0.0, 1.0);
        self.0.value.set(value);
        if !self.is_indeterminate() {
            self.0.native.set_fraction(value);
        }
    }

    pub fn value(&self) -> f64 {
        self.0.value.get()
    }

    /// Whether to show work whose progress is unknown (indeterminate progress).
    ///
    /// While `true`, motion shows "working" instead of a value. Turning it off returns
    /// to showing the value set with [`set_value`](Self::set_value) (the value is
    /// remembered).
    ///
    /// GTK4's progress bar moves each time `pulse` is called, so it is called at a
    /// fixed interval in the meantime.
    pub fn set_indeterminate(&self, indeterminate: bool) {
        if self.is_indeterminate() == indeterminate {
            return;
        }
        if indeterminate {
            let weak = Rc::downgrade(&self.0);
            let source = glib::timeout_add_local(PULSE_INTERVAL, move || {
                let Some(inner) = weak.upgrade() else {
                    return glib::ControlFlow::Break;
                };
                inner.native.pulse();
                glib::ControlFlow::Continue
            });
            *self.0.pulse.borrow_mut() = Some(source);
            self.0.native.pulse();
        } else {
            if let Some(source) = self.0.pulse.borrow_mut().take() {
                source.remove();
            }
            self.0.native.set_fraction(self.0.value.get());
        }
    }

    pub fn is_indeterminate(&self) -> bool {
        self.0.pulse.borrow().is_some()
    }
}

// ---------------------------------------------------------------- Spinner

struct SpinnerInner {
    /// `AdwSpinner`, or `GtkSpinner` when libadwaita is older than 1.6.
    native: gtk::Widget,
    bin: SizeBin,
    spinning: Cell<bool>,
}

/// A spinning indicator (`AdwSpinner`, or `GtkSpinner` with libadwaita older than
/// 1.6).
#[derive(Clone)]
pub struct Spinner(Rc<SpinnerInner>);
impl_widget!(Spinner);

impl Spinner {
    pub(crate) fn new() -> Self {
        // AdwSpinner is available from libadwaita 1.6; naui also supports 1.5.
        let native_type = glib::Type::from_name("AdwSpinner")
            .filter(|ty| ty.is_a(gtk::Widget::static_type()))
            .unwrap_or_else(gtk::Spinner::static_type);
        Self::from_type(native_type)
    }

    fn from_type(native_type: glib::Type) -> Self {
        // Older GTK / libadwaita versions do not give their spinner this role.
        // Set it at construction so set_accessible_label names this same control.
        let native = glib::Object::builder_with_type(native_type)
            .property("accessible-role", gtk::AccessibleRole::ProgressBar)
            .build()
            .downcast::<gtk::Widget>()
            .expect("spinner must be a GTK widget");
        let fallback = native.downcast_ref::<gtk::Spinner>();
        if let Some(spinner) = fallback {
            spinner.set_spinning(true);
        }
        native.update_state(&[gtk::accessible::State::Busy(true)]);
        let bin = SizeBin::wrap(&native);
        bin.mark_scale_below_minimum();
        if fallback.is_some() {
            // AdwSpinner draws at most 64×64 even when its allocation is larger.
            bin.cap_spinner_size(64);
        }
        Self(Rc::new(SpinnerInner {
            native,
            bin,
            spinning: Cell::new(true),
        }))
    }

    /// Whether it spins. Spins from creation.
    ///
    /// While stopped it is not drawn but keeps its space. `GtkSpinner` turns off
    /// `spinning`; `AdwSpinner` has no such switch and spins whenever it is mapped, so
    /// it is unmapped with `child-visible` (its size still counts in the layout). Use
    /// `set_visible(false)` to give the space up as well.
    pub fn set_spinning(&self, spinning: bool) {
        if self.0.spinning.replace(spinning) == spinning {
            return;
        }
        match self.0.native.downcast_ref::<gtk::Spinner>() {
            Some(spinner) => spinner.set_spinning(spinning),
            None => self.0.native.set_child_visible(spinning),
        }
        self.0
            .native
            .update_state(&[gtk::accessible::State::Busy(spinning)]);
    }

    pub fn is_spinning(&self) -> bool {
        self.0.spinning.get()
    }
}

// ------------------------------------------------------------------ Stack

struct StackInner {
    native: gtk::Box,
    bin: SizeBin,
    vertical: Cell<bool>,
    align: Cell<Align>,
    /// Holds the children's handles, keeping them alive along with their closures.
    children: RefCell<Vec<Box<dyn Widget>>>,
    /// The orientation the app gave and the breakpoint, which decide the orientation
    /// the children are laid out in.
    arrangement: Cell<StackArrangement>,
    /// Compares the width after each paint while there is a breakpoint.
    after_paint: RefCell<Option<(gtk::gdk::FrameClock, glib::SignalHandlerId)>>,
    /// Watches for being realized, to connect `after_paint` to the frame clock.
    realize_handlers: RefCell<Vec<glib::SignalHandlerId>>,
    /// Switching to the orientation `arrangement` decided is queued on the main loop.
    switch_scheduled: Cell<bool>,
}

impl Drop for StackInner {
    fn drop(&mut self) {
        if let Some((clock, id)) = self.after_paint.get_mut().take() {
            clock.disconnect(id);
        }
        for id in self.realize_handlers.get_mut().drain(..) {
            self.native.disconnect(id);
        }
    }
}

/// A container that lays out its children vertically / horizontally (`GtkBox`).
#[derive(Clone)]
pub struct Stack(Rc<StackInner>);
impl_widget!(Stack);

impl Stack {
    pub(crate) fn new(orientation: Orientation) -> Self {
        let vertical = orientation.is_vertical();
        let native = gtk::Box::new(to_gtk_orientation(orientation), 0);
        let bin = SizeBin::wrap(&native);
        Self(Rc::new(StackInner {
            native,
            bin,
            vertical: Cell::new(vertical),
            align: Cell::new(Align::default()),
            children: RefCell::new(Vec::new()),
            arrangement: Cell::new(StackArrangement::new(orientation)),
            after_paint: RefCell::new(None),
            realize_handlers: RefCell::new(Vec::new()),
            switch_scheduled: Cell::new(false),
        }))
    }

    /// Switches between laying out vertically and horizontally (`GtkOrientable`).
    ///
    /// The children stay in the box. Each one is placed again for the new cross axis,
    /// so the alignment the previous direction gave it does not carry over.
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
    /// `AdwBreakpointBin` would switch the box by itself, but it takes away the minimum
    /// size of what it holds (the app would have to give it one, and content wider than
    /// that overflows). So naui compares the width after each paint, the same as
    /// `Sidebar`'s automatic collapsing, and switches on the next main loop iteration
    /// (not while GTK is laying out).
    pub fn set_breakpoint(&self, breakpoint: Option<Breakpoint>) {
        let mut arrangement = self.0.arrangement.get();
        arrangement.resized(self.outer_width());
        arrangement.set_breakpoint(breakpoint);
        self.0.arrangement.set(arrangement);
        if breakpoint.is_some() {
            self.watch_width();
        }
        self.arrange(arrangement.orientation());
    }

    pub fn breakpoint(&self) -> Option<Breakpoint> {
        self.0.arrangement.get().breakpoint()
    }

    /// The orientation the children are laid out in now.
    pub fn current_orientation(&self) -> Orientation {
        if self.0.vertical.get() {
            Orientation::Vertical
        } else {
            Orientation::Horizontal
        }
    }

    /// The box's width including its padding (the margins naui puts on it).
    fn outer_width(&self) -> f64 {
        let native = &self.0.native;
        f64::from(native.width() + native.margin_start() + native.margin_end())
    }

    /// Starts comparing the width after each paint. Reconnects whenever the box is
    /// realized again, since the frame clock changes with the window.
    fn watch_width(&self) {
        if !self.0.realize_handlers.borrow().is_empty() {
            return;
        }
        let weak = Rc::downgrade(&self.0);
        let realize = self.0.native.connect_realize(move |_| {
            if let Some(inner) = weak.upgrade() {
                Stack(inner).connect_clock();
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
        if self.0.native.is_realized() {
            self.connect_clock();
        }
    }

    /// Connects `check_width` to the frame clock of the window the box is in.
    fn connect_clock(&self) {
        let Some(clock) = self.0.native.frame_clock() else {
            return;
        };
        let weak = Rc::downgrade(&self.0);
        let handler = clock.connect_after_paint(move |_| {
            if let Some(inner) = weak.upgrade() {
                Stack(inner).check_width();
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
        let changed = arrangement.resized(self.outer_width());
        self.0.arrangement.set(arrangement);
        if !changed || self.0.switch_scheduled.replace(true) {
            return;
        }
        let weak = Rc::downgrade(&self.0);
        glib::idle_add_local_once(move || {
            if let Some(inner) = weak.upgrade() {
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
        if self.0.vertical.replace(vertical) == vertical {
            return;
        }
        self.0
            .native
            .set_orientation(to_gtk_orientation(orientation));
        for child in self.0.children.borrow().iter() {
            child
                .size_bin()
                .set_cross_align(self.0.align.get(), vertical);
        }
    }

    pub fn set_spacing(&self, spacing: f64) {
        self.0
            .native
            .set_spacing(spacing.round().clamp(0.0, i32::MAX as f64) as i32);
    }

    pub fn set_padding(&self, padding: Padding) {
        apply_padding(&self.0.native, padding);
    }

    /// Alignment on the cross axis. The default is [`Align::Center`].
    ///
    /// Children with [`Length::Fill`](naui_core::Length::Fill) on the cross axis follow
    /// their own setting instead.
    pub fn set_align(&self, align: Align) {
        self.0.align.set(align);
        for child in self.0.children.borrow().iter() {
            child
                .size_bin()
                .set_cross_align(align, self.0.vertical.get());
        }
    }

    pub fn append(&self, child: &dyn Widget) {
        let bin = child.size_bin();
        bin.set_cross_align(self.0.align.get(), self.0.vertical.get());
        self.0.native.append(&bin);
        self.0.children.borrow_mut().push(child.boxed_clone());
    }

    /// Inserts a child at the given position. If `index` is at least the current
    /// count, it is appended at the end.
    pub fn insert(&self, index: usize, child: &dyn Widget) {
        let mut children = self.0.children.borrow_mut();
        let index = index.min(children.len());
        let bin = child.size_bin();
        bin.set_cross_align(self.0.align.get(), self.0.vertical.get());
        // `GtkBox` は「この子の後ろへ」で場所を指す。先頭は `None`。
        let after = index
            .checked_sub(1)
            .and_then(|previous| children.get(previous))
            .map(|previous| previous.size_bin());
        self.0.native.insert_child_after(&bin, after.as_ref());
        children.insert(index, child.boxed_clone());
    }

    /// Removes the child at the given position. Does nothing if out of range.
    pub fn remove(&self, index: usize) {
        let child = {
            let mut children = self.0.children.borrow_mut();
            if index >= children.len() {
                return;
            }
            children.remove(index)
        };
        let bin = child.size_bin();
        crate::interaction::clear_focus(&bin);
        self.0.native.remove(&bin);
        bin.set_placement(crate::bin::Placement::Loose);
    }

    /// Removes every child.
    pub fn clear(&self) {
        let children = std::mem::take(&mut *self.0.children.borrow_mut());
        for child in children {
            let bin = child.size_bin();
            crate::interaction::clear_focus(&bin);
            self.0.native.remove(&bin);
            bin.set_placement(crate::bin::Placement::Loose);
        }
    }

    pub fn len(&self) -> usize {
        self.0.children.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

fn to_gtk_orientation(orientation: Orientation) -> gtk::Orientation {
    if orientation.is_vertical() {
        gtk::Orientation::Vertical
    } else {
        gtk::Orientation::Horizontal
    }
}

#[cfg(test)]
mod spinner_fallback_tests {
    use super::*;

    #[test]
    fn fallback_keeps_its_role_and_caps_drawing_without_capping_layout() {
        crate::run_for_test(|_| {
            let spinner = Spinner::from_type(gtk::Spinner::static_type());
            let native = spinner.native_widget();
            let bin = spinner.size_bin();
            assert_eq!(native.accessible_role(), gtk::AccessibleRole::ProgressBar);
            spinner.set_accessible_label(Some("Loading"));
            if std::env::var("GTK_A11Y").as_deref() != Ok("none") {
                assert!(gtk::test_accessible_has_property(
                    &native,
                    gtk::AccessibleProperty::Label
                ));
                assert!(gtk::test_accessible_has_state(
                    &native,
                    gtk::AccessibleState::Busy
                ));
            }
            spinner.set_spinning(false);
            assert!(!spinner.is_spinning());
            spinner.set_spinning(true);
            assert!(spinner.is_spinning());

            spinner.set_sizing(naui_core::Sizing::fixed(128.0, 128.0));
            bin.allocate(128, 128, -1, None);
            let bounds = native.compute_bounds(&bin).expect("fallback bounds");
            assert_eq!((bin.width(), bin.height()), (128, 128));
            assert_eq!(
                (bounds.x(), bounds.y(), bounds.width(), bounds.height()),
                (32.0, 32.0, 64.0, 64.0)
            );
            spinner.set_sizing(naui_core::Sizing::fixed(8.0, 8.0));
            bin.allocate(8, 8, -1, None);
            let bounds = native.compute_bounds(&bin).expect("small fallback bounds");
            assert_eq!((bounds.width(), bounds.height()), (8.0, 8.0));
            Ok(())
        })
        .unwrap();
    }
}
