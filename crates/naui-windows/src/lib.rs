//! # naui-windows
//!
//! naui の Windows バックエンド。**WinUI 3 (Fluent 2) の実コントロール**
//! (`Microsoft.UI.Xaml.Controls.Button` など) を生成する。
//!
//! WinUI 3 は Windows SDK ではなく **Windows App SDK** に含まれるため、
//! - 実行環境に Windows App SDK ランタイムが必要
//! - コントロールは `Application::Start` の後でしか生成できない
//!
//! という制約がある。後者のために naui の公開 API は
//! 「コールバックの中で UI を組み立てる」形になっている。
//!
//! ## 実行要件
//!
//! Windows App SDK 2.x ランタイムが必要。実行時には `V2` のフレームワーク
//! パッケージ依存関係を追加し、インストール済みの最新2.xランタイムを使用する。

#![cfg(target_os = "windows")]

mod app;
mod color_picker;
mod combo_box;
mod date_picker;
mod dialog;
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
mod toast;
mod toggle;
mod toolbar;
mod tree;
mod ui_thread;
mod widgets;
mod window;

use std::cell::{Cell, RefCell};

use naui_core::{DatePickerMode, Error, Orientation, Result, Settings, Theme};

pub use color_picker::ColorPicker;
pub use combo_box::ComboBox;
pub use date_picker::DatePicker;
pub use dialog::Dialog;
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
pub use toast::Toast;
pub use toggle::Toggle;
pub use toolbar::Toolbar;
pub use tree::Tree;
pub use widgets::{
    Button, Checkbox, Label, PasswordInput, ProgressBar, Slider, Stack, TextArea, TextInput, Widget,
};
pub use window::{WeakWindow, Window};

pub(crate) fn to_error(context: &'static str, e: windows_core::Error) -> Error {
    Error::new(context, e.message())
}

/// ウィジェットを生成するための入り口。
pub struct Ui {
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
    shutdown: &'static ui_thread::UiThreadCell<Option<Ui>>,
}

impl Ui {
    fn new(theme: Theme, shutdown: &'static ui_thread::UiThreadCell<Option<Ui>>) -> Self {
        Self {
            theme: Cell::new(theme),
            windows: RefCell::new(Vec::new()),
            dialogs: RefCell::new(Vec::new()),
            popups: RefCell::new(Vec::new()),
            toolbars: RefCell::new(Vec::new()),
            toasts: RefCell::new(Vec::new()),
            shutdown,
        }
    }

    pub fn window(&self, title: &str, width: f64, height: f64) -> Result<Window> {
        let w = Window::new(title, width, height, self.theme.get())?;
        w.install_closing_handler(self.shutdown);
        self.windows.borrow_mut().push(w.clone());
        Ok(w)
    }

    fn clear_windows_for_shutdown(&self) {
        for window in self.windows.borrow().iter() {
            window.clear_content_for_shutdown();
        }
    }

    pub fn stack(&self, orientation: Orientation) -> Result<Stack> {
        Stack::new(orientation)
    }

    /// 行と列で位置を決めるコンテナ。
    pub fn grid(&self) -> Result<Grid> {
        Grid::new()
    }

    /// 中身がはみ出したらスクロールさせるコンテナ。
    pub fn scroll(&self) -> Result<Scroll> {
        Scroll::new()
    }

    /// 見出しを押して中身を出し入れするコンテナ。`text` は見出しの文字。
    pub fn expander(&self, text: &str) -> Result<Expander> {
        Expander::new(text)
    }

    /// 余白そのものになるウィジェット。`Grid` の `Track::Fill` と組み合わせて使う。
    pub fn spacer(&self) -> Result<Spacer> {
        Spacer::new()
    }

    pub fn label(&self, text: &str) -> Result<Label> {
        Label::new(text)
    }

    pub fn button(&self, text: &str) -> Result<Button> {
        Button::new(text)
    }

    pub fn checkbox(&self, label: &str) -> Result<Checkbox> {
        Checkbox::new(label)
    }

    /// 入り切りを切り替えるスイッチ。`label` はとなりへ添える文字。
    pub fn toggle(&self, label: &str) -> Result<Toggle> {
        Toggle::new(label)
    }

