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

mod color_picker;
mod combo_box;
mod date_picker;
mod dialog;
mod editable_combo_box;
mod expander;
mod file_picker;
mod file_saver;
mod layout;
mod list;
mod media;
mod navigation;
mod number_input;
mod popup;
mod radio_group;
mod table;
mod time_picker;
mod toast;
mod toggle;
mod toolbar;
mod tree;
mod widgets;
mod window;

use naui_core::{DatePickerMode, Error, Orientation, Result, Settings, Theme};
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

pub use color_picker::ColorPicker;
pub use combo_box::ComboBox;
pub use date_picker::DatePicker;
pub use dialog::Dialog;
pub use editable_combo_box::EditableComboBox;
pub use expander::Expander;
pub use file_picker::FilePicker;
pub use file_saver::FileSaver;
pub use layout::{Grid, Scroll, Spacer};
pub use list::List;
pub use media::{Audio, Image, Video};
pub use navigation::{Breadcrumbs, Dock, Link, Menu, Navbar, Pagination, Tabs};
pub use number_input::NumberInput;
pub use popup::PopupMenu;
pub use radio_group::RadioGroup;
pub use table::Table;
pub use time_picker::TimePicker;
pub use toast::Toast;
pub use toggle::Toggle;
pub use toolbar::Toolbar;
pub use tree::Tree;
pub use widgets::{
    Button, Checkbox, Label, PasswordInput, ProgressBar, SearchInput, Slider, Stack, TextArea,
    TextInput, Widget,
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
        value.as_string().unwrap_or_else(|| format!("{value:?}")),
    )
}

/// ウィジェットを生成するための入り口。
pub struct Ui {
    document: Document,
    theme: Cell<Theme>,
    windows: RefCell<Vec<Window>>,
    /// ダイアログはどこにも append されないので、ここで保持する。
    dialogs: RefCell<Vec<Dialog>>,
    /// ポップアップメニューはレイアウトに載らないので、親が保持してくれない。
    popups: RefCell<Vec<PopupMenu>>,
    /// ツールバーもレイアウトに載らないので、ここで保持する。
    toolbars: RefCell<Vec<Toolbar>>,
    /// トーストもレイアウトに載らないので、ここで保持する。
    toasts: RefCell<Vec<Toast>>,
}

impl Ui {
    fn new(document: Document, theme: Theme) -> Self {
        Self {
            document,
            theme: Cell::new(theme),
            windows: RefCell::new(Vec::new()),
            dialogs: RefCell::new(Vec::new()),
            popups: RefCell::new(Vec::new()),
            toolbars: RefCell::new(Vec::new()),
            toasts: RefCell::new(Vec::new()),
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

    /// 見出しを押して中身を出し入れするコンテナ。`text` は見出しの文字。
    pub fn expander(&self, text: &str) -> Result<Expander> {
        Expander::new(&self.document, text)
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

    /// 入り切りを切り替えるスイッチ。`label` はとなりへ添える文字。
    pub fn toggle(&self, label: &str) -> Result<Toggle> {
        Toggle::new(&self.document, label)
    }

    /// 1 項目を選ぶドロップダウン。
    pub fn combo_box(&self) -> Result<ComboBox> {
        ComboBox::new(&self.document)
    }

    /// 候補から選ぶことも、自由に打ち込むこともできるコンボボックス。
    pub fn editable_combo_box(&self) -> Result<EditableComboBox> {
        EditableComboBox::new(&self.document)
    }

    /// 選択肢を並べて 1 つだけ選ばせるラジオグループ。
    pub fn radio_group(&self) -> Result<RadioGroup> {
        RadioGroup::new(&self.document)
    }

    /// 日付や時刻を選ばせるコントロール。何を選ばせるかは `mode` で決める。
    pub fn date_picker(&self, mode: DatePickerMode) -> Result<DatePicker> {
        DatePicker::new(&self.document, mode)
    }

    /// 時刻だけを選ばせるコントロール。初期値は現在時刻。
    pub fn time_picker(&self) -> Result<TimePicker> {
        TimePicker::new(&self.document)
    }

    /// 色を選ばせるコントロール。初期値は黒。
    pub fn color_picker(&self) -> Result<ColorPicker> {
        ColorPicker::new(&self.document)
    }

    pub fn text_input(&self, text: &str) -> Result<TextInput> {
        TextInput::new(&self.document, text)
    }

    /// 伏せ字で入力させる欄。中身は `<input type="password">`。
    pub fn password_input(&self) -> Result<PasswordInput> {
        PasswordInput::new(&self.document)
    }

    /// 検索の入力欄。中身は `<input type="search">`。
    pub fn search_input(&self) -> Result<SearchInput> {
        SearchInput::new(&self.document)
    }

    /// 数値を入力させる欄。`value` は初期値。
    pub fn number_input(&self, value: f64) -> Result<NumberInput> {
        NumberInput::new(&self.document, value)
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

    /// ウィンドウの上端に付けるツールバー。
    ///
    /// [`Window::set_toolbar`] で取り付ける。フレームワークが参照を保持するので、
    /// 戻り値を捨てても通知が届かなくなることはない。
    pub fn toolbar(&self) -> Result<Toolbar> {
        let toolbar = Toolbar::new(&self.document)?;
        self.toolbars.borrow_mut().push(toolbar.clone());
        Ok(toolbar)
    }

    /// 縦に並ぶナビゲーション一覧。
    pub fn menu(&self) -> Result<Menu> {
        Menu::new(&self.document)
    }

    /// 選択できる行の一覧。自分でスクロールする。
    pub fn list(&self) -> Result<List> {
        List::new(&self.document)
    }

    /// 列見出しを持つ表。自分でスクロールする。
    pub fn table(&self) -> Result<Table> {
        Table::new(&self.document)
    }

    /// 入れ子の項目を開閉できる一覧。自分でスクロールする。
    pub fn tree(&self) -> Result<Tree> {
        Tree::new(&self.document)
    }

    /// 右クリックで出るポップアップ (コンテキスト) メニュー。
    ///
    /// フレームワークが参照を保持するので、戻り値を捨てても
    /// 取り付け先から消えることはない。
    pub fn popup_menu(&self) -> Result<PopupMenu> {
        let popup = PopupMenu::new(&self.document)?;
        self.popups.borrow_mut().push(popup.clone());
        Ok(popup)
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

    /// 内容をファイルへ保存させるボタン。押すと保存ダイアログが出る。
    ///
    /// `showSaveFilePicker` があればそれを、無ければダウンロードを使う。
    pub fn file_saver(&self, text: &str) -> Result<FileSaver> {
        FileSaver::new(&self.document, text)
    }

    /// 一時的な通知 (トースト)。`message` は出す文字列。
    ///
    /// フレームワークが参照を保持するので、戻り値を捨てても
    /// 通知が届かなくなることはない。
    pub fn toast(&self, message: &str) -> Result<Toast> {
        let toast = Toast::new(&self.document, message)?;
        self.toasts.borrow_mut().push(toast.clone());
        Ok(toast)
    }

    /// モーダルダイアログ。`title` は見出し。中身は `<dialog>`。
    ///
    /// フレームワークが参照を保持するので、戻り値を捨てても
    /// 通知が届かなくなることはない。
    pub fn dialog(&self, title: &str) -> Result<Dialog> {
        let d = Dialog::new(&self.document, title)?;
        self.dialogs.borrow_mut().push(d.clone());
        Ok(d)
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
