//! # naui-gtk
//!
//! naui's Linux backend. It creates **real GTK4 / libadwaita controls**
//! (`GtkApplicationWindow` / `GtkButton` / `GtkEntry` / `GtkScale` /
//! `GtkListBox` …) and relays GTK4 signals to Rust closures.
//!
//! Drawing, layout, IME, accessibility and following the OS theme are all done by
//! GTK4.
//!
//! ## Mapping
//!
//! | naui | GTK4 / libadwaita |
//! | --- | --- |
//! | `run` | `AdwApplication` + `connect_activate` (the UI is built in the callback) |
//! | `Window` | `AdwApplicationWindow` + `AdwToolbarView` + `AdwHeaderBar` |
//! | `Stack` | `GtkBox` |
//! | `Grid` | `GtkGrid` |
//! | `Scroll` | `GtkScrolledWindow` |
//! | `Expander` | `GtkExpander` |
//! | `SplitView` | `GtkPaned` |
//! | `Spacer` | An empty `GtkBox` (`hexpand` / `vexpand`) |
//! | Size specification | `size_request` / `hexpand` / `halign` + the maximum of [`SizeBin`] |
//! | `Label` | `GtkLabel` |
//! | `Button` | `GtkButton` |
//! | `Checkbox` | `GtkCheckButton` |
//! | `Toggle` | A `GtkSwitch` and a `GtkLabel` side by side in a `GtkBox` |
//! | `ComboBox` | `GtkDropDown` + `GtkStringList` |
//! | `EditableComboBox` | `GtkEntry` + `GtkMenuButton` (a popover with a `GtkListBox`) |
//! | `RadioGroup` | Grouped `GtkCheckButton`s laid out in a `GtkBox` |
//! | `DatePicker` | A `GtkMenuButton` + `GtkCalendar` / `GtkSpinButton` combination |
//! | `TimePicker` | `GtkSpinButton`s for hours and minutes separated by `:` |
//! | `ColorPicker` | `GtkColorDialogButton` + `GtkColorDialog` |
//! | `Canvas` | `GtkDrawingArea` (drawn with cairo + Pango) |
//! | `TextInput` | `GtkEntry` |
//! | `TextArea` | A `GtkTextView` in a `GtkScrolledWindow` |
//! | `Slider` | `GtkScale` |
//! | `ProgressBar` | `GtkProgressBar` |
//! | `Spinner` | `AdwSpinner` (`GtkSpinner` with libadwaita older than 1.6) |
//! | `Tabs` | `GtkNotebook` |
//! | `Navbar` / `Dock` / `Menu` / `Pagination` | A row of `GtkToggleButton`s |
//! | `Breadcrumbs` | A `GtkLabel` with links (`<a href>`) + separators |
//! | `PopupMenu` | `GtkPopoverMenu` + `GMenu` + `GSimpleAction` |
//! | `MenuBar` | `GtkPopoverMenuBar` + `GMenu` + `GSimpleAction` |
//! | `List` | A `GtkListBox` in a `GtkScrolledWindow` |
//! | `Link` | `GtkLinkButton` |
//! | `Image` | `GtkPicture` |
//! | `Video` | `GtkPicture` (showing a `GtkMediaFile`) + `GtkMediaControls` |
//! | `Audio` | `GtkMediaControls` + `GtkMediaFile` |
//! | `FilePicker` | `GtkButton` + `GtkFileDialog` |
//! | `Dialog` | `AdwAlertDialog` |
//! | `Toast` | `AdwToast` + `AdwToastOverlay` |
//!
//! ## Differences from other backends
//!
//! - **The `Video` playback bar can be shown and hidden**, because it is built from
//!   `GtkPicture` + `GtkMediaControls` rather than `GtkVideo`; fitting
//!   ([`Fit`](naui_core::Fit)) also works only in this form.
//! - GTK4 has no equivalent of [`Fit::None`](naui_core::Fit::None) (actual size),
//!   so it maps to `GTK_CONTENT_FIT_SCALE_DOWN`, which **does not scale up but does
//!   scale down**.
//! - **Weights of [`Track::Fill`](naui_core::Track::Fill) in a `Grid` have no
//!   effect.** `GtkGrid` cannot give columns or rows a width of their own, and
//!   leftover space is split evenly among the growing columns (the same limitation
//!   as macOS).
//! - `AdwStyleManager` holds the color theme for the whole app, so
//!   [`Window::set_theme`] also applies to the whole app.
//! - Unlike `GtkApplicationWindow`, `AdwApplicationWindow` **has no default title
//!   bar**, so an `AdwHeaderBar` is added by hand. It provides the minimize,
//!   maximize and close buttons (which side they are on follows the desktop's
//!   settings).
//! - **When unmuting, naui writes back the volume it holds.** When muted,
//!   `GtkMediaControls` moves the volume slider to 0 and writes that 0 back to the
//!   `GtkMediaStream`, but GTK4 does not restore the volume on unmute. Without this,
//!   there would be no sound even after unmuting.
//! - Which formats can be played depends on the plugins installed for GStreamer
//!   (because `GtkMediaFile` is built on GStreamer).
//! - With multiple selection in a `List`, **`GtkListBox`'s "activate on single
//!   click" is turned off** (`gtk_list_box_set_activate_on_single_click`). While it
//!   is on, `GtkListBox` ignores Ctrl / Shift on clicks and always falls back to
//!   "select only that row", so rows could be neither added nor removed.
//! - **`Tabs` lets the tab strip scroll** (`gtk_notebook_set_scrollable`). By
//!   default `GtkNotebook` reports "the width to lay out every tab side by side" as
//!   its minimum width, so as tabs are added the window can no longer be shrunk
//!   below it.
//! - **`DatePicker` does not close its calendar when a day is clicked.**
//!   `GtkCalendar` does not distinguish "clicked a day" from "moved to another
//!   month"; both arrive as `day-selected`, so closing on every click would make
//!   changing months impossible.
//! - `Checkbox` and `RadioGroup` **move the indicator to the center of the label's
//!   glyphs**. GTK4 places the indicator at the center of the line box (ascent +
//!   descent), but lines containing Japanese get a larger ascent, pushing the glyphs
//!   down so that only the indicator appears to float (see the `indicator` module
//!   for details).
//!
//! ## How small a window can be shrunk
//!
//! A GTK4 window **cannot be made smaller than the minimum size its content
//! reports**. So axes with [`Length::Fill`](naui_core::Length::Fill) report a
//! minimum of 0 from `measure`. `Fill` means "the parent decides the size", so the
//! content must not decide the window's minimum for its own sake (the same reason
//! the Web backend writes `min-width: 0`).
//!
//! To keep it from shrinking too far, use
//! [`Sizing::min_width`](naui_core::Sizing::min_width) and friends, which GTK4
//! applies again as a minimum through `gtk_widget_set_size_request`.
//!
//! GTK4 does not clip overflowing content by default, so shrinking would draw it
//! beyond the window. So two places are set to `GTK_OVERFLOW_HIDDEN`:
//!
//! - the window's content (`AdwToolbarView`)
//! - widgets with an axis set to `Fill`
//!
//! Having reported a minimum of 0, the proper thing is not to draw beyond the
//! allotted space, the same as pairing CSS's `min-width: 0` with
//! `overflow: hidden`.
//!
//! GTK4 signal handlers take `'static` closures, so the same `Rc<Inner>` +
//! held-closure pattern as macOS / Web works as is (there is no `Send + Sync`
//! constraint as on Windows). The only exception is passing values from other
//! threads, which go back to the main loop through `glib::idle_add_once`
//! (`crate::main_thread`).

