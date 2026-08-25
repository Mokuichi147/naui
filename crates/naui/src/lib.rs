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
//! 余りを吸わせたいときは `Spacer`、ふだんは隠しておきたいものは
//! `Expander` を使う。
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
//! ## 折りたたみ
//!
//! ふだんは隠しておき、見出しを押したときだけ見せたいものは [`Expander`] へ
//! 入れる。中身は 1 つなので、複数並べたいときは `Stack` などのコンテナごと
//! 入れる。
//!
//! ```no_run
//! # use naui::{Orientation, Result, Ui};
//! # fn build(ui: &Ui) -> Result<()> {
//! let details = ui.expander("詳細設定")?;
//! let body = ui.stack(Orientation::Vertical)?;
//! body.append(&ui.checkbox("バックアップを作る")?);
//! details.set_child(&body);
//! details.set_expanded(true); // 通知せずに開く
//! details.on_toggle(|expanded| println!("開いている: {expanded}"));
//! # Ok(())
//! # }
//! ```
//!
//! | naui | Windows | macOS | Linux | Web |
//! | --- | --- | --- | --- | --- |
//! | `Expander` | `Expander` | `NSButton` (入り切り) + `NSStackView` | `GtkExpander` | `<details>` + `<summary>` |
//!
//! Windows は WinUI 3 のネイティブ `Expander` を使う。macOS は AppKit に
//! 単一の Expander がないため、標準の `NSButton` と `NSStackView` を組み合わせる。
//!
//! たたんでいる間、中身はレイアウトから外れる (場所を空けない)。既定は
//! 閉じた状態で、[`set_expanded`](Expander::set_expanded) はプログラムからの
//! 操作なので `on_toggle` を呼ばない ([`Checkbox::set_checked`] と同じ決まり)。
//!
//! ## テキスト入力
//!
//! 1 行なら [`TextInput`]、改行を含む文章なら [`TextArea`]、伏せ字にするなら
//! [`PasswordInput`] を使う。API の形はどれも同じで、IME・コピー / 貼り付け・
//! 取り消しはネイティブに任せている。
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
//! | `PasswordInput` | `PasswordBox` | `NSSecureTextField` | `GtkPasswordEntry` | `<input type="password">` |
//!
//! [`TextArea`] は**長い行を折り返し、はみ出した分は縦にスクロール**する。
//! 折り返しの有無を選ぶ設定は、3 環境の共通部分に無いため持たない。
//! macOS の `NSTextView` にはプレースホルダーが無いので、薄い色のラベルを
//! 重ねている (押すと当たり判定は下の `NSTextView` へ通る)。
//!
//! `set_text` では `on_change` は呼ばれない (`TextInput` と同じく、Windows だけは
//! ネイティブの `TextChanged` が出るため呼ばれる)。
//!
//! [`PasswordInput`] は打った文字を伏せ字にするだけで、API は [`TextInput`] と
//! 同じ。**伏せ字を一時的に外すボタンは持たない** (`NSSecureTextField` に
//! 無いため)。入力された文字列は `text()` で読めるので、扱いはアプリの責任。
//!
//! ## 数値入力
//!
//! 数を入れさせるときは [`NumberInput`] を使う。値は `f64` で、範囲・刻み・
//! 小数桁は [`NumberSpec`] が決める。**既定は整数** (刻み 1、小数桁 0、
//! 範囲の制限なし) なので、小数を扱うなら両方を指定する。
//!
//! ```no_run
//! # use naui::{Result, Ui};
//! # fn build(ui: &Ui) -> Result<()> {
//! let count = ui.number_input(1.0)?;
//! count.set_range(Some(1.0), Some(99.0)); // 1..=99 の外へは出られない
//! count.on_change(|value| println!("{value} 個"));
//!
//! let rate = ui.number_input(0.5)?;
//! rate.set_decimals(2); // 0.50 のように 2 桁で見せる
//! rate.set_step(0.05); // 上下のボタンで動く量
//! # Ok(())
//! # }
//! ```
//!
//! | naui | Windows | macOS | Linux | Web |
//! | --- | --- | --- | --- | --- |
//! | `NumberInput` | `TextBox` + 増減ボタン | `NSTextField` + `NSStepper` | `GtkSpinButton` | `<input type="number">` |
//!
//! 値は**小数桁へ丸めてから範囲へ収める**。範囲の外の値を
//! [`set_value`](NumberInput::set_value) へ渡すと、通知せずに端へ寄る。
//! 打っている最中は表示を書き換えず、読めた時点で `on_change` が呼ばれる。
//! 中身に合わせた幅を持たないので (`TextInput` と同じ)、幅は
//! [`set_sizing`](NumberInput::set_sizing) で指定する。
//! 表示を値へそろえ直すのは**確定したとき** (Enter・欄を離れたとき・増減の
//! ボタンを押したとき) で、数として読めない文字列は確定時に元の値へ戻る。
//!
//! ## 入り切りの切り替え
//!
//! 入っているか切れているかの 2 択は [`Checkbox`] か [`Toggle`] で切り替える。
//! API はどちらも同じで、違うのは見せ方だけ。**印を付けるチェックボックスは
//! 「同意する」のようにまとめて決めるもの、つまみを動かすスイッチはその場で
//! 効く設定**に向く (どちらを使うかは、その環境の作法に合わせる)。
//!
//! ```no_run
//! # use naui::{Result, Ui};
//! # fn build(ui: &Ui) -> Result<()> {
//! let backup = ui.toggle("バックアップを作る")?;
//! backup.set_on(true); // 通知せずに入れる
//! backup.on_toggle(|on| println!("バックアップ: {on}"));
//! # Ok(())
//! # }
//! ```
//!
//! | naui | Windows | macOS | Linux | Web |
//! | --- | --- | --- | --- | --- |
//! | `Checkbox` | `CheckBox` | `NSButton` (チェック型) | `GtkCheckButton` | `<input type="checkbox">` |
//! | `Toggle` | `ToggleSwitch` | `NSSwitch` + `NSTextField` | `GtkSwitch` + `GtkLabel` | `<input type="checkbox" switch>` |
//!
//! [`set_checked`](Checkbox::set_checked) と [`set_on`](Toggle::set_on) は
//! プログラムからの操作なので `on_toggle` を呼ばない。
//!
//! `NSSwitch` と `GtkSwitch` は文字を持たないので、macOS と Linux では
//! ラベルを横へ添えている。Windows は `OnContent` / `OffContent` へ同じ文字を
//! 入れて、入り切りで読みが変わらないようにしている。**切り替えの当たり判定は
//! スイッチの部分**で、Web だけは `<label>` で組む都合上、文字を押しても
//! 切り替わる。
//!
//! Web は `switch` 属性でブラウザへ「スイッチとして描いて」と頼むだけで、
//! naui が見た目を作ることはしない。**この属性に対応しているのは Safari 17.4
//! 以降だけ**なので、Chrome や Firefox ではチェックボックスの見た目で出る
//! (値の扱い・通知・読み上げ (`role="switch"`) は同じ)。
//!
//! ## 選択入力
//!
//! 1 つの候補を省スペースに選ばせるときは [`ComboBox`] を使う。候補は
//! ドロップダウンで表示され、選択されたものはインデックスで返る。
//!
//! ```no_run
//! # use naui::{Result, Ui};
//! # fn build(ui: &Ui) -> Result<()> {
//! let language = ui.combo_box()?;
//! language.set_items(&["Rust", "Swift", "TypeScript"]);
//! language.set_selected(0); // 通知せずに初期値を選ぶ
//! language.on_select(|index| println!("{index} 番目が選ばれた"));
//! # Ok(())
//! # }
//! ```
//!
//! 候補をすべて画面に出して選ばせるなら [`RadioGroup`] を使う。API は
//! `ComboBox` と同じで、違うのは候補の見せ方だけ。
//!
//! ```no_run
//! # use naui::{Orientation, Result, Ui};
//! # fn build(ui: &Ui) -> Result<()> {
//! let plan = ui.radio_group()?;
//! plan.set_items(&["無料", "標準", "上位"]);
//! plan.set_orientation(Orientation::Horizontal); // 既定は縦
//! plan.set_selected(0);
//! plan.on_select(|index| println!("{index} 番目が選ばれた"));
//! # Ok(())
//! # }
//! ```
//!
//! | naui | Windows | macOS | Linux | Web |
//! | --- | --- | --- | --- | --- |
//! | `ComboBox` | `ComboBox` | `NSPopUpButton` | `GtkDropDown` | `<select>` |
//! | `RadioGroup` | `RadioButton` の組 | `NSButton` のラジオ型 | 組にした `GtkCheckButton` | `<input type="radio">` |
//!
//! どちらも [`set_items`](ComboBox::set_items) は候補のインデックスの意味を
//! 変えるため、選択を外す。`set_selected` / `clear_selection` は通知せず、
//! `select` は利用者の操作と同じく `on_select` を呼ぶ。`ComboBox` は自由入力の
//! できる編集可能コンボボックスではない。`RadioGroup` は 1 つのグループなので、
//! 排他になるのはその中だけ。
//!
//! ## 日付と時刻
//!
//! 日付や時刻を選ばせるときは [`DatePicker`] を使う。何を選ばせるかは
//! 生成時の [`DatePickerMode`] で決め、値は [`DateTime`] (年月日と時分) で
//! やり取りする。
//!
//! ```no_run
//! # use naui::{DatePickerMode, DateTime, Result, Ui};
//! # fn build(ui: &Ui) -> Result<()> {
//! let start = ui.date_picker(DatePickerMode::Date)?;
//! start.set_value(DateTime::date(2026, 8, 22)); // 通知せずに値を入れる
//! start.set_range(Some(DateTime::date(2026, 1, 1)), None); // 下限だけ決める
//! start.on_change(|value| println!("{value} が選ばれた"));
//!
//! let alarm = ui.date_picker(DatePickerMode::Time)?;
//! alarm.set_value(DateTime::time(7, 30));
//! # Ok(())
//! # }
//! ```
//!
//! | naui | Windows | macOS | Linux | Web |
//! | --- | --- | --- | --- | --- |
//! | `DatePicker` | `ComboBox` の組 | `NSDatePicker` | `GtkCalendar` + `GtkSpinButton` | `<input type="date">` ほか |
//!
//! 作った直後の値は**その環境の現在日時 (ローカル時刻)** で、空の状態は
//! 持たない (`NSDatePicker` に「未選択」が無いため)。秒も持たない
//! ([`DateTime`] 参照)。
//!
//! [`DatePickerMode::Date`] は時刻を、[`DatePickerMode::Time`] は日付を
//! **選ばせないだけで、捨てはしない**。`set_value` で入れた側の値は
//! [`DatePicker::value`] にそのまま残る。
//!
//! [`set_value`](DatePicker::set_value) は通知せず、暦として成り立たない値
//! (11 月 31 日など) は丸める。[`set_range`](DatePicker::set_range) の外へは
//! 出られず、範囲の比較には**選ばせている部分だけ**を使う。
//!
//! ## 色の選択
//!
//! 色を選ばせるときは [`ColorPicker`] を使う。値は [`Color`] (sRGB の 8 bit)
//! でやり取りし、色を選ぶ UI はその環境のものがそのまま開く。
//!
//! ```no_run
//! # use naui::{Color, Result, Ui};
//! # fn build(ui: &Ui) -> Result<()> {
//! let accent = ui.color_picker()?;
//! accent.set_value(Color::rgb(0x33, 0x66, 0xff)); // 通知せずに値を入れる
//! accent.on_change(|value| println!("{value} が選ばれた")); // #3366ff
//! # Ok(())
//! # }
//! ```
//!
//! | naui | Windows | macOS | Linux | Web |
//! | --- | --- | --- | --- | --- |
//! | `ColorPicker` | `Button` + `Flyout` + `ColorPicker` | `NSColorWell` | `GtkColorDialogButton` | `<input type="color">` |
//!
//! 作った直後の値は黒 ([`Color::BLACK`])。**透明度は扱わない** —
//! `<input type="color">` が不透明な色しか返さないため、4 環境でそろう
//! 範囲に合わせている。
//!
//! [`set_value`](ColorPicker::set_value) は通知せず、
//! [`pick`](ColorPicker::pick) は利用者が選んだのと同じく `on_change` を
//! 呼ぶ ([`ComboBox::set_selected`] と [`ComboBox::select`] と同じ決まり)。
//!
//! Windows の WinUI 3 `ColorPicker` は、スペクトラムとスライダーを縦に
//! 並べた**大きな面**で、他の 3 環境の「色の見本を押すと選択の UI が開く」
//! という形と並び方が違う。そこで WinUI 3 の作法どおり `Button` の
//! `Flyout` へ入れ、ボタンには選んだ色の見本を出している。
//!
//! macOS のカラーパネルはカタログ色 (`systemBlue` など) も返すので、
//! 成分を読む前に sRGB へ変換している。
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
//! ## ツールバー
//!
//! よく使う操作をウィンドウの上端に並べるときは [`Toolbar`] を使う。
//! ほかのウィジェットと違い**レイアウトへは置かない**。macOS の
//! `NSToolbar` が `NSWindow` に取り付けるものだからで、naui でも
//! [`Window::set_toolbar`] で取り付ける ([`Widget`] ではない)。
//!
//! ナビゲーションと違い**選ばれている項目を持たず**、押されるたびに
//! その場でコマンドが走る。項目は [`ToolbarItem`] の並びで渡し、
//! 押されたものは**区切りを含めた並びの**インデックスで返る。
//!
//! 項目は**アイコン**で並ぶ。アイコンの呼び名は環境ごとに違うため
//! ([`ToolbarIcon`] 参照)、naui は操作の種類だけを受け取り、その環境の
//! 標準アイコンへ写す。`label` は読み上げ・ツールチップ・項目が入りきらない
//! ときのメニューに使う。
//!
//! ```no_run
//! # use naui::{Result, ToolbarIcon, ToolbarItem, Ui};
//! # fn build(ui: &Ui) -> Result<()> {
//! let window = ui.window("編集", 800.0, 600.0)?;
//!
//! let toolbar = ui.toolbar()?;
//! toolbar.set_items(&[
//!     ToolbarItem::new(ToolbarIcon::New, "新規"),
//!     ToolbarItem::new(ToolbarIcon::Open, "開く"),
//!     ToolbarItem::separator(),        // 押せないので通知も来ない
//!     ToolbarItem::new(ToolbarIcon::Save, "保存").enabled(false),
//! ]);
//! toolbar.on_activate(|index| println!("{index} 番目が押された"));
//! toolbar.set_item_enabled(3, true);   // 保存できる状態になったら有効にする
//! window.set_toolbar(&toolbar);
//! # Ok(())
//! # }
//! ```
//!
//! | naui | Windows | macOS | Linux | Web |
//! | --- | --- | --- | --- | --- |
//! | `Toolbar` | `StackPanel` + `Button` | `NSToolbar` + `NSToolbarItem` | `AdwHeaderBar` + `GtkButton` | `<div role="toolbar">` + `<button>` |
//! | アイコン | Segoe Fluent Icons | SF Symbols | アイコンテーマ | naui 同梱の SVG |
//!
//! macOS と Linux では**ウィンドウのタイトルバーと一体**で表示される。
//! Windows の `CommandBar` と Web の対応する概念は無いため、
//! タイトルバーの下 (Web ではウィンドウ要素の先頭) に置く。
//!
//! macOS では、ツールバーを付けると**タイトル文字が隠れる**。ツールバーの
//! あるウィンドウでタイトルを出さないのが macOS の作法で、出したままだと
//! タイトルが先頭を占めて項目が右端へ押しやられてしまうため。
//! [`Window::set_title`] の値はウィンドウのタイトルとして残り
//! (ウィンドウメニューや Mission Control には出る)、[`Window::title`] も
//! 返し続ける。[`Window::clear_toolbar`] で外すと表示も戻る。
//!
//! [`Toolbar::len`] は区切りを含めた項目数で、[`Toolbar::activate`] は
//! 利用者が押したのと同じように通知する (区切り・押せない項目・範囲外は
//! 何もしない)。[`Toolbar::set_enabled`] はツールバー全体をまとめて
//! 無効にするもので、項目ごとの指定は残る。
//!
//! ブラウザには OS のアイコンテーマが無いため、Web だけは naui が図形を持つ。
//! 用意しているのは [`ToolbarIcon`] に並ぶ操作だけで、任意の画像は置けない。
//! また naui は項目をインデックスで識別するため、
//! macOS の「ツールバーをカスタマイズ」(利用者による並べ替え) は切ってある。
//! 並べ替えられると通知のインデックスの意味が変わってしまうため。
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
//! 入れ子の項目を開閉して選ぶなら [`Tree`] を使う。
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
//! ## ツリー
//!
//! [`Tree`] は入れ子の項目を開閉できる一覧で、自分でスクロールする。
//! 項目は [`TreeItem`] で、**根からの子インデックスの並び (パス)** で指す。
//! `[0, 2]` は「1 番目の根の 3 番目の子」、空のパスは「選択なし」を表す。
//!
//! ```no_run
//! # use naui::{Length, Result, Sizing, TreeItem, Ui};
//! # fn build(ui: &Ui) -> Result<()> {
//! let tree = ui.tree()?;
//! tree.set_items(&[
//!     TreeItem::new("src")
//!         .expanded(true) // 最初から開いた状態で出す
//!         .children([TreeItem::new("main.rs"), TreeItem::new("lib.rs")]),
//!     TreeItem::new("docs").child(TreeItem::new("guide.md").detail("12 KB")),
//! ]);
//! tree.on_select(|path| println!("{path:?} が選ばれた"));
//! tree.on_expand(|path, expanded| println!("{path:?} は {expanded}"));
//! tree.set_selected(&[0, 1]); // 通知せずに選ぶ (祖先は開かれる)
//!
//! // リストと同じく高さを自分では決めないので、指定しておく。
//! tree.set_sizing(Sizing::new().width(Length::Fill).height(Length::Fixed(220.0)));
//! # Ok(())
//! # }
//! ```
//!
//! | naui | Windows | macOS | Linux | Web |
//! | --- | --- | --- | --- | --- |
//! | `Tree` | `ListBox` + 開閉ボタン | `NSOutlineView` + `NSScrollView` | `GtkListBox` + 開閉ボタン | `<ul role="tree">` |
//!
//! 選べるのは 1 項目だけ (`List` のような複数選択は無い)。
//! `set_selected` / `clear_selection` / `set_expanded` / `expand_all` /
//! `collapse_all` は通知せず、`select` / `expand` / `collapse` は
//! ユーザー操作と同じく通知する。
//!
//! [`TreeItem::enabled`] を `false` にすると、**その子孫もまとめて選べなくなる**。
//! 開閉は項目ごとに覚えられるので、親を閉じてから開き直すと、中の開閉も
//! 元どおりに出てくる (macOS の Finder と同じ)。
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
//!
//! ## ファイルの保存
//!
//! [`FileSaver`] は対になるボタンで、押すとその環境の標準の保存ダイアログが
//! 開く。**保存先のパスを返すのではなく、渡しておいた内容を書き出す**形にして
//! ある。ブラウザに「保存先のパス」という概念が無く、パスを返す API では Web で
//! 何もできないため。
//!
//! ```no_run
//! # use naui::{FileFilter, Result, Ui};
//! # fn build(ui: &Ui) -> Result<()> {
//! let saver = ui.file_saver("保存")?;
//! saver.set_file_name("メモ"); // 拡張子は絞り込みから補われる
//! saver.set_filters(&[FileFilter::new("テキスト", ["txt"])]);
//! saver.set_contents("こんにちは".as_bytes());
//! saver.on_save(|entry| println!("{} へ保存しました", entry.name()));
//! saver.on_error(|error| eprintln!("{error}"));
//! # Ok(())
//! # }
//! ```
//!
//! | naui | Windows | macOS | Linux | Web |
//! | --- | --- | --- | --- | --- |
//! | `FileSaver` | `IFileSaveDialog` | `NSSavePanel` | `GtkFileDialog` (save) | `showSaveFilePicker` / `<a download>` |
//!
//! [`FileSaver::set_contents`] のバイト列が、ユーザーの選んだ場所へそのまま
//! 書かれる。成功すると [`FileSaver::on_save`] に書き出し先が届き、取り消した
//! ときは何も呼ばれない。書き込みに失敗したときだけ [`FileSaver::on_error`] が
//! 呼ばれる。
//!
//! Web はここでも作りが変わる。`showSaveFilePicker` (Chromium 系) があれば OS の
//! 保存ダイアログが出るが、無いブラウザ (Firefox / Safari) では
//! `<a download>` のダウンロードになり、**保存先はブラウザ任せで、確認なしに
//! ダウンロードフォルダーへ落ちることもある**。どちらの場合も
//! [`FileEntry::path`] は `None` で、名前だけが返る。
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
//! ## トースト
//!
//! [`Toast`] は、画面の下端へ短い知らせを出して**自分で消える**通知。
//! ダイアログと違い操作を止めないので、「保存しました」のような
//! 済んだことの知らせに使う。
//!
//! ```no_run
//! # use naui::{Result, Ui};
//! # fn build(ui: &Ui) -> Result<()> {
//! let toast = ui.toast("保存しました")?;
//! toast.set_action("元に戻す"); // 任意。空文字列で外す
//! toast.set_timeout(3.0); // 秒。0 なら自分では消えない
//! toast.on_action(|| println!("元に戻す"));
//! toast.show();
//! # Ok(())
//! # }
//! ```
//!
//! | naui | Windows | macOS | Linux | Web |
//! | --- | --- | --- | --- | --- |
//! | `Toast` | `Grid` を中身へ重ねたもの | `NSVisualEffectView` を中身へ重ねたもの | `AdwToast` + `AdwToastOverlay` | `<div role="status">` |
//!
//! **ネイティブのトーストがあるのは Linux だけ** (`AdwToast`)。Windows の
//! `InfoBar` / `TeachingTip` は `winio-winui3` のバインディングに無く、
//! macOS とブラウザにはそもそも対応するものが無いので、残る 3 環境では
//! naui が同じ形に組み立てる。OS の通知センターへ出すものは、アプリの外へ
//! 出る別の仕組みなので扱わない。
//!
//! **同時に出るのは 1 つ**で、新しく出したものが前のものを置き換える
//! (`AdwToastOverlay` の順番待ちも、naui はこの形にそろえている)。
//! [`Toast::dismiss`] で消したときと、置き換えられたときは `on_dismiss` を
//! 呼ばない (アプリ自身の操作は通知しない、という [`Dialog::close`] と
//! 同じ決まり)。時間の刻みは Linux だけ**秒**なので、1 秒未満の指定は
//! 1 秒になる。
//!
//! ## 検証状況
//!
//! | 環境 | 状態 |
//! | --- | --- |
//! | macOS | 実行・自動テストあり (コンボボックス・ラジオグループ・日付ピッカー・数値入力・パスワード入力・ナビゲーション・リスト・ツリー・ツールバー・ファイル選択・ポップアップメニュー・複数行入力・ダイアログ・トースト・折りたたみ・スイッチ・色ピッカーを含む 100 件) |
//! | Web (wasm) | ブラウザで実行確認 (ナビゲーション、リストの `<select>` と `role="listbox"` の両方、数値入力の丸め・範囲・確定、パスワード入力、ファイル選択、メディアの表示と再生、ダイアログのボタン経由の応答、トーストの表示・操作ボタン・時間切れ・置き換え、折りたたみの開閉と通知、色ピッカーの値の往復と通知を確認。スイッチは切り替えと通知をブラウザで確認 (見た目は Chromium 148 で `switch` 属性が未対応のためチェックボックス)) |
//! | Windows | Windows App SDK 2.3.1 の実機で全ウィジェットとナビゲーションを操作して確認 (トースト・折りたたみ・スイッチを含む)。`ColorPicker` だけは自前の WinRT 投影で、実機未確認 |
//! | Linux | GTK 4.14 / libadwaita 1.5 (Ubuntu 24.04 / Wayland) で `gallery` の全タブ (トースト・折りたたみ・スイッチを含む) を実行確認。GTK4 の実コントロールに対する自動テスト 98 件 (スイッチ・色ピッカーを含む)。メディアは実ファイル (H.264 + AAC) の再生・シーク・状態変化まで確認 |

