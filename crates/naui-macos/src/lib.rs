//! # naui-macos
//!
//! naui の macOS バックエンド。**AppKit の実コントロール**
//! (NSWindow / NSButton / NSTextField / NSSlider / NSStackView …) を生成し、
//! target/action とデリゲートを Rust のクロージャへ中継する。
//!
//! 描画・レイアウト・IME・アクセシビリティはすべて AppKit が行う。

// Objective-C 呼び出しのため unsafe が必要。
#![allow(unsafe_code)]

mod combo_box;
mod dialog;
mod file_picker;
mod layout;
mod list;
mod media;
mod menu_bar;
mod navigation;
mod popup;
mod radio_group;
mod trampoline;
mod widgets;
mod window;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use naui_core::{Error, Orientation, Result, Settings, Theme};
use objc2::rc::Retained;
use objc2::runtime::{NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{define_class, msg_send, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSAppearance, NSAppearanceNameAqua, NSAppearanceNameDarkAqua, NSApplication,
    NSApplicationActivationPolicy, NSApplicationDelegate,
};
use objc2_foundation::NSNotification;

pub use combo_box::ComboBox;
pub use dialog::Dialog;
pub use file_picker::FilePicker;
pub use layout::{Grid, Scroll, Spacer};
pub use list::List;
pub use media::{Audio, Image, Video};
pub use navigation::{Breadcrumbs, Dock, Link, Menu, Navbar, Pagination, Tabs};
pub use popup::PopupMenu;
pub use radio_group::RadioGroup;
pub use widgets::{
    Button, Checkbox, Label, ProgressBar, Slider, Stack, TextArea, TextInput, Widget,
};
pub use window::{WeakWindow, Window};

/// ウィジェットを生成するための入り口。
///
/// AppKit はメインスレッドでしか UI を触れないため、`Ui` は
/// [`MainThreadMarker`] を持ち、`run` のコールバック内でのみ得られる。
pub struct Ui {
    mtm: MainThreadMarker,
    theme: Cell<Theme>,
    /// コールバックが終わってもウィンドウを生かしておくための保持。
    windows: RefCell<Vec<Window>>,
    /// ダイアログはどこにも append されないので、ここで保持する。
    dialogs: RefCell<Vec<Dialog>>,
    /// ポップアップメニューはレイアウトに載らないので、親が保持してくれない。
    popups: RefCell<Vec<PopupMenu>>,
}

impl Ui {
    fn new(mtm: MainThreadMarker, theme: Theme) -> Self {
        Self {
            mtm,
            theme: Cell::new(theme),
            windows: RefCell::new(Vec::new()),
            dialogs: RefCell::new(Vec::new()),
            popups: RefCell::new(Vec::new()),
        }
    }

    /// ウィンドウを作る。フレームワークが参照を保持するので、
    /// 戻り値を捨てても閉じられることはない。
    pub fn window(&self, title: &str, width: f64, height: f64) -> Result<Window> {
        let w = Window::new(self.mtm, title, width, height);
        self.windows.borrow_mut().push(w.clone());
        Ok(w)
    }

    pub fn stack(&self, orientation: Orientation) -> Result<Stack> {
        Ok(Stack::new(self.mtm, orientation))
    }

    /// 行と列で位置を決めるコンテナ。
    pub fn grid(&self) -> Result<Grid> {
        Ok(Grid::new(self.mtm))
    }

    /// 中身がはみ出したらスクロールさせるコンテナ。
    pub fn scroll(&self) -> Result<Scroll> {
        Ok(Scroll::new(self.mtm))
    }

    /// 余白そのものになるウィジェット。スタックの余りを吸って他を押しやる。
    pub fn spacer(&self) -> Result<Spacer> {
        Ok(Spacer::new(self.mtm))
    }

    pub fn label(&self, text: &str) -> Result<Label> {
        Ok(Label::new(self.mtm, text))
    }

    pub fn button(&self, text: &str) -> Result<Button> {
        Ok(Button::new(self.mtm, text))
    }

    pub fn checkbox(&self, label: &str) -> Result<Checkbox> {
        Ok(Checkbox::new(self.mtm, label))
    }

    /// 選択肢を折りたたんで表示するコンボボックス。
    pub fn combo_box(&self) -> Result<ComboBox> {
        Ok(ComboBox::new(self.mtm))
    }

    /// 選択肢を並べて 1 つだけ選ばせるラジオグループ。
    pub fn radio_group(&self) -> Result<RadioGroup> {
        Ok(RadioGroup::new(self.mtm))
    }

    pub fn text_input(&self, text: &str) -> Result<TextInput> {
        Ok(TextInput::new(self.mtm, text))
    }

    /// 改行を含む文字列を入力できる欄。高さは `set_sizing` で指定する。
    pub fn text_area(&self, text: &str) -> Result<TextArea> {
        Ok(TextArea::new(self.mtm, text))
    }

    pub fn slider(&self, min: f64, max: f64) -> Result<Slider> {
        Ok(Slider::new(self.mtm, min, max))
    }

    pub fn progress_bar(&self) -> Result<ProgressBar> {
        Ok(ProgressBar::new(self.mtm))
    }

    /// タブ。中身のウィジェットごと持つ。
    pub fn tabs(&self) -> Result<Tabs> {
        Ok(Tabs::new(self.mtm))
    }

    /// 画面上部に置く横並びのナビゲーション。`title` は左端の見出し。
    pub fn navbar(&self, title: &str) -> Result<Navbar> {
        Ok(Navbar::new(self.mtm, title))
    }

    /// 画面下部に置く横並びのナビゲーション (等幅)。
    pub fn dock(&self) -> Result<Dock> {
        Ok(Dock::new(self.mtm))
    }

    /// 縦に並ぶナビゲーション一覧。
    pub fn menu(&self) -> Result<Menu> {
        Ok(Menu::new(self.mtm))
    }

    /// 選択できる行の一覧。自分でスクロールする。
    pub fn list(&self) -> Result<List> {
        Ok(List::new(self.mtm))
    }

    /// 右クリックで出るポップアップ (コンテキスト) メニュー。
    ///
    /// フレームワークが参照を保持するので、戻り値を捨てても
    /// 取り付け先から消えることはない。
    pub fn popup_menu(&self) -> Result<PopupMenu> {
        let popup = PopupMenu::new(self.mtm);
        self.popups.borrow_mut().push(popup.clone());
        Ok(popup)
    }

    /// パンくず。
    pub fn breadcrumbs(&self) -> Result<Breadcrumbs> {
        Ok(Breadcrumbs::new(self.mtm))
    }

    /// ページ送り。`page_count` はページ数。
    pub fn pagination(&self, page_count: usize) -> Result<Pagination> {
        Ok(Pagination::new(self.mtm, page_count))
    }

    /// リンク。`href` が空でなければ、押したときにブラウザで開く。
    pub fn link(&self, text: &str, href: &str) -> Result<Link> {
        Ok(Link::new(self.mtm, text, href))
    }

    /// 画像。`source` はファイルパスか URL。
    pub fn image(&self, source: &str) -> Result<Image> {
        Ok(Image::new(self.mtm, source))
    }

    /// 動画。`source` はファイルパスか URL。
    pub fn video(&self, source: &str) -> Result<Video> {
        Ok(Video::new(self.mtm, source))
    }

    /// 音声。`source` はファイルパスか URL。
    pub fn audio(&self, source: &str) -> Result<Audio> {
        Ok(Audio::new(self.mtm, source))
    }

    /// ファイルやフォルダーを選ばせるボタン。押すと `NSOpenPanel` が出る。
    pub fn file_picker(&self, text: &str) -> Result<FilePicker> {
        Ok(FilePicker::new(self.mtm, text))
    }

    /// モーダルダイアログ。`title` は見出し。中身は `NSAlert`。
    ///
    /// フレームワークが参照を保持するので、戻り値を捨てても
    /// 通知が届かなくなることはない。
    pub fn dialog(&self, title: &str) -> Result<Dialog> {
        let d = Dialog::new(self.mtm, title);
        self.dialogs.borrow_mut().push(d.clone());
        Ok(d)
    }

    /// 配色テーマを実行中に切り替える。
    pub fn set_theme(&self, theme: Theme) -> Result<()> {
        let appearance = appearance_for_theme(theme);
        NSApplication::sharedApplication(self.mtm).setAppearance(appearance.as_deref());
        self.theme.set(theme);
        Ok(())
    }

    /// 現在選択されている配色テーマを返す。
    pub fn theme(&self) -> Theme {
        self.theme.get()
    }

    /// アプリを終了する。
    pub fn quit(&self) {
        NSApplication::sharedApplication(self.mtm).terminate(None);
    }
}

type BuildFn = Box<dyn FnOnce(&Ui) -> Result<()>>;

struct DelegateState {
    build: RefCell<Option<BuildFn>>,
    ui: Rc<Ui>,
    error: Rc<RefCell<Option<Error>>>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "NauiAppDelegate"]
    #[ivars = DelegateState]
    struct AppDelegate;

    unsafe impl NSObjectProtocol for AppDelegate {}

    unsafe impl NSApplicationDelegate for AppDelegate {
        #[unsafe(method(applicationDidFinishLaunching:))]
        fn did_finish_launching(&self, _notification: &NSNotification) {
            let state = self.ivars();
            let Some(build) = state.build.borrow_mut().take() else {
                return;
            };
            if let Err(e) = build(&state.ui) {
                *state.error.borrow_mut() = Some(e);
                let mtm = MainThreadMarker::from(self);
                NSApplication::sharedApplication(mtm).terminate(None);
            }
        }

        #[unsafe(method(applicationShouldTerminateAfterLastWindowClosed:))]
        fn should_terminate_after_last_window_closed(&self, _app: &NSApplication) -> bool {
            true
        }
    }
);

