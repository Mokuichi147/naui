//! # naui
//!
//! **各 OS のネイティブ UI を、1 つの API から扱う軽量 GUI ツールキット。**
//!
//! naui は自前で描画しない。ボタンは実際に OS のボタンであり、
//! 描画・レイアウト・IME・アクセシビリティ・OS のテーマ追従は
//! すべてプラットフォームのツールキットが行う。
//!
//! | ビルド対象 | 使うツールキット | 例: ボタンの実体 |
//! | --- | --- | --- |
//! | Windows | WinUI 3 (Windows App SDK) | `Microsoft.UI.Xaml.Controls.Button` |
//! | macOS | AppKit | `NSButton` |
//! | Linux | GTK4 / libadwaita | `GtkButton` |
//! | Web (wasm) | DOM | `<button>` |
//!
//! ## 使い方
//!
//! UI は `run` に渡すコールバックの中で組み立てる。
//! WinUI 3 が `Application::Start` より前のコントロール生成を許さないため、
//! 4 バックエンドで同じ形にそろえてある。
//!
//! ```no_run
//! use naui::{Orientation, Padding, Settings, Theme};
//!
//! fn main() -> naui::Result<()> {
//!     naui::run(Settings::new("counter").theme(Theme::System), |ui| {
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
//! `Theme::System` が既定で、固定テーマは `Settings::theme(Theme::Dark)` のように
//! 指定する。実行中は `ui.set_theme(Theme::Light)` などで切り替えられる。
//!
//! ネイティブと Web の両方へ出すときは、入口を [`entry!`] に任せる。Web の
//! `#[wasm_bindgen(start)]` はこのマクロが作るので、**アプリ側に `cfg` も
//! wasm-bindgen への依存も要らない**。
//!
//! ```ignore
//! naui::entry!(naui::Settings::new("gallery"), build); // pub fn start() ができる
//! ```
//!
//! ## 配置とサイズ
//!
//! どのウィジェットも [`Sizing`] で大きさを指定できる。並べ方は
//! `Stack` (縦横) と `Grid` (行と列)、はみ出しは `Scroll`、
//! 余りを吸わせたいときは `Spacer` を使う。
//!
//! ```no_run
//! # use naui::{GridCell, Length, Result, ScrollPolicy, Sizing, Track, Ui};
//! # fn build(ui: &Ui) -> Result<()> {
//! let form = ui.grid()?;
//! form.set_spacing(12.0, 8.0);
//! form.set_column_track(0, Track::Fixed(96.0)); // ラベルの列は固定幅
//! form.set_column_track(1, Track::FILL);        // 入力の列は残りいっぱい
//! form.attach(&ui.label("名前")?, GridCell::new(0, 0));
//!
//! let field = ui.text_input("")?;
//! field.set_sizing(Sizing::fill_width());
//! form.attach(&field, GridCell::new(1, 0));
//!
//! let scroll = ui.scroll()?;
//! scroll.set_policy(ScrollPolicy::Never, ScrollPolicy::Auto);
//! scroll.set_child(&form);
//! scroll.set_sizing(Sizing::new().width(Length::Fill).height(Length::Fixed(160.0)));
//! # Ok(())
//! # }
//! ```
//!
//! [`Length::Fill`] は主軸では「余りを受け取る」、交差軸では
//! 「親いっぱいに広がる」を意味する。Windows の `Stack` は StackPanel なので
//! **主軸の `Fill` と `Spacer` が効かない**。その場合は `Grid` の
//! [`Track::Fill`] を使う。
//!
//! `Fill` に上限 ([`Sizing::max_width`] / [`Sizing::max_height`]) を付けると、
//! 上限は「通常時に確保したい大きさ」も兼ねる。**親が大きさを配れるときは
//! 上限まで広がり、足りなければ上限より小さくなる。** 中身の自然な大きさが
//! 当てにならないウィジェット (読み込み前の動画など) の表示欄は、この形で
//! 大きさを決める。
//!
//! ## テキスト入力
//!
//! 1 行なら [`TextInput`]、改行を含む文章なら [`TextArea`] を使う。API の形は
//! 同じで、どちらも IME・コピー / 貼り付け・取り消しはネイティブに任せている。
//!
//! ```no_run
//! # use naui::{Length, Result, Sizing, Ui};
//! # fn build(ui: &Ui) -> Result<()> {
//! let memo = ui.text_area("")?;
//! memo.set_placeholder("複数行のメモ (改行できます)");
//! // スクロールと同じく中身に合わせた高さを持たないので、指定しておく。
//! memo.set_sizing(Sizing::new().width(Length::Fill).height(Length::Fixed(96.0)));
//! memo.on_change(|text| println!("{} 文字", text.chars().count()));
//! # Ok(())
//! # }
//! ```
//!
//! | naui | Windows | macOS | Linux | Web |
//! | --- | --- | --- | --- | --- |
//! | `TextInput` | `TextBox` | `NSTextField` | `GtkEntry` | `<input type="text">` |
//! | `TextArea` | `TextBox` (`AcceptsReturn`) | `NSTextView` + `NSScrollView` | `GtkTextView` + `GtkScrolledWindow` | `<textarea>` |
//!
//! [`TextArea`] は**長い行を折り返し、はみ出した分は縦にスクロール**する。
//! 折り返しの有無を選ぶ設定は、3 環境の共通部分に無いため持たない。
//! macOS の `NSTextView` にはプレースホルダーが無いので、薄い色のラベルを
//! 重ねている (押すと当たり判定は下の `NSTextView` へ通る)。
//!
//! `set_text` では `on_change` は呼ばれない (`TextInput` と同じく、Windows だけは
//! ネイティブの `TextChanged` が出るため呼ばれる)。
//!
//! ## ナビゲーション
//!
//! タブ・ナビバー・ドック・メニュー・パンくず・ページ送り・リンクは、
//! どの環境でも同じ API で組み立てられる。項目は [`NavItem`] の並びで渡し、
//! 選ばれたものはインデックスで返る。
//!
//! ```no_run
//! # use naui::{NavItem, Result, Ui};
//! # fn build(ui: &Ui) -> Result<()> {
//! let navbar = ui.navbar("naui")?;
//! navbar.set_items(&NavItem::list(["ホーム", "検索", "設定"]));
//! navbar.on_select(|index| println!("{index} 番目が選ばれた"));
//! navbar.set_selected(0); // 通知せずに選択を変える
//! # Ok(())
//! # }
//! ```
//!
//! | naui | Windows | macOS | Linux | Web |
//! | --- | --- | --- | --- | --- |
//! | `Tabs` | `Grid` + `ToggleButton` | `NSTabView` | `GtkNotebook` | `role="tablist"` |
//! | `Navbar` | `ToggleButton` の横並び | `NSSegmentedControl` | `GtkLabel` + `GtkToggleButton` の横並び | `<nav>` |
//! | `Dock` | `ToggleButton` の横並び | `NSSegmentedControl` | `GtkToggleButton` の横並び (等幅) | `<nav>` |
//! | `Menu` | `ToggleButton` の縦並び | `NSButton` の縦並び | `GtkToggleButton` の縦並び | `<nav><ul>` |
//! | `Breadcrumbs` | `HyperlinkButton` + 区切り | `NSPathControl` | `GtkToggleButton` + 区切り | `<nav><ol><a>` |
//! | `Pagination` | `Button` + `ToggleButton` | `NSButton` + `NSSegmentedControl` | `GtkButton` + `GtkToggleButton` | `<nav>` |
//! | `Link` | `HyperlinkButton` | `NSButton` (リンク色) | `GtkLinkButton` | `<a>` |
//!
//! `Menu` は**縦に並ぶナビゲーション一覧**であって、ポップアップメニューではない。
//! 右クリックで出るほうは [`PopupMenu`] を使う。
//! `Dock` の下端への固定は、レイアウトが縦横のスタックだけなのでアプリの責務になる。
//!
//! ## ポップアップ (コンテキスト) メニュー
//!
//! [`PopupMenu`] は画面に並ばないので [`Widget`] ではない。項目は
//! [`PopupItem`] の並びで渡し、選ばれたものは**区切り線を含めた並びの**
//! インデックスで返る。
//!
//! ```no_run
//! # use naui::{PopupItem, Result, Ui};
//! # fn build(ui: &Ui) -> Result<()> {
//! let label = ui.label("右クリックしてください")?;
//!
//! let popup = ui.popup_menu()?;
//! popup.set_items(&[
//!     PopupItem::new("コピー"),
//!     PopupItem::separator(),          // 選べないので通知も来ない
//!     PopupItem::new("削除").enabled(false),
//! ]);
//! popup.on_select(|index| println!("{index} 番目が選ばれた"));
//! popup.attach(&label);                // 右クリックで出る
//! popup.open_at(&label, 0.0, 0.0);     // プログラムから出す (左上からの位置)
//! # Ok(())
//! # }
//! ```
//!
//! | naui | Windows | macOS | Linux | Web |
//! | --- | --- | --- | --- | --- |
//! | `PopupMenu` | ルートに重ねる `Grid` + `Button` | `NSMenu` | `GtkPopoverMenu` + `GMenu` | `<div role="menu">` |
//!
//! **階層 (サブメニュー)・チェック印・ショートカットの表示は持たない。**
//! 項目は「文字・選べるかどうか・区切り線」だけで、これは 4 環境が
//! そろって同じ形で扱える範囲にそろえたため。
//!
//! ネイティブのメニューがあるのは macOS (`NSMenu`) と Linux (`GtkPopoverMenu`)
//! で、**Windows と Web は合成**になる。WinUI 3 の `MenuFlyout` は
//! `winio-winui3` のバインディングに無く、ブラウザには既定の
//! コンテキストメニューを差し替える API が無いため。合成のほうは
//! **キーボード操作 (矢印キーでの移動) を持たない**。Web は Escape で
//! 閉じられるが、Windows はキー入力のバインディングが無いため閉じられない
//! (メニューの外側を押せばどちらも閉じる)。
//!
//! `attach` は同じメニューをいくつのウィジェットにも取り付けられる。
//! Web では**取り付けたウィジェットの上でだけ**ブラウザ既定のメニューを
//! 抑止する。`select` はユーザー操作と同じく通知し (テストや自動操作用)、
//! 区切り線と選べない項目は無視する。
//!
//! ## リスト
//!
//! [`List`] は行が縦に並ぶ一覧で、自分でスクロールする。行は [`ListItem`]、
//! 選び方は [`SelectionMode`] で、選択はインデックスの並びで返る。
//! [`ListItem::detail`] を付けると、行に補助の文字が付く。
//!
//! ```no_run
//! # use naui::{Length, ListItem, Result, SelectionMode, Sizing, Ui};
//! # fn build(ui: &Ui) -> Result<()> {
//! let list = ui.list()?;
//! list.set_items(&[
//!     ListItem::new("札幌"),
//!     ListItem::new("東京").detail("13,960,000 人"), // 2 行目に小さく出る
//! ]);
//! list.set_selection_mode(SelectionMode::Multiple); // 既定は Single
//! list.on_select(|indices| println!("{indices:?} が選ばれた"));
//! list.set_selection(&[0, 2]); // 通知せずに選択を置き換える
//!
//! // スクロールと同じく高さを自分では決めないので、指定しておく。
//! list.set_sizing(Sizing::new().width(Length::Fill).height(Length::Fixed(180.0)));
//! # Ok(())
//! # }
//! ```
//!
//! | naui | Windows | macOS | Linux | Web |
//! | --- | --- | --- | --- | --- |
//! | `List` | `ListBox` + `ListBoxItem` | `NSTableView` (1 列) + `NSScrollView` | `GtkListBox` + `GtkScrolledWindow` | `<select size>` / `<ul role="listbox">` |
//!
//! `Menu` との違いは役割で、`Menu` は**画面を切り替えるナビゲーション**、
//! `List` は**データを選ぶ一覧**。`List` だけが複数選択とスクロールを持つ。
//!
//! [`SelectionMode::Multiple`] はどの環境でも「⌘ / Ctrl や Shift を押しながら選ぶ」
//! 形になる (WinUI では `Multiple` ではなく `Extended` に写している)。
//! 複数選択では選択が 0 件になることがあるため、通知は
//! `on_select(|indices: &[usize]|)` の形で、空の並びも渡される。
//! `set_selection` / `set_selected` / `clear_selection` は通知せず、
//! `select` / `select_many` はユーザー操作と同じく通知する。
//!
//! [`ListItem::detail`] は macOS / Windows では 2 行目になる。**Web は行の
//! 中身で作りが変わり**、文字だけなら `<select size>`、`detail` があれば
//! `<ul role="listbox">` の合成になる (`<option>` はテキストしか持てないため)。
//! 行に置けるのは文字だけで、任意のウィジェットや画像のアイコンは置けない。
//!
//! ## ファイルとフォルダーの選択
//!
//! [`FilePicker`] はボタン 1 つで、押すとその環境の標準のファイル選択が開く
//! (macOS は `NSOpenPanel`、Windows は Common Item Dialog、Web はブラウザのもの)。
//!
//! ```no_run
//! # use naui::{FileFilter, FilePickerMode, Result, Ui};
//! # fn build(ui: &Ui) -> Result<()> {
//! let picker = ui.file_picker("画像を選ぶ")?;
//! picker.set_mode(FilePickerMode::File); // File / Files / Folder
//! picker.set_filters(&[FileFilter::new("画像", ["png", "jpg"])]);
//! picker.on_select(|entries| {
//!     for entry in entries {
//!         println!("{}", entry.name());
//!     }
//! });
//! # Ok(())
//! # }
//! ```
//!
//! 選ばれたものは [`FileEntry`] で返る。[`FileEntry::path`] は macOS / Windows では
//! 絶対パスだが、**Web ではブラウザがパスを渡さないため常に `None`** になる。
//! Web ではさらに、`open()` がユーザー操作のイベント内でしか効かないこと、
//! フォルダー選択が中身の一覧として返るのを naui が 1 件へ畳んでいることに注意。
//! 保存ダイアログは、Web に相当が無いため持たない。
//!
//! ## ダイアログ
//!
//! [`Dialog`] は、その環境の標準のモーダルを出す。見出し・本文・任意の
//! ウィジェット・役割つきのボタン (最大 3 つ) を持ち、閉じた理由が
//! [`DialogResponse`] で返る。
//!
//! ```no_run
//! # use naui::{DialogButtons, DialogResponse, Result, Ui};
//! # fn build(ui: &Ui) -> Result<()> {
//! let dialog = ui.dialog("保存しますか")?;
//! dialog.set_message("変更が残っています。");
//! dialog.set_child(&ui.checkbox("次回から確認しない")?); // 任意のウィジェット
//! dialog.set_buttons(
//!     DialogButtons::new()
//!         .primary("保存")
//!         .secondary("保存しない")
//!         .cancel("キャンセル"),
//! );
//! dialog.on_response(|response| match response {
//!     DialogResponse::Primary => println!("保存する"),
//!     DialogResponse::Secondary => println!("保存しない"),
//!     DialogResponse::Cancel => println!("やめる"), // Esc もここへ来る
//! });
//! dialog.open();
//! # Ok(())
//! # }
//! ```
//!
//! | naui | Windows | macOS | Linux | Web |
//! | --- | --- | --- | --- | --- |
//! | `Dialog` | `ContentDialog` | `NSAlert` (+ `accessoryView`) | `AdwAlertDialog` | `<dialog>` + `showModal()` |
//!
//! ボタンの並びは環境の作法に従う (macOS は主となる操作が右端、WinUI 3 は
//! 左端)。ボタンを 1 つも指定しないと「OK」だけが出る。
//!
//! **macOS の [`Dialog::open`] は閉じられるまで戻らない** (`NSAlert` が
//! アプリモーダルなため) 。`on_response` はその中で呼ばれる。Web と Windows の
//! `open()` はすぐ戻り、通知はあとから届く。[`Dialog::close`] で閉じたときは
//! `on_response` を呼ばない (`set_selected` と同じで、アプリ自身の操作は
//! 通知しない)。
//!
//! ## 検証状況
//!
//! | 環境 | 状態 |
//! | --- | --- |
//! | macOS | 実行・自動テストあり (ナビゲーション・リスト・ファイル選択・ポップアップメニュー・複数行入力・ダイアログを含む 59 件) |
//! | Web (wasm) | ブラウザで実行確認 (ナビゲーション、リストの `<select>` と `role="listbox"` の両方、ファイル選択、メディアの表示と再生、ダイアログのボタン経由の応答を確認) |
//! | Windows | Windows App SDK 2.3.1 の実機で基本ウィジェット・ナビゲーション系・レイアウト・ファイル選択・メディアの読み込みと再生を確認 (`Dialog` はコンパイル確認のみ) |
//! | Linux | GTK 4.14 / libadwaita 1.5 (Ubuntu 24.04 / Wayland) で `gallery` の全タブを実行確認。GTK4 の実コントロールに対する自動テスト 58 件。`Video` / `Audio` の再生だけは実ファイルで未確認 |

