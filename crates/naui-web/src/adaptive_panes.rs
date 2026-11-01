//! A list and its detail, side by side or one at a time (Flexbox `<div>`s).
//!
//! HTML has no element for this. A CSS container query could hide one pane below a
//! width without script, but it cannot tell the page which pane it shows (the back
//! button, [`is_collapsed`](AdaptivePanes::is_collapsed)), has no slack at the
//! boundary, and `container-type: inline-size` stops the container from taking its
//! width from its content. So naui watches the container's width with a
//! `ResizeObserver` (the same one as `Stack::set_breakpoint`) and decides with
//! naui-core's [`PanesArrangement`], switching before the page is painted at that
//! width.
//!
//! Side by side, the list pane has the fixed width, a 1 px line in the system color
//! `GrayText` follows it (the same line as `SplitView`'s divider), and the detail
//! takes the rest. One at a time, the pane not shown is hidden (`hidden` and
//! `display: none`), and while only the detail is shown a `<button>` above it goes
//! back to the list.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use naui_core::{ui_strings, Orientation, Pane, PanesArrangement, Result, DEFAULT_LIST_PANE_WIDTH};
use wasm_bindgen::JsCast;
use web_sys::{Document, Element, HtmlElement};

use crate::layout::{apply_child_layout, mark_parent, ParentLayout};
use crate::to_error;
use crate::widgets::{
    create, impl_widget, Listener, ValueHandler, Widget, WidthWatch, WidthWatcher,
};

/// The margin around the back button.
const BACK_MARGIN: &str = "8px";

fn style(element: &HtmlElement, property: &str, value: &str) {
    let _ = element.style().set_property(property, value);
}

struct AdaptivePanesInner {
    /// A Flexbox `<div>` laying out the list, the line and the detail side.
    element: HtmlElement,
    list_pane: HtmlElement,
    separator: HtmlElement,
    /// The back button and, below it, the detail pane.
    detail_side: HtmlElement,
    detail_pane: HtmlElement,
    back: HtmlElement,
    /// Holds the panes' handles, keeping them alive along with their callbacks.
    list: RefCell<Option<Box<dyn Widget>>>,
    detail: RefCell<Option<Box<dyn Widget>>>,
    list_width: Cell<f64>,
    /// The breakpoint, the last width, and the pane shown while only one is.
    arrangement: Cell<PanesArrangement>,
    on_back: ValueHandler<()>,
    /// The back button's click. Held with the same lifetime as the handle.
    listeners: RefCell<Vec<Listener>>,
    /// `on_file_drop`.
    file_drop: crate::file_drop::FileDrop,
}

impl Drop for AdaptivePanesInner {
    fn drop(&mut self) {
        WidthWatch::forget(&self.element);
    }
}

impl WidthWatcher for AdaptivePanesInner {
    fn check_width(self: Rc<Self>) -> bool {
        let mut arrangement = self.arrangement.get();
        let changed = arrangement.resized(self.element.get_bounding_client_rect().width());
        self.arrangement.set(arrangement);
        if changed {
            self.apply();
        }
        changed
    }
}

/// A list and its detail, side by side when wide and one at a time when narrow.
#[derive(Clone)]
pub struct AdaptivePanes(Rc<AdaptivePanesInner>);
impl_widget!(AdaptivePanes, element);

