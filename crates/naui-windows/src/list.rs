//! リスト (WinUI 3)。
//!
//! WinUI 標準の `ListBox` を `ScrollViewer` に載せて使う。行は `ListBoxItem` で、
//! 中身は透明な `Grid` に載せた `TextBlock` か、任意の組み立て済みウィジェット。
//! 使えない行は `IsEnabled = false`、
//! 複数選択とキーボード操作は `ListBox`、スクロールは外側の `ScrollViewer` が行う。
//!
//! 土台は WinUI 標準の `ListBox`。`ListView` は [`naui_winui3`] の投影に
//! 入っているので、そちらへ移すのは今後の課題。
//!
//! ただし `ListBox` は WinUI 3 でも Fluent 化されていない旧来のコントロールで、
//! 既定のままでは角が四角く、選択した行がアクセント色で全面に塗られる
//! (WinUI 3 の `ListView` とは似ても似つかない見た目になる)。そのため
//! **枠と行の見た目は naui 側で組み直している**。
//!
//! | 部分 | 作り |
//! | --- | --- |
//! | 枠 | 外側の `ScrollViewer` に背景・境界線・角丸を持たせる |
//! | 行 | `ListBoxItem` に差し替えの `ControlTemplate` を当てる |
//! | 選択 | 淡い塗り + 左端のアクセント色のインジケーター |
//!
//! 色・角丸はすべて `{ThemeResource ...}` で引くので、ライト / ダークの
//! 切り替え (ウィンドウの `RequestedTheme`) にそのまま追従する。
//! テーマリソースが引けない環境では、素の `ListBox` に戻して動作を優先する。

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use naui_core::{ListItem, Result, SelectionMode};
use naui_winui3::Microsoft::UI::Xaml::Controls::{
    Grid as XamlGrid, ListBox as XamlListBox, ListBoxItem, Orientation as XamlOrientation,
    ScrollBarVisibility, ScrollViewer, SelectionChangedEventHandler,
    SelectionMode as XamlSelectionMode, StackPanel, TextBlock,
};
use naui_winui3::Microsoft::UI::Xaml::Input::{PointerEventHandler, PointerRoutedEventArgs};
use naui_winui3::Microsoft::UI::Xaml::Markup::XamlReader;
use naui_winui3::Microsoft::UI::Xaml::{ResourceDictionary, Style, UIElement};
use windows::Foundation::PropertyValue;
use windows_core::{IInspectable, Interface, HSTRING};

use crate::layout::ListScrollTarget;
use crate::to_error;
use crate::ui_thread::{HandlerCell, UiThreadCell};
use crate::widgets::{impl_widget, Widget};

/// 行がクリックされたことの通知先。
///
/// WinRT のデリゲートは `Send + Sync` を要求するため `UiThreadCell` に載せる。
/// 呼び出しの間だけクロージャを取り出すので、コールバックの中から
/// 同じ行を操作しても二重借用にならない。
#[derive(Clone)]
struct ActivationHandler(HandlerCell<dyn FnMut()>);

impl ActivationHandler {
    fn new() -> Self {
        Self(Arc::new(UiThreadCell::new(None)))
    }

    fn set(&self, f: impl FnMut() + 'static) {
        self.0.with_mut(|slot| *slot = Some(Box::new(f)));
    }

