//! ツールバー (WinUI 3)。
//!
//! `CommandBar` と `AppBarButton` は `winio-winui3` のバインディングに
//! 含まれていないため、標準の `Button` を `StackPanel` へ横に並べて
//! 構成している。区切りは幅 1 の `Border` で、見た目は Fluent のまま。
//!
//! アイコンは [`ToolbarIcon`](naui_core::ToolbarIcon) を Segoe Fluent Icons
//! の字面へ写したもの。`FontIcon` と `Border` はバインディングに型が無いため、
//! `Border` と同じく XAML から読み込む。`label` はツールチップと読み上げに使う。
//!
//! ほかのバックエンドに合わせて [`Widget`](crate::Widget) にはせず、
//! [`Window::set_toolbar`](crate::Window::set_toolbar) でウィンドウの
//! 上端へ取り付ける。タイトルバーはウィンドウのドラッグ領域なので、
//! そこではなくタイトルバーと中身の間の行へ置く。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use naui_core::ToolbarIcon;
use naui_core::{Result, ToolbarItem};
use windows_core::{Interface, HSTRING};
use winui3::Microsoft::UI::Xaml::Controls::{
    Button as XamlButton, Orientation as XamlOrientation, StackPanel,
};
use winui3::Microsoft::UI::Xaml::Markup::XamlReader;
use winui3::Microsoft::UI::Xaml::{RoutedEventHandler, UIElement};

use crate::navigation::{append, panel, SelectHandler};
use crate::to_error;
use crate::ui_thread::UiThreadCell;

/// 区切り。`Border` は `winio-winui3` のバインディングに型が無いため、
/// XAML から読み込んで `UIElement` として扱う。`ThemeResource` は未パッケージ
/// 起動で解決できないことがあるので、明暗どちらの配色でも見える半透明の
/// グレーを直に指定する。
const SEPARATOR_XAML: &str = r##"<Border xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
    Width="1" MinHeight="16" VerticalAlignment="Stretch"
    Margin="2,4,2,4" Background="#40808080"/>"##;

/// XAML の属性値として安全な文字列にする。ラベルはアプリが決めるため、
/// 引用符や記号が入っていても壊れないようにエスケープする。
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(c),
        }
    }
    out
}

/// アイコン付きボタンの XAML。字面は数値参照で埋める。
fn button_xaml(icon: ToolbarIcon, label: &str) -> String {
    let label = escape(label);
    format!(
        r#"<Button xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
            ToolTipService.ToolTip="{label}" AutomationProperties.Name="{label}"
            Padding="8,6,8,6" Background="Transparent" BorderThickness="0">
            <FontIcon FontFamily="Segoe Fluent Icons" FontSize="16" Glyph="&#x{glyph:04X};"/>
        </Button>"#,
        label = label,
        glyph = icon.fluent_glyph() as u32,
    )
}

struct ToolbarInner {
    native: StackPanel,
    items: RefCell<Vec<ToolbarItem>>,
    /// 項目と同じ並び。区切りのところは `None`。
    buttons: RefCell<Vec<Option<XamlButton>>>,
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
        Ok(Self(Rc::new(ToolbarInner {
            native: panel(XamlOrientation::Horizontal, 6.0)?,
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
        self.0
            .native
            .Children()
            .and_then(|children| children.Clear())
            .map_err(|e| to_error("ツールバーの項目消去", e))?;
        self.0.buttons.borrow_mut().clear();

        let whole = self.0.enabled.get();
        let mut buttons = Vec::with_capacity(items.len());
        for (index, item) in items.iter().enumerate() {
            if item.is_separator() {
                let separator = XamlReader::Load(&HSTRING::from(SEPARATOR_XAML))
                    .and_then(|o| o.cast::<UIElement>())
                    .map_err(|e| to_error("ツールバーの区切り生成", e))?;
                append(&self.0.native, &separator)?;
                buttons.push(None);
                continue;
            }

            // アイコン・ツールチップ・読み上げ名をまとめて XAML で組み立てる。
            let button = XamlReader::Load(&HSTRING::from(button_xaml(item.icon, &item.label)))
                .and_then(|o| o.cast::<XamlButton>())
                .map_err(|e| to_error("ツールバーのボタン生成", e))?;
            let _ = button.SetIsEnabled(item.enabled && whole);

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
                .map_err(|e| to_error("Button の購読", e))?;

            let element = button
                .cast::<UIElement>()
                .map_err(|e| to_error("項目の要素化", e))?;
            append(&self.0.native, &element)?;
            buttons.push(Some(button));
        }

        *self.0.buttons.borrow_mut() = buttons;
        self.0.items.borrow_mut().clear();
        self.0.items.borrow_mut().extend_from_slice(items);
        Ok(())
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
    pub fn native_button(&self, index: usize) -> Option<XamlButton> {
        self.0.buttons.borrow().get(index)?.clone()
    }

    /// 項目を並べている `StackPanel`。
    /// バックエンド固有の脱出口として公開している。
    pub fn native_panel(&self) -> StackPanel {
        self.0.native.clone()
    }

    /// ウィンドウへ差し込む要素。[`crate::Window`] だけが使う。
    pub(crate) fn mount(&self) -> UIElement {
        self.0
            .native
            .cast::<UIElement>()
            .expect("StackPanel は UIElement である")
    }
}
