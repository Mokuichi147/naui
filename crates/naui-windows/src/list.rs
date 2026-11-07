//! Lists (WinUI 3).
//!
//! WinUI's standard `ListView` is used as is. Rows are `ListViewItem`s, holding
//! either a `TextBlock` placed in a transparent `Grid` or any prebuilt widget. Rows
//! that cannot be used get `IsEnabled = false`.
//!
//! The row look (rounded corners, light fill, the accent-colored indicator on the
//! left) comes from `ListView`'s standard template. Only a frame is missing, so
//! `List`, `Table` and `Tree` are surrounded by a `Border` with the same color and
//! rounded corners. All colors are taken with `{ThemeResource ...}`, so they follow
//! light / dark switching (the window's `RequestedTheme`) as is. Where theme
//! resources cannot be obtained, it falls back to a plain `Border`, prioritizing
//! function.
//!
//! Scrolling is owned by an outer `ScrollViewer` holding the `ListView` and two spacers
//! (the same structure as [`crate::Table`]). **With many rows, only the rows on screen
//! are built**: `ListView` creates a `ListViewItem` for every row it is given, so the
//! rows outside the window are left out and the spacers carry their height. The
//! selection is held here, because `ListView` knows only the rows it has. That
//! `ScrollViewer` is registered with the window-wide wheel helper ([`crate::layout`]).
//!
//! For the same reason, in a list that scrolls itself the movement keys and anything with
//! Shift are handled here: `ListView` takes all its rows as the page, and it keeps its
//! keyboard position and the Shift anchor as places among the rows it has, which mean other
//! rows once the window moves. Clicks are left to it, as is everything in a list whose
//! height follows its rows.
//!
//! Row activation is received through the `ListViewItem`'s `DoubleTapped` and
//! Enter's `KeyDown`. Right-clicks are received through `ContextRequested`, the
//! context menu request WinUI fires not only for right-clicks but also for touch
//! long-press, Shift + F10 and the application key; for key operations there is no
//! position, so the row's left edge is passed.

use std::cell::{Cell, RefCell};
use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;

use naui_core::{
    keeps_row_window, list_row_window, merge_hidden_selection, Align, ListItem, Modifiers,
    Orientation, Point, Result, RowSource, RowWindow, SelectionGesture, SelectionMode, Sizing,
    TextColor, TextStyle, ROW_WINDOW_OVERSCAN,
};
use naui_winui3::Microsoft::UI::Xaml::Automation::AutomationProperties;
use naui_winui3::Microsoft::UI::Xaml::Controls::{
    Border, Control, Grid as XamlGrid, ListView, ListViewItem, ListViewSelectionMode,
    ScrollBarVisibility, ScrollMode, ScrollViewer, ScrollViewerViewChangedEventArgs,
    SelectionChangedEventHandler, StackPanel, TextBlock,
};
use naui_winui3::Microsoft::UI::Xaml::Input::{
    ContextRequestedEventArgs, DoubleTappedEventHandler, KeyEventHandler, PointerEventHandler,
    PointerRoutedEventArgs,
};
use naui_winui3::Microsoft::UI::Xaml::Markup::XamlReader;
use naui_winui3::Microsoft::UI::Xaml::Media::VisualTreeHelper;
use naui_winui3::Microsoft::UI::Xaml::{
    DependencyObject, FocusState, FrameworkElement, RoutedEventHandler, Thickness, UIElement,
};
use windows::Foundation::{Point as XamlPoint, PropertyValue, TypedEventHandler};
use windows::System::VirtualKey;
use windows_core::{IInspectable, Interface, HSTRING};

use crate::layout::ListScrollTarget;
use crate::table::selection_gesture;
use crate::to_error;
use crate::ui_thread::{HandlerCell, UiThreadCell};
use crate::widgets::{impl_widget, label_style, Label, SelectHandler, Stack, Widget};

/// The handler notified when a row is clicked.
///
/// WinRT delegates require `Send + Sync`, so it is placed in a `UiThreadCell`. The
/// closure is taken out only for the duration of the call, so operating on the same
/// row from inside the callback does not double-borrow.
#[derive(Clone)]
pub(crate) struct ActivationHandler(HandlerCell<dyn FnMut()>);

impl ActivationHandler {
    pub(crate) fn new() -> Self {
        Self(Arc::new(UiThreadCell::new(None)))
    }

    pub(crate) fn set(&self, f: impl FnMut() + 'static) {
        self.0.with_mut(|slot| *slot = Some(Box::new(f)));
    }

    pub(crate) fn emit(&self) {
        // WinRT のデリゲートから panic を出さないよう try_ 系で触る。
        let Some(Some(mut f)) = self.0.try_with_mut(|slot| slot.take()) else {
            return;
        };
        f();
        self.0.try_with_mut(|slot| {
            if slot.is_none() {
                *slot = Some(f);
            }
        });
    }
}

/// The content of one row. Plain text rows and custom widget rows are normalized
/// into the same type.
enum ListRowContent {
    Item(ListItem),
    Custom(Box<dyn Widget>),
}

impl Clone for ListRowContent {
    fn clone(&self) -> Self {
        match self {
            Self::Item(item) => Self::Item(item.clone()),
            Self::Custom(content) => Self::Custom(content.boxed_clone()),
        }
    }
}

/// One row to put in a list.
///
/// [`ListItem`] is a convenience API for lists that need only strings. Rows that
/// combine icons, several labels, check boxes, trailing buttons and so on, as in a
/// settings screen, are built with `Grid` / `Stack` and wrapped in a `ListRow`.
///
/// ```no_run
/// # use naui_windows::{ListRow, Widget};
/// # fn row(content: &dyn Widget) {
/// let row = ListRow::new(content).selectable(false);
/// row.on_activate(|| println!("row clicked"));
/// # let _ = row;
/// # }
/// ```
pub struct ListRow {
    content: ListRowContent,
    selectable: bool,
    activation: ActivationHandler,
}

impl Clone for ListRow {
    fn clone(&self) -> Self {
        Self {
            content: self.content.clone(),
            selectable: self.selectable,
            activation: self.activation.clone(),
        }
    }
}

impl ListRow {
    /// Uses a widget as the content of one row. By default the whole row can be
    /// selected too.
    pub fn new(content: &dyn Widget) -> Self {
        Self {
            content: ListRowContent::Custom(content.boxed_clone()),
            selectable: true,
            activation: ActivationHandler::new(),
        }
    }

    /// Converts a `ListItem` into a plain text row. The normalization path for
    /// `List::set_items`.
    fn from_item(item: ListItem) -> Self {
        let selectable = item.enabled;
        Self {
            content: ListRowContent::Item(item),
            selectable,
            activation: ActivationHandler::new(),
        }
    }

    /// Set to `false` for rows where only the controls inside are operated.
    pub fn selectable(mut self, selectable: bool) -> Self {
        self.selectable = selectable;
        self
    }

    pub fn is_selectable(&self) -> bool {
        self.selectable
    }

    /// What to do when a label, icon or empty space inside the row is clicked.
    ///
    /// It is not called when a control such as a check box, button or text field is
    /// clicked directly, so the same action does not fire twice.
    pub fn on_activate(&self, f: impl FnMut() + 'static) {
        self.activation.set(f);
    }

    /// A row whose row and contents can be neither operated (`ListItem::enabled(false)`).
    fn is_disabled_item(&self) -> bool {
        matches!(&self.content, ListRowContent::Item(item) if !item.enabled)
    }
}

/// The row padding. `ListView` defaults to 0 vertically, which looks cramped with
/// the two lines of `label` and `detail`, so space is added above and below.
/// Horizontally it is 12, the same as the default. (`Table` must align headings
/// with columns, so it holds the same value itself.)
const ROW_PADDING: Thickness = Thickness {
    Left: 12.0,
    Top: 8.0,
    Right: 12.0,
    Bottom: 8.0,
};

/// The list's frame. `ListView` has no frame, so it is surrounded by a `Border` with
/// the same color and rounded corners as `Table` and `Tree`.
///
/// The rows are rebuilt as a whole on every `set_rows` and whenever the window moves, so
/// the default item transitions (which animate rows as they are added and removed) are
/// cleared. Otherwise the whole list animates each time it is rebuilt. `Table` does the
/// same.
const SURFACE_XAML: &str = r##"<Border
    xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
    Background="{ThemeResource ControlFillColorDefaultBrush}"
    BorderBrush="{ThemeResource ControlStrokeColorDefaultBrush}"
    BorderThickness="1"
    CornerRadius="{ThemeResource ControlCornerRadius}"
    Padding="4">
    <ListView Background="Transparent" BorderThickness="0" Padding="0"
        HorizontalContentAlignment="Stretch">
        <ListView.ItemContainerTransitions>
            <TransitionCollection/>
        </ListView.ItemContainerTransitions>
    </ListView>
</Border>"##;

/// One row (`ListViewItem`). Its content transitions are cleared as well: every row
/// is a new container on each rebuild, so the content's default entrance would play
/// for the whole list.
const ROW_ITEM_XAML: &str = r##"<ListViewItem
    xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation">
    <ListViewItem.ContentTransitions>
        <TransitionCollection/>
    </ListViewItem.ContentTransitions>
</ListViewItem>"##;

/// Tells assistive technology a row's place among all the rows and how many there are.
///
/// The `ListView` has only the rows built, so it would answer with those alone.
pub(crate) fn report_row_position(row: &ListViewItem, index: usize, count: usize) {
    let _ =
        AutomationProperties::SetPositionInSet(row, i32::try_from(index + 1).unwrap_or(i32::MAX));
    let _ = AutomationProperties::SetSizeOfSet(row, i32::try_from(count).unwrap_or(i32::MAX));
}

/// Creates one row container. Falls back to a plain `ListViewItem` if the XAML cannot
/// be read.
pub(crate) fn new_row_item() -> Result<ListViewItem> {
    let loaded = XamlReader::Load(&HSTRING::from(ROW_ITEM_XAML))
        .and_then(|element| element.cast::<ListViewItem>());
    match loaded {
        Ok(item) => Ok(item),
        Err(_) => ListViewItem::new().map_err(|e| to_error("create the ListViewItem", e)),
    }
}

/// The holder for a row's content. With `Background="Transparent"`, it receives the
/// pointer even in empty space without text (to pick up clicks on the row itself).
const ROW_HOST_XAML: &str = r##"<Grid
    xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
    Background="Transparent"/>"##;

