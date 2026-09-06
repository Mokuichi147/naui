use naui::{DialogButtons, DialogResponse, Result, Ui};

use crate::parts;

/// Dialog の既定ボタンと3つの応答、任意の子ウィジェット。Toast の出し方。
pub(crate) fn build(ui: &Ui) -> Result<naui::Stack> {
    let pane = parts::pane(ui)?;

    parts::section(
        ui,
        &pane,
        "Dialog",
        &["見出し、本文、任意の子ウィジェット、最大3種類の応答ボタンを持てます。"],
    )?;

    let status = parts::status(ui, "結果: まだ開いていません")?;

    parts::group(
        ui,
        &pane,
        "ボタン指定なし",
        &["閉じるための OK ボタンが自動で追加されます。"],
    )?;
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

    parts::group(
        ui,
        &pane,
        "Primary / Secondary / Cancel",
        &["ボタンの並びは各 OS の標準に従います。"],
    )?;
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

    parts::section(
        ui,
        &pane,
        "Toast",
        &["画面の下端に出て自分で消える通知です。同時に出るのは1つで、新しいものが前のものを置き換えます。"],
    )?;

    let toast_status = parts::status(ui, "Toast: まだ出していません")?;

    // 何秒かで自分から消えるトースト。
    let saved = ui.toast("保存しました")?;
    saved.set_timeout(3.0);
    saved.on_dismiss({
        let toast_status = toast_status.clone();
        move || toast_status.set_text("Toast: 時間が来て消えました")
    });
    let show_saved = ui.button("3秒で消えるトーストを出す")?;
    show_saved.on_click({
        let saved = saved.clone();
        let toast_status = toast_status.clone();
        move || {
            toast_status.set_text("Toast: 表示中");
            saved.show();
        }
    });
    pane.append(&show_saved);

    // 操作ボタン付き。押すと通知が届き、そのまま消える。
    let deleted = ui.toast("削除しました")?;
    deleted.set_action("元に戻す");
    deleted.on_action({
        let toast_status = toast_status.clone();
        move || toast_status.set_text("Toast: 「元に戻す」が押されました")
    });
    let show_deleted = ui.button("操作ボタン付きのトーストを出す")?;
    show_deleted.on_click({
        let deleted = deleted.clone();
        let toast_status = toast_status.clone();
        move || {
            toast_status.set_text("Toast: 表示中 (元に戻す)");
            deleted.show();
        }
    });
    pane.append(&show_deleted);

    // 時間 0 は「自分では消えない」。アプリ側で消す。
    let sticky = ui.toast("消すまで出したままのトーストです")?;
    sticky.set_timeout(0.0);
    let show_sticky = ui.button("消えないトーストを出す")?;
    show_sticky.on_click({
        let sticky = sticky.clone();
        let toast_status = toast_status.clone();
        move || {
            toast_status.set_text("Toast: 表示中 (消えません)");
            sticky.show();
        }
    });
    let hide_sticky = ui.button("消えないトーストを消す")?;
    hide_sticky.on_click({
        let sticky = sticky.clone();
        let toast_status = toast_status.clone();
        move || {
            sticky.dismiss();
            // dismiss() では on_dismiss を呼ばないので、ここで書き換える。
            toast_status.set_text("Toast: アプリ側から消しました");
        }
    });
    pane.append(&show_sticky);
    pane.append(&hide_sticky);
    pane.append(&toast_status);
    Ok(pane)
}
