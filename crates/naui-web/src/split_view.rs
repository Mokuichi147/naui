//! A split view (`<div>` + `<div role="separator">`).
//!
//! **HTML has no element that "divides panes with a movable divider".** `resize`
//! only puts a handle in an element's corner and is not a mechanism for placing a
//! divider between panes, so, like `Toast`, this is one of the few exceptions naui
//! builds itself.
//!
//! What is built is **only the position and the hit area**; the cursor is left to
//! CSS's standard `col-resize` / `row-resize`. Browsers have no standard look for
//! it, so naui draws no picture.
//!
//! **Only a 1 px line is visible**; the rest is a transparent grab area (a `border`
//! drawn on one side only, with `box-sizing: border-box`). A filled band would look
//! less like a separator and more like a component of its own, standing out from its
//! surroundings. The line color is the system color `GrayText`. `ButtonBorder` and
//! `CanvasText` are **the exact opposite of the background** in some browsers
//! (white in dark mode, black in light mode), too strong for a separator, whereas
//! `GrayText` falls between the background and text colors in either scheme.
//!
//! The divider has `role="separator"` + `tabindex="0"`, so **it can be moved with
//! the keyboard too** (10 px per arrow key press). `aria-valuenow` reports the
//! current position to screen readers.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use naui_core::{clamp_split_position, Orientation, Result, DEFAULT_SPLIT_POSITION};
use wasm_bindgen::JsCast;
use web_sys::{Document, Element, HtmlElement, KeyboardEvent, PointerEvent};

use crate::layout::{apply_child_layout, mark_parent, ParentLayout};
use crate::to_error;
use crate::widgets::{create, impl_widget, Listener, ValueHandler, Widget};

/// The thickness the divider occupies (logical pixels). Wide enough to grab with a
/// finger.
///
/// Of this, only the 1 px of [`DIVIDER_LINE`] is visible; the rest is transparent.
const DIVIDER_THICKNESS: f64 = 6.0;
/// The thickness of the visible line (logical pixels).
const DIVIDER_LINE: f64 = 1.0;
/// How much one arrow key press moves it (logical pixels).
const KEY_STEP: f64 = 10.0;

fn style(element: &HtmlElement, property: &str, value: &str) {
    let _ = element.style().set_property(property, value);
}

struct SplitViewInner {
    /// A Flexbox `<div>` laying out the three side by side (or stacked).
    element: HtmlElement,
    start_pane: HtmlElement,
    divider: HtmlElement,
    end_pane: HtmlElement,
    orientation: Orientation,
    /// Holds the pane's handle, keeping it alive along with its callbacks.
    start: RefCell<Option<Box<dyn Widget>>>,
    end: RefCell<Option<Box<dyn Widget>>>,
    position: Cell<f64>,
    min_start: Cell<f64>,
    min_end: Cell<f64>,
    /// The pointer being dragged. `None` while not grabbed.
    dragging: Cell<Option<i32>>,
    /// The offset between where it was grabbed and the line's position. Remembered so
    /// the line does not jump under the finger.
    grab: Cell<f64>,
    handler: ValueHandler<f64>,
    /// The relays are held with the same lifetime as the handle (so it can be moved from
    /// inside the notification).
    listeners: RefCell<Vec<Listener>>,
    /// `on_file_drop`.
    file_drop: crate::file_drop::FileDrop,
}

/// A container that divides two panes with a movable divider.
#[derive(Clone)]
pub struct SplitView(Rc<SplitViewInner>);
impl_widget!(SplitView, element);