/// A transparent `Grid` (see [`ROW_HOST_XAML`]). Falls back to a plain `Grid` if the
/// XAML cannot be read, which only loses clicks on empty space.
pub(crate) fn transparent_grid() -> Result<XamlGrid> {
    match XamlReader::Load(&HSTRING::from(ROW_HOST_XAML))
        .and_then(|element| element.cast::<XamlGrid>())
    {
        Ok(host) => Ok(host),
        Err(_) => XamlGrid::new().map_err(|e| to_error("create the row container", e)),
    }
}

/// The padding of a row's `ListViewItem`. A row that cannot be selected has none: the
/// padding is moved into its holder (see [`row_host`]), so that the whole row is the
/// holder and a press in its margins is held too.
pub(crate) fn item_padding(selectable: bool) -> Thickness {
    if selectable {
        ROW_PADDING
    } else {
        Thickness {
            Left: 0.0,
            Top: 0.0,
            Right: 0.0,
            Bottom: 0.0,
        }
    }
}

/// Holds a press that starts on a row which cannot be selected, so that `ListView`
/// never sees it and does not show the pressed or selected look for an instant.
///
/// Only the primary button is held, and the release that pairs with it, so a
/// right-click still reaches the context menu. The row's own content is already
/// handled the same way: buttons and fields stop the press, which is why clicking
/// them does not select the row.
///
/// The held press is remembered by its pointer id, so only the release of that same
/// pointer is taken. It is forgotten when that pointer is cancelled, loses capture or
/// leaves the row, so a press that never gets its release cannot swallow a later one.
pub(crate) fn hold_unselectable_press(holder: &XamlGrid) -> Result<()> {
    // 押下中のポインターの id。押していなければ `None`。
    let held: Arc<UiThreadCell<Option<u32>>> = Arc::new(UiThreadCell::new(None));

    let on_press = held.clone();
    let pressed = PointerEventHandler::new(move |_, args| {
        let Some(args) = args.as_ref() else {
            return Ok(());
        };
        if is_primary_press(Some(args)) {
            let id = pointer_id(args);
            on_press.with_mut(|held| *held = Some(id));
            let _ = args.SetHandled(true);
        }
        Ok(())
    });

    let on_release = held.clone();
    let released = PointerEventHandler::new(move |_, args| {
        let Some(args) = args.as_ref() else {
            return Ok(());
        };
        let id = pointer_id(args);
        let ours = on_release.with_mut(|held| {
            if *held == Some(id) {
                *held = None;
                true
            } else {
                false
            }
        });
        if ours {
            let _ = args.SetHandled(true);
        }
        Ok(())
    });

    // 押下が離されずに終わる経路 (キャンセル、捕捉の喪失、行からの離脱) では、
    // 押下を覚えたままにしない。
    let on_cancel = held.clone();
    let canceled = PointerEventHandler::new(move |_, args| {
        forget_press(&on_cancel, args.as_ref());
        Ok(())
    });
    let on_lost = held.clone();
    let capture_lost = PointerEventHandler::new(move |_, args| {
        forget_press(&on_lost, args.as_ref());
        Ok(())
    });
    let on_exit = held.clone();
    let exited = PointerEventHandler::new(move |_, args| {
        forget_press(&on_exit, args.as_ref());
        Ok(())
    });

    holder
        .PointerPressed(&pressed)
        .map_err(|e| to_error("subscribe to row presses", e))?;
    holder
        .PointerReleased(&released)
        .map_err(|e| to_error("subscribe to row releases", e))?;
    holder
        .PointerCanceled(&canceled)
        .map_err(|e| to_error("subscribe to row cancels", e))?;
    holder
        .PointerCaptureLost(&capture_lost)
        .map_err(|e| to_error("subscribe to row capture loss", e))?;
    holder
        .PointerExited(&exited)
        .map_err(|e| to_error("subscribe to row exits", e))?;
    Ok(())
}

/// The id of the pointer that raised the event (mouse, touch or pen each has its own).
fn pointer_id(args: &PointerRoutedEventArgs) -> u32 {
    args.Pointer()
        .and_then(|pointer| pointer.PointerId())
        .unwrap_or(0)
}

/// Forgets the press held for this pointer, if there is one.
fn forget_press(held: &UiThreadCell<Option<u32>>, args: Option<&PointerRoutedEventArgs>) {
    let Some(args) = args else {
        return;
    };
    let id = pointer_id(args);
    held.with_mut(|held| {
        if *held == Some(id) {
            *held = None;
        }
    });
}

/// Loads the themed frame. Falls back to a plain `Border` + `ListView` if it cannot
/// be read.
fn build_surface() -> Result<(Border, ListView)> {
    match load_surface() {
        Ok(surface) => Ok(surface),
        Err(error) => {
            eprintln!("naui-windows: failed to create the themed list frame: {error}");
            plain_surface()
        }
    }
}

fn load_surface() -> Result<(Border, ListView)> {
    let native = XamlReader::Load(&HSTRING::from(SURFACE_XAML))
        .and_then(|element| element.cast::<Border>())
        .map_err(|e| to_error("create the List frame", e))?;
    let list_view = native
        .Child()
        .and_then(|child| child.cast::<ListView>())
        .map_err(|e| to_error("get the List's ListView", e))?;
    Ok((native, list_view))
}

/// The `ListView` of the fallback frame. It is read from XAML, not built in code,
/// because the row transitions can only be cleared there (see [`SURFACE_XAML`]). It
/// has no theme resources, so it can be read even when the frame cannot. Falls back to
/// a plain `ListView` only if XAML cannot be read at all.
const PLAIN_LIST_VIEW_XAML: &str = r##"<ListView
    xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation">
    <ListView.ItemContainerTransitions>
        <TransitionCollection/>
    </ListView.ItemContainerTransitions>
</ListView>"##;

fn plain_surface() -> Result<(Border, ListView)> {
    let list_view = match XamlReader::Load(&HSTRING::from(PLAIN_LIST_VIEW_XAML))
        .and_then(|element| element.cast::<ListView>())
    {
        Ok(list_view) => list_view,
        Err(_) => ListView::new().map_err(|e| to_error("create the ListView", e))?,
    };
    let native = Border::new().map_err(|e| to_error("create the List Border", e))?;
    native
        .SetChild(&list_view)
        .map_err(|e| to_error("add to the List Border", e))?;
    Ok((native, list_view))
}

/// The notification receiving the right-clicked row and position.
type ContextMenuCallback = dyn FnMut(usize, Point);

/// The handler for right-clicks. The closure is taken out only for the duration of
/// the call.
#[derive(Clone)]
struct ContextMenuHandler(HandlerCell<ContextMenuCallback>);

impl ContextMenuHandler {
    fn new() -> Self {
        Self(Arc::new(UiThreadCell::new(None)))
    }

    fn set(&self, f: impl FnMut(usize, Point) + 'static) {
        self.0.with_mut(|slot| *slot = Some(Box::new(f)));
    }

    fn is_set(&self) -> bool {
        self.0.try_with_mut(|slot| slot.is_some()).unwrap_or(false)
    }

    fn emit(&self, index: usize, point: Point) {
        let Some(Some(mut f)) = self.0.try_with_mut(|slot| slot.take()) else {
            return;
        };
        f(index, point);
        self.0.try_with_mut(|slot| {
            if slot.is_none() {
                *slot = Some(f);
            }
        });
    }
}

/// The notification receiving the positions of the selected rows.
pub(crate) type SelectionCallback = dyn FnMut(&[usize]);

/// The handler notified when the selection changes.
///
/// WinRT delegates require `Send + Sync`, so it is placed in a `UiThreadCell`. The
/// closure is taken out only for the duration of the call, so operating on the same
/// list from inside the callback does not double-borrow.
#[derive(Clone)]
pub(crate) struct SelectionHandler(HandlerCell<SelectionCallback>);

impl SelectionHandler {
    pub(crate) fn new() -> Self {
        Self(Arc::new(UiThreadCell::new(None)))
    }

    pub(crate) fn set(&self, f: impl FnMut(&[usize]) + 'static) {
        self.0.with_mut(|slot| *slot = Some(Box::new(f)));
    }

    pub(crate) fn emit(&self, indices: &[usize]) {
        let Some(mut f) = self.0.with_mut(|slot| slot.take()) else {
            return;
        };
        f(indices);
        self.0.with_mut(|slot| {
            if slot.is_none() {
                *slot = Some(f);
            }
        });
    }
}

/// A movement of the keyboard cursor that [`List::move_cursor`] may do itself.
#[derive(Clone, Copy)]
enum RowMotion {
    /// Page Up and Page Down.
    Pages(i32),
    /// The up and down arrows, or Space to stay on the row.
    Lines(i32),
    /// Home.
    First,
    /// End.
    Last,
}

impl RowMotion {
    fn of_key(key: VirtualKey) -> Option<Self> {
        match key {
            VirtualKey::PageUp => Some(Self::Pages(-1)),
            VirtualKey::PageDown => Some(Self::Pages(1)),
            VirtualKey::Up => Some(Self::Lines(-1)),
            VirtualKey::Down => Some(Self::Lines(1)),
            VirtualKey::Space => Some(Self::Lines(0)),
            VirtualKey::Home => Some(Self::First),
            VirtualKey::End => Some(Self::Last),
            _ => None,
        }
    }
}

