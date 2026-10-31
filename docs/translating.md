# Translating apps

naui does not translate an app's strings itself. It provides only what needs the
native side: the OS's preferred languages
([`Ui::preferred_languages`](https://docs.rs/naui/latest/naui/struct.Ui.html#method.preferred_languages))
and a notification when they change
([`Ui::on_preferred_languages_change`](https://docs.rs/naui/latest/naui/struct.Ui.html#method.on_preferred_languages_change)).
Everything else is left to a translation library, so apps that do not translate
gain no dependencies.

This guide combines naui with [Fluent](https://projectfluent.org/) in four steps:

1. [Writing the translations](#writing-the-translations)
2. [Loading them](#loading-the-translations)
3. [Choosing one from the preferred languages](#choosing-a-translation)
4. [Re-setting strings when the language changes](#re-setting-strings-when-the-language-changes)

The complete, runnable version is
[`examples/i18n`](../examples/i18n): a window in English, Japanese and French that
follows the OS language and also lets the user pick one inside the app.

```sh
cargo run -p i18n
```

The same pattern works with gettext or any other library that turns a message ID
into a string: only steps 1 to 3 change.

## Dependencies

```toml
[dependencies]
naui = "0.14"
fluent = "0.17"
fluent-langneg = "0.13"
unic-langid = { version = "0.9", features = ["macros"] }
```

`fluent-langneg` has to be the version that `fluent` itself uses (0.13 for
`fluent` 0.17). From 0.14 it takes `icu_locid`'s identifiers instead of
`unic-langid`'s, so the identifiers `fluent` uses no longer fit. The `macros`
feature is only for writing the fallback language as a constant (`langid!`).

## Writing the translations

Keep one `.ftl` file per language, named by its BCP 47 tag:

```text
locales/
  en-US/app.ftl
  ja/app.ftl
  fr/app.ftl
```

A tag that names only the language (`ja`) is enough unless regions really differ:
step 3 matches `ja-JP` to it. Messages take arguments, choose plural forms by each
language's rules, and carry attributes for a widget's secondary strings:

```ftl
# en-US/app.ftl
greeting = Hello, { $name }!
name-input =
    .placeholder = Your name
clicks =
    { $count ->
        [0] Not clicked yet
        [one] Clicked once
       *[other] Clicked { $count } times
    }
```

```ftl
# ja/app.ftl
greeting = こんにちは、{ $name } さん
name-input =
    .placeholder = お名前
clicks =
    { $count ->
        [0] まだクリックしていません
       *[other] { $count } 回クリックしました
    }
```

Japanese has no plural forms, so it needs only `other`. French counts 0 as `one`,
which the `[0]` variant catches first. See the
[Fluent syntax guide](https://projectfluent.org/fluent/guide/) for the rest.

## Loading the translations

The simplest way is to embed the files into the executable:

```rust
const LOCALES: &[(&str, &str)] = &[
    ("en-US", include_str!("../locales/en-US/app.ftl")),
    ("ja", include_str!("../locales/ja/app.ftl")),
    ("fr", include_str!("../locales/fr/app.ftl")),
];

fn load(lang: &LanguageIdentifier) -> FluentBundle<FluentResource> {
    let tag = lang.to_string();
    let (_, source) = LOCALES.iter().find(|(t, _)| *t == tag).unwrap();
    let resource = FluentResource::try_new(source.to_string())
        .unwrap_or_else(|(_, errors)| panic!("{tag}/app.ftl: {errors:?}"));
    let mut bundle = FluentBundle::new(vec![lang.clone()]);
    bundle.add_resource(resource).unwrap();
    bundle
}
```

Embedding needs nothing next to the executable, so it works the same in a
`.app`, an installer, a `.deb` and on the Web, where there is no file system.
Reading files at run time instead lets translators replace them without a rebuild,
but then each distribution needs its own path (`Contents/Resources` in a macOS
bundle, `share/<app>` on Linux, next to the `.exe` on Windows) and the Web has to
fetch them.

Parse errors in embedded files are the app's bug, so panicking is fine there. With
files read at run time, report the error and fall back to the embedded English
instead.

## Choosing a translation

`fluent-langneg` matches the preferred languages against the ones the app has:

```rust
const FALLBACK: LanguageIdentifier = langid!("en-US");

fn negotiate<S: AsRef<str>>(requested: &[S]) -> Vec<FluentBundle<FluentResource>> {
    // Skip tags that do not parse.
    let requested: Vec<LanguageIdentifier> = requested
        .iter()
        .filter_map(|tag| tag.as_ref().parse().ok())
        .collect();
    let available: Vec<LanguageIdentifier> =
        LOCALES.iter().map(|(tag, _)| tag.parse().unwrap()).collect();
    let fallback = FALLBACK;
    negotiate_languages(&requested, &available, Some(&fallback), NegotiationStrategy::Filtering)
        .into_iter()
        .map(load)
        .collect()
}

let bundles = negotiate(&ui.preferred_languages());
```

`Filtering` returns every matching language in order of preference, with the
fallback last:

| Preferred languages | Chosen |
| --- | --- |
| `ja-JP`, `en-US` | `ja`, `en-US` |
| `en-GB` | `en-US` |
| `de-DE`, `fr-CA`, `ja` | `fr`, `ja`, `en-US` |
| `de` or nothing | `en-US` |

Keep the whole list rather than only the first: looking a message up in each bundle
in turn shows English for a message a translation has not caught up with yet,
instead of its ID.

```rust
fn get(bundles: &[FluentBundle<FluentResource>], id: &str, args: Option<&FluentArgs>) -> String {
    for bundle in bundles {
        if let Some(pattern) = bundle.get_message(id).and_then(|m| m.value()) {
            let mut errors = Vec::new();
            return bundle.format_pattern(pattern, args, &mut errors).into_owned();
        }
    }
    id.to_owned() // in no language: show the ID so it is noticed
}
```

The example wraps the list in a `Translations` type with `get`, `get_args` (with
arguments) and `attr` (an attribute such as `.placeholder`), which the rest of this
guide uses.

What `preferred_languages` returns differs between platforms, and the negotiation
absorbs it:

- Windows lists the languages in Settings, whose tags may name only the language
  (`ja`).
- Linux turns `ja_JP.UTF-8` into `ja-JP`, and returns nothing under the C locale.
- On the Web it is `navigator.languages`, the browser's language settings.

## Re-setting strings when the language changes

naui has no binding mechanism: a widget shows whatever string was last passed to
`set_text`. To update the screen when the language changes, the app keeps, for each
string, a closure that sets it, and runs them all again after choosing a new
translation. The example's `Localizer` does exactly this in about 40 lines
([`l10n.rs`](../examples/i18n/src/l10n.rs)):

```rust
type Binding = Box<dyn Fn(&Translations) -> Option<()>>;

#[derive(Clone)]
pub struct Localizer(Rc<Inner>);

struct Inner {
    current: RefCell<Rc<Translations>>,
    bindings: RefCell<Vec<Binding>>,
}

impl Localizer {
    /// The translation in use, for strings built outside a binding.
    pub fn current(&self) -> Rc<Translations> {
        self.0.current.borrow().clone()
    }

    /// Calls `set` now and again every time the language changes.
    pub fn bind(&self, set: impl Fn(&Translations) -> Option<()> + 'static) {
        if set(&self.current()).is_some() {
            self.0.bindings.borrow_mut().push(Box::new(set));
        }
    }

    pub fn set_languages<S: AsRef<str>>(&self, requested: &[S]) {
        let translations = Rc::new(Translations::negotiate(requested));
        *self.0.current.borrow_mut() = translations.clone();
        // Take the list out, so a binding may call `bind` (by building widgets).
        let mut bindings = std::mem::take(&mut *self.0.bindings.borrow_mut());
        bindings.retain(|set| set(&translations).is_some());
        let mut slot = self.0.bindings.borrow_mut();
        bindings.append(&mut slot);
        *slot = bindings;
    }
}
```

Each binding holds its widget with `downgrade()`, so the `Localizer` never keeps a
closed window's widgets alive. When `upgrade()?` finds the widget gone, the
binding returns `None` and is dropped. Create widgets with an empty string and let
`bind` set the first one, so each string is written in one place:

```rust
let l10n = Localizer::new(&ui.preferred_languages());

let window = ui.window("", 380.0, 260.0)?;
l10n.bind({
    let window = window.downgrade();
    move |tr| {
        window.upgrade()?.set_title(&tr.get("window-title"));
        Some(())
    }
});

let name = ui.text_input("")?;
l10n.bind({
    let name = name.downgrade();
    move |tr| {
        name.upgrade()?.set_placeholder(&tr.attr("name-input", "placeholder"));
        Some(())
    }
});

ui.on_preferred_languages_change({
    let l10n = l10n.clone();
    move |tags| l10n.set_languages(tags)
});
```

A few kinds of strings need more than one line:

- **Strings that also depend on the app's state** (a count, the text typed into a
  field). Put the formatting in one closure, and call it both from the binding and
  from the handler that changes the state, with `l10n.current()`.

  ```rust
  let show_clicks = Rc::new({
      let count = count.clone();
      let clicks = clicks.downgrade();
      move |tr: &Translations| {
          let mut args = FluentArgs::new();
          args.set("count", count.get());
          clicks.upgrade()?.set_text(&tr.get_args("clicks", &args));
          Some(())
      }
  });
  l10n.bind({
      let show_clicks = show_clicks.clone();
      move |tr| show_clicks(tr)
  });
  button.on_click({
      let l10n = l10n.clone();
      move || {
          count.set(count.get() + 1);
          show_clicks(&l10n.current());
      }
  });
  ```

- **Widgets built from a list of items** (`ComboBox`, `RadioGroup`, `List`,
  `Toolbar`, `Sidebar`, `MenuBar`). Their strings change only by passing the whole
  list again (`set_items`, `set_menus`). `set_items` clears the selection without
  notifying, so the binding selects the same index again afterwards.

  ```rust
  l10n.bind({
      let theme = theme.downgrade();
      move |tr| {
          let theme = theme.upgrade()?;
          let selected = theme.selected();
          theme.set_items(&[tr.get("theme-light"), tr.get("theme-dark")]);
          if let Some(index) = selected {
              theme.set_selected(index);
          }
          Some(())
      }
  });
  ```

- **Strings captured when something was shown**, such as a `Dialog` or toast that
  is already open. Build them from `l10n.current()` at the moment they are shown;
  one that is open during a change keeps its language until it closes.

Fluent wraps the values put into a message (`{ $name }`) in Unicode isolation marks
(U+2068 and U+2069), so a name in a right-to-left script does not reorder the
sentence around it. Native text rendering does not draw them, but a test that
compares strings has to remove them, or call `bundle.set_use_isolating(false)`.

## Letting the user choose a language

An app can also offer its own choice of language, with "follow the OS" as one of
the options. Remember which one is chosen, pass that tag to `set_languages`, and
follow `on_preferred_languages_change` only while the choice is "follow the OS":

```rust
let choice: Rc<Cell<Option<&'static str>>> = Rc::new(Cell::new(None));

language.on_select({
    let ui = ui.clone();
    let l10n = l10n.clone();
    let choice = choice.clone();
    move |index| {
        choice.set(index.checked_sub(1).map(|i| CHOICES[i].0));
        let requested = match choice.get() {
            None => ui.preferred_languages(),
            Some(tag) => vec![tag.to_owned()],
        };
        // Re-setting rebuilds this ComboBox's own items, so wait until its
        // notification is over.
        let l10n = l10n.clone();
        ui.tasks().spawn(async move { l10n.set_languages(&requested) });
    }
});

ui.on_preferred_languages_change({
    let l10n = l10n.clone();
    move |tags| {
        if choice.get().is_none() {
            l10n.set_languages(tags);
        }
    }
});
```

Show each language's name in that language ("日本語", "Français") rather than
translating it, so a user who cannot read the current language can still find
their own. To keep the choice across launches, save the tag the same way as other
settings and pass it to `Localizer::new` instead of `ui.preferred_languages()`.

Choosing a language inside the app changes only the app's strings. The strings the
native controls carry themselves (file dialogs, date pickers, text field context
menus) and the few strings naui shows itself stay in the OS language. See
[known limitations](known-limitations.md).

## Trying other languages

The OS language can be changed for a single run:

| Platform | How |
| --- | --- |
| macOS | `cargo run -p i18n -- -AppleLanguages '(fr)'` (sets `NSLocale.preferredLanguages` for that process) |
| Linux | `LANG=fr_FR.UTF-8 cargo run -p i18n` |
| Windows | Move the language to the top in Settings → Time & language → Language & region (told to a running app right away) |
| Web | The browser's language settings |

`on_preferred_languages_change` is never called on Linux, which decides the
languages at startup. On macOS, changing the language in System Settings may
reopen the app instead of notifying it. The choice inside the app (above) is the
reliable way to check that every string is re-set.

The choosing step has no UI, so it can be unit tested directly, as the example
does (`cargo test -p i18n`).

## Dates and numbers

Fluent selects plural forms by language, but it formats numbers without the
language's digit grouping or decimal separator (`1234.5`, not `1,234.5` or
`1 234,5`), and `fluent` has no date formatting. Format these with
[ICU4X](https://github.com/unicode-org/icu4x) for the chosen language and pass the
result into the message as a string, keeping the number itself as a separate
argument where a plural form depends on it.

## Why there is no naui-i18n crate

The roadmap left open whether this glue should become an optional `naui-i18n`
crate. Having written it out, it should not, at least for now:

- Choosing and loading are a few calls to `fluent-langneg` and `fluent`, and
  where translations come from (embedded or files, which files) depends on how the
  app is distributed.
- Re-setting strings is the `Localizer` above. A crate could not make it shorter
  without its own way of naming each widget's strings, because the setters differ
  (`set_text`, `set_title`, `set_placeholder`, `set_items`) and many strings also
  depend on the app's state. A closure per string is already the general form.
- A crate would have to pick one translation library and keep up with its
  versions (as the `fluent-langneg` 0.13 / 0.14 split above shows), while apps
  that use gettext or their own tables would gain nothing.

So the glue stays in the app, and this guide plus `examples/i18n` are the starting
point to copy from. This is revisited if apps keep needing the same additions,
for example once naui can tell the native side the app's language (on the
roadmap), which an app-chosen language would also need to call.
