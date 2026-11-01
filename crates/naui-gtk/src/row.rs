//! The row of `GtkListBox`es that hold selectable rows (`List`, `Table`, `Tree`).
//!
//! When keyboard focus moves into a `GtkListBoxRow` (Tab, or the first focus a
//! window gives on showing), its `focus` vfunc focuses the row and then selects it
//! (`gtk_list_box_row_set_focus`), notifying `selected-rows-changed` with no click
//! or key on the row. In a list whose selection opens a detail, the window would
//! open its first row by itself as soon as it is shown.
//!
//! The other backends (`<select size>`, `NSTableView`) only move focus there, so
//! this row does the same: the same `focus` as GTK's, without the selecting.
//! Focusing the row still moves the list's cursor (its `grab_focus`), so arrow
//! keys go on from it and select, as do clicks and Space.

use gtk::glib;
use gtk::prelude::*;

// glib のサブクラス化マクロが `unsafe impl` を作るため、この中だけ許可する。
#[allow(unsafe_code)]
mod imp {
    use gtk::glib;
    use gtk::prelude::*;
    use gtk::subclass::prelude::*;

    #[derive(Default)]
    pub struct Row;

    #[glib::object_subclass]
    impl ObjectSubclass for Row {
        const NAME: &'static str = "NauiListBoxRow";
        type Type = super::Row;
        type ParentType = gtk::ListBoxRow;
    }

    impl ObjectImpl for Row {}

    impl WidgetImpl for Row {
        /// `gtk_list_box_row_focus` without the selecting after focusing the row.
        fn focus(&self, direction: gtk::DirectionType) -> bool {
            let row = self.obj();
            let had_focus = row.has_focus();
            let focus_child = row.focus_child();
            if focus_child
                .as_ref()
                .is_some_and(|child| child.child_focus(direction))
            {
                return true;
            }
            let child = row.child();
            match direction {
                gtk::DirectionType::Right
                | gtk::DirectionType::TabForward
                | gtk::DirectionType::Down => {
                    if focus_child.is_some() {
                        return false;
                    }
                    if !had_focus && row.grab_focus() {
                        return true;
                    }
                    child.is_some_and(|child| child.child_focus(direction))
                }
                gtk::DirectionType::Left
                | gtk::DirectionType::TabBackward
                | gtk::DirectionType::Up => {
                    if had_focus {
                        return false;
                    }
                    child.is_some_and(|child| child.child_focus(direction)) || row.grab_focus()
                }
                _ => false,
            }
        }
    }

    impl ListBoxRowImpl for Row {}
}

glib::wrapper! {
    /// A `GtkListBoxRow` that is not selected by keyboard focus moving into it.
    pub struct Row(ObjectSubclass<imp::Row>)
        @extends gtk::ListBoxRow, gtk::Widget,
        @implements gtk::Accessible, gtk::Actionable, gtk::Buildable, gtk::ConstraintTarget;
}

/// A new row for a `GtkListBox` whose selection the app is told about.
pub(crate) fn new() -> gtk::ListBoxRow {
    glib::Object::new::<Row>().upcast()
}