    /// 選択肢を折りたたんで表示するコンボボックス。
    pub fn combo_box(&self) -> Result<ComboBox> {
        ComboBox::new()
    }

    /// 選択肢を並べて 1 つだけ選ばせるラジオグループ。
    pub fn radio_group(&self) -> Result<RadioGroup> {
        RadioGroup::new()
    }

    /// 日付や時刻を選ばせるコントロール。何を選ばせるかは `mode` で決める。
    pub fn date_picker(&self, mode: DatePickerMode) -> Result<DatePicker> {
        DatePicker::new(mode)
    }

    /// 色を選ばせるコントロール。初期値は黒。
    pub fn color_picker(&self) -> Result<ColorPicker> {
        ColorPicker::new()
    }

    pub fn text_input(&self, text: &str) -> Result<TextInput> {
        TextInput::new(text)
    }

    /// 伏せ字で入力させる欄。中身は `PasswordBox`。
    pub fn password_input(&self) -> Result<PasswordInput> {
        PasswordInput::new()
    }

    /// 数値を入力させる欄。`value` は初期値。
    pub fn number_input(&self, value: f64) -> Result<NumberInput> {
        NumberInput::new(value)
    }

    /// 改行を含む文字列を入力できる欄。高さは `set_sizing` で指定する。
    pub fn text_area(&self, text: &str) -> Result<TextArea> {
        TextArea::new(text)
    }

    pub fn slider(&self, min: f64, max: f64) -> Result<Slider> {
        Slider::new(min, max)
    }

    pub fn progress_bar(&self) -> Result<ProgressBar> {
        ProgressBar::new()
    }

    /// タブ。中身のウィジェットごと持つ。
    pub fn tabs(&self) -> Result<Tabs> {
        Tabs::new()
    }

    /// 画面上部に置く横並びのナビゲーション。`title` は左端の見出し。
    pub fn navbar(&self, title: &str) -> Result<Navbar> {
        Navbar::new(title)
    }

    /// 画面下部に置く横並びのナビゲーション。
    pub fn dock(&self) -> Result<Dock> {
        Dock::new()
    }

    /// ウィンドウの上端に付けるツールバー。
    ///
    /// [`Window::set_toolbar`] で取り付ける。フレームワークが参照を保持するので、
    /// 戻り値を捨てても通知が届かなくなることはない。
    pub fn toolbar(&self) -> Result<Toolbar> {
        let toolbar = Toolbar::new()?;
        self.toolbars.borrow_mut().push(toolbar.clone());
        Ok(toolbar)
    }

    /// 縦に並ぶナビゲーション一覧。
    pub fn menu(&self) -> Result<Menu> {
        Menu::new()
    }

    /// 選択できる行の一覧。自分でスクロールする。
    pub fn list(&self) -> Result<List> {
        List::new()
    }

    /// 入れ子の項目を開閉できる一覧。自分でスクロールする。
    pub fn tree(&self) -> Result<Tree> {
        Tree::new()
    }

    /// 右クリックで出るポップアップ (コンテキスト) メニュー。
    ///
    /// フレームワークが参照を保持するので、戻り値を捨てても
    /// 取り付け先から消えることはない。
    pub fn popup_menu(&self) -> Result<PopupMenu> {
        let popup = PopupMenu::new()?;
        self.popups.borrow_mut().push(popup.clone());
        Ok(popup)
    }

    /// パンくず。
    pub fn breadcrumbs(&self) -> Result<Breadcrumbs> {
        Breadcrumbs::new()
    }

    /// ページ送り。`page_count` はページ数。
    pub fn pagination(&self, page_count: usize) -> Result<Pagination> {
        Pagination::new(page_count)
    }

    /// リンク。`href` が空でなければ、押したときにブラウザで開く。
    pub fn link(&self, text: &str, href: &str) -> Result<Link> {
        Link::new(text, href)
    }

    /// 画像。`source` はファイルパスか URL。
    pub fn image(&self, source: &str) -> Result<Image> {
        Image::new(source)
    }

