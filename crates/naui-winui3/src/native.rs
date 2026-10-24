//! COM interfaces that do not appear in WinRT metadata.
//!
//! `IWindowNative`, which gets the HWND out of a XAML `Window`, is declared not in a
//! `.winmd` but in a Windows App SDK C++ header (`microsoft.ui.xaml.window.h`). It is
//! not in the generated projection, so it is written by hand here.

// WinRT / COM の名前は元の綴りのまま出す。
#![allow(non_snake_case)]

use windows::Win32::Foundation::HWND;

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
