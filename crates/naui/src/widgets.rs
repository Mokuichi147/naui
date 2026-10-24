use crate::sys;
use crate::Widget;
use naui_core::{Align, EventResponse, KeyEvent, Padding, TextColor, TextStyle};

/// A push button (`GtkButton`).
#[derive(Clone)]
pub struct Button(pub(crate) sys::Button);

impl_widget!(Button);

impl Button {
    #[inline]
    pub fn set_text(&self, text: &str) {
        self.0.set_text(text)
    }

    #[inline]
    pub fn set_enabled(&self, enabled: bool) {
        self.0.set_enabled(enabled)
    }

    /// Called on every click. Calling it again removes the previous one.
    #[inline]
    pub fn on_click(&self, f: impl FnMut() + 'static) {
        self.0.on_click(f)
    }
}

/// A check box (`GtkCheckButton`).
#[derive(Clone)]
pub struct Checkbox(pub(crate) sys::Checkbox);

impl_widget!(Checkbox);

impl Checkbox {
    #[inline]
    pub fn is_checked(&self) -> bool {
        self.0.is_checked()
    }

    /// Changes the state from code. `on_toggle` is not called.
    #[inline]
    pub fn set_checked(&self, checked: bool) {
        self.0.set_checked(checked)
    }

    #[inline]
    pub fn set_enabled(&self, enabled: bool) {
        self.0.set_enabled(enabled)
    }

    /// Called with the new state every time the user toggles it.
    #[inline]
    pub fn on_toggle(&self, f: impl FnMut(bool) + 'static) {
        self.0.on_toggle(f)
    }
}

/// Non-editable text (`GtkLabel`).
#[derive(Clone)]
pub struct Label(pub(crate) sys::Label);

impl_widget!(Label);

impl Label {
    #[inline]
    pub fn text(&self) -> String {
        self.0.text()
    }

    #[inline]
    pub fn set_text(&self, text: &str) {
        self.0.set_text(text)
    }

    /// Whether the text can be selected and copied. Not selectable by default.
    ///
    /// Use it for text readers will want to copy, such as chat messages or error
    /// details. Unlike a text field, the text cannot be edited.
    #[inline]
    pub fn set_selectable(&self, selectable: bool) {
        self.0.set_selectable(selectable)
    }

    #[inline]
    pub fn is_selectable(&self) -> bool {
        self.0.is_selectable()
    }

    /// Whether long text wraps. Does not wrap by default.
    ///
    /// When not wrapping, it stays on one line and whatever does not fit is cut off
    /// with a trailing ellipsis (…) (`End` of `PangoEllipsizeMode`). **The ellipsis also
    /// makes the `GtkLabel`'s minimum width smaller**, so putting it into a narrow
    /// container no longer pushes the whole container wider.
    #[inline]
    pub fn set_wrap(&self, wrap: bool) {
        self.0.set_wrap(wrap)
    }

    /// The level of text size and weight. The default is [`TextStyle::Body`].
    ///
    /// It only applies libadwaita's style classes (`.title-1` and so on), so the theme
    /// decides the point size and weight. It also follows the desktop's font settings
    /// as is.
    #[inline]
    pub fn set_style(&self, style: TextStyle) {
        self.0.set_style(style)
    }

    /// The text color role. The default is [`TextColor::Default`].
    ///
    /// The theme owns the colors themselves (`.accent` is the desktop's accent color).
    /// The theme also follows light / dark switching.
    #[inline]
    pub fn set_color(&self, color: TextColor) {
        self.0.set_color(color)
    }
}

/// Password input (`GtkPasswordEntry`).
///
/// The API is the same as [`TextInput`]; the only difference is that **typed
/// characters are masked**. `GtkPasswordEntry` has a button that temporarily
/// reveals the text (the peek icon), but it is not shown because it is not part of
/// what all four platforms share.
#[derive(Clone)]
pub struct PasswordInput(pub(crate) sys::PasswordInput);

impl_widget!(PasswordInput);

impl PasswordInput {
    /// Notified when a key is pressed. Returning `Handled` keeps the key from reaching
    /// the field and the window (so you can stop newlines or confirmation).
    ///
    /// Keys pressed while an IME is composing are not delivered.
    #[inline]
    pub fn on_key_down(&self, f: impl FnMut(&KeyEvent) -> EventResponse + 'static) {
        self.0.on_key_down(f)
    }

    /// Notified when focus enters (`true`) and leaves (`false`).
    ///
    /// Called only when focus moves within the window. Not called when the window
    /// itself becomes inactive.
    #[inline]
    pub fn on_focus_change(&self, f: impl FnMut(bool) + 'static) {
        self.0.on_focus_change(f)
    }

    /// The text currently entered.
    #[inline]
    pub fn text(&self) -> String {
        self.0.text()
    }

    /// Replaces the contents from code. `on_change` is not called.
    #[inline]
    pub fn set_text(&self, text: &str) {
        self.0.set_text(text)
    }

