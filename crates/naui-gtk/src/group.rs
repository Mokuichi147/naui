//! Dividing and grouping controls (`Separator` = `GtkSeparator`, `Group` =
//! `GtkFrame`).
//!
//! GTK4 has controls for both, so they are used as is. The line's color and the
//! frame's border and rounded corners come from the theme (libadwaita).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use naui_core::{Orientation, Padding, Sizing};

use crate::bin::{apply_physical_padding, SizeBin};
use crate::widgets::{impl_widget, Widget};

// -------------------------------------------------------------- Separator

struct SeparatorInner {
    native: gtk::Separator,
    bin: SizeBin,
}

/// A thin line that divides controls (`GtkSeparator`).
#[derive(Clone)]
pub struct Separator(Rc<SeparatorInner>);
impl_widget!(Separator);

impl Separator {
    pub(crate) fn new(orientation: Orientation) -> Self {
        let native = gtk::Separator::new(if orientation.is_vertical() {
            gtk::Orientation::Vertical
        } else {
            gtk::Orientation::Horizontal
        });
        let bin = SizeBin::wrap(&native);
        // 線は引いた向きいっぱいに伸ばす。太さは GTK4 のテーマが決める。
        bin.apply_sizing(default_separator_sizing(orientation));
        Self(Rc::new(SeparatorInner { native, bin }))
    }
}

/// A line runs the full length of its direction: a horizontal line fills the width,
/// a vertical one the height.
pub(crate) fn default_separator_sizing(orientation: Orientation) -> Sizing {
    if orientation.is_vertical() {
        Sizing::fill_height()
    } else {
        Sizing::fill_width()
    }
}

// ------------------------------------------------------------------ Group

struct GroupInner {
    native: gtk::Frame,
    bin: SizeBin,
    /// Holds the content inside the frame. The padding is its margin, so that moving
    /// the content elsewhere does not carry the padding with it.
    holder: adw::Bin,
    /// Kept to swap the start and end margins when the text direction changes.
    padding: Cell<Padding>,
    /// Holds the content's handle, keeping it alive along with its callbacks.
    child: RefCell<Option<Box<dyn Widget>>>,
}

/// A frame with a heading around related controls (`GtkFrame`).
#[derive(Clone)]
pub struct Group(Rc<GroupInner>);
impl_widget!(Group);

impl Group {
    pub(crate) fn new(text: &str) -> Self {
        let native = gtk::Frame::new(None);
        let holder = adw::Bin::new();
        apply_physical_padding(&holder, naui_core::DEFAULT_GROUP_PADDING);
        native.set_child(Some(&holder));
        let bin = SizeBin::wrap(&native);
        let this = Self(Rc::new(GroupInner {
            native,
            bin,
            holder,
            padding: Cell::new(naui_core::DEFAULT_GROUP_PADDING),
            child: RefCell::new(None),
        }));
        this.set_text(text);

        // 余白の左右は物理的な左右なので、文字の向きが変わったら start と end を入れ替える。
        let weak = Rc::downgrade(&this.0);
        this.0.holder.connect_direction_changed(move |holder, _| {
            if let Some(inner) = weak.upgrade() {
                apply_physical_padding(holder, inner.padding.get());
            }
        });
        this
    }

    /// The heading text. Empty when there is none.
    pub fn text(&self) -> String {
        self.0
            .native
            .label()
            .map(|label| label.to_string())
            .unwrap_or_default()
    }

    /// The heading text. An empty string removes the heading, leaving only the frame.
    pub fn set_text(&self, text: &str) {
        self.0
            .native
            .set_label(if text.is_empty() { None } else { Some(text) });
    }

    /// The space between the frame and the content. The default is
    /// [`DEFAULT_GROUP_PADDING`](naui_core::DEFAULT_GROUP_PADDING).
    ///
    /// `left` and `right` are the physical sides, also in right-to-left text.
    pub fn set_padding(&self, padding: Padding) {
        self.0.padding.set(padding);
        apply_physical_padding(&self.0.holder, padding);
    }

    /// The content inside the frame. Each call replaces it.
    ///
    /// The content fills the inside of the frame.
    ///
    /// A widget still in another container is refused, as GTK refuses it, and the
    /// previous content stays.
    pub fn set_child(&self, child: &dyn Widget) {
        let bin = child.size_bin();
        let previous = self.0.child.borrow().as_ref().map(|child| child.size_bin());
        let mine = previous.as_ref() == Some(&bin);
        if crate::handover::refuses(&bin, mine, "Group::set_child") {
            return;
        }
        if let Some(previous) = previous {
            if previous != bin {
                crate::interaction::clear_focus(&previous);
            }
        }
        bin.fill_parent();
        self.0.holder.set_child(Some(&bin));
        *self.0.child.borrow_mut() = Some(child.boxed_clone());
    }
}
