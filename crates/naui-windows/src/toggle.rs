//! 入り切りのスイッチ (WinUI 3 のネイティブ `ToggleSwitch`)。
//!
//! `winio-winui3` は WinUI 3 API の subset で、`ToggleSwitch` を投影して
//! いない。そのため、`Expander` と同じように公開 WinRT インターフェイスの
//! 必要な部分だけをこのモジュールで定義する。コントロール自体は
//! `XamlReader` から生成される本物の WinUI 3 `ToggleSwitch` で、つまみの
//! 描画・アニメーション・キーボード操作は WinUI の標準テンプレートが行う。
//!
//! ラベルは `OnContent` と `OffContent` へ同じ文字を入れて、スイッチの
//! となりへ出す (WinUI の既定は「オン」「オフ」の切り替わる文字)。

use std::cell::Cell;
use std::ffi::c_void;
use std::rc::Rc;
use std::sync::Arc;

use naui_core::Result;
use windows_core::{IInspectable, Interface, Param, HSTRING};
use winui3::Microsoft::UI::Xaml::Controls::{Control, TextBlock};
use winui3::Microsoft::UI::Xaml::Markup::XamlReader;
use winui3::Microsoft::UI::Xaml::{RoutedEventHandler, UIElement};

use crate::to_error;
use crate::ui_thread::UiThreadCell;
use crate::widgets::{impl_widget, Widget};

const TOGGLE_SWITCH_XAML: &str = r#"<ToggleSwitch
    xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"/>"#;

/// 切り替わったことの通知先。
///
/// WinRT のデリゲートは `Send + Sync` を要求するので [`UiThreadCell`] に
/// 載せる。呼び出しの間だけクロージャを取り出すため、通知の中から同じ
/// スイッチを操作しても二重借用にならない。
#[derive(Clone)]
struct ToggleHandler(Arc<UiThreadCell<Option<Box<dyn FnMut(bool)>>>>);

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
    native: NativeToggleSwitch,
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
        let native = load_toggle_switch()?;
        native
            .set_is_on(false)
            .map_err(|e| to_error("ToggleSwitch の初期化", e))?;
        // 入り切りのどちらでも同じ文字を出す。切り替わる文字にすると、
        // ほかの環境と読みが変わってしまうため。
        for on in [true, false] {
            let text = TextBlock::new().map_err(|e| to_error("ToggleSwitch ラベルの生成", e))?;
            text.SetText(&HSTRING::from(label))
                .map_err(|e| to_error("ToggleSwitch ラベルの設定", e))?;
            let content: IInspectable = text.cast().map_err(|e| to_error("ラベルの変換", e))?;
            let set = if on {
                native.set_on_content(&content)
            } else {
                native.set_off_content(&content)
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
                        let on = inner.native.is_on().unwrap_or(false);
                        inner.handler.emit(on);
                    }
                }
            });
            Ok(())
        });
        self.0
            .native
            .toggled(&toggled)
            .map_err(|e| to_error("ToggleSwitch の購読", e))?;
        Ok(())
    }

    /// 入っているかどうか。
    pub fn is_on(&self) -> bool {
        self.0.native.is_on().unwrap_or(false)
    }

    /// プログラムから切り替える。`on_toggle` は呼ばれない。
    pub fn set_on(&self, on: bool) {
        self.0.silent.set(true);
        let _ = self.0.native.set_is_on(on);
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

fn load_toggle_switch() -> Result<NativeToggleSwitch> {
    XamlReader::Load(&HSTRING::from(TOGGLE_SWITCH_XAML))
        .and_then(|element| element.cast::<NativeToggleSwitch>())
        .map_err(|e| to_error("ToggleSwitch の生成", e))
}

// -------------------------------------------------------------------------
// Microsoft.UI.Xaml.Controls.ToggleSwitch の最小 WinRT 投影
//
// IID と vtable の並びは cppwinrt の生成ヘッダー
// (winrt/impl/Microsoft.UI.Xaml.Controls.0.h) の guid_v / abi<IToggleSwitch>
// に合わせている。使わないメソッドは `usize` で場所だけ空けてある。

windows_core::imp::define_interface!(
    IToggleSwitch,
    IToggleSwitch_Vtbl,
    0x1b17eeb1_74bf_5a83_8161_a86f0fdcdf24
);
impl windows_core::RuntimeType for IToggleSwitch {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}

#[repr(C)]
pub struct IToggleSwitch_Vtbl {
    base__: windows_core::IInspectable_Vtbl,
    is_on: unsafe extern "system" fn(*mut c_void, *mut bool) -> windows_core::HRESULT,
    set_is_on: unsafe extern "system" fn(*mut c_void, bool) -> windows_core::HRESULT,
    header: usize,
    set_header: usize,
    header_template: usize,
    set_header_template: usize,
    on_content: usize,
    set_on_content: unsafe extern "system" fn(*mut c_void, *mut c_void) -> windows_core::HRESULT,
    on_content_template: usize,
    set_on_content_template: usize,
    off_content: usize,
    set_off_content: unsafe extern "system" fn(*mut c_void, *mut c_void) -> windows_core::HRESULT,
    off_content_template: usize,
    set_off_content_template: usize,
    template_settings: usize,
    toggled: unsafe extern "system" fn(*mut c_void, *mut c_void, *mut i64) -> windows_core::HRESULT,
    remove_toggled: unsafe extern "system" fn(*mut c_void, i64) -> windows_core::HRESULT,
}

#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
struct NativeToggleSwitch(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    NativeToggleSwitch,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl windows_core::RuntimeType for NativeToggleSwitch {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IToggleSwitch>();
}
unsafe impl Interface for NativeToggleSwitch {
    type Vtable = IToggleSwitch_Vtbl;
    const IID: windows_core::GUID = IToggleSwitch::IID;
}
impl windows_core::RuntimeName for NativeToggleSwitch {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.ToggleSwitch";
}

impl NativeToggleSwitch {
    fn is_on(&self) -> windows_core::Result<bool> {
        unsafe {
            let mut result = false;
            (Interface::vtable(self).is_on)(Interface::as_raw(self), &mut result).map(|| result)
        }
    }

    fn set_is_on(&self, value: bool) -> windows_core::Result<()> {
        unsafe { (Interface::vtable(self).set_is_on)(Interface::as_raw(self), value).ok() }
    }

    fn set_on_content<P>(&self, value: P) -> windows_core::Result<()>
    where
        P: Param<IInspectable>,
    {
        unsafe {
            (Interface::vtable(self).set_on_content)(Interface::as_raw(self), value.param().abi())
                .ok()
        }
    }

    fn set_off_content<P>(&self, value: P) -> windows_core::Result<()>
    where
        P: Param<IInspectable>,
    {
        unsafe {
            (Interface::vtable(self).set_off_content)(Interface::as_raw(self), value.param().abi())
                .ok()
        }
    }

    fn toggled<P>(&self, handler: P) -> windows_core::Result<i64>
    where
        P: Param<RoutedEventHandler>,
    {
        unsafe {
            let mut token = 0;
            (Interface::vtable(self).toggled)(
                Interface::as_raw(self),
                handler.param().abi(),
                &mut token,
            )
            .map(|| token)
        }
    }
}
