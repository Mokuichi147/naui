//! Fluent と naui をつなぐ部分。翻訳の選択と読み込み、言語が変わったときの
//! 文字列の張り直しをまとめる。

use std::cell::RefCell;
use std::rc::Rc;

use fluent::{FluentArgs, FluentBundle, FluentResource};
use fluent_langneg::{negotiate_languages, NegotiationStrategy};
use unic_langid::{langid, LanguageIdentifier};

/// 同梱する翻訳。`include_str!` で実行ファイルへ埋め込むので、配布物にファイルを
/// 添える必要がなく、Web でもそのまま動く。
const LOCALES: &[(&str, &str)] = &[
    ("en-US", include_str!("../locales/en-US/app.ftl")),
    ("ja", include_str!("../locales/ja/app.ftl")),
    ("fr", include_str!("../locales/fr/app.ftl")),
];

/// どれも合わないときと、訳のない文字列を引くときの言語。
const FALLBACK: LanguageIdentifier = langid!("en-US");

/// 同梱している言語。
pub fn available() -> Vec<LanguageIdentifier> {
    LOCALES
        .iter()
        .map(|(tag, _)| tag.parse().expect("同梱する言語のタグは正しい"))
        .collect()
}

/// 選んだ言語の訳。合った言語から順に並べたバンドルを持ち、先頭にない文字列は
/// 次のバンドルから引く。
pub struct Translations {
    bundles: Vec<FluentBundle<FluentResource>>,
}

impl Translations {
    /// 優先する言語 (`Ui::preferred_languages` の BCP 47 タグ) に合う訳を選ぶ。
    pub fn negotiate<S: AsRef<str>>(requested: &[S]) -> Self {
        // 読めないタグは飛ばす。
        let requested: Vec<LanguageIdentifier> = requested
            .iter()
            .filter_map(|tag| tag.as_ref().parse().ok())
            .collect();
        let available = available();
        let fallback = FALLBACK;
        // Filtering は合う言語をすべて優先順に返し、最後に FALLBACK を足す。
        // "ja-JP" は "ja" に、"en-GB" は "en-US" に合う。
        let chosen = negotiate_languages(
            &requested,
            &available,
            Some(&fallback),
            NegotiationStrategy::Filtering,
        );
        let bundles = chosen.into_iter().map(load).collect();
        Translations { bundles }
    }

    /// 引数のない文字列。
    pub fn get(&self, id: &str) -> String {
        self.format(id, None, None)
    }

    /// `{ $name }` などの引数を埋める文字列。複数形の選び分けも言語ごとに行われる。
    pub fn get_args(&self, id: &str, args: &FluentArgs) -> String {
        self.format(id, None, Some(args))
    }

    /// `.placeholder` などの属性。
    pub fn attr(&self, id: &str, attr: &str) -> String {
        self.format(id, Some(attr), None)
    }

    fn format(&self, id: &str, attr: Option<&str>, args: Option<&FluentArgs>) -> String {
        for bundle in &self.bundles {
            let Some(message) = bundle.get_message(id) else {
                continue;
            };
            let pattern = match attr {
                Some(attr) => message.get_attribute(attr).map(|a| a.value()),
                None => message.value(),
            };
            if let Some(pattern) = pattern {
                let mut errors = Vec::new();
                return bundle
                    .format_pattern(pattern, args, &mut errors)
                    .into_owned();
            }
        }
        // どの言語にもない。画面で気づけるように ID をそのまま出す。
        match attr {
            Some(attr) => format!("{id}.{attr}"),
            None => id.to_owned(),
        }
    }
}

fn load(lang: &LanguageIdentifier) -> FluentBundle<FluentResource> {
    let tag = lang.to_string();
    let (_, source) = LOCALES
        .iter()
        .find(|(t, _)| *t == tag)
        .expect("negotiate_languages は同梱する言語だけを返す");
    let resource = FluentResource::try_new(source.to_string())
        .unwrap_or_else(|(_, errors)| panic!("{tag}/app.ftl を読めない: {errors:?}"));
    let mut bundle = FluentBundle::new(vec![lang.clone()]);
    bundle
        .add_resource(resource)
        .unwrap_or_else(|errors| panic!("{tag}/app.ftl に重複がある: {errors:?}"));
    bundle
}

type Binding = Box<dyn Fn(&Translations) -> Option<()>>;

/// いまの訳と、言語が変わったときに文字列を張り直す処理を持つ。
/// `clone()` は同じ中身への参照を増やすだけなので、コールバックへ渡せる。
#[derive(Clone)]
pub struct Localizer(Rc<Inner>);

struct Inner {
    current: RefCell<Rc<Translations>>,
    bindings: RefCell<Vec<Binding>>,
}

