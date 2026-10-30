//! A split view (WinUI 3's `Grid` + a grabbable divider).
//!
//! **WinUI 3 has no control that "divides panes with a movable divider".** The
//! similarly named `SplitView` is a navigation pane that opens and closes, not
//! something whose width changes by dragging a divider, and `GridSplitter` is in the
//! Community Toolkit, not the Windows App SDK. So it is built in the same form
//! Fluent apps use for the same thing: a divider in the middle of a `Grid` with
//! three columns (rows).
//!
//! What is built is **only the position and the hit area**; the divider's color is
//! taken from a theme resource (`ControlStrokeColorDefaultBrush`), so it follows
//! light / dark switching as is.
//!
//! **Only a 1 px line is visible**; the rest is a transparent grab area
//! (`Background="Transparent"` + a `BorderThickness` on one side only). A filled
//! band would look less like a separator and more like a component of its own,
//! standing out from its surroundings. Unlike `null`, `Transparent` keeps
//! hit-testing, so it still works as a grab area.
//!
//! Column (row) widths are decided with `Pixel` / `Star`. The start side is a
//! `Pixel` of the specified size and the end side a `Star` taking the rest, so when
//! the window grows, the end side takes the extra.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use naui_core::{clamp_split_position, Orientation, Result, DEFAULT_SPLIT_POSITION};
use naui_winui3::Microsoft::UI::Xaml::Controls::{
    ColumnDefinition, Grid as XamlGrid, RowDefinition,
};
use naui_winui3::Microsoft::UI::Xaml::Input::PointerEventHandler;
use naui_winui3::Microsoft::UI::Xaml::Markup::XamlReader;
use naui_winui3::Microsoft::UI::Xaml::{
    FrameworkElement, GridLength, GridUnitType, HorizontalAlignment, UIElement, VerticalAlignment,
};
use windows::Foundation::EventHandler;
use windows_core::{IInspectable, Interface, HSTRING};

use crate::to_error;
use crate::ui_thread::UiThreadCell;
use crate::widgets::{impl_widget, Widget};

/// The thickness the divider occupies (logical pixels). Wide enough to grab with a
/// mouse.
///
/// Of this, only the 1 px line drawn at the boundary is visible; the rest is
/// transparent.
const DIVIDER_THICKNESS: f64 = 6.0;

/// The divider when the panes are side by side. Draws a line only at the boundary
/// (left edge).
const DIVIDER_XAML_HORIZONTAL: &str = r##"<Grid
    xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
    Background="Transparent"
    BorderThickness="1,0,0,0"
    BorderBrush="{ThemeResource ControlStrokeColorDefaultBrush}"/>"##;

/// The divider when the panes are stacked vertically. Draws a line only at the
/// boundary (top edge).
const DIVIDER_XAML_VERTICAL: &str = r##"<Grid
    xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
    Background="Transparent"
    BorderThickness="0,1,0,0"
    BorderBrush="{ThemeResource ControlStrokeColorDefaultBrush}"/>"##;

/// The handler notified when the divider moves.
///
/// It is taken out of the cell while it is called (the same as
/// [`SelectHandler`](crate::widgets)). Operating on the same split view from inside
/// the notification does not cause borrow conflicts.
type ResizeCallback = Box<dyn FnMut(f64)>;

#[derive(Clone)]
struct ResizeHandler(Arc<UiThreadCell<Option<ResizeCallback>>>);

impl ResizeHandler {
    fn new() -> Self {
        Self(Arc::new(UiThreadCell::new(None)))
    }

    fn set(&self, f: impl FnMut(f64) + 'static) {
        self.0.with_mut(|slot| *slot = Some(Box::new(f)));
    }

    fn emit(&self, position: f64) {
        let Some(Some(mut f)) = self.0.try_with_mut(|slot| slot.take()) else {
            return;
        };
        f(position);
        let _ = self.0.try_with_mut(|slot| {
            if slot.is_none() {
                *slot = Some(f);
            }
        });
    }
}

/// Collects only the state touched from events, needed to drag the divider.
struct Geometry {
    native: XamlGrid,
    divider: XamlGrid,
    orientation: Orientation,
    /// The divider position the app specified (the size of the start side).
    ///
    /// When it is too narrow to fit, it is **shown** clamped to the end, but this value
    /// is kept as is, so that it returns to the original position when it widens.
    position: f64,
    min_start: f64,
    min_end: f64,
    /// The position on screen now. If it is the same, the column widths are not
    /// rewritten (writing from inside `LayoutUpdated` would keep layout running
    /// forever).
    shown: f64,
    dragging: bool,
    /// The offset between where it was grabbed and the line's position. Remembered so
    /// the line does not jump under the pointer.
    grab: f64,
}

