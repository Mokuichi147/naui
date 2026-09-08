//! テーブル (WinUI 3)。
//!
//! WinUI 3 に `DataGrid` は無い (Community Toolkit のもの) ので、リスト
//! ([`crate::List`]) と同じ `ListView` を土台にして、**行の中身を `Grid` に
//! した**形で組んでいる。
//!
//! | 部分 | 作り |
//! | --- | --- |
//! | 枠 | 2 行の `Grid` (見出し / 本体) に背景・境界線・角丸を持たせる |
//! | 見出し | 列と同じ `ColumnDefinition` を持つ `Grid` + `TextBlock` |
//! | 本体 | `ListView`。行は `ListViewItem` + `Grid` |
//! | 選択 | `ListView` の標準テンプレート (淡い塗り + 左端の指標) |
//!
//! 見出しと行は**同じ列定義を配る**ことで幅をそろえる。列の幅を
//! ドラッグで変えることはできない (`NSTableView` と違い、WinUI には
//! そのための標準コントロールが無いため)。
//!
//! 色・角丸はすべて `{ThemeResource ...}` で引くので、ライト / ダークの
//! 切り替えにそのまま追従する。テーマリソースが引けない環境では、
//! 素の `Grid` + `ListView` に戻して動作を優先する。

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use naui_core::{
    keeps_hidden_selection, keeps_row_window, row_window, Align, Result, RowWindow, SelectionMode,
    SortOrder, TableColumn, TableRow, ROW_WINDOW_OVERSCAN, ROW_WINDOW_THRESHOLD,
};
use naui_winui3::Microsoft::UI::Dispatching::{DispatcherQueue, DispatcherQueueTimer};
use naui_winui3::Microsoft::UI::Xaml::Controls::{
    Button, ColumnDefinition, Grid as XamlGrid, ListView, ListViewItem, ListViewSelectionMode,
    RowDefinition, ScrollBarVisibility, ScrollViewer, SelectionChangedEventHandler, TextBlock,
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

/// 表の枠。見出しと本体を縦に分けた `Grid` で、境界線と角丸はここが持つ。
///
/// 行 (`ListViewItem`) の余白。見出しの `Padding` と**左右をそろえる**ことで
/// 列の位置が合う。`ListView` の既定の余白は見出しと違うので、行ごとに書く。
const ROW_PADDING: Thickness = Thickness {
    Left: 12.0,
    Top: 8.0,
    Right: 12.0,
    Bottom: 8.0,
};

/// 行の高さの下限。`List` の行と同じ。
const ROW_MIN_HEIGHT: f64 = 36.0;

/// 見出しの `Padding` は、行の `Padding` ([`ROW_PADDING`]) と同じ横位置に
/// なるようにそろえてある。本体へ `Padding` を置くと見出しとずれるため、
/// そちらは 0 のままにしている。
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
        HorizontalContentAlignment="Stretch"/>
</Grid>"##;

/// 並べ替えできる見出しのボタン。地色も枠も出さず、見出しの文字のまま
/// 押せるようにする (WinUI に列見出し用のコントロールが無いため)。
///
/// `HEADER_STYLE_KEY` は `HEADER_STYLE_XAML` の `x:Key` と同じ文字列にする。
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

/// 見出しのボタンに当てる `Style`。読めなければ `None` (既定の見た目)。
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

/// 並べ替えの向きを表す文字。WinUI に列見出しの指標が無いので、
/// 見出しの文字の後ろへ付ける。
fn sort_arrow(order: Option<SortOrder>) -> &'static str {
    match order {
        Some(SortOrder::Ascending) => " ▲",
        Some(SortOrder::Descending) => " ▼",
        None => "",
    }
}

/// 並べ替えが変わったことの通知先。
///
/// WinRT のデリゲートは `Send + Sync` を要求するため `UiThreadCell` に載せる
/// ([`SelectionHandler`] と同じ形)。
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

