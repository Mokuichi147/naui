//! ナビゲーション系のハンドル群 (WinUI 3)。
//!
//! | naui | WinUI 3 |
//! | --- | --- |
//! | `Tabs` | `Grid` + `ToggleButton` |
//! | `Navbar` | `TextBlock` (見出し) + `ToggleButton` の横並び |
//! | `Dock` | `ToggleButton` の横並び |
//! | `Menu` | `ToggleButton` の縦並び |
//! | `Breadcrumbs` | `HyperlinkButton` + 区切りの `TextBlock` |
//! | `Pagination` | `Button` + `ToggleButton` |
//! | `Link` | `HyperlinkButton` |
//!
//! `NavigationView` と `BreadcrumbBar` は `winio-winui3` のバインディングに
//! 含まれていないため、`ToggleButton` (WinUI の標準コントロール) を並べて
//! 構成している。選択状態は `IsChecked` で表すので、見た目は Fluent のまま。

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use naui_core::{NavItem, Result};
use windows_core::{Interface, HSTRING};
use winui3::Microsoft::UI::Xaml::Controls::Primitives::ToggleButton;
use winui3::Microsoft::UI::Xaml::Controls::{
    Button as XamlButton, Grid as XamlGrid, HyperlinkButton,
    Orientation as XamlOrientation, RowDefinition, StackPanel, TextBlock,
};
use winui3::Microsoft::UI::Xaml::{
    FrameworkElement, GridLength, GridUnitType, HorizontalAlignment, RoutedEventHandler, UIElement,
    VerticalAlignment,
};

use crate::to_error;
use crate::ui_thread::UiThreadCell;
use crate::widgets::{bool_ref, impl_widget, Widget};

/// ナビゲーション系ウィジェットの「選択された」通知先。
///
/// WinRT のデリゲートは `Send + Sync` を要求するため、`UiThreadCell` に載せる。
/// 呼び出しの間だけクロージャを取り出すので、コールバックの中から
/// 別のナビゲーションを操作しても二重借用にならない。
#[derive(Clone)]
pub(crate) struct SelectHandler(Arc<UiThreadCell<Option<Box<dyn FnMut(usize)>>>>);

impl SelectHandler {
    pub(crate) fn new() -> Self {
        Self(Arc::new(UiThreadCell::new(None)))
    }

    pub(crate) fn set(&self, f: impl FnMut(usize) + 'static) {
        self.0.with_mut(|slot| *slot = Some(Box::new(f)));
    }

    pub(crate) fn emit(&self, index: usize) {
        let Some(mut f) = self.0.with_mut(|slot| slot.take()) else {
            return;
        };
        f(index);
        self.0.with_mut(|slot| {
            if slot.is_none() {
                *slot = Some(f);
            }
        });
    }
}

/// 文字列を載せた `TextBlock` を作る。
fn text_block(text: &str) -> Result<TextBlock> {
    let block = TextBlock::new().map_err(|e| to_error("TextBlock の生成", e))?;
    block
        .SetText(&HSTRING::from(text))
        .map_err(|e| to_error("TextBlock への設定", e))?;
    Ok(block)
}

/// 縦横どちらかに並べる `StackPanel` を作る。
fn panel(orientation: XamlOrientation, spacing: f64) -> Result<StackPanel> {
    let panel = StackPanel::new().map_err(|e| to_error("StackPanel の生成", e))?;
    panel
        .SetOrientation(orientation)
        .map_err(|e| to_error("StackPanel の向き設定", e))?;
    let _ = panel.SetSpacing(spacing);
    if orientation == XamlOrientation::Horizontal {
        let _ = panel.SetVerticalAlignment(VerticalAlignment::Center);
    }
    Ok(panel)
}

fn append(panel: &StackPanel, element: &UIElement) -> Result<()> {
    panel
        .Children()
        .and_then(|children| children.Append(element))
        .map_err(|e| to_error("StackPanel への追加", e))
}

// -------------------------------------------------------------------- Bar

/// 「項目の並び + いま選ばれているもの」を持つ内部ハンドル。
///
/// ナビバー・ドック・メニュー・パンくず・ページネーションが共有する。
#[derive(Clone)]
struct Bar(Rc<BarInner>);

