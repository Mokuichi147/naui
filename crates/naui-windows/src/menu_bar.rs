//! メニューバー (`Button` + `MenuFlyout` の横並び)。
//!
//! | naui | WinUI 3 |
//! | --- | --- |
//! | `MenuBar` | 地色を消した `Button` を並べた `StackPanel` |
//! | メニュー | `MenuFlyout` (見出しの `Button.Flyout`) |
//! | 項目 | `MenuFlyoutItem` |
//! | 区切り線 | `MenuFlyoutSeparator` |
//!
//! メニューを出す・閉じる・影・角丸・ライトディスミス・キーボード操作 (矢印と
//! Esc)・画面端での回り込みは、すべて `MenuFlyout` が持つ。見出しを押したら
//! 開く動きも `Button.Flyout` に預けるだけでよい。
//!
//! ## `MenuBar` を使っていない理由
//!
//! WinUI 3 には `MenuBar` / `MenuBarItem` があるが、[`naui_winui3`] の投影に
//! 含めていない。投影はコミットしてある生成物で、型を増やすと全体を作り直す
//! ことになるため、naui は**すでに投影してある標準コントロールの組み合わせ**で
//! 同じものを組む (ナビゲーションや `Tabs` と同じ方針)。
//!
//! ## ショートカット
//!
//! `KeyboardAccelerator` も投影に無いので、押されたキーは
//! [`crate::Window`] が根の `KeyDown` で拾い、メニューバーへ渡す。
//! 修飾キーはその場で `GetKeyState` から読む
//! (`KeyRoutedEventArgs` に修飾キーが乗らないため)。項目の右端の表示は
//! `MenuFlyoutItem.KeyboardAcceleratorTextOverride` に入れる。

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::sync::Arc;

use naui_core::{MenuItem, MenuSpec, Result};
use naui_winui3::Microsoft::UI::Xaml::Controls::{
    Button as XamlButton, MenuFlyout, MenuFlyoutItem, MenuFlyoutSeparator,
    Orientation as XamlOrientation, StackPanel,
};
use naui_winui3::Microsoft::UI::Xaml::Markup::XamlReader;
use naui_winui3::Microsoft::UI::Xaml::RoutedEventHandler;
use windows_core::{Interface, HSTRING};

use crate::navigation::{panel, text_block};
use crate::to_error;
use crate::ui_thread::{HandlerCell, UiThreadCell};

/// 見出しのボタン。メニューバーらしく、地色も枠も出さず文字だけを見せる
/// (押したときの淡い塗りは `Button` の標準テンプレートが持つ)。
const TITLE_BUTTON_XAML: &str = r##"<Button
    xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
    Background="Transparent" BorderThickness="0" Padding="10,4"
    VerticalAlignment="Center"/>"##;

/// 押された項目の通知先。
///
/// WinRT のデリゲートは `Send + Sync` を要求するため、`UiThreadCell` に載せる。
/// 呼び出しの間だけクロージャを取り出すので、通知の中から同じメニューバーを
/// 組み替えても二重借用にならない。
#[derive(Clone)]
struct ActivateHandler(HandlerCell<dyn FnMut(usize, usize)>);

impl ActivateHandler {
    fn new() -> Self {
        Self(Arc::new(UiThreadCell::new(None)))
    }

    fn set(&self, f: impl FnMut(usize, usize) + 'static) {
        self.0.with_mut(|slot| *slot = Some(Box::new(f)));
    }

    fn emit(&self, menu: usize, item: usize) {
        let Some(mut f) = self.0.with_mut(|slot| slot.take()) else {
            return;
        };
        f(menu, item);
        self.0.with_mut(|slot| {
            // 呼び出しの中で差し替えられていたら、新しいほうを残す。
            if slot.is_none() {
                *slot = Some(f);
            }
        });
    }
}

