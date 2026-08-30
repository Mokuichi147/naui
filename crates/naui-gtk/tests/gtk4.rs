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
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::task::{Context, Poll};

use adw::prelude::*;
use gtk::glib;
use gtk::pango;
use naui_core::{
    Align, Color, DatePickerMode, DateTime, DialogButtons, DialogResponse, FileFilter,
    FilePickerMode, Fit, GridCell, Length, ListItem, NavItem, Orientation, Padding, PlaybackState,
    PopupItem, Result, ScrollPolicy, SelectionMode, Sizing, SortOrder, TableColumn, TableRow,
    Theme, Time, ToolbarIcon, ToolbarItem, Track, TreeItem,
};
use naui_gtk::{run_for_test, ListRow, Ui, Widget};

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
        (
            "スイッチが切り替わり新しい値を通知する",
            toggle_switches_and_notifies,
        ),
        ("スイッチの set_on は通知しない", toggle_set_is_silent),
        (
            "色ピッカーの値がネイティブの GtkColorDialogButton と往復する",
            color_picker_round_trips,
        ),
        (
            "色ピッカーの set_value は通知しない",
            color_picker_set_is_silent,
        ),
        (
            "チェックボックスの印がラベルの字面にそろう",
            checkbox_indicator_is_aligned_to_text,
        ),
        (
            "コンボボックスの項目と選択がネイティブへ届く",
            combo_box_items_and_selection,
        ),
        (
            "コンボボックスのプログラム変更は通知しない",
            combo_box_programmatic_changes_are_silent,
        ),
        (
            "コンボボックスの通知内で操作と差し替えができる",
            combo_box_callback_is_reentrant_and_replaceable,
        ),
        (
            "自由入力コンボボックスの文字列と候補がネイティブへ届く",
            editable_combo_box_text_and_items,
        ),
        (
            "自由入力コンボボックスのプログラム変更は通知しない",
            editable_combo_box_programmatic_changes_are_silent,
        ),
        (
            "自由入力コンボボックスの通知内で操作と差し替えができる",
            editable_combo_box_callback_is_reentrant_and_replaceable,
        ),
        (
            "ラジオグループの項目と選択がネイティブへ届く",
            radio_group_items_and_selection,
        ),
        (
            "ラジオグループのクリックが 1 つだけ点けて通知する",
            radio_group_click_selects_one,
        ),
        (
            "ラジオグループのプログラム変更は通知しない",
            radio_group_programmatic_changes_are_silent,
        ),
        (
            "日付ピッカーが種別に応じたコントロールだけを並べる",
            date_picker_shows_only_what_its_mode_needs,
        ),
        (
            "日付ピッカーの値がネイティブと往復する",
            date_picker_value_round_trips,
        ),
        (
            "日付ピッカーが選ばせていない部分を保つ",
            date_picker_keeps_the_part_it_does_not_show,
        ),
        (
            "日付ピッカーが範囲の外へ出さない",
            date_picker_stays_inside_the_range,
        ),
        (
            "日付ピッカーのプログラム変更は通知しない",
            date_picker_programmatic_changes_are_silent,
        ),
        (
            "時刻ピッカーが時と分のスピンボタンを並べ、値が往復する",
            time_picker_value_round_trips,
        ),
        (
            "時刻ピッカーが範囲の外へ出さない",
            time_picker_stays_inside_the_range,
        ),
        (
            "数値入力の値がネイティブと往復する",
            number_input_value_round_trips,
        ),
        (
            "数値入力が小数桁と刻みと範囲を GtkSpinButton へ渡す",
            number_input_applies_the_spec,
        ),
        (
            "数値入力の打鍵が通知され、確定で値へそろう",
            number_input_notifies_on_typing,
        ),
        (
            "数値入力が指定された幅に収まり、それ以下には潰れない",
            number_input_keeps_its_buttons_inside,
        ),
        (
            "パスワード入力が伏せ字の欄として往復する",
            password_input_round_trips,
        ),
        (
            "検索入力が GtkSearchEntry として往復する",
            search_input_round_trips,
        ),
        (
            "検索入力が Enter のときだけ確定を通知する",
            search_input_notifies_on_activate,
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
        (
            "Fill は中身の都合で親を押し広げない",
            fill_does_not_push_the_parent_wider,
        ),
        (
            "Fill は配られた場所からはみ出して描かない",
            fill_clips_what_it_cannot_fit,
        ),
        (
            "タブが増えても最小幅が増えない",
            tabs_do_not_widen_with_the_number_of_tabs,
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
        (
            "折りたたみが中身を持ち、開閉を通知する",
            expander_keeps_child_and_notifies,
        ),
        (
            "折りたたみの set_expanded は通知しない",
            expander_set_is_silent,
        ),
        (
            "ラベルは頼まれたときだけ折り返す",
            label_wraps_only_when_asked,
        ),
        (
            "折り返す中身は幅から高さが決まると親へ伝わる",
            wrapping_child_is_measured_by_width,
        ),
        (
            "分割ビューがネイティブの GtkPaned に 2 区画を並べる",
            split_view_arranges_two_panes_in_a_gtk_paned,
        ),
        (
            "分割ビューの set_position は通知せず drag_to は通知する",
            split_view_set_is_silent_and_drag_notifies,
        ),
        (
            "分割ビューの最小の大きさが区画と位置に効く",
            split_view_minimums_reach_the_panes,
        ),
        ("ウィンドウを設定して閉じられる", window_lifecycle),
        ("ウィンドウにヘッダーバーが付く", window_has_a_header_bar),
        ("ナビバーの選択がネイティブと往復する", navbar_selection),
        (
            "ナビバーの set_selected は通知しない",
            navbar_set_selected_is_silent,
        ),
        ("ドックが等幅の項目を持つ", dock_items_are_homogeneous),
        (
            "ツールバーが項目と区切りを GtkBox に並べる",
            toolbar_items_map_to_native,
        ),
        (
            "ツールバーの実行がクロージャへ届く",
            toolbar_activation_notifies,
        ),
        (
            "ツールバーがヘッダーバーへ入り外れる",
            toolbar_attaches_to_the_header_bar,
        ),
        (
            "すべてのアイコンがテーマに実在する",
            toolbar_icons_exist_in_the_theme,
        ),
        (
            "ツールバーの通知内で組み替えと差し替えができる",
            toolbar_callback_is_reentrant_and_replaceable,
        ),
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
            "Auto のリスト高が行数に追従する",
            list_auto_height_follows_rows,
        ),
        (
            "GtkListBox 側の選択がクロージャへ届く",
            list_native_selection_notifies,
        ),
        ("リストの複数選択が 0 件にもなる", list_multiple_selection),
        (
            "任意内容の行が GtkListBoxRow に載る",
            list_accepts_composed_rows,
        ),
        (
            "行のクリックがクロージャへ届く",
            list_row_activation_notifies,
        ),
        (
            "複数選択でクリックの修飾キーが GTK4 へ届く",
            list_multiple_selection_reaches_the_modifier_keys,
        ),
        ("リストが選べない行を飛ばす", list_skips_disabled_rows),
        (
            "リストの通知の中からリストを操作できる",
            list_callback_can_touch_the_list,
        ),
        ("テーブルの行が GtkListBox に並ぶ", table_rows_are_native),
        (
            "テーブルの列の幅と揃えが指定どおりになる",
            table_columns_follow_the_spec,
        ),
        (
            "GtkListBox 側のテーブルの選択がクロージャへ届く",
            table_native_selection_notifies,
        ),
        ("テーブルが選べない行を飛ばす", table_skips_disabled_rows),
        (
            "テーブルの列を差し替えても行が残る",
            table_keeps_rows_when_columns_change,
        ),
        (
            "見出しの並べ替えがボタンとして押せる",
            table_sorting_round_trips,
        ),
        ("ツリーの行が展開に追従する", tree_rows_follow_the_expansion),
        (
            "GtkListBox 側のツリーの選択がクロージャへ届く",
            tree_native_selection_notifies,
        ),
        ("ツリーが選べない枝を飛ばす", tree_skips_disabled_branches),
        (
            "ツリーの開閉ボタンがクロージャへ届く",
            tree_expansion_notifies,
        ),
        (
            "閉じた枝の中の開閉が保たれる",
            tree_remembers_expansion_inside_a_closed_branch,
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
        ("消音を解くと音量が戻る", media_unmuting_restores_the_volume),
        (
            "ファイル選択がボタンとして構成され設定を保つ",
            file_picker_configuration,
        ),
        ("保存ボタンが設定を保つ", file_saver_configuration),
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
        (
            "トーストの設定が AdwToast へ届く",
            toast_configuration_reaches_the_native_toast,
        ),
        (
            "トーストの操作と消滅がクロージャへ届く",
            toast_events_reach_the_closures,
        ),
        (
            "新しいトーストが前のものを黙って置き換える",
            toast_replaces_the_previous_one,
        ),
        ("テーマを実行中に切り替えられる", theme_switch),
        (
            "別スレッドから送った値がラベルへ届く",
            channel_delivers_from_a_worker_thread,
        ),
        (
            "spawn した処理がメインループの上で完了する",
            spawn_runs_a_local_future,
        ),
        ("cancel した処理は走らない", cancel_stops_a_future),
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

/// 印の位置を決めるもとになった数値。
///
/// 答えはフォントの計量で変わるので、落ちたときに環境の違い (日本語を持つ
/// フォントが無い、既定のフォントが小さくて印を下げる余地が無い、など) を
/// 見分けられるようにする。
fn indicator_metrics(button: &gtk::CheckButton, indicator: &gtk::Widget) -> String {
    let Some(label) = indicator.next_sibling().and_downcast::<gtk::Label>() else {
        return "ラベルが無い".to_owned();
    };
    let layout = label.layout();
    let line_height = layout.pixel_extents().1.height();
    let baseline = layout.baseline() as f64 / pango::SCALE as f64;
    let cap = label
        .create_pango_layout(Some("H"))
        .pixel_extents()
        .0
        .height();
    let (_, indicator_nat, _, _) = indicator.measure(gtk::Orientation::Vertical, -1);
    let font = label
        .pango_context()
        .font_description()
        .map(|d| d.to_str().to_string())
        .unwrap_or_default();
    let shift = (baseline - f64::from(cap) / 2.0) - f64::from(line_height) / 2.0;
    format!(
        "font={font:?} line_height={line_height} baseline={baseline} cap={cap} \
         indicator_nat={indicator_nat} shift={shift:.2} slack={} button={:?}",
        line_height - indicator_nat,
        measure_height(button)
    )
}

/// 高さの (最小, 自然な大きさ)。
fn measure_height(widget: &impl IsA<gtk::Widget>) -> (i32, i32) {
    let (min, nat, _, _) = widget.as_ref().measure(gtk::Orientation::Vertical, -1);
    (min, nat)
}

/// 幅を決めたときの高さの (最小, 自然な大きさ)。
fn measure_height_for(widget: &impl IsA<gtk::Widget>, width: i32) -> (i32, i32) {
    let (min, nat, _, _) = widget.as_ref().measure(gtk::Orientation::Vertical, width);
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

/// スイッチは `GtkSwitch` そのもので、ラベルはとなりへ並ぶ。
fn toggle_switches_and_notifies(ui: &Ui) -> Result<()> {
    let toggle = ui.toggle("通知を受け取る")?;
    let (log, sink) = recorder::<bool>();
    toggle.on_toggle(sink);

    // 箱の中は「スイッチ、ラベル」の順。
    let native: gtk::Box = toggle.native_widget().downcast().expect("GtkBox");
    let items = children(&native);
    assert_eq!(items.len(), 2);
    assert!(
        items[0].is::<gtk::Switch>(),
        "はじめが GtkSwitch であること"
    );
    let label: gtk::Label = items[1].clone().downcast().expect("GtkLabel");
    assert_eq!(label.text().as_str(), "通知を受け取る");

    let switch = toggle.native_switch();
    assert!(!toggle.is_on(), "既定は切れていること");
    // GtkSwitch の activate は、利用者が押したときと同じ経路で切り替わる。
    switch.activate();
    assert!(toggle.is_on());
    switch.activate();
    assert!(!toggle.is_on());
    assert_eq!(log.borrow().as_slice(), [true, false]);

    // 無効にすると、スイッチも文字も同じ見た目になる。
    toggle.set_enabled(false);
    assert!(!switch.is_sensitive());
    assert!(!label.is_sensitive());
    Ok(())
}

fn toggle_set_is_silent(ui: &Ui) -> Result<()> {
    let toggle = ui.toggle("通知を受け取る")?;
    let (log, sink) = recorder::<bool>();
    toggle.on_toggle(sink);

    toggle.set_on(true);
    assert!(toggle.is_on());
    assert!(toggle.native_switch().is_active(), "ネイティブへも届くこと");
    toggle.set_on(false);
    assert!(!toggle.is_on());
    assert!(log.borrow().is_empty(), "プログラムからの変更は通知しない");
    Ok(())
}

/// 色ピッカーは `GtkColorDialogButton` そのもので、値はネイティブと往復する。
fn color_picker_round_trips(ui: &Ui) -> Result<()> {
    let picker = ui.color_picker()?;
    let (log, sink) = recorder::<Color>();
    picker.on_change(sink);

    let native: gtk::ColorDialogButton = picker
        .native_widget()
        .downcast()
        .expect("GtkColorDialogButton");
    assert_eq!(native, picker.native_button());
    assert_eq!(picker.value(), Color::BLACK, "既定は黒であること");
    let dialog = native.dialog().expect("GtkColorDialog を持つこと");
    assert!(!dialog.is_with_alpha(), "透明度は扱わないこと");

    // ダイアログで選んだのと同じ経路。GTK4 は `rgba` を書くと通知する。
    let orange = Color::rgb(0xff, 0x88, 0x00);
    native.set_rgba(&gtk::gdk::RGBA::new(1.0, 0x88 as f32 / 255.0, 0.0, 1.0));
    assert_eq!(picker.value(), orange, "ネイティブの色が読めること");
    assert_eq!(log.borrow().as_slice(), [orange]);

    // `pick` はアプリからでも利用者と同じく 1 回だけ通知する。
    let teal = Color::rgb(0x00, 0x80, 0x80);
    picker.pick(teal);
    assert_eq!(picker.value(), teal);
    assert_eq!(log.borrow().as_slice(), [orange, teal]);

    picker.set_enabled(false);
    assert!(!native.is_sensitive());
    Ok(())
}

fn color_picker_set_is_silent(ui: &Ui) -> Result<()> {
    let picker = ui.color_picker()?;
    let (log, sink) = recorder::<Color>();
    picker.on_change(sink);

    let blue = Color::rgb(0x33, 0x66, 0xff);
    picker.set_value(blue);
    assert_eq!(picker.value(), blue);
    assert_eq!(
        Color::from_unit(
            f64::from(picker.native_button().rgba().red()),
            f64::from(picker.native_button().rgba().green()),
            f64::from(picker.native_button().rgba().blue()),
        ),
        blue,
        "ネイティブへも届くこと"
    );
    assert!(log.borrow().is_empty(), "プログラムからの変更は通知しない");
    Ok(())
}

/// GTK4 は印をラベルの「行の箱」の中心へ置くが、日本語の行は ascent が
/// 大きく取られるぶん字面が下に寄る。naui は印へ上マージンを足して字面の
/// 中心へそろえ直す。マージンは行の箱に収まる範囲までなので、そろえても
/// チェックボックスの高さは変わらない。
fn checkbox_indicator_is_aligned_to_text(ui: &Ui) -> Result<()> {
    let checkbox = ui.checkbox("項目を有効にする")?;
    let native: gtk::CheckButton = checkbox.native_widget().downcast().expect("GtkCheckButton");
    let indicator = native.first_child().expect("印のノード");

    let margin = indicator.margin_top();
    assert!(
        margin > 0,
        "日本語のラベルでは印を下げて字面へそろえる (margin_top={margin}, {})",
        indicator_metrics(&native, &indicator)
    );
    assert_eq!(
        measure_height(&native),
        measure_height(&gtk::CheckButton::with_label("項目を有効にする")),
        "そろえてもチェックボックスの高さは変わらない"
    );

    // 画面に出したときも、そろえたぶんを自分で取り消さない。`map` のたびに
    // 測り直しているので、前に足したマージンを二重に数えると 0 へ戻ってしまう。
    let window = ui.window("印の位置", 200.0, 100.0)?;
    window.set_child(&checkbox);
    window.show();
    assert_eq!(
        indicator.margin_top(),
        margin,
        "画面に出しても印の位置は変わらない"
    );
    window.close();

    // ラジオの印も同じようにそろえる。
    let radio = ui.radio_group()?;
    radio.set_items(&["標準"]);
    let button = radio.native_buttons().remove(0);
    assert_eq!(
        button.first_child().expect("印のノード").margin_top(),
        margin,
        "ラジオグループの印も同じだけ下げる"
    );
    Ok(())
}

fn combo_box_items_and_selection(ui: &Ui) -> Result<()> {
    let combo = ui.combo_box()?;
    assert!(combo.is_empty());
    assert_eq!(combo.selected(), None);

    combo.set_items(&["赤", "緑", "青"]);
    assert_eq!(combo.len(), 3);
    assert_eq!(combo.selected(), None, "項目の作り直し後も未選択");

    let native: gtk::DropDown = combo.native_widget().downcast().expect("GtkDropDown");
    assert_eq!(native.selected(), gtk::INVALID_LIST_POSITION);
    let model = native.model().expect("GtkStringList のモデル");
    let green: gtk::StringObject = model
        .item(1)
        .expect("2 番目の項目")
        .downcast()
        .expect("GtkStringObject");
    assert_eq!(green.string().as_str(), "緑");

    combo.set_selected(2);
    assert_eq!(combo.selected(), Some(2));
    assert_eq!(native.selected(), 2);
    combo.set_selected(99);
    assert_eq!(combo.selected(), Some(2), "範囲外は無視する");

    combo.clear_selection();
    assert_eq!(combo.selected(), None);
    combo.set_enabled(false);
    assert!(!native.is_sensitive());
    Ok(())
}

fn combo_box_programmatic_changes_are_silent(ui: &Ui) -> Result<()> {
    let combo = ui.combo_box()?;
    combo.set_items(&["A", "B", "C"]);
    let (log, sink) = recorder::<usize>();
    combo.on_select(sink);

    combo.set_selected(1);
    combo.clear_selection();
    combo.set_items(&["D", "E", "F"]);
    assert!(log.borrow().is_empty(), "プログラムからの変更は通知しない");

    let native: gtk::DropDown = combo.native_widget().downcast().expect("GtkDropDown");
    native.set_selected(2);
    assert_eq!(log.borrow().as_slice(), [2]);

    combo.select(0);
    combo.select(99);
    assert_eq!(log.borrow().as_slice(), [2, 0]);
    Ok(())
}

fn editable_combo_box_text_and_items(ui: &Ui) -> Result<()> {
    let combo = ui.editable_combo_box()?;
    assert!(combo.is_empty());
    assert_eq!(combo.text(), "");
    assert_eq!(combo.selected(), None);

    combo.set_items(&["赤", "緑", "青"]);
    assert_eq!(combo.len(), 3);
    assert_eq!(combo.text(), "", "候補を入れても文字列は変わらない");

    let entry = combo.native_entry();
    let list = combo.native_list();
    assert_eq!(list.row_at_index(3), None, "候補は 3 行");
    let row = list.row_at_index(1).expect("2 番目の候補");
    let label: gtk::Label = row.child().expect("行の中身").downcast().expect("GtkLabel");
    assert_eq!(label.text().as_str(), "緑");

    combo.set_text("桃");
    assert_eq!(combo.text(), "桃");
    assert_eq!(entry.text().as_str(), "桃");
    assert_eq!(combo.selected(), None, "候補に無い文字列も持てる");

    combo.set_selected(1);
    assert_eq!(combo.text(), "緑");
    assert_eq!(combo.selected(), Some(1));
    combo.set_selected(99);
    assert_eq!(combo.text(), "緑", "範囲外は無視する");

    combo.clear();
    assert_eq!(combo.text(), "");

    combo.set_placeholder("色");
    assert_eq!(
        entry.placeholder_text().map(|text| text.to_string()),
        Some("色".to_string())
    );

    combo.set_enabled(false);
    assert!(!combo.native_widget().is_sensitive());
    Ok(())
}

fn editable_combo_box_programmatic_changes_are_silent(ui: &Ui) -> Result<()> {
    let combo = ui.editable_combo_box()?;
    combo.set_items(&["A", "B", "C"]);
    let log = Rc::new(RefCell::new(Vec::new()));
    combo.on_change({
        let log = log.clone();
        move |text: &str| log.borrow_mut().push(text.to_string())
    });

    combo.set_text("Z");
    combo.set_selected(1);
    combo.clear();
    combo.set_items(&["D", "E", "F"]);
    assert!(log.borrow().is_empty(), "プログラムからの変更は通知しない");

    // 打ち込んだときと同じ経路 (`GtkEntry` の `changed`)。
    combo.native_entry().set_text("Z");
    assert_eq!(log.borrow().as_slice(), ["Z"]);

    // 候補の行を押したときの経路。
    let list = combo.native_list();
    let row = list.row_at_index(0).expect("先頭の候補");
    list.emit_by_name::<()>("row-activated", &[&row]);
    assert_eq!(combo.text(), "D", "押した候補が欄へ入る");
    assert_eq!(log.borrow().as_slice(), ["Z", "D"], "通知は 1 回だけ");

    combo.select(2);
    combo.select(99);
    assert_eq!(
        log.borrow().as_slice(),
        ["Z", "D", "F"],
        "範囲外は通知しない"
    );
    Ok(())
}

fn editable_combo_box_callback_is_reentrant_and_replaceable(ui: &Ui) -> Result<()> {
    let combo = ui.editable_combo_box()?;
    combo.set_items(&["A", "B", "C"]);
    let first = Rc::new(RefCell::new(Vec::new()));
    let second = Rc::new(RefCell::new(Vec::new()));
    combo.on_change({
        let combo = combo.clone();
        let first = first.clone();
        let second = second.clone();
        move |text: &str| {
            first.borrow_mut().push(text.to_string());
            // 通知中の操作でも二重借用せず、この変更自体は通知しない。
            combo.set_items(&["D", "E"]);
            combo.set_selected(1);
            combo.on_change({
                let second = second.clone();
                move |text: &str| second.borrow_mut().push(text.to_string())
            });
        }
    });

    combo.select(1);
    assert_eq!(first.borrow().as_slice(), ["B"]);
    assert_eq!(combo.text(), "E");

    combo.select(0);
    assert_eq!(first.borrow().len(), 1, "古い通知先は外れること");
    assert_eq!(second.borrow().as_slice(), ["D"]);
    Ok(())
}

fn combo_box_callback_is_reentrant_and_replaceable(ui: &Ui) -> Result<()> {
    let combo = ui.combo_box()?;
    combo.set_items(&["A", "B", "C"]);
    let first = Rc::new(Cell::new(None));
    let second = Rc::new(RefCell::new(Vec::new()));
    combo.on_select({
        let combo = combo.clone();
        let first = first.clone();
        let second = second.clone();
        move |index| {
            first.set(Some(index));
            // 通知中の操作でも二重借用せず、この変更自体は通知しない。
            combo.set_selected(0);
            combo.on_select({
                let second = second.clone();
                move |index| second.borrow_mut().push(index)
            });
        }
    });

    combo.select(1);
    assert_eq!(first.get(), Some(1));
    assert_eq!(combo.selected(), Some(0));
    combo.select(2);
    assert_eq!(second.borrow().as_slice(), [2]);
    Ok(())
}

fn radio_group_items_and_selection(ui: &Ui) -> Result<()> {
    let radio = ui.radio_group()?;
    assert!(radio.is_empty());
    assert_eq!(radio.selected(), None);

    radio.set_items(&["小", "中", "大"]);
    assert_eq!(radio.len(), 3);
    assert_eq!(radio.selected(), None, "項目の作り直し後も未選択");

    let buttons = radio.native_buttons();
    assert_eq!(buttons.len(), 3);
    assert_eq!(
        buttons[1].label().map(|s| s.to_string()).as_deref(),
        Some("中")
    );
    assert!(buttons.iter().all(|button| !button.is_active()));

    radio.set_selected(2);
    assert_eq!(radio.selected(), Some(2));
    assert!(buttons[2].is_active());
    radio.set_selected(99);
    assert_eq!(radio.selected(), Some(2), "範囲外は無視する");

    radio.clear_selection();
    assert_eq!(radio.selected(), None);
    assert!(buttons.iter().all(|button| !button.is_active()));

    radio.set_enabled(false);
    let native: gtk::Box = radio.native_widget().downcast().expect("GtkBox");
    assert!(!native.is_sensitive());

    radio.set_orientation(Orientation::Horizontal);
    assert_eq!(native.orientation(), gtk::Orientation::Horizontal);
    Ok(())
}

/// ネイティブのクリック経路でも、点くのは常に 1 つだけ。
fn radio_group_click_selects_one(ui: &Ui) -> Result<()> {
    let radio = ui.radio_group()?;
    radio.set_items(&["赤", "緑", "青"]);
    let (log, sink) = recorder::<usize>();
    radio.on_select(sink);

    let buttons = radio.native_buttons();
    buttons[2].activate();
    assert_eq!(radio.selected(), Some(2));
    assert_eq!(log.borrow().as_slice(), [2]);

    buttons[0].activate();
    assert_eq!(radio.selected(), Some(0), "選び直すと前のものは消える");
    // 外れた側の `toggled` は通知しない。
    assert_eq!(log.borrow().as_slice(), [2, 0]);
    assert_eq!(
        buttons.iter().filter(|button| button.is_active()).count(),
        1,
        "点いているのは常に 1 つ"
    );
    Ok(())
}

fn radio_group_programmatic_changes_are_silent(ui: &Ui) -> Result<()> {
    let radio = ui.radio_group()?;
    radio.set_items(&["A", "B", "C"]);
    let (log, sink) = recorder::<usize>();
    radio.on_select(sink);

    radio.set_selected(1);
    radio.set_selected(2);
    radio.clear_selection();
    radio.set_items(&["D", "E", "F"]);
    assert!(log.borrow().is_empty(), "プログラムからの変更は通知しない");

    radio.select(0);
    radio.select(99);
    assert_eq!(log.borrow().as_slice(), [0], "範囲外は通知もしない");

    // 通知の中で同じグループを触っても二重借用にならない。
    let seen = Rc::new(Cell::new(None));
    radio.on_select({
        let radio = radio.clone();
        let seen = seen.clone();
        move |index| {
            seen.set(Some(index));
            radio.set_selected(2);
        }
    });
    radio.select(1);
    assert_eq!(seen.get(), Some(1));
    assert_eq!(radio.selected(), Some(2));
    Ok(())
}

/// 数値入力は `GtkSpinButton` と値をやり取りし、操作を通知する。
fn number_input_value_round_trips(ui: &Ui) -> Result<()> {
    let number = ui.number_input(3.0)?;
    assert_eq!(number.value(), 3.0);
    let native: gtk::SpinButton = number.native_widget().downcast().expect("GtkSpinButton");
    assert_eq!(native.value(), 3.0);
    assert!(native.is_numeric(), "数字以外は受け付けないこと");

    let (log, sink) = recorder::<f64>();
    number.on_change(sink);

    // 既定は整数なので、小数は丸めて入る。
    number.set_value(2.4);
    assert_eq!(number.value(), 2.0);
    assert_eq!(native.value(), 2.0);
    assert!(log.borrow().is_empty(), "set_value は通知しない");

    // 上下のボタンと同じく、ネイティブ側で値を動かす。
    native.set_value(5.0);
    assert_eq!(number.value(), 5.0);
    assert_eq!(log.borrow().as_slice(), [5.0]);

    // 値が動かなければ知らせない。
    native.set_value(5.0);
    assert_eq!(log.borrow().len(), 1);

    number.set_enabled(false);
    assert!(!native.is_sensitive());
    Ok(())
}

/// 小数桁・刻み・範囲は `GtkSpinButton` にも naui の値にも効く。
fn number_input_applies_the_spec(ui: &Ui) -> Result<()> {
    let number = ui.number_input(0.0)?;
    number.set_decimals(2);
    number.set_step(0.5);
    number.set_range(Some(0.0), Some(10.0));

    let native: gtk::SpinButton = number.native_widget().downcast().expect("GtkSpinButton");
    assert_eq!(native.digits(), 2);
    assert_eq!(native.increments(), (0.5, 5.0));
    assert_eq!(native.range(), (0.0, 10.0));

    number.set_value(1.239);
    assert_eq!(number.value(), 1.24, "小数桁へ丸める");

    number.set_value(12.345);
    assert_eq!(number.value(), 10.0, "上限で止まる");
    number.set_value(-1.0);
    assert_eq!(number.value(), 0.0, "下限で止まる");

    // 範囲を外すと自由に入る。
    number.set_range(None, None);
    number.set_value(-30.5);
    assert_eq!(number.value(), -30.5);
    assert_eq!(number.spec().max, None);
    Ok(())
}

/// 打鍵の時点で読める値は通知し、確定 (`update`) で表示と値がそろう。
fn number_input_notifies_on_typing(ui: &Ui) -> Result<()> {
    let number = ui.number_input(0.0)?;
    number.set_range(None, Some(100.0));
    let native: gtk::SpinButton = number.native_widget().downcast().expect("GtkSpinButton");

    let (log, sink) = recorder::<f64>();
    number.on_change(sink);

    // 利用者の打鍵と同じく、欄の文字を差し替える。
    native.set_text("12");
    assert_eq!(number.value(), 12.0);
    assert_eq!(log.borrow().as_slice(), [12.0]);

    // 確定しても、同じ値なら重ねて通知しない。
    native.update();
    assert_eq!(number.value(), 12.0);
    assert_eq!(log.borrow().len(), 1);

    // 範囲の外は端へ寄る。
    native.set_text("999");
    assert_eq!(number.value(), 100.0);
    assert_eq!(log.borrow().as_slice(), [12.0, 100.0]);

    // 数として読めないものは放っておく。
    native.set_text("");
    assert_eq!(number.value(), 100.0);
    assert_eq!(log.borrow().len(), 2);
    Ok(())
}

/// 数値入力は、指定された幅に収まり、欄とボタンの幅より下へは潰れない。
fn number_input_keeps_its_buttons_inside(ui: &Ui) -> Result<()> {
    /// 数値の欄によくある指定 (論理ピクセル)。
    const GIVEN: i32 = 140;

    let number = ui.number_input(120.0)?;
    number.set_decimals(2);
    // 上限なしは `GtkAdjustment` では 16 桁の範囲になる。`GtkSpinButton` は
    // 範囲に入る数の桁数から幅を決めるので、そのままだと桁数ぶんを欲しがる。
    number.set_range(Some(0.0), None);
    let native: gtk::SpinButton = number.native_widget().downcast().expect("GtkSpinButton");

    let (content, _) = measure_width(&native);
    assert!(
        content <= GIVEN,
        "範囲の桁数で幅が決まっていない ({content}px)"
    );

    let bin = bin_of(&number);
    number.set_sizing(Sizing::new().width(Length::Fixed(f64::from(GIVEN))));
    let (min, nat) = measure_width(&bin);
    assert_eq!((min, nat), (GIVEN, GIVEN), "指定した幅そのものになる");

    // 中身の最小より狭い指定は、中身に合わせて押し返す。狭いまま配ると、
    // `GtkSpinButton` は縮まずに上下のボタンを枠の外へ描いてしまう。
    number.set_sizing(Sizing::new().width(Length::Fixed(40.0)));
    let (min, nat) = measure_width(&bin);
    assert_eq!((min, nat), (content, content), "欄とボタンのぶんは残る");

    // 上限付きの `Fill` も同じ。
    number.set_sizing(Sizing::new().width(Length::Fill).max_width(40.0));
    let (min, nat) = measure_width(&bin);
    assert!(
        min >= content && nat >= content,
        "上限で潰されない (最小 {min}px, 自然 {nat}px)"
    );
    Ok(())
}

/// 検索入力は `GtkSearchEntry` そのもので、文字列は往復する。
fn search_input_round_trips(ui: &Ui) -> Result<()> {
    let search = ui.search_input()?;
    let native: gtk::SearchEntry = search.native_widget().downcast().expect("GtkSearchEntry");

    let (log, sink) = recorder::<String>();
    search.on_change({
        let mut sink = sink;
        move |text: &str| sink(text.to_string())
    });

    search.set_text("なう");
    assert_eq!(search.text(), "なう");
    assert_eq!(native.text().as_str(), "なう");
    assert!(log.borrow().is_empty(), "set_text は通知しない");

    search.set_placeholder("検索");
    assert_eq!(
        native.placeholder_text().map(|t| t.to_string()),
        Some("検索".to_string())
    );

    // 利用者の打鍵と同じく、末尾へ差し込む (位置は文字数で数える)。
    let mut position = native.text().chars().count() as i32;
    native.insert_text("い", &mut position);
    assert_eq!(search.text(), "なうい");
    assert_eq!(log.borrow().as_slice(), ["なうい"]);

    search.set_enabled(false);
    assert!(!native.is_sensitive());
    Ok(())
}

/// 確定の通知は `activate` (Enter) のときだけで、打っている間は飛ばない。
fn search_input_notifies_on_activate(ui: &Ui) -> Result<()> {
    let search = ui.search_input()?;
    let native: gtk::SearchEntry = search.native_widget().downcast().expect("GtkSearchEntry");

    let (typed, typed_sink) = recorder::<String>();
    let (submitted, submitted_sink) = recorder::<String>();
    search.on_change({
        let mut sink = typed_sink;
        move |text: &str| sink(text.to_string())
    });
    search.on_search({
        let mut sink = submitted_sink;
        move |text: &str| sink(text.to_string())
    });

    // 利用者の打鍵と同じく、1 文字ずつ末尾へ差し込む。
    let mut position = 0;
    native.insert_text("ね", &mut position);
    native.insert_text("こ", &mut position);
    assert_eq!(typed.borrow().as_slice(), ["ね", "ねこ"]);
    assert!(submitted.borrow().is_empty(), "打つだけでは確定しない");

    native.emit_activate();
    assert_eq!(submitted.borrow().as_slice(), ["ねこ"]);
    assert_eq!(typed.borrow().len(), 2, "確定は打鍵の通知を増やさない");

    // 確定は何度でも起きる。
    search.set_text("いぬ");
    native.emit_activate();
    assert_eq!(submitted.borrow().as_slice(), ["ねこ", "いぬ"]);
    Ok(())
}

/// パスワード入力は `GtkPasswordEntry` そのもので、文字列は往復する。
fn password_input_round_trips(ui: &Ui) -> Result<()> {
    let password = ui.password_input()?;
    let native: gtk::PasswordEntry = password
        .native_widget()
        .downcast()
        .expect("GtkPasswordEntry");

    let (log, sink) = recorder::<String>();
    password.on_change({
        let mut sink = sink;
        move |text: &str| sink(text.to_string())
    });

    password.set_text("ひみつ");
    assert_eq!(password.text(), "ひみつ");
    assert_eq!(native.text().as_str(), "ひみつ");
    assert!(log.borrow().is_empty(), "set_text は通知しない");

    password.set_placeholder("パスワード");
    assert_eq!(
        native.placeholder_text().map(|t| t.to_string()),
        Some("パスワード".to_string())
    );

    // 利用者の打鍵と同じく、末尾へ差し込む。
    // 差し込む位置は文字数で数える (バイト数ではない)。
    let mut position = native.text().chars().count() as i32;
    native.insert_text("！", &mut position);
    assert_eq!(password.text(), "ひみつ！");
    assert_eq!(log.borrow().as_slice(), ["ひみつ！"]);

    password.set_enabled(false);
    assert!(!native.is_sensitive());
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

fn fill_does_not_push_the_parent_wider(ui: &Ui) -> Result<()> {
    const TEXT: &str = "とても長い文字列がここに入っていて自然な幅は大きい";
    let plain = ui.label(TEXT)?;
    let (minimum, natural) = measure_width(&bin_of(&plain));
    assert!(minimum > 0, "指定が無ければ中身の最小をそのまま申告する");

    // `Fill` は「大きさは親が決める」という指定なので、中身の都合で
    // 親 (ひいてはウィンドウの縮められる下限) を押し広げない。
    let filling = ui.label(TEXT)?;
    filling.set_sizing(Sizing::fill_width());
    let (minimum, filled_natural) = measure_width(&bin_of(&filling));
    assert_eq!(minimum, 0, "縮めるときは中身より小さくなれる");
    assert_eq!(filled_natural, natural, "ふだんの大きさは変わらない");

    // 下限が要るときは min_width で指定する。
    let floored = ui.label(TEXT)?;
    floored.set_sizing(Sizing::fill_width().min_width(120.0));
    assert_eq!(measure_width(&bin_of(&floored)).0, 120);
    Ok(())
}

fn tabs_do_not_widen_with_the_number_of_tabs(ui: &Ui) -> Result<()> {
    const LABEL: &str = "とても長いタブの名前";

    // `GtkNotebook` の既定は「全タブが横に並ぶ幅」を最小幅にするため、
    // タブが増えるとウィンドウがそれ以下に縮められなくなる。
    let tabs = ui.tabs()?;
    let native: gtk::Notebook = tabs.native_widget().downcast().expect("GtkNotebook");
    assert!(native.is_scrollable(), "収まらないタブは矢印で送る");

    tabs.add_tab(&format!("{LABEL} 1"), &ui.label("1")?);
    let (one, _) = measure_width(&bin_of(&tabs));
    for index in 2..=8 {
        tabs.add_tab(&format!("{LABEL} {index}"), &ui.label("x")?);
    }
    let (many, _) = measure_width(&bin_of(&tabs));
    assert_eq!(tabs.len(), 8);
    assert!(
        many < one + 64,
        "増えるのは送りの矢印ぶんだけ (1 枚: {one}px, 8 枚: {many}px)"
    );

    // 送りを付けない `GtkNotebook` と比べると、差がそのまま
    // 「ウィンドウを縮められる下限」の差になる。
    let plain = gtk::Notebook::new();
    for index in 1..=8 {
        plain.append_page(
            &gtk::Label::new(Some("x")),
            Some(&gtk::Label::new(Some(&format!("{LABEL} {index}")))),
        );
    }
    let (plain_min, _, _, _) = plain.measure(gtk::Orientation::Horizontal, -1);
    assert!(
        many * 2 < plain_min,
        "送りなしなら全タブぶん ({plain_min}px) 要るところが {many}px で済む"
    );
    Ok(())
}

fn fill_clips_what_it_cannot_fit(ui: &Ui) -> Result<()> {
    let label = ui.label("あ")?;
    let bin = bin_of(&label);
    // 指定が無い間は切り取らない (影や焦点の枠を欠かないため)。
    assert_eq!(bin.overflow(), gtk::Overflow::Visible);

    // `Fill` の軸は最小を 0 として申告する以上、配られた場所からはみ出して
    // 描いてはいけない。CSS の `min-width: 0` と `overflow: hidden` の組み。
    label.set_sizing(Sizing::fill_width());
    assert_eq!(bin.overflow(), gtk::Overflow::Hidden);

    label.set_sizing(Sizing::new());
    assert_eq!(bin.overflow(), gtk::Overflow::Visible, "指定を外せば戻る");
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

fn expander_keeps_child_and_notifies(ui: &Ui) -> Result<()> {
    let expander = ui.expander("詳細設定")?;
    assert_eq!(expander.text(), "詳細設定");
    let stack = ui.stack(Orientation::Vertical)?;
    stack.append(&ui.label("中身")?);
    expander.set_child(&stack);

    let native: gtk::Expander = expander.native_widget().downcast().expect("GtkExpander");
    assert_eq!(native.child(), Some(bin_of(&stack)));
    assert!(!expander.is_expanded(), "既定は閉じていること");

    let (log, sink) = recorder::<bool>();
    expander.on_toggle(sink);

    // GtkExpander の activate は見出しを押したときと同じ経路で開閉する。
    native.activate();
    assert!(expander.is_expanded());
    native.activate();
    assert!(!expander.is_expanded());
    assert_eq!(log.borrow().as_slice(), [true, false]);

    expander.set_text("詳細");
    assert_eq!(expander.text(), "詳細");
    assert_eq!(
        native.label().map(|label| label.to_string()).as_deref(),
        Some("詳細")
    );

    // たたんでいる間、中身は場所を取らない。
    let (_, collapsed) = measure_height(&native);
    expander.set_expanded(true);
    let (_, expanded) = measure_height(&native);
    assert!(
        expanded > collapsed,
        "開くと中身のぶんだけ高くなること: {collapsed} -> {expanded}"
    );
    Ok(())
}

fn expander_set_is_silent(ui: &Ui) -> Result<()> {
    let expander = ui.expander("詳細")?;
    let (log, sink) = recorder::<bool>();
    expander.on_toggle(sink);

    expander.set_expanded(true);
    assert!(expander.is_expanded());
    expander.set_expanded(false);
    assert!(!expander.is_expanded());
    assert!(log.borrow().is_empty(), "プログラムからの変更は通知しない");
    Ok(())
}

fn label_wraps_only_when_asked(ui: &Ui) -> Result<()> {
    let text = "これはとても長い説明の文章で、狭い幅には 1 行で収まりません。";
    let label = ui.label(text)?;
    let native: gtk::Label = label.native_widget().downcast().expect("GtkLabel");

    // 既定は 1 行。入りきらない分は末尾を省略記号で切る。
    assert!(!native.wraps(), "既定は折り返さないこと");
    assert_eq!(native.ellipsize(), pango::EllipsizeMode::End);
    // 省略記号があるぶん、最小幅は文字列の幅よりずっと小さい。
    let (min, natural) = measure_width(&native);
    assert!(
        min < natural,
        "省略できる分だけ最小幅が下がること: 最小 {min} / 自然 {natural}"
    );

    label.set_wrap(true);
    assert!(native.wraps());
    assert_eq!(native.ellipsize(), pango::EllipsizeMode::None);
    assert_eq!(native.wrap_mode(), pango::WrapMode::WordChar);
    // 折り返す側は、狭い幅を渡すと高さが増える。
    let one_line = native.measure(gtk::Orientation::Vertical, -1).0;
    let narrow = native.measure(gtk::Orientation::Vertical, 120).0;
    assert!(
        narrow > one_line,
        "狭いと折り返して高くなること: {one_line} -> {narrow}"
    );

    label.set_wrap(false);
    assert!(!native.wraps(), "何度でも切り替えられること");
    assert_eq!(native.ellipsize(), pango::EllipsizeMode::End);
    Ok(())
}

/// 折り返す中身を包んだ入れ物は、「幅が決まってから高さが決まる」と申告する。
///
/// これが伝わらないと、親は幅を渡さずに高さを尋ね、**折り返す前の 1 行ぶん**
/// しか高さを配らない。狭い区画 (`SplitView` など) に入れた説明文が、折り返した
/// 分だけはみ出して重なって見えるのはこれが原因。
fn wrapping_child_is_measured_by_width(ui: &Ui) -> Result<()> {
    let text = "これはとても長い説明の文章で、狭い幅には 1 行で収まりません。";
    let label = ui.label(text)?;
    label.set_wrap(true);
    label.set_sizing(Sizing::fill_width());
    assert_eq!(
        bin_of(&label).request_mode(),
        gtk::SizeRequestMode::HeightForWidth,
        "入れ物が中身の測り方を伝えること"
    );

    let stack = ui.stack(Orientation::Vertical)?;
    stack.append(&label);
    assert_eq!(
        bin_of(&stack).request_mode(),
        gtk::SizeRequestMode::HeightForWidth,
        "並べる入れ物にも伝わること"
    );

    // 狭い幅を渡すと、折り返した分だけ高さが増える。
    let (_, wide) = measure_height_for(&bin_of(&stack), 400);
    let (_, narrow) = measure_height_for(&bin_of(&stack), 120);
    assert!(narrow > wide, "狭いほど高くなること: {wide} -> {narrow}");

    // 幅に上限があるなら、それより広い幅で尋ねられても上限の幅で折り返す。
    let capped = ui.label(text)?;
    capped.set_wrap(true);
    capped.set_sizing(Sizing::fill_width().max_width(120.0));
    let (_, asked_wide) = measure_height_for(&bin_of(&capped), 400);
    let (_, at_cap) = measure_height_for(&bin_of(&capped), 120);
    assert_eq!(
        asked_wide, at_cap,
        "上限までしか配られないので、上限の幅で測ること"
    );
    Ok(())
}

fn split_view_arranges_two_panes_in_a_gtk_paned(ui: &Ui) -> Result<()> {
    let split = ui.split_view(Orientation::Horizontal)?;
    let native = split.native_paned();
    assert_eq!(
        native.orientation(),
        gtk::Orientation::Horizontal,
        "Horizontal は区画が横に並ぶこと"
    );
    assert!(
        !native.resizes_start_child() && native.resizes_end_child(),
        "余りは end 側が受け取ること"
    );
    assert!(
        !native.shrinks_start_child() && !native.shrinks_end_child(),
        "最小より縮ませないこと"
    );

    let start = ui.label("左")?;
    let end = ui.label("右")?;
    split.set_start(&start);
    split.set_end(&end);
    assert_eq!(
        bin_of(&start).parent(),
        native.start_child(),
        "start 側は GtkPaned の始端の子に入ること"
    );
    assert_eq!(
        bin_of(&end).parent(),
        native.end_child(),
        "end 側は GtkPaned の終端の子に入ること"
    );

    assert_eq!(split.orientation(), Orientation::Horizontal);
    assert_eq!(split.position(), naui_core::DEFAULT_SPLIT_POSITION);
    assert_eq!(native.position(), naui_core::DEFAULT_SPLIT_POSITION as i32);

    // 差し替えると、前の区画は外れる。
    let other = ui.label("差し替え")?;
    split.set_start(&other);
    assert_eq!(bin_of(&other).parent(), native.start_child());
    assert!(
        start
            .native_widget()
            .ancestor(gtk::Paned::static_type())
            .is_none(),
        "前の区画は外れていること"
    );

    let vertical = ui.split_view(Orientation::Vertical)?;
    assert_eq!(
        vertical.native_paned().orientation(),
        gtk::Orientation::Vertical
    );
    Ok(())
}

fn split_view_set_is_silent_and_drag_notifies(ui: &Ui) -> Result<()> {
    let split = ui.split_view(Orientation::Horizontal)?;
    split.set_start(&ui.label("左")?);
    split.set_end(&ui.label("右")?);
    let native = split.native_paned();

    let (log, sink) = recorder::<f64>();
    split.on_resize(sink);

    split.set_position(160.0);
    assert_eq!(split.position(), 160.0);
    assert_eq!(native.position(), 160);
    assert!(log.borrow().is_empty(), "set_position は通知しないこと");

    split.drag_to(240.0);
    assert_eq!(split.position(), 240.0);
    assert_eq!(native.position(), 240);
    assert_eq!(log.borrow().as_slice(), [240.0]);

    // 利用者が仕切りを動かしたときと同じ経路 (position プロパティ) でも届く。
    native.set_position(300);
    assert_eq!(split.position(), 300.0);
    assert_eq!(log.borrow().as_slice(), [240.0, 300.0]);
    Ok(())
}

fn split_view_minimums_reach_the_panes(ui: &Ui) -> Result<()> {
    let split = ui.split_view(Orientation::Horizontal)?;
    split.set_start(&ui.label("左")?);
    split.set_end(&ui.label("右")?);
    let native = split.native_paned();

    let (log, sink) = recorder::<f64>();
    split.on_resize(sink);
    split.set_min_sizes(120.0, 90.0);

    let start_child = native.start_child().expect("始端の子があること");
    let end_child = native.end_child().expect("終端の子があること");
    assert_eq!(
        start_child.size_request(),
        (120, -1),
        "start 側の最小が区画の入れ物へ届くこと"
    );
    assert_eq!(end_child.size_request(), (90, -1));
    assert!(
        measure_width(&start_child).0 >= 120,
        "区画が最小より小さいと申告しないこと"
    );

    // 既定の 200 は範囲の中なので動かない。
    assert_eq!(split.position(), naui_core::DEFAULT_SPLIT_POSITION);
    split.set_position(10.0);
    assert_eq!(
        split.position(),
        120.0,
        "start 側の最小より内側へは行かない"
    );
    assert!(log.borrow().is_empty(), "押し戻しは通知しないこと");
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
    // ヘッダーバーの下に中身が入る。
    let toolbar: adw::ToolbarView = native
        .content()
        .expect("中身")
        .downcast()
        .expect("AdwToolbarView");
    // 中身はそのままではなく、トーストを重ねる入れ物ごしに入る。
    let overlay: adw::ToastOverlay = toolbar
        .content()
        .expect("トーストの入れ物")
        .downcast()
        .expect("AdwToastOverlay");
    assert_eq!(overlay, window.native_toast_overlay());
    assert_eq!(overlay.child(), Some(bin_of(&stack)));
    assert_eq!(
        toolbar.overflow(),
        gtk::Overflow::Hidden,
        "窓より中身が大きくても、ウィンドウの外へは描かない"
    );
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

fn window_has_a_header_bar(ui: &Ui) -> Result<()> {
    // `AdwApplicationWindow` は既定のタイトルバーを持たないので、
    // 最小化・最大化・閉じるのボタンはヘッダーバーが出す。
    let window = ui.window("naui", 320.0, 200.0)?;
    let header = window.native_header_bar();
    assert!(
        header.shows_start_title_buttons() && header.shows_end_title_buttons(),
        "最小化・最大化・閉じるのボタンが出る (どちら側に出るかはデスクトップの設定次第)"
    );
    assert_eq!(
        header.root().map(|r| r.upcast::<gtk::Widget>()),
        Some(window.native_window().upcast())
    );

    // 中身を入れても、ヘッダーバーは置き換わらない。
    window.set_child(&ui.label("0")?);
    assert!(header.root().is_some(), "ヘッダーバーはウィンドウに残る");
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

fn list_auto_height_follows_rows(ui: &Ui) -> Result<()> {
    let list = ui.list()?;
    list.set_sizing(Sizing::fill_width());
    list.set_items(&[ListItem::new("東京").detail("13,960,000 人")]);
    let one_row = measure_height(&bin_of(&list)).1;

    list.set_items(&[
        ListItem::new("東京").detail("13,960,000 人"),
        ListItem::new("大阪").detail("8,838,000 人"),
        ListItem::new("札幌").detail("1,973,000 人"),
    ]);
    let three_rows = measure_height(&bin_of(&list)).1;
    assert!(one_row > 0, "1 行でも自然な高さを持つこと");
    assert!(
        three_rows > one_row,
        "Auto の高さが行数に追従すること: 1 行 {one_row} / 3 行 {three_rows}"
    );
    Ok(())
}

/// 設定画面向けの行は、ラベルだけでなく任意のレイアウトとコントロールを持てる。
fn list_accepts_composed_rows(ui: &Ui) -> Result<()> {
    let content = ui.stack(Orientation::Horizontal)?;
    content.set_spacing(8.0);
    let check = ui.checkbox("")?;
    let title = ui.label("Wi-Fi")?;
    let action = ui.button("設定…")?;
    content.append(&check);
    content.append(&title);
    content.append(&action);
    content.set_sizing(Sizing::fill_width());

    let list = ui.list()?;
    list.set_rows(&[ListRow::new(&content).selectable(false)]);
    assert_eq!(list.len(), 1);

    let native = list_box_of(&list);
    let rows = children(&native);
    assert_eq!(rows.len(), 1);
    let row = rows[0]
        .clone()
        .downcast::<gtk::ListBoxRow>()
        .expect("GtkListBoxRow");
    assert!(
        content.native_widget().is_ancestor(&row),
        "組み立てた中身がそのまま行に載ること"
    );
    assert!(row.is_sensitive(), "行の中のコントロールは押せること");
    assert!(!row.is_selectable(), "行そのものは選べないこと");

    // 選べない行は、GtkListBox 側から選ぼうとしても選択されない。
    native.select_row(Some(&row));
    assert!(list.selection().is_empty());
    list.set_selection(&[0]);
    assert!(list.selection().is_empty());

    // 文字だけの行へ戻すと、こちらは選べる。
    list.set_items(&ListItem::list(["札幌", "東京"]));
    let rows = children(&list_box_of(&list));
    assert_eq!(rows.len(), 2);
    let row = rows[0]
        .clone()
        .downcast::<gtk::ListBoxRow>()
        .expect("GtkListBoxRow");
    assert!(row.is_selectable());
    list.set_selection(&[0]);
    assert_eq!(list.selection(), vec![0]);
    Ok(())
}

/// 行そのものが押されたことがクロージャへ届く。
///
/// 行内のボタンやチェックボックスは自分でクリックを受け取るので、
/// GTK 側でも行の activation とは二重に発火しない。
fn list_row_activation_notifies(ui: &Ui) -> Result<()> {
    let content = ui.stack(Orientation::Horizontal)?;
    let check = ui.checkbox("")?;
    content.append(&check);
    content.append(&ui.label("Wi-Fi")?);
    content.set_sizing(Sizing::fill_width());

    let (log, sink) = recorder::<bool>();
    let row = ListRow::new(&content).selectable(false);
    row.on_activate({
        let check = check.clone();
        let mut sink = sink;
        move || {
            check.set_checked(!check.is_checked());
            sink(check.is_checked());
        }
    });

    // 文字だけの行にも同じように付けられる。
    let text_row = ListRow::new(&ui.label("そのほか")?);
    let (text_log, text_sink) = recorder::<()>();
    text_row.on_activate({
        let mut sink = text_sink;
        move || sink(())
    });

    let list = ui.list()?;
    list.set_rows(&[row, text_row]);

    let native = list_box_of(&list);
    let first = native.row_at_index(0).expect("1 行目");
    assert!(
        first.is_activatable(),
        "選べない行でもクリックは受け取れること"
    );
    first.emit_activate();
    assert_eq!(log.borrow().as_slice(), [true]);
    assert!(check.is_checked(), "行のクリックからチェックが変わること");

    // 2 回目も同じクロージャが呼ばれる (取り出したまま失われない)。
    first.emit_activate();
    assert_eq!(log.borrow().as_slice(), [true, false]);

    let second = native.row_at_index(1).expect("2 行目");
    second.emit_activate();
    assert_eq!(text_log.borrow().len(), 1);
    assert_eq!(log.borrow().len(), 2, "他の行は呼ばれないこと");

    // 使えない行は押せない。
    list.set_items(&[ListItem::new("停止中").enabled(false)]);
    let disabled = list_box_of(&list).row_at_index(0).expect("1 行目");
    assert!(!disabled.is_sensitive());
    assert!(!disabled.is_activatable());
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

fn list_multiple_selection_reaches_the_modifier_keys(ui: &Ui) -> Result<()> {
    // `GtkListBox` は「1 クリックで確定」(既定) の間、クリックに付いている
    // Ctrl / Shift を読まず、必ず「その行だけを選ぶ」に倒す。複数選択で
    // これを切らないと、行を足すことも外すこともできない。
    //
    // 修飾キーを見た先のふるまい (足す / 外す / 範囲) は GTK4 のもので、
    // naui はそこへ届かせているだけなので、ここでは切り替えの有無を見る。
    let list = ui.list()?;
    list.set_items(&ListItem::list(["a", "b", "c"]));
    let native = list_box_of(&list);

    // 単一選択は「1 クリックで確定」のまま (`<select>` と同じ)。
    assert!(native.activates_on_single_click());

    list.set_selection_mode(SelectionMode::Multiple);
    assert!(
        !native.activates_on_single_click(),
        "複数選択では、クリックの Ctrl / Shift を GTK4 に読ませる"
    );

    // 単一選択へ戻したら、元のふるまいに戻る。
    list.set_selection_mode(SelectionMode::Single);
    assert!(native.activates_on_single_click());
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

// ----------------------------------------------------------------- テーブル

/// 見出しの `GtkBox` と、行が並ぶ `GtkListBox`。
fn table_parts(table: &naui_gtk::Table) -> (gtk::Box, gtk::ListBox) {
    let root: gtk::Box = table.native_widget().downcast().expect("GtkBox");
    let header = root
        .first_child()
        .expect("見出し")
        .downcast::<gtk::Box>()
        .expect("見出しは GtkBox");
    (header, table.native_list_box())
}

/// 中身の `GtkLabel` を並び順に集める。
fn table_labels(container: &impl IsA<gtk::Widget>) -> Vec<gtk::Label> {
    children(container)
        .into_iter()
        .filter_map(|child| child.downcast::<gtk::Label>().ok())
        .collect()
}

/// 1 行ぶんのセルの文字。
fn table_cells(row: &gtk::Widget) -> Vec<String> {
    let content = row
        .clone()
        .downcast::<gtk::ListBoxRow>()
        .expect("GtkListBoxRow")
        .child()
        .expect("中身");
    table_labels(&content)
        .into_iter()
        .map(|label| label.text().to_string())
        .collect()
}

fn table_rows_are_native(ui: &Ui) -> Result<()> {
    let table = ui.table()?;
    assert!(table.is_empty());

    table.set_columns(&TableColumn::list(["都市", "人口"]));
    table.set_rows(&TableRow::list([
        ["東京", "13,960,000"],
        ["大阪", "8,838,000"],
    ]));
    assert_eq!(table.column_count(), 2);
    assert_eq!(table.len(), 2);

    let (header, list) = table_parts(&table);
    let titles: Vec<String> = table_labels(&header)
        .into_iter()
        .map(|label| label.text().to_string())
        .collect();
    assert_eq!(titles, ["都市", "人口"]);

    let rows = children(&list);
    assert_eq!(rows.len(), 2);
    assert_eq!(table_cells(&rows[0]), ["東京", "13,960,000"]);

    // 列より短い行は、足りない分が空のセルになる。
    table.set_rows(&[TableRow::new(["札幌"])]);
    let rows = children(&list);
    assert_eq!(table_cells(&rows[0]), ["札幌", ""]);

    // 行を差し替えると、選択も外れる。
    table.set_selected(0);
    assert_eq!(table.selected(), Some(0));
    table.set_rows(&TableRow::list([["福岡", "1,613,000"]]));
    assert_eq!(table.selected(), None);
    Ok(())
}

fn table_columns_follow_the_spec(ui: &Ui) -> Result<()> {
    let table = ui.table()?;
    table.set_columns(&[
        TableColumn::new("品名"),
        TableColumn::new("金額").width(120.0).align(Align::End),
    ]);
    table.set_rows(&[TableRow::new(["珈琲", "¥480"])]);

    let (header, list) = table_parts(&table);
    let head = table_labels(&header);
    assert_eq!(head[1].xalign(), 1.0, "右寄せの列は見出しも右へ寄ること");
    assert_eq!(head[1].width_request(), 120, "指定した幅が入ること");
    assert!(head[0].hexpands(), "指定の無い列だけが余りを受け取ること");
    assert!(!head[1].hexpands());

    let row = children(&list)[0]
        .clone()
        .downcast::<gtk::ListBoxRow>()
        .expect("GtkListBoxRow")
        .child()
        .expect("中身");
    let cells = table_labels(&row);
    assert_eq!(cells[1].xalign(), 1.0);
    assert_eq!(cells[1].width_request(), 120);
    // 見出しとセルが同じ `GtkSizeGroup` に入っていれば、幅は同じになる。
    assert_eq!(
        measure_width(&cells[0]).0,
        measure_width(&head[0]).0,
        "見出しと行の列幅がそろうこと"
    );
    Ok(())
}

fn table_native_selection_notifies(ui: &Ui) -> Result<()> {
    let table = ui.table()?;
    table.set_columns(&TableColumn::list(["時間帯"]));
    table.set_rows(&TableRow::list([["朝"], ["昼"], ["夜"]]));

    let (log, sink) = recorder::<Vec<usize>>();
    table.on_select({
        let mut sink = sink;
        move |indices: &[usize]| sink(indices.to_vec())
    });

    // GtkListBox 側で行を選ぶ (利用者がクリックしたのと同じ)。
    let list = table.native_list_box();
    let row = list.row_at_index(2).expect("3 行目");
    list.select_row(Some(&row));
    assert_eq!(table.selection(), vec![2]);
    assert_eq!(log.borrow().as_slice(), [vec![2]]);

    // プログラムからの変更は通知しない。
    table.set_selection(&[0]);
    assert_eq!(table.selection(), vec![0]);
    assert_eq!(log.borrow().len(), 1);

    // select は通知する。複数選択では 0 件にもなる。
    table.set_selection_mode(SelectionMode::Multiple);
    table.select_many(&[0, 2]);
    table.select_many(&[]);
    assert_eq!(log.borrow().as_slice(), [vec![2], vec![0, 2], vec![]]);
    Ok(())
}

fn table_skips_disabled_rows(ui: &Ui) -> Result<()> {
    let table = ui.table()?;
    table.set_columns(&TableColumn::list(["状態", "件数"]));
    table.set_rows(&[
        TableRow::new(["下書き", "3"]),
        TableRow::new(["送信中", "1"]).enabled(false),
        TableRow::new(["送信済み", "12"]),
    ]);

    table.select(1);
    assert!(table.selection().is_empty(), "選べない行は選ばれないこと");
    table.select(2);
    assert_eq!(table.selection(), vec![2]);

    // GTK4 自身にも「この行は選べない」と伝わっている。
    let list = table.native_list_box();
    let row = list.row_at_index(1).expect("2 行目");
    assert!(!row.is_selectable());
    assert!(!row.is_sensitive());
    Ok(())
}

fn table_keeps_rows_when_columns_change(ui: &Ui) -> Result<()> {
    let table = ui.table()?;
    table.set_columns(&TableColumn::list(["都市", "人口", "面積"]));
    table.set_rows(&TableRow::list([["東京", "13,960,000", "2,194"]]));

    table.set_selected(0);
    table.set_columns(&TableColumn::list(["都市", "人口"]));
    assert_eq!(table.column_count(), 2);
    assert_eq!(table.len(), 1, "行はそのまま残ること");
    assert_eq!(table.selection(), vec![0], "選択もそのまま残ること");

    let (header, list) = table_parts(&table);
    assert_eq!(children(&header).len(), 2, "古い見出しが残らないこと");
    assert_eq!(table_cells(&children(&list)[0]), ["東京", "13,960,000"]);
    Ok(())
}

fn table_sorting_round_trips(ui: &Ui) -> Result<()> {
    let table = ui.table()?;
    table.set_columns(&[
        TableColumn::new("都市").sortable(true),
        TableColumn::new("人口").align(Align::End).sortable(true),
        TableColumn::new("備考"),
    ]);
    table.set_rows(&TableRow::list([
        ["東京", "13,960,000", ""],
        ["大阪", "8,838,000", ""],
    ]));
    assert_eq!(table.sort(), None, "作った直後は指定が無いこと");

    let (header, _) = table_parts(&table);
    let cells = children(&header);
    // 並べ替えられる列だけが押せる (`GtkButton`)。
    assert!(cells[0].is::<gtk::Button>());
    assert!(cells[1].is::<gtk::Button>());
    assert!(
        cells[2].is::<gtk::Label>(),
        "sortable でない列は押せないこと"
    );

    let (log, sink) = recorder::<(usize, SortOrder)>();
    table.on_sort({
        let mut sink = sink;
        move |column, order| sink((column, order))
    });

    // 利用者が見出しを押したのと同じ経路。
    let button = cells[1]
        .clone()
        .downcast::<gtk::Button>()
        .expect("GtkButton");
    button.emit_clicked();
    assert_eq!(table.sort(), Some((1, SortOrder::Ascending)));
    assert_eq!(log.borrow().as_slice(), [(1, SortOrder::Ascending)]);
    // 見出しには向きが出る。
    let label = button
        .child()
        .expect("見出しの中身")
        .downcast::<gtk::Label>()
        .expect("GtkLabel");
    assert!(label.text().ends_with('▲'), "昇順の指標が出ること");

    // もう一度押すと向きが反転する。
    button.emit_clicked();
    assert_eq!(table.sort(), Some((1, SortOrder::Descending)));
    assert!(label.text().ends_with('▼'));
    assert_eq!(log.borrow().len(), 2);

    // 別の列を押すと、その列の昇順から始まる。
    cells[0]
        .clone()
        .downcast::<gtk::Button>()
        .expect("GtkButton")
        .emit_clicked();
    assert_eq!(table.sort(), Some((0, SortOrder::Ascending)));
    assert_eq!(label.text(), "人口", "前の列の指標は外れること");

    // set_sort は通知しない。
    table.set_sort(Some((1, SortOrder::Descending)));
    assert_eq!(table.sort(), Some((1, SortOrder::Descending)));
    assert_eq!(log.borrow().len(), 3, "set_sort は通知しないこと");

    // 並べ替えられない列を指すと、指定は外れる。
    table.set_sort(Some((2, SortOrder::Ascending)));
    assert_eq!(table.sort(), None);
    Ok(())
}

// --------------------------------------------------------------------- ツリー

fn tree_box_of(tree: &naui_gtk::Tree) -> gtk::ListBox {
    tree.native_widget().downcast().expect("GtkListBox")
}

/// テストで使う木。src (2 つの葉) と docs (guide > intro.md)。
fn sample_tree() -> Vec<TreeItem> {
    vec![
        TreeItem::new("src").children([TreeItem::new("main.rs"), TreeItem::new("lib.rs")]),
        TreeItem::new("docs").child(TreeItem::new("guide").child(TreeItem::new("intro.md"))),
    ]
}

/// いま見えている行の文字を上から順に返す。
///
/// 行は全項目ぶん作り置きしてあり、閉じた枝の中は `set_visible(false)` で
/// 隠れている。
fn tree_labels(tree: &naui_gtk::Tree) -> Vec<String> {
    children(&tree_box_of(tree))
        .into_iter()
        .filter_map(|row| row.downcast::<gtk::ListBoxRow>().ok())
        // 祖先の状態に左右されない、行そのものの visible を見る。
        .filter(|row| row.get_visible())
        .filter_map(|row| row.child())
        .filter_map(|line| {
            // 行は [開閉ボタン (または余白), 文字の縦並び] の順。
            let content = children(&line).pop()?;
            let label = children(&content).into_iter().next()?;
            Some(label.downcast::<gtk::Label>().ok()?.text().to_string())
        })
        .collect()
}

/// 行に置かれた開閉ボタン。葉の行には無い。
///
/// `index` は**全項目**を深さ優先で並べたときの位置 (見えていない行も数える)。
fn tree_twisty(tree: &naui_gtk::Tree, index: i32) -> Option<gtk::Button> {
    let row = tree_box_of(tree).row_at_index(index)?;
    let line = row.child()?;
    children(&line).into_iter().next()?.downcast().ok()
}

fn tree_rows_follow_the_expansion(ui: &Ui) -> Result<()> {
    let tree = ui.tree()?;
    assert!(tree.is_empty());

    tree.set_items(&sample_tree());
    assert_eq!(tree.len(), 6, "子孫まで数えること");
    assert_eq!(
        children(&tree_box_of(&tree)).len(),
        6,
        "行は全項目ぶん作り置きされること"
    );
    // 何も開いていないので、見えているのは根の 2 つだけ。
    assert_eq!(tree_labels(&tree), ["src", "docs"]);
    assert!(!tree.is_expanded(&[0]));

    tree.set_expanded(&[0], true);
    assert!(tree.is_expanded(&[0]));
    assert_eq!(tree_labels(&tree), ["src", "main.rs", "lib.rs", "docs"]);

    // 孫まで開くと、間の枝もまとめて開く。
    tree.set_expanded(&[1, 0], true);
    assert!(tree.is_expanded(&[1]), "祖先も開かれること");
    assert_eq!(
        tree_labels(&tree),
        ["src", "main.rs", "lib.rs", "docs", "guide", "intro.md"]
    );

    tree.collapse_all();
    assert_eq!(tree_labels(&tree), ["src", "docs"]);
    tree.expand_all();
    assert_eq!(tree_labels(&tree).len(), 6);

    // 作り直すと、TreeItem::expanded のとおりに戻る。
    tree.set_items(&[TreeItem::new("親")
        .expanded(true)
        .children(TreeItem::list(["子"]))]);
    assert_eq!(tree_labels(&tree), ["親", "子"]);
    assert_eq!(tree.selected(), None);
    Ok(())
}

fn tree_native_selection_notifies(ui: &Ui) -> Result<()> {
    let tree = ui.tree()?;
    tree.set_items(&sample_tree());
    tree.set_expanded(&[0], true);
    assert_eq!(tree.selected(), None, "作った直後は何も選ばれていないこと");

    let (log, sink) = recorder::<Vec<usize>>();
    tree.on_select({
        let mut sink = sink;
        move |path: &[usize]| sink(path.to_vec())
    });

    // GtkListBox 側で行を選ぶ (利用者がクリックしたのと同じ)。
    let native = tree_box_of(&tree);
    let row = native.row_at_index(2).expect("3 行目 (lib.rs)");
    native.select_row(Some(&row));
    assert_eq!(tree.selected(), Some(vec![0, 1]));
    assert_eq!(log.borrow().as_slice(), [vec![0, 1]]);

    // プログラムからの変更は通知しない。閉じた枝の中でも祖先ごと開いて選ぶ。
    tree.set_selected(&[1, 0, 0]);
    assert_eq!(tree.selected(), Some(vec![1, 0, 0]));
    assert!(tree.is_expanded(&[1, 0]), "祖先が開かれること");
    assert_eq!(log.borrow().len(), 1);

    // select は通知する。
    tree.select(&[0]);
    assert_eq!(log.borrow().as_slice(), [vec![0, 1], vec![0]]);

    // 無いパスを選ぶと選択が外れ、空のパスで通知される。
    tree.select(&[9]);
    assert_eq!(tree.selected(), None);
    assert_eq!(log.borrow().as_slice(), [vec![0, 1], vec![0], Vec::new()]);

    // clear_selection は通知しない。
    tree.set_selected(&[0]);
    tree.clear_selection();
    assert_eq!(tree.selected(), None);
    assert_eq!(log.borrow().len(), 3);
    Ok(())
}

fn tree_skips_disabled_branches(ui: &Ui) -> Result<()> {
    let tree = ui.tree()?;
    tree.set_items(&[
        TreeItem::new("有効").child(TreeItem::new("子")),
        TreeItem::new("無効")
            .enabled(false)
            .child(TreeItem::new("孫")),
    ]);
    tree.expand_all();

    let native = tree_box_of(&tree);
    assert!(native.row_at_index(0).expect("1 行目").is_selectable());
    assert!(
        !native.row_at_index(2).expect("3 行目").is_selectable(),
        "無効な枝は GtkListBox 側でも選べない"
    );
    assert!(
        !native.row_at_index(3).expect("4 行目").is_selectable(),
        "無効な枝の中身も選べない"
    );

    tree.select(&[1, 0]);
    assert_eq!(tree.selected(), None);
    tree.select(&[0, 0]);
    assert_eq!(tree.selected(), Some(vec![0, 0]));
    Ok(())
}

fn tree_expansion_notifies(ui: &Ui) -> Result<()> {
    let tree = ui.tree()?;
    tree.set_items(&sample_tree());

    let (log, sink) = recorder::<(Vec<usize>, bool)>();
    tree.on_expand({
        let mut sink = sink;
        move |path: &[usize], expanded: bool| sink((path.to_vec(), expanded))
    });

    tree.expand(&[0]);
    tree.collapse(&[0]);
    assert_eq!(
        log.borrow().as_slice(),
        [(vec![0], true), (vec![0], false)],
        "naui からの開閉が 1 回ずつ通知されること"
    );

    // `set_expanded` と一括の開閉は通知しない。
    tree.set_expanded(&[0], true);
    tree.expand_all();
    tree.collapse_all();
    assert_eq!(log.borrow().len(), 2);

    // 行の開閉ボタンを押す (利用者がクリックしたのと同じ)。
    // 全項目の並びは src, main.rs, lib.rs, docs, guide, intro.md。
    let twisty = tree_twisty(&tree, 3).expect("docs の開閉ボタン");
    twisty.emit_clicked();
    assert!(tree.is_expanded(&[1]));
    assert_eq!(log.borrow().last(), Some(&(vec![1], true)));

    // 葉の行にはボタンが無い。
    assert!(
        tree_twisty(&tree, 1).is_none(),
        "葉 (main.rs) には開閉ボタンが無いこと"
    );
    Ok(())
}

fn tree_remembers_expansion_inside_a_closed_branch(ui: &Ui) -> Result<()> {
    let tree = ui.tree()?;
    tree.set_items(&sample_tree());
    tree.set_expanded(&[1, 0], true);
    assert_eq!(tree_labels(&tree), ["src", "docs", "guide", "intro.md"]);

    let (log, sink) = recorder::<(Vec<usize>, bool)>();
    tree.on_expand({
        let mut sink = sink;
        move |path: &[usize], expanded: bool| sink((path.to_vec(), expanded))
    });

    tree.collapse(&[1]);
    assert_eq!(tree_labels(&tree), ["src", "docs"]);
    assert!(
        tree.is_expanded(&[1, 0]),
        "見えなくなっても、中の開閉は覚えていること"
    );

    tree.expand(&[1]);
    assert_eq!(
        tree_labels(&tree),
        ["src", "docs", "guide", "intro.md"],
        "中の開閉ごと戻ること"
    );
    assert_eq!(
        log.borrow().as_slice(),
        [(vec![1], false), (vec![1], true)],
        "通知は操作した枝の分だけ"
    );
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

fn media_unmuting_restores_the_volume(ui: &Ui) -> Result<()> {
    let audio = ui.audio("/tmp/naui-gtk-test-none.m4a")?;
    let controls: gtk::MediaControls = audio.native_widget().downcast().expect("GtkMediaControls");
    let stream = || controls.media_stream().expect("GtkMediaFile");

    audio.set_volume(0.5);
    assert_eq!(audio.volume(), 0.5);
    assert_eq!(stream().volume(), 0.5);

    audio.set_muted(true);
    assert!(audio.is_muted());
    assert!(stream().is_muted());
    // `GtkMediaControls` は消音になると音量つまみを 0 へ動かし、その 0 を
    // `GtkMediaStream` へ書き戻す。
    assert_eq!(stream().volume(), 0.0);

    audio.set_muted(false);
    assert!(!audio.is_muted());
    assert!(!stream().is_muted());
    // GTK4 は音量を戻さないので、naui が持っている音量を入れ直す。
    // これをしないと、消音を外しても音が出ないままになる。
    assert_eq!(audio.volume(), 0.5, "消音を解くと音量が戻る");
    assert_eq!(stream().volume(), 0.5);
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

/// 保存はボタンとして構成され、設定を保つ。
///
/// `GtkFileDialog` の保存は表示するまで中身を読めないので、
/// **ボタンの実体と設定の保持まで**を確かめる。
fn file_saver_configuration(ui: &Ui) -> Result<()> {
    let saver = ui.file_saver("保存する")?;
    assert_eq!(saver.file_name(), "", "既定の名前は空 (GTK に任せる)");
    assert!(saver.destination().is_none(), "まだ保存していないこと");
    assert_eq!(saver.contents_len(), 0);

    saver.set_file_name("メモ");
    saver.set_filters(&[FileFilter::new("文書", ["txt", "md"])]);
    saver.set_contents("こんにちは".as_bytes());
    assert_eq!(saver.file_name(), "メモ", "補う拡張子は表示のときだけ足す");
    assert_eq!(saver.contents_len(), "こんにちは".len());

    let native: gtk::Button = saver.native_widget().downcast().expect("GtkButton");
    assert_eq!(
        native.label().map(|l| l.to_string()),
        Some("保存する".into())
    );
    saver.set_text("書き出す");
    assert_eq!(
        native.label().map(|l| l.to_string()),
        Some("書き出す".into())
    );

    saver.set_enabled(false);
    assert!(!native.is_sensitive());
    saver.set_enabled(true);

    // コンテナへ入れてもハンドルを手放して大丈夫なこと (他のウィジェットと同じ)。
    let stack = ui.stack(Orientation::Vertical)?;
    stack.append(&saver);
    assert_eq!(stack.len(), 1);
    Ok(())
}

/// トーストの設定が `AdwToast` へ届き、消すと外れること。
fn toast_configuration_reaches_the_native_toast(ui: &Ui) -> Result<()> {
    let window = ui.window("naui", 320.0, 200.0)?;
    let content = ui.stack(Orientation::Vertical)?;
    window.set_child(&content);
    window.show();
    assert!(
        window.native_toast_overlay().child().is_some(),
        "アプリの中身はトーストの入れ物ごしに載ること"
    );

    let toast = ui.toast("保存しました")?;
    toast.set_action("元に戻す");
    toast.set_timeout(3.0);
    assert!(!toast.is_visible(), "作っただけでは出ていないこと");
    assert!(toast.native_toast().is_none());

    toast.show();
    assert!(toast.is_visible());
    let native = toast.native_toast().expect("AdwToast");
    assert_eq!(
        native.title().map(|t| t.to_string()),
        Some("保存しました".into())
    );
    assert_eq!(
        native.button_label().map(|l| l.to_string()),
        Some("元に戻す".into())
    );
    assert_eq!(native.timeout(), 3, "秒で AdwToast へ渡ること");

    // 出したまま書き換えると、出ている AdwToast がその場で変わる。
    toast.set_message("書き出しました");
    toast.set_action("");
    assert_eq!(
        native.title().map(|t| t.to_string()),
        Some("書き出しました".into())
    );
    assert_eq!(native.button_label(), None, "空文字列でボタンが外れること");

    toast.dismiss();
    assert!(!toast.is_visible());
    assert!(toast.native_toast().is_none());
    window.close();
    Ok(())
}

/// 操作ボタンと消滅が、それぞれのクロージャへ届くこと。
fn toast_events_reach_the_closures(ui: &Ui) -> Result<()> {
    let window = ui.window("naui", 320.0, 200.0)?;
    window.set_child(&ui.stack(Orientation::Vertical)?);
    window.show();

    let toast = ui.toast("削除しました")?;
    toast.set_action("元に戻す");
    // 押されるまで消えないトーストにしておく。
    toast.set_timeout(0.0);
    assert_eq!(toast.spec().timeout_secs(), 0);

    let seen = Rc::new(RefCell::new(Vec::new()));
    toast.on_action({
        let seen = seen.clone();
        move || seen.borrow_mut().push("action")
    });
    toast.on_dismiss({
        let seen = seen.clone();
        move || seen.borrow_mut().push("dismiss")
    });

    toast.show();
    let native = toast.native_toast().expect("AdwToast");
    assert_eq!(native.timeout(), 0, "0 は自動で消えない指定であること");
    // 実際に押されたときと同じ順で、AdwToast の側から知らせる。
    native.emit_by_name::<()>("button-clicked", &[]);
    native.dismiss();

    assert_eq!(
        seen.borrow().as_slice(),
        ["action", "dismiss"],
        "押された通知のあとに、消えた通知が届くこと"
    );
    assert!(!toast.is_visible());

    // 消えたあとに来る通知は捨てられ、二重には届かない。
    native.dismiss();
    assert_eq!(seen.borrow().len(), 2);
    window.close();
    Ok(())
}

/// 新しいトーストが前のものを置き換え、置き換えられたほうは通知しないこと。
fn toast_replaces_the_previous_one(ui: &Ui) -> Result<()> {
    let window = ui.window("naui", 320.0, 200.0)?;
    window.set_child(&ui.stack(Orientation::Vertical)?);
    window.show();

    let first = ui.toast("1 つめ")?;
    first.set_timeout(0.0);
    let dismissed = Rc::new(Cell::new(0));
    first.on_dismiss({
        let dismissed = dismissed.clone();
        move || dismissed.set(dismissed.get() + 1)
    });
    first.show();

    let second = ui.toast("2 つめ")?;
    second.set_timeout(0.0);
    second.show();

    assert!(!first.is_visible(), "前のものは消えること");
    assert!(second.is_visible());
    assert_eq!(
        dismissed.get(),
        0,
        "アプリ自身の操作なので、消えた通知は届かないこと"
    );

    second.dismiss();
    window.close();
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

/// ツールバーは区切りを含めた並びで `GtkBox` に載り、区切りのところは
/// ボタンを持たない。
fn toolbar_items_map_to_native(ui: &Ui) -> Result<()> {
    let toolbar = ui.toolbar()?;
    assert!(toolbar.is_empty());

    toolbar.set_items(&[
        ToolbarItem::new(ToolbarIcon::New, "新規"),
        ToolbarItem::separator(),
        ToolbarItem::new(ToolbarIcon::Save, "保存").enabled(false),
    ]);
    assert_eq!(toolbar.len(), 3, "区切りも 1 項目として数える");

    let native = toolbar.native_box();
    assert_eq!(native.orientation(), gtk::Orientation::Horizontal);

    let first = toolbar.native_button(0).expect("先頭はボタン");
    assert_eq!(
        first.icon_name().map(|n| n.to_string()),
        Some("document-new-symbolic".to_string()),
        "アイコンテーマの名前が入る"
    );
    assert_eq!(
        first.tooltip_text().map(|t| t.to_string()),
        Some("新規".to_string()),
        "ラベルはツールチップに出る"
    );
    assert!(first.is_sensitive());
    assert!(toolbar.native_button(1).is_none(), "区切りにボタンは無い");
    assert!(!toolbar
        .native_button(2)
        .expect("3 番目はボタン")
        .is_sensitive());
    assert!(toolbar.native_button(9).is_none(), "範囲外は None");

    // 区切りは GtkSeparator として並ぶ。
    let mut separators = 0;
    let mut child = native.first_child();
    while let Some(current) = child {
        child = current.next_sibling();
        if current.downcast::<gtk::Separator>().is_ok() {
            separators += 1;
        }
    }
    assert_eq!(separators, 1);

    assert!(toolbar.is_item_enabled(0));
    assert!(!toolbar.is_item_enabled(1), "区切りは押せない");
    assert!(!toolbar.is_item_enabled(2));

    // 項目ごとの指定と全体の指定は AND を取る。
    toolbar.set_item_enabled(2, true);
    assert!(toolbar.native_button(2).expect("ボタン").is_sensitive());
    toolbar.set_enabled(false);
    assert!(!toolbar.is_item_enabled(0));
    assert!(!toolbar.native_button(0).expect("ボタン").is_sensitive());
    toolbar.set_enabled(true);
    assert!(
        toolbar.is_item_enabled(2),
        "全体を戻すと項目ごとの指定が残る"
    );

    // 区切りへの set_item_enabled は無視する。
    toolbar.set_item_enabled(1, true);
    assert!(!toolbar.is_item_enabled(1));

    toolbar.set_items(&[]);
    assert!(toolbar.is_empty());
    assert!(native.first_child().is_none());
    Ok(())
}

/// 押されたインデックスは、区切りを含めた並びの位置で届く。
fn toolbar_activation_notifies(ui: &Ui) -> Result<()> {
    let toolbar = ui.toolbar()?;
    toolbar.set_items(&[
        ToolbarItem::new(ToolbarIcon::Cut, "切り取り"),
        ToolbarItem::separator(),
        ToolbarItem::new(ToolbarIcon::Paste, "貼り付け").enabled(false),
    ]);

    let seen = Rc::new(RefCell::new(Vec::new()));
    toolbar.on_activate({
        let seen = seen.clone();
        move |index| seen.borrow_mut().push(index)
    });

    toolbar.activate(0);
    assert_eq!(*seen.borrow(), vec![0]);
    toolbar.activate(1);
    toolbar.activate(2);
    toolbar.activate(9);
    assert_eq!(
        *seen.borrow(),
        vec![0],
        "区切り・押せない項目・範囲外は通知しない"
    );

    // GTK4 側の clicked から届く実際の通知経路も確かめる。
    toolbar.native_button(0).expect("ボタン").emit_clicked();
    assert_eq!(*seen.borrow(), vec![0, 0]);

    toolbar.set_item_enabled(2, true);
    toolbar.activate(2);
    assert_eq!(*seen.borrow(), vec![0, 0, 2]);

    toolbar.set_enabled(false);
    toolbar.activate(0);
    assert_eq!(
        *seen.borrow(),
        vec![0, 0, 2],
        "無効なツールバーは通知しない"
    );
    Ok(())
}

/// ツールバーはヘッダーバーへ入り、外すと消える。
fn toolbar_attaches_to_the_header_bar(ui: &Ui) -> Result<()> {
    let window = ui.window("ツールバー", 400.0, 300.0)?;
    let header = window.native_header_bar();
    let toolbar = ui.toolbar()?;
    toolbar.set_items(&[
        ToolbarItem::new(ToolbarIcon::New, "新規"),
        ToolbarItem::new(ToolbarIcon::Open, "開く"),
    ]);

    window.set_toolbar(&toolbar);
    let mount = toolbar.native_box();
    assert_eq!(
        mount.ancestor(adw::HeaderBar::static_type()).as_ref(),
        Some(header.upcast_ref::<gtk::Widget>()),
        "ヘッダーバーの中に入っていること"
    );

    window.clear_toolbar();
    assert!(
        mount.ancestor(adw::HeaderBar::static_type()).is_none(),
        "外すとヘッダーバーから消える"
    );
    window.close();
    Ok(())
}

/// 通知の中からツールバーを組み替えても、二重借用にならない。
fn toolbar_callback_is_reentrant_and_replaceable(ui: &Ui) -> Result<()> {
    let toolbar = ui.toolbar()?;
    toolbar.set_items(&[
        ToolbarItem::new(ToolbarIcon::New, "春"),
        ToolbarItem::new(ToolbarIcon::Open, "夏"),
        ToolbarItem::new(ToolbarIcon::Save, "秋"),
    ]);

    let first = Rc::new(RefCell::new(Vec::new()));
    let replacement = Rc::new(RefCell::new(Vec::new()));
    toolbar.on_activate({
        let toolbar = toolbar.clone();
        let first = first.clone();
        let replacement = replacement.clone();
        move |index| {
            first.borrow_mut().push(index);
            toolbar.set_items(&[
                ToolbarItem::new(ToolbarIcon::Add, "朝"),
                ToolbarItem::new(ToolbarIcon::Remove, "昼"),
            ]);
            toolbar.on_activate({
                let replacement = replacement.clone();
                move |index| replacement.borrow_mut().push(index)
            });
        }
    });

    toolbar.activate(0);
    assert_eq!(*first.borrow(), vec![0]);
    assert_eq!(toolbar.len(), 2);

    toolbar.activate(1);
    assert_eq!(*first.borrow(), vec![0], "古い通知先は外れること");
    assert_eq!(*replacement.borrow(), vec![1]);
    Ok(())
}

/// 名前を間違えると、その項目だけ「画像がありません」の絵になる。
fn toolbar_icons_exist_in_the_theme(_ui: &Ui) -> Result<()> {
    let display = gtk::gdk::Display::default().expect("ディスプレイ");
    let theme = gtk::IconTheme::for_display(&display);
    let mut missing = Vec::new();
    for icon in ToolbarIcon::ALL {
        if !theme.has_icon(icon.icon_name()) {
            missing.push(icon.icon_name());
        }
    }
    assert!(missing.is_empty(), "テーマに無いアイコン: {missing:?}");
    Ok(())
}

// ------------------------------------------------------------ 日付ピッカー

/// `GtkBox` に並んでいる子の数。
fn children_of(container: &gtk::Box) -> Vec<gtk::Widget> {
    let mut children = Vec::new();
    let mut child = container.first_child();
    while let Some(widget) = child {
        child = widget.next_sibling();
        children.push(widget);
    }
    children
}

/// 種別ごとに、必要なコントロールだけが並ぶ。
fn date_picker_shows_only_what_its_mode_needs(ui: &Ui) -> Result<()> {
    let date = ui.date_picker(DatePickerMode::Date)?;
    assert_eq!(date.mode(), DatePickerMode::Date);
    let native: gtk::Box = date.native_widget().downcast().expect("GtkBox");
    // 日付はボタン 1 つだけ。
    assert_eq!(children_of(&native).len(), 1);

    let time = ui.date_picker(DatePickerMode::Time)?;
    let native: gtk::Box = time.native_widget().downcast().expect("GtkBox");
    // 時・区切り・分。
    assert_eq!(children_of(&native).len(), 3);

    let both = ui.date_picker(DatePickerMode::DateTime)?;
    let native: gtk::Box = both.native_widget().downcast().expect("GtkBox");
    assert_eq!(children_of(&native).len(), 4);

    // 作った直後は現在日時が入っている。
    assert!(date.value().is_valid());
    assert!(date.value().year >= 2000, "{}", date.value());

    // ポップオーバーの中身はカレンダー。
    assert!(both.native_button().popover().is_some());
    Ok(())
}

/// 値の書き込みと、ネイティブ側の操作の両方向。
fn date_picker_value_round_trips(ui: &Ui) -> Result<()> {
    let picker = ui.date_picker(DatePickerMode::DateTime)?;
    let (log, sink) = recorder::<DateTime>();
    picker.on_change(sink);

    picker.set_value(DateTime::new(2026, 8, 22, 9, 30));
    assert_eq!(picker.value(), DateTime::new(2026, 8, 22, 9, 30));
    let calendar = picker.native_calendar();
    let (hour, minute) = picker.native_spins();
    assert_eq!(calendar.date().year(), 2026);
    assert_eq!(calendar.date().month(), 8);
    assert_eq!(calendar.date().day_of_month(), 22);
    assert_eq!(hour.value() as i32, 9);
    assert_eq!(minute.value() as i32, 30);
    assert!(log.borrow().is_empty(), "set_value は通知しない");

    // 暦として成り立たない値は丸める。
    picker.set_value(DateTime::new(2026, 11, 31, 25, 70));
    assert_eq!(picker.value(), DateTime::new(2026, 11, 30, 23, 59));

    // カレンダーで日を選ぶ (GTK 側の `day-selected` が飛ぶ)。
    let chosen = glib::DateTime::from_local(2027, 1, 5, 0, 0, 0.0).expect("日付");
    calendar.select_day(&chosen);
    assert_eq!(picker.value(), DateTime::new(2027, 1, 5, 23, 59));

    // 分のスピンボタンを回す。
    minute.set_value(45.0);
    assert_eq!(picker.value(), DateTime::new(2027, 1, 5, 23, 45));
    assert_eq!(
        log.borrow().as_slice(),
        [
            DateTime::new(2027, 1, 5, 23, 59),
            DateTime::new(2027, 1, 5, 23, 45)
        ]
    );

    picker.set_enabled(false);
    let native: gtk::Box = picker.native_widget().downcast().expect("GtkBox");
    assert!(!native.is_sensitive());
    Ok(())
}

/// 日付だけの表示は時刻を、時刻だけの表示は日付を保つ。
fn date_picker_keeps_the_part_it_does_not_show(ui: &Ui) -> Result<()> {
    let date_only = ui.date_picker(DatePickerMode::Date)?;
    date_only.set_value(DateTime::new(2026, 8, 22, 9, 30));
    let chosen = glib::DateTime::from_local(2027, 1, 5, 0, 0, 0.0).expect("日付");
    date_only.native_calendar().select_day(&chosen);
    assert_eq!(
        date_only.value(),
        DateTime::new(2027, 1, 5, 9, 30),
        "日付を選んでも時刻は残る"
    );

    let time_only = ui.date_picker(DatePickerMode::Time)?;
    time_only.set_value(DateTime::new(2026, 8, 22, 9, 30));
    let (hour, _) = time_only.native_spins();
    hour.set_value(18.0);
    assert_eq!(
        time_only.value(),
        DateTime::new(2026, 8, 22, 18, 30),
        "時刻を選んでも日付は残る"
    );
    Ok(())
}

/// 下限・上限の外へは出ない。
fn date_picker_stays_inside_the_range(ui: &Ui) -> Result<()> {
    let picker = ui.date_picker(DatePickerMode::Date)?;
    picker.set_value(DateTime::new(2026, 6, 15, 9, 30));
    picker.set_range(
        Some(DateTime::date(2026, 1, 1)),
        Some(DateTime::date(2026, 12, 31)),
    );
    assert_eq!(picker.value(), DateTime::new(2026, 6, 15, 9, 30));

    let (log, sink) = recorder::<DateTime>();
    picker.on_change(sink);

    let outside = glib::DateTime::from_local(2030, 5, 5, 0, 0, 0.0).expect("日付");
    picker.native_calendar().select_day(&outside);
    assert_eq!(
        picker.value(),
        DateTime::new(2026, 12, 31, 9, 30),
        "上限で止まり、時刻は動かない"
    );
    // 押し戻した結果はカレンダーの表示にも反映される。
    assert_eq!(picker.native_calendar().date().year(), 2026);
    assert_eq!(picker.native_calendar().date().month(), 12);
    assert_eq!(
        log.borrow().as_slice(),
        [DateTime::new(2026, 12, 31, 9, 30)]
    );

    // 範囲の外にある値を渡すと、通知せずに端へ寄る。
    picker.set_value(DateTime::date(1990, 1, 1));
    assert_eq!(picker.value(), DateTime::new(2026, 1, 1, 0, 0));
    assert_eq!(log.borrow().len(), 1);

    // 時刻だけの表示では日付を見ない。
    let time_only = ui.date_picker(DatePickerMode::Time)?;
    time_only.set_value(DateTime::new(2026, 8, 22, 6, 0));
    time_only.set_range(Some(DateTime::time(9, 0)), Some(DateTime::time(18, 0)));
    assert_eq!(time_only.value(), DateTime::new(2026, 8, 22, 9, 0));
    Ok(())
}

fn date_picker_programmatic_changes_are_silent(ui: &Ui) -> Result<()> {
    let picker = ui.date_picker(DatePickerMode::DateTime)?;
    let (log, sink) = recorder::<DateTime>();
    picker.on_change(sink);

    picker.set_value(DateTime::new(2026, 8, 22, 9, 30));
    picker.set_value(DateTime::new(2027, 1, 5, 18, 45));
    picker.set_range(Some(DateTime::date(2020, 1, 1)), None);
    assert!(log.borrow().is_empty(), "プログラムからの変更は通知しない");

    // 通知の中で同じピッカーを触っても二重借用にならない。
    let seen = Rc::new(Cell::new(None));
    picker.on_change({
        let picker = picker.clone();
        let seen = seen.clone();
        move |value| {
            seen.set(Some(value));
            picker.set_value(DateTime::new(2026, 12, 31, 0, 0));
        }
    });
    let (hour, _) = picker.native_spins();
    hour.set_value(8.0);
    assert_eq!(seen.get(), Some(DateTime::new(2027, 1, 5, 8, 45)));
    assert_eq!(picker.value(), DateTime::new(2026, 12, 31, 0, 0));
    Ok(())
}

/// 時刻ピッカーは時と分の `GtkSpinButton` を `:` で挟んだもの。値はネイティブと
/// 往復し、`set_value` は通知しない。
fn time_picker_value_round_trips(ui: &Ui) -> Result<()> {
    let picker = ui.time_picker()?;
    let native: gtk::Box = picker.native_widget().downcast().expect("GtkBox");
    // 時・区切り・分。
    assert_eq!(children_of(&native).len(), 3);
    // 作った直後は現在時刻。
    assert!(picker.value().is_valid());

    let (log, sink) = recorder::<Time>();
    picker.on_change(sink);

    let (hour, minute) = picker.native_spins();
    picker.set_value(Time::new(9, 30));
    assert_eq!(picker.value(), Time::new(9, 30));
    assert_eq!(hour.value() as i32, 9);
    assert_eq!(minute.value() as i32, 30);
    assert!(log.borrow().is_empty(), "set_value は通知しない");

    // 時計として成り立たない値は丸める。
    picker.set_value(Time::new(25, 70));
    assert_eq!(picker.value(), Time::new(23, 59));

    // スピンボタンを回す (GTK 側の `value-changed` が飛ぶ)。
    hour.set_value(18.0);
    assert_eq!(picker.value(), Time::new(18, 59));
    minute.set_value(45.0);
    assert_eq!(picker.value(), Time::new(18, 45));
    assert_eq!(
        log.borrow().as_slice(),
        [Time::new(18, 59), Time::new(18, 45)]
    );

    picker.set_enabled(false);
    assert!(!native.is_sensitive());
    Ok(())
}

/// 下限・上限の外へは出ない。`GtkSpinButton` に時刻としての範囲は無いので、
/// 押し戻した結果はスピンボタンの表示にも書き戻る。
fn time_picker_stays_inside_the_range(ui: &Ui) -> Result<()> {
    let picker = ui.time_picker()?;
    picker.set_value(Time::new(12, 0));
    picker.set_range(Some(Time::new(9, 0)), Some(Time::new(18, 0)));
    assert_eq!(picker.value(), Time::new(12, 0));

    let (log, sink) = recorder::<Time>();
    picker.on_change(sink);

    let (hour, _) = picker.native_spins();
    hour.set_value(22.0);
    assert_eq!(picker.value(), Time::new(18, 0), "上限で止まること");
    assert_eq!(hour.value() as i32, 18, "表示も押し戻ること");
    assert_eq!(log.borrow().as_slice(), [Time::new(18, 0)]);

    // 範囲の外にある値を渡すと、通知せずに端へ寄る。
    picker.set_value(Time::new(1, 0));
    assert_eq!(picker.value(), Time::new(9, 0));
    assert_eq!(log.borrow().len(), 1);

    // 範囲を外すと自由に選べる。
    picker.set_range(None, None);
    hour.set_value(1.0);
    assert_eq!(picker.value(), Time::new(1, 0));

    // 通知の中で同じピッカーを触っても二重借用にならない。
    let seen = Rc::new(Cell::new(None));
    picker.on_change({
        let picker = picker.clone();
        let seen = seen.clone();
        move |value| {
            seen.set(Some(value));
            picker.set_value(Time::MIDNIGHT);
        }
    });
    hour.set_value(6.0);
    assert_eq!(seen.get(), Some(Time::new(6, 0)));
    assert_eq!(picker.value(), Time::MIDNIGHT);
    Ok(())
}

// ----------------------------------------------------------------- 非同期

/// idle source を吐かせる。メインループは起動しない。
///
/// `run_for_test` はメインループへ入らないので、投函されたものを進めるには
/// メインコンテキストを自分で回す必要がある。
fn pump() {
    let context = glib::MainContext::default();
    // 入っている分だけ回す。取りこぼしと無限ループの両方を避けるため上限を付ける。
    for _ in 0..64 {
        if !context.iteration(false) {
            break;
        }
    }
}

/// 一度だけ譲る future。次のティックで続きが走る。
struct YieldOnce(bool);

impl Future for YieldOnce {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        if self.0 {
            return Poll::Ready(());
        }
        self.0 = true;
        cx.waker().wake_by_ref();
        Poll::Pending
    }
}

/// 別スレッドから送った値が、メインコンテキスト経由でラベルへ届く。
fn channel_delivers_from_a_worker_thread(ui: &Ui) -> Result<()> {
    let label = ui.label("待機中")?;
    let sender = ui.tasks().channel({
        let label = label.clone();
        move |text: String| label.set_text(&text)
    });

    std::thread::spawn(move || sender.send("完了".to_string()))
        .join()
        .expect("ワーカースレッド")
        .expect("送信");

    assert_eq!(
        label.text(),
        "待機中",
        "メインコンテキストを回すまでは届かないこと (必ず後回しになる)"
    );

    pump();
    assert_eq!(label.text(), "完了", "idle source 経由で届くこと");
    Ok(())
}

/// spawn した処理がメインコンテキストの上で進み、完了する。
fn spawn_runs_a_local_future(ui: &Ui) -> Result<()> {
    let label = ui.label("待機中")?;
    let task = ui.tasks().spawn({
        let label = label.clone();
        async move {
            label.set_text("途中");
            YieldOnce(false).await;
            label.set_text("完了");
        }
    });

    assert_eq!(label.text(), "待機中", "その場では走らないこと");
    assert!(!task.is_finished());

    pump();
    assert_eq!(label.text(), "完了", "譲った先まで進むこと");
    assert!(task.is_finished(), "終わったら取っ手が終了を返すこと");
    Ok(())
}

/// cancel した処理は一度も走らない。
fn cancel_stops_a_future(ui: &Ui) -> Result<()> {
    let label = ui.label("待機中")?;
    let task = ui.tasks().spawn({
        let label = label.clone();
        async move {
            label.set_text("ここへは来ない");
        }
    });

    task.cancel();
    pump();

    assert_eq!(label.text(), "待機中", "cancel した処理は走らないこと");
    Ok(())
}
