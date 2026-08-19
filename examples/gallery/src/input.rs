use naui::{Length, Orientation, Padding, Result, Sizing, Ui};

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

    let clear = ui.button("入力をクリア")?;
    clear.on_click({
        let input = input.clone();
        let area = area.clone();
        let input_status = input_status.clone();
        let area_status = area_status.clone();
        move || {
            input.set_text("");
            area.set_text("");
            input_status.set_text("入力値: (空)");
            area_status.set_text("0 行 / 0 文字");
        }
    });
    pane.append(&clear);
    Ok(pane)
}