#![cfg(all(
    unix,
    not(any(target_os = "macos", target_os = "ios", target_os = "android"))
))]
// glib のサブクラス化マクロだけが `unsafe impl` を必要とする (crate::bin)。
#![deny(unsafe_code)]

mod bin;
mod callback;
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
mod indicator;
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
mod tree;
mod widgets;
mod window;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::gio;
use gtk::glib;
use gtk::prelude::*;
use naui_core::{DatePickerMode, Error, Orientation, Result, Settings, Tasks, Theme};

pub use bin::SizeBin;
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

/// Applies a color theme to the whole app.
///
/// libadwaita has only one theme per app, so it cannot be held per window.
pub(crate) fn apply_theme(theme: Theme) {
    adw::StyleManager::default().set_color_scheme(match theme {
        Theme::System => adw::ColorScheme::Default,
        Theme::Light => adw::ColorScheme::ForceLight,
        Theme::Dark => adw::ColorScheme::ForceDark,
    });
}

/// The languages for naui's own strings, most preferred first.
///
/// Chosen the way gettext chooses GTK's own strings, so the two agree: `LANGUAGE`,
/// `LC_ALL`, `LC_MESSAGES` and `LANG` in that order, except that `LANGUAGE` is
/// ignored while the message locale is C.
fn preferred_languages() -> Vec<String> {
    // メッセージのロケールは LC_ALL・LC_MESSAGES・LANG のうち最初の空でないもの。
    let locale = ["LC_ALL", "LC_MESSAGES", "LANG"]
        .into_iter()
        .filter_map(|name| std::env::var(name).ok())
        .find(|value| !value.is_empty())
        .unwrap_or_default();
    languages_for_locale(&locale, glib::language_names().iter().map(|n| n.as_str()))
}

