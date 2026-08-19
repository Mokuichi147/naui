use naui::{DialogButtons, DialogResponse, Orientation, Padding, Result, Ui};

/// Dialog の既定ボタンと3つの応答、任意の子ウィジェット。
pub(crate) fn build(ui: &Ui) -> Result<naui::Stack> {
    let pane = ui.stack(Orientation::Vertical)?;
    pane.set_spacing(14.0);
    pane.set_padding(Padding::all(12.0));

    pane.append(&ui.label("Dialog")?);
    pane.append(
        &ui.label("見出し、本文、任意の子ウィジェット、最大3種類の応答ボタンを持てます。")?,
    );

    let status = ui.label("結果: まだ開いていません")?;

    pane.append(&ui.label("ボタン指定なし")?);
    pane.append(&ui.label("閉じるための OK ボタンが自動で追加されます。")?);
    let simple = ui.dialog("標準ダイアログ")?;
    simple.set_message("ボタンを指定していないダイアログです。");
    simple.on_response({
        let status = status.clone();
        move |_| status.set_text("結果: OK")
    });
    let open_simple = ui.button("標準ダイアログを開く")?;
    open_simple.on_click({
        let simple = simple.clone();
        move || simple.open()
    });
    pane.append(&open_simple);

    pane.append(&ui.label("Primary / Secondary / Cancel")?);
    pane.append(&ui.label("ボタンの並びは各 OS の標準に従います。")?);
    let option = ui.checkbox("子ウィジェットの例")?;
    let roles = ui.dialog("3種類の応答")?;
    roles.set_message("押したボタンは役割で通知されます。");
    roles.set_child(&option);
    roles.set_buttons(
        DialogButtons::new()
            .primary("Primary")
            .secondary("Secondary")
            .cancel("Cancel"),
    );
    roles.on_response({
        let status = status.clone();
        let option = option.clone();
        move |response| {
            let name = match response {
                DialogResponse::Primary => "Primary",
                DialogResponse::Secondary => "Secondary",
                DialogResponse::Cancel => "Cancel",
            };
            status.set_text(&format!(
                "結果: {name} / チェック: {}",
                if option.is_checked() {
                    "オン"
                } else {
                    "オフ"
                }
            ));
        }
    });
    let open_roles = ui.button("3ボタンのダイアログを開く")?;
    open_roles.on_click({
        let roles = roles.clone();
        move || roles.open()
    });
    pane.append(&open_roles);
    pane.append(&status);
    Ok(pane)
}
