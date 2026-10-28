//! # naui-macos
//!
//! naui's macOS backend. It creates **real AppKit controls** (NSWindow / NSButton /
//! NSTextField / NSSlider / NSStackView …) and relays target/action and delegates
//! to Rust closures.
//!
//! Drawing, layout, IME and accessibility are all done by AppKit.

// Objective-C 呼び出しのため unsafe が必要。
#![allow(unsafe_code)]

mod canvas;
mod clipboard;
mod color_picker;
mod combo_box;
mod date_picker;
mod dialog;
mod editable_combo_box;
mod expander;
mod file_picker;
mod file_saver;
mod focus;
mod interaction;
mod keys;
mod layout;
mod list;
mod main_thread;
mod media;
mod menu_bar;
mod navigation;
mod number_input;
mod popup;
mod radio_group;
mod sidebar;
mod split_view;
mod table;
mod time_picker;
mod toast;
mod toggle;
mod toolbar;
mod trampoline;
mod tree;
mod widgets;
mod window;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use naui_core::{DatePickerMode, Error, Orientation, Result, Settings, Tasks, Theme};
use objc2::rc::Retained;
use objc2::runtime::{NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{define_class, msg_send, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSAppearance, NSAppearanceNameAqua, NSAppearanceNameDarkAqua, NSApplication,
    NSApplicationActivationPolicy, NSApplicationDelegate,
};
use objc2_foundation::{NSLocale, NSNotification};

pub use canvas::Canvas;
pub use clipboard::Clipboard;
pub use color_picker::ColorPicker;
pub use combo_box::ComboBox;
pub use date_picker::DatePicker;
pub use dialog::Dialog;
pub use editable_combo_box::EditableComboBox;
pub use expander::Expander;
pub use file_picker::FilePicker;
pub use file_saver::FileSaver;
pub use layout::{Grid, Scroll, Spacer};
pub use list::{List, ListRow};
pub use media::{Audio, Image, Video};
pub use menu_bar::MenuBar;
pub use navigation::{Breadcrumbs, Dock, Link, Menu, Navbar, Pagination, Tabs};
pub use number_input::NumberInput;
pub use popup::PopupMenu;
pub use radio_group::RadioGroup;
pub use sidebar::Sidebar;
pub use split_view::SplitView;
pub use table::{Table, TableCells};
pub use time_picker::TimePicker;
pub use toast::Toast;
pub use toggle::Toggle;
pub use toolbar::Toolbar;
pub use tree::Tree;
pub use widgets::{
    Button, Checkbox, Label, PasswordInput, ProgressBar, SearchInput, Slider, Spinner, Stack,
    TextArea, TextInput, Widget,
};
pub use window::{WeakWindow, Window};

/// The entry point for creating widgets.
///
/// AppKit can only touch the UI on the main thread, so `Ui` holds a
/// [`MainThreadMarker`] and is available only inside the `run` callback.
///
/// Like widget handles, **clones share the same contents**, so moving one into a
/// callback lets you create widgets later. It is main-thread only and not `Send`,
/// so it cannot be passed to other threads (use [`Tasks`] for that).
#[derive(Clone)]
pub struct Ui(Rc<UiInner>);

struct UiInner {
    mtm: MainThreadMarker,
    /// The app name. Used for the heading of the first menu in the menu bar (the app
    /// menu).
    app_name: String,
    theme: Cell<Theme>,
    /// Keeps windows alive after the callback finishes.
    windows: RefCell<Vec<Window>>,
    /// Dialogs are never appended anywhere, so they are held here.
    dialogs: RefCell<Vec<Dialog>>,
    /// Popup menus are not part of the layout, so no parent holds them.
    popups: RefCell<Vec<PopupMenu>>,
    /// Toolbars are not part of the layout either, so they are held here.
    toolbars: RefCell<Vec<Toolbar>>,
    /// Menu bars are not part of the layout either, so they are held here.
    menu_bars: RefCell<Vec<MenuBar>>,
    /// Sidebars are not part of the layout either, so they are held here.
    sidebars: RefCell<Vec<Sidebar>>,
    /// Toasts are not part of the layout either, so they are held here.
    toasts: RefCell<Vec<Toast>>,
    /// The entry point for other threads and async work.
    tasks: Tasks,
}

impl Ui {
    fn new(mtm: MainThreadMarker, app_name: &str, theme: Theme) -> Self {
        naui_core::set_ui_languages(preferred_languages());
        Self(Rc::new(UiInner {
            mtm,
            app_name: app_name.to_string(),
            theme: Cell::new(theme),
            windows: RefCell::new(Vec::new()),
            dialogs: RefCell::new(Vec::new()),
            popups: RefCell::new(Vec::new()),
            toolbars: RefCell::new(Vec::new()),
            menu_bars: RefCell::new(Vec::new()),
            sidebars: RefCell::new(Vec::new()),
            toasts: RefCell::new(Vec::new()),
            tasks: Tasks::from_main_thread(std::sync::Arc::new(main_thread::MainQueue)),
        }))
    }

    /// Creates a window. The framework keeps a reference to it, so dropping the return
    /// value does not close it.
    ///
    /// `width` / `height` are the size of the content area (the same as
    /// [`Window::set_size`]), excluding the title bar and toolbar.
    pub fn window(&self, title: &str, width: f64, height: f64) -> Result<Window> {
        let w = Window::new(self.0.mtm, title, width, height);
        self.0.windows.borrow_mut().push(w.clone());
        Ok(w)
    }

    pub fn stack(&self, orientation: Orientation) -> Result<Stack> {
        Ok(Stack::new(self.0.mtm, orientation))
    }

    /// A container that positions children by row and column.
    pub fn grid(&self) -> Result<Grid> {
        Ok(Grid::new(self.0.mtm))
    }

    /// A container that scrolls when its contents overflow.
    pub fn scroll(&self) -> Result<Scroll> {
        Ok(Scroll::new(self.0.mtm))
    }

    /// A container whose contents are shown and hidden by clicking its heading. `text`
    /// is the heading text.
    pub fn expander(&self, text: &str) -> Result<Expander> {
        Ok(Expander::new(self.0.mtm, text))
    }

    /// A container that divides two panes with a movable divider.
    ///
    /// With `Horizontal`, the panes are side by side and the divider is vertical.
    pub fn split_view(&self, orientation: Orientation) -> Result<SplitView> {
        Ok(SplitView::new(self.0.mtm, orientation))
    }

    /// A widget that is just empty space. It absorbs a stack's leftover space and
    /// pushes the others away.
    pub fn spacer(&self) -> Result<Spacer> {
        Ok(Spacer::new(self.0.mtm))
    }

    pub fn label(&self, text: &str) -> Result<Label> {
        Ok(Label::new(self.0.mtm, text))
    }

    pub fn button(&self, text: &str) -> Result<Button> {
        Ok(Button::new(self.0.mtm, text))
    }

    pub fn checkbox(&self, label: &str) -> Result<Checkbox> {
        Ok(Checkbox::new(self.0.mtm, label))
    }

    /// An on/off switch. `label` is the text placed next to it.
    pub fn toggle(&self, label: &str) -> Result<Toggle> {
        Ok(Toggle::new(self.0.mtm, label))
    }

    /// A combo box that shows its options collapsed.
    pub fn combo_box(&self) -> Result<ComboBox> {
        Ok(ComboBox::new(self.0.mtm))
    }

    /// A combo box where the user can either choose from suggestions or type freely.
    pub fn editable_combo_box(&self) -> Result<EditableComboBox> {
        Ok(EditableComboBox::new(self.0.mtm))
    }

    /// A radio group that lays out options and lets the user choose exactly one.
    pub fn radio_group(&self) -> Result<RadioGroup> {
        Ok(RadioGroup::new(self.0.mtm))
    }

    /// A control for picking a date or time. `mode` decides what can be picked.
    pub fn date_picker(&self, mode: DatePickerMode) -> Result<DatePicker> {
        Ok(DatePicker::new(self.0.mtm, mode))
    }

    /// A control for picking only a time of day. The initial value is the current time.
    pub fn time_picker(&self) -> Result<TimePicker> {
        Ok(TimePicker::new(self.0.mtm))
    }

    /// A control for picking a color. The initial value is black.
    pub fn color_picker(&self) -> Result<ColorPicker> {
        Ok(ColorPicker::new(self.0.mtm))
    }

    /// A surface the app draws on itself. Set its size with `set_sizing`.
    pub fn canvas(&self) -> Result<Canvas> {
        Ok(Canvas::new(self.0.mtm))
    }

    pub fn text_input(&self, text: &str) -> Result<TextInput> {
        Ok(TextInput::new(self.0.mtm, text))
    }

    /// A field whose input is masked. Backed by `NSSecureTextField`.
    pub fn password_input(&self) -> Result<PasswordInput> {
        Ok(PasswordInput::new(self.0.mtm))
    }

    /// A search field. Backed by `NSSearchField`.
    pub fn search_input(&self) -> Result<SearchInput> {
        Ok(SearchInput::new(self.0.mtm))
    }

    /// A field for entering a number. `value` is the initial value.
    pub fn number_input(&self, value: f64) -> Result<NumberInput> {
        Ok(NumberInput::new(self.0.mtm, value))
    }

    /// A field for entering text that includes newlines. Set its height with
    /// `set_sizing`.
    pub fn text_area(&self, text: &str) -> Result<TextArea> {
        Ok(TextArea::new(self.0.mtm, text))
    }

    pub fn slider(&self, min: f64, max: f64) -> Result<Slider> {
        Ok(Slider::new(self.0.mtm, min, max))
    }

    pub fn progress_bar(&self) -> Result<ProgressBar> {
        Ok(ProgressBar::new(self.0.mtm))
    }

    /// A spinning indicator for work in progress. Spins from creation.
    pub fn spinner(&self) -> Result<Spinner> {
        Ok(Spinner::new(self.0.mtm))
    }

    /// Tabs. Holds each tab's content widget.
    pub fn tabs(&self) -> Result<Tabs> {
        Ok(Tabs::new(self.0.mtm))
    }

    /// Horizontal navigation placed at the top of the screen. `title` is the heading
    /// at the left end.
    pub fn navbar(&self, title: &str) -> Result<Navbar> {
        Ok(Navbar::new(self.0.mtm, title))
    }

    /// Horizontal navigation placed at the bottom of the screen (equal widths).
    pub fn dock(&self) -> Result<Dock> {
        Ok(Dock::new(self.0.mtm))
    }

    /// A vertical list of navigation items.
    pub fn menu(&self) -> Result<Menu> {
        Ok(Menu::new(self.0.mtm))
    }

    /// A toolbar attached to the top of the window.
    ///
    /// Attach it with [`Window::set_toolbar`]. The framework keeps a reference to it,
    /// so dropping the return value does not stop notifications.
    pub fn toolbar(&self) -> Result<Toolbar> {
        let toolbar = Toolbar::new(self.0.mtm);
        self.0.toolbars.borrow_mut().push(toolbar.clone());
        Ok(toolbar)
    }

    /// The OS's application menu, shown at the top of the screen.
    ///
    /// Attach it with [`Window::set_menu_bar`]. The framework keeps a reference to it,
    /// so dropping the return value does not stop notifications.
    pub fn menu_bar(&self) -> Result<MenuBar> {
        let menu_bar = MenuBar::new(self.0.mtm, &self.0.app_name);
        self.0.menu_bars.borrow_mut().push(menu_bar.clone());
        Ok(menu_bar)
    }

    /// A sidebar attached to the left of the window.
    ///
    /// Attach it with [`Window::set_sidebar`]. The framework keeps a reference to it,
    /// so dropping the return value does not stop notifications.
    pub fn sidebar(&self) -> Result<Sidebar> {
        let sidebar = Sidebar::new(self.0.mtm);
        self.0.sidebars.borrow_mut().push(sidebar.clone());
        Ok(sidebar)
    }

    /// A list of selectable rows. It scrolls by itself.
    pub fn list(&self) -> Result<List> {
        Ok(List::new(self.0.mtm))
    }

    /// A table with column headings. It scrolls by itself.
    pub fn table(&self) -> Result<Table> {
        Ok(Table::new(self.0.mtm))
    }

    /// A list whose nested items can be expanded and collapsed. It scrolls by itself.
    pub fn tree(&self) -> Result<Tree> {
        Ok(Tree::new(self.0.mtm))
    }

    /// A popup (context) menu shown on right-click.
    ///
    /// The framework keeps a reference to it, so dropping the return value does not
    /// remove it from the widgets it is attached to.
    pub fn popup_menu(&self) -> Result<PopupMenu> {
        let popup = PopupMenu::new(self.0.mtm);
        self.0.popups.borrow_mut().push(popup.clone());
        Ok(popup)
    }

    /// Breadcrumbs.
    pub fn breadcrumbs(&self) -> Result<Breadcrumbs> {
        Ok(Breadcrumbs::new(self.0.mtm))
    }

    /// Pagination. `page_count` is the number of pages.
    pub fn pagination(&self, page_count: usize) -> Result<Pagination> {
        Ok(Pagination::new(self.0.mtm, page_count))
    }

    /// A link. If `href` is not empty, clicking it opens it in the browser.
    pub fn link(&self, text: &str, href: &str) -> Result<Link> {
        Ok(Link::new(self.0.mtm, text, href))
    }

    /// An image. `source` is a file path or a URL.
    pub fn image(&self, source: &str) -> Result<Image> {
        Ok(Image::new(self.0.mtm, source))
    }

    /// Video. `source` is a file path or a URL.
    pub fn video(&self, source: &str) -> Result<Video> {
        Ok(Video::new(self.0.mtm, source))
    }

    /// Audio. `source` is a file path or a URL.
    pub fn audio(&self, source: &str) -> Result<Audio> {
        Ok(Audio::new(self.0.mtm, source))
    }

    /// A button for picking files or folders. Clicking it shows an `NSOpenPanel`.
    pub fn file_picker(&self, text: &str) -> Result<FilePicker> {
        Ok(FilePicker::new(self.0.mtm, text))
    }

    /// A button that lets the user save contents to a file. Clicking it shows an
    /// `NSSavePanel`.
    pub fn file_saver(&self, text: &str) -> Result<FileSaver> {
        Ok(FileSaver::new(self.0.mtm, text))
    }

    /// A transient notification (toast). `message` is the string to show.
    ///
    /// The framework keeps a reference to it, so dropping the return value does not
    /// stop notifications.
    pub fn toast(&self, message: &str) -> Result<Toast> {
        let toast = Toast::new(self.0.mtm, message);
        self.0.toasts.borrow_mut().push(toast.clone());
        Ok(toast)
    }

    /// A modal dialog. `title` is the heading. Backed by `NSAlert`.
    ///
    /// The framework keeps a reference to it, so dropping the return value does not
    /// stop notifications.
    pub fn dialog(&self, title: &str) -> Result<Dialog> {
        let d = Dialog::new(self.0.mtm, title);
        self.0.dialogs.borrow_mut().push(d.clone());
        Ok(d)
    }

    /// Switches the color theme at runtime.
    pub fn set_theme(&self, theme: Theme) -> Result<()> {
        let appearance = appearance_for_theme(theme);
        NSApplication::sharedApplication(self.0.mtm).setAppearance(appearance.as_deref());
        self.0.theme.set(theme);
        Ok(())
    }

    /// Returns the currently selected color theme.
    pub fn theme(&self) -> Theme {
        self.0.theme.get()
    }

    /// The entry point for updating the screen from other threads or async code.
    ///
    /// The returned [`Tasks`] can be cloned and moved into callbacks.
    pub fn tasks(&self) -> Tasks {
        self.0.tasks.clone()
    }

    /// The clipboard. Text can be read and written.
    pub fn clipboard(&self) -> Clipboard {
        Clipboard::new(self.0.tasks.clone())
    }

    /// Opens a URL with the default app (the browser for `https:`, mail for
    /// `mailto:`).
    ///
    /// Opening is left to the OS, so this returns right away. Returns `Err` when the
    /// string cannot be read as a URL (including when it has no scheme) or there is
    /// nothing to open it with. Failures that happen in whatever opens it are not
    /// reported.
    pub fn open_url(&self, url: &str) -> Result<()> {
        // `URLWithString:` は空文字や `example.com` のような scheme の無い文字列にも
        // nil でない URL を返す。それを `NSWorkspace` へ渡すと、Finder が
        // 「アプリケーションを開けません。(-50)」のダイアログを出してしまうので、
        // scheme の無いものはここで弾く。
        let Some(parsed) =
            objc2_foundation::NSURL::URLWithString(&objc2_foundation::NSString::from_str(url))
                .filter(|parsed| parsed.scheme().is_some_and(|scheme| scheme.length() > 0))
        else {
            return Err(Error::new(
                "URL を開く",
                format!("URL として読めません: {url}"),
            ));
        };
        if objc2_app_kit::NSWorkspace::sharedWorkspace().openURL(&parsed) {
            Ok(())
        } else {
            Err(Error::new("URL を開く", format!("開けませんでした: {url}")))
        }
    }

    /// Quits the app.
    pub fn quit(&self) {
        NSApplication::sharedApplication(self.0.mtm).terminate(None);
    }
}

type BuildFn = Box<dyn FnOnce(&Ui) -> Result<()>>;

struct DelegateState {
    build: RefCell<Option<BuildFn>>,
    ui: Ui,
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

/// Starts the app and builds the UI inside `build`.
///
/// `build` is called after the app has finished initializing. Widgets can be
/// created only here because WinUI 3 does not allow creating controls before
/// `Application::Start`, and all four backends follow the same shape.
///
/// This function does not return until every window has been closed.
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
    let error = Rc::new(RefCell::new(None));
    let ui = Ui::new(mtm, &settings.name, settings.theme);
    // ⌘C / ⌘V などはメインメニューのキー等価として配送される。
    // メニューが無いと、テキスト入力で貼り付けができない。
    // 項目名は Ui::new で選んだ言語で作るので、その後に置く。
    menu_bar::install(mtm, &settings.name);
    let delegate = AppDelegate::alloc(mtm).set_ivars(DelegateState {
        build: RefCell::new(Some(Box::new(build))),
        ui: ui.clone(),
        error: error.clone(),
    });
    let delegate: Retained<AppDelegate> = unsafe { msg_send![super(delegate), init] };
    app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));

    app.activate();
    app.run();

    // イベントループが終わった後は、投函しても誰も取り出さない。
    // 送信側へ失敗を返せるようにし、受信クロージャと future を解放する。
    ui.0.tasks.shutdown();

    let failure = error.borrow_mut().take();
    match failure {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

/// Runs only `build` and returns, without entering the event loop. **For automated
/// tests only.**
///
/// Real apps should use [`run`]. AppKit can create controls as long as
/// `NSApplication` is initialized, so this form can verify creating, operating and
/// changing the state of widgets.
pub fn run_for_test<F>(build: F) -> Result<()>
where
    F: FnOnce(&Ui) -> Result<()>,
{
    let mtm = MainThreadMarker::new()
        .ok_or_else(|| Error::new("テスト用の起動", "メインスレッドから呼んでください"))?;
    let app = NSApplication::sharedApplication(mtm);
    // テスト中に Dock アイコンを出さない。
    app.setActivationPolicy(NSApplicationActivationPolicy::Prohibited);
    let ui = Ui::new(mtm, "naui", Theme::System);
    build(&ui)
}

/// Builds the main menu. **For automated tests only.**
///
/// In real apps, [`run`] calls it at startup.
#[doc(hidden)]
pub fn install_menu_bar_for_test(mtm: MainThreadMarker, app_name: &str) {
    menu_bar::install(mtm, app_name);
}

/// The OS's preferred languages, most preferred first (`"ja-JP"` and so on).
///
/// `NSLocale.preferredLanguages` follows the per-app language in System Settings
/// too. AppKit's own strings follow the app bundle's localizations instead, so an
/// app without a bundle gets them in English.
fn preferred_languages() -> Vec<String> {
    NSLocale::preferredLanguages()
        .iter()
        .map(|tag| tag.to_string())
        .collect()
}

fn appearance_for_theme(theme: Theme) -> Option<Retained<NSAppearance>> {
    match theme {
        Theme::System => None,
        Theme::Light => unsafe { NSAppearance::appearanceNamed(NSAppearanceNameAqua) },
        Theme::Dark => unsafe { NSAppearance::appearanceNamed(NSAppearanceNameDarkAqua) },
    }
}
