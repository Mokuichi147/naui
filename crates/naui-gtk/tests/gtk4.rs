//! GTK4 / libadwaita の実コントロールに対する動作確認。
//!
//! `emit_clicked` や `GtkAdjustment` の値変更など**ネイティブ側の操作**を
//! 発生させ、Rust のクロージャへ届くこと・ネイティブの状態が変わることを
//! 確かめる。大きさの指定は `gtk_widget_measure` の結果で確かめる。
//!
//! GTK4 はメインスレッドでしか触れないが、Rust の標準テストハーネスは
//! 各テストを別スレッドで走らせる (`--test-threads=1` でも同じ)。
//! そのため `harness = false` にして、自前のランナーをメインスレッドで回す。
//!
//! ディスプレイ (Wayland か X11) が要る。無い環境では `gtk_init` に失敗する。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use naui_core::{
    Align, DialogButtons, DialogResponse, FileFilter, FilePickerMode, Fit, GridCell, Length,
    ListItem, NavItem, Orientation, Padding, PlaybackState, PopupItem, Result, ScrollPolicy,
    SelectionMode, Sizing, Theme, Track,
};
use naui_gtk::{run_for_test, Ui, Widget};

/// テストケース 1 件。
type Case = (&'static str, fn(&Ui) -> Result<()>);

fn main() {
    let cases: &[Case] = &[
        ("ボタンのクリックがクロージャへ届く", button_click),
        (
            "ボタンの on_click を差し替えられる",
            button_click_is_replaced,
        ),
        (
            "チェックボックスが反転し新しい値を通知する",
            checkbox_toggle,
        ),
        (
            "チェックボックスの set_checked は通知しない",
            checkbox_set_is_silent,
        ),
        ("文字列がネイティブと往復する (日本語含む)", text_round_trip),
        (
            "打鍵が通知され、set_text は通知しない",
            text_input_notifies_on_typing,
        ),
        ("複数行入力が改行込みで往復する", text_area_round_trip),
        (
            "複数行入力のプレースホルダーが出入りする",
            text_area_placeholder,
        ),
        (
            "スライダーの操作が通知され範囲に収まる",
            slider_notifies_and_clamps,
        ),
        ("進捗バーが 0..1 に収まる", progress_clamp),
        ("スタックが子を生かし続ける", stack_keeps_children),
        ("スタックの寄せ方が子へ伝わる", stack_align_reaches_children),
        (
            "交差軸の Fill はスタックの寄せ方より優先される",
            stack_fill_wins_over_align,
        ),
        ("固定の大きさが measure に出る", sizing_fixed_is_measured),
        ("Fill が hexpand と Fill 寄せになる", sizing_fill_expands),
        (
            "上限付きの Fill が上限で頭打ちになる",
            sizing_max_caps_natural,
        ),
        (
            "上限付きの Fill が上限まで場所を確保する",
            sizing_max_reserves_room_for_fill,
        ),
        (
            "最小の大きさが measure の下限になる",
            sizing_min_raises_minimum,
        ),
        ("大きさを指定し直すと以前の指定が消える", sizing_is_replaced),
        ("スペーサーが両方向に広がる", spacer_expands),
        (
            "スタックの余白が中身を狭める",
            stack_padding_shrinks_content,
        ),
        ("グリッドが行と列を広げて子を置く", grid_places_children),
        ("グリッドの子を置き換える", grid_replaces_child),
        (
            "グリッドの Fill の列が余りを受け取る",
            grid_fill_track_expands_child,
        ),
        (
            "グリッドの固定幅の列が子の幅を決める",
            grid_fixed_track_sets_width,
        ),
        (
            "スクロールが中身とポリシーを保つ",
            scroll_keeps_child_and_policy,
        ),
        ("ウィンドウを設定して閉じられる", window_lifecycle),
        ("ナビバーの選択がネイティブと往復する", navbar_selection),
        (
            "ナビバーの set_selected は通知しない",
            navbar_set_selected_is_silent,
        ),
        ("ドックが等幅の項目を持つ", dock_items_are_homogeneous),
        ("メニューの選択が 1 つだけ点く", menu_selection_is_exclusive),
        ("選べない項目は選ばれない", nav_skips_disabled_items),
        ("パンくずが末尾を現在地にする", breadcrumbs_last_is_current),
        ("ページ送りが範囲内に収まる", pagination_steps),
        ("タブが中身ごと切り替わる", tabs_selection),
        ("リンクのクリックがクロージャへ届く", link_click),
        ("リストの行が GtkListBox に並ぶ", list_rows_are_native),
        (
            "リストの補助の文字が 2 行目に出る",
            list_detail_makes_a_second_line,
        ),
        (
            "GtkListBox 側の選択がクロージャへ届く",
            list_native_selection_notifies,
        ),
        ("リストの複数選択が 0 件にもなる", list_multiple_selection),
        ("リストが選べない行を飛ばす", list_skips_disabled_rows),
        (
            "リストの通知の中からリストを操作できる",
            list_callback_can_touch_the_list,
        ),
        (
            "ポップアップが GMenu へ写る",
            popup_items_reach_the_menu_model,
        ),
        (
            "ポップアップの操作がクロージャへ届く",
            popup_action_notifies,
        ),
        (
            "ポップアップは区切り線と無効な項目を飛ばす",
            popup_skips_separator_and_disabled,
        ),
        (
            "画像がローカルのファイルから読み込まれる",
            image_loads_a_local_file,
        ),
        (
            "収め方が GtkPicture の content-fit になる",
            image_fit_maps_to_content_fit,
        ),
        ("メディアの場所が往復する", media_source_round_trips),
        ("再生前の状態は Idle", media_starts_idle),
        (
            "ファイル選択がボタンとして構成され設定を保つ",
            file_picker_configuration,
        ),
        (
            "ダイアログの設定が AdwAlertDialog へ届く",
            dialog_configuration_reaches_the_alert,
        ),
        (
            "ボタンを指定しないダイアログに OK が出る",
            dialog_without_buttons_shows_ok,
        ),
        (
            "ダイアログの応答がクロージャへ届く",
            dialog_response_reaches_the_closure,
        ),
        (
            "出していないダイアログは閉じても何も起きない",
            dialog_is_closed_until_opened,
        ),
        ("テーマを実行中に切り替えられる", theme_switch),
    ];

    let mut failed = 0;
    for (name, case) in cases {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            run_for_test(*case).expect("テスト用の起動")
        }));
        match result {
            Ok(()) => println!("ok   ... {name}"),
            Err(_) => {
                println!("FAIL ... {name}");
                failed += 1;
            }
        }
    }

    println!("\n{} 件中 {} 件成功", cases.len(), cases.len() - failed);
    if failed > 0 {
        std::process::exit(1);
    }
}

