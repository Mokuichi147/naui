//! Cells that put an app's widget in a row (`List` rows, `Table` cells).
//!
//! Most containers put a child in place as soon as they are given it, and afterwards
//! leave it alone if it has moved to another parent ([`crate::layout::is_placed_in`]).
//! A table instead makes a new cell **every time it shows a row** and puts the widget
//! in it then. Checking the cell it is about to fill would always say "not here", so
//! the check runs the other way round:
//!
//! - Handing a widget to the table takes it out of wherever it is ([`claim`]). That is
//!   when the app put it there, so a later move by the app wins over it.
//! - A new cell takes the widget only if it is in no parent, or in a cell this table
//!   made earlier ([`CellOwner::may_take`]). Otherwise the widget was moved since, and
//!   the cell stays empty.
//!
//! The earlier cells are told apart by an owner mark, not by being inside the table:
//! a row that scrolled away, or one replaced by `reloadData`, still holds the widget
//! in a cell that is no longer in the table.

use std::cell::Cell;

use objc2::rc::Retained;
use objc2::runtime::NSObjectProtocol;
use objc2::{define_class, msg_send, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSTableCellView, NSView};

/// Which table made a cell.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct CellOwner(u64);

thread_local! {
    static NEXT_OWNER: Cell<u64> = const { Cell::new(1) };
}

impl CellOwner {
    /// A mark no other table has.
    pub(crate) fn new() -> Self {
        Self(NEXT_OWNER.with(|next| next.replace(next.get() + 1)))
    }

    /// Whether a new cell of this table may put `view` in itself: `view` is in no
    /// parent, or in a cell this table made earlier.
    pub(crate) fn may_take(self, view: &NSView) -> bool {
        match unsafe { view.superview() } {
            None => true,
            Some(parent) => parent
                .downcast_ref::<WidgetCell>()
                .is_some_and(|cell| cell.ivars().get() == self.0),
        }
    }
}

impl Default for CellOwner {
    fn default() -> Self {
        Self::new()
    }
}

/// Takes a widget handed to a table out of its current parent (see the module
/// documentation).
pub(crate) fn claim(view: &NSView) {
    if unsafe { view.superview() }.is_some() {
        view.removeFromSuperview();
    }
}

define_class!(
    /// An `NSTableCellView` that remembers which table made it.
    #[unsafe(super(NSTableCellView))]
    #[thread_kind = MainThreadOnly]
    #[name = "NauiWidgetCell"]
    #[ivars = Cell<u64>]
    pub(crate) struct WidgetCell;

    unsafe impl NSObjectProtocol for WidgetCell {}
);

impl WidgetCell {
    pub(crate) fn with_owner(mtm: MainThreadMarker, owner: CellOwner) -> Retained<NSTableCellView> {
        let this = Self::alloc(mtm).set_ivars(Cell::new(owner.0));
        let this: Retained<Self> = unsafe { msg_send![super(this), init] };
        Retained::into_super(this)
    }
}
