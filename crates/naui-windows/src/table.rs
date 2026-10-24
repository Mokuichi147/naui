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

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use naui_core::{
    keeps_hidden_selection, keeps_row_window, row_window, Align, Result, RowWindow,
    SelectionGesture, SelectionMode, SortOrder, TableColumn, TableRow, ROW_WINDOW_OVERSCAN,
    ROW_WINDOW_THRESHOLD,
};
use naui_winui3::Microsoft::UI::Dispatching::{DispatcherQueue, DispatcherQueueTimer};
use naui_winui3::Microsoft::UI::Xaml::Controls::{
    Button, ColumnDefinition, Grid as XamlGrid, ListView, ListViewItem, ListViewSelectionMode,
    RowDefinition, ScrollBarVisibility, ScrollMode, ScrollViewer, SelectionChangedEventHandler,
    StackPanel, TextBlock,
};
use naui_winui3::Microsoft::UI::Xaml::Input::{PointerEventHandler, TappedEventHandler};
use naui_winui3::Microsoft::UI::Xaml::Markup::XamlReader;
use naui_winui3::Microsoft::UI::Xaml::ResourceDictionary;
use naui_winui3::Microsoft::UI::Xaml::{
    FrameworkElement, GridLength, GridUnitType, HorizontalAlignment, RoutedEventHandler, Style,
    TextAlignment, TextWrapping, Thickness, UIElement, VerticalAlignment,
};
use windows::Foundation::{PropertyValue, TimeSpan};
use windows_core::{IInspectable, Interface, HSTRING};

use crate::layout::ListScrollTarget;
use crate::list::{text_block, ActivationHandler, SelectionHandler};
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
            eprintln!("naui-windows: 見出しのスタイルの生成に失敗: {error}");
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
            eprintln!("naui-windows: テーブルのテーマ付き枠の生成に失敗: {error}");
            plain_surface()
        }
    }
}

fn load_surface() -> Result<Surface> {
    let root = XamlReader::Load(&HSTRING::from(SURFACE_XAML))
        .and_then(|element| element.cast::<XamlGrid>())
        .map_err(|e| to_error("Table の枠の生成", e))?;
    let children = root
        .Children()
        .map_err(|e| to_error("Table の枠の取得", e))?;
    let header = children
        .GetAt(0)
        .and_then(|child| child.cast::<XamlGrid>())
        .map_err(|e| to_error("Table の見出しの取得", e))?;
    let list_view = children
        .GetAt(1)
        .and_then(|child| child.cast::<ListView>())
        .map_err(|e| to_error("Table の本体の取得", e))?;
    Ok(Surface {
        root,
        header,
        list_view,
    })
}

