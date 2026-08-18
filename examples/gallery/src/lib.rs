//! naui の全ウィジェットのデモ。
//!
//! 表示されるコントロールはすべて OS (またはブラウザ) の実ウィジェット。
//! macOS なら NSButton / NSTextField、Web なら `<button>` / `<input>`。

use std::cell::Cell;
use std::rc::Rc;

use naui::{
    Align, FileEntry, FileFilter, FilePickerMode, Fit, GridCell, Length, ListItem, NavItem,
    Orientation, Padding, PlaybackState, PopupItem, Result, ScrollPolicy, SelectionMode, Settings,
    Sizing, Theme, Track, Ui,
};

/// 同梱のサンプル画像の場所。
///
/// ネイティブはビルド時に決まる絶対パス、Web は配信ディレクトリからの
/// 相対 URL になる (`web/build.sh` が `assets/` をコピーする)。
#[cfg(not(target_arch = "wasm32"))]
const SAMPLE_IMAGE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/assets/sample.png");
#[cfg(target_arch = "wasm32")]
const SAMPLE_IMAGE: &str = "assets/sample.png";

/// 共通の UI 構築。バックエンドによらず同じコードが動く。
pub fn build(ui: &Ui) -> Result<()> {
    let window = ui.window("naui gallery", 680.0, 860.0)?;
    let root = ui.stack(Orientation::Vertical)?;
    root.set_spacing(12.0);
    root.set_padding(Padding::all(24.0));

    // Tabs を gallery のカテゴリ切り替えに使う。
    let sections = NavItem::list([
        "ホーム",
        "ウィジェット",
        "ナビゲーション",
        "リスト",
        "レイアウト",
        "ファイル",
        "メディア",
    ]);

    let crumbs = ui.breadcrumbs()?;
    crumbs.set_items(&NavItem::list(["naui", "ホーム"]));
    root.append(&crumbs);

    let route_status = ui.label("ルート: ホーム")?;
    root.append(&route_status);

    // --- ホーム -----------------------------------------------------------
    let home_pane = ui.stack(Orientation::Vertical)?;
    home_pane.set_spacing(12.0);
    home_pane.set_padding(Padding::all(12.0));
    home_pane.append(&ui.label("naui ウィジェットギャラリー")?);
    home_pane.append(&ui.label("タブとパンくずを使って画面を移動できます。")?);
    home_pane.append(&ui.label("カテゴリとパンくずの選択状態も連動します。")?);
    home_pane.append(&ui.link("naui のリポジトリ", "https://github.com/mokuichi147/naui")?);

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

    // 回数のラベルを右クリックすると、ボタンと同じ操作をメニューから行える。
    // ポップアップメニューは画面に並ばないので、取り付け先のウィジェットが
    // そのまま「右クリックできる場所」になる。
    let count_menu_items = vec![
        PopupItem::new("10 増やす"),
        PopupItem::new("0 に戻す"),
        PopupItem::separator(),
        PopupItem::new("減らす (選べません)").enabled(false),
    ];
    let count_menu = ui.popup_menu()?;
    count_menu.set_items(&count_menu_items);
    count_menu.on_select({
        let count = count.clone();
        let label = count_label.clone();
        move |index| {
            match index {
                0 => count.set(count.get() + 10),
                1 => count.set(0),
                // 区切り線と選べない項目は、そもそも通知されない。
                _ => return,
            }
            label.set_text(&format!("押した回数: {}", count.get()));
        }
    });
    count_menu.attach(&count_label);
    controls_pane.append(&ui.label("↑ 回数の行を右クリックすると、メニューからも操作できます")?);

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

    // --- リスト -----------------------------------------------------------
    let list_pane = ui.stack(Orientation::Vertical)?;
    list_pane.set_spacing(12.0);
    list_pane.set_padding(Padding::all(12.0));
    list_pane.append(&ui.label("選択できる行の一覧")?);

    // detail を付けた行は 2 行になる。
    let mut cities = vec![
        ListItem::new("札幌").detail("北海道"),
        ListItem::new("仙台").detail("宮城県"),
        ListItem::new("東京").detail("東京都"),
        ListItem::new("横浜").detail("神奈川県"),
        ListItem::new("名古屋").detail("愛知県"),
    ];
    // detail の無い行は 1 行のまま。混ぜても行ごとに高さが変わる。
    cities.extend(ListItem::list(["京都", "大阪", "神戸", "広島", "福岡"]));
    // 選べない行。クリックもキーボードもネイティブ側が弾く。
    cities.push(ListItem::new("那覇").detail("準備中").enabled(false));
    // detail を外した版。Web ではこちらが `<select>` になる。
    let plain: Vec<ListItem> = cities
        .iter()
        .map(|item| ListItem::new(&item.label).enabled(item.enabled))
        .collect();

    let list = ui.list()?;
    list.set_items(&cities);
    // スクロールと同じく、高さは自分では決まらないので指定する。
    list.set_sizing(
        Sizing::new()
            .width(Length::Fill)
            .height(Length::Fixed(180.0)),
    );

    let list_status = ui.label("選択: なし")?;
    list.on_select({
        let list_status = list_status.clone();
        let cities = cities.clone();
        move |indices| {
            let picked: Vec<&str> = indices
                .iter()
                .filter_map(|&i| cities.get(i).map(|item| item.label.as_str()))
                .collect();
            let text = if picked.is_empty() {
                String::from("なし")
            } else {
                picked.join(" / ")
            };
            list_status.set_text(&format!("選択: {text}"));
        }
    });

    // 一覧を右クリックすると、いま選ばれている行を扱うメニューが出る。
    // 「選択を持つウィジェット + その選択に対する操作」という、
    // コンテキストメニューの一番よくある組み合わせ。
    let list_menu_items = vec![
        PopupItem::new("選んだ行を書き出す"),
        PopupItem::new("先頭を選ぶ"),
        PopupItem::new("選択を外す"),
        PopupItem::separator(),
        PopupItem::new("削除 (選べません)").enabled(false),
    ];
    let list_menu = ui.popup_menu()?;
    list_menu.set_items(&list_menu_items);
    list_menu.on_select({
        let list = list.clone();
        let list_status = list_status.clone();
        let cities = cities.clone();
        move |index| match index {
            0 => {
                let picked: Vec<&str> = list
                    .selection()
                    .iter()
                    .filter_map(|&i| cities.get(i).map(|item| item.label.as_str()))
                    .collect();
                let text = if picked.is_empty() {
                    String::from("なし")
                } else {
                    picked.join(" / ")
                };
                list_status.set_text(&format!("書き出し: {text}"));
            }
            // 通知ありの経路なので、状態表示は on_select 側が更新する。
            1 => list.select(0),
            2 => {
                list.clear_selection();
                list_status.set_text("選択: なし");
            }
            _ => {}
        }
    });
    list_menu.attach(&list);

    // 単一選択と複数選択を切り替える。切り替えると選択は外れる。
    let mode_selector = ui.navbar("選び方")?;
    mode_selector.set_items(&NavItem::list(["1 行だけ", "複数行"]));
    mode_selector.set_selected(0);
    mode_selector.on_select({
        let list = list.clone();
        let list_status = list_status.clone();
        move |index| {
            let mode = if index == 0 {
                SelectionMode::Single
            } else {
                SelectionMode::Multiple
            };
            list.set_selection_mode(mode);
            list_status.set_text("選択: なし");
        }
    });

    let list_buttons = ui.stack(Orientation::Horizontal)?;
    list_buttons.set_spacing(8.0);
    let select_tokyo = ui.button("東京を選ぶ")?;
    select_tokyo.on_click({
        let list = list.clone();
        // 通知ありの経路なので、状態表示も一緒に更新される。
        move || list.select(2)
    });
    let detail_toggle = ui.button("detail を外す")?;
    detail_toggle.on_click({
        let list = list.clone();
        let detail_toggle = detail_toggle.clone();
        let list_status = list_status.clone();
        let cities = cities.clone();
        let plain = plain.clone();
        let showing_detail = Rc::new(Cell::new(true));
        move || {
            let next = !showing_detail.get();
            showing_detail.set(next);
            list.set_items(if next { &cities } else { &plain });
            detail_toggle.set_text(if next {
                "detail を外す"
            } else {
                "detail を付ける"
            });
            // 行を作り直すと選択は外れる (通知は来ない)。
            list_status.set_text("選択: なし");
        }
    });

    let clear_selection = ui.button("選択を消す")?;
    clear_selection.on_click({
        let list = list.clone();
        let list_status = list_status.clone();
        move || {
            // clear_selection は通知しないので、表示は自分で合わせる。
            list.clear_selection();
            list_status.set_text("選択: なし");
        }
    });
    list_buttons.append(&select_tokyo);
    list_buttons.append(&clear_selection);
    list_buttons.append(&detail_toggle);

    list_pane.append(&mode_selector);
    list_pane.append(&list);
    list_pane.append(&ui.label("一覧を右クリックすると、選んだ行に対するメニューが出ます")?);
    list_pane.append(&list_status);
    list_pane.append(&list_buttons);

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

    // --- メディア ---------------------------------------------------------
    let media_pane = build_media_pane(ui)?;

    // 中央の Tabs がこの gallery のカテゴリ切り替えを担う。
    let tabs = ui.tabs()?;
    tabs.add_tab("ホーム", &home_pane);
    tabs.add_tab("ウィジェット", &controls_pane);
    tabs.add_tab("ナビゲーション", &navigation_pane);
    tabs.add_tab("リスト", &list_pane);
    tabs.add_tab("レイアウト", &layout_pane);
    tabs.add_tab("ファイル", &files_pane);
    tabs.add_tab("メディア", &media_pane);
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
            crumbs.set_items(&[NavItem::new("naui"), section.clone()]);
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
                NavItem::new("naui"),
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
                NavItem::new("naui"),
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
                NavItem::new("naui"),
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
                NavItem::new("naui"),
                NavItem::new("ナビゲーション"),
                NavItem::new(format!("{} / {}ページ", item, page + 1)),
            ]);
        }
    });

    window.set_child(&root);
    window.show();
    Ok(())
}