// --------------------------------------------------------------- 小さな道具

/// 「何回呼ばれ、最後に何が届いたか」を数えるための入れ物。
fn recorder<T: Clone + 'static>() -> (Rc<RefCell<Vec<T>>>, impl FnMut(T) + 'static) {
    let log = Rc::new(RefCell::new(Vec::new()));
    let sink = log.clone();
    (log, move |value| sink.borrow_mut().push(value))
}

/// 幅の (最小, 自然な大きさ)。
fn measure_width(widget: &impl IsA<gtk::Widget>) -> (i32, i32) {
    let (min, nat, _, _) = widget.as_ref().measure(gtk::Orientation::Horizontal, -1);
    (min, nat)
}

/// 高さの (最小, 自然な大きさ)。
fn measure_height(widget: &impl IsA<gtk::Widget>) -> (i32, i32) {
    let (min, nat, _, _) = widget.as_ref().measure(gtk::Orientation::Vertical, -1);
    (min, nat)
}

/// ウィジェットがコンテナへ入るときの入れ物 (`SizeBin`)。
fn bin_of(widget: &dyn Widget) -> gtk::Widget {
    widget
        .native_widget()
        .ancestor(naui_gtk::SizeBin::static_type())
        .expect("SizeBin に包まれている")
}

/// `GtkBox` などの子を順に集める。
fn children(widget: &impl IsA<gtk::Widget>) -> Vec<gtk::Widget> {
    let mut out = Vec::new();
    let mut child = widget.as_ref().first_child();
    while let Some(current) = child {
        child = current.next_sibling();
        out.push(current);
    }
    out
}

/// 1x1 の PNG。画像の読み込みを実ファイルで確かめるために書き出す。
const PNG_1X1: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90, 0x77, 0x53,
    0xde, 0x00, 0x00, 0x00, 0x0c, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0xf8, 0xcf, 0xc0, 0x00,
    0x00, 0x03, 0x01, 0x01, 0x00, 0xc9, 0xfe, 0x92, 0xef, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e,
    0x44, 0xae, 0x42, 0x60, 0x82,
];

fn write_png() -> std::path::PathBuf {
    let path = std::env::temp_dir().join("naui-gtk-test-1x1.png");
    std::fs::write(&path, PNG_1X1).expect("PNG の書き出し");
    path
}

// ----------------------------------------------------------- 基本ウィジェット

fn button_click(ui: &Ui) -> Result<()> {
    let button = ui.button("押す")?;
    let count = Rc::new(Cell::new(0));
    button.on_click({
        let count = count.clone();
        move || count.set(count.get() + 1)
    });

    let native: gtk::Button = button.native_widget().downcast().expect("GtkButton");
    native.emit_clicked();
    native.emit_clicked();
    assert_eq!(count.get(), 2);
    Ok(())
}

fn button_click_is_replaced(ui: &Ui) -> Result<()> {
    let button = ui.button("押す")?;
    let (log, sink) = recorder::<&'static str>();
    button.on_click({
        let mut sink = sink;
        move || sink("最初")
    });
    let (log2, sink2) = recorder::<&'static str>();
    button.on_click({
        let mut sink2 = sink2;
        move || sink2("あと")
    });

    let native: gtk::Button = button.native_widget().downcast().expect("GtkButton");
    native.emit_clicked();
    assert!(log.borrow().is_empty(), "差し替え前のものは呼ばれない");
    assert_eq!(log2.borrow().as_slice(), ["あと"]);
    Ok(())
}

fn checkbox_toggle(ui: &Ui) -> Result<()> {
    let checkbox = ui.checkbox("同意する")?;
    let (log, sink) = recorder::<bool>();
    checkbox.on_toggle(sink);

    let native: gtk::CheckButton = checkbox.native_widget().downcast().expect("GtkCheckButton");
    assert!(!checkbox.is_checked());
    native.activate();
    assert!(checkbox.is_checked());
    native.activate();
    assert!(!checkbox.is_checked());
    assert_eq!(log.borrow().as_slice(), [true, false]);
    Ok(())
}

fn checkbox_set_is_silent(ui: &Ui) -> Result<()> {
    let checkbox = ui.checkbox("同意する")?;
    let (log, sink) = recorder::<bool>();
    checkbox.on_toggle(sink);

    checkbox.set_checked(true);
    assert!(checkbox.is_checked());
    checkbox.set_checked(false);
    assert!(log.borrow().is_empty(), "プログラムからの変更は通知しない");
    Ok(())
}

fn text_round_trip(ui: &Ui) -> Result<()> {
    let input = ui.text_input("")?;
    input.set_text("こんにちは 世界");
    assert_eq!(input.text(), "こんにちは 世界");

    let native: gtk::Entry = input.native_widget().downcast().expect("GtkEntry");
    assert_eq!(native.text().as_str(), "こんにちは 世界");

    input.set_placeholder("名前");
    assert_eq!(
        native.placeholder_text().map(|t| t.to_string()),
        Some("名前".to_string())
    );

    input.set_enabled(false);
    assert!(!native.is_sensitive());
    Ok(())
}

fn text_input_notifies_on_typing(ui: &Ui) -> Result<()> {
    let input = ui.text_input("")?;
    let (log, sink) = recorder::<String>();
    input.on_change({
        let mut sink = sink;
        move |text: &str| sink(text.to_string())
    });

    input.set_text("あ");
    assert!(log.borrow().is_empty(), "set_text は通知しない");

    // 利用者の打鍵と同じく、バッファーへ差し込む。
    let native: gtk::Entry = input.native_widget().downcast().expect("GtkEntry");
    let mut position = native.text_length();
    native.buffer().insert_text(position, "い");
    position = native.text_length();
    native.buffer().insert_text(position, "う");

    assert_eq!(log.borrow().as_slice(), ["あい", "あいう"]);
    assert_eq!(input.text(), "あいう");
    Ok(())
}

