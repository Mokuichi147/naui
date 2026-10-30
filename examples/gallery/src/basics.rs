use std::cell::{Cell, RefCell};
use std::rc::Rc;

use naui::{
    Align, Button, Checkbox, ComboBox, Orientation, RadioGroup, Result, Slider, TextColor,
    TextStyle, Theme, Toggle, Ui, WindowPlacement, WindowState,
};

use crate::parts::{self, Disabler, Form, Notice};

/// Button、Checkbox、Toggle、RadioGroup、Slider、ProgressBar、Spinner、ComboBox とテーマ、
/// Label の文字づかい、クリップボード・URL・タイマー、ウィンドウの状態。
pub(crate) fn build(ui: &Ui, window: &naui::Window, notice: &Notice) -> Result<naui::Stack> {
    let pane = parts::pane(ui)?;
    let disabler = Disabler::new(ui, &pane)?;

    parts::section(ui, &pane, "Controls")?;
    let form = Form::new(ui, &pane)?;

    let count = Rc::new(Cell::new(0usize));
    let buttons = parts::row(ui)?;
    let click = ui.button("Click")?;
    disabler.add(&click, Button::set_enabled);
    click.on_click({
        let count = count.clone();
        let notice = notice.clone();
        move || {
            let next = count.get() + 1;
            count.set(next);
            notice.show(&format!("Clicked {next} times"));
        }
    });
    let reset = ui.button("Reset")?;
    disabler.add(&reset, Button::set_enabled);
    reset.on_click({
        let count = count.clone();
        let notice = notice.clone();
        move || {
            count.set(0);
            notice.show("Click count reset to 0");
        }
    });
    // 右端のボタンは、画面のスイッチに関係なくいつも無効。
    let disabled = ui.button("Disabled")?;
    disabled.set_enabled(false);
    buttons.append(&click);
    buttons.append(&reset);
    buttons.append(&disabled);
    form.row(ui, "Button", &buttons)?;

    let checkbox = ui.checkbox("Enable item")?;
    disabler.add(&checkbox, Checkbox::set_enabled);
    checkbox.on_toggle({
        let notice = notice.clone();
        move |checked| notice.show(if checked { "Checked" } else { "Unchecked" })
    });
    form.row(ui, "Checkbox", &checkbox)?;

    let toggle = ui.toggle("Back up")?;
    disabler.add(&toggle, Toggle::set_enabled);
    toggle.on_toggle({
        let notice = notice.clone();
        move |on| notice.show(if on { "Backup: on" } else { "Backup: off" })
    });
    // set_on はアプリ自身の操作なので on_toggle を呼ばない。
    // 押しても何も知らせが出ないことで確かめられる。
    let toggle_reset = ui.button("Turn off")?;
    disabler.add(&toggle_reset, Button::set_enabled);
    toggle_reset.on_click({
        let toggle = toggle.clone();
        move || toggle.set_on(false)
    });
    let toggle_row = parts::row(ui)?;
    toggle_row.append(&toggle);
    toggle_row.append(&toggle_reset);
    form.row(ui, "Toggle", &toggle_row)?;

    let plans = ["Free", "Standard", "Pro"];
    let plan = ui.radio_group()?;
    disabler.add(&plan, RadioGroup::set_enabled);
    plan.set_items(&plans);
    plan.set_orientation(Orientation::Horizontal);
    plan.on_select({
        let notice = notice.clone();
        move |index| {
            let name = plans.get(index).copied().unwrap_or("unknown");
            notice.show(&format!("Plan: {name}"));
        }
    });
    // clear_selection も on_select を呼ばない (set_on と同じ決まり)。
    let clear_plan = ui.button("Clear")?;
    disabler.add(&clear_plan, Button::set_enabled);
    clear_plan.on_click({
        let plan = plan.clone();
        move || plan.clear_selection()
    });
    let plan_row = parts::row(ui)?;
    plan_row.append(&plan);
    plan_row.append(&clear_plan);
    form.row(ui, "RadioGroup", &plan_row)?;

    // Slider の値を、すぐ下の ProgressBar と右端の値へ映す。
    let value = parts::readout(ui, "40%")?;
    let progress = ui.progress_bar()?;
    progress.set_value(0.4);
    progress.set_sizing(naui::Sizing::fill_width());
    let slider = ui.slider(0.0, 1.0)?;
    disabler.add(&slider, Slider::set_enabled);
    slider.set_value(0.4);
    slider.set_sizing(naui::Sizing::fill_width());
    slider.on_change({
        let progress = progress.clone();
        let value = value.clone();
        move |ratio| {
            progress.set_value(ratio);
            value.set_text(&format!("{:.0}%", ratio * 100.0));
        }
    });
    form.row_with(ui, "Slider", &slider, &value)?;
    let busy = ui.checkbox("Indeterminate")?;
    busy.on_toggle({
        let progress = progress.clone();
        move |on| progress.set_indeterminate(on)
    });
    form.row_with(ui, "ProgressBar", &progress, &busy)?;

    // 回っているかどうかを、隣のチェックボックスで切り替える。止めても場所は残る。
    let spinner = ui.spinner()?;
    let spinning = ui.checkbox("Spinning")?;
    spinning.set_checked(true);
    spinning.on_toggle({
        let spinner = spinner.clone();
        move |on| spinner.set_spinning(on)
    });
    form.row_with(ui, "Spinner", &spinner, &spinning)?;

    let theme = ui.combo_box()?;
    disabler.add(&theme, ComboBox::set_enabled);
    theme.set_items(&["System", "Light", "Dark"]);
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
                    notice.show(&format!("Could not change the theme: {error}"));
                }
            }
        }
    });
    form.row(ui, "Theme", &theme)?;

    text_styles(ui, &pane)?;
    system(ui, &pane, notice)?;
    window_controls(ui, &pane, window, notice)?;
    Ok(pane)
}

