# naui-macos

The macOS backend of [naui](https://crates.io/crates/naui).

It creates real AppKit controls (`NSWindow` / `NSButton` / `NSTextField` / `NSSlider` /
`NSStackView` …) and forwards target/action and delegates to Rust closures.
Drawing, layout, IME, accessibility, and theme changes are all handled by
AppKit.

Apps do not need to depend on it directly; `naui` pulls this crate in when
building for macOS. For cases the common API does not cover, each widget
returns its AppKit object through `native_*`.

It uses `unsafe` to call into Objective-C.

## Tests

AppKit requires the main thread, so the integration tests do not use the
standard harness (`harness = false`). Running them requires a macOS GUI session.

```sh
cargo test -p naui-macos
```

## License

MIT OR Apache-2.0
