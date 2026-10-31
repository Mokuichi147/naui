//! COM interfaces that do not appear in WinRT metadata.
//!
//! `IWindowNative`, which gets the HWND out of a XAML `Window`, is declared not in a
//! `.winmd` but in a Windows App SDK C++ header (`microsoft.ui.xaml.window.h`). It is
//! not in the generated projection, so it is written by hand here.
//!
//! The same goes for the interfaces that let Direct2D draw into Composition
//! surfaces (`ICompositorInterop`, `ICompositionGraphicsDeviceInterop` and
//! `ICompositionDrawingSurfaceInterop`, from `Microsoft.UI.Composition.Interop.h`).
//! Their IIDs differ from the system's `Windows.UI.Composition` versions of the same
//! names, so the `windows` crate's declarations cannot be used for them.

// WinRT / COM の名前は元の綴りのまま出す。
#![allow(non_snake_case)]

use windows::Win32::Foundation::{HWND, POINT, RECT, SIZE};

windows_core::imp::define_interface!(
    IWindowNative,
    IWindowNative_Vtbl,
    0xeecdbf0e_bae9_4cb6_a68e_9598e1cb57bb
);
windows_core::imp::interface_hierarchy!(IWindowNative, windows_core::IUnknown);

impl IWindowNative {
    /// This window's HWND.
    ///
    /// # Safety
    ///
    /// The caller guarantees that this `IWindowNative` belongs to a live window.
    pub unsafe fn WindowHandle(&self) -> windows_core::Result<HWND> {
        unsafe {
            let mut handle = core::mem::zeroed();
            (windows_core::Interface::vtable(self).WindowHandle)(
                windows_core::Interface::as_raw(self),
                &mut handle,
            )
            .map(|| handle)
        }
    }
}

#[repr(C)]
#[doc(hidden)]
pub struct IWindowNative_Vtbl {
    pub base__: windows_core::IUnknown_Vtbl,
    pub WindowHandle:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut HWND) -> windows_core::HRESULT,
}

windows_core::imp::define_interface!(
    ICompositorInterop,
    ICompositorInterop_Vtbl,
    0xfab19398_6d19_4d8a_b752_8f096c396069
);
windows_core::imp::interface_hierarchy!(ICompositorInterop, windows_core::IUnknown);

impl ICompositorInterop {
    /// Makes a `CompositionGraphicsDevice` that draws with `rendering_device` (an
    /// `ID2D1Device` or `ID3D11Device`).
    ///
    /// # Safety
    ///
    /// `rendering_device` must be a live Direct2D or Direct3D device.
    pub unsafe fn CreateGraphicsDevice<P0>(
        &self,
        rendering_device: P0,
    ) -> windows_core::Result<crate::Microsoft::UI::Composition::CompositionGraphicsDevice>
    where
        P0: windows_core::Param<windows_core::IUnknown>,
    {
        unsafe {
            let mut result = core::ptr::null_mut();
            (windows_core::Interface::vtable(self).CreateGraphicsDevice)(
                windows_core::Interface::as_raw(self),
                rendering_device.param().abi(),
                &mut result,
            )
            .and_then(|| windows_core::Type::from_abi(result))
        }
    }
}

#[repr(C)]
#[doc(hidden)]
pub struct ICompositorInterop_Vtbl {
    pub base__: windows_core::IUnknown_Vtbl,
    pub CreateGraphicsDevice: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}

windows_core::imp::define_interface!(
    ICompositionGraphicsDeviceInterop,
    ICompositionGraphicsDeviceInterop_Vtbl,
    0x4afa8030_bc70_4b0c_b1c7_6e69f933dc83
);
windows_core::imp::interface_hierarchy!(ICompositionGraphicsDeviceInterop, windows_core::IUnknown);

impl ICompositionGraphicsDeviceInterop {
    /// Replaces the device it draws with (after the previous one was lost). Every
    /// surface made from it loses its contents and must be drawn again.
    ///
    /// # Safety
    ///
    /// `rendering_device` must be a live Direct2D or Direct3D device.
    pub unsafe fn SetRenderingDevice<P0>(&self, rendering_device: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<windows_core::IUnknown>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).SetRenderingDevice)(
                windows_core::Interface::as_raw(self),
                rendering_device.param().abi(),
            )
            .ok()
        }
    }
}

#[repr(C)]
#[doc(hidden)]
pub struct ICompositionGraphicsDeviceInterop_Vtbl {
    pub base__: windows_core::IUnknown_Vtbl,
    pub GetRenderingDevice: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub SetRenderingDevice: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}

windows_core::imp::define_interface!(
    ICompositionDrawingSurfaceInterop,
    ICompositionDrawingSurfaceInterop_Vtbl,
    0x2d6355c2_ad57_4eae_92e4_4c3eff65d578
);
windows_core::imp::interface_hierarchy!(ICompositionDrawingSurfaceInterop, windows_core::IUnknown);

impl ICompositionDrawingSurfaceInterop {
    /// Starts drawing on the whole surface and returns the object to draw with (an
    /// `ID2D1DeviceContext` when `T` is one) and where the surface begins inside it,
    /// in pixels. The surface may share its texture with others, so drawing must stay
    /// inside the surface's size from that point.
    ///
    /// # Safety
    ///
    /// Every successful call must be followed by [`EndDraw`](Self::EndDraw).
    pub unsafe fn BeginDraw<T: windows_core::Interface>(&self) -> windows_core::Result<(T, POINT)> {
        unsafe {
            let mut object = core::ptr::null_mut();
            let mut offset = POINT::default();
            (windows_core::Interface::vtable(self).BeginDraw)(
                windows_core::Interface::as_raw(self),
                core::ptr::null(),
                &T::IID,
                &mut object,
                &mut offset,
            )
            .ok()?;
            Ok((T::from_raw(object), offset))
        }
    }

    /// Ends what [`BeginDraw`](Self::BeginDraw) started and hands it to the
    /// compositor.
    ///
    /// # Safety
    ///
    /// The object `BeginDraw` returned must no longer be drawn with.
    pub unsafe fn EndDraw(&self) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).EndDraw)(windows_core::Interface::as_raw(self))
                .ok()
        }
    }
}

#[repr(C)]
#[doc(hidden)]
pub struct ICompositionDrawingSurfaceInterop_Vtbl {
    pub base__: windows_core::IUnknown_Vtbl,
    pub BeginDraw: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *const RECT,
        *const windows_core::GUID,
        *mut *mut core::ffi::c_void,
        *mut POINT,
    ) -> windows_core::HRESULT,
    pub EndDraw: unsafe extern "system" fn(*mut core::ffi::c_void) -> windows_core::HRESULT,
    pub Resize: unsafe extern "system" fn(*mut core::ffi::c_void, SIZE) -> windows_core::HRESULT,
    pub Scroll: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *const RECT,
        *const RECT,
        i32,
        i32,
    ) -> windows_core::HRESULT,
    pub ResumeDraw: unsafe extern "system" fn(*mut core::ffi::c_void) -> windows_core::HRESULT,
    pub SuspendDraw: unsafe extern "system" fn(*mut core::ffi::c_void) -> windows_core::HRESULT,
}
