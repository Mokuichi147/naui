//! # miui-gtk (骨組み・未実装)
//!
//! miui の Linux バックエンド。**まだ実装されていない。**
//!
//! 他のバックエンドと同じ API の形だけを定義してあり、
//! 呼ぶと必ず「未実装」のエラーを返す。ビルドは通るが動作はしない。
//!
//! ## 未実装である理由
//!
//! GTK4 / libadwaita のバインディング (`gtk4` / `libadwaita` クレート) は
//! ビルドに GTK4 の開発用システムライブラリと pkg-config を要求するため、
//! この実装を書いた環境 (macOS) では **コンパイル確認すらできない**。
//! 動作未確認のコードを「実装済み」として置くより、
//! 空であることを明示するほうが誠実だと判断した。
//!
//! ## 実装するときの対応表
//!
//! | miui | GTK4 / libadwaita |
//! | --- | --- |
//! | `run` | `gtk::Application` + `connect_activate` (コールバック内で UI 構築) |
//! | `Window` | `adw::ApplicationWindow` |
//! | `Stack` | `gtk::Box` (`Orientation::Vertical` / `Horizontal`) |
//! | `Grid` | `gtk::Grid` (`attach` に行・列とスパンを渡す) |
//! | `Scroll` | `gtk::ScrolledWindow` (`set_policy`) |
//! | `Spacer` | 中身の無い `gtk::Box` (`set_hexpand` / `set_vexpand`) |
//! | 大きさの指定 | `set_size_request` / `set_hexpand` / `set_halign` |
//! | `Label` | `gtk::Label` |
//! | `Button` | `gtk::Button` + `connect_clicked` |
//! | `Checkbox` | `gtk::CheckButton` + `connect_toggled` |
//! | `TextInput` | `gtk::Entry` + `connect_changed` |
//! | `Slider` | `gtk::Scale` + `connect_value_changed` |
//! | `ProgressBar` | `gtk::ProgressBar` |
//! | `Tabs` | `gtk::Notebook` (または `adw::ViewStack` + `adw::ViewSwitcher`) |
//! | `Navbar` | `adw::HeaderBar` + `adw::ViewSwitcher` |
//! | `Dock` | `adw::ViewSwitcherBar` |
//! | `Menu` | `gtk::ListBox` (`connect_row_selected`) |
//! | `Breadcrumbs` | 相当するものが無いため `gtk::Box` + `gtk::Button` |
//! | `Pagination` | 相当するものが無いため `gtk::Box` + `gtk::Button` |
//! | `Link` | `gtk::LinkButton` |
//!
//! GTK のシグナルハンドラは `'static` なクロージャを受けるので、
//! macOS/Web と同じ `Rc<Inner>` + クロージャ保持の形がそのまま使える
//! (Windows のような `Send + Sync` 制約は無い)。

#![cfg(all(
    unix,
    not(any(target_os = "macos", target_os = "ios", target_os = "android"))
))]
#![forbid(unsafe_code)]

use std::cell::{Cell, RefCell};

use miui_core::{
    Align, Error, GridCell, NavItem, Orientation, Padding, Result, ScrollPolicy, Settings, Sizing,
    Theme, Track,
};

fn unimplemented_error(what: &'static str) -> Error {
    Error::new(
        what,
        "Linux (GTK4) バックエンドは未実装です。crates/miui-gtk のドキュメントを参照してください",
    )
}

/// GTK4 の実ウィジェットに対応する予定のハンドル。現状は中身を持たない。
macro_rules! placeholder_widget {
    ($name:ident) => {
        // 中身は実装時に入る。骨組みの間は読まれないので警告を止める。
        #[allow(dead_code)]
        #[derive(Clone)]
        pub struct $name(std::rc::Rc<()>);

        impl Widget for $name {
            fn boxed_clone(&self) -> Box<dyn Widget> {
                Box::new(self.clone())
            }
        }

        impl $name {
            /// 大きさを指定する (未実装)。
            pub fn set_sizing(&self, _sizing: Sizing) {}
        }
    };
}

/// miui のウィジェットが実装する共通インタフェース。
pub trait Widget: 'static {
    #[doc(hidden)]
    fn boxed_clone(&self) -> Box<dyn Widget>;
}

