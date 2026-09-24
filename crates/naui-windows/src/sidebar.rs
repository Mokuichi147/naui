//! サイドバー (WinUI 3 の `NavigationView`)。
//!
//! Windows の「設定」と同じ、左ペイン (`PaneDisplayMode::Left`) の
//! `NavigationView` を使う。ペインの材質・選択のピル・項目の並び・キーボード
//! 操作は WinUI が持つ。
//!
//! | naui | WinUI 3 |
//! | --- | --- |
//! | サイドバー全体 | `NavigationView` (`PaneDisplayMode::Left`) |
//! | 項目 | `NavigationViewItem` + `FontIcon` (Segoe Fluent Icons) |
//! | まとまりの見出し | `NavigationViewItemHeader` |
//! | まとまりの間 | `NavigationViewItemSeparator` |
//! | 右の区画 | `NavigationView.Content`。ウィンドウの子をここへ置く |
//! | 仕切り | ペインの右端に重ねた透明な `Grid` (naui の `SplitView` と同じつかみ代) |
//!
//! `NavigationView` のペインの幅 (`OpenPaneLength`) は利用者が変えられる
//! 作りになっていないが、ほかの 3 環境のサイドバーはどれも仕切りで幅を変え
//! られる。そこで naui の `SplitView` と同じ 6 px の透明なつかみ代をペインの
//! 右端へ重ね、ドラッグで `OpenPaneLength` を動かす。境目の線は
//! `NavigationView` 自身が描くので、つかみ代は透明のまま (線を足さない)。
//! カーソルの形は `SplitView` と同じく変えない (`ProtectedCursor` が投影に無い)。
//!
//! 開閉は `NavigationView` 標準のペインを畳むボタン (ハンバーガー) で行う。
//! Windows の作法どおり、畳むとペインは**アイコンだけの細い帯**になる
//! (`IsPaneOpen` が偽。帯にボタンが残るので開き直せる)。アイコンの無い項目は
//! 畳んでいる間は何も出ない。戻るボタンと設定項目は出さない。

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use naui_core::{
    clamp_split_position, sidebar_item, sidebar_len, sidebar_rows, Result, SidebarItem, SidebarRow,
    SidebarSection, DEFAULT_SIDEBAR_WIDTH, SIDEBAR_MIN_WIDTH,
};
use naui_winui3::Microsoft::UI::Xaml::Controls::{
    FontIcon, Grid as XamlGrid, NavigationView, NavigationViewBackButtonVisible,
    NavigationViewItem, NavigationViewItemHeader, NavigationViewItemSeparator,
    NavigationViewPaneDisplayMode, NavigationViewSelectionChangedEventArgs,
};
use naui_winui3::Microsoft::UI::Xaml::Input::{PointerEventHandler, PointerRoutedEventArgs};
use naui_winui3::Microsoft::UI::Xaml::Markup::XamlReader;
use naui_winui3::Microsoft::UI::Xaml::{
    FrameworkElement, TextTrimming, Thickness, UIElement, Visibility,
};
use windows::Foundation::{PropertyValue, TypedEventHandler};
use windows_core::{IInspectable, IUnknown, Interface, HSTRING};

use crate::navigation::{text_block, SelectHandler};
use crate::to_error;
use crate::ui_thread::{HandlerCell, UiThreadCell};

/// 仕切りのつかみ代の幅 (論理ピクセル)。naui の `SplitView` と同じ。
const GRIP_THICKNESS: f64 = 6.0;

/// ペインの右端に重ねるつかみ代。`Transparent` は `null` と違って当たり判定が
/// 残るので、見えないままつかめる。
const GRIP_XAML: &str = r##"<Grid
    xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
    Background="Transparent"
    Width="6"
    HorizontalAlignment="Left"
    VerticalAlignment="Stretch"/>"##;

struct SidebarInner {
    native: NavigationView,
    sections: RefCell<Vec<SidebarSection>>,
    /// 項目の通し番号と同じ並びの `NavigationViewItem`。
    items: RefCell<Vec<NavigationViewItem>>,
    selected: Cell<Option<usize>>,
    handler: SelectHandler,
    /// プログラムから選択を変えている間だけ通知を止める。
    /// `SelectedItem` の書き換えでも `SelectionChanged` が起きるため。
    silent: Cell<bool>,
    width: Cell<f64>,
    on_resize: ValueHandler<f64>,
    /// 最後に知っている開閉。利用者の開閉だけを通知するために比べる。
    collapsed: Cell<bool>,
    on_collapse: ValueHandler<bool>,
    /// `NavigationView` と仕切りを重ねる `Grid`。ウィンドウの中へ置くのはこれ。
    host: XamlGrid,
    /// ペインの右端に重ねる透明なつかみ代。
    grip: XamlGrid,
    /// つかんでいる間は真。
    dragging: Cell<bool>,
    /// つかんだ場所とペインの右端とのずれ。
    grab: Cell<f64>,
}

