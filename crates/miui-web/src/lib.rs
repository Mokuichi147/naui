//! # miui-web
//!
//! miui の Web バックエンド。**DOM の標準コントロール**
//! (`<button>` / `<input>` / `<progress>` …) をそのまま生成する。
//! ブラウザにおける「ネイティブ UI」はフォームコントロールそのものなので、
//! 見た目を作り込むことはせず、ブラウザ既定のスタイルに任せている。
//!
//! レイアウトだけは Flexbox を使う (AppKit の NSStackView、
//! WinUI 3 の StackPanel、GTK4 の GtkBox に対応する)。

#![cfg(target_arch = "wasm32")]
#![forbid(unsafe_code)]

mod widgets;
mod window;

use miui_core::{Error, Orientation, Result, Settings};
use std::cell::RefCell;
use web_sys::Document;

pub use widgets::{Button, Checkbox, Label, ProgressBar, Slider, Stack, TextInput, Widget};
pub use window::Window;

pub(crate) fn document() -> Result<Document> {
    web_sys::window()
        .and_then(|w| w.document())
        .ok_or_else(|| Error::new("document の取得", "ブラウザ環境ではありません"))
}

pub(crate) fn to_error(context: &'static str, value: wasm_bindgen::JsValue) -> Error {
    Error::new(
        context,
        value
            .as_string()
            .unwrap_or_else(|| format!("{value:?}")),
    )
}

/// ウィジェットを生成するための入り口。
pub struct Ui {
    document: Document,
    windows: RefCell<Vec<Window>>,
}

impl Ui {
    fn new(document: Document) -> Self {
        Self {
            document,
            windows: RefCell::new(Vec::new()),
        }
    }

    /// ウィンドウを作る。ブラウザには OS のウィンドウが無いため、
    /// `<body>` 直下のブロック要素として表現し、タイトルは
    /// `document.title` に反映する。
    pub fn window(&self, title: &str, width: f64, height: f64) -> Result<Window> {
        let w = Window::new(&self.document, title, width, height)?;
        self.windows.borrow_mut().push(w.clone());
        Ok(w)
    }

    pub fn stack(&self, orientation: Orientation) -> Result<Stack> {
        Stack::new(&self.document, orientation)
    }

    pub fn label(&self, text: &str) -> Result<Label> {
        Label::new(&self.document, text)
    }

    pub fn button(&self, text: &str) -> Result<Button> {
        Button::new(&self.document, text)
    }

    pub fn checkbox(&self, label: &str) -> Result<Checkbox> {
        Checkbox::new(&self.document, label)
    }

    pub fn text_input(&self, text: &str) -> Result<TextInput> {
        TextInput::new(&self.document, text)
    }

    pub fn slider(&self, min: f64, max: f64) -> Result<Slider> {
        Slider::new(&self.document, min, max)
    }

    pub fn progress_bar(&self) -> Result<ProgressBar> {
        ProgressBar::new(&self.document)
    }

    /// ブラウザではアプリを終了する概念が無いため、何もしない。
    pub fn quit(&self) {}
}

/// UI を組み立てる。
///
/// ブラウザのイベントループはページ自身が回しているため、この関数は
/// `build` を実行したらすぐ戻る。ウィジェットとコールバックは
/// フレームワークが保持し続ける。
pub fn run<F>(settings: Settings, build: F) -> Result<()>
where
    F: FnOnce(&Ui) -> Result<()> + 'static,
{
    let document = document()?;
    document.set_title(&settings.name);
    let ui = Ui::new(document);
    build(&ui)?;
    // ウィンドウ (と、そこにぶら下がるクロージャ) をページの寿命まで保持する。
    KEEP.with(|k| k.borrow_mut().push(ui));
    Ok(())
}

thread_local! {
    static KEEP: RefCell<Vec<Ui>> = const { RefCell::new(Vec::new()) };
}