impl AdaptivePanes {
    pub(crate) fn new(doc: &Document) -> Result<Self> {
        let element: HtmlElement = create(doc, "div")?.unchecked_into();
        style(&element, "display", "flex");
        style(&element, "flex-direction", "row");
        style(&element, "align-items", "stretch");
        style(&element, "min-width", "0");
        style(&element, "min-height", "0");

        let list_pane = pane(doc)?;
        let detail_pane = pane(doc)?;
        style(&detail_pane, "flex", "1 1 0");

        let separator: HtmlElement = create(doc, "div")?.unchecked_into();
        let _ = separator.set_attribute("role", "separator");
        let _ = separator.set_attribute("aria-orientation", "vertical");
        style(&separator, "flex", "0 0 auto");
        style(&separator, "border-left", "1px solid GrayText");

        let back: HtmlElement = create(doc, "button")?.unchecked_into();
        let _ = back.set_attribute("type", "button");
        let label = ui_strings().back;
        // 矢印は飾り。読み上げは文字列だけにする。
        back.set_text_content(Some(&format!("\u{2190} {label}")));
        let _ = back.set_attribute("aria-label", label);
        style(&back, "align-self", "flex-start");
        style(&back, "margin", BACK_MARGIN);

        let detail_side: HtmlElement = create(doc, "div")?.unchecked_into();
        style(&detail_side, "display", "flex");
        style(&detail_side, "flex-direction", "column");
        style(&detail_side, "flex", "1 1 0");
        style(&detail_side, "min-width", "0");
        style(&detail_side, "min-height", "0");
        for part in [&back, &detail_pane] {
            detail_side
                .append_child(part)
                .map_err(|e| to_error("build the adaptive panes", e))?;
        }
        for part in [&list_pane, &separator, &detail_side] {
            element
                .append_child(part)
                .map_err(|e| to_error("build the adaptive panes", e))?;
        }

        let this = Self(Rc::new(AdaptivePanesInner {
            element,
            list_pane,
            separator,
            detail_side,
            detail_pane,
            back,
            list: RefCell::new(None),
            detail: RefCell::new(None),
            list_width: Cell::new(DEFAULT_LIST_PANE_WIDTH),
            arrangement: Cell::new(PanesArrangement::default()),
            on_back: ValueHandler::default(),
            listeners: RefCell::new(Vec::new()),
            file_drop: Default::default(),
        }));
        this.0.apply();

        let weak = Rc::downgrade(&this.0);
        let click = Listener::attach(this.0.back.as_ref(), "click", move || {
            if let Some(inner) = weak.upgrade() {
                AdaptivePanes(inner).go_back();
            }
        })?;
        this.0.listeners.borrow_mut().push(click);
        let weak: Weak<AdaptivePanesInner> = Rc::downgrade(&this.0);
        WidthWatch::watch(this.0.element.as_ref(), weak);
        Ok(this)
    }

    /// The list pane. Each call replaces it.
    pub fn set_list(&self, child: &dyn Widget) {
        put(&self.0.list_pane, &self.0.list, child);
    }

    /// The detail pane. Each call replaces it.
    pub fn set_detail(&self, child: &dyn Widget) {
        put(&self.0.detail_pane, &self.0.detail, child);
    }

    /// The narrowest width at which both panes are shown.
    pub fn set_breakpoint(&self, width: f64) {
        let mut arrangement = self.0.arrangement.get();
        arrangement.resized(self.0.element.get_bounding_client_rect().width());
        arrangement.set_breakpoint(width);
        self.0.arrangement.set(arrangement);
        self.0.apply();
    }

    pub fn breakpoint(&self) -> f64 {
        self.0.arrangement.get().breakpoint()
    }

    /// The list pane's width while both panes are shown.
    pub fn set_list_width(&self, width: f64) {
        self.0.list_width.set(width.max(0.0));
        self.0.apply();
    }

    pub fn list_width(&self) -> f64 {
        self.0.list_width.get()
    }

    /// Whether only one pane is shown.
    pub fn is_collapsed(&self) -> bool {
        self.0.arrangement.get().is_collapsed()
    }

    /// The pane shown while only one is.
    pub fn visible_pane(&self) -> Pane {
        self.0.arrangement.get().visible()
    }

    /// Chooses the pane shown while only one is. `on_back` is not called.
    pub fn show(&self, pane: Pane) {
        let mut arrangement = self.0.arrangement.get();
        if arrangement.show(pane) {
            self.0.arrangement.set(arrangement);
            self.0.apply();
        }
    }

    /// Goes back to the list as if the user had pressed the back button. Calls
    /// `on_back`, even when the list is already shown.
    pub fn go_back(&self) {
        self.show(Pane::List);
        self.0.on_back.emit(());
    }

    /// Called when the user goes back from the detail to the list.
    pub fn on_back(&self, mut f: impl FnMut() + 'static) {
        self.0.on_back.set(move |()| f());
    }

    /// The back button. Public as a backend-specific escape hatch.
    pub fn native_back_button(&self) -> HtmlElement {
        self.0.back.clone()
    }
}

