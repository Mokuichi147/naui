//! # naui-web
//!
//! naui's Web backend. It creates **the DOM's standard controls** (`<button>` /
//! `<input>` / `<progress>` …) as they are. In a browser, "native UI" is the form
//! controls themselves, so no custom look is built; the browser's default styles
//! are used.
//!
//! Only the layout uses Flexbox (corresponding to AppKit's NSStackView, GTK4's
//! GtkBox and the `Grid` WinUI 3's `Stack` is built on).

#![cfg(target_arch = "wasm32")]
#![forbid(unsafe_code)]

mod adaptive_panes;
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
mod menu_dom;
mod navigation;
mod number_input;
mod popover;
mod popup;
mod radio_group;
mod sheet;
mod sidebar;
mod split_view;
mod table;
mod time_picker;
mod toast;
mod toggle;
mod toolbar;
mod tree;
mod web_ext;
mod widgets;
mod window;

use naui_core::{DatePickerMode, Error, Orientation, Result, Settings, Tasks, Theme};
use std::cell::Cell;
use std::cell::RefCell;
use std::rc::Rc;
use wasm_bindgen::JsCast;
use web_sys::{Document, HtmlElement};

/// Re-exported for building the wasm entry point.
///
/// So that users of `#[wasm_bindgen(start)]` need not depend on wasm-bindgen
/// directly, the one the backend uses is passed through as is (so the versions
/// never disagree).
#[doc(hidden)]
pub use wasm_bindgen;

#[doc(hidden)]
pub use adaptive_panes::AdaptivePanes;
pub use focus::registered_focus_elements;

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
pub use popover::Popover;
pub use popup::PopupMenu;
pub use radio_group::RadioGroup;
pub use sheet::Sheet;
pub use sidebar::Sidebar;
pub use split_view::SplitView;
pub use table::{Table, TableCells};
pub use time_picker::TimePicker;
pub use toast::Toast;
pub use toggle::Toggle;
pub use toolbar::Toolbar;
pub use tree::Tree;
pub use web_ext::WebWidgetExt;
pub use widgets::{
    Button, Checkbox, Label, PasswordInput, ProgressBar, SearchInput, Slider, Spinner, Stack,
    TextArea, TextInput, Widget,
};
pub use window::{WeakWindow, Window};

pub(crate) fn document() -> Result<Document> {
    web_sys::window()
        .and_then(|w| w.document())
        .ok_or_else(|| Error::new("get document", "not running in a browser"))
}

pub(crate) fn to_error(context: &'static str, value: wasm_bindgen::JsValue) -> Error {
    use wasm_bindgen::JsCast;
    // 例外 (`DOMException` も `Error` を継ぐ) は名前とメッセージだけを出す。
    // `Debug` ではスタックまで重なって読みにくい。
    let message = match (value.as_string(), value.dyn_ref::<js_sys::Error>()) {
        (Some(text), _) => text,
        (None, Some(error)) => format!("{}: {}", error.name(), error.message()),
        (None, None) => format!("{value:?}"),
    };
    Error::new(context, message)
}

/// The entry point for creating widgets.
///
/// Like widget handles, **clones share the same contents**, so moving one into a
/// callback lets you create widgets later.
#[derive(Clone)]
pub struct Ui(Rc<UiInner>);

struct UiInner {
    document: Document,
    theme: Cell<Theme>,
    windows: RefCell<Vec<Window>>,
    /// Dialogs are never appended anywhere, so they are held here.
    dialogs: RefCell<Vec<Dialog>>,
    /// Sheets are not part of the layout either, so they are held here.
    sheets: RefCell<Vec<Sheet>>,
    /// Popup menus are not part of the layout, so no parent holds them.
    popups: RefCell<Vec<PopupMenu>>,
    /// Popovers are not part of the layout either, so they are held here.
    popovers: RefCell<Vec<Popover>>,
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
    /// The browser's preferred languages, read again on `languagechange`.
    languages: naui_core::PreferredLanguages,
    /// The subscription to `languagechange` on `window`.
    language_listener: RefCell<Option<widgets::Listener>>,
}

