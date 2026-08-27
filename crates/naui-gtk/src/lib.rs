//! # naui-gtk
//!
//! naui の Linux バックエンド。**GTK4 / libadwaita の実コントロール**
//! (`GtkApplicationWindow` / `GtkButton` / `GtkEntry` / `GtkScale` /
//! `GtkListBox` …) を生成し、GTK4 のシグナルを Rust のクロージャへ中継する。
//!
//! 描画・レイアウト・IME・アクセシビリティ・OS のテーマ追従はすべて GTK4 が行う。
//!
//! ## 対応表
//!
//! | naui | GTK4 / libadwaita |
//! | --- | --- |
//! | `run` | `AdwApplication` + `connect_activate` (コールバック内で UI 構築) |
//! | `Window` | `AdwApplicationWindow` + `AdwToolbarView` + `AdwHeaderBar` |
//! | `Stack` | `GtkBox` |
//! | `Grid` | `GtkGrid` |
//! | `Scroll` | `GtkScrolledWindow` |
//! | `Expander` | `GtkExpander` |
//! | `SplitView` | `GtkPaned` |
//! | `Spacer` | 中身の無い `GtkBox` (`hexpand` / `vexpand`) |
//! | 大きさの指定 | `size_request` / `hexpand` / `halign` + [`SizeBin`] の上限 |
//! | `Label` | `GtkLabel` |
//! | `Button` | `GtkButton` |
//! | `Checkbox` | `GtkCheckButton` |
//! | `Toggle` | `GtkSwitch` + `GtkLabel` を `GtkBox` へ並べたもの |
//! | `ComboBox` | `GtkDropDown` + `GtkStringList` |
//! | `EditableComboBox` | `GtkEntry` + `GtkMenuButton` (`GtkListBox` のポップオーバー) |
//! | `RadioGroup` | 組にした `GtkCheckButton` を `GtkBox` へ並べたもの |
//! | `DatePicker` | `GtkMenuButton` + `GtkCalendar` / `GtkSpinButton` の組 |
//! | `TimePicker` | 時と分の `GtkSpinButton` を `:` で挟んだもの |
//! | `ColorPicker` | `GtkColorDialogButton` + `GtkColorDialog` |
//! | `TextInput` | `GtkEntry` |
//! | `TextArea` | `GtkTextView` を `GtkScrolledWindow` に載せたもの |
//! | `Slider` | `GtkScale` |
//! | `ProgressBar` | `GtkProgressBar` |
//! | `Tabs` | `GtkNotebook` |
//! | `Navbar` / `Dock` / `Menu` / `Breadcrumbs` / `Pagination` | `GtkToggleButton` の並び |
//! | `PopupMenu` | `GtkPopoverMenu` + `GMenu` + `GSimpleAction` |
//! | `List` | `GtkListBox` を `GtkScrolledWindow` に載せたもの |
//! | `Link` | `GtkLinkButton` |
//! | `Image` | `GtkPicture` |
//! | `Video` | `GtkPicture` (`GtkMediaFile` を映す) + `GtkMediaControls` |
//! | `Audio` | `GtkMediaControls` + `GtkMediaFile` |
//! | `FilePicker` | `GtkButton` + `GtkFileDialog` |
//! | `Dialog` | `AdwAlertDialog` |
//! | `Toast` | `AdwToast` + `AdwToastOverlay` |
//!
//! ## 他のバックエンドとの違い
//!
//! - **`Video` の再生バーは出し入れできる**が、`GtkVideo` ではなく
//!   `GtkPicture` + `GtkMediaControls` で組んでいるためで、収め方
//!   ([`Fit`](naui_core::Fit)) もこの形でだけ効く。
//! - [`Fit::None`](naui_core::Fit::None) (原寸) にあたるものが GTK4 に無いため、
//!   **拡大はしないが縮小はする** `GTK_CONTENT_FIT_SCALE_DOWN` に写す。
//! - `Grid` の [`Track::Fill`](naui_core::Track::Fill) の**重みは効かない**。
//!   `GtkGrid` は列や行そのものに幅を持たせられず、余りは広がる列で等分される
//!   (macOS と同じ制限)。
//! - 配色テーマは `AdwStyleManager` がアプリ全体に持つため、
//!   [`Window::set_theme`] もアプリ全体に効く。
//! - `AdwApplicationWindow` は `GtkApplicationWindow` と違い**既定の
//!   タイトルバーを持たない**ので、`AdwHeaderBar` を自分で載せている。
//!   最小化・最大化・閉じるのボタンはこれが出す (どちら側に並ぶかは
//!   デスクトップの設定に従う)。
//! - **消音を解いたときは、naui が持っている音量を入れ直している。**
//!   `GtkMediaControls` は消音になると音量つまみを 0 へ動かし、その 0 を
//!   `GtkMediaStream` へ書き戻すが、GTK4 は消音を解いても音量を戻さない。
//!   そのままだと消音を外しても音が出ない。
//! - 再生できる形式は GStreamer に入っているプラグイン次第
//!   (`GtkMediaFile` が GStreamer に載っているため)。
//! - `List` の複数選択では、`GtkListBox` の**「1 クリックで確定」を切っている**
//!   (`gtk_list_box_set_activate_on_single_click`)。これが有効な間、
//!   `GtkListBox` はクリックに付いている Ctrl / Shift を読まず、必ず
//!   「その行だけを選ぶ」に倒すため、行を足すことも外すこともできなくなる。
//! - `Tabs` は**タブ列を送れるようにしてある** (`gtk_notebook_set_scrollable`)。
//!   既定の `GtkNotebook` は「全タブが横に並ぶ幅」を最小幅として申告するため、
//!   タブが増えるとウィンドウをそれ以下に縮められなくなる。
//! - **`DatePicker` はカレンダーを開いても、日を押した時点では閉じない。**
//!   `GtkCalendar` は「日を押した」と「月を送った」を区別せず、どちらも
//!   `day-selected` で届くため、押すたびに閉じると月を送れなくなる。
//! - `Checkbox` と `RadioGroup` は、**印をラベルの字面の中心へ寄せ直している**。
//!   GTK4 は印を行の箱 (ascent + descent) の中心に置くが、日本語を含む行は
//!   ascent が大きく取られるぶん字面が下に寄り、印だけが浮いて見えるため
//!   (詳しくは `indicator` モジュール)。
//!
//! ## ウィンドウを縮められる下限
//!
//! GTK4 のウィンドウは、中身が申告する**最小の大きさより小さくできない**。
//! そのため [`Length::Fill`](naui_core::Length::Fill) を指定した軸は、
//! `measure` の最小を 0 として申告している。`Fill` は「大きさは親が決める」
//! という指定なので、中身の都合でウィンドウの下限を決めてしまわないようにする
//! ため (Web バックエンドが `min-width: 0` を書いているのと同じ理由)。
//!
//! 縮めすぎないようにするには [`Sizing::min_width`](naui_core::Sizing::min_width)
//! などを使う。こちらは `gtk_widget_set_size_request` として GTK4 が改めて
//! 下限に効かせる。
//!
//! GTK4 は既定でははみ出した中身を切り取らないので、そのままだと縮めたときに
//! ウィンドウの外まで描かれてしまう。そこで
//!
//! - ウィンドウの中身 (`AdwToolbarView`)
//! - `Fill` を指定した軸を持つウィジェット
//!
//! の 2 か所を `GTK_OVERFLOW_HIDDEN` にしている。最小を 0 と申告する以上、
//! 配られた場所からはみ出して描かないのが筋で、CSS の `min-width: 0` と
//! `overflow: hidden` を組みで使うのと同じ。
//!
//! GTK4 のシグナルハンドラは `'static` なクロージャを受けるので、macOS / Web と
//! 同じ `Rc<Inner>` + クロージャ保持の形がそのまま使える (Windows のような
//! `Send + Sync` 制約は無い)。別スレッドからの受け渡しだけは例外で、
//! `glib::idle_add_once` を通してメインループへ戻す (`crate::main_thread`)。

