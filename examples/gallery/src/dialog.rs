use naui::{
    Align, DialogButtons, DialogResponse, Orientation, PopoverPosition, Result, ToastPosition,
    ToastPriority, Ui,
};

use crate::parts::{self, Notice};

/// Dialog の既定ボタンと3つの応答、任意の子ウィジェット。Sheet・Popover・Toast の出し方
/// (Toast は待ち行列・複数の操作・優先度・位置)。
pub(crate) fn build(ui: &Ui, notice: &Notice) -> Result<naui::Stack> {
    let pane = parts::pane(ui)?;

    // 見出し、本文、任意の子ウィジェット、最大 3 種類の応答ボタンを持てる。
    parts::section(ui, &pane, "Dialog")?;
    let dialogs = parts::row(ui)?;

    // ボタンを指定しないと、閉じるための OK ボタンが自動で付く。
    let simple = ui.dialog("Simple dialog")?;
    simple.set_message("This dialog has no buttons of its own, so OK is added.");
    simple.on_response({
        let notice = notice.clone();
        move |_| notice.show("Dialog: OK")
    });
    let open_simple = ui.button("Simple…")?;
    open_simple.on_click({
        let simple = simple.clone();
        move || simple.open()
    });
    dialogs.append(&open_simple);

    // ボタンの並びは各 OS の標準に従う。押したボタンは役割で通知される。
    let option = ui.checkbox("Don't ask again")?;
    let roles = ui.dialog("Save changes?")?;
    roles.set_message("Your changes will be lost if you don't save them.");
    roles.set_child(&option);
    roles.set_buttons(
        DialogButtons::new()
            .primary("Save")
            .secondary("Don't Save")
            .cancel("Cancel"),
    );
    roles.on_response({
        let notice = notice.clone();
        let option = option.clone();
        move |response| {
            let name = match response {
                DialogResponse::Primary => "Primary",
                DialogResponse::Secondary => "Secondary",
                DialogResponse::Cancel => "Cancel",
            };
            notice.show(&format!(
                "Dialog: {name}, checkbox {}",
                if option.is_checked() { "on" } else { "off" }
            ));
        }
    });
    let open_roles = ui.button("Three buttons…")?;
    open_roles.on_click({
        let roles = roles.clone();
        move || roles.open()
    });
    dialogs.append(&open_roles);
    pane.append(&dialogs);

    // 任意の中身を持つダイアログ。ボタンは役割で渡し、置き場所は OS に従う
    // (macOS はシートの右下、Linux は見出しの帯、Windows は ContentDialog の下端)。
    parts::section(ui, &pane, "Sheet")?;
    let sheet = ui.sheet("New folder")?;
    let form = ui.stack(Orientation::Vertical)?;
    form.set_spacing(8.0);
    form.set_align(Align::Start);
    form.append(&ui.label("Name")?);
    let name = ui.text_input("")?;
    form.append(&name);
    let open_after = ui.checkbox("Open after creating")?;
    form.append(&open_after);
    sheet.set_child(&form);
    sheet.set_buttons(DialogButtons::new().primary("Create").cancel("Cancel"));
    // 名前が空の間は作成できない。Enter でも押せない。
    sheet.set_button_enabled(DialogResponse::Primary, false);
    name.on_change({
        let sheet = sheet.clone();
        move |text| sheet.set_button_enabled(DialogResponse::Primary, !text.trim().is_empty())
    });
    sheet.on_response({
        let notice = notice.clone();
        let name = name.clone();
        let open_after = open_after.clone();
        move |response| match response {
            DialogResponse::Primary => notice.show(&format!(
                "Sheet: create “{}”{}",
                name.text().trim(),
                if open_after.is_checked() {
                    " and open it"
                } else {
                    ""
                }
            )),
            _ => notice.show("Sheet: Cancel"),
        }
    });
    let open_sheet = ui.button("New folder…")?;
    open_sheet.on_click({
        let sheet = sheet.clone();
        let name = name.clone();
        move || {
            name.set_text("");
            sheet.set_button_enabled(DialogResponse::Primary, false);
            sheet.open();
        }
    });
    pane.append(&open_sheet);

    // ボタンの横に出る小さな画面。外側を押すか Esc で閉じ、出す側に場所が
    // 無ければ反対側へ回る (どれも OS の部品が自分で行う)。
    parts::section(ui, &pane, "Popover")?;
    let popover = ui.popover()?;
    let content = ui.stack(Orientation::Vertical)?;
    content.set_spacing(8.0);
    content.append(&ui.checkbox("Wrap lines")?);
    content.append(&ui.toggle("Show line numbers")?);
    let done = ui.button("Done")?;
    content.append(&done);
    popover.set_child(&content);
    // 利用者が閉じたときだけ届く。close() はアプリ自身の操作なので呼ばれない。
    popover.on_close({
        let notice = notice.clone();
        move || notice.show("Popover closed by the user (on_close)")
    });
    done.on_click({
        let popover = popover.clone();
        let notice = notice.clone();
        move || {
            popover.close();
            notice.show("Popover: Done");
        }
    });
    let open_popover = ui.button("Settings…")?;
    open_popover.on_click({
        // 自分の通知の中から自分を指すので、弱い参照で持つ。
        let button = open_popover.downgrade();
        let popover = popover.clone();
        move || {
            if let Some(button) = button.upgrade() {
                popover.open(&button);
            }
        }
    });
    let sides = [
        ("Bottom", PopoverPosition::Bottom),
        ("Top", PopoverPosition::Top),
        ("Left", PopoverPosition::Left),
        ("Right", PopoverPosition::Right),
    ];
    let (side_row, side) = parts::choice(ui, "Side", &sides.map(|(name, _)| name))?;
    side.on_select(move |index| popover.set_position(sides[index].1));
    pane.append(&open_popover);
    pane.append(&side_row);

    // 画面の端に出て自分で消える通知。同時に出るのは 1 つで、後から出したものは
    // 前のものが消えるまで並んで待つ。
    parts::section(ui, &pane, "Toast")?;
    let toasts = parts::row(ui)?;

    // ギャラリーの操作結果の知らせもトーストなので、ここで出すものと同じ列に並ぶ。

    // 何秒かで自分から消えるトースト。
    let saved = ui.toast("Saved")?;
    saved.set_timeout(3.0);
    saved.on_dismiss({
        let notice = notice.clone();
        move || notice.show("“Saved” timed out (on_dismiss)")
    });
    let show_saved = ui.button("3 seconds")?;
    show_saved.on_click({
        let saved = saved.clone();
        move || saved.show()
    });
    toasts.append(&show_saved);

    // 操作ボタン付き。押したボタンの番号が届き、そのまま消える。
    let deleted = ui.toast("Deleted")?;
    deleted.set_actions(&["Undo", "Show"]);
    deleted.on_action({
        let notice = notice.clone();
        move |index| match index {
            0 => notice.show("Undone (on_action 0)"),
            _ => notice.show("Shown (on_action 1)"),
        }
    });
    let show_deleted = ui.button("With actions")?;
    show_deleted.on_click({
        let deleted = deleted.clone();
        move || deleted.show()
    });
    toasts.append(&show_deleted);

    // 優先度 High は並ばずにすぐ出て、出ていたものを列の先頭へ戻す。
    let urgent = ui.toast("Connection lost")?;
    urgent.set_priority(ToastPriority::High);
    let show_urgent = ui.button("High priority")?;
    show_urgent.on_click({
        let urgent = urgent.clone();
        move || urgent.show()
    });
    toasts.append(&show_urgent);

    // 時間 0 は「自分では消えない」。アプリ側で消す。
    // dismiss() はアプリ自身の操作なので on_dismiss を呼ばない。
    let sticky = ui.toast("Stays until dismissed")?;
    sticky.set_timeout(0.0);
    let show_sticky = ui.button("Sticky")?;
    show_sticky.on_click({
        let sticky = sticky.clone();
        move || sticky.show()
    });
    let hide_sticky = ui.button("Dismiss sticky")?;
    hide_sticky.on_click({
        let sticky = sticky.clone();
        move || sticky.dismiss()
    });
    toasts.append(&show_sticky);
    toasts.append(&hide_sticky);
    pane.append(&toasts);

    // 出る場所。次に出たときから効く (Linux は下中央のまま)。
    let positions = [
        ("Top start", ToastPosition::TopStart),
        ("Top center", ToastPosition::TopCenter),
        ("Top end", ToastPosition::TopEnd),
        ("Bottom start", ToastPosition::BottomStart),
        ("Bottom center", ToastPosition::BottomCenter),
        ("Bottom end", ToastPosition::BottomEnd),
    ];
    let (position_row, position) = parts::choice(ui, "Position", &positions.map(|(name, _)| name))?;
    position.set_selected(4);
    position.on_select({
        let all = [
            saved.clone(),
            deleted.clone(),
            urgent.clone(),
            sticky.clone(),
        ];
        move |index| {
            for toast in &all {
                toast.set_position(positions[index].1);
            }
        }
    });
    pane.append(&position_row);
    Ok(pane)
}