#![forbid(unsafe_code)]

pub use naui_core::{
    accept_attribute, media, Align, DialogButtons, DialogResponse, Error, FileEntry, FileFilter,
    FilePickerMode, Fit, GridCell, Length, ListItem, NavItem, Orientation, Padding, PlaybackState,
    PopupItem, Result, ScrollPolicy, SelectionMode, Settings, Sizing, Theme, Track,
};

#[cfg(target_arch = "wasm32")]
pub use naui_web::{
    run, Audio, Breadcrumbs, Button, Checkbox, Dialog, Dock, FilePicker, Grid, Image, Label, Link,
    List, Menu, Navbar, Pagination, PopupMenu, ProgressBar, Scroll, Slider, Spacer, Stack, Tabs,
    TextArea, TextInput, Ui, Video, WeakWindow, Widget, Window,
};

#[cfg(all(not(target_arch = "wasm32"), target_os = "macos"))]
pub use naui_macos::{
    run, Audio, Breadcrumbs, Button, Checkbox, Dialog, Dock, FilePicker, Grid, Image, Label, Link,
    List, Menu, Navbar, Pagination, PopupMenu, ProgressBar, Scroll, Slider, Spacer, Stack, Tabs,
    TextArea, TextInput, Ui, Video, WeakWindow, Widget, Window,
};

