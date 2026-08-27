//! # naui-core
//!
//! バックエンド (AppKit / WinUI 3 / GTK4 / DOM) に依存しない値型と、
//! バックエンドに依らない受け渡し (チャネル / タスク) を置く。
//! ウィジェットそのものは各バックエンドが OS のネイティブコントロールとして
//! 実装するため、ここには描画もレイアウト計算も存在しない。

#![forbid(unsafe_code)]

mod channel;
mod color;
mod datetime;
mod dialog;
mod file;
mod layout;
mod list;
mod main_thread;
pub mod media;
mod number;
mod popup;
mod table;
mod task;
mod toast;
mod toolbar;
mod tree;

pub use channel::Sender;
pub use color::Color;
pub use datetime::{days_in_month, is_leap_year, DatePickerMode, DateTime, Time};
pub use dialog::{DialogButtons, DialogResponse};
pub use file::{
    accept_attribute, default_extension, with_default_extension, FileEntry, FileFilter,
    FilePickerMode,
};
pub use layout::{GridCell, Length, ScrollPolicy, Sizing, Track};
pub use list::{ListItem, SelectionMode};
pub use main_thread::{MainThread, Tasks, Work};
pub use media::{Fit, PlaybackState};
pub use number::NumberSpec;
pub use popup::PopupItem;
pub use table::{SortOrder, TableColumn, TableRow};
pub use task::Task;
pub use toast::ToastSpec;
pub use toolbar::{ToolbarIcon, ToolbarItem};
pub use tree::TreeItem;

use std::fmt;

/// naui の操作結果。
pub type Result<T> = std::result::Result<T, Error>;

/// ネイティブ側の失敗を包むエラー。
#[derive(Debug, Clone)]
pub struct Error {
    context: &'static str,
    detail: String,
}

impl Error {
    pub fn new(context: &'static str, detail: impl Into<String>) -> Self {
        Self {
            context,
            detail: detail.into(),
        }
    }

    /// 失敗した操作の名前。
    pub fn context(&self) -> &'static str {
        self.context
    }

    /// バックエンドが返した詳細。
    pub fn detail(&self) -> &str {
        &self.detail
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} に失敗しました: {}", self.context, self.detail)
    }
}

impl std::error::Error for Error {}

/// アプリケーションに適用する配色テーマ。
///
/// `System` は OS やブラウザの設定に追従する。明示的に固定したい場合は
/// `Light` または `Dark` を指定できる。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Theme {
    /// OS / ブラウザの設定に追従する (既定値)。
    #[default]
    System,
    /// ライトテーマを使う。
    Light,
    /// ダークテーマを使う。
    Dark,
}

/// スタックの並び方向。
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

/// 交差軸方向の揃え。ネイティブのコンテナが持つ最小公倍数だけを提供する。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Align {
    Start,
    #[default]
    Center,
    End,
    /// 交差軸いっぱいに広げる。
    Fill,
}

/// 上下左右の余白 (論理ピクセル)。
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

/// ナビゲーションの 1 項目。
///
/// タブ・ナビバー・ドック・メニュー・パンくずは、どれも
/// 「項目の並び + いま選ばれているもの」という同じ構造を持つ。
/// その項目を表すのがこの型で、バックエンドはこれを
/// NSSegmentedControl のセグメントや `<a>` などに写す。
///
/// 項目の識別はアプリ側が持つ順序 (インデックス) で行う。
/// 選択の通知もインデックスで返るので、`&[NavItem]` を作った側が
/// そのまま画面の切り替えに使える。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NavItem {
    /// 画面に出る文字列。
    pub label: String,
    /// 選べるかどうか。
    pub enabled: bool,
}

impl NavItem {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            enabled: true,
        }
    }

    /// 選べるかどうかを指定する (既定は選べる)。
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// 文字列の並びから項目列を作る。
    ///
    /// ```
    /// # use naui_core::NavItem;
    /// let items = NavItem::list(["ホーム", "検索", "設定"]);
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

/// アプリ起動時の設定。
#[derive(Debug, Clone)]
pub struct Settings {
    /// アプリ名 (ウィンドウタイトルの既定値、GTK のアプリ ID 表示名)。
    pub name: String,
    /// 逆ドメイン形式の識別子。GTK4 が要求するため必須扱いにしている。
    pub app_id: String,
    /// 起動時に適用する配色テーマ (既定は [`Theme::System`])。
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

    /// 起動時の配色テーマを指定する。
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
        let e = Error::new("ボタンの生成", "E_FAIL");
        assert_eq!(e.to_string(), "ボタンの生成 に失敗しました: E_FAIL");
    }
}