struct ListInner {
    native: Border,
    list_view: ListView,
    /// The scroll area bundling the row list and the spacers before and after it.
    scroll: ScrollViewer,
    /// What `scroll` scrolls. Row positions are measured from here.
    body: StackPanel,
    /// Frames filling in for the rows outside the window.
    top_spacer: XamlGrid,
    bottom_spacer: XamlGrid,
    /// The registration with the wheel helper, made after `Loaded`.
    wheel: RefCell<Option<Rc<ListScrollTarget>>>,
    /// The built rows themselves, in order from `window.start`.
    native_rows: RefCell<Vec<ListViewItem>>,
    /// The contents of the built rows, in the same order as `native_rows`. It also keeps
    /// the callbacks and the like contained in the rows alive.
    realized: RefCell<Vec<ListRow>>,
    /// Where the rows come from.
    rows: RowSource<ListRow>,
    /// The range of rows currently built. With many rows, only those around the screen.
    window: Cell<RowWindow>,
    /// The first row the `ListView` has now. `window` moves ahead of it just before the
    /// rows are rebuilt.
    built_start: Cell<usize>,
    /// The range the last build failed on, if it did.
    failed_window: Cell<Option<RowWindow>>,
    /// Whether rows are being built right now. Prevents nested rebuilds.
    rebuilding: Cell<bool>,
    /// The selected rows (ascending). Rows outside the window are included, so this is
    /// the source of truth, not the `ListView`.
    selected: RefCell<Vec<usize>>,
    /// The anchor of a Shift range selection, as a row number: `ListView` keeps it as a
    /// place among the rows it has, so it means another row once the window moves.
    anchor: Cell<Option<usize>>,
    /// The row the pointer last went over or pressed.
    pointer_row: Cell<Option<usize>>,
    /// The height of one row (logical pixels). Either measured or set with
    /// [`List::set_row_height`].
    row_height: Cell<f64>,
    /// Whether `row_height` is from rows that are gone, so the next measurement replaces
    /// it instead of only growing it.
    row_height_stale: Cell<bool>,
    /// Whether rows were built and not measured yet (they have no size until layout).
    row_height_pending: Cell<bool>,
    /// The row height decided by the app. If absent, it is measured from the built rows.
    fixed_row_height: Cell<Option<f64>>,
    mode: Cell<SelectionMode>,
    handler: SelectionHandler,
    /// Stops notifications only while the selection is being changed from code, since
    /// rewriting `IsSelected` also fires `SelectionChanged`.
    silent: Rc<Cell<bool>>,
    /// State for the window-wide wheel helper to choose the List's ScrollViewer.
    hovered: Arc<UiThreadCell<usize>>,
    /// The handler for double-clicks and Enter.
    row_activate: SelectHandler,
    /// The handler for right-clicks.
    context_menu: ContextMenuHandler,
}

/// A vertical list of selectable rows (`ListView`).
///
/// Its height follows its rows; to fix it regardless of the number of rows, specify it
/// with `set_sizing`. Do so for a list that may have more than about
/// [`ROW_WINDOW_THRESHOLD`](naui_core::ROW_WINDOW_THRESHOLD) rows: following the rows
/// builds all of them, and a UI Automation client walking them can make the app stop
/// responding, because the cost in WinUI's `ListView` grows steeply with the rows built.
#[derive(Clone)]
pub struct List(Rc<ListInner>);
impl_widget!(List, native);

impl List {
    pub(crate) fn new() -> Result<Self> {
        let (native, list_view) = build_surface()?;
        list_view
            .SetSelectionMode(ListViewSelectionMode::Single)
            .map_err(|e| to_error("set the ListView selection mode", e))?;
        // 横スクロールは持たせない (行は幅いっぱいに広げる)。
        let _ = ScrollViewer::SetHorizontalScrollBarVisibility2(
            &list_view,
            ScrollBarVisibility::Disabled,
        );
        let hovered = Arc::new(UiThreadCell::new(0));

        // `ListView` の Header / Footer で巨大な空白を動かすと、行があっても描かれない
        // ことがある (`Table` と同じ)。スクロールと詰め物は外側が持ち、`ListView` は
        // 実体化した行だけを測る。
        ScrollViewer::SetVerticalScrollBarVisibility2(&list_view, ScrollBarVisibility::Disabled)
            .map_err(|e| to_error("disable the row list scroll bar", e))?;
        ScrollViewer::SetVerticalScrollMode2(&list_view, ScrollMode::Disabled)
            .map_err(|e| to_error("disable the row list scrolling", e))?;
        let top_spacer = XamlGrid::new().map_err(|e| to_error("create the List spacer", e))?;
        let bottom_spacer = XamlGrid::new().map_err(|e| to_error("create the List spacer", e))?;
        let body = StackPanel::new().map_err(|e| to_error("create the List body", e))?;
        native
            .SetChild(Option::<&UIElement>::None)
            .map_err(|e| to_error("move the row list", e))?;
        let body_children = body.Children().map_err(|e| to_error("get the body", e))?;
        for element in [
            top_spacer.cast::<UIElement>(),
            list_view.cast::<UIElement>(),
            bottom_spacer.cast::<UIElement>(),
        ] {
            body_children
                .Append(&element.map_err(|e| to_error("convert the body to UIElement", e))?)
                .map_err(|e| to_error("add to the body", e))?;
        }
        let scroll = ScrollViewer::new().map_err(|e| to_error("create the list scroll area", e))?;
        // 横スクロールは持たせない (行は幅いっぱいに広げる)。
        scroll
            .SetHorizontalScrollBarVisibility(ScrollBarVisibility::Disabled)
            .map_err(|e| to_error("disable horizontal scrolling", e))?;
        scroll
            .SetContent(&body)
            .map_err(|e| to_error("set the list scroll content", e))?;
        native
            .SetChild(&scroll)
            .map_err(|e| to_error("add the list body", e))?;

        let this = Self(Rc::new(ListInner {
            native,
            list_view,
            scroll,
            body,
            top_spacer,
            bottom_spacer,
            wheel: RefCell::new(None),
            native_rows: RefCell::new(Vec::new()),
            realized: RefCell::new(Vec::new()),
            rows: RowSource::default(),
            window: Cell::new(RowWindow::default()),
            built_start: Cell::new(0),
            failed_window: Cell::new(None),
            rebuilding: Cell::new(false),
            selected: RefCell::new(Vec::new()),
            anchor: Cell::new(None),
            pointer_row: Cell::new(None),
            row_height: Cell::new(0.0),
            row_height_stale: Cell::new(false),
            row_height_pending: Cell::new(false),
            fixed_row_height: Cell::new(None),
            mode: Cell::new(SelectionMode::Single),
            handler: SelectionHandler::new(),
            silent: Rc::new(Cell::new(false)),
            hovered,
            row_activate: SelectHandler::new(),
            context_menu: ContextMenuHandler::new(),
        }));

        // ハンドルを強く持つと購読との間で循環するため、弱参照にする。
        //
        // 選べない行が押されたときは、この中から選択を描き戻す。その書き戻しで
        // `SelectionChanged` がその場でもう一度起きるため、`with_mut` では
        // 二重借用の panic が WinRT の境界を越えてクラッシュになる。再入を
        // 取りこぼしとして扱える `try_with_mut` を使う (再入時は `silent` が
        // 立っていて、どのみち捨てる通知になる)。
        let state = UiThreadCell::new(Rc::downgrade(&this.0));
        let handler = SelectionChangedEventHandler::new(move |_sender, _args| {
            let _ = state.try_with_mut(|weak| {
                if let Some(inner) = weak.upgrade() {
                    let list = List(inner);
                    if !list.0.silent.get() {
                        let indices = list.read_native_selection();
                        *list.0.selected.borrow_mut() = indices.clone();
                        list.0.handler.emit(&indices);
                    }
                }
            });
            Ok(())
        });
        this.0
            .list_view
            .SelectionChanged(&handler)
            .map_err(|e| to_error("subscribe to the ListView", e))?;
        this.install_wheel_target()?;
        this.install_window_updates()?;

        let state = UiThreadCell::new(Rc::downgrade(&this.0));
        let got_focus = RoutedEventHandler::new(move |_, _| {
            let _ = state.try_with_mut(|weak| {
                if let Some(inner) = weak.upgrade() {
                    List(inner).keep_focus_on_screen();
                }
            });
            Ok(())
        });
        this.0
            .list_view
            .GotFocus(&got_focus)
            .map_err(|e| to_error("subscribe to List focus changes", e))?;

        // `layout` のホイール補助は、子要素の上でも `ScrollViewer` を動かせる
        // ように、ウィンドウ全体のホイールを先に処理する。List の上では
        // List 専用の外側の ScrollViewer を選ばせるため、ホバー状態を共有する。
        let entered_state = this.0.hovered.clone();
        let entered = PointerEventHandler::new(move |_, _| {
            entered_state.with_mut(|hovered| *hovered = hovered.saturating_add(1));
            Ok(())
        });
        let exited_state = this.0.hovered.clone();
        let exited = PointerEventHandler::new(move |_, _| {
            exited_state.with_mut(|hovered| *hovered = hovered.saturating_sub(1));
            Ok(())
        });
        this.0
            .native
            .PointerEntered(&entered)
            .map_err(|e| to_error("subscribe to List pointer events", e))?;
        this.0
            .native
            .PointerExited(&exited)
            .map_err(|e| to_error("subscribe to List pointer events", e))?;

        // タブの切り替えなどで PointerEntered が発生しないままポインターが
        // List 上へ移動する場合がある。その場合も次のホイール入力より前に
        // List を対象として記録できるよう、PointerMoved でも補正する。
        let moved_state = this.0.hovered.clone();
        let moved = PointerEventHandler::new(move |_, _| {
            moved_state.with_mut(|hovered| {
                if *hovered == 0 {
                    *hovered = 1;
                }
            });
            Ok(())
        });
        this.0
            .native
            .PointerMoved(&moved)
            .map_err(|e| to_error("subscribe to List pointer events", e))?;
        Ok(this)
    }

    /// Registers the list's `ScrollViewer` as a destination of the wheel helper.
    ///
    /// After the display size is decided on `Loaded`, the visible range is updated too.
    fn install_wheel_target(&self) -> Result<()> {
        let state = UiThreadCell::new(Rc::downgrade(&self.0));
        let loaded = RoutedEventHandler::new(move |_, _| {
            let _ = state.try_with_mut(|weak| {
                if let Some(inner) = weak.upgrade() {
                    List(inner).register_wheel_target();
                }
            });
            Ok(())
        });
        self.0
            .list_view
            .Loaded(&loaded)
            .map_err(|e| to_error("subscribe to List Loaded", e))?;
        Ok(())
    }

    /// Registers with the wheel helper, exactly once.
    fn register_wheel_target(&self) {
        if self.0.wheel.borrow().is_some() {
            return;
        }
        let target =
            crate::layout::register_list_scroll(self.0.scroll.clone(), self.0.hovered.clone());
        *self.0.wheel.borrow_mut() = Some(target);
        // 大きさが決まったので、組み立てる範囲を引き直す。
        self.update_window();
    }

