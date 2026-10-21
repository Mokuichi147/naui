//! リスト (WinUI 3)。
//!
//! WinUI 標準の `ListView` をそのまま使う。行は `ListViewItem` で、中身は
//! 透明な `Grid` に載せた `TextBlock` か、任意の組み立て済みウィジェット。
//! 使えない行は `IsEnabled = false`、複数選択とキーボード操作は `ListView` が
//! 行う。
//!
//! 行の見た目 (角丸・淡い塗り・左端のアクセント色のインジケーター) は
//! `ListView` の標準テンプレートが持っている。枠だけは付いていないので、
//! `List` と `Table` と `Tree` で同じ色・角丸の `Border` で囲む。
//! 色はすべて `{ThemeResource ...}` で引くので、ライト / ダークの切り替え
//! (ウィンドウの `RequestedTheme`) にそのまま追従する。テーマリソースが
//! 引けない環境では、素の `Border` に戻して動作を優先する。
//!
//! スクロールは `ListView` が自分で持つ (中の一覧は与えられた高さぶんしか
//! 並べないので、外側の `ScrollViewer` へ預けると伸びずに切れてしまう)。
//! ウィンドウ全体のホイール補助 ([`crate::layout`]) には、テンプレートの
//! 中にある `ScrollViewer` を見つけて登録する。
//!
//! 行の起動は `ListViewItem` の `DoubleTapped` と、Enter の `KeyDown` で受ける。
//! 右クリックは `ContextRequested` で受ける。こちらは WinUI が右クリックだけで
//! なく、タッチの長押し・Shift + F10・アプリケーションキーもまとめて出す
//! 文脈メニューの要求で、キー操作のときは位置が無いので行の左端を渡す。

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use naui_core::{
    Align, ListItem, Orientation, Point, Result, SelectionMode, Sizing, TextColor, TextStyle,
};
use naui_winui3::Microsoft::UI::Xaml::Controls::{
    Border, Control, Grid as XamlGrid, ListView, ListViewItem, ListViewSelectionMode,
    ScrollBarVisibility, ScrollViewer, SelectionChangedEventHandler, TextBlock,
};
use naui_winui3::Microsoft::UI::Xaml::Input::{
    ContextRequestedEventArgs, DoubleTappedEventHandler, KeyEventHandler, PointerEventHandler,
    PointerRoutedEventArgs,
};
use naui_winui3::Microsoft::UI::Xaml::Markup::XamlReader;
use naui_winui3::Microsoft::UI::Xaml::Media::VisualTreeHelper;
use naui_winui3::Microsoft::UI::Xaml::{
    DependencyObject, FrameworkElement, RoutedEventHandler, Thickness, UIElement,
};
use windows::Foundation::{Point as XamlPoint, TypedEventHandler};
use windows::System::VirtualKey;
use windows_core::{IInspectable, Interface, HSTRING};

use crate::layout::ListScrollTarget;
use crate::to_error;
use crate::ui_thread::{HandlerCell, UiThreadCell};
use crate::widgets::{impl_widget, label_style, Label, SelectHandler, Stack, Widget};

/// 行がクリックされたことの通知先。
///
/// WinRT のデリゲートは `Send + Sync` を要求するため `UiThreadCell` に載せる。
/// 呼び出しの間だけクロージャを取り出すので、コールバックの中から
/// 同じ行を操作しても二重借用にならない。
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

/// 行の余白。`ListView` の既定は縦が 0 で、`label` と `detail` の 2 行を
/// 載せると詰まって見えるため、上下を空ける。横は既定と同じ 12。
/// (`Table` は見出しと列をそろえる必要があるので、同じ値を自分で持つ。)
const ROW_PADDING: Thickness = Thickness {
    Left: 12.0,
    Top: 8.0,
    Right: 12.0,
    Bottom: 8.0,
};

/// 一覧の枠。`ListView` は枠を持たないので、`Table` と `Tree` と同じ色・
/// 角丸の `Border` で囲む。
const SURFACE_XAML: &str = r##"<Border
    xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
    Background="{ThemeResource ControlFillColorDefaultBrush}"
    BorderBrush="{ThemeResource ControlStrokeColorDefaultBrush}"
    BorderThickness="1"
    CornerRadius="{ThemeResource ControlCornerRadius}"
    Padding="4">
    <ListView Background="Transparent" BorderThickness="0" Padding="0"
        HorizontalContentAlignment="Stretch"/>
</Border>"##;

/// 行の中身を載せる器。`Background="Transparent"` にすると、文字の無い余白でも
/// ポインターを受け取れる (行そのもののクリックを拾うため)。
const ROW_HOST_XAML: &str = r##"<Grid
    xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
    Background="Transparent"/>"##;

/// テーマ付きの枠を読み込む。読めなければ素の `Border` + `ListView` に戻す。
fn build_surface() -> Result<(Border, ListView)> {
    match load_surface() {
        Ok(surface) => Ok(surface),
        Err(error) => {
            eprintln!("naui-windows: リストのテーマ付き枠の生成に失敗: {error}");
            plain_surface()
        }
    }
}

