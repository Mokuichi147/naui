//! Tables (WinUI 3).
//!
//! WinUI 3 has no `DataGrid` (it belongs to the Community Toolkit), so it is built
//! on the same `ListView` as lists ([`crate::List`]), **with each row's content made
//! a `Grid`**.
//!
//! | Part | Built from |
//! | --- | --- |
//! | Frame | A two-row `Grid` (headings / body) with background, border and rounded corners |
//! | Headings | A `Grid` with the same `ColumnDefinition`s as the columns + `TextBlock`s |
//! | Body | `ListView`. Rows are `ListViewItem` + `Grid` |
//! | Selection | `ListView`'s standard template (light fill + indicator at the left edge) |
//!
//! Headings and rows align their widths by **being given the same column
//! definitions**. Column widths cannot be changed by dragging (unlike `NSTableView`,
//! WinUI has no standard control for it).
//!
//! Colors and rounded corners are all taken with `{ThemeResource ...}`, so they
//! follow light / dark switching as is. Where theme resources cannot be obtained, it
//! falls back to a plain `Grid` + `ListView`, prioritizing function.
//!
//! Scrolling, the rows outside the window and the keyboard work as in [`crate::List`]: an
//! outer `ScrollViewer` holds the `ListView` and two spacers, the selection and the Shift
//! anchor are row numbers kept here, and in a table that scrolls itself the movement keys
//! and anything with Shift are handled here. Unlike a `List`, a `Table` with an `Auto`
//! height also scrolls itself when its parent gives it no more room than that, so that is
//! what decides it, not the height set.

use std::cell::{Cell, RefCell};
use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;

use naui_core::{
    keeps_row_window, merge_hidden_selection, row_window, Align, Modifiers, Result, RowSource,
    RowWindow, SelectionGesture, SelectionMode, SortOrder, TableColumn, TableRow,
    ROW_WINDOW_OVERSCAN,
};
use naui_winui3::Microsoft::UI::Xaml::Controls::{
    Button, ColumnDefinition, Control, Grid as XamlGrid, ListView, ListViewItem,
    ListViewSelectionMode, RowDefinition, ScrollBarVisibility, ScrollMode, ScrollViewer,
    ScrollViewerViewChangedEventArgs, SelectionChangedEventHandler, StackPanel, TextBlock,
};
use naui_winui3::Microsoft::UI::Xaml::Input::{
    KeyEventHandler, PointerEventHandler, TappedEventHandler,
};
use naui_winui3::Microsoft::UI::Xaml::Markup::XamlReader;
use naui_winui3::Microsoft::UI::Xaml::ResourceDictionary;
use naui_winui3::Microsoft::UI::Xaml::{
    FocusState, FrameworkElement, GridLength, GridUnitType, HorizontalAlignment,
    RoutedEventHandler, Style, TextAlignment, TextWrapping, Thickness, UIElement,
    VerticalAlignment,
};
use windows::Foundation::{Point as XamlPoint, PropertyValue};
use windows::System::VirtualKey;
use windows_core::{IInspectable, Interface, HSTRING};

use crate::layout::ListScrollTarget;
use crate::list::{
    hold_unselectable_press, item_padding, new_row_item, release_row_content, report_row_position,
    text_block, transparent_grid, ActivationHandler, RowMotion, SelectionHandler,
};
use crate::to_error;
use crate::ui_thread::{HandlerCell, UiThreadCell};
use crate::widgets::{impl_widget, Widget};

/// The table's frame: a `Grid` splitting headings and body vertically, which holds
/// the border and rounded corners.
///
/// The padding of rows (`ListViewItem`s). **Aligning left and right** with the
/// headings' `Padding` makes the column positions match. `ListView`'s default
/// padding differs from the headings', so it is written per row.
const ROW_PADDING: Thickness = Thickness {
    Left: 12.0,
    Top: 8.0,
    Right: 12.0,
    Bottom: 8.0,
};

/// The minimum row height. The same as `List` rows.
const ROW_MIN_HEIGHT: f64 = 36.0;

/// The headings' `Padding` is aligned so that it has the same horizontal position as
/// the rows' `Padding` ([`ROW_PADDING`]). Putting `Padding` on the body would shift
/// it from the headings, so that stays 0.
const SURFACE_XAML: &str = r##"<Grid
    xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
    Background="{ThemeResource ControlFillColorDefaultBrush}"
    BorderBrush="{ThemeResource ControlStrokeColorDefaultBrush}"
    BorderThickness="1"
    CornerRadius="{ThemeResource ControlCornerRadius}">
    <Grid.RowDefinitions>
        <RowDefinition Height="Auto"/>
        <RowDefinition Height="*"/>
    </Grid.RowDefinitions>
    <Grid Grid.Row="0" Padding="12,6"
        BorderThickness="0,0,0,1"
        BorderBrush="{ThemeResource ControlStrokeColorDefaultBrush}"/>
    <ListView Grid.Row="1" Background="Transparent" BorderThickness="0" Padding="0"
        HorizontalContentAlignment="Stretch">
        <ListView.ItemContainerTransitions>
            <TransitionCollection/>
        </ListView.ItemContainerTransitions>
        <ListView.ItemsPanel>
            <ItemsPanelTemplate><StackPanel/></ItemsPanelTemplate>
        </ListView.ItemsPanel>
    </ListView>
</Grid>"##;

// 仮想化は RowWindow が担当する。ItemsStackPanel の仮想化・アンカー補正を
// 重ねると、窓の移動に伴う詰め物の高さ変更で表示位置が動いてしまう。
const PLAIN_LIST_XAML: &str = r#"<ListView
    xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
    HorizontalContentAlignment="Stretch" Padding="0">
    <ListView.ItemContainerTransitions>
        <TransitionCollection/>
    </ListView.ItemContainerTransitions>
    <ListView.ItemsPanel>
        <ItemsPanelTemplate><StackPanel/></ItemsPanelTemplate>
    </ListView.ItemsPanel>
</ListView>"#;

/// The button of a sortable heading. With no background or border, it stays
/// clickable as the heading text (WinUI has no control for column headings).
///
/// `HEADER_STYLE_KEY` must be the same string as the `x:Key` of `HEADER_STYLE_XAML`.
const HEADER_STYLE_KEY: &str = "NauiTableHeaderStyle";
const HEADER_STYLE_XAML: &str = r##"<ResourceDictionary
    xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
    xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml">
    <Style x:Key="NauiTableHeaderStyle" TargetType="Button">
        <Setter Property="Background" Value="Transparent"/>
        <Setter Property="BorderThickness" Value="0"/>
        <Setter Property="Padding" Value="0"/>
        <Setter Property="MinWidth" Value="0"/>
        <Setter Property="MinHeight" Value="0"/>
        <Setter Property="HorizontalAlignment" Value="Stretch"/>
        <Setter Property="HorizontalContentAlignment" Value="Stretch"/>
        <Setter Property="UseSystemFocusVisuals" Value="True"/>
    </Style>
</ResourceDictionary>"##;