    /// Moves the built range along with scrolling and layout.
    ///
    /// `ViewChanged` also arrives for intermediate progress (dragging the thumb,
    /// inertia), so the rows follow while scrolling. Layout completing gets row swaps in
    /// before drawing, and it is also when newly built rows first have a size.
    fn install_window_updates(&self) -> Result<()> {
        let state = UiThreadCell::new(Rc::downgrade(&self.0));
        let handler = windows::Foundation::EventHandler::<IInspectable>::new(move |_, _| {
            let _ = state.try_with_mut(|weak| {
                if let Some(inner) = weak.upgrade() {
                    List(inner).update_window();
                }
            });
            Ok(())
        });
        self.0
            .list_view
            .LayoutUpdated(&handler)
            .map_err(|e| to_error("subscribe to List layout updates", e))?;

        let state = UiThreadCell::new(Rc::downgrade(&self.0));
        let handler = windows::Foundation::EventHandler::<ScrollViewerViewChangedEventArgs>::new(
            move |_, _| {
                let _ = state.try_with_mut(|weak| {
                    if let Some(inner) = weak.upgrade() {
                        List(inner).update_window();
                    }
                });
                Ok(())
            },
        );
        self.0
            .scroll
            .ViewChanged(&handler)
            .map_err(|e| to_error("subscribe to List scroll position changes", e))?;

        // `Auto` の高さでは全行を組み立て、指定があれば絞る。実際の大きさが同じままの
        // 変更もあるので、`SizeChanged` は待たない。
        let target = Rc::downgrade(&self.0);
        crate::layout::on_sizing_change(&self.native_element(), move || {
            if let Some(inner) = target.upgrade() {
                List(inner).update_window();
            }
        });
        Ok(())
    }

    /// Rebuilds the rows. Indices change meaning, so the selection is cleared.
    pub fn set_items(&self, items: &[ListItem]) {
        self.0
            .rows
            .set_rows(items.iter().cloned().map(ListRow::from_item).collect());
        self.reset_rows();
    }

    /// Rebuilds the rows. Both plain text rows and custom-content rows are built
    /// through this path.
    ///
    /// Controls inside a row can have their own callbacks as usual. Indices change
    /// meaning, so the selection is cleared.
    pub fn set_rows(&self, rows: &[ListRow]) {
        self.0.rows.set_rows(rows.to_vec());
        self.reset_rows();
    }

    /// Passes rows in a form that **builds each row when it becomes visible**.
    ///
    /// `count` is the number of rows, and `build` receives an index from 0 to
    /// `count - 1` and returns that row ([`ListRow`]). It is called only for the rows on
    /// screen (and a few before and after), so opening stays fast even with hundreds of
    /// thousands of rows. Indices change meaning, so the selection is cleared.
    ///
    /// The widgets in a row may be recreated every time it is built. Keep per-row state
    /// in the app's data and build from it inside `build`.
    ///
    /// ```no_run
    /// # use naui_windows::{List, ListRow, Ui};
    /// # fn fill(ui: &Ui, list: &List, cities: Vec<String>) {
    /// let ui = ui.clone();
    /// list.set_row_builder(cities.len(), move |index| {
    ///     Ok(ListRow::new(&ui.label(&cities[index])?))
    /// });
    /// # }
    /// ```
    pub fn set_row_builder(
        &self,
        count: usize,
        build: impl FnMut(usize) -> Result<ListRow> + 'static,
    ) {
        self.0.rows.set_builder(count, build);
        self.reset_rows();
    }

    /// Passes a function that answers whether a row can be selected **without building
    /// the row**.
    ///
    /// For rows built with [`List::set_row_builder`], naui does not know whether rows
    /// not yet built can be selected. With this function, [`List::set_selection`] can
    /// keep its promise of "removing rows that cannot be selected" even for rows off
    /// screen. Without it, rows follow the [`ListRow::selectable`] they were last built
    /// with, even after leaving the screen, and rows not built since the rows were passed
    /// or refreshed are treated as selectable.
    ///
    /// Its answers must match the [`ListRow::selectable`] returned by `build`. It stays when
    /// the rows are replaced, so pass a new one if its answers change.
    pub fn set_row_selectable(&self, selectable: impl Fn(usize) -> bool + 'static) {
        self.0.rows.set_selectable(selectable);
    }

    /// Rebuilds the visible rows. The row count and selection stay.
    ///
    /// Use it when what the function passed to [`List::set_row_builder`] returns has
    /// changed.
    pub fn refresh(&self) {
        self.0.row_height_stale.set(true);
        self.0.rows.forget_selectable();
        // 行を組み立てる関数の中から呼ばれたら、組み立てている側が最初からやり直す。
        if self.0.rebuilding.get() {
            self.0.rows.invalidate();
            return;
        }
        self.build_window();
        // 中身や指定が変わって行の高さが動いていれば、窓の位置も変わる。測った結果が
        // 同じでも窓を引き直す (`set_row_height` は先に高さを書き換えている)。
        self.settle_row_height();
        self.update_window();
    }

    /// Sets the row height in logical pixels. Passing 0 or less measures it from the
    /// rows.
    ///
    /// Every row gets at least this height. With more rows than
    /// [`ROW_WINDOW_THRESHOLD`](naui_core::ROW_WINDOW_THRESHOLD), rows off screen are
    /// filled in as "row count × this height", so set it when the rows' heights vary. A
    /// row built later that needs more raises the height for every row.
    pub fn set_row_height(&self, height: f64) {
        self.0
            .fixed_row_height
            .set((height > 0.0).then_some(height));
        // 指定を外したときは、組み立て直した行から測り直す。
        self.0.row_height.set(height.max(0.0));
        self.refresh();
    }

    /// The row height (logical pixels). 0 if there are no rows or none has been built yet.
    ///
    /// With few rows and no height set, rows may differ, and this is the tallest built.
    pub fn row_height(&self) -> f64 {
        if self.is_empty() && self.0.fixed_row_height.get().is_none() {
            return 0.0;
        }
        self.0.row_height.get()
    }

    // ------------------------------------------------------- 行を絞る窓

    /// Rebuilds the rows and moves the window back to the start. The selection is
    /// cleared too.
    fn reset_rows(&self) {
        self.0.selected.borrow_mut().clear();
        self.0.anchor.set(None);
        self.0.row_height_stale.set(true);
        // 組み立ての途中で差し替えられたら、組み立てている側が新しい行でやり直す。
        if self.0.rebuilding.get() {
            return;
        }
        self.0.window.set(self.compute_window(ROW_WINDOW_OVERSCAN));
        self.build_window();
        if self.settle_row_height() {
            self.update_window();
        }
    }

    /// Rebuilds every `ListView` row of the current window.
    fn build_window(&self) {
        self.build_window_from(None);
    }

    /// Builds the current window. The rows `previous` has in common with it stay as they
    /// are, unless the rows are replaced during the build.
    fn build_window_from(&self, previous: Option<RowWindow>) {
        // 組み立ての途中でアプリのコードが動くので、そこから呼ばれても
        // 二重に作り直さない。
        if self.0.rebuilding.replace(true) {
            return;
        }
        self.0.rows.build_until_settled(|replaced| {
            if replaced {
                self.0.window.set(self.compute_window(ROW_WINDOW_OVERSCAN));
            }
            if let Err(error) = self.build_window_once(previous.filter(|_| !replaced)) {
                eprintln!("naui-windows: failed to build the list rows: {error}");
            }
        });
        self.0.rebuilding.set(false);
    }

    fn build_window_once(&self, previous: Option<RowWindow>) -> Result<()> {
        // コレクションを空にせず置換し、スクロールのリセットを避ける。
        // 行数が減って位置が補正された場合だけ、レイアウト後に戻す。
        let offset = self.0.scroll.VerticalOffset().ok();
        let children = self
            .0
            .list_view
            .Items()
            .map_err(|e| to_error("get the rows", e))?;
        let previous_count = children
            .Size()
            .map_err(|e| to_error("get the row count", e))?;
        let focused = self.focused_row();
        let window = self.0.window.get();
        let generation = self.0.rows.generation();
        let built = std::mem::take(&mut *self.0.native_rows.borrow_mut());
        let built_rows = std::mem::take(&mut *self.0.realized.borrow_mut());
        let built_start = self.0.built_start.get();
        // 窓に残る行は触らない。作り直すと、中のコントロールのフォーカスが失われる。
        // 前回の組み立てが途中で終わっていたら、位置が合わないので全部作り直す。
        // 全行を組み立てるときも、そろえた高さから行ごとの高さへ戻すために作り直す。
        let kept = previous
            .filter(|previous| previous.len() == built.len() && !window.is_complete())
            .map_or(0..0, |previous| previous.overlap(window));
        // 残る行が無いときは、取り除かずに置き換える (上のコメントのとおり)。
        let replaceable = match kept.is_empty() {
            true => previous_count,
            false => 0,
        };
        // 高さが分かっていれば、作るときから同じ高さにする (測るまで行が動かない)。
        let min_height = match window.is_complete() || self.0.row_height_stale.get() {
            true => 0.0,
            false => self.0.row_height.get(),
        };
        let mut native_rows = Vec::with_capacity(window.len());
        let mut realized = Vec::with_capacity(window.len());
        // 失敗しても `ListView` に残っている行は、管理から外さない。外すと、次の組み立てで
        // 中身を取り出せず、アプリが持っているウィジェットを載せ直せなくなる。窓は実際に
        // 組み立てた行まで縮めるので、行の番号はずれない。
        let mut outcome = Ok(());
        for (offset, (native, row)) in built.into_iter().zip(built_rows).enumerate() {
            if kept.contains(&(built_start + offset)) {
                native_rows.push(native);
                realized.push(row);
                continue;
            }
            release_row_content(&native);
            if kept.is_empty() {
                continue;
            }
            let at = native_rows.len() as u32;
            if let Err(error) = self.without_notifying(|_| children.RemoveAt(at)) {
                outcome = Err(to_error("remove the row", error));
                native_rows.push(native);
                realized.push(row);
            }
        }
        // 残る行から外側へ組み立てるので、途中で失敗しても行は連続している。
        let mut first = match kept.is_empty() {
            true => window.start,
            false => kept.start,
        };
        let after_kept = window
            .indices()
            .filter(|&index| kept.is_empty() || index >= kept.end);
        let before_kept = (window.start..kept.start).rev();
        for index in after_kept.chain(before_kept) {
            if outcome.is_err() {
                break;
            }
            let Some(row) = self.0.rows.row(index) else {
                continue;
            };
            // 差し替えられたら、この行も残りの窓ももう古い。
            if self.0.rows.generation() != generation {
                break;
            }
            // 組み立てに失敗した行は、空の行にする。
            let mut row = row.unwrap_or_else(|_| ListRow::from_item(ListItem::new("")));
            // 行に載せられないウィジェットが返ったときも同じ。行が欠けると、行の位置と
            // 窓の位置がずれて、選択やフォーカスが別の行へ届く。
            let built = self
                .new_native_row(index, &row, window.count, min_height)
                .or_else(|error| {
                    eprintln!("naui-windows: failed to build list row {index}: {error}");
                    row = ListRow::from_item(ListItem::new(""));
                    self.new_native_row(index, &row, window.count, min_height)
                });
            self.0.rows.note_selectable(index, row.is_selectable());
            let native = match built {
                Ok(native) => native,
                Err(error) => {
                    outcome = Err(error);
                    break;
                }
            };
            let before = index < kept.start;
            let slot = match before {
                true => 0,
                false => native_rows.len(),
            };
            let placed = native.cast::<IInspectable>().and_then(|element| {
                self.without_notifying(|_| {
                    if before {
                        children.InsertAt(slot as u32, &element)
                    } else if (slot as u32) < replaceable {
                        children.SetAt(slot as u32, &element)
                    } else {
                        children.Append(&element)
                    }
                })
            });
            if let Err(error) = placed {
                release_row_content(&native);
                outcome = Err(to_error("place the row", error));
                break;
            }
            native_rows.insert(slot, native);
            realized.insert(slot, row);
            first -= usize::from(before);
        }
        for _ in native_rows.len() as u32..replaceable {
            if let Err(error) = self.without_notifying(|_| children.RemoveAtEnd()) {
                outcome = Err(to_error("remove the extra rows", error));
                break;
            }
        }
        let built_count = native_rows.len();
        *self.0.native_rows.borrow_mut() = native_rows;
        *self.0.realized.borrow_mut() = realized;
        self.0.built_start.set(first);
        self.0.row_height_pending.set(true);
        // 同じ窓で何度も失敗しないよう、やり直すのは窓が動くか `refresh` のときにする。
        self.0.failed_window.set(outcome.is_err().then_some(window));
        let window = match outcome.is_err() {
            true => RowWindow {
                start: first,
                end: first + built_count,
                count: window.count,
            },
            false => window,
        };
        self.0.window.set(window);

        let height = self.0.row_height.get();
        set_spacer_height(&self.0.top_spacer, window.leading(height));
        set_spacer_height(&self.0.bottom_spacer, window.trailing(height));

        // 覚えている選択を、組み立て直した行へ写す。
        let picked = self.selection();
        self.write_selection(&picked);

        self.0
            .scroll
            .UpdateLayout()
            .map_err(|e| to_error("update the list layout", e))?;
        // 矢印キーで窓の端まで来たときに組み立て直しても、操作が続けられるように。
        // 窓に残る行も、手前の行を取り除くと `ListView` がフォーカスを持ち去ることがある。
        let now_focused = self.focused_row().map(|(index, _)| index);
        if let Some((index, state)) = focused.filter(|&(index, _)| now_focused != Some(index)) {
            self.restore_focus(index, state);
        }
        let Some(offset) = offset else {
            return outcome;
        };
        // 毎回 ChangeView を呼ぶと、ホイールの進行中の移動まで止めてしまう。
        if (self.0.scroll.VerticalOffset().unwrap_or(offset) - offset).abs() < 0.5 {
            return outcome;
        }
        outcome.and(self.change_view(offset))
    }

