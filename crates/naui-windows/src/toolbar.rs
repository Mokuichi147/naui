//! The toolbar (WinUI 3's native `CommandBar`).
//!
//! | naui | WinUI 3 |
//! | --- | --- |
//! | `Toolbar` | `CommandBar` (`PrimaryCommands`) |
//! | Item | `AppBarButton` + `FontIcon` |
//! | Separator | `AppBarSeparator` |
//!
//! Labels are hidden by setting `DefaultLabelPosition` to `Collapsed`, laying out
//! only the icons (to match the other three platforms' toolbars). For the same
//! reason items are left-aligned (`CommandBar` defaults to right alignment, with the
//! left being the place for `Content`). The hidden labels go to
//! `AutomationProperties.Name` and `ToolTipService.ToolTip`, so they appear for
//! screen readers and tooltips.
//!
//! When there is not enough width, `CommandBar` moves items to the overflow menu by
//! itself. Items never get cut off and become unclickable.
//!
//! Icons are [`ToolbarIcon`](naui_core::ToolbarIcon) mapped to Segoe Fluent Icons
//! glyphs. `FontIcon`'s default typeface is Segoe Fluent Icons, so only the glyph is
//! passed.
//!
//! To match the other backends it is not a [`Widget`](crate::Widget); it is
//! attached to the right of the window title with
//! [`Window::set_toolbar`](crate::Window::set_toolbar) (the same position as macOS's
//! `NSToolbar` and Linux's `AdwHeaderBar`). Passing an element to `SetTitleBar`
//! would start window drags even over the interactive controls inside it, so the
//! drag region is split into two rectangles ([`crate::window`]).
//!
//! Each icon is 32 wide, the same as its height. `AppBarButton`'s default is 68, but
//! that width is for layouts showing the label below the icon and is excessive here
//! with labels hidden. For the same reason the height is reduced to the title bar's
//! [`CAPTION_HEIGHT`](crate::window) rather than 48, which leaves room for the label
//! ([`apply_compact_resources`]).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use naui_core::ToolbarIcon;
use naui_core::{Result, ToolbarItem};
use naui_winui3::Microsoft::UI::Xaml::Automation::AutomationProperties;
use naui_winui3::Microsoft::UI::Xaml::Controls::{
    AppBarButton, AppBarSeparator, CommandBar, CommandBarDefaultLabelPosition, FontIcon,
    ToolTipService,
};
use naui_winui3::Microsoft::UI::Xaml::Markup::XamlReader;
use naui_winui3::Microsoft::UI::Xaml::{
    FrameworkElement, HorizontalAlignment, ResourceDictionary, RoutedEventHandler, UIElement,
    VerticalAlignment,
};
use windows::Foundation::PropertyValue;
use windows_core::{Interface, HSTRING};

use crate::to_error;
use crate::ui_thread::UiThreadCell;
use crate::window::CAPTION_HEIGHT;

use crate::navigation::SelectHandler;

/// The width per icon. `AppBarButton`'s default is 68, but that width is for layouts
/// showing the label below the icon and is excessive in naui, which hides labels.
/// It is the same as [`ITEM_HEIGHT`], making the space around the icon equal on all
/// sides.
const ITEM_WIDTH: f64 = ITEM_HEIGHT;

/// The height per icon. It fits the title bar's height ([`CAPTION_HEIGHT`], the same
/// as the minimize, maximize and close buttons).
const ITEM_HEIGHT: f64 = CAPTION_HEIGHT;

/// The height of the icon box. `AppBarButton`'s template puts the icon in a 16
/// `Viewbox`. The separator line also ends up the same height, with the theme's
/// 8-unit margin above and below, so icons and separators line up.
const ICON_HEIGHT: f64 = 16.0;

/// How far to inset the hover / pressed underlay (`AppBarButtonInnerBorder`) inside
/// the button. The default vertical margin is 6, sized for a 48 band, which in a 32
/// band gives a flat shape 20 high and 28 wide. Making it 2, the same as left and
/// right, gives a 28 square.
const INNER_BORDER_INSET: f64 = 2.0;

struct ToolbarInner {
    native: CommandBar,
    items: RefCell<Vec<ToolbarItem>>,
    /// In the same order as the items. `None` at separators.
    buttons: RefCell<Vec<Option<AppBarButton>>>,
    handler: SelectHandler,
    /// Whether the whole toolbar is enabled. ANDed with the per-item settings.
    enabled: Cell<bool>,
}