fn text_area_round_trip(ui: &Ui) -> Result<()> {
    let area = ui.text_area("")?;
    let (log, sink) = recorder::<String>();
    area.on_change({
        let mut sink = sink;
        move |text: &str| sink(text.to_string())
    });

    area.set_text("1 行目\n2 行目");
    assert_eq!(area.text(), "1 行目\n2 行目");
    assert!(log.borrow().is_empty(), "set_text は通知しない");

    let native: gtk::TextView = area.native_widget().downcast().expect("GtkTextView");
    native.buffer().insert_at_cursor("\n3 行目");
    assert_eq!(area.text(), "1 行目\n2 行目\n3 行目");
    assert_eq!(log.borrow().len(), 1);
    Ok(())
}

fn text_area_placeholder(ui: &Ui) -> Result<()> {
    let area = ui.text_area("")?;
    area.set_placeholder("メモ");

    let native: gtk::TextView = area.native_widget().downcast().expect("GtkTextView");
    let overlay = bin_of(&area).first_child().expect("Overlay");
    let placeholder = children(&overlay)
        .into_iter()
        .find_map(|child| child.downcast::<gtk::Label>().ok())
        .expect("プレースホルダーのラベル");

    assert!(placeholder.is_visible(), "空のときは出る");
    assert_eq!(placeholder.text().as_str(), "メモ");
    // 入力の邪魔をしない。
    assert!(!placeholder.can_target());

    native.buffer().insert_at_cursor("あ");
    assert!(!placeholder.is_visible(), "何か入っていれば消える");

    area.set_text("");
    assert!(placeholder.is_visible(), "空に戻れば出る");
    Ok(())
}

fn slider_notifies_and_clamps(ui: &Ui) -> Result<()> {
    let slider = ui.slider(0.0, 10.0)?;
    let (log, sink) = recorder::<f64>();
    slider.on_change(sink);

    slider.set_value(3.0);
    assert_eq!(slider.value(), 3.0);
    assert!(log.borrow().is_empty(), "set_value は通知しない");

    // つまみを動かしたのと同じく、GtkAdjustment を動かす。
    let native: gtk::Scale = slider.native_widget().downcast().expect("GtkScale");
    native.adjustment().set_value(7.5);
    assert_eq!(log.borrow().as_slice(), [7.5]);

    // 範囲の外はネイティブが丸める。
    slider.set_value(99.0);
    assert_eq!(slider.value(), 10.0);
    slider.set_value(-99.0);
    assert_eq!(slider.value(), 0.0);
    Ok(())
}

fn progress_clamp(ui: &Ui) -> Result<()> {
    let progress = ui.progress_bar()?;
    progress.set_value(0.25);
    assert_eq!(progress.value(), 0.25);
    progress.set_value(9.0);
    assert_eq!(progress.value(), 1.0);
    progress.set_value(-1.0);
    assert_eq!(progress.value(), 0.0);

    let native: gtk::ProgressBar = progress.native_widget().downcast().expect("GtkProgressBar");
    assert_eq!(native.fraction(), 0.0);
    Ok(())
}

// ------------------------------------------------------------------ レイアウト

fn stack_keeps_children(ui: &Ui) -> Result<()> {
    let stack = ui.stack(Orientation::Vertical)?;
    assert!(stack.is_empty());

    let label = ui.label("あ")?;
    let button = ui.button("い")?;
    stack.append(&label);
    stack.append(&button);
    assert_eq!(stack.len(), 2);

    let native: gtk::Box = stack.native_widget().downcast().expect("GtkBox");
    assert_eq!(children(&native).len(), 2);
    assert_eq!(native.orientation(), gtk::Orientation::Vertical);

    // ハンドルを落としてもネイティブは生きている。
    drop(label);
    assert_eq!(children(&native).len(), 2);
    Ok(())
}

fn stack_align_reaches_children(ui: &Ui) -> Result<()> {
    let stack = ui.stack(Orientation::Vertical)?;
    let label = ui.label("あ")?;
    stack.append(&label);

    stack.set_align(Align::Start);
    assert_eq!(bin_of(&label).halign(), gtk::Align::Start);
    stack.set_align(Align::End);
    assert_eq!(bin_of(&label).halign(), gtk::Align::End);

    // あとから足した子にも同じ寄せ方が付く。
    let button = ui.button("い")?;
    stack.append(&button);
    assert_eq!(bin_of(&button).halign(), gtk::Align::End);
    Ok(())
}

fn stack_fill_wins_over_align(ui: &Ui) -> Result<()> {
    let stack = ui.stack(Orientation::Vertical)?;
    let field = ui.text_input("")?;
    field.set_sizing(Sizing::fill_width());
    stack.append(&field);

    stack.set_align(Align::Start);
    assert_eq!(
        bin_of(&field).halign(),
        gtk::Align::Fill,
        "交差軸の Fill はスタックの寄せ方より優先される"
    );
    assert!(bin_of(&field).hexpands());
    Ok(())
}

fn sizing_fixed_is_measured(ui: &Ui) -> Result<()> {
    let button = ui.button("押す")?;
    button.set_sizing(Sizing::fixed(200.0, 40.0));

    let bin = bin_of(&button);
    assert_eq!(measure_width(&bin), (200, 200));
    assert_eq!(measure_height(&bin), (40, 40));
    // 固定と言った以上、余りは受け取らない。
    assert!(!bin.hexpands());
    assert_ne!(bin.halign(), gtk::Align::Fill);
    Ok(())
}

fn sizing_fill_expands(ui: &Ui) -> Result<()> {
    let button = ui.button("押す")?;
    button.set_sizing(Sizing::fill());

    let bin = bin_of(&button);
    assert!(bin.hexpands());
    assert!(bin.vexpands());
    assert_eq!(bin.halign(), gtk::Align::Fill);
    assert_eq!(bin.valign(), gtk::Align::Fill);
    Ok(())
}

