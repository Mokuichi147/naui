//! The drawing surface (a `Grid` showing a Composition drawing surface that
//! Direct2D draws on).
//!
//! WinUI 3 has no "surface the app paints pixels on". Win2D is a separate package
//! rather than part of the Windows App SDK, so the surface is put together from
//! what the SDK has: a `CompositionDrawingSurface` made from a
//! `CompositionGraphicsDevice` backed by a Direct2D device, painted with a
//! `CompositionSurfaceBrush` on a `SpriteVisual` that
//! `ElementCompositionPreview::SetElementChildVisual` places over the `Grid`.
//! Like the other three platforms (Core Graphics, cairo, `<canvas>`), the commands
//! are rasterized by a 2D API (Direct2D, with DirectWrite for text; see
//! `render.rs`).
//!
//! On every redraw, the commands `on_draw` recorded in the [`Painter`] are replayed
//! onto the whole surface. The surface is sized in physical pixels (the logical size
//! times `XamlRoot.RasterizationScale`) and the visual in logical pixels, so one
//! surface pixel is one screen pixel. A change of scale alone (moving to another
//! display) does not change the size, so `XamlRoot.Changed` is watched too.
//!
//! Earlier versions turned the commands into one XAML string of `Path` and
//! `TextBlock` elements for `XamlReader`. Drawings with thousands of commands took
//! over half a second per redraw that way, most of it parsing the string and
//! rendering the elements, so it was replaced.
//!
//! Size changes are caught with `SizeChanged`, and the pointer comes from
//! `PointerPressed` / `PointerMoved` / `PointerReleased`. Whether the cursor is over
//! the surface is remembered from those plus the mouse and touchpad share of
//! `PointerEntered` / `PointerExited`.
//!
//! The button is identified by `PointerPointProperties`'s `IsLeftButtonPressed` and
//! friends. WinUI reports presses and releases of second and later buttons
//! (pressing right while holding left, for example) through `PointerMoved`, so the
//! set of buttons held is remembered per pointer, and differences from last time
//! are delivered as presses and releases (treated the same whichever notification
//! they come through).
//!
//! The wheel amount is 120 per notch (`WHEEL_DELTA`), so 120 is converted to
//! [`WHEEL_STEP`]. The vertical amount is "positive when turned away", the opposite
//! of the other three platforms, so the sign is flipped; horizontal is positive to
//! the right and delivered as is.
//!
//! The wheel arrives by two routes. A real mouse wheel sometimes never reaches
//! XAML (a DPI-unaware window with focus gets none), so naui first receives wheels
//! to the foreground window with a `WH_MOUSE_LL` hook, and in a DPI-unaware window
//! also moves `Scroll` there (`layout.rs`). This hook first delivers to
//! a surface with `on_wheel` under the pointer, if any ([`dispatch_wheel`]); with
//! `Handled`, the message is discarded and not passed to the outer `Scroll`. When
//! the hook does not receive it (the window is not in the foreground, for example),
//! it is received through `PointerWheelChanged`, setting `Handled` for `Handled`. A
//! wheel that the hook delivered and got `Continue` for, arriving from XAML too, is
//! discarded as the second delivery.

mod render;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};
use std::sync::Arc;
use std::time::{Duration, Instant};

use naui_core::{
    DrawCommand, EventResponse, Painter, Point, PointerButton, PointerEvent, PointerPhase, Result,
    WheelHandler, WHEEL_STEP,
};
use naui_winui3::ICompositionDrawingSurfaceInterop;
use naui_winui3::Microsoft::Graphics::DirectX::{DirectXAlphaMode, DirectXPixelFormat};
use naui_winui3::Microsoft::UI::Composition::{
    CompositionDrawingSurface, CompositionStretch, SpriteVisual,
};
use naui_winui3::Microsoft::UI::Dispatching::{DispatcherQueue, DispatcherQueueHandler};
use naui_winui3::Microsoft::UI::Input::{PointerDeviceType, PointerPointProperties};
use naui_winui3::Microsoft::UI::Xaml::Controls::Grid as XamlGrid;
use naui_winui3::Microsoft::UI::Xaml::Hosting::ElementCompositionPreview;
use naui_winui3::Microsoft::UI::Xaml::Input::{PointerEventHandler, PointerRoutedEventArgs};
use naui_winui3::Microsoft::UI::Xaml::Markup::XamlReader;
use naui_winui3::Microsoft::UI::Xaml::{
    FrameworkElement, SizeChangedEventHandler, UIElement, XamlRoot, XamlRootChangedEventArgs,
};
use windows::Foundation::TypedEventHandler;
use windows::Graphics::SizeInt32;
use windows::Win32::Graphics::Direct2D::ID2D1DeviceContext;
use windows_core::{Interface, HSTRING};
use windows_numerics::Vector2;

