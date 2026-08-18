//! # naui-web
//!
//! naui の Web バックエンド。**DOM の標準コントロール**
//! (`<button>` / `<input>` / `<progress>` …) をそのまま生成する。
//! ブラウザにおける「ネイティブ UI」はフォームコントロールそのものなので、
//! 見た目を作り込むことはせず、ブラウザ既定のスタイルに任せている。
//!
//! レイアウトだけは Flexbox を使う (AppKit の NSStackView、
//! WinUI 3 の StackPanel、GTK4 の GtkBox に対応する)。

#![cfg(target_arch = "wasm32")]
#![forbid(unsafe_code)]

mod file_picker;
mod layout;
mod list;
mod media;
mod navigation;
mod widgets;
mod window;

use naui_core::{Error, Orientation, Result, Settings, Theme};
use std::cell::Cell;
use std::cell::RefCell;
use wasm_bindgen::JsCast;
use web_sys::{Document, HtmlElement};

/// wasm の入口を組み立てるために再公開する。
///
/// `#[wasm_bindgen(start)]` を使う側が wasm-bindgen へ直接依存しなくて済むよう、
/// バックエンドが使っているものをそのまま渡す (版が食い違わない)。
#[doc(hidden)]
pub use wasm_bindgen;

pub use file_picker::FilePicker;
pub use layout::{Grid, Scroll, Spacer};
pub use list::List;
pub use media::{Audio, Image, Video};
pub use navigation::{Breadcrumbs, Dock, Link, Menu, Navbar, Pagination, Tabs};
pub use widgets::{
    Button, Checkbox, Label, ProgressBar, Slider, Stack, TextArea, TextInput, Widget,
};
pub use window::{WeakWindow, Window};

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
    theme: Cell<Theme>,
    windows: RefCell<Vec<Window>>,
}

impl Ui {
    fn new(document: Document, theme: Theme) -> Self {
        Self {
            document,
            theme: Cell::new(theme),
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

    /// 行と列で位置を決めるコンテナ。
    pub fn grid(&self) -> Result<Grid> {
        Grid::new(&self.document)
    }

    /// 中身がはみ出したらスクロールさせるコンテナ。
    pub fn scroll(&self) -> Result<Scroll> {
        Scroll::new(&self.document)
    }

    /// 余白そのものになるウィジェット。スタックの余りを吸って他を押しやる。
    pub fn spacer(&self) -> Result<Spacer> {
        Spacer::new(&self.document)
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

    /// 改行を含む文字列を入力できる欄。高さは `set_sizing` で指定する。
    pub fn text_area(&self, text: &str) -> Result<TextArea> {
        TextArea::new(&self.document, text)
    }

    pub fn slider(&self, min: f64, max: f64) -> Result<Slider> {
        Slider::new(&self.document, min, max)
    }

    pub fn progress_bar(&self) -> Result<ProgressBar> {
        ProgressBar::new(&self.document)
    }

    /// タブ。中身のウィジェットごと持つ。
    pub fn tabs(&self) -> Result<Tabs> {
        Tabs::new(&self.document)
    }

    /// 画面上部に置く横並びのナビゲーション。`title` は左端の見出し。
    pub fn navbar(&self, title: &str) -> Result<Navbar> {
        Navbar::new(&self.document, title)
    }

    /// 画面下部に置く横並びのナビゲーション (等幅)。
    pub fn dock(&self) -> Result<Dock> {
        Dock::new(&self.document)
    }

    /// 縦に並ぶナビゲーション一覧。
    pub fn menu(&self) -> Result<Menu> {
        Menu::new(&self.document)
    }

    /// 選択できる行の一覧。自分でスクロールする。
    pub fn list(&self) -> Result<List> {
        List::new(&self.document)
    }

    /// パンくず。
    pub fn breadcrumbs(&self) -> Result<Breadcrumbs> {
        Breadcrumbs::new(&self.document)
    }

    /// ページ送り。`page_count` はページ数。
    pub fn pagination(&self, page_count: usize) -> Result<Pagination> {
        Pagination::new(&self.document, page_count)
    }

    /// リンク。`href` が空でなければ、押したときに別タブで開く。
    pub fn link(&self, text: &str, href: &str) -> Result<Link> {
        Link::new(&self.document, text, href)
    }

    /// 画像。`source` はファイルパスか URL。
    pub fn image(&self, source: &str) -> Result<Image> {
        Image::new(&self.document, source)
    }

    /// 動画。`source` はファイルパスか URL。
    pub fn video(&self, source: &str) -> Result<Video> {
        Video::new(&self.document, source)
    }

    /// 音声。`source` はファイルパスか URL。
    pub fn audio(&self, source: &str) -> Result<Audio> {
        Audio::new(&self.document, source)
    }

    /// ファイルやフォルダーを選ばせるボタン。中身は `<input type="file">`。
    pub fn file_picker(&self, text: &str) -> Result<FilePicker> {
        FilePicker::new(&self.document, text)
    }

    /// 配色テーマを実行中に切り替える。
    pub fn set_theme(&self, theme: Theme) -> Result<()> {
        apply_theme(&self.document, theme)?;
        self.theme.set(theme);
        Ok(())
    }

    /// 現在選択されている配色テーマを返す。
    pub fn theme(&self) -> Theme {
        self.theme.get()
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
    apply_theme(&document, settings.theme)?;
    let ui = Ui::new(document, settings.theme);
    build(&ui)?;
    // ウィンドウ (と、そこにぶら下がるクロージャ) をページの寿命まで保持する。
    KEEP.with(|k| k.borrow_mut().push(ui));
    Ok(())
}

thread_local! {
    static KEEP: RefCell<Vec<Ui>> = const { RefCell::new(Vec::new()) };
}

pub(crate) fn apply_theme(document: &Document, theme: Theme) -> Result<()> {
    let root: HtmlElement = document
        .document_element()
        .ok_or_else(|| Error::new("document 要素の取得", "html 要素がありません"))?
        .unchecked_into();
    let value = match theme {
        Theme::System => "light dark",
        Theme::Light => "light",
        Theme::Dark => "dark",
    };
    root.style()
        .set_property("color-scheme", value)
        .map_err(|e| to_error("color-scheme の設定", e))
}