fn plain_surface() -> Result<Surface> {
    let root = XamlGrid::new().map_err(|e| to_error("Table の Grid の生成", e))?;
    let definitions = root
        .RowDefinitions()
        .map_err(|e| to_error("Table の行定義の取得", e))?;
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
        let definition = RowDefinition::new().map_err(|e| to_error("Table の行定義の生成", e))?;
        definition
            .SetHeight(height)
            .map_err(|e| to_error("Table の行定義の設定", e))?;
        definitions
            .Append(&definition)
            .map_err(|e| to_error("Table の行定義の追加", e))?;
    }

    let header = XamlGrid::new().map_err(|e| to_error("Table の見出しの生成", e))?;
    let _ = header.SetPadding(Thickness {
        Left: 12.0,
        Top: 6.0,
        Right: 12.0,
        Bottom: 6.0,
    });
    let list_view = XamlReader::Load(&HSTRING::from(PLAIN_LIST_XAML))
        .and_then(|element| element.cast::<ListView>())
        .map_err(|e| to_error("ListView の生成", e))?;

    let children = root
        .Children()
        .map_err(|e| to_error("Table の枠の取得", e))?;
    for (row, part) in [
        header.cast::<IInspectable>(),
        list_view.cast::<IInspectable>(),
    ]
    .into_iter()
    .enumerate()
    {
        let part = part.map_err(|e| to_error("Table の要素化", e))?;
        // 行の指定は FrameworkElement、追加は UIElement として渡す。
        let framework = part
            .cast::<FrameworkElement>()
            .map_err(|e| to_error("Table の要素化", e))?;
        XamlGrid::SetRow(&framework, row as i32).map_err(|e| to_error("Table の行の指定", e))?;
        let element = part
            .cast::<UIElement>()
            .map_err(|e| to_error("Table の要素化", e))?;
        children
            .Append(&element)
            .map_err(|e| to_error("Table の枠への追加", e))?;
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
        .map_err(|e| to_error("列間隔の設定", e))?;
    let definitions = grid
        .ColumnDefinitions()
        .map_err(|e| to_error("列定義の取得", e))?;
    definitions
        .Clear()
        .map_err(|e| to_error("列定義の消去", e))?;
    for column in columns {
        let definition = ColumnDefinition::new().map_err(|e| to_error("列定義の生成", e))?;
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
            .map_err(|e| to_error("列幅の設定", e))?;
        definitions
            .Append(&definition)
            .map_err(|e| to_error("列定義の追加", e))?;
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
        .map_err(|e| to_error("セルの要素化", e))?;
    framework
        .SetVerticalAlignment(VerticalAlignment::Center)
        .map_err(|e| to_error("セルの縦揃え", e))?;
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
            .map_err(|e| to_error("セルの文字位置の補正", e))?;
    }
    XamlGrid::SetColumn(&framework, index as i32).map_err(|e| to_error("セルの列の指定", e))?;
    let element = block
        .cast::<UIElement>()
        .map_err(|e| to_error("セルの要素化", e))?;
    grid.Children()
        .and_then(|children| children.Append(&element))
        .map_err(|e| to_error("セルの追加", e))?;
    Ok(())
}

/// The interval (milliseconds) for re-reading the display position.
///
/// `ScrollViewer.ViewChanged` is not in this projection, so while scrolling the
/// display position is read at a short interval. It only reads, so nothing happens
/// if the position does not change.
const SCROLL_POLL_MILLIS: i64 = 50;

/// One millisecond of `TimeSpan` (in 100-nanosecond units).
const TICKS_PER_MILLI: i64 = 10_000;

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

/// The closure that builds rows. Taken out only for the duration of the call.
/// The slot for the closure that builds rows.
type RowBuildCell = Rc<RefCell<Option<Box<dyn FnMut(usize) -> Result<TableCells>>>>>;

/// The slot for the function answering "can this row be selected?"
/// ([`Table::set_row_selectable`]).
type RowSelectableCell = RefCell<Option<Rc<dyn Fn(usize) -> bool>>>;

/// The closure that builds rows.
///
/// It is taken out only for the duration of the call, so operating on the table
/// from inside the build does not double-borrow.
#[derive(Clone, Default)]
struct RowBuilder(RowBuildCell);

impl RowBuilder {
    fn set(&self, f: impl FnMut(usize) -> Result<TableCells> + 'static) {
        *self.0.borrow_mut() = Some(Box::new(f));
    }

    fn clear(&self) {
        *self.0.borrow_mut() = None;
    }

    fn is_set(&self) -> bool {
        self.0.borrow().is_some()
    }

    fn build(&self, index: usize) -> Option<Result<TableCells>> {
        let mut f = self.0.borrow_mut().take()?;
        let cells = f(index);
        let mut slot = self.0.borrow_mut();
        if slot.is_none() {
            *slot = Some(f);
        }
        Some(cells)
    }
}

/// Where rows come from: either text-only rows ([`Table::set_rows`]) or rows built
/// when they become visible ([`Table::set_row_builder`]).
#[derive(Default)]
struct RowsState {
    rows: RefCell<Vec<TableRow>>,
    builder: RowBuilder,
    /// The function that answers "can it be selected?" without building the row
    /// ([`Table::set_row_selectable`]).
    selectable: RowSelectableCell,
    count: Cell<usize>,
}

