//! The sidebar (WinUI 3's `NavigationView`).
//!
//! A left-pane `NavigationView` is used, the same as Windows Settings (in
//! `PaneDisplayMode::Auto`, which is the left pane while the window is wide enough). The pane's material, the selection pill, item layout and
//! keyboard navigation are owned by WinUI.
//!
//! | naui | WinUI 3 |
//! | --- | --- |
//! | The whole sidebar | `NavigationView` (`PaneDisplayMode::Auto`, or `Left` without automatic collapsing) |
//! | Item | `NavigationViewItem` + `FontIcon` (Segoe Fluent Icons) |
//! | Group heading | `NavigationViewItemHeader` |
//! | Between groups | `NavigationViewItemSeparator` |
//! | Right pane | `NavigationView.Content`. The window's child goes here |
//! | Divider | A transparent `Grid` overlaid on the pane's right edge (the same grab area as naui's `SplitView`) |
//!
//! The width of a `NavigationView`'s pane (`OpenPaneLength`) is not designed to be
//! changed by the user, but the sidebars of the other three platforms can all be
//! resized with a divider. So the same 6 px transparent grab area as naui's
//! `SplitView` is overlaid on the pane's right edge, and dragging it moves
//! `OpenPaneLength`. The boundary line is drawn by the `NavigationView` itself, so
//! the grab area stays transparent (no line is added). As with `SplitView`, the
//! cursor shape is not changed (`ProtectedCursor` is not in the projection).
//!
//! It is collapsed and expanded with the `NavigationView`'s standard pane toggle
//! button (hamburger). Following Windows conventions, collapsing turns the pane into
//! **a thin strip of icons only** (`IsPaneOpen` false; the button stays in the strip
//! so it can be reopened). Items without icons show nothing while collapsed. The
//! back button and the settings item are not shown.
//!
//! Collapsing when the window narrows is the `NavigationView`'s own
//! (`PaneDisplayMode::Auto`). Below `ExpandedModeThresholdWidth` it switches to the
//! compact strip by itself, and opening the pane there lays it over the content
//! (dismissed by clicking outside). naui sets the threshold to the sidebar's width
//! plus [`SIDEBAR_COLLAPSE_CONTENT_WIDTH`], and `CompactModeThresholdWidth` to 0 so
//! that the strip never disappears entirely (`LeftMinimal`).

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use naui_core::{
    clamp_split_position, sidebar_item, sidebar_len, sidebar_rows, Result, SidebarItem, SidebarRow,
    SidebarSection, DEFAULT_SIDEBAR_WIDTH, SIDEBAR_COLLAPSE_CONTENT_WIDTH, SIDEBAR_MIN_WIDTH,
};
use naui_winui3::Microsoft::UI::Xaml::Controls::{
    Control, FontIcon, Grid as XamlGrid, NavigationView, NavigationViewBackButtonVisible,
    NavigationViewItem, NavigationViewItemHeader, NavigationViewItemSeparator,
    NavigationViewPaneDisplayMode, NavigationViewSelectionChangedEventArgs,
};
use naui_winui3::Microsoft::UI::Xaml::Input::{PointerEventHandler, PointerRoutedEventArgs};
use naui_winui3::Microsoft::UI::Xaml::Markup::XamlReader;
use naui_winui3::Microsoft::UI::Xaml::Media::VisualTreeHelper;
use naui_winui3::Microsoft::UI::Xaml::{
    DependencyObject, FrameworkElement, RoutedEventHandler, TextTrimming, Thickness, UIElement,
    Visibility,
};
use windows::Foundation::{PropertyValue, TypedEventHandler};
use windows_core::{IInspectable, IUnknown, Interface, HSTRING};

use crate::navigation::{text_block, SelectHandler};
use crate::to_error;
use crate::ui_thread::{HandlerCell, UiThreadCell};

/// The width of the divider's grab area (logical pixels). The same as naui's
/// `SplitView`.
const GRIP_THICKNESS: f64 = 6.0;