use crate::to_error;
use crate::ui_thread::{HandlerCell, UiThreadCell};
use crate::widgets::{impl_widget, Widget};

/// The base of the surface. With `Background="Transparent"`, pointer hit-testing
/// remains even where nothing is drawn (`null` lets it pass through).
const HOST_XAML: &str = r##"<Grid
    xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
    Background="Transparent"/>"##;

/// The slot for the closure that decides what to draw.
///
/// It is called from a WinRT event (`SizeChanged`), so it is placed in a
/// [`UiThreadCell`] and taken out while it is being called (the same rule as
/// `ColorHandler` and others).
#[derive(Clone)]
struct DrawHandler(HandlerCell<dyn FnMut(&mut Painter)>);

impl DrawHandler {
    fn new() -> Self {
        Self(Arc::new(UiThreadCell::new(None)))
    }

    fn set(&self, f: impl FnMut(&mut Painter) + 'static) {
        self.0.with_mut(|slot| *slot = Some(Box::new(f)));
    }

    /// Calls it and returns whether it drew. `false` if not set yet.
    fn emit(&self, painter: &mut Painter) -> bool {
        let Some(Some(mut f)) = self.0.try_with_mut(|slot| slot.take()) else {
            return false;
        };
        f(painter);
        let _ = self.0.try_with_mut(|slot| {
            if slot.is_none() {
                *slot = Some(f);
            }
        });
        true
    }
}

#[derive(Clone)]
struct PointerHandler(HandlerCell<dyn FnMut(PointerEvent)>);

impl PointerHandler {
    fn new() -> Self {
        Self(Arc::new(UiThreadCell::new(None)))
    }

    fn set(&self, f: impl FnMut(PointerEvent) + 'static) {
        self.0.with_mut(|slot| *slot = Some(Box::new(f)));
    }

    fn emit(&self, event: PointerEvent) {
        let Some(Some(mut f)) = self.0.try_with_mut(|slot| slot.take()) else {
            return;
        };
        f(event);
        let _ = self.0.try_with_mut(|slot| {
            if slot.is_none() {
                *slot = Some(f);
            }
        });
    }
}

struct CanvasInner {
    native: XamlGrid,
    draw: DrawHandler,
    pointer: PointerHandler,
    wheel: WheelHandler,
    /// The set of buttons currently held, per pointer (a sum of [`button_bit`]s).
    pressed: RefCell<HashMap<u32, u8>>,
    /// Whether a redraw is queued on the `DispatcherQueue`.
    scheduled: Cell<bool>,
    /// The last position of the cursor over the surface ([`moves_cursor`]). Remembered
    /// on entering and on press, move and release; `None` once it leaves. Used to decide
    /// the position of wheels delivered from the hook and which surface to deliver to.
    hover: Cell<Option<Point>>,
    /// Whether it was added to [`WHEEL_CANVASES`] (whether `on_wheel` was set).
    wheel_registered: Cell<bool>,
    /// When the hook last delivered a wheel and got `Continue`. If the same wheel also
    /// arrives through `PointerWheelChanged`, it is discarded.
    hook_wheel: Cell<Option<Instant>>,
    /// The Composition surface drawn on. Made at the first draw with a size.
    surface: RefCell<Option<Surface>>,
    /// The `XamlRoot` whose `Changed` is watched, with the subscription's token.
    root: RefCell<Option<(XamlRoot, i64)>>,
    /// The `RasterizationScale` of the last draw.
    scale: Cell<f64>,
}

/// The visual placed over the `Grid` and the surface its brush shows.
struct Surface {
    visual: SpriteVisual,
    surface: CompositionDrawingSurface,
    /// The surface's size in physical pixels.
    pixels: (u32, u32),
}

thread_local! {
    /// The surfaces with `on_wheel` set. The destination of wheels received by the hook
    /// is looked up here.
    static WHEEL_CANVASES: RefCell<Vec<Weak<CanvasInner>>> = const { RefCell::new(Vec::new()) };
    /// Every surface on this thread. All of them are drawn again after the device is
    /// lost.
    static CANVASES: RefCell<Vec<Weak<CanvasInner>>> = const { RefCell::new(Vec::new()) };
    /// How many `XamlRoot.Changed` subscriptions surfaces hold. Read by tests to see
    /// that dropped surfaces unsubscribe.
    static ROOT_WATCHES: Cell<usize> = const { Cell::new(0) };
}

