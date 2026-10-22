# naui-winui3

A **WinRT projection of WinUI 3 (Windows App SDK)**, covering only what naui uses.

WinUI 3 controls live in the Windows App SDK rather than the Windows SDK, and
the `windows` crate has no projection for them. This crate holds a projection
(`src/bindings.rs`) generated with [`windows-bindgen`] from the `.winmd` files
Microsoft distributes, plus the following pieces the generated code alone does
not provide:

- `bootstrap`: lets an unpackaged app attach the Windows App SDK runtime to its
  own process (dynamic dependencies)
- `self_contained`: uses a Windows App SDK runtime bundled with the exe
  (the `self-contained` feature). `build.rs` fetches and places it at build time
- `compose`: groundwork for subclassing composable classes such as
  `Application` from Rust
- `native`: `IWindowNative`, which gets the HWND out of a XAML `Window`

How to regenerate `src/bindings.rs` is described in `tools/winui3-bindgen/README.md`.

**This is groundwork for naui and is not meant to be used on its own.** If you
want to use WinUI 3 from Rust in general, picking the types you need yourself
and running them through `windows-bindgen` will give you something smaller.

[`windows-bindgen`]: https://crates.io/crates/windows-bindgen
