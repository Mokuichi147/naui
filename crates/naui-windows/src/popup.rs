//! ポップアップ (コンテキスト) メニュー (WinUI 3)。
//!
//! `MenuFlyout` は `winio-winui3` のバインディングに含まれていないため、
//! **ウィンドウのルートに重ねる `Grid` + `Button` の縦並び**で構成している
//! (`Navbar` などを `ToggleButton` で組んでいるのと同じ方針)。
//!
//! | naui | WinUI 3 |
//! | --- | --- |
//! | `PopupMenu` | ルートに重ねる `Grid` (受け皿) + `Grid` (枠) |
//! | 項目 | `Button` (枠なし・左寄せ) |
//! | 区切り線 | 高さ 1 の `Grid` |
//!
//! 色は `{ThemeResource ...}` で引くので、Fluent のテーマ切り替えに追従する。

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::sync::Arc;

use naui_core::{Error, PopupItem, Result};
use windows_core::{Interface, HSTRING};
use winui3::Microsoft::UI::Xaml::Controls::{
    Button as XamlButton, Canvas, Grid as XamlGrid, Panel, StackPanel, TextBlock,
};
use winui3::Microsoft::UI::Xaml::Input::PointerEventHandler;
use winui3::Microsoft::UI::Xaml::Markup::XamlReader;
use winui3::Microsoft::UI::Xaml::{
    FrameworkElement, RoutedEventHandler, Thickness, UIElement,
};

use crate::navigation::SelectHandler;
use crate::to_error;
use crate::ui_thread::UiThreadCell;
use crate::widgets::Widget;

/// 重ねる受け皿を、行や列を切ったルートでも全面に広げるための span。
/// WinUI は実際の行数・列数までに丸めるので、多めに指定して構わない。
const OVERLAY_SPAN: i32 = 1024;

/// メニューを他の要素より手前に出すための Z 順。
const OVERLAY_Z_INDEX: i32 = 1000;

/// メニューの枠の見た目。使えるテーマリソースは環境で違うため、
/// 読めたものを先頭から採用する。
const SURFACE_BRUSHES: &[(&str, &str)] = &[
    (
        "{ThemeResource MenuFlyoutPresenterBackground}",
        "{ThemeResource MenuFlyoutPresenterBorderBrush}",
    ),
    (
        "{ThemeResource CardBackgroundFillColorDefaultBrush}",
        "{ThemeResource CardStrokeColorDefaultBrush}",
    ),
    (
        "{ThemeResource ApplicationPageBackgroundThemeBrush}",
        "{ThemeResource ApplicationForegroundThemeBrush}",
    ),
];

fn surface_xaml(background: &str, border: &str) -> String {
    format!(
        r##"<Grid xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
            Background="Transparent">
            <Grid HorizontalAlignment="Left" VerticalAlignment="Top" MinWidth="160"
                Background="{background}" BorderBrush="{border}"
                BorderThickness="1" CornerRadius="8" Padding="4">
                <StackPanel Orientation="Vertical">
                    <StackPanel.Resources>
                        <Style TargetType="Button">
                            <Setter Property="Background" Value="Transparent"/>
                            <Setter Property="BorderThickness" Value="0"/>
                            <Setter Property="CornerRadius" Value="4"/>
                            <Setter Property="Padding" Value="12,6"/>
                            <Setter Property="HorizontalAlignment" Value="Stretch"/>
                            <Setter Property="HorizontalContentAlignment" Value="Left"/>
                        </Style>
                    </StackPanel.Resources>
                </StackPanel>
            </Grid>
        </Grid>"##
    )
}

struct Surface {
    /// 画面いっぱいの受け皿。外側を押すと閉じる。
    overlay: XamlGrid,
    /// メニューの枠。位置は `Margin` で決める。
    frame: XamlGrid,
    /// 項目を縦に並べる場所。
    panel: StackPanel,
}

fn build_surface() -> Result<Surface> {
    let mut last: Option<Error> = None;
    for (background, border) in SURFACE_BRUSHES {
        match load_surface(background, border) {
            Ok(surface) => return Ok(surface),
            Err(e) => last = Some(e),
        }
    }
    Err(last.unwrap_or_else(|| Error::new("メニューの生成", "テーマリソースがありません")))
}

