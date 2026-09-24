use naui::{
    Button, Color, ColorPicker, DatePicker, DatePickerMode, DateTime, EditableComboBox, Label,
    Length, NumberInput, PasswordInput, Result, SearchInput, Sizing, TextArea, TextInput, Time,
    TimePicker, Ui,
};

use crate::parts::{self, Disabler, Notice};

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
///
/// 無効状態は、画面の先頭のスイッチでこの画面の入力欄をまとめて切り替える。
///
/// 値が届いたことはトーストで知らせる。文字数・一致・候補・合計のように
/// 入力に合わせて変わり続ける表示は、実際のフォームと同じく欄のそばへ置く。
pub(crate) fn build(ui: &Ui, notice: &Notice) -> Result<naui::Stack> {
    let pane = parts::pane(ui)?;

    let disabler = Disabler::new(
        ui,
        &pane,
        &["どの入力欄も set_enabled(false) で操作を止められます。入れた値はそのまま残ります。"],
    )?;

    parts::section(
        ui,
        &pane,
        "TextInput",
        &["1行入力。入力内容は変更通知から取得できます。"],
    )?;
    let input = ui.text_input("")?;
    disabler.add(&input, TextInput::set_enabled);
    input.set_placeholder("プレースホルダー");
    input.set_sizing(Sizing::fill_width());
    input.on_change({
        let notice = notice.clone();
        move |text| {
            if text.is_empty() {
                notice.show("入力値: (空)");
            } else {
                notice.show(&format!("入力値: {text}"));
            }
        }
    });
    pane.append(&input);

    parts::section(
        ui,
        &pane,
        "TextArea",
        &["改行・折り返し・縦スクロールに対応する複数行入力です。"],
    )?;
    let area_status = parts::readout(ui, "")?;
    let area = ui.text_area("")?;
    disabler.add(&area, TextArea::set_enabled);
    area.set_placeholder("複数行のテキストを入力");
    area.set_sizing(
        Sizing::new()
            .width(Length::Fill)
            .height(Length::Fixed(150.0)),
    );
    // 欄のそばの表示は、いまの中身から作り直す。まとめて戻すときにも使う。
    let describe_area = {
        let area = area.clone();
        let area_status = area_status.clone();
        move || {
            let text = area.text();
            let lines = if text.is_empty() {
                0
            } else {
                text.split('\n').count()
            };
            area_status.set_text(&format!("{lines} 行 / {} 文字", text.chars().count()));
        }
    };
    describe_area();
    area.on_change({
        let describe_area = describe_area.clone();
        move |_| describe_area()
    });
    pane.append(&area);
    pane.append(&area_status);

    parts::section(
        ui,
        &pane,
        "PasswordInput",
        &["打った文字が伏せ字になる 1 行入力です。API は TextInput と同じです。"],
    )?;
    // 実際のログインフォームに近い見え方にするため、幅を決めて置く。
    let password_width = Sizing::new().width(Length::Fixed(240.0));
    let password_status = parts::readout(ui, "")?;
    let password = ui.password_input()?;
    disabler.add(&password, PasswordInput::set_enabled);
    password.set_placeholder("パスワード");
    password.set_sizing(password_width);
    let confirm = ui.password_input()?;
    disabler.add(&confirm, PasswordInput::set_enabled);
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
    describe_password();
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

    parts::section(
        ui,
        &pane,
        "SearchInput",
        &["検索の欄です。打つたびに絞り込み、Enter で確定します。"],
    )?;
    // 絞り込む対象。確定したときは選ばれた 1 件を出す。
    let fruits = ["りんご", "みかん", "ぶどう", "もも", "なし"];
    let search_status = parts::readout(ui, "")?;
    let search = ui.search_input()?;
    disabler.add(&search, SearchInput::set_enabled);
    search.set_placeholder("検索");
    search.set_sizing(Sizing::new().width(Length::Fixed(240.0)));
    let describe_search = {
        let search = search.clone();
        let search_status = search_status.clone();
        move || {
            let text = search.text();
            let hits: Vec<&str> = fruits
                .iter()
                .copied()
                .filter(|name| name.contains(text.as_str()))
                .collect();
            if hits.is_empty() {
                search_status.set_text("候補: (なし)");
            } else {
                search_status.set_text(&format!("候補: {}", hits.join(" / ")));
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
                notice.show("検索: (空)");
            } else {
                notice.show(&format!("検索: {text} を探しました"));
            }
        }
    });
    pane.append(&search);
    pane.append(&search_status);

    parts::section(
        ui,
        &pane,
        "EditableComboBox",
        &["候補から選ぶことも、候補にない値を打ち込むこともできる入力欄です。値は文字列で返ります。"],
    )?;
    let city = ui.editable_combo_box()?;
    disabler.add(&city, EditableComboBox::set_enabled);
    city.set_items(&["東京", "大阪", "札幌", "福岡", "那覇"]);
    city.set_placeholder("都市名");
    // 入力欄なので、中身に合わせた幅を持たない。ここで決めておく。
    city.set_sizing(Sizing::new().width(Length::Fixed(240.0)));
    city.on_change({
        let notice = notice.clone();
        let city = city.clone();
        move |text| {
            if text.is_empty() {
                notice.show("都市: (空)");
            } else {
                let source = match city.selected() {
                    Some(index) => format!("候補 {index} と一致"),
                    None => "候補にない値".to_string(),
                };
                notice.show(&format!("都市: {text} ({source})"));
            }
        }
    });
    pane.append(&city);

    parts::section(
        ui,
        &pane,
        "NumberInput",
        &["数値の入力欄です。範囲・刻み・小数桁を指定できます。"],
    )?;
    // 数値の欄は中身に合わせた幅を持たないので、ここで決めておく。上下の
    // ボタンや消去ボタンが並ぶぶん、1 行入力より広めに取る。
    let number_width = Sizing::new().width(Length::Fixed(200.0));
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

    parts::section(
        ui,
        &pane,
        "DatePicker",
        &["日付・時刻・その両方を選べます。値は年月日と時分で返ります。"],
    )?;

    let date = ui.date_picker(DatePickerMode::Date)?;
    disabler.add(&date, DatePicker::set_enabled);
    date.on_change({
        let notice = notice.clone();
        // 日付だけの欄なので、時刻の部分は出さずに読む。
        move |value| {
            notice.show(&format!(
                "日付: {:04}-{:02}-{:02}",
                value.year, value.month, value.day
            ));
        }
    });
    pane.append(&date);

    let time = ui.date_picker(DatePickerMode::Time)?;
    disabler.add(&time, DatePicker::set_enabled);
    time.set_value(DateTime::time(7, 30));
    time.on_change({
        let notice = notice.clone();
        move |value| notice.show(&format!("時刻: {:02}:{:02}", value.hour, value.minute))
    });
    pane.append(&time);

    // 範囲を決めると、その外へは出られなくなる。
    let deadline = ui.date_picker(DatePickerMode::DateTime)?;
    disabler.add(&deadline, DatePicker::set_enabled);
    let today = deadline.value();
    deadline.set_range(
        Some(DateTime::date(today.year, today.month, today.day)),
        Some(DateTime::new(today.year + 1, 12, 31, 23, 59)),
    );
    deadline.on_change({
        let notice = notice.clone();
        move |value| notice.show(&format!("期限: {value}"))
    });
    pane.append(&parts::note(
        ui,
        "今日から翌年末までしか選べない DateTime の例です。",
    )?);
    pane.append(&deadline);

    parts::section(
        ui,
        &pane,
        "TimePicker",
        &["時刻だけを選ばせます。値は時分 (Time) で返り、日付は持ちません。"],
    )?;

    let alarm = ui.time_picker()?;
    disabler.add(&alarm, TimePicker::set_enabled);
    alarm.set_value(Time::new(7, 30));
    alarm.on_change({
        let notice = notice.clone();
        move |value| notice.show(&format!("起床: {value}"))
    });
    pane.append(&alarm);

    // 範囲を決めると、その外へは出られなくなる。
    pane.append(&parts::note(ui, "9:00〜18:00 しか選べない例です。")?);
    let meeting = ui.time_picker()?;
    disabler.add(&meeting, TimePicker::set_enabled);
    meeting.set_range(Some(Time::new(9, 0)), Some(Time::new(18, 0)));
    meeting.set_value(Time::new(13, 0));
    meeting.on_change({
        let notice = notice.clone();
        move |value| notice.show(&format!("会議: {value}"))
    });
    pane.append(&meeting);

    parts::section(
        ui,
        &pane,
        "ColorPicker",
        &["色を選ばせます。値は sRGB の 8 bit で返ります。"],
    )?;

    let color = ui.color_picker()?;
    disabler.add(&color, ColorPicker::set_enabled);
    color.set_value(Color::rgb(0x33, 0x66, 0xff));
    color.on_change({
        let notice = notice.clone();
        move |value| {
            notice.show(&format!(
                "色: {value} (R {}, G {}, B {})",
                value.r, value.g, value.b
            ));
        }
    });
    pane.append(&color);

    // `pick` は利用者が選んだのと同じ経路なので、on_change も呼ばれる。
    let color_reset = ui.button("既定の色に戻す")?;
    disabler.add(&color_reset, Button::set_enabled);
    color_reset.on_click({
        let color = color.clone();
        move || color.pick(Color::rgb(0x33, 0x66, 0xff))
    });
    pane.append(&color_reset);

    parts::section(
        ui,
        &pane,
        "まとめて戻す",
        &["この画面の欄を、一度にすべて初期値へ戻します。"],
    )?;
    // 初期値は、組み立て終えたいまの値。
    let initial_count = count.value();
    let initial_price = price.value();
    let initial_date = date.value();
    let initial_time = time.value();
    let initial_deadline = deadline.value();
    let initial_alarm = alarm.value();
    let initial_meeting = meeting.value();
    let initial_color = color.value();
    let clear = ui.button("すべて初期値へ戻す")?;
    disabler.add(&clear, Button::set_enabled);
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
            // set_text / set_value は on_change を呼ばないので、欄のそばの
            // 表示はこちらで作り直す。
            describe_area();
            describe_password();
            describe_search();
            show_total(&count, &price, &total_status);
            notice.show("すべて初期値へ戻しました");
        }
    });
    pane.append(&clear);
    Ok(pane)
}