/// Label の段階と色。見本の文字そのものが、指定した段階と役割の名前になっている。
///
/// 大きさは段階で、色は役割で指定する。級数と色そのものは OS が決めるので、
/// テーマや文字サイズの設定に追従する。
fn text_styles(ui: &Ui, pane: &naui::Stack) -> Result<()> {
    parts::section(ui, pane, "Label")?;
    let styles = ui.stack(Orientation::Vertical)?;
    styles.set_spacing(4.0);
    for (style, name) in [
        (TextStyle::LargeTitle, "LargeTitle"),
        (TextStyle::Title, "Title"),
        (TextStyle::Subtitle, "Subtitle"),
        (TextStyle::Heading, "Heading"),
        (TextStyle::Body, "Body"),
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
    Ok(())
}

/// クリップボード・URL・1 秒ごとのタイマー。
fn system(ui: &Ui, pane: &naui::Stack, notice: &Notice) -> Result<()> {
    parts::section(ui, pane, "System")?;
    let form = Form::new(ui, pane)?;

    // 選んでコピーできる文。ボタンからも同じ文を書き込む。
    let quote = ui.label("naui drives each platform's native UI from one API.")?;
    quote.set_selectable(true);
    quote.set_wrap(true);
    quote.set_sizing(naui::Sizing::fill_width());
    form.row(ui, "Selectable", &quote)?;

    let pasted = parts::readout(ui, "")?;
    let clipboard_row = parts::row(ui)?;
    let copy = ui.button("Copy text")?;
    copy.on_click({
        let clipboard = ui.clipboard();
        let quote = quote.clone();
        let notice = notice.clone();
        move || {
            let notice = notice.clone();
            clipboard.set_text(&quote.text(), move |result| match result {
                Ok(()) => notice.show("Copied to the clipboard"),
                Err(error) => notice.show(&format!("Could not copy: {error}")),
            });
        }
    });
    let paste = ui.button("Read clipboard")?;
    paste.on_click({
        let clipboard = ui.clipboard();
        let pasted = pasted.clone();
        move || {
            let pasted = pasted.clone();
            clipboard.read_text(move |text| {
                pasted.set_text(text.as_deref().unwrap_or("(no text)"));
            });
        }
    });
    clipboard_row.append(&copy);
    clipboard_row.append(&paste);
    clipboard_row.append(&pasted);
    form.row(ui, "Clipboard", &clipboard_row)?;

    let open = ui.button("Open the naui repository")?;
    open.on_click({
        let ui = ui.clone();
        let notice = notice.clone();
        move || {
            if let Err(error) = ui.open_url("https://github.com/mokuichi147/naui") {
                notice.show(&format!("Could not open: {error}"));
            }
        }
    });
    form.row(ui, "URL", &open)?;

    // OS の優先言語。設定を変えると (Linux 以外は) 実行中に書き換わる。
    let languages_text = |tags: &[String]| {
        if tags.is_empty() {
            "(none)".to_string()
        } else {
            tags.join(", ")
        }
    };
    let languages = ui.label(&languages_text(&ui.preferred_languages()))?;
    languages.set_selectable(true);
    languages.set_wrap(true);
    languages.set_sizing(naui::Sizing::fill_width());
    let weak_languages = languages.downgrade();
    ui.on_preferred_languages_change(move |tags| {
        if let Some(languages) = weak_languages.upgrade() {
            languages.set_text(&languages_text(tags));
        }
    });
    form.row(ui, "Languages", &languages)?;

    let elapsed = parts::readout(ui, "0 s")?;
    let seconds = Rc::new(Cell::new(0u32));
    ui.tasks().every(std::time::Duration::from_secs(1), {
        let elapsed = elapsed.clone();
        move || {
            seconds.set(seconds.get() + 1);
            elapsed.set_text(&format!("{} s", seconds.get()));
        }
    });
    form.row(ui, "Timer", &elapsed)?;
    Ok(())
}

/// ウィンドウの最大化・最小化・全画面・位置・大きさを変えられるかどうか、
/// 大きさ・位置・状態の保存と復元。
///
/// タイトルバーのボタンや Esc で変えたときも、右端の状態が追いかける。
/// Linux と Web は位置を扱えず、Web は全画面だけ。扱えないことは押したときに
/// トーストで知らせる。
fn window_controls(
    ui: &Ui,
    pane: &naui::Stack,
    window: &naui::Window,
    notice: &Notice,
) -> Result<()> {
    parts::section(ui, pane, "Window")?;
    let form = Form::new(ui, pane)?;

    let state = parts::readout(ui, state_name(window.state()))?;
    window.on_state_change({
        let state = state.clone();
        move |value| state.set_text(state_name(value))
    });

    let states = parts::row(ui)?;
    for (title, value) in [
        ("Maximize", WindowState::Maximized),
        ("Minimize", WindowState::Minimized),
        ("Full screen", WindowState::Fullscreen),
        ("Restore", WindowState::Normal),
    ] {
        let button = ui.button(title)?;
        let weak_window = window.downgrade();
        button.on_click(move || {
            if let Some(window) = weak_window.upgrade() {
                window.set_state(value);
            }
        });
        states.append(&button);
    }
    // 状態はボタンの並びの続きに置く。値の列へ置くと、ボタンから離れた右端へ行く。
    states.append(&state);
    form.row(ui, "State", &states)?;

    let places = parts::row(ui)?;
    let read = ui.button("Read")?;
    read.on_click({
        let weak_window = window.downgrade();
        let notice = notice.clone();
        move || {
            let Some(window) = weak_window.upgrade() else {
                return;
            };
            match window.position() {
                Some((x, y)) => notice.show(&format!("Top left is at ({x:.0}, {y:.0})")),
                None => notice.show("Window position is not available here"),
            }
        }
    });
    places.append(&read);
    let corner = ui.button("Move to top left")?;
    corner.on_click({
        let weak_window = window.downgrade();
        move || {
            if let Some(window) = weak_window.upgrade() {
                window.set_position(40.0, 40.0);
            }
        }
    });
    places.append(&corner);
    form.row(ui, "Position", &places)?;

    // アプリなら終了時に設定ファイルへ書き、次の起動で `show` の前に戻す。
    // ここでは覚えておくだけにして、動かしたり最大化したりしてから戻せるようにする。
    let saved = Rc::new(RefCell::new(None::<WindowPlacement>));
    let placements = parts::row(ui)?;
    let save = ui.button("Save")?;
    save.on_click({
        let weak_window = window.downgrade();
        let saved = saved.clone();
        let notice = notice.clone();
        move || {
            if let Some(window) = weak_window.upgrade() {
                let placement = window.placement();
                notice.show(&format!("Saved \"{placement}\""));
                *saved.borrow_mut() = Some(placement);
            }
        }
    });
    placements.append(&save);
    let restore = ui.button("Restore saved")?;
    restore.on_click({
        let weak_window = window.downgrade();
        let notice = notice.clone();
        move || {
            let Some(window) = weak_window.upgrade() else {
                return;
            };
            match *saved.borrow() {
                Some(placement) => window.set_placement(&placement),
                None => notice.show("Nothing saved yet"),
            }
        }
    });
    placements.append(&restore);
    form.row(ui, "Placement", &placements)?;

    let resizable = ui.checkbox("Resizable")?;
    resizable.set_checked(window.is_resizable());
    resizable.on_toggle({
        let weak_window = window.downgrade();
        move |on| {
            if let Some(window) = weak_window.upgrade() {
                window.set_resizable(on);
            }
        }
    });
    form.row(ui, "Size", &resizable)?;
    Ok(())
}

fn state_name(state: WindowState) -> &'static str {
    match state {
        WindowState::Normal => "Normal",
        WindowState::Maximized => "Maximized",
        WindowState::Minimized => "Minimized",
        WindowState::Fullscreen => "Full screen",
    }
}

fn theme_index(theme: Theme) -> usize {
    match theme {
        Theme::System => 0,
        Theme::Light => 1,
        Theme::Dark => 2,
    }
}