/// How long until `PointerWheelChanged` is treated as the same wheel the hook
/// delivered.
const HOOK_WHEEL_ECHO: Duration = Duration::from_millis(250);

/// Turns `MouseWheelDelta` (120 per notch) into the amount delivered (logical
/// pixels).
fn wheel_amount(delta: f64, horizontal: bool) -> (f64, f64) {
    let amount = delta / 120.0 * WHEEL_STEP;
    if horizontal {
        (amount, 0.0)
    } else {
        (0.0, -amount)
    }
}

/// Delivers a wheel received by the hook to a surface with `on_wheel` under the
/// pointer.
///
/// `delta` is the amount of `WM_MOUSEWHEEL` / `WM_MOUSEHWHEEL` (120 per notch).
/// `None` if there is no destination; otherwise whether the app returned
/// `Handled`.
pub(crate) fn dispatch_wheel(delta: f64, horizontal: bool) -> Option<bool> {
    // アプリのクロージャを呼ぶ前に借用を返す (中で面を作るかもしれない)。
    let (inner, point) = WHEEL_CANVASES.with(|canvases| {
        let mut canvases = canvases.borrow_mut();
        canvases.retain(|canvas| canvas.strong_count() != 0);
        canvases.iter().rev().find_map(|weak| {
            let inner = weak.upgrade()?;
            let point = inner.hover.get()?;
            Some((inner, point))
        })
    })?;
    let (x, y) = wheel_amount(delta, horizontal);
    let handled = inner.wheel.emit(PointerEvent::wheel(point, x, y)) == EventResponse::Handled;
    // `Handled` ならメッセージはフックで止まり、XAML へは届かない。
    inner.hook_wheel.set((!handled).then(Instant::now));
    Some(handled)
}

/// A surface the app draws on itself (`Grid` + a Composition surface Direct2D draws on).
///
/// Its size is not determined by its contents, so set it with `set_sizing` (the
/// same as `Scroll`).
#[derive(Clone)]
pub struct Canvas(Rc<CanvasInner>);
impl_widget!(Canvas, native);

impl Canvas {
    pub(crate) fn new() -> Result<Self> {
        let native: XamlGrid = XamlReader::Load(&HSTRING::from(HOST_XAML))
            .and_then(|element| element.cast::<XamlGrid>())
            .map_err(|e| to_error("create the Canvas", e))?;
        let this = Self(Rc::new(CanvasInner {
            native,
            draw: DrawHandler::new(),
            pointer: PointerHandler::new(),
            wheel: WheelHandler::default(),
            pressed: RefCell::new(HashMap::new()),
            scheduled: Cell::new(false),
            hover: Cell::new(None),
            wheel_registered: Cell::new(false),
            hook_wheel: Cell::new(None),
            surface: RefCell::new(None),
            root: RefCell::new(None),
            scale: Cell::new(1.0),
        }));
        CANVASES.with(|canvases| {
            let mut canvases = canvases.borrow_mut();
            canvases.retain(|canvas| canvas.strong_count() != 0);
            canvases.push(Rc::downgrade(&this.0));
        });
        this.track_size()?;
        this.track_pointer()?;
        Ok(this)
    }

    /// Redraws when the size is decided (changes). The first draw starts here too.
    fn track_size(&self) -> Result<()> {
        let target = UiThreadCell::new(Rc::downgrade(&self.0));
        let changed = SizeChangedEventHandler::new(move |_, _| {
            let _ = target.try_with_mut(|weak| {
                if let Some(inner) = weak.upgrade() {
                    inner.paint();
                }
            });
            Ok(())
        });
        self.0
            .native
            .cast::<FrameworkElement>()
            .and_then(|element| element.SizeChanged(&changed))
            .map_err(|e| to_error("subscribe to Canvas size changes", e))?;
        Ok(())
    }