fn load_surface(background: &str, border: &str) -> Result<Surface> {
    let overlay = XamlReader::Load(&HSTRING::from(surface_xaml(background, border)))
        .map_err(|e| to_error("メニュー要素の生成", e))?
        .cast::<XamlGrid>()
        .map_err(|e| to_error("メニュー要素への変換", e))?;
    let frame = overlay
        .Children()
        .and_then(|children| children.GetAt(0))
        .map_err(|e| to_error("メニュー枠の取得", e))?
        .cast::<XamlGrid>()
        .map_err(|e| to_error("メニュー枠への変換", e))?;
    let panel = frame
        .Children()
        .and_then(|children| children.GetAt(0))
        .map_err(|e| to_error("メニュー項目欄の取得", e))?
        .cast::<StackPanel>()
        .map_err(|e| to_error("メニュー項目欄への変換", e))?;
    Ok(Surface {
        overlay,
        frame,
        panel,
    })
}

struct PopupMenuInner {
    surface: Surface,
    /// 項目ごとのボタン。区切り線の位置は `None`。
    buttons: RefCell<Vec<Option<XamlButton>>>,
    /// 取り付けたウィジェットのハンドルと `PointerPressed` のトークン。
    attached: RefCell<Vec<(UIElement, i64, Box<dyn Widget>)>>,
    /// いま受け皿を載せている親。閉じるときに取り外す。
    host: RefCell<Option<Panel>>,
    handler: SelectHandler,
    open: Cell<bool>,
}

/// ポップアップ (コンテキスト) メニュー。
///
/// 画面に並ぶウィジェットではないので [`Widget`] ではない。
/// [`crate::Ui`] が生成したメニューを保持するため、戻り値を捨てても
/// 取り付け先から消えることはない。
#[derive(Clone)]
pub struct PopupMenu(Rc<PopupMenuInner>);

impl PopupMenu {
    pub(crate) fn new() -> Result<Self> {
        let surface = build_surface()?;
        let this = Self(Rc::new(PopupMenuInner {
            surface,
            buttons: RefCell::new(Vec::new()),
            attached: RefCell::new(Vec::new()),
            host: RefCell::new(None),
            handler: SelectHandler::new(),
            open: Cell::new(false),
        }));
        this.install_dismiss();
        Ok(this)
    }

    /// 受け皿を押したら閉じるようにする。
    fn install_dismiss(&self) {
        let weak = weak_cell(&self.0);
        let handler = PointerEventHandler::new(move |_sender, _args| {
            if let Some(menu) = weak.with_mut(|weak| weak.upgrade()) {
                PopupMenu(menu).close();
            }
            Ok(())
        });
        let _ = self.0.surface.overlay.PointerPressed(&handler);
    }

    /// 項目を作り直す。以前の項目は取り除かれる。
    ///
    /// インデックスは区切り線を含めた並びの位置。
    pub fn set_items(&self, items: &[PopupItem]) {
        let Ok(children) = self.0.surface.panel.Children() else {
            return;
        };
        let _ = children.Clear();
        self.0.buttons.borrow_mut().clear();

        let mut buttons = Vec::with_capacity(items.len());
        for (index, item) in items.iter().enumerate() {
            if item.is_separator() {
                if let Ok(separator) = self.build_separator() {
                    let _ = children.Append(&separator);
                }
                buttons.push(None);
                continue;
            }
            match self.build_item(&item.label, item.enabled, index) {
                Ok(button) => {
                    if let Ok(element) = button.cast::<UIElement>() {
                        let _ = children.Append(&element);
                    }
                    buttons.push(Some(button));
                }
                Err(_) => buttons.push(None),
            }
        }
        *self.0.buttons.borrow_mut() = buttons;
    }

