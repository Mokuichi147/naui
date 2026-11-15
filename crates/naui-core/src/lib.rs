//! # naui-core
//!
//! Value types that do not depend on a backend (AppKit / WinUI 3 / GTK4 / DOM),
//! plus backend-independent message passing (channels / tasks). The widgets
//! themselves are implemented by each backend as the OS's native controls, so
//! there is no drawing here. Layout is computed only where a native control has no
//! say of its own and backends share the answer (such as how a grid's `Fill`
//! tracks split the room by weight).

#![forbid(unsafe_code)]

/// A container that remembers a single callback.
///
/// Writing `Option<Box<dyn FnMut(..)>>` every time makes the types deep and is hard
/// on both readers and `clippy::type_complexity`. The function type inside is
/// written by the user (as in `Slot<dyn FnMut(&str)>`), so higher-ranked lifetimes
/// are preserved.
pub(crate) type Slot<F> = Option<Box<F>>;

mod button;
mod canvas;
mod channel;
mod color;
mod datetime;
mod dialog;
mod direction;
mod file;
mod focus;
mod grid;
mod group;
mod key;
mod languages;
mod layout;
mod list;
mod main_thread;
pub mod media;
mod menu;
mod number;
mod panes;
mod popover;
mod popup;
mod rich_text;
mod row_window;
mod scroll;
mod sidebar;
mod split;
mod table;
mod task;
mod text;
mod timer;
mod toast;
mod toolbar;
mod tree;
mod ui_text;
mod weak;
mod window;

pub use button::ButtonRole;
pub use canvas::{
    DrawCommand, Painter, Path, PathSegment, Point, PointerButton, PointerEvent, PointerPhase,
    Rect, WheelHandler, WHEEL_STEP,
};
pub use channel::{ChannelCloser, Sender};
pub use color::Color;
pub use datetime::{days_in_month, is_leap_year, DatePickerMode, DateTime, Time};
pub use dialog::{
    default_response, DialogButtons, DialogResponse, EnabledButtons, SHEET_MIN_WIDTH,
};
pub use direction::{app_layout_direction, set_app_layout_direction, LayoutDirection};
pub use file::{
    accept_attribute, default_extension, with_default_extension, FileEntry, FileFilter,
    FilePickerMode,
};
pub use focus::{FocusHandler, FocusOwner};
pub use grid::{allocate_tracks, distribute_fill, TrackRequest};
pub use group::DEFAULT_GROUP_PADDING;
pub use key::{EventResponse, Key, KeyEvent, KeyHandler, Modifiers};
pub use languages::{language_tags_from_locale_names, PreferredLanguages};
pub use layout::{
    Breakpoint, GridCell, Length, ScrollMetrics, ScrollPolicy, ScrollTarget, Sizing,
    StackArrangement, Track,
};
pub use list::{ListItem, SelectionMode};
pub use main_thread::{MainThread, Tasks, Work};
pub use media::{Fit, PlaybackState};
pub use menu::{MenuHandler, MenuItem, MenuShortcut, MenuSpec};
pub use number::NumberSpec;
pub use panes::{Pane, PanesArrangement, DEFAULT_LIST_PANE_WIDTH, DEFAULT_PANES_BREAKPOINT};
pub use popover::PopoverPosition;
pub use popup::PopupItem;
pub use rich_text::{LinkHandler, RichText, TextRun};
pub use row_window::{
    keeps_hidden_selection, keeps_row_window, list_row_window, merge_hidden_selection, row_window,
    RowSource, RowWindow, SelectionGesture, ROW_WINDOW_OVERSCAN, ROW_WINDOW_THRESHOLD,
};
pub use scroll::ScrollNotifier;
pub use sidebar::{
    sidebar_item, sidebar_len, sidebar_rows, SidebarAutoChange, SidebarAutoCollapse, SidebarItem,
    SidebarRow, SidebarSection, DEFAULT_SIDEBAR_WIDTH, SIDEBAR_COLLAPSE_CONTENT_WIDTH,
    SIDEBAR_MIN_WIDTH,
};
pub use split::{clamp_split_position, DEFAULT_SPLIT_POSITION};
pub use table::{SortOrder, TableColumn, TableRow};
pub use task::Task;
pub use text::{TextColor, TextStyle};
pub use timer::{Sleep, Timer};
pub use toast::ToastSpec;
pub use toolbar::{ToolbarIcon, ToolbarItem};
pub use tree::TreeItem;
pub use ui_text::{app_language, set_app_language, set_ui_languages, ui_strings, UiStrings};
pub use weak::WeakWidget;
pub use window::{
    is_on_screen, CloseHandler, CloseResponse, StateHandler, WindowPlacement, WindowState,
};

