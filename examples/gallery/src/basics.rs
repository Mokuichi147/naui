use std::cell::Cell;
use std::rc::Rc;

use naui::{Orientation, Padding, Result, Theme, Ui};

/// Label、Button、Checkbox、Toggle、RadioGroup、Slider、ProgressBar、ComboBox とテーマ。
pub(crate) fn build(ui: &Ui, window: &naui::Window) -> Result<naui::Stack> {
    let pane = ui.stack(Orientation::Vertical)?;
    pane.set_spacing(12.0);
    pane.set_padding(Padding::all(12.0));

    pane.append(&ui.label("Label / Button")?);
    pane.append(&ui.label("通常・操作中・無効の状態を確認できます。")?);

    let count = Rc::new(Cell::new(0usize));
    let button_status = ui.label("クリック回数: 0")?;
    let buttons = ui.stack(Orientation::Horizontal)?;
    buttons.set_spacing(8.0);

    let click = ui.button("クリック")?;
    click.on_click({
        let count = count.clone();
        let button_status = button_status.clone();
        move || {
            let next = count.get() + 1;
            count.set(next);
            button_status.set_text(&format!("クリック回数: {next}"));
        }
    });
    let reset = ui.button("リセット")?;
    reset.on_click({
        let count = count.clone();
        let button_status = button_status.clone();
        move || {
            count.set(0);
            button_status.set_text("クリック回数: 0");
        }
    });
    let disabled = ui.button("無効なボタン")?;
    disabled.set_enabled(false);
    buttons.append(&click);
    buttons.append(&reset);
    buttons.append(&disabled);
    pane.append(&buttons);
    pane.append(&button_status);

    pane.append(&ui.label("Checkbox")?);
    let check_status = ui.label("チェック状態: オフ")?;
    let checkbox = ui.checkbox("項目を有効にする")?;
    checkbox.on_toggle({
        let check_status = check_status.clone();
        move |checked| {
            check_status.set_text(if checked {
                "チェック状態: オン"
            } else {
                "チェック状態: オフ"
            });
        }
    });
    pane.append(&checkbox);
    pane.append(&check_status);

    pane.append(&ui.label("Toggle")?);
    pane.append(&ui.label("チェックボックスと同じ 2 択を、スイッチの形で切り替えます。")?);
    let toggle_status = ui.label("バックアップ: 切")?;
    let toggle = ui.toggle("バックアップを作る")?;
    toggle.on_toggle({
        let toggle_status = toggle_status.clone();
        move |on| {
            toggle_status.set_text(if on {
                "バックアップ: 入"
            } else {
                "バックアップ: 切"
            });
        }
    });
    let toggle_reset = ui.button("切に戻す")?;
    toggle_reset.on_click({
        let toggle = toggle.clone();
        let toggle_status = toggle_status.clone();
        move || {
            // set_on は通知しないので、表示はこちらで戻す。
            toggle.set_on(false);
            toggle_status.set_text("バックアップ: 切");
        }
    });
    pane.append(&toggle);
    pane.append(&toggle_reset);
    pane.append(&toggle_status);

    pane.append(&ui.label("RadioGroup")?);
    pane.append(&ui.label("候補を並べて 1 つだけ選べます。選び直すと前の選択は外れます。")?);
    let plans = ["無料", "標準", "上位"];
    let plan_status = ui.label("プラン: 未選択")?;
    let plan = ui.radio_group()?;
    plan.set_items(&plans);
    plan.on_select({
        let plan_status = plan_status.clone();
        move |index| {
            let name = plans.get(index).copied().unwrap_or("不明");
            plan_status.set_text(&format!("プラン: {name}"));
        }
    });
    let clear_plan = ui.button("選択を外す")?;
    clear_plan.on_click({
        let plan = plan.clone();
        let plan_status = plan_status.clone();
        move || {
            // clear_selection は通知しないので、表示はこちらで戻す。
            plan.clear_selection();
            plan_status.set_text("プラン: 未選択");
        }
    });
    pane.append(&plan);
    pane.append(&clear_plan);
    pane.append(&plan_status);

    pane.append(&ui.label("Slider / ProgressBar")?);
    pane.append(&ui.label("Slider の値を ProgressBar と数値表示へ反映します。")?);
    let value_status = ui.label("値: 40%")?;
    let progress = ui.progress_bar()?;
    progress.set_value(0.4);
    let slider = ui.slider(0.0, 1.0)?;
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

    pane.append(&ui.label("ComboBox / Theme")?);
    pane.append(&ui.label("ドロップダウンからアプリの配色を選べます。")?);
    let theme_status = ui.label(&format!("現在: {}", theme_name(ui.theme())))?;
    let theme = ui.combo_box()?;
    theme.set_items(&["システム", "ライト", "ダーク"]);
    theme.set_selected(theme_index(ui.theme()));
    let weak_window = window.downgrade();
    theme.on_select({
        let theme_status = theme_status.clone();
        move |index| {
            let Some((name, selected)) = [
                ("システム", Theme::System),
                ("ライト", Theme::Light),
                ("ダーク", Theme::Dark),
            ]
            .get(index)
            .copied() else {
                return;
            };
            if let Some(window) = weak_window.upgrade() {
                if window.set_theme(selected).is_ok() {
                    theme_status.set_text(&format!("現在: {name}"));
                }
            }
        }
    });
    pane.append(&theme);
    pane.append(&theme_status);
    Ok(pane)
}

fn theme_name(theme: Theme) -> &'static str {
    match theme {
        Theme::System => "システム",
        Theme::Light => "ライト",
        Theme::Dark => "ダーク",
    }
}

fn theme_index(theme: Theme) -> usize {
    match theme {
        Theme::System => 0,
        Theme::Light => 1,
        Theme::Dark => 2,
    }
}