/// `language_names` (glib's "ja_JP.UTF-8", "ja_JP", "ja", "C" and so on) unless
/// the message locale `locale` is C, in which case only "C".
fn languages_for_locale<'a>(
    locale: &str,
    language_names: impl Iterator<Item = &'a str>,
) -> Vec<String> {
    // glib は C ロケールでも LANGUAGE を読むが、gettext は読まずに翻訳しない。
    // "C.UTF-8" も gettext は C として扱う。
    let is_c = locale.is_empty() || locale == "POSIX" || locale.split('.').next() == Some("C");
    if is_c {
        vec!["C".to_string()]
    } else {
        language_names.map(str::to_string).collect()
    }
}

/// The entry point for creating widgets.
///
/// GTK4 can only create windows after the `GtkApplication` has started, so a `Ui`
/// is available only inside the [`run`] callback.
///
/// Like widget handles, **clones share the same contents**, so moving one into a
/// callback lets you create widgets later. It is main-thread only and not `Send`,
/// so it cannot be passed to other threads (use [`Tasks`] for that).
#[derive(Clone)]
pub struct Ui(Rc<UiInner>);

struct UiInner {
    app: adw::Application,
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
    fn new(app: adw::Application, theme: Theme) -> Self {
        naui_core::set_ui_languages(preferred_languages());
        Self(Rc::new(UiInner {
            app,
            theme: Cell::new(theme),
            windows: RefCell::new(Vec::new()),
            dialogs: RefCell::new(Vec::new()),
            popups: RefCell::new(Vec::new()),
            toolbars: RefCell::new(Vec::new()),
            menu_bars: RefCell::new(Vec::new()),
            sidebars: RefCell::new(Vec::new()),
            toasts: RefCell::new(Vec::new()),
            tasks: Tasks::from_main_thread(std::sync::Arc::new(main_thread::Idle)),
        }))
    }

    /// The corresponding `GtkApplication`. Public as a backend-specific escape hatch.
    pub fn native_application(&self) -> adw::Application {
        self.0.app.clone()
    }

    /// Creates a window. The framework keeps a reference to it, so dropping the return
    /// value does not close it.
    ///
    /// `width` / `height` are the size of the content area (the same as
    /// [`Window::set_size`]), excluding the header bar and menu bar.
    pub fn window(&self, title: &str, width: f64, height: f64) -> Result<Window> {
        let window = Window::new(&self.0.app, title, width, height);
        self.0.windows.borrow_mut().push(window.clone());
        Ok(window)
    }

