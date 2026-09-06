//! 文字づかい (大きさの段階と、色の役割)。
//!
//! 4 環境とも「本文を基準にした見出しの段階」と「意味を表す文字色」を
//! 標準で持っている。naui は**段階と役割だけ**を受け取り、その環境の標準へ
//! 写す ([`crate::ToolbarIcon`] と同じ決まり)。級数と色そのものは OS が
//! 決めるので、文字を大きくする設定・アクセントカラー・ライト / ダークの
//! 切り替えにそのまま追従する。

/// 文字の大きさと太さの段階。既定は [`TextStyle::Body`]。
///
/// | 段階 | macOS | Windows | Linux | Web |
/// | --- | --- | --- | --- | --- |
/// | `LargeTitle` | `NSFontTextStyleLargeTitle` | `TitleLargeTextBlockStyle` | `.title-1` | `2em` / 700 |
/// | `Title` | `NSFontTextStyleTitle1` | `TitleTextBlockStyle` | `.title-2` | `1.5em` / 700 |
/// | `Subtitle` | `NSFontTextStyleTitle3` | `SubtitleTextBlockStyle` | `.title-3` | `1.25em` / 600 |
/// | `Heading` | `NSFontTextStyleHeadline` | `BodyStrongTextBlockStyle` | `.heading` | `1em` / 700 |
/// | `Body` | `NSFontTextStyleBody` | `BodyTextBlockStyle` | (既定) | (既定) |
/// | `Caption` | `NSFontTextStyleCaption1` | `CaptionTextBlockStyle` | `.caption` | `0.85em` |
///
/// **Web だけは標準の段階が無い**ので、naui が CSS の相対値で他の 3 環境へ
/// そろえている (ラベルの折り返しと同じ扱い)。相対値なので、ブラウザや
/// ユーザーが決めた基準の文字サイズには追従する。
///
/// ```
/// # use naui_core::TextStyle;
/// assert_eq!(TextStyle::default(), TextStyle::Body);
/// assert_eq!(TextStyle::Title.style_class(), Some("title-2"));
/// assert_eq!(TextStyle::Title.xaml_style_key(), "TitleTextBlockStyle");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TextStyle {
    /// 画面の顔になる、いちばん大きい見出し。
    LargeTitle,
    /// 章の見出し。
    Title,
    /// 節の見出し。
    Subtitle,
    /// 本文と同じ大きさの、太い見出し。
    Heading,
    /// 本文 (既定)。
    #[default]
    Body,
    /// 補足や注釈のための、小さい文字。
    Caption,
}

impl TextStyle {
    /// すべての段階。対応表の網羅を確かめるために使う。
    pub const ALL: [TextStyle; 6] = [
        TextStyle::LargeTitle,
        TextStyle::Title,
        TextStyle::Subtitle,
        TextStyle::Heading,
        TextStyle::Body,
        TextStyle::Caption,
    ];

    /// Linux (GTK4 / libadwaita) が使うスタイルクラス。
    ///
    /// [`TextStyle::Body`] はクラスを付けない (ウィジェットの既定がそれ)。
    pub fn style_class(self) -> Option<&'static str> {
        match self {
            TextStyle::LargeTitle => Some("title-1"),
            TextStyle::Title => Some("title-2"),
            TextStyle::Subtitle => Some("title-3"),
            TextStyle::Heading => Some("heading"),
            TextStyle::Body => None,
            TextStyle::Caption => Some("caption"),
        }
    }

    /// Windows (WinUI 3) の type ramp が持つ `Style` の `x:Key`。
    pub fn xaml_style_key(self) -> &'static str {
        match self {
            TextStyle::LargeTitle => "TitleLargeTextBlockStyle",
            TextStyle::Title => "TitleTextBlockStyle",
            TextStyle::Subtitle => "SubtitleTextBlockStyle",
            TextStyle::Heading => "BodyStrongTextBlockStyle",
            TextStyle::Body => "BodyTextBlockStyle",
            TextStyle::Caption => "CaptionTextBlockStyle",
        }
    }

    /// Web で使う `font-size`。本文は指定しない (`None`)。
    ///
    /// 基準の文字サイズはブラウザとユーザーが決めるので、値は相対で持つ。
    pub fn css_font_size(self) -> Option<&'static str> {
        match self {
            TextStyle::LargeTitle => Some("2em"),
            TextStyle::Title => Some("1.5em"),
            TextStyle::Subtitle => Some("1.25em"),
            TextStyle::Heading => None,
            TextStyle::Body => None,
            TextStyle::Caption => Some("0.85em"),
        }
    }

    /// Web で使う `font-weight`。太さを変えないものは `None`。
    pub fn css_font_weight(self) -> Option<&'static str> {
        match self {
            TextStyle::LargeTitle => Some("700"),
            TextStyle::Title => Some("700"),
            TextStyle::Subtitle => Some("600"),
            TextStyle::Heading => Some("700"),
            TextStyle::Body => None,
            TextStyle::Caption => None,
        }
    }
}