/// The grab area overlaid on the pane's right edge. Unlike `null`, `Transparent`
/// keeps hit-testing, so it can be grabbed while invisible.
const GRIP_XAML: &str = r##"<Grid
    xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
    Background="Transparent"
    Width="6"
    HorizontalAlignment="Left"
    VerticalAlignment="Stretch"/>"##;

struct SidebarInner {
    native: NavigationView,
    sections: RefCell<Vec<SidebarSection>>,
    /// The `NavigationViewItem`s, in the same order as the items' running numbers.
    items: RefCell<Vec<NavigationViewItem>>,
    selected: Cell<Option<usize>>,
    handler: SelectHandler,
    /// Stops notifications only while the selection is being changed from code, since
    /// rewriting `SelectedItem` also fires `SelectionChanged`.
    silent: Cell<bool>,
    width: Cell<f64>,
    on_resize: ValueHandler<f64>,
    /// The last known collapsed state. Compared so that only the user's toggles are
    /// notified.
    collapsed: Cell<bool>,
    on_collapse: ValueHandler<bool>,
    /// Whether the pane collapses automatically when the window narrows
    /// (`PaneDisplayMode::Auto`).
    auto_collapse: Cell<bool>,
    /// The `Grid` overlaying the `NavigationView` and the divider. This is what is
    /// placed inside the window.
    host: XamlGrid,
    /// The transparent grab area overlaid on the pane's right edge.
    grip: XamlGrid,
    /// True while grabbed.
    dragging: Cell<bool>,
    /// The offset between where it was grabbed and the pane's right edge.
    grab: Cell<f64>,
}

/// A handler that receives one value. The closure is taken out only for the duration
/// of the call (to allow re-entrancy).
#[derive(Clone)]
struct ValueHandler<T: 'static>(HandlerCell<dyn FnMut(T)>);

impl<T: 'static> ValueHandler<T> {
    fn new() -> Self {
        Self(Arc::new(UiThreadCell::new(None)))
    }

    fn set(&self, f: impl FnMut(T) + 'static) {
        self.0.with_mut(|slot| *slot = Some(Box::new(f)));
    }

    fn emit(&self, value: T) {
        let Some(mut f) = self.0.with_mut(|slot| slot.take()) else {
            return;
        };
        f(value);
        self.0.with_mut(|slot| {
            if slot.is_none() {
                *slot = Some(f);
            }
        });
    }
}

/// A sidebar attached to the left of the window.
///
/// Attach it with [`Window::set_sidebar`](crate::Window::set_sidebar). It is not
/// placed in a layout, so it is not a [`Widget`](crate::Widget).
#[derive(Clone)]
pub struct Sidebar(Rc<SidebarInner>);

