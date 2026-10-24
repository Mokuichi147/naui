//! Typography (size levels and color roles).
//!
//! All four platforms come with "heading levels relative to body text" and "text
//! colors that carry meaning" as standard. naui takes **only the level and the
//! role** and maps them to the platform's standard (the same rule as
//! [`crate::ToolbarIcon`]). The OS decides the point sizes and the colors
//! themselves, so text follows larger-text settings, the accent color and
//! light / dark switching as is.

/// Levels of text size and weight. The default is [`TextStyle::Body`].
///
/// | Level | macOS | Windows | Linux | Web |
/// | --- | --- | --- | --- | --- |
/// | `LargeTitle` | `NSFontTextStyleLargeTitle` | `TitleLargeTextBlockStyle` | `.title-1` | `2em` / 700 |
/// | `Title` | `NSFontTextStyleTitle1` | `TitleTextBlockStyle` | `.title-2` | `1.5em` / 700 |
/// | `Subtitle` | `NSFontTextStyleTitle3` | `SubtitleTextBlockStyle` | `.title-3` | `1.25em` / 600 |
/// | `Heading` | `NSFontTextStyleHeadline` | `BodyStrongTextBlockStyle` | `.heading` | `1em` / 700 |
/// | `Body` | `NSFontTextStyleBody` | `BodyTextBlockStyle` | (default) | (default) |
/// | `Caption` | `NSFontTextStyleSubheadline` | `CaptionTextBlockStyle` | `.caption` | `0.85em` |
///
/// **Only the Web has no standard levels**, so naui matches the other three
/// platforms with relative CSS values (the same treatment as label wrapping).
/// Because they are relative, they follow the base font size set by the browser or
/// the user.
///
/// ```
/// # use naui_core::TextStyle;
/// assert_eq!(TextStyle::default(), TextStyle::Body);
/// assert_eq!(TextStyle::Title.style_class(), Some("title-2"));
/// assert_eq!(TextStyle::Title.xaml_style_key(), "TitleTextBlockStyle");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TextStyle {
    /// The largest heading, the face of the screen.
    LargeTitle,
    /// A chapter heading.
    Title,
    /// A section heading.
    Subtitle,
    /// A bold heading at body size.
    Heading,
    /// Body text (default).
    #[default]
    Body,
    /// Small text for notes and annotations.
    Caption,
}

impl TextStyle {
    /// Every level. Used to check that the mapping tables are exhaustive.
    pub const ALL: [TextStyle; 6] = [
        TextStyle::LargeTitle,
        TextStyle::Title,
        TextStyle::Subtitle,
        TextStyle::Heading,
        TextStyle::Body,
        TextStyle::Caption,
    ];

    /// The style class used on Linux (GTK4 / libadwaita).
    ///
    /// [`TextStyle::Body`] gets no class (it is the widget's default).
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

    /// The `x:Key` of the `Style` in Windows (WinUI 3)'s type ramp.
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

    /// The `font-size` used on the Web. Not set for body text (`None`).
    ///
    /// The browser and the user decide the base font size, so values are relative.
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

    /// The `font-weight` used on the Web. `None` for levels that do not change weight.
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

/// Text color roles. The default is [`TextColor::Default`].
///
/// | Role | macOS | Windows | Linux | Web |
/// | --- | --- | --- | --- | --- |
/// | `Default` | `labelColor` | (default) | (default) | (default) |
/// | `Secondary` | `secondaryLabelColor` | `TextFillColorSecondaryBrush` | `.dim-label` | `GrayText` |
/// | `Accent` | `controlAccentColor` | `AccentTextFillColorPrimaryBrush` | `.accent` | `AccentColor` |
/// | `Success` | `systemGreenColor` | `SystemFillColorSuccessBrush` | `.success` | chosen by naui |
/// | `Warning` | `systemOrangeColor` | `SystemFillColorCautionBrush` | `.warning` | chosen by naui |
/// | `Danger` | `systemRedColor` | `SystemFillColorCriticalBrush` | `.error` | chosen by naui |
///
/// All three native platforms take the OS's colors directly. **Only on the Web do
/// browsers lack system colors for success, warning and danger**, so naui decides
/// just those with `light-dark()` (the other roles use CSS system colors).
///
/// ```
/// # use naui_core::TextColor;
/// assert_eq!(TextColor::default(), TextColor::Default);
/// assert_eq!(TextColor::Secondary.style_class(), Some("dim-label"));
/// assert_eq!(TextColor::Default.xaml_brush_key(), None);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TextColor {
    /// The body text color (default).
    #[default]
    Default,
    /// A dim color for notes and supplementary text.
    Secondary,
    /// The OS accent color.
    Accent,
    /// A color for success or completion.
    Success,
    /// A color that calls for attention.
    Warning,
    /// A color for failure or danger.
    Danger,
}

impl TextColor {
    /// Every role. Used to check that the mapping tables are exhaustive.
    pub const ALL: [TextColor; 6] = [
        TextColor::Default,
        TextColor::Secondary,
        TextColor::Accent,
        TextColor::Success,
        TextColor::Warning,
        TextColor::Danger,
    ];

    /// The style class used on Linux (GTK4 / libadwaita).
    ///
    /// [`TextColor::Default`] gets no class (it is the widget's default).
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

    /// The `x:Key` of the `Brush` in Windows (WinUI 3)'s theme resources.
    ///
    /// `None` for [`TextColor::Default`], which uses the color decided by the type
    /// ramp's `Style` (`TextFillColorPrimaryBrush`) as is.
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

    /// The `color` used on the Web. Not set for body text (`None`).
    ///
    /// `GrayText` and `AccentColor` are CSS system colors, so they follow the browser
    /// and OS settings. The other three have no system colors, so they are values
    /// chosen by naui, using `light-dark()` to give a suitable color for light and dark
    /// (naui writes `color-scheme` onto `html` as the theme).
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

    /// Each level maps to a distinct target. If two levels land on the same key, the
    /// heading hierarchy collapses on that platform only.
    #[test]
    fn styles_map_to_distinct_targets() {
        let classes: HashSet<_> = TextStyle::ALL.iter().map(|s| s.style_class()).collect();
        assert_eq!(classes.len(), TextStyle::ALL.len());
        let keys: HashSet<_> = TextStyle::ALL.iter().map(|s| s.xaml_style_key()).collect();
        assert_eq!(keys.len(), TextStyle::ALL.len());
    }

    /// The same for colors. On the Web, however, `Heading` changes only the weight and
    /// not the size, so its `css_font_size` may coincide with body text.
    #[test]
    fn colors_map_to_distinct_targets() {
        let classes: HashSet<_> = TextColor::ALL.iter().map(|c| c.style_class()).collect();
        assert_eq!(classes.len(), TextColor::ALL.len());
        let keys: HashSet<_> = TextColor::ALL.iter().map(|c| c.xaml_brush_key()).collect();
        assert_eq!(keys.len(), TextColor::ALL.len());
        let colors: HashSet<_> = TextColor::ALL.iter().map(|c| c.css_color()).collect();
        assert_eq!(colors.len(), TextColor::ALL.len());
    }

    /// Only the default level and role are "unset". If this breaks, labels with nothing
    /// specified would get classes or colors too.
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

    /// Web sizes are relative. Absolute values (px) would stop following the base font
    /// size set by the browser or the user.
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