impl SplitView {
    pub(crate) fn new(doc: &Document, orientation: Orientation) -> Result<Self> {
        let vertical = orientation.is_vertical();
        let element: HtmlElement = create(doc, "div")?.unchecked_into();
        style(&element, "display", "flex");
        style(
            &element,
            "flex-direction",
            if vertical { "column" } else { "row" },
        );
        style(&element, "align-items", "stretch");
        style(&element, "min-width", "0");
        style(&element, "min-height", "0");

        let start_pane = pane(doc)?;
        let end_pane = pane(doc)?;
        // start は指定した大きさを保ち、余りは end が受け取る。
        // せまいときだけ start が縮む (end は自分の最小まで守られる)。
        style(&start_pane, "flex", "0 1 auto");
        style(&end_pane, "flex", "1 1 0");

        let divider: HtmlElement = create(doc, "div")?.unchecked_into();
        divider
            .set_attribute("role", "separator")
            .map_err(|e| to_error("build the split view", e))?;
        let _ = divider.set_attribute(
            "aria-orientation",
            // 区画が横に並ぶとき、仕切りそのものは縦向き。
            if vertical { "horizontal" } else { "vertical" },
        );
        let _ = divider.set_attribute("tabindex", "0");
        style(&divider, "flex", "0 0 auto");
        style(&divider, "align-self", "stretch");
        // 見えるのは区画の境目に引く 1 px の線だけ。残りは透明なつかみ代。
        style(&divider, "box-sizing", "border-box");
        style(&divider, "background", "transparent");
        let line = format!("{DIVIDER_LINE}px solid GrayText");
        if vertical {
            style(&divider, "height", &format!("{DIVIDER_THICKNESS}px"));
            style(&divider, "border-top", &line);
            style(&divider, "cursor", "row-resize");
        } else {
            style(&divider, "width", &format!("{DIVIDER_THICKNESS}px"));
            style(&divider, "border-left", &line);
            style(&divider, "cursor", "col-resize");
        }
        // ドラッグ中に中身の文字が選択されてしまわないようにする。
        style(&divider, "touch-action", "none");
        style(&divider, "user-select", "none");

        for part in [&start_pane, &divider, &end_pane] {
            element
                .append_child(part)
                .map_err(|e| to_error("build the split view", e))?;
        }

        let this = Self(Rc::new(SplitViewInner {
            element,
            start_pane,
            divider,
            end_pane,
            orientation,
            start: RefCell::new(None),
            end: RefCell::new(None),
            position: Cell::new(DEFAULT_SPLIT_POSITION),
            min_start: Cell::new(0.0),
            min_end: Cell::new(0.0),
            dragging: Cell::new(None),
            grab: Cell::new(0.0),
            handler: ValueHandler::default(),
            listeners: RefCell::new(Vec::new()),
            file_drop: Default::default(),
        }));
        this.0.write_position();
        this.attach_listeners()?;
        Ok(this)
    }