impl Geometry {
    /// The size the two panes can share (excluding the divider). 0 if still 0.
    fn total(&self) -> f64 {
        let Ok(element) = self.native.cast::<FrameworkElement>() else {
            return 0.0;
        };
        let length = if self.orientation.is_vertical() {
            element.ActualHeight()
        } else {
            element.ActualWidth()
        };
        let length = length.unwrap_or(0.0);
        if length <= 0.0 {
            return 0.0;
        }
        (length - DIVIDER_THICKNESS).max(0.0)
    }

    fn clamp(&self, position: f64) -> f64 {
        clamp_split_position(position, self.total(), self.min_start, self.min_end)
    }

    /// Clamps the specified position to the current size and writes it into the column
    /// (row) widths.
    fn apply(&mut self) {
        let shown = self.clamp(self.position);
        if (shown - self.shown).abs() < 0.5 {
            return;
        }
        self.shown = shown;
        let length = GridLength {
            Value: shown,
            GridUnitType: GridUnitType::Pixel,
        };
        if self.orientation.is_vertical() {
            if let Ok(rows) = self.native.RowDefinitions() {
                if let Ok(row) = rows.GetAt(0) {
                    let _ = row.SetHeight(length);
                }
            }
        } else if let Ok(columns) = self.native.ColumnDefinitions() {
            if let Ok(column) = columns.GetAt(0) {
                let _ = column.SetWidth(length);
            }
        }
    }

    /// Reinterprets the pointer's position as the distance from the split view's start
    /// edge.
    fn pointer_offset(&self, x: f64, y: f64) -> f64 {
        if self.orientation.is_vertical() {
            y
        } else {
            x
        }
    }
}

struct SplitViewInner {
    native: XamlGrid,
    start_pane: XamlGrid,
    end_pane: XamlGrid,
    orientation: Orientation,
    /// Holds the pane's handle, keeping it alive along with its callbacks.
    start: RefCell<Option<Box<dyn Widget>>>,
    end: RefCell<Option<Box<dyn Widget>>>,
    /// The position naui remembers, separately from the displayed position.
    position: Cell<f64>,
    geometry: Arc<UiThreadCell<Geometry>>,
    handler: ResizeHandler,
}

/// A container that divides two panes with a movable divider.
#[derive(Clone)]
pub struct SplitView(Rc<SplitViewInner>);
impl_widget!(SplitView, native);

impl SplitView {
    pub(crate) fn new(orientation: Orientation) -> Result<Self> {
        let native = XamlGrid::new().map_err(|e| to_error("create the SplitView Grid", e))?;
        let start_pane = pane()?;
        let end_pane = pane()?;
        let divider = divider(orientation)?;

        define_tracks(&native, orientation)?;
        for (index, element) in [
            start_pane.cast::<UIElement>(),
            divider.cast::<UIElement>(),
            end_pane.cast::<UIElement>(),
        ]
        .into_iter()
        .enumerate()
        {
            let element =
                element.map_err(|e| to_error("convert the SplitView pane to UIElement", e))?;
            place(&element, index as i32, orientation)?;
            native
                .Children()
                .and_then(|children| children.Append(&element))
                .map_err(|e| to_error("add to the SplitView", e))?;
        }

        let geometry = Arc::new(UiThreadCell::new(Geometry {
            native: native.clone(),
            divider: divider.clone(),
            orientation,
            position: DEFAULT_SPLIT_POSITION,
            min_start: 0.0,
            min_end: 0.0,
            // 列の初期値は 0 なので、最初の apply で必ず書き込まれる。
            shown: -1.0,
            dragging: false,
            grab: 0.0,
        }));
        let handler = ResizeHandler::new();

        let this = Self(Rc::new(SplitViewInner {
            native,
            start_pane,
            end_pane,
            orientation,
            start: RefCell::new(None),
            end: RefCell::new(None),
            position: Cell::new(DEFAULT_SPLIT_POSITION),
            geometry,
            handler,
        }));
        this.track_pointer(&divider)?;
        this.track_layout()?;
        Ok(this)
    }