/// 表示形式と、そこへ振り分ける拡張子。
///
/// **ファイル選択の絞り込みと、選ばれたあとの振り分けを、どちらもこの表から
/// 作る。** 片方だけ直して食い違うことが無いようにするため。選択は
/// この表にある拡張子だけを受け付けるので、選ばれたものは必ずどれかに入る。
const MEDIA_FORMS: [(&str, &[&str]); 3] = [
    (
        "画像",
        &["png", "jpg", "jpeg", "gif", "bmp", "webp", "heic", "tiff"],
    ),
    ("動画", &["mp4", "m4v", "mov", "webm", "mkv", "avi"]),
    ("音声", &["m4a", "mp3", "aac", "wav", "flac", "ogg"]),
];

/// メディア表示の基準高さ。横幅はウィンドウに合わせて伸縮させる。
const MEDIA_DISPLAY_HEIGHT: f64 = 315.0;

/// 名前や場所の拡張子から、どの表示形式かを決める。
///
/// ファイル選択から来たものは [`MEDIA_FORMS`] で絞ってあるので必ず決まる。
/// 直接入力された場所だけは、知らない拡張子や拡張子なしがあり得るので `None`。
fn media_form_of(source: &str) -> Option<usize> {
    // クエリとフラグメントを落としてから、末尾の拡張子を見る。
    let extension = source
        .split(['?', '#'])
        .next()?
        .rsplit(['/', '\\'])
        .next()?
        .rsplit_once('.')
        .filter(|(stem, ext)| !stem.is_empty() && !ext.is_empty())?
        .1
        .to_ascii_lowercase();
    MEDIA_FORMS
        .iter()
        .position(|(_, extensions)| extensions.contains(&extension.as_str()))
}

