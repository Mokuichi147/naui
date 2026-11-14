use crate::sys;
use crate::Widget;
use naui_core::Padding;

/// A thin line that divides controls (`GtkSeparator`).
///
/// A horizontal line fills the width and a vertical one the height by default;
/// [`set_sizing`](Self::set_sizing) replaces that.
#[derive(Clone)]
pub struct Separator(pub(crate) sys::Separator);

impl_widget!(Separator);

/// A frame with a heading around related controls (`GtkFrame`).
///
/// Use it to group the controls of a settings screen, or the content of a
/// [`Popover`](crate::Popover) or [`Sheet`](crate::Sheet). Windows has no control
/// for this, so naui builds it from the Fluent card look (see
/// [Separators and groups](crate::guide#separators-and-groups)).
#[derive(Clone)]
pub struct Group(pub(crate) sys::Group);

impl_widget!(Group);

impl Group {
    /// The heading text. Empty when there is none.
    #[inline]
    pub fn text(&self) -> String {
        self.0.text()
    }

    /// The heading text. An empty string removes the heading, leaving only the frame.
    #[inline]
    pub fn set_text(&self, text: &str) {
        self.0.set_text(text)
    }

    /// The space between the frame and the content. The default is
    /// [`DEFAULT_GROUP_PADDING`](naui_core::DEFAULT_GROUP_PADDING).
    ///
    /// `start` is the side where the content starts: the right in a right-to-left
    /// layout.
    #[inline]
    pub fn set_padding(&self, padding: Padding) {
        self.0.set_padding(padding)
    }

    /// The content inside the frame. Each call replaces it.
    ///
    /// The content fills the inside of the frame. To put several controls in a group,
    /// put them in a [`Stack`](crate::Stack) and set that.
    #[inline]
    pub fn set_child(&self, child: &dyn Widget) {
        self.0.set_child(child.as_backend())
    }
}
