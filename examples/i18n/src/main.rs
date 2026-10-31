//! Fluent で naui のアプリを翻訳するサンプル。OS の優先言語から訳を選び、
//! 言語が変わったら表示中の文字列を張り直す。
//!
//! ```sh
//! cargo run -p i18n
//! ```

// Windows のリリースビルドでは、起動時にコンソール画面を出さない。
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod l10n;

use std::cell::Cell;
use std::rc::Rc;

use fluent::FluentArgs;
use naui::{Orientation, Padding, Settings};

use crate::l10n::{Localizer, Translations};

/// 言語の選択肢。どの言語で表示していても、その言語自身の名前で出す。
const CHOICES: &[(&str, &str)] = &[("en-US", "English"), ("ja", "日本語"), ("fr", "Français")];

fn main() -> naui::Result<()> {
    naui::run(Settings::new("i18n"), |ui| {
        let l10n = Localizer::new(&ui.preferred_languages());
        // アプリの中で選んだ言語。None なら OS の設定に従う。
        let choice: Rc<Cell<Option<&'static str>>> = Rc::new(Cell::new(None));

        // 文字列は空で作り、bind がすぐに今の言語で設定する。
        let window = ui.window("", 380.0, 260.0)?;
        l10n.bind({
            let window = window.downgrade();
            move |tr| {
                window.upgrade()?.set_title(&tr.get("window-title"));
                Some(())
            }
        });

        let stack = ui.stack(Orientation::Vertical)?;
        stack.set_spacing(12.0);
        stack.set_padding(Padding::all(24.0));

        // 挨拶は名前と言語のどちらが変わっても書き直す。
        let name = ui.text_input("")?;
        let greeting = ui.label("")?;
        l10n.bind({
            let name = name.downgrade();
            move |tr| {
                name.upgrade()?
                    .set_placeholder(&tr.attr("name-input", "placeholder"));
                Some(())
            }
        });
        let show_greeting = Rc::new({
            let name = name.downgrade();
            let greeting = greeting.downgrade();
            move |tr: &Translations| {
                let typed = name.upgrade()?.text();
                let who = match typed.trim() {
                    "" => tr.get("anonymous"),
                    typed => typed.to_owned(),
                };
                let mut args = FluentArgs::new();
                args.set("name", who);
                greeting
                    .upgrade()?
                    .set_text(&tr.get_args("greeting", &args));
                Some(())
            }
        });
        l10n.bind({
            let show_greeting = show_greeting.clone();
            move |tr| show_greeting(tr)
        });
        name.on_change({
            let l10n = l10n.clone();
            move |_| {
                show_greeting(&l10n.current());
            }
        });

        // 回数の表示。複数形の選び分けは .ftl 側で言語ごとに書く。
        let count = Rc::new(Cell::new(0u32));
        let clicks = ui.label("")?;
        let button = ui.button("")?;
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
        l10n.bind({
            let button = button.downgrade();
            move |tr| {
                button.upgrade()?.set_text(&tr.get("click-button"));
                Some(())
            }
        });
        button.on_click({
            let l10n = l10n.clone();
            move || {
                count.set(count.get() + 1);
                show_clicks(&l10n.current());
            }
        });

        // 言語の選択。先頭は「システムの設定」。
        let language_row = ui.stack(Orientation::Horizontal)?;
        language_row.set_spacing(8.0);
        let language_label = ui.label("")?;
        let language = ui.combo_box()?;
        l10n.bind({
            let language_label = language_label.downgrade();
            move |tr| {
                language_label
                    .upgrade()?
                    .set_text(&tr.get("language-label"));
                Some(())
            }
        });
        l10n.bind({
            let language = language.downgrade();
            let choice = choice.clone();
            move |tr| {
                let language = language.upgrade()?;
                let mut items = vec![tr.get("language-system")];
                items.extend(CHOICES.iter().map(|(_, name)| name.to_string()));
                // 項目を作り直すと選択が消えるので、選び直す。
                language.set_items(&items);
                let index = match choice.get() {
                    None => 0,
                    Some(tag) => 1 + CHOICES.iter().position(|(t, _)| *t == tag)?,
                };
                language.set_selected(index);
                Some(())
            }
        });
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
                // 張り直しはこの ComboBox 自身の項目も作り直すので、選択の通知が
                // 終わってから行う。
                let l10n = l10n.clone();
                ui.tasks()
                    .spawn(async move { l10n.set_languages(&requested) });
            }
        });

        // OS の言語が変わったら、アプリの中で言語を選んでいないときだけ従う。
        ui.on_preferred_languages_change({
            let l10n = l10n.clone();
            move |tags| {
                if choice.get().is_none() {
                    l10n.set_languages(tags);
                }
            }
        });

        language_row.append(&language_label);
        language_row.append(&language);
        stack.append(&name);
        stack.append(&greeting);
        stack.append(&clicks);
        stack.append(&button);
        stack.append(&language_row);

        window.set_child(&stack);
        window.show();
        Ok(())
    })
}
