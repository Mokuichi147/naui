//! miui の全ウィジェットのデモ。
//!
//! 表示されるコントロールはすべて OS (またはブラウザ) の実ウィジェット。
//! macOS なら NSButton / NSTextField、Web なら `<button>` / `<input>`。

use std::cell::Cell;
use std::rc::Rc;

use miui::{
    FileEntry, FileFilter, FilePickerMode, GridCell, Length, NavItem, Orientation, Padding, Result,
    ScrollPolicy, Settings, Sizing, Theme, Track, Ui,
};

/// 共通の UI 構築。バックエンドによらず同じコードが動く。
pub fn build(ui: &Ui) -> Result<()> {
    let window = ui.window("miui gallery", 680.0, 860.0)?;
    let root = ui.stack(Orientation::Vertical)?;
    root.set_spacing(12.0);
    root.set_padding(Padding::all(24.0));

    // Tabs を gallery のカテゴリ切り替えに使う。
    let sections = NavItem::list([
        "ホーム",
        "ウィジェット",
        "ナビゲーション",
        "レイアウト",
        "ファイル",
    ]);

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
    home_pane.append(&ui.label("タブとパンくずを使って画面を移動できます。")?);
    home_pane.append(&ui.label("カテゴリとパンくずの選択状態も連動します。")?);
    home_pane.append(&ui.link("miui のリポジトリ", "https://github.com/mokuichi147/miui")?);

    let theme_status = ui.label(&format!("テーマ: {}", theme_name(ui.theme())))?;
    let theme_selector = ui.navbar("テーマ")?;
    theme_selector.set_items(&NavItem::list(["システム", "ライト", "ダーク"]));
    theme_selector.set_selected(theme_index(ui.theme()));
    let weak_window = window.downgrade();
    theme_selector.on_select({
        let window = weak_window.clone();
        let theme_status = theme_status.clone();
        move |index| {
            let Some((name, theme)) = [
                ("システム", Theme::System),
                ("ライト", Theme::Light),
                ("ダーク", Theme::Dark),
            ]
            .get(index)
            .copied() else {
                return;
            };
            if let Some(window) = window.upgrade() {
                if window.set_theme(theme).is_ok() {
                    theme_status.set_text(&format!("テーマ: {name}"));
                }
            }
        }
    });
    home_pane.append(&theme_selector);
    home_pane.append(&theme_status);

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

    // Navbar と Dock はカテゴリ切り替えと混同しないよう、詳細画面内で
    // 個別のナビゲーションウィジェットとして表示する。
    let navbar = ui.navbar("フォルダ")?;
    navbar.set_items(&menu_items);
    navigation_pane.append(&navbar);

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

    let dock_items = NavItem::list(["前へ", "再読み込み", "次へ"]);
    let dock = ui.dock()?;
    dock.set_items(&dock_items);

    // --- レイアウト -------------------------------------------------------
    let layout_pane = ui.stack(Orientation::Vertical)?;
    layout_pane.set_spacing(12.0);
    layout_pane.set_padding(Padding::all(12.0));
    layout_pane.append(&ui.label("グリッド・スクロール・スペーサー")?);

    // ラベルの列は固定幅、入力の列は残りいっぱい。
    let form = ui.grid()?;
    form.set_spacing(12.0, 8.0);
    form.set_column_track(0, Track::Fixed(96.0));
    form.set_column_track(1, Track::FILL);
    form.set_sizing(Sizing::fill_width());
    for (row, caption) in ["名前", "メール"].iter().enumerate() {
        form.attach(&ui.label(caption)?, GridCell::new(0, row));
        let field = ui.text_input("")?;
        field.set_placeholder(caption);
        field.set_sizing(Sizing::fill_width());
        form.attach(&field, GridCell::new(1, row));
    }
    let submit = ui.button("送信")?;
    // 最小幅だけ決めて、あとはネイティブの大きさに任せる。
    submit.set_sizing(Sizing::new().min_width(160.0));
    form.attach(&submit, GridCell::new(0, 2).span(2, 1));
    layout_pane.append(&form);

    // 高さを決めた枠の中で、はみ出した分だけスクロールする。
    let long_list = ui.stack(Orientation::Vertical)?;
    long_list.set_spacing(4.0);
    long_list.set_padding(Padding::all(8.0));
    for index in 1..=30 {
        long_list.append(&ui.label(&format!("スクロールする行 {index}"))?);
    }
    let scroll = ui.scroll()?;
    scroll.set_policy(ScrollPolicy::Never, ScrollPolicy::Auto);
    scroll.set_child(&long_list);
    scroll.set_sizing(
        Sizing::new()
            .width(Length::Fill)
            .height(Length::Fixed(160.0)),
    );
    layout_pane.append(&scroll);

    // スペーサーが余りを吸うので、次に置いたものが下端へ寄る。
    layout_pane.append(&ui.spacer()?);
    layout_pane.append(&ui.label("スペーサーの後ろは下端に寄る")?);

    // --- ファイルとフォルダー ---------------------------------------------
    let files_pane = ui.stack(Orientation::Vertical)?;
    files_pane.set_spacing(12.0);
    files_pane.set_padding(Padding::all(12.0));
    files_pane.append(&ui.label("ファイルとフォルダーの選択")?);
    files_pane.append(&ui.label("押すと、その環境の標準のダイアログが開きます。")?);

    let picked = ui.label("選択: まだありません")?;

    let pick_file = ui.file_picker("画像を 1 つ選ぶ")?;
    pick_file.set_filters(&[FileFilter::new("画像", ["png", "jpg", "jpeg", "gif"])]);
    pick_file.on_select({
        let picked = picked.clone();
        move |entries| picked.set_text(&describe(entries))
    });
    files_pane.append(&pick_file);

    let pick_files = ui.file_picker("ファイルを複数選ぶ")?;
    pick_files.set_mode(FilePickerMode::Files);
    pick_files.on_select({
        let picked = picked.clone();
        move |entries| picked.set_text(&describe(entries))
    });
    files_pane.append(&pick_files);

    let pick_folder = ui.file_picker("フォルダーを選ぶ")?;
    pick_folder.set_mode(FilePickerMode::Folder);
    pick_folder.on_select({
        let picked = picked.clone();
        move |entries| picked.set_text(&describe(entries))
    });
    files_pane.append(&pick_folder);

    files_pane.append(&picked);

    // 中央の Tabs がこの gallery のカテゴリ切り替えを担う。
    let tabs = ui.tabs()?;
    tabs.add_tab("ホーム", &home_pane);
    tabs.add_tab("ウィジェット", &controls_pane);
    tabs.add_tab("ナビゲーション", &navigation_pane);
    tabs.add_tab("レイアウト", &layout_pane);
    tabs.add_tab("ファイル", &files_pane);
    // タブがウィンドウの余りを受け取り、下のものを端へ寄せる。
    tabs.set_sizing(Sizing::fill());
    root.append(&tabs);

    // 余りはタブが取るので、ドックはウィンドウの下端に並ぶ。
    dock.set_sizing(Sizing::fill_width());
    root.append(&dock);

    tabs.on_select({
        let crumbs = crumbs.clone();
        let route_status = route_status.clone();
        let sections = sections.clone();
        move |index| {
            let Some(section) = sections.get(index) else {
                return;
            };
            crumbs.set_items(&[NavItem::new("miui"), section.clone()]);
            route_status.set_text(&format!("ルート: {}", section.label));
        }
    });

    // パンくずの上位階層を選ぶと、その階層の画面へ戻る。
    crumbs.on_select({
        let tabs = tabs.clone();
        move |index| {
            if index == 0 {
                tabs.select(0);
            } else if index == 1 {
                tabs.select(tabs.selected().unwrap_or(0));
            }
        }
    });

    navbar.on_select({
        let crumbs = crumbs.clone();
        let nav_status = nav_status.clone();
        let route_status = route_status.clone();
        let menu_items = menu_items.clone();
        move |index| {
            let Some(item) = menu_items.get(index) else {
                return;
            };
            nav_status.set_text(&format!("ナビバー: {}", item.label));
            route_status.set_text(&format!("ルート: ナビゲーション / {}", item.label));
            crumbs.set_items(&[
                NavItem::new("miui"),
                NavItem::new("ナビゲーション"),
                item.clone(),
            ]);
        }
    });

    dock.on_select({
        let crumbs = crumbs.clone();
        let nav_status = nav_status.clone();
        let route_status = route_status.clone();
        let dock_items = dock_items.clone();
        move |index| {
            let Some(item) = dock_items.get(index) else {
                return;
            };
            nav_status.set_text(&format!("ドック: {}", item.label));
            route_status.set_text(&format!("ルート: ナビゲーション / {}", item.label));
            crumbs.set_items(&[
                NavItem::new("miui"),
                NavItem::new("ナビゲーション"),
                item.clone(),
            ]);
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

    tabs.set_selected(0);

    window.set_child(&root);
    window.show();
    Ok(())
}

/// ネイティブ / Web 共通の起動処理。
pub fn start() -> Result<()> {
    miui::run(Settings::new("miui gallery"), build)
}

/// 選ばれたものを 1 行で表す。
///
/// Web はパスを渡さないので、名前だけになる環境があることも示す。
fn describe(entries: &[FileEntry]) -> String {
    match entries {
        [] => "選択: ありません".to_string(),
        [entry] => match entry.path() {
            Some(path) => format!("選択: {}", path.display()),
            None => format!("選択: {} (この環境はパスを渡しません)", entry.name()),
        },
        many => format!("選択: {} 件 ({} ほか)", many.len(), many[0].name()),
    }
}

fn theme_name(theme: Theme) -> &'static str {
    match theme {
        Theme::System => "システム",
        Theme::Light => "ライト",
        Theme::Dark => "ダーク",
    }
}

fn theme_index(theme: Theme) -> usize {
    match theme {
        Theme::System => 0,
        Theme::Light => 1,
        Theme::Dark => 2,
    }
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
