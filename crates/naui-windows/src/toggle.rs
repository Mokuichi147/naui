//! 入り切りのスイッチ (WinUI 3 のネイティブ `ToggleSwitch`)。
//!
//! コントロールは [`naui_winui3`] の投影から直に作る。つまみの描画・
//! アニメーション・キーボード操作は WinUI の標準テンプレートが行う。
//!
//! ラベルは `OnContent` と `OffContent` へ同じ文字を入れて、スイッチの
//! となりへ出す (WinUI の既定は「オン」「オフ」の切り替わる文字)。

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use naui_core::Result;
use naui_winui3::Microsoft::UI::Xaml::Controls::{Control, TextBlock, ToggleSwitch};
use naui_winui3::Microsoft::UI::Xaml::{RoutedEventHandler, UIElement};
use windows_core::{IInspectable, Interface, HSTRING};

use crate::to_error;
use crate::ui_thread::{HandlerCell, UiThreadCell};
use crate::widgets::{impl_widget, Widget};

/// 切り替わったことの通知先。
///
/// WinRT のデリゲートは `Send + Sync` を要求するので [`UiThreadCell`] に
/// 載せる。呼び出しの間だけクロージャを取り出すため、通知の中から同じ
/// スイッチを操作しても二重借用にならない。
#[derive(Clone)]
struct ToggleHandler(HandlerCell<dyn FnMut(bool)>);

impl ToggleHandler {
    fn new() -> Self {
        Self(Arc::new(UiThreadCell::new(None)))
    }

    fn set(&self, f: impl FnMut(bool) + 'static) {
        self.0.with_mut(|slot| *slot = Some(Box::new(f)));
    }

    fn emit(&self, on: bool) {
        let Some(Some(mut f)) = self.0.try_with_mut(|slot| slot.take()) else {
            return;
        };
        f(on);
        let _ = self.0.try_with_mut(|slot| {
            if slot.is_none() {
                *slot = Some(f);
            }
        });
    }
}

struct ToggleInner {
    native: ToggleSwitch,
    handler: ToggleHandler,
    /// `set_on` による変更では `on_toggle` を呼ばない。
    silent: Cell<bool>,
}

/// 入り切りを切り替えるスイッチ (`ToggleSwitch`)。
#[derive(Clone)]
pub struct Toggle(Rc<ToggleInner>);
impl_widget!(Toggle, native);

impl Toggle {
    pub(crate) fn new(label: &str) -> Result<Self> {
        let native = ToggleSwitch::new().map_err(|e| to_error("ToggleSwitch の生成", e))?;
        native
            .SetIsOn(false)
            .map_err(|e| to_error("ToggleSwitch の初期化", e))?;
        // 入り切りのどちらでも同じ文字を出す。切り替わる文字にすると、
        // ほかの環境と読みが変わってしまうため。
        for on in [true, false] {
            let text = TextBlock::new().map_err(|e| to_error("ToggleSwitch ラベルの生成", e))?;
            text.SetText(&HSTRING::from(label))
                .map_err(|e| to_error("ToggleSwitch ラベルの設定", e))?;
            let content: IInspectable = text.cast().map_err(|e| to_error("ラベルの変換", e))?;
            let set = if on {
                native.SetOnContent(&content)
            } else {
                native.SetOffContent(&content)
            };
            set.map_err(|e| to_error("ToggleSwitch のラベル設定", e))?;
        }

        let this = Self(Rc::new(ToggleInner {
            native,
            handler: ToggleHandler::new(),
            silent: Cell::new(false),
        }));
        this.connect()?;
        Ok(this)
    }

    /// WinUI の `Toggled` を Rust のクロージャへつなぐ。
    ///
    /// `Toggled` は `IsOn` を書き換えたときにも飛ぶので、`set_on` の間は
    /// 黙らせる (macOS / GTK / Web と同じく、通知は利用者の操作のときだけ)。
    fn connect(&self) -> Result<()> {
        let target = Arc::new(UiThreadCell::new(Rc::downgrade(&self.0)));
        let toggled = RoutedEventHandler::new(move |_sender, _args| {
            let _ = target.try_with_mut(|weak| {
                if let Some(inner) = weak.upgrade() {
                    if !inner.silent.get() {
                        let on = inner.native.IsOn().unwrap_or(false);
                        inner.handler.emit(on);
                    }
                }
            });
            Ok(())
        });
        self.0
            .native
            .Toggled(&toggled)
            .map_err(|e| to_error("ToggleSwitch の購読", e))?;
        Ok(())
    }

    /// 入っているかどうか。
    pub fn is_on(&self) -> bool {
        self.0.native.IsOn().unwrap_or(false)
    }

    /// プログラムから切り替える。`on_toggle` は呼ばれない。
    pub fn set_on(&self, on: bool) {
        self.0.silent.set(true);
        let _ = self.0.native.SetIsOn(on);
        self.0.silent.set(false);
    }

    pub fn set_enabled(&self, enabled: bool) {
        if let Ok(control) = self.0.native.cast::<Control>() {
            let _ = control.SetIsEnabled(enabled);
        }
    }

    /// 利用者が切り替えるたびに、切り替えた後の状態で呼ばれる。
    pub fn on_toggle(&self, f: impl FnMut(bool) + 'static) {
        self.0.handler.set(f);
    }
}