/// The `Style` applied to heading buttons. `None` (the default look) if it cannot be
/// read.
fn header_style() -> Option<Style> {
    let dictionary = XamlReader::Load(&HSTRING::from(HEADER_STYLE_XAML))
        .and_then(|element| element.cast::<ResourceDictionary>());
    let dictionary = match dictionary {
        Ok(dictionary) => dictionary,
        Err(error) => {
            eprintln!("naui-windows: failed to create the header style: {error}");
            return None;
        }
    };
    PropertyValue::CreateString(&HSTRING::from(HEADER_STYLE_KEY))
        .and_then(|key| dictionary.Lookup(&key))
        .and_then(|style| style.cast::<Style>())
        .ok()
}

/// The character indicating the sort direction. WinUI has no column heading
/// indicator, so it is appended after the heading text.
fn sort_arrow(order: Option<SortOrder>) -> &'static str {
    match order {
        Some(SortOrder::Ascending) => " ▲",
        Some(SortOrder::Descending) => " ▼",
        None => "",
    }
}

/// The handler notified when the sort changes.
///
/// WinRT delegates require `Send + Sync`, so it is placed in a `UiThreadCell` (the
/// same shape as [`SelectionHandler`]).
#[derive(Clone)]
struct SortHandler(HandlerCell<dyn FnMut(usize, SortOrder)>);

impl SortHandler {
    fn new() -> Self {
        Self(Arc::new(UiThreadCell::new(None)))
    }

    fn set(&self, f: impl FnMut(usize, SortOrder) + 'static) {
        self.0.with_mut(|slot| *slot = Some(Box::new(f)));
    }

    fn emit(&self, column: usize, order: SortOrder) {
        let Some(mut f) = self.0.with_mut(|slot| slot.take()) else {
            return;
        };
        f(column, order);
        self.0.with_mut(|slot| {
            if slot.is_none() {
                *slot = Some(f);
            }
        });
    }
}

/// The frame bundling the headings and the body.
struct Surface {
    root: XamlGrid,
    header: XamlGrid,
    list_view: ListView,
}

/// Loads the themed frame. Rebuilds it with a plain `Grid` if it cannot be read.
fn build_surface() -> Result<Surface> {
    match load_surface() {
        Ok(surface) => Ok(surface),
        Err(error) => {
            eprintln!("naui-windows: failed to create the themed table frame: {error}");
            plain_surface()
        }
    }
}

fn load_surface() -> Result<Surface> {
    let root = XamlReader::Load(&HSTRING::from(SURFACE_XAML))
        .and_then(|element| element.cast::<XamlGrid>())
        .map_err(|e| to_error("create the Table frame", e))?;
    let children = root
        .Children()
        .map_err(|e| to_error("get the Table frame", e))?;
    let header = children
        .GetAt(0)
        .and_then(|child| child.cast::<XamlGrid>())
        .map_err(|e| to_error("get the Table header", e))?;
    let list_view = children
        .GetAt(1)
        .and_then(|child| child.cast::<ListView>())
        .map_err(|e| to_error("get the Table body", e))?;
    Ok(Surface {
        root,
        header,
        list_view,
    })
}

fn plain_surface() -> Result<Surface> {
    let root = XamlGrid::new().map_err(|e| to_error("create the Table Grid", e))?;
    let definitions = root
        .RowDefinitions()
        .map_err(|e| to_error("get the Table row definitions", e))?;
    for height in [
        GridLength {
            Value: 1.0,
            GridUnitType: GridUnitType::Auto,
        },
        GridLength {
            Value: 1.0,
            GridUnitType: GridUnitType::Star,
        },
    ] {
        let definition =
            RowDefinition::new().map_err(|e| to_error("create the Table row definition", e))?;
        definition
            .SetHeight(height)
            .map_err(|e| to_error("set the Table row definition", e))?;
        definitions
            .Append(&definition)
            .map_err(|e| to_error("add the Table row definition", e))?;
    }

    let header = XamlGrid::new().map_err(|e| to_error("create the Table header", e))?;
    let _ = header.SetPadding(Thickness {
        Left: 12.0,
        Top: 6.0,
        Right: 12.0,
        Bottom: 6.0,
    });
    let list_view = XamlReader::Load(&HSTRING::from(PLAIN_LIST_XAML))
        .and_then(|element| element.cast::<ListView>())
        .map_err(|e| to_error("create the ListView", e))?;

    let children = root
        .Children()
        .map_err(|e| to_error("get the Table frame", e))?;
    for (row, part) in [
        header.cast::<IInspectable>(),
        list_view.cast::<IInspectable>(),
    ]
    .into_iter()
    .enumerate()
    {
        let part = part.map_err(|e| to_error("convert the Table part", e))?;
        // 行の指定は FrameworkElement、追加は UIElement として渡す。
        let framework = part
            .cast::<FrameworkElement>()
            .map_err(|e| to_error("convert the Table part", e))?;
        XamlGrid::SetRow(&framework, row as i32).map_err(|e| to_error("set the Table row", e))?;
        let element = part
            .cast::<UIElement>()
            .map_err(|e| to_error("convert the Table part", e))?;
        children
            .Append(&element)
            .map_err(|e| to_error("add to the Table frame", e))?;
    }
    Ok(Surface {
        root,
        header,
        list_view,
    })
}

/// Maps to WinUI's text alignment. `Fill` means nothing for text, so it is treated
/// the same as left.
fn text_alignment(align: Align) -> TextAlignment {
    match align {
        Align::Center => TextAlignment::Center,
        Align::End => TextAlignment::Right,
        Align::Start | Align::Fill => TextAlignment::Left,
    }
}

/// Hands out the column definitions as the `Grid`'s `ColumnDefinition`s.
///
/// Giving the same ones to both headings and rows aligns the widths.
fn apply_columns(grid: &XamlGrid, columns: &[TableColumn]) -> Result<()> {
    grid.SetColumnSpacing(12.0)
        .map_err(|e| to_error("set the column spacing", e))?;
    let definitions = grid
        .ColumnDefinitions()
        .map_err(|e| to_error("get the column definitions", e))?;
    definitions
        .Clear()
        .map_err(|e| to_error("clear the column definitions", e))?;
    for column in columns {
        let definition =
            ColumnDefinition::new().map_err(|e| to_error("create the column definition", e))?;
        // 幅の指定が無い列だけで、余りを分け合う。
        let width = match column.width {
            Some(width) => GridLength {
                Value: width,
                GridUnitType: GridUnitType::Pixel,
            },
            None => GridLength {
                Value: 1.0,
                GridUnitType: GridUnitType::Star,
            },
        };
        definition
            .SetWidth(width)
            .map_err(|e| to_error("set the column width", e))?;
        definitions
            .Append(&definition)
            .map_err(|e| to_error("add the column definition", e))?;
    }
    Ok(())
}