#![forbid(unsafe_code)]

pub use naui_core::{
    accept_attribute, days_in_month, default_extension, is_leap_year, media,
    with_default_extension, Align, Color, DatePickerMode, DateTime, DialogButtons, DialogResponse,
    Error, FileEntry, FileFilter, FilePickerMode, Fit, GridCell, Length, ListItem, NavItem,
    NumberSpec, Orientation, Padding, PlaybackState, PopupItem, Result, ScrollPolicy,
    SelectionMode, Settings, Sizing, Theme, ToastSpec, ToolbarIcon, ToolbarItem, Track, TreeItem,
};

#[cfg(all(not(target_arch = "wasm32"), target_os = "macos"))]
pub use naui_macos::{
    run, Audio, Breadcrumbs, Button, Checkbox, ColorPicker, ComboBox, DatePicker, Dialog, Dock,
    Expander, FilePicker, FileSaver, Grid, Image, Label, Link, List, Menu, Navbar, NumberInput,
    Pagination, PasswordInput, PopupMenu, ProgressBar, RadioGroup, Scroll, Slider, Spacer, Stack,
    Tabs, TextArea, TextInput, Toast, Toggle, Toolbar, Tree, Ui, Video, WeakWindow, Widget, Window,
};
#[cfg(target_arch = "wasm32")]
pub use naui_web::{
    run, Audio, Breadcrumbs, Button, Checkbox, ColorPicker, ComboBox, DatePicker, Dialog, Dock,
    Expander, FilePicker, FileSaver, Grid, Image, Label, Link, List, Menu, Navbar, NumberInput,
    Pagination, PasswordInput, PopupMenu, ProgressBar, RadioGroup, Scroll, Slider, Spacer, Stack,
    Tabs, TextArea, TextInput, Toast, Toggle, Toolbar, Tree, Ui, Video, WeakWindow, Widget, Window,
};
#[cfg(all(not(target_arch = "wasm32"), target_os = "windows"))]
pub use naui_windows::{
    run, Audio, Breadcrumbs, Button, Checkbox, ColorPicker, ComboBox, DatePicker, Dialog, Dock,
    Expander, FilePicker, FileSaver, Grid, Image, Label, Link, List, Menu, Navbar, NumberInput,
    Pagination, PasswordInput, PopupMenu, ProgressBar, RadioGroup, Scroll, Slider, Spacer, Stack,
    Tabs, TextArea, TextInput, Toast, Toggle, Toolbar, Tree, Ui, Video, WeakWindow, Widget, Window,
};