struct BarInner {
    panel: StackPanel,
    buttons: RefCell<Vec<ToggleButton>>,
    handler: SelectHandler,
    selected: Cell<Option<usize>>,
    /// 2 つ目以降の項目の前に区切りを入れる (パンくず用)。
    separators: bool,
}

impl Bar {
    fn new(panel: StackPanel, separators: bool) -> Self {
        Self(Rc::new(BarInner {
            panel,
            buttons: RefCell::new(Vec::new()),
            handler: SelectHandler::new(),
            selected: Cell::new(None),
            separators,
        }))
    }

    fn set_items(&self, items: &[NavItem]) -> Result<()> {
        let children = self
            .0
            .panel
            .Children()
            .map_err(|e| to_error("項目の取得", e))?;
        children.Clear().map_err(|e| to_error("項目の消去", e))?;
        self.0.buttons.borrow_mut().clear();

        let mut buttons = Vec::with_capacity(items.len());
        for (index, item) in items.iter().enumerate() {
            if self.0.separators && index > 0 {
                let separator = text_block("/")?;
                let element = separator
                    .cast::<UIElement>()
                    .map_err(|e| to_error("区切りの要素化", e))?;
                append(&self.0.panel, &element)?;
            }

            let button = ToggleButton::new().map_err(|e| to_error("ToggleButton の生成", e))?;
            button
                .SetContent(&text_block(&item.label)?)
                .map_err(|e| to_error("ToggleButton への内容設定", e))?;
            let _ = button.SetIsEnabled(item.enabled);
            let _ = button.SetIsChecked(&bool_ref(false)?);

            // Click はユーザー操作でしか発火しない。`SetIsChecked` では
            // 呼ばれないので、プログラムからの選択と混ざらない。
            // ハンドルを強く持つと購読との間で循環するため、弱参照にする。
            let state = UiThreadCell::new(Rc::downgrade(&self.0));
            let handler = RoutedEventHandler::new(move |_sender, _args| {
                state.with_mut(|weak| {
                    if let Some(inner) = weak.upgrade() {
                        Bar(inner).select(index);
                    }
                });
                Ok(())
            });
            button
                .Click(&handler)
                .map_err(|e| to_error("ToggleButton の購読", e))?;

            let element = button
                .cast::<UIElement>()
                .map_err(|e| to_error("項目の要素化", e))?;
            append(&self.0.panel, &element)?;
            buttons.push(button);
        }
        *self.0.buttons.borrow_mut() = buttons;
        // 項目が変わればインデックスの意味も変わるので、選択は必ず外す。
        self.mark_selected(None);
        Ok(())
    }

    /// 選択されたボタンだけを押し込む。
    fn mark_selected(&self, index: Option<usize>) {
        if let Ok(checked) = bool_ref(true) {
            if let Ok(unchecked) = bool_ref(false) {
                for (i, button) in self.0.buttons.borrow().iter().enumerate() {
                    let value = if Some(i) == index {
                        &checked
                    } else {
                        &unchecked
                    };
                    let _ = button.SetIsChecked(value);
                }
            }
        }
        self.0.selected.set(index);
    }

    fn len(&self) -> usize {
        self.0.buttons.borrow().len()
    }

    fn selected(&self) -> Option<usize> {
        self.0.selected.get()
    }

    fn set_selected(&self, index: usize) {
        if index < self.len() {
            self.mark_selected(Some(index));
        }
    }

    fn select(&self, index: usize) {
        if index < self.len() {
            self.mark_selected(Some(index));
            self.0.handler.emit(index);
        }
    }

    fn on_select(&self, f: impl FnMut(usize) + 'static) {
        self.0.handler.set(f);
    }
}

/// 項目を持つナビゲーションの共通実装を生やす。
macro_rules! impl_item_bar {
    ($t:ty) => {
        impl $t {
            /// 項目を作り直す。インデックスの意味が変わるため、選択は外れる。
            pub fn set_items(&self, items: &[NavItem]) {
                let _ = self.0.bar.set_items(items);
            }

            pub fn len(&self) -> usize {
                self.0.bar.len()
            }

            pub fn is_empty(&self) -> bool {
                self.len() == 0
            }

            pub fn selected(&self) -> Option<usize> {
                self.0.bar.selected()
            }

            /// 通知せずに選択を変える。
            pub fn set_selected(&self, index: usize) {
                self.0.bar.set_selected(index);
            }

            /// ユーザーが選んだのと同じ経路で選択を変える (通知あり)。
            pub fn select(&self, index: usize) {
                self.0.bar.select(index);
            }

            /// 項目が選ばれたときに、そのインデックスで呼ばれる。
            pub fn on_select(&self, f: impl FnMut(usize) + 'static) {
                self.0.bar.on_select(f);
            }
        }
    };
}

