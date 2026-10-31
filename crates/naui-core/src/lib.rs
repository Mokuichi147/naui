//! # naui-core
//!
//! Value types that do not depend on a backend (AppKit / WinUI 3 / GTK4 / DOM),
//! plus backend-independent message passing (channels / tasks). The widgets
//! themselves are implemented by each backend as the OS's native controls, so
//! there is no drawing or layout computation here.

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
mod file;
mod focus;
mod key;
mod languages;
mod layout;
mod list;
mod main_thread;
pub mod media;
mod menu;
mod number;
mod popover;
mod popup;
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
pub use channel::Sender;
pub use color::Color;
pub use datetime::{days_in_month, is_leap_year, DatePickerMode, DateTime, Time};
pub use dialog::{DialogButtons, DialogResponse};
pub use file::{
    accept_attribute, default_extension, with_default_extension, FileEntry, FileFilter,
    FilePickerMode,
};
pub use focus::{FocusHandler, FocusOwner};
pub use key::{EventResponse, Key, KeyEvent, KeyHandler, Modifiers};
pub use languages::{language_tags_from_locale_names, PreferredLanguages};
pub use layout::{GridCell, Length, ScrollMetrics, ScrollPolicy, ScrollTarget, Sizing, Track};
pub use list::{ListItem, SelectionMode};
pub use main_thread::{MainThread, Tasks, Work};
pub use media::{Fit, PlaybackState};
pub use menu::{MenuHandler, MenuItem, MenuShortcut, MenuSpec};
pub use number::NumberSpec;
pub use popover::PopoverPosition;
pub use popup::PopupItem;
pub use row_window::{
    keeps_hidden_selection, keeps_row_window, row_window, RowWindow, SelectionGesture,
    ROW_WINDOW_OVERSCAN, ROW_WINDOW_THRESHOLD,
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
pub use ui_text::{set_ui_languages, ui_strings, UiStrings};
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

/// Padding on the top, bottom, left and right (logical pixels).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Padding {
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
    pub left: f64,
}

impl Padding {
    pub const ZERO: Padding = Padding::all(0.0);

    pub const fn all(v: f64) -> Self {
        Self {
            top: v,
            right: v,
            bottom: v,
            left: v,
        }
    }

    pub const fn symmetric(vertical: f64, horizontal: f64) -> Self {
        Self {
            top: vertical,
            right: horizontal,
            bottom: vertical,
            left: horizontal,
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
