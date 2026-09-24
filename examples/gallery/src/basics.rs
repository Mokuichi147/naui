use std::cell::Cell;
use std::rc::Rc;

use naui::{
    Align, Button, Checkbox, ComboBox, Orientation, RadioGroup, Result, Slider, TextColor,
    TextStyle, Theme, Toggle, Ui,
};

use crate::parts::{self, Disabler, Notice};

/// Label、Button、Checkbox、Toggle、RadioGroup、Slider、ProgressBar、ComboBox とテーマ。
pub(crate) fn build(ui: &Ui, window: &naui::Window, notice: &Notice) -> Result<naui::Stack> {
    let pane = parts::pane(ui)?;

    let disabler = Disabler::new(
        ui,
        &pane,
        &["ボタンも選ぶ部品も、set_enabled(false) で操作を止められます。選んだ状態はそのまま残ります。"],
    )?;

    parts::section(
        ui,
        &pane,
        "Label / Button",
        &["通常・操作中・無効の状態を確認できます。右端のボタンは常に無効です。"],
    )?;

    let count = Rc::new(Cell::new(0usize));
    let buttons = ui.stack(Orientation::Horizontal)?;
    buttons.set_spacing(8.0);

    let click = ui.button("クリック")?;
    disabler.add(&click, Button::set_enabled);
    click.on_click({
        let count = count.clone();
        let notice = notice.clone();
        move || {
            let next = count.get() + 1;
            count.set(next);
            notice.show(&format!("クリック回数: {next}"));
        }
    });
    let reset = ui.button("リセット")?;
    disabler.add(&reset, Button::set_enabled);
    reset.on_click({
        let count = count.clone();
        let notice = notice.clone();
        move || {
            count.set(0);
            notice.show("クリック回数を 0 に戻しました");
        }
    });
    let disabled = ui.button("無効なボタン")?;
    disabled.set_enabled(false);
    buttons.append(&click);
    buttons.append(&reset);
    buttons.append(&disabled);
    pane.append(&buttons);

    parts::section(
        ui,
        &pane,
        "Label の文字づかい",
        &[
            "大きさは段階で、色は役割で指定します。級数と色そのものは OS が決めるので、\
             テーマや文字サイズの設定に追従します。",
        ],
    )?;

    let styles = ui.stack(Orientation::Vertical)?;
    styles.set_spacing(4.0);
    for (style, name) in [
        (TextStyle::LargeTitle, "LargeTitle"),
        (TextStyle::Title, "Title"),
        (TextStyle::Subtitle, "Subtitle"),
        (TextStyle::Heading, "Heading"),
        (TextStyle::Body, "Body (既定)"),
        (TextStyle::Caption, "Caption"),
    ] {
        let sample = ui.label(name)?;
        sample.set_style(style);
        styles.append(&sample);
    }
    // 見本も左端でそろえる (Stack の交差軸は既定が中央ぞろえ)。
    styles.set_align(Align::Start);
    pane.append(&styles);

    let colors = ui.stack(Orientation::Horizontal)?;
    colors.set_spacing(12.0);
    for (color, name) in [
        (TextColor::Default, "Default"),
        (TextColor::Secondary, "Secondary"),
        (TextColor::Accent, "Accent"),
        (TextColor::Success, "Success"),
        (TextColor::Warning, "Warning"),
        (TextColor::Danger, "Danger"),
    ] {
        let sample = ui.label(name)?;
        sample.set_color(color);
        colors.append(&sample);
    }
    pane.append(&colors);

    parts::section(
        ui,
        &pane,
        "Checkbox",
        &["入り切りを 2 択で持ちます。切り替えると通知が届きます。"],
    )?;
    let checkbox = ui.checkbox("項目を有効にする")?;
    disabler.add(&checkbox, Checkbox::set_enabled);
    checkbox.on_toggle({
        let notice = notice.clone();
        move |checked| {
            notice.show(if checked {
                "チェック状態: オン"
            } else {
                "チェック状態: オフ"
            });
        }
    });
    pane.append(&checkbox);

    parts::section(
        ui,
        &pane,
        "Toggle",
        &["チェックボックスと同じ 2 択を、スイッチの形で切り替えます。"],
    )?;
    let toggle = ui.toggle("バックアップを作る")?;
    disabler.add(&toggle, Toggle::set_enabled);
    toggle.on_toggle({
        let notice = notice.clone();
        move |on| {
            notice.show(if on {
                "バックアップ: 入"
            } else {
                "バックアップ: 切"
            });
        }
    });
    // set_on はアプリ自身の操作なので on_toggle を呼ばない。
    // 押しても何も知らせが出ないことで確かめられる。
    let toggle_reset = ui.button("切に戻す")?;
    disabler.add(&toggle_reset, Button::set_enabled);
    toggle_reset.on_click({
        let toggle = toggle.clone();
        move || toggle.set_on(false)
    });
    pane.append(&toggle);
    pane.append(&toggle_reset);

    parts::section(
        ui,
        &pane,
        "RadioGroup",
        &["候補を並べて 1 つだけ選べます。選び直すと前の選択は外れます。"],
    )?;
    let plans = ["無料", "標準", "上位"];
    let plan = ui.radio_group()?;
    disabler.add(&plan, RadioGroup::set_enabled);
    plan.set_items(&plans);
    plan.on_select({
        let notice = notice.clone();
        move |index| {
            let name = plans.get(index).copied().unwrap_or("不明");
            notice.show(&format!("プラン: {name}"));
        }
    });
    // clear_selection も on_select を呼ばない (set_on と同じ決まり)。
    let clear_plan = ui.button("選択を外す")?;
    disabler.add(&clear_plan, Button::set_enabled);
    clear_plan.on_click({
        let plan = plan.clone();
        move || plan.clear_selection()
    });
    pane.append(&plan);
    pane.append(&clear_plan);

    parts::section(
        ui,
        &pane,
        "Slider / ProgressBar",
        &["Slider の値を ProgressBar と数値表示へ反映します。"],
    )?;
    let value_status = parts::readout(ui, "値: 40%")?;
    let progress = ui.progress_bar()?;
    progress.set_value(0.4);
    let slider = ui.slider(0.0, 1.0)?;
    disabler.add(&slider, Slider::set_enabled);
    slider.set_value(0.4);
    slider.on_change({
        let progress = progress.clone();
        let value_status = value_status.clone();
        move |value| {
            progress.set_value(value);
            value_status.set_text(&format!("値: {:.0}%", value * 100.0));
        }
    });
    pane.append(&slider);
    pane.append(&progress);
    pane.append(&value_status);

    parts::section(
        ui,
        &pane,
        "ComboBox / Theme",
        &["ドロップダウンからアプリの配色を選べます。"],
    )?;
    let theme = ui.combo_box()?;
    disabler.add(&theme, ComboBox::set_enabled);
    theme.set_items(&["システム", "ライト", "ダーク"]);
    theme.set_selected(theme_index(ui.theme()));
    let weak_window = window.downgrade();
    theme.on_select({
        let notice = notice.clone();
        move |index| {
            let Some(selected) = [Theme::System, Theme::Light, Theme::Dark]
                .get(index)
                .copied()
            else {
                return;
            };
            // 切り替わったことは配色そのものでわかるので、知らせるのは失敗だけ。
            if let Some(window) = weak_window.upgrade() {
                if let Err(error) = window.set_theme(selected) {
                    notice.show(&format!("配色を変えられません: {error}"));
                }
            }
        }
    });
    pane.append(&theme);
    Ok(pane)
}

fn theme_index(theme: Theme) -> usize {
    match theme {
        Theme::System => 0,
        Theme::Light => 1,
        Theme::Dark => 2,
    }
}