// ------------------------------------------------------------------- Tabs

struct TabsInner {
    native: XamlGrid,
    headers: StackPanel,
    content: XamlGrid,
    /// 子のハンドルを保持し、選択中の内容を表示する。
    children: RefCell<Vec<Box<dyn Widget>>>,
    buttons: RefCell<Vec<ToggleButton>>,
    handler: SelectHandler,
    selected: Cell<Option<usize>>,
}

/// タブ。見出しと選択中の中身を `Grid` で表示する。
#[derive(Clone)]
pub struct Tabs(Rc<TabsInner>);
impl_widget!(Tabs, native);

impl Tabs {
    pub(crate) fn new() -> Result<Self> {
        // Windows App SDK 2.3.1 の未パッケージ起動では、TabView の既定
        // テンプレート適用時に Microsoft.UI.Xaml.dll が fail-fast する。
        // ToggleButton と Grid で構成すると、同じ API を保ったまま安定して
        // 実行でき、選択中の内容へウィンドウの余りの高さも渡せる。
        let native = XamlGrid::new().map_err(|e| to_error("タブGridの生成", e))?;
        let headers = panel(XamlOrientation::Horizontal, 4.0)?;
        let content = XamlGrid::new().map_err(|e| to_error("タブ内容Gridの生成", e))?;

        let rows = native
            .RowDefinitions()
            .map_err(|e| to_error("タブ行定義の取得", e))?;
        let header_row = RowDefinition::new().map_err(|e| to_error("タブ見出し行の生成", e))?;
        header_row
            .SetHeight(GridLength {
                Value: 1.0,
                GridUnitType: GridUnitType::Auto,
            })
            .map_err(|e| to_error("タブ見出し行の設定", e))?;
        rows.Append(&header_row)
            .map_err(|e| to_error("タブ見出し行の追加", e))?;
        let content_row = RowDefinition::new().map_err(|e| to_error("タブ内容行の生成", e))?;
        content_row
            .SetHeight(GridLength {
                Value: 1.0,
                GridUnitType: GridUnitType::Star,
            })
            .map_err(|e| to_error("タブ内容行の設定", e))?;
        rows.Append(&content_row)
            .map_err(|e| to_error("タブ内容行の追加", e))?;

        let headers_element = headers
            .cast::<UIElement>()
            .map_err(|e| to_error("タブ見出しの要素化", e))?;
        let content_element = content
            .cast::<UIElement>()
            .map_err(|e| to_error("タブ内容の要素化", e))?;
        let headers_framework = headers
            .cast::<FrameworkElement>()
            .map_err(|e| to_error("タブ見出しのレイアウト要素化", e))?;
        let content_framework = content
            .cast::<FrameworkElement>()
            .map_err(|e| to_error("タブ内容のレイアウト要素化", e))?;
        let _ = XamlGrid::SetRow(&headers_framework, 0);
        let _ = XamlGrid::SetRow(&content_framework, 1);
        let _ = content_framework.SetHorizontalAlignment(HorizontalAlignment::Stretch);
        let _ = content_framework.SetVerticalAlignment(VerticalAlignment::Stretch);
        native
            .Children()
            .and_then(|children| children.Append(&headers_element))
            .map_err(|e| to_error("タブ見出しの追加", e))?;
        native
            .Children()
            .and_then(|children| children.Append(&content_element))
            .map_err(|e| to_error("タブ内容の追加", e))?;

        Ok(Self(Rc::new(TabsInner {
            native,
            headers,
            content,
            children: RefCell::new(Vec::new()),
            buttons: RefCell::new(Vec::new()),
            handler: SelectHandler::new(),
            selected: Cell::new(None),
        })))
    }

