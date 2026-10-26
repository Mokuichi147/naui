//! Operations every widget has (show/hide / focus / tooltip).
//!
//! Showing and hiding switches the whole [`SizeBin`] placed in the parent. Hiding
//! only the content would leave the container's padding and size settings taking
//! up space. Focus and tooltips are attached to the content control.

use gtk::prelude::*;

use crate::bin::SizeBin;

pub(crate) fn set_visible(bin: &SizeBin, visible: bool) {
    bin.set_visible(visible);
}

pub(crate) fn is_visible(bin: &SizeBin) -> bool {
    bin.is_visible()
}

/// End focus before unparenting. GTK defers its own focus cleanup until painting;
/// reattaching the subtree in the same turn can otherwise keep the old focus.
pub(crate) fn clear_focus(widget: &impl IsA<gtk::Widget>) {
    let widget = widget.as_ref();
    if let Some(root) = widget.root() {
        if root
            .focus()
            .is_some_and(|focus| focus == *widget || focus.is_ancestor(widget))
        {
            root.set_focus(None::<&gtk::Widget>);
        }
    }
}

/// Moves keyboard focus here. `true` if it moved.
///
/// If the widget itself cannot take focus, focus moves to the first descendant
/// that can. It cannot move while not shown (before the window appears, or while
/// hidden).
pub(crate) fn request_focus(widget: &gtk::Widget) -> bool {
    if !widget.is_mapped() {
        return false;
    }
    grab_first(widget)
}

fn grab_first(widget: &gtk::Widget) -> bool {
    if !widget.is_visible() || !widget.is_sensitive() {
        return false;
    }
    if widget.grab_focus() {
        return true;
    }
    let mut child = widget.first_child();
    while let Some(current) = child {
        if grab_first(&current) {
            return true;
        }
        child = current.next_sibling();
    }
    false
}

/// The description shown when the pointer hovers over it. `None` removes it.
pub(crate) fn set_tooltip(widget: &gtk::Widget, text: Option<&str>) {
    widget.set_tooltip_text(text);
}

/// The name reported to screen readers (`GTK_ACCESSIBLE_PROPERTY_LABEL`). `None`
/// removes it.
///
/// A `GtkButton` with text (toggles and links too) has `LABELLED_BY` set so that
/// it takes its name from the label inside. For names, `LABELLED_BY` takes
/// precedence over `LABEL` (the same as ARIA's aria-labelledby and aria-label), so
/// while a name is set that relation is removed, and when it is removed the
/// relation to the inner label is restored.
pub(crate) fn set_accessible_label(widget: &gtk::Widget, text: Option<&str>) {
    let label_child = widget
        .downcast_ref::<gtk::Button>()
        .and_then(|button| button.child())
        .filter(|child| child.is::<gtk::Label>());
    match text {
        Some(text) => {
            widget.update_property(&[gtk::accessible::Property::Label(text)]);
            if label_child.is_some() {
                widget.reset_relation(gtk::AccessibleRelation::LabelledBy);
            }
        }
        None => {
            widget.reset_property(gtk::AccessibleProperty::Label);
            if let Some(child) = label_child {
                widget.update_relation(&[gtk::accessible::Relation::LabelledBy(&[
                    child.upcast_ref()
                ])]);
            }
        }
    }
}