    /// Creates the `ListViewItem` of one row.
    fn new_native_row(
        &self,
        index: usize,
        row: &ListRow,
        count: usize,
        min_height: f64,
    ) -> Result<ListViewItem> {
        let native = new_row_item()?;
        let _ = native.SetPadding(item_padding(row.is_selectable()));
        match self.0.fixed_row_height.get() {
            Some(height) => {
                let _ = native.SetMinHeight(height);
            }
            None if min_height > 0.0 => {
                let _ = native.SetMinHeight(min_height);
            }
            None => {}
        }
        let host = row_host(row)?;
        // 取り込んだウィジェットを、捨てる入れ物に残さない。
        let in_row = |result: windows_core::Result<()>| {
            result.map_err(|error| {
                release_host_content(&host);
                to_error("set the row content", error)
            })
        };
        // スクロールのあとポインターを動かさずに押すと、`PointerEntered` は来ない。
        let state = UiThreadCell::new(Rc::downgrade(&self.0));
        let pressed = PointerEventHandler::new(move |_, _| {
            let _ = state.try_with_mut(|weak| {
                if let Some(inner) = weak.upgrade() {
                    inner.pointer_row.set(Some(index));
                }
            });
            Ok(())
        });
        let _ = host.PointerPressed(&pressed);
        in_row(native.SetContent(&host))?;
        // 選べないだけの行は中のコントロールを押せる必要があるので、
        // 無効にするのは `ListItem::enabled(false)` の行だけ。
        let _ = native.SetIsEnabled(!row.is_disabled_item());
        report_row_position(&native, index, count);
        // 無効の行には入力が届かないので、起動も文脈メニューも出ない。
        if let Err(error) = self.listen_row_events(&native, index) {
            release_row_content(&native);
            return Err(error);
        }
        Ok(native)
    }

    /// Moves the keyboard cursor from row `from`, as `ListView` would if it had every row.
    /// `false` if `ListView` should do it.
    ///
    /// `ListView` knows only the rows it has, and it takes those as the page because the
    /// scrolling is outside it: Page Down goes straight to the last one even in a short
    /// list. It also keeps the Shift anchor as a place among those rows. So the movements
    /// that depend on either are done here, before it sees the key.
    fn move_cursor(&self, from: usize, motion: RowMotion, modifiers: Modifiers) -> bool {
        let height = self.0.row_height.get();
        let count = self.len();
        let extending = modifiers.shift && self.0.mode.get().is_multiple();
        // 高さが行に従うリストは全行を持ち、スクロールするのは親なので、任せたままでよい。
        if height <= 0.0 || count == 0 || !self.height_is_limited() {
            return false;
        }
        let viewport = self.0.scroll.ViewportHeight().unwrap_or(0.0);
        let view_top = self.0.scroll.VerticalOffset().unwrap_or(0.0);
        let (target, top) = match motion {
            RowMotion::Pages(pages) => {
                let distance = (viewport / height).floor().max(1.0) as usize;
                let target = match pages < 0 {
                    true => from.saturating_sub(distance),
                    false => from.saturating_add(distance).min(count - 1),
                };
                (target, view_top + viewport * f64::from(pages))
            }
            RowMotion::Lines(lines) => (
                from.saturating_add_signed(lines as isize).min(count - 1),
                view_top,
            ),
            RowMotion::First => (0, view_top),
            RowMotion::Last => (count - 1, view_top),
        };
        // 選択を動かす移動は、選べない行 (見出しなど) へ止まらない。止まると選択が空になる。
        // Ctrl はフォーカスだけなので、行の中のコントロールへ入れるように止まる。Space は
        // その行のままなので探さない。
        let selecting = (extending || !modifiers.control) && !matches!(motion, RowMotion::Lines(0));
        let target = match selecting {
            true => {
                match self.nearest_row(target, from, 0..count, |row| self.is_selectable(*row)) {
                    Some(target) => target,
                    None => return true,
                }
            }
            false => target,
        };
        let top = self.offset_showing(target, top, viewport);
        // `ChangeView` はすぐには位置へ反映されないので、窓は移る先の位置から決める。
        self.move_window(top);
        self.scroll_now(top);
        // ビルダーの行は、組み立てて初めて選べないと分かることがある。
        let Some(landed) = self.focus_nearest(target, from, FocusState::Keyboard, selecting) else {
            return true;
        };
        // 無効の行を飛ばすと、移った行が画面の外のことがある。
        if landed != target {
            self.scroll_now(self.offset_showing(landed, top, viewport));
        }
        let picked = match (extending, modifiers.control) {
            (true, _) => {
                let anchor = self.0.anchor.get().unwrap_or(from);
                self.0.anchor.set(Some(anchor));
                let range: Vec<usize> = (anchor.min(landed)..=anchor.max(landed)).collect();
                self.normalize(&range)
            }
            // Ctrl + Space は足し引きする。
            (false, true) if matches!(motion, RowMotion::Lines(0)) => {
                self.0.anchor.set(Some(landed));
                let mut picked = self.selection();
                match picked.binary_search(&landed) {
                    Ok(at) => {
                        picked.remove(at);
                    }
                    Err(at) => picked.insert(at, landed),
                }
                self.normalize(&picked)
            }
            // Ctrl はフォーカスだけを動かす。
            (false, true) => return true,
            // 選べない行の上の Space は何もしない。
            (false, false) if !self.is_selectable(landed) => return true,
            (false, false) => {
                self.0.anchor.set(Some(landed));
                self.normalize(&[landed])
            }
        };
        if picked != self.selection() {
            self.write_selection(&picked);
            self.0.handler.emit(&picked);
        }
        true
    }

    /// Selects every row for Ctrl + A, including those not built: `ListView` would select
    /// only the ones it has. `false` if only one row can be selected.
    fn select_every_row(&self) -> bool {
        if !self.0.mode.get().is_multiple() {
            return false;
        }
        let every: Vec<usize> = (0..self.len()).collect();
        let picked = self.normalize(&every);
        if picked != self.selection() {
            self.write_selection(&picked);
            self.0.handler.emit(&picked);
        }
        true
    }

    /// Whether the app limited the height (`Fixed` or `Fill`), so the list scrolls itself.
    fn height_is_limited(&self) -> bool {
        crate::layout::height_is_limited(&self.native_element())
    }

    /// The scroll position nearest `top` that shows row `index`.
    fn offset_showing(&self, index: usize, top: f64, viewport: f64) -> f64 {
        let (row_top, row_bottom) = self.row_span(index);
        if row_top < top {
            row_top
        } else if row_bottom > top + viewport {
            row_bottom - viewport
        } else {
            top
        }
    }

    /// Moves the scroll position, in place by the time this returns.
    fn scroll_now(&self, offset: f64) {
        let current = self.0.scroll.VerticalOffset().unwrap_or(offset);
        if (offset - current).abs() >= 0.5 && self.change_view(offset).is_ok() {
            let _ = self.0.scroll.UpdateLayout();
        }
    }

    /// The nearest row in `rows` that `accepts`, looking on from `target` in the direction
    /// of travel from `from`, or else back toward `from`.
    fn nearest_row(
        &self,
        target: usize,
        from: usize,
        rows: Range<usize>,
        accepts: impl FnMut(&usize) -> bool,
    ) -> Option<usize> {
        let mut accepts = accepts;
        match target >= from {
            true => (target..rows.end)
                .find(&mut accepts)
                .or_else(|| (from..target).rev().find(&mut accepts)),
            false => (rows.start..=target)
                .rev()
                .find(&mut accepts)
                .or_else(|| (target + 1..=from).find(&mut accepts)),
        }
    }

