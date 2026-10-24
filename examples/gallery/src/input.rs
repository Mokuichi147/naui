use naui::{
    Button, Color, ColorPicker, DatePicker, DatePickerMode, DateTime, EditableComboBox,
    EventResponse, Key, KeyEvent, Label, Length, NumberInput, PasswordInput, Result, SearchInput,
    Sizing, Stack, TextArea, TextInput, Time, TimePicker, Ui, Widget,
};

use crate::parts::{self, Disabler, Form, Notice};

/// 数量と単価から合計を出して表示する。
fn show_total(count: &NumberInput, price: &NumberInput, status: &Label) {
    status.set_text(&format!("{:.2}", count.value() * price.value()));
}

/// 選べる範囲の両端を、ウィジェットの左右に置いた行。
///
/// 「下限 ≤ [ウィジェット] ≤ 上限」と並べ、範囲を説明の文ではなく並びで見せる。
fn bounded(ui: &Ui, low: &str, widget: &dyn Widget, high: &str) -> Result<Stack> {
    let row = parts::row(ui)?;
    row.append(&ui.label(&format!("{low} ≤"))?);
    row.append(widget);
    row.append(&ui.label(&format!("≤ {high}"))?);
    Ok(row)
}

/// 1 行入力・複数行入力・数値・日付・時刻・色と、プレースホルダー・無効状態。
///
/// 無効状態は、画面の先頭のスイッチでこの画面の入力欄をまとめて切り替える。
///
/// 値が届いたことはトーストで知らせる。文字数・一致・候補・合計のように
/// 入力に合わせて変わり続ける表示は、実際のフォームと同じく欄の右に置く。
pub(crate) fn build(ui: &Ui, notice: &Notice) -> Result<naui::Stack> {
    let pane = parts::pane(ui)?;
    // 画面全体に効く操作は、画面の先頭の 1 行にまとめる。
    let header = parts::row(ui)?;
    header.set_spacing(16.0);
    let disabler = Disabler::new(ui, &header)?;
    // 押したときの処理は、戻す先の欄をすべて作ってから付ける。
    let clear = ui.button("Reset all fields")?;
    disabler.add(&clear, Button::set_enabled);
    header.append(&clear);
    pane.append(&header);

    parts::section(ui, &pane, "Text")?;
    let form = Form::new(ui, &pane)?;

    // 打つたびの変更 (on_change)、Enter での確定 (on_submit)、
    // フォーカスの出入り (on_focus_change) を受け取る。
    let input = ui.text_input("")?;
    disabler.add(&input, TextInput::set_enabled);
    input.set_placeholder("Placeholder");
    input.set_sizing(Sizing::fill_width());
    input.on_change({
        let notice = notice.clone();
        move |text| {
            if text.is_empty() {
                notice.show("Text: (empty)");
            } else {
                notice.show(&format!("Text: {text}"));
            }
        }
    });
    input.on_submit({
        let notice = notice.clone();
        move |text| notice.show(&format!("Submitted: {text}"))
    });
    // 欄を離れたときに中身を確かめる。
    let input_status = parts::readout(ui, "Not focused")?;
    // 通知は欄が持つので、欄自身は弱参照で捕まえる (循環させない)。
    input.on_focus_change({
        let input = input.downgrade();
        let input_status = input_status.clone();
        move |focused| {
            let Some(input) = input.upgrade() else {
                return;
            };
            if focused {
                input_status.set_text("Focused");
            } else if input.text().trim().is_empty() {
                input_status.set_text("Left empty");
            } else {
                input_status.set_text("Left");
            }
        }
    });
    form.row_with(ui, "TextInput", &input, &input_status)?;

    // 打った文字が伏せ字になる 1 行入力。API は TextInput と同じ。
    // 実際のログインフォームに近い見え方にするため、幅を決めて置く。
    let password_width = Sizing::new().width(Length::Fixed(200.0));
    let password_status = parts::readout(ui, "")?;
    let password = ui.password_input()?;
    disabler.add(&password, PasswordInput::set_enabled);
    password.set_placeholder("Password");
    password.set_sizing(password_width);
    let confirm = ui.password_input()?;
    disabler.add(&confirm, PasswordInput::set_enabled);
    confirm.set_placeholder("Confirm");
    confirm.set_sizing(password_width);
    // 画面に出すのは長さと一致だけ。中身は表示しない。両方の欄の通知から
    // 呼ぶので、どちらも弱参照で捕まえる。
    let describe_password = {
        let password = password.downgrade();
        let confirm = confirm.downgrade();
        let password_status = password_status.clone();
        move || {
            let (Some(password), Some(confirm)) = (password.upgrade(), confirm.upgrade()) else {
                return;
            };
            let typed = password.text();
            if typed.is_empty() {
                password_status.set_text("Empty");
            } else if typed == confirm.text() {
                password_status.set_text(&format!("{} chars, match", typed.chars().count()));
            } else {
                password_status.set_text(&format!("{} chars, differ", typed.chars().count()));
            }
        }
    };
    describe_password();
    password.on_change({
        let describe_password = describe_password.clone();
        move |_| describe_password()
    });
    confirm.on_change({
        let describe_password = describe_password.clone();
        move |_| describe_password()
    });
    let passwords = parts::row(ui)?;
    passwords.append(&password);
    passwords.append(&confirm);
    form.row_with(ui, "PasswordInput", &passwords, &password_status)?;

    // 打つたびに絞り込み、Enter で確定する。確定したときは選ばれた 1 件を出す。
    let fruits = ["Apple", "Orange", "Grape", "Peach", "Pear"];
    let search_status = parts::readout(ui, "")?;
    let search = ui.search_input()?;
    disabler.add(&search, SearchInput::set_enabled);
    search.set_placeholder("Search fruit");
    search.set_sizing(Sizing::new().width(Length::Fixed(240.0)));
    let describe_search = {
        let search = search.downgrade();
        let search_status = search_status.clone();
        move || {
            let Some(search) = search.upgrade() else {
                return;
            };
            let text = search.text().to_lowercase();
            if text.is_empty() {
                search_status.set_text("");
                return;
            }
            let hits: Vec<&str> = fruits
                .iter()
                .copied()
                .filter(|name| name.to_lowercase().contains(text.as_str()))
                .collect();
            if hits.is_empty() {
                search_status.set_text("No match");
            } else {
                search_status.set_text(&hits.join(" / "));
            }
        }
    };
    describe_search();
    search.on_change({
        let describe_search = describe_search.clone();
        move |_| describe_search()
    });
    search.on_search({
        let notice = notice.clone();
        move |text| {
            if text.is_empty() {
                notice.show("Search: (empty)");
            } else {
                notice.show(&format!("Searched for {text}"));
            }
        }
    });
    // 欄の幅を決めてあるので、候補は値の列ではなく欄のすぐ右に並べる。
    let search_row = parts::row(ui)?;
    search_row.append(&search);
    search_row.append(&search_status);
    form.row(ui, "SearchInput", &search_row)?;

    // 候補から選ぶことも、候補にない値を打ち込むこともできる。値は文字列で返る。
    let city = ui.editable_combo_box()?;
    disabler.add(&city, EditableComboBox::set_enabled);
    city.set_items(&["Tokyo", "Osaka", "Sapporo", "Fukuoka", "Naha"]);
    city.set_placeholder("City");
    // 入力欄なので、中身に合わせた幅を持たない。ここで決めておく。
    city.set_sizing(Sizing::new().width(Length::Fixed(240.0)));
    city.on_change({
        let notice = notice.clone();
        let city = city.downgrade();
        move |text| {
            let Some(city) = city.upgrade() else { return };
            if text.is_empty() {
                notice.show("City: (empty)");
            } else {
                let source = match city.selected() {
                    Some(index) => format!("item {index}"),
                    None => "not in the list".to_string(),
                };
                notice.show(&format!("City: {text} ({source})"));
            }
        }
    });
    form.row(ui, "EditableComboBox", &city)?;

    // 改行・折り返し・縦スクロールに対応する複数行入力。
    let area_status = parts::readout(ui, "")?;
    let area = ui.text_area("")?;
    disabler.add(&area, TextArea::set_enabled);
    area.set_placeholder("Multiple lines of text");
    area.set_sizing(
        Sizing::new()
            .width(Length::Fill)
            .height(Length::Fixed(120.0)),
    );
    // 欄の右の表示は、いまの中身から作り直す。まとめて戻すときにも使う。
    // area の通知からも呼ぶので、area は弱参照で捕まえる。
    let describe_area = {
        let area = area.downgrade();
        let area_status = area_status.clone();
        move || {
            let Some(area) = area.upgrade() else { return };
            let text = area.text();
            let lines = if text.is_empty() {
                0
            } else {
                text.split('\n').count()
            };
            area_status.set_text(&format!("{lines} lines, {} chars", text.chars().count()));
        }
    };
    describe_area();
    area.on_change({
        let describe_area = describe_area.clone();
        move |_| describe_area()
    });
    form.row_with(ui, "TextArea", &area, &area_status)?;

    build_keyboard(ui, &pane)?;

    // 「数量 × 単価 = 合計」と式の形に並べ、合計がどこから来るかを並びで見せる。
    // 数量は 1〜99 の整数、単価は小数 2 桁で 0.05 刻み。
    parts::section(ui, &pane, "NumberInput")?;
    // 数値の欄は中身に合わせた幅を持たないので、ここで決めておく。上下の
    // ボタンや消去ボタンが並ぶぶん、1 行入力より広めに取る。
    let number_width = Sizing::new().width(Length::Fixed(160.0));
    let count = ui.number_input(1.0)?;
    disabler.add(&count, NumberInput::set_enabled);
    count.set_range(Some(1.0), Some(99.0));
    count.set_sizing(number_width);
    let price = ui.number_input(120.0)?;
    disabler.add(&price, NumberInput::set_enabled);
    price.set_decimals(2);
    price.set_step(0.05);
    price.set_range(Some(0.0), None);
    price.set_sizing(number_width);
    let total_status = parts::readout(ui, "")?;
    show_total(&count, &price, &total_status);
    // 2 つの欄の通知が互いを捕まえ合うので、どちらも弱参照にする。
    let update_total = {
        let count = count.downgrade();
        let price = price.downgrade();
        let total_status = total_status.clone();
        move || {
            if let (Some(count), Some(price)) = (count.upgrade(), price.upgrade()) {
                show_total(&count, &price, &total_status);
            }
        }
    };
    count.on_change({
        let update_total = update_total.clone();
        move |_| update_total()
    });
    price.on_change(move |_| update_total());
    let formula = parts::row(ui)?;
    formula.append(&count);
    formula.append(&ui.label("×")?);
    formula.append(&price);
    formula.append(&ui.label("=")?);
    formula.append(&total_status);
    pane.append(&formula);

    // 日付・時刻・その両方を選べる。値は年月日と時分で返る。
    parts::section(ui, &pane, "DatePicker")?;
    let form = Form::new(ui, &pane)?;

    let date = ui.date_picker(DatePickerMode::Date)?;
    disabler.add(&date, DatePicker::set_enabled);
    date.on_change({
        let notice = notice.clone();
        // 日付だけの欄なので、時刻の部分は出さずに読む。
        move |value| {
            notice.show(&format!(
                "Date: {:04}-{:02}-{:02}",
                value.year, value.month, value.day
            ));
        }
    });
    form.row(ui, "Date", &date)?;

    let time = ui.date_picker(DatePickerMode::Time)?;
    disabler.add(&time, DatePicker::set_enabled);
    time.set_value(DateTime::time(7, 30));
    time.on_change({
        let notice = notice.clone();
        move |value| notice.show(&format!("Time: {:02}:{:02}", value.hour, value.minute))
    });
    form.row(ui, "Time", &time)?;

    // 範囲を決めると、その外へは出られなくなる。今日から翌年末まで。
    let deadline = ui.date_picker(DatePickerMode::DateTime)?;
    disabler.add(&deadline, DatePicker::set_enabled);
    let today = deadline.value();
    let last_day = DateTime::new(today.year + 1, 12, 31, 23, 59);
    deadline.set_range(
        Some(DateTime::date(today.year, today.month, today.day)),
        Some(last_day),
    );
    deadline.on_change({
        let notice = notice.clone();
        move |value| notice.show(&format!("Deadline: {value}"))
    });
    form.row(
        ui,
        "Deadline",
        &bounded(
            ui,
            &format!("{:04}-{:02}-{:02}", today.year, today.month, today.day),
            &deadline,
            &format!("{:04}-12-31", last_day.year),
        )?,
    )?;

    // 時刻だけを選ばせる。値は時分 (Time) で返り、日付は持たない。
    parts::section(ui, &pane, "TimePicker")?;
    let form = Form::new(ui, &pane)?;

    let alarm = ui.time_picker()?;
    disabler.add(&alarm, TimePicker::set_enabled);
    alarm.set_value(Time::new(7, 30));
    alarm.on_change({
        let notice = notice.clone();
        move |value| notice.show(&format!("Alarm: {value}"))
    });
    form.row(ui, "Alarm", &alarm)?;

    // 範囲を決めると、その外へは出られなくなる。
    let opens = Time::new(9, 0);
    let closes = Time::new(18, 0);
    let meeting = ui.time_picker()?;
    disabler.add(&meeting, TimePicker::set_enabled);
    meeting.set_range(Some(opens), Some(closes));
    meeting.set_value(Time::new(13, 0));
    meeting.on_change({
        let notice = notice.clone();
        move |value| notice.show(&format!("Meeting: {value}"))
    });
    form.row(
        ui,
        "Meeting",
        &bounded(ui, &opens.to_string(), &meeting, &closes.to_string())?,
    )?;

    // 色を選ばせる。値は sRGB の 8 bit で返る。
    parts::section(ui, &pane, "ColorPicker")?;
    let color = ui.color_picker()?;
    disabler.add(&color, ColorPicker::set_enabled);
    color.set_value(Color::rgb(0x33, 0x66, 0xff));
    color.on_change({
        let notice = notice.clone();
        move |value| {
            notice.show(&format!(
                "Color: {value} (R {}, G {}, B {})",
                value.r, value.g, value.b
            ));
        }
    });
    // `pick` は利用者が選んだのと同じ経路なので、on_change も呼ばれる。
    let color_reset = ui.button("Default color")?;
    disabler.add(&color_reset, Button::set_enabled);
    color_reset.on_click({
        let color = color.clone();
        move || color.pick(Color::rgb(0x33, 0x66, 0xff))
    });
    let color_row = parts::row(ui)?;
    color_row.append(&color);
    color_row.append(&color_reset);
    pane.append(&color_row);

    // 先頭の「Reset all fields」で、この画面の欄を一度にすべて初期値へ戻す。
    // 初期値は、組み立て終えたいまの値。
    let initial_count = count.value();
    let initial_price = price.value();
    let initial_date = date.value();
    let initial_time = time.value();
    let initial_deadline = deadline.value();
    let initial_alarm = alarm.value();
    let initial_meeting = meeting.value();
    let initial_color = color.value();
    clear.on_click({
        let notice = notice.clone();
        move || {
            input.set_text("");
            area.set_text("");
            password.set_text("");
            confirm.set_text("");
            search.set_text("");
            city.set_text("");
            count.set_value(initial_count);
            price.set_value(initial_price);
            date.set_value(initial_date);
            time.set_value(initial_time);
            deadline.set_value(initial_deadline);
            alarm.set_value(initial_alarm);
            meeting.set_value(initial_meeting);
            color.set_value(initial_color);
            // set_text / set_value は on_change を呼ばないので、欄の右の
            // 表示はこちらで作り直す。
            describe_area();
            describe_password();
            describe_search();
            show_total(&count, &price, &total_status);
            notice.show("All fields reset");
        }
    });
    Ok(pane)
}