/// A row of frequently used actions attached to the top of the window.
///
/// Not a [`Widget`](crate::Widget). Attach it with
/// [`Window::set_toolbar`](crate::Window::set_toolbar). Unlike navigation, it
/// **has no selected item**; every click calls [`on_activate`](Self::on_activate)
/// with the index. The index is the position in the sequence including separators;
/// separators are never returned.
#[derive(Clone)]
pub struct Toolbar(Rc<ToolbarInner>);

impl Toolbar {
    pub(crate) fn new() -> Result<Self> {
        let native = CommandBar::new().map_err(|e| to_error("CommandBar の生成", e))?;
        native
            .SetDefaultLabelPosition(CommandBarDefaultLabelPosition::Collapsed)
            .map_err(|e| to_error("ツールバーのラベル位置の設定", e))?;
        // `CommandBar` は `PrimaryCommands` を右へ寄せる (左は `Content` の
        // 場所)。naui のツールバーはほかの 3 環境と同じく左詰めにしたいので、
        // 帯そのものを左へ寄せて中身の幅にする。
        native
            .SetHorizontalAlignment(HorizontalAlignment::Left)
            .map_err(|e| to_error("ツールバーの配置設定", e))?;
        // 帯もタイトルバーの高さへ収める。テーマの `MinHeight` が 48 なので、
        // `Height` だけでは縮まない。
        native
            .SetMinHeight(ITEM_HEIGHT)
            .map_err(|e| to_error("ツールバーの最小高さの設定", e))?;
        native
            .SetHeight(ITEM_HEIGHT)
            .map_err(|e| to_error("ツールバーの高さの設定", e))?;
        // 項目の並びと項目そのものの寸法はテンプレートが持っている。帯へ
        // 入れたリソースは中の要素からも引かれるので、ここで一度だけ渡す。
        apply_compact_resources(
            &native
                .cast::<FrameworkElement>()
                .map_err(|e| to_error("ツールバーの変換", e))?,
        )?;
        Ok(Self(Rc::new(ToolbarInner {
            native,
            items: RefCell::new(Vec::new()),
            buttons: RefCell::new(Vec::new()),
            handler: SelectHandler::new(),
            enabled: Cell::new(true),
        })))
    }

    /// Rebuilds the items. The previous items are removed.
    ///
    /// The index is the position in the sequence including separators.
    pub fn set_items(&self, items: &[ToolbarItem]) {
        let _ = self.rebuild(items);
    }

    fn rebuild(&self, items: &[ToolbarItem]) -> Result<()> {
        let commands = self
            .0
            .native
            .PrimaryCommands()
            .map_err(|e| to_error("ツールバーの項目取得", e))?;
        commands
            .Clear()
            .map_err(|e| to_error("ツールバーの項目消去", e))?;
        self.0.buttons.borrow_mut().clear();

        let whole = self.0.enabled.get();
        let mut buttons = Vec::with_capacity(items.len());
        for (index, item) in items.iter().enumerate() {
            if item.is_separator() {
                let separator =
                    AppBarSeparator::new().map_err(|e| to_error("ツールバーの区切り生成", e))?;
                let _ = separator.SetVerticalAlignment(VerticalAlignment::Top);
                apply_separator_metrics(&separator);
                let metrics = RoutedEventHandler::new(|sender, _| {
                    if let Some(separator) = sender
                        .as_ref()
                        .and_then(|s| s.cast::<AppBarSeparator>().ok())
                    {
                        apply_separator_metrics(&separator);
                    }
                    Ok(())
                });
                let _ = separator.Loaded(&metrics);
                commands
                    .Append(&separator)
                    .map_err(|e| to_error("ツールバーへの区切り追加", e))?;
                buttons.push(None);
                continue;
            }

            let button = self.build_button(item.icon, &item.label, index)?;
            let _ = button.SetIsEnabled(item.enabled && whole);
            commands
                .Append(&button)
                .map_err(|e| to_error("ツールバーへの項目追加", e))?;
            buttons.push(Some(button));
        }

        *self.0.buttons.borrow_mut() = buttons;
        self.0.items.borrow_mut().clear();
        self.0.items.borrow_mut().extend_from_slice(items);
        Ok(())
    }