    /// 動画。`source` はファイルパスか URL。
    pub fn video(&self, source: &str) -> Result<Video> {
        Video::new(source)
    }

    /// 音声。`source` はファイルパスか URL。
    pub fn audio(&self, source: &str) -> Result<Audio> {
        Audio::new(source)
    }

    /// ファイルやフォルダーを選ばせるボタン。押すと共通ダイアログが出る。
    pub fn file_picker(&self, text: &str) -> Result<FilePicker> {
        FilePicker::new(text)
    }

    /// 内容をファイルへ保存させるボタン。押すと共通ダイアログの保存が出る。
    pub fn file_saver(&self, text: &str) -> Result<FileSaver> {
        FileSaver::new(text)
    }

    /// 一時的な通知 (トースト)。`message` は出す文字列。
    ///
    /// フレームワークが参照を保持するので、戻り値を捨てても
    /// 通知が届かなくなることはない。
    pub fn toast(&self, message: &str) -> Result<Toast> {
        let toast = Toast::new(message)?;
        self.toasts.borrow_mut().push(toast.clone());
        Ok(toast)
    }

    /// モーダルダイアログ。`title` は見出し。中身は `ContentDialog`。
    ///
    /// フレームワークが参照を保持するので、戻り値を捨てても
    /// 通知が届かなくなることはない。
    pub fn dialog(&self, title: &str) -> Result<Dialog> {
        let d = Dialog::new(title, self.theme.get())?;
        self.dialogs.borrow_mut().push(d.clone());
        Ok(d)
    }

    /// 配色テーマを実行中に切り替える。
    pub fn set_theme(&self, theme: Theme) -> Result<()> {
        for window in self.windows.borrow().iter() {
            window.set_theme(theme)?;
        }
        // ダイアログはウィンドウとは別の層に出るので、個別に伝える。
        for dialog in self.dialogs.borrow().iter() {
            dialog.set_theme(theme);
        }
        self.theme.set(theme);
        Ok(())
    }

    /// 現在選択されている配色テーマを返す。
    pub fn theme(&self) -> Theme {
        self.theme.get()
    }

    /// アプリを終了する。
    pub fn quit(&self) {
        if let Ok(app) = winui3::Microsoft::UI::Xaml::Application::Current() {
            let _ = app.Exit();
        }
    }
}

/// アプリを起動し、`build` の中で UI を組み立てる。
///
/// Windows App SDK のブートストラップを行ってから `Application::Start` に入り、
/// XAML の初期化が終わったところで `build` を呼ぶ。
/// この関数はアプリが終了するまで戻らない。
pub fn run<F>(settings: Settings, build: F) -> Result<()>
where
    F: FnOnce(&Ui) -> Result<()> + 'static,
{
    use winui3::Microsoft::UI::Xaml::{Application, ApplicationInitializationCallback};
    use winui3::{init_apartment, ApartmentType};

    let _dependency = winui3::bootstrap::PackageDependency::initialize_version(
        winui3::bootstrap::WindowsAppSDKVersion::V2,
    )
    .map_err(|e| to_error("Windows App SDK 2.x の初期化", e))?;
    init_apartment(ApartmentType::SingleThreaded)
        .map_err(|e| to_error("COM アパートメントの初期化", e))?;

    let failure: &'static ui_thread::UiThreadCell<Option<Error>> =
        Box::leak(Box::new(ui_thread::UiThreadCell::new(None)));
    let state: &'static ui_thread::UiThreadCell<Option<F>> =
        Box::leak(Box::new(ui_thread::UiThreadCell::new(Some(build))));
    let ui_state: &'static ui_thread::UiThreadCell<Option<Ui>> =
        Box::leak(Box::new(ui_thread::UiThreadCell::new(None)));

    Application::Start(&ApplicationInitializationCallback::new(move |_| {
        let app = app::compose(state, failure, ui_state, settings.theme)?;
        std::mem::forget(app);
        Ok(())
    }))
    .map_err(|e| to_error("Application::Start", e))?;

    match failure.with_mut_cross_thread(|slot| slot.take()) {
        Some(e) => Err(e),
        None => Ok(()),
    }
}
