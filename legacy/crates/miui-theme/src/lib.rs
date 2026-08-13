//! # miui-theme
//!
//! 各プラットフォームの「最新のデザイン言語」に対応するトークン集。
//!
//! ここにあるのは **色と寸法の値だけ** で、OS のウィジェットは一切呼ばない。
//! 実際の描画は `miui-render` が自前で行う。つまり「Fluent 2 に**似せた**見た目」
//! であって、WinUI 3 のコントロールそのものではない。
//!
//! **既定では、ビルド対象のプラットフォームに対応するトークンだけをコンパイルする。**
//! Windows 向けにビルドすれば Fluent 2 だけ、macOS 向けなら macOS だけが入る。
//!
//! | ビルド対象 | 使われるトークン |
//! | --- | --- |
//! | Windows | [`fluent`] … Fluent 2 (WinUI 3) |
//! | macOS | [`cupertino`] … macOS |
//! | その他の Unix | [`adwaita`] … GNOME / libadwaita |
//! | wasm32 | [`web`] … ブラウザ既定 |
//!
//! デザインの比較やスクリーンショット生成のために全スタイルを一度に扱いたい
//! 場合だけ、`all-styles` フィーチャを有効にすると [`for_style`] が使える。

#![forbid(unsafe_code)]

#[cfg(any(
    feature = "all-styles",
    all(not(target_arch = "wasm32"), target_os = "windows")
))]
pub mod fluent;

#[cfg(any(
    feature = "all-styles",
    all(not(target_arch = "wasm32"), target_os = "macos")
))]
pub mod cupertino;

#[cfg(any(
    feature = "all-styles",
    all(
        not(target_arch = "wasm32"),
        not(target_os = "windows"),
        not(target_os = "macos")
    )
))]
pub mod adwaita;

#[cfg(any(feature = "all-styles", target_arch = "wasm32"))]
pub mod web;

pub use miui_core::theme::{ColorMode, Metrics, Palette, PlatformStyle, Theme, Typography};

/// ビルド対象のプラットフォームに対応するトークンでテーマを構築する。
///
/// 選ばれるのはあくまで「そのプラットフォームのデザイン言語を模したトークン」で、
/// OS のウィジェットが使われるわけではない (モジュール一覧を参照)。
pub fn for_target(mode: ColorMode) -> Theme {
    #[cfg(target_arch = "wasm32")]
    {
        web::theme(mode)
    }
    #[cfg(all(not(target_arch = "wasm32"), target_os = "windows"))]
    {
        fluent::theme(mode)
    }
    #[cfg(all(not(target_arch = "wasm32"), target_os = "macos"))]
    {
        cupertino::theme(mode)
    }
    #[cfg(all(
        not(target_arch = "wasm32"),
        not(target_os = "windows"),
        not(target_os = "macos")
    ))]
    {
        adwaita::theme(mode)
    }
}

/// スタイルを明示して構築する。`all-styles` フィーチャが必要。
///
/// 通常のアプリでは使わない。デザインの比較、スクリーンショットの生成、
/// 見た目の回帰テストのための入口。
#[cfg(feature = "all-styles")]
pub fn for_style(style: PlatformStyle, mode: ColorMode) -> Theme {
    match style {
        PlatformStyle::Fluent => fluent::theme(mode),
        PlatformStyle::Cupertino => cupertino::theme(mode),
        PlatformStyle::Adwaita => adwaita::theme(mode),
        PlatformStyle::Web => web::theme(mode),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contrast(fg: miui_core::Color, bg: miui_core::Color) -> f32 {
        let fg = fg.over(bg);
        let l1 = fg.luminance().max(bg.luminance()) + 0.05;
        let l2 = fg.luminance().min(bg.luminance()) + 0.05;
        l1 / l2
    }

    fn check(theme: &Theme) {
        let bg = theme.color.window_bg;
        assert!(
            contrast(theme.color.text, bg) >= 4.5,
            "{:?}/{:?} の本文コントラストが不足: {:.2}",
            theme.style,
            theme.mode,
            contrast(theme.color.text, bg)
        );
        let accent_bg = theme.color.accent.over(bg);
        assert!(
            contrast(theme.color.text_on_accent, accent_bg) >= 3.0,
            "{:?}/{:?} のアクセント上コントラストが不足: {:.2}",
            theme.style,
            theme.mode,
            contrast(theme.color.text_on_accent, accent_bg)
        );
        // スイッチのオフ状態が白いつまみと区別できること。
        assert!(
            contrast(miui_core::Color::WHITE, theme.color.switch_track_off.over(bg)) >= 1.15
                || contrast(theme.color.text_secondary, theme.color.switch_track_off.over(bg))
                    >= 3.0,
            "{:?}/{:?} のスイッチ (オフ) が地と区別できない",
            theme.style,
            theme.mode
        );
    }

    /// ビルド対象のテーマは常に検証する。
    #[test]
    fn target_theme_is_legible() {
        check(&for_target(ColorMode::Light));
        check(&for_target(ColorMode::Dark));
    }

    /// `all-styles` を有効にしたときは全スタイルを検証する。
    #[cfg(feature = "all-styles")]
    #[test]
    fn every_style_is_legible() {
        for style in [
            PlatformStyle::Fluent,
            PlatformStyle::Cupertino,
            PlatformStyle::Adwaita,
            PlatformStyle::Web,
        ] {
            for mode in [ColorMode::Light, ColorMode::Dark] {
                check(&for_style(style, mode));
            }
        }
    }
}
