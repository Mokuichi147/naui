//! Looking up the notes example's strings in the user's language, with
//! [Fluent](https://projectfluent.org/). Plain Rust, without naui.
//!
//! The strings themselves are not here but in the app crate (`app/locales`), which
//! hands them to [`Translations::negotiate`]. A screen names the strings it shows by
//! their IDs and is given the [`Translations`] in its `render`. So changing a string
//! rebuilds only the app crate, and the screens depend on this crate only for the
//! lookup, which seldom changes.
//!
//! How the files are written, and how the languages are chosen, is in the guide
//! `docs/translating.md`, whose example (`examples/i18n`) this follows.

use fluent::{FluentArgs, FluentBundle, FluentResource};
use fluent_langneg::{negotiate_languages, NegotiationStrategy};
use unic_langid::LanguageIdentifier;

pub use fluent::FluentValue;

/// The translations an app bundles: each language's BCP 47 tag and its `.ftl` source.
/// The first one is used for what no other language has, so it has every string.
pub type Locales = [(&'static str, &'static str)];

/// The strings in the languages chosen, the best match first.
pub struct Translations {
    bundles: Vec<FluentBundle<FluentResource>>,
}

impl std::fmt::Debug for Translations {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let languages: Vec<String> = self
            .bundles
            .iter()
            .flat_map(|bundle| bundle.locales.iter().map(ToString::to_string))
            .collect();
        f.debug_struct("Translations")
            .field("languages", &languages)
            .finish()
    }
}

impl Translations {
    /// Chooses from `locales` the languages that match `requested`, in its order: the
    /// tags of `Ui::preferred_languages`. `ja-JP` matches `ja`, and the first of
    /// `locales` comes last, for the strings the others lack.
    pub fn negotiate<S: AsRef<str>>(requested: &[S], locales: &'static Locales) -> Self {
        // Tags that do not parse are skipped.
        let requested: Vec<LanguageIdentifier> = requested
            .iter()
            .filter_map(|tag| tag.as_ref().parse().ok())
            .collect();
        let available: Vec<LanguageIdentifier> = locales
            .iter()
            .map(|(tag, _)| tag.parse().expect("a bundled language's tag is valid"))
            .collect();
        let fallback = available.first().expect("at least one language").clone();
        let chosen = negotiate_languages(
            &requested,
            &available,
            Some(&fallback),
            NegotiationStrategy::Filtering,
        );
        let bundles = chosen.into_iter().map(|lang| load(lang, locales)).collect();
        Self { bundles }
    }

    /// A string without arguments.
    pub fn get(&self, id: &str) -> String {
        self.format(id, None)
    }

    /// A string with arguments, such as `{ $count }`, given as
    /// `[("count", 3.into())]`. Plural forms are chosen by the language's own rules.
    pub fn get_args<'a>(
        &self,
        id: &str,
        args: impl IntoIterator<Item = (&'a str, FluentValue<'a>)>,
    ) -> String {
        let mut fluent_args = FluentArgs::new();
        for (name, value) in args {
            fluent_args.set(name, value);
        }
        self.format(id, Some(&fluent_args))
    }

    fn format(&self, id: &str, args: Option<&FluentArgs>) -> String {
        for bundle in &self.bundles {
            let Some(pattern) = bundle.get_message(id).and_then(|message| message.value()) else {
                continue;
            };
            let mut errors = Vec::new();
            return bundle
                .format_pattern(pattern, args, &mut errors)
                .into_owned();
        }
        // In no language: the ID is shown, so that it is noticed.
        id.to_owned()
    }
}

fn load(lang: &LanguageIdentifier, locales: &Locales) -> FluentBundle<FluentResource> {
    let tag = lang.to_string();
    let (_, source) = locales
        .iter()
        .find(|(t, _)| *t == tag)
        .expect("negotiate_languages returns bundled languages only");
    // The files are part of the app, so a mistake in them is the app's bug.
    let resource = FluentResource::try_new(source.to_string())
        .unwrap_or_else(|(_, errors)| panic!("{tag}: {errors:?}"));
    let mut bundle = FluentBundle::new(vec![lang.clone()]);
    bundle
        .add_resource(resource)
        .unwrap_or_else(|errors| panic!("{tag}: {errors:?}"));
    bundle
}

/// What each language lacks, or has but the first does not: `(tag, id)`. A string a
/// translation lacks shows in the first language without a word, so a test of the app
/// checks that this is empty.
pub fn mismatches(locales: &Locales) -> Vec<(&'static str, String)> {
    let Some(((_, first), others)) = locales.split_first() else {
        return Vec::new();
    };
    let expected = ids(first);
    let mut found = Vec::new();
    for (tag, source) in others {
        let ids = ids(source);
        found.extend(
            expected
                .symmetric_difference(&ids)
                .map(|id| (*tag, id.clone())),
        );
    }
    found
}

/// The IDs of the messages in an `.ftl` source: the names that start a line.
fn ids(source: &str) -> std::collections::BTreeSet<String> {
    source
        .lines()
        .filter_map(|line| {
            let (name, _) = line.split_once('=')?;
            let name = name.trim_end();
            let valid = name.starts_with(|c: char| c.is_ascii_alphabetic())
                && name
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
            valid.then(|| name.to_owned())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOCALES: &Locales = &[
        (
            "en-US",
            "hello = Hello\nnotes =\n    { $count ->\n        [one] One note\n       *[other] { $count } notes\n    }\nonly-english = English\n",
        ),
        (
            "ja",
            "hello = こんにちは\nnotes = { $count } 件のメモ\nonly-japanese = 日本語\n",
        ),
    ];

    // Fluent puts what is filled in between the isolation marks U+2068 and U+2069.
    fn plain(text: String) -> String {
        text.replace(['\u{2068}', '\u{2069}'], "")
    }

    #[test]
    fn the_preferred_language_comes_first_and_the_first_bundled_one_fills_in() {
        let tr = Translations::negotiate(&["ja-JP", "en-GB"], LOCALES);
        assert_eq!(tr.get("hello"), "こんにちは");
        assert_eq!(tr.get("only-english"), "English");
        assert_eq!(tr.get("missing"), "missing");
        for requested in [&["de"][..], &[], &["C"]] {
            assert_eq!(
                Translations::negotiate(requested, LOCALES).get("hello"),
                "Hello"
            );
        }
    }

    #[test]
    fn plurals_follow_each_languages_rules() {
        let en = Translations::negotiate(&["en"], LOCALES);
        let ja = Translations::negotiate(&["ja"], LOCALES);
        let notes = |tr: &Translations, count: usize| {
            plain(tr.get_args("notes", [("count", count.into())]))
        };
        assert_eq!(notes(&en, 1), "One note");
        assert_eq!(notes(&en, 2), "2 notes");
        assert_eq!(notes(&ja, 1), "1 件のメモ");
    }

    #[test]
    fn mismatches_name_what_a_language_lacks_or_adds() {
        assert_eq!(
            mismatches(LOCALES),
            [
                ("ja", "only-english".to_owned()),
                ("ja", "only-japanese".to_owned())
            ]
        );
    }
}
