//! WinRT のメタデータに出てこない COM インターフェース。
//!
//! XAML の `Window` から HWND を取り出す `IWindowNative` は、`.winmd` では
//! なく Windows App SDK の C++ ヘッダー (`microsoft.ui.xaml.window.h`) で
//! 宣言されている。生成した投影には入らないので、ここで手で書く。

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
    /// このウィンドウの HWND。
    ///
    /// # Safety
    ///
    /// 呼び出し側は、この `IWindowNative` が生きているウィンドウのもので
    /// あることを保証する。
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
