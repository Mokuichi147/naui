# naui

[![crates.io](https://img.shields.io/crates/v/naui.svg)](https://crates.io/crates/naui)
[![docs.rs](https://docs.rs/naui/badge.svg)](https://docs.rs/naui)
[![License](https://img.shields.io/crates/l/naui.svg)](#license)
[![CI](https://github.com/mokuichi147/naui/actions/workflows/ci.yml/badge.svg)](https://github.com/mokuichi147/naui/actions/workflows/ci.yml)

A lightweight GUI toolkit that drives each OS's native UI from a single Rust API.

naui does not draw its own widgets. Buttons, text fields, layout, IME,
accessibility, and theme changes are all left to the toolkit each platform
provides.

| Target | Backend | What a button really is |
| --- | --- | --- |
| Windows | WinUI 3 (Windows App SDK) | `Microsoft.UI.Xaml.Controls.Button` |
| macOS | AppKit | `NSButton` |
| Linux | GTK4 / libadwaita | `GtkButton` |
| Web (wasm) | DOM | `<button>` |

The same Gallery code running on each platform. Each one looks the way its OS
normally does.

| Windows 11 Pro 26H2 (Windows App SDK 2.5.1) | macOS Golden Gate 27.0.1 |
| --- | --- |
| ![Gallery running on Windows (WinUI 3)](docs/screenshots/windows.webp) | ![Gallery running on macOS (AppKit)](docs/screenshots/macos.webp) |
| **Ubuntu 26.04.1 LTS** | **Safari 27.0.1** |
| ![Gallery running on Linux (GTK4 / libadwaita)](docs/screenshots/linux.webp) | ![Gallery running on the Web (DOM)](docs/screenshots/web.webp) |

## Features

- Four backends behind one API
- Uses the OS's own rendering, input, accessibility, and theming
- Lightweight: no layout computation or media decoding on the Rust side
  (where the platform needs filling in, it is spelled out in the
  [known limitations](docs/known-limitations.md))
- Reach the native objects directly when the common API is not enough
- Ships with a minimal example and a Gallery that shows off every widget by category

## Quick start

```sh
cargo add naui
```

Put the following in `src/main.rs` and `cargo run` it to get a counter with a
native OS button.

```rust
use std::cell::Cell;
use std::rc::Rc;

use naui::{Orientation, Padding, Settings};

fn main() -> naui::Result<()> {
    naui::run(Settings::new("counter"), |ui| {
        let window = ui.window("counter", 320.0, 200.0)?;
        let stack = ui.stack(Orientation::Vertical)?;
        stack.set_spacing(12.0);
        stack.set_padding(Padding::all(24.0));

        let label = ui.label("0")?;
        let button = ui.button("Increment")?;
        let count = Rc::new(Cell::new(0));

        button.on_click({
            let label = label.clone();
            move || {
                count.set(count.get() + 1);
                label.set_text(&count.get().to_string());
            }
        });

        stack.append(&label);
        stack.append(&button);
        window.set_child(&stack);
        window.show();
        Ok(())
    })
}
```

Examples for every widget are in [`examples/gallery`](examples/gallery/src/lib.rs).

### Requirements

- Rust 1.88 or later
- Linux: development libraries for GTK 4.10+ and libadwaita 1.5+
- Windows: the Windows App SDK 1.3+ runtime (tested with 2.x).
  Not needed if you [bundle it](#bundling-the-windows-app-sdk-runtime)

On Ubuntu 24.04 you can install them with:

```sh
sudo apt install libgtk-4-dev libadwaita-1-dev build-essential pkg-config
```

### Bundling the Windows App SDK runtime

Enable the `self-contained` feature to ship the runtime alongside your exe.
For the license of the downloaded packages and how to distribute them, see
[bundling the runtime](docs/windows-self-contained.md).

### Running the Gallery

```sh
git clone https://github.com/mokuichi147/naui.git
cd naui
cargo run -p gallery
```

To run it on the Web (wasm), install `wasm-bindgen-cli` at the same version as
in `Cargo.lock` (currently 0.2.127), build, and open <http://localhost:8080/>.

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.127
cd examples/gallery/web
./build.sh
python3 -m http.server 8080
```

## Widgets

| Category | API |
| --- | --- |
| Windows & layout | `Window`, `Stack`, `Grid`, `Scroll`, `Spacer`, `Expander`, `SplitView`, `Toolbar`, `MenuBar`, `Sidebar` |
| Basics & input | `Label`, `Button`, `Checkbox`, `Toggle`, `TextInput`, `TextArea`, `PasswordInput`, `SearchInput`, `NumberInput`, `Slider`, `ProgressBar`, `Spinner` |
| Data selection | `ComboBox`, `EditableComboBox`, `RadioGroup`, `DatePicker`, `TimePicker`, `ColorPicker`, `List`, `Table`, `Tree` |
| Files & media | `FilePicker`, `FileSaver`, `Image`, `Video`, `Audio` |
| Drawing | `Canvas` |
| Overlays | `PopupMenu`, `Popover`, `Dialog`, `Toast` |
| Navigation | `Tabs`, `Navbar`, `Dock`, `Menu`, `Breadcrumbs`, `Pagination`, `Link` |

Which control each widget becomes on each platform is listed in
[widget implementations](docs/widgets.md), and platform-specific caveats in the
[known limitations](docs/known-limitations.md). All four platforms are checked
with integration tests and the Gallery ([verification](docs/development.md#verification)).

## Documentation

The guides below are currently written in Japanese.

- [Guide](crates/naui/src/guide.md): adding and removing widgets, themes, layout and sizing, how each widget behaves on each platform, threads and async (shown as [`naui::guide`](https://docs.rs/naui/latest/naui/guide/) on docs.rs)
- [Widget implementations](docs/widgets.md): the native control and construction per platform, key data types, access to native objects
- [Known limitations](docs/known-limitations.md): limitations per platform
- [Bundling the runtime](docs/windows-self-contained.md): shipping the Windows App SDK with your exe
- [Translating apps](docs/translating.md): combining Fluent with naui, following the OS language and its changes ([`examples/i18n`](examples/i18n))
- [Development](docs/development.md): verification, crate layout, tests, CI, releases
- [Roadmap](docs/roadmap.md): missing features and priorities
- [API reference (docs.rs)](https://docs.rs/naui)

## License

[MIT](LICENSE-MIT) OR [Apache-2.0](LICENSE-APACHE)