/// 見出しと本体をまとめた枠。
struct Surface {
    root: XamlGrid,
    header: XamlGrid,
    list_view: ListView,
}

/// テーマ付きの枠を読み込む。読めなければ素の `Grid` で組み直す。
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
    let list_view = ListView::new().map_err(|e| to_error("ListView の生成", e))?;

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

/// WinUI の文字揃えへ写す。`Fill` は文字に意味が無いので左と同じ扱い。
fn text_alignment(align: Align) -> TextAlignment {
    match align {
        Align::Center => TextAlignment::Center,
        Align::End => TextAlignment::Right,
        Align::Start | Align::Fill => TextAlignment::Left,
    }
}

/// 列の定義を `Grid` の `ColumnDefinition` として配る。
///
/// 見出しにも行にも同じものを配ることで、幅がそろう。
fn apply_columns(grid: &XamlGrid, columns: &[TableColumn]) -> Result<()> {
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

/// セル 1 つ分の `TextBlock` を、列の位置へ置く。
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
    XamlGrid::SetColumn(&framework, index as i32).map_err(|e| to_error("セルの列の指定", e))?;
    let element = block
        .cast::<UIElement>()
        .map_err(|e| to_error("セルの要素化", e))?;
    grid.Children()
        .and_then(|children| children.Append(&element))
        .map_err(|e| to_error("セルの追加", e))?;
    Ok(())
}

/// 表示位置を読み直す間隔 (ミリ秒)。
///
/// `ScrollViewer.ViewChanged` はこの投影に入っていないので、スクロール中は
/// 短い間隔で表示位置を読む。読むだけなので、位置が変わらなければ何もしない。
const SCROLL_POLL_MILLIS: i64 = 50;

/// `TimeSpan` の 1 ミリ秒 (100 ナノ秒きざみ)。
const TICKS_PER_MILLI: i64 = 10_000;

/// セル 1 つ分の中身。
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

