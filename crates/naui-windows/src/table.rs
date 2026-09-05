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

use naui_core::{Align, Result, SelectionMode, SortOrder, TableColumn, TableRow};
use naui_winui3::Microsoft::UI::Xaml::Controls::{
    Button, ColumnDefinition, Grid as XamlGrid, ListView, ListViewItem, ListViewSelectionMode,
    RowDefinition, ScrollBarVisibility, ScrollViewer, SelectionChangedEventHandler, TextBlock,
};
use naui_winui3::Microsoft::UI::Xaml::Input::PointerEventHandler;
use naui_winui3::Microsoft::UI::Xaml::Markup::XamlReader;
use naui_winui3::Microsoft::UI::Xaml::ResourceDictionary;
use naui_winui3::Microsoft::UI::Xaml::{
    FrameworkElement, GridLength, GridUnitType, RoutedEventHandler, Style, TextAlignment,
    TextWrapping, Thickness, UIElement,
};
use windows::Foundation::PropertyValue;
use windows_core::{IInspectable, Interface, HSTRING};

use crate::layout::ListScrollTarget;
use crate::list::{text_block, SelectionHandler};
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

struct TableInner {
    native: XamlGrid,
    header: XamlGrid,
    list_view: ListView,
    /// ホイール補助への登録。テンプレートの `ScrollViewer` が現れるまで
    /// 決まらないので、`Loaded` のあとで入る。
    wheel: RefCell<Option<Rc<ListScrollTarget>>>,
    /// 行そのもの。選択の読み書きはここを通す。
    row_items: RefCell<Vec<ListViewItem>>,
    columns: RefCell<Vec<TableColumn>>,
    rows: RefCell<Vec<TableRow>>,
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

        let this = Self(Rc::new(TableInner {
            native: surface.root,
            header: surface.header,
            list_view: surface.list_view,
            wheel: RefCell::new(None),
            row_items: RefCell::new(Vec::new()),
            columns: RefCell::new(Vec::new()),
            rows: RefCell::new(Vec::new()),
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
                        let indices = table.selection();
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
        let rows = self.0.rows.borrow().clone();
        let _ = self.rebuild_rows(&rows);
        self.write_selection(&picked);
    }

    /// 列数。
    pub fn column_count(&self) -> usize {
        self.0.columns.borrow().len()
    }

    /// 行を作り直す。インデックスの意味が変わるため、選択は外れる。
    pub fn set_rows(&self, rows: &[TableRow]) {
        *self.0.rows.borrow_mut() = rows.to_vec();
        let _ = self.rebuild_rows(rows);
    }

    /// 行数。
    pub fn len(&self) -> usize {
        self.0.rows.borrow().len()
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
    pub fn selection(&self) -> Vec<usize> {
        self.0
            .row_items
            .borrow()
            .iter()
            .enumerate()
            .filter(|(_, row)| row.IsSelected().unwrap_or(false))
            .map(|(index, _)| index)
            .collect()
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
        let target = crate::layout::register_list_scroll(scroll, self.0.hovered.clone());
        *self.0.wheel.borrow_mut() = Some(target);
    }

    /// 行を、いまの列と行から作り直す。
    fn rebuild_rows(&self, rows: &[TableRow]) -> Result<()> {
        let children = self
            .0
            .list_view
            .Items()
            .map_err(|e| to_error("行の取得", e))?;
        self.without_notifying(|_| children.Clear())
            .map_err(|e| to_error("行の消去", e))?;
        self.0.row_items.borrow_mut().clear();

        let columns = self.0.columns.borrow();
        let mut items = Vec::with_capacity(rows.len());
        for row in rows {
            let item = ListViewItem::new().map_err(|e| to_error("ListViewItem の生成", e))?;
            // 見出しと列をそろえるため、余白と高さは行ごとに書く。
            let _ = item.SetPadding(ROW_PADDING);
            let _ = item.SetMinHeight(ROW_MIN_HEIGHT);
            let content = XamlGrid::new().map_err(|e| to_error("行の Grid の生成", e))?;
            apply_columns(&content, &columns)?;
            for (index, column) in columns.iter().enumerate() {
                append_cell(&content, index, row.cell(index), column.align, false)?;
            }
            item.SetContent(&content)
                .map_err(|e| to_error("行への内容設定", e))?;
            let _ = item.SetIsEnabled(row.enabled);

            let element = item
                .cast::<IInspectable>()
                .map_err(|e| to_error("行の要素化", e))?;
            self.without_notifying(|_| children.Append(&element))
                .map_err(|e| to_error("行の追加", e))?;
            items.push(item);
        }
        *self.0.row_items.borrow_mut() = items;
        self.write_selection(&[]);
        Ok(())
    }

    // --------------------------------------------------------------- 選択

    /// 指定された選択を、この表で意味を持つ形にそろえる。
    fn normalize(&self, indices: &[usize]) -> Vec<usize> {
        let rows = self.0.rows.borrow();
        self.0
            .mode
            .get()
            .normalize_by(indices, |i| rows.get(i).is_some_and(|row| row.enabled))
    }

    /// 選択をそのまま行へ書き込む (通知は起きない)。
    fn write_selection(&self, indices: &[usize]) {
        self.without_notifying(|this| {
            for (index, item) in this.0.row_items.borrow().iter().enumerate() {
                let _ = item.SetIsSelected(indices.contains(&index));
            }
        });
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