impl Sidebar {
    pub(crate) fn new() -> Result<Self> {
        let native = NavigationView::new().map_err(|e| to_error("NavigationView の生成", e))?;
        native
            .SetPaneDisplayMode(NavigationViewPaneDisplayMode::Auto)
            .map_err(|e| to_error("NavigationView の表示形式の設定", e))?;
        // 狭くても細い帯は残す (既定の 641 未満では帯ごと消える)。
        let _ = native.SetCompactModeThresholdWidth(0.0);
        let _ = native.SetIsBackButtonVisible(NavigationViewBackButtonVisible::Collapsed);
        let _ = native.SetIsSettingsVisible(false);
        let _ = native.SetIsPaneToggleButtonVisible(true);
        // タイトルバーは naui が上に別の行で持つので、その分を空けない。
        let _ = native.SetIsTitleBarAutoPaddingEnabled(false);
        let _ = native.SetIsPaneOpen(true);

        let host = XamlGrid::new().map_err(|e| to_error("サイドバーの Grid の生成", e))?;
        let grip = XamlReader::Load(&HSTRING::from(GRIP_XAML))
            .and_then(|grip| grip.cast::<XamlGrid>())
            .map_err(|e| to_error("サイドバーの仕切りの生成", e))?;
        for element in [native.cast::<UIElement>(), grip.cast::<UIElement>()] {
            let element = element.map_err(|e| to_error("サイドバーの要素化", e))?;
            host.Children()
                .and_then(|children| children.Append(&element))
                .map_err(|e| to_error("サイドバーの組み立て", e))?;
        }

        let this = Self(Rc::new(SidebarInner {
            native,
            sections: RefCell::new(Vec::new()),
            items: RefCell::new(Vec::new()),
            selected: Cell::new(None),
            handler: SelectHandler::new(),
            silent: Cell::new(false),
            width: Cell::new(DEFAULT_SIDEBAR_WIDTH),
            on_resize: ValueHandler::new(),
            collapsed: Cell::new(false),
            on_collapse: ValueHandler::new(),
            auto_collapse: Cell::new(true),
            host,
            grip,
            dragging: Cell::new(false),
            grab: Cell::new(0.0),
        }));
        this.apply_width();
        this.track_grip()?;

        // ハンドルを強く持つと購読との間で循環するため、弱参照にする。
        let state = UiThreadCell::new(Rc::downgrade(&this.0));
        let changed =
            TypedEventHandler::<NavigationView, NavigationViewSelectionChangedEventArgs>::new(
                move |_sender, args| {
                    let selected = args.as_ref().and_then(|args| args.SelectedItem().ok());
                    let _ = state.try_with_mut(|weak| {
                        if let Some(inner) = weak.upgrade() {
                            Sidebar(inner).selection_changed(selected);
                        }
                    });
                    Ok(())
                },
            );
        this.0
            .native
            .SelectionChanged(&changed)
            .map_err(|e| to_error("NavigationView の購読", e))?;

        // 畳むボタンでの開閉。プログラムからの開閉でも届くので、覚えている
        // 状態と食い違ったときだけ通知する (`set_collapsed` は先に覚える)。
        for opened in [true, false] {
            let state = UiThreadCell::new(Rc::downgrade(&this.0));
            let handler = TypedEventHandler::<NavigationView, IInspectable>::new(move |_, _| {
                let _ = state.try_with_mut(|weak| {
                    if let Some(inner) = weak.upgrade() {
                        Sidebar(inner).pane_changed(!opened);
                    }
                });
                Ok(())
            });
            let result = if opened {
                this.0.native.PaneOpened(&handler)
            } else {
                this.0.native.PaneClosed(&handler)
            };
            result.map_err(|e| to_error("NavigationView の開閉の購読", e))?;
        }
        Ok(this)
    }

    /// When the pane opens or collapses, records it again and notifies if it changed.
    fn pane_changed(&self, collapsed: bool) {
        self.show_grip(!collapsed);
        if self.0.collapsed.replace(collapsed) != collapsed {
            self.0.on_collapse.emit(collapsed);
        }
    }

    /// Connects how the divider is grabbed (pointer). The same shape as `SplitView`.
    fn track_grip(&self) -> Result<()> {
        let grip = self.0.grip.clone();
        let with = |f: fn(&Sidebar, &PointerRoutedEventArgs)| {
            let state = UiThreadCell::new(Rc::downgrade(&self.0));
            PointerEventHandler::new(move |_, args| {
                if let Some(args) = args.as_ref() {
                    let _ = state.try_with_mut(|weak| {
                        if let Some(inner) = weak.upgrade() {
                            f(&Sidebar(inner), args);
                        }
                    });
                }
                Ok(())
            })
        };
        grip.PointerPressed(&with(|sidebar, args| {
            if let Ok(pointer) = args.Pointer() {
                let _ = sidebar.0.grip.CapturePointer(&pointer);
            }
            if let Some(x) = sidebar.pointer_x(args) {
                // どこをつかんでも、ペインの端がポインターの下へ飛ばないように。
                sidebar.0.grab.set(x - sidebar.0.width.get());
            }
            sidebar.0.dragging.set(true);
        }))
        .map_err(|e| to_error("サイドバーの仕切りの購読", e))?;
        grip.PointerMoved(&with(|sidebar, args| {
            if !sidebar.0.dragging.get() {
                return;
            }
            let Some(x) = sidebar.pointer_x(args) else {
                return;
            };
            let total = sidebar
                .0
                .host
                .cast::<FrameworkElement>()
                .and_then(|host| host.ActualWidth())
                .unwrap_or(0.0);
            // 自動で畳むときは、中身がしきい値より狭くなるところまでは
            // 広げさせない (つかんでいる最中に帯へ切り替わらないように)。
            let min_content = if sidebar.0.auto_collapse.get() {
                SIDEBAR_COLLAPSE_CONTENT_WIDTH
            } else {
                0.0
            };
            let width = clamp_split_position(
                x - sidebar.0.grab.get(),
                total,
                SIDEBAR_MIN_WIDTH,
                min_content,
            );
            if (width - sidebar.0.width.get()).abs() < 0.5 {
                return;
            }
            sidebar.0.width.set(width);
            sidebar.apply_width();
            sidebar.0.on_resize.emit(width);
        }))
        .map_err(|e| to_error("サイドバーの仕切りの購読", e))?;
        grip.PointerReleased(&with(|sidebar, args| {
            if let Ok(pointer) = args.Pointer() {
                let _ = sidebar.0.grip.ReleasePointerCapture(&pointer);
            }
            sidebar.0.dragging.set(false);
        }))
        .map_err(|e| to_error("サイドバーの仕切りの購読", e))?;
        grip.PointerCaptureLost(&with(|sidebar, _| sidebar.0.dragging.set(false)))
            .map_err(|e| to_error("サイドバーの仕切りの購読", e))?;
        Ok(())
    }