/// 見出し 1 つぶんのコントロール。
struct MenuParts {
    /// 見出しのボタン。
    title: XamlButton,
    /// 見出しの下に出るメニュー。
    flyout: MenuFlyout,
    /// 項目ごとの `MenuFlyoutItem`。区切り線の位置は `None`。
    items: Vec<Option<MenuFlyoutItem>>,
}

struct MenuBarInner {
    native: StackPanel,
    menus: RefCell<Vec<MenuSpec>>,
    parts: RefCell<Vec<MenuParts>>,
    handler: ActivateHandler,
    /// メニューバー全体の有効・無効。項目ごとの指定と AND を取る。
    enabled: Cell<bool>,
}

/// ウィンドウの上端に付く、OS のアプリケーションメニュー。
///
/// 画面に並ぶウィジェットではないので [`Widget`](crate::Widget) ではない。
/// [`Window::set_menu_bar`](crate::Window::set_menu_bar) で取り付ける。
/// 項目が押されるたびに、その **(見出しのインデックス, 項目のインデックス)**
/// で [`on_activate`](Self::on_activate) が呼ばれる。項目のインデックスは
/// 区切り線を含めた並びの位置で、区切り線が返ることはない。
#[derive(Clone)]
pub struct MenuBar(Rc<MenuBarInner>);

impl MenuBar {
    pub(crate) fn new() -> Result<Self> {
        // タイトルバーの下に敷く帯なので、見出しは詰めて並べる。
        let native = panel(XamlOrientation::Horizontal, 0.0)?;
        Ok(Self(Rc::new(MenuBarInner {
            native,
            menus: RefCell::new(Vec::new()),
            parts: RefCell::new(Vec::new()),
            handler: ActivateHandler::new(),
            enabled: Cell::new(true),
        })))
    }

    /// メニューを作り直す。以前のメニューは取り除かれる。
    ///
    /// 見出しのインデックスは渡した並びの位置、項目のインデックスは
    /// 区切り線を含めた並びの位置。
    pub fn set_menus(&self, menus: &[MenuSpec]) {
        let Ok(children) = self.0.native.Children() else {
            return;
        };
        let _ = children.Clear();
        self.0.parts.borrow_mut().clear();

        let whole = self.0.enabled.get();
        let mut parts = Vec::with_capacity(menus.len());
        for (menu_index, spec) in menus.iter().enumerate() {
            let Ok(title) = title_button(&spec.title) else {
                continue;
            };
            let _ = title.SetIsEnabled(whole);
            let Ok(flyout) = MenuFlyout::new() else {
                continue;
            };

            let mut items = Vec::with_capacity(spec.items.len());
            if let Ok(entries) = flyout.Items() {
                for (item_index, entry) in spec.items.iter().enumerate() {
                    if entry.is_separator() {
                        if let Ok(separator) = MenuFlyoutSeparator::new() {
                            let _ = entries.Append(&separator);
                        }
                        items.push(None);
                        continue;
                    }
                    match self.build_item(entry, menu_index, item_index, whole) {
                        Ok(native) => {
                            let _ = entries.Append(&native);
                            items.push(Some(native));
                        }
                        Err(_) => items.push(None),
                    }
                }
            }

            // 見出しを押したら開くところは `Button` が引き受ける。
            let _ = title.SetFlyout(&flyout);
            let _ = children.Append(&title);
            parts.push(MenuParts {
                title,
                flyout,
                items,
            });
        }

        *self.0.parts.borrow_mut() = parts;
        let mut stored = self.0.menus.borrow_mut();
        stored.clear();
        stored.extend_from_slice(menus);
    }

