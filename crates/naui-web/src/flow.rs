//! `Flow`: a Flexbox row that wraps (`flex-wrap`).
//!
//! The browser breaks the lines itself, so this only writes properties, the same
//! as a horizontal `Stack` plus `flex-wrap: wrap`.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use naui_core::{Align, Orientation, Padding, Result};
use wasm_bindgen::JsCast;
use web_sys::{Document, Element, HtmlElement};

use crate::layout::ParentLayout;
use crate::widgets::{apply_padding, create, impl_widget, Widget};

struct FlowInner {
    element: HtmlElement,
    children: RefCell<Vec<Box<dyn Widget>>>,
    align: Cell<Align>,
    /// `on_file_drop`.
    file_drop: crate::file_drop::FileDrop,
}

/// A container that wraps its children onto new lines (a Flexbox `<div>` with
/// `flex-wrap: wrap`).
#[derive(Clone)]
pub struct Flow(Rc<FlowInner>);
impl_widget!(Flow, element);

impl Flow {
    pub(crate) fn new(doc: &Document) -> Result<Self> {
        let element: HtmlElement = create(doc, "div")?.unchecked_into();
        let style = element.style();
        let _ = style.set_property("display", "flex");
        let _ = style.set_property("flex-direction", "row");
        let _ = style.set_property("flex-wrap", "wrap");
        // 行の中では縦の中央に置く (ほかの環境と同じ)。行どうしは上へ詰める。
        let _ = style.set_property("align-items", "center");
        let _ = style.set_property("align-content", "flex-start");
        let this = Self(Rc::new(FlowInner {
            element,
            children: RefCell::new(Vec::new()),
            align: Cell::new(Align::Start),
            file_drop: Default::default(),
        }));
        this.apply_align();
        Ok(this)
    }

    /// The children are laid out as in a horizontal `Stack`: `Fill` on the width
    /// is `flex-grow` (the room left in the line), `Fill` on the height
    /// `align-self: stretch` (the line's height).
    fn parent_layout(&self) -> ParentLayout {
        ParentLayout::Flex(Orientation::Horizontal)
    }

    /// `column` is CSS's `column-gap` (between children in a line), `row` its
    /// `row-gap` (between lines).
    pub fn set_spacing(&self, column: f64, row: f64) {
        let _ = self
            .0
            .element
            .style()
            .set_property("gap", &format!("{row}px {column}px"));
    }

    pub fn set_padding(&self, padding: Padding) {
        apply_padding(&self.0.element, padding);
    }

    /// `justify-content`. `Fill` is the same as `Start`, as on the other platforms.
    pub fn set_align(&self, align: Align) {
        if self.0.align.replace(align) != align {
            self.apply_align();
        }
    }

    fn apply_align(&self) {
        let value = match self.0.align.get() {
            Align::Start | Align::Fill => "flex-start",
            Align::Center => "center",
            Align::End => "flex-end",
        };
        let _ = self
            .0
            .element
            .style()
            .set_property("justify-content", value);
        crate::layout::mark_parent(&self.0.element, self.parent_layout());
    }

    fn forget(children: &mut Vec<Box<dyn Widget>>, element: &Element) {
        children.retain(|child| child.native_element() != *element);
    }

    /// Appends a child at the end.
    pub fn append(&self, child: &dyn Widget) {
        let element = child.native_element();
        if self.0.element.append_child(&element).is_ok() {
            crate::layout::apply_child_layout(&element, self.parent_layout());
            let mut children = self.0.children.borrow_mut();
            Self::forget(&mut children, &element);
            children.push(child.boxed_clone());
        }
    }

    /// Inserts a child at the given position. If `index` is at least the current
    /// count, it is appended at the end.
    pub fn insert(&self, index: usize, child: &dyn Widget) {
        let element = child.native_element();
        let mut children = self.0.children.borrow_mut();
        let mut index = index.min(children.len());
        // 同じ子が記録に残っていれば (外さずに入れ直した子)、その分を詰めて数える。
        if let Some(old) = children
            .iter()
            .position(|child| child.native_element() == element)
        {
            if old < index {
                index -= 1;
            }
            children.remove(old);
        }
        // 前に置く相手は、まだこの中にいる子から選ぶ。
        let anchor = children[index..]
            .iter()
            .map(|next| next.native_element())
            .find(|next| crate::handover::is_placed_in(next, &self.0.element));
        let inserted = self.0.element.insert_before(&element, anchor.as_deref());
        if inserted.is_ok() {
            crate::layout::apply_child_layout(&element, self.parent_layout());
            children.insert(index, child.boxed_clone());
        }
    }

    /// Removes the child at the given position. Does nothing if out of range.
    pub fn remove(&self, index: usize) {
        let mut children = self.0.children.borrow_mut();
        if index >= children.len() {
            return;
        }
        let child = children.remove(index);
        let element = child.native_element();
        // 外さずにほかへ移った子は、新しい親から取り上げない。
        if crate::handover::is_placed_in(&element, &self.0.element) {
            let _ = self.0.element.remove_child(&element);
        }
    }

    /// Removes every child.
    pub fn clear(&self) {
        for child in std::mem::take(&mut *self.0.children.borrow_mut()) {
            let element = child.native_element();
            if crate::handover::is_placed_in(&element, &self.0.element) {
                let _ = self.0.element.remove_child(&element);
            }
        }
    }

    pub fn len(&self) -> usize {
        self.0.children.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
