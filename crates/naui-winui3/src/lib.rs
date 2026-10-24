//! # naui-winui3
//!
//! **A WinRT projection of WinUI 3 (Windows App SDK)**, limited to what naui uses.
//!
//! WinUI 3 controls are part of the Windows App SDK, not the Windows SDK, and the
//! `windows` crate has no projection for them. naui builds a projection from the
//! `.winmd` files Microsoft distributes using [`windows-bindgen`], and exposes the
//! output as [`Microsoft`] and [`Windows`]. See `tools/winui3-bindgen` for how it is
//! generated.
//!
//! This crate adds by hand what the generated output alone lacks:
//!
//! - [`bootstrap`]: the mechanism by which an unpackaged app attaches the Windows
//!   App SDK runtime to its own process (dynamic dependency)
//! - `self_contained`: the mechanism for using a Windows App SDK runtime bundled with
//!   the exe (the `self-contained` feature)
//! - [`Compose`]: the foundation for **inheriting composable classes such as
//!   `Application` on the Rust side**. `Application::Start` needs not the base class
//!   itself but a composed object implementing `IApplicationOverrides`
//! - [`IWindowNative`]: a COM interface that does not appear in WinRT metadata, for
//!   getting the HWND out of a XAML `Window`
//!
//! [`windows-bindgen`]: https://crates.io/crates/windows-bindgen

#![cfg(target_os = "windows")]

// 生成物。`allow` と体裁は `windows-bindgen` の出力そのままにして、
// `tools/winui3-bindgen` を流し直せば同じ内容になるようにしておく。
#[rustfmt::skip]
mod bindings;

pub mod bootstrap;
mod compose;
mod native;
#[cfg(feature = "self-contained")]
pub mod self_contained;

pub use bindings::{Microsoft, Windows};
pub use compose::{ChildClass, Compose, CreateInstanceFn};
pub use native::IWindowNative;

/// The kind of COM apartment.
pub enum ApartmentType {
    /// One per thread. XAML works only with this.
    SingleThreaded,
    /// One per process.
    MultiThreaded,
}

/// Initializes this thread's COM apartment.
///
/// WinUI 3 controls can only be created in the UI thread's STA, so call this with
/// [`ApartmentType::SingleThreaded`] before `Application::Start`.
#[inline]
pub fn init_apartment(apartment_type: ApartmentType) -> windows_core::Result<()> {
    use windows::Win32::System::WinRT::{
        RoInitialize, RO_INIT_MULTITHREADED, RO_INIT_SINGLETHREADED,
    };
    let kind = match apartment_type {
        ApartmentType::SingleThreaded => RO_INIT_SINGLETHREADED,
        ApartmentType::MultiThreaded => RO_INIT_MULTITHREADED,
    };
    unsafe { RoInitialize(kind) }
}