    fn build_button(&self, icon: ToolbarIcon, label: &str, index: usize) -> Result<AppBarButton> {
        let button = AppBarButton::new().map_err(|e| to_error("ツールバーのボタン生成", e))?;
        button
            .SetVerticalAlignment(VerticalAlignment::Top)
            .map_err(|e| to_error("ツールバーのボタン配置の設定", e))?;
        apply_button_metrics(&button);
        // 置かれ方が変わると読み込み直されるので、そのたびに渡し直す。
        // ハンドラは `sender` から引くだけにして、ボタンを掴まない
        // (掴むと購読との間で循環する)。
        let metrics = RoutedEventHandler::new(|sender, _| {
            if let Some(button) = sender.as_ref().and_then(|s| s.cast::<AppBarButton>().ok()) {
                apply_button_metrics(&button);
            }
            Ok(())
        });
        button
            .Loaded(&metrics)
            .map_err(|e| to_error("ツールバーのボタンの読み込み購読", e))?;
        let glyph = FontIcon::new().map_err(|e| to_error("ツールバーの印の生成", e))?;
        glyph
            .SetGlyph(&HSTRING::from(icon.fluent_glyph().to_string()))
            .map_err(|e| to_error("ツールバーの印の設定", e))?;
        button
            .SetIcon(&glyph)
            .map_err(|e| to_error("ツールバーの印の取り付け", e))?;

        // ラベルは隠すので、読み上げとツールチップへ回す。
        let text = HSTRING::from(label);
        button
            .SetLabel(&text)
            .map_err(|e| to_error("ツールバーのラベル設定", e))?;
        let _ = AutomationProperties::SetName(&button, &text);
        if let Ok(tip) = PropertyValue::CreateString(&text) {
            let _ = ToolTipService::SetToolTip(&button, &tip);
        }

        // ハンドルを強く持つと購読との間で循環するため、弱参照にする。
        let state = UiThreadCell::new(Rc::downgrade(&self.0));
        let handler = RoutedEventHandler::new(move |_sender, _args| {
            state.with_mut(|weak| {
                if let Some(inner) = weak.upgrade() {
                    inner.handler.emit(index);
                }
            });
            Ok(())
        });
        button
            .Click(&handler)
            .map_err(|e| to_error("AppBarButton の購読", e))?;
        Ok(button)
    }