    /// Connects how the divider is grabbed (pointer and keyboard).
    fn attach_listeners(&self) -> Result<()> {
        let mut listeners = Vec::new();
        let divider = self.0.divider.clone();

        // 中継が持つのは弱い参照だけにする (強い参照だと、ハンドルを捨てても
        // 中継ごと自分自身が生き残ってしまう)。
        let weak: Weak<SplitViewInner> = Rc::downgrade(&self.0);

        let down = {
            let weak = weak.clone();
            Listener::attach_event(divider.as_ref(), "pointerdown", move |event| {
                let (Some(inner), Ok(event)) = (weak.upgrade(), event.dyn_into::<PointerEvent>())
                else {
                    return;
                };
                inner.dragging.set(Some(event.pointer_id()));
                // つかんだ場所と線とのずれを保つ (どこをつかんでも線が
                // 指の下へ飛ばないようにする)。
                inner
                    .grab
                    .set(inner.pointer_offset(&event) - inner.position.get());
                // つかんでいる間は、仕切りの外へ出てもイベントを受け取る。
                let _ = inner.divider.set_pointer_capture(event.pointer_id());
                event.prevent_default();
            })?
        };
        listeners.push(down);

        let moved = {
            let weak = weak.clone();
            Listener::attach_event(divider.as_ref(), "pointermove", move |event| {
                let (Some(inner), Ok(event)) = (weak.upgrade(), event.dyn_into::<PointerEvent>())
                else {
                    return;
                };
                if inner.dragging.get() != Some(event.pointer_id()) {
                    return;
                }
                event.prevent_default();
                let position = inner.pointer_offset(&event) - inner.grab.get();
                inner.move_to(position);
            })?
        };
        listeners.push(moved);

        for name in ["pointerup", "pointercancel"] {
            let weak = weak.clone();
            listeners.push(Listener::attach_event(
                divider.as_ref(),
                name,
                move |event| {
                    let (Some(inner), Ok(event)) =
                        (weak.upgrade(), event.dyn_into::<PointerEvent>())
                    else {
                        return;
                    };
                    if inner.dragging.get() != Some(event.pointer_id()) {
                        return;
                    }
                    inner.dragging.set(None);
                    let _ = inner.divider.release_pointer_capture(event.pointer_id());
                },
            )?);
        }

        let keys = Listener::attach_event(divider.as_ref(), "keydown", move |event| {
            let (Some(inner), Ok(event)) = (weak.upgrade(), event.dyn_into::<KeyboardEvent>())
            else {
                return;
            };
            let (back, forward) = if inner.orientation.is_vertical() {
                ("ArrowUp", "ArrowDown")
            } else {
                ("ArrowLeft", "ArrowRight")
            };
            let key = event.key();
            let step = if key == back {
                -KEY_STEP
            } else if key == forward {
                KEY_STEP
            } else {
                return;
            };
            event.prevent_default();
            inner.move_to(inner.position.get() + step);
        })?;
        listeners.push(keys);

        *self.0.listeners.borrow_mut() = listeners;
        Ok(())
    }

    /// The layout direction. With `Horizontal`, the panes are side by side (the divider
    /// is vertical).
    pub fn orientation(&self) -> Orientation {
        self.0.orientation
    }

    /// The left (or top) pane. Each call replaces it.
    pub fn set_start(&self, child: &dyn Widget) {
        self.0.set_pane(true, child);
    }

    /// The right (or bottom) pane. Each call replaces it.
    pub fn set_end(&self, child: &dyn Widget) {
        self.0.set_pane(false, child);
    }

    /// The current divider position (size of the start side, logical pixels).
    pub fn position(&self) -> f64 {
        self.0.position.get()
    }

    /// Moves the divider. `on_resize` is not called.
    pub fn set_position(&self, position: f64) {
        self.0.position.set(self.0.clamp(position));
        self.0.write_position();
    }

    /// Moves the divider as if the user had dragged it. Calls `on_resize`.
    pub fn drag_to(&self, position: f64) {
        self.0.move_to(position);
    }

    /// The minimum sizes of the two panes. Both default to 0.
    pub fn set_min_sizes(&self, start: f64, end: f64) {
        self.0.min_start.set(start.max(0.0));
        self.0.min_end.set(end.max(0.0));
        // いまの位置が範囲の外なら押し戻す (通知はしない)。
        self.0.position.set(self.0.clamp(self.0.position.get()));
        self.0.write_position();
    }

    /// Called with the new position every time the user moves the divider.
    pub fn on_resize(&self, f: impl FnMut(f64) + 'static) {
        self.0.handler.set(f);
    }

    /// The divider's `<div>`. Public as a backend-specific escape hatch.
    pub fn native_divider(&self) -> HtmlElement {
        self.0.divider.clone()
    }

    /// The `<div>` of the start-side pane. Used to place elements that are not widgets
    /// directly (the `<aside>` of [`Sidebar`](crate::Sidebar)).
    pub(crate) fn start_element(&self) -> HtmlElement {
        self.0.start_pane.clone()
    }

    /// The `<div>` of the end-side pane.
    pub(crate) fn end_element(&self) -> HtmlElement {
        self.0.end_pane.clone()
    }

