//! # miui
//!
//! Rust だけで書かれた、とても軽量なクロスプラットフォーム GUI。
//!
//! - **1 つのコードで 4 環境**: Windows / macOS / Linux / Web (wasm)
//! - **各プラットフォームのデザイン言語を模した見た目**: Windows なら
//!   Fluent 2 (WinUI 3)、macOS なら macOS、Linux なら Adwaita (GNOME)、
//!   Web ならニュートラル。トークンだけを差し替えるので UI コードは 1 つで済む。
//! - **軽量**: GPU も外部 2D ライブラリも使わず、SDF ベースの自作
//!   ソフトウェアラスタライザで描画する。依存はウィンドウ生成 (`winit`)、
//!   ピクセルバッファ提示 (`softbuffer`)、グリフ展開 (`fontdue`) のみ。
//!
//! # 重要: OS のネイティブウィジェットは使っていない
//!
//! miui はウィンドウに 1 枚のピクセルバッファを描くだけで、WinUI 3 /
//! AppKit / GTK4 のコントロールは一切呼ばない。ボタンもテキスト入力も
//! miui が自前で描いた図形であり、**各 OS のデザイン言語を模した再現**である。
//!
//! この方式には次の帰結がある。
//!
//! - OS のアクセシビリティツリーに乗らない (スクリーンリーダーが認識しない)
//! - OS 標準のコンテキストメニューやドラッグ&ドロップ等は自前実装が必要
//! - OS のテーマ変更 (アクセントカラー設定など) には追従しない
//! - 一方で、Web を含む 4 環境で完全に同じ挙動・同じ見た目になる
//!
//! 本物のネイティブウィジェットが必要な用途では、各 OS のツールキットを
//! FFI で束ねる別の設計を検討する必要がある。
//!
//! ## 使い方
//!
//! ```no_run
//! use miui::prelude::*;
//!
//! #[derive(Default)]
//! struct Counter {
//!     count: i32,
//! }
//!
//! #[derive(Clone)]
//! enum Msg {
//!     Increment,
//! }
//!
//! impl Application for Counter {
//!     type Message = Msg;
//!
//!     fn view(&self) -> Element<Msg> {
//!         Element::new(
//!             column()
//!                 .spacing(12.0)
//!                 .padding(Insets::all(24.0))
//!                 .child(Text::new(format!("count: {}", self.count)).title())
//!                 .child(Button::new("増やす").accent().on_press(Msg::Increment)),
//!         )
//!     }
//!
//!     fn update(&mut self, message: Msg) {
//!         match message {
//!             Msg::Increment => self.count += 1,
//!         }
//!     }
//! }
//!
//! fn main() {
//!     miui::run(Counter::default(), Settings::new("counter"));
//! }
//! ```

mod app;
pub mod headless;
mod runtime;

pub use app::{Application, Environment, FontSpec, Settings};
pub use runtime::run;

pub use miui_core as core;
pub use miui_render as render;
pub use miui_theme as theme;
pub use miui_widgets as widgets;

/// よく使う型をまとめて取り込むための再エクスポート。
pub mod prelude {
    pub use crate::app::{Application, Environment, FontSpec, Settings};
    pub use crate::run;

    pub use miui_core::color::{Brush, Color};
    pub use miui_core::event::{Event, Key, Modifiers, MouseButton};
    pub use miui_core::geometry::{Corners, Insets, Point, Rect, Size};
    pub use miui_core::layout::{Alignment, BoxConstraints, CrossAxis, MainAxis};
    pub use miui_core::painter::{FontFamily, FontWeight, TextAlign, TextStyle};
    pub use miui_core::theme::{ColorMode, PlatformStyle, Theme};
    pub use miui_core::widget::{Element, Widget};

    pub use miui_widgets::{
        column, row, Button, ButtonVariant, Checkbox, Container, Divider, IconButton, IconGlyph,
        ProgressBar, Radio, Scroll, SizedBox, Slider, Spacer, Switch, Text, TextInput, TextRole,
    };
}
