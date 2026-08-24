//! 折りたたみ (WinUI 3 のネイティブ `Expander`)。
//!
//! `winio-winui3` は WinUI 3 API の subset で、`Expander` を投影していない。
//! そのため、`DatePicker` と同じく、公開 WinRT インターフェイスの必要な部分
//! だけをこのモジュールで定義する。コントロール自体は `XamlReader` から生成
//! される本物の WinUI 3 `Expander` で、開閉・見出し・中身のレイアウトは
//! WinUI の標準テンプレートが行う。

use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::rc::Rc;
use std::sync::Arc;

use naui_core::Result;
use windows::Foundation::TypedEventHandler;
use windows_core::{Interface, Param, HSTRING};
use winui3::Microsoft::UI::Xaml::Controls::{ContentControl, Control, TextBlock};
use winui3::Microsoft::UI::Xaml::Markup::XamlReader;
use winui3::Microsoft::UI::Xaml::UIElement;

use crate::to_error;
use crate::ui_thread::UiThreadCell;
use crate::widgets::{impl_widget, Widget};

const EXPANDER_XAML: &str = r#"<Expander
    xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
    HorizontalAlignment="Stretch" HorizontalContentAlignment="Stretch"/>"#;

/// 開閉が変わったことの通知先。
///
/// WinRT のデリゲートは `Send + Sync` を要求するので [`UiThreadCell`] に
/// 載せる。呼び出しの間だけクロージャを取り出すため、通知の中から同じ
/// 折りたたみを操作しても二重借用にならない。
#[derive(Clone)]
struct ToggleHandler(Arc<UiThreadCell<Option<Box<dyn FnMut(bool)>>>>);

impl ToggleHandler {
    fn new() -> Self {
        Self(Arc::new(UiThreadCell::new(None)))
    }

    fn set(&self, f: impl FnMut(bool) + 'static) {
        self.0.with_mut(|slot| *slot = Some(Box::new(f)));
    }

    fn emit(&self, expanded: bool) {
        let Some(Some(mut f)) = self.0.try_with_mut(|slot| slot.take()) else {
            return;
        };
        f(expanded);
        let _ = self.0.try_with_mut(|slot| {
            if slot.is_none() {
                *slot = Some(f);
            }
        });
    }
}

struct ExpanderInner {
    native: NativeExpander,
    label: TextBlock,
    /// 中身のハンドルを保持し、コールバックごと生かしておく。
    child: RefCell<Option<Box<dyn Widget>>>,
    handler: ToggleHandler,
    /// `set_expanded` による変更では `on_toggle` を呼ばない。
    silent: Cell<bool>,
}

/// 見出しを押して中身を出し入れするコンテナ。
#[derive(Clone)]
pub struct Expander(Rc<ExpanderInner>);
impl_widget!(Expander, native);

impl Expander {
    pub(crate) fn new(text: &str) -> Result<Self> {
        let native = load_expander()?;
        let label = TextBlock::new().map_err(|e| to_error("見出しの生成", e))?;
        label
            .SetText(&HSTRING::from(text))
            .map_err(|e| to_error("見出しの設定", e))?;
        native
            .set_header(&label)
            .map_err(|e| to_error("Expander の見出し設定", e))?;

        let this = Self(Rc::new(ExpanderInner {
            native,
            label,
            child: RefCell::new(None),
            handler: ToggleHandler::new(),
            silent: Cell::new(false),
        }));
        this.connect()?;
        Ok(this)
    }