/// Places one cell's `TextBlock` at the column's position.
fn append_cell(
    grid: &XamlGrid,
    index: usize,
    text: &str,
    align: Align,
    secondary: bool,
) -> Result<()> {
    let block = text_block(text, secondary)?;
    let _ = block.SetTextAlignment(text_alignment(align));
    // 列より長い文字は折り返さず、列の幅で切る。
    let _ = block.SetTextWrapping(TextWrapping::NoWrap);
    let framework = block
        .cast::<FrameworkElement>()
        .map_err(|e| to_error("convert the cell to FrameworkElement", e))?;
    framework
        .SetVerticalAlignment(VerticalAlignment::Center)
        .map_err(|e| to_error("set the cell's vertical alignment", e))?;
    // 行ボックスの中央より字面がわずかに下に見えるため、文字だけを
    // 1 DIP 上へ光学補正する。上下の合計は 0 で、行の高さは変えない。
    if !secondary {
        framework
            .SetMargin(Thickness {
                Left: 0.0,
                Top: -1.0,
                Right: 0.0,
                Bottom: 1.0,
            })
            .map_err(|e| to_error("adjust the cell text position", e))?;
    }
    XamlGrid::SetColumn(&framework, index as i32)
        .map_err(|e| to_error("set the cell column", e))?;
    let element = block
        .cast::<UIElement>()
        .map_err(|e| to_error("convert the cell to FrameworkElement", e))?;
    grid.Children()
        .and_then(|children| children.Append(&element))
        .map_err(|e| to_error("add the cell", e))?;
    Ok(())
}

/// The content of one cell.
enum CellContent {
    Text(String),
    Widget(Box<dyn Widget>),
}

impl Clone for CellContent {
    fn clone(&self) -> Self {
        match self {
            Self::Text(text) => Self::Text(text.clone()),
            Self::Widget(content) => Self::Widget(content.boxed_clone()),
        }
    }
}

/// The contents of one row to put in a table.
///
/// [`TableRow`] is a convenience API for tables that need only strings. To put
/// buttons, check boxes or icons in cells, build the contents with `Grid` / `Stack`
/// and line them up in this type. Rows are returned from
/// [`Table::set_row_builder`].
///
/// ```no_run
/// # use naui_windows::{Table, TableCells};
/// # fn fill(table: &Table, cities: Vec<String>) {
/// table.set_row_builder(cities.len(), move |index| {
///     Ok(TableCells::new().text(&cities[index]).text("13,960,000"))
/// });
/// # }
/// ```
pub struct TableCells {
    cells: Vec<CellContent>,
    selectable: bool,
    /// For text-only rows whose `enabled` is `false`. Makes the whole row inoperable.
    dimmed: bool,
    activation: ActivationHandler,
}

impl Clone for TableCells {
    fn clone(&self) -> Self {
        Self {
            cells: self.cells.clone(),
            selectable: self.selectable,
            dimmed: self.dimmed,
            activation: self.activation.clone(),
        }
    }
}

impl Default for TableCells {
    fn default() -> Self {
        Self::new()
    }
}

impl TableCells {
    /// Creates a row with no cells.
    pub fn new() -> Self {
        Self {
            cells: Vec::new(),
            selectable: true,
            dimmed: false,
            activation: ActivationHandler::new(),
        }
    }

    /// Adds one text cell. Alignment follows the column's setting.
    pub fn text(mut self, text: impl Into<String>) -> Self {
        self.cells.push(CellContent::Text(text.into()));
        self
    }

    /// Adds one widget cell.
    pub fn cell(mut self, content: &dyn Widget) -> Self {
        self.cells.push(CellContent::Widget(content.boxed_clone()));
        self
    }

    /// Whether the whole row can be selected (it can by default).
    pub fn selectable(mut self, selectable: bool) -> Self {
        self.selectable = selectable;
        self
    }

    pub fn is_selectable(&self) -> bool {
        self.selectable
    }

    /// The number of columns.
    pub fn len(&self) -> usize {
        self.cells.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    /// What to do when a cell or empty space in the row is clicked.
    ///
    /// When a button or text field is clicked directly, that control receives the
    /// click, so this is not called.
    pub fn on_activate(&self, f: impl FnMut() + 'static) {
        self.activation.set(f);
    }

    /// Creates one from a text-only row. `enabled` becomes "whether it can be
    /// selected" as is.
    fn from_row(row: &TableRow) -> Self {
        Self {
            cells: row.cells.iter().cloned().map(CellContent::Text).collect(),
            selectable: row.enabled,
            dimmed: !row.enabled,
            activation: ActivationHandler::new(),
        }
    }

    fn content(&self, index: usize) -> Option<&CellContent> {
        self.cells.get(index)
    }
}

/// The `Grid` holding one row's cells.
///
/// It is transparent, not empty, so a press between the cells reaches it.
///
/// A row that cannot be selected takes over the padding its `ListViewItem` gives up,
/// and holds the press over the whole row (see [`hold_unselectable_press`]). Otherwise
/// a click in its margins or between cells would select it for an instant.
fn row_grid(selectable: bool) -> Result<XamlGrid> {
    let grid = transparent_grid()?;
    if !selectable {
        let _ = grid.SetPadding(ROW_PADDING);
        hold_unselectable_press(&grid)?;
    }
    Ok(grid)
}

/// Places a widget cell at the column's position.
fn append_widget_cell(
    grid: &XamlGrid,
    index: usize,
    content: &dyn Widget,
    align: Align,
) -> Result<()> {
    let element = content.native_element();
    let framework = element
        .cast::<FrameworkElement>()
        .map_err(|e| to_error("convert the cell to FrameworkElement", e))?;
    // 列の中のどこへ置くかは列の指定に従う。`Fill` は列いっぱいに広げる。
    let _ = framework.SetHorizontalAlignment(match align {
        Align::Center => HorizontalAlignment::Center,
        Align::End => HorizontalAlignment::Right,
        Align::Fill => HorizontalAlignment::Stretch,
        Align::Start => HorizontalAlignment::Left,
    });
    let _ = framework.SetVerticalAlignment(VerticalAlignment::Center);
    XamlGrid::SetColumn(&framework, index as i32)
        .map_err(|e| to_error("set the cell column", e))?;
    grid.Children()
        .and_then(|children| children.Append(&element))
        .map_err(|e| to_error("add the cell", e))?;
    Ok(())
}

/// Places a row's cells in its `Grid`, in the order of the columns.
fn fill_row(grid: &XamlGrid, cells: &TableCells, columns: &[TableColumn]) -> Result<()> {
    apply_columns(grid, columns)?;
    for (index, column) in columns.iter().enumerate() {
        match cells.content(index) {
            Some(CellContent::Widget(widget)) => {
                append_widget_cell(grid, index, &**widget, column.align)?;
            }
            Some(CellContent::Text(text)) => {
                append_cell(grid, index, text, column.align, false)?;
            }
            // 列より短い行は、足りない分が空のセルになる。
            None => append_cell(grid, index, "", column.align, false)?,
        }
    }
    Ok(())
}

/// Infers how the selection is being changed from the modifier keys held now.
///
/// `SelectionChanged` does not carry modifier keys, so the key state is read on the
/// spot. `GetKeyState` returns the state as far as the thread has processed, so it
/// is the state at the same moment as the click or key operation that caused the
/// selection.
pub(crate) fn selection_gesture() -> SelectionGesture {
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_CONTROL, VK_SHIFT};

