//! Lists (WinUI 3).
//!
//! WinUI's standard `ListView` is used as is. Rows are `ListViewItem`s, holding
//! either a `TextBlock` placed in a transparent `Grid` or any prebuilt widget. Rows
//! that cannot be used get `IsEnabled = false`, and multiple selection and keyboard
//! navigation are done by `ListView`.
//!
//! The row look (rounded corners, light fill, the accent-colored indicator on the
//! left) comes from `ListView`'s standard template. Only a frame is missing, so
//! `List`, `Table` and `Tree` are surrounded by a `Border` with the same color and
//! rounded corners. All colors are taken with `{ThemeResource ...}`, so they follow
//! light / dark switching (the window's `RequestedTheme`) as is. Where theme
//! resources cannot be obtained, it falls back to a plain `Border`, prioritizing
//! function.
//!
//! Scrolling is owned by the `ListView` itself (the inner list lays out only as many
//! rows as fit its given height, so entrusting it to an outer `ScrollViewer` would
//! cut it off instead of growing). For the window-wide wheel helper
//! ([`crate::layout`]), the `ScrollViewer` inside the template is found and
//! registered.
//!
//! Row activation is received through the `ListViewItem`'s `DoubleTapped` and
//! Enter's `KeyDown`. Right-clicks are received through `ContextRequested`, the
//! context menu request WinUI fires not only for right-clicks but also for touch
//! long-press, Shift + F10 and the application key; for key operations there is no
//! position, so the row's left edge is passed.

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

/// The holder for a row's content. With `Background="Transparent"`, it receives the
/// pointer even in empty space without text (to pick up clicks on the row itself).
const ROW_HOST_XAML: &str = r##"<Grid
    xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
    Background="Transparent"/>"##;

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

fn plain_surface() -> Result<(Border, ListView)> {
    let list_view = ListView::new().map_err(|e| to_error("create the ListView", e))?;
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

struct ListInner {
    native: Border,
    list_view: ListView,
    /// The registration with the wheel helper. It cannot be decided until the
    /// template's `ScrollViewer` appears, so it is made after `Loaded`.
    wheel: RefCell<Option<Rc<ListScrollTarget>>>,
    /// The rows themselves. Reading and writing the selection goes through here.
    native_rows: RefCell<Vec<ListViewItem>>,
    /// The row models. Whether they can be selected and their activation are looked up
    /// from here. It also keeps the callbacks and the like contained in the rows alive.
    rows: RefCell<Vec<ListRow>>,
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
/// The `ListView` itself owns its height, but to fix it regardless of the number of
/// rows, specify it with `set_sizing`.
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
            .map_err(|e| to_error("subscribe to the ListView", e))?;
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

    /// Registers the `ScrollViewer` inside the template as a destination of the wheel
    /// helper.
    ///
    /// The `ListView`'s contents are not assembled until `Loaded`, so it waits for that.
    /// If it is not found it simply does not register; the control's own scrolling still
    /// works.
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
        let Some(scroll) = crate::layout::scroll_viewer_within(&self.0.list_view) else {
            return;
        };
        let target = crate::layout::register_list_scroll(scroll, self.0.hovered.clone());
        *self.0.wheel.borrow_mut() = Some(target);
    }

    /// Rebuilds the rows. Indices change meaning, so the selection is cleared.
    pub fn set_items(&self, items: &[ListItem]) {
        let rows: Vec<ListRow> = items.iter().cloned().map(ListRow::from_item).collect();
        self.set_rows(&rows);
    }

    /// Rebuilds the rows. Both plain text rows and custom-content rows are built
    /// through this path.
    ///
    /// Controls inside a row can have their own callbacks as usual. Indices change
    /// meaning, so the selection is cleared.
    pub fn set_rows(&self, rows: &[ListRow]) {
        let _ = self.rebuild(rows);
    }

    fn rebuild(&self, rows: &[ListRow]) -> Result<()> {
        let children = self
            .0
            .list_view
            .Items()
            .map_err(|e| to_error("get the rows", e))?;
        self.release_row_contents();
        self.without_notifying(|_| children.Clear())
            .map_err(|e| to_error("clear the rows", e))?;
        self.0.native_rows.borrow_mut().clear();

        let mut native_rows = Vec::with_capacity(rows.len());
        for (index, row) in rows.iter().enumerate() {
            let native = ListViewItem::new().map_err(|e| to_error("create the ListViewItem", e))?;
            let _ = native.SetPadding(ROW_PADDING);
            let host = row_host(row)?;
            native
                .SetContent(&host)
                .map_err(|e| to_error("set the row content", e))?;
            // 選べないだけの行は中のコントロールを押せる必要があるので、
            // 無効にするのは `ListItem::enabled(false)` の行だけ。
            let _ = native.SetIsEnabled(!row.is_disabled_item());
            // 無効の行には入力が届かないので、起動も文脈メニューも出ない。
            self.listen_row_events(&native, index)?;

            let element = native
                .cast::<IInspectable>()
                .map_err(|e| to_error("convert the row to UIElement", e))?;
            self.without_notifying(|_| children.Append(&element))
                .map_err(|e| to_error("add the row", e))?;
            native_rows.push(native);
        }
        *self.0.native_rows.borrow_mut() = native_rows;
        *self.0.rows.borrow_mut() = rows.to_vec();
        self.write_selection(&[]);
        Ok(())
    }

    /// Receives row activation (double-click, Enter) and context menu requests.
    ///
    /// Rows are recreated on every `set_rows`, so the index can be the position at
    /// creation. The subscriptions hold the `List` weakly (avoiding the cycle row →
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

    /// Empties the holders of the rows currently laid out.
    ///
    /// Custom-content rows place the widget the app holds directly into the holder
    /// (`Grid`). Letting go of the holder with it still inside leaves the widget "with
    /// a parent", and the next rebuild is rejected with "Element is already the child of
    /// another element". So it is always removed here before the holder is discarded.
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

    /// The number of rows.
    pub fn len(&self) -> usize {
        self.0.rows.borrow().len()
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

    /// Scrolls until row `index` is visible. The selection does not change.
    ///
    /// If it is already visible nothing moves; otherwise it is aligned to the nearer
    /// edge (the top if it is above, the bottom if it is below). Does nothing if out of
    /// range.
    pub fn scroll_to_row(&self, index: usize) {
        let item = self.0.native_rows.borrow().get(index).cloned();
        if let Some(item) = item.and_then(|item| item.cast::<windows_core::IInspectable>().ok()) {
            let _ = self.0.list_view.ScrollIntoView(&item);
        }
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
        let rows = self.0.rows.borrow();
        self.0.mode.get().normalize_by(indices, |index| {
            rows.get(index).is_some_and(ListRow::is_selectable)
        })
    }

    /// Writes the selection into the rows as is (no notification).
    fn write_selection(&self, indices: &[usize]) {
        self.without_notifying(|this| {
            for (index, native) in this.0.native_rows.borrow().iter().enumerate() {
                let _ = native.SetIsSelected(indices.contains(&index));
            }
        });
    }

    /// Operates with notifications from WinUI stopped.
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
    let host = match XamlReader::Load(&HSTRING::from(ROW_HOST_XAML))
        .and_then(|element| element.cast::<XamlGrid>())
    {
        Ok(host) => host,
        // 透明な背景を持てないと余白では押せなくなるが、
        // 文字やアイコンの上では届くので動作を優先する。
        Err(_) => XamlGrid::new().map_err(|e| to_error("create the row container", e))?,
    };
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
