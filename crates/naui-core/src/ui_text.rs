//! The strings naui itself shows, in the user's language.
//!
//! A few strings are chosen by naui rather than by the app: the default macOS
//! main menu, the `Sidebar` toggle, the accessible names of `Breadcrumbs` and
//! `Pagination`, and the "OK" of [`DialogButtons::ok`](crate::DialogButtons::ok).
//! They follow the OS's preferred languages, falling back to English when none of
//! them is in the table.
//!
//! Each backend reads the preferred languages from the OS when the app starts and
//! passes them to [`set_ui_languages`]; widgets then take their strings from
//! [`ui_strings`]. A later change of the languages (which apps follow through
//! [`PreferredLanguages`](crate::PreferredLanguages)) does not switch them, so they
//! stay in the same language as the native controls' strings until a restart. Strings the native controls carry themselves (WinUI's
//! `NavigationView` toggle, AppKit's sidebar button) are left to the native side.
//!
//! Translating the app's own strings is not done here. That is left to libraries
//! such as Fluent or gettext.

use std::sync::RwLock;

/// The strings naui itself shows, in one language.
#[derive(Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct UiStrings {
    /// The language tag of this table (`"en"`, `"ja"` and so on).
    pub language: &'static str,
    /// The button that only closes a dialog.
    pub ok: &'static str,
    /// The title of the standard Edit menu (macOS).
    pub edit_menu: &'static str,
    /// The Edit menu's Undo item (macOS).
    pub undo: &'static str,
    /// The Edit menu's Redo item (macOS).
    pub redo: &'static str,
    /// The Edit menu's Cut item (macOS).
    pub cut: &'static str,
    /// The Edit menu's Copy item (macOS).
    pub copy: &'static str,
    /// The Edit menu's Paste item (macOS).
    pub paste: &'static str,
    /// The Edit menu's Select All item (macOS).
    pub select_all: &'static str,
    /// The label and tooltip of the sidebar and its toggle (Web and Linux).
    pub sidebar: &'static str,
    /// The accessible name of `Breadcrumbs` (Web).
    pub breadcrumbs: &'static str,
    /// The accessible name of `Pagination` (Web).
    pub pagination: &'static str,
    /// The app menu's quit item, with `{app}` standing for the app name.
    quit_template: &'static str,
}

impl UiStrings {
    /// The app menu's quit item for `app_name` ("Quit Notes" and so on).
    pub fn quit(&self, app_name: &str) -> String {
        self.quit_template.replace("{app}", app_name)
    }

    /// The table for the first language in `preferred` that naui has strings for.
    /// English if there is none.
    ///
    /// Tags may be BCP 47 (`"ja-JP"`, `"zh-Hans-CN"`) or POSIX locale names
    /// (`"ja_JP.UTF-8"`, `"C"`), which is what the OSes hand out. Only the language
    /// subtag is compared.
    ///
    /// ```
    /// # use naui_core::UiStrings;
    /// assert_eq!(UiStrings::for_languages(["fr-FR", "ja-JP"]).language, "ja");
    /// assert_eq!(UiStrings::for_languages(["fr-FR"]).language, "en");
    /// ```
    pub fn for_languages<I, S>(preferred: I) -> &'static UiStrings
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        preferred
            .into_iter()
            .find_map(|tag| table_for(tag.as_ref()))
            .unwrap_or(&ENGLISH)
    }
}

/// The table currently in use. English until a backend sets the languages.
static CURRENT: RwLock<&'static UiStrings> = RwLock::new(&ENGLISH);

/// Chooses the strings from the OS's preferred languages, most preferred first.
///
/// Called by each backend when the app starts.
pub fn set_ui_languages<I, S>(preferred: I)
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let strings = UiStrings::for_languages(preferred);
    // 書き手が途中で落ちても、表は差し替えるだけなので壊れた状態は残らない。
    *CURRENT.write().unwrap_or_else(|e| e.into_inner()) = strings;
}