    fn track_pointer(&self) -> Result<()> {
        let element = self
            .0
            .native
            .cast::<UIElement>()
            .map_err(|e| to_error("subscribe to Canvas pointer events", e))?;
        for phase in [PointerPhase::Down, PointerPhase::Move, PointerPhase::Up] {
            let target = UiThreadCell::new(Rc::downgrade(&self.0));
            let handler = PointerEventHandler::new(move |_, args| {
                let Some(args) = args.as_ref() else {
                    return Ok(());
                };
                let _ = target.try_with_mut(|weak: &mut Weak<CanvasInner>| {
                    let Some(inner) = weak.upgrade() else {
                        return;
                    };
                    let Ok(root) = inner.native.cast::<UIElement>() else {
                        return;
                    };
                    // 押している間は面の外へ出ても動きと解放が届くようにする。
                    if let Ok(pointer) = args.Pointer() {
                        match phase {
                            PointerPhase::Down => {
                                let _ = root.CapturePointer(&pointer);
                            }
                            PointerPhase::Up => {
                                let _ = root.ReleasePointerCapture(&pointer);
                            }
                            _ => {}
                        }
                    }
                    let Ok(current) = args.GetCurrentPoint(&root) else {
                        return;
                    };
                    let Ok(position) = current.Position() else {
                        return;
                    };
                    let point = Point::new(f64::from(position.X), f64::from(position.Y));
                    // 捕捉中は面の外の位置も届くので、面の上のときだけ覚える。
                    if moves_cursor(args) {
                        let (width, height) = inner.actual_size();
                        let inside =
                            (0.0..=width).contains(&point.x) && (0.0..=height).contains(&point.y);
                        inner.hover.set(inside.then_some(point));
                    }
                    let now = current
                        .Properties()
                        .map(|properties| pressed_buttons(&properties))
                        .unwrap_or(0);
                    let id = current.PointerId().unwrap_or(0);
                    let seen = {
                        let mut pressed = inner.pressed.borrow_mut();
                        if now == 0 {
                            pressed.remove(&id)
                        } else {
                            pressed.insert(id, now)
                        }
                    };
                    let before = previous_buttons(phase, seen, now);
                    for (phase, button) in button_changes(phase, before, now) {
                        inner
                            .pointer
                            .emit(PointerEvent::new(phase, point).button(button));
                    }
                });
                Ok(())
            });
            match phase {
                PointerPhase::Down => element.PointerPressed(&handler),
                PointerPhase::Up => element.PointerReleased(&handler),
                _ => element.PointerMoved(&handler),
            }
            .map_err(|e| to_error("subscribe to Canvas pointer events", e))?;
        }

        // 動かずに面の上へ来ることもある (外のスライダーを引いて面の上で離すと、
        // 捕捉が解けたところで `PointerEntered` だけが届く)。そのまま回した
        // ホイールをフックが届けられるよう、入ったときにも位置を覚える。
        let target = UiThreadCell::new(Rc::downgrade(&self.0));
        let entered = PointerEventHandler::new(move |_, args| {
            let Some(args) = args.as_ref().filter(|args| moves_cursor(args)) else {
                return Ok(());
            };
            let _ = target.try_with_mut(|weak: &mut Weak<CanvasInner>| {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                let Ok(root) = inner.native.cast::<UIElement>() else {
                    return;
                };
                let Ok(position) = args.GetCurrentPoint(&root).and_then(|p| p.Position()) else {
                    return;
                };
                inner.hover.set(Some(Point::new(
                    f64::from(position.X),
                    f64::from(position.Y),
                )));
            });
            Ok(())
        });
        element
            .PointerEntered(&entered)
            .map_err(|e| to_error("subscribe to Canvas pointer events", e))?;

        let target = UiThreadCell::new(Rc::downgrade(&self.0));
        let exited = PointerEventHandler::new(move |_, args| {
            if !args.as_ref().is_some_and(moves_cursor) {
                return Ok(());
            }
            let _ = target.try_with_mut(|weak: &mut Weak<CanvasInner>| {
                if let Some(inner) = weak.upgrade() {
                    inner.hover.set(None);
                }
            });
            Ok(())
        });
        element
            .PointerExited(&exited)
            .map_err(|e| to_error("subscribe to Canvas pointer events", e))?;

        let target = UiThreadCell::new(Rc::downgrade(&self.0));
        let wheel = PointerEventHandler::new(move |_, args| {
            let Some(args) = args.as_ref() else {
                return Ok(());
            };
            let _ = target.try_with_mut(|weak: &mut Weak<CanvasInner>| {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                // フックがもう届けたホイール。`Continue` だったので Handled は立てない。
                if inner
                    .hook_wheel
                    .take()
                    .is_some_and(|at| at.elapsed() < HOOK_WHEEL_ECHO)
                {
                    return;
                }
                let Ok(root) = inner.native.cast::<UIElement>() else {
                    return;
                };
                let Ok(current) = args.GetCurrentPoint(&root) else {
                    return;
                };
                let (Ok(position), Ok(properties)) = (current.Position(), current.Properties())
                else {
                    return;
                };
                let (x, y) = wheel_amount(
                    f64::from(properties.MouseWheelDelta().unwrap_or(0)),
                    properties.IsHorizontalMouseWheel().unwrap_or(false),
                );
                let event = PointerEvent::wheel(
                    Point::new(f64::from(position.X), f64::from(position.Y)),
                    x,
                    y,
                );
                if inner.wheel.emit(event) == EventResponse::Handled {
                    let _ = args.SetHandled(true);
                }
            });
            Ok(())
        });
        element
            .PointerWheelChanged(&wheel)
            .map_err(|e| to_error("subscribe to Canvas wheel events", e))?;
        Ok(())
    }

