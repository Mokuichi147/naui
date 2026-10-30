use crate::sys;
use crate::{
    Audio, Breadcrumbs, Button, Canvas, Checkbox, Clipboard, ColorPicker, ComboBox, DatePicker,
    Dialog, Dock, EditableComboBox, Expander, FilePicker, FileSaver, Grid, Image, Label, Link,
    List, Menu, MenuBar, Navbar, NumberInput, Pagination, PasswordInput, PopupMenu, ProgressBar,
    RadioGroup, Scroll, SearchInput, Sidebar, Slider, Spacer, Spinner, SplitView, Stack, Table,
    Tabs, TextArea, TextInput, TimePicker, Toast, Toggle, Toolbar, Tree, Video, Window,
};
use naui_core::{DatePickerMode, Orientation, Result, Settings, Tasks, Theme};

/// The entry point for creating widgets.
///
/// GTK4 can only create windows after the `GtkApplication` has started, so a `Ui`
/// is available only inside the [`run`] callback.
///
/// Like widget handles, **clones share the same contents**, so moving one into a
/// callback lets you create widgets later. It is main-thread only and not `Send`,
/// so it cannot be passed to other threads (use [`Tasks`] for that).
#[derive(Clone)]
pub struct Ui(pub(crate) sys::Ui);

impl_backend!(Ui);

impl Ui {
    /// Creates a window. The framework keeps a reference to it, so dropping the return
    /// value does not close it.
    ///
    /// `width` / `height` are the size of the content area (the same as
    /// [`Window::set_size`]), excluding the header bar and menu bar.
    #[inline]
    pub fn window(&self, title: &str, width: f64, height: f64) -> Result<Window> {
        self.0.window(title, width, height).map(Window)
    }

    #[inline]
    pub fn stack(&self, orientation: Orientation) -> Result<Stack> {
        self.0.stack(orientation).map(Stack)
    }

    /// A container that positions children by row and column.
    #[inline]
    pub fn grid(&self) -> Result<Grid> {
        self.0.grid().map(Grid)
    }

    /// A container that scrolls when its contents overflow.
    #[inline]
    pub fn scroll(&self) -> Result<Scroll> {
        self.0.scroll().map(Scroll)
    }

    /// A container whose contents are shown and hidden by clicking its heading. `text`
    /// is the heading text.
    #[inline]
    pub fn expander(&self, text: &str) -> Result<Expander> {
        self.0.expander(text).map(Expander)
    }

    /// A container that divides two panes with a movable divider.
    ///
    /// With `Horizontal`, the panes are side by side and the divider is vertical.
    #[inline]
    pub fn split_view(&self, orientation: Orientation) -> Result<SplitView> {
        self.0.split_view(orientation).map(SplitView)
    }

    /// A widget that is just empty space. It absorbs a stack's leftover space and
    /// pushes the others away.
    #[inline]
    pub fn spacer(&self) -> Result<Spacer> {
        self.0.spacer().map(Spacer)
    }

    #[inline]
    pub fn label(&self, text: &str) -> Result<Label> {
        self.0.label(text).map(Label)
    }

    #[inline]
    pub fn button(&self, text: &str) -> Result<Button> {
        self.0.button(text).map(Button)
    }

    #[inline]
    pub fn checkbox(&self, label: &str) -> Result<Checkbox> {
        self.0.checkbox(label).map(Checkbox)
    }

    /// An on/off switch. `label` is the text placed next to it.
    #[inline]
    pub fn toggle(&self, label: &str) -> Result<Toggle> {
        self.0.toggle(label).map(Toggle)
    }

    /// A combo box that shows its options collapsed.
    #[inline]
    pub fn combo_box(&self) -> Result<ComboBox> {
        self.0.combo_box().map(ComboBox)
    }

    /// A combo box where the user can either choose from suggestions or type freely.
    #[inline]
    pub fn editable_combo_box(&self) -> Result<EditableComboBox> {
        self.0.editable_combo_box().map(EditableComboBox)
    }

    /// A radio group that lays out options and lets the user choose exactly one.
    #[inline]
    pub fn radio_group(&self) -> Result<RadioGroup> {
        self.0.radio_group().map(RadioGroup)
    }

    /// A control for picking a date or time. `mode` decides what can be picked.
    #[inline]
    pub fn date_picker(&self, mode: DatePickerMode) -> Result<DatePicker> {
        self.0.date_picker(mode).map(DatePicker)
    }

    /// A control for picking only a time of day. The initial value is the current time.
    #[inline]
    pub fn time_picker(&self) -> Result<TimePicker> {
        self.0.time_picker().map(TimePicker)
    }

    /// A control for picking a color. The initial value is black.
    #[inline]
    pub fn color_picker(&self) -> Result<ColorPicker> {
        self.0.color_picker().map(ColorPicker)
    }

