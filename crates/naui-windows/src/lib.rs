//! # naui-windows
//!
//! naui's Windows backend. It creates **real WinUI 3 (Fluent 2) controls**
//! (`Microsoft.UI.Xaml.Controls.Button` and so on).
//!
//! WinUI 3 is part of the **Windows App SDK**, not the Windows SDK, so there are
//! these constraints:
//! - the Windows App SDK runtime is required at run time
//! - controls can only be created after `Application::Start`
//!
//! Because of the latter, naui's public API takes the form "build the UI inside a
//! callback".
//!
//! ## Requirements
//!
//! The Windows App SDK runtime 1.3 or later is required. At startup it adds the
//! dependency on the framework package, but instead of pinning a version it tries
//! from 2.x down to 1.3, newest first, and uses **the first one it can attach**. The
//! line shipped with the OS (`Microsoft.WindowsAppRuntime.CBS*`) is also a
//! candidate.
//!
//! The minimum is 1.3 because `MicaBackdrop` and `Window.SystemBackdrop`, used for
//! the window background, were introduced there.
//!
//! Enabling the `self-contained` feature lets the runtime be bundled next to the
//! exe. In that case only the bundled files are used, and no framework package is
//! looked for. It is an error if the bundled files are not next to the exe.

#![cfg(target_os = "windows")]

mod app;
mod canvas;
mod clipboard;
mod color_picker;
mod combo_box;
mod date_picker;
mod dialog;
mod editable_combo_box;
mod expander;
mod file_drop;
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
mod sdk;
mod search_input;
mod sidebar;
mod split_view;
mod table;
mod time_picker;
mod toast;
mod toggle;
mod toolbar;
mod tree;
mod ui_thread;
mod widgets;
mod window;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use naui_core::{DatePickerMode, Error, Orientation, Result, Settings, Tasks, Theme};

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
pub use menu_bar::{MenuBar, MenuEntry};
pub use navigation::{Breadcrumbs, Dock, Link, Menu, Navbar, Pagination, Tabs};
pub use number_input::NumberInput;
pub use popup::PopupMenu;
pub use radio_group::RadioGroup;
pub use search_input::SearchInput;
pub use sidebar::Sidebar;
pub use split_view::SplitView;
pub use table::{Table, TableCells};
pub use time_picker::TimePicker;
pub use toast::Toast;
pub use toggle::Toggle;
pub use toolbar::Toolbar;
pub use tree::Tree;
pub use widgets::{
    Button, Checkbox, Label, PasswordInput, ProgressBar, Slider, Spinner, Stack, TextArea,
    TextInput, Widget,
};
pub use window::{WeakWindow, Window};

pub(crate) fn to_error(context: &'static str, e: windows_core::Error) -> Error {
    Error::new(context, e.message())
}

/// A container that remembers a single callback. The same role as the alias of the
/// same name in `naui_core`, making the nesting of `Option<Box<dyn FnMut(..)>>` one
/// level shallower.
pub(crate) type Slot<F> = Option<Box<F>>;

/// The entry point for creating widgets.
///
/// Like widget handles, **clones share the same contents**, so moving one into a
/// callback lets you create widgets later. It is UI-thread only and not `Send`, so
/// it cannot be passed to other threads (use [`Tasks`] for that).
#[derive(Clone)]
pub struct Ui(Rc<UiInner>);

struct UiInner {
    theme: Cell<Theme>,
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
    shutdown: &'static ui_thread::UiThreadCell<Option<Ui>>,
}

impl Ui {
    fn new(theme: Theme, shutdown: &'static ui_thread::UiThreadCell<Option<Ui>>) -> Self {
        naui_core::set_ui_languages(preferred_languages());
        Self(Rc::new(UiInner {
            theme: Cell::new(theme),
            windows: RefCell::new(Vec::new()),
            dialogs: RefCell::new(Vec::new()),
            popups: RefCell::new(Vec::new()),
            toolbars: RefCell::new(Vec::new()),
            menu_bars: RefCell::new(Vec::new()),
            sidebars: RefCell::new(Vec::new()),
            toasts: RefCell::new(Vec::new()),
            // `Ui::new` は OnLaunched (UI スレッド) から呼ばれる。
            tasks: Tasks::from_main_thread(std::sync::Arc::new(
                main_thread::Dispatcher::for_current_thread(),
            )),
            shutdown,
        }))
    }