    /// The pointer's horizontal position (from the sidebar's left edge).
    fn pointer_x(&self, args: &PointerRoutedEventArgs) -> Option<f64> {
        let host = self.0.host.cast::<UIElement>().ok()?;
        let point = args.GetCurrentPoint(&host).ok()?.Position().ok()?;
        Some(f64::from(point.X))
    }

    /// Shows the divider (only while the pane is open; the collapsed strip's width
    /// cannot be changed).
    fn show_grip(&self, show: bool) {
        let _ = self.0.grip.SetVisibility(if show {
            Visibility::Visible
        } else {
            Visibility::Collapsed
        });
    }

    /// Receives `SelectionChanged`, remembers the selected number, and notifies if it
    /// changed.
    ///
    /// Nothing is notified when the number does not change. The `NavigationView` also
    /// re-sends a selection set from code when it loads its template (arriving outside
    /// `silent`).
    fn selection_changed(&self, selected: Option<IInspectable>) {
        let index = selected.and_then(|item| self.index_of(&item));
        let changed = self.0.selected.replace(index) != index;
        if self.0.silent.get() || !changed {
            return;
        }
        if let Some(index) = index {
            self.0.handler.emit(index);
        }
    }

    /// Lays out items by group. Each call replaces them.
    ///
    /// The selected item stays selected (without notifying) if the same running number
    /// can still be selected.
    pub fn set_sections(&self, sections: &[SidebarSection]) {
        let keep = self
            .0
            .selected
            .get()
            .filter(|&index| sidebar_item(sections, index).is_some_and(|item| item.enabled));
        *self.0.sections.borrow_mut() = sections.to_vec();
        self.without_notifying(|| {
            if let Err(error) = self.rebuild(sections) {
                eprintln!("naui-windows: サイドバーの項目を作れませんでした: {error}");
            }
        });
        self.0.selected.set(None);
        if let Some(index) = keep {
            self.set_selected(index);
        }
    }

    /// Lays out items as a single group without a heading.
    pub fn set_items(&self, items: &[SidebarItem]) {
        self.set_sections(&[SidebarSection::untitled(items.iter().cloned())]);
    }

