# naui

各 OS のネイティブ UI を、1 つの Rust API から扱う軽量 GUI ツールキットです。

アプリが依存するのはこのクレートだけで、ビルド対象に応じたバックエンドが
自動で選ばれます。

| ビルド対象 | バックエンド | 例: ボタンの実体 |
| --- | --- | --- |
| Windows | `naui-windows` (WinUI 3) | `Microsoft.UI.Xaml.Controls.Button` |
| macOS | `naui-macos` (AppKit) | `NSButton` |
| Linux | `naui-gtk` (GTK4 / libadwaita) | `GtkButton` |
| Web (wasm32) | `naui-web` (DOM) | `<button>` |

## 使い方

```sh
cargo add naui
```

UI は `run` に渡すコールバックの中で組み立てます。WinUI 3 が
`Application::Start` より前のコントロール生成を許さないため、4 バックエンドで
同じ形にそろえてあります。

```rust
use naui::{Orientation, Padding, Settings};

fn main() -> naui::Result<()> {
    naui::run(Settings::new("counter"), |ui| {
        let window = ui.window("counter", 320.0, 180.0)?;
        let stack = ui.stack(Orientation::Vertical)?;
        stack.set_spacing(12.0);
        stack.set_padding(Padding::all(20.0));

        let label = ui.label("0")?;
        let button = ui.button("増やす")?;

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

ネイティブと Web の両方へ出すときは、入口を `entry!` に任せます。Web の
`#[wasm_bindgen(start)]` はこのマクロが作るので、アプリ側に `cfg` も
wasm-bindgen への依存も要りません。

ウィジェット一覧、レイアウトの決まり、環境ごとの制限は
[リポジトリの README](https://github.com/mokuichi147/naui#readme) を参照してください。

## ライセンス

MIT OR Apache-2.0