/// 値を 1 つ受け取る通知先。呼び出しの間だけクロージャを取り出す (再入に備える)。
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

/// ウィンドウの左に付けるサイドバー。
///
/// [`Window::set_sidebar`](crate::Window::set_sidebar) で取り付ける。
/// レイアウトには置かないので [`Widget`](crate::Widget) ではない。
#[derive(Clone)]
pub struct Sidebar(Rc<SidebarInner>);

impl Sidebar {
    pub(crate) fn new() -> Result<Self> {
        let native = NavigationView::new().map_err(|e| to_error("NavigationView の生成", e))?;
        native
            .SetPaneDisplayMode(NavigationViewPaneDisplayMode::Left)
            .map_err(|e| to_error("NavigationView の表示形式の設定", e))?;
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

    /// ペインが開いた・畳まれたときに、変わっていれば覚え直して通知する。
    fn pane_changed(&self, collapsed: bool) {
        self.show_grip(!collapsed);
        if self.0.collapsed.replace(collapsed) != collapsed {
            self.0.on_collapse.emit(collapsed);
        }
    }

    /// 仕切りのつまみ方 (ポインター) をつなぐ。`SplitView` と同じ形。
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
            let width =
                clamp_split_position(x - sidebar.0.grab.get(), total, SIDEBAR_MIN_WIDTH, 0.0);
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

    /// ポインターの横位置 (サイドバーの左端から)。
    fn pointer_x(&self, args: &PointerRoutedEventArgs) -> Option<f64> {
        let host = self.0.host.cast::<UIElement>().ok()?;
        let point = args.GetCurrentPoint(&host).ok()?.Position().ok()?;
        Some(f64::from(point.X))
    }

    /// 仕切りを出す (ペインが開いている間だけ。畳んだ帯の幅は変えられない)。
    fn show_grip(&self, show: bool) {
        let _ = self.0.grip.SetVisibility(if show {
            Visibility::Visible
        } else {
            Visibility::Collapsed
        });
    }

    /// `SelectionChanged` を受けて、選ばれた番号を覚え、変わっていれば通知する。
    ///
    /// 番号が変わらないときは通知しない。`NavigationView` はテンプレートを
    /// 読み込んだときにも、プログラムから置いた選択をもう一度流してくる
    /// (そのときは `silent` の外で届く)。
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

    /// 項目をまとまりごとに並べる。呼ぶたびに置き換わる。
    ///
    /// 選ばれていた項目は、同じ通し番号がまだ選べるなら選ばれたまま残る
    /// (通知はしない)。
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

    /// 見出しの無いまとまり 1 つだけで並べる。
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

    /// `SelectedItem` が何番目の項目か。
    ///
    /// COM で同じオブジェクトだと言えるのは `IUnknown` のポインターが
    /// 等しいときだけなので、そろえてから比べる。
    fn index_of(&self, selected: &IInspectable) -> Option<usize> {
        let target = selected.cast::<IUnknown>().ok()?;
        self.0.items.borrow().iter().position(|item| {
            item.cast::<IUnknown>()
                .is_ok_and(|item| item.as_raw() == target.as_raw())
        })
    }

    /// まとまりをまたいだ項目の数。
    pub fn len(&self) -> usize {
        sidebar_len(&self.0.sections.borrow())
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// 選ばれている項目の通し番号。
    pub fn selected(&self) -> Option<usize> {
        self.0.selected.get()
    }

    /// 通知せずに選択を変える。範囲外・選べない項目は無視する。
    pub fn set_selected(&self, index: usize) {
        let Some(item) = self.selectable_item(index) else {
            return;
        };
        self.without_notifying(|| {
            let _ = self.0.native.SetSelectedItem(&item);
        });
        self.0.selected.set(Some(index));
    }

    /// 選択を外す (通知しない)。
    pub fn clear_selection(&self) {
        self.without_notifying(|| {
            let _ = self.0.native.SetSelectedItem(None::<&IInspectable>);
        });
        self.0.selected.set(None);
    }

    /// 利用者が選んだのと同じように選び、通知する。
    ///
    /// 範囲外・選べない項目は無視する。すでに選ばれている項目でも通知する。
    pub fn select(&self, index: usize) {
        if self.selectable_item(index).is_none() {
            return;
        }
        self.set_selected(index);
        self.0.handler.emit(index);
    }

    /// 項目が選ばれたときの通知先。引数は通し番号。
    pub fn on_select(&self, f: impl FnMut(usize) + 'static) {
        self.0.handler.set(f);
    }

    /// サイドバーの幅 (論理ピクセル)。既定は [`DEFAULT_SIDEBAR_WIDTH`]。
    ///
    /// 利用者は仕切りをドラッグして幅を変えられる (下限は
    /// [`SIDEBAR_MIN_WIDTH`])。変えたあとの幅は [`width`](Self::width) が返し、
    /// [`on_resize`](Self::on_resize) で届く。この呼び出しでは `on_resize` を
    /// 呼ばない。
    pub fn set_width(&self, width: f64) {
        if !width.is_finite() || width <= 0.0 {
            return;
        }
        self.0.width.set(width.max(SIDEBAR_MIN_WIDTH));
        self.apply_width();
    }

    /// サイドバーの幅。閉じていても開いたときの幅を返す。
    pub fn width(&self) -> f64 {
        self.0.width.get()
    }

    /// 利用者が仕切りで幅を変えるたび、変えた後の幅で呼ばれる。
    pub fn on_resize(&self, f: impl FnMut(f64) + 'static) {
        self.0.on_resize.set(f);
    }

    /// 幅をペインへ書き、仕切りをペインの右端 (つかみ代の真ん中が境目) へ置く。
    fn apply_width(&self) {
        let width = self.0.width.get();
        let _ = self.0.native.SetOpenPaneLength(width);
        let _ = self.0.grip.SetMargin(Thickness {
            Left: (width - GRIP_THICKNESS / 2.0).max(0.0),
            Top: 0.0,
            Right: 0.0,
            Bottom: 0.0,
        });
    }

    /// サイドバーを閉じる (`true`) か開く (`false`)。
    ///
    /// 閉じても項目と選択は残る。Windows ではアイコンだけの細い帯に畳まれる
    /// (畳むボタンを押したときと同じ)。[`on_collapse`](Self::on_collapse) は
    /// 呼ばない。
    pub fn set_collapsed(&self, collapsed: bool) {
        self.0.collapsed.set(collapsed);
        let _ = self.0.native.SetIsPaneOpen(!collapsed);
        self.show_grip(!collapsed);
    }

    /// サイドバーが閉じているかどうか。
    pub fn is_collapsed(&self) -> bool {
        !self.0.native.IsPaneOpen().unwrap_or(true)
    }

    /// 利用者がサイドバーを開閉したときの通知先。引数は閉じたかどうか。
    ///
    /// 畳むボタンの操作で呼ばれ、[`set_collapsed`](Self::set_collapsed)
    /// では呼ばれない。
    pub fn on_collapse(&self, f: impl FnMut(bool) + 'static) {
        self.0.on_collapse.set(f);
    }

    /// WinUI 3 の実体 (`NavigationView`)。バックエンド固有の脱出口。
    pub fn native_navigation_view(&self) -> NavigationView {
        self.0.native.clone()
    }

    /// ペインの右端に重ねた仕切り。バックエンド固有の脱出口。
    pub fn native_divider(&self) -> XamlGrid {
        self.0.grip.clone()
    }

    /// ウィンドウの中へ置く要素 (`NavigationView` と仕切りを重ねた `Grid`)。
    pub(crate) fn element(&self) -> Result<UIElement> {
        self.0
            .host
            .cast::<UIElement>()
            .map_err(|e| to_error("サイドバーの要素化", e))
    }

    /// 右の区画へウィンドウの子を置く。`None` なら空にする。
    pub(crate) fn set_content(&self, element: Option<&UIElement>) {
        let result = match element {
            Some(element) => element
                .cast::<IInspectable>()
                .and_then(|element| self.0.native.SetContent(&element)),
            None => self.0.native.SetContent(None::<&IInspectable>),
        };
        if let Err(error) = result {
            eprintln!("naui-windows: サイドバーの中身を置けませんでした: {error}");
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

/// 項目 1 つ。アイコンがあれば Segoe Fluent Icons の字面で付ける。
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