    /// Connects how the divider is grabbed (pointer).
    fn track_pointer(&self, divider: &XamlGrid) -> Result<()> {
        let pressed = {
            let geometry = self.0.geometry.clone();
            PointerEventHandler::new(move |_, args| {
                let Some(args) = args.as_ref() else {
                    return Ok(());
                };
                geometry.try_with_mut(|geometry| {
                    if let Ok(pointer) = args.Pointer() {
                        let _ = geometry.divider.CapturePointer(&pointer);
                    }
                    // つかんだ場所と線とのずれを保つ (どこをつかんでも線が
                    // ポインターの下へ飛ばないようにする)。
                    if let Some(offset) = geometry
                        .native
                        .cast::<UIElement>()
                        .ok()
                        .and_then(|root| args.GetCurrentPoint(&root).ok())
                        .and_then(|point| point.Position().ok())
                        .map(|point| {
                            geometry.pointer_offset(f64::from(point.X), f64::from(point.Y))
                        })
                    {
                        geometry.grab = offset - geometry.position;
                    }
                    geometry.dragging = true;
                });
                Ok(())
            })
        };
        divider
            .PointerPressed(&pressed)
            .map_err(|e| to_error("subscribe to SplitView pointer events", e))?;

        let moved = {
            let geometry = self.0.geometry.clone();
            let handler = self.0.handler.clone();
            PointerEventHandler::new(move |_, args| {
                let Some(args) = args.as_ref() else {
                    return Ok(());
                };
                let moved = geometry.try_with_mut(|geometry| {
                    if !geometry.dragging {
                        return None;
                    }
                    let root = geometry.native.cast::<UIElement>().ok()?;
                    let point = args.GetCurrentPoint(&root).ok()?.Position().ok()?;
                    let position = geometry.pointer_offset(f64::from(point.X), f64::from(point.Y))
                        - geometry.grab;
                    let clamped = geometry.clamp(position);
                    if (clamped - geometry.position).abs() < 0.5 {
                        return None;
                    }
                    geometry.position = clamped;
                    geometry.apply();
                    Some(clamped)
                });
                if let Some(Some(position)) = moved {
                    handler.emit(position);
                }
                Ok(())
            })
        };
        divider
            .PointerMoved(&moved)
            .map_err(|e| to_error("subscribe to SplitView pointer events", e))?;

        let released = {
            let geometry = self.0.geometry.clone();
            PointerEventHandler::new(move |_, args| {
                geometry.try_with_mut(|geometry| {
                    if let Some(args) = args.as_ref() {
                        if let Ok(pointer) = args.Pointer() {
                            let _ = geometry.divider.ReleasePointerCapture(&pointer);
                        }
                    }
                    geometry.dragging = false;
                });
                Ok(())
            })
        };
        divider
            .PointerReleased(&released)
            .map_err(|e| to_error("subscribe to SplitView pointer events", e))?;

        let lost = {
            let geometry = self.0.geometry.clone();
            PointerEventHandler::new(move |_, _| {
                geometry.try_with_mut(|geometry| geometry.dragging = false);
                Ok(())
            })
        };
        divider
            .PointerCaptureLost(&lost)
            .map_err(|e| to_error("subscribe to SplitView pointer events", e))?;
        Ok(())
    }

    /// Re-clamps the position when the size is decided (changes).
    ///
    /// Size changes are caught with `LayoutUpdated` (`SizeChanged` would also catch them,
    /// but this one is used to catch the update right after moving the divider too).
    /// Nothing is written when the displayed position does not change, so layout never
    /// loops forever.
    fn track_layout(&self) -> Result<()> {
        let geometry = self.0.geometry.clone();
        let updated = EventHandler::<IInspectable>::new(move |_, _| {
            geometry.try_with_mut(|geometry| geometry.apply());
            Ok(())
        });
        self.0
            .native
            .cast::<FrameworkElement>()
            .and_then(|element| element.LayoutUpdated(&updated))
            .map_err(|e| to_error("subscribe to SplitView layout updates", e))?;
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
        let clamped = self.0.geometry.with_mut(|geometry| {
            let clamped = geometry.clamp(position);
            geometry.position = clamped;
            geometry.apply();
            clamped
        });
        self.0.position.set(clamped);
    }

    /// Moves the divider as if the user had dragged it. Calls `on_resize`.
    pub fn drag_to(&self, position: f64) {
        self.set_position(position);
        self.0.handler.emit(self.0.position.get());
    }

