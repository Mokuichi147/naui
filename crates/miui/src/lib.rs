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
//! use miui::{Orientation, Padding, Settings, Theme};
//!
//! fn main() -> miui::Result<()> {
//!     miui::run(Settings::new("counter").theme(Theme::System), |ui| {
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
//! ## 配置とサイズ
//!
//! どのウィジェットも [`Sizing`] で大きさを指定できる。並べ方は
//! `Stack` (縦横) と `Grid` (行と列)、はみ出しは `Scroll`、
//! 余りを吸わせたいときは `Spacer` を使う。
//!
//! ```no_run
//! # use miui::{GridCell, Length, Result, ScrollPolicy, Sizing, Track, Ui};
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
//! ## ナビゲーション
//!
//! タブ・ナビバー・ドック・メニュー・パンくず・ページ送り・リンクは、
//! どの環境でも同じ API で組み立てられる。項目は [`NavItem`] の並びで渡し、
//! 選ばれたものはインデックスで返る。
//!
//! ```no_run
//! # use miui::{NavItem, Result, Ui};
//! # fn build(ui: &Ui) -> Result<()> {
//! let navbar = ui.navbar("miui")?;
//! navbar.set_items(&NavItem::list(["ホーム", "検索", "設定"]));
//! navbar.on_select(|index| println!("{index} 番目が選ばれた"));
//! navbar.set_selected(0); // 通知せずに選択を変える
//! # Ok(())
//! # }
//! ```
//!
//! | miui | Windows | macOS | Web |
//! | --- | --- | --- | --- |
//! | `Tabs` | `StackPanel` + `ToggleButton` | `NSTabView` | `role="tablist"` |
//! | `Navbar` | `ToggleButton` の横並び | `NSSegmentedControl` | `<nav>` |
//! | `Dock` | `ToggleButton` の横並び | `NSSegmentedControl` | `<nav>` |
//! | `Menu` | `ToggleButton` の縦並び | `NSButton` の縦並び | `<nav><ul>` |
//! | `Breadcrumbs` | `HyperlinkButton` + 区切り | `NSPathControl` | `<nav><ol><a>` |
//! | `Pagination` | `Button` + `ToggleButton` | `NSButton` + `NSSegmentedControl` | `<nav>` |
//! | `Link` | `HyperlinkButton` | `NSButton` (リンク色) | `<a>` |
//!
//! `Menu` は**縦に並ぶナビゲーション一覧**であって、ポップアップメニューではない。
//! `Dock` の下端への固定は、レイアウトが縦横のスタックだけなのでアプリの責務になる。
//!
//! ## 検証状況
//!
//! | 環境 | 状態 |
//! | --- | --- |
//! | macOS | 実行・自動テストあり (ナビゲーションを含む 14 件) |
//! | Web (wasm) | ブラウザで実行確認 (ナビゲーションのクリックまで確認) |
//! | Windows | Windows App SDK 2.3.1 の実機で基本ウィジェットとナビゲーション系を確認 |
//! | Linux | 未実装 |

#![forbid(unsafe_code)]

pub use miui_core::{
    Align, Error, GridCell, Length, NavItem, Orientation, Padding, Result, ScrollPolicy, Settings,
    Sizing, Theme, Track,
};

#[cfg(target_arch = "wasm32")]
pub use miui_web::{
    run, Breadcrumbs, Button, Checkbox, Dock, Grid, Label, Link, Menu, Navbar, Pagination,
    ProgressBar, Scroll, Slider, Spacer, Stack, Tabs, TextInput, Ui, Widget, Window,
};

#[cfg(all(not(target_arch = "wasm32"), target_os = "macos"))]
pub use miui_macos::{
    run, Breadcrumbs, Button, Checkbox, Dock, Grid, Label, Link, Menu, Navbar, Pagination,
    ProgressBar, Scroll, Slider, Spacer, Stack, Tabs, TextInput, Ui, Widget, Window,
};

#[cfg(all(not(target_arch = "wasm32"), target_os = "windows"))]
pub use miui_windows::{
    run, Breadcrumbs, Button, Checkbox, Dock, Grid, Label, Link, Menu, Navbar, Pagination,
    ProgressBar, Scroll, Slider, Spacer, Stack, Tabs, TextInput, Ui, WeakWindow, Widget, Window,
};

#[cfg(all(
    not(target_arch = "wasm32"),
    unix,
    not(any(target_os = "macos", target_os = "ios", target_os = "android"))
))]
pub use miui_gtk::{
    run, Breadcrumbs, Button, Checkbox, Dock, Grid, Label, Link, Menu, Navbar, Pagination,
    ProgressBar, Scroll, Slider, Spacer, Stack, Tabs, TextInput, Ui, Widget, Window,
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
    window.set_theme(Theme::Dark)?;

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

    grid.attach(&label, GridCell::new(0, 0));
    grid.attach(&button, GridCell::new(1, 0).span(2, 1));
    scroll.set_child(&grid);

    stack.append(&spacer);
    stack.append(&scroll);
    stack.append(&label);
    stack.append(&button);
    stack.append(&checkbox);
    stack.append(&input);
    stack.append(&slider);
    stack.append(&progress);
    stack.append(&tabs);
    stack.append(&navbar);
    stack.append(&dock);
    stack.append(&menu);
    stack.append(&breadcrumbs);
    stack.append(&pagination);
    stack.append(&link);
    window.set_child(&stack);

    ui.quit();
    Ok(())
}