    fn build_item(&self, label: &str, enabled: bool, index: usize) -> Result<XamlButton> {
        let button = XamlButton::new().map_err(|e| to_error("メニュー項目の生成", e))?;
        let text = TextBlock::new().map_err(|e| to_error("メニュー項目の文字生成", e))?;
        text.SetText(&HSTRING::from(label))
            .map_err(|e| to_error("メニュー項目の文字設定", e))?;
        button
            .SetContent(&text)
            .map_err(|e| to_error("メニュー項目への文字設定", e))?;
        let _ = button.SetIsEnabled(enabled);

        let weak = weak_cell(&self.0);
        let handler = RoutedEventHandler::new(move |_sender, _args| {
            if let Some(inner) = weak.with_mut(|weak| weak.upgrade()) {
                let menu = PopupMenu(inner);
                menu.close();
                menu.0.handler.emit(index);
            }
            Ok(())
        });
        button
            .Click(&handler)
            .map_err(|e| to_error("メニュー項目の購読", e))?;
        Ok(button)
    }

    fn build_separator(&self) -> Result<UIElement> {
        let separator = XamlGrid::new().map_err(|e| to_error("区切り線の生成", e))?;
        separator
            .SetHeight(1.0)
            .map_err(|e| to_error("区切り線の高さ設定", e))?;
        let _ = separator.SetMargin(Thickness {
            Left: 0.0,
            Top: 4.0,
            Right: 0.0,
            Bottom: 4.0,
        });
        // 枠と同じ色にすると、テーマの切り替えにそのまま追従する。
        if let Ok(brush) = self.0.surface.frame.BorderBrush() {
            let _ = separator.SetBackground(&brush);
        }
        separator
            .cast::<UIElement>()
            .map_err(|e| to_error("区切り線の要素化", e))
    }

