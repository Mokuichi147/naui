//! # miui-render
//!
//! miui の描画バックエンド。外部の 2D ライブラリや GPU API に依存せず、
//! 符号付き距離関数 (SDF) によるソフトウェアラスタライズだけで
//! UI を描く。依存は TrueType のグリフ展開 (`fontdue`) のみ。

#![forbid(unsafe_code)]

mod canvas;
mod fonts;
pub mod sdf;

pub use canvas::Canvas;
pub use fonts::Fonts;