/// アプリを起動し、`build` の中で UI を組み立てる。
///
/// `build` はアプリの初期化が終わってから呼ばれる。ここでしかウィジェットを
/// 作れないのは、WinUI 3 が `Application::Start` 前のコントロール生成を
/// 許さないためで、4 バックエンドで同じ形にそろえてある。
///
/// この関数はウィンドウがすべて閉じられるまで戻らない。
pub fn run<F>(settings: Settings, build: F) -> Result<()>
where
    F: FnOnce(&Ui) -> Result<()> + 'static,
{
    let mtm = MainThreadMarker::new()
        .ok_or_else(|| Error::new("naui の起動", "メインスレッドから呼んでください"))?;
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
    let appearance = appearance_for_theme(settings.theme);
    app.setAppearance(appearance.as_deref());
    // ⌘C / ⌘V などはメインメニューのキー等価として配送される。
    // メニューが無いと、テキスト入力で貼り付けができない。
    menu_bar::install(mtm, &settings.name);

    let error = Rc::new(RefCell::new(None));
    let delegate = AppDelegate::alloc(mtm).set_ivars(DelegateState {
        build: RefCell::new(Some(Box::new(build))),
        ui: Rc::new(Ui::new(mtm, settings.theme)),
        error: error.clone(),
    });
    let delegate: Retained<AppDelegate> = unsafe { msg_send![super(delegate), init] };
    app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));

    app.activate();
    app.run();

    let failure = error.borrow_mut().take();
    match failure {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

/// イベントループへ入らずに `build` だけを実行して戻る。**自動テスト専用**。
///
/// 実際のアプリでは [`run`] を使うこと。AppKit は
/// `NSApplication` さえ初期化されていればコントロールを生成できるため、
/// この形でウィジェットの生成・操作・状態変化を検証できる。
pub fn run_for_test<F>(build: F) -> Result<()>
where
    F: FnOnce(&Ui) -> Result<()>,
{
    let mtm = MainThreadMarker::new()
        .ok_or_else(|| Error::new("テスト用の起動", "メインスレッドから呼んでください"))?;
    let app = NSApplication::sharedApplication(mtm);
    // テスト中に Dock アイコンを出さない。
    app.setActivationPolicy(NSApplicationActivationPolicy::Prohibited);
    let ui = Ui::new(mtm, Theme::System);
    build(&ui)
}

/// メインメニューを組み立てる。**自動テスト専用**。
///
/// 実際のアプリでは [`run`] が起動時に呼ぶ。
#[doc(hidden)]
pub fn install_menu_bar_for_test(mtm: MainThreadMarker, app_name: &str) {
    menu_bar::install(mtm, app_name);
}

fn appearance_for_theme(theme: Theme) -> Option<Retained<NSAppearance>> {
    match theme {
        Theme::System => None,
        Theme::Light => unsafe { NSAppearance::appearanceNamed(NSAppearanceNameAqua) },
        Theme::Dark => unsafe { NSAppearance::appearanceNamed(NSAppearanceNameDarkAqua) },
    }
}