impl Ui {
    fn new(document: Document, theme: Theme) -> Self {
        let languages = preferred_languages();
        naui_core::set_ui_languages(&languages);
        let this = Self(Rc::new(UiInner {
            document,
            theme: Cell::new(theme),
            windows: RefCell::new(Vec::new()),
            dialogs: RefCell::new(Vec::new()),
            sheets: RefCell::new(Vec::new()),
            popups: RefCell::new(Vec::new()),
            popovers: RefCell::new(Vec::new()),
            toolbars: RefCell::new(Vec::new()),
            menu_bars: RefCell::new(Vec::new()),
            sidebars: RefCell::new(Vec::new()),
            toasts: RefCell::new(Vec::new()),
            tasks: Tasks::from_main_thread(std::sync::Arc::new(main_thread::Microtask)),
            languages: naui_core::PreferredLanguages::new(languages),
            language_listener: RefCell::new(None),
        }));
        if let Some(window) = web_sys::window() {
            // 窓は Ui より長く生きるので、弱参照でたどる。
            let weak = Rc::downgrade(&this.0);
            let listener = widgets::Listener::attach(&window, "languagechange", move || {
                if let Some(inner) = weak.upgrade() {
                    inner.languages.update(preferred_languages());
                }
            });
            *this.0.language_listener.borrow_mut() = listener.ok();
        }
        this
    }

    /// Creates a window. Browsers have no OS windows, so it is represented as a block
    /// element directly under `<body>`, and the title is reflected in
    /// `document.title`.
    ///
    /// `width` / `height` are the size of the content area (the same as
    /// [`Window::set_size`]), excluding the menu bar and toolbar rows. The width acts
    /// as a maximum and the height as a minimum.
    pub fn window(&self, title: &str, width: f64, height: f64) -> Result<Window> {
        let w = Window::new(&self.0.document, title, width, height)?;
        self.0.windows.borrow_mut().push(w.clone());
        Ok(w)
    }

    pub fn stack(&self, orientation: Orientation) -> Result<Stack> {
        Stack::new(&self.0.document, orientation)
    }

    /// A container that positions children by row and column.
    pub fn grid(&self) -> Result<Grid> {
        Grid::new(&self.0.document)
    }

    /// A container that scrolls when its contents overflow.
    pub fn scroll(&self) -> Result<Scroll> {
        Scroll::new(&self.0.document)
    }

    /// A container whose contents are shown and hidden by clicking its heading. `text`
    /// is the heading text.
    pub fn expander(&self, text: &str) -> Result<Expander> {
        Expander::new(&self.0.document, text)
    }

    /// A container that divides two panes with a movable divider.
    ///
    /// With `Horizontal`, the panes are side by side and the divider is vertical.
    pub fn split_view(&self, orientation: Orientation) -> Result<SplitView> {
        SplitView::new(&self.0.document, orientation)
    }

    /// A list and its detail, side by side when wide and one at a time when narrow.
    pub fn adaptive_panes(&self) -> Result<AdaptivePanes> {
        AdaptivePanes::new(&self.0.document)
    }

    /// A widget that is just empty space. It absorbs a stack's leftover space and
    /// pushes the others away.
    pub fn spacer(&self) -> Result<Spacer> {
        Spacer::new(&self.0.document)
    }

    pub fn label(&self, text: &str) -> Result<Label> {
        Label::new(&self.0.document, text)
    }

    pub fn button(&self, text: &str) -> Result<Button> {
        Button::new(&self.0.document, text)
    }

    pub fn checkbox(&self, label: &str) -> Result<Checkbox> {
        Checkbox::new(&self.0.document, label)
    }

    /// An on/off switch. `label` is the text placed next to it.
    pub fn toggle(&self, label: &str) -> Result<Toggle> {
        Toggle::new(&self.0.document, label)
    }

    /// A drop-down for choosing one item.
    pub fn combo_box(&self) -> Result<ComboBox> {
        ComboBox::new(&self.0.document)
    }

    /// A combo box where the user can either choose from suggestions or type freely.
    pub fn editable_combo_box(&self) -> Result<EditableComboBox> {
        EditableComboBox::new(&self.0.document)
    }

    /// A radio group that lays out options and lets the user choose exactly one.
    pub fn radio_group(&self) -> Result<RadioGroup> {
        RadioGroup::new(&self.0.document)
    }