fn sizing_max_caps_natural(ui: &Ui) -> Result<()> {
    const TEXT: &str = "とても長い文字列がここに入っていて自然な幅は大きい";
    // GTK4 は一度測った結果をレイアウトのたびに捨てる。ここではレイアウトを
    // 回さないので、上限あり / なしを別のラベルで比べる。
    let plain = ui.label(TEXT)?;
    let (_, natural) = measure_width(&bin_of(&plain));
    assert!(natural > 40, "そのままなら 40px より広い");

    let capped = ui.label(TEXT)?;
    capped.set_sizing(Sizing::fill().max_width(40.0));
    let bin = bin_of(&capped);
    let (minimum, natural) = measure_width(&bin);
    assert_eq!(natural, 40, "自然な大きさは上限で頭打ちになる");
    assert!(minimum <= 40, "狭いときは上限より小さくなれる");
    // 上限を効かせるため、寄せ方は Fill にしない。
    assert_ne!(bin.halign(), gtk::Align::Fill);
    assert!(bin.hexpands(), "余りは受け取る");
    Ok(())
}

fn sizing_max_reserves_room_for_fill(ui: &Ui) -> Result<()> {
    // 中身の自然な高さは小さいが、`Fill` に上限を付けたときの上限は
    // 「通常時に確保したい大きさ」も兼ねる (macOS / Web と同じ)。
    let label = ui.label("あ")?;
    let (_, plain) = measure_height(&bin_of(&label));
    assert!(plain < 315, "そのままなら 315px より低い");

    let framed = ui.label("あ")?;
    framed.set_sizing(Sizing::fill().max_height(315.0));
    let (minimum, natural) = measure_height(&bin_of(&framed));
    assert_eq!(natural, 315, "空きがあれば上限まで広がる");
    assert!(minimum <= 315, "空きが足りなければ上限より小さくなる");

    // 上限だけ (Fill ではない) のときは、中身の大きさを超えない。
    let hugging = ui.label("あ")?;
    hugging.set_sizing(Sizing::new().max_height(315.0));
    assert_eq!(measure_height(&bin_of(&hugging)).1, plain);
    Ok(())
}

fn sizing_min_raises_minimum(ui: &Ui) -> Result<()> {
    let button = ui.button("小")?;
    let bin = bin_of(&button);
    let (before, _) = measure_width(&bin);
    assert!(before < 300);

    button.set_sizing(Sizing::new().min_width(300.0));
    let (minimum, natural) = measure_width(&bin);
    assert_eq!(minimum, 300);
    assert_eq!(natural, 300);
    Ok(())
}

fn sizing_is_replaced(ui: &Ui) -> Result<()> {
    let button = ui.button("押す")?;
    let bin = bin_of(&button);

    button.set_sizing(Sizing::fixed(200.0, 40.0));
    assert_eq!(measure_width(&bin), (200, 200));

    // 指定し直すと以前のものは残らない。
    button.set_sizing(Sizing::new());
    let (_, natural) = measure_width(&bin);
    assert!(natural < 200, "固定幅が残っていない");
    assert!(!bin.hexpands());

    button.set_sizing(Sizing::fixed(120.0, 40.0));
    assert_eq!(measure_width(&bin), (120, 120));
    Ok(())
}

fn spacer_expands(ui: &Ui) -> Result<()> {
    let spacer = ui.spacer()?;
    let bin = bin_of(&spacer);
    assert!(bin.hexpands());
    assert!(bin.vexpands());
    // 中身が無いので自然な大きさは 0。
    assert_eq!(measure_width(&bin), (0, 0));
    Ok(())
}

fn stack_padding_shrinks_content(ui: &Ui) -> Result<()> {
    let stack = ui.stack(Orientation::Vertical)?;
    let label = ui.label("あ")?;
    stack.append(&label);

    let bin = bin_of(&stack);
    let (_, before) = measure_width(&bin);
    stack.set_padding(Padding::symmetric(10.0, 24.0));
    let (_, after) = measure_width(&bin);
    assert_eq!(after - before, 48, "左右の余白ぶんだけ広がる");

    let native: gtk::Box = stack.native_widget().downcast().expect("GtkBox");
    assert_eq!(native.margin_start(), 24);
    assert_eq!(native.margin_top(), 10);
    Ok(())
}

fn grid_places_children(ui: &Ui) -> Result<()> {
    let grid = ui.grid()?;
    assert!(grid.is_empty());
    assert_eq!((grid.columns(), grid.rows()), (0, 0));

    grid.attach(&ui.label("名前")?, GridCell::new(0, 0));
    grid.attach(&ui.text_input("")?, GridCell::new(1, 0));
    grid.attach(&ui.label("備考")?, GridCell::new(0, 1).span(2, 1));

    assert_eq!(grid.len(), 3);
    assert_eq!(grid.columns(), 2);
    assert_eq!(grid.rows(), 2);

    let native: gtk::Grid = grid.native_widget().downcast().expect("GtkGrid");
    assert_eq!(children(&native).len(), 3);

    grid.set_spacing(12.0, 8.0);
    assert_eq!(native.column_spacing(), 12);
    assert_eq!(native.row_spacing(), 8);
    Ok(())
}

fn grid_replaces_child(ui: &Ui) -> Result<()> {
    let grid = ui.grid()?;
    let first = ui.label("さいしょ")?;
    grid.attach(&first, GridCell::new(0, 0));
    assert_eq!(grid.len(), 1);

    let second = ui.label("あと")?;
    grid.replace(&second, GridCell::new(0, 0));
    assert_eq!(grid.len(), 1);

    let native: gtk::Grid = grid.native_widget().downcast().expect("GtkGrid");
    let placed = children(&native);
    assert_eq!(placed.len(), 1);
    assert_eq!(placed[0], bin_of(&second));
    Ok(())
}

fn grid_fill_track_expands_child(ui: &Ui) -> Result<()> {
    let grid = ui.grid()?;
    let field = ui.text_input("")?;
    grid.attach(&field, GridCell::new(1, 0));
    grid.set_column_track(1, Track::FILL);

    let bin = bin_of(&field);
    assert!(bin.hexpands(), "Fill の列に入った子が余りを受け取る");
    assert_eq!(bin.halign(), gtk::Align::Fill);
    Ok(())
}

