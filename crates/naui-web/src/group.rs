//! Dividing and grouping controls (`Separator` = `<hr>`, `Group` = `<fieldset>` +
//! `<legend>`).
//!
//! HTML has elements for both, so they are used as is: `<hr>` is reported to
//! assistive technologies as a separator, and `<fieldset>` as a group named by its
//! `<legend>`. Only the browser's dated defaults (an inset line, a grooved border, a
//! margin) are replaced, with a 1 px `GrayText` line like `SplitView`'s divider.

use std::cell::RefCell;
use std::rc::Rc;

use naui_core::{Orientation, Padding, Result, Sizing};
use wasm_bindgen::JsCast;
use web_sys::{Document, Element, HtmlElement};

use crate::layout::{apply_child_layout, mark_parent, ParentLayout};
use crate::to_error;
use crate::widgets::{create, impl_widget, Widget};

/// The line drawn by both, the same as `SplitView`'s divider.
const LINE: &str = "1px solid GrayText";

/// The rounding of the group's frame.
const GROUP_RADIUS: &str = "6px";

fn style(element: &HtmlElement, name: &str, value: &str) {
    let _ = element.style().set_property(name, value);
}

// -------------------------------------------------------------- Separator

struct SeparatorInner {
    element: HtmlElement,
    /// `on_file_drop`.
    file_drop: crate::file_drop::FileDrop,
}

/// A thin line that divides controls (`<hr>`).
#[derive(Clone)]
pub struct Separator(Rc<SeparatorInner>);
impl_widget!(Separator, element);

impl Separator {
    pub(crate) fn new(doc: &Document, orientation: Orientation) -> Result<Self> {
        let element: HtmlElement = create(doc, "hr")?.unchecked_into();
        // `<hr>` は既定で上下に余白を取り、くぼんだ 2 本の線を引く。
        // ほかの環境の区切り線と同じ、余白の無い 1 本の線にする。
        style(&element, "margin", "0");
        style(&element, "border", "none");
        style(&element, "flex-shrink", "0");
        if orientation.is_vertical() {
            // `<hr>` の役割 (separator) の既定の向きは横。縦線はそう伝える。
            element
                .set_attribute("aria-orientation", "vertical")
                .map_err(|e| to_error("build the separator", e))?;
            style(&element, "width", "0");
            style(&element, "border-left", LINE);
        } else {
            style(&element, "height", "0");
            style(&element, "border-top", LINE);
        }
        let this = Self(Rc::new(SeparatorInner {
            element,
            file_drop: Default::default(),
        }));
        // 線は引いた向きいっぱいに伸ばす。
        this.set_sizing(if orientation.is_vertical() {
            Sizing::fill_height()
        } else {
            Sizing::fill_width()
        });
        Ok(this)
    }
}

// ------------------------------------------------------------------ Group

struct GroupInner {
    /// `<fieldset><legend>heading</legend>content</fieldset>`
    element: HtmlElement,
    /// Present in the `<fieldset>` only while the heading is not empty, so that an
    /// empty heading leaves no gap in the border.
    legend: HtmlElement,
    /// Holds the content's handle, keeping it alive along with its callbacks.
    child: RefCell<Option<Box<dyn Widget>>>,
    /// `on_file_drop`.
    file_drop: crate::file_drop::FileDrop,
}

/// A frame with a heading around related controls (`<fieldset>` + `<legend>`).
#[derive(Clone)]
pub struct Group(Rc<GroupInner>);
impl_widget!(Group, element);

impl Group {
    pub(crate) fn new(doc: &Document, text: &str) -> Result<Self> {
        let element: HtmlElement = create(doc, "fieldset")?.unchecked_into();
        let legend: HtmlElement = create(doc, "legend")?.unchecked_into();
        style(&element, "margin", "0");
        // `<fieldset>` の既定の最小幅は中身の最小幅 (min-content)。ほかの
        // コンテナと同じく、狭い場所では縮めるようにする。
        style(&element, "min-inline-size", "0");
        style(&element, "min-width", "0");
        style(&element, "box-sizing", "border-box");
        style(&element, "border", LINE);
        style(&element, "border-radius", GROUP_RADIUS);
        crate::widgets::apply_padding(&element, naui_core::DEFAULT_GROUP_PADDING);
        // 中身は 1 つのマスに入れ、枠の内側いっぱいに広げる。`<legend>` は
        // マスに入らない (枠の線の上に描かれる)。
        style(&element, "display", "grid");
        style(&element, "grid-template-columns", "minmax(0, 1fr)");
        style(&element, "grid-template-rows", "minmax(0, 1fr)");
        style(&legend, "padding", "0 4px");
        mark_parent(&element, ParentLayout::Grid);

        let this = Self(Rc::new(GroupInner {
            element,
            legend,
            child: RefCell::new(None),
            file_drop: Default::default(),
        }));
        this.set_text(text);
        Ok(this)
    }

    /// The heading text. Empty when there is none.
    pub fn text(&self) -> String {
        self.0.legend.text_content().unwrap_or_default()
    }

    /// The heading text. An empty string removes the heading, leaving only the frame.
    pub fn set_text(&self, text: &str) {
        self.0.legend.set_text_content(Some(text));
        let attached = self.0.legend.parent_node().is_some();
        if text.is_empty() {
            if attached {
                let _ = self.0.element.remove_child(&self.0.legend);
            }
        } else if !attached {
            // `<legend>` は `<fieldset>` の最初の子でないと見出しにならない。
            let first = self.0.element.first_child();
            let _ = self.0.element.insert_before(&self.0.legend, first.as_ref());
        }
    }

    /// The space between the frame and the content. The default is
    /// [`DEFAULT_GROUP_PADDING`](naui_core::DEFAULT_GROUP_PADDING).
    ///
    /// In a right-to-left layout `left` and `right` are mirrored: `left` is the
    /// side where the content starts.
    pub fn set_padding(&self, padding: Padding) {
        crate::widgets::apply_padding(&self.0.element, padding);
    }

    /// The content inside the frame. Each call replaces it.
    ///
    /// The content fills the inside of the frame.
    pub fn set_child(&self, child: &dyn Widget) {
        if let Some(previous) = self.0.child.borrow_mut().take() {
            let _ = self.0.element.remove_child(&previous.native_element());
        }
        let element = child.native_element();
        if self.0.element.append_child(&element).is_ok() {
            apply_child_layout(&element, ParentLayout::Grid);
            *self.0.child.borrow_mut() = Some(child.boxed_clone());
        }
    }
}
