#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]

pub use naui_core::{
    accept_attribute, clamp_split_position, days_in_month, default_extension, is_leap_year, media,
    with_default_extension, Align, Breakpoint, ButtonRole, CloseResponse, Color, DatePickerMode,
    DateTime, DialogButtons, DialogResponse, DrawCommand, Error, EventResponse, FileEntry,
    FileFilter, FilePickerMode, Fit, GridCell, Key, KeyEvent, Length, ListItem, MenuItem,
    MenuShortcut, MenuSpec, Modifiers, NavItem, NumberSpec, Orientation, Padding, Painter, Pane,
    Path, PathSegment, PlaybackState, Point, PointerButton, PointerEvent, PointerPhase,
    PopoverPosition, PopupItem, Rect, Result, ScrollMetrics, ScrollPolicy, SelectionMode, Sender,
    Settings, SidebarItem, SidebarSection, Sizing, Sleep, SortOrder, TableColumn, TableRow, Task,
    Tasks, TextColor, TextStyle, Theme, Time, Timer, ToastSpec, ToolbarIcon, ToolbarItem, Track,
    TreeItem, WeakWidget, WindowPlacement, WindowState, DEFAULT_LIST_PANE_WIDTH,
    DEFAULT_PANES_BREAKPOINT, DEFAULT_SIDEBAR_WIDTH, DEFAULT_SPLIT_POSITION, ROW_WINDOW_THRESHOLD,
    SHEET_MIN_WIDTH, SIDEBAR_COLLAPSE_CONTENT_WIDTH, SIDEBAR_MIN_WIDTH, WHEEL_STEP,
};

// ビルド対象のバックエンド。naui の型はこれの型を包む。
#[cfg(all(
    not(target_arch = "wasm32"),
    unix,
    not(any(target_os = "macos", target_os = "ios", target_os = "android"))
))]
use naui_gtk as sys;
#[cfg(all(not(target_arch = "wasm32"), target_os = "macos"))]
use naui_macos as sys;
#[cfg(target_arch = "wasm32")]
use naui_web as sys;
#[cfg(all(not(target_arch = "wasm32"), target_os = "windows"))]
use naui_windows as sys;

/// The Linux backend (`naui-gtk`). The types returned by `backend()` on naui's
/// types, and the `native_*` functions that take out the native parts, are here.
#[cfg(all(
    not(target_arch = "wasm32"),
    unix,
    not(any(target_os = "macos", target_os = "ios", target_os = "android"))
))]
pub use naui_gtk as gtk;
/// The macOS backend (`naui-macos`). The types returned by `backend()` on naui's
/// types, and the `native_*` functions that take out the native parts, are here.
#[cfg(all(not(target_arch = "wasm32"), target_os = "macos"))]
pub use naui_macos as macos;
/// The Windows backend (`naui-windows`). The types returned by `backend()` on
/// naui's types, and the `native_*` functions that take out the native parts, are
/// here.
#[cfg(all(not(target_arch = "wasm32"), target_os = "windows"))]
pub use naui_windows as windows;

#[macro_use]
mod widget;

mod adaptive_panes;
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
mod layout;
mod list;
mod media_widgets;
mod menu_bar;
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
mod ui;
mod widgets;
mod window;

pub use adaptive_panes::AdaptivePanes;
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
pub use media_widgets::{Audio, Image, Video};
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
pub use ui::{run, Ui};
pub use widget::Widget;
pub use widgets::{
    Button, Checkbox, Label, PasswordInput, ProgressBar, SearchInput, Slider, Spinner, Stack,
    TextArea, TextInput,
};
pub use window::{WeakWindow, Window};

pub mod guide {
    //! The usage guide. Covers layout, how each widget behaves and how platforms
    //! differ, and other threads and async.
    #![doc = include_str!("guide.md")]

    // 手引きの中の `[`Ui`]` のようなリンクを、クレートの直下の名前で引くため。
    #[allow(unused_imports)]
    use crate::*;
}

