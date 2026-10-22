# naui-windows

The Windows backend of [naui](https://crates.io/crates/naui).

It creates real WinUI 3 (Fluent 2) controls (`Microsoft.UI.Xaml.Controls.Button`
and so on). Drawing, layout, IME, accessibility, and theme changes are all
handled by WinUI 3.

Apps do not need to depend on it directly; `naui` pulls this crate in when
building for Windows. On other platforms the crate is empty and pulls in no
dependencies. For cases the common API does not cover, each widget returns its
WinUI 3 object through `native_*`.

## Runtime requirements

WinUI 3 ships in the **Windows App SDK**, not the Windows SDK, so the machine
running the app needs the Windows App SDK 1.3+ framework runtime. At startup
the backend adds a dependency on the framework package. Rather than pinning a
version, it tries versions from 2.x down to 1.3, newest first, and uses the
first one it can attach. The OS-bundled line (`Microsoft.WindowsAppRuntime.CBS*`)
is also a candidate. It is tested with 2.x.

Enable the `self-contained` feature to ship the runtime alongside your exe.
At build time the pieces are fetched from nuget.org and placed next to the exe's
output, and at run time only those are used (it is an error if they are not next
to the exe). See
[bundling the runtime](https://github.com/mokuichi147/naui/blob/main/docs/windows-self-contained.md)
(in Japanese) for details.

Also, controls can only be created after `Application::Start`. This constraint
is why naui's public API has you build the UI inside a callback.

## License

MIT OR Apache-2.0
