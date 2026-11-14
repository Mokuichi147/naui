//! Handing a widget to a container while it is still in another one.
//!
//! GTK4 refuses to give a widget a second parent: `gtk_box_append`,
//! `adw_bin_set_child` and the like check that the widget has no parent and do nothing
//! otherwise. naui follows that (the guide says adding a widget that is still in
//! another container is not supported, and GTK refuses it), but it has to refuse
//! **before** it touches anything:
//!
//! - A container that places a child as soon as it is given one (`Stack`, `Grid`,
//!   `Scroll`, `Group`, ...) checks first ([`refuses`]). A refused child is not
//!   recorded and nothing is written on it (its alignment, its placement), and the
//!   previous child of a one-child container stays. The record then always matches
//!   what is in the native container, so removing, clearing and the settings that go
//!   through the children only touch children that are really there.
//! - `List` rows, `Table` cells, `Dialog` and `Sheet` place their widgets later
//!   (each time a row is built or the dialog opens), so the widget may have moved in
//!   between. Being handed a widget is a claim ([`Owner::claim`]): it succeeds when
//!   the widget has no parent or is in one of this owner's own cells, and then marks
//!   this owner as the widget's last taker. When it comes to placing the widget,
//!   the owner does so only if the widget has no parent and no other owner took it
//!   since ([`Owner::may_place`]). Otherwise the cell stays empty and the widget is
//!   left as it is, wherever the app put it.
//!
//! The mark matters for a widget no container holds at the moment: a closed
//! `Dialog`'s content, or a row's content scrolled out of a `List`. Handed on to
//! another owner, it is the new owner's, and the previous one does not take it back
//! when it opens again or builds the row again.

use std::cell::Cell;

use gtk::glib;
use gtk::prelude::*;

use crate::bin::SizeBin;

/// Whether `bin` is in another container, so `container` must refuse it.
///
/// `mine` says whether `bin` is the child the container already has (giving it the
/// same child again changes nothing). A refusal is reported the way GTK reports its
/// own, as a critical message.
pub(crate) fn refuses(bin: &SizeBin, mine: bool, container: &str) -> bool {
    if mine || bin.parent().is_none() {
        return false;
    }
    report(container);
    true
}

fn report(container: &str) {
    glib::g_critical!(
        "naui",
        "{container}: the widget is still in another container; remove it from there first"
    );
}

thread_local! {
    static NEXT_OWNER: Cell<u64> = const { Cell::new(1) };
}

/// One of the containers that place their widgets later, as the last taker of a
/// widget.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Owner(u64);

impl Owner {
    /// A mark no other owner has.
    pub(crate) fn new() -> Self {
        Self(NEXT_OWNER.with(|next| next.replace(next.get() + 1)))
    }

    /// Handing `bin` to this owner. Succeeds when `bin` has no parent or `in_own_cell`
    /// says it is in one of this owner's cells; it then becomes this owner's.
    /// Otherwise reports the refusal and returns `false`.
    pub(crate) fn claim(self, bin: &SizeBin, in_own_cell: bool, container: &str) -> bool {
        if bin.parent().is_some() && !in_own_cell {
            report(container);
            return false;
        }
        bin.set_last_taker(self.0);
        true
    }

    /// Whether a new cell of this owner may put `bin` in itself: it has no parent, and
    /// no other owner took it last.
    pub(crate) fn may_place(self, bin: &SizeBin) -> bool {
        let taker = bin.last_taker();
        bin.parent().is_none() && (taker == 0 || taker == self.0)
    }
}