    /// A control for picking a date or time. `mode` decides what can be picked.
    pub fn date_picker(&self, mode: DatePickerMode) -> Result<DatePicker> {
        DatePicker::new(&self.0.document, mode)
    }

    /// A control for picking only a time of day. The initial value is the current time.
    pub fn time_picker(&self) -> Result<TimePicker> {
        TimePicker::new(&self.0.document)
    }

    /// A control for picking a color. The initial value is black.
    pub fn color_picker(&self) -> Result<ColorPicker> {
        ColorPicker::new(&self.0.document)
    }

    /// A surface the app draws on itself. Set its size with `set_sizing`.
    pub fn canvas(&self) -> Result<Canvas> {
        Canvas::new(&self.0.document)
    }

    pub fn text_input(&self, text: &str) -> Result<TextInput> {
        TextInput::new(&self.0.document, text)
    }

    /// A field whose input is masked. Backed by `<input type="password">`.
    pub fn password_input(&self) -> Result<PasswordInput> {
        PasswordInput::new(&self.0.document)
    }

    /// A search field. Backed by `<input type="search">`.
    pub fn search_input(&self) -> Result<SearchInput> {
        SearchInput::new(&self.0.document)
    }

    /// A field for entering a number. `value` is the initial value.
    pub fn number_input(&self, value: f64) -> Result<NumberInput> {
        NumberInput::new(&self.0.document, value)
    }

    /// A field for entering text that includes newlines. Set its height with
    /// `set_sizing`.
    pub fn text_area(&self, text: &str) -> Result<TextArea> {
        TextArea::new(&self.0.document, text)
    }

    pub fn slider(&self, min: f64, max: f64) -> Result<Slider> {
        Slider::new(&self.0.document, min, max)
    }

    pub fn progress_bar(&self) -> Result<ProgressBar> {
        ProgressBar::new(&self.0.document)
    }

    /// A spinning indicator for work in progress. Spins from creation.
    ///
    /// The DOM has no spinner, so it is an indeterminate `<progress>`, which browsers
    /// draw as a bar.
    pub fn spinner(&self) -> Result<Spinner> {
        Spinner::new(&self.0.document)
    }

    /// Tabs. Holds each tab's content widget.
    pub fn tabs(&self) -> Result<Tabs> {
        Tabs::new(&self.0.document)
    }

    /// Horizontal navigation placed at the top of the screen. `title` is the heading
    /// at the left end.
    pub fn navbar(&self, title: &str) -> Result<Navbar> {
        Navbar::new(&self.0.document, title)
    }

    /// Horizontal navigation placed at the bottom of the screen (equal widths).
    pub fn dock(&self) -> Result<Dock> {
        Dock::new(&self.0.document)
    }

    /// A toolbar attached to the top of the window.
    ///
    /// Attach it with [`Window::set_toolbar`]. The framework keeps a reference to it,
    /// so dropping the return value does not stop notifications.
    pub fn toolbar(&self) -> Result<Toolbar> {
        let toolbar = Toolbar::new(&self.0.document)?;
        self.0.toolbars.borrow_mut().push(toolbar.clone());
        Ok(toolbar)
    }

    /// The equivalent of the OS's application menu, attached to the top of the window.
    ///
    /// Attach it with [`Window::set_menu_bar`]. The framework keeps a reference to it,
    /// so dropping the return value does not stop notifications.
    pub fn menu_bar(&self) -> Result<MenuBar> {
        let menu_bar = MenuBar::new(&self.0.document)?;
        self.0.menu_bars.borrow_mut().push(menu_bar.clone());
        Ok(menu_bar)
    }

    /// A sidebar attached to the left of the window.
    ///
    /// Attach it with [`Window::set_sidebar`]. The framework keeps a reference to it,
    /// so dropping the return value does not stop notifications.
    pub fn sidebar(&self) -> Result<Sidebar> {
        let sidebar = Sidebar::new(&self.0.document)?;
        self.0.sidebars.borrow_mut().push(sidebar.clone());
        Ok(sidebar)
    }

    /// A vertical list of navigation items.
    pub fn menu(&self) -> Result<Menu> {
        Menu::new(&self.0.document)
    }