    /// The number of items, separators included.
    pub fn len(&self) -> usize {
        self.0.items.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Enables or disables one item. Does nothing for separators or out of range.
    pub fn set_item_enabled(&self, index: usize, enabled: bool) {
        let mut items = self.0.items.borrow_mut();
        let Some(item) = items.get_mut(index) else {
            return;
        };
        if item.is_separator() {
            return;
        }
        item.enabled = enabled;
        drop(items);
        self.apply_enabled();
    }

    /// Whether the item can be clicked now. `false` for separators and out of range.
    pub fn is_item_enabled(&self, index: usize) -> bool {
        self.0.enabled.get()
            && self
                .0
                .items
                .borrow()
                .get(index)
                .is_some_and(|item| !item.is_separator() && item.enabled)
    }

    /// Enables or disables the whole toolbar. Per-item settings are kept.
    pub fn set_enabled(&self, enabled: bool) {
        self.0.enabled.set(enabled);
        self.apply_enabled();
    }

    /// Applies the per-item settings and the overall setting to the native side.
    fn apply_enabled(&self) {
        let whole = self.0.enabled.get();
        let items = self.0.items.borrow();
        for (button, item) in self.0.buttons.borrow().iter().zip(items.iter()) {
            if let Some(button) = button {
                let _ = button.SetIsEnabled(item.enabled && whole);
            }
        }
    }

    /// Runs the item as if the user had clicked it.
    ///
    /// Does nothing for separators, disabled items or out of range.
    pub fn activate(&self, index: usize) {
        if self.is_item_enabled(index) {
            self.0.handler.emit(index);
        }
    }

    /// Called with the index when an item is clicked. Setting it again removes the
    /// previous callback.
    pub fn on_activate(&self, f: impl FnMut(usize) + 'static) {
        self.0.handler.set(f);
    }

    /// The WinUI 3 button for the item. `None` for separators and out of range. Public
    /// as a backend-specific escape hatch.
    pub fn native_button(&self, index: usize) -> Option<AppBarButton> {
        self.0.buttons.borrow().get(index)?.clone()
    }

    /// The `CommandBar` laying out the items. Public as a backend-specific escape
    /// hatch.
    pub fn native_command_bar(&self) -> CommandBar {
        self.0.native.clone()
    }

    /// The element inserted into the window. Used only by [`crate::Window`].
    pub(crate) fn mount(&self) -> UIElement {
        self.0
            .native
            .cast::<UIElement>()
            .expect("CommandBar は UIElement である")
    }
}

/// Replaces theme resources so the band and items fit the title bar's height.
///
/// The templates of `CommandBar`, `AppBarButton` and `AppBarSeparator` are built
/// assuming a 48 band that shows labels below the icons. Giving the elements a
/// `Height` does not shrink the insides of the templates, so the theme resources that
/// decide those dimensions are overridden with the same names. Theme resources are
/// looked up through the parents when loaded, so putting them on the band once
/// applies them to the items inside too, without affecting other `CommandBar`s.
///
/// - `AppBarThemeCompactHeight` / `AppBarThemeMinHeight`: the height of the row of
///   items. The separator line is what remains after 8 above and below within it
///   (the same as [`ICON_HEIGHT`]).
/// - `AppBarButtonContentViewboxCollapsedMargin`: the space around the icon box
///   ([`ICON_HEIGHT`]) when labels are hidden. Top and bottom are equal to center it.
/// - `AppBarButtonInnerBorderMargin` / `...CompactMargin`: the margin of the
///   underlay colored on hover / press ([`INNER_BORDER_INSET`]). The default is 6
///   above and below for a 48 band, which is flat at 32. naui, with labels hidden,
///   never enters the `...CompactMargin` state, but both are set so the shape is the
///   same either way.
fn apply_compact_resources(element: &FrameworkElement) -> Result<()> {
    let pad = (ITEM_HEIGHT - ICON_HEIGHT) / 2.0;
    let inset = INNER_BORDER_INSET;
    let dictionary = XamlReader::Load(&HSTRING::from(format!(
        r##"<ResourceDictionary xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
            xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml">
            <x:Double x:Key="AppBarThemeCompactHeight">{ITEM_HEIGHT}</x:Double>
            <x:Double x:Key="AppBarThemeMinHeight">{ITEM_HEIGHT}</x:Double>
            <Thickness x:Key="AppBarButtonContentViewboxCollapsedMargin">0,{pad},0,{pad}</Thickness>
            <Thickness x:Key="AppBarButtonInnerBorderMargin">{inset},{inset},{inset},{inset}</Thickness>
            <Thickness x:Key="AppBarButtonInnerBorderCompactMargin">{inset},{inset},{inset},{inset}</Thickness>
        </ResourceDictionary>"##
    )))
    .map_err(|e| to_error("ツールバーの寸法リソースの生成", e))?
    .cast::<ResourceDictionary>()
    .map_err(|e| to_error("ツールバーの寸法リソースへの変換", e))?;
    element
        .SetResources(&dictionary)
        .map_err(|e| to_error("ツールバーの寸法リソースの登録", e))
}

/// Dimensions that should take effect only while in the band.
///
/// When there is not enough width, `CommandBar` moves items to the overflow menu.
/// Menu rows are a wide shape with the label right of the icon, and the menu's style
/// makes the width `Auto`. Leaving the 32-square dimensions as local values here
/// would squash the label to 32 wide, since local values beat styles. So the
/// dimensions are released while moved, and given back on return to the band.
///
/// The projection has no `ClearValue`, so `Width` and `Height` are given
/// `f64::NAN`, the same as XAML's `Auto`, and `MinHeight` 0, deferring to the
/// defaults.
fn apply_button_metrics(button: &AppBarButton) {
    if button.IsInOverflow().unwrap_or(false) {
        let _ = button.SetWidth(f64::NAN);
        let _ = button.SetHeight(f64::NAN);
        let _ = button.SetMinHeight(0.0);
        return;
    }
    let _ = button.SetWidth(ITEM_WIDTH);
    let _ = button.SetHeight(ITEM_HEIGHT);
    let _ = button.SetMinHeight(ITEM_HEIGHT);
}

/// The same for separators. In the menu they become full-row lines, so the height
/// is released.
fn apply_separator_metrics(separator: &AppBarSeparator) {
    if separator.IsInOverflow().unwrap_or(false) {
        let _ = separator.SetHeight(f64::NAN);
        let _ = separator.SetMinHeight(0.0);
        return;
    }
    let _ = separator.SetHeight(ITEM_HEIGHT);
    let _ = separator.SetMinHeight(ITEM_HEIGHT);
}