    pub fn stack(&self, orientation: Orientation) -> Result<Stack> {
        Ok(Stack::new(orientation))
    }

    /// A container that positions children by row and column.
    pub fn grid(&self) -> Result<Grid> {
        Ok(Grid::new())
    }

    /// A container that scrolls when its contents overflow.
    pub fn scroll(&self) -> Result<Scroll> {
        Ok(Scroll::new())
    }

    /// A container whose contents are shown and hidden by clicking its heading. `text`
    /// is the heading text.
    pub fn expander(&self, text: &str) -> Result<Expander> {
        Ok(Expander::new(text))
    }

    /// A container that divides two panes with a movable divider.
    ///
    /// With `Horizontal`, the panes are side by side and the divider is vertical.
    pub fn split_view(&self, orientation: Orientation) -> Result<SplitView> {
        Ok(SplitView::new(orientation))
    }

    /// A widget that is just empty space. It absorbs a stack's leftover space and
    /// pushes the others away.
    pub fn spacer(&self) -> Result<Spacer> {
        Ok(Spacer::new())
    }

    pub fn label(&self, text: &str) -> Result<Label> {
        Ok(Label::new(text))
    }

    pub fn button(&self, text: &str) -> Result<Button> {
        Ok(Button::new(text))
    }

    pub fn checkbox(&self, label: &str) -> Result<Checkbox> {
        Ok(Checkbox::new(label))
    }

    /// An on/off switch. `label` is the text placed next to it.
    pub fn toggle(&self, label: &str) -> Result<Toggle> {
        Ok(Toggle::new(label))
    }

    /// A combo box that shows its options collapsed.
    pub fn combo_box(&self) -> Result<ComboBox> {
        Ok(ComboBox::new())
    }

    /// A combo box where the user can either choose from suggestions or type freely.
    pub fn editable_combo_box(&self) -> Result<EditableComboBox> {
        Ok(EditableComboBox::new())
    }

    /// A radio group that lays out options and lets the user choose exactly one.
    pub fn radio_group(&self) -> Result<RadioGroup> {
        Ok(RadioGroup::new())
    }

    /// A control for picking a date or time. `mode` decides what can be picked.
    pub fn date_picker(&self, mode: DatePickerMode) -> Result<DatePicker> {
        Ok(DatePicker::new(mode))
    }

    /// A control for picking only a time of day. The initial value is the current time.
    pub fn time_picker(&self) -> Result<TimePicker> {
        Ok(TimePicker::new())
    }

    /// A control for picking a color. The initial value is black.
    pub fn color_picker(&self) -> Result<ColorPicker> {
        Ok(ColorPicker::new())
    }

    /// A surface the app draws on itself. Set its size with `set_sizing`.
    pub fn canvas(&self) -> Result<Canvas> {
        Ok(Canvas::new())
    }

    pub fn text_input(&self, text: &str) -> Result<TextInput> {
        Ok(TextInput::new(text))
    }

    /// A field whose input is masked. Backed by `GtkPasswordEntry`.
    pub fn password_input(&self) -> Result<PasswordInput> {
        Ok(PasswordInput::new())
    }

    /// A search field. Backed by `GtkSearchEntry`.
    pub fn search_input(&self) -> Result<SearchInput> {
        Ok(SearchInput::new())
    }

    /// A field for entering a number. `value` is the initial value.
    pub fn number_input(&self, value: f64) -> Result<NumberInput> {
        Ok(NumberInput::new(value))
    }

    /// A field for entering text that includes newlines. Set its height with
    /// `set_sizing`.
    pub fn text_area(&self, text: &str) -> Result<TextArea> {
        Ok(TextArea::new(text))
    }

    pub fn slider(&self, min: f64, max: f64) -> Result<Slider> {
        Ok(Slider::new(min, max))
    }

    pub fn progress_bar(&self) -> Result<ProgressBar> {
        Ok(ProgressBar::new())
    }