#![cfg(all(
    unix,
    not(any(target_os = "macos", target_os = "ios", target_os = "android"))
))]
// glib のサブクラス化マクロだけが `unsafe impl` を必要とする (crate::bin)。
#![deny(unsafe_code)]

mod bin;
mod callback;
mod color_picker;
mod combo_box;
mod date_picker;
mod dialog;
mod editable_combo_box;
mod expander;
mod file_picker;
mod file_saver;
mod indicator;
mod layout;
mod list;
mod main_thread;
mod media;
mod navigation;
mod number_input;
mod popup;
mod radio_group;
mod split_view;
mod table;
mod time_picker;
mod toast;
mod toggle;
mod toolbar;
mod tree;
mod widgets;
mod window;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::gio;
use gtk::glib;
use gtk::prelude::*;
use naui_core::{DatePickerMode, Error, Orientation, Result, Settings, Tasks, Theme};

pub use bin::SizeBin;
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
pub use split_view::SplitView;
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

/// 配色テーマをアプリ全体へ適用する。
///
/// libadwaita のテーマはアプリに 1 つしか無いので、ウィンドウごとには持てない。
pub(crate) fn apply_theme(theme: Theme) {
    adw::StyleManager::default().set_color_scheme(match theme {
        Theme::System => adw::ColorScheme::Default,
        Theme::Light => adw::ColorScheme::ForceLight,
        Theme::Dark => adw::ColorScheme::ForceDark,
    });
}