fn load_surface() -> Result<(Border, ListView)> {
    let native = XamlReader::Load(&HSTRING::from(SURFACE_XAML))
        .and_then(|element| element.cast::<Border>())
        .map_err(|e| to_error("List の枠の生成", e))?;
    let list_view = native
        .Child()
        .and_then(|child| child.cast::<ListView>())
        .map_err(|e| to_error("List の ListView の取得", e))?;
    Ok((native, list_view))
}

fn plain_surface() -> Result<(Border, ListView)> {
    let list_view = ListView::new().map_err(|e| to_error("ListView の生成", e))?;
    let native = Border::new().map_err(|e| to_error("List の Border 生成", e))?;
    native
        .SetChild(&list_view)
        .map_err(|e| to_error("List の Border への追加", e))?;
    Ok((native, list_view))
}

/// 右クリックされた行と位置を受け取る通知。
type ContextMenuCallback = dyn FnMut(usize, Point);

/// 右クリックの通知先。呼び出しの間だけクロージャを取り出す。
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
    native: Border,
    list_view: ListView,
    /// ホイール補助への登録。テンプレートの `ScrollViewer` が現れるまで
    /// 決まらないので、`Loaded` のあとで入る。
    wheel: RefCell<Option<Rc<ListScrollTarget>>>,
    /// 行そのもの。選択の読み書きはここを通す。
    native_rows: RefCell<Vec<ListViewItem>>,
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
    /// ダブルクリックと Enter の通知先。
    row_activate: SelectHandler,
    /// 右クリックの通知先。
    context_menu: ContextMenuHandler,
}

/// 縦に並ぶ選択できる一覧 (`ListView`)。
///
/// 高さは `ListView` 自身が持つが、行数に関係なく固定したいときは
/// `set_sizing` で指定する。
#[derive(Clone)]
pub struct List(Rc<ListInner>);
impl_widget!(List, native);