/// The strings naui itself shows, in the language chosen at startup.
pub fn ui_strings() -> &'static UiStrings {
    *CURRENT.read().unwrap_or_else(|e| e.into_inner())
}

/// The table whose language matches the tag's language subtag.
fn table_for(tag: &str) -> Option<&'static UiStrings> {
    // "ja_JP.UTF-8@modifier" や "zh-Hans-CN" から言語の部分だけを取る。
    let language = tag
        .split(['-', '_', '.', '@'])
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    match language.as_str() {
        // "C" と "POSIX" は翻訳しないロケール。
        "en" | "c" | "posix" => Some(&ENGLISH),
        "ja" => Some(&JAPANESE),
        _ => None,
    }
}

const ENGLISH: UiStrings = UiStrings {
    language: "en",
    ok: "OK",
    edit_menu: "Edit",
    undo: "Undo",
    redo: "Redo",
    cut: "Cut",
    copy: "Copy",
    paste: "Paste",
    select_all: "Select All",
    sidebar: "Sidebar",
    breadcrumbs: "Breadcrumbs",
    pagination: "Pagination",
    quit_template: "Quit {app}",
};

const JAPANESE: UiStrings = UiStrings {
    language: "ja",
    ok: "OK",
    edit_menu: "編集",
    undo: "取り消す",
    redo: "やり直す",
    cut: "カット",
    copy: "コピー",
    paste: "ペースト",
    select_all: "すべてを選択",
    sidebar: "サイドバー",
    breadcrumbs: "パンくず",
    pagination: "ページ送り",
    quit_template: "{app} を終了",
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_bcp47_and_posix_tags() {
        for tag in [
            "ja",
            "ja-JP",
            "JA-jp",
            "ja_JP.UTF-8",
            "ja_JP@euro",
            "ja.UTF-8",
        ] {
            assert_eq!(UiStrings::for_languages([tag]).language, "ja", "{tag}");
        }
        for tag in ["en", "en-US", "en_GB.UTF-8", "C", "POSIX", "C.UTF-8"] {
            assert_eq!(UiStrings::for_languages([tag]).language, "en", "{tag}");
        }
    }

    #[test]
    fn takes_the_first_language_it_has() {
        assert_eq!(
            UiStrings::for_languages(["zh-Hans-CN", "ja-JP", "en-US"]).language,
            "ja"
        );
        assert_eq!(
            UiStrings::for_languages(["en-US", "ja-JP"]).language,
            "en",
            "英語が先なら日本語があっても英語"
        );
    }

    #[test]
    fn falls_back_to_english() {
        assert_eq!(UiStrings::for_languages(["de-DE", "fr"]).language, "en");
        assert_eq!(
            UiStrings::for_languages(Vec::<String>::new()).language,
            "en"
        );
        assert_eq!(UiStrings::for_languages([""]).language, "en");
    }

    #[test]
    fn set_ui_languages_switches_the_current_table() {
        set_ui_languages(["ja-JP"]);
        assert_eq!(ui_strings(), &JAPANESE);
        set_ui_languages(["de-DE"]);
        assert_eq!(ui_strings(), &ENGLISH, "表に無い言語では英語へ戻る");
    }

    #[test]
    fn quit_puts_the_app_name_in() {
        assert_eq!(ENGLISH.quit("Notes"), "Quit Notes");
        assert_eq!(JAPANESE.quit("メモ"), "メモ を終了");
    }

    #[test]
    fn every_table_fills_every_string() {
        for table in [&ENGLISH, &JAPANESE] {
            let UiStrings {
                language,
                ok,
                edit_menu,
                undo,
                redo,
                cut,
                copy,
                paste,
                select_all,
                sidebar,
                breadcrumbs,
                pagination,
                quit_template,
            } = table;
            for text in [
                language,
                ok,
                edit_menu,
                undo,
                redo,
                cut,
                copy,
                paste,
                select_all,
                sidebar,
                breadcrumbs,
                pagination,
            ] {
                assert!(!text.is_empty(), "{language}");
            }
            assert!(quit_template.contains("{app}"), "{language}");
        }
    }
}