    fn emit(&self) {
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

/// 1 行の内容。通常の文字行も任意ウィジェット行も同じ型へ正規化する。
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

/// リストへ載せる 1 行。
///
/// [`ListItem`] は文字列だけで済む一覧向けの簡便 API であり、設定画面のように
/// アイコン、複数のラベル、チェックボックス、末尾のボタンなどを組み合わせる
/// 行は `Grid` / `Stack` で作って `ListRow` に包む。
///
/// ```no_run
/// # use naui_windows::{ListRow, Widget};
/// # fn row(content: &dyn Widget) {
/// let row = ListRow::new(content).selectable(false);
/// row.on_activate(|| println!("行がクリックされました"));
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
    /// ウィジェットを 1 行の内容として使う。既定では行全体も選択できる。
    pub fn new(content: &dyn Widget) -> Self {
        Self {
            content: ListRowContent::Custom(content.boxed_clone()),
            selectable: true,
            activation: ActivationHandler::new(),
        }
    }

    /// `ListItem` を通常の文字行へ変換する。`List::set_items` の正規化経路。
    fn from_item(item: ListItem) -> Self {
        let selectable = item.enabled;
        Self {
            content: ListRowContent::Item(item),
            selectable,
            activation: ActivationHandler::new(),
        }
    }

    /// 行内のコントロールだけを操作する行では `false` にする。
    pub fn selectable(mut self, selectable: bool) -> Self {
        self.selectable = selectable;
        self
    }

    pub fn is_selectable(&self) -> bool {
        self.selectable
    }

    /// 行内のラベル・アイコン・余白がクリックされたときに呼ぶ処理。
    ///
    /// チェックボックス、ボタン、入力欄などのコントロールを直接押した場合は
    /// 呼ばれないため、同じ操作が二重に発火しない。
    pub fn on_activate(&self, f: impl FnMut() + 'static) {
        self.activation.set(f);
    }

    /// 行そのものも中身も操作できない行 (`ListItem::enabled(false)`)。
    fn is_disabled_item(&self) -> bool {
        matches!(&self.content, ListRowContent::Item(item) if !item.enabled)
    }
}

/// 一覧の枠。`ListBox` 自身は中身の高さいっぱいに伸びてしまい、角丸も境界線も
/// 見えなくなるため、見えている大きさと一致する外側の `ScrollViewer` に持たせる。
const SURFACE_XAML: &str = r##"<ScrollViewer
    xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
    HorizontalScrollBarVisibility="Disabled"
    VerticalScrollBarVisibility="Auto"
    Background="{ThemeResource ControlFillColorDefaultBrush}"
    BorderBrush="{ThemeResource ControlStrokeColorDefaultBrush}"
    BorderThickness="1"
    CornerRadius="{ThemeResource ControlCornerRadius}"
    Padding="4">
    <ListBox Background="Transparent" BorderThickness="0" Padding="0"
        HorizontalContentAlignment="Stretch"
        ScrollViewer.HorizontalScrollBarVisibility="Disabled"
        ScrollViewer.VerticalScrollBarVisibility="Disabled"/>
</ScrollViewer>"##;

/// 行の見た目。WinUI 3 の `ListView` の行に合わせて、
/// 淡い塗り + 左端のアクセント色のインジケーターで選択を表す。
///
/// 状態の名前は `CommonStates` (ポインター) と `SelectionStates` (選択) に
/// 分けてある。塗る `Border` も分けてあるので、どちらか一方の名前しか
/// 使わないコントロールでも、片方が消えるだけで破綻しない。
///
/// `ROW_STYLE_KEY` は `ROW_STYLE_XAML` の `x:Key` と同じ文字列にする。
const ROW_STYLE_KEY: &str = "NauiListRowStyle";
const ROW_STYLE_XAML: &str = r##"<ResourceDictionary
    xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
    xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml">
    <Style x:Key="NauiListRowStyle" TargetType="ListBoxItem">
        <Setter Property="Background" Value="Transparent"/>
        <Setter Property="Foreground" Value="{ThemeResource TextFillColorPrimaryBrush}"/>
        <Setter Property="Padding" Value="12,8"/>
        <Setter Property="MinHeight" Value="36"/>
        <Setter Property="Margin" Value="0,1"/>
        <Setter Property="HorizontalContentAlignment" Value="Stretch"/>
        <Setter Property="VerticalContentAlignment" Value="Center"/>
        <Setter Property="UseSystemFocusVisuals" Value="True"/>
        <Setter Property="Template">
            <Setter.Value>
                <ControlTemplate TargetType="ListBoxItem">
                    <Grid Background="Transparent">
                        <VisualStateManager.VisualStateGroups>
                            <VisualStateGroup x:Name="CommonStates">
                                <VisualState x:Name="Normal"/>
                                <VisualState x:Name="PointerOver">
                                    <VisualState.Setters>
                                        <Setter Target="Hover.Background"
                                            Value="{ThemeResource SubtleFillColorSecondaryBrush}"/>
                                    </VisualState.Setters>
                                </VisualState>
                                <VisualState x:Name="Pressed">
                                    <VisualState.Setters>
                                        <Setter Target="Hover.Background"
                                            Value="{ThemeResource SubtleFillColorTertiaryBrush}"/>
                                    </VisualState.Setters>
                                </VisualState>
                                <VisualState x:Name="Disabled">
                                    <VisualState.Setters>
                                        <!-- 主・副の 2 行をまとめて薄くする。
                                            副次テキストは自分の色を持つので、
                                            文字色の差し替えでは片方しか効かない。 -->
                                        <Setter Target="Content.Opacity" Value="0.4"/>
                                    </VisualState.Setters>
                                </VisualState>
                            </VisualStateGroup>
                            <VisualStateGroup x:Name="SelectionStates">
                                <VisualState x:Name="Unselected"/>
                                <VisualState x:Name="Selected">
                                    <VisualState.Setters>
                                        <Setter Target="Fill.Background"
                                            Value="{ThemeResource SubtleFillColorSecondaryBrush}"/>
                                        <Setter Target="Indicator.Opacity" Value="1"/>
                                    </VisualState.Setters>
                                </VisualState>
                                <VisualState x:Name="SelectedUnfocused">
                                    <VisualState.Setters>
                                        <Setter Target="Fill.Background"
                                            Value="{ThemeResource SubtleFillColorSecondaryBrush}"/>
                                        <Setter Target="Indicator.Opacity" Value="1"/>
                                    </VisualState.Setters>
                                </VisualState>
                                <VisualState x:Name="SelectedPointerOver">
                                    <VisualState.Setters>
                                        <Setter Target="Fill.Background"
                                            Value="{ThemeResource SubtleFillColorTertiaryBrush}"/>
                                        <Setter Target="Indicator.Opacity" Value="1"/>
                                    </VisualState.Setters>
                                </VisualState>
                                <VisualState x:Name="SelectedPressed">
                                    <VisualState.Setters>
                                        <Setter Target="Fill.Background"
                                            Value="{ThemeResource SubtleFillColorTertiaryBrush}"/>
                                        <Setter Target="Indicator.Opacity" Value="1"/>
                                    </VisualState.Setters>
                                </VisualState>
                                <VisualState x:Name="SelectedDisabled">
                                    <VisualState.Setters>
                                        <Setter Target="Fill.Background"
                                            Value="{ThemeResource SubtleFillColorSecondaryBrush}"/>
                                        <Setter Target="Content.Opacity" Value="0.4"/>
                                        <Setter Target="Indicator.Opacity" Value="0.4"/>
                                    </VisualState.Setters>
                                </VisualState>
                            </VisualStateGroup>
                        </VisualStateManager.VisualStateGroups>
                        <Border x:Name="Hover" Background="Transparent"
                            CornerRadius="{ThemeResource ControlCornerRadius}"/>
                        <Border x:Name="Fill" Background="Transparent"
                            CornerRadius="{ThemeResource ControlCornerRadius}"/>
                        <Border x:Name="Indicator" Width="3" Height="16" Opacity="0"
                            Margin="1,0,0,0" CornerRadius="1.5"
                            HorizontalAlignment="Left" VerticalAlignment="Center"
                            Background="{ThemeResource AccentFillColorDefaultBrush}"/>
                        <ContentPresenter x:Name="Content"
                            Content="{TemplateBinding Content}"
                            ContentTemplate="{TemplateBinding ContentTemplate}"
                            Foreground="{TemplateBinding Foreground}"
                            Padding="{TemplateBinding Padding}"
                            HorizontalAlignment="{TemplateBinding HorizontalContentAlignment}"
                            VerticalAlignment="{TemplateBinding VerticalContentAlignment}"/>
                    </Grid>
                </ControlTemplate>
            </Setter.Value>
        </Setter>
    </Style>
</ResourceDictionary>"##;

/// 行の中身を載せる器。`Background="Transparent"` にすると、文字の無い余白でも
/// ポインターを受け取れる (行そのもののクリックを拾うため)。
const ROW_HOST_XAML: &str = r##"<Grid
    xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
    Background="Transparent"/>"##;

/// 行の副次テキスト。色は Fluent の副次テキスト用テーマリソースから引く。
const DETAIL_XAML: &str = r##"<TextBlock
    xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
    FontSize="12" Foreground="{ThemeResource TextFillColorSecondaryBrush}"/>"##;

/// テーマ付きの枠を読み込む。読めなければ素の `ScrollViewer` + `ListBox` に戻す。
pub(crate) fn build_surface() -> Result<(ScrollViewer, XamlListBox)> {
    match load_surface() {
        Ok(surface) => Ok(surface),
        Err(error) => {
            eprintln!("naui-windows: リストのテーマ付き枠の生成に失敗: {error}");
            plain_surface()
        }
    }
}

fn load_surface() -> Result<(ScrollViewer, XamlListBox)> {
    let native = XamlReader::Load(&HSTRING::from(SURFACE_XAML))
        .and_then(|element| element.cast::<ScrollViewer>())
        .map_err(|e| to_error("List の枠の生成", e))?;
    let list_box = native
        .Content()
        .and_then(|content| content.cast::<XamlListBox>())
        .map_err(|e| to_error("List の ListBox の取得", e))?;
    Ok((native, list_box))
}

fn plain_surface() -> Result<(ScrollViewer, XamlListBox)> {
    let list_box = XamlListBox::new().map_err(|e| to_error("ListBox の生成", e))?;
    let native = ScrollViewer::new().map_err(|e| to_error("List の ScrollViewer 生成", e))?;
    let element = list_box
        .cast::<IInspectable>()
        .map_err(|e| to_error("ListBox の要素化", e))?;
    native
        .SetContent(&element)
        .map_err(|e| to_error("List の ScrollViewer への追加", e))?;
    Ok((native, list_box))
}

/// 行に当てる `Style`。読めなければ `None` (WinUI 既定の見た目のまま)。
pub(crate) fn row_style() -> Option<Style> {
    let dictionary = XamlReader::Load(&HSTRING::from(ROW_STYLE_XAML))
        .and_then(|element| element.cast::<ResourceDictionary>())
        .map_err(|e| to_error("行のスタイルの生成", e));
    let dictionary = match dictionary {
        Ok(dictionary) => dictionary,
        Err(error) => {
            eprintln!("naui-windows: リストの行のスタイルの生成に失敗: {error}");
            return None;
        }
    };
    PropertyValue::CreateString(&HSTRING::from(ROW_STYLE_KEY))
        .and_then(|key| dictionary.Lookup(&key))
        .and_then(|style| style.cast::<Style>())
        .ok()
}

/// 選ばれている行の位置を受け取る通知。
pub(crate) type SelectionCallback = dyn FnMut(&[usize]);

/// 選択が変わったことの通知先。
///
/// WinRT のデリゲートは `Send + Sync` を要求するため `UiThreadCell` に載せる。
/// 呼び出しの間だけクロージャを取り出すので、コールバックの中から
/// 同じリストを操作しても二重借用にならない。
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

struct ListInner {
    native: ScrollViewer,
    list_box: XamlListBox,
    _wheel: Rc<ListScrollTarget>,
    /// 行そのもの。選択の読み書きはここを通す。
    native_rows: RefCell<Vec<ListBoxItem>>,
    /// 行のモデル。選べるかどうかも activation もここから引く。
    /// 行に含まれるコールバック等を生かしておく役目も持つ。
    rows: RefCell<Vec<ListRow>>,
    mode: Cell<SelectionMode>,
    handler: SelectionHandler,
    /// プログラムから選択を変えている間だけ通知を止める。
    /// `IsSelected` の書き換えでも `SelectionChanged` が起きるため。
    silent: Rc<Cell<bool>>,
    /// ウィンドウ全体のホイール補助が List の ScrollViewer を選ぶための状態。
    hovered: Arc<UiThreadCell<usize>>,
    /// 行に当てる見た目。読めなかったときだけ `None`。
    row_style: Option<Style>,
}

/// 縦に並ぶ選択できる一覧 (ListBox)。
///
/// 高さは `ListBox` 自身が持つが、行数に関係なく固定したいときは
/// `set_sizing` で指定する。
#[derive(Clone)]
pub struct List(Rc<ListInner>);
impl_widget!(List, native);

impl List {
    pub(crate) fn new() -> Result<Self> {
        let (native, list_box) = build_surface()?;
        list_box
            .SetSelectionMode(XamlSelectionMode::Single)
            .map_err(|e| to_error("ListBox の選択方法の設定", e))?;
        // スクロールは外側の ScrollViewer に任せるため、ListBox の
        // テンプレート内にある ScrollViewer は二重スクロールさせない。
        let _ = ScrollViewer::SetHorizontalScrollBarVisibility2(
            &list_box,
            ScrollBarVisibility::Disabled,
        );
        let _ =
            ScrollViewer::SetVerticalScrollBarVisibility2(&list_box, ScrollBarVisibility::Disabled);
        let _ = native.SetHorizontalScrollBarVisibility(ScrollBarVisibility::Disabled);
        let _ = native.SetVerticalScrollBarVisibility(ScrollBarVisibility::Auto);
        let hovered = Arc::new(UiThreadCell::new(0));
        let wheel = crate::layout::register_list_scroll(native.clone(), hovered.clone());

        let this = Self(Rc::new(ListInner {
            native,
            list_box,
            _wheel: wheel,
            native_rows: RefCell::new(Vec::new()),
            rows: RefCell::new(Vec::new()),
            mode: Cell::new(SelectionMode::Single),
            handler: SelectionHandler::new(),
            silent: Rc::new(Cell::new(false)),
            hovered,
            row_style: row_style(),
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
                        let indices = list.selection();
                        // 任意内容の設定行など、行全体を選ばない項目へ付いた
                        // ListBox の選択は即座に戻す。子の Button 等は無効にしない。
                        list.write_selection(&indices);
                        list.0.handler.emit(&indices);
                    }
                }
            });
            Ok(())
        });
        this.0
            .list_box
            .SelectionChanged(&handler)
            .map_err(|e| to_error("ListBox の購読", e))?;

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
            .map_err(|e| to_error("ListBox のポインター購読", e))?;
        this.0
            .native
            .PointerExited(&exited)
            .map_err(|e| to_error("ListBox のポインター購読", e))?;

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
            .map_err(|e| to_error("ListBox のポインター購読", e))?;
        Ok(this)
    }

    /// 行を作り直す。インデックスの意味が変わるため、選択は外れる。
    pub fn set_items(&self, items: &[ListItem]) {
        let rows: Vec<ListRow> = items.iter().cloned().map(ListRow::from_item).collect();
        self.set_rows(&rows);
    }

    /// 行を作り直す。通常の文字行も任意内容の行もこの経路で組み立てる。
    ///
    /// 行内のコントロールは通常どおりそれぞれのコールバックを持てる。
    /// インデックスの意味が変わるため、選択は外れる。
    pub fn set_rows(&self, rows: &[ListRow]) {
        let _ = self.rebuild(rows);
    }

    fn rebuild(&self, rows: &[ListRow]) -> Result<()> {
        let children = self
            .0
            .list_box
            .Items()
            .map_err(|e| to_error("行の取得", e))?;
        self.release_row_contents();
        self.without_notifying(|_| children.Clear())
            .map_err(|e| to_error("行の消去", e))?;
        self.0.native_rows.borrow_mut().clear();

        let mut native_rows = Vec::with_capacity(rows.len());
        for row in rows {
            let native = ListBoxItem::new().map_err(|e| to_error("ListBoxItem の生成", e))?;
            if let Some(style) = self.0.row_style.as_ref() {
                let _ = native.SetStyle(style);
            }
            let host = row_host(row)?;
            native
                .SetContent(&host)
                .map_err(|e| to_error("行への内容設定", e))?;
            // 選べないだけの行は中のコントロールを押せる必要があるので、
            // 無効にするのは `ListItem::enabled(false)` の行だけ。
            let _ = native.SetIsEnabled(!row.is_disabled_item());

            let element = native
                .cast::<IInspectable>()
                .map_err(|e| to_error("行の要素化", e))?;
            self.without_notifying(|_| children.Append(&element))
                .map_err(|e| to_error("行の追加", e))?;
            native_rows.push(native);
        }
        *self.0.native_rows.borrow_mut() = native_rows;
        *self.0.rows.borrow_mut() = rows.to_vec();
        self.write_selection(&[]);
        Ok(())
    }

    /// いま並んでいる行の器を空にする。
    ///
    /// 任意内容の行は、アプリが持っているウィジェットをそのまま器 (`Grid`) へ
    /// 載せている。載せたまま器を手放すと、そのウィジェットは「親がある」
    /// ままになり、次に組み直すときに
    /// 「Element is already the child of another element」で弾かれる。
    /// 器を捨てる前に、ここで必ず外しておく。
    fn release_row_contents(&self) {
        for native in self.0.native_rows.borrow().iter() {
            let Ok(host) = native
                .Content()
                .and_then(|content| content.cast::<XamlGrid>())
            else {
                continue;
            };
            if let Ok(children) = host.Children() {
                let _ = children.Clear();
            }
        }
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
    /// `Multiple` はクリックのたびに反転する挙動で、macOS / Web と揃わないため。
    pub fn set_selection_mode(&self, mode: SelectionMode) {
        self.0.mode.set(mode);
        let native = if mode.is_multiple() {
            XamlSelectionMode::Extended
        } else {
            XamlSelectionMode::Single
        };
        let _ = self.without_notifying(|this| this.0.list_box.SetSelectionMode(native));
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
        let rows = self.0.rows.borrow();
        self.0
            .native_rows
            .borrow()
            .iter()
            .enumerate()
            .filter(|(index, native)| {
                rows.get(*index).is_some_and(ListRow::is_selectable)
                    && native.IsSelected().unwrap_or(false)
            })
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
    /// ([`SelectionMode::normalize`])。
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

    /// 中身の `ListBox`。バックエンド固有の脱出口として公開している。
    pub fn native_list_box(&self) -> XamlListBox {
        self.0.list_box.clone()
    }

    fn normalize(&self, indices: &[usize]) -> Vec<usize> {
        let rows = self.0.rows.borrow();
        self.0.mode.get().normalize_by(indices, |index| {
            rows.get(index).is_some_and(ListRow::is_selectable)
        })
    }

    /// 選択をそのまま行へ書き込む (通知は起きない)。
    fn write_selection(&self, indices: &[usize]) {
        self.without_notifying(|this| {
            for (index, native) in this.0.native_rows.borrow().iter().enumerate() {
                let _ = native.SetIsSelected(indices.contains(&index));
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

impl Drop for ListInner {
    fn drop(&mut self) {
        self.hovered.with_mut(|hovered| *hovered = 0);
    }
}

/// 行の中身を、押されたことを拾える器に載せる。
///
/// 器の `Background` は透明なので、文字の無い余白で押しても
/// `PointerPressed` が届く。ボタン・チェックボックス・入力欄はこの
/// イベントを自分で処理して止めるため、行の activation とは二重に
/// 発火しない (ほかの環境と同じ切り分け)。
fn row_host(row: &ListRow) -> Result<XamlGrid> {
    let host = match XamlReader::Load(&HSTRING::from(ROW_HOST_XAML))
        .and_then(|element| element.cast::<XamlGrid>())
    {
        Ok(host) => host,
        // 透明な背景を持てないと余白では押せなくなるが、
        // 文字やアイコンの上では届くので動作を優先する。
        Err(_) => XamlGrid::new().map_err(|e| to_error("行の器の生成", e))?,
    };
    let children = host.Children().map_err(|e| to_error("行の器の取得", e))?;
    children
        .Append(&row_content(row)?)
        .map_err(|e| to_error("行への追加", e))?;

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
    Ok(host)
}

/// 押されたのが主ボタン (左クリック・タッチ・ペン) か。
fn is_primary_press(args: Option<&PointerRoutedEventArgs>) -> bool {
    let Some(args) = args else {
        return false;
    };
    args.GetCurrentPoint(Option::<&UIElement>::None)
        .and_then(|point| point.Properties())
        .and_then(|properties| properties.IsLeftButtonPressed())
        .unwrap_or(false)
}

/// 1 行分の中身を作る。
///
/// 文字行は `detail` が無ければ `TextBlock` 1 つ、あれば縦の `StackPanel` に
/// 2 つ入れる。縦の位置合わせは WinUI のレイアウトパスが行うので、naui は
/// 組むだけ。任意内容の行は、組み立て済みのウィジェットをそのまま使う。
fn row_content(row: &ListRow) -> Result<UIElement> {
    let item = match &row.content {
        ListRowContent::Custom(content) => return Ok(content.native_element()),
        ListRowContent::Item(item) => item,
    };
    let title = text_block(&item.label, false)?;
    let Some(detail) = &item.detail else {
        return title
            .cast::<UIElement>()
            .map_err(|e| to_error("行の要素化", e));
    };

    let panel = StackPanel::new().map_err(|e| to_error("行の StackPanel の生成", e))?;
    panel
        .SetOrientation(XamlOrientation::Vertical)
        .map_err(|e| to_error("行の向き設定", e))?;
    let children = panel
        .Children()
        .map_err(|e| to_error("行の中身の取得", e))?;
    children
        .Append(
            &title
                .cast::<UIElement>()
                .map_err(|e| to_error("行の要素化", e))?,
        )
        .map_err(|e| to_error("行への追加", e))?;
    let sub = text_block(detail, true)?;
    children
        .Append(
            &sub.cast::<UIElement>()
                .map_err(|e| to_error("行の要素化", e))?,
        )
        .map_err(|e| to_error("行への追加", e))?;
    panel
        .cast::<UIElement>()
        .map_err(|e| to_error("行の要素化", e))
}

/// 行に載せる 1 本の文字。`secondary` なら Fluent の副次テキストに合わせる。
///
/// 主テキストの色は行 (`ListBoxItem`) から受け継ぐので指定しない。
/// 副次テキストだけは、テーマに追従する色を `{ThemeResource}` で引くために
/// XAML から作る。引けなければ濃さを下げるだけの見た目に落とす。
pub(crate) fn text_block(text: &str, secondary: bool) -> Result<TextBlock> {
    let block = if secondary {
        match XamlReader::Load(&HSTRING::from(DETAIL_XAML))
            .and_then(|element| element.cast::<TextBlock>())
        {
            Ok(block) => block,
            Err(_) => {
                let block = TextBlock::new().map_err(|e| to_error("行ラベルの生成", e))?;
                let _ = block.SetFontSize(12.0);
                let _ = block.SetOpacity(0.7);
                block
            }
        }
    } else {
        TextBlock::new().map_err(|e| to_error("行ラベルの生成", e))?
    };
    block
        .SetText(&HSTRING::from(text))
        .map_err(|e| to_error("行ラベルの設定", e))?;
    Ok(block)
}
