//! Cells that put an app's widget in a row (`List` rows, `Table` cells).
//!
//! Most containers put a child in place as soon as they are given it, and afterwards
//! leave it alone if it has moved to another parent ([`crate::layout::is_placed_in`]).
//! A table instead makes a new cell **every time it shows a row** and puts the widget
//! in it then. Checking the cell it is about to fill would always say "not here", so
//! the check runs the other way round:
//!
//! - Handing a widget to the table takes it out of wherever it is and marks the table
//!   as its last taker ([`claim`]). That is when the app put it there, so a later move
//!   by the app wins over it.
//! - A new cell takes the widget only if it is in a cell this table made earlier, or in
//!   no parent with this table as its last taker ([`CellOwner::may_take`]). Otherwise
//!   the widget was moved since, and the cell stays empty.
//!
//! The mark matters for a widget handed on to another table or dialog that has not
//! shown it yet: it is in no parent then, but belongs to the new taker.
//!
//! The earlier cells are told apart by an owner mark, not by being inside the table:
//! a row that scrolled away, or one replaced by `reloadData`, still holds the widget
//! in a cell that is no longer in the table.

use std::cell::Cell;
use std::ffi::c_void;

use objc2::ffi::{
    objc_getAssociatedObject, objc_setAssociatedObject, OBJC_ASSOCIATION_RETAIN_NONATOMIC,
};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObjectProtocol};
use objc2::{define_class, msg_send, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSTableCellView, NSView};
use objc2_foundation::NSNumber;

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

    /// Whether a new cell of this table may put `view` in itself: `view` is in a cell
    /// this table made earlier, or in no parent with this table as its last taker.
    pub(crate) fn may_take(self, view: &NSView) -> bool {
        match unsafe { view.superview() } {
            None => self.is_last_taker_of(view),
            Some(parent) => parent
                .downcast_ref::<WidgetCell>()
                .is_some_and(|cell| cell.ivars().get() == self.0),
        }
    }

    /// Whether this owner took `view` last (or nothing ever did).
    pub(crate) fn is_last_taker_of(self, view: &NSView) -> bool {
        last_taker(view).is_none_or(|taker| taker == self.0)
    }
}

/// The key of the associated object that records the last taker of a view.
static LAST_TAKER_KEY: u8 = 0;

fn last_taker(view: &NSView) -> Option<u64> {
    let key = (&raw const LAST_TAKER_KEY).cast::<c_void>();
    let object: *const AnyObject = (view as *const NSView).cast();
    let value = unsafe { objc_getAssociatedObject(object, key) };
    // 入れるのは `mark_taker` の NSNumber だけ。
    let number = unsafe { value.cast::<NSNumber>().as_ref() }?;
    Some(number.as_u64())
}

fn mark_taker(view: &NSView, owner: CellOwner) {
    let key = (&raw const LAST_TAKER_KEY).cast::<c_void>();
    let number = NSNumber::new_u64(owner.0);
    let object: *mut AnyObject = (view as *const NSView).cast_mut().cast();
    unsafe {
        objc_setAssociatedObject(
            object,
            key,
            Retained::as_ptr(&number).cast_mut().cast(),
            OBJC_ASSOCIATION_RETAIN_NONATOMIC,
        )
    };
}

impl Default for CellOwner {
    fn default() -> Self {
        Self::new()
    }
}

/// Takes a widget handed to `owner` out of its current parent and marks `owner` as its
/// last taker (see the module documentation).
pub(crate) fn claim(view: &NSView, owner: CellOwner) {
    mark_taker(view, owner);
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
