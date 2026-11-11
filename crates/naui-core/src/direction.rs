//! The writing direction the app's layout follows.
//!
//! Each native toolkit mirrors its own controls once it is told the direction: rows
//! of a horizontal `Stack` start at the right, `Grid` columns are numbered from the
//! right, a slider's minimum is at the right, the sidebar sits on the right. naui
//! only tells it, once for the whole app at startup, and pins left-to-right on the
//! few widgets whose content must not be mirrored.
//!
//! Each backend resolves [`Settings::layout_direction`](crate::Settings::layout_direction)
//! with [`LayoutDirection::resolve`] when the app starts and records the result with
//! [`set_app_layout_direction`], so that widgets created later can read it.

use std::sync::RwLock;

/// Which way the app's layout runs. See
/// [`Settings::layout_direction`](crate::Settings::layout_direction).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum LayoutDirection {
    /// The direction of the app's language ([`Settings::language`](crate::Settings::language)),
    /// or, without one, the native toolkit's own default.
    #[default]
    Auto,
    /// Left to right.
    Ltr,
    /// Right to left: the layout is mirrored.
    Rtl,
}

impl LayoutDirection {
    /// The direction a language is written in, from a BCP 47 tag (`"ar-EG"`,
    /// `"az-Arab"`). POSIX locale names (`"he_IL.UTF-8"`) are read too.
    ///
    /// A script subtag decides when there is one, so `"az-Arab"` is right to left and
    /// `"ar-Latn"` left to right; otherwise the language's usual script does.
    /// Unknown languages are left to right.
    ///
    /// ```
    /// # use naui_core::LayoutDirection;
    /// assert_eq!(LayoutDirection::for_language("ar-EG"), LayoutDirection::Rtl);
    /// assert_eq!(LayoutDirection::for_language("he_IL.UTF-8"), LayoutDirection::Rtl);
    /// assert_eq!(LayoutDirection::for_language("ja-JP"), LayoutDirection::Ltr);
    /// assert_eq!(LayoutDirection::for_language("az-Arab"), LayoutDirection::Rtl);
    /// assert_eq!(LayoutDirection::for_language("ar-Latn"), LayoutDirection::Ltr);
    /// ```
    pub fn for_language(tag: &str) -> LayoutDirection {
        // POSIX の形 (ja_JP.UTF-8@modifier) のコード集合と修飾子は方向に関係しない。
        let tag = tag.trim();
        let tag = tag.split(['.', '@']).next().unwrap_or_default();
        let mut subtags = tag.split(['-', '_']).filter(|s| !s.is_empty());
        let Some(language) = subtags.next() else {
            return LayoutDirection::Ltr;
        };
        // 文字体系は言語のすぐ後ろ (拡張言語の 3 文字が挟まることもある)。
        let script = subtags
            .take(4)
            .take_while(|s| !(s.len() == 1))
            .find(|s| s.len() == 4 && s.chars().all(|c| c.is_ascii_alphabetic()));
        let rtl = match script {
            Some(script) => RTL_SCRIPTS
                .iter()
                .any(|known| known.eq_ignore_ascii_case(script)),
            None => RTL_LANGUAGES
                .iter()
                .any(|known| known.eq_ignore_ascii_case(language)),
        };
        if rtl {
            LayoutDirection::Rtl
        } else {
            LayoutDirection::Ltr
        }
    }

    /// The direction the backends apply: `Ltr` or `Rtl`, or `None` to leave the
    /// native toolkit's default alone.
    ///
    /// `Auto` takes the direction of `language` (the app's language), and is `None`
    /// without one.
    ///
    /// ```
    /// # use naui_core::LayoutDirection;
    /// assert_eq!(LayoutDirection::Auto.resolve(Some("fa-IR")), Some(LayoutDirection::Rtl));
    /// assert_eq!(LayoutDirection::Auto.resolve(Some("en")), Some(LayoutDirection::Ltr));
    /// assert_eq!(LayoutDirection::Auto.resolve(None), None);
    /// assert_eq!(LayoutDirection::Rtl.resolve(Some("en")), Some(LayoutDirection::Rtl));
    /// ```
    pub fn resolve(self, language: Option<&str>) -> Option<LayoutDirection> {
        match self {
            LayoutDirection::Auto => language.map(LayoutDirection::for_language),
            fixed => Some(fixed),
        }
    }

