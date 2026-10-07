use std::cell::Cell;
use std::rc::Rc;

use naui::{
    Align, Button, Checkbox, ComboBox, Orientation, RadioGroup, Result, Slider, TextColor,
    TextStyle, Theme, Toggle, Ui,
};

use crate::parts::{self, Disabler, Notice};

/// ギャラリーの題と説明、Label、Button、Checkbox、Toggle、RadioGroup、Slider、
/// ProgressBar、ComboBox とテーマ。
///
/// 起動したときに最初に出る画面なので、ギャラリー全体の題と説明もここに置く。
pub(crate) fn build(ui: &Ui, window: &naui::Window, notice: &Notice) -> Result<naui::Stack> {
    let pane = parts::pane(ui)?;

    // 画面の顔になる見出しなので、節の見出しより 1 段大きい段階を指定する。
    let title = ui.label("naui UI ギャラリー")?;
    title.set_style(TextStyle::Title);
    pane.append(&title);
    pane.append(&parts::note(
        ui,
        "UI の種別ごとに、特徴と状態を確認できます。左のサイドバーで種別を選びます。操作の結果は画面の下端にトーストで出ます。",
    )?);

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
    let busy = ui.checkbox("処理中 (進み具合が分からない表示)")?;
    busy.on_toggle({
        let progress = progress.clone();
        move |on| progress.set_indeterminate(on)
    });
    pane.append(&busy);

    parts::section(
        ui,
        &pane,
        "クリップボード / URL / タイマー",
        &[
            "下の文字は選んでコピーできます。ボタンでクリップボードへ書き込み、読み戻せます。",
            "タイマーは 1 秒ごとに数えます。",
        ],
    )?;
    let quote = ui.label("naui は各 OS のネイティブ UI を 1 つの API から扱います。")?;
    quote.set_selectable(true);
    quote.set_wrap(true);
    quote.set_sizing(naui::Sizing::fill_width());
    pane.append(&quote);

    let clipboard_status = parts::readout(ui, "クリップボード: -")?;
    let clipboard_row = ui.stack(Orientation::Horizontal)?;
    clipboard_row.set_spacing(8.0);
    let copy = ui.button("文をコピー")?;
    copy.on_click({
        let clipboard = ui.clipboard();
        let quote = quote.clone();
        let notice = notice.clone();
        move || match clipboard.set_text(&quote.text()) {
            Ok(()) => notice.show("クリップボードへ書き込みました"),
            Err(error) => notice.show(&format!("書き込めませんでした: {error}")),
        }
    });
    let paste = ui.button("クリップボードを読む")?;
    paste.on_click({
        let clipboard = ui.clipboard();
        let status = clipboard_status.clone();
        move || {
            let status = status.clone();
            clipboard.read_text(move |text| {
                status.set_text(&format!(
                    "クリップボード: {}",
                    text.as_deref().unwrap_or("(文字なし)")
                ));
            });
        }
    });
    let open = ui.button("naui のリポジトリを開く")?;
    open.on_click({
        let ui = ui.clone();
        let notice = notice.clone();
        move || {
            if let Err(error) = ui.open_url("https://github.com/mokuichi147/naui") {
                notice.show(&format!("開けませんでした: {error}"));
            }
        }
    });
    clipboard_row.append(&copy);
    clipboard_row.append(&paste);
    clipboard_row.append(&open);
    pane.append(&clipboard_row);
    pane.append(&clipboard_status);

    let elapsed = parts::readout(ui, "経過: 0 秒")?;
    let seconds = Rc::new(Cell::new(0u32));
    ui.tasks().every(std::time::Duration::from_secs(1), {
        let elapsed = elapsed.clone();
        move || {
            seconds.set(seconds.get() + 1);
            elapsed.set_text(&format!("経過: {} 秒", seconds.get()));
        }
    });
    pane.append(&elapsed);

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