/// ウィジェットを生成するための入り口。
///
/// GTK4 は `GtkApplication` が起動したあとでしかウィンドウを作れないため、
/// `Ui` は [`run`] のコールバックの中でしか得られない。
pub struct Ui {
    app: adw::Application,
    theme: Cell<Theme>,
    /// コールバックが終わってもウィンドウを生かしておくための保持。
    windows: RefCell<Vec<Window>>,
    /// ダイアログはどこにも append されないので、ここで保持する。
    dialogs: RefCell<Vec<Dialog>>,
    /// ポップアップメニューはレイアウトに載らないので、親が保持してくれない。
    popups: RefCell<Vec<PopupMenu>>,
    /// ツールバーもレイアウトに載らないので、ここで保持する。
    toolbars: RefCell<Vec<Toolbar>>,
    /// トーストもレイアウトに載らないので、ここで保持する。
    toasts: RefCell<Vec<Toast>>,
    /// 別スレッドと非同期処理の入り口。
    tasks: Tasks,
}

impl Ui {
    fn new(app: adw::Application, theme: Theme) -> Self {
        Self {
            app,
            theme: Cell::new(theme),
            windows: RefCell::new(Vec::new()),
            dialogs: RefCell::new(Vec::new()),
            popups: RefCell::new(Vec::new()),
            toolbars: RefCell::new(Vec::new()),
            toasts: RefCell::new(Vec::new()),
            tasks: Tasks::from_main_thread(std::sync::Arc::new(main_thread::Idle)),
        }
    }

    /// 対応する `GtkApplication`。バックエンド固有の脱出口として公開している。
    pub fn native_application(&self) -> adw::Application {
        self.app.clone()
    }

    /// ウィンドウを作る。フレームワークが参照を保持するので、
    /// 戻り値を捨てても閉じられることはない。
    pub fn window(&self, title: &str, width: f64, height: f64) -> Result<Window> {
        let window = Window::new(&self.app, title, width, height);
        self.windows.borrow_mut().push(window.clone());
        Ok(window)
    }

    pub fn stack(&self, orientation: Orientation) -> Result<Stack> {
        Ok(Stack::new(orientation))
    }

    /// 行と列で位置を決めるコンテナ。
    pub fn grid(&self) -> Result<Grid> {
        Ok(Grid::new())
    }

    /// 中身がはみ出したらスクロールさせるコンテナ。
    pub fn scroll(&self) -> Result<Scroll> {
        Ok(Scroll::new())
    }

    /// 見出しを押して中身を出し入れするコンテナ。`text` は見出しの文字。
    pub fn expander(&self, text: &str) -> Result<Expander> {
        Ok(Expander::new(text))
    }

    /// 2 つの区画を、動かせる仕切りで分けるコンテナ。
    ///
    /// `Horizontal` なら区画が横に並び、仕切りは縦になる。
    pub fn split_view(&self, orientation: Orientation) -> Result<SplitView> {
        Ok(SplitView::new(orientation))
    }