    /// Creates a window.
    ///
    /// `width` / `height` are the size of the content area (the same as
    /// [`Window::set_size`]), excluding the title bar, menu bar and window frame.
    pub fn window(&self, title: &str, width: f64, height: f64) -> Result<Window> {
        let w = Window::new(
            title,
            width,
            height,
            self.0.theme.get(),
            self.0.tasks.clone(),
        )?;
        w.install_closing_handler(self.0.shutdown);
        self.0.windows.borrow_mut().push(w.clone());
        Ok(w)
    }

    fn clear_windows_for_shutdown(&self) {
        for window in self.0.windows.borrow().iter() {
            window.clear_content_for_shutdown();
        }
        release_widget_handlers();
    }

    pub fn stack(&self, orientation: Orientation) -> Result<Stack> {
        Stack::new(orientation)
    }

    /// A container that positions children by row and column.
    pub fn grid(&self) -> Result<Grid> {
        Grid::new()
    }

    /// A container that scrolls when its contents overflow.
    pub fn scroll(&self) -> Result<Scroll> {
        Scroll::new()
    }

    /// A container whose contents are shown and hidden by clicking its heading. `text`
    /// is the heading text.
    pub fn expander(&self, text: &str) -> Result<Expander> {
        Expander::new(text)
    }

    /// A container that divides two panes with a movable divider.
    ///
    /// With `Horizontal`, the panes are side by side and the divider is vertical.
    pub fn split_view(&self, orientation: Orientation) -> Result<SplitView> {
        SplitView::new(orientation)
    }