    #[inline]
    pub fn set_placeholder(&self, text: &str) {
        self.0.set_placeholder(text)
    }

    #[inline]
    pub fn set_enabled(&self, enabled: bool) {
        self.0.set_enabled(enabled)
    }

    /// Called with the current contents every time the user types.
    #[inline]
    pub fn on_change(&self, f: impl FnMut(&str) + 'static) {
        self.0.on_change(f)
    }

    /// Called with the current contents when the user confirms with Enter (the
    /// `activate` signal).
    #[inline]
    pub fn on_submit(&self, f: impl FnMut(&str) + 'static) {
        self.0.on_submit(f)
    }
}

/// A progress bar (`GtkProgressBar`).
#[derive(Clone)]
pub struct ProgressBar(pub(crate) sys::ProgressBar);

impl_widget!(ProgressBar);

impl ProgressBar {
    /// 0.0..=1.0.
    #[inline]
    pub fn set_value(&self, value: f64) {
        self.0.set_value(value)
    }

    #[inline]
    pub fn value(&self) -> f64 {
        self.0.value()
    }

    /// Whether to show work whose progress is unknown (indeterminate progress).
    ///
    /// While `true`, motion shows "working" instead of a value. Turning it off returns
    /// to showing the value set with [`set_value`](Self::set_value) (the value is
    /// remembered).
    ///
    /// GTK4's progress bar moves each time `pulse` is called, so it is called at a
    /// fixed interval in the meantime.
    #[inline]
    pub fn set_indeterminate(&self, indeterminate: bool) {
        self.0.set_indeterminate(indeterminate)
    }

    #[inline]
    pub fn is_indeterminate(&self) -> bool {
        self.0.is_indeterminate()
    }
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
pub struct SearchInput(pub(crate) sys::SearchInput);

impl_widget!(SearchInput);

impl SearchInput {
    /// Notified when a key is pressed. Returning `Handled` keeps the key from reaching
    /// the field and the window (so you can stop newlines or confirmation).
    ///
    /// Keys pressed while an IME is composing are not delivered.
    #[inline]
    pub fn on_key_down(&self, f: impl FnMut(&KeyEvent) -> EventResponse + 'static) {
        self.0.on_key_down(f)
    }

    /// Notified when focus enters (`true`) and leaves (`false`).
    ///
    /// Called only when focus moves within the window. Not called when the window
    /// itself becomes inactive.
    #[inline]
    pub fn on_focus_change(&self, f: impl FnMut(bool) + 'static) {
        self.0.on_focus_change(f)
    }

    /// The text currently entered.
    #[inline]
    pub fn text(&self) -> String {
        self.0.text()
    }

    /// Replaces the contents from code. `on_change` is not called.
    #[inline]
    pub fn set_text(&self, text: &str) {
        self.0.set_text(text)
    }

    #[inline]
    pub fn set_placeholder(&self, text: &str) {
        self.0.set_placeholder(text)
    }

    #[inline]
    pub fn set_enabled(&self, enabled: bool) {
        self.0.set_enabled(enabled)
    }

    /// Called with the current contents every time the user types.
    #[inline]
    pub fn on_change(&self, f: impl FnMut(&str) + 'static) {
        self.0.on_change(f)
    }

    /// Called with the current contents when the user confirms with Enter.
    #[inline]
    pub fn on_search(&self, f: impl FnMut(&str) + 'static) {
        self.0.on_search(f)
    }
}

/// A slider (`GtkScale`).
#[derive(Clone)]
pub struct Slider(pub(crate) sys::Slider);

impl_widget!(Slider);

impl Slider {
    #[inline]
    pub fn value(&self) -> f64 {
        self.0.value()
    }

    /// Changes the value from code. `on_change` is not called.
    #[inline]
    pub fn set_value(&self, value: f64) {
        self.0.set_value(value)
    }

    #[inline]
    pub fn set_enabled(&self, enabled: bool) {
        self.0.set_enabled(enabled)
    }

    /// Called with the value every time the knob moves.
    #[inline]
    pub fn on_change(&self, f: impl FnMut(f64) + 'static) {
        self.0.on_change(f)
    }
}

/// A container that lays out its children vertically / horizontally (`GtkBox`).
#[derive(Clone)]
pub struct Stack(pub(crate) sys::Stack);

impl_widget!(Stack);

impl Stack {
    #[inline]
    pub fn set_spacing(&self, spacing: f64) {
        self.0.set_spacing(spacing)
    }

    #[inline]
    pub fn set_padding(&self, padding: Padding) {
        self.0.set_padding(padding)
    }

    /// Alignment on the cross axis. The default is [`Align::Center`].
    ///
    /// Children with [`Length::Fill`](naui_core::Length::Fill) on the cross axis follow
    /// their own setting instead.
    #[inline]
    pub fn set_align(&self, align: Align) {
        self.0.set_align(align)
    }

    #[inline]
    pub fn append(&self, child: &dyn Widget) {
        self.0.append(child.as_backend())
    }