placeholder_widget!(Label);
placeholder_widget!(Button);
placeholder_widget!(Checkbox);
placeholder_widget!(TextInput);
placeholder_widget!(Slider);
placeholder_widget!(ProgressBar);
placeholder_widget!(Stack);
placeholder_widget!(Tabs);
placeholder_widget!(Navbar);
placeholder_widget!(Dock);
placeholder_widget!(Menu);
placeholder_widget!(Breadcrumbs);
placeholder_widget!(Pagination);
placeholder_widget!(Link);
placeholder_widget!(Grid);
placeholder_widget!(Scroll);
placeholder_widget!(Spacer);

impl Label {
    pub fn text(&self) -> String {
        String::new()
    }
    pub fn set_text(&self, _text: &str) {}
}

impl Button {
    pub fn set_text(&self, _text: &str) {}
    pub fn set_enabled(&self, _enabled: bool) {}
    pub fn on_click(&self, _f: impl FnMut() + 'static) {}
}

impl Checkbox {
    pub fn is_checked(&self) -> bool {
        false
    }
    pub fn set_checked(&self, _checked: bool) {}
    pub fn set_enabled(&self, _enabled: bool) {}
    pub fn on_toggle(&self, _f: impl FnMut(bool) + 'static) {}
}

impl TextInput {
    pub fn text(&self) -> String {
        String::new()
    }
    pub fn set_text(&self, _text: &str) {}
    pub fn set_placeholder(&self, _text: &str) {}
    pub fn set_enabled(&self, _enabled: bool) {}
    pub fn on_change(&self, _f: impl FnMut(&str) + 'static) {}
}

impl Slider {
    pub fn value(&self) -> f64 {
        0.0
    }
    pub fn set_value(&self, _value: f64) {}
    pub fn set_enabled(&self, _enabled: bool) {}
    pub fn on_change(&self, _f: impl FnMut(f64) + 'static) {}
}

impl ProgressBar {
    pub fn value(&self) -> f64 {
        0.0
    }
    pub fn set_value(&self, _value: f64) {}
}

impl Stack {
    pub fn set_spacing(&self, _spacing: f64) {}
    pub fn set_padding(&self, _padding: Padding) {}
    pub fn set_align(&self, _align: Align) {}
    pub fn append(&self, _child: &dyn Widget) {}
    pub fn len(&self) -> usize {
        0
    }
    pub fn is_empty(&self) -> bool {
        true
    }
}

impl Grid {
    pub fn set_spacing(&self, _column: f64, _row: f64) {}
    pub fn set_padding(&self, _padding: Padding) {}
    pub fn attach(&self, _child: &dyn Widget, _cell: GridCell) {}
    pub fn set_column_track(&self, _index: usize, _track: Track) {}
    pub fn set_row_track(&self, _index: usize, _track: Track) {}
    pub fn columns(&self) -> usize {
        0
    }
    pub fn rows(&self) -> usize {
        0
    }
    pub fn len(&self) -> usize {
        0
    }
    pub fn is_empty(&self) -> bool {
        true
    }
}

impl Scroll {
    pub fn set_policy(&self, _horizontal: ScrollPolicy, _vertical: ScrollPolicy) {}
    pub fn set_child(&self, _child: &dyn Widget) {}
}

/// 項目を持つナビゲーションの共通実装 (未実装)。
macro_rules! placeholder_item_bar {
    ($name:ident) => {
        impl $name {
            pub fn set_items(&self, _items: &[NavItem]) {}
            pub fn len(&self) -> usize {
                0
            }
            pub fn is_empty(&self) -> bool {
                true
            }
            pub fn selected(&self) -> Option<usize> {
                None
            }
            pub fn set_selected(&self, _index: usize) {}
            pub fn select(&self, _index: usize) {}
            pub fn on_select(&self, _f: impl FnMut(usize) + 'static) {}
        }
    };
}

placeholder_item_bar!(Navbar);
placeholder_item_bar!(Dock);
placeholder_item_bar!(Menu);
placeholder_item_bar!(Breadcrumbs);

impl Navbar {
    pub fn set_title(&self, _title: &str) {}
    pub fn title(&self) -> String {
        String::new()
    }
}

impl Tabs {
    pub fn add_tab(&self, _label: &str, _child: &dyn Widget) {}
    pub fn len(&self) -> usize {
        0
    }
    pub fn is_empty(&self) -> bool {
        true
    }
    pub fn selected(&self) -> Option<usize> {
        None
    }
    pub fn set_selected(&self, _index: usize) {}
    pub fn select(&self, _index: usize) {}
    pub fn on_select(&self, _f: impl FnMut(usize) + 'static) {}
}

impl Pagination {
    pub fn set_page_count(&self, _count: usize) {}
    pub fn page_count(&self) -> usize {
        0
    }
    pub fn page(&self) -> usize {
        0
    }
    pub fn set_page(&self, _page: usize) {}
    pub fn select(&self, _page: usize) {}
    pub fn go_previous(&self) {}
    pub fn go_next(&self) {}
    pub fn on_change(&self, _f: impl FnMut(usize) + 'static) {}
}

impl Link {
    pub fn text(&self) -> String {
        String::new()
    }
    pub fn set_text(&self, _text: &str) {}
    pub fn href(&self) -> String {
        String::new()
    }
    pub fn set_href(&self, _href: &str) {}
    pub fn set_enabled(&self, _enabled: bool) {}
    pub fn on_click(&self, _f: impl FnMut() + 'static) {}
}

/// トップレベルウィンドウ (未実装)。
#[allow(dead_code)]
#[derive(Clone)]
pub struct Window(std::rc::Rc<()>);

impl Window {
    pub fn set_title(&self, _title: &str) {}
    pub fn title(&self) -> String {
        String::new()
    }
    pub fn set_size(&self, _width: f64, _height: f64) {}
    pub fn set_child(&self, _child: &dyn Widget) {}
    pub fn show(&self) {}
    pub fn close(&self) {}
    pub fn is_visible(&self) -> bool {
        false
    }
    pub fn set_theme(&self, _theme: Theme) -> Result<()> {
        Ok(())
    }
}

/// ウィジェットを生成するための入り口 (未実装)。
pub struct Ui {
    theme: Cell<Theme>,
    _private: RefCell<()>,
}

impl Ui {
    pub fn window(&self, _title: &str, _width: f64, _height: f64) -> Result<Window> {
        Err(unimplemented_error("ウィンドウの生成"))
    }
    pub fn stack(&self, _orientation: Orientation) -> Result<Stack> {
        Err(unimplemented_error("Stack の生成"))
    }
    pub fn grid(&self) -> Result<Grid> {
        Err(unimplemented_error("Grid の生成"))
    }
    pub fn scroll(&self) -> Result<Scroll> {
        Err(unimplemented_error("Scroll の生成"))
    }
    pub fn spacer(&self) -> Result<Spacer> {
        Err(unimplemented_error("Spacer の生成"))
    }
    pub fn label(&self, _text: &str) -> Result<Label> {
        Err(unimplemented_error("Label の生成"))
    }
    pub fn button(&self, _text: &str) -> Result<Button> {
        Err(unimplemented_error("Button の生成"))
    }
    pub fn checkbox(&self, _label: &str) -> Result<Checkbox> {
        Err(unimplemented_error("Checkbox の生成"))
    }
    pub fn text_input(&self, _text: &str) -> Result<TextInput> {
        Err(unimplemented_error("TextInput の生成"))
    }
    pub fn slider(&self, _min: f64, _max: f64) -> Result<Slider> {
        Err(unimplemented_error("Slider の生成"))
    }
    pub fn progress_bar(&self) -> Result<ProgressBar> {
        Err(unimplemented_error("ProgressBar の生成"))
    }
    pub fn tabs(&self) -> Result<Tabs> {
        Err(unimplemented_error("Tabs の生成"))
    }
    pub fn navbar(&self, _title: &str) -> Result<Navbar> {
        Err(unimplemented_error("Navbar の生成"))
    }
    pub fn dock(&self) -> Result<Dock> {
        Err(unimplemented_error("Dock の生成"))
    }
    pub fn menu(&self) -> Result<Menu> {
        Err(unimplemented_error("Menu の生成"))
    }
    pub fn breadcrumbs(&self) -> Result<Breadcrumbs> {
        Err(unimplemented_error("Breadcrumbs の生成"))
    }
    pub fn pagination(&self, _page_count: usize) -> Result<Pagination> {
        Err(unimplemented_error("Pagination の生成"))
    }
    pub fn link(&self, _text: &str, _href: &str) -> Result<Link> {
        Err(unimplemented_error("Link の生成"))
    }

    /// 配色テーマを記録する。GTK4 バックエンドが未実装のため、現時点では描画しない。
    pub fn set_theme(&self, theme: Theme) -> Result<()> {
        self.theme.set(theme);
        Ok(())
    }

    pub fn theme(&self) -> Theme {
        self.theme.get()
    }
    pub fn quit(&self) {}
}

/// 未実装。呼ぶと必ずエラーを返す。
pub fn run<F>(_settings: Settings, _build: F) -> Result<()>
where
    F: FnOnce(&Ui) -> Result<()> + 'static,
{
    Err(unimplemented_error("Linux でのアプリ起動"))
}