    /// Focuses row `target`, or if it cannot take the focus (it is disabled) or
    /// `selectable_only` rules it out, the nearest built row that fits. Returns that row.
    fn focus_nearest(
        &self,
        target: usize,
        from: usize,
        state: FocusState,
        selectable_only: bool,
    ) -> Option<usize> {
        let window = self.0.window.get();
        if window.is_empty() {
            return None;
        }
        let target = target.clamp(window.start, window.end - 1);
        let from = from.clamp(window.start, window.end - 1);
        let start = self.0.built_start.get();
        let rows = self.0.native_rows.borrow().clone();
        self.nearest_row(target, from, window.start..window.end, |index| {
            (!selectable_only || self.is_selectable(*index))
                && rows
                    .get(index - start)
                    .and_then(|row| row.cast::<Control>().ok())
                    .is_some_and(|row| row.Focus(state).unwrap_or(false))
        })
    }

    /// Moves a focus that landed on a row off screen to the nearest row on screen.
    ///
    /// Focus changes do not scroll a list that scrolls itself (see
    /// [`List::update_window`]), and `ListView` picks rows without looking at what is on
    /// screen: Tab can land on the first row built. The next key would then pull the view
    /// to that row.
    fn keep_focus_on_screen(&self) {
        let height = self.0.row_height.get();
        let viewport = self.0.scroll.ViewportHeight().unwrap_or(0.0);
        if self.0.rebuilding.get() || !self.height_is_limited() || height <= 0.0 || viewport <= 0.0
        {
            return;
        }
        let Some((index, state)) = self.focused_row() else {
            return;
        };
        let top = self.0.scroll.VerticalOffset().unwrap_or(0.0);
        let (row_top, row_bottom) = self.row_span(index);
        let target = if row_bottom <= top {
            (top / height).ceil() as usize
        } else if row_top >= top + viewport {
            (((top + viewport) / height).floor() as usize).saturating_sub(1)
        } else {
            return;
        };
        self.focus_nearest(target, index, state, false);
    }

    /// The row holding the keyboard focus itself, and how it got it.
    fn focused_row(&self) -> Option<(usize, FocusState)> {
        let rows = self.0.native_rows.borrow();
        rows.iter().enumerate().find_map(|(offset, row)| {
            let state = row.cast::<Control>().and_then(|c| c.FocusState()).ok()?;
            (state != FocusState::Unfocused).then_some((self.0.built_start.get() + offset, state))
        })
    }

    /// Gives the focus back to a row just rebuilt, if it is on screen.
    ///
    /// `Focus` also scrolls the row into view, so a row the wheel is moving away from is
    /// left alone: the scrolling would be pulled back to it.
    fn restore_focus(&self, index: usize, state: FocusState) {
        let window = self.0.window.get();
        if !window.contains(index) {
            return;
        }
        let start = self.0.built_start.get();
        let height = self.0.row_height.get();
        let top = index as f64 * height;
        let view_top = self.0.scroll.VerticalOffset().unwrap_or(0.0);
        let viewport = self.0.scroll.ViewportHeight().unwrap_or(0.0);
        if top + height < view_top || top > view_top + viewport {
            return;
        }
        if let Some(row) = self.0.native_rows.borrow().get(index - start) {
            let _ = row.cast::<Control>().and_then(|row| row.Focus(state));
        }
    }

    /// Moves the vertical scroll without animation.
    fn change_view(&self, offset: f64) -> Result<()> {
        let offset = PropertyValue::CreateDouble(offset)
            .and_then(|value| value.cast::<windows::Foundation::IReference<f64>>())
            .map_err(|e| to_error("box the scroll position", e))?;
        self.0
            .scroll
            .ChangeViewWithOptionalAnimation(None, &offset, None, true)
            .map_err(|e| to_error("move the scroll position", e))?;
        Ok(())
    }

    /// Computes the range of rows to build from the current scroll position.
    fn compute_window(&self, overscan: usize) -> RowWindow {
        self.window_at(self.0.scroll.VerticalOffset().unwrap_or(0.0), overscan)
    }

    /// Computes the range of rows to build at a scroll position.
    fn window_at(&self, offset: f64, overscan: usize) -> RowWindow {
        // 詰め物が窓の外の行と同じ高さを持つので、表示位置は
        // 「全行を入れたとき」と同じ座標になる。
        list_row_window(
            crate::layout::height_is_limited(&self.native_element()),
            self.len(),
            offset,
            self.0.scroll.ViewportHeight().unwrap_or(0.0),
            self.0.row_height.get(),
            overscan,
        )
    }

    /// Moves the built range along with scrolling.
    ///
    /// Rebuilding for every row scrolled is heavy, so the window is reused until you
    /// have scrolled through half of the extra rows ([`ROW_WINDOW_OVERSCAN`]). Even then,
    /// only the rows that were not there are built.
    fn update_window(&self) {
        // UpdateLayout 中の再入では、差し替え途中の位置を読まない。
        if self.0.rebuilding.get() {
            return;
        }
        // 自分でスクロールするリストでは、フォーカスの自動スクロールを止める。フォーカスの
        // 行が窓から外れると XAML が画面の外の行へフォーカスを移し、遅れてそこへ飛ぶので、
        // 押下と解放の間だとクリックまで失われる。キーで移るときの位置は `move_cursor` が
        // 決める。高さが行に従うリストをスクロールするのは親なので、そのまま任せる。
        let _ = self
            .0
            .scroll
            .SetBringIntoViewOnFocusChange(!self.height_is_limited());
        // 行の追加直後は ActualHeight が 0 のことがある。レイアウトのあとで測り、
        // 全行分のスクロール領域を確保する。
        if self.0.row_height_pending.get() {
            self.settle_row_height();
        }
        // 隠れている間は大きさが読めないので、組み立てた行をそのまま残す。
        if self.0.scroll.ViewportHeight().unwrap_or(0.0) <= 0.0 && !self.is_empty() {
            return;
        }
        self.move_window(self.0.scroll.VerticalOffset().unwrap_or(0.0));
    }

    /// Moves the built range for a scroll position, unless the rows built are enough.
    fn move_window(&self, offset: f64) {
        let current = self.0.window.get();
        let needed = self.window_at(offset, ROW_WINDOW_OVERSCAN / 2);
        let next = self.window_at(offset, ROW_WINDOW_OVERSCAN);
        // 前回の組み立てが途中で失敗していたら、残った行は使わずに全部作り直す。
        // 失敗した窓そのものは、レイアウトのたびにやり直さない。
        let failed = self.0.failed_window.get();
        if failed == Some(next)
            || (failed.is_none() && (next == current || keeps_row_window(current, needed, next)))
        {
            return;
        }
        self.0.window.set(next);
        self.build_window_from(failed.is_none().then_some(current));
        if self.settle_row_height() {
            self.move_window(offset);
        }
    }

    /// Finds the height of one row from the built rows. `true` if it changed.
    ///
    /// Row height is decided by the content, so it is not known until a row is laid out.
    /// **The tallest built row is used, and every built row is given that height.**
    /// The rows outside the window are filled in as "row count × this height", so rows
    /// of differing height (only some with `detail`, for instance) would make the
    /// scroll bar drift every time the window moves. A taller row appearing later grows
    /// the height once.
    fn settle_row_height(&self) -> bool {
        let rows = self.0.native_rows.borrow().clone();
        let tallest = rows
            .iter()
            .filter_map(|row| row.ActualHeight().ok())
            .fold(0.0, f64::max);
        // まだレイアウトされていない。次のレイアウトのあとで測る。
        if tallest <= 0.0 {
            return false;
        }
        self.0.row_height_pending.set(false);
        let previous = self.0.row_height.get();
        // 指定は下限。中身がそれより高い行は伸びるので、窓の計算もそちらに合わせる。
        let measured = match self.0.row_height_stale.replace(false) {
            true => tallest,
            false => tallest.max(previous),
        }
        .max(self.0.fixed_row_height.get().unwrap_or(0.0));
        let window = self.0.window.get();
        // 行が少なければ行ごとの高さのまま (窓の外の行が無いので、ずれない)。
        if !window.is_complete() {
            for row in &rows {
                let _ = row.SetMinHeight(measured);
            }
        }
        self.0.row_height.set(measured);
        set_spacer_height(&self.0.top_spacer, window.leading(measured));
        set_spacer_height(&self.0.bottom_spacer, window.trailing(measured));
        (previous - measured).abs() >= 0.5
    }

    /// Where a row is within the scrolled content (top and bottom).
    ///
    /// With every row built, it is the row's real position (heights may differ). While
    /// only some rows are built, it is computed from the row height, so it can point at
    /// rows that do not exist yet.
    fn row_span(&self, index: usize) -> (f64, f64) {
        if self.0.window.get().is_complete() {
            let item = self.0.native_rows.borrow().get(index).cloned();
            let span = item.and_then(|item| {
                let height = item.ActualHeight().ok().filter(|&height| height > 0.0)?;
                let at = item
                    .TransformToVisual(&self.0.body)
                    .and_then(|transform| transform.TransformPoint(XamlPoint { X: 0.0, Y: 0.0 }))
                    .ok()?;
                Some((f64::from(at.Y), f64::from(at.Y) + height))
            });
            if let Some(span) = span {
                return span;
            }
        }
        let height = self.0.row_height.get().max(1.0);
        let top = index as f64 * height;
        (top, top + height)
    }