    // 最上位のビットが立っていれば、そのキーは押されている。
    let down = |key: windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY| -> bool {
        let state = unsafe { GetKeyState(i32::from(key.0)) };
        state < 0
    };
    if down(VK_SHIFT) {
        // Shift は起点からの範囲。選び直しなので、窓の外の選択は落とす。
        return SelectionGesture::Extend;
    }
    if down(VK_CONTROL) {
        return SelectionGesture::Toggle;
    }
    SelectionGesture::Plain
}

/// Gives the spacer the height of the rows outside the window.
fn set_spacer_height(spacer: &XamlGrid, height: f64) {
    let _ = spacer.SetHeight(height.max(0.0));
}

struct TableInner {
    native: XamlGrid,
    header: XamlGrid,
    list_view: ListView,
    /// The registration with the wheel helper. It cannot be decided until the
    /// template's `ScrollViewer` appears, so it is made after `Loaded`.
    wheel: RefCell<Option<Rc<ListScrollTarget>>>,
    /// The built rows themselves, in order from `built_start`.
    row_items: RefCell<Vec<ListViewItem>>,
    /// The contents of the built rows, in the same order as `row_items`.
    realized: RefCell<Vec<TableCells>>,
    columns: RefCell<Vec<TableColumn>>,
    /// Where the rows come from.
    rows: RowSource<TableCells>,
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
    /// [`Table::set_row_height`].
    row_height: Cell<f64>,
    /// The row height decided by the app. If absent, it is measured from a built row.
    fixed_row_height: Cell<Option<f64>>,
    /// A frame filling in for the rows outside the window (placed before and after the
    /// row list).
    top_spacer: XamlGrid,
    bottom_spacer: XamlGrid,
    /// The scroll area bundling the row list and the spacers before and after it.
    scroll: ScrollViewer,
    /// What `scroll` scrolls. Row positions are measured from here.
    body: StackPanel,
    mode: Cell<SelectionMode>,
    handler: SelectionHandler,
    /// The current sort (column and direction).
    sort: Cell<Option<(usize, SortOrder)>>,
    sort_handler: SortHandler,
    /// The heading buttons. Only sortable columns have one. Used to rewrite the
    /// indicator.
    header_buttons: RefCell<Vec<Option<Button>>>,
    /// The look applied to heading buttons. `None` only when it could not be read.
    header_style: Option<Style>,
    /// Stops notifications only while the selection is being changed from code, since
    /// rewriting `IsSelected` also fires `SelectionChanged`.
    silent: Rc<Cell<bool>>,
    /// State for the window-wide wheel helper to choose this table's ScrollViewer.
    hovered: Arc<UiThreadCell<usize>>,
}

/// A table with column headings (Grid + ListBox).
///
/// Its height is decided by its contents, so to fix it regardless of the number of
/// rows, specify it with `set_sizing`.
#[derive(Clone)]
pub struct Table(Rc<TableInner>);
impl_widget!(Table, native);