/// 画像・動画・音声のデモ。
///
/// **ファイル選択は 1 つだけ。** 受け付ける拡張子を [`MEDIA_FORMS`] に絞って
/// あるので、選ばれたものは必ず画像 / 動画 / 音声のどれかに決まり、
/// 対応する表示形式へ自動で切り替わる。
fn build_media_pane(ui: &Ui) -> Result<naui::Stack> {
    let pane = ui.stack(Orientation::Vertical)?;
    pane.set_spacing(12.0);
    pane.set_padding(Padding::all(12.0));
    pane.set_align(Align::Start);

    let description = ui.label("選んだファイルの種類に合わせて表示形式が切り替わります。")?;
    let description_row = ui.stack(Orientation::Horizontal)?;
    description_row.append(&description);
    pane.append(&description_row);

    let (image_pane, image) = build_image_pane(ui)?;
    let (video_pane, video) = build_video_pane(ui)?;
    let (audio_pane, audio) = build_audio_pane(ui)?;

    // MediaPlayerElement をタブのコンテンツにすると、動画・音声ペインを
    // 初めて表示する瞬間に環境によっては問題が起きる。また、この Gallery
    // ではファイルの種類に応じて自動で切り替えるため、種類のタブは見せず、
    // 選択中のペインだけを Grid の子として置く。
    let forms = {
        let forms = ui.grid()?;
        forms.set_column_track(0, Track::FILL);
        forms.set_row_track(0, Track::FILL);
        forms.attach(&image_pane, GridCell::new(0, 0));
        forms
    };

    let status = ui.label("種類: 画像 (同梱のサンプル)")?;
    // 横 Stack を 1 層はさんで、ラベルの幅を内容ぶんに保つ。裸のラベルは
    // 親の幅いっぱいに引き伸ばされることがあり、表示形式側の Fill と
    // 混ざって幅が揺れる。
    let status_row = ui.stack(Orientation::Horizontal)?;
    status_row.append(&status);

    // 場所を対応するウィジェットへ渡し、その表示形式へ移る。
    let show = {
        let image = image.clone();
        let video = video.clone();
        let audio = audio.clone();
        let forms = forms.clone();
        let image_pane = image_pane.clone();
        let video_pane = video_pane.clone();
        let audio_pane = audio_pane.clone();
        let status = status.clone();
        move |form: usize, source: &str| {
            match form {
                0 => forms.replace(&image_pane, GridCell::new(0, 0)),
                1 => forms.replace(&video_pane, GridCell::new(0, 0)),
                _ => forms.replace(&audio_pane, GridCell::new(0, 0)),
            }
            match form {
                0 => image.set_source(source),
                1 => video.set_source(source),
                _ => audio.set_source(source),
            }
            status.set_text(&format!("種類: {}", MEDIA_FORMS[form].0));
        }
    };
    let show = Rc::new(show);

    // --- ファイル選択 ------------------------------------------------------
    //
    // 絞り込みは表示形式の表そのものから作る。ここに無い拡張子は選べないので、
    // 「種類が分からないファイルを選ばれた」という状態にならない。
    let pick = ui.file_picker("メディアを選ぶ")?;
    let extensions: Vec<&str> = MEDIA_FORMS
        .iter()
        .flat_map(|(_, extensions)| extensions.iter().copied())
        .collect();
    pick.set_filters(&[FileFilter::new("メディア", extensions)]);

    let field = ui.text_input("")?;
    field.set_placeholder("パス または https://…");
    field.set_sizing(Sizing::fill_width());

    pick.on_select({
        let show = show.clone();
        let field = field.clone();
        let status = status.clone();
        move |entries| {
            let Some(entry) = entries.first() else {
                return;
            };
            // ネイティブは絶対パス、Web はブラウザが作る blob URL。
            let Some(source) = entry.source() else {
                status.set_text(&format!("{} (場所を取得できません)", entry.name()));
                return;
            };
            field.set_text(source);
            // 種類は名前から決める。Web の blob URL には拡張子が無いため。
            match media_form_of(entry.name()) {
                Some(form) => show(form, source),
                // 絞り込みを通っているので、ここへは来ないはず。
                None => status.set_text(&format!("種類: 判定できません ({})", entry.name())),
            }
        }
    });

    // --- 場所の直接入力 ----------------------------------------------------
    //
    // ファイル選択では扱えない URL を試すための欄。こちらは絞り込みが
    // 効かないので、知らない拡張子はそのまま知らないと出す。
    let load = ui.button("読み込む")?;
    load.on_click({
        let show = show.clone();
        let field = field.clone();
        let status = status.clone();
        move || {
            let source = field.text();
            if source.is_empty() {
                return;
            }
            match media_form_of(&source) {
                Some(form) => show(form, &source),
                None => status.set_text("種類: 判定できません (拡張子で判断しています)"),
            }
        }
    });

    let row = ui.stack(Orientation::Horizontal)?;
    row.set_spacing(8.0);
    row.append(&field);
    row.append(&load);
    row.append(&pick);
    row.set_sizing(Sizing::fill_width());
    pane.append(&row);
    pane.append(&status_row);

    // 縦 Stack の最後の Fill だけが、ウィンドウの高さの余りを受け取る。
    forms.set_sizing(Sizing::fill());
    pane.append(&forms);

    Ok(pane)
}

