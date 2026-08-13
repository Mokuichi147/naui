//! # miui-core
//!
//! バックエンド (AppKit / WinUI 3 / GTK4 / DOM) に依存しない値型だけを置く。
//! ウィジェットそのものは各バックエンドが OS のネイティブコントロールとして
//! 実装するため、ここには描画もレイアウト計算も存在しない。

#![forbid(unsafe_code)]

use std::fmt;

/// miui の操作結果。
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

/// アプリ起動時の設定。
#[derive(Debug, Clone)]
pub struct Settings {
    /// アプリ名 (ウィンドウタイトルの既定値、GTK のアプリ ID 表示名)。
    pub name: String,
    /// 逆ドメイン形式の識別子。GTK4 が要求するため必須扱いにしている。
    pub app_id: String,
}

impl Settings {
    pub fn new(name: impl Into<String>) -> Self {
        let name = name.into();
        let app_id = format!(
            "org.miui.{}",
            name.chars()
                .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
                .collect::<String>()
        );
        Self { name, app_id }
    }

    pub fn app_id(mut self, id: impl Into<String>) -> Self {
        self.app_id = id.into();
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_id_is_derived_from_the_name() {
        let s = Settings::new("my app");
        assert_eq!(s.app_id, "org.miui.my_app");
        assert_eq!(Settings::new("x").app_id("com.example.x").app_id, "com.example.x");
    }

    #[test]
    fn error_displays_context_and_detail() {
        let e = Error::new("ボタンの生成", "E_FAIL");
        assert_eq!(e.to_string(), "ボタンの生成 に失敗しました: E_FAIL");
    }
}