/// 表へ載せる 1 行の中身。
///
/// [`TableRow`] は文字列だけで済む表向けの簡便 API であり、セルにボタンや
/// チェックボックス、アイコンを置きたいときは `Grid` / `Stack` で中身を
/// 作ってこの型へ並べる。行は [`Table::set_row_builder`] から返す。
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
    /// 文字だけの行で `enabled` が `false` のとき。行ごと操作できなくする。
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
    /// セルが 1 つも無い行を作る。
    pub fn new() -> Self {
        Self {
            cells: Vec::new(),
            selectable: true,
            dimmed: false,
            activation: ActivationHandler::new(),
        }
    }

    /// 文字のセルを 1 つ足す。揃えは列の指定に従う。
    pub fn text(mut self, text: impl Into<String>) -> Self {
        self.cells.push(CellContent::Text(text.into()));
        self
    }

    /// ウィジェットのセルを 1 つ足す。
    pub fn cell(mut self, content: &dyn Widget) -> Self {
        self.cells.push(CellContent::Widget(content.boxed_clone()));
        self
    }

    /// 行全体を選択できるようにするかどうか (既定はできる)。
    pub fn selectable(mut self, selectable: bool) -> Self {
        self.selectable = selectable;
        self
    }

    pub fn is_selectable(&self) -> bool {
        self.selectable
    }

    /// 列数。
    pub fn len(&self) -> usize {
        self.cells.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    /// 行のセルや余白がクリックされたときに呼ぶ処理。
    ///
    /// ボタンや入力欄を直接押した場合は、そのコントロールがクリックを
    /// 受け取るので呼ばれない。
    pub fn on_activate(&self, f: impl FnMut() + 'static) {
        self.activation.set(f);
    }

    /// 文字だけの行から作る。`enabled` はそのまま「選べるか」になる。
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

/// 行を組み立てるクロージャ。呼び出しの間だけ取り出す。
/// 行を組み立てるクロージャの置き場。
type RowBuildCell = Rc<RefCell<Option<Box<dyn FnMut(usize) -> Result<TableCells>>>>>;

/// 行を組み立てるクロージャ。
///
/// 呼び出しの間だけ取り出すので、組み立ての中から表を操作しても
/// 二重借用にならない。
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

/// 行の出どころ。文字だけの行 ([`Table::set_rows`]) と、
/// 見えたときに組み立てる行 ([`Table::set_row_builder`]) の 2 通り。
#[derive(Default)]
struct RowsState {
    rows: RefCell<Vec<TableRow>>,
    builder: RowBuilder,
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

    /// その行の中身。無ければ `None`。
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

    /// 文字だけの行で、その行が選べるか。組み立てる行では `None`。
    fn text_row_selectable(&self, index: usize) -> Option<bool> {
        match self.builder.is_set() {
            true => None,
            false => Some(self.rows.borrow().get(index).is_some_and(|row| row.enabled)),
        }
    }
}

/// ウィジェットのセルを、列の位置へ置く。
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

/// 窓の外にある行の分を、詰め物の高さとして持たせる。
fn set_spacer_height(spacer: &XamlGrid, height: f64) {
    let _ = spacer.SetHeight(height.max(0.0));
}

struct TableInner {
    native: XamlGrid,
    header: XamlGrid,
    list_view: ListView,
    /// ホイール補助への登録。テンプレートの `ScrollViewer` が現れるまで
    /// 決まらないので、`Loaded` のあとで入る。
    wheel: RefCell<Option<Rc<ListScrollTarget>>>,
    /// 組み立ててある行そのもの。並びは `window.start` から。
    row_items: RefCell<Vec<ListViewItem>>,
    /// 組み立ててある行の中身。`row_items` と同じ並び。
    realized: RefCell<Vec<TableCells>>,
    columns: RefCell<Vec<TableColumn>>,
    rows: Rc<RowsState>,
    /// いま組み立ててある行の範囲。行数が多いと画面の前後だけになる。
    window: Cell<RowWindow>,
    /// 行を組み立てている最中か。入れ子の作り直しを防ぐ。
    rebuilding: Cell<bool>,
    /// 選ばれている行 (昇順)。窓の外の行も入るので、`ListView` ではなく
    /// ここが正。
    selected: RefCell<Vec<usize>>,
    /// 1 行の高さ (論理ピクセル)。測った値か [`Table::set_row_height`] の指定。
    row_height: Cell<f64>,
    /// アプリが決めた行の高さ。無ければ組み立てた行から測る。
    fixed_row_height: Cell<Option<f64>>,
    /// 窓の外にある行の分を埋める枠 (`ListView` の Header / Footer)。
    top_spacer: XamlGrid,
    bottom_spacer: XamlGrid,
    /// 表示位置を読むタイマー。行を絞っている間だけ動かす。
    scroll_timer: RefCell<Option<DispatcherQueueTimer>>,
    /// テンプレートの中の `ScrollViewer`。`Loaded` のあとで入る。
    scroll: RefCell<Option<ScrollViewer>>,
    mode: Cell<SelectionMode>,
    handler: SelectionHandler,
    /// いまの並べ替え (列と向き)。
    sort: Cell<Option<(usize, SortOrder)>>,
    sort_handler: SortHandler,
    /// 見出しのボタン。並べ替えできる列にだけある。指標の書き替えに使う。
    header_buttons: RefCell<Vec<Option<Button>>>,
    /// 見出しのボタンに当てる見た目。読めなかったときだけ `None`。
    header_style: Option<Style>,
    /// プログラムから選択を変えている間だけ通知を止める。
    /// `IsSelected` の書き換えでも `SelectionChanged` が起きるため。
    silent: Rc<Cell<bool>>,
    /// ウィンドウ全体のホイール補助が、この表の ScrollViewer を選ぶための状態。
    hovered: Arc<UiThreadCell<usize>>,
}

/// 列見出しを持つ表 (Grid + ListBox)。
///
/// 高さは中身から決まるので、行数に関係なく固定したいときは
/// `set_sizing` で指定する。
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
        // 行が多い表では、画面に出ている分だけを `Items` へ入れ、残りの行が
        // 占めるはずの高さを Header / Footer が持つ。スクロールバーの長さも
        // 位置も、全行を入れたときと変わらない。
        let _ = surface.list_view.SetHeader(&top_spacer);
        let _ = surface.list_view.SetFooter(&bottom_spacer);

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
            scroll: RefCell::new(None),
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

    /// 列を作り直す。行と選択はそのまま残り、セルの並べ直しだけが起きる。
    ///
    /// 並べ替えの指定も、その列がまだ並べ替えられるなら残る。
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

    /// 列数。
    pub fn column_count(&self) -> usize {
        self.0.columns.borrow().len()
    }

    /// 行を作り直す。インデックスの意味が変わるため、選択は外れる。
    ///
    /// 行数が多いときは、`ListViewItem` を作るのも画面に出ている分だけに
    /// なる (残りは Header / Footer の詰め物が高さを持つので、
    /// スクロールバーの長さは全行分のまま)。
    pub fn set_rows(&self, rows: &[TableRow]) {
        self.0.rows.set_rows(rows);
        self.reset_rows();
    }

    /// セルにウィジェットを置ける行を、**見えたときに組み立てる**形で渡す。
    ///
    /// `count` は行数で、`build` は 0 から `count - 1` のインデックスを受けて
    /// その行の中身 ([`TableCells`]) を返す。呼ばれるのは画面に出ている行
    /// (と、その少し前後) だけなので、行数が数十万になっても開くのは速い。
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

    /// 見えている行を組み立て直す。行数と選択はそのまま。
    pub fn refresh(&self) {
        let picked = self.selection();
        self.build_window();
        self.write_selection(&picked);
        // 中身が変わって行の高さが動いていれば、窓を引き直す。
        if self.measure_row_height() {
            self.update_window();
        }
    }

    /// 行の高さを論理ピクセルで決める。0 以下を渡すと、組み立てた行から測る。
    ///
    /// どの行も同じ高さになる。画面の外にある行の分は「行数 × この高さ」で
    /// 詰めるので、行の高さがそろっていないと、スクロールバーの長さが
    /// 実際と少しずれる。
    pub fn set_row_height(&self, height: f64) {
        self.0
            .fixed_row_height
            .set((height > 0.0).then_some(height));
        // 指定を外したときは、組み立て直した行から測り直す。
        self.0.row_height.set(height.max(0.0));
        self.refresh();
    }

    /// 行の高さ (論理ピクセル)。まだ 1 行も組み立てていなければ 0。
    pub fn row_height(&self) -> f64 {
        self.0.row_height.get()
    }

    /// 行数。
    pub fn len(&self) -> usize {
        self.0.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// 選び方を変える。選択の意味が変わるため、選択は外れる。
    ///
    /// 複数選択は WinUI の `Extended` (Ctrl / Shift を押しながら選ぶ) に写す。
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

    /// 選ばれている行のうち、いちばん上のもの。
    pub fn selected(&self) -> Option<usize> {
        self.selection().first().copied()
    }

    /// 選ばれている行 (昇順)。単一選択なら 0 件か 1 件。
    ///
    /// 窓の外にある行も入る。`ListView` が知っているのは組み立ててある行
    /// だけなので、覚えているほうを返す。
    pub fn selection(&self) -> Vec<usize> {
        self.0.selected.borrow().clone()
    }

    /// 通知せずに 1 行だけを選ぶ。
    pub fn set_selected(&self, index: usize) {
        self.set_selection(&[index]);
    }

    /// 通知せずに選択を置き換える。
    ///
    /// 範囲外・選べない行・重複は取り除かれ、単一選択なら先頭の 1 件だけが残る
    /// ([`SelectionMode::normalize_by`])。
    pub fn set_selection(&self, indices: &[usize]) {
        let picked = self.normalize(indices);
        self.write_selection(&picked);
    }

    /// 通知せずに選択をすべて外す。
    pub fn clear_selection(&self) {
        self.write_selection(&[]);
    }

    /// ユーザーが選んだのと同じ経路で 1 行を選ぶ (通知あり)。
    pub fn select(&self, index: usize) {
        self.select_many(&[index]);
    }

    /// ユーザーが選んだのと同じ経路で選択を置き換える (通知あり)。
    pub fn select_many(&self, indices: &[usize]) {
        self.set_selection(indices);
        // 同じ選択を選び直すと `SelectionChanged` は起きないため、
        // 通知の回数をそろえてここで 1 回だけ出す。
        let actual = self.selection();
        self.0.handler.emit(&actual);
    }

    /// 選択が変わったときに、選ばれている行 (昇順) で呼ばれる。
    ///
    /// 複数選択では 0 件で呼ばれることもある。
    pub fn on_select(&self, f: impl FnMut(&[usize]) + 'static) {
        self.0.handler.set(f);
    }

    /// 見出しが押されて並べ替えの指定が変わったときに、
    /// 押された列と向きで呼ばれる。
    ///
    /// [`TableColumn::sortable`] を立てた列だけが押せる。**行を並べ替えるのは
    /// アプリの仕事**で、通知を受けたら並べ替えた行を [`Table::set_rows`] で
    /// 渡し直す (`set_rows` は選択を外すので、必要なら選び直す)。
    pub fn on_sort(&self, f: impl FnMut(usize, SortOrder) + 'static) {
        self.0.sort_handler.set(f);
    }

    /// いまの並べ替えの指定 (列と向き)。押されたことが無ければ `None`。
    pub fn sort(&self) -> Option<(usize, SortOrder)> {
        self.0.sort.get()
    }

    /// 通知せずに並べ替えの指定を置き換える。見出しの指標だけが変わる。
    ///
    /// 並べ替えられない列や範囲外の列を指すと、指定は外れる。
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

    /// 中身の `ListBox`。バックエンド固有の脱出口として公開している。
    pub fn native_list_view(&self) -> ListView {
        self.0.list_view.clone()
    }

    // ------------------------------------------------------------ 組み立て

    /// 見出しを、いまの列の定義から作り直す。
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

    /// 見出しが押されたとき。同じ列なら向きを反転し、違う列なら昇順から。
    fn on_header_activated(&self, index: usize) {
        let next = match self.0.sort.get() {
            Some((column, order)) if column == index => (index, order.reversed()),
            _ => (index, SortOrder::Ascending),
        };
        self.0.sort.set(Some(next));
        self.apply_sort();
        self.0.sort_handler.emit(next.0, next.1);
    }

    /// 並べ替えの指定を見出しへ書く。
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

    /// テンプレートの中の `ScrollViewer` を、ホイール補助の行き先として登録する。
    ///
    /// `ListView` の中身は `Loaded` まで組み上がらないので、そこまで待つ。
    /// 見つからなければ登録しないだけで、コントロール自身のスクロールは動く。
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

    /// ホイール補助への登録を 1 回だけ行う。
    fn register_wheel_target(&self) {
        if self.0.wheel.borrow().is_some() {
            return;
        }
        let Some(scroll) = crate::layout::scroll_viewer_within(&self.0.list_view) else {
            return;
        };
        *self.0.scroll.borrow_mut() = Some(scroll.clone());
        let target = crate::layout::register_list_scroll(scroll, self.0.hovered.clone());
        *self.0.wheel.borrow_mut() = Some(target);
        // 大きさが決まったので、組み立てる範囲を引き直す。
        self.update_window();
    }

    // ------------------------------------------------------- 行を絞る窓

    /// 行を作り直して、窓を先頭へ戻す。選択も外れる。
    fn reset_rows(&self) {
        self.0.selected.borrow_mut().clear();
        self.0.window.set(self.compute_window(ROW_WINDOW_OVERSCAN));
        self.build_window();
        if self.measure_row_height() {
            self.update_window();
        }
        self.watch_scrolling();
    }

    /// いまの窓の分だけ、`ListView` の行を作り直す。
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
        let children = self
            .0
            .list_view
            .Items()
            .map_err(|e| to_error("行の取得", e))?;
        self.without_notifying(|_| children.Clear())
            .map_err(|e| to_error("行の消去", e))?;
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
            self.without_notifying(|_| children.Append(&element))
                .map_err(|e| to_error("行の追加", e))?;
            items.push(item);
            realized.push(cells);
        }
        *self.0.row_items.borrow_mut() = items;
        *self.0.realized.borrow_mut() = realized;

        let height = self.0.row_height.get();
        set_spacer_height(&self.0.top_spacer, window.leading(height));
        set_spacer_height(&self.0.bottom_spacer, window.trailing(height));

        // 覚えている選択を、組み立て直した行へ写す。
        let picked = self.selection();
        self.write_selection(&picked);
        Ok(())
    }

    /// 行が押されたときに activation を出す購読を付ける。
    ///
    /// セルのボタンや入力欄はそれ自身がクリックを受け取り、`Tapped` は
    /// そこで止まるので、行の activation とは二重にならない。
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

    /// その行の activation を出す。
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

    /// いまのスクロール位置から、組み立てておく行の範囲を求める。
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

    /// スクロールに合わせて、組み立てる範囲を動かす。
    ///
    /// 1 行スクロールするたびに作り直すのは重いので、余分に作ってある分
    /// ([`ROW_WINDOW_OVERSCAN`]) の半分までは、そのまま使う。
    fn update_window(&self) {
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

    /// 組み立てた行から 1 行の高さを測る。変わったら `true`。
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

    /// 行を絞っている間だけ、表示位置を読み直すタイマーを動かす。
    ///
    /// `ScrollViewer.ViewChanged` はこの投影に入っていないので、
    /// スクロールに気づく手立てがこれしかない。行が少ない表では止めておく。
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

    /// 指定された選択を、この表で意味を持つ形にそろえる。
    fn normalize(&self, indices: &[usize]) -> Vec<usize> {
        self.0
            .mode
            .get()
            .normalize_by(indices, |index| self.is_selectable(index))
    }

    /// その行を選べるか。
    ///
    /// 組み立てる行では、まだ作っていない行は「選べる」とみなす
    /// (作ってみないと分からないため)。
    fn is_selectable(&self, index: usize) -> bool {
        if index >= self.len() {
            return false;
        }
        if let Some(enabled) = self.0.rows.text_row_selectable(index) {
            return enabled;
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

    /// 選択を覚えて、組み立ててある行へ書き込む (通知は起きない)。
    ///
    /// 窓の外の行には `ListViewItem` が無いので書けない。窓が動いて
    /// 組み立て直したときに、また覚えているほうから書く。
    fn write_selection(&self, indices: &[usize]) {
        *self.0.selected.borrow_mut() = indices.to_vec();
        let start = self.0.window.get().start;
        self.without_notifying(|this| {
            for (offset, item) in this.0.row_items.borrow().iter().enumerate() {
                let _ = item.SetIsSelected(indices.contains(&(start + offset)));
            }
        });
    }

    /// ユーザーが変えた選択を読む。窓の外の行の扱いもここで決める。
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
        // 変わり方から決める (`keeps_hidden_selection`)。
        let previous = self.0.selected.borrow().clone();
        let inside: Vec<usize> = previous
            .iter()
            .copied()
            .filter(|&index| window.contains(index))
            .collect();
        if !keeps_hidden_selection(&inside, &picked) {
            return picked;
        }
        picked.extend(previous.iter().copied().filter(|&i| !window.contains(i)));
        picked.sort_unstable();
        picked.dedup();
        picked
    }

    /// WinUI からの通知を止めたまま操作する。
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