    /// WinUI の `Expanding` / `Collapsed` を Rust のクロージャへつなぐ。
    fn connect(&self) -> Result<()> {
        let expanding_target = Arc::new(UiThreadCell::new(Rc::downgrade(&self.0)));
        let expanding = TypedEventHandler::<NativeExpander, NativeExpanderExpandingEventArgs>::new(
            move |_, _| {
                let _ = expanding_target.try_with_mut(|weak| {
                    if let Some(inner) = weak.upgrade() {
                        if !inner.silent.get() {
                            inner.handler.emit(true);
                        }
                    }
                });
                Ok(())
            },
        );
        self.0
            .native
            .expanding(&expanding)
            .map_err(|e| to_error("Expander の展開購読", e))?;

        let collapsed_target = Arc::new(UiThreadCell::new(Rc::downgrade(&self.0)));
        let collapsed = TypedEventHandler::<NativeExpander, NativeExpanderCollapsedEventArgs>::new(
            move |_, _| {
                let _ = collapsed_target.try_with_mut(|weak| {
                    if let Some(inner) = weak.upgrade() {
                        if !inner.silent.get() {
                            inner.handler.emit(false);
                        }
                    }
                });
                Ok(())
            },
        );
        self.0
            .native
            .collapsed(&collapsed)
            .map_err(|e| to_error("Expander の折りたたみ購読", e))?;
        Ok(())
    }

    /// 見出しの文字。
    pub fn text(&self) -> String {
        self.0
            .label
            .Text()
            .map(|text| text.to_string())
            .unwrap_or_default()
    }

    pub fn set_text(&self, text: &str) {
        let _ = self.0.label.SetText(&HSTRING::from(text));
    }

    /// 開いているかどうか。
    pub fn is_expanded(&self) -> bool {
        self.0.native.is_expanded().unwrap_or(false)
    }

    /// プログラムから開閉する。`on_toggle` は呼ばれない。
    pub fn set_expanded(&self, expanded: bool) {
        self.0.silent.set(true);
        let _ = self.0.native.set_is_expanded(expanded);
        self.0.silent.set(false);
    }

    pub fn set_enabled(&self, enabled: bool) {
        if let Ok(control) = self.0.native.cast::<Control>() {
            let _ = control.SetIsEnabled(enabled);
        }
    }

    /// 折りたたむ中身。呼ぶたびに置き換わる。
    pub fn set_child(&self, child: &dyn Widget) {
        let Ok(content) = self.0.native.cast::<ContentControl>() else {
            return;
        };
        if content.SetContent(None).is_err() {
            return;
        }
        let element = child.native_element();
        if content.SetContent(&element).is_ok() {
            *self.0.child.borrow_mut() = Some(child.boxed_clone());
        }
    }

    /// 利用者が開閉するたびに、変わった後の状態で呼ばれる。
    pub fn on_toggle(&self, f: impl FnMut(bool) + 'static) {
        self.0.handler.set(f);
    }
}

fn load_expander() -> Result<NativeExpander> {
    XamlReader::Load(&HSTRING::from(EXPANDER_XAML))
        .and_then(|element| element.cast::<NativeExpander>())
        .map_err(|e| to_error("Expander の生成", e))
}

// -------------------------------------------------------------------------
// Microsoft.UI.Xaml.Controls.Expander の最小 WinRT 投影

windows_core::imp::define_interface!(
    IExpander,
    IExpander_Vtbl,
    0xca633942_e584_55c2_b7ee_cffc73c8127a
);
impl windows_core::RuntimeType for IExpander {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}

#[repr(C)]
pub struct IExpander_Vtbl {
    base__: windows_core::IInspectable_Vtbl,
    header: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> windows_core::HRESULT,
    set_header: unsafe extern "system" fn(*mut c_void, *mut c_void) -> windows_core::HRESULT,
    header_template: usize,
    set_header_template: usize,
    header_template_selector: usize,
    set_header_template_selector: usize,
    is_expanded: unsafe extern "system" fn(*mut c_void, *mut bool) -> windows_core::HRESULT,
    set_is_expanded: unsafe extern "system" fn(*mut c_void, bool) -> windows_core::HRESULT,
    expand_direction: usize,
    set_expand_direction: usize,
    expanding:
        unsafe extern "system" fn(*mut c_void, *mut c_void, *mut i64) -> windows_core::HRESULT,
    remove_expanding: unsafe extern "system" fn(*mut c_void, i64) -> windows_core::HRESULT,
    collapsed:
        unsafe extern "system" fn(*mut c_void, *mut c_void, *mut i64) -> windows_core::HRESULT,
    remove_collapsed: unsafe extern "system" fn(*mut c_void, i64) -> windows_core::HRESULT,
    template_settings: usize,
}