impl RowsState {
    fn len(&self) -> usize {
        self.count.get()
    }

    fn set_rows(&self, rows: &[TableRow]) {
        self.builder.clear();
        *self.rows.borrow_mut() = rows.to_vec();
        self.count.set(rows.len());
    }

    fn set_builder(&self, count: usize, f: impl FnMut(usize) -> Result<TableCells> + 'static) {
        self.rows.borrow_mut().clear();
        self.builder.set(f);
        self.count.set(count);
    }

    /// The contents of that row. `None` if there is none.
    fn cells(&self, index: usize) -> Option<TableCells> {
        if index >= self.count.get() {
            return None;
        }
        if !self.builder.is_set() {
            return self.rows.borrow().get(index).map(TableCells::from_row);
        }
        // 組み立てに失敗した行は、列だけそろえた空の行にする。
        Some(self.builder.build(index)?.unwrap_or_default())
    }

    fn set_selectable(&self, f: impl Fn(usize) -> bool + 'static) {
        *self.selectable.borrow_mut() = Some(Rc::new(f));
    }

    /// Whether the row can be selected, if that can be answered without building it.
    ///
    /// No borrow is held during the call (the app may touch the table from inside it).
    fn selectable_hint(&self, index: usize) -> Option<bool> {
        let f = self.selectable.borrow().clone()?;
        Some(f(index))
    }

    /// For text-only rows, whether the row can be selected. `None` for built rows.
    fn text_row_selectable(&self, index: usize) -> Option<bool> {
        match self.builder.is_set() {
            true => None,
            false => Some(self.rows.borrow().get(index).is_some_and(|row| row.enabled)),
        }
    }
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
        .map_err(|e| to_error("セルの要素化", e))?;
    // 列の中のどこへ置くかは列の指定に従う。`Fill` は列いっぱいに広げる。
    let _ = framework.SetHorizontalAlignment(match align {
        Align::Center => HorizontalAlignment::Center,
        Align::End => HorizontalAlignment::Right,
        Align::Fill => HorizontalAlignment::Stretch,
        Align::Start => HorizontalAlignment::Left,
    });
    let _ = framework.SetVerticalAlignment(VerticalAlignment::Center);
    XamlGrid::SetColumn(&framework, index as i32).map_err(|e| to_error("セルの列の指定", e))?;
    grid.Children()
        .and_then(|children| children.Append(&element))
        .map_err(|e| to_error("セルの追加", e))?;
    Ok(())
}