    /// A spinning indicator for work in progress. Spins from creation.
    pub fn spinner(&self) -> Result<Spinner> {
        Ok(Spinner::new())
    }

    /// An image. `source` is a file path or a URL.
    pub fn image(&self, source: &str) -> Result<Image> {
        Ok(Image::new(source))
    }

    /// Video. `source` is a file path or a URL.
    pub fn video(&self, source: &str) -> Result<Video> {
        Ok(Video::new(source))
    }

    /// Audio. `source` is a file path or a URL.
    pub fn audio(&self, source: &str) -> Result<Audio> {
        Ok(Audio::new(source))
    }

    /// Tabs. Holds each tab's content widget.
    pub fn tabs(&self) -> Result<Tabs> {
        Ok(Tabs::new())
    }

    /// Horizontal navigation placed at the top of the screen. `title` is the heading
    /// at the left end.
    pub fn navbar(&self, title: &str) -> Result<Navbar> {
        Ok(Navbar::new(title))
    }

    /// Horizontal navigation placed at the bottom of the screen (equal widths).
    pub fn dock(&self) -> Result<Dock> {
        Ok(Dock::new())
    }

    /// A vertical list of navigation items.
    pub fn menu(&self) -> Result<Menu> {
        Ok(Menu::new())
    }

    /// A toolbar attached to the top of the window.
    ///
    /// Attach it with [`Window::set_toolbar`]. The framework keeps a reference to it,
    /// so dropping the return value does not stop notifications.
    pub fn toolbar(&self) -> Result<Toolbar> {
        let toolbar = Toolbar::new();
        self.0.toolbars.borrow_mut().push(toolbar.clone());
        Ok(toolbar)
    }

    /// The OS's application menu, attached to the top of the window.
    ///
    /// Attach it with [`Window::set_menu_bar`]. The framework keeps a reference to it,
    /// so dropping the return value does not stop notifications.
    pub fn menu_bar(&self) -> Result<MenuBar> {
        let menu_bar = MenuBar::new(&self.0.app);
        self.0.menu_bars.borrow_mut().push(menu_bar.clone());
        Ok(menu_bar)
    }

    /// A sidebar attached to the left of the window.
    ///
    /// Attach it with [`Window::set_sidebar`]. The framework keeps a reference to it,
    /// so dropping the return value does not stop notifications.
    pub fn sidebar(&self) -> Result<Sidebar> {
        let sidebar = Sidebar::new();
        self.0.sidebars.borrow_mut().push(sidebar.clone());
        Ok(sidebar)
    }

    /// A list of selectable rows. It scrolls by itself.
    pub fn list(&self) -> Result<List> {
        Ok(List::new())
    }

    /// A table with column headings. It scrolls by itself.
    pub fn table(&self) -> Result<Table> {
        Ok(Table::new())
    }

    /// A list whose nested items can be expanded and collapsed. It scrolls by itself.
    pub fn tree(&self) -> Result<Tree> {
        Ok(Tree::new())
    }

    /// A popup (context) menu shown on right-click.
    ///
    /// The framework keeps a reference to it, so dropping the return value does not
    /// remove it from the widgets it is attached to.
    pub fn popup_menu(&self) -> Result<PopupMenu> {
        let popup = PopupMenu::new();
        self.0.popups.borrow_mut().push(popup.clone());
        Ok(popup)
    }

    /// Breadcrumbs.
    pub fn breadcrumbs(&self) -> Result<Breadcrumbs> {
        Ok(Breadcrumbs::new())
    }

    /// Pagination. `page_count` is the number of pages.
    pub fn pagination(&self, page_count: usize) -> Result<Pagination> {
        Ok(Pagination::new(page_count))
    }

    /// A link. If `href` is not empty, clicking it opens it with the default handler.
    pub fn link(&self, text: &str, href: &str) -> Result<Link> {
        Ok(Link::new(text, href))
    }