/// 画像の表示形式。収め方を切り替えられる。
fn build_image_pane(ui: &Ui) -> Result<(naui::Grid, naui::Image)> {
    let pane = ui.grid()?;
    pane.set_spacing(0.0, 8.0);
    pane.set_padding(Padding::all(8.0));
    pane.set_sizing(Sizing::fill());
    pane.set_column_track(0, Track::FILL);
    pane.set_row_track(0, Track::FILL);

    // 何も選ばれていない間は同梱のサンプルを出しておく。
    let image = ui.image(SAMPLE_IMAGE)?;
    image.set_alt("斜めのグラデーションと市松模様のサンプル画像");
    // 元の高さ 315px を基準にし、横幅はウィンドウの変更へ追従させる。
    image.set_sizing(Sizing::fill().max_height(MEDIA_DISPLAY_HEIGHT));
    pane.attach(&image, GridCell::new(0, 0));

    let fits = [
        ("contain", Fit::Contain),
        ("cover", Fit::Cover),
        ("fill", Fit::Fill),
        ("none", Fit::None),
    ];
    let selector = ui.navbar("収め方")?;
    selector.set_items(&NavItem::list(fits.map(|(name, _)| name)));
    selector.set_selected(0);
    selector.on_select({
        let image = image.clone();
        move |index| {
            if let Some((_, fit)) = fits.get(index) {
                image.set_fit(*fit);
            }
        }
    });
    selector.set_sizing(Sizing::fill_width());
    pane.attach(&selector, GridCell::new(0, 1));
    Ok((pane, image))
}

