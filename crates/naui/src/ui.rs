use crate::sys;
use crate::{
    Audio, Breadcrumbs, Button, Canvas, Checkbox, Clipboard, ColorPicker, ComboBox, DatePicker,
    Dialog, Dock, EditableComboBox, Expander, FilePicker, FileSaver, Grid, Image, Label, Link,
    List, Menu, MenuBar, Navbar, NumberInput, Pagination, PasswordInput, PopupMenu, ProgressBar,
    RadioGroup, Scroll, SearchInput, Sidebar, Slider, Spacer, SplitView, Stack, Table, Tabs,
    TextArea, TextInput, TimePicker, Toast, Toggle, Toolbar, Tree, Video, Window,
};
use naui_core::{DatePickerMode, Orientation, Result, Settings, Tasks, Theme};

/// ウィジェットを生成するための入り口。
///
/// GTK4 は `GtkApplication` が起動したあとでしかウィンドウを作れないため、
/// `Ui` は [`run`] のコールバックの中でしか得られない。
///
/// ウィジェットのハンドルと同じく **clone しても中身は同じ**なので、
/// コールバックへ持ち込めば後からウィジェットを作れる。メインスレッド
/// 専用で `Send` ではないため、別スレッドへは渡せない (そちらは
/// [`Tasks`] を使う)。
#[derive(Clone)]
pub struct Ui(pub(crate) sys::Ui);

impl_backend!(Ui);

impl Ui {
    /// ウィンドウを作る。フレームワークが参照を保持するので、
    /// 戻り値を捨てても閉じられることはない。
    ///
    /// `width` / `height` は中身の領域の大きさ ([`Window::set_size`] と同じ)。
    /// ヘッダーバーとメニューバーは含まない。
    #[inline]
    pub fn window(&self, title: &str, width: f64, height: f64) -> Result<Window> {
        self.0.window(title, width, height).map(Window)
    }

    #[inline]
    pub fn stack(&self, orientation: Orientation) -> Result<Stack> {
        self.0.stack(orientation).map(Stack)
    }

    /// 行と列で位置を決めるコンテナ。
    #[inline]
    pub fn grid(&self) -> Result<Grid> {
        self.0.grid().map(Grid)
    }

    /// 中身がはみ出したらスクロールさせるコンテナ。
    #[inline]
    pub fn scroll(&self) -> Result<Scroll> {
        self.0.scroll().map(Scroll)
    }

    /// 見出しを押して中身を出し入れするコンテナ。`text` は見出しの文字。
    #[inline]
    pub fn expander(&self, text: &str) -> Result<Expander> {
        self.0.expander(text).map(Expander)
    }

    /// 2 つの区画を、動かせる仕切りで分けるコンテナ。
    ///
    /// `Horizontal` なら区画が横に並び、仕切りは縦になる。
    #[inline]
    pub fn split_view(&self, orientation: Orientation) -> Result<SplitView> {
        self.0.split_view(orientation).map(SplitView)
    }

    /// 余白そのものになるウィジェット。スタックの余りを吸って他を押しやる。
    #[inline]
    pub fn spacer(&self) -> Result<Spacer> {
        self.0.spacer().map(Spacer)
    }

    #[inline]
    pub fn label(&self, text: &str) -> Result<Label> {
        self.0.label(text).map(Label)
    }

    #[inline]
    pub fn button(&self, text: &str) -> Result<Button> {
        self.0.button(text).map(Button)
    }

    #[inline]
    pub fn checkbox(&self, label: &str) -> Result<Checkbox> {
        self.0.checkbox(label).map(Checkbox)
    }

    /// 入り切りを切り替えるスイッチ。`label` はとなりへ添える文字。
    #[inline]
    pub fn toggle(&self, label: &str) -> Result<Toggle> {
        self.0.toggle(label).map(Toggle)
    }

    /// 選択肢を折りたたんで表示するコンボボックス。
    #[inline]
    pub fn combo_box(&self) -> Result<ComboBox> {
        self.0.combo_box().map(ComboBox)
    }

    /// 候補から選ぶことも、自由に打ち込むこともできるコンボボックス。
    #[inline]
    pub fn editable_combo_box(&self) -> Result<EditableComboBox> {
        self.0.editable_combo_box().map(EditableComboBox)
    }

    /// 選択肢を並べて 1 つだけ選ばせるラジオグループ。
    #[inline]
    pub fn radio_group(&self) -> Result<RadioGroup> {
        self.0.radio_group().map(RadioGroup)
    }

