//! miui の全ウィジェットのデモ。
//!
//! 表示されるコントロールはすべて OS (またはブラウザ) の実ウィジェット。
//! macOS なら NSButton / NSTextField、Web なら `<button>` / `<input>`。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use miui::{NavItem, Orientation, Padding, Result, Settings, Ui};

/// 共通の UI 構築。バックエンドによらず同じコードが動く。
pub fn build(ui: &Ui) -> Result<()> {
    let window = ui.window("miui gallery", 520.0, 900.0)?;
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

    // --- ナビゲーション ---------------------------------------------------
    // ここから下は、どのバックエンドでも同じ API で組み立てられる
    // ナビゲーション一式 (ナビバー / パンくず / タブ / メニュー /
    // ページ送り / ドック / リンク)。
    root.append(&ui.label("― ナビゲーション ―")?);

    let sections = NavItem::list(["ホーム", "ギャラリー", "設定"]);
    let nav_status = ui.label("いまいる場所: ホーム")?;

    let crumbs = ui.breadcrumbs()?;
    crumbs.set_items(&NavItem::list(["miui", "ホーム"]));

    let navbar = ui.navbar("miui")?;
    navbar.set_items(&sections);
    navbar.on_select({
        let crumbs = crumbs.clone();
        let nav_status = nav_status.clone();
        let sections = sections.clone();
        move |index| {
            let label = &sections[index].label;
            crumbs.set_items(&[NavItem::new("miui"), NavItem::new(label.clone())]);
            nav_status.set_text(&format!("いまいる場所: {label}"));
        }
    });
    navbar.set_selected(0);

    root.append(&navbar);
    root.append(&crumbs);
    root.append(&nav_status);

    // タブ。中身のウィジェットごと持つ。
    let tabs = ui.tabs()?;

    let menu_pane = ui.stack(Orientation::Vertical)?;
    menu_pane.set_spacing(8.0);
    menu_pane.set_padding(Padding::all(12.0));
    let menu_status = ui.label("メニュー: 未選択")?;
    let menu = ui.menu()?;
    menu.set_items(&NavItem::list(["受信箱", "送信済み", "アーカイブ"]));
    menu.on_select({
        let menu_status = menu_status.clone();
        move |index| menu_status.set_text(&format!("メニュー: {index} 番目"))
    });
    menu_pane.append(&menu);
    menu_pane.append(&menu_status);

    let pager_pane = ui.stack(Orientation::Vertical)?;
    pager_pane.set_spacing(8.0);
    pager_pane.set_padding(Padding::all(12.0));
    let pager_status = ui.label("1 / 5 ページ")?;
    let pager = ui.pagination(5)?;
    pager.on_change({
        let pager_status = pager_status.clone();
        move |page| pager_status.set_text(&format!("{} / 5 ページ", page + 1))
    });
    pager_pane.append(&pager);
    pager_pane.append(&pager_status);

    tabs.add_tab("メニュー", &menu_pane);
    tabs.add_tab("ページ送り", &pager_pane);
    root.append(&tabs);

    // ドックは配置がアプリの責務なので、縦スタックの末尾に置く。
    let dock = ui.dock()?;
    dock.set_items(&NavItem::list(["ホーム", "履歴", "設定"]));
    dock.on_select({
        let nav_status = nav_status.clone();
        move |index| nav_status.set_text(&format!("ドック: {index} 番目"))
    });

    let link = ui.link("miui のリポジトリ", "https://github.com/mokuichi147/miui")?;
    root.append(&link);
    root.append(&dock);

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
