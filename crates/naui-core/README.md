# naui-core

The crate that holds the shared value types of [naui](https://crates.io/crates/naui).

It does not depend on any backend (AppKit / WinUI 3 / GTK4 / DOM), and it does
no drawing or layout computation. The widgets themselves are implemented by
each backend as the OS's native controls.

What it contains:

- `Error` / `Result`, `Settings`, `Theme`, `Orientation`, `Align`, `Padding`
- Layout specifications (`Sizing`, `Length`, `Track`, `GridCell`, `ScrollPolicy`)
- Values passed to widgets (`Color`, `DateTime`, `Time`, `ListItem`, `TreeItem`,
  `TableColumn`, `TableRow`, `FileFilter`, `ToolbarItem`, `ToastSpec`, and more)
- Values received from notifications (`KeyEvent`, `PointerEvent`, `WindowState`, and more)
- Backend-independent message passing (`Sender`, `Tasks`, `Task`, `MainThread`)
- Weak references to widgets (`WeakWidget`)

It is `#![forbid(unsafe_code)]`.

Apps do not need to depend on it directly; `naui` re-exports these types.

## License

MIT OR Apache-2.0