/// Infers how the selection is being changed from the modifier keys held now.
///
/// `SelectionChanged` does not carry modifier keys, so the key state is read on the
/// spot. `GetKeyState` returns the state as far as the thread has processed, so it
/// is the state at the same moment as the click or key operation that caused the
/// selection.
fn selection_gesture() -> SelectionGesture {
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
    /// The built rows themselves, in order from `window.start`.
    row_items: RefCell<Vec<ListViewItem>>,
    /// The contents of the built rows, in the same order as `row_items`.
    realized: RefCell<Vec<TableCells>>,
    columns: RefCell<Vec<TableColumn>>,
    rows: Rc<RowsState>,
    /// The range of rows currently built. With many rows, only those around the screen.
    window: Cell<RowWindow>,
    /// Whether rows are being built right now. Prevents nested rebuilds.
    rebuilding: Cell<bool>,
    /// The selected rows (ascending). Rows outside the window are included, so this is
    /// the source of truth, not the `ListView`.
    selected: RefCell<Vec<usize>>,
    /// The height of one row (logical pixels). Either measured or set with
    /// [`Table::set_row_height`].
    row_height: Cell<f64>,
    /// The row height decided by the app. If absent, it is measured from a built row.
    fixed_row_height: Cell<Option<f64>>,
    /// A frame filling in for the rows outside the window (placed before and after the
    /// row list).
    top_spacer: XamlGrid,
    bottom_spacer: XamlGrid,
    /// The timer that reads the display position. Runs only while rows are limited.
    scroll_timer: RefCell<Option<DispatcherQueueTimer>>,
    /// The scroll area bundling the row list and the spacers before and after it.
    scroll: RefCell<Option<ScrollViewer>>,
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
            .map_err(|e| to_error("ListView の選択方法の設定", e))?;
        // 横スクロールは持たせない (列は見出しと同じ幅で並べる)。
        let _ = ScrollViewer::SetHorizontalScrollBarVisibility2(
            &surface.list_view,
            ScrollBarVisibility::Disabled,
        );

        let hovered = Arc::new(UiThreadCell::new(0));

        let top_spacer = XamlGrid::new().map_err(|e| to_error("Table の詰め物の生成", e))?;
        let bottom_spacer = XamlGrid::new().map_err(|e| to_error("Table の詰め物の生成", e))?;
        // ListView 内の Header/Footer で巨大な空白を動かすと、行が存在しても
        // 内部の表示領域のクリップが追従せず描画されないことがある。
        // スクロールと詰め物は外側が持ち、ListView は実体化した行だけを測る。
        ScrollViewer::SetVerticalScrollBarVisibility2(
            &surface.list_view,
            ScrollBarVisibility::Disabled,
        )
        .map_err(|e| to_error("行リストのスクロールバー無効化", e))?;
        ScrollViewer::SetVerticalScrollMode2(&surface.list_view, ScrollMode::Disabled)
            .map_err(|e| to_error("行リストのスクロール無効化", e))?;
        let body = StackPanel::new().map_err(|e| to_error("Table の本体の生成", e))?;
        let root_children = surface
            .root
            .Children()
            .map_err(|e| to_error("枠の取得", e))?;
        root_children
            .RemoveAt(1)
            .map_err(|e| to_error("行リストの移動", e))?;
        let body_children = body.Children().map_err(|e| to_error("本体の取得", e))?;
        for element in [
            top_spacer.cast::<UIElement>(),
            surface.list_view.cast::<UIElement>(),
            bottom_spacer.cast::<UIElement>(),
        ] {
            body_children
                .Append(&element.map_err(|e| to_error("本体の要素化", e))?)
                .map_err(|e| to_error("本体への追加", e))?;
        }
        let scroll = ScrollViewer::new().map_err(|e| to_error("表のスクロール領域の生成", e))?;
        scroll
            .SetHorizontalScrollBarVisibility(ScrollBarVisibility::Disabled)
            .map_err(|e| to_error("横スクロールの無効化", e))?;
        scroll
            .SetContent(&body)
            .map_err(|e| to_error("表のスクロール内容の設定", e))?;
        XamlGrid::SetRow(&scroll, 1).map_err(|e| to_error("表の本体の配置", e))?;
        root_children
            .Append(&scroll)
            .map_err(|e| to_error("表の本体の追加", e))?;

        let this = Self(Rc::new(TableInner {
            native: surface.root,
            header: surface.header,
            list_view: surface.list_view,
            wheel: RefCell::new(None),
            row_items: RefCell::new(Vec::new()),
            realized: RefCell::new(Vec::new()),
            columns: RefCell::new(Vec::new()),
            rows: Rc::new(RowsState::default()),
            window: Cell::new(RowWindow::default()),
            rebuilding: Cell::new(false),
            selected: RefCell::new(Vec::new()),
            row_height: Cell::new(0.0),
            fixed_row_height: Cell::new(None),
            top_spacer,
            bottom_spacer,
            scroll_timer: RefCell::new(None),
            scroll: RefCell::new(Some(scroll)),
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
            .map_err(|e| to_error("ListView の購読", e))?;
        this.install_wheel_target()?;
        this.install_layout_updates()?;

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
            .map_err(|e| to_error("Table のポインター購読", e))?;
        this.0
            .native
            .PointerExited(&exited)
            .map_err(|e| to_error("Table のポインター購読", e))?;
        this.0
            .native
            .PointerMoved(&moved)
            .map_err(|e| to_error("Table のポインター購読", e))?;
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
        self.0.rows.set_rows(rows);
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
    /// screen. Without it, built rows follow [`TableCells::selectable`] and rows not
    /// yet built are treated as selectable.
    ///
    /// Its answers must match the [`TableCells::selectable`] returned by `build`.
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

    /// The row height (logical pixels). 0 if no row has been built yet.
    pub fn row_height(&self) -> f64 {
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
        self.write_selection(&[]);
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
        let Some(scroll) = self.0.scroll.borrow().clone() else {
            return;
        };
        let height = self.0.row_height.get().max(1.0);
        let top = index as f64 * height;
        let bottom = top + height;
        let view_top = scroll.VerticalOffset().unwrap_or(0.0);
        let viewport = scroll.ViewportHeight().unwrap_or(0.0);
        let target = if top < view_top {
            top
        } else if bottom > view_top + viewport {
            bottom - viewport
        } else {
            return;
        };
        let offset = PropertyValue::CreateDouble(target)
            .and_then(|value| value.cast::<windows::Foundation::IReference<f64>>());
        if let Ok(offset) = offset {
            let _ = scroll.ChangeViewWithOptionalAnimation(None, &offset, None, true);
        }
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
        self.write_selection(&picked);
    }

    /// Clears the whole selection without notifying.
    pub fn clear_selection(&self) {
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
            .map_err(|e| to_error("見出しの取得", e))?;
        children.Clear().map_err(|e| to_error("見出しの消去", e))?;
        apply_columns(&self.0.header, &columns)?;

        let mut buttons = Vec::with_capacity(columns.len());
        for (index, column) in columns.iter().enumerate() {
            if !column.sortable {
                append_cell(&self.0.header, index, &column.title, column.align, true)?;
                buttons.push(None);
                continue;
            }
            // 並べ替えられる列は、見出しそのものをボタンにする。
            let button = Button::new().map_err(|e| to_error("見出しのボタンの生成", e))?;
            if let Some(style) = self.0.header_style.as_ref() {
                let _ = button.SetStyle(style);
            }
            let label = text_block(&column.title, true)?;
            let _ = label.SetTextAlignment(text_alignment(column.align));
            let _ = label.SetTextWrapping(TextWrapping::NoWrap);
            button
                .SetContent(&label)
                .map_err(|e| to_error("見出しへの内容設定", e))?;

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
                .map_err(|e| to_error("見出しの購読", e))?;

            let framework = button
                .cast::<FrameworkElement>()
                .map_err(|e| to_error("見出しの要素化", e))?;
            XamlGrid::SetColumn(&framework, index as i32)
                .map_err(|e| to_error("見出しの列の指定", e))?;
            let element = button
                .cast::<UIElement>()
                .map_err(|e| to_error("見出しの要素化", e))?;
            children
                .Append(&element)
                .map_err(|e| to_error("見出しの追加", e))?;
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
            .map_err(|e| to_error("Table の表示の購読", e))?;
        Ok(())
    }

    /// Registers with the wheel helper, exactly once.
    fn register_wheel_target(&self) {
        if self.0.wheel.borrow().is_some() {
            return;
        }
        let Some(scroll) = self.0.scroll.borrow().clone() else {
            return;
        };
        let target = crate::layout::register_list_scroll(scroll, self.0.hovered.clone());
        *self.0.wheel.borrow_mut() = Some(target);
        // 大きさが決まったので、組み立てる範囲を引き直す。
        self.update_window();
    }

    // ------------------------------------------------------- 行を絞る窓

    /// Gets row swaps in before drawing. With the timer alone, empty spacers would be
    /// visible for a moment after scrolling, so it also updates when layout completes.
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
            .map_err(|e| to_error("Table のレイアウト更新の購読", e))?;
        Ok(())
    }

    /// Rebuilds the rows and moves the window back to the start. The selection is
    /// cleared too.
    fn reset_rows(&self) {
        self.0.selected.borrow_mut().clear();
        self.0.window.set(self.compute_window(ROW_WINDOW_OVERSCAN));
        self.build_window();
        if self.measure_row_height() {
            self.update_window();
        }
        self.watch_scrolling();
    }

    /// Rebuilds the `ListView` rows for just the current window.
    fn build_window(&self) {
        // 組み立ての途中でアプリのコードが動くので、そこから呼ばれても
        // 二重に作り直さない。
        if self.0.rebuilding.replace(true) {
            return;
        }
        let result = self.build_window_once();
        self.0.rebuilding.set(false);
        if let Err(error) = result {
            eprintln!("naui-windows: テーブルの行の組み立てに失敗: {error}");
        }
    }

    fn build_window_once(&self) -> Result<()> {
        // コレクションを空にせず置換し、スクロールのリセットを避ける。
        // 行数が減って位置が補正された場合だけ、レイアウト後に戻す。
        let scroll = self.0.scroll.borrow().clone();
        let offset = scroll
            .as_ref()
            .and_then(|scroll| scroll.VerticalOffset().ok());
        let children = self
            .0
            .list_view
            .Items()
            .map_err(|e| to_error("行の取得", e))?;
        let previous_count = children.Size().map_err(|e| to_error("行数の取得", e))?;
        self.0.row_items.borrow_mut().clear();
        self.0.realized.borrow_mut().clear();

        let window = self.0.window.get();
        // 組み立ての中でアプリのコードが動くので、列の借用は持ち越さない。
        let columns = self.0.columns.borrow().clone();
        let mut items = Vec::with_capacity(window.len());
        let mut realized = Vec::with_capacity(window.len());
        for index in window.indices() {
            let Some(cells) = self.0.rows.cells(index) else {
                continue;
            };
            let item = ListViewItem::new().map_err(|e| to_error("ListViewItem の生成", e))?;
            // 見出しと列をそろえるため、余白と高さは行ごとに書く。
            let _ = item.SetPadding(ROW_PADDING);
            match self.0.fixed_row_height.get() {
                Some(height) => {
                    let _ = item.SetHeight(height);
                    let _ = item.SetMinHeight(height);
                }
                None => {
                    let _ = item.SetMinHeight(ROW_MIN_HEIGHT);
                }
            }
            let content = XamlGrid::new().map_err(|e| to_error("行の Grid の生成", e))?;
            apply_columns(&content, &columns)?;
            for (column_index, column) in columns.iter().enumerate() {
                match cells.content(column_index) {
                    Some(CellContent::Widget(widget)) => {
                        append_widget_cell(&content, column_index, &**widget, column.align)?;
                    }
                    // 列より短い行は、足りない分が空のセルになる。
                    Some(CellContent::Text(text)) => {
                        append_cell(&content, column_index, text, column.align, false)?;
                    }
                    None => append_cell(&content, column_index, "", column.align, false)?,
                }
            }
            item.SetContent(&content)
                .map_err(|e| to_error("行への内容設定", e))?;
            // 文字だけの行で `enabled` が `false` のときは、行ごと操作できない。
            let _ = item.SetIsEnabled(!cells.dimmed);
            self.attach_row_click(&item, index)?;

            let element = item
                .cast::<IInspectable>()
                .map_err(|e| to_error("行の要素化", e))?;
            let slot = items.len() as u32;
            self.without_notifying(|_| {
                if slot < previous_count {
                    children.SetAt(slot, &element)
                } else {
                    children.Append(&element)
                }
            })
            .map_err(|e| to_error("行の置換", e))?;
            items.push(item);
            realized.push(cells);
        }
        for _ in items.len() as u32..previous_count {
            self.without_notifying(|_| children.RemoveAtEnd())
                .map_err(|e| to_error("余った行の消去", e))?;
        }
        *self.0.row_items.borrow_mut() = items;
        *self.0.realized.borrow_mut() = realized;

        let height = self.0.row_height.get();
        set_spacer_height(&self.0.top_spacer, window.leading(height));
        set_spacer_height(&self.0.bottom_spacer, window.trailing(height));

        // 覚えている選択を、組み立て直した行へ写す。
        let picked = self.selection();
        self.write_selection(&picked);
        if let (Some(scroll), Some(offset)) = (scroll, offset) {
            scroll
                .UpdateLayout()
                .map_err(|e| to_error("テーブルのレイアウト更新", e))?;
            // 毎回 ChangeView を呼ぶと、ホイールの進行中の移動まで止めてしまう。
            if (scroll.VerticalOffset().unwrap_or(offset) - offset).abs() < 0.5 {
                return Ok(());
            }
            let offset = PropertyValue::CreateDouble(offset)
                .and_then(|value| value.cast::<windows::Foundation::IReference<f64>>())
                .map_err(|e| to_error("スクロール位置の生成", e))?;
            scroll
                .ChangeViewWithOptionalAnimation(None, &offset, None, true)
                .map_err(|e| to_error("スクロール位置の復元", e))?;
        }
        Ok(())
    }

    /// Attaches the subscription that fires the activation when a row is clicked.
    ///
    /// Buttons and fields in cells receive the click themselves and `Tapped` stops
    /// there, so it does not double up with the row's activation.
    fn attach_row_click(&self, item: &ListViewItem, index: usize) -> Result<()> {
        let state = UiThreadCell::new(Rc::downgrade(&self.0));
        let handler = TappedEventHandler::new(move |_sender, _args| {
            let _ = state.try_with_mut(|weak| {
                if let Some(inner) = weak.upgrade() {
                    Table(inner).activate_row(index);
                }
            });
            Ok(())
        });
        item.Tapped(&handler).map_err(|e| to_error("行の購読", e))?;
        Ok(())
    }

    /// Fires that row's activation.
    fn activate_row(&self, index: usize) {
        let window = self.0.window.get();
        let activation = self
            .0
            .realized
            .borrow()
            .get(index.wrapping_sub(window.start))
            .map(|cells| cells.activation.clone());
        if let Some(activation) = activation {
            activation.emit();
        }
    }

    /// Computes the range of rows to build from the current scroll position.
    fn compute_window(&self, overscan: usize) -> RowWindow {
        let scroll = self.0.scroll.borrow().clone();
        let (offset, viewport) = match scroll {
            Some(scroll) => (
                scroll.VerticalOffset().unwrap_or(0.0),
                scroll.ViewportHeight().unwrap_or(0.0),
            ),
            None => (0.0, 0.0),
        };
        // 詰め物が窓の外の行と同じ高さを持つので、表示位置は
        // 「全行を入れたとき」と同じ座標になる。
        row_window(
            self.len(),
            offset,
            viewport,
            self.0.row_height.get(),
            overscan,
        )
    }

    /// Moves the built range along with scrolling.
    ///
    /// Rebuilding for every row scrolled is heavy, so the window is reused until you
    /// have scrolled through half of the extra rows ([`ROW_WINDOW_OVERSCAN`]).
    fn update_window(&self) {
        // UpdateLayout 中の再入では、差し替え途中の位置を読まない。
        if self.0.rebuilding.get() {
            return;
        }
        // 行の追加直後は ActualHeight が 0 のことがある。窓が同じでも
        // レイアウト後に再計測し、全行分のスクロール領域を確保する。
        self.measure_row_height();
        let current = self.0.window.get();
        let needed = self.compute_window(ROW_WINDOW_OVERSCAN / 2);
        let next = self.compute_window(ROW_WINDOW_OVERSCAN);
        if next == current || keeps_row_window(current, needed, next) {
            return;
        }
        self.0.window.set(next);
        self.build_window();
        self.measure_row_height();
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

    /// Runs the timer that re-reads the display position, only while rows are limited.
    ///
    /// `ScrollViewer.ViewChanged` is not in this projection, so this is the only way to
    /// notice scrolling. It is stopped for tables with few rows.
    fn watch_scrolling(&self) {
        let needed = self.len() > ROW_WINDOW_THRESHOLD;
        if !needed {
            if let Some(timer) = self.0.scroll_timer.borrow_mut().take() {
                let _ = timer.Stop();
            }
            return;
        }
        if self.0.scroll_timer.borrow().is_some() {
            return;
        }
        let Ok(queue) = DispatcherQueue::GetForCurrentThread() else {
            return;
        };
        let Ok(timer) = queue.CreateTimer() else {
            return;
        };
        let interval = TimeSpan {
            Duration: SCROLL_POLL_MILLIS * TICKS_PER_MILLI,
        };
        if timer.SetInterval(interval).is_err() || timer.SetIsRepeating(true).is_err() {
            return;
        }
        let state = UiThreadCell::new(Rc::downgrade(&self.0));
        let handler = windows::Foundation::TypedEventHandler::<
            DispatcherQueueTimer,
            windows_core::IInspectable,
        >::new(move |_sender, _args| {
            let _ = state.try_with_mut(|weak| {
                if let Some(inner) = weak.upgrade() {
                    Table(inner).update_window();
                }
            });
            Ok(())
        });
        if timer.Tick(&handler).is_err() || timer.Start().is_err() {
            return;
        }
        *self.0.scroll_timer.borrow_mut() = Some(timer);
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
        if index >= self.len() {
            return false;
        }
        if let Some(enabled) = self.0.rows.text_row_selectable(index) {
            return enabled;
        }
        // 組み立てずに答えられるなら、そちらが先 (画面の外の行にも効く)。
        if let Some(selectable) = self.0.rows.selectable_hint(index) {
            return selectable;
        }
        let window = self.0.window.get();
        match window.contains(index) {
            true => self
                .0
                .realized
                .borrow()
                .get(index - window.start)
                .is_none_or(TableCells::is_selectable),
            false => true,
        }
    }

    /// Remembers the selection and writes it into the built rows (no notification).
    ///
    /// Rows outside the window have no `ListViewItem`, so they cannot be written. When
    /// the window moves and rows are rebuilt, it is written again from the remembered
    /// selection.
    fn write_selection(&self, indices: &[usize]) {
        *self.0.selected.borrow_mut() = indices.to_vec();
        let start = self.0.window.get().start;
        self.without_notifying(|this| {
            for (offset, item) in this.0.row_items.borrow().iter().enumerate() {
                let _ = item.SetIsSelected(indices.contains(&(start + offset)));
            }
        });
    }

    /// Reads the selection the user changed. How rows outside the window are treated is
    /// decided here too.
    fn read_native_selection(&self) -> Vec<usize> {
        let window = self.0.window.get();
        let mut picked: Vec<usize> = self
            .0
            .row_items
            .borrow()
            .iter()
            .enumerate()
            .filter(|(_, item)| item.IsSelected().unwrap_or(false))
            .map(|(offset, _)| window.start + offset)
            .collect();
        // `ListViewItem` には「選ばせない」指定が無い (`IsEnabled` を落とすと
        // 中のボタンまで効かなくなる) ので、選べない行はここで外し、
        // ネイティブ側へも書き戻す。
        if picked.iter().any(|&index| !self.is_selectable(index)) {
            picked.retain(|&index| self.is_selectable(index));
            self.write_selection(&picked);
        }
        if window.is_complete() {
            return picked;
        }
        // 組み立ててある行の選択しか届かないので、窓の外の選択を残すかどうかを
        // 修飾キーから決める (`keeps_hidden_selection`)。
        let previous = self.0.selected.borrow().clone();
        let inside: Vec<usize> = previous
            .iter()
            .copied()
            .filter(|&index| window.contains(index))
            .collect();
        if !keeps_hidden_selection(&inside, &picked, selection_gesture()) {
            return picked;
        }
        picked.extend(previous.iter().copied().filter(|&i| !window.contains(i)));
        picked.sort_unstable();
        picked.dedup();
        picked
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