impl Table {
    pub(crate) fn new() -> Result<Self> {
        let surface = build_surface()?;
        surface
            .list_view
            .SetSelectionMode(ListViewSelectionMode::Single)
            .map_err(|e| to_error("set the ListView selection mode", e))?;
        // 横スクロールは持たせない (列は見出しと同じ幅で並べる)。
        let _ = ScrollViewer::SetHorizontalScrollBarVisibility2(
            &surface.list_view,
            ScrollBarVisibility::Disabled,
        );

        let hovered = Arc::new(UiThreadCell::new(0));

        let top_spacer = XamlGrid::new().map_err(|e| to_error("create the Table spacer", e))?;
        let bottom_spacer = XamlGrid::new().map_err(|e| to_error("create the Table spacer", e))?;
        // ListView 内の Header/Footer で巨大な空白を動かすと、行が存在しても
        // 内部の表示領域のクリップが追従せず描画されないことがある。
        // スクロールと詰め物は外側が持ち、ListView は実体化した行だけを測る。
        ScrollViewer::SetVerticalScrollBarVisibility2(
            &surface.list_view,
            ScrollBarVisibility::Disabled,
        )
        .map_err(|e| to_error("disable the row list scroll bar", e))?;
        ScrollViewer::SetVerticalScrollMode2(&surface.list_view, ScrollMode::Disabled)
            .map_err(|e| to_error("disable the row list scrolling", e))?;
        let body = StackPanel::new().map_err(|e| to_error("create the Table body", e))?;
        let root_children = surface
            .root
            .Children()
            .map_err(|e| to_error("get the frame", e))?;
        root_children
            .RemoveAt(1)
            .map_err(|e| to_error("move the row list", e))?;
        let body_children = body.Children().map_err(|e| to_error("get the body", e))?;
        for element in [
            top_spacer.cast::<UIElement>(),
            surface.list_view.cast::<UIElement>(),
            bottom_spacer.cast::<UIElement>(),
        ] {
            body_children
                .Append(&element.map_err(|e| to_error("convert the body to UIElement", e))?)
                .map_err(|e| to_error("add to the body", e))?;
        }
        let scroll =
            ScrollViewer::new().map_err(|e| to_error("create the table scroll area", e))?;
        scroll
            .SetHorizontalScrollBarVisibility(ScrollBarVisibility::Disabled)
            .map_err(|e| to_error("disable horizontal scrolling", e))?;
        scroll
            .SetContent(&body)
            .map_err(|e| to_error("set the table scroll content", e))?;
        XamlGrid::SetRow(&scroll, 1).map_err(|e| to_error("place the table body", e))?;
        root_children
            .Append(&scroll)
            .map_err(|e| to_error("add the table body", e))?;

        let this = Self(Rc::new(TableInner {
            native: surface.root,
            header: surface.header,
            list_view: surface.list_view,
            wheel: RefCell::new(None),
            row_items: RefCell::new(Vec::new()),
            realized: RefCell::new(Vec::new()),
            columns: RefCell::new(Vec::new()),
            rows: RowSource::default(),
            window: Cell::new(RowWindow::default()),
            built_start: Cell::new(0),
            failed_window: Cell::new(None),
            rebuilding: Cell::new(false),
            selected: RefCell::new(Vec::new()),
            anchor: Cell::new(None),
            pointer_row: Cell::new(None),
            row_height: Cell::new(0.0),
            fixed_row_height: Cell::new(None),
            top_spacer,
            bottom_spacer,
            scroll,
            body,
            mode: Cell::new(SelectionMode::Single),
            handler: SelectionHandler::new(),
            sort: Cell::new(None),
            sort_handler: SortHandler::new(),
            header_buttons: RefCell::new(Vec::new()),
            header_style: header_style(),
            silent: Rc::new(Cell::new(false)),
            hovered,
        }));

        // ハンドルを強く持つと購読との間で循環するため、弱参照にする。
        //
        // 通知を受けた側が `set_selection` などで選択を書き換えると、その場で
        // `SelectionChanged` がもう一度起きる。`with_mut` では二重借用の panic が
        // WinRT の境界を越えてクラッシュになるため、再入を取りこぼしとして
        // 扱える `try_with_mut` を使う (List・Tree と同じ)。
        let state = UiThreadCell::new(Rc::downgrade(&this.0));
        let handler = SelectionChangedEventHandler::new(move |_sender, _args| {
            let _ = state.try_with_mut(|weak| {
                if let Some(inner) = weak.upgrade() {
                    let table = Table(inner);
                    if !table.0.silent.get() {
                        let indices = table.read_native_selection();
                        *table.0.selected.borrow_mut() = indices.clone();
                        table.0.handler.emit(&indices);
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
        this.install_layout_updates()?;
        this.install_scroll_updates()?;

        let state = UiThreadCell::new(Rc::downgrade(&this.0));
        let got_focus = RoutedEventHandler::new(move |_, _| {
            let _ = state.try_with_mut(|weak| {
                if let Some(inner) = weak.upgrade() {
                    Table(inner).keep_focus_on_screen();
                }
            });
            Ok(())
        });
        this.0
            .list_view
            .GotFocus(&got_focus)
            .map_err(|e| to_error("subscribe to Table focus changes", e))?;

        // ホイールの扱いはリストと同じ。ポインターがこの表の上にある間だけ、
        // ウィンドウ全体のホイール補助が表の ScrollViewer を選ぶ。
        let entered_state = this.0.hovered.clone();
        let entered = PointerEventHandler::new(move |_, _| {
            entered_state.with_mut(|hovered| *hovered = hovered.saturating_add(1));
            Ok(())
        });
        let exited_state = this.0.hovered.clone();
        let exited = PointerEventHandler::new(move |_, _| {
            exited_state.with_mut(|hovered| {
                if *hovered > 0 {
                    *hovered -= 1;
                }
            });
            Ok(())
        });
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
            .PointerEntered(&entered)
            .map_err(|e| to_error("subscribe to Table pointer events", e))?;
        this.0
            .native
            .PointerExited(&exited)
            .map_err(|e| to_error("subscribe to Table pointer events", e))?;
        this.0
            .native
            .PointerMoved(&moved)
            .map_err(|e| to_error("subscribe to Table pointer events", e))?;
        Ok(this)
    }

    /// Rebuilds the columns. Rows and selection stay; only the cells are rearranged.
    ///
    /// The sort setting also stays if that column can still be sorted.
    pub fn set_columns(&self, columns: &[TableColumn]) {
        // 行を組み直すと `IsSelected` が落ちるので、選択は覚えて書き戻す。
        let picked = self.selection();
        let sort = self
            .0
            .sort
            .get()
            .filter(|&(column, _)| columns.get(column).is_some_and(|spec| spec.sortable));
        self.0.sort.set(sort);
        *self.0.columns.borrow_mut() = columns.to_vec();
        let _ = self.rebuild_header();
        // セルの数と揃えが変わるので、行も組み直す。
        self.build_window();
        self.write_selection(&picked);
    }

    /// The number of columns.
    pub fn column_count(&self) -> usize {
        self.0.columns.borrow().len()
    }

    /// Rebuilds the rows. Indices change meaning, so the selection is cleared.
    ///
    /// With many rows, `ListViewItem`s are created only for the rows on screen (the
    /// spacers before and after carry the height of the rest, so the scroll bar length
    /// still reflects all the rows).
    pub fn set_rows(&self, rows: &[TableRow]) {
        self.0
            .rows
            .set_rows(rows.iter().map(TableCells::from_row).collect());
        self.reset_rows();
    }

    /// Passes rows whose cells can hold widgets, in a form that **builds each row when
    /// it becomes visible**.
    ///
    /// `count` is the number of rows, and `build` receives an index from 0 to
    /// `count - 1` and returns that row's contents ([`TableCells`]). It is called only
    /// for the rows on screen (and a few before and after), so opening stays fast even
    /// with hundreds of thousands of rows.
    ///
    /// ```no_run
    /// # use naui_windows::{Table, TableCells};
    /// # fn fill(table: &Table, cities: Vec<String>) {
    /// table.set_row_builder(cities.len(), move |index| {
    ///     Ok(TableCells::new().text(&cities[index]))
    /// });
    /// # }
    /// ```
    pub fn set_row_builder(
        &self,
        count: usize,
        build: impl FnMut(usize) -> Result<TableCells> + 'static,
    ) {
        self.0.rows.set_builder(count, build);
        self.reset_rows();
    }

    /// Passes a function that answers whether a row can be selected **without building
    /// the row**.
    ///
    /// For rows built with [`Table::set_row_builder`], naui does not know whether rows
    /// not yet built can be selected. With this function, [`Table::set_selection`] can
    /// keep its promise of "removing rows that cannot be selected" even for rows off
    /// screen. Without it, rows follow the [`TableCells::selectable`] they were last built
    /// with, even after leaving the screen, and rows not built since the rows were passed
    /// or refreshed are treated as selectable.
    ///
    /// Its answers must match the [`TableCells::selectable`] returned by `build`. It stays
    /// when the rows are replaced, so pass a new one if its answers change.
    ///
    /// ```no_run
    /// # use naui_windows::{Table, TableCells};
    /// # fn fill(table: &Table, done: std::rc::Rc<Vec<bool>>) {
    /// let rows = done.clone();
    /// table.set_row_selectable(move |index| !rows[index]);
    /// # }
    /// ```
    pub fn set_row_selectable(&self, selectable: impl Fn(usize) -> bool + 'static) {
        self.0.rows.set_selectable(selectable);
    }

    /// Rebuilds the visible rows. The row count and selection stay.
    pub fn refresh(&self) {
        self.0.rows.forget_selectable();
        // 行を組み立てる関数の中から呼ばれたら、組み立てている側が最初からやり直す。
        if self.0.rebuilding.get() {
            self.0.rows.invalidate();
            return;
        }
        let picked = self.selection();
        self.build_window();
        self.write_selection(&picked);
        // 中身が変わって行の高さが動いていれば、窓を引き直す。
        if self.measure_row_height() {
            self.update_window();
        }
    }

    /// Sets the row height in logical pixels. Passing 0 or less measures it from a
    /// built row.
    ///
    /// Every row gets the same height. Rows off screen are filled in as "row count ×
    /// this height", so if row heights are not uniform the scroll bar length will be
    /// slightly off from reality.
    pub fn set_row_height(&self, height: f64) {
        self.0
            .fixed_row_height
            .set((height > 0.0).then_some(height));
        // 指定を外したときは、組み立て直した行から測り直す。
        self.0.row_height.set(height.max(0.0));
        self.refresh();
    }

    /// The row height (logical pixels). 0 if there are no rows or none has been built yet.
    pub fn row_height(&self) -> f64 {
        if self.is_empty() && self.0.fixed_row_height.get().is_none() {
            return 0.0;
        }
        self.0.row_height.get()
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
    /// Shift).
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
    ///
    /// Even while only some rows are built, the position is computed from the row
    /// height, so it can scroll to rows not yet built (the destination row is built on
    /// the spot).
    pub fn scroll_to_row(&self, index: usize) {
        if index >= self.len() {
            return;
        }
        // 組み立てたばかりの行にも位置が付くように、レイアウトを先に済ませる。
        let _ = self.0.scroll.UpdateLayout();
        self.measure_row_height();
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
    /// selection only the first remains ([`SelectionMode::normalize_by`]).
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

    /// Called with the clicked column and direction when a heading is clicked and the
    /// sort setting changes.
    ///
    /// Only columns with [`TableColumn::sortable`] set can be clicked. **Sorting the
    /// rows is the app's job**: when notified, pass the sorted rows again with
    /// [`Table::set_rows`] (`set_rows` clears the selection, so reselect if needed).
    pub fn on_sort(&self, f: impl FnMut(usize, SortOrder) + 'static) {
        self.0.sort_handler.set(f);
    }

    /// The current sort setting (column and direction). `None` if no heading has been
    /// clicked.
    pub fn sort(&self) -> Option<(usize, SortOrder)> {
        self.0.sort.get()
    }

    /// Replaces the sort setting without notifying. Only the heading's indicator
    /// changes.
    ///
    /// Pointing at a column that cannot be sorted, or one out of range, clears the
    /// setting.
    pub fn set_sort(&self, sort: Option<(usize, SortOrder)>) {
        let sort = sort.filter(|&(column, _)| {
            self.0
                .columns
                .borrow()
                .get(column)
                .is_some_and(|spec| spec.sortable)
        });
        self.0.sort.set(sort);
        self.apply_sort();
    }

    /// The inner `ListBox`. Public as a backend-specific escape hatch.
    pub fn native_list_view(&self) -> ListView {
        self.0.list_view.clone()
    }

    // ------------------------------------------------------------ 組み立て

    /// Rebuilds the headings from the current column definitions.
    fn rebuild_header(&self) -> Result<()> {
        let columns = self.0.columns.borrow();
        let children = self
            .0
            .header
            .Children()
            .map_err(|e| to_error("get the headers", e))?;
        children
            .Clear()
            .map_err(|e| to_error("clear the headers", e))?;
        apply_columns(&self.0.header, &columns)?;

        let mut buttons = Vec::with_capacity(columns.len());
        for (index, column) in columns.iter().enumerate() {
            if !column.sortable {
                append_cell(&self.0.header, index, &column.title, column.align, true)?;
                buttons.push(None);
                continue;
            }
            // 並べ替えられる列は、見出しそのものをボタンにする。
            let button = Button::new().map_err(|e| to_error("create the header button", e))?;
            if let Some(style) = self.0.header_style.as_ref() {
                let _ = button.SetStyle(style);
            }
            let label = text_block(&column.title, true)?;
            let _ = label.SetTextAlignment(text_alignment(column.align));
            let _ = label.SetTextWrapping(TextWrapping::NoWrap);
            button
                .SetContent(&label)
                .map_err(|e| to_error("set the header content", e))?;

            let state = UiThreadCell::new(Rc::downgrade(&self.0));
            let click = RoutedEventHandler::new(move |_, _| {
                state.with_mut(|weak| {
                    if let Some(inner) = weak.upgrade() {
                        Table(inner).on_header_activated(index);
                    }
                });
                Ok(())
            });
            button
                .Click(&click)
                .map_err(|e| to_error("subscribe to the header", e))?;

            let framework = button
                .cast::<FrameworkElement>()
                .map_err(|e| to_error("convert the header to UIElement", e))?;
            XamlGrid::SetColumn(&framework, index as i32)
                .map_err(|e| to_error("set the header column", e))?;
            let element = button
                .cast::<UIElement>()
                .map_err(|e| to_error("convert the header to UIElement", e))?;
            children
                .Append(&element)
                .map_err(|e| to_error("add the header", e))?;
            buttons.push(Some(button));
        }
        *self.0.header_buttons.borrow_mut() = buttons;
        drop(columns);
        self.apply_sort();
        Ok(())
    }

    /// When a heading is clicked. The same column reverses direction; a different
    /// column starts ascending.
    fn on_header_activated(&self, index: usize) {
        let next = match self.0.sort.get() {
            Some((column, order)) if column == index => (index, order.reversed()),
            _ => (index, SortOrder::Ascending),
        };
        self.0.sort.set(Some(next));
        self.apply_sort();
        self.0.sort_handler.emit(next.0, next.1);
    }

    /// Writes the sort setting into the headings.
    fn apply_sort(&self) {
        let sort = self.0.sort.get();
        let columns = self.0.columns.borrow();
        for (index, button) in self.0.header_buttons.borrow().iter().enumerate() {
            let Some(button) = button else {
                continue;
            };
            let order = sort.filter(|&(column, _)| column == index).map(|(_, o)| o);
            let title = columns.get(index).map(|c| c.title.as_str()).unwrap_or("");
            let text = format!("{title}{}", sort_arrow(order));
            if let Ok(label) = button
                .Content()
                .and_then(|content| content.cast::<TextBlock>())
            {
                let _ = label.SetText(&HSTRING::from(text));
            }
        }
    }

    /// Registers the table's `ScrollViewer` as a destination of the wheel helper.
    ///
    /// After the display size is decided on `Loaded`, the visible range is updated too.
    fn install_wheel_target(&self) -> Result<()> {
        let state = UiThreadCell::new(Rc::downgrade(&self.0));
        let loaded = RoutedEventHandler::new(move |_, _| {
            let _ = state.try_with_mut(|weak| {
                if let Some(inner) = weak.upgrade() {
                    Table(inner).register_wheel_target();
                }
            });
            Ok(())
        });
        self.0
            .list_view
            .Loaded(&loaded)
            .map_err(|e| to_error("subscribe to Table Loaded", e))?;
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

    // ------------------------------------------------------- 行を絞る窓

    /// Gets row swaps in before drawing. With `ViewChanged` alone, empty spacers would
    /// be visible for a moment after scrolling, so it also updates when layout
    /// completes.
    fn install_layout_updates(&self) -> Result<()> {
        let state = UiThreadCell::new(Rc::downgrade(&self.0));
        let handler = windows::Foundation::EventHandler::<IInspectable>::new(move |_, _| {
            let _ = state.try_with_mut(|weak| {
                if let Some(inner) = weak.upgrade() {
                    Table(inner).update_window();
                }
            });
            Ok(())
        });
        self.0
            .list_view
            .LayoutUpdated(&handler)
            .map_err(|e| to_error("subscribe to Table layout updates", e))?;
        Ok(())
    }

    /// Moves the built range along with scrolling.
    ///
    /// `ViewChanged` also arrives for intermediate progress (dragging the thumb,
    /// inertia), so the rows follow while scrolling. With few rows the window covers
    /// every row, so nothing is rebuilt.
    fn install_scroll_updates(&self) -> Result<()> {
        let state = UiThreadCell::new(Rc::downgrade(&self.0));
        let handler = windows::Foundation::EventHandler::<ScrollViewerViewChangedEventArgs>::new(
            move |_, _| {
                let _ = state.try_with_mut(|weak| {
                    if let Some(inner) = weak.upgrade() {
                        Table(inner).update_window();
                    }
                });
                Ok(())
            },
        );
        self.0
            .scroll
            .ViewChanged(&handler)
            .map_err(|e| to_error("subscribe to Table scroll position changes", e))?;
        Ok(())
    }

    /// Rebuilds the rows and moves the window back to the start. The selection is
    /// cleared too.
    fn reset_rows(&self) {
        self.0.selected.borrow_mut().clear();
        self.0.anchor.set(None);
        // 組み立ての途中で差し替えられたら、組み立てている側が新しい行でやり直す。
        if self.0.rebuilding.get() {
            return;
        }
        self.0.window.set(self.compute_window(ROW_WINDOW_OVERSCAN));
        self.build_window();
        if self.measure_row_height() {
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
                eprintln!("naui-windows: failed to build the table rows: {error}");
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
        let built = std::mem::take(&mut *self.0.row_items.borrow_mut());
        let built_rows = std::mem::take(&mut *self.0.realized.borrow_mut());
        let built_start = self.0.built_start.get();
        // 窓に残る行は触らない。作り直すと、押している最中の行ならクリックが失われ、
        // 中のコントロールのフォーカスも失われる。前回の組み立てが途中で終わっていたら、
        // 位置が合わないので全部作り直す。
        let kept = previous
            .filter(|previous| previous.len() == built.len())
            .map_or(0..0, |previous| previous.overlap(window));
        // 残る行が無いときは、取り除かずに置き換える (上のコメントのとおり)。
        let replaceable = match kept.is_empty() {
            true => previous_count,
            false => 0,
        };
        // 組み立ての中でアプリのコードが動くので、列の借用は持ち越さない。
        let columns = self.0.columns.borrow().clone();
        let mut row_items = Vec::with_capacity(window.len());
        let mut realized = Vec::with_capacity(window.len());
        // 失敗しても `ListView` に残っている行は、管理から外さない。外すと、次の組み立てで
        // 中身を取り出せず、アプリが持っているウィジェットを載せ直せなくなる。窓は実際に
        // 組み立てた行まで縮めるので、行の番号はずれない。
        let mut outcome = Ok(());
        for (offset, (item, cells)) in built.into_iter().zip(built_rows).enumerate() {
            if kept.contains(&(built_start + offset)) {
                row_items.push(item);
                realized.push(cells);
                continue;
            }
            release_row_content(&item);
            if kept.is_empty() {
                continue;
            }
            let at = row_items.len() as u32;
            if let Err(error) = self.without_notifying(|_| children.RemoveAt(at)) {
                outcome = Err(to_error("remove the row", error));
                row_items.push(item);
                realized.push(cells);
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
            let Some(cells) = self.0.rows.row(index) else {
                continue;
            };
            // 差し替えられたら、この行も残りの窓ももう古い。
            if self.0.rows.generation() != generation {
                break;
            }
            // 組み立てに失敗した行は、列だけそろえた空の行にする。
            let mut cells = cells.unwrap_or_default();
            // 行に載せられないウィジェットが返ったときも同じ。行が欠けると、行の位置と
            // 窓の位置がずれて、選択やフォーカスが別の行へ届く。
            let built = self
                .new_native_row(index, &cells, &columns, window.count)
                .or_else(|error| {
                    eprintln!("naui-windows: failed to build table row {index}: {error}");
                    cells = TableCells::default();
                    self.new_native_row(index, &cells, &columns, window.count)
                });
            self.0.rows.note_selectable(index, cells.is_selectable());
            let item = match built {
                Ok(item) => item,
                Err(error) => {
                    outcome = Err(error);
                    break;
                }
            };
            let before = index < kept.start;
            let slot = match before {
                true => 0,
                false => row_items.len(),
            };
            let placed = item.cast::<IInspectable>().and_then(|element| {
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
                release_row_content(&item);
                outcome = Err(to_error("place the row", error));
                break;
            }
            row_items.insert(slot, item);
            realized.insert(slot, cells);
            first -= usize::from(before);
        }
        for _ in row_items.len() as u32..replaceable {
            if let Err(error) = self.without_notifying(|_| children.RemoveAtEnd()) {
                outcome = Err(to_error("remove the extra rows", error));
                break;
            }
        }
        let built_count = row_items.len();
        *self.0.row_items.borrow_mut() = row_items;
        *self.0.realized.borrow_mut() = realized;
        self.0.built_start.set(first);
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
            .map_err(|e| to_error("update the table layout", e))?;
        // 矢印キーで窓の端まで来たときに組み立て直しても、操作が続けられるように。
        // 残る行でも、押した直後に窓が動くと `ListView` がフォーカスを落とすことがある。
        if let Some((index, state)) = focused {
            if !kept.contains(&index) {
                self.restore_focus(index, state);
            } else if self.focused_row().is_none() {
                self.focus_built_row(index, state);
            }
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
        cells: &TableCells,
        columns: &[TableColumn],
        count: usize,
    ) -> Result<ListViewItem> {
        let selectable = cells.is_selectable();
        let item = new_row_item()?;
        // 見出しと列をそろえるため、余白と高さは行ごとに書く。
        // 選べない行の余白は、中の `Grid` が受け持つ (`row_grid`)。
        let _ = item.SetPadding(item_padding(selectable));
        match self.0.fixed_row_height.get() {
            Some(height) => {
                let _ = item.SetHeight(height);
                let _ = item.SetMinHeight(height);
            }
            None => {
                let _ = item.SetMinHeight(ROW_MIN_HEIGHT);
            }
        }
        let content = row_grid(selectable)?;
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
        let _ = content.PointerPressed(&pressed);
        // 途中で失敗しても、載せ終わったウィジェットには親を残さない。残すと、次の
        // 組み立てで同じウィジェットを載せられなくなる。
        let filled = fill_row(&content, cells, columns).and_then(|()| {
            item.SetContent(&content)
                .map_err(|e| to_error("set the row content", e))
        });
        if let Err(error) = filled {
            if let Ok(children) = content.Children() {
                let _ = children.Clear();
            }
            return Err(error);
        }
        // 文字だけの行で `enabled` が `false` のときは、行ごと操作できない。
        let _ = item.SetIsEnabled(!cells.dimmed);
        report_row_position(&item, index, count);
        if let Err(error) = self.listen_row_events(&item, index) {
            release_row_content(&item);
            return Err(error);
        }
        Ok(item)
    }

    /// Receives clicks and the movement keys on a row.
    ///
    /// A row stays this row for as long as it is built, so the index can be the row's place
    /// in the whole table at creation. Buttons and fields in cells receive the click
    /// themselves and `Tapped` stops there, so it does not double up with the row's
    /// activation. The subscriptions hold the `Table` weakly (avoiding the cycle row →
    /// subscription → `Table` → row).
    fn listen_row_events(&self, item: &ListViewItem, index: usize) -> Result<()> {
        let state = UiThreadCell::new(Rc::downgrade(&self.0));
        let tapped = TappedEventHandler::new(move |_sender, _args| {
            let _ = state.try_with_mut(|weak| {
                if let Some(inner) = weak.upgrade() {
                    Table(inner).activate_row(index);
                }
            });
            Ok(())
        });
        item.Tapped(&tapped)
            .map_err(|e| to_error("subscribe to the row", e))?;

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
                            .is_some_and(|inner| Table(inner).select_every_row())
                    })
                    .unwrap_or(false);
                if selected {
                    let _ = args.SetHandled(true);
                }
                return Ok(());
            }
            let Some(motion) = RowMotion::of_key(key).filter(|_| on_row) else {
                return Ok(());
            };
            let moved = !modifiers.alt
                && !modifiers.meta
                && state
                    .try_with_mut(|weak| {
                        weak.upgrade()
                            .is_some_and(|inner| Table(inner).move_cursor(index, motion, modifiers))
                    })
                    .unwrap_or(false);
            if moved {
                let _ = args.SetHandled(true);
            }
            Ok(())
        });
        item.KeyDown(&keyed)
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
        item.PointerEntered(&entered)
            .map_err(|e| to_error("subscribe to row pointer events", e))?;
        Ok(())
    }

    /// Fires that row's activation.
    fn activate_row(&self, index: usize) {
        let activation = index
            .checked_sub(self.0.built_start.get())
            .and_then(|offset| {
                self.0
                    .realized
                    .borrow()
                    .get(offset)
                    .map(|cells| cells.activation.clone())
            });
        if let Some(activation) = activation {
            activation.emit();
        }
    }

    /// Moves the keyboard cursor from row `from`, as `ListView` would if it had every row.
    /// `false` if `ListView` should do it.
    ///
    /// `ListView` knows only the rows it has, and it takes those as the page because the
    /// scrolling is outside it: Page Down goes straight to the last one even in a short
    /// table. It also keeps the Shift anchor as a place among those rows. So the movements
    /// that depend on either are done here, before it sees the key.
    fn move_cursor(&self, from: usize, motion: RowMotion, modifiers: Modifiers) -> bool {
        let height = self.0.row_height.get();
        let count = self.len();
        let extending = modifiers.shift && self.0.mode.get().is_multiple();
        // スクロールしない表は全行が見えているか、スクロールするのが親なので、任せたままでよい。
        if height <= 0.0 || count == 0 || !self.scrolls_itself() {
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
        // 選択を動かす移動は、選べない行へ止まらない。止まると選択が空になる。Ctrl は
        // フォーカスだけなので、行の中のコントロールへ入れるように止まる。Space はその行の
        // ままなので探さない。
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

    /// Whether this table's own `ScrollViewer` has anywhere to go.
    ///
    /// The height set does not tell: a table with an `Auto` height scrolls itself when its
    /// parent gives it no more room than that, and only then has rows out of view.
    fn scrolls_itself(&self) -> bool {
        self.0.scroll.ScrollableHeight().unwrap_or(0.0) > 0.0
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
        let rows = self.0.row_items.borrow().clone();
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
    /// Focus changes do not scroll a table that scrolls itself (see
    /// [`Table::update_window`]), and `ListView` picks rows without looking at what is on
    /// screen: Tab can land on the first row built. The next key would then pull the view
    /// to that row.
    fn keep_focus_on_screen(&self) {
        let height = self.0.row_height.get();
        let viewport = self.0.scroll.ViewportHeight().unwrap_or(0.0);
        if self.0.rebuilding.get() || !self.scrolls_itself() || height <= 0.0 || viewport <= 0.0 {
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
        let rows = self.0.row_items.borrow();
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
        if let Some(row) = self.0.row_items.borrow().get(index - start) {
            let _ = row.cast::<Control>().and_then(|row| row.Focus(state));
        }
    }

    /// Gives the focus to a built row, wherever it is.
    fn focus_built_row(&self, index: usize, state: FocusState) {
        let row = index
            .checked_sub(self.0.built_start.get())
            .and_then(|offset| self.0.row_items.borrow().get(offset).cloned());
        if let Some(row) = row.and_then(|row| row.cast::<Control>().ok()) {
            let _ = row.Focus(state);
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
        row_window(
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
        // 自分でスクロールする表では、フォーカスの自動スクロールを止める。フォーカスの
        // 行が窓から外れると XAML が画面の外の行へフォーカスを移し、遅れてそこへ飛ぶので、
        // 押下と解放の間だとクリックまで失われる。キーで移るときの位置は `move_cursor` が
        // 決める。スクロールするのが親なら、そのまま任せる。
        let _ = self
            .0
            .scroll
            .SetBringIntoViewOnFocusChange(!self.scrolls_itself());
        // 行の追加直後は ActualHeight が 0 のことがある。窓が同じでも
        // レイアウト後に再計測し、全行分のスクロール領域を確保する。
        self.measure_row_height();
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
        if self.measure_row_height() {
            self.move_window(offset);
        }
    }

    /// Measures the height of one row from the built rows. `true` if it changed.
    fn measure_row_height(&self) -> bool {
        if self.0.fixed_row_height.get().is_some() {
            return false;
        }
        let measured = self
            .0
            .row_items
            .borrow()
            .first()
            .and_then(|item| item.ActualHeight().ok())
            .unwrap_or(0.0);
        if measured <= 0.0 || (self.0.row_height.get() - measured).abs() < 0.5 {
            return false;
        }
        self.0.row_height.set(measured);
        let window = self.0.window.get();
        set_spacer_height(&self.0.top_spacer, window.leading(measured));
        set_spacer_height(&self.0.bottom_spacer, window.trailing(measured));
        true
    }

    /// Where a row is within the scrolled content (top and bottom).
    ///
    /// With every row built, it is the row's real position (heights may differ). While
    /// only some rows are built, it is computed from the row height, so it can point at
    /// rows that do not exist yet.
    fn row_span(&self, index: usize) -> (f64, f64) {
        if self.0.window.get().is_complete() {
            let item = self.0.row_items.borrow().get(index).cloned();
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

    // --------------------------------------------------------------- 選択

    /// Normalizes the requested selection into a form meaningful for this table.
    fn normalize(&self, indices: &[usize]) -> Vec<usize> {
        self.0
            .mode
            .get()
            .normalize_by(indices, |index| self.is_selectable(index))
    }

    /// Whether the row can be selected.
    ///
    /// For built rows, rows not yet built are assumed "selectable" (it cannot be known
    /// without building them).
    fn is_selectable(&self, index: usize) -> bool {
        self.0.rows.is_selectable(index, TableCells::is_selectable)
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
            for (offset, item) in this.0.row_items.borrow().iter().enumerate() {
                let _ = item.SetIsSelected(indices.binary_search(&(start + offset)).is_ok());
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
            .row_items
            .borrow()
            .iter()
            .enumerate()
            .filter(|(_, item)| item.IsSelected().unwrap_or(false))
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
        // スクロールしない表はキーも `ListView` に任せていて、窓が動かないので範囲は
        // そのままで正しい。
        if gesture == SelectionGesture::Extend && self.scrolls_itself() {
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
            // スクロールしない表では `ListView` のフォーカスはずれない。
            if self.scrolls_itself() {
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
        self.focus_built_row(index, FocusState::Pointer);
    }

    /// Operates with notifications from WinUI stopped.
    fn without_notifying<R>(&self, f: impl FnOnce(&Self) -> R) -> R {
        let previous = self.0.silent.replace(true);
        let result = f(self);
        self.0.silent.set(previous);
        result
    }
}

impl Drop for TableInner {
    fn drop(&mut self) {
        self.hovered.with_mut(|hovered| *hovered = 0);
    }
}