    /// 日付や時刻を選ばせるコントロール。何を選ばせるかは `mode` で決める。
    #[inline]
    pub fn date_picker(&self, mode: DatePickerMode) -> Result<DatePicker> {
        self.0.date_picker(mode).map(DatePicker)
    }

    /// 時刻だけを選ばせるコントロール。初期値は現在時刻。
    #[inline]
    pub fn time_picker(&self) -> Result<TimePicker> {
        self.0.time_picker().map(TimePicker)
    }

    /// 色を選ばせるコントロール。初期値は黒。
    #[inline]
    pub fn color_picker(&self) -> Result<ColorPicker> {
        self.0.color_picker().map(ColorPicker)
    }

    /// アプリが自分で描く面。大きさは `set_sizing` で指定する。
    #[inline]
    pub fn canvas(&self) -> Result<Canvas> {
        self.0.canvas().map(Canvas)
    }

    #[inline]
    pub fn text_input(&self, text: &str) -> Result<TextInput> {
        self.0.text_input(text).map(TextInput)
    }

    /// 伏せ字で入力させる欄。中身は `GtkPasswordEntry`。
    #[inline]
    pub fn password_input(&self) -> Result<PasswordInput> {
        self.0.password_input().map(PasswordInput)
    }

    /// 検索の入力欄。中身は `GtkSearchEntry`。
    #[inline]
    pub fn search_input(&self) -> Result<SearchInput> {
        self.0.search_input().map(SearchInput)
    }

    /// 数値を入力させる欄。`value` は初期値。
    #[inline]
    pub fn number_input(&self, value: f64) -> Result<NumberInput> {
        self.0.number_input(value).map(NumberInput)
    }

    /// 改行を含む文字列を入力できる欄。高さは `set_sizing` で指定する。
    #[inline]
    pub fn text_area(&self, text: &str) -> Result<TextArea> {
        self.0.text_area(text).map(TextArea)
    }

    #[inline]
    pub fn slider(&self, min: f64, max: f64) -> Result<Slider> {
        self.0.slider(min, max).map(Slider)
    }

    #[inline]
    pub fn progress_bar(&self) -> Result<ProgressBar> {
        self.0.progress_bar().map(ProgressBar)
    }

    /// 画像。`source` はファイルパスか URL。
    #[inline]
    pub fn image(&self, source: &str) -> Result<Image> {
        self.0.image(source).map(Image)
    }

    /// 動画。`source` はファイルパスか URL。
    #[inline]
    pub fn video(&self, source: &str) -> Result<Video> {
        self.0.video(source).map(Video)
    }

    /// 音声。`source` はファイルパスか URL。
    #[inline]
    pub fn audio(&self, source: &str) -> Result<Audio> {
        self.0.audio(source).map(Audio)
    }

    /// タブ。中身のウィジェットごと持つ。
    #[inline]
    pub fn tabs(&self) -> Result<Tabs> {
        self.0.tabs().map(Tabs)
    }

    /// 画面上部に置く横並びのナビゲーション。`title` は左端の見出し。
    #[inline]
    pub fn navbar(&self, title: &str) -> Result<Navbar> {
        self.0.navbar(title).map(Navbar)
    }

    /// 画面下部に置く横並びのナビゲーション (等幅)。
    #[inline]
    pub fn dock(&self) -> Result<Dock> {
        self.0.dock().map(Dock)
    }

    /// 縦に並ぶナビゲーション一覧。
    #[inline]
    pub fn menu(&self) -> Result<Menu> {
        self.0.menu().map(Menu)
    }

    /// ウィンドウの上端に付けるツールバー。
    ///
    /// [`Window::set_toolbar`] で取り付ける。フレームワークが参照を保持するので、
    /// 戻り値を捨てても通知が届かなくなることはない。
    #[inline]
    pub fn toolbar(&self) -> Result<Toolbar> {
        self.0.toolbar().map(Toolbar)
    }

    /// ウィンドウの上端に付ける、OS のアプリケーションメニュー。
    ///
    /// [`Window::set_menu_bar`] で取り付ける。フレームワークが参照を保持するので、
    /// 戻り値を捨てても通知が届かなくなることはない。
    #[inline]
    pub fn menu_bar(&self) -> Result<MenuBar> {
        self.0.menu_bar().map(MenuBar)
    }

    /// ウィンドウの左に付けるサイドバー。
    ///
    /// [`Window::set_sidebar`] で取り付ける。フレームワークが参照を保持するので、
    /// 戻り値を捨てても通知が届かなくなることはない。
    #[inline]
    pub fn sidebar(&self) -> Result<Sidebar> {
        self.0.sidebar().map(Sidebar)
    }