/// 動画の表示形式。再生の操作を一通り並べる。
fn build_video_pane(ui: &Ui) -> Result<(naui::Stack, naui::Video)> {
    let pane = ui.stack(Orientation::Vertical)?;
    pane.set_spacing(8.0);
    pane.set_padding(Padding::all(8.0));
    pane.set_align(Align::Start);
    pane.set_sizing(Sizing::fill());

    let media_frame = ui.grid()?;
    media_frame.set_column_track(0, Track::FILL);
    media_frame.set_row_track(0, Track::FILL);
    // 上限付きの Fill は「空間があれば 315px、狭いときはそれ以下」を表す。
    media_frame.set_sizing(Sizing::fill().max_height(MEDIA_DISPLAY_HEIGHT));

    let video = ui.video("")?;
    // 動画の intrinsic size には任せず、画像と同じ表示欄の大きさに合わせる。
    video.set_sizing(Sizing::fill().max_height(MEDIA_DISPLAY_HEIGHT));
    // 動画フレームと操作欄は別ウィジェットに分け、操作欄がフレームへ
    // 重ならないようにする。
    media_frame.attach(&video, GridCell::new(0, 0));
    pane.append(&media_frame);

    let controls = ui.stack(Orientation::Vertical)?;
    controls.set_spacing(8.0);
    controls.set_sizing(Sizing::fill_width());

    let status = ui.label("状態: 未再生")?;
    video.on_state_change({
        let status = status.clone();
        move |state| status.set_text(&format!("状態: {}", state_name(state)))
    });

    let buttons = ui.stack(Orientation::Horizontal)?;
    buttons.set_spacing(8.0);
    let play = ui.button("再生")?;
    play.on_click({
        let video = video.clone();
        move || video.play()
    });
    let pause = ui.button("一時停止")?;
    pause.on_click({
        let video = video.clone();
        move || video.pause()
    });
    buttons.append(&play);
    buttons.append(&pause);
    controls.append(&buttons);
    controls.append(&status);

    let position = ui.label("位置: -")?;
    controls.append(&position);

    // 長さが決まるまでシークできないので、割合で指定する。
    let seek = ui.slider(0.0, 1.0)?;
    // つまみを「再生に合わせて動かす」ときと「ユーザーが動かした」ときを
    // 区別する目印。こちらから値を書くとスライダーの変更通知が出る環境
    // (WinUI) があるため、これが無いと再生位置を書き戻すたびにシークが走る。
    let syncing = Rc::new(Cell::new(false));
    seek.on_change({
        let video = video.clone();
        let syncing = syncing.clone();
        move |ratio| {
            if syncing.get() {
                return;
            }
            if let Some(duration) = video.duration() {
                video.seek(duration * ratio);
            }
        }
    });
    // 再生に合わせてつまみと位置表示を進める。
    video.on_position_change({
        let video = video.clone();
        let seek = seek.clone();
        let position = position.clone();
        let syncing = syncing.clone();
        move |seconds| {
            let Some(duration) = video.duration().filter(|d| *d > 0.0) else {
                return;
            };
            syncing.set(true);
            seek.set_value((seconds / duration).clamp(0.0, 1.0));
            syncing.set(false);
            position.set_text(&format!("位置: {seconds:.1} / {duration:.1} 秒"));
        }
    });
    controls.append(&seek);

    let volume_label = ui.label("音量: 100%")?;
    let volume = ui.slider(0.0, 1.0)?;
    volume.set_value(1.0);
    volume.on_change({
        let video = video.clone();
        let volume_label = volume_label.clone();
        move |value| {
            video.set_volume(value);
            volume_label.set_text(&format!("音量: {:.0}%", value * 100.0));
        }
    });
    controls.append(&volume);
    controls.append(&volume_label);

    let toggles = ui.stack(Orientation::Horizontal)?;
    toggles.set_spacing(12.0);
    let muted = ui.checkbox("消音")?;
    muted.on_toggle({
        let video = video.clone();
        move |on| video.set_muted(on)
    });
    let looping = ui.checkbox("繰り返し")?;
    looping.on_toggle({
        let video = video.clone();
        move |on| video.set_loop(on)
    });
    toggles.append(&muted);
    toggles.append(&looping);
    controls.append(&toggles);
    pane.append(&controls);
    Ok((pane, video))
}