    /// A widget that is just empty space. Use it together with a `Grid`'s
    /// `Track::Fill`.
    pub fn spacer(&self) -> Result<Spacer> {
        Spacer::new()
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

    /// An on/off switch. `label` is the text placed next to it.
    pub fn toggle(&self, label: &str) -> Result<Toggle> {
        Toggle::new(label)
    }

    /// A combo box that shows its options collapsed.
    pub fn combo_box(&self) -> Result<ComboBox> {
        ComboBox::new()
    }

    /// A combo box where the user can either choose from suggestions or type freely.
    pub fn editable_combo_box(&self) -> Result<EditableComboBox> {
        EditableComboBox::new()
    }

    /// A radio group that lays out options and lets the user choose exactly one.
    pub fn radio_group(&self) -> Result<RadioGroup> {
        RadioGroup::new()
    }

    /// A control for picking a date or time. `mode` decides what can be picked.
    pub fn date_picker(&self, mode: DatePickerMode) -> Result<DatePicker> {
        DatePicker::new(mode)
    }

    /// A control for picking only a time of day. The initial value is the current time.
    pub fn time_picker(&self) -> Result<TimePicker> {
        TimePicker::new()
    }

    /// A control for picking a color. The initial value is black.
    pub fn color_picker(&self) -> Result<ColorPicker> {
        ColorPicker::new()
    }

    /// A surface the app draws on itself. Set its size with `set_sizing`.
    pub fn canvas(&self) -> Result<Canvas> {
        Canvas::new()
    }

    pub fn text_input(&self, text: &str) -> Result<TextInput> {
        TextInput::new(text)
    }

    /// A field whose input is masked. Backed by `PasswordBox`.
    pub fn password_input(&self) -> Result<PasswordInput> {
        PasswordInput::new()
    }

    /// A search field. Backed by `AutoSuggestBox`.
    pub fn search_input(&self) -> Result<SearchInput> {
        SearchInput::new()
    }

    /// A field for entering a number. `value` is the initial value.
    pub fn number_input(&self, value: f64) -> Result<NumberInput> {
        NumberInput::new(value)
    }

    /// A field for entering text that includes newlines. Set its height with
    /// `set_sizing`.
    pub fn text_area(&self, text: &str) -> Result<TextArea> {
        TextArea::new(text)
    }

    pub fn slider(&self, min: f64, max: f64) -> Result<Slider> {
        Slider::new(min, max)
    }

    pub fn progress_bar(&self) -> Result<ProgressBar> {
        ProgressBar::new()
    }

    /// A spinning indicator for work in progress. Spins from creation.
    pub fn spinner(&self) -> Result<Spinner> {
        Spinner::new()
    }

    /// Tabs. Holds each tab's content widget.
    pub fn tabs(&self) -> Result<Tabs> {
        Tabs::new()
    }

    /// Horizontal navigation placed at the top of the screen. `title` is the heading
    /// at the left end.
    pub fn navbar(&self, title: &str) -> Result<Navbar> {
        Navbar::new(title)
    }

    /// Horizontal navigation placed at the bottom of the screen.
    pub fn dock(&self) -> Result<Dock> {
        Dock::new()
    }

    /// A toolbar attached to the top of the window.
    ///
    /// Attach it with [`Window::set_toolbar`]. The framework keeps a reference to it,
    /// so dropping the return value does not stop notifications.
    pub fn toolbar(&self) -> Result<Toolbar> {
        let toolbar = Toolbar::new()?;
        self.0.toolbars.borrow_mut().push(toolbar.clone());
        Ok(toolbar)
    }

    /// The OS's application menu, attached to the top of the window.
    ///
    /// Attach it with [`Window::set_menu_bar`]. The framework keeps a reference to it,
    /// so dropping the return value does not stop notifications.
    pub fn menu_bar(&self) -> Result<MenuBar> {
        let menu_bar = MenuBar::new()?;
        self.0.menu_bars.borrow_mut().push(menu_bar.clone());
        Ok(menu_bar)
    }

    /// A sidebar attached to the left of the window.
    ///
    /// Attach it with [`Window::set_sidebar`]. The framework keeps a reference to it,
    /// so dropping the return value does not stop notifications.
    pub fn sidebar(&self) -> Result<Sidebar> {
        let sidebar = Sidebar::new()?;
        self.0.sidebars.borrow_mut().push(sidebar.clone());
        Ok(sidebar)
    }

    /// A vertical list of navigation items.
    pub fn menu(&self) -> Result<Menu> {
        Menu::new()
    }

    /// A list of selectable rows. It scrolls by itself.
    pub fn list(&self) -> Result<List> {
        List::new()
    }

    /// A table with column headings. It scrolls by itself.
    pub fn table(&self) -> Result<Table> {
        Table::new()
    }

    /// A list whose nested items can be expanded and collapsed. It scrolls by itself.
    pub fn tree(&self) -> Result<Tree> {
        Tree::new()
    }

    /// A popup (context) menu shown on right-click.
    ///
    /// The framework keeps a reference to it, so dropping the return value does not
    /// remove it from the widgets it is attached to.
    pub fn popup_menu(&self) -> Result<PopupMenu> {
        let popup = PopupMenu::new()?;
        self.0.popups.borrow_mut().push(popup.clone());
        Ok(popup)
    }

    /// Breadcrumbs.
    pub fn breadcrumbs(&self) -> Result<Breadcrumbs> {
        Breadcrumbs::new()
    }

    /// Pagination. `page_count` is the number of pages.
    pub fn pagination(&self, page_count: usize) -> Result<Pagination> {
        Pagination::new(page_count)
    }

    /// A link. If `href` is not empty, clicking it opens it in the browser.
    pub fn link(&self, text: &str, href: &str) -> Result<Link> {
        Link::new(text, href)
    }

    /// An image. `source` is a file path or a URL.
    pub fn image(&self, source: &str) -> Result<Image> {
        Image::new(source)
    }

    /// Video. `source` is a file path or a URL.
    pub fn video(&self, source: &str) -> Result<Video> {
        Video::new(source)
    }

    /// Audio. `source` is a file path or a URL.
    pub fn audio(&self, source: &str) -> Result<Audio> {
        Audio::new(source)
    }

    /// A button for picking files or folders. Clicking it shows the common dialog.
    pub fn file_picker(&self, text: &str) -> Result<FilePicker> {
        FilePicker::new(text)
    }

    /// A button that lets the user save contents to a file. Clicking it shows the save
    /// mode of the common dialog.
    pub fn file_saver(&self, text: &str) -> Result<FileSaver> {
        FileSaver::new(text)
    }

    /// A transient notification (toast). `message` is the string to show.
    ///
    /// The framework keeps a reference to it, so dropping the return value does not
    /// stop notifications.
    pub fn toast(&self, message: &str) -> Result<Toast> {
        let toast = Toast::new(message)?;
        self.0.toasts.borrow_mut().push(toast.clone());
        Ok(toast)
    }

    /// A modal dialog. `title` is the heading. Backed by `ContentDialog`.
    ///
    /// The framework keeps a reference to it, so dropping the return value does not
    /// stop notifications.
    pub fn dialog(&self, title: &str) -> Result<Dialog> {
        let d = Dialog::new(title, self.0.theme.get())?;
        self.0.dialogs.borrow_mut().push(d.clone());
        Ok(d)
    }

    /// Switches the color theme at runtime.
    pub fn set_theme(&self, theme: Theme) -> Result<()> {
        for window in self.0.windows.borrow().iter() {
            window.set_theme(theme)?;
        }
        // ダイアログはウィンドウとは別の層に出るので、個別に伝える。
        for dialog in self.0.dialogs.borrow().iter() {
            dialog.set_theme(theme);
        }
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
    /// string cannot be read as a URL or there is nothing to open it with. Failures
    /// that happen in whatever opens it are not reported.
    pub fn open_url(&self, url: &str) -> Result<()> {
        use windows::Win32::UI::Shell::ShellExecuteW;
        use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
        use windows_core::{w, HSTRING};

        if url.trim().is_empty() || !url.contains(':') {
            return Err(Error::new(
                "URL を開く",
                format!("URL として読めません: {url}"),
            ));
        }
        let target = HSTRING::from(url);
        // 32 以下はエラーの番号 (`ShellExecute` の古い約束)。
        let result = unsafe { ShellExecuteW(None, w!("open"), &target, None, None, SW_SHOWNORMAL) };
        if result.0 as isize > 32 {
            Ok(())
        } else {
            Err(Error::new("URL を開く", format!("開けませんでした: {url}")))
        }
    }

    /// Quits the app.
    pub fn quit(&self) {
        // `Application::Exit` でもウィンドウは通知を上げずに畳まれるので、
        // 取り付けた Mica のコントローラーはここで閉じる。
        for window in self.0.windows.borrow().iter() {
            window.release_backdrop();
        }
        release_widget_handlers();
        if let Ok(app) = naui_winui3::Microsoft::UI::Xaml::Application::Current() {
            let _ = app.Exit();
        }
    }
}

/// Releases the `on_key_down`, `on_file_drop` and `on_focus_change` handlers.
///
/// The `on_key_down` and `on_file_drop` handlers are held by a `thread_local!` table rather than by the
/// elements, so they remain even after the XAML tree is removed. Unless the shutdown
/// path goes through here, the widgets captured by the closures are not released
/// until the process ends (see [`ui_thread::Registry`]). The `on_focus_change`
/// handlers are held by the widgets, but the focus watcher holds on to the last
/// owner.
fn release_widget_handlers() {
    keys::release_handlers();
    file_drop::release_handlers();
    focus::release_handlers();
}

/// The user's preferred UI languages, most preferred first (`"ja-JP"` and so on).
///
/// Read with `GetUserPreferredUILanguages`, which works for unpackaged apps too.
/// Empty if it fails, which leaves naui's strings in English.
fn preferred_languages() -> Vec<String> {
    use windows::Win32::Globalization::{GetUserPreferredUILanguages, MUI_LANGUAGE_NAME};

    let mut count = 0u32;
    let mut len = 0u32;
    // 1 回目で必要な長さを聞き、2 回目で受け取る。中身は NUL 区切りで、
    // 最後に NUL が 2 つ続く。
    if unsafe { GetUserPreferredUILanguages(MUI_LANGUAGE_NAME, &mut count, None, &mut len) }
        .is_err()
    {
        return Vec::new();
    }
    let mut buffer = vec![0u16; len as usize];
    if unsafe {
        GetUserPreferredUILanguages(
            MUI_LANGUAGE_NAME,
            &mut count,
            Some(windows_core::PWSTR(buffer.as_mut_ptr())),
            &mut len,
        )
    }
    .is_err()
    {
        return Vec::new();
    }
    buffer
        .split(|&c| c == 0)
        .filter(|tag| !tag.is_empty())
        .map(String::from_utf16_lossy)
        .collect()
}

/// Starts the app and builds the UI inside `build`.
///
/// After attaching the Windows App SDK runtime (the bundled files with
/// `self-contained`, otherwise trying versions newest first as described in the
/// crate docs), it enters `Application::Start` and calls `build` once XAML
/// initialization has finished. This function does not return until the app exits.
pub fn run<F>(settings: Settings, build: F) -> Result<()>
where
    F: FnOnce(&Ui) -> Result<()> + 'static,
{
    use naui_winui3::Microsoft::UI::Xaml::{Application, ApplicationInitializationCallback};
    use naui_winui3::{init_apartment, ApartmentType};

    let _runtime = sdk::initialize()?;
    init_apartment(ApartmentType::SingleThreaded)
        .map_err(|e| to_error("COM アパートメントの初期化", e))?;

    let failure: &'static ui_thread::UiThreadCell<Option<Error>> =
        Box::leak(Box::new(ui_thread::UiThreadCell::new(None)));
    let state: &'static ui_thread::UiThreadCell<Option<F>> =
        Box::leak(Box::new(ui_thread::UiThreadCell::new(Some(build))));
    let ui_state: &'static ui_thread::UiThreadCell<Option<Ui>> =
        Box::leak(Box::new(ui_thread::UiThreadCell::new(None)));

    Application::Start(&ApplicationInitializationCallback::new(move |_| {
        let app = app::compose(state, failure, ui_state, settings.theme)?;
        std::mem::forget(app);
        Ok(())
    }))
    .map_err(|e| to_error("Application::Start", e))?;

    match failure.with_mut_cross_thread(|slot| slot.take()) {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

/// Calls `build` exactly once when WinUI 3 initialization has finished, and ends
/// the app when it returns. **For automated tests only.**
///
/// Real apps should use [`run`]. Unlike AppKit or the DOM, WinUI 3 cannot create a
/// single control before `Application::Start`. Moreover, `Application::Start` can be
/// called only once per process, so tests take the form "start the app, run every
/// case inside it, then wind it down". This is where it differs from the macOS
/// version (`naui_macos::run_for_test`), which can start over per case.
#[doc(hidden)]
pub fn run_for_test<F>(build: F) -> Result<()>
where
    F: FnOnce(&Ui) -> Result<()> + 'static,
{
    run(Settings::new("naui tests"), move |ui| {
        let result = build(ui);
        // 失敗したときは `run` の側が畳むので、二重に呼ばない。
        if result.is_ok() {
            // 実際のアプリでは AppWindow の `Closing` がこの後片づけをする。
            // テストは `OnLaunched` の中で畳むので、その時点ではまだ `Ui` が
            // 置き場へ入っておらず `Closing` から届かない。XAML のツリーが
            // 壊される前に中身を外さないと、終了時にアクセス違反になる。
            ui.clear_windows_for_shutdown();
            // 畳むのはイベントループへ入ってから。`OnLaunched` の中で
            // `Application::Exit` を呼ぶと、ループが始まる前に XAML の
            // 後片づけが走り、終了時にアクセス違反になる。
            //
            // `build` が積んだ仕事のほうが先に入っているので、そちらが
            // 終わってからここへ来る。
            let deferred = ui.clone();
            let quit = ui.tasks().channel(move |()| deferred.quit());
            if quit.send(()).is_err() {
                // 積めなかったときは、その場で畳むしかない。
                ui.quit();
            }
        }
        result
    })
}