use std::fmt;

/// The result of a naui operation.
pub type Result<T> = std::result::Result<T, Error>;

/// An error wrapping a failure on the native side.
#[derive(Debug, Clone)]
pub struct Error {
    context: &'static str,
    detail: String,
}

impl Error {
    /// Creates an error. `context` names the operation as a verb phrase such as
    /// `"create the Button"`, and is shown as "failed to {context}: {detail}".
    pub fn new(context: &'static str, detail: impl Into<String>) -> Self {
        Self {
            context,
            detail: detail.into(),
        }
    }

    /// The name of the operation that failed.
    pub fn context(&self) -> &'static str {
        self.context
    }

    /// The detail returned by the backend.
    pub fn detail(&self) -> &str {
        &self.detail
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "failed to {}: {}", self.context, self.detail)
    }
}

impl std::error::Error for Error {}

/// The color theme applied to the application.
///
/// `System` follows the OS or browser setting. Specify `Light` or `Dark` to pin it
/// explicitly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Theme {
    /// Follows the OS / browser setting (default).
    #[default]
    System,
    /// Uses the light theme.
    Light,
    /// Uses the dark theme.
    Dark,
}

/// The direction a stack lays out its children.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Orientation {
    #[default]
    Vertical,
    Horizontal,
}

impl Orientation {
    pub fn is_vertical(self) -> bool {
        matches!(self, Orientation::Vertical)
    }
}

/// Alignment on the cross axis. Offers only the common subset the native
/// containers have.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Align {
    Start,
    #[default]
    Center,
    End,
    /// Stretches to fill the cross axis.
    Fill,
}

/// Padding on the top and bottom, and on the start and end of rows (logical pixels).
///
/// `start` is the side where a row starts and `end` the side where it ends: the
/// left and the right, swapped in a right-to-left layout ([`LayoutDirection::Rtl`])
/// so that the padding is mirrored with the rest of the layout, as SwiftUI's
/// leading and trailing and GTK's start and end margins are.
///
/// ```
/// # use naui_core::Padding;
/// let padding = Padding { start: 16.0, ..Padding::symmetric(8.0, 4.0) };
/// assert_eq!((padding.top, padding.end), (8.0, 4.0));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Padding {
    pub top: f64,
    pub bottom: f64,
    /// The side where rows start: the left, or the right in a right-to-left layout.
    pub start: f64,
    /// The side where rows end: the right, or the left in a right-to-left layout.
    pub end: f64,
}

impl Padding {
    pub const ZERO: Padding = Padding::all(0.0);

    pub const fn all(v: f64) -> Self {
        Self {
            top: v,
            bottom: v,
            start: v,
            end: v,
        }
    }

    /// `vertical` on the top and bottom, `horizontal` on the start and end.
    pub const fn symmetric(vertical: f64, horizontal: f64) -> Self {
        Self {
            top: vertical,
            bottom: vertical,
            start: horizontal,
            end: horizontal,
        }
    }
}

/// One navigation item.
///
/// Tabs, nav bars, docks, menus and breadcrumbs all share the same structure: "a
/// sequence of items + the one currently selected". This type represents those
/// items, and backends map it to NSSegmentedControl segments, `<a>` elements and so
/// on.
///
/// Items are identified by their order (index), which the app owns. Selection
/// notifications also come back as indices, so whoever built the `&[NavItem]` can
/// use them directly to switch screens.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NavItem {
    /// The text shown on screen.
    pub label: String,
    /// Whether it can be selected.
    pub enabled: bool,
}