    /// タブを 1 枚追加する。`child` がそのタブの中身になる。
    pub fn add_tab(&self, label: &str, child: &dyn Widget) {
        let Ok(button) = ToggleButton::new() else {
            return;
        };
        let Ok(header) = text_block(label) else {
            return;
        };
        if button.SetContent(&header).is_err() {
            return;
        }
        let _ = button.SetIsChecked(bool_ref(false).ok().as_ref());

        let index = self.0.children.borrow().len();
        let state = UiThreadCell::new(Rc::downgrade(&self.0));
        let handler = RoutedEventHandler::new(move |_sender, _args| {
            state.with_mut(|weak| {
                if let Some(inner) = weak.upgrade() {
                    Tabs(inner).select(index);
                }
            });
            Ok(())
        });
        if button.Click(&handler).is_err() {
            return;
        }

        let Ok(element) = button.cast::<UIElement>() else {
            return;
        };
        if append(&self.0.headers, &element).is_ok() {
            self.0.buttons.borrow_mut().push(button);
            self.0.children.borrow_mut().push(child.boxed_clone());
            if self.0.selected.get().is_none() {
                self.mark_selected(Some(0));
            }
        }
    }

    pub fn len(&self) -> usize {
        self.0.children.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn selected(&self) -> Option<usize> {
        self.0.selected.get()
    }

    /// 通知せずに選択を変える。
    pub fn set_selected(&self, index: usize) {
        if index < self.len() {
            self.mark_selected(Some(index));
        }
    }

    /// ユーザーが選んだのと同じ経路で選択を変える (通知あり)。
    pub fn select(&self, index: usize) {
        if index < self.len() {
            self.mark_selected(Some(index));
            self.0.handler.emit(index);
        }
    }

    fn mark_selected(&self, index: Option<usize>) {
        if let Ok(checked) = bool_ref(true) {
            if let Ok(unchecked) = bool_ref(false) {
                for (i, button) in self.0.buttons.borrow().iter().enumerate() {
                    let value = if Some(i) == index {
                        &checked
                    } else {
                        &unchecked
                    };
                    let _ = button.SetIsChecked(value);
                }
            }
        }

        if let Ok(children) = self.0.content.Children() {
            let _ = children.Clear();
            if let Some(selected) = index {
                if let Some(child) = self.0.children.borrow().get(selected) {
                    let element = child.native_element();
                    if let Ok(framework) = element.cast::<FrameworkElement>() {
                        let _ = XamlGrid::SetRow(&framework, 0);
                        let _ = XamlGrid::SetColumn(&framework, 0);
                    }
                    let _ = children.Append(&element);
                }
            }
        }
        self.0.selected.set(index);
    }

    /// タブが切り替わったときに、そのインデックスで呼ばれる。
    pub fn on_select(&self, f: impl FnMut(usize) + 'static) {
        self.0.handler.set(f);
    }
}

// ----------------------------------------------------------------- Navbar

struct NavbarInner {
    native: StackPanel,
    title: TextBlock,
    bar: Bar,
}

/// 画面上部に置く横並びのナビゲーション。
#[derive(Clone)]
pub struct Navbar(Rc<NavbarInner>);
impl_widget!(Navbar, native);
impl_item_bar!(Navbar);

impl Navbar {
    pub(crate) fn new(title: &str) -> Result<Self> {
        let native = panel(XamlOrientation::Horizontal, 12.0)?;
        let title_block = text_block(title)?;
        let element = title_block
            .cast::<UIElement>()
            .map_err(|e| to_error("見出しの要素化", e))?;
        append(&native, &element)?;

        let items = panel(XamlOrientation::Horizontal, 4.0)?;
        let element = items
            .cast::<UIElement>()
            .map_err(|e| to_error("項目欄の要素化", e))?;
        append(&native, &element)?;

        Ok(Self(Rc::new(NavbarInner {
            native,
            title: title_block,
            bar: Bar::new(items, false),
        })))
    }

    /// 左端の見出し。
    pub fn set_title(&self, title: &str) {
        let _ = self.0.title.SetText(&HSTRING::from(title));
    }