    fn rebuild(&self, sections: &[SidebarSection]) -> Result<()> {
        let menu = self
            .0
            .native
            .MenuItems()
            .map_err(|e| to_error("NavigationView の項目の取得", e))?;
        menu.Clear()
            .map_err(|e| to_error("NavigationView の項目の消去", e))?;
        self.0.items.borrow_mut().clear();

        let mut items = Vec::new();
        for row in sidebar_rows(sections) {
            let entry: IInspectable = match row {
                SidebarRow::Gap => NavigationViewItemSeparator::new()
                    .and_then(|s| s.cast())
                    .map_err(|e| to_error("サイドバーの区切りの生成", e))?,
                SidebarRow::Header(title) => {
                    let header = NavigationViewItemHeader::new()
                        .map_err(|e| to_error("サイドバーの見出しの生成", e))?;
                    // 見出しの書式 (小さく太く) はテンプレートが文字へ当てる。
                    let text = PropertyValue::CreateString(&HSTRING::from(title))
                        .map_err(|e| to_error("サイドバーの見出しの文字の生成", e))?;
                    header
                        .SetContent(&text)
                        .map_err(|e| to_error("サイドバーの見出しの設定", e))?;
                    header
                        .cast()
                        .map_err(|e| to_error("サイドバーの見出しの変換", e))?
                }
                SidebarRow::Item(_, item) => {
                    let entry = item_entry(item)?;
                    let inspectable = entry
                        .cast()
                        .map_err(|e| to_error("サイドバーの項目の変換", e))?;
                    items.push(entry);
                    inspectable
                }
            };
            menu.Append(&entry)
                .map_err(|e| to_error("サイドバーの項目の追加", e))?;
        }
        *self.0.items.borrow_mut() = items;
        Ok(())
    }

    /// Which item the `SelectedItem` is.
    ///
    /// In COM, objects can be called the same only when their `IUnknown` pointers are
    /// equal, so they are normalized before comparing.
    fn index_of(&self, selected: &IInspectable) -> Option<usize> {
        let target = selected.cast::<IUnknown>().ok()?;
        self.0.items.borrow().iter().position(|item| {
            item.cast::<IUnknown>()
                .is_ok_and(|item| item.as_raw() == target.as_raw())
        })
    }

