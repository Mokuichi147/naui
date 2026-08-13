//! WinUI 3 の `Application` 派生オブジェクト。
//!
//! `Application::Start` には基底クラスそのものではなく、
//! `IApplicationOverrides::OnLaunched` と XAML メタデータプロバイダーを
//! 実装した composed object が必要。

use std::ptr::NonNull;

use miui_core::{Error, Result, Theme};
use windows_core::{
    Array, ComObject, ComObjectInner, ComObjectInterface, IInspectable, IInspectable_Vtbl,
    IUnknown, IUnknownImpl, Interface, InterfaceRef, Ref, GUID, HRESULT, HSTRING,
};
use winui3::Microsoft::UI::Xaml::Markup::XamlReader;
use winui3::Microsoft::UI::Xaml::Markup::{
    IXamlMetadataProvider, IXamlMetadataProvider_Impl, IXamlMetadataProvider_Vtbl, IXamlType,
    XmlnsDefinition,
};
use winui3::Microsoft::UI::Xaml::XamlTypeInfo::XamlControlsXamlMetaDataProvider;
use winui3::Microsoft::UI::Xaml::{
    Application, IApplicationFactory, IApplicationOverrides, IApplicationOverrides_Impl,
    IApplicationOverrides_Vtbl, LaunchActivatedEventArgs,
};
use winui3::{ChildClass, Compose, CreateInstanceFn};

use crate::ui_thread::UiThreadCell;
use crate::Ui;

pub struct App<F>
where
    F: FnOnce(&Ui) -> Result<()> + 'static,
{
    provider: XamlControlsXamlMetaDataProvider,
    state: &'static UiThreadCell<Option<F>>,
    failure: &'static UiThreadCell<Option<Error>>,
    theme: Theme,
}

impl<F> IApplicationOverrides_Impl for AppImpl<F>
where
    F: FnOnce(&Ui) -> Result<()> + 'static,
{
    fn OnLaunched(&self, _args: Ref<'_, LaunchActivatedEventArgs>) -> windows_core::Result<()> {
        let current = Application::Current()?;
        // Application.RequestedTheme は触らない。未指定時は Windows の設定に
        // 追従し、実行中の切り替えは各ウィンドウのルート要素で行う。
        let resources = XamlReader::Load(&HSTRING::from(
            r##"<ResourceDictionary xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation">
                <Style TargetType="Button">
                    <Setter Property="BorderThickness" Value="0"/>
                    <Setter Property="CornerRadius" Value="4"/>
                    <Setter Property="Padding" Value="16,8"/>
                    <Setter Property="HorizontalContentAlignment" Value="Center"/>
                </Style>
                <Style TargetType="CheckBox">
                    <Setter Property="Padding" Value="4,6"/>
                </Style>
                <Style TargetType="TextBox">
                    <Setter Property="BorderThickness" Value="1"/>
                    <Setter Property="CornerRadius" Value="4"/>
                    <Setter Property="Padding" Value="12,8"/>
                </Style>
            </ResourceDictionary>"##,
        ))?
        .cast::<winui3::Microsoft::UI::Xaml::ResourceDictionary>()?;
        current
            .Resources()?
            .MergedDictionaries()?
            .Append(&resources)?;
        let Some(build) = self.this.state.with_mut_cross_thread(|build| build.take()) else {
            return Ok(());
        };
        let ui = Ui::new(self.this.theme);
        if let Err(error) = build(&ui) {
            self.this
                .failure
                .with_mut_cross_thread(|slot| *slot = Some(error));
            if let Ok(app) = Application::Current() {
                let _ = app.Exit();
            }
        }
        // ウィンドウとウィジェットをアプリの寿命まで保持する。
        std::mem::forget(ui);
        Ok(())
    }
}

