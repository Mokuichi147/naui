//! naui の全ウィジェットのデモ。
//!
//! 表示されるコントロールはすべて OS (またはブラウザ) の実ウィジェット。
//! macOS なら NSButton / NSTextField、Web なら `<button>` / `<input>`。

use std::cell::Cell;
use std::rc::Rc;

use naui::{
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
    let window = ui.window("naui gallery", 680.0, 860.0)?;
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

    tabs.set_selected(5); // 一時: 確認用

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
fn build_media_pane(ui: &Ui) -> Result<naui::Grid> {
    let pane = ui.grid()?;
    pane.set_spacing(0.0, 12.0);
    pane.set_padding(Padding::all(12.0));
    pane.set_sizing(Sizing::fill());
    pane.set_column_track(0, Track::FILL);
    pane.set_row_track(0, Track::Auto);
    pane.set_row_track(1, Track::Auto);
    pane.set_row_track(2, Track::Auto);
    pane.set_row_track(3, Track::FILL);

    let description = ui.label("選んだファイルの種類に合わせて表示形式が切り替わります。")?;
    pane.attach(&description, GridCell::new(0, 0));

    let (image_pane, image) = build_image_pane(ui)?;
    let (video_pane, video) = build_video_pane(ui)?;
    let (audio_pane, audio) = build_audio_pane(ui)?;

    // MediaPlayerElement を WinUI の TabView のコンテンツにすると、
    // TabView が動画・音声ペインを初めて表示する瞬間に Microsoft.UI.Xaml
    // が 0xc000027b で fail-fast する環境がある。Windows では Grid の子を
    // 選択時に差し替え、未選択の MediaPlayerElement を visual tree に置かない。
    #[cfg(target_os = "windows")]
    let forms = {
        let forms = ui.grid()?;
        forms.set_column_track(0, Track::FILL);
        forms.set_row_track(0, Track::FILL);
        forms.attach(&image_pane, GridCell::new(0, 0));
        forms
    };
    #[cfg(not(target_os = "windows"))]
    let forms = {
        let forms = ui.tabs()?;
        forms.add_tab(MEDIA_FORMS[0].0, &image_pane);
        forms.add_tab(MEDIA_FORMS[1].0, &video_pane);
        forms.add_tab(MEDIA_FORMS[2].0, &audio_pane);
        forms.set_selected(0);
        forms
    };

    let status = ui.label("種類: 画像 (同梱のサンプル)")?;

    // 場所を対応するウィジェットへ渡し、その表示形式へ移る。
    let show = {
        let image = image.clone();
        let video = video.clone();
        let audio = audio.clone();
        let forms = forms.clone();
        #[cfg(target_os = "windows")]
        let image_pane = image_pane.clone();
        #[cfg(target_os = "windows")]
        let video_pane = video_pane.clone();
        #[cfg(target_os = "windows")]
        let audio_pane = audio_pane.clone();
        let status = status.clone();
        move |form: usize, source: &str| {
            #[cfg(target_os = "windows")]
            match form {
                0 => forms.replace(&image_pane, GridCell::new(0, 0)),
                1 => forms.replace(&video_pane, GridCell::new(0, 0)),
                _ => forms.replace(&audio_pane, GridCell::new(0, 0)),
            }
            #[cfg(not(target_os = "windows"))]
            forms.select(form);
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
    pane.attach(&row, GridCell::new(0, 1));
    pane.attach(&status, GridCell::new(0, 2));

    // ウィンドウの高さが変わったときも、メディア表示側へ余りを渡す。
    forms.set_sizing(Sizing::fill());
    pane.attach(&forms, GridCell::new(0, 3));

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
fn build_video_pane(ui: &Ui) -> Result<(naui::Grid, naui::Video)> {
    let pane = ui.grid()?;
    pane.set_spacing(0.0, 8.0);
    pane.set_padding(Padding::all(8.0));
    pane.set_sizing(Sizing::fill());
    pane.set_column_track(0, Track::FILL);
    pane.set_row_track(0, Track::FILL);
    pane.set_row_track(1, Track::Auto);

    let media_frame = ui.grid()?;
    media_frame.set_column_track(0, Track::FILL);
    media_frame.set_row_track(0, Track::FILL);
    media_frame.set_sizing(Sizing::fill());

    let video = ui.video("")?;
    video.set_sizing(Sizing::fill());
    // 動画フレームと操作欄は別ウィジェットに分け、操作欄がフレームへ
    // 重ならないようにする。
    media_frame.attach(&video, GridCell::new(0, 0));
    pane.attach(&media_frame, GridCell::new(0, 0));

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
    pane.attach(&controls, GridCell::new(0, 1));
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


/// ネイティブ / Web 共通の起動処理。
pub fn start() -> Result<()> {
    naui::run(Settings::new("naui gallery"), build)
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