/// 文字色の役割。既定は [`TextColor::Default`]。
///
/// | 役割 | macOS | Windows | Linux | Web |
/// | --- | --- | --- | --- | --- |
/// | `Default` | `labelColor` | (既定) | (既定) | (既定) |
/// | `Secondary` | `secondaryLabelColor` | `TextFillColorSecondaryBrush` | `.dim-label` | `GrayText` |
/// | `Accent` | `controlAccentColor` | `AccentTextFillColorPrimaryBrush` | `.accent` | `AccentColor` |
/// | `Success` | `systemGreenColor` | `SystemFillColorSuccessBrush` | `.success` | naui が決める |
/// | `Warning` | `systemOrangeColor` | `SystemFillColorCautionBrush` | `.warning` | naui が決める |
/// | `Danger` | `systemRedColor` | `SystemFillColorCriticalBrush` | `.error` | naui が決める |
///
/// ネイティブの 3 環境は、どれも OS が持つ色をそのまま引く。**Web だけは
/// 成功・注意・危険にあたるシステム色がブラウザに無い**ので、そこだけ naui が
/// `light-dark()` で決めている (それ以外の役割は CSS のシステム色を使う)。
///
/// ```
/// # use naui_core::TextColor;
/// assert_eq!(TextColor::default(), TextColor::Default);
/// assert_eq!(TextColor::Secondary.style_class(), Some("dim-label"));
/// assert_eq!(TextColor::Default.xaml_brush_key(), None);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TextColor {
    /// 本文の色 (既定)。
    #[default]
    Default,
    /// 補足や添え書きのための、淡い色。
    Secondary,
    /// OS のアクセントカラー。
    Accent,
    /// 成功や完了を表す色。
    Success,
    /// 注意を促す色。
    Warning,
    /// 失敗や危険を表す色。
    Danger,
}

impl TextColor {
    /// すべての役割。対応表の網羅を確かめるために使う。
    pub const ALL: [TextColor; 6] = [
        TextColor::Default,
        TextColor::Secondary,
        TextColor::Accent,
        TextColor::Success,
        TextColor::Warning,
        TextColor::Danger,
    ];