fn grid_fixed_track_sets_width(ui: &Ui) -> Result<()> {
    let grid = ui.grid()?;
    let label = ui.label("名前")?;
    grid.attach(&label, GridCell::new(0, 0));
    grid.set_column_track(0, Track::Fixed(96.0));

    assert_eq!(measure_width(&bin_of(&label)), (96, 96));

    // 子が自分で大きさを指定していれば、そちらが優先される。
    let other = ui.label("備考")?;
    other.set_sizing(Sizing::new().width(Length::Fixed(150.0)));
    grid.attach(&other, GridCell::new(0, 1));
    assert_eq!(measure_width(&bin_of(&other)), (150, 150));
    Ok(())
}

fn scroll_keeps_child_and_policy(ui: &Ui) -> Result<()> {
    let scroll = ui.scroll()?;
    let stack = ui.stack(Orientation::Vertical)?;
    stack.append(&ui.label("あ")?);
    scroll.set_child(&stack);

    let native: gtk::ScrolledWindow = scroll
        .native_widget()
        .downcast()
        .expect("GtkScrolledWindow");
    // GtkScrolledWindow は、自分でスクロールしない中身を GtkViewport に載せる。
    let viewport: gtk::Viewport = native
        .child()
        .expect("中身")
        .downcast()
        .expect("GtkViewport");
    assert_eq!(viewport.child(), Some(bin_of(&stack)));

    scroll.set_policy(ScrollPolicy::Never, ScrollPolicy::Always);
    assert_eq!(
        native.policy(),
        (gtk::PolicyType::Never, gtk::PolicyType::Always)
    );
    Ok(())
}

fn window_lifecycle(ui: &Ui) -> Result<()> {
    let window = ui.window("naui", 320.0, 200.0)?;
    assert_eq!(window.title(), "naui");
    window.set_title("かうんた");
    assert_eq!(window.title(), "かうんた");

    let stack = ui.stack(Orientation::Vertical)?;
    stack.append(&ui.label("0")?);
    window.set_child(&stack);

    let native = window.native_window();
    assert_eq!(native.content(), Some(bin_of(&stack)));
    // ウィンドウの中身は窓いっぱいに広がる。
    assert_eq!(bin_of(&stack).halign(), gtk::Align::Fill);
    assert_eq!(bin_of(&stack).valign(), gtk::Align::Fill);

    assert!(!window.is_visible());
    window.show();
    assert!(window.is_visible());
    window.close();
    assert!(!window.is_visible());

    // 弱参照はハンドルが生きている間だけ辿れる。
    let weak = window.downgrade();
    assert!(weak.upgrade().is_some());
    Ok(())
}

// -------------------------------------------------------------- ナビゲーション

/// 項目のトグルボタンだけを取り出す。
fn toggle_buttons(widget: &dyn Widget) -> Vec<gtk::ToggleButton> {
    fn walk(widget: &gtk::Widget, out: &mut Vec<gtk::ToggleButton>) {
        let mut child = widget.first_child();
        while let Some(current) = child {
            child = current.next_sibling();
            if let Ok(toggle) = current.clone().downcast::<gtk::ToggleButton>() {
                out.push(toggle);
            } else {
                walk(&current, out);
            }
        }
    }
    let mut out = Vec::new();
    walk(&widget.native_widget(), &mut out);
    out
}

fn navbar_selection(ui: &Ui) -> Result<()> {
    let navbar = ui.navbar("naui")?;
    assert_eq!(navbar.title(), "naui");
    navbar.set_title("ギャラリー");
    assert_eq!(navbar.title(), "ギャラリー");

    navbar.set_items(&NavItem::list(["ホーム", "検索", "設定"]));
    assert_eq!(navbar.len(), 3);
    assert_eq!(navbar.selected(), None);

    let (log, sink) = recorder::<usize>();
    navbar.on_select(sink);

    // 実際のボタンを押す。
    let buttons = toggle_buttons(&navbar);
    assert_eq!(buttons.len(), 3);
    buttons[1].emit_clicked();

    assert_eq!(navbar.selected(), Some(1));
    assert_eq!(log.borrow().as_slice(), [1]);
    assert!(buttons[1].is_active());
    assert!(!buttons[0].is_active());
    Ok(())
}

fn navbar_set_selected_is_silent(ui: &Ui) -> Result<()> {
    let navbar = ui.navbar("naui")?;
    navbar.set_items(&NavItem::list(["一覧", "詳細"]));
    let (log, sink) = recorder::<usize>();
    navbar.on_select(sink);

    navbar.set_selected(1);
    assert_eq!(navbar.selected(), Some(1));
    assert!(log.borrow().is_empty(), "set_selected は通知しない");

    navbar.select(0);
    assert_eq!(log.borrow().as_slice(), [0]);
    Ok(())
}

fn dock_items_are_homogeneous(ui: &Ui) -> Result<()> {
    let dock = ui.dock()?;
    dock.set_items(&NavItem::list(["ホーム", "とても長い項目名", "設定"]));
    assert_eq!(dock.len(), 3);

    let native: gtk::Box = dock.native_widget().downcast().expect("GtkBox");
    assert!(native.is_homogeneous(), "ドックの項目は等幅");
    Ok(())
}

fn menu_selection_is_exclusive(ui: &Ui) -> Result<()> {
    let menu = ui.menu()?;
    menu.set_items(&NavItem::list(["一覧", "詳細", "設定"]));
    let buttons = toggle_buttons(&menu);

    buttons[0].emit_clicked();
    buttons[2].emit_clicked();
    assert_eq!(menu.selected(), Some(2));
    assert_eq!(
        buttons.iter().filter(|b| b.is_active()).count(),
        1,
        "点いているのは 1 つだけ"
    );

    let native: gtk::Box = menu.native_widget().downcast().expect("GtkBox");
    assert_eq!(native.orientation(), gtk::Orientation::Vertical);
    Ok(())
}

fn nav_skips_disabled_items(ui: &Ui) -> Result<()> {
    let menu = ui.menu()?;
    menu.set_items(&[
        NavItem::new("一覧"),
        NavItem::new("詳細").enabled(false),
        NavItem::new("設定"),
    ]);
    let (log, sink) = recorder::<usize>();
    menu.on_select(sink);

    let buttons = toggle_buttons(&menu);
    assert!(!buttons[1].is_sensitive(), "選べない項目は押せない");

    menu.select(1);
    assert_eq!(menu.selected(), None, "選べない項目は選ばれない");
    assert!(log.borrow().is_empty());

    menu.select(2);
    assert_eq!(menu.selected(), Some(2));
    assert_eq!(log.borrow().as_slice(), [2]);
    Ok(())
}

