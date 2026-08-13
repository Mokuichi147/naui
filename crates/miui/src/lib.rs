//! # miui
//!
//! **各 OS のネイティブ UI を、1 つの API から扱う軽量 GUI ツールキット。**
//!
//! miui は自前で描画しない。ボタンは実際に OS のボタンであり、
//! 描画・レイアウト・IME・アクセシビリティ・OS のテーマ追従は
//! すべてプラットフォームのツールキットが行う。
//!
//! | ビルド対象 | 使うツールキット | 例: ボタンの実体 |
//! | --- | --- | --- |
//! | Windows | WinUI 3 (Windows App SDK) | `Microsoft.UI.Xaml.Controls.Button` |
//! | macOS | AppKit | `NSButton` |
//! | Linux | GTK4 / libadwaita | `GtkButton` (未実装) |
//! | Web (wasm) | DOM | `<button>` |
//!
//! ## 使い方
//!
//! UI は `run` に渡すコールバックの中で組み立てる。
//! WinUI 3 が `Application::Start` より前のコントロール生成を許さないため、
//! 4 バックエンドで同じ形にそろえてある。
//!
//! ```no_run
//! use miui::{Orientation, Padding, Settings};
//!
//! fn main() -> miui::Result<()> {
//!     miui::run(Settings::new("counter"), |ui| {
//!         let window = ui.window("counter", 320.0, 180.0)?;
//!         let stack = ui.stack(Orientation::Vertical)?;
//!         stack.set_spacing(12.0);
//!         stack.set_padding(Padding::all(20.0));
//!
//!         let label = ui.label("0")?;
//!         let button = ui.button("増やす")?;
//!
//!         let count = std::cell::Cell::new(0);
//!         button.on_click({
//!             let label = label.clone();
//!             move || {
//!                 count.set(count.get() + 1);
//!                 label.set_text(&count.get().to_string());
//!             }
//!         });
//!
//!         stack.append(&label);
//!         stack.append(&button);
//!         window.set_child(&stack);
//!         window.show();
//!         Ok(())
//!     })
//! }
//! ```
//!
//! ## 検証状況
//!
//! | 環境 | 状態 |
//! | --- | --- |
//! | macOS | 実行・自動テストあり |
//! | Web (wasm) | ブラウザで実行確認 |
//! | Windows | Windows App SDK 2.3.1 の実機で `cargo run -p gallery` を実行確認 |
//! | Linux | 未実装 |

#![forbid(unsafe_code)]

pub use miui_core::{Align, Error, Orientation, Padding, Result, Settings};

#[cfg(target_arch = "wasm32")]
pub use miui_web::{
    run, Button, Checkbox, Label, ProgressBar, Slider, Stack, TextInput, Ui, Widget, Window,
};

#[cfg(all(not(target_arch = "wasm32"), target_os = "macos"))]
pub use miui_macos::{
    run, Button, Checkbox, Label, ProgressBar, Slider, Stack, TextInput, Ui, Widget, Window,
};

#[cfg(all(not(target_arch = "wasm32"), target_os = "windows"))]
pub use miui_windows::{
    run, Button, Checkbox, Label, ProgressBar, Slider, Stack, TextInput, Ui, Widget, Window,
};

#[cfg(all(
    not(target_arch = "wasm32"),
    unix,
    not(any(target_os = "macos", target_os = "ios", target_os = "android"))
))]
pub use miui_gtk::{
    run, Button, Checkbox, Label, ProgressBar, Slider, Stack, TextInput, Ui, Widget, Window,
};

/// バックエンド間で API がずれていないことを、コンパイル時に検査する。
///
/// バックエンドは別々のクレートなので、シグネチャの食い違いは型検査でしか
/// 捕まえられない。この関数はどのターゲットでもコンパイルされ、公開 API を
/// 一通り呼ぶ。**実行はされない。**
#[doc(hidden)]
#[allow(dead_code)]
fn __api_contract(ui: &Ui) -> Result<()> {
    let window: Window = ui.window("t", 100.0, 100.0)?;
    window.set_title("t");
    let _: String = window.title();
    window.set_size(1.0, 1.0);
    window.show();
    window.close();
    let _: bool = window.is_visible();

    let stack: Stack = ui.stack(Orientation::Vertical)?;
    stack.set_spacing(1.0);
    stack.set_padding(Padding::all(1.0));
    stack.set_align(Align::Center);
    let _: usize = stack.len();
    let _: bool = stack.is_empty();

    let label: Label = ui.label("t")?;
    let _: String = label.text();
    label.set_text("t");

    let button: Button = ui.button("t")?;
    button.set_text("t");
    button.set_enabled(true);
    button.on_click(|| {});

    let checkbox: Checkbox = ui.checkbox("t")?;
    let _: bool = checkbox.is_checked();
    checkbox.set_checked(true);
    checkbox.set_enabled(true);
    checkbox.on_toggle(|_v: bool| {});

    let input: TextInput = ui.text_input("t")?;
    let _: String = input.text();
    input.set_text("t");
    input.set_placeholder("t");
    input.set_enabled(true);
    input.on_change(|_s: &str| {});

    let slider: Slider = ui.slider(0.0, 1.0)?;
    let _: f64 = slider.value();
    slider.set_value(0.5);
    slider.set_enabled(true);
    slider.on_change(|_v: f64| {});

    let progress: ProgressBar = ui.progress_bar()?;
    let _: f64 = progress.value();
    progress.set_value(0.5);

    stack.append(&label);
    stack.append(&button);
    stack.append(&checkbox);
    stack.append(&input);
    stack.append(&slider);
    stack.append(&progress);
    window.set_child(&stack);

    ui.quit();
    Ok(())
}
