//! 分割ビュー (`<div>` + `<div role="separator">`)。
//!
//! **HTML に「動かせる仕切りで区画を分ける」要素は無い。** `resize` は
//! 要素の隅につまみを出すだけで、区画の間に仕切りを立てる仕組みではないため、
//! `Toast` と同じく naui が組み立てる数少ない例外になる。
//!
//! 組み立てるのは**位置と当たり判定だけ**で、仕切りの色は CSS のシステム
//! カラー (`ButtonBorder`)、カーソルは CSS の標準の `col-resize` /
//! `row-resize` に任せる。ブラウザに標準の見た目が無いので、naui が絵を描く
//! ことはしない。
//!
//! 仕切りは `role="separator"` + `tabindex="0"` なので、**キーボードでも
//! 動かせる** (矢印キーで 10 px ずつ)。読み上げには `aria-valuenow` で
//! いまの位置を伝える。

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use naui_core::{clamp_split_position, Orientation, Result, DEFAULT_SPLIT_POSITION};
use wasm_bindgen::JsCast;
use web_sys::{Document, Element, HtmlElement, KeyboardEvent, PointerEvent};

use crate::layout::{apply_child_layout, mark_parent, ParentLayout};
use crate::to_error;
use crate::widgets::{create, impl_widget, Listener, ValueHandler, Widget};

/// 仕切りの太さ (論理ピクセル)。指でもつまめる幅にしてある。
const DIVIDER_THICKNESS: f64 = 6.0;
/// 矢印キー 1 回で動く量 (論理ピクセル)。
const KEY_STEP: f64 = 10.0;

fn style(element: &HtmlElement, property: &str, value: &str) {
    let _ = element.style().set_property(property, value);
}

struct SplitViewInner {
    /// 3 つを横 (縦) に並べる Flexbox の `<div>`。
    element: HtmlElement,
    start_pane: HtmlElement,
    divider: HtmlElement,
    end_pane: HtmlElement,
    orientation: Orientation,
    /// 区画のハンドルを保持し、コールバックごと生かしておく。
    start: RefCell<Option<Box<dyn Widget>>>,
    end: RefCell<Option<Box<dyn Widget>>>,
    position: Cell<f64>,
    min_start: Cell<f64>,
    min_end: Cell<f64>,
    /// ドラッグ中のポインター。つかんでいない間は `None`。
    dragging: Cell<Option<i32>>,
    handler: ValueHandler<f64>,
    /// 中継はハンドルと同じ寿命で持つ (通知の中から動かせるように)。
    listeners: RefCell<Vec<Listener>>,
}

/// 2 つの区画を、動かせる仕切りで分けるコンテナ。
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
            .map_err(|e| to_error("分割ビューの組み立て", e))?;
        let _ = divider.set_attribute(
            "aria-orientation",
            // 区画が横に並ぶとき、仕切りそのものは縦向き。
            if vertical { "horizontal" } else { "vertical" },
        );
        let _ = divider.set_attribute("tabindex", "0");
        style(&divider, "flex", "0 0 auto");
        style(&divider, "align-self", "stretch");
        style(&divider, "background", "ButtonBorder");
        if vertical {
            style(&divider, "height", &format!("{DIVIDER_THICKNESS}px"));
            style(&divider, "cursor", "row-resize");
        } else {
            style(&divider, "width", &format!("{DIVIDER_THICKNESS}px"));
            style(&divider, "cursor", "col-resize");
        }
        // ドラッグ中に中身の文字が選択されてしまわないようにする。
        style(&divider, "touch-action", "none");
        style(&divider, "user-select", "none");

        for part in [&start_pane, &divider, &end_pane] {
            element
                .append_child(part)
                .map_err(|e| to_error("分割ビューの組み立て", e))?;
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
            handler: ValueHandler::default(),
            listeners: RefCell::new(Vec::new()),
        }));
        this.0.write_position();
        this.attach_listeners()?;
        Ok(this)
    }

    /// 仕切りのつまみ方 (ポインターとキーボード) をつなぐ。
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
                let position = inner.position_from_pointer(&event);
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

    /// 並べる向き。`Horizontal` なら区画が横に並ぶ (仕切りは縦)。
    pub fn orientation(&self) -> Orientation {
        self.0.orientation
    }

    /// 左 (または上) の区画。呼ぶたびに置き換わる。
    pub fn set_start(&self, child: &dyn Widget) {
        self.0.set_pane(true, child);
    }

    /// 右 (または下) の区画。呼ぶたびに置き換わる。
    pub fn set_end(&self, child: &dyn Widget) {
        self.0.set_pane(false, child);
    }

    /// いまの仕切りの位置 (start 側の大きさ、論理ピクセル)。
    pub fn position(&self) -> f64 {
        self.0.position.get()
    }

    /// 仕切りを動かす。`on_resize` は呼ばれない。
    pub fn set_position(&self, position: f64) {
        self.0.position.set(self.0.clamp(position));
        self.0.write_position();
    }

    /// 利用者がドラッグしたのと同じく仕切りを動かす。`on_resize` を呼ぶ。
    pub fn drag_to(&self, position: f64) {
        self.0.move_to(position);
    }

    /// 両側の区画の最小の大きさ。既定はどちらも 0。
    pub fn set_min_sizes(&self, start: f64, end: f64) {
        self.0.min_start.set(start.max(0.0));
        self.0.min_end.set(end.max(0.0));
        // いまの位置が範囲の外なら押し戻す (通知はしない)。
        self.0.position.set(self.0.clamp(self.0.position.get()));
        self.0.write_position();
    }

    /// 利用者が仕切りを動かすたび、動いた後の位置で呼ばれる。
    pub fn on_resize(&self, f: impl FnMut(f64) + 'static) {
        self.0.handler.set(f);
    }

    /// 仕切りの `<div>`。バックエンド固有の脱出口として公開している。
    pub fn native_divider(&self) -> HtmlElement {
        self.0.divider.clone()
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

    /// 2 つの区画が分け合える大きさ (仕切りの分を除く)。まだ 0 なら 0。
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

    /// ポインターの位置を、start 側の大きさへ読み替える。
    fn position_from_pointer(&self, event: &PointerEvent) -> f64 {
        let rect = self.element.get_bounding_client_rect();
        if self.orientation.is_vertical() {
            f64::from(event.client_y()) - rect.top() - DIVIDER_THICKNESS / 2.0
        } else {
            f64::from(event.client_x()) - rect.left() - DIVIDER_THICKNESS / 2.0
        }
    }

    /// 利用者が動かしたときの処理。変わったときだけ通知する。
    fn move_to(&self, position: f64) {
        let clamped = self.clamp(position);
        if (clamped - self.position.get()).abs() < 0.5 {
            return;
        }
        self.position.set(clamped);
        self.write_position();
        self.handler.emit(clamped);
    }

    /// いまの位置と最小の大きさを CSS と ARIA へ書く。
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

/// 区画 1 つぶんの `<div>`。中は縦の Flexbox。
fn pane(doc: &Document) -> Result<HtmlElement> {
    let element: HtmlElement = create(doc, "div")?.unchecked_into();
    style(&element, "display", "flex");
    style(&element, "flex-direction", "column");
    style(&element, "overflow", "hidden");
    style(&element, "box-sizing", "border-box");
    mark_parent(&element, ParentLayout::Flex(Orientation::Vertical));
    Ok(element)
}
