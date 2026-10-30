//! The drawing surface (WinUI 3's `Canvas` holding `Path`s and `TextBlock`s).
//!
//! WinUI 3 has no "surface the app paints pixels on". Win2D is a separate package
//! rather than part of the Windows App SDK, and `SwapChainPanel` is a door for
//! driving DirectX yourself, so it is built by placing XAML's **shape elements**
//! (`Microsoft.UI.Xaml.Shapes.Path` and `TextBlock`) into a `Canvas`. Fills,
//! strokes and text are all drawn by the XAML renderer (Composition), so
//! antialiasing, display scaling and dark-theme text get the same quality as other
//! controls.
//!
//! On every redraw, the commands `on_draw` recorded in the [`Painter`] become one
//! XAML string (a `<Canvas>` and its children) passed to `XamlReader`, and the
//! resulting elements replace the previous surface wholesale. `Path` shapes are
//! given in XAML's path syntax (`M` / `L` / `C` / `Z`), so the `Path` and
//! `PathGeometry` types need not be added to the projection.
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

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::fmt::Write as _;
use std::rc::{Rc, Weak};
use std::sync::Arc;
use std::time::{Duration, Instant};

use naui_core::{
    Align, Color, DrawCommand, EventResponse, Painter, Path, PathSegment, Point, PointerButton,
    PointerEvent, PointerPhase, Result, WheelHandler, WHEEL_STEP,
};
use naui_winui3::Microsoft::UI::Dispatching::{DispatcherQueue, DispatcherQueueHandler};
use naui_winui3::Microsoft::UI::Input::{PointerDeviceType, PointerPointProperties};
use naui_winui3::Microsoft::UI::Xaml::Controls::{Canvas as XamlCanvas, Grid as XamlGrid};
use naui_winui3::Microsoft::UI::Xaml::Input::{PointerEventHandler, PointerRoutedEventArgs};
use naui_winui3::Microsoft::UI::Xaml::Markup::XamlReader;
use naui_winui3::Microsoft::UI::Xaml::{FrameworkElement, SizeChangedEventHandler, UIElement};
use windows::Foundation::Size;
use windows_core::{Interface, HSTRING};

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
}

thread_local! {
    /// The surfaces with `on_wheel` set. The destination of wheels received by the hook
    /// is looked up here.
    static WHEEL_CANVASES: RefCell<Vec<Weak<CanvasInner>>> = const { RefCell::new(Vec::new()) };
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

/// A surface the app draws on itself (`Grid` + `Canvas` + `Path` / `TextBlock`).
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
        }));
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
        if self.0.scheduled.replace(true) {
            return;
        }
        let Ok(queue) = DispatcherQueue::GetForCurrentThread() else {
            self.0.scheduled.set(false);
            self.0.paint();
            return;
        };
        let target = UiThreadCell::new(Rc::downgrade(&self.0));
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
            self.0.scheduled.set(false);
            self.0.paint();
        }
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

    /// Returns the `on_draw` commands as XAML for a surface of the given size. **For
    /// automated tests only.**
    ///
    /// Lets you check how commands map to XAML shapes without showing anything on
    /// screen.
    #[doc(hidden)]
    pub fn xaml_for_test(&self, width: f64, height: f64) -> String {
        let mut painter = Painter::new(width, height);
        self.0.draw.emit(&mut painter);
        scene_xaml(width, height, painter.commands())
    }

    /// The base `Grid`. Public as a backend-specific escape hatch.
    ///
    /// What is drawn is placed as children of the single `Canvas` inside it.
    pub fn native_grid(&self) -> XamlGrid {
        self.0.native.clone()
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

    /// Calls `on_draw` and replaces the surface with the resulting XAML.
    fn paint(&self) {
        let (width, height) = self.actual_size();
        if width <= 0.0 || height <= 0.0 {
            return;
        }
        let mut painter = Painter::new(width, height);
        if !self.draw.emit(&mut painter) {
            return;
        }
        let xaml = scene_xaml(width, height, painter.commands());
        let Ok(scene) = XamlReader::Load(&HSTRING::from(xaml)).and_then(|e| e.cast::<UIElement>())
        else {
            return;
        };
        align_text(&scene, painter.commands());
        if let Ok(children) = self.native.Children() {
            let _ = children.Clear();
            let _ = children.Append(&scene);
        }
    }
}