    /// Receives row activation (double-click, Enter) and context menu requests.
    ///
    /// A row stays this row for as long as it is built, so the index can be the row's place
    /// in the whole list at creation. The subscriptions hold the `List` weakly (avoiding the cycle row →
    /// subscription → `List` → row).
    fn listen_row_events(&self, native: &ListViewItem, index: usize) -> Result<()> {
        let activate = self.0.row_activate.clone();
        let double = DoubleTappedEventHandler::new(move |sender, args| {
            // ボタンは `DoubleTapped` を止めないので、行内のコントロールを
            // ダブルクリックしても行まで上がってくる。それは起動にしない。
            let source = args.as_ref().and_then(|args| args.OriginalSource().ok());
            if let (Some(source), Some(sender)) = (source, sender.as_ref()) {
                if pressed_on_control(&source, sender) {
                    return Ok(());
                }
            }
            activate.emit(index);
            Ok(())
        });
        native
            .DoubleTapped(&double)
            .map_err(|e| to_error("subscribe to row double clicks", e))?;

        let activate = self.0.row_activate.clone();
        let state = UiThreadCell::new(Rc::downgrade(&self.0));
        let keyed = KeyEventHandler::new(move |sender, args| {
            let Some(args) = args.as_ref() else {
                return Ok(());
            };
            // 行の中の入力欄やボタンが受けたキーは、そのコントロールのもの。
            // 行そのものにフォーカスがあるときだけ行の操作にする。
            let on_row = match (args.OriginalSource(), sender.as_ref()) {
                (Ok(source), Some(sender)) => &source == sender,
                _ => false,
            };
            let key = args.Key().unwrap_or(VirtualKey::None);
            let modifiers = crate::keys::modifiers();
            let plain_ctrl =
                modifiers.control && !modifiers.shift && !modifiers.alt && !modifiers.meta;
            if on_row && key == VirtualKey::A && plain_ctrl {
                let selected = state
                    .try_with_mut(|weak| {
                        weak.upgrade()
                            .is_some_and(|inner| List(inner).select_every_row())
                    })
                    .unwrap_or(false);
                if selected {
                    let _ = args.SetHandled(true);
                }
                return Ok(());
            }
            if let Some(motion) = RowMotion::of_key(key).filter(|_| on_row) {
                let moved = !modifiers.alt
                    && !modifiers.meta
                    && state
                        .try_with_mut(|weak| {
                            weak.upgrade().is_some_and(|inner| {
                                List(inner).move_cursor(index, motion, modifiers)
                            })
                        })
                        .unwrap_or(false);
                if moved {
                    let _ = args.SetHandled(true);
                }
                return Ok(());
            }
            let modified = modifiers.shift || modifiers.control || modifiers.alt || modifiers.meta;
            if !on_row || key != VirtualKey::Enter || modified {
                return Ok(());
            }
            // 受け取り手がいないときは止めず、親のキー処理へ渡す。
            if !activate.is_set() {
                return Ok(());
            }
            let _ = args.SetHandled(true);
            activate.emit(index);
            Ok(())
        });
        native
            .KeyDown(&keyed)
            .map_err(|e| to_error("subscribe to row key presses", e))?;

        // Shift クリックの先の行は `SelectionChanged` では分からない。行の余白の押下は
        // `ListViewItem` が止めるので、ポインターが乗った行も覚えておく。
        let state = UiThreadCell::new(Rc::downgrade(&self.0));
        let entered = PointerEventHandler::new(move |_, _| {
            let _ = state.try_with_mut(|weak| {
                if let Some(inner) = weak.upgrade() {
                    inner.pointer_row.set(Some(index));
                }
            });
            Ok(())
        });
        native
            .PointerEntered(&entered)
            .map_err(|e| to_error("subscribe to row pointer events", e))?;

        let state = UiThreadCell::new(Rc::downgrade(&self.0));
        let requested = TypedEventHandler::new(
            move |sender: windows_core::Ref<'_, UIElement>,
                  args: windows_core::Ref<'_, ContextRequestedEventArgs>| {
                let Some(args) = args.as_ref() else {
                    return Ok(());
                };
                let _ = state.try_with_mut(|weak| {
                    let Some(inner) = weak.upgrade() else {
                        return;
                    };
                    if !inner.context_menu.is_set() {
                        return;
                    }
                    let point = context_point(&inner.native, sender.as_ref(), args);
                    // 祖先に付けた `PopupMenu` (`ContextFlyout`) と二重に出さない。
                    let _ = args.SetHandled(true);
                    inner.context_menu.emit(index, point);
                });
                Ok(())
            },
        );
        native
            .ContextRequested(&requested)
            .map_err(|e| to_error("subscribe to the row context menu", e))?;
        Ok(())
    }

    /// The number of rows.
    pub fn len(&self) -> usize {
        self.0.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Changes how rows are selected. The meaning of the selection changes, so the
    /// selection is cleared.
    ///
    /// Multiple selection maps to WinUI's `Extended` (select while holding Ctrl /
    /// Shift). `Multiple` toggles on every click, which does not match macOS / Web.
    pub fn set_selection_mode(&self, mode: SelectionMode) {
        self.0.mode.set(mode);
        let native = if mode.is_multiple() {
            ListViewSelectionMode::Extended
        } else {
            ListViewSelectionMode::Single
        };
        let _ = self.without_notifying(|this| this.0.list_view.SetSelectionMode(native));
        self.clear_selection();
    }

    pub fn selection_mode(&self) -> SelectionMode {
        self.0.mode.get()
    }

    /// The topmost of the selected rows.
    pub fn selected(&self) -> Option<usize> {
        self.selection().first().copied()
    }

    /// The selected rows (ascending). Zero or one for single selection.
    ///
    /// Rows outside the window are included. The `ListView` knows only the rows that are
    /// built, so the remembered selection is returned.
    pub fn selection(&self) -> Vec<usize> {
        self.0.selected.borrow().clone()
    }

    /// Scrolls until row `index` is visible. The selection does not change.
    ///
    /// If it is already visible nothing moves; otherwise it is aligned to the nearer
    /// edge (the top if it is above, the bottom if it is below). Does nothing if out of
    /// range.
    pub fn scroll_to_row(&self, index: usize) {
        if index >= self.len() {
            return;
        }
        // 組み立てたばかりの行にも位置が付くように、レイアウトを先に済ませる。
        let _ = self.0.scroll.UpdateLayout();
        if self.0.row_height_pending.get() {
            self.settle_row_height();
        }
        let (top, bottom) = self.row_span(index);
        let view_top = self.0.scroll.VerticalOffset().unwrap_or(0.0);
        let viewport = self.0.scroll.ViewportHeight().unwrap_or(0.0);
        let target = if top < view_top {
            top
        } else if bottom > view_top + viewport {
            bottom - viewport
        } else {
            return;
        };
        // 位置を先に動かさないと、窓が古い位置で決まる。
        self.scroll_now(target);
        self.update_window();
    }

    /// Selects exactly one row without notifying.
    pub fn set_selected(&self, index: usize) {
        self.set_selection(&[index]);
    }

    /// Replaces the selection without notifying.
    ///
    /// Out-of-range rows, unselectable rows and duplicates are removed, and with single
    /// selection only the first remains ([`SelectionMode::normalize`]).
    pub fn set_selection(&self, indices: &[usize]) {
        let picked = self.normalize(indices);
        self.0.anchor.set(picked.last().copied());
        self.write_selection(&picked);
    }

    /// Clears the whole selection without notifying.
    pub fn clear_selection(&self) {
        self.0.anchor.set(None);
        self.write_selection(&[]);
    }

    /// Selects one row through the same path as a user's selection (notifies).
    pub fn select(&self, index: usize) {
        self.select_many(&[index]);
    }

    /// Replaces the selection through the same path as a user's selection (notifies).
    pub fn select_many(&self, indices: &[usize]) {
        self.set_selection(indices);
        // 同じ選択を選び直すと `SelectionChanged` は起きないため、
        // 通知の回数をそろえてここで 1 回だけ出す。
        let actual = self.selection();
        self.0.handler.emit(&actual);
    }

    /// Called with the selected rows (ascending) when the selection changes.
    ///
    /// With multiple selection it may be called with none.
    pub fn on_select(&self, f: impl FnMut(&[usize]) + 'static) {
        self.0.handler.set(f);
    }

    /// Called with the row's index when a row is activated (double-clicked, or Enter
    /// while the row has focus).
    ///
    /// To open everything in a multiple selection, read [`List::selection`] inside it.
    /// Not called for rows of `ListItem`s that cannot be selected (`enabled(false)`).
    pub fn on_row_activate(&self, f: impl FnMut(usize) + 'static) {
        self.0.row_activate.set(f);
    }

    /// Called with the row's index and position when a row is right-clicked (including
    /// touch long-press, Shift + F10 and the application key).
    ///
    /// The position is in logical pixels from the top left of this list and can be
    /// passed straight to [`crate::PopupMenu::open_at`]. For key operations it is the
    /// row's left edge (vertically centered). The selection does not change. Not called
    /// for empty space without rows.
    pub fn on_context_menu(&self, f: impl FnMut(usize, Point) + 'static) {
        self.0.context_menu.set(f);
    }

    /// The inner `ListView`. Public as a backend-specific escape hatch.
    pub fn native_list_view(&self) -> ListView {
        self.0.list_view.clone()
    }

    fn normalize(&self, indices: &[usize]) -> Vec<usize> {
        self.0
            .mode
            .get()
            .normalize_by(indices, |index| self.is_selectable(index))
    }

    /// Whether the row can be selected.
    fn is_selectable(&self, index: usize) -> bool {
        self.0.rows.is_selectable(index, ListRow::is_selectable)
    }

    /// Remembers the selection and writes it into the built rows (no notification).
    ///
    /// Rows outside the window have no `ListViewItem`, so they cannot be written. When
    /// the window moves and rows are rebuilt, it is written again from the remembered
    /// selection.
    fn write_selection(&self, indices: &[usize]) {
        *self.0.selected.borrow_mut() = indices.to_vec();
        let start = self.0.built_start.get();
        self.without_notifying(|this| {
            for (offset, native) in this.0.native_rows.borrow().iter().enumerate() {
                let _ = native.SetIsSelected(indices.binary_search(&(start + offset)).is_ok());
            }
        });
    }

    /// Reads the selection the user changed. How rows outside the window are treated is
    /// decided here too.
    fn read_native_selection(&self) -> Vec<usize> {
        let window = self.0.window.get();
        let start = self.0.built_start.get();
        let mut picked: Vec<usize> = self
            .0
            .native_rows
            .borrow()
            .iter()
            .enumerate()
            .filter(|(_, native)| native.IsSelected().unwrap_or(false))
            .map(|(offset, _)| start + offset)
            .collect();
        // `ListViewItem` には「選ばせない」指定が無い (`IsEnabled` を落とすと
        // 中のボタンまで効かなくなる) ので、選べない行はここで外し、
        // ネイティブ側へも書き戻す。
        if picked.iter().any(|&index| !self.is_selectable(index)) {
            picked.retain(|&index| self.is_selectable(index));
            let kept = self.0.selected.borrow().clone();
            self.write_selection(&picked);
            *self.0.selected.borrow_mut() = kept;
        }
        // 単一選択では、窓の外の行を残すと 2 行選ばれてしまう。
        let gesture = match self.0.mode.get().is_multiple() {
            true => selection_gesture(),
            false => SelectionGesture::Plain,
        };
        let previous = self.0.selected.borrow().clone();
        // Shift のキー操作は `move_cursor` が受けるので、ここへ来る Shift はクリック。
        // `ListView` の範囲は持っている行の中で決まるので、覚えている起点から引き直す。
        // 高さが行に従うリストはキーも `ListView` に任せていて、全行を持っているので
        // 範囲はそのままで正しい。
        if gesture == SelectionGesture::Extend && self.height_is_limited() {
            // 押した行は `ListView` の範囲の端にある。覚えている行は、スクロールのあと
            // ポインターを動かさずに押すと古いことがあるので、近いほうの端を選ぶ。
            let clicked = picked.first().zip(picked.last()).map(|(&first, &last)| {
                match self.0.pointer_row.get() {
                    Some(row) if row.abs_diff(first) < row.abs_diff(last) => first,
                    _ => last,
                }
            });
            if let Some((anchor, clicked)) = self.0.anchor.get().zip(clicked) {
                let range: Vec<usize> = (anchor.min(clicked)..=anchor.max(clicked)).collect();
                let picked = self.normalize(&range);
                self.write_selection(&picked);
                // 押した行のはずなので、フォーカスが行の外へ出ていても移す。
                self.focus_row_at(clicked);
                return picked;
            }
        }
        let cursor = match gesture {
            // 足し引きした行だけが、前の選択と違う。
            SelectionGesture::Toggle => window.indices().rfind(|index| {
                previous.binary_search(index).is_ok() != picked.binary_search(index).is_ok()
            }),
            _ => picked.last().copied(),
        };
        if let Some(cursor) = cursor {
            self.0.anchor.set(Some(cursor));
            // 全行を持つリストでは `ListView` のフォーカスはずれない。
            if self.height_is_limited() {
                self.follow_with_focus(cursor);
            }
        }
        merge_hidden_selection(window, &previous, picked, gesture)
    }

    /// Moves the focus to the row the selection just changed on, if it is on another row.
    ///
    /// Once the window has moved, `ListView` can leave it where it was, and the next key
    /// would then start from that row.
    fn follow_with_focus(&self, index: usize) {
        if self
            .focused_row()
            .is_some_and(|(focused, _)| focused != index)
        {
            self.focus_row_at(index);
        }
    }

    /// Gives the focus to a built row, as a click would.
    fn focus_row_at(&self, index: usize) {
        let row = index
            .checked_sub(self.0.built_start.get())
            .and_then(|offset| self.0.native_rows.borrow().get(offset).cloned());
        if let Some(row) = row.and_then(|row| row.cast::<Control>().ok()) {
            let _ = row.Focus(FocusState::Pointer);
        }
    }

    /// Operates with notifications from WinUI stopped.
    fn without_notifying<R>(&self, f: impl FnOnce(&Self) -> R) -> R {
        let previous = self.0.silent.replace(true);
        let result = f(self);
        self.0.silent.set(previous);
        result
    }
}

