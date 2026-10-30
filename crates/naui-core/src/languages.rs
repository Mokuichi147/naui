//! The OS's preferred languages, for apps choosing which translation to show.
//!
//! Each backend reads them when the app starts and again when the OS reports a
//! change, and hands them to [`PreferredLanguages`], which keeps the latest list
//! and notifies the app only when it actually changed.
//!
//! naui's own strings ([`ui_strings`](crate::ui_strings)) do not follow a change.
//! They sit beside the strings the native controls carry themselves, which keep the
//! language chosen at startup until the app is restarted, so switching only naui's
//! would mix two languages on one screen.

use std::cell::RefCell;

use crate::Slot;

/// The app's closure for changes.
type ChangeHandler = RefCell<Slot<dyn FnMut(&[String])>>;

/// The preferred languages as last read from the OS, and the app's closure for
/// changes.
///
/// ```
/// # use naui_core::PreferredLanguages;
/// # use std::{cell::RefCell, rc::Rc};
/// let languages = PreferredLanguages::new(vec!["ja-JP".into()]);
/// let seen = Rc::new(RefCell::new(Vec::new()));
/// let sink = seen.clone();
/// languages.on_change(move |tags| sink.borrow_mut().push(tags.to_vec()));
/// assert!(!languages.update(vec!["ja-JP".into()]), "同じなら知らせない");
/// assert!(languages.update(vec!["en-US".into(), "ja-JP".into()]));
/// assert_eq!(*seen.borrow(), [vec!["en-US", "ja-JP"]]);
/// assert_eq!(languages.get(), ["en-US", "ja-JP"]);
/// ```
#[derive(Default)]
pub struct PreferredLanguages {
    current: RefCell<Vec<String>>,
    on_change: ChangeHandler,
}

impl PreferredLanguages {
    /// Starts from the languages read when the app started.
    pub fn new(languages: Vec<String>) -> Self {
        Self {
            current: RefCell::new(languages),
            on_change: RefCell::new(None),
        }
    }

    /// The languages last read, most preferred first.
    pub fn get(&self) -> Vec<String> {
        self.current.borrow().clone()
    }

    /// Replaces the closure called when the languages change.
    pub fn on_change(&self, f: impl FnMut(&[String]) + 'static) {
        *self.on_change.borrow_mut() = Some(Box::new(f));
    }

    /// Takes the languages read again from the OS. Notifies and returns `true` only
    /// when they differ from the last ones (the order counts).
    ///
    /// The OSes also report changes that leave the languages as they were (a change
    /// of region or calendar), so those are dropped here.
    pub fn update(&self, languages: Vec<String>) -> bool {
        if *self.current.borrow() == languages {
            return false;
        }
        *self.current.borrow_mut() = languages.clone();
        let Some(mut f) = self.on_change.borrow_mut().take() else {
            return true;
        };
        f(&languages);
        // 呼び出し中に差し替えられていたら、新しいほうを残す。
        let mut slot = self.on_change.borrow_mut();
        if slot.is_none() {
            *slot = Some(f);
        }
        true
    }
}

/// Turns POSIX locale names (`"ja_JP.UTF-8"`, `"ja_JP"`, `"ja"`, `"C"`, the way
/// `g_get_language_names` lists them) into BCP 47 tags (`"ja-JP"`, `"ja"`), keeping
/// the order and dropping repeats.
///
/// The encoding and the `@modifier` are dropped, and so are `"C"` and `"POSIX"`,
/// which name no language. Tags that are already BCP 47 pass through.
///
/// ```
/// # use naui_core::language_tags_from_locale_names;
/// assert_eq!(
///     language_tags_from_locale_names(["ja_JP.UTF-8", "ja_JP", "ja.UTF-8", "ja", "C"]),
///     ["ja-JP", "ja"]
/// );
/// ```
pub fn language_tags_from_locale_names<I, S>(names: I) -> Vec<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut tags: Vec<String> = Vec::new();
    for name in names {
        // "sr_RS.UTF-8@latin" のエンコーディングと修飾子を落とす。
        let base = name
            .as_ref()
            .split(['.', '@'])
            .next()
            .unwrap_or_default()
            .trim();
        if base.is_empty() || base.eq_ignore_ascii_case("C") || base.eq_ignore_ascii_case("POSIX") {
            continue;
        }
        let tag = base.replace('_', "-");
        if !tags.contains(&tag) {
            tags.push(tag);
        }
    }
    tags
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::rc::Rc;

    #[test]
    fn only_a_real_change_is_notified() {
        let languages = PreferredLanguages::new(vec!["ja-JP".into(), "en-US".into()]);
        let count = Rc::new(RefCell::new(0));
        let sink = count.clone();
        languages.on_change(move |_| *sink.borrow_mut() += 1);
        assert!(!languages.update(vec!["ja-JP".into(), "en-US".into()]));
        assert!(
            languages.update(vec!["en-US".into(), "ja-JP".into()]),
            "並びが変わっただけでも変化"
        );
        assert!(!languages.update(vec!["en-US".into(), "ja-JP".into()]));
        assert_eq!(*count.borrow(), 1);
    }

    #[test]
    fn a_change_without_a_closure_is_still_kept() {
        let languages = PreferredLanguages::new(vec!["ja".into()]);
        assert!(languages.update(vec!["fr".into()]));
        assert_eq!(languages.get(), ["fr"]);
    }

    #[test]
    fn the_closure_may_read_and_replace_itself() {
        let languages = Rc::new(PreferredLanguages::new(Vec::new()));
        let seen = Rc::new(RefCell::new(Vec::new()));
        let (inner, sink) = (languages.clone(), seen.clone());
        languages.on_change(move |tags| {
            // 知らせの中から読み直しても借用が残っていない。
            assert_eq!(inner.get(), tags);
            let sink = sink.clone();
            inner.on_change(move |tags| sink.borrow_mut().push(format!("new {tags:?}")));
        });
        languages.update(vec!["de".into()]);
        languages.update(vec!["it".into()]);
        assert_eq!(*seen.borrow(), [r#"new ["it"]"#]);
    }

    #[test]
    fn locale_names_become_bcp47() {
        assert_eq!(
            language_tags_from_locale_names([
                "pt_BR.UTF-8",
                "pt_BR",
                "pt.UTF-8",
                "pt",
                "en_US",
                "en",
                "C"
            ]),
            ["pt-BR", "pt", "en-US", "en"]
        );
        assert_eq!(
            language_tags_from_locale_names(["sr_RS@latin", "de_DE@euro"]),
            ["sr-RS", "de-DE"]
        );
        assert_eq!(
            language_tags_from_locale_names(["zh-Hans-CN", "ja-JP"]),
            ["zh-Hans-CN", "ja-JP"],
            "BCP 47 はそのまま"
        );
        assert!(language_tags_from_locale_names(["C", "POSIX", "C.UTF-8", ""]).is_empty());
    }
}