    /// 選択できる行の一覧。自分でスクロールする。
    #[inline]
    pub fn list(&self) -> Result<List> {
        self.0.list().map(List)
    }

    /// 列見出しを持つ表。自分でスクロールする。
    #[inline]
    pub fn table(&self) -> Result<Table> {
        self.0.table().map(Table)
    }

    /// 入れ子の項目を開閉できる一覧。自分でスクロールする。
    #[inline]
    pub fn tree(&self) -> Result<Tree> {
        self.0.tree().map(Tree)
    }

    /// 右クリックで出るポップアップ (コンテキスト) メニュー。
    ///
    /// フレームワークが参照を保持するので、戻り値を捨てても
    /// 取り付け先から消えることはない。
    #[inline]
    pub fn popup_menu(&self) -> Result<PopupMenu> {
        self.0.popup_menu().map(PopupMenu)
    }

    /// パンくず。
    #[inline]
    pub fn breadcrumbs(&self) -> Result<Breadcrumbs> {
        self.0.breadcrumbs().map(Breadcrumbs)
    }

    /// ページ送り。`page_count` はページ数。
    #[inline]
    pub fn pagination(&self, page_count: usize) -> Result<Pagination> {
        self.0.pagination(page_count).map(Pagination)
    }

    /// リンク。`href` が空でなければ、押したときに既定のハンドラで開く。
    #[inline]
    pub fn link(&self, text: &str, href: &str) -> Result<Link> {
        self.0.link(text, href).map(Link)
    }

    /// ファイルやフォルダーを選ばせるボタン。押すと `GtkFileDialog` が出る。
    #[inline]
    pub fn file_picker(&self, text: &str) -> Result<FilePicker> {
        self.0.file_picker(text).map(FilePicker)
    }

    /// 内容をファイルへ保存させるボタン。押すと `GtkFileDialog` の保存が出る。
    #[inline]
    pub fn file_saver(&self, text: &str) -> Result<FileSaver> {
        self.0.file_saver(text).map(FileSaver)
    }

    /// 一時的な通知 (トースト)。`message` は出す文字列。
    ///
    /// フレームワークが参照を保持するので、戻り値を捨てても
    /// 通知が届かなくなることはない。
    #[inline]
    pub fn toast(&self, message: &str) -> Result<Toast> {
        self.0.toast(message).map(Toast)
    }

    /// モーダルダイアログ。フレームワークが参照を保持する。
    #[inline]
    pub fn dialog(&self, title: &str) -> Result<Dialog> {
        self.0.dialog(title).map(Dialog)
    }

    /// 配色テーマを切り替える。
    #[inline]
    pub fn set_theme(&self, theme: Theme) -> Result<()> {
        self.0.set_theme(theme)
    }

    #[inline]
    pub fn theme(&self) -> Theme {
        self.0.theme()
    }

    /// 別スレッドや非同期処理から画面を書き換えるための入り口。
    ///
    /// 返る [`Tasks`] は clone してコールバックへ持ち込める。
    #[inline]
    pub fn tasks(&self) -> Tasks {
        self.0.tasks()
    }

    /// クリップボード。文字の読み書きができる。
    #[inline]
    pub fn clipboard(&self) -> Clipboard {
        Clipboard(self.0.clipboard())
    }

    /// URL を既定のアプリ (`https:` ならブラウザ、`mailto:` ならメール) で開く。
    ///
    /// 開くのは OS に任せるので、すぐに戻る。渡した文字列を URL として
    /// 読めない・開く先が無いときは `Err`。開いた先で起きた失敗は分からない。
    ///
    /// GTK4 は開く処理を非同期で行う (`GtkUriLauncher`) ので、ここで返るのは
    /// URL として読めたかどうかだけ。
    #[inline]
    pub fn open_url(&self, url: &str) -> Result<()> {
        self.0.open_url(url)
    }

    /// アプリを終了する。
    #[inline]
    pub fn quit(&self) {
        self.0.quit()
    }
}

/// アプリを起動し、`build` の中で UI を組み立てる。
///
/// `build` は `GtkApplication` の `activate` の中で 1 回だけ呼ばれる。
/// この関数はウィンドウがすべて閉じるまで戻らない。
#[inline]
pub fn run<F>(settings: Settings, build: F) -> Result<()>
where
    F: FnOnce(&Ui) -> Result<()> + 'static,
{
    sys::run(settings, move |ui| build(&Ui(ui.clone())))
}
