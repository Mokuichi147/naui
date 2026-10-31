use naui::{DialogButtons, DialogResponse, Orientation, PopoverPosition, Result, Ui};

use crate::parts::{self, Notice};

/// Dialog の既定ボタンと3つの応答、任意の子ウィジェット。Popover と Toast の出し方。
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

    // 画面の下端に出て自分で消える通知。同時に出るのは 1 つで、新しいものが
    // 前のものを置き換える。
    parts::section(ui, &pane, "Toast")?;
    let toasts = parts::row(ui)?;

    // ギャラリーの操作結果の知らせもトーストなので、ここで出すものとは
    // 互いに置き換え合う。

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

    // 操作ボタン付き。押すと通知が届き、そのまま消える。
    let deleted = ui.toast("Deleted")?;
    deleted.set_action("Undo");
    deleted.on_action({
        let notice = notice.clone();
        move || notice.show("Undone (on_action)")
    });
    let show_deleted = ui.button("With action")?;
    show_deleted.on_click({
        let deleted = deleted.clone();
        move || deleted.show()
    });
    toasts.append(&show_deleted);

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
    Ok(pane)
}