    /// Linux (GTK4 / libadwaita) が使うスタイルクラス。
    ///
    /// [`TextColor::Default`] はクラスを付けない (ウィジェットの既定がそれ)。
    pub fn style_class(self) -> Option<&'static str> {
        match self {
            TextColor::Default => None,
            TextColor::Secondary => Some("dim-label"),
            TextColor::Accent => Some("accent"),
            TextColor::Success => Some("success"),
            TextColor::Warning => Some("warning"),
            TextColor::Danger => Some("error"),
        }
    }

    /// Windows (WinUI 3) のテーマリソースが持つ `Brush` の `x:Key`。
    ///
    /// [`TextColor::Default`] は `None`。type ramp の `Style` が決める色
    /// (`TextFillColorPrimaryBrush`) をそのまま使う。
    pub fn xaml_brush_key(self) -> Option<&'static str> {
        match self {
            TextColor::Default => None,
            TextColor::Secondary => Some("TextFillColorSecondaryBrush"),
            TextColor::Accent => Some("AccentTextFillColorPrimaryBrush"),
            TextColor::Success => Some("SystemFillColorSuccessBrush"),
            TextColor::Warning => Some("SystemFillColorCautionBrush"),
            TextColor::Danger => Some("SystemFillColorCriticalBrush"),
        }
    }

    /// Web で使う `color`。本文は指定しない (`None`)。
    ///
    /// `GrayText` と `AccentColor` は CSS のシステム色なので、ブラウザと OS の
    /// 設定に追従する。残りの 3 つはシステム色が無いため naui が決めた値で、
    /// `light-dark()` で明暗それぞれに合う色を出す (`color-scheme` は
    /// naui がテーマとして `html` へ書いている)。
    pub fn css_color(self) -> Option<&'static str> {
        match self {
            TextColor::Default => None,
            TextColor::Secondary => Some("GrayText"),
            TextColor::Accent => Some("AccentColor"),
            TextColor::Success => Some("light-dark(#1a7f37, #3fb950)"),
            TextColor::Warning => Some("light-dark(#9a6700, #d29922)"),
            TextColor::Danger => Some("light-dark(#cf222e, #f85149)"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn all_covers_every_style_and_color() {
        assert_eq!(
            TextStyle::ALL.len(),
            TextStyle::ALL.iter().collect::<HashSet<_>>().len()
        );
        assert_eq!(
            TextColor::ALL.len(),
            TextColor::ALL.iter().collect::<HashSet<_>>().len()
        );
    }

    /// 段階ごとに違う写し先へ行く。同じキーへ 2 つの段階が落ちると、
    /// 見出しの階層がその環境だけ潰れる。
    #[test]
    fn styles_map_to_distinct_targets() {
        let classes: HashSet<_> = TextStyle::ALL.iter().map(|s| s.style_class()).collect();
        assert_eq!(classes.len(), TextStyle::ALL.len());
        let keys: HashSet<_> = TextStyle::ALL.iter().map(|s| s.xaml_style_key()).collect();
        assert_eq!(keys.len(), TextStyle::ALL.len());
    }

    /// 色も同じ。ただし Web の `Heading` は太さだけで大きさを変えないので、
    /// `css_font_size` は本文と重なってよい。
    #[test]
    fn colors_map_to_distinct_targets() {
        let classes: HashSet<_> = TextColor::ALL.iter().map(|c| c.style_class()).collect();
        assert_eq!(classes.len(), TextColor::ALL.len());
        let keys: HashSet<_> = TextColor::ALL.iter().map(|c| c.xaml_brush_key()).collect();
        assert_eq!(keys.len(), TextColor::ALL.len());
        let colors: HashSet<_> = TextColor::ALL.iter().map(|c| c.css_color()).collect();
        assert_eq!(colors.len(), TextColor::ALL.len());
    }

    /// 既定の段階と役割だけが「指定なし」になる。ここが崩れると、
    /// 何も指定していないラベルにまでクラスや色が付く。
    #[test]
    fn only_the_defaults_are_unset() {
        for style in TextStyle::ALL {
            assert_eq!(
                style.style_class().is_none(),
                style == TextStyle::Body,
                "{style:?} のスタイルクラス"
            );
        }
        for color in TextColor::ALL {
            assert_eq!(
                color.style_class().is_none(),
                color == TextColor::Default,
                "{color:?} のスタイルクラス"
            );
            assert_eq!(
                color.xaml_brush_key().is_none(),
                color == TextColor::Default,
                "{color:?} のブラシ"
            );
            assert_eq!(
                color.css_color().is_none(),
                color == TextColor::Default,
                "{color:?} の CSS 色"
            );
        }
    }

    /// Web の大きさは相対値で持つ。絶対値 (px) を書くと、ブラウザや
    /// ユーザーが決めた基準の文字サイズに追従しなくなる。
    #[test]
    fn css_font_sizes_are_relative() {
        for style in TextStyle::ALL {
            let Some(size) = style.css_font_size() else {
                continue;
            };
            assert!(size.ends_with("em"), "{style:?} の font-size: {size}");
        }
    }
}