    pub fn title(&self) -> String {
        self.0
            .title
            .Text()
            .map(|s| s.to_string())
            .unwrap_or_default()
    }
}

// ------------------------------------------------------------------- Dock

struct DockInner {
    native: StackPanel,
    bar: Bar,
}

/// 画面下部に置く横並びのナビゲーション。
///
/// **配置はアプリの責務**で、naui のレイアウトは縦横のスタックしか持たないため、
/// ウィンドウ下端への固定はできない。縦スタックの最後に置くと下端寄りになる。
#[derive(Clone)]
pub struct Dock(Rc<DockInner>);
impl_widget!(Dock, native);
impl_item_bar!(Dock);

impl Dock {
    pub(crate) fn new() -> Result<Self> {
        let native = panel(XamlOrientation::Horizontal, 4.0)?;
        Ok(Self(Rc::new(DockInner {
            native: native.clone(),
            bar: Bar::new(native, false),
        })))
    }
}

// ------------------------------------------------------------------- Menu

struct MenuInner {
    native: StackPanel,
    bar: Bar,
}

/// 縦に並ぶナビゲーション一覧。
///
/// ポップアップの `MenuFlyout` ではない。
#[derive(Clone)]
pub struct Menu(Rc<MenuInner>);
impl_widget!(Menu, native);
impl_item_bar!(Menu);

impl Menu {
    pub(crate) fn new() -> Result<Self> {
        let native = panel(XamlOrientation::Vertical, 2.0)?;
        Ok(Self(Rc::new(MenuInner {
            native: native.clone(),
            bar: Bar::new(native, false),
        })))
    }
}

// ------------------------------------------------------------ Breadcrumbs

struct BreadcrumbsInner {
    native: StackPanel,
    bar: BreadcrumbBar,
}

/// リンク表示のパンくず用バー。
#[derive(Clone)]
struct BreadcrumbBar(Rc<BreadcrumbBarInner>);

struct BreadcrumbBarInner {
    panel: StackPanel,
    links: RefCell<Vec<HyperlinkButton>>,
    handler: SelectHandler,
    selected: Cell<Option<usize>>,
}

impl BreadcrumbBar {
    fn new(panel: StackPanel) -> Self {
        Self(Rc::new(BreadcrumbBarInner {
            panel,
            links: RefCell::new(Vec::new()),
            handler: SelectHandler::new(),
            selected: Cell::new(None),
        }))
    }

    fn set_items(&self, items: &[NavItem]) -> Result<()> {
        let children = self
            .0
            .panel
            .Children()
            .map_err(|e| to_error("パンくず項目の取得", e))?;
        children
            .Clear()
            .map_err(|e| to_error("パンくず項目の消去", e))?;
        self.0.links.borrow_mut().clear();

        let mut links = Vec::with_capacity(items.len());
        for (index, item) in items.iter().enumerate() {
            if index > 0 {
                let separator = text_block("/")?;
                separator
                    .SetVerticalAlignment(VerticalAlignment::Center)
                    .map_err(|e| to_error("パンくず区切りの縦位置設定", e))?;
                let element = separator
                    .cast::<UIElement>()
                    .map_err(|e| to_error("パンくず区切りの要素化", e))?;
                append(&self.0.panel, &element)?;
            }

            let link = HyperlinkButton::new()
                .map_err(|e| to_error("パンくずリンクの生成", e))?;
            link
                .SetContent(&text_block(&item.label)?)
                .map_err(|e| to_error("パンくずリンクの内容設定", e))?;
            link
                .SetNavigateUri(None)
                .map_err(|e| to_error("パンくずリンクの遷移先設定", e))?;
            link
                .SetVerticalAlignment(VerticalAlignment::Center)
                .map_err(|e| to_error("パンくずリンクの縦位置設定", e))?;
            let _ = link.SetIsEnabled(item.enabled);

            let state = UiThreadCell::new(Rc::downgrade(&self.0));
            let handler = RoutedEventHandler::new(move |_sender, _args| {
                state.with_mut(|weak| {
                    if let Some(inner) = weak.upgrade() {
                        BreadcrumbBar(inner).select(index);
                    }
                });
                Ok(())
            });
            link
                .Click(&handler)
                .map_err(|e| to_error("パンくずリンクの購読", e))?;

            let element = link
                .cast::<UIElement>()
                .map_err(|e| to_error("パンくずリンクの要素化", e))?;
            append(&self.0.panel, &element)?;
            links.push(link);
        }
        *self.0.links.borrow_mut() = links;
        self.mark_selected(None);
        Ok(())
    }