    /// 余白そのものになるウィジェット。スタックの余りを吸って他を押しやる。
    pub fn spacer(&self) -> Result<Spacer> {
        Ok(Spacer::new())
    }

    pub fn label(&self, text: &str) -> Result<Label> {
        Ok(Label::new(text))
    }

    pub fn button(&self, text: &str) -> Result<Button> {
        Ok(Button::new(text))
    }

    pub fn checkbox(&self, label: &str) -> Result<Checkbox> {
        Ok(Checkbox::new(label))
    }

    /// 入り切りを切り替えるスイッチ。`label` はとなりへ添える文字。
    pub fn toggle(&self, label: &str) -> Result<Toggle> {
        Ok(Toggle::new(label))
    }

    /// 選択肢を折りたたんで表示するコンボボックス。
    pub fn combo_box(&self) -> Result<ComboBox> {
        Ok(ComboBox::new())
    }

    /// 候補から選ぶことも、自由に打ち込むこともできるコンボボックス。
    pub fn editable_combo_box(&self) -> Result<EditableComboBox> {
        Ok(EditableComboBox::new())
    }

    /// 選択肢を並べて 1 つだけ選ばせるラジオグループ。
    pub fn radio_group(&self) -> Result<RadioGroup> {
        Ok(RadioGroup::new())
    }

    /// 日付や時刻を選ばせるコントロール。何を選ばせるかは `mode` で決める。
    pub fn date_picker(&self, mode: DatePickerMode) -> Result<DatePicker> {
        Ok(DatePicker::new(mode))
    }

    /// 時刻だけを選ばせるコントロール。初期値は現在時刻。
    pub fn time_picker(&self) -> Result<TimePicker> {
        Ok(TimePicker::new())
    }

    /// 色を選ばせるコントロール。初期値は黒。
    pub fn color_picker(&self) -> Result<ColorPicker> {
        Ok(ColorPicker::new())
    }

    pub fn text_input(&self, text: &str) -> Result<TextInput> {
        Ok(TextInput::new(text))
    }

    /// 伏せ字で入力させる欄。中身は `GtkPasswordEntry`。
    pub fn password_input(&self) -> Result<PasswordInput> {
        Ok(PasswordInput::new())
    }

    /// 検索の入力欄。中身は `GtkSearchEntry`。
    pub fn search_input(&self) -> Result<SearchInput> {
        Ok(SearchInput::new())
    }

    /// 数値を入力させる欄。`value` は初期値。
    pub fn number_input(&self, value: f64) -> Result<NumberInput> {
        Ok(NumberInput::new(value))
    }

    /// 改行を含む文字列を入力できる欄。高さは `set_sizing` で指定する。
    pub fn text_area(&self, text: &str) -> Result<TextArea> {
        Ok(TextArea::new(text))
    }

    pub fn slider(&self, min: f64, max: f64) -> Result<Slider> {
        Ok(Slider::new(min, max))
    }

    pub fn progress_bar(&self) -> Result<ProgressBar> {
        Ok(ProgressBar::new())
    }

    /// 画像。`source` はファイルパスか URL。
    pub fn image(&self, source: &str) -> Result<Image> {
        Ok(Image::new(source))
    }

    /// 動画。`source` はファイルパスか URL。
    pub fn video(&self, source: &str) -> Result<Video> {
        Ok(Video::new(source))
    }

    /// 音声。`source` はファイルパスか URL。
    pub fn audio(&self, source: &str) -> Result<Audio> {
        Ok(Audio::new(source))
    }

    /// タブ。中身のウィジェットごと持つ。
    pub fn tabs(&self) -> Result<Tabs> {
        Ok(Tabs::new())
    }

    /// 画面上部に置く横並びのナビゲーション。`title` は左端の見出し。
    pub fn navbar(&self, title: &str) -> Result<Navbar> {
        Ok(Navbar::new(title))
    }

    /// 画面下部に置く横並びのナビゲーション (等幅)。
    pub fn dock(&self) -> Result<Dock> {
        Ok(Dock::new())
    }

    /// 縦に並ぶナビゲーション一覧。
    pub fn menu(&self) -> Result<Menu> {
        Ok(Menu::new())
    }

