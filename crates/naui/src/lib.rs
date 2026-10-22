#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]

pub use naui_core::{
    accept_attribute, clamp_split_position, days_in_month, default_extension, is_leap_year, media,
    with_default_extension, Align, CloseResponse, Color, DatePickerMode, DateTime, DialogButtons,
    DialogResponse, DrawCommand, Error, EventResponse, FileEntry, FileFilter, FilePickerMode, Fit,
    GridCell, Key, KeyEvent, Length, ListItem, MenuItem, MenuShortcut, MenuSpec, Modifiers,
    NavItem, NumberSpec, Orientation, Padding, Painter, Path, PathSegment, PlaybackState, Point,
    PointerButton, PointerEvent, PointerPhase, PopupItem, Rect, Result, ScrollMetrics,
    ScrollPolicy, SelectionMode, Sender, Settings, SidebarItem, SidebarSection, Sizing, Sleep,
    SortOrder, TableColumn, TableRow, Task, Tasks, TextColor, TextStyle, Theme, Time, Timer,
    ToastSpec, ToolbarIcon, ToolbarItem, Track, TreeItem, WeakWidget, WindowState,
    DEFAULT_SIDEBAR_WIDTH, DEFAULT_SPLIT_POSITION, ROW_WINDOW_THRESHOLD, SIDEBAR_MIN_WIDTH,
    WHEEL_STEP,
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

/// Linux のバックエンド (`naui-gtk`)。naui の型の `backend()` が返す型と、
/// ネイティブの部品を取り出す `native_*` はここにある。
#[cfg(all(
    not(target_arch = "wasm32"),
    unix,
    not(any(target_os = "macos", target_os = "ios", target_os = "android"))
))]
pub use naui_gtk as gtk;
/// macOS のバックエンド (`naui-macos`)。naui の型の `backend()` が返す型と、
/// ネイティブの部品を取り出す `native_*` はここにある。
#[cfg(all(not(target_arch = "wasm32"), target_os = "macos"))]
pub use naui_macos as macos;
/// Windows のバックエンド (`naui-windows`)。naui の型の `backend()` が返す型と、
/// ネイティブの部品を取り出す `native_*` はここにある。
#[cfg(all(not(target_arch = "wasm32"), target_os = "windows"))]
pub use naui_windows as windows;

#[macro_use]
mod widget;

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
mod ui;
mod widgets;
mod window;

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
pub use ui::{run, Ui};
pub use widget::Widget;
pub use widgets::{
    Button, Checkbox, Label, PasswordInput, ProgressBar, SearchInput, Slider, Stack, TextArea,
    TextInput,
};
pub use window::{WeakWindow, Window};

pub mod guide {
    //! 使い方の手引き。レイアウト、ウィジェットごとの振る舞いと環境の違い、
    //! 別スレッドと非同期をまとめている。
    #![doc = include_str!("guide.md")]

    // 手引きの中の `[`Ui`]` のようなリンクを、クレートの直下の名前で引くため。
    #[allow(unused_imports)]
    use crate::*;
}

/// Web のバックエンド (`naui-web`) と、Web だけの DOM 操作 (CSS のクラスと属性)。
///
/// naui の型の `backend()` が返す型と、ネイティブの部品を取り出す `native_*` は
/// ここにある。ほかの環境に対応物が無いので、Web 向けにビルドしたときだけある。
/// 使うところは `#[cfg(target_arch = "wasm32")]` で囲う。
///
/// ```ignore
/// #[cfg(target_arch = "wasm32")]
/// {
///     use naui::web::WebWidgetExt;
///     button.add_class("primary");                 // 文書側の CSS で見た目を変える
///     button.set_attribute("data-testid", "save"); // テスト用の目印
/// }
/// ```
///
/// `style` と `data-naui-*` は naui が使うので、`set_attribute` /
/// `remove_attribute` では書き換えられない (`false` が返る)。`title` は
/// `set_tooltip` が書く属性と同じなので、後から書いたほうが残る。
#[cfg(target_arch = "wasm32")]
pub mod web {
    #[doc(no_inline)]
    pub use naui_web::*;

