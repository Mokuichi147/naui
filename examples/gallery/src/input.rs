use naui::{
    Color, DatePickerMode, DateTime, Label, Length, NumberInput, Orientation, Padding, Result,
    Sizing, Time, Ui,
};

/// 日付だけを取り出した表示。
fn describe_date(value: DateTime) -> String {
    format!(
        "日付: {:04}-{:02}-{:02}",
        value.year, value.month, value.day
    )
}

/// 数量と単価から合計を出して表示する。
fn show_total(count: &NumberInput, price: &NumberInput, status: &Label) {
    let total = count.value() * price.value();
    status.set_text(&format!(
        "{} 個 × {:.2} 円 = {:.2} 円",
        count.value(),
        price.value(),
        total
    ));
}

/// 1行入力・複数行入力と、プレースホルダー・無効状態。
pub(crate) fn build(ui: &Ui) -> Result<naui::Stack> {
    let pane = ui.stack(Orientation::Vertical)?;
    pane.set_spacing(12.0);
    pane.set_padding(Padding::all(12.0));

    pane.append(&ui.label("TextInput")?);
    pane.append(&ui.label("1行入力。入力内容は変更通知から取得できます。")?);
    let input_status = ui.label("入力値: (空)")?;
    let input = ui.text_input("")?;
    input.set_placeholder("プレースホルダー");
    input.set_sizing(Sizing::fill_width());
    input.on_change({
        let input_status = input_status.clone();
        move |text| {
            if text.is_empty() {
                input_status.set_text("入力値: (空)");
            } else {
                input_status.set_text(&format!("入力値: {text}"));
            }
        }
    });
    pane.append(&input);
    pane.append(&input_status);

    pane.append(&ui.label("TextArea")?);
    pane.append(&ui.label("改行・折り返し・縦スクロールに対応する複数行入力です。")?);
    let area_status = ui.label("0 行 / 0 文字")?;
    let area = ui.text_area("")?;
    area.set_placeholder("複数行のテキストを入力");
    area.set_sizing(
        Sizing::new()
            .width(Length::Fill)
            .height(Length::Fixed(150.0)),
    );
    area.on_change({
        let area_status = area_status.clone();
        move |text| {
            let lines = if text.is_empty() {
                0
            } else {
                text.split('\n').count()
            };
            area_status.set_text(&format!("{lines} 行 / {} 文字", text.chars().count()));
        }
    });
    pane.append(&area);
    pane.append(&area_status);

    pane.append(&ui.label("PasswordInput")?);
    pane.append(&ui.label("打った文字が伏せ字になる 1 行入力です。API は TextInput と同じです。")?);
    // 実際のログインフォームに近い見え方にするため、幅を決めて置く。
    let password_width = Sizing::new().width(Length::Fixed(240.0));
    let password_status = ui.label("パスワード: 未入力")?;
    let password = ui.password_input()?;
    password.set_placeholder("パスワード");
    password.set_sizing(password_width);
    let confirm = ui.password_input()?;
    confirm.set_placeholder("パスワード (確認)");
    confirm.set_sizing(password_width);
    // 画面に出すのは長さと一致だけ。中身は表示しない。
    let describe_password = {
        let password = password.clone();
        let confirm = confirm.clone();
        let password_status = password_status.clone();
        move || {
            let typed = password.text();
            if typed.is_empty() {
                password_status.set_text("パスワード: 未入力");
            } else if typed == confirm.text() {
                password_status.set_text(&format!(
                    "{} 文字 / 確認と一致しています",
                    typed.chars().count()
                ));
            } else {
                password_status.set_text(&format!(
                    "{} 文字 / 確認と一致しません",
                    typed.chars().count()
                ));
            }
        }
    };
    password.on_change({
        let describe_password = describe_password.clone();
        move |_| describe_password()
    });
    confirm.on_change({
        let describe_password = describe_password.clone();
        move |_| describe_password()
    });
    pane.append(&password);
    pane.append(&confirm);
    pane.append(&password_status);

    pane.append(&ui.label("SearchInput")?);
    pane.append(&ui.label("検索の欄です。打つたびに絞り込み、Enter で確定します。")?);
    // 絞り込む対象。確定したときは選ばれた 1 件を出す。
    let fruits = ["りんご", "みかん", "ぶどう", "もも", "なし"];
    let search_status = ui.label("候補: りんご / みかん / ぶどう / もも / なし")?;
    let search = ui.search_input()?;
    search.set_placeholder("検索");
    search.set_sizing(Sizing::new().width(Length::Fixed(240.0)));
    search.on_change({
        let search_status = search_status.clone();
        move |text| {
            let hits: Vec<&str> = fruits
                .iter()
                .copied()
                .filter(|name| name.contains(text))
                .collect();
            if hits.is_empty() {
                search_status.set_text("候補: (なし)");
            } else {
                search_status.set_text(&format!("候補: {}", hits.join(" / ")));
            }
        }
    });
    search.on_search({
        let search_status = search_status.clone();
        move |text| {
            if text.is_empty() {
                search_status.set_text("検索: (空)");
            } else {
                search_status.set_text(&format!("検索: {text} を探しました"));
            }
        }
    });
    pane.append(&search);
    pane.append(&search_status);

    pane.append(&ui.label("EditableComboBox")?);
    pane.append(&ui.label(
        "候補から選ぶことも、候補にない値を打ち込むこともできる入力欄です。値は文字列で返ります。",
    )?);
    let city_status = ui.label("都市: (空)")?;
    let city = ui.editable_combo_box()?;
    city.set_items(&["東京", "大阪", "札幌", "福岡", "那覇"]);
    city.set_placeholder("都市名");
    // 入力欄なので、中身に合わせた幅を持たない。ここで決めておく。
    city.set_sizing(Sizing::new().width(Length::Fixed(240.0)));
    city.on_change({
        let city_status = city_status.clone();
        let city = city.clone();
        move |text| {
            if text.is_empty() {
                city_status.set_text("都市: (空)");
            } else {
                let source = match city.selected() {
                    Some(index) => format!("候補 {index} と一致"),
                    None => "候補にない値".to_string(),
                };
                city_status.set_text(&format!("都市: {text} ({source})"));
            }
        }
    });
    pane.append(&city);
    pane.append(&city_status);

    pane.append(&ui.label("選ばせない状態にもできます。")?);
    let city_disabled = ui.editable_combo_box()?;
    city_disabled.set_items(&["東京", "大阪"]);
    city_disabled.set_selected(0);
    city_disabled.set_enabled(false);
    city_disabled.set_sizing(Sizing::new().width(Length::Fixed(240.0)));
    pane.append(&city_disabled);

    pane.append(&ui.label("NumberInput")?);
    pane.append(&ui.label("数値の入力欄です。範囲・刻み・小数桁を指定できます。")?);
    // 数値の欄は中身に合わせた幅を持たないので、ここで決めておく。上下の
    // ボタンや消去ボタンが並ぶぶん、1 行入力より広めに取る。
    let number_width = Sizing::new().width(Length::Fixed(200.0));
    let count = ui.number_input(1.0)?;
    count.set_range(Some(1.0), Some(99.0));
    count.set_sizing(number_width);
    let price = ui.number_input(120.0)?;
    price.set_decimals(2);
    price.set_step(0.05);
    price.set_range(Some(0.0), None);
    price.set_sizing(number_width);
    let total_status = ui.label("")?;
    show_total(&count, &price, &total_status);
    count.on_change({
        let count = count.clone();
        let price = price.clone();
        let total_status = total_status.clone();
        move |_| show_total(&count, &price, &total_status)
    });
    price.on_change({
        let count = count.clone();
        let price = price.clone();
        let total_status = total_status.clone();
        move |_| show_total(&count, &price, &total_status)
    });
    pane.append(&ui.label("数量 (1〜99 の整数)")?);
    pane.append(&count);
    pane.append(&ui.label("単価 (小数 2 桁、0.05 刻み)")?);
    pane.append(&price);
    pane.append(&total_status);

    pane.append(&ui.label("無効状態")?);
    let disabled_input = ui.text_input("編集できない1行入力")?;
    disabled_input.set_enabled(false);
    disabled_input.set_sizing(Sizing::fill_width());
    let disabled_area = ui.text_area("編集できない複数行入力")?;
    disabled_area.set_enabled(false);
    disabled_area.set_sizing(
        Sizing::new()
            .width(Length::Fill)
            .height(Length::Fixed(80.0)),
    );
    let disabled_number = ui.number_input(42.0)?;
    disabled_number.set_enabled(false);
    disabled_number.set_sizing(number_width);
    let disabled_password = ui.password_input()?;
    disabled_password.set_text("ひみつ");
    disabled_password.set_enabled(false);
    disabled_password.set_sizing(password_width);
    pane.append(&disabled_input);
    pane.append(&disabled_area);
    pane.append(&disabled_password);
    pane.append(&disabled_number);

    pane.append(&ui.label("DatePicker")?);
    pane.append(&ui.label("日付・時刻・その両方を選べます。値は年月日と時分で返ります。")?);

    let date = ui.date_picker(DatePickerMode::Date)?;
    // 日付だけの表示なので、時刻の部分は出さずに読む。
    let date_status = ui.label(&describe_date(date.value()))?;
    date.on_change({
        let date_status = date_status.clone();
        move |value| date_status.set_text(&describe_date(value))
    });
    pane.append(&date);
    pane.append(&date_status);

    let time = ui.date_picker(DatePickerMode::Time)?;
    time.set_value(DateTime::time(7, 30));
    let time_status = ui.label(&format!(
        "時刻: {:02}:{:02}",
        time.value().hour,
        time.value().minute
    ))?;
    time.on_change({
        let time_status = time_status.clone();
        move |value| {
            time_status.set_text(&format!("時刻: {:02}:{:02}", value.hour, value.minute));
        }
    });
    pane.append(&time);
    pane.append(&time_status);

    // 範囲を決めると、その外へは出られなくなる。
    let deadline = ui.date_picker(DatePickerMode::DateTime)?;
    let today = deadline.value();
    deadline.set_range(
        Some(DateTime::date(today.year, today.month, today.day)),
        Some(DateTime::new(today.year + 1, 12, 31, 23, 59)),
    );
    let deadline_status = ui.label(&format!("期限: {}", deadline.value()))?;
    deadline.on_change({
        let deadline_status = deadline_status.clone();
        move |value| deadline_status.set_text(&format!("期限: {value}"))
    });
    pane.append(&ui.label("今日から翌年末までしか選べない DateTime の例です。")?);
    pane.append(&deadline);
    pane.append(&deadline_status);

    pane.append(&ui.label("選ばせない状態にもできます。")?);
    let disabled_date = ui.date_picker(DatePickerMode::Date)?;
    disabled_date.set_enabled(false);
    pane.append(&disabled_date);

    pane.append(&ui.label("TimePicker")?);
    pane.append(&ui.label("時刻だけを選ばせます。値は時分 (Time) で返り、日付は持ちません。")?);

    let alarm = ui.time_picker()?;
    alarm.set_value(Time::new(7, 30));
    let alarm_status = ui.label(&format!("起床: {}", alarm.value()))?;
    alarm.on_change({
        let alarm_status = alarm_status.clone();
        move |value| alarm_status.set_text(&format!("起床: {value}"))
    });
    pane.append(&alarm);
    pane.append(&alarm_status);

    // 範囲を決めると、その外へは出られなくなる。
    pane.append(&ui.label("9:00〜18:00 しか選べない例です。")?);
    let meeting = ui.time_picker()?;
    meeting.set_range(Some(Time::new(9, 0)), Some(Time::new(18, 0)));
    meeting.set_value(Time::new(13, 0));
    let meeting_status = ui.label(&format!("会議: {}", meeting.value()))?;
    meeting.on_change({
        let meeting_status = meeting_status.clone();
        move |value| meeting_status.set_text(&format!("会議: {value}"))
    });
    pane.append(&meeting);
    pane.append(&meeting_status);

    pane.append(&ui.label("選ばせない状態にもできます。")?);
    let disabled_time = ui.time_picker()?;
    disabled_time.set_value(Time::new(0, 0));
    disabled_time.set_enabled(false);
    pane.append(&disabled_time);

    pane.append(&ui.label("ColorPicker")?);
    pane.append(&ui.label("色を選ばせます。値は sRGB の 8 bit で返ります。")?);

    let color = ui.color_picker()?;
    color.set_value(Color::rgb(0x33, 0x66, 0xff));
    let color_status = ui.label(&format!("色: {}", color.value()))?;
    color.on_change({
        let color_status = color_status.clone();
        move |value| {
            color_status.set_text(&format!(
                "色: {value} (R {}, G {}, B {})",
                value.r, value.g, value.b
            ));
        }
    });
    pane.append(&color);
    pane.append(&color_status);

    // `pick` は利用者が選んだのと同じ経路なので、表示も通知経由で直る。
    let color_reset = ui.button("既定の色に戻す")?;
    color_reset.on_click({
        let color = color.clone();
        move || color.pick(Color::rgb(0x33, 0x66, 0xff))
    });
    pane.append(&color_reset);

    pane.append(&ui.label("選ばせない状態にもできます。")?);
    let disabled_color = ui.color_picker()?;
    disabled_color.set_value(Color::rgb(0x88, 0x88, 0x88));
    disabled_color.set_enabled(false);
    pane.append(&disabled_color);

    let clear = ui.button("入力をクリア")?;
    clear.on_click({
        let input = input.clone();
        let area = area.clone();
        let input_status = input_status.clone();
        let area_status = area_status.clone();
        let time = time.clone();
        let time_status = time_status.clone();
        let alarm = alarm.clone();
        let alarm_status = alarm_status.clone();
        let password = password.clone();
        let confirm = confirm.clone();
        let password_status = password_status.clone();
        move || {
            input.set_text("");
            area.set_text("");
            input_status.set_text("入力値: (空)");
            area_status.set_text("0 行 / 0 文字");
            password.set_text("");
            confirm.set_text("");
            password_status.set_text("パスワード: 未入力");
            // set_value は通知しないので、表示は自分で戻す。
            time.set_value(DateTime::time(7, 30));
            time_status.set_text("時刻: 07:30");
            alarm.set_value(Time::new(7, 30));
            alarm_status.set_text("起床: 07:30");
        }
    });
    pane.append(&clear);
    Ok(pane)
}