#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
struct NativeExpander(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    NativeExpander,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl windows_core::RuntimeType for NativeExpander {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IExpander>();
}
unsafe impl Interface for NativeExpander {
    type Vtable = IExpander_Vtbl;
    const IID: windows_core::GUID = IExpander::IID;
}
impl windows_core::RuntimeName for NativeExpander {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.Expander";
}

impl NativeExpander {
    fn set_header<P>(&self, value: P) -> windows_core::Result<()>
    where
        P: Param<windows_core::IInspectable>,
    {
        unsafe {
            (Interface::vtable(self).set_header)(Interface::as_raw(self), value.param().abi()).ok()
        }
    }

    fn is_expanded(&self) -> windows_core::Result<bool> {
        unsafe {
            let mut result = false;
            (Interface::vtable(self).is_expanded)(Interface::as_raw(self), &mut result)
                .map(|| result)
        }
    }

    fn set_is_expanded(&self, value: bool) -> windows_core::Result<()> {
        unsafe { (Interface::vtable(self).set_is_expanded)(Interface::as_raw(self), value).ok() }
    }

    fn expanding<P>(&self, handler: P) -> windows_core::Result<i64>
    where
        P: Param<TypedEventHandler<NativeExpander, NativeExpanderExpandingEventArgs>>,
    {
        unsafe {
            let mut token = 0;
            (Interface::vtable(self).expanding)(
                Interface::as_raw(self),
                handler.param().abi(),
                &mut token,
            )
            .map(|| token)
        }
    }

    fn collapsed<P>(&self, handler: P) -> windows_core::Result<i64>
    where
        P: Param<TypedEventHandler<NativeExpander, NativeExpanderCollapsedEventArgs>>,
    {
        unsafe {
            let mut token = 0;
            (Interface::vtable(self).collapsed)(
                Interface::as_raw(self),
                handler.param().abi(),
                &mut token,
            )
            .map(|| token)
        }
    }
}

windows_core::imp::define_interface!(
    IExpanderExpandingEventArgs,
    IExpanderExpandingEventArgs_Vtbl,
    0x433f2e36_19e7_579c_b4ce_9ce5d510d001
);
impl windows_core::RuntimeType for IExpanderExpandingEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}

#[repr(C)]
pub struct IExpanderExpandingEventArgs_Vtbl {
    base__: windows_core::IInspectable_Vtbl,
}

#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
struct NativeExpanderExpandingEventArgs(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    NativeExpanderExpandingEventArgs,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl windows_core::RuntimeType for NativeExpanderExpandingEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IExpanderExpandingEventArgs>();
}
unsafe impl Interface for NativeExpanderExpandingEventArgs {
    type Vtable = IExpanderExpandingEventArgs_Vtbl;
    const IID: windows_core::GUID = IExpanderExpandingEventArgs::IID;
}
impl windows_core::RuntimeName for NativeExpanderExpandingEventArgs {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.ExpanderExpandingEventArgs";
}

windows_core::imp::define_interface!(
    IExpanderCollapsedEventArgs,
    IExpanderCollapsedEventArgs_Vtbl,
    0x968a6870_7426_535e_a526_279e6eedecd0
);
impl windows_core::RuntimeType for IExpanderCollapsedEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}

#[repr(C)]
pub struct IExpanderCollapsedEventArgs_Vtbl {
    base__: windows_core::IInspectable_Vtbl,
}

#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
struct NativeExpanderCollapsedEventArgs(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    NativeExpanderCollapsedEventArgs,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl windows_core::RuntimeType for NativeExpanderCollapsedEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IExpanderCollapsedEventArgs>();
}
unsafe impl Interface for NativeExpanderCollapsedEventArgs {
    type Vtable = IExpanderCollapsedEventArgs_Vtbl;
    const IID: windows_core::GUID = IExpanderCollapsedEventArgs::IID;
}
impl windows_core::RuntimeName for NativeExpanderCollapsedEventArgs {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.ExpanderCollapsedEventArgs";
}