/// Empties the holder of a row that is about to be discarded.
///
/// Custom-content rows place the widget the app holds directly into the holder (`Grid`).
/// Letting go of the holder with it still inside leaves the widget "with a parent", and
/// the next rebuild is rejected with "Element is already the child of another element".
fn release_row_content(native: &ListViewItem) {
    if let Ok(host) = native
        .Content()
        .and_then(|content| content.cast::<XamlGrid>())
    {
        release_host_content(&host);
    }
}

/// Takes the content out of a row's holder.
fn release_host_content(host: &XamlGrid) {
    if let Ok(children) = host.Children() {
        let _ = children.Clear();
    }
}

/// Gives the spacer the height of the rows outside the window.
fn set_spacer_height(spacer: &XamlGrid, height: f64) {
    let _ = spacer.SetHeight(height.max(0.0));
}

impl Drop for ListInner {
    fn drop(&mut self) {
        self.hovered.with_mut(|hovered| *hovered = 0);
    }
}

/// Computes the context menu's position in coordinates from the top left of `list`
/// (the List's outer frame).
///
/// When requested by a pointer, its position; for key operations without a
/// position, the vertical center of the row's left edge.
fn context_point(
    list: &Border,
    row: Option<&UIElement>,
    args: &ContextRequestedEventArgs,
) -> Point {
    let mut at = XamlPoint::default();
    if args.TryGetPosition(list, &mut at).unwrap_or(false) {
        return Point::new(f64::from(at.X), f64::from(at.Y));
    }
    let Some(row) = row else {
        return Point::new(0.0, 0.0);
    };
    let middle = row
        .cast::<FrameworkElement>()
        .and_then(|element| element.ActualHeight())
        .unwrap_or(0.0)
        / 2.0;
    row.TransformToVisual(list)
        .and_then(|transform| {
            transform.TransformPoint(XamlPoint {
                X: 0.0,
                Y: middle as f32,
            })
        })
        .map(|at| Point::new(f64::from(at.X), f64::from(at.Y)))
        .unwrap_or(Point::new(0.0, 0.0))
}

/// Places a row's content in a holder that can pick up clicks.
///
/// The holder's `Background` is transparent, so `PointerPressed` arrives even when
/// clicking empty space without text. Buttons, check boxes and fields handle this
/// event themselves and stop it, so they do not fire together with the row's
/// activation (the same division as the other platforms).
fn row_host(row: &ListRow) -> Result<XamlGrid> {
    // 透明な背景を持てないと余白では押せなくなるが、文字やアイコンの上では
    // 届くので、`transparent_grid` は読めなければ普通の `Grid` にする。
    let host = transparent_grid()?;
    let children = host
        .Children()
        .map_err(|e| to_error("get the row container", e))?;
    children
        .Append(&row_content(row)?)
        .map_err(|e| to_error("add to the row", e))?;

    let activation = row.activation.clone();
    let handler = PointerEventHandler::new(move |_sender, args| {
        // 右クリックは行の操作にしない (macOS の mouseDown と同じ扱い)。
        if !is_primary_press(args.as_ref()) {
            return Ok(());
        }
        activation.emit();
        Ok(())
    });
    let _ = host.PointerPressed(&handler);
    if !row.is_selectable() {
        // 選べない行は、余白を含めて行の全体を入れ物が受け持ち、押下を止める。
        let _ = host.SetPadding(ROW_PADDING);
        if let Err(error) = hold_unselectable_press(&host) {
            release_host_content(&host);
            return Err(error);
        }
    }
    Ok(host)
}

/// Whether `source`, clicked inside the row, is over a control that takes input
/// itself.
///
/// Walking up the parents from `source` to just before the row (`row`), it is true
/// if there is an enabled, focusable `Control` (buttons, check boxes, fields and so
/// on). Things that do not take focus, such as `ScrollViewer` and `ProgressBar`, are
/// treated as part of the row. This matches the division of whether
/// `PointerPressed` is stopped.
fn pressed_on_control(source: &IInspectable, row: &IInspectable) -> bool {
    let mut current = source.cast::<DependencyObject>().ok();
    while let Some(object) = current {
        if crate::keys::same(&object, row) {
            return false;
        }
        if let Ok(control) = object.cast::<Control>() {
            let enabled = control.IsEnabled().unwrap_or(false);
            let tab_stop = control
                .cast::<UIElement>()
                .and_then(|element| element.IsTabStop())
                .unwrap_or(false);
            if enabled && tab_stop {
                return true;
            }
        }
        current = VisualTreeHelper::GetParent(&object).ok();
    }
    false
}

/// Whether the press was the primary button (left click, touch, pen).
fn is_primary_press(args: Option<&PointerRoutedEventArgs>) -> bool {
    let Some(args) = args else {
        return false;
    };
    args.GetCurrentPoint(Option::<&UIElement>::None)
        .and_then(|point| point.Properties())
        .and_then(|properties| properties.IsLeftButtonPressed())
        .unwrap_or(false)
}

/// Creates the content of one row.
///
/// Custom-content rows use the prebuilt widget as is.
fn row_content(row: &ListRow) -> Result<UIElement> {
    match &row.content {
        ListRowContent::Custom(content) => Ok(content.native_element()),
        ListRowContent::Item(item) => {
            Ok(item_content(&item.label, item.detail.as_deref())?.native_element())
        }
    }
}

/// The spacing between the label and the secondary text.
const DETAIL_SPACING: f64 = 2.0;

/// One piece of text placed in a row. With `secondary`, it gets the same look as a
/// `List`'s secondary text.
///
/// For places where naui's `Label` cannot be used (such as table cells, which decide
/// text alignment and wrapping directly). The look's values are taken from
/// [`TextStyle::Caption`] and [`TextColor::Secondary`], so they are not written
/// here.
pub(crate) fn text_block(text: &str, secondary: bool) -> Result<TextBlock> {
    let block = TextBlock::new().map_err(|e| to_error("create the row label", e))?;
    if secondary {
        if let Some(style) = label_style(TextStyle::Caption, TextColor::Secondary) {
            let _ = block.SetStyle(&style);
        }
    }
    block
        .SetText(&HSTRING::from(text))
        .map_err(|e| to_error("set the row label", e))?;
    Ok(block)
}

/// The content of a text-only row.
///
/// The smallness and dimness of the secondary text come from
/// [`TextStyle::Caption`] and [`TextColor::Secondary`], so neither point size nor
/// brush is written here. The main text's color is not set (it inherits from the
/// row's `ListViewItem`).
pub(crate) fn item_content(label: &str, detail: Option<&str>) -> Result<Stack> {
    let content = Stack::new(Orientation::Vertical)?;
    // `StackPanel` そのものを行の幅いっぱいに広げる。`Align::Start` にすると
    // `HorizontalAlignment=Left` で中身の幅まで縮み、中の文字が行からはみ出す。
    content.set_align(Align::Fill);
    content.set_spacing(DETAIL_SPACING);
    content.append(&row_label(label)?);
    if let Some(detail) = detail {
        let sub = row_label(detail)?;
        sub.set_style(TextStyle::Caption);
        sub.set_color(TextColor::Secondary);
        content.append(&sub);
    }
    Ok(content)
}

/// One piece of text placed in a row.
///
/// **It stretches to the full width of the row.** `Label` cuts the end with
/// `TextTrimming="CharacterEllipsis"`, but that works only once the width is
/// decided.
fn row_label(text: &str) -> Result<Label> {
    let label = Label::new(text)?;
    label.set_sizing(Sizing::fill_width());
    Ok(label)
}