/// The XAML for the whole surface. `Canvas` only places its children and has no size
/// of its own, so it is given a width and height, and overflow is cut off with
/// `Clip`.
fn scene_xaml(width: f64, height: f64, commands: &[DrawCommand]) -> String {
    let mut xaml = String::new();
    let _ = write!(
        xaml,
        r##"<Canvas xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation" xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml" Width="{w}" Height="{h}" IsHitTestVisible="False"><Canvas.Clip><RectangleGeometry Rect="0,0,{w},{h}"/></Canvas.Clip>"##,
        w = number(width),
        h = number(height),
    );
    for (index, command) in commands.iter().enumerate() {
        match command {
            DrawCommand::Fill {
                path,
                color,
                opacity,
            } => {
                let _ = write!(
                    xaml,
                    r##"<Path Fill="{}" Data="{}"/>"##,
                    argb(*color, *opacity),
                    path_data(path),
                );
            }
            DrawCommand::Stroke {
                path,
                color,
                width,
                dash,
                opacity,
            } => {
                let _ = write!(
                    xaml,
                    r##"<Path Stroke="{}" StrokeThickness="{}" StrokeLineJoin="Miter" StrokeStartLineCap="Flat" StrokeEndLineCap="Flat""##,
                    argb(*color, *opacity),
                    number(*width),
                );
                if !dash.is_empty() {
                    // XAML の破線は**線の太さを 1 とする単位**で指定する。
                    let pattern: Vec<String> = dash.iter().map(|v| number(v / width)).collect();
                    let _ = write!(xaml, r##" StrokeDashArray="{}""##, pattern.join(" "));
                }
                let _ = write!(xaml, r##" Data="{}"/>"##, path_data(path));
            }
            DrawCommand::Text {
                text,
                at,
                size,
                color,
                opacity,
                ..
            } => {
                let _ = write!(
                    xaml,
                    r##"<TextBlock x:Name="{}" Canvas.Left="{}" Canvas.Top="{}" FontSize="{}" Foreground="{}" TextWrapping="NoWrap" Text="{}"/>"##,
                    text_name(index),
                    number(at.x),
                    number(at.y),
                    number(*size),
                    argb(*color, *opacity),
                    escape(text),
                );
            }
        }
    }
    xaml.push_str("</Canvas>");
    xaml
}

/// Shifts centered and right-aligned text left by its measured width.
///
/// XAML's `Canvas` can only place things by their top left, so the loaded
/// `TextBlock`s are measured and `Canvas.Left` is rewritten.
fn align_text(scene: &UIElement, commands: &[DrawCommand]) {
    let Ok(root) = scene.cast::<FrameworkElement>() else {
        return;
    };
    for (index, command) in commands.iter().enumerate() {
        let DrawCommand::Text { at, align, .. } = command else {
            continue;
        };
        if matches!(align, Align::Start | Align::Fill) {
            continue;
        }
        let Ok(block) = root
            .FindName(&HSTRING::from(text_name(index)))
            .and_then(|value| value.cast::<UIElement>())
        else {
            continue;
        };
        let _ = block.Measure(Size {
            Width: f32::INFINITY,
            Height: f32::INFINITY,
        });
        let Ok(desired) = block.DesiredSize() else {
            continue;
        };
        let width = f64::from(desired.Width);
        let left = if matches!(align, Align::Center) {
            at.x - width / 2.0
        } else {
            at.x - width
        };
        let _ = XamlCanvas::SetLeft(&block, left);
    }
}

fn text_name(index: usize) -> String {
    format!("t{index}")
}

/// XAML's path syntax. `F1` is nonzero (the same as the other three platforms'
/// default).
fn path_data(path: &Path) -> String {
    let mut data = String::from("F1");
    for segment in path.segments() {
        match *segment {
            PathSegment::MoveTo(p) => {
                let _ = write!(data, " M{},{}", number(p.x), number(p.y));
            }
            PathSegment::LineTo(p) => {
                let _ = write!(data, " L{},{}", number(p.x), number(p.y));
            }
            PathSegment::CubicTo {
                control1,
                control2,
                to,
            } => {
                let _ = write!(
                    data,
                    " C{},{} {},{} {},{}",
                    number(control1.x),
                    number(control1.y),
                    number(control2.x),
                    number(control2.y),
                    number(to.x),
                    number(to.y),
                );
            }
            PathSegment::Close => data.push_str(" Z"),
        }
    }
    data
}

/// A number that can be written into XAML. Infinities and NaN cannot be read by the
/// parser, so they become 0.
fn number(value: f64) -> String {
    if value.is_finite() {
        format!("{value}")
    } else {
        "0".to_string()
    }
}

/// `#AARRGGBB`. The opacity maps to alpha.
fn argb(color: Color, opacity: f64) -> String {
    let alpha = (opacity.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!(
        "#{:02X}{:02X}{:02X}{:02X}",
        alpha, color.r, color.g, color.b
    )
}

/// Converts XML's reserved characters into entity references so it can be written as
/// an attribute value.
///
/// XAML tries to read attribute values starting with `{` as markup extensions
/// (`{Binding}` and so on) and fails, so `{}` is put at the start to make it read
/// as a string (by XAML's rules, `{}` marks "what follows is text").
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    if text.starts_with('{') {
        out.push_str("{}");
    }
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            '\n' | '\r' | '\t' => out.push(' '),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use naui_core::Rect;

    #[test]
    fn path_data_uses_the_xaml_mini_language() {
        let path = Path::new()
            .move_to(Point::new(1.0, 2.0))
            .line_to(Point::new(3.5, 4.0))
            .cubic_to(
                Point::new(0.0, 0.0),
                Point::new(1.0, 1.0),
                Point::new(2.0, 2.0),
            )
            .close();
        assert_eq!(path_data(&path), "F1 M1,2 L3.5,4 C0,0 1,1 2,2 Z");
    }

    #[test]
    fn scene_carries_fill_stroke_and_text() {
        let mut painter = Painter::new(100.0, 50.0);
        painter.set_opacity(0.5);
        painter.fill_rect(
            Rect::new(0.0, 0.0, 10.0, 10.0),
            Color::rgb(0xff, 0x88, 0x00),
        );
        painter.set_opacity(1.0);
        painter.set_dash(&[4.0, 2.0]);
        painter.line(
            Point::new(0.0, 0.0),
            Point::new(9.0, 9.0),
            Color::BLACK,
            2.0,
        );
        painter.text("a<b>&\"c\"", Point::new(5.0, 6.0), 12.0, Color::WHITE);
        let xaml = scene_xaml(100.0, 50.0, painter.commands());
        assert!(xaml.starts_with("<Canvas "), "{xaml}");
        assert!(xaml.contains(r#"Width="100" Height="50""#));
        assert!(xaml.contains(r#"<RectangleGeometry Rect="0,0,100,50"/>"#));
        assert!(xaml.contains(r##"<Path Fill="#80FF8800" Data="F1 M0,0 L10,0 L10,10 L0,10 Z"/>"##));
        assert!(
            xaml.contains(r##"Stroke="#FF000000" StrokeThickness="2""##),
            "{xaml}"
        );
        assert!(
            xaml.contains(r#"StrokeDashArray="2 1""#),
            "破線は太さを 1 とする単位に直す: {xaml}"
        );
        assert!(
            xaml.contains(r#"Text="a&lt;b&gt;&amp;&quot;c&quot;""#),
            "{xaml}"
        );
        assert!(xaml.contains(r#"x:Name="t2""#));
        assert!(xaml.ends_with("</Canvas>"));
    }

    #[test]
    fn leading_brace_is_not_a_markup_extension() {
        assert_eq!(escape("{Binding}"), "{}{Binding}");
        assert_eq!(escape("{"), "{}{");
        assert_eq!(escape("a{b}"), "a{b}", "先頭でなければそのまま");
        assert_eq!(escape(""), "");
    }

    #[test]
    fn numbers_stay_parsable() {
        assert_eq!(number(1.5), "1.5");
        assert_eq!(number(-0.25), "-0.25");
        assert_eq!(number(f64::NAN), "0");
        assert_eq!(number(f64::INFINITY), "0");
        assert_eq!(argb(Color::rgb(1, 2, 3), 2.0), "#FF010203");
        assert_eq!(argb(Color::rgb(1, 2, 3), 0.0), "#00010203");
    }

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