impl NavItem {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            enabled: true,
        }
    }

    /// Sets whether it can be selected (selectable by default).
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Creates a list of items from a sequence of strings.
    ///
    /// ```
    /// # use naui_core::NavItem;
    /// let items = NavItem::list(["Home", "Search", "Settings"]);
    /// assert_eq!(items.len(), 3);
    /// ```
    pub fn list<I, S>(labels: I) -> Vec<NavItem>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        labels.into_iter().map(NavItem::new).collect()
    }
}

impl From<&str> for NavItem {
    fn from(label: &str) -> Self {
        NavItem::new(label)
    }
}

impl From<String> for NavItem {
    fn from(label: String) -> Self {
        NavItem::new(label)
    }
}

/// Settings at app startup.
#[derive(Debug, Clone)]
pub struct Settings {
    /// The app name (the default window title, and the display name for GTK's app ID).
    pub name: String,
    /// An identifier in reverse-domain form. Treated as required because GTK4 requires
    /// it.
    pub app_id: String,
    /// The color theme applied at startup (default [`Theme::System`]).
    pub theme: Theme,
    /// The language the app shows its content in, as a BCP 47 tag (`"ja-JP"`).
    /// `None` (the default) follows the OS. See [`Settings::language`].
    pub language: Option<String>,
    /// Which way the layout runs (default [`LayoutDirection::Auto`]). See
    /// [`Settings::layout_direction`].
    pub layout_direction: LayoutDirection,
}

impl Settings {
    pub fn new(name: impl Into<String>) -> Self {
        let name = name.into();
        let app_id = format!(
            "org.naui.{}",
            name.chars()
                .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
                .collect::<String>()
        );
        Self {
            name,
            app_id,
            theme: Theme::System,
            language: None,
            layout_direction: LayoutDirection::Auto,
        }
    }

    pub fn app_id(mut self, id: impl Into<String>) -> Self {
        self.app_id = id.into();
        self
    }

