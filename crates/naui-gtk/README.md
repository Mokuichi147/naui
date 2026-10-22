# naui-gtk

The Linux backend of [naui](https://crates.io/crates/naui).

It creates real GTK4 / libadwaita controls (`AdwApplicationWindow` / `GtkButton` /
`GtkEntry` / `GtkScale` / `GtkListBox` …) and forwards GTK4 signals to Rust
closures. Drawing, layout, IME, accessibility, and theme changes are all
handled by GTK4.

Apps do not need to depend on it directly; `naui` pulls this crate in when
building for Linux. For cases the common API does not cover, each widget
returns its GTK4 / libadwaita object through `native_*`.

The mapping between `naui` and GTK4 is in the
[API documentation](https://docs.rs/naui-gtk).

## Build requirements

You need the development libraries for GTK4 and libadwaita. On Ubuntu 24.04:

```sh
sudo apt install libgtk-4-dev libadwaita-1-dev build-essential pkg-config
```

On other operating systems the crate is empty and pulls in none of these
dependencies (so that `cargo check --workspace` passes on macOS).

## Tests

GTK4 requires the main thread, so the integration tests do not use the standard
harness (`harness = false`). Running them requires a display.

```sh
cargo test -p naui-gtk
```

## License

MIT OR Apache-2.0