    /// A button for picking files or folders. Clicking it shows a `GtkFileDialog`.
    pub fn file_picker(&self, text: &str) -> Result<FilePicker> {
        Ok(FilePicker::new(text))
    }

    /// A button that lets the user save contents to a file. Clicking it shows the save
    /// mode of `GtkFileDialog`.
    pub fn file_saver(&self, text: &str) -> Result<FileSaver> {
        Ok(FileSaver::new(text))
    }

    /// A transient notification (toast). `message` is the string to show.
    ///
    /// The framework keeps a reference to it, so dropping the return value does not
    /// stop notifications.
    pub fn toast(&self, message: &str) -> Result<Toast> {
        let toast = Toast::new(&self.0.app, message);
        self.0.toasts.borrow_mut().push(toast.clone());
        Ok(toast)
    }

    /// A modal dialog. The framework keeps a reference to it.
    pub fn dialog(&self, title: &str) -> Result<Dialog> {
        let dialog = Dialog::new(&self.0.app, title);
        self.0.dialogs.borrow_mut().push(dialog.clone());
        Ok(dialog)
    }

    /// Switches the color theme.
    pub fn set_theme(&self, theme: Theme) -> Result<()> {
        self.0.theme.set(theme);
        apply_theme(theme);
        Ok(())
    }

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
        Clipboard
    }

    /// Opens a URL with the default app (the browser for `https:`, mail for
    /// `mailto:`).
    ///
    /// Opening is left to the OS, so this returns right away. Returns `Err` when the
    /// string cannot be read as a URL or there is nothing to open it with. Failures
    /// that happen in whatever opens it are not reported.
    ///
    /// GTK4 opens it asynchronously (`GtkUriLauncher`), so what this returns is only
    /// whether it could be read as a URL.
    pub fn open_url(&self, url: &str) -> Result<()> {
        if gtk::glib::Uri::parse(url, gtk::glib::UriFlags::NONE).is_err() {
            return Err(Error::new(
                "open the URL",
                format!("not a valid URL: {url}"),
            ));
        }
        let parent = self.0.app.active_window();
        gtk::UriLauncher::new(url).launch(parent.as_ref(), None::<&gtk::gio::Cancellable>, |_| {});
        Ok(())
    }

    /// Quits the app.
    pub fn quit(&self) {
        self.0.app.quit();
    }
}

thread_local! {
    /// The `GtkApplication` reused by [`run_for_test`].
    static TEST_APP: RefCell<Option<adw::Application>> = const { RefCell::new(None) };
}

thread_local! {
    /// Keeps the `Ui` alive even after the `run` callback finishes.
    ///
    /// Widget closures refer to handles held by the `Ui` (dialogs, popups), so it
    /// cannot be dropped at the end of the callback.
    static KEEP_ALIVE: RefCell<Vec<Ui>> = const { RefCell::new(Vec::new()) };
}

/// Creates only a `Ui` without running the main loop. **For automated tests only.**
///
/// GTK4 widgets cannot be created before `gtk_init`, so only initialization and
/// registration of the `GtkApplication` are done, and `build` is called directly.
#[doc(hidden)]
pub fn run_for_test<F>(build: F) -> Result<()>
where
    F: FnOnce(&Ui) -> Result<()>,
{
    if !gtk::is_initialized() {
        adw::init().map_err(|e| Error::new("start for testing", e.to_string()))?;
        // メインループを回さないのでフレームクロックが進まない。`GtkSwitch` の
        // ような、切り替えをアニメーションで見せるウィジェットは、アニメーション
        // が入ったままだと値が変わらないままになる。デスクトップの設定に
        // 左右されないよう、テストの間だけ切っておく。
        if let Some(settings) = gtk::Settings::default() {
            settings.set_gtk_enable_animations(false);
        }
    }
    // `GtkApplication` は 1 プロセスに 1 つ。登録は同じオブジェクトパスを
    // 使うので、ケースごとに作り直すと 2 回目の登録で失敗する。
    let app = TEST_APP.with(|slot| -> Result<adw::Application> {
        if let Some(app) = slot.borrow().as_ref() {
            return Ok(app.clone());
        }
        let app = adw::Application::builder()
            .application_id("org.naui.test")
            // 既存プロセスへ起動要求が転送されないようにする。
            .flags(gio::ApplicationFlags::NON_UNIQUE)
            .build();
        app.register(gio::Cancellable::NONE)
            .map_err(|e| Error::new("start for testing", e.to_string()))?;
        *slot.borrow_mut() = Some(app.clone());
        Ok(app)
    })?;
    let ui = Ui::new(app, Theme::System);
    build(&ui)
}