    /// A surface the app draws on itself. Set its size with `set_sizing`.
    #[inline]
    pub fn canvas(&self) -> Result<Canvas> {
        self.0.canvas().map(Canvas)
    }

    #[inline]
    pub fn text_input(&self, text: &str) -> Result<TextInput> {
        self.0.text_input(text).map(TextInput)
    }

    /// A field whose input is masked. Backed by `GtkPasswordEntry`.
    #[inline]
    pub fn password_input(&self) -> Result<PasswordInput> {
        self.0.password_input().map(PasswordInput)
    }

    /// A search field. Backed by `GtkSearchEntry`.
    #[inline]
    pub fn search_input(&self) -> Result<SearchInput> {
        self.0.search_input().map(SearchInput)
    }

    /// A field for entering a number. `value` is the initial value.
    #[inline]
    pub fn number_input(&self, value: f64) -> Result<NumberInput> {
        self.0.number_input(value).map(NumberInput)
    }

    /// A field for entering text that includes newlines. Set its height with
    /// `set_sizing`.
    #[inline]
    pub fn text_area(&self, text: &str) -> Result<TextArea> {
        self.0.text_area(text).map(TextArea)
    }

    #[inline]
    pub fn slider(&self, min: f64, max: f64) -> Result<Slider> {
        self.0.slider(min, max).map(Slider)
    }

    #[inline]
    pub fn progress_bar(&self) -> Result<ProgressBar> {
        self.0.progress_bar().map(ProgressBar)
    }

    /// A spinning indicator for work in progress. Spins from creation.
    #[inline]
    pub fn spinner(&self) -> Result<Spinner> {
        self.0.spinner().map(Spinner)
    }

    /// An image. `source` is a file path or a URL.
    #[inline]
    pub fn image(&self, source: &str) -> Result<Image> {
        self.0.image(source).map(Image)
    }

    /// Video. `source` is a file path or a URL.
    #[inline]
    pub fn video(&self, source: &str) -> Result<Video> {
        self.0.video(source).map(Video)
    }

    /// Audio. `source` is a file path or a URL.
    #[inline]
    pub fn audio(&self, source: &str) -> Result<Audio> {
        self.0.audio(source).map(Audio)
    }

    /// Tabs. Holds each tab's content widget.
    #[inline]
    pub fn tabs(&self) -> Result<Tabs> {
        self.0.tabs().map(Tabs)
    }

    /// Horizontal navigation placed at the top of the screen. `title` is the heading
    /// at the left end.
    #[inline]
    pub fn navbar(&self, title: &str) -> Result<Navbar> {
        self.0.navbar(title).map(Navbar)
    }

    /// Horizontal navigation placed at the bottom of the screen (equal widths).
    #[inline]
    pub fn dock(&self) -> Result<Dock> {
        self.0.dock().map(Dock)
    }

    /// A vertical list of navigation items.
    #[inline]
    pub fn menu(&self) -> Result<Menu> {
        self.0.menu().map(Menu)
    }

    /// A toolbar attached to the top of the window.
    ///
    /// Attach it with [`Window::set_toolbar`]. The framework keeps a reference to it,
    /// so dropping the return value does not stop notifications.
    #[inline]
    pub fn toolbar(&self) -> Result<Toolbar> {
        self.0.toolbar().map(Toolbar)
    }

    /// The OS's application menu, attached to the top of the window.
    ///
    /// Attach it with [`Window::set_menu_bar`]. The framework keeps a reference to it,
    /// so dropping the return value does not stop notifications.
    #[inline]
    pub fn menu_bar(&self) -> Result<MenuBar> {
        self.0.menu_bar().map(MenuBar)
    }

    /// A sidebar attached to the left of the window.
    ///
    /// Attach it with [`Window::set_sidebar`]. The framework keeps a reference to it,
    /// so dropping the return value does not stop notifications.
    #[inline]
    pub fn sidebar(&self) -> Result<Sidebar> {
        self.0.sidebar().map(Sidebar)
    }

    /// A list of selectable rows. It scrolls by itself.
    #[inline]
    pub fn list(&self) -> Result<List> {
        self.0.list().map(List)
    }

    /// A table with column headings. It scrolls by itself.
    #[inline]
    pub fn table(&self) -> Result<Table> {
        self.0.table().map(Table)
    }

    /// A list whose nested items can be expanded and collapsed. It scrolls by itself.
    #[inline]
    pub fn tree(&self) -> Result<Tree> {
        self.0.tree().map(Tree)
    }

