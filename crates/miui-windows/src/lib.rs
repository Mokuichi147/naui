//! # miui-windows
//!
//! miui の Windows バックエンド。**WinUI 3 (Fluent 2) の実コントロール**
//! (`Microsoft.UI.Xaml.Controls.Button` など) を生成する。
//!
//! WinUI 3 は Windows SDK ではなく **Windows App SDK** に含まれるため、
//! - 実行環境に Windows App SDK ランタイムが必要
//! - コントロールは `Application::Start` の後でしか生成できない
//!
//! という制約がある。後者のために miui の公開 API は
//! 「コールバックの中で UI を組み立てる」形になっている。
//!
//! ## 実行要件
//!
//! Windows App SDK 2.x ランタイムが必要。実行時には `V2` のフレームワーク
//! パッケージ依存関係を追加し、インストール済みの最新2.xランタイムを使用する。

#![cfg(target_os = "windows")]

mod app;
mod navigation;
mod ui_thread;
mod widgets;
mod window;

use std::cell::{Cell, RefCell};

use miui_core::{Error, Orientation, Result, Settings, Theme};

pub use navigation::{Breadcrumbs, Dock, Link, Menu, Navbar, Pagination, Tabs};
pub use widgets::{Button, Checkbox, Label, ProgressBar, Slider, Stack, TextInput, Widget};
pub use window::Window;

pub(crate) fn to_error(context: &'static str, e: windows_core::Error) -> Error {
    Error::new(context, e.message())
}

/// ウィジェットを生成するための入り口。
pub struct Ui {
    theme: Cell<Theme>,
    windows: RefCell<Vec<Window>>,
}

impl Ui {
    fn new(theme: Theme) -> Self {
        Self {
            theme: Cell::new(theme),
            windows: RefCell::new(Vec::new()),
        }
    }

    pub fn window(&self, title: &str, width: f64, height: f64) -> Result<Window> {
        let w = Window::new(title, width, height, self.theme.get())?;
        self.windows.borrow_mut().push(w.clone());
        Ok(w)
    }

    pub fn stack(&self, orientation: Orientation) -> Result<Stack> {
        Stack::new(orientation)
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

    pub fn text_input(&self, text: &str) -> Result<TextInput> {
        TextInput::new(text)
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

    /// 縦に並ぶナビゲーション一覧。
    pub fn menu(&self) -> Result<Menu> {
        Menu::new()
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

    /// 配色テーマを実行中に切り替える。
    pub fn set_theme(&self, theme: Theme) -> Result<()> {
        for window in self.windows.borrow().iter() {
            window.set_theme(theme)?;
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

    Application::Start(&ApplicationInitializationCallback::new(move |_| {
        let app = app::compose(state, failure, settings.theme)?;
        std::mem::forget(app);
        Ok(())
    }))
    .map_err(|e| to_error("Application::Start", e))?;

    match failure.with_mut_cross_thread(|slot| slot.take()) {
        Some(e) => Err(e),
        None => Ok(()),
    }
}