    /// The number of items across groups.
    pub fn len(&self) -> usize {
        sidebar_len(&self.0.sections.borrow())
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The running number of the selected item.
    pub fn selected(&self) -> Option<usize> {
        self.0.selected.get()
    }

    /// Changes the selection without notifying. Out-of-range and unselectable items are
    /// ignored.
    pub fn set_selected(&self, index: usize) {
        let Some(item) = self.selectable_item(index) else {
            return;
        };
        self.without_notifying(|| {
            let _ = self.0.native.SetSelectedItem(&item);
        });
        self.0.selected.set(Some(index));
    }

    /// Clears the selection (without notifying).
    pub fn clear_selection(&self) {
        self.without_notifying(|| {
            let _ = self.0.native.SetSelectedItem(None::<&IInspectable>);
        });
        self.0.selected.set(None);
    }

    /// Selects as if the user had chosen it, and notifies.
    ///
    /// Out-of-range and unselectable items are ignored. Notifies even if the item is
    /// already selected.
    pub fn select(&self, index: usize) {
        if self.selectable_item(index).is_none() {
            return;
        }
        self.set_selected(index);
        self.0.handler.emit(index);
    }

    /// The handler called when an item is chosen. The argument is the running number.
    pub fn on_select(&self, f: impl FnMut(usize) + 'static) {
        self.0.handler.set(f);
    }

    /// The sidebar width (logical pixels). The default is [`DEFAULT_SIDEBAR_WIDTH`].
    ///
    /// The user can change the width by dragging the divider (the minimum is
    /// [`SIDEBAR_MIN_WIDTH`]). The width after a change is returned by
    /// [`width`](Self::width) and delivered through [`on_resize`](Self::on_resize).
    /// This call does not call `on_resize`.
    pub fn set_width(&self, width: f64) {
        if !width.is_finite() || width <= 0.0 {
            return;
        }
        self.0.width.set(width.max(SIDEBAR_MIN_WIDTH));
        self.apply_width();
    }

    /// The sidebar width. Even while collapsed, returns the width it has when expanded.
    pub fn width(&self) -> f64 {
        self.0.width.get()
    }

    /// Called with the new width every time the user changes it with the divider.
    pub fn on_resize(&self, f: impl FnMut(f64) + 'static) {
        self.0.on_resize.set(f);
    }

    /// Writes the width to the pane and places the divider at the pane's right edge (the
    /// middle of the grab area is the boundary). The width at which the pane collapses
    /// automatically follows it.
    fn apply_width(&self) {
        let width = self.0.width.get();
        let _ = self.0.native.SetOpenPaneLength(width);
        let _ = self
            .0
            .native
            .SetExpandedModeThresholdWidth(width + SIDEBAR_COLLAPSE_CONTENT_WIDTH);
        let _ = self.0.grip.SetMargin(Thickness {
            Left: (width - GRIP_THICKNESS / 2.0).max(0.0),
            Top: 0.0,
            Right: 0.0,
            Bottom: 0.0,
        });
    }

    /// Collapses (`true`) or expands (`false`) the sidebar.
    ///
    /// Items and selection are kept while collapsed. On Windows it collapses into a thin
    /// strip of icons only (the same as clicking the toggle button).
    /// [`on_collapse`](Self::on_collapse) is not called.
    pub fn set_collapsed(&self, collapsed: bool) {
        self.0.collapsed.set(collapsed);
        let _ = self.0.native.SetIsPaneOpen(!collapsed);
        self.show_grip(!collapsed);
    }

    /// Whether the sidebar is collapsed.
    pub fn is_collapsed(&self) -> bool {
        !self.0.native.IsPaneOpen().unwrap_or(true)
    }

    /// The handler called when the sidebar collapses or expands other than by
    /// [`set_collapsed`](Self::set_collapsed). The argument is whether it is now
    /// collapsed.
    ///
    /// Called when the toggle button is used, when the pane laid over the content is
    /// dismissed, and when the window narrows or widens with
    /// [`set_auto_collapse`](Self::set_auto_collapse) on.
    pub fn on_collapse(&self, f: impl FnMut(bool) + 'static) {
        self.0.on_collapse.set(f);
    }

    /// Whether the sidebar collapses by itself when the window narrows, and expands
    /// again when it widens. On by default.
    ///
    /// It collapses into the strip when the content beside it would get narrower than
    /// [`SIDEBAR_COLLAPSE_CONTENT_WIDTH`] (`PaneDisplayMode::Auto`). Opened while
    /// narrow, the pane is laid over the content. While it is on, the divider does not
    /// widen the sidebar past that point. Off, it stays `PaneDisplayMode::Left`; the
    /// sidebar is left collapsed or expanded as it is.
    pub fn set_auto_collapse(&self, auto: bool) {
        self.0.auto_collapse.set(auto);
        let mode = if auto {
            NavigationViewPaneDisplayMode::Auto
        } else {
            NavigationViewPaneDisplayMode::Left
        };
        let _ = self.0.native.SetPaneDisplayMode(mode);
    }

    /// Whether the sidebar collapses by itself when the window narrows.
    pub fn auto_collapse(&self) -> bool {
        self.0.auto_collapse.get()
    }

    /// The real WinUI 3 object (`NavigationView`). A backend-specific escape hatch.
    pub fn native_navigation_view(&self) -> NavigationView {
        self.0.native.clone()
    }

    /// The divider overlaid on the pane's right edge. A backend-specific escape hatch.
    pub fn native_divider(&self) -> XamlGrid {
        self.0.grip.clone()
    }

    /// The element placed inside the window (a `Grid` overlaying the `NavigationView`
    /// and the divider).
    pub(crate) fn element(&self) -> Result<UIElement> {
        self.0
            .host
            .cast::<UIElement>()
            .map_err(|e| to_error("サイドバーの要素化", e))
    }

    /// Places the window's child in the right pane. `None` empties it.
    pub(crate) fn set_content(&self, element: Option<&UIElement>) -> bool {
        let result = match element {
            Some(element) => element
                .cast::<IInspectable>()
                .and_then(|element| self.0.native.SetContent(&element)),
            None => self.0.native.SetContent(None::<&IInspectable>),
        };
        match result {
            Ok(()) => true,
            Err(error) => {
                eprintln!("naui-windows: サイドバーの中身を置けませんでした: {error}");
                false
            }
        }
    }

    fn selectable_item(&self, index: usize) -> Option<NavigationViewItem> {
        let enabled =
            sidebar_item(&self.0.sections.borrow(), index).is_some_and(|item| item.enabled);
        if !enabled {
            return None;
        }
        self.0.items.borrow().get(index).cloned()
    }

    fn without_notifying(&self, f: impl FnOnce()) {
        let previous = self.0.silent.replace(true);
        f();
        self.0.silent.set(previous);
    }
}

/// One item. The icon, if any, is attached as a Segoe Fluent Icons glyph.
fn item_entry(item: &SidebarItem) -> Result<NavigationViewItem> {
    let entry = NavigationViewItem::new().map_err(|e| to_error("サイドバーの項目の生成", e))?;
    // 入りきらないときは、ほかの 3 環境と同じく末尾を省略記号にする。
    // 既定 (`TextTrimming::None`) のままだと、字の途中で断ち切られる。
    let label = text_block(&item.label)?;
    label
        .SetTextTrimming(TextTrimming::CharacterEllipsis)
        .map_err(|e| to_error("サイドバーの項目の省略の設定", e))?;
    entry
        .SetContent(&label)
        .map_err(|e| to_error("サイドバーの項目の設定", e))?;
    // テンプレートは項目が画面に載るときに当たる。載り直すたびに当て直すので、
    // そのつど右の溝を外す。送り主から引くので項目を抱え込まない。
    let loaded = RoutedEventHandler::new(|sender, _| {
        if let Some(entry) = sender
            .as_ref()
            .and_then(|s| s.cast::<NavigationViewItem>().ok())
        {
            drop_chevron_gutter(&entry);
        }
        Ok(())
    });
    entry
        .Loaded(&loaded)
        .map_err(|e| to_error("サイドバーの項目の読み込み購読", e))?;
    if let Some(icon) = item.icon {
        let glyph = FontIcon::new().map_err(|e| to_error("サイドバーの印の生成", e))?;
        glyph
            .SetGlyph(&HSTRING::from(icon.fluent_glyph().to_string()))
            .map_err(|e| to_error("サイドバーの印の設定", e))?;
        entry
            .SetIcon(&glyph)
            .map_err(|e| to_error("サイドバーの印の取り付け", e))?;
    }
    if !item.enabled {
        let _ = entry.SetIsEnabled(false);
        let _ = entry.SetSelectsOnInvoked(false);
    }
    Ok(entry)
}

/// Removes the gutter (14 px) to the item's right, so the text can use space almost
/// up to the right edge of the selection band.
///
/// The `NavigationViewItemPresenter` template writes `Margin="0,0,14,0"` directly on
/// the content frame (`ContentGrid`). It exists to place the chevron for expanding
/// nested items, sticking out into this gutter with `Margin="0,0,-14,0"`; naui's
/// items are never nested, so it stays empty, and text gets truncated even though
/// the band shows space. Between the text and the band's right edge, the 8 px of the
/// `ContentPresenter` remains.
///
/// If the template part cannot be found, nothing is done (WinUI's default stays).
fn drop_chevron_gutter(entry: &NavigationViewItem) {
    let Some(grid) = template_part(entry, "NavigationViewItemPresenter")
        .and_then(|presenter| template_part(&presenter, "ContentGrid"))
    else {
        return;
    };
    let Ok(margin) = grid.Margin() else {
        return;
    };
    if margin.Right != 0.0 {
        let _ = grid.SetMargin(Thickness {
            Right: 0.0,
            ..margin
        });
    }
}

/// Looks up a part from a control's template by name.
fn template_part<T: Interface>(control: &T, name: &str) -> Option<FrameworkElement> {
    let _ = control.cast::<Control>().ok()?.ApplyTemplate();
    control
        .cast::<DependencyObject>()
        .and_then(|control| VisualTreeHelper::GetChild(&control, 0))
        .and_then(|root| root.cast::<FrameworkElement>())
        .and_then(|root| root.FindName(&HSTRING::from(name)))
        .and_then(|part| part.cast::<FrameworkElement>())
        .ok()
}