    /// Sets the color theme at startup.
    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = theme;
        self
    }

    /// Tells the native side which language the app shows its content in, as a BCP
    /// 47 tag (`"ja-JP"`, `"zh-Hant"`).
    ///
    /// It decides which glyphs are chosen for text written in several languages'
    /// scripts (an app showing Japanese on an OS set to another language otherwise
    /// gets Chinese forms of the kanji) and how screen readers pronounce it. naui's
    /// own strings follow it too.
    ///
    /// | Windows | macOS | Linux | Web |
    /// | --- | --- | --- | --- |
    /// | `FrameworkElement.Language` on each window's content | `AppleLanguages` in the arguments domain (the per-app language of System Settings) | `PANGO_LANGUAGE` before GTK starts | `lang` on `<html>` |
    ///
    /// It is read once at startup: macOS and Linux cannot change it in a running
    /// app. Linux sets `PANGO_LANGUAGE` only while no other thread is running, as
    /// changing the environment would race a thread reading it; call `run` before
    /// starting threads (or set `PANGO_LANGUAGE` yourself) to have it there. On macOS [`Ui::preferred_languages`] lists it first, as for an app
    /// whose language is set in System Settings. The strings the native controls
    /// carry themselves (file dialogs, context menus) are not switched by it on any
    /// platform: AppKit chooses those itself, and a file dialog was seen to stay in
    /// the same language whichever tag was given.
    ///
    /// [`Ui::preferred_languages`]: https://docs.rs/naui/latest/naui/struct.Ui.html#method.preferred_languages
    pub fn language(mut self, tag: impl Into<String>) -> Self {
        let tag = tag.into();
        self.language = (!tag.trim().is_empty()).then(|| tag.trim().to_owned());
        self
    }

    /// Sets which way the layout runs: [`LayoutDirection::Rtl`] mirrors it for
    /// right-to-left languages such as Arabic and Hebrew.
    ///
    /// The native toolkit mirrors its controls once it is told the direction:
    /// children of a horizontal `Stack` start at the right, `Grid` columns are
    /// numbered from the right, a `Sidebar` sits on the right, sliders and progress
    /// bars fill from the right, and text starts at the right. [`Padding`]'s `start`
    /// and `end` follow, so `start` is then on the right. Pictures and drawings are not mirrored, nor are `Canvas`, the
    /// number in a `NumberInput` (which stays `-5`, not `5-`), the fields of
    /// `DatePicker` and `TimePicker` on the Web, or the controls of `Video` and
    /// `Audio`, which follow the direction of time on the tape.
    ///
    /// [`LayoutDirection::Auto`] (the default) takes the direction of
    /// [`Settings::language`]. Without a language, each toolkit keeps its own
    /// default: Linux follows the locale (GTK mirrors an untranslated app too when
    /// `LANG` is Arabic), macOS stayed left to right in every case tried (an app
    /// localized into Arabic and launched with Arabic as its language included),
    /// and Windows and the Web stay left to right. An app that translates its strings should pass the
    /// language it chose to [`Settings::language`], so that the layout follows it
    /// on every platform.
    ///
    /// | Windows | macOS | Linux | Web |
    /// | --- | --- | --- | --- |
    /// | `FlowDirection` on each window's content | `AppleTextDirection` and `NSForceRightToLeftWritingDirection` in the arguments domain (`NSApp.userInterfaceLayoutDirection`) | `gtk_widget_set_default_direction` | `dir` on `<html>` |
    ///
    /// It applies to the whole app and is read once at startup: AppKit decides its
    /// layout direction once for the app, so neither a single widget nor a running
    /// app can be switched. [`Ui::layout_direction`] tells which way the layout
    /// ended up running.
    ///
    /// [`Ui::layout_direction`]: https://docs.rs/naui/latest/naui/struct.Ui.html#method.layout_direction
    pub fn layout_direction(mut self, direction: LayoutDirection) -> Self {
        self.layout_direction = direction;
        self
    }

    /// The direction the backends apply at startup: `Some(Ltr)` or `Some(Rtl)`, or
    /// `None` to leave the native default alone. See [`LayoutDirection::resolve`].
    pub fn resolved_layout_direction(&self) -> Option<LayoutDirection> {
        self.layout_direction.resolve(self.language.as_deref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_id_is_derived_from_the_name() {
        let s = Settings::new("my app");
        assert_eq!(s.app_id, "org.naui.my_app");
        assert_eq!(
            Settings::new("x").app_id("com.example.x").app_id,
            "com.example.x"
        );
    }

    #[test]
    fn layout_direction_follows_the_language_by_default() {
        let settings = Settings::new("app");
        assert_eq!(settings.layout_direction, LayoutDirection::Auto);
        assert_eq!(settings.resolved_layout_direction(), None);
        let arabic = Settings::new("app").language("ar-EG");
        assert_eq!(
            arabic.resolved_layout_direction(),
            Some(LayoutDirection::Rtl)
        );
        let forced = arabic.layout_direction(LayoutDirection::Ltr);
        assert_eq!(
            forced.resolved_layout_direction(),
            Some(LayoutDirection::Ltr)
        );
    }

    #[test]
    fn language_defaults_to_the_os() {
        assert_eq!(Settings::new("app").language, None);
        assert_eq!(
            Settings::new("app").language(" ja-JP ").language.as_deref(),
            Some("ja-JP")
        );
        assert_eq!(Settings::new("app").language("").language, None);
    }

    #[test]
    fn settings_default_to_system_theme() {
        assert_eq!(Settings::new("my app").theme, Theme::System);
        assert_eq!(
            Settings::new("my app").theme(Theme::Dark).theme,
            Theme::Dark
        );
    }

    #[test]
    fn nav_item_defaults_to_enabled() {
        let item = NavItem::new("ホーム");
        assert_eq!(item.label, "ホーム");
        assert!(item.enabled);
        assert!(!NavItem::new("設定").enabled(false).enabled);
    }

    #[test]
    fn nav_item_list_keeps_order() {
        let items = NavItem::list(["一覧", "詳細"]);
        assert_eq!(items[0], NavItem::from("一覧"));
        assert_eq!(items[1].label, "詳細");
    }

    #[test]
    fn error_displays_context_and_detail() {
        let e = Error::new("create the button", "E_FAIL");
        assert_eq!(e.to_string(), "failed to create the button: E_FAIL");
    }
}