/// on_key_down で Enter は送信、Shift+Enter は改行にする。
///
/// IME で変換している間の Enter は届かないので、変換の確定では送信しない。
/// どのキーで何が起きるかは欄のプレースホルダーに書き、押したキーと
/// 送った文は欄の下に出す。
fn build_keyboard(ui: &Ui, pane: &Stack) -> Result<()> {
    parts::section(ui, pane, "Keyboard")?;
    let form = Form::new(ui, pane)?;

    let sent = parts::readout(ui, "—")?;
    let last_key = parts::readout(ui, "—")?;
    let message = ui.text_area("")?;
    message.set_placeholder("Enter to send, Shift+Enter for a new line");
    message.set_sizing(
        Sizing::new()
            .width(Length::Fill)
            .height(Length::Fixed(80.0)),
    );
    message.on_key_down({
        let message = message.downgrade();
        let sent = sent.clone();
        let last_key = last_key.clone();
        move |event: &KeyEvent| {
            last_key.set_text(&format!(
                "{}{:?}",
                if event.modifiers.shift { "Shift+" } else { "" },
                event.key,
            ));
            if event.key != Key::Enter || event.modifiers.shift {
                return EventResponse::Continue;
            }
            let Some(message) = message.upgrade() else {
                return EventResponse::Continue;
            };
            let text = message.text();
            if !text.trim().is_empty() {
                sent.set_text(&text.replace('\n', " ⏎ "));
                message.set_text("");
            }
            EventResponse::Handled
        }
    });
    form.row(ui, "Message", &message)?;
    form.row(ui, "Last key", &last_key)?;
    form.row(ui, "Sent", &sent)?;
    Ok(())
}