    /// ウィンドウの上端に付けるツールバー。
    ///
    /// [`Window::set_toolbar`] で取り付ける。フレームワークが参照を保持するので、
    /// 戻り値を捨てても通知が届かなくなることはない。
    pub fn toolbar(&self) -> Result<Toolbar> {
        let toolbar = Toolbar::new();
        self.toolbars.borrow_mut().push(toolbar.clone());
        Ok(toolbar)
    }

    /// 選択できる行の一覧。自分でスクロールする。
    pub fn list(&self) -> Result<List> {
        Ok(List::new())
    }

    /// 列見出しを持つ表。自分でスクロールする。
    pub fn table(&self) -> Result<Table> {
        Ok(Table::new())
    }

    /// 入れ子の項目を開閉できる一覧。自分でスクロールする。
    pub fn tree(&self) -> Result<Tree> {
        Ok(Tree::new())
    }

    /// 右クリックで出るポップアップ (コンテキスト) メニュー。
    ///
    /// フレームワークが参照を保持するので、戻り値を捨てても
    /// 取り付け先から消えることはない。
    pub fn popup_menu(&self) -> Result<PopupMenu> {
        let popup = PopupMenu::new();
        self.popups.borrow_mut().push(popup.clone());
        Ok(popup)
    }

    /// パンくず。
    pub fn breadcrumbs(&self) -> Result<Breadcrumbs> {
        Ok(Breadcrumbs::new())
    }

    /// ページ送り。`page_count` はページ数。
    pub fn pagination(&self, page_count: usize) -> Result<Pagination> {
        Ok(Pagination::new(page_count))
    }

    /// リンク。`href` が空でなければ、押したときに既定のハンドラで開く。
    pub fn link(&self, text: &str, href: &str) -> Result<Link> {
        Ok(Link::new(text, href))
    }

    /// ファイルやフォルダーを選ばせるボタン。押すと `GtkFileDialog` が出る。
    pub fn file_picker(&self, text: &str) -> Result<FilePicker> {
        Ok(FilePicker::new(text))
    }

    /// 内容をファイルへ保存させるボタン。押すと `GtkFileDialog` の保存が出る。
    pub fn file_saver(&self, text: &str) -> Result<FileSaver> {
        Ok(FileSaver::new(text))
    }

    /// 一時的な通知 (トースト)。`message` は出す文字列。
    ///
    /// フレームワークが参照を保持するので、戻り値を捨てても
    /// 通知が届かなくなることはない。
    pub fn toast(&self, message: &str) -> Result<Toast> {
        let toast = Toast::new(&self.app, message);
        self.toasts.borrow_mut().push(toast.clone());
        Ok(toast)
    }

    /// モーダルダイアログ。フレームワークが参照を保持する。
    pub fn dialog(&self, title: &str) -> Result<Dialog> {
        let dialog = Dialog::new(&self.app, title);
        self.dialogs.borrow_mut().push(dialog.clone());
        Ok(dialog)
    }

    /// 配色テーマを切り替える。
    pub fn set_theme(&self, theme: Theme) -> Result<()> {
        self.theme.set(theme);
        apply_theme(theme);
        Ok(())
    }

    pub fn theme(&self) -> Theme {
        self.theme.get()
    }

    /// 別スレッドや非同期処理から画面を書き換えるための入り口。
    ///
    /// 返る [`Tasks`] は clone してコールバックへ持ち込める。
    pub fn tasks(&self) -> Tasks {
        self.tasks.clone()
    }

    /// アプリを終了する。
    pub fn quit(&self) {
        self.app.quit();
    }
}

thread_local! {
    /// [`run_for_test`] が使い回す `GtkApplication`。
    static TEST_APP: RefCell<Option<adw::Application>> = const { RefCell::new(None) };
}

thread_local! {
    /// `run` のコールバックが終わってからも `Ui` を生かしておく。
    ///
    /// ウィジェットのクロージャは `Ui` が持つハンドル (ダイアログ・ポップアップ)
    /// を参照するので、コールバックの終わりで落とすわけにはいかない。
    static KEEP_ALIVE: RefCell<Vec<Rc<Ui>>> = const { RefCell::new(Vec::new()) };
}

