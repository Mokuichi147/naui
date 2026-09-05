//! ツールバー (WinUI 3 のネイティブ `CommandBar`)。
//!
//! | naui | WinUI 3 |
//! | --- | --- |
//! | `Toolbar` | `CommandBar` (`PrimaryCommands`) |
//! | 項目 | `AppBarButton` + `FontIcon` |
//! | 区切り | `AppBarSeparator` |
//!
//! ラベルは `DefaultLabelPosition` を `Collapsed` にして隠し、印だけを並べる
//! (ほかの 3 環境のツールバーに合わせる)。隠したラベルは
//! `AutomationProperties.Name` と `ToolTipService.ToolTip` へ回すので、
//! 読み上げとツールチップには出る。
//!
//! 幅が足りなくなると `CommandBar` が自分で項目をオーバーフローメニューへ
//! 送る。切り詰めて押せなくなることはない。
//!
//! アイコンは [`ToolbarIcon`](naui_core::ToolbarIcon) を Segoe Fluent Icons の
//! 字面へ写したもの。`FontIcon` の既定の書体がその Segoe Fluent Icons なので、
//! 字面だけを渡す。
//!
//! ほかのバックエンドに合わせて [`Widget`](crate::Widget) にはせず、
//! [`Window::set_toolbar`](crate::Window::set_toolbar) でウィンドウの
//! 上端へ取り付ける。タイトルバーはウィンドウのドラッグ領域なので、
//! そこではなくタイトルバーと中身の間の行へ置く。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use naui_core::ToolbarIcon;
use naui_core::{Result, ToolbarItem};
use naui_winui3::Microsoft::UI::Xaml::Automation::AutomationProperties;
use naui_winui3::Microsoft::UI::Xaml::Controls::{
    AppBarButton, AppBarSeparator, CommandBar, CommandBarDefaultLabelPosition, FontIcon,
    ToolTipService,
};
use naui_winui3::Microsoft::UI::Xaml::{RoutedEventHandler, UIElement};
use windows::Foundation::PropertyValue;
use windows_core::{Interface, HSTRING};

use crate::to_error;
use crate::ui_thread::UiThreadCell;

use crate::navigation::SelectHandler;

struct ToolbarInner {
    native: CommandBar,
    items: RefCell<Vec<ToolbarItem>>,
    /// 項目と同じ並び。区切りのところは `None`。
    buttons: RefCell<Vec<Option<AppBarButton>>>,
    handler: SelectHandler,
    /// ツールバー全体の有効・無効。項目ごとの指定と AND を取る。
    enabled: Cell<bool>,
}

/// ウィンドウの上端に付く、よく使う操作の並び。
///
/// [`Widget`](crate::Widget) ではない。
/// [`Window::set_toolbar`](crate::Window::set_toolbar) で取り付ける。
/// ナビゲーションと違い**選ばれている項目を持たず**、押されるたびに
/// そのインデックスで [`on_activate`](Self::on_activate) が呼ばれる。
/// インデックスは区切りを含めた並びの位置で、区切りが返ることはない。
#[derive(Clone)]
pub struct Toolbar(Rc<ToolbarInner>);

impl Toolbar {
    pub(crate) fn new() -> Result<Self> {
        let native = CommandBar::new().map_err(|e| to_error("CommandBar の生成", e))?;
        native
            .SetDefaultLabelPosition(CommandBarDefaultLabelPosition::Collapsed)
            .map_err(|e| to_error("ツールバーのラベル位置の設定", e))?;
        Ok(Self(Rc::new(ToolbarInner {
            native,
            items: RefCell::new(Vec::new()),
            buttons: RefCell::new(Vec::new()),
            handler: SelectHandler::new(),
            enabled: Cell::new(true),
        })))
    }

    /// 項目を作り直す。以前の項目は取り除かれる。
    ///
    /// インデックスは区切りを含めた並びの位置。
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

    /// 区切りを含めた項目数。
    pub fn len(&self) -> usize {
        self.0.items.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// 項目 1 つの有効・無効を変える。区切りと範囲外は何もしない。
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

    /// いま押せる項目か。区切りと範囲外は `false`。
    pub fn is_item_enabled(&self, index: usize) -> bool {
        self.0.enabled.get()
            && self
                .0
                .items
                .borrow()
                .get(index)
                .is_some_and(|item| !item.is_separator() && item.enabled)
    }

    /// ツールバー全体の有効・無効を変える。項目ごとの指定は残る。
    pub fn set_enabled(&self, enabled: bool) {
        self.0.enabled.set(enabled);
        self.apply_enabled();
    }

    /// 項目ごとの指定と全体の指定をネイティブへ反映する。
    fn apply_enabled(&self) {
        let whole = self.0.enabled.get();
        let items = self.0.items.borrow();
        for (button, item) in self.0.buttons.borrow().iter().zip(items.iter()) {
            if let Some(button) = button {
                let _ = button.SetIsEnabled(item.enabled && whole);
            }
        }
    }

    /// 利用者が押したのと同じように項目を実行する。
    ///
    /// 区切り・押せない項目・範囲外は何もしない。
    pub fn activate(&self, index: usize) {
        if self.is_item_enabled(index) {
            self.0.handler.emit(index);
        }
    }

    /// 項目が押されたときに、そのインデックスで呼ばれる。
    /// 設定し直すと以前のコールバックは外れる。
    pub fn on_activate(&self, f: impl FnMut(usize) + 'static) {
        self.0.handler.set(f);
    }

    /// 項目に対応する WinUI 3 のボタン。区切りと範囲外は `None`。
    /// バックエンド固有の脱出口として公開している。
    pub fn native_button(&self, index: usize) -> Option<AppBarButton> {
        self.0.buttons.borrow().get(index)?.clone()
    }

    /// 項目を並べている `CommandBar`。
    /// バックエンド固有の脱出口として公開している。
    pub fn native_command_bar(&self) -> CommandBar {
        self.0.native.clone()
    }

    /// ウィンドウへ差し込む要素。[`crate::Window`] だけが使う。
    pub(crate) fn mount(&self) -> UIElement {
        self.0
            .native
            .cast::<UIElement>()
            .expect("CommandBar は UIElement である")
    }
}