fn breadcrumbs_last_is_current(ui: &Ui) -> Result<()> {
    let breadcrumbs = ui.breadcrumbs()?;
    breadcrumbs.set_items(&NavItem::list(["ホーム", "書類", "2026"]));
    assert_eq!(breadcrumbs.len(), 3);
    assert_eq!(breadcrumbs.selected(), Some(2), "末尾がいまいる場所");

    let (log, sink) = recorder::<usize>();
    breadcrumbs.on_select(sink);
    toggle_buttons(&breadcrumbs)[0].emit_clicked();
    assert_eq!(log.borrow().as_slice(), [0]);
    assert_eq!(breadcrumbs.selected(), Some(0));

    // 区切りのラベルが項目の間に入る。
    let native: gtk::Box = breadcrumbs.native_widget().downcast().expect("GtkBox");
    let separators = children(&native)
        .into_iter()
        .filter(|child| child.is::<gtk::Label>())
        .count();
    assert_eq!(separators, 2);
    Ok(())
}

fn pagination_steps(ui: &Ui) -> Result<()> {
    let pagination = ui.pagination(3)?;
    assert_eq!(pagination.page_count(), 3);
    assert_eq!(pagination.page(), 0);

    let (log, sink) = recorder::<usize>();
    pagination.on_change(sink);

    pagination.go_previous();
    assert_eq!(pagination.page(), 0, "先頭より前へは行かない");
    assert!(log.borrow().is_empty());

    pagination.go_next();
    pagination.go_next();
    assert_eq!(pagination.page(), 2);
    pagination.go_next();
    assert_eq!(pagination.page(), 2, "末尾より後ろへは行かない");
    assert_eq!(log.borrow().as_slice(), [1, 2]);

    pagination.set_page(0);
    assert_eq!(pagination.page(), 0);
    assert_eq!(log.borrow().len(), 2, "set_page は通知しない");
    Ok(())
}

fn tabs_selection(ui: &Ui) -> Result<()> {
    let tabs = ui.tabs()?;
    assert!(tabs.is_empty());

    let first = ui.label("1 枚目")?;
    let second = ui.label("2 枚目")?;
    tabs.add_tab("あ", &first);
    tabs.add_tab("い", &second);
    assert_eq!(tabs.len(), 2);
    assert_eq!(tabs.selected(), Some(0));

    let (log, sink) = recorder::<usize>();
    tabs.on_select(sink);

    // GtkNotebook 側でページを切り替える (利用者がタブを押したのと同じ)。
    let native: gtk::Notebook = tabs.native_widget().downcast().expect("GtkNotebook");
    native.set_current_page(Some(1));
    assert_eq!(tabs.selected(), Some(1));
    assert_eq!(log.borrow().as_slice(), [1]);

    tabs.set_selected(0);
    assert_eq!(tabs.selected(), Some(0));
    assert_eq!(log.borrow().len(), 1, "set_selected は通知しない");

    // 同じタブを選び直しても 1 回だけ通知される。
    tabs.select(0);
    assert_eq!(log.borrow().as_slice(), [1, 0]);
    Ok(())
}

fn link_click(ui: &Ui) -> Result<()> {
    let link = ui.link("naui", "")?;
    assert_eq!(link.text(), "naui");
    assert_eq!(link.href(), "");

    let count = Rc::new(Cell::new(0));
    link.on_click({
        let count = count.clone();
        move || count.set(count.get() + 1)
    });

    let native: gtk::LinkButton = link.native_widget().downcast().expect("GtkLinkButton");
    // 行き先が空なので、押しても外部を開こうとはしない。
    native.emit_clicked();
    assert_eq!(count.get(), 1);

    link.set_href("https://example.com");
    assert_eq!(link.href(), "https://example.com");
    link.set_text("例");
    assert_eq!(link.text(), "例");
    Ok(())
}

// ------------------------------------------------------------------- リスト

fn list_box_of(list: &naui_gtk::List) -> gtk::ListBox {
    list.native_widget().downcast().expect("GtkListBox")
}

fn list_rows_are_native(ui: &Ui) -> Result<()> {
    let list = ui.list()?;
    assert!(list.is_empty());

    list.set_items(&ListItem::list(["札幌", "東京", "那覇"]));
    assert_eq!(list.len(), 3);

    let native = list_box_of(&list);
    let rows = children(&native);
    assert_eq!(rows.len(), 3);
    assert!(rows[0].is::<gtk::ListBoxRow>());

    // 行を差し替えると、選択も外れる。
    list.set_selected(1);
    assert_eq!(list.selected(), Some(1));
    list.set_items(&ListItem::list(["福岡"]));
    assert_eq!(list.len(), 1);
    assert_eq!(list.selected(), None);
    Ok(())
}

fn list_detail_makes_a_second_line(ui: &Ui) -> Result<()> {
    let list = ui.list()?;
    list.set_items(&[
        ListItem::new("東京").detail("13,960,000 人"),
        ListItem::new("札幌"),
    ]);

    let native = list_box_of(&list);
    let rows = children(&native);
    let labels = |row: &gtk::Widget| -> Vec<String> {
        let content = row
            .clone()
            .downcast::<gtk::ListBoxRow>()
            .expect("GtkListBoxRow")
            .child()
            .expect("中身");
        children(&content)
            .into_iter()
            .filter_map(|child| child.downcast::<gtk::Label>().ok())
            .map(|label| label.text().to_string())
            .collect()
    };
    assert_eq!(labels(&rows[0]), ["東京", "13,960,000 人"]);
    assert_eq!(labels(&rows[1]), ["札幌"]);
    Ok(())
}