#[cfg(all(not(target_arch = "wasm32"), target_os = "windows"))]
pub use naui_windows::{
    run, Audio, Breadcrumbs, Button, Checkbox, Dialog, Dock, FilePicker, Grid, Image, Label, Link,
    List, Menu, Navbar, Pagination, PopupMenu, ProgressBar, Scroll, Slider, Spacer, Stack, Tabs,
    TextArea, TextInput, Ui, Video, WeakWindow, Widget, Window,
};

#[cfg(all(
    not(target_arch = "wasm32"),
    unix,
    not(any(target_os = "macos", target_os = "ios", target_os = "android"))
))]
pub use naui_gtk::{
    run, Audio, Breadcrumbs, Button, Checkbox, Dialog, Dock, FilePicker, Grid, Image, Label, Link,
    List, Menu, Navbar, Pagination, PopupMenu, ProgressBar, Scroll, Slider, Spacer, Stack, Tabs,
    TextArea, TextInput, Ui, Video, WeakWindow, Widget, Window,
};

/// `entry!` が使う wasm-bindgen の再公開。直接使うものではない。
#[cfg(target_arch = "wasm32")]
#[doc(hidden)]
pub use naui_web::wasm_bindgen;

/// ネイティブと Web の入口をまとめて作る。
///
/// アプリの起動処理は環境ごとに形が違う。ネイティブは `main` から呼ぶだけだが、
/// Web はブラウザから呼ばれる関数を `#[wasm_bindgen(start)]` で公開する必要が
/// ある。このマクロが両方を作るので、**アプリ側に `cfg` を書かずに済む**
/// (wasm-bindgen への依存も要らない)。
///
/// ```ignore
/// naui::entry!(naui::Settings::new("gallery"), build);
///
/// fn build(ui: &naui::Ui) -> naui::Result<()> { /* ... */ Ok(()) }
/// ```
///
/// 展開されるのは次の 2 つ。
///
/// - `pub fn start() -> Result<()>` — どの環境でもある起動処理。ネイティブの
///   `main` からはこれを呼ぶ。
/// - Web だけ: ブラウザが読み込み後に呼ぶ入口。失敗は JS の例外として投げるので、
///   ブラウザのコンソールに理由が出る。
#[macro_export]
macro_rules! entry {
    ($settings:expr, $build:expr $(,)?) => {
        /// ネイティブ / Web 共通の起動処理。
        pub fn start() -> $crate::Result<()> {
            $crate::run($settings, $build)
        }

        #[cfg(target_arch = "wasm32")]
        #[doc(hidden)]
        mod __naui_entry {
            // wasm-bindgen が展開するコードは `wasm_bindgen::` を参照するので、
            // naui が再公開しているものをその名前で見えるようにする。
            use $crate::wasm_bindgen;

            /// 読み込みが終わったブラウザが呼ぶ入口。
            #[wasm_bindgen::prelude::wasm_bindgen(start)]
            pub fn start() -> ::core::result::Result<(), wasm_bindgen::JsValue> {
                super::start().map_err(|e| wasm_bindgen::JsValue::from_str(&e.to_string()))
            }
        }
    };
}

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
    window.set_theme(Theme::Dark)?;
    let weak_window = window.downgrade();
    let _: Option<Window> = weak_window.upgrade();

    let _: Theme = ui.theme();
    ui.set_theme(Theme::Dark)?;
    ui.set_theme(Theme::System)?;

    let stack: Stack = ui.stack(Orientation::Vertical)?;
    stack.set_spacing(1.0);
    stack.set_padding(Padding::all(1.0));
    stack.set_align(Align::Center);
    stack.set_sizing(Sizing::fill());
    let _: usize = stack.len();
    let _: bool = stack.is_empty();

    // --- レイアウト -------------------------------------------------------
    let grid: Grid = ui.grid()?;
    grid.set_spacing(1.0, 1.0);
    grid.set_padding(Padding::all(1.0));
    grid.set_column_track(0, Track::Fixed(1.0));
    grid.set_row_track(0, Track::FILL);
    grid.set_sizing(Sizing::AUTO);
    let _: usize = grid.columns();
    let _: usize = grid.rows();
    let _: usize = grid.len();
    let _: bool = grid.is_empty();

    let scroll: Scroll = ui.scroll()?;
    scroll.set_policy(ScrollPolicy::Never, ScrollPolicy::Auto);
    scroll.set_sizing(Sizing::new().width(Length::Fill).min_height(1.0));

    let spacer: Spacer = ui.spacer()?;
    spacer.set_sizing(Sizing::fill_height());

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

    let text_area: TextArea = ui.text_area("t")?;
    let _: String = text_area.text();
    text_area.set_text("t\nt");
    text_area.set_placeholder("t");
    text_area.set_enabled(true);
    text_area.on_change(|_s: &str| {});
    text_area.set_sizing(Sizing::new().width(Length::Fill).height(Length::Fixed(1.0)));

    let slider: Slider = ui.slider(0.0, 1.0)?;
    let _: f64 = slider.value();
    slider.set_value(0.5);
    slider.set_enabled(true);
    slider.on_change(|_v: f64| {});

    let progress: ProgressBar = ui.progress_bar()?;
    let _: f64 = progress.value();
    progress.set_value(0.5);

    // --- ナビゲーション ---------------------------------------------------
    let items = [NavItem::new("t"), NavItem::new("t").enabled(false)];

    let tabs: Tabs = ui.tabs()?;
    tabs.add_tab("t", &label);
    let _: usize = tabs.len();
    let _: bool = tabs.is_empty();
    let _: Option<usize> = tabs.selected();
    tabs.set_selected(0);
    tabs.select(0);
    tabs.on_select(|_index: usize| {});

    let navbar: Navbar = ui.navbar("t")?;
    navbar.set_title("t");
    let _: String = navbar.title();
    navbar.set_items(&items);
    let _: usize = navbar.len();
    let _: bool = navbar.is_empty();
    let _: Option<usize> = navbar.selected();
    navbar.set_selected(0);
    navbar.select(0);
    navbar.on_select(|_index: usize| {});

    let dock: Dock = ui.dock()?;
    dock.set_items(&items);
    let _: usize = dock.len();
    let _: bool = dock.is_empty();
    let _: Option<usize> = dock.selected();
    dock.set_selected(0);
    dock.select(0);
    dock.on_select(|_index: usize| {});

    let menu: Menu = ui.menu()?;
    menu.set_items(&items);
    let _: usize = menu.len();
    let _: bool = menu.is_empty();
    let _: Option<usize> = menu.selected();
    menu.set_selected(0);
    menu.select(0);
    menu.on_select(|_index: usize| {});

    // --- リスト -----------------------------------------------------------
    let list: List = ui.list()?;
    list.set_items(&[
        ListItem::new("t"),
        ListItem::new("t").detail("t"),
        ListItem::new("t").enabled(false),
    ]);
    let _: usize = list.len();
    let _: bool = list.is_empty();
    list.set_selection_mode(SelectionMode::Multiple);
    let _: SelectionMode = list.selection_mode();
    let _: Option<usize> = list.selected();
    let _: Vec<usize> = list.selection();
    list.set_selected(0);
    list.set_selection(&[0, 1]);
    list.clear_selection();
    list.select(0);
    list.select_many(&[0, 1]);
    list.on_select(|_indices: &[usize]| {});

    // --- ポップアップ (コンテキスト) メニュー -----------------------------
    let popup: PopupMenu = ui.popup_menu()?;
    popup.set_items(&[
        PopupItem::new("t"),
        PopupItem::separator(),
        PopupItem::new("t").enabled(false),
    ]);
    let _: usize = popup.len();
    let _: bool = popup.is_empty();
    popup.attach(&label);
    popup.select(0);
    popup.close();
    popup.on_select(|_index: usize| {});
    // `open_at` はメニューを実際に出す (macOS では閉じるまで戻らない) ので、
    // 呼ばずに型だけ確かめる。
    let _: fn(&PopupMenu, &dyn Widget, f64, f64) = PopupMenu::open_at;

    let breadcrumbs: Breadcrumbs = ui.breadcrumbs()?;
    breadcrumbs.set_items(&items);
    let _: usize = breadcrumbs.len();
    let _: bool = breadcrumbs.is_empty();
    let _: Option<usize> = breadcrumbs.selected();
    breadcrumbs.set_selected(0);
    breadcrumbs.select(0);
    breadcrumbs.on_select(|_index: usize| {});

    let pagination: Pagination = ui.pagination(3)?;
    pagination.set_page_count(5);
    let _: usize = pagination.page_count();
    let _: usize = pagination.page();
    pagination.set_page(1);
    pagination.select(1);
    pagination.go_previous();
    pagination.go_next();
    pagination.on_change(|_page: usize| {});

    let link: Link = ui.link("t", "https://example.com")?;
    let _: String = link.text();
    link.set_text("t");
    let _: String = link.href();
    link.set_href("t");
    link.set_enabled(true);
    link.on_click(|| {});

    // --- ファイル / フォルダーの選択 ---------------------------------------
    // --- メディア ---------------------------------------------------------
    let image: Image = ui.image("t.png")?;
    let _: String = image.source();
    image.set_source("t.png");
    let _: bool = image.is_loaded();
    image.set_fit(Fit::Cover);
    image.set_alt("t");

    let video: Video = ui.video("t.mp4")?;
    video.set_fit(Fit::Contain);
    let audio: Audio = ui.audio("t.m4a")?;
    // 再生の API は動画と音声で同じ形。両方を同じ順で呼んで確かめる。
    macro_rules! contract_playback {
        ($m:expr) => {{
            let m = $m;
            let _: String = m.source();
            m.set_source("t");
            m.play();
            m.pause();
            let _: PlaybackState = m.state();
            let _: bool = m.is_playing();
            m.seek(1.0);
            let _: f64 = m.position();
            let _: Option<f64> = m.duration();
            m.set_volume(0.5);
            let _: f64 = m.volume();
            m.set_muted(true);
            let _: bool = m.is_muted();
            m.set_loop(true);
            let _: bool = m.is_loop();
            m.set_autoplay(false);
            m.set_controls(true);
            m.on_state_change(|_state: PlaybackState| {});
            m.on_position_change(|_seconds: f64| {});
        }};
    }
    contract_playback!(&video);
    contract_playback!(&audio);

    // --- ダイアログ -------------------------------------------------------
    let dialog: Dialog = ui.dialog("t")?;
    dialog.set_title("t");
    let _: String = dialog.title();
    dialog.set_message("t");
    let _: String = dialog.message();
    dialog.set_child(&ui.label("t")?);
    dialog.set_buttons(DialogButtons::new().primary("t").secondary("t").cancel("t"));
    let _: DialogButtons = dialog.buttons();
    dialog.on_response(|_response: DialogResponse| {});
    let _: bool = dialog.is_open();
    dialog.close();
    // `open()` はダイアログを出すので、契約の確認では呼ばない。

    let picker: FilePicker = ui.file_picker("t")?;
    picker.set_text("t");
    picker.set_enabled(true);
    picker.set_mode(FilePickerMode::Files);
    let _: FilePickerMode = picker.mode();
    picker.set_filters(&[FileFilter::new("t", ["png"])]);
    let _: Vec<FileEntry> = picker.selection();
    picker.on_select(|_entries: &[FileEntry]| {});
    // `open()` はダイアログを出すので、契約の確認では呼ばない。

    grid.attach(&label, GridCell::new(0, 0));
    grid.attach(&button, GridCell::new(1, 0).span(2, 1));
    scroll.set_child(&grid);

    stack.append(&spacer);
    stack.append(&scroll);
    stack.append(&image);
    stack.append(&video);
    stack.append(&audio);
    stack.append(&label);
    stack.append(&button);
    stack.append(&checkbox);
    stack.append(&input);
    stack.append(&text_area);
    stack.append(&slider);
    stack.append(&progress);
    stack.append(&tabs);
    stack.append(&navbar);
    stack.append(&dock);
    stack.append(&menu);
    stack.append(&list);
    stack.append(&breadcrumbs);
    stack.append(&pagination);
    stack.append(&link);
    stack.append(&picker);
    window.set_child(&stack);

    ui.quit();
    Ok(())
}