    /// The closure that decides what to draw. Called with a fresh [`Painter`] every
    /// time it is redrawn.
    ///
    /// It is called after [`redraw`](Self::redraw) and when the surface changes size.
    pub fn on_draw(&self, f: impl FnMut(&mut Painter) + 'static) {
        self.0.draw.set(f);
        self.redraw();
    }

    /// Requests a redraw. Nothing is drawn on the spot; `on_draw` is called once the UI
    /// thread is free (calling it many times in a row collapses into one).
    pub fn redraw(&self) {
        schedule_paint(&self.0);
    }

    /// Pointer (mouse, touch, pen) presses, moves and releases. Positions are logical
    /// pixels with the surface's top left as origin. Moves also arrive while nothing is
    /// pressed (hover). Presses and releases carry which button
    /// ([`PointerEvent::button`]).
    pub fn on_pointer(&self, f: impl FnMut(PointerEvent) + 'static) {
        self.0.pointer.set(f);
    }

    /// The wheel. Amounts are logical pixels.
    ///
    /// Returning `Handled` keeps it from reaching the outer scroll view. With
    /// `Continue`, the outer `ScrollViewer` scrolls as usual.
    ///
    /// For surfaces inside a `Scroll`, two-finger scrolling on a precision touchpad
    /// does not arrive. The outer `ScrollViewer` receives touchpad gestures itself, and
    /// they become neither `PointerWheelChanged` nor wheel messages.
    pub fn on_wheel(&self, f: impl FnMut(PointerEvent) -> EventResponse + 'static) {
        self.0.wheel.set(f);
        if !self.0.wheel_registered.replace(true) {
            WHEEL_CANVASES.with(|canvases| canvases.borrow_mut().push(Rc::downgrade(&self.0)));
        }
    }

    /// The current size of the surface (width, height). 0 if not laid out yet.
    pub fn size(&self) -> (f64, f64) {
        self.0.actual_size()
    }

    /// Draws on the spot. **For automated tests only.**
    #[doc(hidden)]
    pub fn draw_for_test(&self) {
        self.0.paint();
    }

    /// Draws the `on_draw` commands for a surface of `width` × `height` logical pixels
    /// at `scale` into an offscreen bitmap, and returns its size in pixels and its
    /// pixels (premultiplied BGRA, top row first). **For automated tests only.**
    ///
    /// Lets you check what Direct2D draws without showing anything on screen. `None`
    /// when Direct2D cannot be used.
    #[doc(hidden)]
    pub fn pixels_for_test(
        &self,
        width: f64,
        height: f64,
        scale: f64,
    ) -> Option<(u32, u32, Vec<u8>)> {
        let mut painter = Painter::new(width, height);
        self.0.draw.emit(&mut painter);
        render::with_renderer(|renderer| renderer.pixels(width, height, scale, painter.commands()))?
            .ok()
    }

    /// Signals the device-removal event as Direct3D does after a GPU reset, so the
    /// devices are made again and every surface is redrawn. **For automated tests
    /// only.** Returns whether there was an event to signal (the surfaces share one
    /// set of devices per thread, made at the first draw).
    #[doc(hidden)]
    pub fn signal_device_removed_for_test(&self) -> bool {
        render::signal_removal_for_test()
    }

    /// How many `XamlRoot.Changed` subscriptions the surfaces on this thread hold.
    /// **For automated tests only.**
    #[doc(hidden)]
    pub fn root_watches_for_test() -> usize {
        ROOT_WATCHES.with(Cell::get)
    }