impl Localizer {
    pub fn new<S: AsRef<str>>(requested: &[S]) -> Self {
        Localizer(Rc::new(Inner {
            current: RefCell::new(Rc::new(Translations::negotiate(requested))),
            bindings: RefCell::new(Vec::new()),
        }))
    }

    /// いまの訳。
    pub fn current(&self) -> Rc<Translations> {
        self.0.current.borrow().clone()
    }

    /// 文字列を設定する処理を登録する。すぐに一度呼び、言語が変わるたびに呼び直す。
    ///
    /// ウィジェットは `downgrade()` で持ち、`upgrade()?` で取り出す。`None` を
    /// 返すと (ウィジェットが閉じられて消えていたら) 登録を外す。
    pub fn bind(&self, set: impl Fn(&Translations) -> Option<()> + 'static) {
        if set(&self.current()).is_some() {
            self.0.bindings.borrow_mut().push(Box::new(set));
        }
    }

    /// 言語を選び直して、登録した文字列をすべて張り直す。
    pub fn set_languages<S: AsRef<str>>(&self, requested: &[S]) {
        let translations = Rc::new(Translations::negotiate(requested));
        *self.0.current.borrow_mut() = translations.clone();
        // 張り直しの途中で bind が呼ばれても (ウィジェットを作り直すなど) 借用が
        // ぶつからないよう、一覧を取り出してから回す。
        let mut bindings = std::mem::take(&mut *self.0.bindings.borrow_mut());
        bindings.retain(|set| set(&translations).is_some());
        let mut slot = self.0.bindings.borrow_mut();
        bindings.append(&mut slot);
        *slot = bindings;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fluent::fluent_args;

    // Fluent は差し込んだ値を双方向の分離記号 (U+2068 / U+2069) で囲む。
    fn plain(text: String) -> String {
        text.replace(['\u{2068}', '\u{2069}'], "")
    }

    #[test]
    fn a_tag_with_a_region_matches_the_language_only_translation() {
        let tr = Translations::negotiate(&["ja-JP"]);
        assert_eq!(tr.get("click-button"), "クリック");
    }

    #[test]
    fn another_region_matches_the_bundled_region() {
        let tr = Translations::negotiate(&["en-GB"]);
        assert_eq!(tr.get("click-button"), "Click");
    }

    #[test]
    fn an_unmatched_language_falls_through_to_the_next_one() {
        let tr = Translations::negotiate(&["de-DE", "fr-CA", "ja"]);
        assert_eq!(tr.get("click-button"), "Cliquer");
    }

    #[test]
    fn nothing_matching_falls_back_to_english() {
        for requested in [&["de"][..], &[], &["C"]] {
            let tr = Translations::negotiate(requested);
            assert_eq!(tr.get("click-button"), "Click");
        }
    }

    #[test]
    fn plurals_follow_each_languages_rules() {
        let en = Translations::negotiate(&["en-US"]);
        let fr = Translations::negotiate(&["fr"]);
        let ja = Translations::negotiate(&["ja"]);
        let clicks =
            |tr: &Translations, n: u32| plain(tr.get_args("clicks", &fluent_args!["count" => n]));
        assert_eq!(clicks(&en, 1), "Clicked once");
        assert_eq!(clicks(&en, 2), "Clicked 2 times");
        assert_eq!(clicks(&fr, 1), "Cliqué une fois");
        assert_eq!(clicks(&fr, 2), "Cliqué 2 fois");
        assert_eq!(clicks(&ja, 0), "まだクリックしていません");
        assert_eq!(clicks(&ja, 1), "1 回クリックしました");
    }

    #[test]
    fn attributes_can_be_looked_up() {
        let tr = Translations::negotiate(&["ja"]);
        assert_eq!(tr.attr("name-input", "placeholder"), "お名前");
    }

    #[test]
    fn rebinding_drops_bindings_whose_widget_is_gone() {
        let l10n = Localizer::new(&["en-US"]);
        let alive = Rc::new(RefCell::new(String::new()));
        let gone: Rc<RefCell<String>> = Rc::new(RefCell::new(String::new()));
        let calls = Rc::new(std::cell::Cell::new(0));
        l10n.bind({
            let alive = Rc::downgrade(&alive);
            move |tr| {
                *alive.upgrade()?.borrow_mut() = tr.get("click-button");
                Some(())
            }
        });
        l10n.bind({
            let gone = Rc::downgrade(&gone);
            let calls = calls.clone();
            move |tr| {
                calls.set(calls.get() + 1);
                *gone.upgrade()?.borrow_mut() = tr.get("click-button");
                Some(())
            }
        });
        assert_eq!(*alive.borrow(), "Click");
        drop(gone);
        l10n.set_languages(&["ja"]);
        l10n.set_languages(&["fr"]);
        assert_eq!(*alive.borrow(), "Cliquer");
        // 登録時と、消えたことに気づいた 1 回目の張り直しだけ呼ばれる。
        assert_eq!(calls.get(), 2);
    }
}