impl List {
    pub(crate) fn new() -> Result<Self> {
        let (native, list_view) = build_surface()?;
        list_view
            .SetSelectionMode(ListViewSelectionMode::Single)
            .map_err(|e| to_error("ListView の選択方法の設定", e))?;
        // 横スクロールは持たせない (行は幅いっぱいに広げる)。
        let _ = ScrollViewer::SetHorizontalScrollBarVisibility2(
            &list_view,
            ScrollBarVisibility::Disabled,
        );
        let hovered = Arc::new(UiThreadCell::new(0));

        let this = Self(Rc::new(ListInner {
            native,
            list_view,
            wheel: RefCell::new(None),
            native_rows: RefCell::new(Vec::new()),
            rows: RefCell::new(Vec::new()),
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
            .list_view
            .SelectionChanged(&handler)
            .map_err(|e| to_error("ListView の購読", e))?;
        this.install_wheel_target()?;

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
            .map_err(|e| to_error("List のポインター購読", e))?;
        this.0
            .native
            .PointerExited(&exited)
            .map_err(|e| to_error("List のポインター購読", e))?;

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
            .map_err(|e| to_error("List のポインター購読", e))?;
        Ok(this)
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
                    List(inner).register_wheel_target();
                }
            });
            Ok(())
        });
        self.0
            .list_view
            .Loaded(&loaded)
            .map_err(|e| to_error("List の表示の購読", e))?;
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
            .list_view
            .Items()
            .map_err(|e| to_error("行の取得", e))?;
        self.release_row_contents();
        self.without_notifying(|_| children.Clear())
            .map_err(|e| to_error("行の消去", e))?;
        self.0.native_rows.borrow_mut().clear();

        let mut native_rows = Vec::with_capacity(rows.len());
        for (index, row) in rows.iter().enumerate() {
            let native = ListViewItem::new().map_err(|e| to_error("ListViewItem の生成", e))?;
            let _ = native.SetPadding(ROW_PADDING);
            let host = row_host(row)?;
            native
                .SetContent(&host)
                .map_err(|e| to_error("行への内容設定", e))?;
            // 選べないだけの行は中のコントロールを押せる必要があるので、
            // 無効にするのは `ListItem::enabled(false)` の行だけ。
            let _ = native.SetIsEnabled(!row.is_disabled_item());
            // 無効の行には入力が届かないので、起動も文脈メニューも出ない。
            self.listen_row_events(&native, index)?;

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

    /// 行の起動 (ダブルクリック・Enter) と文脈メニューの要求を受ける。
    ///
    /// 行は `set_rows` のたびに作り直すので、インデックスは作るときの位置でよい。
    /// 購読は `List` を弱参照で持つ (行 → 購読 → `List` → 行 の循環を作らない)。
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
            .map_err(|e| to_error("行のダブルクリックの購読", e))?;

        let activate = self.0.row_activate.clone();
        let keyed = KeyEventHandler::new(move |sender, args| {
            let Some(args) = args.as_ref() else {
                return Ok(());
            };
            // 行の中の入力欄やボタンが受けた Enter は、そのコントロールのもの。
            // 行そのものにフォーカスがあるときだけ起動にする。
            let on_row = match (args.OriginalSource(), sender.as_ref()) {
                (Ok(source), Some(sender)) => &source == sender,
                _ => false,
            };
            let modifiers = crate::keys::modifiers();
            let modified = modifiers.shift || modifiers.control || modifiers.alt || modifiers.meta;
            if !on_row || args.Key().unwrap_or(VirtualKey::None) != VirtualKey::Enter || modified {
                return Ok(());
            }
            let _ = args.SetHandled(true);
            activate.emit(index);
            Ok(())
        });
        native
            .KeyDown(&keyed)
            .map_err(|e| to_error("行のキー操作の購読", e))?;

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
            .map_err(|e| to_error("行の文脈メニューの購読", e))?;
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

    /// `index` 行目が見えるところまでスクロールする。選択は変わらない。
    ///
    /// すでに見えていれば動かさず、見えていなければ近いほうの端に合わせる
    /// (上にあれば上端、下にあれば下端)。範囲外なら何もしない。
    pub fn scroll_to_row(&self, index: usize) {
        let item = self.0.native_rows.borrow().get(index).cloned();
        if let Some(item) = item.and_then(|item| item.cast::<windows_core::IInspectable>().ok()) {
            let _ = self.0.list_view.ScrollIntoView(&item);
        }
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

    /// 行が起動されたとき (ダブルクリック、または行にフォーカスがある状態での
    /// Enter) に、その行のインデックスで呼ばれる。
    ///
    /// 複数選択のすべてを開きたいときは、中で [`List::selection`] を読む。
    /// 選べない `ListItem` (`enabled(false)`) の行では呼ばれない。
    pub fn on_row_activate(&self, f: impl FnMut(usize) + 'static) {
        self.0.row_activate.set(f);
    }

    /// 行が右クリック (タッチの長押し・Shift + F10・アプリケーションキーを含む)
    /// されたときに、その行のインデックスと位置で呼ばれる。
    ///
    /// 位置はこのリストの左上からの論理ピクセルで、[`crate::PopupMenu::open_at`]
    /// へそのまま渡せる。キー操作のときは行の左端 (縦は中央) になる。
    /// 選択は変えない。行の無い余白では呼ばれない。
    pub fn on_context_menu(&self, f: impl FnMut(usize, Point) + 'static) {
        self.0.context_menu.set(f);
    }

    /// 中身の `ListView`。バックエンド固有の脱出口として公開している。
    pub fn native_list_view(&self) -> ListView {
        self.0.list_view.clone()
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

/// 文脈メニューの位置を、`list` (List の外枠) の左上からの座標で求める。
///
/// ポインターで求められたときはその位置、キー操作で位置が無いときは
/// 行の左端の縦の中央。
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

/// 行の中で押された `source` が、自分で入力を受けるコントロールの上か。
///
/// `source` から行 (`row`) の手前まで親へたどり、有効でフォーカスを受ける
/// `Control` (ボタン・チェックボックス・入力欄など) があれば真。
/// `ScrollViewer` や `ProgressBar` のようにフォーカスを受けないものは行の
/// 一部として扱う。`PointerPressed` を止めるかどうかの切り分けにそろえている。
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
/// 任意内容の行は、組み立て済みのウィジェットをそのまま使う。
fn row_content(row: &ListRow) -> Result<UIElement> {
    match &row.content {
        ListRowContent::Custom(content) => Ok(content.native_element()),
        ListRowContent::Item(item) => {
            Ok(item_content(&item.label, item.detail.as_deref())?.native_element())
        }
    }
}

/// ラベルと補助の文字の間隔。
const DETAIL_SPACING: f64 = 2.0;

/// 行に載せる 1 本の文字。`secondary` なら `List` の補助と同じ見た目にする。
///
/// naui の `Label` を使えない場所 (表のセルのように文字揃えと折り返しを
/// 直に決めるところ) 向け。見た目の値は [`TextStyle::Caption`] と
/// [`TextColor::Secondary`] から引くので、ここには書かない。
pub(crate) fn text_block(text: &str, secondary: bool) -> Result<TextBlock> {
    let block = TextBlock::new().map_err(|e| to_error("行ラベルの生成", e))?;
    if secondary {
        if let Some(style) = label_style(TextStyle::Caption, TextColor::Secondary) {
            let _ = block.SetStyle(&style);
        }
    }
    block
        .SetText(&HSTRING::from(text))
        .map_err(|e| to_error("行ラベルの設定", e))?;
    Ok(block)
}

/// 文字だけの行の中身。
///
/// 副次テキストの小ささと淡さは [`TextStyle::Caption`] と
/// [`TextColor::Secondary`] が決めるので、ここに級数もブラシも書かない。
/// 主テキストの色は指定しない (行の `ListViewItem` から受け継ぐ)。
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

/// 行に載せる 1 本の文字。
///
/// **行の幅いっぱいに広げる。** `Label` は `TextTrimming="CharacterEllipsis"`
/// で末尾を切るが、それが効くのは幅が決まってから。
fn row_label(text: &str) -> Result<Label> {
    let label = Label::new(text)?;
    label.set_sizing(Sizing::fill_width());
    Ok(label)
}
