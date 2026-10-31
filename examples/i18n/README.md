# i18n

An example of translating a naui app with [Fluent](https://projectfluent.org/).
It shows a window in English, Japanese and French.

```sh
cargo run -p i18n
```

It covers choosing a translation from `Ui::preferred_languages`, loading `.ftl`
files embedded into the executable, re-setting the strings on screen when
`Ui::on_preferred_languages_change` reports a change, and letting the user pick a
language inside the app. The glue between Fluent and naui is in
[`src/l10n.rs`](src/l10n.rs), and the walkthrough is in
[Translating apps](../../docs/translating.md).

To start it in another language on macOS, pass the language list for that run:

```sh
cargo run -p i18n -- -AppleLanguages '(fr)'
```

On Linux, set `LANG` (for example `LANG=fr_FR.UTF-8`).