    fn build_item(
        &self,
        entry: &MenuItem,
        menu_index: usize,
        item_index: usize,
        whole: bool,
    ) -> Result<MenuFlyoutItem> {
        let native = MenuFlyoutItem::new().map_err(|e| to_error("メニュー項目の生成", e))?;
        native
            .SetText(&HSTRING::from(entry.label.as_str()))
            .map_err(|e| to_error("メニュー項目の文字設定", e))?;
        let _ = native.SetIsEnabled(entry.enabled && whole);
        if let Some(shortcut) = entry.shortcut {
            // 右端の表示だけ。押されたキーは `handle_key` が拾う。
            let _ = native.SetKeyboardAcceleratorTextOverride(&HSTRING::from(shortcut.label()));
        }

        // ハンドルを強く持つと購読との間で循環するため、弱参照にする。
        let weak = weak_cell(&self.0);
        let handler = RoutedEventHandler::new(move |_sender, _args| {
            if let Some(inner) = weak.try_with_mut(|weak| weak.upgrade()).flatten() {
                // 閉じるのは `MenuFlyout` が自分で行う。
                MenuBar(inner).0.handler.emit(menu_index, item_index);
            }
            Ok(())
        });
        native
            .Click(&handler)
            .map_err(|e| to_error("メニュー項目の購読", e))?;
        Ok(native)
    }