    fn mark_selected(&self, index: Option<usize>) {
        self.0.selected.set(index);
    }

    fn len(&self) -> usize {
        self.0.links.borrow().len()
    }

    fn selected(&self) -> Option<usize> {
        self.0.selected.get()
    }

    fn set_selected(&self, index: usize) {
        if index < self.len() {
            self.mark_selected(Some(index));
        }
    }

    fn select(&self, index: usize) {
        if index < self.len() {
            self.mark_selected(Some(index));
            self.0.handler.emit(index);
        }
    }

    fn on_select(&self, f: impl FnMut(usize) + 'static) {
        self.0.handler.set(f);
    }
}

/// パンくず。
#[derive(Clone)]
pub struct Breadcrumbs(Rc<BreadcrumbsInner>);
impl_widget!(Breadcrumbs, native);

impl Breadcrumbs {
    pub(crate) fn new() -> Result<Self> {
        let native = panel(XamlOrientation::Horizontal, 4.0)?;
        Ok(Self(Rc::new(BreadcrumbsInner {
            native: native.clone(),
            bar: BreadcrumbBar::new(native),
        })))
    }

    /// 階層を作り直す。末尾がいまいる場所になる。
    pub fn set_items(&self, items: &[NavItem]) {
        let _ = self.0.bar.set_items(items);
        if let Some(last) = items.len().checked_sub(1) {
            self.0.bar.set_selected(last);
        }
    }

    pub fn len(&self) -> usize {
        self.0.bar.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// いまいる場所。既定では末尾の項目。
    pub fn selected(&self) -> Option<usize> {
        self.0.bar.selected()
    }

    /// 通知せずにいまいる場所を変える。
    pub fn set_selected(&self, index: usize) {
        self.0.bar.set_selected(index);
    }

    /// ユーザーが選んだのと同じ経路で選択を変える (通知あり)。
    pub fn select(&self, index: usize) {
        self.0.bar.select(index);
    }

    /// 項目が選ばれたときに、そのインデックスで呼ばれる。
    pub fn on_select(&self, f: impl FnMut(usize) + 'static) {
        self.0.bar.on_select(f);
    }
}

// ------------------------------------------------------------- Pagination

struct PaginationInner {
    native: StackPanel,
    bar: Bar,
}

/// ページ送り。
///
/// WinUI に相当するコントロールが無いため、前へ / 次への `Button` と
/// ページ番号の `ToggleButton` を並べて構成している。
#[derive(Clone)]
pub struct Pagination(Rc<PaginationInner>);
impl_widget!(Pagination, native);

impl Pagination {
    pub(crate) fn new(page_count: usize) -> Result<Self> {
        let native = panel(XamlOrientation::Horizontal, 4.0)?;
        let numbers = panel(XamlOrientation::Horizontal, 4.0)?;

        let prev = XamlButton::new().map_err(|e| to_error("Button の生成", e))?;
        prev.SetContent(&text_block("‹")?)
            .map_err(|e| to_error("Button への内容設定", e))?;
        let next = XamlButton::new().map_err(|e| to_error("Button の生成", e))?;
        next.SetContent(&text_block("›")?)
            .map_err(|e| to_error("Button への内容設定", e))?;

        for element in [
            prev.cast::<UIElement>(),
            numbers.cast::<UIElement>(),
            next.cast::<UIElement>(),
        ] {
            let element = element.map_err(|e| to_error("ページ送りの要素化", e))?;
            append(&native, &element)?;
        }

        let this = Self(Rc::new(PaginationInner {
            native,
            bar: Bar::new(numbers, false),
        }));

        for (button, forward) in [(&prev, false), (&next, true)] {
            let state = UiThreadCell::new(Rc::downgrade(&this.0));
            let handler = RoutedEventHandler::new(move |_sender, _args| {
                state.with_mut(|weak| {
                    let Some(inner) = weak.upgrade() else {
                        return;
                    };
                    let pager = Pagination(inner);
                    if forward {
                        pager.go_next();
                    } else {
                        pager.go_previous();
                    }
                });
                Ok(())
            });
            button
                .Click(&handler)
                .map_err(|e| to_error("ページ送りの購読", e))?;
        }

        this.set_page_count(page_count);
        Ok(this)
    }

