//! miui の全ウィジェットのデモ。
//!
//! 表示されるコントロールはすべて OS (またはブラウザ) の実ウィジェット。
//! macOS なら NSButton / NSTextField、Web なら `<button>` / `<input>`。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use miui::{Orientation, Padding, Result, Settings, Ui};

/// 共通の UI 構築。バックエンドによらず同じコードが動く。
pub fn build(ui: &Ui) -> Result<()> {
    let window = ui.window("miui gallery", 460.0, 560.0)?;
    let root = ui.stack(Orientation::Vertical)?;
    root.set_spacing(16.0);
    root.set_padding(Padding::all(24.0));

    root.append(&ui.label("miui ウィジェットギャラリー")?);
    root.append(&ui.label("ここに並んでいるのは、すべて OS の実ウィジェットです。")?);

    // --- ボタンとカウンタ -------------------------------------------------
    let count = Rc::new(Cell::new(0i32));
    let count_label = ui.label("押した回数: 0")?;
    let buttons = ui.stack(Orientation::Horizontal)?;
    buttons.set_spacing(8.0);

    let press = ui.button("押す")?;
    press.on_click({
        let count = count.clone();
        let label = count_label.clone();
        move || {
            count.set(count.get() + 1);
            label.set_text(&format!("押した回数: {}", count.get()));
        }
    });
    let reset = ui.button("リセット")?;
    reset.on_click({
        let count = count.clone();
        let label = count_label.clone();
        move || {
            count.set(0);
            label.set_text("押した回数: 0");
        }
    });
    let disabled = ui.button("無効")?;
    disabled.set_enabled(false);

    buttons.append(&press);
    buttons.append(&reset);
    buttons.append(&disabled);
    root.append(&count_label);
    root.append(&buttons);

    // --- チェックボックス -------------------------------------------------
    let status = ui.label("通知: オフ")?;
    let checkbox = ui.checkbox("通知を受け取る")?;
    checkbox.on_toggle({
        let status = status.clone();
        move |on| {
            status.set_text(if on {
                "通知: オン"
            } else {
                "通知: オフ"
            })
        }
    });
    root.append(&checkbox);
    root.append(&status);

    // --- テキスト入力 -----------------------------------------------------
    let greeting = ui.label("名前を入力してください")?;
    let input = ui.text_input("")?;
    input.set_placeholder("名前 (日本語も入力できます)");
    input.on_change({
        let greeting = greeting.clone();
        move |text| {
            if text.is_empty() {
                greeting.set_text("名前を入力してください");
            } else {
                greeting.set_text(&format!("こんにちは、{text} さん"));
            }
        }
    });
    root.append(&input);
    root.append(&greeting);

    // --- スライダーと進捗バー ---------------------------------------------
    let volume_label = ui.label("音量: 40%")?;
    let progress = ui.progress_bar()?;
    progress.set_value(0.4);
    let slider = ui.slider(0.0, 1.0)?;
    slider.set_value(0.4);
    slider.on_change({
        let volume_label = volume_label.clone();
        let progress = progress.clone();
        move |v| {
            volume_label.set_text(&format!("音量: {:.0}%", v * 100.0));
            progress.set_value(v);
        }
    });
    root.append(&slider);
    root.append(&progress);
    root.append(&volume_label);

    // 押されたイベントの記録 (デモ用に保持しておく)。
    let _log: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));

    window.set_child(&root);
    window.show();
    Ok(())
}

/// ネイティブ / Web 共通の起動処理。
pub fn start() -> Result<()> {
    miui::run(Settings::new("miui gallery"), build)
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn wasm_start() {
    if let Err(e) = start() {
        web_sys_error(&e.to_string());
    }
}

#[cfg(target_arch = "wasm32")]
fn web_sys_error(message: &str) {
    wasm_bindgen::JsValue::from_str(message);
    panic!("{message}");
}
