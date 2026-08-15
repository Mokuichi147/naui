//! miui の全ウィジェットのデモ。
//!
//! 表示されるコントロールはすべて OS (またはブラウザ) の実ウィジェット。
//! macOS なら NSButton / NSTextField、Web なら `<button>` / `<input>`。

use std::cell::Cell;
use std::rc::Rc;

use miui::{
    FileEntry, FileFilter, FilePickerMode, Fit, GridCell, Length, NavItem, Orientation, Padding,
    PlaybackState, Result, ScrollPolicy, Settings, Sizing, Theme, Track, Ui,
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
        "メディア",
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

    // --- メディア ---------------------------------------------------------
    let media_pane = build_media_pane(ui)?;

    // 中央の Tabs がこの gallery のカテゴリ切り替えを担う。
    let tabs = ui.tabs()?;
    tabs.add_tab("ホーム", &home_pane);
    tabs.add_tab("ウィジェット", &controls_pane);
    tabs.add_tab("ナビゲーション", &navigation_pane);
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

/// 画像・動画・音声のデモ。
///
/// 画像は同梱のサンプルを表示し、収め方を切り替えられる。動画と音声は
/// ファイル選択か、パス / URL の直接入力で読み込ませる
/// (メディアそのものは同梱していない)。
fn build_media_pane(ui: &Ui) -> Result<miui::Stack> {
    let pane = ui.stack(Orientation::Vertical)?;
    pane.set_spacing(12.0);
    pane.set_padding(Padding::all(12.0));

    // --- 画像 -------------------------------------------------------------
    pane.append(&ui.label("画像 (同梱のサンプル)")?);

    let image = ui.image(SAMPLE_IMAGE)?;
    image.set_alt("斜めのグラデーションと市松模様のサンプル画像");
    // 枠を画像より横長にしておくと、収め方の違いが見て分かる。
    image.set_sizing(Sizing::fixed(240.0, 140.0));
    pane.append(&image);

    let fits = [
        ("contain", Fit::Contain),
        ("cover", Fit::Cover),
        ("fill", Fit::Fill),
        ("none", Fit::None),
    ];
    let fit_selector = ui.navbar("収め方")?;
    fit_selector.set_items(&NavItem::list(fits.map(|(name, _)| name)));
    fit_selector.set_selected(0);
    fit_selector.on_select({
        let image = image.clone();
        move |index| {
            if let Some((_, fit)) = fits.get(index) {
                image.set_fit(*fit);
            }
        }
    });
    pane.append(&fit_selector);

    // --- 動画 -------------------------------------------------------------
    pane.append(&ui.label("動画 (パスか URL を入れて「読み込む」)")?);

    let video = ui.video("")?;
    video.set_sizing(Sizing::fixed(320.0, 180.0));

    let video_field = ui.text_input("")?;
    video_field.set_placeholder("/path/to/clip.mp4 または https://…");
    video_field.set_sizing(Sizing::fill_width());
    let video_load = ui.button("読み込む")?;
    video_load.on_click({
        let video = video.clone();
        let field = video_field.clone();
        move || video.set_source(&field.text())
    });
    // ファイル選択からも指定できるようにする。押すとその環境の標準の
    // ダイアログが開く。
    let video_pick = ui.file_picker("ファイルを選ぶ")?;
    video_pick.set_filters(&[FileFilter::new(
        "動画",
        ["mp4", "mov", "m4v", "webm", "mkv"],
    )]);
    video_pick.on_select({
        let video = video.clone();
        let field = video_field.clone();
        move |entries| apply_picked(entries, &field, |source| video.set_source(source))
    });

    let video_row = ui.stack(Orientation::Horizontal)?;
    video_row.set_spacing(8.0);
    video_row.append(&video_field);
    video_row.append(&video_load);
    video_row.append(&video_pick);
    video_row.set_sizing(Sizing::fill_width());
    pane.append(&video_row);
    pane.append(&video);

    // 再生状態は、ネイティブの再生バーを押したときにも届く。
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
    // 再生位置は時々刻々と変わるが、miui にタイマーは無いので押して読み直す。
    let position = ui.label("位置: -")?;
    let refresh = ui.button("位置を読む")?;
    refresh.on_click({
        let video = video.clone();
        let position = position.clone();
        move || {
            let text = match video.duration() {
                Some(duration) => {
                    format!("位置: {:.1} / {:.1} 秒", video.position(), duration)
                }
                None => "位置: 読み込み中 (長さが未確定)".to_string(),
            };
            position.set_text(&text);
        }
    });
    buttons.append(&play);
    buttons.append(&pause);
    buttons.append(&refresh);
    pane.append(&buttons);
    pane.append(&status);
    pane.append(&position);

    // 長さが決まるまでシークできないので、割合で指定する。
    let seek_label = ui.label("シーク: 0%")?;
    let seek = ui.slider(0.0, 1.0)?;

    // つまみを「再生に合わせて動かす」ときと「ユーザーが動かした」ときを
    // 区別する目印。こちらから値を書くとスライダーの変更通知が出る環境
    // (WinUI) があるため、これが無いと再生位置を書き戻すたびにシークが
    // 走ってしまう。
    let syncing = Rc::new(Cell::new(false));

    seek.on_change({
        let video = video.clone();
        let seek_label = seek_label.clone();
        let syncing = syncing.clone();
        move |ratio| {
            if syncing.get() {
                return;
            }
            seek_label.set_text(&format!("シーク: {:.0}%", ratio * 100.0));
            if let Some(duration) = video.duration() {
                video.seek(duration * ratio);
            }
        }
    });

    // 再生に合わせてつまみと位置表示を進める。
    video.on_position_change({
        let video = video.clone();
        let seek = seek.clone();
        let seek_label = seek_label.clone();
        let position = position.clone();
        let syncing = syncing.clone();
        move |seconds| {
            let Some(duration) = video.duration() else {
                return;
            };
            if duration <= 0.0 {
                return;
            }
            let ratio = (seconds / duration).clamp(0.0, 1.0);
            syncing.set(true);
            seek.set_value(ratio);
            syncing.set(false);
            seek_label.set_text(&format!("シーク: {:.0}%", ratio * 100.0));
            position.set_text(&format!("位置: {seconds:.1} / {duration:.1} 秒"));
        }
    });

    pane.append(&seek);
    pane.append(&seek_label);

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
    pane.append(&volume);
    pane.append(&volume_label);

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
    pane.append(&toggles);

    // --- 音声 -------------------------------------------------------------
    pane.append(&ui.label("音声 (操作はネイティブの再生バーから)")?);

    let audio = ui.audio("")?;
    audio.set_sizing(Sizing::fill_width());

    let audio_field = ui.text_input("")?;
    audio_field.set_placeholder("/path/to/bgm.m4a または https://…");
    audio_field.set_sizing(Sizing::fill_width());
    let audio_load = ui.button("読み込む")?;
    audio_load.on_click({
        let audio = audio.clone();
        let field = audio_field.clone();
        move || audio.set_source(&field.text())
    });
    let audio_pick = ui.file_picker("ファイルを選ぶ")?;
    audio_pick.set_filters(&[FileFilter::new(
        "音声",
        ["m4a", "mp3", "aac", "wav", "flac", "ogg"],
    )]);
    audio_pick.on_select({
        let audio = audio.clone();
        let field = audio_field.clone();
        move |entries| apply_picked(entries, &field, |source| audio.set_source(source))
    });

    let audio_row = ui.stack(Orientation::Horizontal)?;
    audio_row.set_spacing(8.0);
    audio_row.append(&audio_field);
    audio_row.append(&audio_load);
    audio_row.append(&audio_pick);
    audio_row.set_sizing(Sizing::fill_width());
    pane.append(&audio_row);
    pane.append(&audio);

    Ok(pane)
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

/// 選ばれたファイルを入力欄に書き戻し、メディアへ渡す。
///
/// 渡すのは `FileEntry::source()`。ネイティブでは絶対パス、Web では
/// ブラウザが作る `blob:` URL になり、どちらもそのまま `set_source` へ渡せる。
fn apply_picked(entries: &[FileEntry], field: &miui::TextInput, set: impl FnOnce(&str)) {
    let Some(entry) = entries.first() else {
        return;
    };
    match entry.source() {
        Some(source) => {
            field.set_text(source);
            set(source);
        }
        None => field.set_text(&format!("{} (場所を取得できません)", entry.name())),
    }
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