impl<F> IXamlMetadataProvider_Impl for AppImpl<F>
where
    F: FnOnce(&Ui) -> Result<()> + 'static,
{
    fn GetXamlType(
        &self,
        r#type: &winui3::Windows::UI::Xaml::Interop::TypeName,
    ) -> windows_core::Result<IXamlType> {
        self.this.provider.GetXamlType(r#type)
    }

    fn GetXamlTypeByFullName(&self, full_name: &HSTRING) -> windows_core::Result<IXamlType> {
        self.this.provider.GetXamlTypeByFullName(full_name)
    }

    fn GetXmlnsDefinitions(&self) -> windows_core::Result<Array<XmlnsDefinition>> {
        self.this.provider.GetXmlnsDefinitions()
    }
}

#[repr(C)]
pub struct AppImpl<F>
where
    F: FnOnce(&Ui) -> Result<()> + 'static,
{
    identity: &'static IInspectable_Vtbl,
    overrides: &'static IApplicationOverrides_Vtbl,
    metadata: &'static IXamlMetadataProvider_Vtbl,
    this: App<F>,
    count: windows_core::imp::WeakRefCount,
}

impl<F> AppImpl<F>
where
    F: FnOnce(&Ui) -> Result<()> + 'static,
{
    const VTABLE_IDENTITY: IInspectable_Vtbl = IInspectable_Vtbl::new::<Self, Application, 0>();
    const VTABLE_OVERRIDES: IApplicationOverrides_Vtbl =
        IApplicationOverrides_Vtbl::new::<Self, -1>();
    const VTABLE_METADATA: IXamlMetadataProvider_Vtbl =
        IXamlMetadataProvider_Vtbl::new::<Self, -2>();
    fn into_outer(this: App<F>) -> Self {
        Self {
            identity: &Self::VTABLE_IDENTITY,
            overrides: &Self::VTABLE_OVERRIDES,
            metadata: &Self::VTABLE_METADATA,
            this,
            count: windows_core::imp::WeakRefCount::new(),
        }
    }
}

impl<F> IUnknownImpl for AppImpl<F>
where
    F: FnOnce(&Ui) -> Result<()> + 'static,
{
    type Impl = App<F>;

    fn get_impl(&self) -> &Self::Impl {
        &self.this
    }

    fn get_impl_mut(&mut self) -> &mut Self::Impl {
        &mut self.this
    }

    fn into_inner(self) -> Self::Impl {
        self.this
    }

    fn AddRef(&self) -> u32 {
        self.count.add_ref()
    }

    unsafe fn Release(self_: *mut Self) -> u32 {
        let remaining = (*self_).count.release();
        if remaining == 0 {
            drop(Box::from_raw(self_));
        }
        remaining
    }

    fn is_reference_count_one(&self) -> bool {
        self.count.is_one()
    }

    fn to_object(&self) -> ComObject<Self::Impl> {
        self.count.add_ref();
        unsafe { ComObject::from_raw(NonNull::from(self)) }
    }

    unsafe fn GetTrustLevel(&self, value: *mut i32) -> HRESULT {
        if value.is_null() {
            return HRESULT(-2147467261);
        }
        *value = 0;
        HRESULT(0)
    }

    unsafe fn QueryInterface(
        &self,
        iid: *const GUID,
        interface: *mut *mut std::ffi::c_void,
    ) -> HRESULT {
        if iid.is_null() || interface.is_null() {
            return HRESULT(-2147467261);
        }

        let iid = *iid;
        let interface_ptr =
            if iid == <IInspectable as Interface>::IID || iid == <IUnknown as Interface>::IID {
                &self.identity as *const _ as *const std::ffi::c_void
            } else if IApplicationOverrides_Vtbl::matches(&iid) {
                &self.overrides as *const _ as *const std::ffi::c_void
            } else if IXamlMetadataProvider_Vtbl::matches(&iid) {
                &self.metadata as *const _ as *const std::ffi::c_void
            } else if iid == <windows_core::imp::IMarshal as Interface>::IID {
                return windows_core::imp::marshaler(
                    self.to_interface::<IInspectable>().into(),
                    interface,
                );
            } else {
                std::ptr::null()
            };

        *interface = interface_ptr as *mut std::ffi::c_void;
        if interface_ptr.is_null() {
            HRESULT(-2147467262)
        } else {
            self.count.add_ref();
            HRESULT(0)
        }
    }
}

impl<F> ComObjectInterface<IInspectable> for AppImpl<F>
where
    F: FnOnce(&Ui) -> Result<()> + 'static,
{
    fn as_interface_ref(&self) -> InterfaceRef<'_, IInspectable> {
        unsafe { std::mem::transmute(&self.identity) }
    }
}

impl<F> ComObjectInterface<IUnknown> for AppImpl<F>
where
    F: FnOnce(&Ui) -> Result<()> + 'static,
{
    fn as_interface_ref(&self) -> InterfaceRef<'_, IUnknown> {
        unsafe { std::mem::transmute(&self.identity) }
    }
}

impl<F> ComObjectInterface<IApplicationOverrides> for AppImpl<F>
where
    F: FnOnce(&Ui) -> Result<()> + 'static,
{
    fn as_interface_ref(&self) -> InterfaceRef<'_, IApplicationOverrides> {
        unsafe { std::mem::transmute(&self.overrides) }
    }
}

impl<F> ComObjectInterface<IXamlMetadataProvider> for AppImpl<F>
where
    F: FnOnce(&Ui) -> Result<()> + 'static,
{
    fn as_interface_ref(&self) -> InterfaceRef<'_, IXamlMetadataProvider> {
        unsafe { std::mem::transmute(&self.metadata) }
    }
}

impl<F> ComObjectInner for App<F>
where
    F: FnOnce(&Ui) -> Result<()> + 'static,
{
    type Outer = AppImpl<F>;

    fn into_object(self) -> ComObject<Self> {
        let boxed = Box::new(AppImpl::into_outer(self));
        unsafe { ComObject::from_raw(NonNull::new_unchecked(Box::into_raw(boxed))) }
    }
}

impl<F> From<App<F>> for IInspectable
where
    F: FnOnce(&Ui) -> Result<()> + 'static,
{
    fn from(app: App<F>) -> Self {
        ComObject::new(app).into_interface()
    }
}

impl<F> ChildClass for App<F>
where
    F: FnOnce(&Ui) -> Result<()> + 'static,
{
    type BaseType = Application;
    type FactoryInterface = IApplicationFactory;

    fn create_interface_fn(
        vtable: &<Self::FactoryInterface as Interface>::Vtable,
    ) -> CreateInstanceFn {
        vtable.CreateInstance
    }

    fn identity_vtable(vtable: &mut Self::Outer) -> &mut &'static IInspectable_Vtbl {
        &mut vtable.identity
    }

    fn ref_count(vtable: &Self::Outer) -> &windows_core::imp::WeakRefCount {
        &vtable.count
    }

    fn into_outer(self) -> Self::Outer {
        AppImpl::into_outer(self)
    }
}

pub(crate) fn compose<F>(
    state: &'static UiThreadCell<Option<F>>,
    failure: &'static UiThreadCell<Option<Error>>,
    theme: Theme,
) -> windows_core::Result<Application>
where
    F: FnOnce(&Ui) -> Result<()> + 'static,
{
    XamlControlsXamlMetaDataProvider::Initialize()?;
    let provider = XamlControlsXamlMetaDataProvider::new()?;
    Compose::compose(App {
        provider,
        state,
        failure,
        theme,
    })
}