    /// Hides (`true`) or shows (`false`) the start-side pane and the divider.
    ///
    /// While hidden, the end side takes everything. The position is remembered, so
    /// showing it again restores the original size.
    ///
    /// The `hidden` attribute alone does not make it disappear: the pane has an inline
    /// `display: flex`, which is stronger than the browser's default
    /// `[hidden] { display: none }`. So `display` is switched together with the
    /// attribute (which carries the meaning for screen readers).
    pub(crate) fn set_start_hidden(&self, hidden: bool) {
        self.0.start_pane.set_hidden(hidden);
        style(
            &self.0.start_pane,
            "display",
            if hidden { "none" } else { "flex" },
        );
        self.0.divider.set_hidden(hidden);
    }

    /// Whether the start-side pane and the divider are hidden.
    pub(crate) fn is_start_hidden(&self) -> bool {
        self.0.start_pane.hidden()
    }
}

impl SplitViewInner {
    fn set_pane(&self, is_start: bool, child: &dyn Widget) {
        let pane = if is_start {
            &self.start_pane
        } else {
            &self.end_pane
        };
        pane.set_inner_html("");
        let element = child.native_element();
        if pane.append_child(&element).is_ok() {
            // 区画の中は縦の Flexbox。`Stack` と同じ扱いで中身を置く。
            apply_child_layout(&element, ParentLayout::Flex(Orientation::Vertical));
            let slot = if is_start { &self.start } else { &self.end };
            *slot.borrow_mut() = Some(child.boxed_clone());
        }
    }

    /// The size the two panes can share (excluding the divider). 0 if still 0.
    fn total(&self) -> f64 {
        let rect = self.element.get_bounding_client_rect();
        let length = if self.orientation.is_vertical() {
            rect.height()
        } else {
            rect.width()
        };
        if length <= 0.0 {
            return 0.0;
        }
        (length - DIVIDER_THICKNESS).max(0.0)
    }

    fn clamp(&self, position: f64) -> f64 {
        clamp_split_position(
            position,
            self.total(),
            self.min_start.get(),
            self.min_end.get(),
        )
    }

    /// Reinterprets the pointer's position as the distance from the split view's start
    /// edge.
    fn pointer_offset(&self, event: &PointerEvent) -> f64 {
        let rect = self.element.get_bounding_client_rect();
        if self.orientation.is_vertical() {
            f64::from(event.client_y()) - rect.top()
        } else {
            f64::from(event.client_x()) - rect.left()
        }
    }

    /// Handling when the user moves it. Notifies only when it changed.
    fn move_to(&self, position: f64) {
        let clamped = self.clamp(position);
        if (clamped - self.position.get()).abs() < 0.5 {
            return;
        }
        self.position.set(clamped);
        self.write_position();
        self.handler.emit(clamped);
    }

    /// Writes the current position and the minimum sizes into CSS and ARIA.
    fn write_position(&self) {
        let position = self.position.get();
        let (size, min) = if self.orientation.is_vertical() {
            ("height", "min-height")
        } else {
            ("width", "min-width")
        };
        style(&self.start_pane, size, &format!("{position}px"));
        style(
            &self.start_pane,
            min,
            &format!("{}px", self.min_start.get()),
        );
        style(&self.end_pane, min, &format!("{}px", self.min_end.get()));
        let _ = self
            .divider
            .set_attribute("aria-valuenow", &format!("{}", position.round()));
    }
}

/// The `<div>` for one pane. Its inside is a vertical Flexbox.
fn pane(doc: &Document) -> Result<HtmlElement> {
    let element: HtmlElement = create(doc, "div")?.unchecked_into();
    style(&element, "display", "flex");
    style(&element, "flex-direction", "column");
    style(&element, "overflow", "hidden");
    style(&element, "box-sizing", "border-box");
    mark_parent(&element, ParentLayout::Flex(Orientation::Vertical));
    Ok(element)
}