    /// A popup (context) menu shown on right-click.
    ///
    /// The framework keeps a reference to it, so dropping the return value does not
    /// remove it from the widgets it is attached to.
    #[inline]
    pub fn popup_menu(&self) -> Result<PopupMenu> {
        self.0.popup_menu().map(PopupMenu)
    }

    /// Breadcrumbs.
    #[inline]
    pub fn breadcrumbs(&self) -> Result<Breadcrumbs> {
        self.0.breadcrumbs().map(Breadcrumbs)
    }

    /// Pagination. `page_count` is the number of pages.
    #[inline]
    pub fn pagination(&self, page_count: usize) -> Result<Pagination> {
        self.0.pagination(page_count).map(Pagination)
    }

    /// A link. If `href` is not empty, clicking it opens it with the default handler.
    #[inline]
    pub fn link(&self, text: &str, href: &str) -> Result<Link> {
        self.0.link(text, href).map(Link)
    }

    /// A button for picking files or folders. Clicking it shows a `GtkFileDialog`.
    #[inline]
    pub fn file_picker(&self, text: &str) -> Result<FilePicker> {
        self.0.file_picker(text).map(FilePicker)
    }

    /// A button that lets the user save contents to a file. Clicking it shows the save
    /// mode of `GtkFileDialog`.
    #[inline]
    pub fn file_saver(&self, text: &str) -> Result<FileSaver> {
        self.0.file_saver(text).map(FileSaver)
    }

    /// A transient notification (toast). `message` is the string to show.
    ///
    /// The framework keeps a reference to it, so dropping the return value does not
    /// stop notifications.
    #[inline]
    pub fn toast(&self, message: &str) -> Result<Toast> {
        self.0.toast(message).map(Toast)
    }

    /// A modal dialog. The framework keeps a reference to it.
    #[inline]
    pub fn dialog(&self, title: &str) -> Result<Dialog> {
        self.0.dialog(title).map(Dialog)
    }

    /// Switches the color theme.
    #[inline]
    pub fn set_theme(&self, theme: Theme) -> Result<()> {
        self.0.set_theme(theme)
    }

    #[inline]
    pub fn theme(&self) -> Theme {
        self.0.theme()
    }

    /// The OS's preferred languages as BCP 47 tags, most preferred first (`"ja-JP"`,
    /// `"en-US"` and so on). The basis for choosing which translation to show.
    ///
    /// Read from `NSLocale.preferredLanguages` on macOS,
    /// `GetUserPreferredUILanguages` on Windows and `navigator.languages` on the
    /// Web. Linux reads the `LANGUAGE` / `LC_ALL` / `LC_MESSAGES` / `LANG`
    /// environment variables the way gettext does (`g_get_language_names`), turning
    /// `ja_JP.UTF-8` into `ja-JP`. Empty if the OS names none (the C locale on Linux).
    #[inline]
    pub fn preferred_languages(&self) -> Vec<String> {
        self.0.preferred_languages()
    }

    /// Called with the new list when the OS's preferred languages change. Setting it
    /// again replaces the previous closure.
    ///
    /// Called only when the list actually changed, on
    /// `NSCurrentLocaleDidChangeNotification` (macOS), `WM_SETTINGCHANGE` with
    /// `"intl"` (Windows) and `languagechange` (Web). Linux decides the languages from
    /// environment variables at startup, so it is never called there.
    ///
    /// naui's own strings (the default macOS main menu, the `Sidebar` toggle and so
    /// on) keep the language chosen at startup, like the strings the native controls
    /// carry themselves. Updating the app's strings is up to the app, for example by
    /// holding widgets with `downgrade()` and calling `set_text`.
    #[inline]
    pub fn on_preferred_languages_change(&self, f: impl FnMut(&[String]) + 'static) {
        self.0.on_preferred_languages_change(f)
    }

    /// The entry point for updating the screen from other threads or async code.
    ///
    /// The returned [`Tasks`] can be cloned and moved into callbacks.
    #[inline]
    pub fn tasks(&self) -> Tasks {
        self.0.tasks()
    }

    /// The clipboard. Text can be read and written.
    #[inline]
    pub fn clipboard(&self) -> Clipboard {
        Clipboard(self.0.clipboard())
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
    #[inline]
    pub fn open_url(&self, url: &str) -> Result<()> {
        self.0.open_url(url)
    }

    /// Quits the app.
    #[inline]
    pub fn quit(&self) {
        self.0.quit()
    }
}

/// Starts the app and builds the UI inside `build`.
///
/// `build` is called exactly once, inside the `GtkApplication`'s `activate`. This
/// function does not return until every window has closed.
#[inline]
pub fn run<F>(settings: Settings, build: F) -> Result<()>
where
    F: FnOnce(&Ui) -> Result<()> + 'static,
{
    sys::run(settings, move |ui| build(&Ui(ui.clone())))
}