    /// The base `Grid`. Public as a backend-specific escape hatch.
    ///
    /// What is drawn is a `SpriteVisual` placed over it with
    /// `ElementCompositionPreview::SetElementChildVisual`, not one of its children.
    pub fn native_grid(&self) -> XamlGrid {
        self.0.native.clone()
    }
}

/// Queues a draw on the `DispatcherQueue`. Requests made before it runs collapse
/// into one.
fn schedule_paint(inner: &Rc<CanvasInner>) {
    if inner.scheduled.replace(true) {
        return;
    }
    let Ok(queue) = DispatcherQueue::GetForCurrentThread() else {
        inner.scheduled.set(false);
        inner.paint();
        return;
    };
    let target = UiThreadCell::new(Rc::downgrade(inner));
    let enqueued = queue.TryEnqueue(&DispatcherQueueHandler::new(move || {
        let _ = target.try_with_mut(|weak| {
            if let Some(inner) = weak.upgrade() {
                inner.scheduled.set(false);
                inner.paint();
            }
        });
        Ok(())
    }));
    if !matches!(enqueued, Ok(true)) {
        inner.scheduled.set(false);
        inner.paint();
    }
}

/// Releases the Direct2D and Composition devices the surfaces share. Called on the
/// way to shutting down, while Composition is still running (see `render::Slot`).
pub(crate) fn release_devices() {
    render::release();
}

/// Replaces lost devices and draws every surface again.
///
/// Called when a draw fails with a lost device, and when Direct3D reports the device
/// removed (`render::RemovalWatch`), which also covers surfaces that are never
/// redrawn by the app. `generation` is the devices that were lost; if they were
/// already replaced (both routes reported the same loss), nothing is done.
fn recover_devices(generation: u64) {
    if render::recover_if_current(generation) {
        repaint_all();
    }
}

/// Draws every surface on this thread again (after the device was replaced, which
/// empties them all).
fn repaint_all() {
    let canvases: Vec<Rc<CanvasInner>> = CANVASES.with(|canvases| {
        let mut canvases = canvases.borrow_mut();
        canvases.retain(|canvas| canvas.strong_count() != 0);
        canvases.iter().filter_map(Weak::upgrade).collect()
    });
    for canvas in &canvases {
        schedule_paint(canvas);
    }
}

/// Whether it is a pointer that moves the cursor (mouse and touchpad).
///
/// Wheels received by the hook go to the cursor's position, so `hover` is remembered
/// from these alone. Touch gets `PointerExited` when the finger lifts, so counting
/// it would erase the position even while the mouse is still over the surface.
fn moves_cursor(args: &PointerRoutedEventArgs) -> bool {
    let kind = args
        .Pointer()
        .and_then(|pointer| pointer.PointerDeviceType());
    !matches!(kind, Ok(PointerDeviceType::Touch | PointerDeviceType::Pen))
}

/// The buttons distinguished. Also the order in which presses and releases are
/// delivered.
const BUTTONS: [PointerButton; 5] = [
    PointerButton::Primary,
    PointerButton::Secondary,
    PointerButton::Middle,
    PointerButton::Back,
    PointerButton::Forward,
];

/// The bit of the button within a set of buttons.
fn button_bit(button: PointerButton) -> u8 {
    match button {
        PointerButton::Primary => 1,
        PointerButton::Secondary => 2,
        PointerButton::Middle => 4,
        PointerButton::Back => 8,
        PointerButton::Forward => 16,
        _ => 0,
    }
}

/// The set of buttons currently held. Touch and pen tips count as
/// `IsLeftButtonPressed`.
fn pressed_buttons(properties: &PointerPointProperties) -> u8 {
    let checks = [
        properties.IsLeftButtonPressed(),
        properties.IsRightButtonPressed(),
        properties.IsMiddleButtonPressed(),
        properties.IsXButton1Pressed(),
        properties.IsXButton2Pressed(),
    ];
    BUTTONS
        .iter()
        .zip(checks)
        .filter(|(_, pressed)| matches!(pressed, Ok(true)))
        .fold(0, |bits, (button, _)| bits | button_bit(*button))
}

/// The previous set of buttons to diff against.
///
/// `seen` is the remembered set; `None` for a pointer not remembered yet.
fn previous_buttons(phase: PointerPhase, seen: Option<u8>, now: u8) -> u8 {
    match (phase, seen) {
        // `PointerPressed` は何も押していない状態からの最初の押下でだけ
        // 届く。捕捉を失って解放を見逃したときの残りは捨てる。
        (PointerPhase::Down, _) => 0,
        (_, Some(before)) => before,
        // 面の外で押したまま入ってきたポインター。そのボタンは面の上で
        // 押したのではないので、いまの組を基準にし、この先の変化だけ届ける。
        (_, None) => now,
    }
}

