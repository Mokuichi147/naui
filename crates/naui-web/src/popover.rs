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
//! The element lives in `<body>`, or, while shown beside a widget inside a
//! `<dialog>` or another popover, inside that element:
//!
//! - a modal dialog makes everything outside it inert, and the top layer does not
//!   change that, so a popover left in `<body>` could not be clicked or focused;
//! - the browser treats a popover as nested in another only when it is inside it in
//!   the DOM (or names it as the source, which not every engine supports yet).
//!   Otherwise showing it closes the other, and the widget it points at with it.
//!
//! It goes back to `<body>` once closed, because WebKit stops rendering for good
//! when a modal dialog or a popover holding an element with
//! `position-try-fallbacks` closes (shown or not). The `toggle` that would put a
//! nested popover back arrives too late for its parent closing with it, so a
//! closing popover moves the closed popovers inside it out on `beforetoggle`.
//!
//! Hiding the window or dialog the widget is in does not hide the popover (it is in
//! the top layer), so [`crate::Window::close`] and the dialog's closing close the
//! popovers shown beside widgets inside them, the way a click outside does. Esc in a
//! dialog closes the topmost such popover first rather than the dialog.
//!
//! ## Differences from other platforms
//!
//! In a browser without anchor positioning, the popover is shown in the middle of
//! the window (the user agent's default for `[popover]`). The placement styles are
//! only set where the browser supports them, since without `position-area` they
//! would only undo that centering.
//!
//! That the user closed it is learned from the `toggle` event, which arrives after
//! the fact (a task later), so [`Popover::is_open`] reads `:popover-open` rather
//! than waiting for it.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

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
    /// Every popover made, for closing the ones inside a window or dialog that closes.
    static POPOVERS: RefCell<Vec<Weak<PopoverInner>>> = const { RefCell::new(Vec::new()) };
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
    /// The receivers of `toggle` and `beforetoggle`.
    listeners: RefCell<Vec<Listener>>,
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
        let _ = style.set_property("padding", PADDING);
        // 既定の `inset: 0; margin: auto` は画面の中央に置くためのもの。外すと
        // `position-area` の側で、ウィジェットの中央にそろう。アンカー配置のない
        // ブラウザで外すと左上へずれて画面から出るので、そのときは中央のままにする。
        if supports_anchor_positioning() {
            let _ = style.set_property("position-anchor", &anchor_name);
            let _ = style.set_property("inset", "auto");
            let _ = style.set_property("margin", GAP);
        }
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
            listeners: RefCell::new(Vec::new()),
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
            let popover = Popover(inner.clone());
            popover.release_anchor();
            // 知らせを待つ間に開き直していれば、その置き場所のままにする。
            if !popover.is_open() {
                popover.park();
            }
            // `close` が先に下ろしていれば、アプリが閉じたので知らせない。
            if inner.open.replace(false) {
                Popover(inner).emit_close();
            }
        })?;
        // 入れ子の子は親より先に閉じるので、親が閉じる直前には閉じ終えている。
        let weak = Rc::downgrade(&this.0);
        let before =
            Listener::attach_event(this.0.element.as_ref(), "beforetoggle", move |event| {
                let closing = event
                    .dyn_ref::<ToggleEvent>()
                    .is_some_and(|event| event.new_state() == "closed");
                if let (true, Some(inner)) = (closing, weak.upgrade()) {
                    park_closed_within(&inner.element);
                }
            })?;
        *this.0.listeners.borrow_mut() = vec![listener, before];
        POPOVERS.with(|popovers| {
            let mut popovers = popovers.borrow_mut();
            popovers.retain(|weak| weak.strong_count() > 0);
            popovers.push(Rc::downgrade(&this.0));
        });
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
        // 中身だけを外す。入れ子で開いたポップオーバーもこの要素の中にいる。
        if let Some(previous) = self.0.child.borrow_mut().take() {
            previous.native_element().remove();
        }
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
    /// Does nothing while `widget` is not shown (not in the document, or in a closed
    /// window).
    pub fn open(&self, widget: &dyn Widget) {
        let target = widget.native_element();
        // 閉じたウィンドウは `display: none` で文書に残るので、箱があるかも見る。
        if !target.is_connected() || target.get_client_rects().length() == 0 {
            return;
        }
        // モーダルの `<dialog>` の外は inert になり、top layer に出ても押せない。
        // また、ブラウザはポップオーバーの入れ子を DOM の親子で見分けるので、別の
        // ポップオーバーの中から開くときはその中に置かないと、開いた時点で相手が閉じる。
        // どちらも、ウィジェットにいちばん近いその要素の中へ移す。出ている要素を移すと、
        // `toggle` を出さずに閉じる (HTML の removing steps) ので、下で出し直す。
        let host = match target.closest("[popover], dialog") {
            Ok(Some(dialog)) => Some(dialog),
            _ => target
                .owner_document()
                .and_then(|doc| doc.body())
                .map(|body| body.unchecked_into()),
        };
        if let Some(host) = host {
            let parent = self.0.element.parent_element();
            if parent.as_ref() != Some(&host) && host.append_child(&self.0.element).is_err() {
                return;
            }
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

    /// Puts the element back in `<body>` (see the module docs). Only while it is
    /// hidden, since moving a shown popover closes it.
    fn park(&self) {
        let body = self.0.element.owner_document().and_then(|doc| doc.body());
        if let Some(body) = body {
            if self.0.element.parent_element().as_deref() != Some(body.as_ref()) {
                let _ = body.append_child(&self.0.element);
            }
        }
    }

    /// Closes it. `on_close` is not called. Does nothing if it is not open.
    pub fn close(&self) {
        if !self.0.open.replace(false) {
            return;
        }
        self.release_anchor();
        let _ = self.0.element.hide_popover();
        self.park();
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

/// Closes the open popovers shown beside widgets inside `container`, the way a click
/// outside does (`on_close` is notified), and moves any popover element inside it
/// back to `<body>`. Returns whether any was open.
pub(crate) fn dismiss_within(container: &Element) -> bool {
    let popovers: Vec<Popover> = POPOVERS.with(|popovers| {
        popovers
            .borrow()
            .iter()
            .filter_map(Weak::upgrade)
            .map(Popover)
            .collect()
    });
    let mut dismissed = false;
    for popover in &popovers {
        let anchored = popover
            .0
            .anchor
            .borrow()
            .as_ref()
            .is_some_and(|anchor| container.contains(Some(anchor.native_element().as_ref())));
        if anchored && popover.is_open() {
            popover.dismiss();
            dismissed = true;
        }
        // 知らせ (`toggle`) を待たずに、ダイアログが閉じる前に出しておく。
        if !popover.is_open() && container.contains(Some(popover.0.element.as_ref())) {
            popover.park();
        }
    }
    dismissed
}

/// Closes the topmost open popover shown beside a widget inside `container`, the
/// way Esc does (`on_close` is notified). Returns whether there was one.
pub(crate) fn dismiss_topmost_within(container: &Element) -> bool {
    let open: Vec<Popover> = POPOVERS.with(|popovers| {
        popovers
            .borrow()
            .iter()
            .filter_map(Weak::upgrade)
            .map(Popover)
            .filter(|popover| {
                popover.is_open()
                    && popover.0.anchor.borrow().as_ref().is_some_and(|anchor| {
                        container.contains(Some(anchor.native_element().as_ref()))
                    })
            })
            .collect()
    });
    // 入れ子の子は親の要素の中にいるので、ほかを中に持たないものがいちばん上。
    let topmost = open.iter().find(|popover| {
        !open.iter().any(|other| {
            !Rc::ptr_eq(&popover.0, &other.0)
                && popover.0.element.contains(Some(other.0.element.as_ref()))
        })
    });
    match topmost {
        Some(popover) => {
            popover.dismiss();
            true
        }
        None => false,
    }
}

/// Moves the closed popover elements inside `container` back to `<body>`.
fn park_closed_within(container: &Element) {
    let Ok(nested) = container.query_selector_all("[popover]:not(:popover-open)") else {
        return;
    };
    let body = container.owner_document().and_then(|doc| doc.body());
    let Some(body) = body else {
        return;
    };
    for index in 0..nested.length() {
        if let Some(node) = nested.item(index) {
            let _ = body.append_child(&node);
        }
    }
}

/// Whether the browser has CSS anchor positioning (`position-area`).
fn supports_anchor_positioning() -> bool {
    web_sys::css::supports_with_value("position-area", "bottom").unwrap_or(false)
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