    /// Inserts a child at the given position. If `index` is at least the current
    /// count, it is appended at the end.
    #[inline]
    pub fn insert(&self, index: usize, child: &dyn Widget) {
        self.0.insert(index, child.as_backend())
    }

    /// Removes the child at the given position. Does nothing if out of range.
    #[inline]
    pub fn remove(&self, index: usize) {
        self.0.remove(index)
    }

    /// Removes every child.
    #[inline]
    pub fn clear(&self) {
        self.0.clear()
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// A field for entering text that includes newlines (`GtkTextView`).
///
/// Like a scroll view, its height does not follow its contents, so set it with
/// [`TextArea::set_sizing`].
#[derive(Clone)]
pub struct TextArea(pub(crate) sys::TextArea);

impl_widget!(TextArea);

impl TextArea {
    /// Notified when a key is pressed. Returning `Handled` keeps the key from reaching
    /// the field and the window (so you can stop newlines or confirmation).
    ///
    /// Keys pressed while an IME is composing are not delivered.
    #[inline]
    pub fn on_key_down(&self, f: impl FnMut(&KeyEvent) -> EventResponse + 'static) {
        self.0.on_key_down(f)
    }

    /// Notified when focus enters (`true`) and leaves (`false`).
    ///
    /// Called only when focus moves within the window. Not called when the window
    /// itself becomes inactive.
    #[inline]
    pub fn on_focus_change(&self, f: impl FnMut(bool) + 'static) {
        self.0.on_focus_change(f)
    }

    #[inline]
    pub fn text(&self) -> String {
        self.0.text()
    }

    /// Replaces the contents from code. `on_change` is not called.
    #[inline]
    pub fn set_text(&self, text: &str) {
        self.0.set_text(text)
    }

    #[inline]
    pub fn set_placeholder(&self, text: &str) {
        self.0.set_placeholder(text)
    }

    #[inline]
    pub fn set_enabled(&self, enabled: bool) {
        self.0.set_enabled(enabled)
    }

    /// Called with the current contents every time the user types.
    #[inline]
    pub fn on_change(&self, f: impl FnMut(&str) + 'static) {
        self.0.on_change(f)
    }

    /// Whether it is read-only. Editable by default.
    ///
    /// While read-only, the text can still be selected and copied, and it still takes
    /// focus (unlike `set_enabled(false)`, it is not dimmed). Use it for fields that
    /// show logs or generated output.
    #[inline]
    pub fn set_read_only(&self, read_only: bool) {
        self.0.set_read_only(read_only)
    }

    #[inline]
    pub fn is_read_only(&self) -> bool {
        self.0.is_read_only()
    }
}

/// Single-line text input (`GtkEntry`).
#[derive(Clone)]
pub struct TextInput(pub(crate) sys::TextInput);

impl_widget!(TextInput);

impl TextInput {
    /// Notified when a key is pressed. Returning `Handled` keeps the key from reaching
    /// the field and the window (so you can stop newlines or confirmation).
    ///
    /// Keys pressed while an IME is composing are not delivered.
    #[inline]
    pub fn on_key_down(&self, f: impl FnMut(&KeyEvent) -> EventResponse + 'static) {
        self.0.on_key_down(f)
    }

    /// Notified when focus enters (`true`) and leaves (`false`).
    ///
    /// Called only when focus moves within the window. Not called when the window
    /// itself becomes inactive.
    #[inline]
    pub fn on_focus_change(&self, f: impl FnMut(bool) + 'static) {
        self.0.on_focus_change(f)
    }

    #[inline]
    pub fn text(&self) -> String {
        self.0.text()
    }

    /// Replaces the contents from code. `on_change` is not called.
    #[inline]
    pub fn set_text(&self, text: &str) {
        self.0.set_text(text)
    }

    #[inline]
    pub fn set_placeholder(&self, text: &str) {
        self.0.set_placeholder(text)
    }

    #[inline]
    pub fn set_enabled(&self, enabled: bool) {
        self.0.set_enabled(enabled)
    }

    /// Called with the current contents every time the user types.
    #[inline]
    pub fn on_change(&self, f: impl FnMut(&str) + 'static) {
        self.0.on_change(f)
    }

    /// Called with the current contents when the user confirms with Enter (the
    /// `activate` signal).
    ///
    /// Not called for an Enter pressed while an IME is composing (which confirms the
    /// conversion). Not called while read-only either.
    #[inline]
    pub fn on_submit(&self, f: impl FnMut(&str) + 'static) {
        self.0.on_submit(f)
    }

    /// Whether it is read-only. Editable by default.
    ///
    /// While read-only, the text can still be selected and copied, and it still takes
    /// focus (unlike `set_enabled(false)`, it is not dimmed). Use it for fields that
    /// show logs or generated output.
    #[inline]
    pub fn set_read_only(&self, read_only: bool) {
        self.0.set_read_only(read_only)
    }

    #[inline]
    pub fn is_read_only(&self) -> bool {
        self.0.is_read_only()
    }
}
