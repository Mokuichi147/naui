//! # miui-core
//!
//! miui のプラットフォーム非依存な中核。ここには OS 依存のコードも
//! 描画実装も一切含まれず、次の語彙だけを定義する。
//!
//! - 幾何 ([`geometry`]) と色 ([`color`])
//! - 入力イベントの正規化形 ([`event`])
//! - レイアウト制約 ([`layout`])
//! - 描画バックエンドのインタフェース ([`painter`])
//! - デザイントークンの型 ([`theme`])
//! - ウィジェット抽象とツリー ([`widget`])

#![forbid(unsafe_code)]

pub mod color;
pub mod event;
pub mod geometry;
pub mod layout;
pub mod painter;
pub mod theme;
pub mod widget;

pub use color::{Brush, Color};
pub use event::{Event, Key, Modifiers, MouseButton};
pub use geometry::{Corners, Insets, Point, Rect, Size};
pub use layout::{Alignment, BoxConstraints, CrossAxis, MainAxis};
pub use painter::{
    FontFamily, FontWeight, LineMetrics, Painter, TextAlign, TextMeasurer, TextStyle,
};
pub use theme::{ColorMode, Metrics, Palette, PlatformStyle, Theme, Typography};
pub use widget::{
    CursorIcon, Element, EventCx, Id, Interaction, LayoutCx, Node, PaintCx, StateStore, Widget,
};
