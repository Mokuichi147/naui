# naui-web

The Web (wasm) backend of [naui](https://crates.io/crates/naui).

It creates standard DOM controls (`<button>` / `<input>` / `<progress>` …) as
they are. In a browser, the "native UI" is the form controls themselves, so it
does not style them and leaves their look to the browser defaults. Only layout
uses Flexbox.

Apps do not need to depend on it directly; `naui` pulls this crate in when
building for `wasm32`. On other targets the crate is empty and pulls in none of
its dependencies, such as wasm-bindgen. For cases the common API does not cover,
each widget returns its `web_sys` element through `native_*`.

## Building

You need the wasm target and `wasm-bindgen-cli` at the same version as the one
recorded in `Cargo.lock`.

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.127
```

If you leave the entry point to `naui::entry!`, the macro generates
`#[wasm_bindgen(start)]`, so the app itself does not need to depend on
wasm-bindgen.

## Limitations

wasm has no threads, so `std::thread::spawn` is not available. Instead of
sending values from another thread, call `Sender::send` inside a future passed
to `Tasks::spawn`. Other limitations are collected in the
[known limitations](https://github.com/mokuichi147/naui/blob/main/docs/known-limitations.md#web)
(in Japanese).

## License

MIT OR Apache-2.0