/// Turns the difference between the previous and current button sets into the
/// sequence of notifications to deliver.
///
/// If there is no difference, WinUI's notification kind is delivered as is (without
/// a button).
fn button_changes(
    phase: PointerPhase,
    before: u8,
    now: u8,
) -> Vec<(PointerPhase, Option<PointerButton>)> {
    let mut changes = Vec::new();
    for button in BUTTONS {
        let bit = button_bit(button);
        if before & bit != 0 && now & bit == 0 {
            changes.push((PointerPhase::Up, Some(button)));
        }
    }
    for button in BUTTONS {
        let bit = button_bit(button);
        if before & bit == 0 && now & bit != 0 {
            changes.push((PointerPhase::Down, Some(button)));
        }
    }
    if changes.is_empty() {
        changes.push((phase, None));
    }
    changes
}

impl CanvasInner {
    fn actual_size(&self) -> (f64, f64) {
        let Ok(element) = self.native.cast::<FrameworkElement>() else {
            return (0.0, 0.0);
        };
        (
            element.ActualWidth().unwrap_or(0.0),
            element.ActualHeight().unwrap_or(0.0),
        )
    }

    /// The display scale of the window the surface is in (1.0 before it is in one).
    fn rasterization_scale(&self) -> f64 {
        self.native
            .XamlRoot()
            .and_then(|root| root.RasterizationScale())
            .ok()
            .filter(|scale| scale.is_finite() && *scale > 0.0)
            .unwrap_or(1.0)
    }

    /// Calls `on_draw` and replays the commands onto the surface.
    fn paint(self: &Rc<Self>) {
        let (width, height) = self.actual_size();
        if width <= 0.0 || height <= 0.0 {
            return;
        }
        self.watch_root();
        let scale = self.rasterization_scale();
        let mut painter = Painter::new(width, height);
        if !self.draw.emit(&mut painter) {
            return;
        }
        self.scale.set(scale);
        let drawn = render::with_renderer(|renderer| {
            self.present(renderer, width, height, scale, painter.commands())
        });
        if let Some(Err(error)) = drawn {
            if let Some(generation) = render::current_generation() {
                if render::is_device_lost(&error) {
                    recover_devices(generation);
                }
            }
        }
    }

    fn present(
        &self,
        renderer: &mut render::Renderer,
        width: f64,
        height: f64,
        scale: f64,
        commands: &[DrawCommand],
    ) -> windows_core::Result<()> {
        let pixels = (
            render::pixel_extent(width, scale),
            render::pixel_extent(height, scale),
        );
        let size = SizeInt32 {
            Width: pixels.0 as i32,
            Height: pixels.1 as i32,
        };
        let mut slot = self.surface.borrow_mut();
        match slot.as_mut() {
            Some(current) if current.pixels == pixels => {}
            Some(current) => {
                current.surface.Resize(size)?;
                current.pixels = pixels;
            }
            None => *slot = Some(self.attach_surface(renderer, size, pixels)?),
        }
        let Some(current) = slot.as_ref() else {
            return Ok(());
        };
        // 面の 1 画素が画面の 1 画素になるよう、見た目の大きさは画素数から決める。
        current.visual.SetSize(Vector2::new(
            (f64::from(pixels.0) / scale) as f32,
            (f64::from(pixels.1) / scale) as f32,
        ))?;
        let interop: ICompositionDrawingSurfaceInterop = current.surface.cast()?;
        // SAFETY: 成功した `BeginDraw` には必ず `EndDraw` を対にし、その前に
        // 描画用のコンテキストを手放す。
        unsafe {
            let (context, offset) = interop.BeginDraw::<ID2D1DeviceContext>()?;
            let drawn = renderer.draw_on_surface(
                &context,
                (offset.x, offset.y),
                pixels,
                scale,
                width,
                height,
                commands,
            );
            drop(context);
            let ended = interop.EndDraw();
            drawn.and(ended)
        }
    }

