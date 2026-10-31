//! Popovers (DOM).
//!
//! The browser's popover is an element with the **`popover="auto"` attribute**;
//! once `showPopover()` is called, putting it in the top layer, closing it on a
//! click outside or Esc (light dismiss) and closing other open popovers are all
//! done by the browser. Colors and the border are the user agent's own for
//! `[popover]` (`Canvas` / `CanvasText`), so they follow the `color-scheme` that
//! [`crate::Ui::set_theme`] sets on `<html>`.
//!
//! Placing it beside the widget uses **CSS anchor positioning**: the widget gets an
//! `anchor-name` unique to this popover, and the popover a `position-anchor` naming
//! it, a `position-area` for the side and `position-try-fallbacks` for moving to the
//! opposite side when there is no room. Newer parts of the specification, such as
//! `position-visibility`, still differ between engines, so they are not used.
//!
//! ## Differences from other platforms
//!
//! In a browser without anchor positioning, the popover is shown in the middle of
//! the window (the user agent's default for `[popover]`).
//!
//! That the user closed it is learned from the `toggle` event, which arrives after
//! the fact (a task later), so [`Popover::is_open`] reads `:popover-open` rather
//! than waiting for it.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use naui_core::{PopoverPosition, Result};
use wasm_bindgen::JsCast;
use web_sys::{Document, Element, HtmlElement, ToggleEvent};

use crate::to_error;
use crate::widgets::{create, Listener, Widget};

/// The gap between the widget and the popover, and the padding around the content
/// (logical pixels).
const GAP: &str = "4px";
const PADDING: &str = "12px";

thread_local! {
    /// The number for the next popover's anchor name.
    static NEXT_ID: Cell<u64> = const { Cell::new(0) };
}

struct PopoverInner {
    element: HtmlElement,
    /// The `anchor-name` this popover gives the widget it is shown beside.
    anchor_name: String,
    /// The widget it is shown beside. Held so the name can be taken off again.
    anchor: RefCell<Option<Box<dyn Widget>>>,
    child: RefCell<Option<Box<dyn Widget>>>,
    position: Cell<PopoverPosition>,
    /// Whether it is open as far as naui knows. Cleared by `close` before
    /// `hidePopover()`, so the `toggle` that follows is not passed on.
    open: Cell<bool>,
    on_close: RefCell<Option<Box<dyn FnMut()>>>,
    /// The receiver of `toggle`.
    listener: RefCell<Option<Listener>>,
}

/// A panel shown beside a widget (an element with the `popover` attribute).
///
/// It is not a widget laid out on screen, so it is not a [`Widget`].
#[derive(Clone)]
pub struct Popover(Rc<PopoverInner>);

impl Popover {
    pub(crate) fn new(doc: &Document) -> Result<Self> {
        let element: HtmlElement = create(doc, "div")?.unchecked_into();
        element
            .set_popover(Some("auto"))
            .map_err(|e| to_error("make the popover", e))?;
        let id = NEXT_ID.with(|next| next.replace(next.get() + 1));
        let anchor_name = format!("--naui-popover-{id}");
        let style = element.style();
        let _ = style.set_property("position-anchor", &anchor_name);
        // 既定の `inset: 0; margin: auto` は画面の中央に置くためのもの。外すと
        // `position-area` の側で、ウィジェットの中央にそろう。
        let _ = style.set_property("inset", "auto");
        let _ = style.set_property("margin", GAP);
        let _ = style.set_property("padding", PADDING);
        // `showPopover()` は文書に入っている要素にしか効かないため、
        // 作った時点で `<body>` へ入れておく (`Dialog` と同じ)。
        doc.body()
            .ok_or_else(|| naui_core::Error::new("get body", "there is no body"))?
            .append_child(&element)
            .map_err(|e| to_error("add the popover", e))?;

        let this = Self(Rc::new(PopoverInner {
            element,
            anchor_name,
            anchor: RefCell::new(None),
            child: RefCell::new(None),
            position: Cell::new(PopoverPosition::default()),
            open: Cell::new(false),
            on_close: RefCell::new(None),
            listener: RefCell::new(None),
        }));
        this.apply_position();

        let weak = Rc::downgrade(&this.0);
        let listener = Listener::attach_event(this.0.element.as_ref(), "toggle", move |event| {
            let Some(inner) = weak.upgrade() else {
                return;
            };
            let closed = event
                .dyn_ref::<ToggleEvent>()
                .is_some_and(|event| event.new_state() == "closed");
            if !closed {
                return;
            }
            Popover(inner.clone()).release_anchor();
            // `close` が先に下ろしていれば、アプリが閉じたので知らせない。
            if inner.open.replace(false) {
                Popover(inner).emit_close();
            }
        })?;
        *this.0.listener.borrow_mut() = Some(listener);
        Ok(this)
    }

