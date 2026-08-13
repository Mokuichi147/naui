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

pub use miui_core::{Align, Error, NavItem, Orientation, Padding, Result, Settings};

#[cfg(target_arch = "wasm32")]
pub use miui_web::{
    run, Breadcrumbs, Button, Checkbox, Dock, Label, Link, Menu, Navbar, Pagination, ProgressBar,
    Slider, Stack, Tabs, TextInput, Ui, Widget, Window,
};

#[cfg(all(not(target_arch = "wasm32"), target_os = "macos"))]
pub use miui_macos::{
    run, Breadcrumbs, Button, Checkbox, Dock, Label, Link, Menu, Navbar, Pagination, ProgressBar,
    Slider, Stack, Tabs, TextInput, Ui, Widget, Window,
};

#[cfg(all(not(target_arch = "wasm32"), target_os = "windows"))]
pub use miui_windows::{
    run, Breadcrumbs, Button, Checkbox, Dock, Label, Link, Menu, Navbar, Pagination, ProgressBar,
    Slider, Stack, Tabs, TextInput, Ui, Widget, Window,
};

#[cfg(all(
    not(target_arch = "wasm32"),
    unix,
    not(any(target_os = "macos", target_os = "ios", target_os = "android"))
))]
pub use miui_gtk::{
    run, Breadcrumbs, Button, Checkbox, Dock, Label, Link, Menu, Navbar, Pagination, ProgressBar,
    Slider, Stack, Tabs, TextInput, Ui, Widget, Window,
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