    /// A list of selectable rows. It scrolls by itself.
    pub fn list(&self) -> Result<List> {
        List::new(&self.0.document)
    }

    /// A table with column headings. It scrolls by itself.
    pub fn table(&self) -> Result<Table> {
        Table::new(&self.0.document)
    }

    /// A list whose nested items can be expanded and collapsed. It scrolls by itself.
    pub fn tree(&self) -> Result<Tree> {
        Tree::new(&self.0.document)
    }

    /// A popup (context) menu shown on right-click.
    ///
    /// The framework keeps a reference to it, so dropping the return value does not
    /// remove it from the widgets it is attached to.
    pub fn popup_menu(&self) -> Result<PopupMenu> {
        let popup = PopupMenu::new(&self.0.document)?;
        self.0.popups.borrow_mut().push(popup.clone());
        Ok(popup)
    }

    /// A panel shown beside a widget (an element with the `popover` attribute).
    ///
    /// The framework keeps a reference to it, so dropping the return value does not
    /// stop notifications.
    pub fn popover(&self) -> Result<Popover> {
        let popover = Popover::new(&self.0.document)?;
        self.0.popovers.borrow_mut().push(popover.clone());
        Ok(popover)
    }

    /// Breadcrumbs.
    pub fn breadcrumbs(&self) -> Result<Breadcrumbs> {
        Breadcrumbs::new(&self.0.document)
    }

    /// Pagination. `page_count` is the number of pages.
    pub fn pagination(&self, page_count: usize) -> Result<Pagination> {
        Pagination::new(&self.0.document, page_count)
    }

    /// A link. If `href` is not empty, clicking it opens it in a new tab.
    pub fn link(&self, text: &str, href: &str) -> Result<Link> {
        Link::new(&self.0.document, text, href)
    }

    /// An image. `source` is a file path or a URL.
    pub fn image(&self, source: &str) -> Result<Image> {
        Image::new(&self.0.document, source)
    }

    /// Video. `source` is a file path or a URL.
    pub fn video(&self, source: &str) -> Result<Video> {
        Video::new(&self.0.document, source)
    }

    /// Audio. `source` is a file path or a URL.
    pub fn audio(&self, source: &str) -> Result<Audio> {
        Audio::new(&self.0.document, source)
    }

    /// A button for picking files or folders. Backed by `<input type="file">`.
    pub fn file_picker(&self, text: &str) -> Result<FilePicker> {
        FilePicker::new(&self.0.document, text)
    }

    /// A button that lets the user save contents to a file. Clicking it shows a save
    /// dialog.
    ///
    /// Uses `showSaveFilePicker` if available, and a download otherwise.
    pub fn file_saver(&self, text: &str) -> Result<FileSaver> {
        FileSaver::new(&self.0.document, text)
    }

    /// A transient notification (toast). `message` is the string to show.
    ///
    /// The framework keeps a reference to it, so dropping the return value does not
    /// stop notifications.
    pub fn toast(&self, message: &str) -> Result<Toast> {
        let toast = Toast::new(&self.0.document, message)?;
        self.0.toasts.borrow_mut().push(toast.clone());
        Ok(toast)
    }

    /// A modal dialog. `title` is the heading. Backed by `<dialog>`.
    ///
    /// The framework keeps a reference to it, so dropping the return value does not
    /// stop notifications.
    pub fn dialog(&self, title: &str) -> Result<Dialog> {
        let d = Dialog::new(&self.0.document, title)?;
        self.0.dialogs.borrow_mut().push(d.clone());
        Ok(d)
    }

    /// A modal dialog holding any widget as its content. `title` is the heading.
    /// Backed by `<dialog>`.
    ///
    /// The framework keeps a reference to it, so dropping the return value does not
    /// stop notifications.
    pub fn sheet(&self, title: &str) -> Result<Sheet> {
        let sheet = Sheet::new(&self.0.document, title)?;
        self.0.sheets.borrow_mut().push(sheet.clone());
        Ok(sheet)
    }

    /// Switches the color theme at runtime.
    pub fn set_theme(&self, theme: Theme) -> Result<()> {
        apply_theme(&self.0.document, theme)?;
        self.0.theme.set(theme);
        Ok(())
    }