/// メインループを回さずに `Ui` だけを作る。**自動テスト専用**。
///
/// GTK4 のウィジェットは `gtk_init` のあとでないと作れないので、初期化と
/// `GtkApplication` の登録だけを行い、`build` をそのまま呼ぶ。
#[doc(hidden)]
pub fn run_for_test<F>(build: F) -> Result<()>
where
    F: FnOnce(&Ui) -> Result<()>,
{
    if !gtk::is_initialized() {
        adw::init().map_err(|e| Error::new("テスト用の起動", e.to_string()))?;
        // メインループを回さないのでフレームクロックが進まない。`GtkSwitch` の
        // ような、切り替えをアニメーションで見せるウィジェットは、アニメーション
        // が入ったままだと値が変わらないままになる。デスクトップの設定に
        // 左右されないよう、テストの間だけ切っておく。
        if let Some(settings) = gtk::Settings::default() {
            settings.set_gtk_enable_animations(false);
        }
    }
    // `GtkApplication` は 1 プロセスに 1 つ。登録は同じオブジェクトパスを
    // 使うので、ケースごとに作り直すと 2 回目の登録で失敗する。
    let app = TEST_APP.with(|slot| -> Result<adw::Application> {
        if let Some(app) = slot.borrow().as_ref() {
            return Ok(app.clone());
        }
        let app = adw::Application::builder()
            .application_id("org.naui.test")
            // 既存プロセスへ起動要求が転送されないようにする。
            .flags(gio::ApplicationFlags::NON_UNIQUE)
            .build();
        app.register(gio::Cancellable::NONE)
            .map_err(|e| Error::new("テスト用の起動", e.to_string()))?;
        *slot.borrow_mut() = Some(app.clone());
        Ok(app)
    })?;
    let ui = Ui::new(app, Theme::System);
    build(&ui)
}

/// アプリを起動し、`build` の中で UI を組み立てる。
///
/// `build` は `GtkApplication` の `activate` の中で 1 回だけ呼ばれる。
/// この関数はウィンドウがすべて閉じるまで戻らない。
pub fn run<F>(settings: Settings, build: F) -> Result<()>
where
    F: FnOnce(&Ui) -> Result<()> + 'static,
{
    // GTK4 のアプリ ID は書き方が決まっている。不正なまま渡すと GLib が
    // 落ちるので、ここで弾いて naui のエラーとして返す。
    if !gio::Application::id_is_valid(&settings.app_id) {
        return Err(Error::new(
            "Linux でのアプリ起動",
            format!(
                "アプリ ID `{}` は GTK4 が受け付けない形です。\
                 `Settings::app_id` で逆ドメイン形式 (例: com.example.myapp) を指定してください",
                settings.app_id
            ),
        ));
    }

    let app = adw::Application::builder()
        .application_id(&settings.app_id)
        .build();

    let theme = settings.theme;
    let build = RefCell::new(Some(build));
    let failure: Rc<RefCell<Option<Error>>> = Rc::new(RefCell::new(None));
    {
        let failure = failure.clone();
        app.connect_activate(move |app| {
            let Some(build) = build.borrow_mut().take() else {
                // 2 回目以降の activate (既存プロセスへの起動要求)。
                return;
            };
            apply_theme(theme);
            let ui = Rc::new(Ui::new(app.clone(), theme));
            match build(&ui) {
                Ok(()) => KEEP_ALIVE.with(|slot| slot.borrow_mut().push(ui)),
                Err(error) => {
                    *failure.borrow_mut() = Some(error);
                    app.quit();
                }
            }
        });
    }

    // コマンドライン引数は naui の API に無いので、GTK4 へは渡さない。
    let code = app.run_with_args::<&str>(&[]);
    // メインループが終わった後は、投函しても誰も取り出さない。
    // 送信側へ失敗を返せるようにし、受信クロージャと future を解放する。
    KEEP_ALIVE.with(|slot| {
        let alive = std::mem::take(&mut *slot.borrow_mut());
        for ui in &alive {
            ui.tasks.shutdown();
        }
    });

    if let Some(error) = failure.borrow_mut().take() {
        return Err(error);
    }
    if code != glib::ExitCode::SUCCESS {
        return Err(Error::new(
            "Linux でのアプリ起動",
            format!("GtkApplication が {code:?} で終了しました"),
        ));
    }
    Ok(())
}