    /// ウィジェットの要素の CSS のクラスと属性を扱う。Web でだけ使える。
    ///
    /// 対象は親へ置いている要素 ([`naui_web::Widget::native_element`])。naui は
    /// クラスを使わないので、クラスは自由に付け外しできる。属性のうち `role` や
    /// `aria-*`・`tabindex` などは naui も書くので、上書きするとウィジェットの
    /// ふるまいや読み上げが変わることがある。`title` は `set_tooltip` が書く
    /// 属性と同じなので、後から書いたほうが残る。
    pub trait WebWidgetExt: crate::Widget {
        /// CSS のクラスを足す。
        fn add_class(&self, name: &str) {
            naui_web::WebWidgetExt::add_class(self.as_backend(), name)
        }

        /// CSS のクラスを外す。
        fn remove_class(&self, name: &str) {
            naui_web::WebWidgetExt::remove_class(self.as_backend(), name)
        }

        /// CSS のクラスが付いているか。
        fn has_class(&self, name: &str) -> bool {
            naui_web::WebWidgetExt::has_class(self.as_backend(), name)
        }

        /// 属性を書く。書けたら `true`。
        ///
        /// `style` と `data-naui-*` は naui が使うので書かない (`false` を返す)。
        fn set_attribute(&self, name: &str, value: &str) -> bool {
            naui_web::WebWidgetExt::set_attribute(self.as_backend(), name, value)
        }

        /// 属性を外す。外せたら (もともと無かったときも) `true`。
        ///
        /// `style` と `data-naui-*` は naui が使うので外さない (`false` を返す)。
        fn remove_attribute(&self, name: &str) -> bool {
            naui_web::WebWidgetExt::remove_attribute(self.as_backend(), name)
        }

        /// 属性の値。無ければ `None`。
        fn attribute(&self, name: &str) -> Option<String> {
            naui_web::WebWidgetExt::attribute(self.as_backend(), name)
        }
    }

    impl<T: crate::Widget + ?Sized> WebWidgetExt for T {}
}

/// `entry!` が使う wasm-bindgen の再公開。直接使うものではない。
#[cfg(target_arch = "wasm32")]
#[doc(hidden)]
pub use naui_web::wasm_bindgen;

/// ネイティブと Web の入口をまとめて作る。
///
/// アプリの起動処理は環境ごとに形が違う。ネイティブは `main` から呼ぶだけだが、
/// Web はブラウザから呼ばれる関数を `#[wasm_bindgen(start)]` で公開する必要が
/// ある。このマクロが両方を作るので、**アプリ側に `cfg` を書かずに済む**
/// (wasm-bindgen への依存も要らない)。
///
/// ```ignore
/// naui::entry!(naui::Settings::new("gallery"), build);
///
/// fn build(ui: &naui::Ui) -> naui::Result<()> { /* ... */ Ok(()) }
/// ```
///
/// 展開されるのは次の 2 つ。
///
/// - `pub fn start() -> Result<()>` — どの環境でもある起動処理。ネイティブの
///   `main` からはこれを呼ぶ。
/// - Web だけ: ブラウザが読み込み後に呼ぶ入口。失敗は JS の例外として投げるので、
///   ブラウザのコンソールに理由が出る。
#[macro_export]
macro_rules! entry {
    ($settings:expr, $build:expr $(,)?) => {
        /// ネイティブ / Web 共通の起動処理。
        pub fn start() -> $crate::Result<()> {
            $crate::run($settings, $build)
        }

        #[cfg(target_arch = "wasm32")]
        #[doc(hidden)]
        mod __naui_entry {
            // wasm-bindgen が展開するコードは `wasm_bindgen::` を参照するので、
            // naui が再公開しているものをその名前で見えるようにする。
            use $crate::wasm_bindgen;

            /// 読み込みが終わったブラウザが呼ぶ入口。
            #[wasm_bindgen::prelude::wasm_bindgen(start)]
            pub fn start() -> ::core::result::Result<(), wasm_bindgen::JsValue> {
                super::start().map_err(|e| wasm_bindgen::JsValue::from_str(&e.to_string()))
            }
        }
    };
}