    /// Notification. So that setting it again from inside the notification does not
    /// double-borrow, the closure is taken out only for the duration of the call.
    fn emit_close(&self) {
        let Some(mut f) = self.0.on_close.borrow_mut().take() else {
            return;
        };
        f();
        let mut slot = self.0.on_close.borrow_mut();
        if slot.is_none() {
            *slot = Some(f);
        }
    }

    /// The content. Each call replaces it.
    pub fn set_child(&self, child: &dyn Widget) {
        self.0.element.set_inner_html("");
        let element = child.native_element();
        if self.0.element.append_child(&element).is_ok() {
            // あとから `set_sizing` したときと同じ扱いにし、前の親のための指定を残さない。
            crate::layout::apply_child_layout(&element, crate::layout::ParentLayout::Block);
            *self.0.child.borrow_mut() = Some(child.boxed_clone());
        }
    }

    /// The side it prefers to appear on. Bottom by default.
    pub fn set_position(&self, position: PopoverPosition) {
        self.0.position.set(position);
        self.apply_position();
    }

    pub fn position(&self) -> PopoverPosition {
        self.0.position.get()
    }

    fn apply_position(&self) {
        let position = self.0.position.get();
        let (area, fallback) = match position {
            PopoverPosition::Top => ("top", "flip-block"),
            PopoverPosition::Bottom => ("bottom", "flip-block"),
            PopoverPosition::Left => ("left", "flip-inline"),
            PopoverPosition::Right => ("right", "flip-inline"),
        };
        let style = self.0.element.style();
        let _ = style.set_property("position-area", area);
        let _ = style.set_property("position-try-fallbacks", fallback);
    }

    /// Shows it beside `widget`. If it is already open, it moves to `widget`.
    ///
    /// Does nothing while `widget` is not in the document.
    pub fn open(&self, widget: &dyn Widget) {
        let target = widget.native_element();
        if !target.is_connected() {
            return;
        }
        self.release_anchor();
        set_anchor_name(&target, &self.0.anchor_name, true);
        *self.0.anchor.borrow_mut() = Some(widget.boxed_clone());
        if !self.is_open() && self.0.element.show_popover().is_err() {
            self.release_anchor();
            return;
        }
        self.0.open.set(true);
    }

    /// Takes this popover's name off the widget it was shown beside.
    fn release_anchor(&self) {
        if let Some(anchor) = self.0.anchor.borrow_mut().take() {
            set_anchor_name(&anchor.native_element(), &self.0.anchor_name, false);
        }
    }

    /// Closes it. `on_close` is not called. Does nothing if it is not open.
    pub fn close(&self) {
        if !self.0.open.replace(false) {
            return;
        }
        let _ = self.0.element.hide_popover();
    }

    /// Whether it is open.
    pub fn is_open(&self) -> bool {
        self.0.element.matches(":popover-open").unwrap_or(false)
    }

    /// Called when the user closes it (a click outside, Esc).
    ///
    /// Not called when it is closed with [`close`](Self::close). Setting it again
    /// replaces the previous closure. The browser tells it with `toggle`, which
    /// arrives a task after the popover has closed.
    pub fn on_close(&self, f: impl FnMut() + 'static) {
        *self.0.on_close.borrow_mut() = Some(Box::new(f));
    }

    /// Closes it the way a click outside does, notifying `on_close` (for tests and
    /// automation). The notification arrives with `toggle`, a task later.
    pub fn dismiss(&self) {
        let _ = self.0.element.hide_popover();
    }

    /// The element with the `popover` attribute. A backend-specific escape hatch.
    pub fn native_element(&self) -> Element {
        self.0.element.clone().unchecked_into()
    }
}

/// Adds `name` to, or removes it from, the element's `anchor-name` (a
/// comma-separated list, since several popovers can point at the same widget).
fn set_anchor_name(element: &Element, name: &str, present: bool) {
    let Some(element) = element.dyn_ref::<HtmlElement>() else {
        return;
    };
    let style = element.style();
    let current = style.get_property_value("anchor-name").unwrap_or_default();
    let mut names: Vec<&str> = current
        .split(',')
        .map(str::trim)
        .filter(|n| !n.is_empty() && *n != "none" && *n != name)
        .collect();
    if present {
        names.push(name);
    }
    if names.is_empty() {
        let _ = style.remove_property("anchor-name");
    } else {
        let _ = style.set_property("anchor-name", &names.join(", "));
    }
}
