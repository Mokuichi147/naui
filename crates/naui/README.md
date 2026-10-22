# naui

A lightweight GUI toolkit that drives each OS's native UI from a single Rust API.

This is the only crate your app depends on; the backend is chosen automatically
based on the build target.

| Build target | Backend | Example: what a button really is |
| --- | --- | --- |
| Windows | `naui-windows` (WinUI 3) | `Microsoft.UI.Xaml.Controls.Button` |
| macOS | `naui-macos` (AppKit) | `NSButton` |
| Linux | `naui-gtk` (GTK4 / libadwaita) | `GtkButton` |
| Web (wasm32) | `naui-web` (DOM) | `<button>` |

## Usage

```sh
cargo add naui
```

You build the UI inside the callback passed to `run`. WinUI 3 does not allow
creating controls before `Application::Start`, so all four backends share this
same shape.

```rust
use naui::{Orientation, Padding, Settings};

fn main() -> naui::Result<()> {
    naui::run(Settings::new("counter"), |ui| {
        let window = ui.window("counter", 320.0, 180.0)?;
        let stack = ui.stack(Orientation::Vertical)?;
        stack.set_spacing(12.0);
        stack.set_padding(Padding::all(20.0));

        let label = ui.label("0")?;
        let button = ui.button("Increment")?;

        let count = std::cell::Cell::new(0);
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

When targeting both native and the Web, leave the entry point to `entry!`. The
macro generates the Web's `#[wasm_bindgen(start)]`, so the app needs neither
`cfg` nor a dependency on wasm-bindgen.

On Windows, the machine running the app needs the Windows App SDK runtime.
Enable the `self-contained` feature to ship the runtime alongside your exe.

```toml
[dependencies]
naui = { version = "0.10", features = ["self-contained"] }
```

The [guide](https://docs.rs/naui/latest/naui/guide/) (in Japanese) continues
from here: adding and removing widgets, layout and sizing, how each widget
behaves on each platform, and threads and async. For the list of widgets and
per-platform limitations, see the
[repository README](https://github.com/mokuichi147/naui#readme).

## License

MIT OR Apache-2.0