    /// The minimum sizes of the two panes. Both default to 0.
    pub fn set_min_sizes(&self, start: f64, end: f64) {
        let clamped = self.0.geometry.with_mut(|geometry| {
            geometry.min_start = start.max(0.0);
            geometry.min_end = end.max(0.0);
            // いまの位置が範囲の外なら押し戻す (通知はしない)。
            geometry.position = geometry.clamp(geometry.position);
            geometry.apply();
            geometry.position
        });
        self.0.position.set(clamped);
    }

    /// Called with the new position every time the user moves the divider.
    pub fn on_resize(&self, f: impl FnMut(f64) + 'static) {
        self.0.handler.set(f);
    }
}

impl SplitViewInner {
    fn set_pane(&self, is_start: bool, child: &dyn Widget) {
        let pane = if is_start {
            &self.start_pane
        } else {
            &self.end_pane
        };
        if let Ok(children) = pane.Children() {
            let _ = children.Clear();
        }
        let element = child.native_element();
        let appended = pane
            .Children()
            .and_then(|children| children.Append(&element));
        if appended.is_ok() {
            let slot = if is_start { &self.start } else { &self.end };
            *slot.borrow_mut() = Some(child.boxed_clone());
        }
    }
}

/// The container for one pane. Its content is placed to fill the allotted space.
fn pane() -> Result<XamlGrid> {
    let pane = XamlGrid::new().map_err(|e| to_error("create the SplitView pane", e))?;
    let element = pane
        .cast::<FrameworkElement>()
        .map_err(|e| to_error("convert the SplitView pane to UIElement", e))?;
    let _ = element.SetHorizontalAlignment(HorizontalAlignment::Stretch);
    let _ = element.SetVerticalAlignment(VerticalAlignment::Stretch);
    Ok(pane)
}

/// The divider. Where theme resources cannot be obtained, it falls back to a
/// colorless `Grid`.
fn divider(orientation: Orientation) -> Result<XamlGrid> {
    let xaml = if orientation.is_vertical() {
        DIVIDER_XAML_VERTICAL
    } else {
        DIVIDER_XAML_HORIZONTAL
    };
    let divider = match XamlReader::Load(&HSTRING::from(xaml))
        .and_then(|element| element.cast::<XamlGrid>())
    {
        Ok(divider) => divider,
        Err(error) => {
            eprintln!("naui-windows: failed to create the split view divider: {error}");
            XamlGrid::new().map_err(|e| to_error("create the SplitView divider", e))?
        }
    };
    let element = divider
        .cast::<FrameworkElement>()
        .map_err(|e| to_error("convert the SplitView divider to UIElement", e))?;
    let _ = element.SetHorizontalAlignment(HorizontalAlignment::Stretch);
    let _ = element.SetVerticalAlignment(VerticalAlignment::Stretch);
    Ok(divider)
}

/// Creates the three columns (rows): start / divider / end.
fn define_tracks(native: &XamlGrid, orientation: Orientation) -> Result<()> {
    let lengths = [
        GridLength {
            Value: DEFAULT_SPLIT_POSITION,
            GridUnitType: GridUnitType::Pixel,
        },
        GridLength {
            Value: DIVIDER_THICKNESS,
            GridUnitType: GridUnitType::Pixel,
        },
        GridLength {
            Value: 1.0,
            GridUnitType: GridUnitType::Star,
        },
    ];
    for length in lengths {
        if orientation.is_vertical() {
            let row = RowDefinition::new().map_err(|e| to_error("create the SplitView row", e))?;
            row.SetHeight(length)
                .map_err(|e| to_error("set the SplitView row", e))?;
            native
                .RowDefinitions()
                .and_then(|rows| rows.Append(&row))
                .map_err(|e| to_error("add the SplitView row", e))?;
        } else {
            let column =
                ColumnDefinition::new().map_err(|e| to_error("create the SplitView column", e))?;
            column
                .SetWidth(length)
                .map_err(|e| to_error("set the SplitView column", e))?;
            native
                .ColumnDefinitions()
                .and_then(|columns| columns.Append(&column))
                .map_err(|e| to_error("add the SplitView column", e))?;
        }
    }
    Ok(())
}

/// Places the element in column (row) `index`.
fn place(element: &UIElement, index: i32, orientation: Orientation) -> Result<()> {
    let framework = element
        .cast::<FrameworkElement>()
        .map_err(|e| to_error("place the SplitView", e))?;
    if orientation.is_vertical() {
        XamlGrid::SetRow(&framework, index).map_err(|e| to_error("place the SplitView", e))?;
    } else {
        XamlGrid::SetColumn(&framework, index).map_err(|e| to_error("place the SplitView", e))?;
    }
    Ok(())
}