/// Starts the app and builds the UI inside `build`.
///
/// `build` is called exactly once, inside the `GtkApplication`'s `activate`. This
/// function does not return until every window has closed.
pub fn run<F>(settings: Settings, build: F) -> Result<()>
where
    F: FnOnce(&Ui) -> Result<()> + 'static,
{
    // GTK4 のアプリ ID は書き方が決まっている。不正なまま渡すと GLib が
    // 落ちるので、ここで弾いて naui のエラーとして返す。
    if !gio::Application::id_is_valid(&settings.app_id) {
        return Err(Error::new(
            "start the app on Linux",
            format!(
                "GTK4 does not accept the app ID `{}`. \
                 Give `Settings::app_id` in reverse-domain form (e.g. com.example.myapp)",
                settings.app_id
            ),
        ));
    }

    let app = adw::Application::builder()
        .application_id(&settings.app_id)
        .build();

    let theme = settings.theme;
    let build = RefCell::new(Some(build));
    let failure: Rc<RefCell<Option<Error>>> = Rc::new(RefCell::new(None));
    {
        let failure = failure.clone();
        app.connect_activate(move |app| {
            let Some(build) = build.borrow_mut().take() else {
                // 2 回目以降の activate (既存プロセスへの起動要求)。
                return;
            };
            apply_theme(theme);
            let ui = Ui::new(app.clone(), theme);
            match build(&ui) {
                Ok(()) => KEEP_ALIVE.with(|slot| slot.borrow_mut().push(ui)),
                Err(error) => {
                    *failure.borrow_mut() = Some(error);
                    app.quit();
                }
            }
        });
    }

    // コマンドライン引数は naui の API に無いので、GTK4 へは渡さない。
    let code = app.run_with_args::<&str>(&[]);
    // メインループが終わった後は、投函しても誰も取り出さない。
    // 送信側へ失敗を返せるようにし、受信クロージャと future を解放する。
    KEEP_ALIVE.with(|slot| {
        let alive = std::mem::take(&mut *slot.borrow_mut());
        for ui in &alive {
            ui.0.tasks.shutdown();
        }
    });

    if let Some(error) = failure.borrow_mut().take() {
        return Err(error);
    }
    if code != glib::ExitCode::SUCCESS {
        return Err(Error::new(
            "start the app on Linux",
            format!("GtkApplication exited with {code:?}"),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn c_locale_ignores_language() {
        // LANGUAGE=ja LANG=C のとき glib は ["ja", "C"] を返す。
        for locale in ["C", "C.UTF-8", "C.utf8", "POSIX", ""] {
            assert_eq!(
                languages_for_locale(locale, ["ja", "C"].into_iter()),
                ["C"],
                "{locale:?}"
            );
        }
    }

    #[test]
    fn other_locales_keep_glibs_order() {
        assert_eq!(
            languages_for_locale("en_US.UTF-8", ["fr", "ja", "C"].into_iter()),
            ["fr", "ja", "C"],
            "LANGUAGE があればロケールより先"
        );
        assert_eq!(
            languages_for_locale(
                "ja_JP.UTF-8",
                ["ja_JP.UTF-8", "ja_JP", "ja", "C"].into_iter()
            ),
            ["ja_JP.UTF-8", "ja_JP", "ja", "C"]
        );
    }
}
