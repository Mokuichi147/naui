//! miui の全ウィジェットのデモ。
//!
//! 表示されるコントロールはすべて OS (またはブラウザ) の実ウィジェット。
//! macOS なら NSButton / NSTextField、Web なら `<button>` / `<input>`。

use std::cell::Cell;
use std::rc::Rc;

use miui::{NavItem, Orientation, Padding, Result, Settings, Ui};

/// 共通の UI 構築。バックエンドによらず同じコードが動く。
pub fn build(ui: &Ui) -> Result<()> {
    let window = ui.window("miui gallery", 680.0, 860.0)?;
    let root = ui.stack(Orientation::Vertical)?;
    root.set_spacing(12.0);
    root.set_padding(Padding::all(24.0));

    // Navbar / Breadcrumbs / Tabs / Dock を 1 つの画面遷移として連動させる。
    let sections = NavItem::list(["ホーム", "ウィジェット", "ナビゲーション"]);
    let active_section = Rc::new(Cell::new(0usize));

    let navbar = ui.navbar("miui")?;
    navbar.set_items(&sections);
    root.append(&navbar);

    let crumbs = ui.breadcrumbs()?;
    crumbs.set_items(&NavItem::list(["miui", "ホーム"]));
    root.append(&crumbs);

    let route_status = ui.label("ルート: ホーム")?;
    root.append(&route_status);

    // --- ホーム -----------------------------------------------------------
    let home_pane = ui.stack(Orientation::Vertical)?;
    home_pane.set_spacing(12.0);
    home_pane.set_padding(Padding::all(12.0));
    home_pane.append(&ui.label("miui ウィジェットギャラリー")?);
    home_pane
        .append(&ui.label("上のナビバー、下のドック、パンくず、タブを使って画面を移動できます。")?);
    home_pane.append(&ui.label("各画面は同じ API で組み立てられ、選択状態も連動します。")?);
    home_pane.append(&ui.link("miui のリポジトリ", "https://github.com/mokuichi147/miui")?);

    // --- 基本ウィジェット -------------------------------------------------
    let controls_pane = ui.stack(Orientation::Vertical)?;
    controls_pane.set_spacing(12.0);
    controls_pane.set_padding(Padding::all(12.0));
    controls_pane.append(&ui.label("基本ウィジェット")?);

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
    controls_pane.append(&count_label);
    controls_pane.append(&buttons);

    let notification_status = ui.label("通知: オフ")?;
    let checkbox = ui.checkbox("通知を受け取る")?;
    checkbox.on_toggle({
        let status = notification_status.clone();
        move |on| {
            status.set_text(if on {
                "通知: オン"
            } else {
                "通知: オフ"
            })
        }
    });
    controls_pane.append(&checkbox);
    controls_pane.append(&notification_status);

    let greeting = ui.label("名前を入力してください")?;
    let input = ui.text_input("")?;
    input.set_placeholder("名前 (日本語も入力できます)");
    input.on_change({
        let greeting = greeting.clone();
        move |text| {
            if text.is_empty() {
                greeting.set_text("名前を入力してください");
            } else {
                let message = format!("こんにちは、{text} さん");
                greeting.set_text(&message);
            }
        }
    });
    controls_pane.append(&input);
    controls_pane.append(&greeting);

    let volume_label = ui.label("音量: 40%")?;
    let progress = ui.progress_bar()?;
    progress.set_value(0.4);
    let slider = ui.slider(0.0, 1.0)?;
    slider.set_value(0.4);
    slider.on_change({
        let volume_label = volume_label.clone();
        let progress = progress.clone();
        move |value| {
            volume_label.set_text(&format!("音量: {:.0}%", value * 100.0));
            progress.set_value(value);
        }
    });
    controls_pane.append(&slider);
    controls_pane.append(&progress);
    controls_pane.append(&volume_label);

    // --- ナビゲーション ---------------------------------------------------
    let navigation_pane = ui.stack(Orientation::Vertical)?;
    navigation_pane.set_spacing(12.0);
    navigation_pane.set_padding(Padding::all(12.0));
    navigation_pane.append(&ui.label("ナビゲーションの詳細")?);
    navigation_pane
        .append(&ui.label("メニューとページ送りを操作すると、パンくずも現在位置を表示します。")?);

    let menu_items = NavItem::list(["受信箱", "送信済み", "アーカイブ"]);
    let menu = ui.menu()?;
    menu.set_items(&menu_items);
    menu.set_selected(0);
    navigation_pane.append(&menu);

    let nav_status = ui.label("場所: 受信箱 / 1 ページ")?;
    navigation_pane.append(&nav_status);

    let pager = ui.pagination(5)?;
    let pager_status = ui.label("ページ: 1 / 5")?;
    navigation_pane.append(&pager);
    navigation_pane.append(&pager_status);

    // 中央の Tabs がこの gallery の主画面。Navbar と Dock からも切り替える。
    let tabs = ui.tabs()?;
    tabs.add_tab("ホーム", &home_pane);
    tabs.add_tab("ウィジェット", &controls_pane);
    tabs.add_tab("ナビゲーション", &navigation_pane);
    root.append(&tabs);

    // ドックはアプリの主画面と同じ項目を持つため、別の入口として使える。
    let dock = ui.dock()?;
    dock.set_items(&sections);
    root.append(&dock);

    // Navbar / Tabs / Dock のいずれから選んでも同じルートへ同期する。
    navbar.on_select({
        let tabs = tabs.clone();
        let crumbs = crumbs.clone();
        let route_status = route_status.clone();
        let active_section = active_section.clone();
        let sections = sections.clone();
        move |index| {
            let Some(section) = sections.get(index) else {
                return;
            };
            active_section.set(index);
            tabs.set_selected(index);
            crumbs.set_items(&[NavItem::new("miui"), section.clone()]);
            route_status.set_text(&format!("ルート: {}", section.label));
        }
    });

    tabs.on_select({
        let navbar = navbar.clone();
        let crumbs = crumbs.clone();
        let route_status = route_status.clone();
        let active_section = active_section.clone();
        let sections = sections.clone();
        move |index| {
            let Some(section) = sections.get(index) else {
                return;
            };
            active_section.set(index);
            navbar.set_selected(index);
            crumbs.set_items(&[NavItem::new("miui"), section.clone()]);
            route_status.set_text(&format!("ルート: {}", section.label));
        }
    });

    dock.on_select({
        let navbar = navbar.clone();
        let tabs = tabs.clone();
        let crumbs = crumbs.clone();
        let route_status = route_status.clone();
        let active_section = active_section.clone();
        let sections = sections.clone();
        move |index| {
            let Some(section) = sections.get(index) else {
                return;
            };
            active_section.set(index);
            navbar.set_selected(index);
            tabs.set_selected(index);
            crumbs.set_items(&[NavItem::new("miui"), section.clone()]);
            route_status.set_text(&format!("ルート: {}", section.label));
        }
    });

    // パンくずの上位階層を選ぶと、その階層の画面へ戻る。
    crumbs.on_select({
        let navbar = navbar.clone();
        let active_section = active_section.clone();
        move |index| {
            if index <= 1 {
                navbar.select(if index == 0 { 0 } else { active_section.get() });
            }
        }
    });

    menu.on_select({
        let crumbs = crumbs.clone();
        let nav_status = nav_status.clone();
        let route_status = route_status.clone();
        let pager = pager.clone();
        let menu_items = menu_items.clone();
        move |index| {
            let Some(item) = menu_items.get(index) else {
                return;
            };
            let page = pager.page() + 1;
            nav_status.set_text(&format!("場所: {} / {} ページ", item.label, page));
            route_status.set_text(&format!("ルート: ナビゲーション / {}", item.label));
            crumbs.set_items(&[
                NavItem::new("miui"),
                NavItem::new("ナビゲーション"),
                item.clone(),
            ]);
        }
    });

    pager.on_change({
        let crumbs = crumbs.clone();
        let nav_status = nav_status.clone();
        let pager_status = pager_status.clone();
        let menu = menu.clone();
        let menu_items = menu_items.clone();
        move |page| {
            let item = menu
                .selected()
                .and_then(|index| menu_items.get(index))
                .map(|item| item.label.as_str())
                .unwrap_or("受信箱");
            pager_status.set_text(&format!("ページ: {} / 5", page + 1));
            nav_status.set_text(&format!("場所: {} / {} ページ", item, page + 1));
            crumbs.set_items(&[
                NavItem::new("miui"),
                NavItem::new("ナビゲーション"),
                NavItem::new(format!("{} / {}ページ", item, page + 1)),
            ]);
        }
    });

    navbar.set_selected(0);
    tabs.set_selected(0);
    dock.set_selected(0);

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
