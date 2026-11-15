//! Handing a widget to a container while it is still in another one.
//!
//! The DOM moves an element that is appended somewhere else, so a widget added to a
//! container without being removed from the previous one moves. The previous container
//! still counts it as a child (the guide says this is not supported, and how the Web
//! behaves). From then on that container leaves it alone:
//!
//! - A container that places a child as soon as it is given one (`Stack`, `Popover`,
//!   `Window`, ...) touches the child (its layout, its focus, taking it out) only while
//!   the child is still directly where the container put it ([`is_placed_in`]).
//! - `List` rows and `Table` cells are made each time a row is built, and the widget
//!   is put in the new cell then. Checking the cell about to be filled would always say
//!   "not here", so the check runs the other way round. Handing a widget over takes it
//!   out of wherever it is and marks this owner as its last taker ([`Owner::claim`]).
//!   A new cell takes it only if it is in a cell this owner made earlier, or in no
//!   parent with this owner as its last taker ([`Owner::may_take`]). Otherwise the app
//!   has moved it since, and the cell stays empty.
//!
//! The mark matters for a widget handed on to another owner that has not built its row
//! yet: it is in no parent then, but belongs to the new owner. The earlier cells are
//! told apart by an owner mark, not by being inside the list: a row that scrolled
//! away is taken out of the document still holding its widget.

use std::cell::Cell;

use wasm_bindgen::JsValue;
use web_sys::{Element, Node};

/// Whether `element` is directly inside `host`, where a container put it.
pub(crate) fn is_placed_in(element: &Element, host: &Node) -> bool {
    element.parent_node().as_ref() == Some(host)
}

/// The property on a cell that records which owner made it.
const CELL_OWNER: &str = "__nauiCellOwner";
/// The property on a widget's element that records its last taker.
const LAST_TAKER: &str = "__nauiLastTaker";

thread_local! {
    static NEXT_OWNER: Cell<u32> = const { Cell::new(1) };
}

/// A `List` or a `Table`, as the maker of cells and the last taker of a widget.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Owner(u32);

impl Owner {
    /// A mark no other owner has.
    pub(crate) fn new() -> Self {
        Self(NEXT_OWNER.with(|next| next.replace(next.get() + 1)))
    }

    /// Marks `cell` as made by this owner.
    pub(crate) fn mark_cell(self, cell: &Element) {
        set_mark(cell, CELL_OWNER, self.0);
    }

    /// Takes a widget handed to this owner out of its current parent and marks this
    /// owner as its last taker.
    pub(crate) fn claim(self, element: &Element) {
        set_mark(element, LAST_TAKER, self.0);
        element.remove();
    }

    /// Whether a new cell of this owner may put `element` in itself: it is in a cell
    /// this owner made earlier, or in no parent with this owner as its last taker (or
    /// with none).
    pub(crate) fn may_take(self, element: &Element) -> bool {
        match element.parent_node() {
            None => read_mark(element, LAST_TAKER).is_none_or(|taker| taker == self.0),
            Some(parent) => read_mark(&parent, CELL_OWNER) == Some(self.0),
        }
    }
}

fn set_mark(target: &JsValue, key: &str, value: u32) {
    let _ = js_sys::Reflect::set(target, &JsValue::from_str(key), &JsValue::from(value));
}

fn read_mark(target: &JsValue, key: &str) -> Option<u32> {
    js_sys::Reflect::get(target, &JsValue::from_str(key))
        .ok()?
        .as_f64()
        .map(|value| value as u32)
}