    /// ページ数を変える。表示は 1 始まり、API は 0 始まり。
    pub fn set_page_count(&self, count: usize) {
        let items: Vec<NavItem> = (1..=count).map(|n| NavItem::new(n.to_string())).collect();
        let _ = self.0.bar.set_items(&items);
        if count > 0 {
            self.0.bar.set_selected(0);
        }
    }

    pub fn page_count(&self) -> usize {
        self.0.bar.len()
    }

    /// いまのページ (0 始まり)。
    pub fn page(&self) -> usize {
        self.0.bar.selected().unwrap_or(0)
    }

    /// 通知せずにページを変える。
    pub fn set_page(&self, page: usize) {
        self.0.bar.set_selected(page);
    }

    /// ユーザーが選んだのと同じ経路でページを変える (通知あり)。
    pub fn select(&self, page: usize) {
        self.0.bar.select(page);
    }

    /// 前のページへ。先頭にいるときは何もしない。
    pub fn go_previous(&self) {
        let page = self.page();
        if page > 0 {
            self.select(page - 1);
        }
    }

    /// 次のページへ。末尾にいるときは何もしない。
    pub fn go_next(&self) {
        let page = self.page();
        if page + 1 < self.page_count() {
            self.select(page + 1);
        }
    }

    /// ページが変わったときに、その番号 (0 始まり) で呼ばれる。
    pub fn on_change(&self, f: impl FnMut(usize) + 'static) {
        self.0.bar.on_select(f);
    }
}

// ------------------------------------------------------------------- Link

struct LinkInner {
    native: HyperlinkButton,
    label: TextBlock,
    href: RefCell<String>,
    token: RefCell<Option<i64>>,
}

/// リンク (HyperlinkButton)。
///
/// `href` が空でなければ、押したときに WinUI が既定のブラウザで開く。
#[derive(Clone)]
pub struct Link(Rc<LinkInner>);
impl_widget!(Link, native);

impl Link {
    pub(crate) fn new(text: &str, href: &str) -> Result<Self> {
        let native = HyperlinkButton::new().map_err(|e| to_error("HyperlinkButton の生成", e))?;
        let label = text_block(text)?;
        native
            .SetContent(&label)
            .map_err(|e| to_error("HyperlinkButton への内容設定", e))?;

        let this = Self(Rc::new(LinkInner {
            native,
            label,
            href: RefCell::new(String::new()),
            token: RefCell::new(None),
        }));
        this.set_href(href);
        Ok(this)
    }

    pub fn text(&self) -> String {
        self.0
            .label
            .Text()
            .map(|s| s.to_string())
            .unwrap_or_default()
    }

    pub fn set_text(&self, text: &str) {
        let _ = self.0.label.SetText(&HSTRING::from(text));
    }

    pub fn href(&self) -> String {
        self.0.href.borrow().clone()
    }

    pub fn set_href(&self, href: &str) {
        *self.0.href.borrow_mut() = href.to_string();
        if href.is_empty() {
            let _ = self.0.native.SetNavigateUri(None);
            return;
        }
        if let Ok(uri) = windows::Foundation::Uri::CreateUri(&HSTRING::from(href)) {
            let _ = self.0.native.SetNavigateUri(&uri);
        }
    }

    pub fn set_enabled(&self, enabled: bool) {
        let _ = self.0.native.SetIsEnabled(enabled);
    }

    /// 押されたときに呼ばれる。設定し直すと以前のものは外れる。
    ///
    /// `href` を開くのは WinUI 自身が行う。
    pub fn on_click(&self, f: impl FnMut() + 'static) {
        if let Some(token) = self.0.token.borrow_mut().take() {
            let _ = self.0.native.RemoveClick(token);
        }
        let f = UiThreadCell::new(f);
        let handler = RoutedEventHandler::new(move |_sender, _args| {
            f.with_mut(|f| f());
            Ok(())
        });
        if let Ok(token) = self.0.native.Click(&handler) {
            *self.0.token.borrow_mut() = Some(token);
        }
    }
}