    /// 見出しの数。
    pub fn len(&self) -> usize {
        self.0.menus.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// 見出し 1 つが持つ、区切り線を含めた項目数。範囲外は `0`。
    pub fn menu_len(&self, menu: usize) -> usize {
        self.0
            .menus
            .borrow()
            .get(menu)
            .map_or(0, |spec| spec.items.len())
    }

    /// 項目 1 つの有効・無効を変える。区切り線と範囲外は何もしない。
    pub fn set_item_enabled(&self, menu: usize, item: usize, enabled: bool) {
        let mut menus = self.0.menus.borrow_mut();
        let Some(entry) = menus
            .get_mut(menu)
            .and_then(|spec| spec.items.get_mut(item))
        else {
            return;
        };
        if entry.is_separator() {
            return;
        }
        entry.enabled = enabled;
        drop(menus);
        self.apply_enabled();
    }

    /// いま押せる項目か。区切り線と範囲外は `false`。
    pub fn is_item_enabled(&self, menu: usize, item: usize) -> bool {
        self.0.enabled.get()
            && self
                .0
                .menus
                .borrow()
                .get(menu)
                .and_then(|spec| spec.items.get(item))
                .is_some_and(|entry| !entry.is_separator() && entry.enabled)
    }

    /// メニューバー全体の有効・無効を変える。項目ごとの指定は残る。
    pub fn set_enabled(&self, enabled: bool) {
        self.0.enabled.set(enabled);
        if !enabled {
            self.close();
        }
        self.apply_enabled();
    }

    /// 項目ごとの指定と全体の指定をネイティブへ反映する。
    fn apply_enabled(&self) {
        let whole = self.0.enabled.get();
        let menus = self.0.menus.borrow();
        for (spec, parts) in menus.iter().zip(self.0.parts.borrow().iter()) {
            let _ = parts.title.SetIsEnabled(whole);
            for (entry, native) in spec.items.iter().zip(parts.items.iter()) {
                if let Some(native) = native {
                    let _ = native.SetIsEnabled(entry.enabled && whole);
                }
            }
        }
    }

    /// 出ているメニューを閉じる。出ていなければ何もしない。
    ///
    /// ほかの 3 環境には「メニューをプログラムから閉じる」手段が無いので、
    /// 公開 API にはしない。
    pub(crate) fn close(&self) {
        for parts in self.0.parts.borrow().iter() {
            let _ = parts.flyout.Hide();
        }
    }

    /// 利用者が選んだのと同じように項目を実行する。
    ///
    /// 区切り線・押せない項目・範囲外は何もしない。
    pub fn activate(&self, menu: usize, item: usize) {
        if self.is_item_enabled(menu, item) {
            self.0.handler.emit(menu, item);
        }
    }

    /// 項目が押されたときに、その見出しと項目のインデックスで呼ばれる。
    /// 設定し直すと以前のコールバックは外れる。
    pub fn on_activate(&self, f: impl FnMut(usize, usize) + 'static) {
        self.0.handler.set(f);
    }

    /// 見出しを並べている `StackPanel`。
    /// バックエンド固有の脱出口として公開している。
    pub fn native_panel(&self) -> StackPanel {
        self.0.native.clone()
    }

    /// 見出しの下に出る `MenuFlyout`。範囲外は `None`。
    /// バックエンド固有の脱出口として公開している。
    pub fn native_flyout(&self, menu: usize) -> Option<MenuFlyout> {
        Some(self.0.parts.borrow().get(menu)?.flyout.clone())
    }

    /// 項目に対応する `MenuFlyoutItem`。区切り線と範囲外は `None`。
    /// バックエンド固有の脱出口として公開している。
    pub fn native_item(&self, menu: usize, item: usize) -> Option<MenuFlyoutItem> {
        self.0.parts.borrow().get(menu)?.items.get(item)?.clone()
    }

    /// ウィンドウへ差し込む要素。[`crate::Window`] だけが使う。
    pub(crate) fn mount(&self) -> StackPanel {
        self.0.native.clone()
    }

    /// 押されたキーがショートカットなら実行し、`true` を返す。
    ///
    /// [`crate::Window`] が根の `KeyDown` から呼ぶ。`key` は仮想キーコード。
    pub(crate) fn handle_key(&self, key: i32) -> bool {
        if !self.0.enabled.get() {
            return false;
        }
        let (primary, shift, alt) = modifiers();
        if !primary {
            return false;
        }
        // 通知の中でメニューを組み替えられても二重借用にならないよう、
        // 借用を返してから呼ぶ。
        let hit = {
            let menus = self.0.menus.borrow();
            menus.iter().enumerate().find_map(|(menu, spec)| {
                spec.items
                    .iter()
                    .enumerate()
                    .find(|(_, entry)| {
                        !entry.is_separator()
                            && entry.enabled
                            && entry.shortcut.is_some_and(|s| {
                                s.virtual_key() == key && s.shift == shift && s.alt == alt
                            })
                    })
                    .map(|(item, _)| (menu, item))
            })
        };
        let Some((menu, item)) = hit else {
            return false;
        };
        self.close();
        self.0.handler.emit(menu, item);
        true
    }
}

/// いま押されている修飾キー (主修飾キー, ⇧, Alt)。
///
/// `KeyRoutedEventArgs` に修飾キーが乗らないため、その場のキーの状態を読む。
/// `GetKeyState` はスレッドが処理したところまでの状態を返すので、いま
/// 配送されているキー操作と同じ時点の状態になる (`Table` の選択と同じ)。
fn modifiers() -> (bool, bool, bool) {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetKeyState, VIRTUAL_KEY, VK_CONTROL, VK_MENU, VK_SHIFT,
    };

    // 最上位のビットが立っていれば、そのキーは押されている。
    let down = |key: VIRTUAL_KEY| -> bool {
        let state = unsafe { GetKeyState(i32::from(key.0)) };
        state < 0
    };
    (down(VK_CONTROL), down(VK_SHIFT), down(VK_MENU))
}

/// 見出しのボタン。XAML を読めなければ素の `Button` に戻す。
fn title_button(title: &str) -> Result<XamlButton> {
    let button = match XamlReader::Load(&HSTRING::from(TITLE_BUTTON_XAML))
        .and_then(|element| element.cast::<XamlButton>())
    {
        Ok(button) => button,
        Err(_) => XamlButton::new().map_err(|e| to_error("メニュー見出しの生成", e))?,
    };
    button
        .SetContent(&text_block(title)?)
        .map_err(|e| to_error("メニュー見出しの文字設定", e))?;
    Ok(button)
}

/// WinRT のデリゲートは `Send` を要求するので、UI スレッド限定のセルに包む。
fn weak_cell(inner: &Rc<MenuBarInner>) -> Arc<UiThreadCell<Weak<MenuBarInner>>> {
    Arc::new(UiThreadCell::new(Rc::downgrade(inner)))
}
