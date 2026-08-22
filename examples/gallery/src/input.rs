use naui::{DatePickerMode, DateTime, Length, Orientation, Padding, Result, Sizing, Ui};

/// 日付だけを取り出した表示。
fn describe_date(value: DateTime) -> String {
    format!("日付: {:04}-{:02}-{:02}", value.year, value.month, value.day)
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
    pane.append(&disabled_input);
    pane.append(&disabled_area);

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
    let time_status = ui.label(&format!("時刻: {:02}:{:02}", time.value().hour, time.value().minute))?;
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

    let clear = ui.button("入力をクリア")?;
    clear.on_click({
        let input = input.clone();
        let area = area.clone();
        let input_status = input_status.clone();
        let area_status = area_status.clone();
        let time = time.clone();
        let time_status = time_status.clone();
        move || {
            input.set_text("");
            area.set_text("");
            input_status.set_text("入力値: (空)");
            area_status.set_text("0 行 / 0 文字");
            // set_value は通知しないので、表示は自分で戻す。
            time.set_value(DateTime::time(7, 30));
            time_status.set_text("時刻: 07:30");
        }
    });
    pane.append(&clear);
    Ok(pane)
}