    /// 区切り線を含めた項目数。
    pub fn len(&self) -> usize {
        self.0.buttons.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// ウィジェットの右クリックでこのメニューを出すようにする。
    ///
    /// WinUI にはブラウザのような既定のコンテキストメニューが無いので、
    /// 抑止すべきものは無い。
    ///
    /// **`Button` のようにコントロール自身が `PointerPressed` を
    /// 処理してしまうものへ取り付けても、右クリックが届かない可能性がある**
    /// (実機で未確認)。届かない場合は、そのコントロールを包む
    /// `Stack` などへ取り付けること。
    pub fn attach(&self, widget: &dyn Widget) {
        let element = widget.native_element();
        let weak = weak_cell(&self.0);
        let handler = PointerEventHandler::new(move |sender, args| {
            let Some(inner) = weak.with_mut(|weak| weak.upgrade()) else {
                return Ok(());
            };
            let Some(args) = args.as_ref() else {
                return Ok(());
            };
            let Some(source) = sender
                .as_ref()
                .and_then(|sender| sender.cast::<UIElement>().ok())
            else {
                return Ok(());
            };
            let Some((host, _, _)) = host_of(&source) else {
                return Ok(());
            };
            let Ok(host_element) = host.cast::<UIElement>() else {
                return Ok(());
            };
            let Ok(point) = args.GetCurrentPoint(&host_element) else {
                return Ok(());
            };
            let is_right = point
                .Properties()
                .and_then(|properties| properties.IsRightButtonPressed())
                .unwrap_or(false);
            if !is_right {
                return Ok(());
            }
            let _ = args.SetHandled(true);
            if let Ok(position) = point.Position() {
                PopupMenu(inner).show_on(&host, position.X as f64, position.Y as f64);
            }
            Ok(())
        });
        if let Ok(token) = element.PointerPressed(&handler) {
            self.0
                .attached
                .borrow_mut()
                .push((element, token, widget.boxed_clone()));
        }
    }

    /// プログラムからメニューを出す。位置は `widget` の**左上から**の
    /// 論理ピクセル (y は下向き)。
    pub fn open_at(&self, widget: &dyn Widget, x: f64, y: f64) {
        let element = widget.native_element();
        let Some((host, offset_x, offset_y)) = host_of(&element) else {
            return;
        };
        self.show_on(&host, offset_x + x, offset_y + y);
    }

    /// 受け皿を親へ載せ、指定の位置にメニューを出す。
    fn show_on(&self, host: &Panel, x: f64, y: f64) {
        self.close();
        let overlay = &self.0.surface.overlay;
        let _ = self.0.surface.frame.SetMargin(Thickness {
            Left: x,
            Top: y,
            Right: 0.0,
            Bottom: 0.0,
        });
        let Ok(element) = overlay.cast::<UIElement>() else {
            return;
        };
        // 行や列を切ってある親でも、受け皿は全面を覆う。
        let _ = XamlGrid::SetRow(overlay, 0);
        let _ = XamlGrid::SetColumn(overlay, 0);
        let _ = XamlGrid::SetRowSpan(overlay, OVERLAY_SPAN);
        let _ = XamlGrid::SetColumnSpan(overlay, OVERLAY_SPAN);
        let _ = Canvas::SetZIndex(overlay, OVERLAY_Z_INDEX);
        if host
            .Children()
            .and_then(|children| children.Append(&element))
            .is_ok()
        {
            *self.0.host.borrow_mut() = Some(host.clone());
            self.0.open.set(true);
        }
    }

    /// 出ているメニューを閉じる。出ていなければ何もしない。
    pub fn close(&self) {
        let Some(host) = self.0.host.borrow_mut().take() else {
            return;
        };
        self.0.open.set(false);
        let Ok(element) = self.0.surface.overlay.cast::<UIElement>() else {
            return;
        };
        if let Ok(children) = host.Children() {
            let mut index = 0u32;
            if children.IndexOf(&element, &mut index).unwrap_or(false) {
                let _ = children.RemoveAt(index);
            }
        }
    }

    /// ユーザーが選んだのと同じ経路で項目を選ぶ (テストや自動操作用)。
    ///
    /// 区切り線と、選べない項目は無視する。
    pub fn select(&self, index: usize) {
        let button = self.0.buttons.borrow().get(index).cloned().flatten();
        let Some(button) = button else {
            return;
        };
        if !button.IsEnabled().unwrap_or(false) {
            return;
        }
        self.close();
        self.0.handler.emit(index);
    }

    /// 項目が選ばれたときに、そのインデックスで呼ばれる。
    pub fn on_select(&self, f: impl FnMut(usize) + 'static) {
        self.0.handler.set(f);
    }

    /// メニューを載せる受け皿。バックエンド固有の脱出口として公開している。
    pub fn native_element(&self) -> UIElement {
        self.0
            .surface
            .overlay
            .cast::<UIElement>()
            .expect("Grid は UIElement である")
    }
}

/// WinRT のデリゲートは `Send` を要求するので、UI スレッド限定のセルに包む。
fn weak_cell(inner: &Rc<PopupMenuInner>) -> Arc<UiThreadCell<Weak<PopupMenuInner>>> {
    Arc::new(UiThreadCell::new(Rc::downgrade(inner)))
}

/// メニューを載せる親と、`element` のその親から見た位置を返す。
///
/// XAML ツリーを根までたどり、**いちばん外側の `Panel`** を親に選ぶ。
/// `TransformToVisual` はバインディングに無いため、位置は途中の
/// `ActualOffset` を足し合わせて求める (naui のレイアウトは平行移動だけ)。
fn host_of(element: &UIElement) -> Option<(Panel, f64, f64)> {
    let element = element.cast::<FrameworkElement>().ok()?;
    let mut chain = vec![element.clone()];
    let mut current = element;
    while let Some(parent) = current
        .Parent()
        .ok()
        .and_then(|parent| parent.cast::<FrameworkElement>().ok())
    {
        chain.push(parent.clone());
        current = parent;
    }

    let (index, host) = chain
        .iter()
        .enumerate()
        .rev()
        .find_map(|(index, item)| item.cast::<Panel>().ok().map(|panel| (index, panel)))?;
    if index == 0 {
        // 取り付け先そのものが親になることはない (自分の中には出せない)。
        return None;
    }

    let (mut x, mut y) = (0.0, 0.0);
    for item in chain.iter().take(index) {
        if let Ok(offset) = item.ActualOffset() {
            x += offset.X as f64;
            y += offset.Y as f64;
        }
    }
    Some((host, x, y))
}