/// The Web backend (`naui-web`) and Web-only DOM operations (CSS classes and
/// attributes).
///
/// The types returned by `backend()` on naui's types, and the `native_*` functions
/// that take out the native parts, are here. Other platforms have no counterpart,
/// so this exists only when building for the Web. Wrap code that uses it in
/// `#[cfg(target_arch = "wasm32")]`.
///
/// ```ignore
/// #[cfg(target_arch = "wasm32")]
/// {
///     use naui::web::WebWidgetExt;
///     button.add_class("primary");                 // style it with the page's CSS
///     button.set_attribute("data-testid", "save"); // a marker for tests
/// }
/// ```
///
/// naui uses `style` and `data-naui-*`, so `set_attribute` / `remove_attribute`
/// cannot change them (they return `false`). `title` is the same attribute that
/// `set_tooltip` writes, so whichever is written last wins.
#[cfg(target_arch = "wasm32")]
pub mod web {
    #[doc(no_inline)]
    pub use naui_web::*;

    /// Handles the CSS classes and attributes of a widget's element. Web only.
    ///
    /// The target is the element placed into the parent
    /// ([`naui_web::Widget::native_element`]). naui uses no classes, so you can add and
    /// remove classes freely. Among attributes, naui also writes `role`, `aria-*`,
    /// `tabindex` and others, so overwriting them can change the widget's behavior or
    /// how it is read aloud. `title` is the same attribute that `set_tooltip` writes,
    /// so whichever is written last wins.
    pub trait WebWidgetExt: crate::Widget {
        /// Adds a CSS class.
        fn add_class(&self, name: &str) {
            naui_web::WebWidgetExt::add_class(self.as_backend(), name)
        }

        /// Removes a CSS class.
        fn remove_class(&self, name: &str) {
            naui_web::WebWidgetExt::remove_class(self.as_backend(), name)
        }

        /// Whether a CSS class is present.
        fn has_class(&self, name: &str) -> bool {
            naui_web::WebWidgetExt::has_class(self.as_backend(), name)
        }

        /// Writes an attribute. `true` if it was written.
        ///
        /// naui uses `style` and `data-naui-*`, so they are not written (returns `false`).
        fn set_attribute(&self, name: &str, value: &str) -> bool {
            naui_web::WebWidgetExt::set_attribute(self.as_backend(), name, value)
        }

        /// Removes an attribute. `true` if it was removed (including when it was not there
        /// to begin with).
        ///
        /// naui uses `style` and `data-naui-*`, so they are not removed (returns `false`).
        fn remove_attribute(&self, name: &str) -> bool {
            naui_web::WebWidgetExt::remove_attribute(self.as_backend(), name)
        }

        /// The attribute's value. `None` if absent.
        fn attribute(&self, name: &str) -> Option<String> {
            naui_web::WebWidgetExt::attribute(self.as_backend(), name)
        }
    }

    impl<T: crate::Widget + ?Sized> WebWidgetExt for T {}
}

/// A re-export of wasm-bindgen for `entry!`. Not meant to be used directly.
#[cfg(target_arch = "wasm32")]
#[doc(hidden)]
pub use naui_web::wasm_bindgen;

/// Creates the native and Web entry points in one go.
///
/// How an app starts differs by platform. Natively it is just called from `main`,
/// but on the Web a function called by the browser must be exported with
/// `#[wasm_bindgen(start)]`. This macro creates both, so **the app needs no `cfg`**
/// (and no dependency on wasm-bindgen).
///
/// ```ignore
/// naui::entry!(naui::Settings::new("gallery"), build);
///
/// fn build(ui: &naui::Ui) -> naui::Result<()> { /* ... */ Ok(()) }
/// ```
///
/// It expands to these two:
///
/// - `pub fn start() -> Result<()>` — the startup routine present on every
///   platform. Call it from the native `main`.
/// - Web only: the entry point the browser calls after loading. Failures are thrown
///   as JS exceptions, so the reason shows up in the browser console.
#[macro_export]
macro_rules! entry {
    ($settings:expr, $build:expr $(,)?) => {
        /// The startup routine shared by native and Web.
        pub fn start() -> $crate::Result<()> {
            $crate::run($settings, $build)
        }

        #[cfg(target_arch = "wasm32")]
        #[doc(hidden)]
        mod __naui_entry {
            // wasm-bindgen が展開するコードは `wasm_bindgen::` を参照するので、
            // naui が再公開しているものをその名前で見えるようにする。
            use $crate::wasm_bindgen;

            /// The entry point the browser calls once loading has finished.
            #[wasm_bindgen::prelude::wasm_bindgen(start)]
            pub fn start() -> ::core::result::Result<(), wasm_bindgen::JsValue> {
                super::start().map_err(|e| wasm_bindgen::JsValue::from_str(&e.to_string()))
            }
        }
    };
}
