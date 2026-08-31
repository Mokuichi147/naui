//! # naui-winui3
//!
//! naui が使う分だけの **WinUI 3 (Windows App SDK) の WinRT 投影**。
//!
//! WinUI 3 のコントロールは Windows SDK ではなく Windows App SDK に入って
//! いて、`windows` クレートには投影が無い。naui は Microsoft が配る
//! `.winmd` から [`windows-bindgen`] で投影を作り、その出力
//! ([`bindings`]) をこのクレートへ収めている。生成のしかたは
//! `tools/winui3-bindgen` を参照。
//!
//! 生成物だけでは足りないものを、このクレートが手で足す。
//!
//! - [`bootstrap`]: 未パッケージのアプリが Windows App SDK ランタイムを
//!   自分のプロセスへ取り付ける仕組み (動的依存)
//! - [`compose`]: `Application` のような **composable なクラスを Rust 側で
//!   継承する**ための土台。`Application::Start` には基底クラスそのもの
//!   ではなく、`IApplicationOverrides` を実装した合成オブジェクトが要る
//! - [`native`]: XAML の `Window` から HWND を取り出す、WinRT のメタデータ
//!   には出てこない COM インターフェース
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

pub use bindings::{Microsoft, Windows};
pub use compose::{ChildClass, Compose, CreateInstanceFn};
pub use native::IWindowNative;

/// COM アパートメントの種類。
pub enum ApartmentType {
    /// スレッドごとに 1 つ。XAML はこちらでなければ動かない。
    SingleThreaded,
    /// プロセスで 1 つ。
    MultiThreaded,
}

/// このスレッドの COM アパートメントを初期化する。
///
/// WinUI 3 のコントロールは UI スレッドの STA でしか作れないので、
/// `Application::Start` の前に [`ApartmentType::SingleThreaded`] で呼ぶ。
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