fn list_native_selection_notifies(ui: &Ui) -> Result<()> {
    let list = ui.list()?;
    list.set_items(&ListItem::list(["札幌", "東京", "那覇"]));

    let (log, sink) = recorder::<Vec<usize>>();
    list.on_select({
        let mut sink = sink;
        move |indices: &[usize]| sink(indices.to_vec())
    });

    // GtkListBox 側で行を選ぶ (利用者がクリックしたのと同じ)。
    let native = list_box_of(&list);
    let row = native.row_at_index(2).expect("3 行目");
    native.select_row(Some(&row));

    assert_eq!(list.selection(), vec![2]);
    assert_eq!(log.borrow().as_slice(), [vec![2]]);

    // プログラムからの変更は通知しない。
    list.set_selection(&[0]);
    assert_eq!(list.selection(), vec![0]);
    assert_eq!(log.borrow().len(), 1);

    // select は通知する。
    list.select(1);
    assert_eq!(log.borrow().as_slice(), [vec![2], vec![1]]);
    Ok(())
}

fn list_multiple_selection(ui: &Ui) -> Result<()> {
    let list = ui.list()?;
    list.set_items(&ListItem::list(["a", "b", "c"]));
    list.set_selection_mode(SelectionMode::Multiple);
    assert_eq!(list.selection_mode(), SelectionMode::Multiple);

    list.set_selection(&[2, 0, 2, 9]);
    assert_eq!(
        list.selection(),
        vec![0, 2],
        "並べ替えて重複と範囲外を捨てる"
    );

    list.clear_selection();
    assert!(list.selection().is_empty(), "0 件にもなる");
    assert_eq!(list.selected(), None);

    // 単一選択に戻すと先頭の 1 件だけになる。
    list.set_selection_mode(SelectionMode::Single);
    list.set_selection(&[2, 1]);
    assert_eq!(list.selection(), vec![1]);
    Ok(())
}

fn list_skips_disabled_rows(ui: &Ui) -> Result<()> {
    let list = ui.list()?;
    list.set_items(&[
        ListItem::new("a"),
        ListItem::new("b").enabled(false),
        ListItem::new("c"),
    ]);

    let native = list_box_of(&list);
    let row = native.row_at_index(1).expect("2 行目");
    assert!(
        !row.is_selectable(),
        "選べない行は GtkListBox 側でも選べない"
    );

    list.set_selection(&[1]);
    assert!(list.selection().is_empty());

    list.set_selection(&[0, 1, 2]);
    assert_eq!(list.selection(), vec![0], "単一選択では先頭の 1 件だけ");
    Ok(())
}

fn list_callback_can_touch_the_list(ui: &Ui) -> Result<()> {
    let list = ui.list()?;
    list.set_items(&ListItem::list(["a", "b", "c"]));

    let seen = Rc::new(RefCell::new(Vec::new()));
    {
        let list = list.clone();
        let seen = seen.clone();
        list.clone().on_select(move |indices: &[usize]| {
            // 通知の中から一覧を触っても借用が衝突しない。
            seen.borrow_mut().push((indices.to_vec(), list.len()));
        });
    }

    list.select(1);
    assert_eq!(seen.borrow().as_slice(), [(vec![1], 3)]);
    Ok(())
}

// ---------------------------------------------------------------- ポップアップ

fn popup_items_reach_the_menu_model(ui: &Ui) -> Result<()> {
    let popup = ui.popup_menu()?;
    popup.set_items(&[
        PopupItem::new("コピー"),
        PopupItem::separator(),
        PopupItem::new("削除").enabled(false),
    ]);
    assert_eq!(popup.len(), 3, "区切り線も数に入る");

    let model = popup.native_popover().menu_model().expect("GMenu");
    // 区切り線で節が分かれるので、上位のモデルは 2 つの節を持つ。
    assert_eq!(model.n_items(), 2);
    Ok(())
}

fn popup_action_notifies(ui: &Ui) -> Result<()> {
    let popup = ui.popup_menu()?;
    popup.set_items(&PopupItem::list(["コピー", "貼り付け"]));

    let (log, sink) = recorder::<usize>();
    popup.on_select(sink);

    // GTK4 のメニューは操作 (GAction) で動く。項目を選んだのと同じ経路。
    popup
        .native_popover()
        .activate_action("naui-popup.item1", None)
        .expect("メニューの操作");
    assert_eq!(log.borrow().as_slice(), [1]);
    Ok(())
}

fn popup_skips_separator_and_disabled(ui: &Ui) -> Result<()> {
    let popup = ui.popup_menu()?;
    popup.set_items(&[
        PopupItem::new("コピー"),
        PopupItem::separator(),
        PopupItem::new("削除").enabled(false),
    ]);
    let (log, sink) = recorder::<usize>();
    popup.on_select(sink);

    popup.select(1);
    popup.select(2);
    popup.select(9);
    assert!(
        log.borrow().is_empty(),
        "区切り線と選べない項目は通知しない"
    );

    popup.select(0);
    assert_eq!(log.borrow().as_slice(), [0]);
    Ok(())
}

// ------------------------------------------------------------------ メディア

fn image_loads_a_local_file(ui: &Ui) -> Result<()> {
    let path = write_png();
    let image = ui.image(path.to_str().expect("パス"))?;
    assert_eq!(image.source(), path.to_str().unwrap());
    assert!(image.is_loaded(), "実ファイルが読み込まれる");

    let native: gtk::Picture = image.native_widget().downcast().expect("GtkPicture");
    assert!(native.paintable().is_some());

    image.set_source("");
    assert!(!image.is_loaded());
    Ok(())
}

fn image_fit_maps_to_content_fit(ui: &Ui) -> Result<()> {
    let image = ui.image("")?;
    let native: gtk::Picture = image.native_widget().downcast().expect("GtkPicture");

    for (fit, expected) in [
        (Fit::Contain, gtk::ContentFit::Contain),
        (Fit::Cover, gtk::ContentFit::Cover),
        (Fit::Fill, gtk::ContentFit::Fill),
        // 原寸にあたるものが GTK4 に無いので、拡大しない ScaleDown へ写す。
        (Fit::None, gtk::ContentFit::ScaleDown),
    ] {
        image.set_fit(fit);
        assert_eq!(native.content_fit(), expected);
    }

    image.set_alt("ねこ");
    assert_eq!(
        native.alternative_text().map(|t| t.to_string()),
        Some("ねこ".to_string())
    );
    Ok(())
}