#[cfg(all(
    not(target_arch = "wasm32"),
    unix,
    not(any(target_os = "macos", target_os = "ios", target_os = "android"))
))]
pub use naui_gtk::{
    run, Audio, Breadcrumbs, Button, Checkbox, ColorPicker, ComboBox, DatePicker, Dialog, Dock,
    Expander, FilePicker, FileSaver, Grid, Image, Label, Link, List, Menu, Navbar, NumberInput,
    Pagination, PasswordInput, PopupMenu, ProgressBar, RadioGroup, Scroll, Slider, Spacer, Stack,
    Tabs, TextArea, TextInput, Toast, Toggle, Toolbar, Tree, Ui, Video, WeakWindow, Widget, Window,
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

    let expander: Expander = ui.expander("t")?;
    let _: String = expander.text();
    expander.set_text("t");
    let _: bool = expander.is_expanded();
    expander.set_expanded(true);
    expander.set_enabled(true);
    expander.on_toggle(|_expanded: bool| {});
    expander.set_sizing(Sizing::fill_width());

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

    let toggle: Toggle = ui.toggle("t")?;
    let _: bool = toggle.is_on();
    toggle.set_on(true);
    toggle.set_enabled(true);
    toggle.on_toggle(|_v: bool| {});
    toggle.set_sizing(Sizing::fill_width());

    let color_picker: ColorPicker = ui.color_picker()?;
    let _: Color = color_picker.value();
    color_picker.set_value(Color::rgb(1, 2, 3));
    color_picker.pick(Color::WHITE);
    color_picker.set_enabled(true);
    color_picker.on_change(|_value: Color| {});
    color_picker.set_sizing(Sizing::fill_width());

    let combo_box: ComboBox = ui.combo_box()?;
    combo_box.set_items(&["a", "b"]);
    let _: usize = combo_box.len();
    let _: bool = combo_box.is_empty();
    let _: Option<usize> = combo_box.selected();
    combo_box.set_selected(0);
    combo_box.clear_selection();
    combo_box.select(1);
    combo_box.set_enabled(true);
    combo_box.on_select(|_index: usize| {});
    combo_box.set_sizing(Sizing::fill_width());

    let radio_group: RadioGroup = ui.radio_group()?;
    radio_group.set_items(&["a", "b"]);
    let _: usize = radio_group.len();
    let _: bool = radio_group.is_empty();
    let _: Option<usize> = radio_group.selected();
    radio_group.set_selected(0);
    radio_group.clear_selection();
    radio_group.select(1);
    radio_group.set_orientation(Orientation::Horizontal);
    radio_group.set_enabled(true);
    radio_group.on_select(|_index: usize| {});
    radio_group.set_sizing(Sizing::fill_width());

    let date_picker: DatePicker = ui.date_picker(DatePickerMode::Date)?;
    let _: DatePickerMode = date_picker.mode();
    let _: DateTime = date_picker.value();
    date_picker.set_value(DateTime::date(2026, 8, 22));
    date_picker.set_range(Some(DateTime::date(2026, 1, 1)), None);
    date_picker.set_range(None, None);
    date_picker.set_enabled(true);
    date_picker.on_change(|_value: DateTime| {});
    date_picker.set_sizing(Sizing::fill_width());
    let _: DatePicker = ui.date_picker(DatePickerMode::Time)?;
    let _: DatePicker = ui.date_picker(DatePickerMode::DateTime)?;

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

    let password: PasswordInput = ui.password_input()?;
    let _: String = password.text();
    password.set_text("t");
    password.set_placeholder("t");
    password.set_enabled(true);
    password.on_change(|_s: &str| {});
    password.set_sizing(Sizing::fill_width());

    let number: NumberInput = ui.number_input(1.0)?;
    let _: f64 = number.value();
    let _: NumberSpec = number.spec();
    number.set_value(2.0);
    number.set_range(Some(0.0), Some(10.0));
    number.set_range(None, None);
    number.set_step(0.5);
    number.set_decimals(2);
    number.set_enabled(true);
    number.on_change(|_v: f64| {});
    number.set_sizing(Sizing::fill_width());

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

    // --- ツールバー (ウィンドウに取り付ける。Widget ではない) -------------
    let toolbar: Toolbar = ui.toolbar()?;
    toolbar.set_items(&[
        ToolbarItem::new(ToolbarIcon::New, "t"),
        ToolbarItem::separator(),
        ToolbarItem::new(ToolbarIcon::Save, "t").enabled(false),
    ]);
    let _: &str = ToolbarIcon::Open.sf_symbol();
    let _: &str = ToolbarIcon::Open.icon_name();
    let _: char = ToolbarIcon::Open.fluent_glyph();
    let _: &str = ToolbarIcon::Open.svg_path();
    let _: usize = toolbar.len();
    let _: bool = toolbar.is_empty();
    let _: bool = toolbar.is_item_enabled(0);
    toolbar.set_item_enabled(2, true);
    toolbar.set_enabled(true);
    toolbar.activate(0);
    toolbar.on_activate(|_index: usize| {});
    window.set_toolbar(&toolbar);
    window.clear_toolbar();

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

    // --- ツリー -----------------------------------------------------------
    let tree: Tree = ui.tree()?;
    tree.set_items(&[
        TreeItem::new("t"),
        TreeItem::new("t")
            .detail("t")
            .expanded(true)
            .children([TreeItem::new("t"), TreeItem::from("t")]),
        TreeItem::new("t").enabled(false).child(TreeItem::new("t")),
    ]);
    let _: usize = tree.len();
    let _: bool = tree.is_empty();
    let _: Option<Vec<usize>> = tree.selected();
    tree.set_selected(&[1, 0]);
    tree.clear_selection();
    tree.select(&[0]);
    tree.on_select(|_path: &[usize]| {});
    let _: bool = tree.is_expanded(&[1]);
    tree.set_expanded(&[1], true);
    tree.expand(&[1]);
    tree.collapse(&[1]);
    tree.expand_all();
    tree.collapse_all();
    tree.on_expand(|_path: &[usize], _expanded: bool| {});
    tree.set_sizing(Sizing::new().width(Length::Fill).height(Length::Fixed(1.0)));

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

    // --- トースト ---------------------------------------------------------
    let toast: Toast = ui.toast("t")?;
    toast.set_message("t");
    let _: String = toast.message();
    toast.set_action("t");
    let _: String = toast.action();
    toast.set_timeout(1.0);
    let _: f64 = toast.timeout();
    let _: ToastSpec = toast.spec();
    toast.on_action(|| {});
    toast.on_dismiss(|| {});
    let _: bool = toast.is_visible();
    toast.show();
    toast.dismiss();

    let picker: FilePicker = ui.file_picker("t")?;
    picker.set_text("t");
    picker.set_enabled(true);
    picker.set_mode(FilePickerMode::Files);
    let _: FilePickerMode = picker.mode();
    picker.set_filters(&[FileFilter::new("t", ["png"])]);
    let _: Vec<FileEntry> = picker.selection();
    picker.on_select(|_entries: &[FileEntry]| {});
    // `open()` はダイアログを出すので、契約の確認では呼ばない。

    let saver: FileSaver = ui.file_saver("t")?;
    saver.set_text("t");
    saver.set_enabled(true);
    saver.set_file_name("t");
    let _: String = saver.file_name();
    saver.set_filters(&[FileFilter::new("t", ["txt"])]);
    saver.set_contents(b"t");
    let _: usize = saver.contents_len();
    let _: Option<FileEntry> = saver.destination();
    saver.on_save(|_entry: &FileEntry| {});
    saver.on_error(|_error: &Error| {});
    // `open()` はダイアログを出すので、契約の確認では呼ばない。

    grid.attach(&label, GridCell::new(0, 0));
    grid.attach(&button, GridCell::new(1, 0).span(2, 1));
    scroll.set_child(&grid);
    expander.set_child(&checkbox);

    stack.append(&spacer);
    stack.append(&scroll);
    stack.append(&image);
    stack.append(&video);
    stack.append(&audio);
    stack.append(&label);
    stack.append(&button);
    stack.append(&checkbox);
    stack.append(&combo_box);
    stack.append(&radio_group);
    stack.append(&date_picker);
    stack.append(&color_picker);
    stack.append(&input);
    stack.append(&text_area);
    stack.append(&slider);
    stack.append(&progress);
    stack.append(&tabs);
    stack.append(&navbar);
    stack.append(&dock);
    stack.append(&menu);
    stack.append(&list);
    stack.append(&tree);
    stack.append(&breadcrumbs);
    stack.append(&pagination);
    stack.append(&link);
    stack.append(&picker);
    stack.append(&saver);
    stack.append(&expander);
    window.set_child(&stack);

    ui.quit();
    Ok(())
}