    /// Makes the surface and the visual showing it, and places the visual over the
    /// `Grid`.
    fn attach_surface(
        &self,
        renderer: &mut render::Renderer,
        size: SizeInt32,
        pixels: (u32, u32),
    ) -> windows_core::Result<Surface> {
        let element: UIElement = self.native.cast()?;
        let compositor = ElementCompositionPreview::GetElementVisual(&element)?.Compositor()?;
        let device = renderer.graphics_device(&compositor)?;
        let surface = device.CreateDrawingSurface2(
            size,
            DirectXPixelFormat::B8G8R8A8UIntNormalized,
            DirectXAlphaMode::Premultiplied,
        )?;
        let brush = compositor.CreateSurfaceBrushWithSurface(&surface)?;
        brush.SetStretch(CompositionStretch::Fill)?;
        let visual = compositor.CreateSpriteVisual()?;
        visual.SetBrush(&brush)?;
        ElementCompositionPreview::SetElementChildVisual(&element, &visual)?;
        Ok(Surface {
            visual,
            surface,
            pixels,
        })
    }

    /// Watches `XamlRoot.Changed` of the window the surface is in, to draw again when
    /// only the display scale changes. Moves to the new `XamlRoot` when the surface
    /// moves to another window.
    fn watch_root(self: &Rc<Self>) {
        let Ok(root) = self.native.XamlRoot() else {
            return;
        };
        let mut watched = self.root.borrow_mut();
        if watched
            .as_ref()
            .is_some_and(|(current, _)| *current == root)
        {
            return;
        }
        if let Some((previous, token)) = watched.take() {
            unwatch_root(&previous, token);
        }
        let target = UiThreadCell::new(Rc::downgrade(self));
        let changed = TypedEventHandler::<XamlRoot, XamlRootChangedEventArgs>::new(move |_, _| {
            let _ = target.try_with_mut(|weak| {
                if let Some(inner) = weak.upgrade() {
                    if inner.rasterization_scale() != inner.scale.get() {
                        schedule_paint(&inner);
                    }
                }
            });
            Ok(())
        });
        if let Ok(token) = root.Changed(&changed) {
            *watched = Some((root, token));
            let _ = ROOT_WATCHES.try_with(|count| count.set(count.get() + 1));
        }
    }
}

impl Drop for CanvasInner {
    /// Unsubscribes from `XamlRoot.Changed`. The `XamlRoot` outlives the surface (it
    /// is the window's), so without this every surface made and dropped in a window
    /// would leave a delegate behind, run on every resize and scale change.
    fn drop(&mut self) {
        if let Some((root, token)) = self.root.get_mut().take() {
            unwatch_root(&root, token);
        }
    }
}

fn unwatch_root(root: &XamlRoot, token: i64) {
    let _ = root.RemoveChanged(token);
    let _ = ROOT_WATCHES.try_with(|count| count.set(count.get().saturating_sub(1)));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wheel_amount_reads_down_and_right_as_positive() {
        // 奥へ 1 刻み (+120) は上へ読み戻す向きなので負。
        assert_eq!(wheel_amount(120.0, false), (0.0, -WHEEL_STEP));
        assert_eq!(wheel_amount(-240.0, false), (0.0, 2.0 * WHEEL_STEP));
        // 横は右が正のまま。
        assert_eq!(wheel_amount(120.0, true), (WHEEL_STEP, 0.0));
    }

    #[test]
    fn button_held_from_outside_is_not_a_press() {
        let left = button_bit(PointerButton::Primary);
        let right = button_bit(PointerButton::Secondary);
        // 面の外で左を押したまま入ってきた。押下ではなく移動のまま届く。
        let before = previous_buttons(PointerPhase::Move, None, left);
        assert_eq!(
            button_changes(PointerPhase::Move, before, left),
            [(PointerPhase::Move, None)]
        );
        // その後に面の上で右を押した分は押下として届く。
        let before = previous_buttons(PointerPhase::Move, Some(left), left | right);
        assert_eq!(
            button_changes(PointerPhase::Move, before, left | right),
            [(PointerPhase::Down, Some(PointerButton::Secondary))]
        );
        // 覚えていた押下を全部離したら、解放として届く。
        let before = previous_buttons(PointerPhase::Move, Some(left), 0);
        assert_eq!(
            button_changes(PointerPhase::Move, before, 0),
            [(PointerPhase::Up, Some(PointerButton::Primary))]
        );
        // `PointerPressed` は覚えていた残りがあっても最初の押下として扱う。
        let before = previous_buttons(PointerPhase::Down, Some(right), left);
        assert_eq!(
            button_changes(PointerPhase::Down, before, left),
            [(PointerPhase::Down, Some(PointerButton::Primary))]
        );
    }
}