impl AdaptivePanesInner {
    /// Makes the elements match `arrangement`.
    fn apply(&self) {
        let arrangement = self.arrangement.get();
        let collapsed = arrangement.is_collapsed();
        let shows_list = arrangement.shows(Pane::List);
        let shows_detail = arrangement.shows(Pane::Detail);

        let shows_back = arrangement.shows_back();
        // 隠す区画にフォーカスがあれば、出す区画へ移す (戻るボタンで戻ったときも、
        // ボタンごと隠れるので一覧へ移る)。広げて戻るボタンだけが隠れるときも、
        // そのフォーカスを詳細へ移す。
        let refocus = if !shows_list && holds_focus(&self.list_pane) {
            Some(Pane::Detail)
        } else if !shows_detail && holds_focus(&self.detail_side) {
            Some(Pane::List)
        } else if !shows_back && holds_focus(&self.back) {
            Some(Pane::Detail)
        } else {
            None
        };

        if collapsed {
            // 1 区画のときは、出ている側が幅いっぱいに広がる。
            style(&self.list_pane, "flex", "1 1 0");
            style(&self.list_pane, "width", "auto");
        } else {
            style(&self.list_pane, "flex", "0 0 auto");
            style(
                &self.list_pane,
                "width",
                &format!("{}px", self.list_width.get()),
            );
        }
        set_shown(&self.list_pane, shows_list, "flex");
        set_shown(&self.detail_side, shows_detail, "flex");
        set_shown(&self.separator, !collapsed, "block");
        set_shown(&self.back, shows_back, "inline-block");

        if let Some(to) = refocus {
            self.move_focus(to, shows_back);
        }
    }

    /// Moves the focus into the pane `to`, out of a part being hidden.
    ///
    /// A detail with nothing that takes focus (only labels) hands it to the back
    /// button while that is shown, and otherwise to the list, so a keyboard user
    /// keeps a place to continue from.
    fn move_focus(&self, to: Pane, back_shown: bool) {
        if to == Pane::Detail && focus_child(&self.detail) {
            return;
        }
        if to == Pane::Detail && back_shown && crate::interaction::request_focus(&self.back) {
            return;
        }
        focus_child(&self.list);
    }
}

/// Moves the focus into a pane's widget. `true` if it moved.
///
/// The borrow ends before `focus()`: the browser reports the focus change on the
/// spot, and the app's `on_focus_change` may replace the pane from in there.
fn focus_child(slot: &RefCell<Option<Box<dyn Widget>>>) -> bool {
    let element = slot.borrow().as_ref().map(|child| child.native_element());
    element.is_some_and(|element| crate::interaction::request_focus(&element))
}

/// Shows or hides one of naui's own parts. `display` is the value while shown.
fn set_shown(element: &HtmlElement, shown: bool, display: &str) {
    element.set_hidden(!shown);
    style(element, "display", if shown { display } else { "none" });
}

/// Whether the focused element is `element` or inside it.
fn holds_focus(element: &HtmlElement) -> bool {
    element
        .owner_document()
        .and_then(|document| document.active_element())
        .is_some_and(|focused| element.contains(Some(&focused)))
}

/// Puts `child` into a pane, replacing what was there.
fn put(pane: &HtmlElement, slot: &RefCell<Option<Box<dyn Widget>>>, child: &dyn Widget) {
    pane.set_inner_html("");
    let element: Element = child.native_element();
    if pane.append_child(&element).is_ok() {
        // 区画の中は縦の Flexbox。`SplitView` の区画と同じ扱いで中身を置く。
        apply_child_layout(&element, ParentLayout::Flex(Orientation::Vertical));
        *slot.borrow_mut() = Some(child.boxed_clone());
    }
}

/// The container for one pane (the same as `SplitView`'s panes).
fn pane(doc: &Document) -> Result<HtmlElement> {
    let element: HtmlElement = create(doc, "div")?.unchecked_into();
    style(&element, "display", "flex");
    style(&element, "flex-direction", "column");
    style(&element, "overflow", "hidden");
    style(&element, "box-sizing", "border-box");
    style(&element, "min-width", "0");
    style(&element, "min-height", "0");
    mark_parent(&element, ParentLayout::Flex(Orientation::Vertical));
    Ok(element)
}