    /// Whether this is [`LayoutDirection::Rtl`].
    pub fn is_rtl(self) -> bool {
        self == LayoutDirection::Rtl
    }
}

/// Languages whose usual script runs right to left (ISO 639 codes, after CLDR's
/// likely scripts). `iw` and `ji` are the old codes of Hebrew and Yiddish, which
/// Java and some OSes still hand out.
const RTL_LANGUAGES: &[&str] = &[
    "ar", "arc", "azb", "bal", "bqi", "ckb", "dv", "fa", "glk", "he", "iw", "ji", "khw", "ks",
    "lrc", "luz", "mzn", "nqo", "pnb", "prs", "ps", "sd", "sdh", "skr", "syr", "ug", "ur", "yi",
];

/// Scripts written right to left (ISO 15924 codes) that living languages use.
const RTL_SCRIPTS: &[&str] = &[
    "Adlm", "Arab", "Aran", "Hebr", "Mand", "Mend", "Nkoo", "Rohg", "Samr", "Syrc", "Thaa", "Yezi",
];

/// The direction the app's layout was given at startup.
static APP_DIRECTION: RwLock<Option<LayoutDirection>> = RwLock::new(None);

/// Records the direction the backend applied at startup (`None` when it left the
/// native default alone). Called by each backend when the app starts.
pub fn set_app_layout_direction(direction: Option<LayoutDirection>) {
    *APP_DIRECTION.write().unwrap_or_else(|e| e.into_inner()) = direction;
}

/// The direction applied at startup: `Some(Ltr)` or `Some(Rtl)`, or `None` when the
/// native default was left alone.
pub fn app_layout_direction() -> Option<LayoutDirection> {
    *APP_DIRECTION.read().unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn languages_and_scripts() {
        for tag in [
            "ar", "AR-sa", "fa-IR", "ur-PK", "he", "iw", "yi", "ps-AF", "ckb", "dv",
        ] {
            assert_eq!(
                LayoutDirection::for_language(tag),
                LayoutDirection::Rtl,
                "{tag}"
            );
        }
        for tag in [
            "en",
            "ja-JP",
            "zh-Hant-TW",
            "ku",
            "",
            "  ",
            "C",
            "x-private",
        ] {
            assert_eq!(
                LayoutDirection::for_language(tag),
                LayoutDirection::Ltr,
                "{tag}"
            );
        }
        // 文字体系が言語より優先する。
        assert_eq!(
            LayoutDirection::for_language("pa-Arab-PK"),
            LayoutDirection::Rtl
        );
        assert_eq!(
            LayoutDirection::for_language("ff-Adlm"),
            LayoutDirection::Rtl
        );
        assert_eq!(
            LayoutDirection::for_language("ku-Arab"),
            LayoutDirection::Rtl
        );
        assert_eq!(
            LayoutDirection::for_language("sd-Deva"),
            LayoutDirection::Ltr
        );
        assert_eq!(
            LayoutDirection::for_language("az-Latn-IR"),
            LayoutDirection::Ltr
        );
        // POSIX の名前。
        assert_eq!(
            LayoutDirection::for_language("ar_EG.UTF-8"),
            LayoutDirection::Rtl
        );
        // 拡張部分の 4 文字は文字体系ではない。
        assert_eq!(
            LayoutDirection::for_language("en-u-Arab"),
            LayoutDirection::Ltr
        );
    }

    #[test]
    fn fixed_directions_ignore_the_language() {
        assert_eq!(
            LayoutDirection::Ltr.resolve(Some("ar")),
            Some(LayoutDirection::Ltr)
        );
        assert_eq!(
            LayoutDirection::Rtl.resolve(None),
            Some(LayoutDirection::Rtl)
        );
        assert_eq!(LayoutDirection::default(), LayoutDirection::Auto);
    }
}