/// 音声の表示形式。WinUI 標準バーを使わず、Gallery 側の操作欄を使う。
fn build_audio_pane(ui: &Ui) -> Result<(naui::Stack, naui::Audio)> {
    let pane = ui.stack(Orientation::Vertical)?;
    pane.set_spacing(8.0);
    pane.set_padding(Padding::all(8.0));
    pane.append(&ui.label("再生ボタンから操作できます。")?);

    let audio = ui.audio("")?;
    audio.set_sizing(Sizing::fill_width());
    pane.append(&audio);

    let buttons = ui.stack(Orientation::Horizontal)?;
    buttons.set_spacing(8.0);
    let play = ui.button("再生")?;
    play.on_click({
        let audio = audio.clone();
        move || audio.play()
    });
    let pause = ui.button("一時停止")?;
    pause.on_click({
        let audio = audio.clone();
        move || audio.pause()
    });
    buttons.append(&play);
    buttons.append(&pause);
    pane.append(&buttons);

    let status = ui.label("状態: 未再生")?;
    audio.on_state_change({
        let status = status.clone();
        move |state| status.set_text(&format!("状態: {}", state_name(state)))
    });
    pane.append(&status);
    Ok((pane, audio))
}

fn state_name(state: PlaybackState) -> &'static str {
    match state {
        PlaybackState::Idle => "未再生",
        PlaybackState::Buffering => "読み込み中",
        PlaybackState::Playing => "再生中",
        PlaybackState::Paused => "一時停止",
        PlaybackState::Ended => "再生終了",
    }
}

// ネイティブの `start()` と、Web のブラウザから呼ばれる入口を作る。
// 環境ごとの入口の違いは naui 側にあるので、ここは 1 行で済む。
naui::entry!(Settings::new("naui gallery"), build);

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