fn media_source_round_trips(ui: &Ui) -> Result<()> {
    let video = ui.video("/tmp/clip.mp4")?;
    assert_eq!(video.source(), "/tmp/clip.mp4");
    video.set_source("/tmp/other.mp4");
    assert_eq!(video.source(), "/tmp/other.mp4");

    let audio = ui.audio("/tmp/song.m4a")?;
    assert_eq!(audio.source(), "/tmp/song.m4a");

    // 収め方は映像面へ届く。
    video.set_fit(Fit::Cover);
    Ok(())
}

fn media_starts_idle(ui: &Ui) -> Result<()> {
    let audio = ui.audio("")?;
    assert_eq!(audio.state(), PlaybackState::Idle);
    assert!(!audio.is_playing());
    assert_eq!(audio.position(), 0.0);
    assert_eq!(audio.duration(), None);

    // 設定は場所を差し替えても残る。
    audio.set_volume(0.25);
    audio.set_muted(true);
    audio.set_loop(true);
    assert_eq!(audio.volume(), 0.25);
    assert!(audio.is_muted());
    assert!(audio.is_loop());

    audio.set_volume(9.0);
    assert_eq!(audio.volume(), 1.0, "音量は 0..1 に収まる");
    Ok(())
}

// ------------------------------------------------- ファイル選択とダイアログ

fn file_picker_configuration(ui: &Ui) -> Result<()> {
    let picker = ui.file_picker("選ぶ")?;
    assert_eq!(picker.mode(), FilePickerMode::File);
    assert!(picker.selection().is_empty());

    picker.set_mode(FilePickerMode::Folder);
    assert_eq!(picker.mode(), FilePickerMode::Folder);
    picker.set_filters(&[FileFilter::new("画像", ["png", "jpg"])]);

    let native: gtk::Button = picker.native_widget().downcast().expect("GtkButton");
    assert_eq!(native.label().map(|l| l.to_string()), Some("選ぶ".into()));
    picker.set_text("フォルダーを選ぶ");
    assert_eq!(
        native.label().map(|l| l.to_string()),
        Some("フォルダーを選ぶ".into())
    );

    picker.set_enabled(false);
    assert!(!native.is_sensitive());
    Ok(())
}

fn dialog_configuration_reaches_the_alert(ui: &Ui) -> Result<()> {
    let window = ui.window("naui", 320.0, 200.0)?;
    window.show();

    let dialog = ui.dialog("保存しますか")?;
    dialog.set_message("変更が残っています");
    dialog.set_buttons(
        DialogButtons::new()
            .primary("保存")
            .secondary("保存しない")
            .cancel("キャンセル"),
    );
    dialog.set_child(&ui.checkbox("次回から確認しない")?);
    assert!(!dialog.is_open());

    dialog.open();
    assert!(dialog.is_open());

    let native = dialog.native_dialog().expect("AdwAlertDialog");
    assert_eq!(
        native.heading().map(|h| h.to_string()),
        Some("保存しますか".into())
    );
    assert_eq!(native.body().to_string(), "変更が残っています");
    for id in ["primary", "secondary", "cancel"] {
        assert!(native.has_response(id), "{id} のボタンが出る");
    }
    // GNOME の並びでは、主となる操作が既定のボタン。
    assert_eq!(
        native.default_response().map(|r| r.to_string()),
        Some("primary".into())
    );
    assert!(native.extra_child().is_some(), "中身のウィジェットが載る");

    dialog.close();
    assert!(!dialog.is_open());
    window.close();
    Ok(())
}

fn dialog_without_buttons_shows_ok(ui: &Ui) -> Result<()> {
    let window = ui.window("naui", 320.0, 200.0)?;
    window.show();

    let dialog = ui.dialog("おしらせ")?;
    assert!(dialog.buttons().is_empty());
    dialog.open();

    let native = dialog.native_dialog().expect("AdwAlertDialog");
    assert!(native.has_response("cancel"), "閉じるための OK が出る");
    assert!(!native.has_response("primary"));
    assert!(!native.has_response("secondary"));

    dialog.close();
    assert!(!dialog.is_open());
    window.close();
    Ok(())
}

fn dialog_response_reaches_the_closure(ui: &Ui) -> Result<()> {
    let window = ui.window("naui", 320.0, 200.0)?;
    window.show();

    // 主となる操作のボタンを押したときの経路。
    let confirm = ui.dialog("消しますか")?;
    confirm.set_buttons(DialogButtons::new().primary("消す").cancel("やめる"));
    let (log, sink) = recorder::<DialogResponse>();
    confirm.on_response(sink);
    confirm.open();
    let native = confirm.native_dialog().expect("AdwAlertDialog");
    native.emit_by_name::<()>("response", &[&"primary"]);
    assert_eq!(log.borrow().as_slice(), [DialogResponse::Primary]);
    assert!(!confirm.is_open());
    // 閉じたあとに来る応答は捨てられ、二重には届かない。
    native.force_close();
    assert_eq!(log.borrow().len(), 1);

    // Esc や外側を押して閉じたときは取り消し扱いになる。
    let notice = ui.dialog("おしらせ")?;
    let (closed, sink) = recorder::<DialogResponse>();
    notice.on_response(sink);
    notice.open();
    notice.close();
    assert_eq!(closed.borrow().as_slice(), [DialogResponse::Cancel]);
    assert!(!notice.is_open());

    window.close();
    Ok(())
}

fn dialog_is_closed_until_opened(ui: &Ui) -> Result<()> {
    let dialog = ui.dialog("おしらせ")?;
    let (log, sink) = recorder::<DialogResponse>();
    dialog.on_response(sink);

    dialog.close();
    assert!(!dialog.is_open());
    assert!(log.borrow().is_empty(), "出していないので何も起きない");
    assert!(dialog.native_dialog().is_none());
    Ok(())
}

fn theme_switch(ui: &Ui) -> Result<()> {
    assert_eq!(ui.theme(), Theme::System);
    let manager = adw::StyleManager::default();

    ui.set_theme(Theme::Dark)?;
    assert_eq!(ui.theme(), Theme::Dark);
    assert_eq!(manager.color_scheme(), adw::ColorScheme::ForceDark);

    ui.set_theme(Theme::Light)?;
    assert_eq!(manager.color_scheme(), adw::ColorScheme::ForceLight);

    ui.set_theme(Theme::System)?;
    assert_eq!(manager.color_scheme(), adw::ColorScheme::Default);
    Ok(())
}