    /// Returns the currently selected color theme.
    pub fn theme(&self) -> Theme {
        self.0.theme.get()
    }

    /// The browser's preferred languages as BCP 47 tags, most preferred first
    /// (`navigator.languages`). The basis for choosing which translation to show.
    ///
    /// Some browsers leave `languages` empty, and `navigator.language` is used then.
    pub fn preferred_languages(&self) -> Vec<String> {
        self.0.languages.get()
    }

    /// Called with the new list when the browser's preferred languages change.
    /// Setting it again replaces the previous closure.
    ///
    /// Read again on `window`'s `languagechange`, and called only when the list
    /// actually changed. naui's own strings, like the browser's own strings (date
    /// inputs, validation messages), keep the language chosen at startup.
    pub fn on_preferred_languages_change(&self, f: impl FnMut(&[String]) + 'static) {
        self.0.languages.on_change(f);
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
    /// On the Web it opens in a new tab. Browsers open new tabs only inside a user
    /// action (a click, for example), so call it from a button's `on_click` or
    /// similar. `Err` if the browser blocks the tab.
    pub fn open_url(&self, url: &str) -> Result<()> {
        let window =
            web_sys::window().ok_or_else(|| Error::new("open the URL", "there is no window"))?;
        // `noopener` を付けると開けたときも `null` が返り、止められたのと
        // 見分けられない。付けずに開き、開いた先から元のページを触れないよう
        // `opener` を外す。
        let opened = window
            .open_with_url_and_target(url, "_blank")
            .map_err(|e| to_error("open the URL", e))?
            .ok_or_else(|| Error::new("open the URL", "the browser blocked the new tab"))?;
        let _ = opened.set_opener(&wasm_bindgen::JsValue::NULL);
        Ok(())
    }

    /// Browsers have no concept of quitting an app, so this does nothing.
    pub fn quit(&self) {}
}

/// Runs only `build` and returns, without touching the page's event loop. **For
/// automated tests only.**
///
/// Real apps should use [`run`]. Unlike [`run`], it touches neither
/// `document.title` nor the color theme, so creating, operating and changing the
/// state of widgets can be verified without changing the look of the page the test
/// runner prepared.
#[doc(hidden)]
pub fn run_for_test<F>(build: F) -> Result<()>
where
    F: FnOnce(&Ui) -> Result<()>,
{
    let ui = Ui::new(document()?, Theme::System);
    build(&ui)
}

/// Builds the UI.
///
/// The page itself runs the browser's event loop, so this function returns as soon
/// as `build` has run. The framework keeps holding the widgets and callbacks.
pub fn run<F>(settings: Settings, build: F) -> Result<()>
where
    F: FnOnce(&Ui) -> Result<()> + 'static,
{
    let document = document()?;
    document.set_title(&settings.name);
    apply_theme(&document, settings.theme)?;
    let ui = Ui::new(document, settings.theme);
    build(&ui)?;
    // ウィンドウ (と、そこにぶら下がるクロージャ) をページの寿命まで保持する。
    KEEP.with(|k| k.borrow_mut().push(ui));
    Ok(())
}

thread_local! {
    static KEEP: RefCell<Vec<Ui>> = const { RefCell::new(Vec::new()) };
}

/// The browser's preferred languages, most preferred first (`navigator.languages`).
///
/// Some browsers leave `languages` empty, so `navigator.language` is used then.
fn preferred_languages() -> Vec<String> {
    let Some(navigator) = web_sys::window().map(|window| window.navigator()) else {
        return Vec::new();
    };
    let languages: Vec<String> = navigator
        .languages()
        .iter()
        .filter_map(|tag| tag.as_string())
        .collect();
    if languages.is_empty() {
        navigator.language().into_iter().collect()
    } else {
        languages
    }
}

pub(crate) fn apply_theme(document: &Document, theme: Theme) -> Result<()> {
    let root: HtmlElement = document
        .document_element()
        .ok_or_else(|| Error::new("get the document element", "there is no html element"))?
        .unchecked_into();
    let value = match theme {
        Theme::System => "light dark",
        Theme::Light => "light",
        Theme::Dark => "dark",
    };
    root.style()
        .set_property("color-scheme", value)
        .map_err(|e| to_error("set color-scheme", e))
}
