//! AppKit の実コントロールに対する動作確認。
//!
//! `performClick` などネイティブ側の操作を発生させ、Rust のクロージャへ
//! 届くこと・ネイティブの状態が変わることを確かめる。
//!
//! AppKit はメインスレッドでしか触れないが、Rust の標準テストハーネスは
//! 各テストを別スレッドで走らせる (`--test-threads=1` でも同じ)。
//! そのため `harness = false` にして、自前のランナーをメインスレッドで回す。

use std::cell::{Cell, RefCell};
use std::future::Future;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::pin::Pin;
use std::rc::Rc;
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use naui_core::{
    Align, Color, DatePickerMode, DateTime, DialogButtons, DialogResponse, FileFilter,
    FilePickerMode, Fit, GridCell, Length, ListItem, NavItem, Orientation, Padding, PlaybackState,
    PopupItem, Result, ScrollPolicy, SelectionMode, Sizing, SortOrder, TableColumn, TableRow,
    Theme, Time, ToolbarIcon, ToolbarItem, Track, TreeItem,
};
use naui_macos::{run_for_test, ListRow, Ui, Widget};
use objc2::rc::Retained;
use objc2::sel;
use objc2::{msg_send, AnyThread, MainThreadMarker, Message};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSButton, NSColor, NSColorSpace, NSComboBox,
    NSControlStateValueOff, NSDatePicker, NSDatePickerElementFlags, NSEvent, NSEventMask,
    NSEventModifierFlags, NSEventType, NSImage, NSImageScaling, NSImageView, NSLayoutConstraint,
    NSLayoutConstraintOrientation, NSOutlineViewDelegate, NSScrollView, NSScrollerStyle,
    NSSearchField, NSSecureTextField, NSSegmentedControl, NSStepper, NSSwitch, NSTableViewDelegate,
    NSTextField, NSTextInputClient, NSTextView, NSUserInterfaceItemIdentification, NSView,
    NSWindow, NSWindowTitleVisibility,
};
use objc2_foundation::{
    NSCalendar, NSCalendarIdentifierGregorian, NSDate, NSDateComponents, NSDefaultRunLoopMode,
    NSNotFound, NSNotification, NSPoint, NSRange, NSRunLoop, NSSize, NSString,
};

/// テストケース 1 件。
type Case = (&'static str, fn(&Ui) -> Result<()>);

fn main() {
    let cases: &[Case] = &[
        ("ボタンのクリックがクロージャへ届く", button_click),
        (
            "チェックボックスが反転し新しい値を通知する",
            checkbox_toggle,
        ),
        (
            "スイッチが切り替わり新しい値を通知する",
            toggle_switches_and_notifies,
        ),
        (
            "スイッチがネイティブの NSSwitch とラベルを横に並べる",
            toggle_places_a_native_switch_beside_its_label,
        ),
        (
            "色ピッカーの値がネイティブの NSColorWell と往復する",
            color_picker_round_trips,
        ),
        (
            "色ピッカーがカタログ色を sRGB として読む",
            color_picker_reads_catalog_colors_as_srgb,
        ),
        ("文字列がネイティブと往復する (日本語含む)", text_round_trip),
        ("複数行入力が改行込みで往復する", text_area_round_trip),
        (
            "複数行入力の打鍵が通知され、プレースホルダーが消える",
            text_area_notifies_while_typing,
        ),
        (
            "複数行入力のプレースホルダーがクリックを通す",
            text_area_placeholder_passes_clicks,
        ),
        (
            "複数行入力が指定した高さに収まり、折り返し幅が追従する",
            text_area_follows_the_given_size,
        ),
        ("スライダーが範囲でクランプされる", slider_clamp),
        ("進捗バーが 0..1 に収まる", progress_clamp),
        (
            "コンボボックスの選択がネイティブと往復する",
            combo_box_selection_round_trips,
        ),
        (
            "コンボボックスの通知中に内容と通知先を差し替えられる",
            combo_box_callback_is_reentrant,
        ),
        (
            "自由入力コンボボックスの文字列がネイティブと往復する",
            editable_combo_box_text_round_trips,
        ),
        (
            "自由入力コンボボックスの通知中に内容と通知先を差し替えられる",
            editable_combo_box_callback_is_reentrant,
        ),
        (
            "自由入力コンボボックスの候補を打鍵のたびに絞り込める",
            editable_combo_box_items_can_be_narrowed_while_typing,
        ),
        (
            "ラジオグループの選択がネイティブと往復する",
            radio_group_selection_round_trips,
        ),
        (
            "ラジオグループのクリックが 1 つだけ点けて通知する",
            radio_group_click_selects_one,
        ),
        (
            "ラジオグループの通知中に内容と通知先を差し替えられる",
            radio_group_callback_is_reentrant,
        ),
        ("スタックが子を生かし続ける", stack_keeps_children),
        (
            "スタックが子を差し込み・外し・空にできる",
            stack_inserts_and_removes_children,
        ),
        (
            "グリッドがマス単位で子を外せる",
            grid_removes_children_by_cell,
        ),
        (
            "グリッドの結合したマスは外した後も 1 つのマス",
            grid_keeps_merges_after_the_child_is_removed,
        ),
        ("タブを外して空にできる", tabs_remove_and_clear),
        ("ウィンドウを設定して閉じられる", window_lifecycle),
        ("ナビバーの選択がネイティブと往復する", navbar_selection),
        ("ドックが等幅の項目を持つ", dock_items),
        ("タブが中身ごと切り替わる", tabs_selection),
        ("メニューの選択が 1 つだけ点く", menu_selection),
        (
            "リストの行の文字が縦中央にそろう",
            list_rows_are_vertically_centered,
        ),
        (
            "リストの補助の文字が 2 行目に出る",
            list_detail_makes_a_second_line,
        ),
        (
            "リストの選択がネイティブと往復する",
            list_selection_round_trips,
        ),
        ("リストの複数選択が 0 件にもなる", list_multiple_selection),
        ("リストが選べない行を飛ばす", list_skips_disabled_rows),
        (
            "行のクリックがクロージャへ届く",
            list_row_activation_notifies,
        ),
        (
            "リストに任意ウィジェットの行を載せられる",
            list_accepts_composed_rows,
        ),
        (
            "clone した Ui で行を後から作れる",
            ui_clone_builds_rows_from_a_callback,
        ),
        (
            "リストの行が NSTableView に描かれる",
            list_rows_are_native_views,
        ),
        (
            "NSTableView 側の選択がクロージャへ届く",
            list_native_selection_notifies,
        ),
        (
            "リストの通知の中からリストを操作できる",
            list_callback_can_touch_the_list,
        ),
        (
            "テーブルの列と行が NSTableView に並ぶ",
            table_columns_and_rows_are_native,
        ),
        (
            "テーブルの列の幅と揃えが指定どおりになる",
            table_columns_follow_the_spec,
        ),
        (
            "テーブルの選択がネイティブと往復する",
            table_selection_round_trips,
        ),
        ("テーブルが選べない行を飛ばす", table_skips_disabled_rows),
        (
            "テーブルの列を差し替えても行が残る",
            table_keeps_rows_when_columns_change,
        ),
        (
            "NSTableView 側の選択がテーブルのクロージャへ届く",
            table_native_selection_notifies,
        ),
        (
            "列を何度も切り替えても表が使えたままになる",
            table_survives_repeated_column_changes,
        ),
        (
            "見出しの並べ替えがネイティブと往復する",
            table_sorting_round_trips,
        ),
        (
            "列を入れ替えても幅いっぱいを使い続ける",
            table_columns_keep_filling_the_width,
        ),
        ("ツリーの行が展開に追従する", tree_rows_follow_the_expansion),
        (
            "ツリーの選択がネイティブと往復する",
            tree_selection_round_trips,
        ),
        ("ツリーが選べない枝を飛ばす", tree_skips_disabled_branches),
        ("ツリーの開閉が通知される", tree_expansion_notifies),
        (
            "閉じた枝の中の開閉が保たれる",
            tree_remembers_expansion_inside_a_closed_branch,
        ),
        (
            "ツリーの行が NSOutlineView に描かれる",
            tree_rows_are_native_views,
        ),
        (
            "ツリーの通知の中からツリーを操作できる",
            tree_callback_can_touch_the_tree,
        ),
        ("パンくずが末尾を現在地にする", breadcrumbs_path),
        ("ページ送りが範囲内に収まる", pagination_steps),
        ("リンクのクリックがクロージャへ届く", link_click),
        ("テーマを実行中に切り替えられる", theme_switch),
        ("大きさの指定が制約として反映される", sizing_constraints),
        (
            "大きさを指定し直しても制約が積み上がらない",
            sizing_is_replaced,
        ),
        ("交差軸の Fill が親の幅に合わせて広がる", stack_fill_cross),
        (
            "Stack の Auto の子が余白で横に広がらない",
            stack_auto_does_not_fill,
        ),
        ("Stack の主軸の Fill は余白を受け取る", stack_fill_main),
        (
            "上限付きの Fill が空間のあるときは上限まで広がる",
            fill_with_max_prefers_the_max,
        ),
        (
            "交差軸の Fill に上限を付けても親の幅と衝突しない",
            stack_fill_cross_respects_max,
        ),
        ("スペーサーが余りを吸って後続を端へ寄せる", spacer_pushes),
        ("グリッドが行と列を広げて子を置く", grid_places_children),
        (
            "グリッドの Auto の子が再レイアウトで横に広がらない",
            grid_auto_does_not_fill,
        ),
        (
            "グリッドの Auto 行が Fill 行の高さを奪わない",
            grid_fill_row_keeps_the_rest,
        ),
        (
            "グリッドの Fill の子がセルの取り分だけ広がる",
            grid_fill_column_takes_only_its_share,
        ),
        (
            "グリッドの Fill の子が固定幅の列を奪わない",
            grid_fill_column_leaves_fixed_columns,
        ),
        (
            "Gallery のメディア欄が高さ変更後にラベル幅を広げない",
            gallery_media_status_does_not_expand,
        ),
        ("グリッドの子を置き換える", grid_replaces_child),
        ("スクロールが中身を保持する", scroll_keeps_child),
        (
            "折りたたみがクリックで開閉し、変わった後を通知する",
            expander_toggles_and_notifies,
        ),
        (
            "たたんだ折りたたみの中身がレイアウトから外れる",
            expander_collapsed_child_leaves_the_layout,
        ),
        (
            "折りたたみの中身が幅いっぱいに置かれる",
            expander_child_fills_the_width,
        ),
        (
            "ラベルは頼まれたときだけ折り返す",
            label_wraps_only_when_asked,
        ),
        (
            "分割ビューがネイティブの NSSplitView に 2 区画を並べる",
            split_view_arranges_two_panes_in_a_native_split_view,
        ),
        (
            "分割ビューが指定した位置で仕切りを置き start 側の大きさを保つ",
            split_view_keeps_the_start_size_when_the_window_grows,
        ),
        (
            "分割ビューのドラッグが最小の大きさに収まり通知する",
            split_view_drag_clamps_to_the_minimums_and_notifies,
        ),
        (
            "縦に長いタブの中身がスクロールで下まで届く",
            tall_tab_content_scrolls,
        ),
        (
            "場所を取るスクローラが出ても中身が見える幅に収まる",
            scroll_content_fits_beside_a_legacy_scroller,
        ),
        ("グリッドの同じ行が縦中央でそろう", grid_row_alignment),
        (
            "ファイル選択がボタンとして構成され設定を保つ",
            file_picker_configuration,
        ),
        (
            "保存の設定が NSSavePanel へ届く",
            file_saver_configuration_reaches_the_panel,
        ),
        (
            "保存の既定名に絞り込みの拡張子が付く",
            file_saver_adds_the_default_extension,
        ),
        (
            "ダイアログの設定が NSAlert へ届く",
            dialog_configuration_reaches_the_alert,
        ),
        (
            "ダイアログのボタンが役割の順に並ぶ",
            dialog_buttons_follow_the_macos_order,
        ),
        (
            "ボタンを指定しないダイアログに OK が出る",
            dialog_without_buttons_shows_ok,
        ),
        (
            "出していないダイアログは閉じても何も起きない",
            dialog_is_closed_until_opened,
        ),
        (
            "トーストがウィンドウへ重なり、消すと外れる",
            toast_is_placed_over_the_window,
        ),
        (
            "トーストの操作ボタンが通知して閉じる",
            toast_action_notifies_and_dismisses,
        ),
        (
            "新しいトーストが前のものを黙って置き換える",
            toast_replaces_the_previous_one,
        ),
        (
            "トーストが指定した時間で自分から消える",
            toast_dismisses_itself_after_the_timeout,
        ),
        (
            "編集メニューが貼り付けをレスポンダチェーンへ配送する",
            menu_bar_provides_edit_shortcuts,
        ),
        (
            "画像がローカルのファイルから読み込まれる",
            image_loads_a_local_file,
        ),
        (
            "収め方が NSImageView の imageScaling になる",
            image_fit_maps_to_native_scaling,
        ),
        (
            "動画の表示領域が読み込み前から高さを保つ",
            video_display_reserves_height,
        ),
        (
            "音声が最後まで再生され Ended が届く",
            audio_plays_to_the_end,
        ),
        (
            "繰り返し再生が末尾で止まらない",
            audio_loops_back_to_the_start,
        ),
        (
            "再生位置の指定が AVPlayer に届く",
            media_seek_moves_the_position,
        ),
        ("音量と消音がネイティブと往復する", media_volume_round_trips),
        (
            "メディアのハンドルを捨てても落ちない",
            media_handles_can_be_dropped,
        ),
        (
            "再生位置が定期的にクロージャへ届く",
            media_reports_position_while_playing,
        ),
        (
            "別スレッドから送った値がラベルへ届く",
            channel_delivers_from_a_worker_thread,
        ),
        (
            "spawn した処理がランループの上で完了する",
            spawn_runs_a_local_future,
        ),
        ("cancel した処理は走らない", cancel_stops_a_future),
        (
            "ツールバーが NSToolbar に項目と区切りを並べる",
            toolbar_items_map_to_native,
        ),
        (
            "ツールバーの実行がクロージャへ届く",
            toolbar_activation_notifies,
        ),
        (
            "すべてのアイコンが SF Symbols に実在する",
            toolbar_icons_exist_as_sf_symbols,
        ),
        (
            "ツールバーが NSWindow に取り付き外れる",
            toolbar_attaches_to_a_window,
        ),
        (
            "ツールバーの通知中に内容と通知先を差し替えられる",
            toolbar_callback_is_reentrant,
        ),
        (
            "ポップアップメニューが項目と区切り線を NSMenu に写す",
            popup_menu_items_map_to_native,
        ),
        (
            "日付ピッカーが表示する項目を種別から決める",
            date_picker_shows_the_elements_of_its_mode,
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
            "日付ピッカーの通知中に値と通知先を差し替えられる",
            date_picker_callback_is_reentrant,
        ),
        (
            "時刻ピッカーが時分だけを出し、値がネイティブと往復する",
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
            "数値入力が小数桁と範囲を守る",
            number_input_applies_the_spec,
        ),
        (
            "数値入力の打鍵が通知され、確定で表示が直る",
            number_input_notifies_while_typing,
        ),
        (
            "パスワード入力が伏せ字の欄として往復する",
            password_input_round_trips,
        ),
        (
            "検索入力が NSSearchField として往復する",
            search_input_round_trips,
        ),
        (
            "検索入力が Enter のときだけ確定を通知する",
            search_input_notifies_on_return,
        ),
        (
            "ポップアップメニューの選択がクロージャへ届く",
            popup_menu_selection_notifies,
        ),
        (
            "ポップアップメニューが取り付け先のビューのメニューになる",
            popup_menu_attaches_to_a_view,
        ),
    ];

    let mut failed = 0;
    for (name, case) in cases {
        let result = catch_unwind(AssertUnwindSafe(|| {
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

/// ボタンのクリックが Rust のクロージャへ届く。
fn button_click(ui: &Ui) -> Result<()> {
    let button = ui.button("押す")?;
    let hits = Rc::new(RefCell::new(0));
    button.on_click({
        let hits = hits.clone();
        move || *hits.borrow_mut() += 1
    });

    button.click();
    button.click();
    assert_eq!(*hits.borrow(), 2);
    Ok(())
}

/// チェックボックスはネイティブの状態が変わり、変更後の値が通知される。
fn checkbox_toggle(ui: &Ui) -> Result<()> {
    let checkbox = ui.checkbox("有効")?;
    assert!(!checkbox.is_checked());

    let seen = Rc::new(RefCell::new(Vec::new()));
    checkbox.on_toggle({
        let seen = seen.clone();
        move |v| seen.borrow_mut().push(v)
    });

    checkbox.click();
    assert!(checkbox.is_checked(), "ネイティブ側の状態が変わること");
    checkbox.click();
    assert!(!checkbox.is_checked());
    assert_eq!(*seen.borrow(), vec![true, false]);

    checkbox.set_checked(true);
    assert!(checkbox.is_checked());
    Ok(())
}

/// スイッチはネイティブの状態が変わり、変更後の値が通知される。
fn toggle_switches_and_notifies(ui: &Ui) -> Result<()> {
    let toggle = ui.toggle("通知を受け取る")?;
    assert!(!toggle.is_on(), "既定は切れていること");

    let seen = Rc::new(RefCell::new(Vec::new()));
    toggle.on_toggle({
        let seen = seen.clone();
        move |v| seen.borrow_mut().push(v)
    });

    toggle.click();
    assert!(toggle.is_on(), "ネイティブ側の状態が変わること");
    toggle.click();
    assert!(!toggle.is_on());
    assert_eq!(*seen.borrow(), vec![true, false]);

    toggle.set_on(true);
    assert!(toggle.is_on());
    toggle.set_on(false);
    assert!(!toggle.is_on());
    assert_eq!(
        *seen.borrow(),
        vec![true, false],
        "プログラムからの変更は通知しない"
    );
    Ok(())
}

/// スイッチは `NSSwitch` そのもので、ラベルはとなりへ並ぶ。
/// 幅の余りはラベルが受け取り、スイッチは自分の大きさのままでいる。
fn toggle_places_a_native_switch_beside_its_label(ui: &Ui) -> Result<()> {
    let toggle = ui.toggle("通知を受け取る")?;
    let view = toggle.native_view();
    view.setFrameSize(NSSize::new(400.0, 40.0));
    view.layoutSubtreeIfNeeded();

    let subviews = view.subviews();
    let switch = (0..subviews.len())
        .find_map(|index| subviews.objectAtIndex(index).downcast::<NSSwitch>().ok())
        .expect("NSSwitch であること");
    let label = (0..subviews.len())
        .find_map(|index| subviews.objectAtIndex(index).downcast::<NSTextField>().ok())
        .expect("ラベルがあること");
    assert_eq!(label.stringValue().to_string(), "通知を受け取る");
    assert_eq!(switch, toggle.native_switch());

    let switch_frame = switch.frame();
    let label_frame = label.frame();
    assert!(
        switch_frame.origin.x < label_frame.origin.x,
        "スイッチが文字より左にあること"
    );
    assert!(
        switch_frame.size.width < 100.0,
        "スイッチは自分の大きさのままでいること: {}",
        switch_frame.size.width
    );

    toggle.set_enabled(false);
    assert!(!switch.isEnabled(), "スイッチが無効になること");
    Ok(())
}

/// 色ピッカーは `NSColorWell` そのもので、値はネイティブと往復する。
/// `set_value` は通知せず、`pick` は利用者の操作と同じく 1 回だけ通知する。
fn color_picker_round_trips(ui: &Ui) -> Result<()> {
    let picker = ui.color_picker()?;
    assert_eq!(picker.value(), Color::BLACK, "既定は黒であること");

    let well = picker.native_well();
    assert!(!well.supportsAlpha(), "透明度は扱わないこと");

    let seen = Rc::new(RefCell::new(Vec::new()));
    picker.on_change({
        let seen = seen.clone();
        move |v| seen.borrow_mut().push(v)
    });

    let orange = Color::rgb(0xff, 0x88, 0x00);
    picker.set_value(orange);
    assert_eq!(picker.value(), orange);
    let native = well
        .color()
        .colorUsingColorSpace(&NSColorSpace::sRGBColorSpace())
        .expect("sRGB へ変換できること");
    assert_eq!(
        Color::from_unit(
            native.redComponent(),
            native.greenComponent(),
            native.blueComponent(),
        ),
        orange,
        "ネイティブ側の色も変わること"
    );
    assert!(seen.borrow().is_empty(), "プログラムからの変更は通知しない");

    // 利用者がカラーパネルで色を変えたときの経路 (target/action)。
    let teal = Color::rgb(0x00, 0x80, 0x80);
    let (r, g, b) = teal.to_unit();
    well.setColor(&NSColor::colorWithSRGBRed_green_blue_alpha(r, g, b, 1.0));
    unsafe { well.sendAction_to(well.action(), well.target().as_deref()) };
    assert_eq!(picker.value(), teal);
    assert_eq!(*seen.borrow(), vec![teal], "ネイティブの操作が届くこと");

    // `pick` はアプリからでも利用者と同じく 1 回だけ通知する。
    let plum = Color::rgb(0x99, 0x33, 0x99);
    picker.pick(plum);
    assert_eq!(picker.value(), plum);
    assert_eq!(*seen.borrow(), vec![teal, plum]);

    picker.set_enabled(false);
    assert!(!well.isEnabled(), "色ウェルが無効になること");
    assert!(!well.isActive(), "カラーパネルとのつながりが切れること");
    Ok(())
}

/// カラーパネルはカタログ色 (`systemBlue` など) も返す。成分を読む前に
/// sRGB へ変換しているので、そのまま [`Color`] として受け取れる。
fn color_picker_reads_catalog_colors_as_srgb(ui: &Ui) -> Result<()> {
    let picker = ui.color_picker()?;
    let well = picker.native_well();

    // 色空間を持たないカタログ色を、ネイティブ側へ直接入れる。
    well.setColor(unsafe { &NSColor::systemBlueColor() });
    let value = picker.value();
    let expected = unsafe { NSColor::systemBlueColor() }
        .colorUsingColorSpace(&NSColorSpace::sRGBColorSpace())
        .expect("sRGB へ変換できること");
    assert_eq!(
        value,
        Color::from_unit(
            expected.redComponent(),
            expected.greenComponent(),
            expected.blueComponent(),
        ),
        "カタログ色でも成分が読めること"
    );
    assert_ne!(value, Color::BLACK, "変換に失敗して黒へ落ちていないこと");
    Ok(())
}

/// ラベルとテキスト入力の文字列がネイティブへ反映される。
fn text_round_trip(ui: &Ui) -> Result<()> {
    let label = ui.label("初期値")?;
    assert_eq!(label.text(), "初期値");
    label.set_text("こんにちは");
    assert_eq!(label.text(), "こんにちは");

    let input = ui.text_input("あ")?;
    assert_eq!(input.text(), "あ");
    input.set_text("あいう");
    assert_eq!(input.text(), "あいう");
    input.set_placeholder("名前");
    Ok(())
}

/// 複数行入力は改行を含む文字列をそのまま保つ。
fn text_area_round_trip(ui: &Ui) -> Result<()> {
    let area = ui.text_area("あ\nい")?;
    assert_eq!(area.text(), "あ\nい");

    area.set_text("一行目\n二行目\n三行目");
    assert_eq!(area.text(), "一行目\n二行目\n三行目");

    // NSScrollView に載っているので、外から見えるビューはスクロールビュー。
    let view = area.native_view();
    assert!(
        view.downcast_ref::<objc2_app_kit::NSScrollView>().is_some(),
        "複数行入力はスクロールビューごと 1 つのウィジェットであること"
    );

    let text_view = area.native_text_view();
    let default_color = text_view.textColor();
    area.set_enabled(false);
    assert!(!text_view.isEditable(), "無効なら編集できないこと");
    area.set_enabled(true);
    assert!(text_view.isEditable());
    assert_eq!(
        text_view.textColor(),
        default_color,
        "有効へ戻したら NSTextView の既定の文字色へ戻ること"
    );
    Ok(())
}

/// 打鍵のたびに通知が届き、プレースホルダーは中身の有無で出入りする。
fn text_area_notifies_while_typing(ui: &Ui) -> Result<()> {
    let area = ui.text_area("")?;
    area.set_placeholder("本文");
    let text_view = area.native_text_view();
    let placeholder = placeholder_label(&text_view).expect("プレースホルダーが重なっていること");
    assert_eq!(placeholder.stringValue().to_string(), "本文");
    assert!(!placeholder.isHidden(), "空のときは出ていること");

    let seen = Rc::new(RefCell::new(Vec::new()));
    area.on_change({
        let seen = seen.clone();
        move |text| seen.borrow_mut().push(text.to_string())
    });

    for chunk in ["あ", "\n", "い"] {
        type_into(&text_view, chunk);
    }
    assert_eq!(area.text(), "あ\nい");
    assert_eq!(
        *seen.borrow(),
        vec!["あ".to_string(), "あ\n".to_string(), "あ\nい".to_string()],
        "改行の入力でも通知されること"
    );
    assert!(placeholder.isHidden(), "入力後は隠れていること");

    area.set_text("");
    assert!(!placeholder.isHidden(), "空に戻したら出ること");
    assert_eq!(
        seen.borrow().len(),
        3,
        "set_text では通知しないこと (1 行入力と同じ)"
    );
    Ok(())
}

/// プレースホルダーの上をクリックしても、当たり判定は下の NSTextView へ通る。
fn text_area_placeholder_passes_clicks(ui: &Ui) -> Result<()> {
    let area = ui.text_area("")?;
    area.set_placeholder("本文");
    let text_view = area.native_text_view();
    // レイアウトを確定させてから、ラベルの中心を叩く。
    let view: &NSView = text_view.as_ref();
    view.layoutSubtreeIfNeeded();
    let placeholder = placeholder_label(&text_view).expect("プレースホルダーが重なっていること");
    let frame = placeholder.frame();
    let point = objc2_foundation::NSPoint::new(
        frame.origin.x + frame.size.width / 2.0,
        frame.origin.y + frame.size.height / 2.0,
    );
    let hit = view.hitTest(point);
    assert!(
        hit.is_some_and(|hit| hit.downcast_ref::<NSTextView>().is_some()),
        "プレースホルダーではなく NSTextView が受け取ること"
    );
    Ok(())
}

/// 高さは `set_sizing` の指定どおりになり、折り返しの幅は親に追従する。
fn text_area_follows_the_given_size(ui: &Ui) -> Result<()> {
    let stack = ui.stack(Orientation::Vertical)?;
    stack.set_sizing(Sizing::fill());
    let area = ui.text_area("あ")?;
    area.set_sizing(
        Sizing::new()
            .width(naui_core::Length::Fill)
            .height(naui_core::Length::Fixed(96.0)),
    );
    stack.append(&area);

    let root = stack.native_view();
    root.setFrameSize(NSSize::new(400.0, 400.0));
    root.layoutSubtreeIfNeeded();

    let frame = area.native_view().frame();
    assert!(
        (frame.size.height - 96.0).abs() < 1e-6 && (frame.size.width - 400.0).abs() < 1e-6,
        "指定した高さと親の幅になること: {frame:?}"
    );

    // 中の NSTextView は、枠のぶんだけ内側でスクロールビューに追従する。
    // 折り返しはこの幅で起きるので、親が広がれば折り返しも変わる。
    let inner = area.native_text_view().frame();
    assert!(
        inner.size.width > 0.0 && inner.size.width <= frame.size.width,
        "折り返しの幅がスクロールビューに収まること: {inner:?}"
    );

    root.setFrameSize(NSSize::new(240.0, 400.0));
    root.layoutSubtreeIfNeeded();
    let narrow = area.native_text_view().frame();
    assert!(
        narrow.size.width < inner.size.width,
        "親が狭くなれば折り返しの幅も狭くなること: {inner:?} -> {narrow:?}"
    );
    Ok(())
}

/// 複数行入力に重ねたプレースホルダーのラベルを取り出す。
fn placeholder_label(text_view: &NSTextView) -> Option<Retained<NSTextField>> {
    let view: &NSView = text_view.as_ref();
    let subviews = view.subviews();
    (0..subviews.len())
        .map(|index| subviews.objectAtIndex(index))
        .find_map(|subview| subview.downcast::<NSTextField>().ok())
}

/// IME や物理キーボードと同じ経路で文字を入れる。
fn type_into(text_view: &NSTextView, text: &str) {
    let string = NSString::from_str(text);
    unsafe {
        text_view.insertText_replacementRange(&string, NSRange::new(NSNotFound as usize, 0));
    }
}

/// スライダーは範囲でクランプされる。
fn slider_clamp(ui: &Ui) -> Result<()> {
    let slider = ui.slider(0.0, 10.0)?;
    slider.set_value(4.0);
    assert!((slider.value() - 4.0).abs() < 1e-9);

    slider.set_value(99.0);
    assert!(
        (slider.value() - 10.0).abs() < 1e-9,
        "NSSlider が最大値でクランプすること: {}",
        slider.value()
    );
    Ok(())
}

/// 進捗バーは 0.0..=1.0 に収まる。
fn progress_clamp(ui: &Ui) -> Result<()> {
    let bar = ui.progress_bar()?;
    bar.set_value(0.25);
    assert!((bar.value() - 0.25).abs() < 1e-9);
    bar.set_value(5.0);
    assert!((bar.value() - 1.0).abs() < 1e-9);
    Ok(())
}

/// 項目・未選択状態・通知の有無が NSPopUpButton と一致する。
fn combo_box_selection_round_trips(ui: &Ui) -> Result<()> {
    let combo = ui.combo_box()?;
    assert!(combo.is_empty());
    assert_eq!(combo.selected(), None);

    combo.set_items(&["東京", "大阪", "札幌"]);
    assert_eq!(combo.len(), 3);
    assert_eq!(combo.selected(), None, "項目の作り直し後は未選択");
    let native = combo.native_combo_box();
    assert_eq!(native.numberOfItems(), 3);
    assert_eq!(native.itemTitleAtIndex(1).to_string(), "大阪");

    let seen = Rc::new(RefCell::new(Vec::new()));
    combo.on_select({
        let seen = seen.clone();
        move |index| seen.borrow_mut().push(index)
    });

    combo.set_selected(1);
    assert_eq!(combo.selected(), Some(1));
    assert!(seen.borrow().is_empty(), "set_selected は通知しない");
    combo.set_selected(99);
    assert_eq!(combo.selected(), Some(1), "範囲外は無視する");

    combo.clear_selection();
    assert_eq!(combo.selected(), None);
    assert!(seen.borrow().is_empty(), "clear_selection は通知しない");

    combo.select(2);
    assert_eq!(combo.selected(), Some(2));
    assert_eq!(*seen.borrow(), vec![2]);
    combo.select(99);
    assert_eq!(*seen.borrow(), vec![2], "範囲外は通知もしない");

    // AppKit の target/action から届く実際の通知経路も確かめる。
    native.selectItemAtIndex(0);
    let target = native.target();
    unsafe { native.sendAction_to(native.action(), target.as_deref()) };
    assert_eq!(*seen.borrow(), vec![2, 0]);

    combo.set_enabled(false);
    assert!(!native.isEnabled());
    combo.set_items(&["那覇"]);
    assert_eq!(combo.selected(), None);
    assert_eq!(*seen.borrow(), vec![2, 0], "再構築は通知しない");
    Ok(())
}

/// 文字列・候補・通知の有無が NSComboBox と一致する。
fn editable_combo_box_text_round_trips(ui: &Ui) -> Result<()> {
    let combo = ui.editable_combo_box()?;
    assert!(combo.is_empty());
    assert_eq!(combo.text(), "");
    assert_eq!(combo.selected(), None);

    combo.set_items(&["東京", "大阪", "札幌"]);
    assert_eq!(combo.len(), 3);
    assert_eq!(combo.text(), "", "候補を入れても文字列は変わらない");
    let native = combo.native_combo_box();
    assert_eq!(native.numberOfItems(), 3);

    let seen = Rc::new(RefCell::new(Vec::new()));
    combo.on_change({
        let seen = seen.clone();
        move |text: &str| seen.borrow_mut().push(text.to_string())
    });

    combo.set_text("京都");
    assert_eq!(combo.text(), "京都");
    assert_eq!(combo.selected(), None, "候補に無い文字列も持てる");
    assert!(seen.borrow().is_empty(), "set_text は通知しない");

    combo.set_selected(1);
    assert_eq!(combo.text(), "大阪");
    assert_eq!(combo.selected(), Some(1));
    assert_eq!(native.indexOfSelectedItem(), 1, "一覧側の選択もそろう");
    assert!(seen.borrow().is_empty(), "set_selected は通知しない");
    combo.set_selected(99);
    assert_eq!(combo.text(), "大阪", "範囲外は無視する");

    combo.clear();
    assert_eq!(combo.text(), "");
    assert!(seen.borrow().is_empty(), "clear は通知しない");

    combo.select(2);
    assert_eq!(combo.text(), "札幌");
    assert_eq!(*seen.borrow(), vec!["札幌".to_string()]);
    combo.select(99);
    assert_eq!(seen.borrow().len(), 1, "範囲外は通知もしない");

    // 打ち込んだときに AppKit が送る通知の経路。
    type_into_field(&native, "な");
    assert_eq!(combo.text(), "な");
    assert_eq!(combo.selected(), None);
    assert_eq!(
        *seen.borrow(),
        vec!["札幌".to_string(), "な".to_string()],
        "打鍵は 1 回だけ通知する"
    );

    // 一覧から候補を選んだときの経路。
    pick_natively(&native, 0);
    assert_eq!(combo.text(), "東京", "選んだ候補が欄へ入る");
    assert_eq!(combo.selected(), Some(0));
    assert_eq!(
        *seen.borrow(),
        vec!["札幌".to_string(), "な".to_string(), "東京".to_string()],
        "選択の通知が二重に届かない"
    );

    combo.set_enabled(false);
    assert!(!native.isEnabled());

    combo.set_items(&["那覇"]);
    assert_eq!(combo.len(), 1);
    assert_eq!(combo.text(), "東京", "候補を作り直しても文字列は残る");
    assert_eq!(combo.selected(), None, "一致する候補が無くなる");
    assert_eq!(seen.borrow().len(), 3, "作り直しは通知しない");
    Ok(())
}

/// 通知の最中でも同じ入力欄を操作し、コールバックを差し替えられる。
fn editable_combo_box_callback_is_reentrant(ui: &Ui) -> Result<()> {
    let combo = ui.editable_combo_box()?;
    combo.set_items(&["春", "夏", "秋"]);

    let first = Rc::new(RefCell::new(Vec::new()));
    let replacement = Rc::new(RefCell::new(Vec::new()));
    combo.on_change({
        let combo = combo.clone();
        let first = first.clone();
        let replacement = replacement.clone();
        move |text: &str| {
            first.borrow_mut().push(text.to_string());
            combo.set_items(&["朝", "昼"]);
            combo.set_selected(1);
            combo.on_change({
                let replacement = replacement.clone();
                move |text: &str| replacement.borrow_mut().push(text.to_string())
            });
        }
    });

    combo.select(0);
    assert_eq!(*first.borrow(), vec!["春".to_string()]);
    assert_eq!(combo.len(), 2);
    assert_eq!(combo.text(), "昼");

    combo.select(0);
    assert_eq!(first.borrow().len(), 1, "古い通知先は外れること");
    assert_eq!(*replacement.borrow(), vec!["朝".to_string()]);
    Ok(())
}

/// `NSComboBox` は一覧を絞り込まないので、絞りたいアプリは `on_change` の中で
/// 候補を入れ替える。README で案内しているこの手順が実際に通ることを確かめる。
fn editable_combo_box_items_can_be_narrowed_while_typing(ui: &Ui) -> Result<()> {
    let combo = ui.editable_combo_box()?;
    let all = ["東京", "東大阪", "大阪", "札幌"];
    combo.set_items(&all);

    combo.on_change({
        let combo = combo.clone();
        move |text: &str| {
            let narrowed: Vec<&str> = all
                .iter()
                .copied()
                .filter(|item| item.starts_with(text))
                .collect();
            combo.set_items(&narrowed);
        }
    });

    let native = combo.native_combo_box();
    type_into_field(&native, "東");
    assert_eq!(combo.len(), 2, "打った文字で候補を絞れる");
    assert_eq!(native.numberOfItems(), 2, "絞り込みがネイティブにも届く");
    assert_eq!(combo.text(), "東", "候補を入れ替えても文字列は動かない");
    assert_eq!(combo.selected(), None, "一致する候補はまだ無い");

    type_into_field(&native, "東大阪");
    assert_eq!(combo.len(), 1);
    assert_eq!(combo.text(), "東大阪");
    assert_eq!(combo.selected(), Some(0), "残った 1 件と一致する");

    // 絞り込みで候補が空になっても、打った文字はそのまま残る。
    type_into_field(&native, "京都");
    assert!(combo.is_empty());
    assert_eq!(combo.text(), "京都");
    assert_eq!(combo.selected(), None);
    Ok(())
}

/// 通知の最中でも同じコンボボックスを操作し、コールバックを差し替えられる。
fn combo_box_callback_is_reentrant(ui: &Ui) -> Result<()> {
    let combo = ui.combo_box()?;
    combo.set_items(&["春", "夏", "秋"]);

    let first = Rc::new(RefCell::new(Vec::new()));
    let replacement = Rc::new(RefCell::new(Vec::new()));
    combo.on_select({
        let combo = combo.clone();
        let first = first.clone();
        let replacement = replacement.clone();
        move |index| {
            first.borrow_mut().push(index);
            combo.set_items(&["朝", "昼"]);
            combo.set_selected(1);
            combo.on_select({
                let replacement = replacement.clone();
                move |index| replacement.borrow_mut().push(index)
            });
        }
    });

    combo.select(0);
    assert_eq!(*first.borrow(), vec![0]);
    assert_eq!(combo.len(), 2);
    assert_eq!(combo.selected(), Some(1));

    combo.select(0);
    assert_eq!(*first.borrow(), vec![0], "古い通知先は外れること");
    assert_eq!(*replacement.borrow(), vec![0]);
    Ok(())
}

/// 項目・未選択状態・通知の有無が NSButton のラジオ型と一致する。
fn radio_group_selection_round_trips(ui: &Ui) -> Result<()> {
    let radio = ui.radio_group()?;
    assert!(radio.is_empty());
    assert_eq!(radio.selected(), None);

    radio.set_items(&["小", "中", "大"]);
    assert_eq!(radio.len(), 3);
    assert_eq!(radio.selected(), None, "項目の作り直し後は未選択");
    let buttons = radio.native_buttons();
    assert_eq!(buttons.len(), 3);
    assert_eq!(buttons[1].title().to_string(), "中");

    let seen = Rc::new(RefCell::new(Vec::new()));
    radio.on_select({
        let seen = seen.clone();
        move |index| seen.borrow_mut().push(index)
    });

    radio.set_selected(1);
    assert_eq!(radio.selected(), Some(1));
    assert!(seen.borrow().is_empty(), "set_selected は通知しない");
    radio.set_selected(99);
    assert_eq!(radio.selected(), Some(1), "範囲外は無視する");

    radio.clear_selection();
    assert_eq!(radio.selected(), None);
    assert!(seen.borrow().is_empty(), "clear_selection は通知しない");

    radio.select(2);
    assert_eq!(radio.selected(), Some(2));
    assert_eq!(*seen.borrow(), vec![2]);
    radio.select(99);
    assert_eq!(*seen.borrow(), vec![2], "範囲外は通知もしない");

    radio.set_enabled(false);
    assert!(buttons.iter().all(|button| !button.isEnabled()));
    // 無効のまま項目を作り直しても、その指定は引き継がれる。
    radio.set_items(&["単一"]);
    assert_eq!(radio.selected(), None);
    assert!(!radio.native_buttons()[0].isEnabled());
    assert_eq!(*seen.borrow(), vec![2], "再構築は通知しない");
    Ok(())
}

/// AppKit の target/action から届く経路でも、点くのは 1 つだけ。
fn radio_group_click_selects_one(ui: &Ui) -> Result<()> {
    let radio = ui.radio_group()?;
    radio.set_items(&["赤", "緑", "青"]);
    let seen = Rc::new(RefCell::new(Vec::new()));
    radio.on_select({
        let seen = seen.clone();
        move |index| seen.borrow_mut().push(index)
    });

    let buttons = radio.native_buttons();
    unsafe { buttons[2].performClick(None) };
    assert_eq!(radio.selected(), Some(2));
    assert_eq!(*seen.borrow(), vec![2]);

    unsafe { buttons[0].performClick(None) };
    assert_eq!(radio.selected(), Some(0), "選び直すと前のものは消える");
    assert_eq!(*seen.borrow(), vec![2, 0]);
    assert_eq!(
        buttons
            .iter()
            .filter(|button| button.state() != NSControlStateValueOff)
            .count(),
        1,
        "点いているのは常に 1 つ"
    );
    Ok(())
}

/// 通知の最中でも同じラジオグループを操作し、コールバックを差し替えられる。
fn radio_group_callback_is_reentrant(ui: &Ui) -> Result<()> {
    let radio = ui.radio_group()?;
    radio.set_items(&["春", "夏", "秋"]);

    let first = Rc::new(RefCell::new(Vec::new()));
    let replacement = Rc::new(RefCell::new(Vec::new()));
    radio.on_select({
        let radio = radio.clone();
        let first = first.clone();
        let replacement = replacement.clone();
        move |index| {
            first.borrow_mut().push(index);
            radio.set_items(&["朝", "昼"]);
            radio.set_selected(1);
            radio.on_select({
                let replacement = replacement.clone();
                move |index| replacement.borrow_mut().push(index)
            });
        }
    });

    radio.select(0);
    assert_eq!(*first.borrow(), vec![0]);
    assert_eq!(radio.len(), 2);
    assert_eq!(radio.selected(), Some(1));

    radio.select(0);
    assert_eq!(*first.borrow(), vec![0], "古い通知先は外れること");
    assert_eq!(*replacement.borrow(), vec![0]);
    Ok(())
}

/// スタックへ追加した子は、ハンドルを捨ててもコールバックが生き続ける。
/// `Stack` は後から子を差し込み、外し、空にできる。
fn stack_inserts_and_removes_children(ui: &Ui) -> Result<()> {
    let stack = ui.stack(Orientation::Vertical)?;
    let first = ui.label("A")?;
    let last = ui.label("C")?;
    stack.append(&first);
    stack.append(&last);

    // 間へ差し込む。
    let middle = ui.label("B")?;
    stack.insert(1, &middle);
    assert_eq!(stack.len(), 3);
    assert_eq!(arranged_labels(&stack), ["A", "B", "C"]);

    // 範囲外の index は末尾へ足す。
    stack.insert(99, &ui.label("D")?);
    assert_eq!(arranged_labels(&stack), ["A", "B", "C", "D"]);

    // 外すと NSStackView からも消える。
    stack.remove(1);
    assert_eq!(stack.len(), 3);
    assert_eq!(arranged_labels(&stack), ["A", "C", "D"]);
    assert!(
        unsafe { middle.native_view().superview() }.is_none(),
        "外した子はビュー階層からも抜けること"
    );

    // 範囲外の index は何もしない。
    stack.remove(9);
    assert_eq!(stack.len(), 3);

    // 主軸の Fill を持つ子を入れても、外した後にまた足せる。
    let filler = ui.label("E")?;
    filler.set_sizing(Sizing::new().height(Length::Fill));
    stack.append(&filler);
    assert_eq!(arranged_labels(&stack), ["A", "C", "D", "E"]);
    stack.remove(3);
    assert_eq!(arranged_labels(&stack), ["A", "C", "D"]);

    // 外したぶんは AppKit の計算する大きさにも出る。
    let view = stack.native_view();
    view.layoutSubtreeIfNeeded();
    let before = view.fittingSize().height;
    stack.remove(0);
    view.layoutSubtreeIfNeeded();
    let after = view.fittingSize().height;
    assert!(
        after < before,
        "子を外したらスタックが縮むこと: 外す前 {before} / 外した後 {after}"
    );
    stack.insert(0, &ui.label("A")?);

    stack.clear();
    assert_eq!(stack.len(), 0);
    assert!(stack.is_empty());
    assert!(arranged_labels(&stack).is_empty());
    assert!(unsafe { first.native_view().superview() }.is_none());

    // 空にした後もふつうに積める。
    stack.append(&ui.label("F")?);
    assert_eq!(arranged_labels(&stack), ["F"]);
    Ok(())
}

/// `NSStackView` に並んでいるラベルの文字を、並び順で取り出す。
fn arranged_labels(stack: &naui_macos::Stack) -> Vec<String> {
    let native: Retained<objc2_app_kit::NSStackView> = stack
        .native_view()
        .downcast()
        .expect("NSStackView であること");
    let arranged = native.arrangedSubviews();
    (0..arranged.len())
        .filter_map(|index| arranged.objectAtIndex(index).downcast::<NSTextField>().ok())
        .map(|field| field.stringValue().to_string())
        .collect()
}

/// `Grid` はマスを指定して子を外せる。
fn grid_removes_children_by_cell(ui: &Ui) -> Result<()> {
    let grid = ui.grid()?;
    let name = ui.label("名前")?;
    let field = ui.text_input("")?;
    grid.attach(&name, GridCell::new(0, 0));
    grid.attach(&field, GridCell::new(1, 0));
    assert_eq!(grid.len(), 2);

    grid.remove(GridCell::new(0, 0));
    assert_eq!(grid.len(), 1);
    assert!(
        unsafe { name.native_view().superview() }.is_none(),
        "外した子はビュー階層からも抜けること"
    );
    assert!(
        unsafe { field.native_view().superview() }.is_some(),
        "他のマスの子は残ること"
    );

    // 何も無いマスを指定しても何も起きない。
    grid.remove(GridCell::new(0, 0));
    assert_eq!(grid.len(), 1);

    // 空いたマスへまた置ける。
    let renamed = ui.label("表示名")?;
    grid.attach(&renamed, GridCell::new(0, 0));
    assert_eq!(grid.len(), 2);

    // replace は「そのマスだけ」差し替える (他のマスは残る)。
    let replaced = ui.label("別名")?;
    grid.replace(&replaced, GridCell::new(0, 0));
    assert_eq!(grid.len(), 2, "replace は他のマスの子を外さないこと");
    assert!(unsafe { renamed.native_view().superview() }.is_none());
    assert!(unsafe { field.native_view().superview() }.is_some());

    grid.clear();
    assert_eq!(grid.len(), 0);
    assert!(grid.is_empty());
    assert!(unsafe { field.native_view().superview() }.is_none());

    // 行と列の指定は残るので、そのまま置き直せる。
    grid.attach(&ui.label("再")?, GridCell::new(0, 0));
    assert_eq!(grid.len(), 1);
    Ok(())
}

/// 結合したマス (span を持つ子) を外しても、結合そのものは残る。
///
/// `NSGridView` に結合を解く API は無く、行や列ごと外すこともできない
/// (結合をまたぐ行・列の削除は AppKit が例外を投げる)。そこで naui は
/// **結合した範囲を「1 つのマス」として扱いきる**ことで、見え方を決めている。
fn grid_keeps_merges_after_the_child_is_removed(ui: &Ui) -> Result<()> {
    let grid = ui.grid()?;
    grid.set_column_track(0, Track::Fixed(120.0));
    grid.set_column_track(1, Track::Fixed(80.0));

    let wide = ui.label("2 列にまたがるセル")?;
    grid.attach(&wide, GridCell::new(0, 0).span(2, 1));
    let below = ui.label("下の行")?;
    grid.attach(&below, GridCell::new(0, 1));
    assert!(
        shares_cell(&grid, (0, 0), (1, 0)),
        "span を持つ子はマスを結合すること"
    );

    let root = grid.native_view();
    root.setFrameSize(NSSize::new(400.0, 200.0));
    root.layoutSubtreeIfNeeded();
    let normal = below.native_view().frame();

    grid.remove(GridCell::new(0, 0));
    assert!(
        shares_cell(&grid, (0, 0), (1, 0)),
        "外しても結合は残ること (AppKit に解く手段が無い)"
    );

    // それでも、1 マスぶんの子を置いたときの位置と大きさは他の行と変わらない。
    let single = ui.label("下の行")?;
    grid.attach(&single, GridCell::new(0, 0));
    root.layoutSubtreeIfNeeded();
    let placed = single.native_view().frame();
    assert!(
        (placed.origin.x - normal.origin.x).abs() < 0.5
            && (placed.size.width - normal.size.width).abs() < 0.5,
        "結合の跡でも 1 マスと同じ位置・大きさで出ること: 跡 {placed:?} / ふつうの行 {normal:?}"
    );

    // ただし `Fill` の子は、1 マスではなく結合した範囲いっぱいに広がる。
    let stretched = ui.text_input("")?;
    stretched.set_sizing(Sizing::fill_width());
    grid.attach(&stretched, GridCell::new(0, 0));
    let plain = ui.text_input("")?;
    plain.set_sizing(Sizing::fill_width());
    grid.attach(&plain, GridCell::new(0, 2));
    root.layoutSubtreeIfNeeded();
    let merged_width = stretched.native_view().frame().size.width;
    let single_width = plain.native_view().frame().size.width;
    assert!(
        merged_width > single_width + 40.0,
        "Fill の子は結合した範囲まで広がること: 結合の跡 {merged_width} / ふつうの行 {single_width}"
    );

    // 結合した範囲は 1 つのマスなので、範囲内の別の位置へ置くと前の子が外れる。
    let other = ui.label("2 列目")?;
    grid.attach(&other, GridCell::new(1, 0));
    assert_eq!(grid.len(), 3, "重ねて置いた分だけ増えないこと");
    assert!(
        unsafe { stretched.native_view().superview() }.is_none(),
        "前の子はビュー階層からも外れること (宙に浮いたまま残さない)"
    );
    assert!(unsafe { other.native_view().superview() }.is_some());
    assert!(
        unsafe { below.native_view().superview() }.is_some(),
        "結合と関係ないマスの子は残ること"
    );
    Ok(())
}

/// 2 つの場所が同じ `NSGridCell` を指しているか (= 結合されているか)。
fn shares_cell(grid: &naui_macos::Grid, left: (isize, isize), right: (isize, isize)) -> bool {
    let native: Retained<objc2_app_kit::NSGridView> = grid
        .native_view()
        .downcast()
        .expect("NSGridView であること");
    let first = native.cellAtColumnIndex_rowIndex(left.0, left.1);
    let second = native.cellAtColumnIndex_rowIndex(right.0, right.1);
    Retained::as_ptr(&first) == Retained::as_ptr(&second)
}

/// タブは後から外せる。選択の寄せ直しは通知しない。
fn tabs_remove_and_clear(ui: &Ui) -> Result<()> {
    let tabs = ui.tabs()?;
    let first = ui.label("1 枚目")?;
    tabs.add_tab("A", &first);
    tabs.add_tab("B", &ui.label("2 枚目")?);
    tabs.add_tab("C", &ui.label("3 枚目")?);
    assert_eq!(tabs.len(), 3);

    let seen = Rc::new(RefCell::new(Vec::new()));
    tabs.on_select({
        let seen = seen.clone();
        move |index| seen.borrow_mut().push(index)
    });

    tabs.set_selected(2);
    assert_eq!(tabs.selected(), Some(2));

    // 選択より前のタブを外すと、選択は同じタブへ付いていく。
    tabs.remove_tab(0);
    assert_eq!(tabs.len(), 2);
    assert_eq!(tabs.selected(), Some(1));
    assert!(seen.borrow().is_empty(), "外したことは通知しないこと");
    assert!(
        unsafe { first.native_view().superview() }.is_none(),
        "外したタブの中身はビュー階層からも抜けること"
    );

    // 選択中のタブを外すと、環境が近くのタブを選び直す (通知はしない)。
    tabs.remove_tab(1);
    assert_eq!(tabs.len(), 1);
    assert_eq!(tabs.selected(), Some(0));
    assert!(seen.borrow().is_empty());

    // 範囲外は何もしない。
    tabs.remove_tab(5);
    assert_eq!(tabs.len(), 1);

    tabs.clear();
    assert_eq!(tabs.len(), 0);
    assert!(tabs.is_empty());
    assert_eq!(tabs.selected(), None);
    assert!(seen.borrow().is_empty());

    // 空にした後もふつうに足せる。
    tabs.add_tab("D", &ui.label("新しい 1 枚目")?);
    assert_eq!(tabs.len(), 1);
    assert_eq!(tabs.selected(), Some(0));
    Ok(())
}

fn stack_keeps_children(ui: &Ui) -> Result<()> {
    let stack = ui.stack(Orientation::Vertical)?;
    stack.set_spacing(8.0);
    stack.set_padding(Padding::all(16.0));

    let hits = Rc::new(RefCell::new(0));
    {
        let button = ui.button("押す")?;
        button.on_click({
            let hits = hits.clone();
            move || *hits.borrow_mut() += 1
        });
        stack.append(&button);
        // ここで button ハンドルは落ちるが、stack が保持している。
    }
    assert_eq!(stack.len(), 1);

    // ネイティブのビュー階層から取り出してクリックする。
    let view = stack.native_view();
    let subviews = view.subviews();
    let control = (0..subviews.len())
        .find_map(|index| {
            subviews
                .objectAtIndex(index)
                .downcast::<objc2_app_kit::NSButton>()
                .ok()
        })
        .expect("NSButton であること");
    unsafe { control.performClick(None) };

    assert_eq!(
        *hits.borrow(),
        1,
        "ハンドルを捨てた後もクロージャが呼ばれること"
    );
    Ok(())
}

/// ナビバーは NSSegmentedControl の選択と往復し、選択を通知する。
fn navbar_selection(ui: &Ui) -> Result<()> {
    let navbar = ui.navbar("naui")?;
    assert_eq!(navbar.title(), "naui");
    navbar.set_items(&NavItem::list(["ホーム", "検索", "設定"]));
    assert_eq!(navbar.len(), 3);

    let view = navbar.native_view();
    view.setFrameSize(NSSize::new(500.0, 40.0));
    view.layoutSubtreeIfNeeded();
    let subviews = view.subviews();
    let title = (0..subviews.len())
        .find_map(|index| subviews.objectAtIndex(index).downcast::<NSTextField>().ok())
        .expect("見出しがあること");
    let segments = (0..subviews.len())
        .find_map(|index| {
            subviews
                .objectAtIndex(index)
                .downcast::<NSSegmentedControl>()
                .ok()
        })
        .expect("セグメントがあること");
    assert!(
        title.frame().size.width < 200.0,
        "見出しが余白で広がらないこと"
    );
    assert!(
        segments.frame().size.width < 250.0,
        "セグメントが余白で広がらないこと"
    );

    let seen = Rc::new(RefCell::new(Vec::new()));
    navbar.on_select({
        let seen = seen.clone();
        move |index| seen.borrow_mut().push(index)
    });

    navbar.select(2);
    assert_eq!(navbar.selected(), Some(2), "ネイティブ側の選択が変わること");
    assert_eq!(*seen.borrow(), vec![2]);

    // set_selected は通知しない。
    navbar.set_selected(0);
    assert_eq!(navbar.selected(), Some(0));
    assert_eq!(*seen.borrow(), vec![2]);

    // 範囲外は無視する。
    navbar.select(9);
    assert_eq!(navbar.selected(), Some(0));
    assert_eq!(*seen.borrow(), vec![2]);
    Ok(())
}

/// ドックは項目を持ち、無効な項目も並べられる。
fn dock_items(ui: &Ui) -> Result<()> {
    let dock = ui.dock()?;
    assert!(dock.is_empty());
    dock.set_items(&[
        NavItem::new("ホーム"),
        NavItem::new("履歴").enabled(false),
        NavItem::new("設定"),
    ]);
    assert_eq!(dock.len(), 3);

    let last = Rc::new(RefCell::new(None));
    dock.on_select({
        let last = last.clone();
        move |index| *last.borrow_mut() = Some(index)
    });
    dock.select(2);
    assert_eq!(*last.borrow(), Some(2));

    // 項目を入れ替えたら選択も外れる。
    dock.set_items(&NavItem::list(["ホーム", "設定"]));
    assert_eq!(dock.len(), 2);
    assert_eq!(dock.selected(), None);
    dock.set_items(&[]);
    assert!(dock.is_empty());
    assert_eq!(dock.selected(), None);
    Ok(())
}

/// タブは中身のウィジェットを保持し、選択で切り替わる。
fn tabs_selection(ui: &Ui) -> Result<()> {
    let tabs = ui.tabs()?;
    let hits = Rc::new(RefCell::new(0));
    {
        let first = ui.label("1 枚目")?;
        let second = ui.button("2 枚目")?;
        second.on_click({
            let hits = hits.clone();
            move || *hits.borrow_mut() += 1
        });
        tabs.add_tab("概要", &first);
        tabs.add_tab("詳細", &second);
        // ここでハンドルは落ちるが、タブが保持している。
    }
    assert_eq!(tabs.len(), 2);
    assert_eq!(tabs.selected(), Some(0), "最初のタブが選ばれていること");

    let seen = Rc::new(RefCell::new(Vec::new()));
    tabs.on_select({
        let seen = seen.clone();
        move |index| seen.borrow_mut().push(index)
    });

    tabs.select(1);
    assert_eq!(tabs.selected(), Some(1));
    assert_eq!(*seen.borrow(), vec![1], "1 回だけ通知されること");

    tabs.set_selected(0);
    assert_eq!(tabs.selected(), Some(0));
    assert_eq!(*seen.borrow(), vec![1], "set_selected は通知しないこと");
    Ok(())
}

/// メニューは選択したボタンだけが押し込まれる。
fn menu_selection(ui: &Ui) -> Result<()> {
    let menu = ui.menu()?;
    menu.set_items(&NavItem::list(["受信箱", "送信済み", "ゴミ箱"]));
    assert_eq!(menu.len(), 3);
    assert_eq!(menu.selected(), None);

    let seen = Rc::new(RefCell::new(Vec::new()));
    menu.on_select({
        let seen = seen.clone();
        move |index| seen.borrow_mut().push(index)
    });

    menu.select(1);
    assert_eq!(menu.selected(), Some(1));
    assert_eq!(*seen.borrow(), vec![1]);

    // ネイティブのボタン状態も 1 つだけ On になる。
    let view = menu.native_view();
    let subviews = view.subviews();
    assert_eq!(subviews.len(), 3, "NSStackView に 3 つ並んでいること");
    let on_count = (0..subviews.len())
        .filter_map(|i| {
            subviews
                .objectAtIndex(i)
                .downcast::<objc2_app_kit::NSButton>()
                .ok()
        })
        .filter(|button| button.state() != objc2_app_kit::NSControlStateValueOff)
        .count();
    assert_eq!(on_count, 1, "押し込まれているのは 1 つだけであること");

    // 項目を作り直すと選択は外れる。
    menu.set_items(&NavItem::list(["受信箱"]));
    assert_eq!(menu.selected(), None);
    assert_eq!(menu.len(), 1);
    Ok(())
}

/// パンくずは末尾を現在地とし、クリックでその位置を通知する。
fn breadcrumbs_path(ui: &Ui) -> Result<()> {
    let crumbs = ui.breadcrumbs()?;
    assert!(crumbs.is_empty());

    crumbs.set_items(&NavItem::list(["ホーム", "設定", "通知"]));
    assert_eq!(crumbs.len(), 3);
    assert_eq!(crumbs.selected(), Some(2), "末尾が現在地であること");

    let seen = Rc::new(RefCell::new(Vec::new()));
    crumbs.on_select({
        let seen = seen.clone();
        move |index| seen.borrow_mut().push(index)
    });

    crumbs.select(0);
    assert_eq!(crumbs.selected(), Some(0));
    assert_eq!(*seen.borrow(), vec![0]);

    // 階層を差し替えると現在地も末尾へ戻る。
    crumbs.set_items(&NavItem::list(["ホーム", "設定"]));
    assert_eq!(crumbs.selected(), Some(1));
    Ok(())
}

/// ページ送りは端で止まる。
fn pagination_steps(ui: &Ui) -> Result<()> {
    let pager = ui.pagination(3)?;
    assert_eq!(pager.page_count(), 3);
    assert_eq!(pager.page(), 0);

    let seen = Rc::new(RefCell::new(Vec::new()));
    pager.on_change({
        let seen = seen.clone();
        move |page| seen.borrow_mut().push(page)
    });

    pager.go_previous();
    assert_eq!(pager.page(), 0, "先頭より前へは進まないこと");
    assert!(seen.borrow().is_empty());

    pager.go_next();
    pager.go_next();
    assert_eq!(pager.page(), 2);
    pager.go_next();
    assert_eq!(pager.page(), 2, "末尾より後ろへは進まないこと");
    assert_eq!(*seen.borrow(), vec![1, 2]);

    pager.set_page_count(5);
    assert_eq!(pager.page_count(), 5);
    assert_eq!(pager.page(), 0, "ページ数を変えたら先頭に戻ること");
    Ok(())
}

/// リンクはネイティブのクリックでクロージャが呼ばれる。
fn link_click(ui: &Ui) -> Result<()> {
    // href を空にしておく (テスト中にブラウザを開かないため)。
    let link = ui.link("naui", "")?;
    assert_eq!(link.text(), "naui");
    assert_eq!(link.href(), "");

    let hits = Rc::new(RefCell::new(0));
    link.on_click({
        let hits = hits.clone();
        move || *hits.borrow_mut() += 1
    });

    link.click();
    link.click();
    assert_eq!(*hits.borrow(), 2);

    link.set_text("naui のリポジトリ");
    assert_eq!(link.text(), "naui のリポジトリ");
    Ok(())
}

/// テーマはシステム追従を既定とし、実行中に固定テーマへ切り替えられる。
fn theme_switch(ui: &Ui) -> Result<()> {
    assert_eq!(ui.theme(), Theme::System);
    ui.set_theme(Theme::Dark)?;
    assert_eq!(ui.theme(), Theme::Dark);
    ui.set_theme(Theme::Light)?;
    assert_eq!(ui.theme(), Theme::Light);
    ui.set_theme(Theme::System)?;
    assert_eq!(ui.theme(), Theme::System);
    Ok(())
}

/// ウィンドウは生成・設定・クローズまで通る (二重解放しない)。
fn window_lifecycle(ui: &Ui) -> Result<()> {
    let window = ui.window("タイトル", 420.0, 260.0)?;
    assert_eq!(window.title(), "タイトル");
    window.set_title("別のタイトル");
    assert_eq!(window.title(), "別のタイトル");

    let stack = ui.stack(Orientation::Vertical)?;
    stack.append(&ui.label("中身")?);
    window.set_child(&stack);

    window.show();
    assert!(window.is_visible());
    window.close();
    assert!(!window.is_visible());
    Ok(())
}

/// 大きさの指定が AppKit の制約になり、値まで届く。
fn sizing_constraints(ui: &Ui) -> Result<()> {
    let button = ui.button("固定")?;
    button.set_sizing(Sizing::fixed(160.0, 44.0).min_width(80.0));

    let view = button.native_view();
    let sizes = naui_constraints(&view);
    assert_eq!(sizes.len(), 3, "幅・高さ・最小幅の 3 本が付くこと");
    assert!(
        sizes.iter().any(|c| (c.constant() - 160.0).abs() < 1e-9),
        "指定した幅が制約の定数になること"
    );

    // 制約は AppKit 側で本当に効く (fitting size に出る)。
    let fitting = view.fittingSize();
    assert!(
        (fitting.width - 160.0).abs() < 1e-6 && (fitting.height - 44.0).abs() < 1e-6,
        "AppKit が計算した大きさ: {fitting:?}"
    );
    Ok(())
}

/// 指定し直しても制約は積み上がらず、AppKit 内部の制約も壊さない。
fn sizing_is_replaced(ui: &Ui) -> Result<()> {
    let label = ui.label("文字")?;
    let view = label.native_view();
    // AppKit が内部で付ける制約 (intrinsic content size 用) の本数。
    let before = view.constraints().len();

    // 文字幅より広い値を順に指定する (狭めると圧縮抵抗と競合するため)。
    label.set_sizing(Sizing::fixed(200.0, 20.0));
    label.set_sizing(Sizing::fixed(220.0, 30.0));
    label.set_sizing(Sizing::fixed(240.0, 40.0));

    let mine = naui_constraints(&view);
    assert_eq!(mine.len(), 2, "最後の指定だけが残ること");
    assert!(
        mine.iter().all(|c| c.isActive()),
        "残った制約は有効であること"
    );
    assert_eq!(
        view.constraints().len(),
        before + 2,
        "AppKit 自身の制約は外れていないこと"
    );

    // 制約が効くのは alignment rect なので、frame は数ピクセル広くなりうる
    // (NSTextField はフォーカスリング用の余白を持つ)。
    let fitting = view.fittingSize();
    assert!(
        (fitting.width - 240.0).abs() <= 5.0,
        "最後に指定した幅になること: {fitting:?}"
    );
    Ok(())
}

/// 縦スタックの中で幅 Fill を指定した子は、余白を除いた親の幅まで広がる。
fn stack_fill_cross(ui: &Ui) -> Result<()> {
    let stack = ui.stack(Orientation::Vertical)?;
    stack.set_padding(Padding::all(10.0));
    let wide = ui.button("広がる")?;
    wide.set_sizing(Sizing::fill_width());
    let narrow = ui.button("広がらない")?;
    stack.append(&wide);
    stack.append(&narrow);

    let root = stack.native_view();
    root.setFrameSize(NSSize::new(300.0, 200.0));
    root.layoutSubtreeIfNeeded();

    let filled = wide.native_view().frame();
    assert!(
        (filled.size.width - 280.0).abs() < 1e-6,
        "左右の余白 10px を除いた幅になること: {filled:?}"
    );
    assert!(
        narrow.native_view().frame().size.width < 280.0,
        "指定していない子は中身の幅のままであること"
    );

    // 余白を変えると、広がっている子も追従する。
    stack.set_padding(Padding::all(30.0));
    root.layoutSubtreeIfNeeded();
    assert!(
        (wide.native_view().frame().size.width - 240.0).abs() < 1e-6,
        "余白の変更に追従すること: {:?}",
        wide.native_view().frame()
    );
    Ok(())
}

/// 横スタックの `Auto` の子は、余った主軸の空間を勝手に受け取らない。
fn stack_auto_does_not_fill(ui: &Ui) -> Result<()> {
    let stack = ui.stack(Orientation::Horizontal)?;
    stack.set_spacing(12.0);
    let first = ui.button("短")?;
    let second = ui.button("少し長い項目")?;
    stack.append(&first);
    stack.append(&second);

    let root = stack.native_view();
    root.setFrameSize(NSSize::new(500.0, 60.0));
    root.layoutSubtreeIfNeeded();

    let first_frame = first.native_view().frame();
    let second_frame = second.native_view().frame();
    assert!(
        first_frame.size.width < 150.0,
        "Auto の先頭項目が余白で広がらないこと: {first_frame:?}"
    );
    assert!(
        second_frame.size.width < 250.0,
        "Auto の後続項目が余白で広がらないこと: {second_frame:?}"
    );
    assert!(
        first_frame.origin.x.abs() < 1e-6,
        "余った空間で先頭項目が中央へ移動しないこと: {first_frame:?}"
    );
    assert!(
        (second_frame.origin.x - first_frame.origin.x - first_frame.size.width - 12.0).abs() < 1e-6,
        "指定した間隔だけで項目が並ぶこと: {first_frame:?} / {second_frame:?}"
    );
    Ok(())
}

/// 横スタックで幅 `Fill` を指定した子は、明示的に余りを受け取る。
fn stack_fill_main(ui: &Ui) -> Result<()> {
    let stack = ui.stack(Orientation::Horizontal)?;
    stack.set_spacing(12.0);
    let fill = ui.button("伸びる")?;
    fill.set_sizing(Sizing::fill_width());
    let fixed = ui.button("固定")?;
    stack.append(&fill);
    stack.append(&fixed);

    let root = stack.native_view();
    root.setFrameSize(NSSize::new(500.0, 60.0));
    root.layoutSubtreeIfNeeded();

    let fill_frame = fill.native_view().frame();
    let fixed_frame = fixed.native_view().frame();
    assert!(
        fill_frame.size.width > fixed_frame.size.width + 150.0,
        "主軸の Fill だけが余白を受け取ること: {fill_frame:?} / {fixed_frame:?}"
    );
    Ok(())
}

/// 交差軸の `Fill` に上限を付けても、親の幅と衝突しない。
///
/// 交差軸の `Fill` は「親の幅に合わせる」だが、上限があるならそちらが優先。
/// 必須制約どうしがぶつかると、AppKit がどちらかを勝手に落としてしまう。
fn stack_fill_cross_respects_max(ui: &Ui) -> Result<()> {
    // 縦並びの交差軸は横。
    let vertical = ui.stack(Orientation::Vertical)?;
    let capped = ui.button("上限まで")?;
    capped.set_sizing(Sizing::fill_width().max_width(200.0));
    vertical.append(&capped);

    let root = vertical.native_view();
    root.setFrameSize(NSSize::new(640.0, 200.0));
    root.layoutSubtreeIfNeeded();
    // 親に合わせる制約は必須ではない (必須だと上限とぶつかり、AppKit が
    // どちらを落とすか分からなくなる)。
    let all = root.constraints();
    let ours: Vec<_> = (0..all.len())
        .map(|i| all.objectAtIndex(i))
        .filter(|c| c.identifier().is_none())
        .collect();
    assert!(
        ours.iter().any(|c| c.priority() < 1000.0),
        "親に合わせる制約は必須より下であること"
    );

    let frame = capped.native_view().frame();
    assert!(
        (frame.size.width - 200.0).abs() < 1e-6,
        "交差軸の上限が親の幅より優先されること: {frame:?}"
    );

    // 横並びの交差軸は縦。
    let horizontal = ui.stack(Orientation::Horizontal)?;
    let short = ui.button("上限まで")?;
    short.set_sizing(Sizing::fill_height().max_height(24.0));
    horizontal.append(&short);

    let root = horizontal.native_view();
    root.setFrameSize(NSSize::new(300.0, 200.0));
    root.layoutSubtreeIfNeeded();
    let frame = short.native_view().frame();
    assert!(
        (frame.size.height - 24.0).abs() < 1e-6,
        "交差軸の上限が親の高さより優先されること: {frame:?}"
    );
    Ok(())
}

/// 上限付きの `Fill` は、空間があれば上限まで広がり、狭ければそれ以下になる。
///
/// CSS や WinUI の `Stretch` と同じ意味になるよう、上限を「通常時に確保したい
/// 大きさ」としても扱う。横も縦も同じ扱いで、指定は `Sizing` だけで済む。
fn fill_with_max_prefers_the_max(ui: &Ui) -> Result<()> {
    let stack = ui.stack(Orientation::Horizontal)?;
    let capped = ui.button("上限まで")?;
    capped.set_sizing(Sizing::fill_width().max_width(200.0));
    stack.append(&capped);
    stack.append(&ui.spacer()?);

    // 上限 (必須) と、上限まで広がりたい希望 (弱い) の 2 本。
    let constraints = naui_constraints(&capped.native_view());
    assert_eq!(constraints.len(), 2, "上限と希望の 2 本が付くこと");
    assert!(
        constraints.iter().any(|c| c.priority() < 100.0),
        "希望のほうは弱い優先度であること"
    );

    let root = stack.native_view();
    root.setFrameSize(NSSize::new(640.0, 60.0));
    root.layoutSubtreeIfNeeded();
    let wide = capped.native_view().frame();
    assert!(
        (wide.size.width - 200.0).abs() < 1e-6,
        "空間があるときは上限まで広がること: {wide:?}"
    );

    root.setFrameSize(NSSize::new(120.0, 60.0));
    root.layoutSubtreeIfNeeded();
    let narrow = capped.native_view().frame();
    assert!(
        narrow.size.width < wide.size.width,
        "狭いときは上限より小さくなること: {wide:?} -> {narrow:?}"
    );
    Ok(())
}

/// スペーサーが縦の余りをすべて受け取り、後ろの子を下端へ寄せる。
fn spacer_pushes(ui: &Ui) -> Result<()> {
    let stack = ui.stack(Orientation::Vertical)?;
    let top = ui.label("上")?;
    let bottom = ui.label("下")?;
    stack.append(&top);
    stack.append(&ui.spacer()?);
    stack.append(&bottom);

    let root = stack.native_view();
    root.setFrameSize(NSSize::new(200.0, 400.0));
    root.layoutSubtreeIfNeeded();

    let top_frame = top.native_view().frame();
    let bottom_frame = bottom.native_view().frame();
    // AppKit の座標系は左下原点。下端の子ほど y が小さい。
    assert!(
        bottom_frame.origin.y < 1.0,
        "後ろの子が下端へ寄ること: {bottom_frame:?}"
    );
    assert!(
        top_frame.origin.y > 380.0,
        "先頭の子は上端に残ること: {top_frame:?}"
    );
    Ok(())
}

/// グリッドは必要な行と列を自分で増やし、子を保持する。
fn grid_places_children(ui: &Ui) -> Result<()> {
    let grid = ui.grid()?;
    grid.set_spacing(8.0, 4.0);
    grid.set_padding(Padding::all(12.0));
    assert!(grid.is_empty());

    grid.attach(&ui.label("左上")?, GridCell::new(0, 0));
    let middle = ui.label("中央")?;
    grid.attach(&middle, GridCell::new(1, 0));
    grid.attach(&ui.label("右下")?, GridCell::new(2, 1));
    assert_eq!((grid.columns(), grid.rows()), (3, 2), "足りない分は増える");

    let wide = ui.label("2 マス分")?;
    grid.attach(&wide, GridCell::new(0, 2).span(3, 1));
    assert_eq!(grid.len(), 4);
    assert_eq!(grid.rows(), 3);
    assert!(
        unsafe { wide.native_view().superview() }.is_some(),
        "置いた子がグリッドの中に入ること"
    );

    grid.set_column_track(0, Track::Fixed(120.0));
    grid.set_column_track(1, Track::FILL);
    grid.set_row_track(0, Track::Fixed(40.0));

    let root = grid.native_view();
    root.setFrameSize(NSSize::new(400.0, 300.0));
    root.layoutSubtreeIfNeeded();
    // 2 列目の左端 = 左の余白 12 + 1 列目の幅 120 + 列間 8。
    // frame は alignment rect より数ピクセル外側に出ることがある。
    let x = middle.native_view().frame().origin.x;
    assert!(
        (x - 140.0).abs() <= 5.0,
        "固定した列幅と余白が効くこと: {x}"
    );
    Ok(())
}

/// `Track::Fill` の列に置いた `Auto` の子は、再レイアウト後も内容幅を保つ。
fn grid_auto_does_not_fill(ui: &Ui) -> Result<()> {
    let grid = ui.grid()?;
    grid.set_column_track(0, Track::FILL);
    grid.set_row_track(0, Track::Auto);
    let status = ui.label("種類: 画像 (同梱のサンプル)")?;
    grid.attach(&status, GridCell::new(0, 0));

    let root = grid.native_view();
    root.setFrameSize(NSSize::new(500.0, 860.0));
    root.layoutSubtreeIfNeeded();
    let initial = status.native_view().frame();

    root.setFrameSize(NSSize::new(500.0, 300.0));
    root.layoutSubtreeIfNeeded();
    let shrunk = status.native_view().frame();

    root.setFrameSize(NSSize::new(500.0, 860.0));
    root.layoutSubtreeIfNeeded();
    let resized = status.native_view().frame();
    assert!(
        resized.size.width < 300.0,
        "再レイアウト後もラベルが列幅へ広がらないこと: {initial:?} / {resized:?}"
    );
    assert!(
        (initial.size.width - resized.size.width).abs() < 1e-6,
        "高さ変更でラベルの幅が変わらないこと: {initial:?} / {resized:?}"
    );
    assert!(
        (shrunk.size.width - resized.size.width).abs() < 1e-6,
        "縮小中もラベルの幅が変わらないこと: {shrunk:?} / {resized:?}"
    );
    Ok(())
}

/// Gallery と同じ Stack -> Tabs -> Stack -> Grid の階層で、高さの縮小・拡大を行う。
fn gallery_media_status_does_not_expand(ui: &Ui) -> Result<()> {
    let media = ui.stack(Orientation::Vertical)?;
    media.set_spacing(12.0);
    media.set_padding(Padding::all(12.0));
    media.set_align(Align::Start);
    let description = ui.label("選んだファイルの種類に合わせて表示形式が切り替わります。")?;
    let description_row = ui.stack(Orientation::Horizontal)?;
    description_row.append(&description);
    media.append(&description_row);

    let image_pane = ui.grid()?;
    image_pane.set_spacing(0.0, 8.0);
    image_pane.set_padding(Padding::all(8.0));
    image_pane.set_sizing(Sizing::fill());
    image_pane.set_column_track(0, Track::FILL);
    image_pane.set_row_track(0, Track::FILL);
    let image = ui.image(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../examples/gallery/assets/sample.png"
    ))?;
    image.set_sizing(Sizing::fill().max_height(315.0));
    image_pane.attach(&image, GridCell::new(0, 0));
    let selector = ui.navbar("収め方")?;
    selector.set_items(&NavItem::list(["contain", "cover", "fill", "none"]));
    selector.set_sizing(Sizing::fill_width());
    image_pane.attach(&selector, GridCell::new(0, 1));

    let forms = ui.grid()?;
    forms.set_sizing(Sizing::fill());
    forms.set_column_track(0, Track::FILL);
    forms.set_row_track(0, Track::FILL);
    forms.attach(&image_pane, GridCell::new(0, 0));

    let video_pane = ui.stack(Orientation::Vertical)?;
    video_pane.set_spacing(8.0);
    video_pane.set_padding(Padding::all(8.0));
    video_pane.set_align(Align::Start);
    video_pane.set_sizing(Sizing::fill());
    let video_frame = ui.grid()?;
    video_frame.set_sizing(Sizing::fill().max_height(315.0));
    video_frame.set_column_track(0, Track::FILL);
    video_frame.set_row_track(0, Track::FILL);
    let video = ui.video("")?;
    video.set_sizing(Sizing::fill().max_height(315.0));
    video_frame.attach(&video, GridCell::new(0, 0));
    video_pane.append(&video_frame);
    let video_controls = ui.stack(Orientation::Vertical)?;
    video_controls.append(&ui.label("状態: 未再生")?);
    video_pane.append(&video_controls);

    let status = ui.label("種類: 画像 (同梱のサンプル)")?;
    let status_row = ui.stack(Orientation::Horizontal)?;
    status_row.append(&status);
    let field = ui.text_input("")?;
    field.set_sizing(Sizing::fill_width());
    let row = ui.stack(Orientation::Horizontal)?;
    row.set_spacing(8.0);
    let load = ui.button("読み込む")?;
    let pick = ui.button("選ぶ")?;
    row.append(&field);
    row.append(&load);
    row.append(&pick);
    row.set_sizing(Sizing::fill_width());
    media.append(&row);
    media.append(&status_row);
    media.append(&forms);

    let root = ui.stack(Orientation::Vertical)?;
    root.set_spacing(12.0);
    root.set_padding(Padding::all(24.0));
    root.append(&ui.label("naui")?);
    root.append(&ui.label("ルート: メディア")?);
    let tabs = ui.tabs()?;
    let home = ui.label("ホーム")?;
    tabs.add_tab("ホーム", &home);
    tabs.add_tab("メディア", &media);
    tabs.set_sizing(Sizing::fill());
    root.append(&tabs);
    root.append(&ui.label("下端")?);

    let window = ui.window("media layout", 680.0, 860.0)?;
    window.set_child(&root);
    window.show();
    let native_window = window.native_window();
    let content = native_window
        .contentView()
        .expect("コンテンツビューがあること");
    content.setFrameSize(NSSize::new(680.0, 860.0));
    content.layoutSubtreeIfNeeded();
    pump(0.05);
    tabs.select(1);
    content.layoutSubtreeIfNeeded();
    pump(0.05);
    let initial = status.native_view().frame();
    let description_initial = description.native_view().frame();
    let row_initial = row.native_view().frame();
    native_window.setContentSize(NSSize::new(680.0, 350.0));
    content.setFrameSize(NSSize::new(680.0, 350.0));
    content.layoutSubtreeIfNeeded();
    pump(0.05);
    let shrunk = status.native_view().frame();
    let description_shrunk = description.native_view().frame();
    let row_shrunk = row.native_view().frame();
    native_window.setContentSize(NSSize::new(680.0, 860.0));
    content.setFrameSize(NSSize::new(680.0, 860.0));
    content.layoutSubtreeIfNeeded();
    pump(0.05);
    let resized = status.native_view().frame();
    let description_resized = description.native_view().frame();
    let row_resized = row.native_view().frame();
    let image_frame = image.native_view().frame();
    forms.replace(&video_pane, GridCell::new(0, 0));
    content.layoutSubtreeIfNeeded();
    pump(0.05);
    let video_frame_after_replace = video.native_view().frame();
    forms.replace(&image_pane, GridCell::new(0, 0));
    content.layoutSubtreeIfNeeded();
    pump(0.05);
    assert!(
        resized.size.width < 300.0,
        "高さ変更後もステータスが列幅へ広がらないこと: {initial:?} / {shrunk:?} / {resized:?}"
    );
    assert!(
        (initial.size.width - resized.size.width).abs() < 1e-6,
        "縮小してから戻してもステータス幅が変わらないこと: {initial:?} / {shrunk:?} / {resized:?}"
    );
    assert!(
        description_resized.size.width < 500.0,
        "高さ変更後も説明文が列幅へ広がらないこと: {description_initial:?} / {description_shrunk:?} / {description_resized:?}"
    );
    assert!(
        (description_initial.size.width - description_resized.size.width).abs() < 1e-6,
        "縮小してから戻しても説明文の幅が変わらないこと: {description_initial:?} / {description_shrunk:?} / {description_resized:?}"
    );
    assert!(
        row_resized.size.height < 100.0,
        "高さ変更後もパス入力行がメディア表示の余白を受け取らないこと: {row_initial:?} / {row_shrunk:?} / {row_resized:?}"
    );
    assert!(
        (row_initial.size.height - row_resized.size.height).abs() < 1e-6,
        "縮小してから戻してもパス入力行の高さが変わらないこと: {row_initial:?} / {row_shrunk:?} / {row_resized:?}"
    );
    assert!(
        image_frame.size.width > 500.0,
        "Auto のラベルがあっても画像の Fill は列いっぱいになること: {image_frame:?}"
    );
    assert!(
        video_frame_after_replace.size.height > 200.0,
        "画像から動画へ差し替えた直後も動画表示欄が潰れないこと: {video_frame_after_replace:?}"
    );
    window.close();
    Ok(())
}

/// グリッドの差し替えで、前の子が親に残らない。
fn grid_replaces_child(ui: &Ui) -> Result<()> {
    let grid = ui.grid()?;
    let photo = ui.label("写真")?;
    let video = ui.label("動画")?;
    let cell = GridCell::new(0, 0);

    grid.attach(&photo, cell);
    assert!(unsafe { photo.native_view().superview() }.is_some());

    grid.replace(&video, cell);

    assert_eq!(grid.len(), 1);
    assert!(unsafe { photo.native_view().superview() }.is_none());
    assert!(unsafe { video.native_view().superview() }.is_some());
    Ok(())
}

/// 折りたたみは見出しを押すたびに開閉し、変わった後の状態を通知する。
fn expander_toggles_and_notifies(ui: &Ui) -> Result<()> {
    let expander = ui.expander("詳細設定")?;
    assert_eq!(expander.text(), "詳細設定");

    let body = ui.label("中身")?;
    expander.set_child(&body);
    assert!(!expander.is_expanded(), "既定は閉じていること");
    assert!(
        body.native_view().isHidden(),
        "閉じている間、中身は隠れていること"
    );

    let seen = Rc::new(RefCell::new(Vec::new()));
    expander.on_toggle({
        let seen = seen.clone();
        move |expanded| seen.borrow_mut().push(expanded)
    });

    expander.click();
    assert!(expander.is_expanded());
    assert!(!body.native_view().isHidden(), "開くと中身が出ること");
    expander.click();
    assert!(!expander.is_expanded());
    assert!(body.native_view().isHidden());
    assert_eq!(*seen.borrow(), vec![true, false]);

    expander.set_expanded(true);
    assert!(expander.is_expanded());
    assert!(!body.native_view().isHidden());
    assert_eq!(
        seen.borrow().len(),
        2,
        "プログラムからの開閉は通知しないこと"
    );

    expander.set_text("詳細");
    assert_eq!(expander.text(), "詳細");
    assert_eq!(
        expander.native_header().title().to_string(),
        "詳細",
        "見出しのボタンへ文字が届くこと"
    );
    Ok(())
}

/// たたんでいる間、中身はレイアウトから外れて場所を空けない。
fn expander_collapsed_child_leaves_the_layout(ui: &Ui) -> Result<()> {
    let expander = ui.expander("詳細")?;
    let body = ui.text_area("あ\nい\nう")?;
    body.set_sizing(
        Sizing::new()
            .width(Length::Fixed(200.0))
            .height(Length::Fixed(120.0)),
    );
    expander.set_child(&body);

    let view = expander.native_view();
    view.layoutSubtreeIfNeeded();
    let collapsed = view.fittingSize().height;
    assert!(collapsed > 0.0, "見出しのぶんの高さはあること");

    expander.set_expanded(true);
    view.layoutSubtreeIfNeeded();
    let expanded = view.fittingSize().height;
    assert!(
        expanded >= collapsed + 120.0,
        "開くと中身のぶんだけ高くなること: {collapsed} -> {expanded}"
    );

    expander.set_expanded(false);
    view.layoutSubtreeIfNeeded();
    assert!(
        (view.fittingSize().height - collapsed).abs() < 1.0,
        "たたむと見出しだけの高さへ戻ること"
    );
    Ok(())
}

/// 中身は指定が無くても、折りたたみの幅いっぱいに置かれる (`Scroll` と同じ)。
///
/// 中身の幅が中身自身の内容で決まると、中で左右のどこへ寄せても場所が
/// ずれて見える (4 環境で見え方がそろわない)。
fn expander_child_fills_the_width(ui: &Ui) -> Result<()> {
    let window = ui.window("折りたたみ", 400.0, 300.0)?;
    let root = ui.stack(Orientation::Vertical)?;
    let expander = ui.expander("詳細")?;
    expander.set_sizing(Sizing::fill_width());

    let body = ui.stack(Orientation::Vertical)?;
    body.set_align(Align::Start);
    body.append(&ui.label("短い")?);
    expander.set_child(&body);
    expander.set_expanded(true);
    root.append(&expander);
    window.set_child(&root);

    let content = window
        .native_window()
        .contentView()
        .expect("ウィンドウの中身があること");
    content.setFrameSize(NSSize::new(400.0, 300.0));
    content.layoutSubtreeIfNeeded();

    let outer = expander.native_view().frame();
    let inner = body.native_view().frame();
    assert!(
        outer.size.width > 0.0,
        "折りたたみが幅を持つこと: {outer:?}"
    );
    assert!(
        (outer.size.width - inner.size.width).abs() < 1.0,
        "中身が折りたたみの幅いっぱいに置かれること: {outer:?} / {inner:?}"
    );
    assert!(
        (outer.origin.x - inner.origin.x).abs() < 1.0,
        "中身が折りたたみの左端から始まること: {outer:?} / {inner:?}"
    );
    Ok(())
}

/// `Label` の既定は 1 行で、`set_wrap(true)` のときだけ折り返す。
fn label_wraps_only_when_asked(ui: &Ui) -> Result<()> {
    let window = ui.window("折り返し", 200.0, 240.0)?;
    let stack = ui.stack(Orientation::Vertical)?;
    let text = "これはとても長い説明の文章で、狭い幅には 1 行で収まりません。";

    let plain = ui.label(text)?;
    plain.set_sizing(Sizing::fill_width());
    let wrapped = ui.label(text)?;
    wrapped.set_wrap(true);
    wrapped.set_sizing(Sizing::fill_width());
    stack.append(&plain);
    stack.append(&wrapped);
    window.set_child(&stack);

    let content = window
        .native_window()
        .contentView()
        .expect("ウィンドウの中身があること");
    content.setFrameSize(NSSize::new(200.0, 240.0));
    // 折り返す幅は frame の変化を見て決まるので、そのぶん多く回す。
    content.layoutSubtreeIfNeeded();
    content.layoutSubtreeIfNeeded();

    let plain_frame = plain.native_view().frame();
    let wrapped_frame = wrapped.native_view().frame();
    // NSTextField は左右に 2pt の差し込みを持つので、その分だけ大きく出る。
    assert!(
        (plain_frame.size.width - wrapped_frame.size.width).abs() < 1.0
            && wrapped_frame.size.width <= 210.0,
        "どちらも配られた幅に収まること: {plain_frame:?} / {wrapped_frame:?}"
    );
    assert!(
        wrapped_frame.size.height > plain_frame.size.height * 1.5,
        "折り返した側だけ高くなること: 1 行 {} / 折り返し {}",
        plain_frame.size.height,
        wrapped_frame.size.height
    );

    // 折り返さない側は 1 行のまま、末尾を省略記号で切る。
    let plain_native: Retained<NSTextField> = plain
        .native_view()
        .downcast()
        .expect("ラベルは NSTextField であること");
    assert_eq!(plain_native.maximumNumberOfLines(), 1);
    assert_eq!(
        plain_native.lineBreakMode(),
        objc2_app_kit::NSLineBreakMode::ByTruncatingTail
    );

    // 折り返しは何度でも切り替えられる。
    wrapped.set_wrap(false);
    content.layoutSubtreeIfNeeded();
    content.layoutSubtreeIfNeeded();
    assert!(
        (wrapped.native_view().frame().size.height - plain_frame.size.height).abs() < 1.0,
        "戻すと 1 行に戻ること: {:?}",
        wrapped.native_view().frame()
    );
    Ok(())
}

/// 分割ビューは NSSplitView そのもので、2 つの区画を並べる。
fn split_view_arranges_two_panes_in_a_native_split_view(ui: &Ui) -> Result<()> {
    let split = ui.split_view(Orientation::Horizontal)?;
    let start = ui.label("左")?;
    let end = ui.label("右")?;
    split.set_start(&start);
    split.set_end(&end);

    let native = split.native_split_view();
    assert!(
        native.isVertical(),
        "Horizontal は区画が横に並ぶので、仕切りは縦であること"
    );
    let arranged = native.arrangedSubviews();
    assert_eq!(arranged.len(), 2, "区画は 2 つであること");
    assert!(
        std::ptr::eq(
            &*arranged.objectAtIndex(0) as *const _,
            &*start.native_view() as *const _
        ),
        "先頭が start 側であること"
    );
    assert!(
        std::ptr::eq(
            &*arranged.objectAtIndex(1) as *const _,
            &*end.native_view() as *const _
        ),
        "2 つめが end 側であること"
    );
    assert_eq!(split.orientation(), Orientation::Horizontal);
    assert_eq!(
        split.position(),
        naui_core::DEFAULT_SPLIT_POSITION,
        "作った直後は既定の位置であること"
    );

    let vertical = ui.split_view(Orientation::Vertical)?;
    assert!(
        !vertical.native_split_view().isVertical(),
        "Vertical は区画が縦に並ぶので、仕切りは横であること"
    );
    Ok(())
}

/// 指定した位置で仕切りが置かれ、広がった分は end 側が受け取る。
fn split_view_keeps_the_start_size_when_the_window_grows(ui: &Ui) -> Result<()> {
    let window = ui.window("分割", 400.0, 300.0)?;
    let split = ui.split_view(Orientation::Horizontal)?;
    let start = ui.stack(Orientation::Vertical)?;
    start.append(&ui.label("サイドバー")?);
    let end = ui.stack(Orientation::Vertical)?;
    end.append(&ui.label("本文")?);
    split.set_start(&start);
    split.set_end(&end);
    split.set_position(150.0);
    window.set_child(&split);

    let native = split.native_split_view();
    native.setFrameSize(NSSize::new(400.0, 300.0));
    native.layoutSubtreeIfNeeded();

    let start_width = start.native_view().frame().size.width;
    assert!(
        (start_width - 150.0).abs() < 1.0,
        "指定した位置に仕切りが置かれること: {start_width}"
    );

    // ウィンドウが広がっても、start 側は指定した大きさのまま。
    native.setFrameSize(NSSize::new(700.0, 300.0));
    native.layoutSubtreeIfNeeded();
    let start_width = start.native_view().frame().size.width;
    let end_width = end.native_view().frame().size.width;
    assert!(
        (start_width - 150.0).abs() < 1.0,
        "広げても start 側は同じ大きさであること: {start_width}"
    );
    assert!(
        end_width > 400.0,
        "広がった分は end 側が受け取ること: {end_width}"
    );
    assert!(
        (split.position() - 150.0).abs() < 1.0,
        "位置は変わらないこと: {}",
        split.position()
    );
    Ok(())
}

/// 仕切りは最小の大きさの内側にだけ動き、動いたら通知が届く。
fn split_view_drag_clamps_to_the_minimums_and_notifies(ui: &Ui) -> Result<()> {
    let window = ui.window("分割", 400.0, 300.0)?;
    let split = ui.split_view(Orientation::Horizontal)?;
    split.set_start(&ui.label("左")?);
    split.set_end(&ui.label("右")?);
    split.set_min_sizes(80.0, 120.0);
    window.set_child(&split);

    let native = split.native_split_view();
    native.setFrameSize(NSSize::new(400.0, 300.0));
    native.layoutSubtreeIfNeeded();

    let seen = Rc::new(RefCell::new(Vec::new()));
    split.on_resize({
        let seen = seen.clone();
        move |position| seen.borrow_mut().push(position)
    });

    split.set_position(200.0);
    assert!(
        seen.borrow().is_empty(),
        "set_position では通知しないこと: {:?}",
        seen.borrow()
    );

    split.drag_to(10.0);
    assert!(
        (split.position() - 80.0).abs() < 1.0,
        "start 側の最小より内側へは行かないこと: {}",
        split.position()
    );

    let total = 400.0 - native.dividerThickness();
    split.drag_to(400.0);
    assert!(
        (split.position() - (total - 120.0)).abs() < 1.0,
        "end 側の最小を残すこと: {} (全体 {total})",
        split.position()
    );

    let seen = seen.borrow();
    assert_eq!(seen.len(), 2, "動かした 2 回だけ通知されること: {seen:?}");
    assert!((seen[0] - 80.0).abs() < 1.0, "{seen:?}");
    assert!((seen[1] - (total - 120.0)).abs() < 1.0, "{seen:?}");
    Ok(())
}

/// スクロールは中身のハンドルを捨てても保持し続ける。
fn scroll_keeps_child(ui: &Ui) -> Result<()> {
    let scroll = ui.scroll()?;
    scroll.set_sizing(Sizing::fixed(200.0, 100.0));

    let hits = Rc::new(RefCell::new(0));
    let button = {
        let inner = ui.stack(Orientation::Vertical)?;
        let button = ui.button("押す")?;
        button.on_click({
            let hits = hits.clone();
            move || *hits.borrow_mut() += 1
        });
        inner.append(&button);
        // 中身を長くして、縦にはみ出させる。
        for i in 0..20 {
            inner.append(&ui.label(&format!("行 {i}"))?);
        }
        scroll.set_child(&inner);
        button
    };

    scroll.set_policy(ScrollPolicy::Never, ScrollPolicy::Auto);
    let root = scroll.native_view();
    root.layoutSubtreeIfNeeded();

    button.click();
    assert_eq!(*hits.borrow(), 1, "中身のコールバックが生きていること");
    assert!(
        unsafe { button.native_view().superview() }.is_some(),
        "中身がスクロールの中に入っていること"
    );
    Ok(())
}

/// naui が付けた大きさの制約だけを取り出す。
fn naui_constraints(view: &NSView) -> Vec<Retained<NSLayoutConstraint>> {
    let all = view.constraints();
    (0..all.len())
        .map(|i| all.objectAtIndex(i))
        .filter(|c| {
            c.identifier()
                .is_some_and(|id| id.to_string() == "naui.sizing")
        })
        .collect()
}

/// 同じ行に高さの違うものを置いても、縦中央でそろう。
///
/// NSGridView の既定は上ぞろえなので、ラベル (16pt) と入力欄 (24pt) を
/// 並べるとラベルだけが上にずれる。
fn grid_row_alignment(ui: &Ui) -> Result<()> {
    let grid = ui.grid()?;
    grid.set_spacing(12.0, 8.0);
    grid.set_column_track(0, Track::Fixed(96.0));
    grid.set_column_track(1, Track::FILL);
    let label = ui.label("名前")?;
    let field = ui.text_input("")?;
    field.set_sizing(Sizing::fill_width());
    grid.attach(&label, GridCell::new(0, 0));
    grid.attach(&field, GridCell::new(1, 0));

    let root = grid.native_view();
    root.setFrameSize(NSSize::new(400.0, 200.0));
    root.layoutSubtreeIfNeeded();

    let label_frame = label.native_view().frame();
    let field_frame = field.native_view().frame();
    assert!(
        label_frame.size.height < field_frame.size.height,
        "前提: ラベルのほうが低いこと ({label_frame:?} / {field_frame:?})"
    );
    let label_center = label_frame.origin.y + label_frame.size.height / 2.0;
    let field_center = field_frame.origin.y + field_frame.size.height / 2.0;
    assert!(
        (label_center - field_center).abs() <= 1.0,
        "同じ行の中心がそろうこと: ラベル {label_center} / 入力欄 {field_center}"
    );
    Ok(())
}

/// ファイル選択は AppKit のボタンとして構成され、設定を保つ。
///
/// `NSOpenPanel` はアプリモーダルで、出すと閉じられるまで戻らない。
/// 自動テストからは開けないので、**ボタンの実体と設定の保持まで**を確かめる。
fn file_picker_configuration(ui: &Ui) -> Result<()> {
    let picker = ui.file_picker("ファイルを選ぶ")?;

    let view = picker.native_view();
    let button = view
        .downcast::<NSButton>()
        .expect("実体は NSButton であること");
    assert_eq!(button.title().to_string(), "ファイルを選ぶ");
    picker.set_text("フォルダーを選ぶ");
    assert_eq!(button.title().to_string(), "フォルダーを選ぶ");

    assert!(button.isEnabled(), "既定では押せること");
    picker.set_enabled(false);
    assert!(!button.isEnabled());
    picker.set_enabled(true);

    assert_eq!(picker.mode(), FilePickerMode::File, "既定はファイル 1 つ");
    picker.set_mode(FilePickerMode::Folder);
    assert_eq!(picker.mode(), FilePickerMode::Folder);
    picker.set_mode(FilePickerMode::Files);
    assert_eq!(picker.mode(), FilePickerMode::Files);

    // 絞り込みは開くまで使われないので、設定しても選択は変わらない。
    picker.set_filters(&[FileFilter::new("画像", ["png", "jpg"])]);
    assert!(picker.selection().is_empty(), "まだ何も選ばれていないこと");

    // コンテナへ入れてもハンドルを手放して大丈夫なこと (他のウィジェットと同じ)。
    let stack = ui.stack(Orientation::Vertical)?;
    stack.append(&picker);
    assert_eq!(stack.len(), 1);
    Ok(())
}

/// 保存はボタンとして構成され、設定が `NSSavePanel` へ届く。
///
/// `NSSavePanel` もアプリモーダルで、出すと閉じられるまで戻らない。
/// 自動テストからは開けないので、**表示前のパネルの中身**を確かめる。
fn file_saver_configuration_reaches_the_panel(ui: &Ui) -> Result<()> {
    let saver = ui.file_saver("保存する")?;

    let view = saver.native_view();
    let button = view
        .downcast::<NSButton>()
        .expect("実体は NSButton であること");
    assert_eq!(button.title().to_string(), "保存する");
    saver.set_text("書き出す");
    assert_eq!(button.title().to_string(), "書き出す");

    assert!(button.isEnabled(), "既定では押せること");
    saver.set_enabled(false);
    assert!(!button.isEnabled());
    saver.set_enabled(true);

    assert_eq!(saver.file_name(), "", "既定の名前は空 (AppKit に任せる)");
    assert!(saver.destination().is_none(), "まだ保存していないこと");
    assert_eq!(saver.contents_len(), 0);

    saver.set_file_name("メモ.txt");
    saver.set_contents("こんにちは".as_bytes());
    saver.set_filters(&[
        FileFilter::new("文書", ["txt", "md"]),
        FileFilter::new("画像", ["png"]),
    ]);
    assert_eq!(saver.file_name(), "メモ.txt");
    assert_eq!(saver.contents_len(), "こんにちは".len());

    let panel = saver.native_panel();
    assert_eq!(panel.nameFieldStringValue().to_string(), "メモ.txt");
    #[allow(deprecated)]
    let types: Vec<String> = panel
        .allowedFileTypes()
        .expect("絞り込みが届いていること")
        .iter()
        .map(|t| t.to_string())
        .collect();
    assert_eq!(
        types,
        ["txt", "md", "png"],
        "全ての絞り込みが平らに並ぶこと"
    );

    // コンテナへ入れてもハンドルを手放して大丈夫なこと (他のウィジェットと同じ)。
    let stack = ui.stack(Orientation::Vertical)?;
    stack.append(&saver);
    assert_eq!(stack.len(), 1);
    Ok(())
}

/// 拡張子の無い既定名には、絞り込みの先頭の拡張子が付く。
fn file_saver_adds_the_default_extension(ui: &Ui) -> Result<()> {
    let saver = ui.file_saver("保存")?;
    saver.set_filters(&[FileFilter::new("文書", ["txt", "md"])]);

    saver.set_file_name("メモ");
    assert_eq!(
        saver.native_panel().nameFieldStringValue().to_string(),
        "メモ.txt"
    );
    // すでに絞り込みの拡張子が付いていれば足さない。
    saver.set_file_name("メモ.md");
    assert_eq!(
        saver.native_panel().nameFieldStringValue().to_string(),
        "メモ.md"
    );
    // 名前を指定しなければ AppKit の既定 (「Untitled」) のまま。
    saver.set_file_name("");
    let default_name = saver.native_panel().nameFieldStringValue().to_string();
    assert!(
        !default_name.is_empty() && !default_name.contains("メモ"),
        "名前を指定しなければ AppKit の既定のままであること: {default_name}"
    );
    Ok(())
}

/// メインメニューに編集項目があり、⌘V がレスポンダチェーンへ流れる。
///
/// macOS では ⌘C / ⌘V はメインメニューのキー等価として配送される。
/// メニューが無いと、テキスト入力にフォーカスがあっても貼り付けができない。
fn menu_bar_provides_edit_shortcuts(_ui: &Ui) -> Result<()> {
    let mtm = objc2::MainThreadMarker::new().expect("メインスレッド");
    naui_macos::install_menu_bar_for_test(mtm, "naui test");

    let app = objc2_app_kit::NSApplication::sharedApplication(mtm);
    let main = app.mainMenu().expect("メインメニューがあること");
    let edit = (0..main.numberOfItems())
        .filter_map(|i| main.itemAtIndex(i).and_then(|item| item.submenu()))
        .find(|menu| menu.title().to_string() == "編集")
        .expect("編集メニューがあること");

    let paste = (0..edit.numberOfItems())
        .filter_map(|i| edit.itemAtIndex(i))
        .find(|item| item.action() == Some(objc2::sel!(paste:)))
        .expect("paste: を送る項目があること");
    assert_eq!(
        paste.keyEquivalent().to_string(),
        "v",
        "⌘V に割り当てられていること"
    );
    // ターゲットが nil = レスポンダチェーンへ流す (編集中のコントロールへ届く)。
    assert!(
        unsafe { paste.target() }.is_none(),
        "ターゲットを固定せず、いま編集中のコントロールへ送ること"
    );

    // 起動時に一度組み立てたら、呼び直しても作り直さない。
    let again = app.mainMenu().expect("メインメニューがあること");
    naui_macos::install_menu_bar_for_test(mtm, "naui test");
    assert!(
        app.mainMenu().is_some_and(|m| m == again),
        "既にメニューがあれば作り直さないこと"
    );
    Ok(())
}

// --------------------------------------------------------------- メディア

/// 2x2 の PNG。画像の読み込みを、実ファイルを介して確かめるために埋め込む。
const PNG_2X2: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x02, 0x08, 0x02, 0x00, 0x00, 0x00, 0xfd, 0xd4, 0x9a,
    0x73, 0x00, 0x00, 0x00, 0x10, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0xf8, 0xcf, 0x00, 0x04,
    0xff, 0x19, 0x20, 0x14, 0x00, 0x1b, 0xf2, 0x03, 0xfd, 0xd6, 0x96, 0xf2, 0x2b, 0x00, 0x00, 0x00,
    0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
];

/// テストが使う一時ファイル。落ちても残らないよう Drop で消す。
struct Fixture {
    dir: std::path::PathBuf,
}

impl Fixture {
    /// テストごとに別のディレクトリを作る。
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("naui-{}-{}", std::process::id(), name));
        std::fs::create_dir_all(&dir).expect("一時ディレクトリの作成");
        Self { dir }
    }

    fn write(&self, name: &str, bytes: &[u8]) -> String {
        let path = self.dir.join(name);
        std::fs::write(&path, bytes).expect("一時ファイルの書き出し");
        path.to_str().expect("UTF-8 のパス").to_string()
    }

    /// 0.5 秒の無音 WAV。AVFoundation が実際に再生できる最小のメディア。
    fn silent_wav(&self, name: &str) -> String {
        const RATE: u32 = 8000;
        const SAMPLES: u32 = RATE / 2;
        let data_len = SAMPLES * 2;
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + data_len).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes()); // fmt チャンクの長さ
        wav.extend_from_slice(&1u16.to_le_bytes()); // リニア PCM
        wav.extend_from_slice(&1u16.to_le_bytes()); // モノラル
        wav.extend_from_slice(&RATE.to_le_bytes());
        wav.extend_from_slice(&(RATE * 2).to_le_bytes()); // バイト毎秒
        wav.extend_from_slice(&2u16.to_le_bytes()); // ブロックあたりのバイト数
        wav.extend_from_slice(&16u16.to_le_bytes()); // 量子化ビット数
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&data_len.to_le_bytes());
        wav.resize(wav.len() + data_len as usize, 0);
        self.write(name, &wav)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// ランループを回して、AVFoundation の非同期な読み込みと再生を進める。
///
/// アセットの読み込みも再生位置の更新もランループの上で起きるため、
/// これを回さないと長さは決まらず、再生も進まない。
fn pump(seconds: f64) {
    let until = NSDate::dateWithTimeIntervalSinceNow(seconds);
    NSRunLoop::currentRunLoop().runUntilDate(&until);
}

/// 条件が満たされるまで、上限までランループを回す。
///
/// 読み込み・再生・タイマーの進み方はマシンの速さに左右されるので、固定時間の
/// `pump` では遅い CI ランナーで足りないことがある。待ちたいものはこちらで待つ。
/// 上限まで待っても満たされないときはそのまま返るので、呼び出し側で確かめる。
fn pump_until(limit: f64, mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs_f64(limit);
    while !ready() && Instant::now() < deadline {
        pump(0.05);
    }
}

/// 状態の変化を記録するクロージャを付ける。
fn record_states(seen: &Rc<RefCell<Vec<PlaybackState>>>) -> impl FnMut(PlaybackState) + 'static {
    let seen = seen.clone();
    move |state| seen.borrow_mut().push(state)
}

/// 画像がローカルのファイルから NSImage として読み込まれる。
fn image_loads_a_local_file(ui: &Ui) -> Result<()> {
    let fixture = Fixture::new("image");
    let path = fixture.write("a.png", PNG_2X2);

    let image = ui.image(&path)?;
    assert_eq!(image.source(), path, "渡した文字列がそのまま返ること");
    assert!(image.is_loaded(), "NSImage が読み込まれていること");

    // NSImageView が実際に画像を持ち、大きさも PNG のとおりであること。
    let view = image.native_view();
    let native = view
        .downcast_ref::<NSImageView>()
        .expect("実体が NSImageView であること");
    let size = native.image().expect("NSImage があること").size();
    assert_eq!((size.width, size.height), (2.0, 2.0));

    // 存在しない場所を指すと画像は外れる (NSImage が nil を返す)。
    image.set_source("/naui/does/not/exist.png");
    assert!(!image.is_loaded());
    assert!(native.image().is_none());

    // 空文字列でも落ちない。
    image.set_source("");
    assert!(!image.is_loaded());
    Ok(())
}

/// 収め方の指定が NSImageView の imageScaling になる。
fn image_fit_maps_to_native_scaling(ui: &Ui) -> Result<()> {
    let image = ui.image("")?;
    let view = image.native_view();
    let native = view
        .downcast_ref::<NSImageView>()
        .expect("実体が NSImageView であること");
    assert!(native.clipsToBounds(), "画像表示領域からはみ出さないこと");

    // 既定は縦横比を保って収める。
    assert_eq!(
        native.imageScaling(),
        NSImageScaling::ScaleProportionallyUpOrDown
    );
    image.set_fit(Fit::Fill);
    assert_eq!(
        native.imageScaling(),
        NSImageScaling::ScaleAxesIndependently
    );
    image.set_fit(Fit::None);
    assert_eq!(native.imageScaling(), NSImageScaling::ScaleNone);
    // Cover は imageScaling の値を共有するが、NauiImageView::drawRect で独自に描画する。
    image.set_fit(Fit::Cover);
    assert_eq!(
        native.imageScaling(),
        NSImageScaling::ScaleProportionallyUpOrDown
    );
    Ok(())
}

/// AVPlayerView に動画の intrinsic size がまだ無くても、表示領域を確保する。
///
/// 上限付きの `Fill` は「空間があれば上限まで、狭ければそれ以下」を表す。
/// Gallery 側に macOS だけの指定を書かずに済むよう、この意味は `Sizing` から
/// 導く (バックエンド共通の約束)。
fn video_display_reserves_height(ui: &Ui) -> Result<()> {
    let pane = ui.stack(Orientation::Vertical)?;
    pane.set_spacing(8.0);
    pane.set_padding(Padding::all(8.0));
    pane.set_align(Align::Start);
    pane.set_sizing(Sizing::fill());
    let video = ui.video("")?;
    video.set_sizing(Sizing::fill().max_height(315.0));
    let media_frame = ui.grid()?;
    media_frame.set_sizing(Sizing::fill().max_height(315.0));
    media_frame.set_column_track(0, Track::FILL);
    media_frame.set_row_track(0, Track::FILL);
    media_frame.attach(&video, GridCell::new(0, 0));
    pane.append(&media_frame);
    let controls = ui.stack(Orientation::Vertical)?;
    controls.set_spacing(8.0);
    let buttons = ui.stack(Orientation::Horizontal)?;
    buttons.set_spacing(8.0);
    buttons.append(&ui.button("再生")?);
    buttons.append(&ui.button("一時停止")?);
    controls.append(&buttons);
    controls.append(&ui.label("状態: 未再生")?);
    controls.append(&ui.label("位置: -")?);
    controls.append(&ui.slider(0.0, 1.0)?);
    controls.append(&ui.slider(0.0, 1.0)?);
    controls.append(&ui.label("音量: 100%")?);
    let toggles = ui.stack(Orientation::Horizontal)?;
    toggles.set_spacing(12.0);
    toggles.append(&ui.checkbox("消音")?);
    toggles.append(&ui.checkbox("繰り返し")?);
    controls.append(&toggles);
    pane.append(&controls);

    let root = pane.native_view();
    root.setFrameSize(NSSize::new(640.0, 860.0));
    root.layoutSubtreeIfNeeded();
    let initial = video.native_view().frame();
    assert!(
        (initial.size.height - 315.0).abs() < 1e-6,
        "通常の高さでは動画表示領域を上限まで確保すること: {initial:?}"
    );
    root.setFrameSize(NSSize::new(640.0, 300.0));
    root.layoutSubtreeIfNeeded();
    let narrow = video.native_view().frame();
    assert!(
        narrow.size.height < initial.size.height,
        "ウィンドウが狭いときは動画表示領域も縮められること: {initial:?} -> {narrow:?}"
    );
    assert!(
        narrow.size.height > 0.0,
        "動画表示領域が完全には潰れないこと: {narrow:?}"
    );

    Ok(())
}

/// 音声を最後まで再生し、状態と長さが AVPlayer から届く。
fn audio_plays_to_the_end(ui: &Ui) -> Result<()> {
    let fixture = Fixture::new("audio");
    let audio = ui.audio(&fixture.silent_wav("a.wav"))?;
    let seen = Rc::new(RefCell::new(Vec::new()));
    audio.on_state_change(record_states(&seen));

    // 読み込みが終わるまで長さは決まらない。
    assert_eq!(audio.duration(), None, "読み込み前の長さは None であること");
    pump_until(5.0, || audio.duration().is_some());
    let duration = audio.duration().expect("読み込み後は長さが決まること");
    assert!(
        (duration - 0.5).abs() < 0.05,
        "WAV の長さ 0.5 秒が返ること: {duration}"
    );

    audio.play();
    pump_until(5.0, || audio.state() == PlaybackState::Ended);
    assert_eq!(
        audio.state(),
        PlaybackState::Ended,
        "最後まで再生したら Ended になること"
    );
    let states = seen.borrow().clone();
    assert!(
        states.contains(&PlaybackState::Playing),
        "再生中が通知されること: {states:?}"
    );
    assert_eq!(
        states.last(),
        Some(&PlaybackState::Ended),
        "最後の通知が Ended であること: {states:?}"
    );
    assert!(
        audio.position() > 0.4,
        "再生位置が進んでいること: {}",
        audio.position()
    );

    // 再生し終えた後の play は、先頭へ戻してから鳴らす。
    audio.play();
    pump_until(5.0, || audio.position() < 0.4);
    assert!(
        audio.position() < 0.4,
        "先頭へ戻って再生されること: {}",
        audio.position()
    );
    Ok(())
}

/// 繰り返しを指定すると、末尾で止まらずに先頭へ戻る。
fn audio_loops_back_to_the_start(ui: &Ui) -> Result<()> {
    let fixture = Fixture::new("loop");
    let audio = ui.audio(&fixture.silent_wav("a.wav"))?;
    audio.set_loop(true);
    assert!(audio.is_loop());
    let seen = Rc::new(RefCell::new(Vec::new()));
    audio.on_state_change(record_states(&seen));

    pump_until(5.0, || audio.duration().is_some());
    audio.play();
    // 0.5 秒のメディアが末尾を越えて先頭へ戻るまで回す。時間で待つと遅い
    // ランナーでは一周しきらず、繰り返しを確かめないまま通ってしまう。
    let passed_end = Cell::new(false);
    let looped = Cell::new(false);
    pump_until(5.0, || {
        let position = audio.position();
        if position > 0.4 {
            passed_end.set(true);
        } else if passed_end.get() && position < 0.2 {
            looped.set(true);
        }
        looped.get()
    });
    assert!(looped.get(), "末尾まで進んで先頭へ戻ること");

    let states = seen.borrow().clone();
    assert!(
        !states.contains(&PlaybackState::Ended),
        "繰り返し中は Ended にならないこと: {states:?}"
    );
    assert_eq!(
        audio.state(),
        PlaybackState::Playing,
        "末尾を越えても再生が続くこと: {states:?}"
    );
    Ok(())
}

/// 再生位置を指定すると、AVPlayer の現在位置が動く。
fn media_seek_moves_the_position(ui: &Ui) -> Result<()> {
    let fixture = Fixture::new("seek");
    let video = ui.video(&fixture.silent_wav("a.wav"))?;
    pump_until(5.0, || video.duration().is_some());

    video.seek(0.25);
    pump_until(5.0, || (video.position() - 0.25).abs() < 0.1);
    let position = video.position();
    assert!(
        (position - 0.25).abs() < 0.1,
        "指定した位置へ移ること: {position}"
    );

    // 負の値は先頭として扱う。
    video.seek(-5.0);
    pump_until(5.0, || video.position() < 0.1);
    assert!(
        video.position() < 0.1,
        "先頭へ戻ること: {}",
        video.position()
    );
    Ok(())
}

/// 音量と消音が AVPlayer と往復し、範囲外は丸められる。
fn media_volume_round_trips(ui: &Ui) -> Result<()> {
    let video = ui.video("")?;
    video.set_volume(0.25);
    assert!((video.volume() - 0.25).abs() < 1e-6);

    video.set_volume(3.0);
    assert!((video.volume() - 1.0).abs() < 1e-6, "1.0 で丸めること");
    video.set_volume(-1.0);
    assert!((video.volume() - 0.0).abs() < 1e-6, "0.0 で丸めること");

    assert!(!video.is_muted());
    video.set_muted(true);
    assert!(video.is_muted(), "AVPlayer の消音が効くこと");
    video.set_muted(false);
    assert!(!video.is_muted());
    Ok(())
}

/// ハンドルを捨てても、KVO を張ったままの AVPlayer で異常終了しない。
///
/// KVO の観測者を登録したまま AVPlayer を解放すると AppKit が落ちる。
/// Drop で確実に外していることを、実際に解放して確かめる。
fn media_handles_can_be_dropped(ui: &Ui) -> Result<()> {
    let fixture = Fixture::new("drop");
    let path = fixture.silent_wav("a.wav");
    {
        let audio = ui.audio(&path)?;
        audio.play();
        pump(0.2);
        let video = ui.video(&path)?;
        video.play();
    }
    // 解放後にランループを回しても、外れた観測者へ通知が飛ばないこと。
    pump(0.3);
    Ok(())
}

/// 再生中、再生位置が定期的にクロージャへ届く。
///
/// シークバーを再生に追従させるための通知。AVPlayer の
/// `addPeriodicTimeObserverForInterval:` から来る。
fn media_reports_position_while_playing(ui: &Ui) -> Result<()> {
    let fixture = Fixture::new("position");
    let audio = ui.audio(&fixture.silent_wav("a.wav"))?;
    let seen = Rc::new(RefCell::new(Vec::new()));
    audio.on_position_change({
        let seen = seen.clone();
        move |seconds| seen.borrow_mut().push(seconds)
    });

    pump_until(5.0, || audio.duration().is_some());
    audio.play();
    pump_until(5.0, || {
        let positions = seen.borrow();
        positions.len() >= 2 && positions.iter().any(|&p| p > 0.1)
    });

    let positions = seen.borrow().clone();
    // 0.5 秒のメディアを 0.25 秒間隔で観測するので、複数回届くはず。
    assert!(
        positions.len() >= 2,
        "再生中に複数回届くこと: {positions:?}"
    );
    assert!(
        positions.iter().any(|&p| p > 0.1),
        "0 以外の位置が届くこと: {positions:?}"
    );
    // 繰り返しは指定していないので、先頭へ巻き戻ることはない。
    //
    // ただし末尾では AVPlayer が「長さちょうど」へ丸めるため、直前の観測
    // (長さを僅かに越えた値) からごく小さく戻ることがある
    // (例: 0.500151 → 0.5)。それは巻き戻しではないので許容する。
    const SNAP_BACK: f64 = 0.05;
    assert!(
        positions.windows(2).all(|w| w[1] >= w[0] - SNAP_BACK),
        "位置が先頭へ戻らないこと: {positions:?}"
    );
    Ok(())
}

/// リストの選択が NSTableView と往復し、`set_selected` は通知しない。
fn list_selection_round_trips(ui: &Ui) -> Result<()> {
    let list = ui.list()?;
    list.set_items(&ListItem::list(["東京", "大阪", "札幌"]));
    assert_eq!(list.len(), 3);
    assert_eq!(list.selected(), None, "作った直後は何も選ばれていないこと");
    assert_eq!(list.selection_mode(), SelectionMode::Single);

    let seen: Rc<RefCell<Vec<Vec<usize>>>> = Rc::new(RefCell::new(Vec::new()));
    list.on_select({
        let seen = seen.clone();
        move |indices| seen.borrow_mut().push(indices.to_vec())
    });

    list.select(1);
    assert_eq!(list.selected(), Some(1));
    assert_eq!(*seen.borrow(), vec![vec![1]]);

    // ネイティブ側の選択も動いていること。
    let table = list.native_table();
    assert_eq!(table.selectedRow(), 1);
    assert_eq!(table.numberOfRows(), 3);

    // 単一選択では 2 行目を選ぶと 1 行目は外れる。
    list.select(2);
    assert_eq!(list.selection(), vec![2]);

    // `set_selected` は通知しない。
    list.set_selected(0);
    assert_eq!(list.selected(), Some(0));
    assert_eq!(*seen.borrow(), vec![vec![1], vec![2]], "通知は 2 回だけ");

    // 行を作り直すと選択は外れる。
    list.set_items(&ListItem::list(["那覇"]));
    assert_eq!(list.len(), 1);
    assert_eq!(list.selected(), None);
    assert_eq!(
        *seen.borrow(),
        vec![vec![1], vec![2]],
        "作り直しも通知しない"
    );
    Ok(())
}

/// 複数選択では複数行が選ばれ、選択が 0 件にもなる。
fn list_multiple_selection(ui: &Ui) -> Result<()> {
    let list = ui.list()?;
    list.set_items(&ListItem::list(["赤", "青", "緑", "黄"]));
    list.set_selection_mode(SelectionMode::Multiple);
    assert!(list.selection_mode().is_multiple());
    assert!(
        list.native_table().allowsMultipleSelection(),
        "NSTableView 側も複数選択になっていること"
    );

    let seen: Rc<RefCell<Vec<Vec<usize>>>> = Rc::new(RefCell::new(Vec::new()));
    list.on_select({
        let seen = seen.clone();
        move |indices| seen.borrow_mut().push(indices.to_vec())
    });

    // 並びは昇順にそろい、重複と範囲外は落ちる。
    list.select_many(&[3, 0, 3, 99]);
    assert_eq!(list.selection(), vec![0, 3]);
    assert_eq!(*seen.borrow(), vec![vec![0, 3]]);
    assert_eq!(list.selected(), Some(0), "selected は先頭の行を返すこと");

    // 0 件へも戻せる。
    list.select_many(&[]);
    assert_eq!(list.selection(), Vec::<usize>::new());
    assert_eq!(*seen.borrow(), vec![vec![0, 3], vec![]]);

    // clear_selection は通知しない。
    list.set_selection(&[1, 2]);
    assert_eq!(list.selection(), vec![1, 2]);
    list.clear_selection();
    assert!(list.selection().is_empty());
    assert_eq!(
        seen.borrow().len(),
        2,
        "set_selection と clear は通知しない"
    );

    // 単一選択へ戻すと選択は外れ、以後は 1 行だけになる。
    list.set_selection_mode(SelectionMode::Single);
    assert!(list.selection().is_empty());
    list.set_selection(&[1, 2]);
    assert_eq!(list.selection(), vec![1], "単一選択では先頭の 1 件だけ");
    Ok(())
}

/// 選べない行は、プログラムからもネイティブからも選ばれない。
fn list_skips_disabled_rows(ui: &Ui) -> Result<()> {
    let list = ui.list()?;
    list.set_items(&[
        ListItem::new("下書き"),
        ListItem::new("送信中").enabled(false),
        ListItem::new("送信済み"),
    ]);

    let seen: Rc<RefCell<Vec<Vec<usize>>>> = Rc::new(RefCell::new(Vec::new()));
    list.on_select({
        let seen = seen.clone();
        move |indices| seen.borrow_mut().push(indices.to_vec())
    });

    list.select(1);
    assert!(list.selection().is_empty(), "選べない行は選ばれないこと");
    assert_eq!(*seen.borrow(), vec![Vec::<usize>::new()]);

    list.select(2);
    assert_eq!(list.selection(), vec![2]);

    // AppKit 自身にも「この行は選べない」と伝わっている。
    let table = list.native_table();
    let delegate = unsafe { table.delegate() }.expect("デリゲートがあること");
    assert!(!delegate.tableView_shouldSelectRow(&table, 1));
    assert!(delegate.tableView_shouldSelectRow(&table, 2));
    Ok(())
}

/// ウィンドウのイベントキューへ本物のクリックを積み、配送まで進める。
///
/// `mouseDown:` を直接呼ぶのとは違い、AppKit のヒットテストと追跡ループを
/// そのまま通る。`NSTableView` のように「どのビューが受け取るか」で
/// ふるまいが変わるものは、この経路でないと確かめられない。
fn click_view(app: &NSApplication, window: &NSWindow, view: &NSView) {
    let bounds = view.bounds();
    let center = NSPoint::new(bounds.size.width / 2.0, bounds.size.height / 2.0);
    let point = view.convertPoint_toView(center, None);
    for kind in [NSEventType::LeftMouseDown, NSEventType::LeftMouseUp] {
        let event = NSEvent::mouseEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_clickCount_pressure(
            kind,
            point,
            NSEventModifierFlags::empty(),
            0.0,
            window.windowNumber(),
            None,
            0,
            1,
            1.0,
        )
        .expect("クリックイベント");
        unsafe { app.postEvent_atStart(&event, false) };
    }
    deliver_events(app);
    pump(0.05);
    deliver_events(app);
}

/// キューに溜まっているイベントをすべて配送する。
fn deliver_events(app: &NSApplication) {
    while let Some(event) = unsafe {
        app.nextEventMatchingMask_untilDate_inMode_dequeue(
            NSEventMask::Any,
            Some(&NSDate::distantPast()),
            NSDefaultRunLoopMode,
            true,
        )
    } {
        app.sendEvent(&event);
    }
}

/// 行そのもののクリックが `on_activate` へ届き、行内のコントロールを
/// 直接押したときは二重に発火しない。
///
/// AppKit は表示専用のラベルやアイコンの上のクリックを `NSTableView` へ渡す
/// (ヒットテストがセルではなくテーブルを返す) ため、naui はテーブルの action で
/// 行クリックを受けている。セル側の `mouseDown:` では実際のクリックが届かない
/// ので、ここでは本物のイベントを配送して確かめる。
fn list_row_activation_notifies(ui: &Ui) -> Result<()> {
    let mtm = MainThreadMarker::new().expect("メインスレッド");
    let app = NSApplication::sharedApplication(mtm);

    let window = ui.window("行のクリック", 480.0, 320.0)?;
    let root = ui.stack(Orientation::Vertical)?;
    root.set_sizing(Sizing::fill());

    let content = ui.grid()?;
    content.set_column_track(0, Track::Auto);
    content.set_column_track(1, Track::FILL);
    content.set_column_track(2, Track::Auto);
    content.set_spacing(10.0, 0.0);
    let check = ui.checkbox("")?;
    content.attach(&check, GridCell::new(0, 0));
    let text = ui.stack(Orientation::Vertical)?;
    text.set_align(Align::Start);
    let title = ui.label("Wi-Fi")?;
    text.append(&title);
    text.append(&ui.label("メニューバーに表示")?);
    text.set_sizing(Sizing::fill_width());
    content.attach(&text, GridCell::new(1, 0));
    let action = ui.button("オプション…")?;
    let clicks = Rc::new(Cell::new(0));
    action.on_click({
        let clicks = clicks.clone();
        move || clicks.set(clicks.get() + 1)
    });
    content.attach(&action, GridCell::new(2, 0));
    content.set_sizing(Sizing::fill_width());

    let row = ListRow::new(&content).selectable(false);
    let activated = Rc::new(Cell::new(0));
    row.on_activate({
        let check = check.clone();
        let activated = activated.clone();
        move || {
            activated.set(activated.get() + 1);
            check.set_checked(!check.is_checked());
        }
    });

    let list = ui.list()?;
    list.set_rows(&[row]);
    list.set_sizing(Sizing::fill_width());
    root.append(&list);
    window.set_child(&root);

    // 行クリックを受ける口が付いていること (見た目に依らない確認)。
    let table = list.native_table();
    assert!(
        unsafe { table.target() }.is_some(),
        "行クリックを受ける target が付いていること"
    );
    assert_eq!(
        table.action(),
        Some(sel!(invoke:)),
        "行クリックを action で受けていること"
    );

    // アクティブでないアプリのウィンドウは key になれず、AppKit は
    // `NSTableView` へクリックを渡さない。テストの間だけアクセサリにする。
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    #[allow(deprecated)]
    app.activateIgnoringOtherApps(true);
    window.show();
    let native = window.native_window();
    // アクティブ化はランループの上で進むので、key になるまで少し待つ。
    for _ in 0..40 {
        if native.isKeyWindow() {
            break;
        }
        pump(0.05);
        deliver_events(&app);
    }

    let content_view = native.contentView().expect("contentView");
    content_view.layoutSubtreeIfNeeded();
    // 行ビューは表示のときに作られるので、先に作らせてから描画まで進める。
    let _ = table.viewAtColumn_row_makeIfNecessary(0, 0, true);
    content_view.layoutSubtreeIfNeeded();
    unsafe { content_view.display() };

    if native.isKeyWindow() {
        click_view(&app, &native, &title.native_view());
        assert_eq!(activated.get(), 1, "行クリックが 1 回だけ通知されること");
        assert!(check.is_checked(), "行クリックからチェックが切り替わること");

        click_view(&app, &native, &check.native_view());
        assert!(
            !check.is_checked(),
            "チェックボックスの直接操作でも切り替わること"
        );
        assert_eq!(
            activated.get(),
            1,
            "チェックボックスの直接操作では行が二重発火しないこと"
        );

        click_view(&app, &native, &action.native_view());
        assert_eq!(clicks.get(), 1, "行内ボタンのクリックが届くこと");
        assert_eq!(
            activated.get(),
            1,
            "行内ボタンの直接操作では行 activation が発火しないこと"
        );
    } else {
        // ウィンドウを key にできない環境 (画面セッションが無いなど) では、
        // AppKit がクリックを配送しないので確かめられない。
        println!("(key ウィンドウにできないため、実クリックの確認は飛ばしました)");
    }

    window.close();
    app.setActivationPolicy(NSApplicationActivationPolicy::Prohibited);
    Ok(())
}

/// 設定画面向けの行は、ラベルだけでなく任意のレイアウトとコントロールを持てる。
fn list_accepts_composed_rows(ui: &Ui) -> Result<()> {
    let content = ui.grid()?;
    content.set_column_track(0, Track::Auto);
    content.set_column_track(1, Track::FILL);
    content.set_column_track(2, Track::Auto);
    content.set_spacing(8.0, 0.0);
    let check = ui.checkbox("")?;
    let text = ui.stack(Orientation::Vertical)?;
    text.set_align(Align::Start);
    text.set_spacing(2.0);
    let title = ui.label("Wi-Fi")?;
    let detail = ui.label("メニューバーに表示")?;
    text.append(&title);
    text.append(&detail);
    text.set_sizing(Sizing::fill_width());
    let action = ui.button("オプション…")?;
    content.attach(&check, GridCell::new(0, 0));
    content.attach(&text, GridCell::new(1, 0));
    content.attach(&action, GridCell::new(2, 0));
    content.set_sizing(Sizing::fill_width());
    let make_row = |title: &str, detail: &str, action: &str| -> Result<naui_macos::Grid> {
        let row = ui.grid()?;
        row.set_column_track(0, Track::Auto);
        row.set_column_track(1, Track::FILL);
        row.set_column_track(2, Track::Auto);
        row.set_spacing(8.0, 0.0);
        let text = ui.stack(Orientation::Vertical)?;
        text.set_align(Align::Start);
        text.set_spacing(2.0);
        text.append(&ui.label(title)?);
        text.append(&ui.label(detail)?);
        text.set_sizing(Sizing::fill_width());
        row.attach(&ui.checkbox("")?, GridCell::new(0, 0));
        row.attach(&text, GridCell::new(1, 0));
        row.attach(&ui.button(action)?, GridCell::new(2, 0));
        row.set_sizing(Sizing::fill_width());
        Ok(row)
    };
    let second = make_row("Bluetooth", "近くのデバイスを管理", "詳細…")?;
    let last = make_row("バッテリー", "残量を表示", "設定…")?;
    let row_contents = [content.clone(), second.clone(), last.clone()];

    let list = ui.list()?;
    list.set_rows(&[
        ListRow::new(&content).selectable(false),
        ListRow::new(&second).selectable(false),
        ListRow::new(&last).selectable(false),
    ]);
    list.set_sizing(Sizing::fill_width());
    assert_eq!(list.len(), 3);

    // 行内のコントロールだけを操作する 1 行目は、プログラムからも選べない。
    list.select(0);
    assert!(list.selection().is_empty());
    list.select(1);
    assert!(list.selection().is_empty());

    let table = list.native_table();
    let mount = ui.stack(Orientation::Vertical)?;
    mount.append(&list);
    let root = mount.native_view();
    root.setFrameSize(NSSize::new(900.0, 240.0));
    // 1 回目で Grid の幅と各セルの fitting height が決まり、List の
    // intrinsic height が更新される。2 回目で親 Stack まで解き直す。
    root.layoutSubtreeIfNeeded();
    root.layoutSubtreeIfNeeded();
    let first = table
        .viewAtColumn_row_makeIfNecessary(0, 0, true)
        .expect("任意内容の行ビュー");
    assert_eq!(first.subviews().len(), 1, "組み立てた内容が 1 つ載ること");
    assert!(
        first.subviews().objectAtIndex(0).subviews().len() >= 3,
        "渡した Grid の各要素が保たれること"
    );

    let check_frame = check.native_view().frame();
    let text_frame = text.native_view().frame();
    let action_frame = action.native_view().frame();
    assert!(
        text_frame.origin.x - (check_frame.origin.x + check_frame.size.width) < 16.0,
        "本文はチェックの直後に置かれること: {check_frame:?} / {text_frame:?}"
    );
    assert!(
        text_frame.origin.x < 50.0 && action_frame.origin.x > 600.0,
        "本文列は左から始まり、操作は右端へ寄ること: {text_frame:?} / {action_frame:?}"
    );

    // 行高そのものだけでなく、全内容が行内へ収まり、2 本のラベルが互いに
    // 潰れていないことを確認する。これが無いと 40pt 必要な Stack を 24pt の
    // Grid 行へ押し込み、見た目では隣接行まで重なる不具合を見逃す。
    for (index, row_content) in row_contents.iter().enumerate() {
        let cell = table
            .viewAtColumn_row_makeIfNecessary(0, index as isize, true)
            .expect("任意内容の行ビュー");
        let row_rect = table.rectOfRow(index as isize);
        let grid_frame = row_content.native_view().frame();
        assert!(
            grid_frame.origin.y >= 0.0
                && grid_frame.origin.y + grid_frame.size.height <= cell.frame().size.height + 0.5,
            "{index} 行目の Grid がセル内に収まること: 行 {row_rect:?} / セル {:?} / Grid {grid_frame:?}",
            cell.frame()
        );
        assert!(
            row_rect.size.height >= grid_frame.size.height + 8.0 - 0.5,
            "{index} 行目が内容高と上下余白を持つこと: 行 {row_rect:?} / Grid {grid_frame:?}"
        );
        let grid = row_content.native_view();
        for child_index in 0..grid.subviews().len() {
            let child = grid.subviews().objectAtIndex(child_index);
            let frame = child.frame();
            assert!(
                frame.origin.y >= -0.5
                    && frame.origin.y + frame.size.height <= grid_frame.size.height + 0.5,
                "{index} 行目の子 {child_index} が Grid 内に収まること: {frame:?} / Grid {grid_frame:?}"
            );
        }
    }
    let title_frame = title.native_view().frame();
    let detail_frame = detail.native_view().frame();
    assert!(
        title_frame.size.height > 0.0 && detail_frame.size.height > 0.0,
        "タイトルと補足がどちらも描画高を持つこと: {title_frame:?} / {detail_frame:?}"
    );
    assert!(
        detail_frame.origin.y + detail_frame.size.height <= title_frame.origin.y + 0.5,
        "タイトルと補足が重ならないこと: {title_frame:?} / {detail_frame:?}"
    );

    // ギャラリーと同じ 3 行を固定高さなしで表示し、最後の行の下に
    // 大きな空白を残さない。横位置だけではリスト全体の崩れを見逃すため、
    // Auto で決まった可視高さとスクロールの要否も確かめる。
    let scroll = list
        .native_view()
        .downcast::<NSScrollView>()
        .expect("NSScrollView");
    let visible_bottom = scroll.contentView().bounds().size.height;
    let document_height = table.frame().size.height;
    let final_row = table.rectOfRow(2);
    let rows_bottom = final_row.origin.y + final_row.size.height;
    let trailing_space = visible_bottom - rows_bottom;
    assert!(
        (0.0..=16.0).contains(&trailing_space),
        "最終行の下に大きな空白が無いこと: 可視高 {visible_bottom} / 行の下端 {rows_bottom} / 空白 {trailing_space}"
    );
    assert!(
        document_height <= visible_bottom,
        "3 行だけならスクロールを必要としないこと: 中身 {document_height} / 可視高 {visible_bottom}"
    );
    let vertical_scroller = scroll.verticalScroller().expect("縦スクローラー");
    assert!(
        !vertical_scroller.isEnabled(),
        "3 行が収まるときは縦スクローラーを無効にすること"
    );
    // 表示後に内容が変わっても、作成時に測った固定値へ留まらないこと。
    let previous_row_height = table.rectOfRow(0).size.height;
    let extra = ui.label("接続済み")?;
    text.append(&extra);
    root.layoutSubtreeIfNeeded();
    root.layoutSubtreeIfNeeded();
    let expanded_row = table.rectOfRow(0);
    let expanded_grid = content.native_view().frame();
    assert!(
        expanded_row.size.height > previous_row_height + 8.0,
        "Stack の内容追加後に Auto 行が高くなること: 変更前 {previous_row_height} / 変更後 {expanded_row:?}"
    );
    assert!(
        expanded_grid.origin.y + expanded_grid.size.height
            <= table
                .viewAtColumn_row_makeIfNecessary(0, 0, true)
                .expect("更新後の行ビュー")
                .frame()
                .size
                .height
                + 0.5,
        "更新後の Grid も行内に収まること: {expanded_grid:?} / 行 {expanded_row:?}"
    );
    assert!(extra.native_view().frame().size.height > 0.0);

    let expanded_list_height = scroll.contentView().bounds().size.height;
    list.set_rows(&[ListRow::new(&content).selectable(false)]);
    root.layoutSubtreeIfNeeded();
    root.layoutSubtreeIfNeeded();
    let one_row_height = scroll.contentView().bounds().size.height;
    assert!(
        one_row_height < expanded_list_height,
        "行を減らしたら Auto の高さも縮むこと: 3 行 {expanded_list_height} / 1 行 {one_row_height}"
    );
    Ok(())
}

/// `Ui` は clone できるので、コールバックの中からでもウィジェットを作れる。
///
/// 行の中身は `build` の中で全部そろうとは限らない。押されたときに
/// 1 行増やすような画面では、通知の中で新しいウィジェットを作って
/// [`List::set_rows`] へ渡す必要がある。
fn ui_clone_builds_rows_from_a_callback(ui: &Ui) -> Result<()> {
    let list = ui.list()?;
    list.set_sizing(Sizing::fill_width());

    // 行は積み上げていくので、アプリ側で並びを持つ。
    let rows: Rc<RefCell<Vec<ListRow>>> = Rc::new(RefCell::new(Vec::new()));
    let add = ui.button("行を足す")?;
    add.on_click({
        // コールバックへ持ち込むのは clone した `Ui`。中身は同じ。
        let ui = ui.clone();
        let list = list.clone();
        let rows = rows.clone();
        move || {
            let index = rows.borrow().len() + 1;
            let content = ui.stack(Orientation::Horizontal).expect("行の中身");
            content.append(&ui.label(&format!("行 {index}")).expect("行のラベル"));
            content.set_sizing(Sizing::fill_width());
            rows.borrow_mut().push(ListRow::new(&content));
            list.set_rows(&rows.borrow());
        }
    });

    assert_eq!(list.len(), 0);
    add.click();
    add.click();
    assert_eq!(list.len(), 2, "コールバックの中で作った行が載ること");

    let table = list.native_table();
    assert_eq!(table.numberOfRows(), 2, "NSTableView 側も 2 行になること");

    // 後から足した行も、他の行と同じようにネイティブのビューになる。
    let stack = ui.stack(Orientation::Vertical)?;
    stack.append(&list);
    let root = stack.native_view();
    root.setFrameSize(NSSize::new(400.0, 300.0));
    root.layoutSubtreeIfNeeded();
    assert!(
        table.viewAtColumn_row_makeIfNecessary(0, 1, true).is_some(),
        "後から足した行が NSTableView に描かれること"
    );

    // 選択もふつうの行と同じに動く。
    list.select(1);
    assert_eq!(list.selected(), Some(1));
    assert_eq!(table.selectedRow(), 1);
    Ok(())
}

/// 行の中身は AppKit がデリゲートに作らせた NSTextField になる。
fn list_rows_are_native_views(ui: &Ui) -> Result<()> {
    let list = ui.list()?;
    list.set_items(&ListItem::list(["日本語の行", "ASCII row"]));
    list.set_sizing(Sizing::fixed(240.0, 120.0));

    // 大きさの制約は親のレイアウトで効くので、スタックに入れてから測る。
    let stack = ui.stack(Orientation::Vertical)?;
    stack.append(&list);
    let root = stack.native_view();
    root.setFrameSize(NSSize::new(400.0, 300.0));
    root.layoutSubtreeIfNeeded();

    // スクロールビューは大きさの指定どおりになり、表は幅いっぱいに広がる。
    let frame = list.native_view().frame();
    assert!(
        (frame.size.width - 240.0).abs() < 1e-6 && (frame.size.height - 120.0).abs() < 1e-6,
        "指定した大きさになること: {frame:?}"
    );
    let table = list.native_table();
    assert!(
        table.frame().size.width > 0.0,
        "NSTableView がスクロールの中で幅を持つこと"
    );

    let cell = table
        .viewAtColumn_row_makeIfNecessary(0, 0, true)
        .expect("1 行目のビューが作られること")
        .downcast::<objc2_app_kit::NSTableCellView>()
        .expect("行は NSTableCellView であること");
    let field = unsafe { cell.textField() }.expect("行に文字が入っていること");
    assert_eq!(field.stringValue().to_string(), "日本語の行");
    Ok(())
}

// ------------------------------------------------------------------ Table
/// 列の合計幅 (列の間の余白こみ) と、表そのものの幅。
fn table_widths(table: &naui_macos::Table) -> (f64, f64) {
    let native = table.native_table();
    let columns = native.tableColumns();
    let total: f64 = (0..columns.len())
        .map(|i| columns.objectAtIndex(i).width())
        .sum();
    let spacing = native.intercellSpacing().width * columns.len() as f64;
    (total + spacing, native.frame().size.width)
}

/// 列を減らして戻しても、幅を指定していない列が余りを受け取り続ける。
///
/// AppKit の列の自動調整は、幅を固定した列があると余りを配りきれない
/// (表の右側が空いたままになる)。naui が配り直していることを確かめる。
fn table_columns_keep_filling_the_width(ui: &Ui) -> Result<()> {
    let wide = vec![
        TableColumn::new("都市"),
        TableColumn::new("人口").width(120.0).align(Align::End),
        TableColumn::new("面積").width(100.0).align(Align::End),
    ];
    let narrow = vec![
        TableColumn::new("都市"),
        TableColumn::new("人口").width(120.0).align(Align::End),
    ];

    let table = ui.table()?;
    table.set_columns(&wide);
    table.set_rows(&TableRow::list([["東京", "13,960,000", "2,194"]]));
    table.set_sizing(Sizing::fixed(400.0, 120.0));

    let stack = ui.stack(Orientation::Vertical)?;
    stack.append(&table);
    let root = stack.native_view();
    root.setFrameSize(NSSize::new(500.0, 300.0));
    root.layoutSubtreeIfNeeded();

    let (total, width) = table_widths(&table);
    assert!(
        width > 0.0 && (total - width).abs() < 2.0,
        "はじめは幅いっぱいを使うこと: 列の合計 {total} / 表の幅 {width}"
    );

    // 列を減らして、戻す (ギャラリーの「面積を隠す / 表示」と同じ)。
    table.set_columns(&narrow);
    root.layoutSubtreeIfNeeded();
    let (total, width) = table_widths(&table);
    assert!(
        (total - width).abs() < 2.0,
        "列を減らしても幅いっぱいを使うこと: 列の合計 {total} / 表の幅 {width}"
    );

    table.set_columns(&wide);
    root.layoutSubtreeIfNeeded();
    let (total, width) = table_widths(&table);
    assert!(
        (total - width).abs() < 2.0,
        "列を戻しても幅いっぱいを使うこと: 列の合計 {total} / 表の幅 {width}"
    );

    // 表そのものが広がったときも、余りは指定の無い列が受け取る。
    table.set_sizing(Sizing::fixed(560.0, 120.0));
    root.setFrameSize(NSSize::new(700.0, 300.0));
    root.layoutSubtreeIfNeeded();
    let (total, width) = table_widths(&table);
    assert!(
        (total - width).abs() < 2.0,
        "広げた分も受け取ること: 列の合計 {total} / 表の幅 {width}"
    );

    // 幅を指定した列は動かない。
    let columns = table.native_table().tableColumns();
    assert_eq!(columns.objectAtIndex(1).width(), 120.0);
    assert_eq!(columns.objectAtIndex(2).width(), 100.0);
    Ok(())
}

/// 見出しを押した並べ替えが、AppKit と往復する。
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

    let seen: Rc<RefCell<Vec<(usize, SortOrder)>>> = Rc::new(RefCell::new(Vec::new()));
    table.on_sort({
        let seen = seen.clone();
        move |column, order| seen.borrow_mut().push((column, order))
    });

    let native = table.native_table();
    let columns = native.tableColumns();
    // 並べ替えられる列にだけ、AppKit の「押せるヘッダー」が付く。
    assert!(columns.objectAtIndex(0).sortDescriptorPrototype().is_some());
    assert!(columns.objectAtIndex(1).sortDescriptorPrototype().is_some());
    assert!(
        columns.objectAtIndex(2).sortDescriptorPrototype().is_none(),
        "sortable でない列は押せないこと"
    );

    // 利用者が見出しを押したときと同じ経路 (AppKit が sortDescriptors を差し替える)。
    let prototype = columns
        .objectAtIndex(1)
        .sortDescriptorPrototype()
        .expect("押せる列であること");
    native.setSortDescriptors(&objc2_foundation::NSArray::from_retained_slice(&[
        prototype,
    ]));
    assert_eq!(table.sort(), Some((1, SortOrder::Ascending)));
    assert_eq!(*seen.borrow(), vec![(1, SortOrder::Ascending)]);

    // 逆向きも同じ経路で届く。
    let reversed = objc2_foundation::NSSortDescriptor::sortDescriptorWithKey_ascending(
        Some(&objc2_foundation::NSString::from_str("naui.table.column.1")),
        false,
    );
    native.setSortDescriptors(&objc2_foundation::NSArray::from_retained_slice(&[reversed]));
    assert_eq!(table.sort(), Some((1, SortOrder::Descending)));
    assert_eq!(seen.borrow().len(), 2);

    // set_sort は通知しない。
    table.set_sort(Some((0, SortOrder::Ascending)));
    assert_eq!(table.sort(), Some((0, SortOrder::Ascending)));
    assert_eq!(seen.borrow().len(), 2, "set_sort は通知しないこと");

    // 並べ替えられない列を指すと、指定は外れる。
    table.set_sort(Some((2, SortOrder::Ascending)));
    assert_eq!(table.sort(), None);

    // 列を作り直しても、指定は残る。
    table.set_sort(Some((1, SortOrder::Descending)));
    table.set_columns(&[
        TableColumn::new("都市").sortable(true),
        TableColumn::new("人口").align(Align::End).sortable(true),
    ]);
    assert_eq!(table.sort(), Some((1, SortOrder::Descending)));
    assert_eq!(seen.borrow().len(), 2, "列の作り直しでも通知しないこと");
    Ok(())
}

/// 画面に出ている表の列を、ギャラリーの「面積を隠す / 表示」と同じように
/// 何度も入れ替える。
///
/// **セルのビューを作らせてから列を入れ替えるのが肝。** 行の高さを AppKit に
/// 求めさせる (`usesAutomaticRowHeights`) と、次の `addTableColumn:` で
/// 「no common ancestor」の例外になり、表がそこで壊れる。
fn table_survives_repeated_column_changes(ui: &Ui) -> Result<()> {
    let wide = vec![
        TableColumn::new("都市"),
        TableColumn::new("人口").width(120.0).align(Align::End),
        TableColumn::new("面積").width(100.0).align(Align::End),
    ];
    let narrow = vec![
        TableColumn::new("都市"),
        TableColumn::new("人口").width(120.0).align(Align::End),
    ];
    let rows = vec![
        TableRow::new(["東京", "13,960,000", "2,194"]),
        TableRow::new(["大阪", "8,838,000", "1,905"]),
        TableRow::new(["集計中", "—", "—"]).enabled(false),
        TableRow::new(["札幌", "1,973,000", "1,121"]),
    ];

    let table = ui.table()?;
    table.set_columns(&wide);
    table.set_rows(&rows);

    let seen: Rc<RefCell<Vec<Vec<usize>>>> = Rc::new(RefCell::new(Vec::new()));
    table.on_select({
        let seen = seen.clone();
        move |indices| seen.borrow_mut().push(indices.to_vec())
    });

    let native = table.native_table();
    // 行の高さは文字より高く、どの行も同じ (自動ではない)。
    assert!(
        native.rowHeight() > 16.0,
        "行の高さが文字に足りること: {}",
        native.rowHeight()
    );

    for round in 0..2 {
        table.select(1);
        assert_eq!(table.selection(), vec![1], "{round} 周目: 選べること");

        table.set_columns(&narrow);
        assert_eq!(native.tableColumns().len(), 2, "{round} 周目: 列が減ること");
        assert_eq!(table.selection(), vec![1], "{round} 周目: 選択が残ること");

        table.set_columns(&wide);
        assert_eq!(native.tableColumns().len(), 3, "{round} 周目: 列が戻ること");

        // 戻した列のセルを実体化する (画面に出ているのと同じ状態にする)。
        let cell = native
            .viewAtColumn_row_makeIfNecessary(2, 0, true)
            .expect("3 列目のセルが作られること")
            .downcast::<objc2_app_kit::NSTableCellView>()
            .expect("セルは NSTableCellView であること");
        let field = unsafe { cell.textField() }.expect("セルに文字が入っていること");
        assert_eq!(field.stringValue().to_string(), "2,194");

        // 列を入れ替えた後も、選択を切り替えられる。
        table.select(3);
        assert_eq!(
            table.selection(),
            vec![3],
            "{round} 周目: 選択を変えられること"
        );
        table.clear_selection();
    }
    assert_eq!(
        *seen.borrow(),
        vec![vec![1], vec![3], vec![1], vec![3]],
        "通知は select のぶんだけ出ること"
    );
    Ok(())
}

/// 列と行が、そのまま `NSTableView` の列と行になる。
fn table_columns_and_rows_are_native(ui: &Ui) -> Result<()> {
    let table = ui.table()?;
    table.set_columns(&TableColumn::list(["都市", "人口"]));
    table.set_rows(&TableRow::list([
        ["東京", "13,960,000"],
        ["大阪", "8,838,000"],
    ]));
    assert_eq!(table.column_count(), 2);
    assert_eq!(table.len(), 2);
    assert!(!table.is_empty());

    let native = table.native_table();
    assert_eq!(native.numberOfRows(), 2);
    assert_eq!(native.tableColumns().len(), 2);
    assert!(native.headerView().is_some(), "列見出しが出ていること");
    assert_eq!(
        native.tableColumns().objectAtIndex(1).title().to_string(),
        "人口"
    );

    // セルは NSTableCellView で、列と行の交点の文字が入る。
    let cell = native
        .viewAtColumn_row_makeIfNecessary(1, 0, true)
        .expect("1 行目 2 列目のビューが作られること")
        .downcast::<objc2_app_kit::NSTableCellView>()
        .expect("セルは NSTableCellView であること");
    let field = unsafe { cell.textField() }.expect("セルに文字が入っていること");
    assert_eq!(field.stringValue().to_string(), "13,960,000");

    // 列より短い行は、足りない分が空のセルになる。
    table.set_rows(&[TableRow::new(["札幌"])]);
    let cell = native
        .viewAtColumn_row_makeIfNecessary(1, 0, true)
        .expect("セルが作られること")
        .downcast::<objc2_app_kit::NSTableCellView>()
        .expect("セルは NSTableCellView であること");
    let field = unsafe { cell.textField() }.expect("セルに文字が入っていること");
    assert_eq!(field.stringValue().to_string(), "");
    Ok(())
}

/// 幅を指定した列は固定され、指定の無い列だけが余りを分け合う。
/// 文字の揃えは、セルと見出しの両方に効く。
fn table_columns_follow_the_spec(ui: &Ui) -> Result<()> {
    let table = ui.table()?;
    table.set_columns(&[
        TableColumn::new("品名"),
        TableColumn::new("金額").width(120.0).align(Align::End),
    ]);
    table.set_rows(&[TableRow::new(["珈琲", "¥480"])]);

    let native = table.native_table();
    let columns = native.tableColumns();
    let flexible = columns.objectAtIndex(0);
    let fixed = columns.objectAtIndex(1);
    assert!(
        flexible.width() >= 40.0 && flexible.maxWidth() > 120.0,
        "指定の無い列は伸び縮みできること: {}",
        flexible.width()
    );
    assert_eq!(fixed.width(), 120.0);
    assert_eq!(fixed.minWidth(), 120.0, "指定した列は固定されること");
    assert_eq!(fixed.maxWidth(), 120.0);
    assert_eq!(
        fixed.headerCell().alignment(),
        objc2_app_kit::NSTextAlignment::Right,
        "見出しもセルと同じ揃えになること"
    );

    let cell = native
        .viewAtColumn_row_makeIfNecessary(1, 0, true)
        .expect("セルが作られること")
        .downcast::<objc2_app_kit::NSTableCellView>()
        .expect("セルは NSTableCellView であること");
    let field = unsafe { cell.textField() }.expect("セルに文字が入っていること");
    assert_eq!(field.alignment(), objc2_app_kit::NSTextAlignment::Right);

    // 揃えを効かせるため、セルの文字は列いっぱいに広がる。
    table.set_sizing(Sizing::fixed(320.0, 120.0));
    let stack = ui.stack(Orientation::Vertical)?;
    stack.append(&table);
    let root = stack.native_view();
    root.setFrameSize(NSSize::new(400.0, 300.0));
    root.layoutSubtreeIfNeeded();
    let cell = native
        .viewAtColumn_row_makeIfNecessary(1, 0, true)
        .expect("セルが作られること");
    cell.layoutSubtreeIfNeeded();
    let field_width = unsafe { cell.downcast_ref::<objc2_app_kit::NSTableCellView>() }
        .and_then(|cell| unsafe { cell.textField() })
        .map(|field| field.frame().size.width)
        .unwrap_or_default();
    assert!(
        field_width > cell.frame().size.width - 20.0,
        "セルの文字が列いっぱいに広がること: {field_width} / {}",
        cell.frame().size.width
    );
    Ok(())
}

/// 選択は行のインデックスで往復し、通知もそのまま届く。
fn table_selection_round_trips(ui: &Ui) -> Result<()> {
    let table = ui.table()?;
    table.set_columns(&TableColumn::list(["名前", "係"]));
    table.set_rows(&TableRow::list([
        ["朝比奈", "受付"],
        ["三上", "会計"],
        ["若宮", "記録"],
    ]));
    assert_eq!(table.selected(), None, "作った直後は何も選ばれていないこと");
    assert_eq!(table.selection_mode(), SelectionMode::Single);

    let seen: Rc<RefCell<Vec<Vec<usize>>>> = Rc::new(RefCell::new(Vec::new()));
    table.on_select({
        let seen = seen.clone();
        move |indices| seen.borrow_mut().push(indices.to_vec())
    });

    table.select(1);
    assert_eq!(table.selected(), Some(1));
    assert_eq!(table.native_table().selectedRow(), 1);
    assert_eq!(*seen.borrow(), vec![vec![1]]);

    // 通知なしの経路では、クロージャは呼ばれない。
    table.set_selected(2);
    assert_eq!(table.selection(), vec![2]);
    assert_eq!(*seen.borrow(), vec![vec![1]]);
    table.clear_selection();
    assert!(table.selection().is_empty());
    assert_eq!(*seen.borrow(), vec![vec![1]]);

    // 複数選択では、昇順にそろい 0 件にもなる。
    table.set_selection_mode(SelectionMode::Multiple);
    assert!(table.native_table().allowsMultipleSelection());
    table.select_many(&[2, 0, 2, 99]);
    assert_eq!(table.selection(), vec![0, 2]);
    assert_eq!(*seen.borrow(), vec![vec![1], vec![0, 2]]);
    table.select_many(&[]);
    assert_eq!(table.selection(), Vec::<usize>::new());
    assert_eq!(*seen.borrow(), vec![vec![1], vec![0, 2], vec![]]);

    // 行を作り直すと選択は外れる。
    table.select(1);
    table.set_rows(&TableRow::list([["新", "係"]]));
    assert!(
        table.selection().is_empty(),
        "行の入れ替えで選択が外れること"
    );
    Ok(())
}

/// 選べない行は、naui からも AppKit からも選べない。
fn table_skips_disabled_rows(ui: &Ui) -> Result<()> {
    let table = ui.table()?;
    table.set_columns(&TableColumn::list(["状態", "件数"]));
    table.set_rows(&[
        TableRow::new(["下書き", "3"]),
        TableRow::new(["送信中", "1"]).enabled(false),
        TableRow::new(["送信済み", "12"]),
    ]);

    let seen: Rc<RefCell<Vec<Vec<usize>>>> = Rc::new(RefCell::new(Vec::new()));
    table.on_select({
        let seen = seen.clone();
        move |indices| seen.borrow_mut().push(indices.to_vec())
    });

    table.select(1);
    assert!(table.selection().is_empty(), "選べない行は選ばれないこと");
    assert_eq!(*seen.borrow(), vec![Vec::<usize>::new()]);
    table.select(2);
    assert_eq!(table.selection(), vec![2]);

    // AppKit 自身にも「この行は選べない」と伝わっている。
    let native = table.native_table();
    let delegate = unsafe { native.delegate() }.expect("デリゲートがあること");
    assert!(!delegate.tableView_shouldSelectRow(&native, 1));
    assert!(delegate.tableView_shouldSelectRow(&native, 2));
    Ok(())
}

/// 列だけを差し替えても、行の中身は残ったまま並べ直される。
fn table_keeps_rows_when_columns_change(ui: &Ui) -> Result<()> {
    let table = ui.table()?;
    table.set_columns(&TableColumn::list(["都市", "人口", "面積"]));
    table.set_rows(&TableRow::list([["東京", "13,960,000", "2,194"]]));

    table.set_selected(0);
    table.set_columns(&TableColumn::list(["都市", "人口"]));
    assert_eq!(table.column_count(), 2);
    assert_eq!(table.len(), 1, "行はそのまま残ること");
    assert_eq!(table.selection(), vec![0], "選択もそのまま残ること");

    let native = table.native_table();
    assert_eq!(native.tableColumns().len(), 2, "古い列が残らないこと");
    let cell = native
        .viewAtColumn_row_makeIfNecessary(1, 0, true)
        .expect("セルが作られること")
        .downcast::<objc2_app_kit::NSTableCellView>()
        .expect("セルは NSTableCellView であること");
    let field = unsafe { cell.textField() }.expect("セルに文字が入っていること");
    assert_eq!(field.stringValue().to_string(), "13,960,000");
    Ok(())
}

/// AppKit 側で選択が変わったときも、そのままクロージャへ届く。
fn table_native_selection_notifies(ui: &Ui) -> Result<()> {
    let table = ui.table()?;
    table.set_columns(&TableColumn::list(["時間帯"]));
    table.set_rows(&TableRow::list([["朝"], ["昼"], ["夜"]]));
    table.set_selection_mode(SelectionMode::Multiple);

    let seen: Rc<RefCell<Vec<Vec<usize>>>> = Rc::new(RefCell::new(Vec::new()));
    table.on_select({
        let seen = seen.clone();
        move |indices| seen.borrow_mut().push(indices.to_vec())
    });

    let native = table.native_table();
    let rows = objc2_foundation::NSMutableIndexSet::new();
    rows.addIndex(0);
    rows.addIndex(2);
    native.selectRowIndexes_byExtendingSelection(&rows, false);

    assert_eq!(
        *seen.borrow(),
        vec![vec![0, 2]],
        "ネイティブ側の選択がそのまま届くこと"
    );
    assert_eq!(table.selection(), vec![0, 2]);
    Ok(())
}

/// AppKit 側で選択が変わったときも、そのままクロージャへ届く。
///
/// `select` は naui が通知を出す経路なので、ここでは naui を通さずに
/// `NSTableView` を直接動かし、デリゲート経由の通知を確かめる。
fn list_native_selection_notifies(ui: &Ui) -> Result<()> {
    let list = ui.list()?;
    list.set_items(&ListItem::list(["朝", "昼", "夜"]));
    list.set_selection_mode(SelectionMode::Multiple);

    let seen: Rc<RefCell<Vec<Vec<usize>>>> = Rc::new(RefCell::new(Vec::new()));
    list.on_select({
        let seen = seen.clone();
        move |indices| seen.borrow_mut().push(indices.to_vec())
    });

    let table = list.native_table();
    let rows = objc2_foundation::NSMutableIndexSet::new();
    rows.addIndex(0);
    rows.addIndex(2);
    table.selectRowIndexes_byExtendingSelection(&rows, false);

    assert_eq!(
        *seen.borrow(),
        vec![vec![0, 2]],
        "ネイティブ側の選択がそのまま届くこと"
    );
    assert_eq!(list.selection(), vec![0, 2]);
    Ok(())
}

/// 通知の中から同じリストを触っても、AppKit ごと壊れない。
///
/// `on_select` の中で行を作り直すと、AppKit がデリゲートを呼んでいる最中に
/// `reloadData` が走る。ギャラリーの「選択に合わせて表示を変える」形が
/// まさにこれなので、実際にやって確かめる。
fn list_callback_can_touch_the_list(ui: &Ui) -> Result<()> {
    let list = ui.list()?;
    list.set_items(&ListItem::list(["春", "夏", "秋", "冬"]));

    let seen: Rc<RefCell<Vec<Vec<usize>>>> = Rc::new(RefCell::new(Vec::new()));
    list.on_select({
        let seen = seen.clone();
        let list = list.clone();
        move |indices| {
            seen.borrow_mut().push(indices.to_vec());
            // 1 回目の通知でだけ、中身ごと差し替える。
            if seen.borrow().len() == 1 {
                list.set_items(&ListItem::list(["朝", "昼"]));
                list.set_selected(1);
            }
        }
    });

    // naui を通さず、AppKit 側から選択を起こす。
    let rows = objc2_foundation::NSMutableIndexSet::new();
    rows.addIndex(2);
    list.native_table()
        .selectRowIndexes_byExtendingSelection(&rows, false);

    assert_eq!(*seen.borrow(), vec![vec![2]], "通知は 1 回だけ");
    assert_eq!(list.len(), 2, "コールバックの中の差し替えが効くこと");
    assert_eq!(list.selected(), Some(1));
    Ok(())
}

/// 行の文字が、行の高さの縦中央に来る。
///
/// 文字だけの `NSTextField` を行にすると、行の高さ (Inset スタイルでは 24pt) に
/// 対して文字の高さ (16pt 前後) が足りず、AppKit が上寄せで描いてしまう。
/// 選択の帯は行いっぱいに出るので、そのままだと帯と文字がずれる。
fn list_rows_are_vertically_centered(ui: &Ui) -> Result<()> {
    let list = ui.list()?;
    list.set_items(&ListItem::list(["札幌", "仙台", "東京"]));
    list.set_sizing(Sizing::fixed(240.0, 180.0));
    let stack = ui.stack(Orientation::Vertical)?;
    stack.append(&list);
    let root = stack.native_view();
    root.setFrameSize(NSSize::new(400.0, 300.0));
    root.layoutSubtreeIfNeeded();

    let table = list.native_table();
    let cell = table
        .viewAtColumn_row_makeIfNecessary(0, 1, true)
        .expect("行のビューが作られること");
    let subviews = cell.subviews();
    assert_eq!(subviews.len(), 1, "行の中身は文字 1 つ");
    let field = subviews.objectAtIndex(0).frame();
    let cell_height = cell.frame().size.height;

    assert!(
        field.size.height < cell_height,
        "文字は行より低いこと (縦中央ぞろえが要る状況であること): {field:?} / {cell_height}"
    );
    let field_center = field.origin.y + field.size.height / 2.0;
    assert!(
        (field_center - cell_height / 2.0).abs() < 0.5,
        "文字が行の縦中央にあること: 文字の中心 {field_center} / 行の中心 {}",
        cell_height / 2.0
    );
    Ok(())
}

/// `detail` を付けた行だけが 2 行になり、そのぶん高くなる。
///
/// 行の高さを決めるのは AppKit (`usesAutomaticRowHeights`) なので、
/// naui 側は制約を張るだけ。高さの数値ではなく「1 行より高い」ことを見る。
fn list_detail_makes_a_second_line(ui: &Ui) -> Result<()> {
    let list = ui.list()?;
    list.set_items(&[
        ListItem::new("東京"),
        ListItem::new("大阪").detail("2,750,000 人"),
    ]);
    list.set_sizing(Sizing::fixed(240.0, 180.0));
    let stack = ui.stack(Orientation::Vertical)?;
    stack.append(&list);
    let root = stack.native_view();
    root.setFrameSize(NSSize::new(400.0, 300.0));
    root.layoutSubtreeIfNeeded();

    let table = list.native_table();
    let plain = table
        .viewAtColumn_row_makeIfNecessary(0, 0, true)
        .expect("1 行目のビュー");
    let detailed = table
        .viewAtColumn_row_makeIfNecessary(0, 1, true)
        .expect("2 行目のビュー");
    assert_eq!(plain.subviews().len(), 1, "detail が無い行は文字 1 本");
    assert_eq!(detailed.subviews().len(), 2, "detail がある行は文字 2 本");

    let plain_height = table.rectOfRow(0).size.height;
    let detailed_height = table.rectOfRow(1).size.height;
    assert!(
        detailed_height > plain_height,
        "2 行になった行のほうが高いこと: {detailed_height} <= {plain_height}"
    );

    // 補助の文字は本文の下に来る。NSTableCellView は反転していないので、
    // 画面の下ほど y が小さい (反転していれば逆になる)。
    let title = detailed.subviews().objectAtIndex(0).frame();
    let sub = detailed.subviews().objectAtIndex(1).frame();
    let title_is_above = if detailed.isFlipped() {
        title.origin.y + title.size.height <= sub.origin.y
    } else {
        sub.origin.y + sub.size.height <= title.origin.y
    };
    assert!(
        title_is_above,
        "補助の文字が本文の下にあること: 本文 {title:?} / 補助 {sub:?}"
    );
    assert!(
        sub.size.height < title.size.height,
        "補助の文字のほうが小さいこと: {} / {}",
        sub.size.height,
        title.size.height
    );
    Ok(())
}

/// 項目と区切り線が、そのまま NSMenu の中身になる。
fn popup_menu_items_map_to_native(ui: &Ui) -> Result<()> {
    let popup = ui.popup_menu()?;
    assert!(popup.is_empty());

    popup.set_items(&[
        PopupItem::new("コピー"),
        PopupItem::separator(),
        PopupItem::new("削除").enabled(false),
    ]);
    assert_eq!(popup.len(), 3, "区切り線も数に入ること");

    let menu = popup.native_menu();
    assert_eq!(menu.numberOfItems(), 3);
    assert!(
        !menu.autoenablesItems(),
        "AppKit の自動有効化を切らないと enabled(false) が無視される"
    );

    let copy = menu.itemAtIndex(0).expect("1 番目の項目");
    assert_eq!(copy.title().to_string(), "コピー");
    assert!(copy.isEnabled());
    assert!(menu.itemAtIndex(1).expect("2 番目の項目").isSeparatorItem());
    let remove = menu.itemAtIndex(2).expect("3 番目の項目");
    assert!(!remove.isEnabled(), "選べない項目は無効のままであること");

    // 作り直すと以前の項目は残らない。
    popup.set_items(&PopupItem::list(["貼り付け"]));
    assert_eq!(popup.len(), 1);
    assert_eq!(popup.native_menu().numberOfItems(), 1);
    Ok(())
}

/// 項目を選ぶと、区切り線を含めた並びの位置がクロージャへ届く。
fn popup_menu_selection_notifies(ui: &Ui) -> Result<()> {
    let popup = ui.popup_menu()?;
    popup.set_items(&[
        PopupItem::new("コピー"),
        PopupItem::separator(),
        PopupItem::new("貼り付け"),
        PopupItem::new("削除").enabled(false),
    ]);

    let seen = Rc::new(RefCell::new(Vec::new()));
    popup.on_select({
        let seen = seen.clone();
        move |index| seen.borrow_mut().push(index)
    });

    popup.select(2);
    assert_eq!(*seen.borrow(), vec![2], "区切り線を数えた位置で届くこと");

    // 区切り線・選べない項目・範囲外は通知しない。
    popup.select(1);
    popup.select(3);
    popup.select(9);
    assert_eq!(*seen.borrow(), vec![2]);
    Ok(())
}

/// 取り付けたウィジェットのビューが、そのメニューを持つようになる。
fn popup_menu_attaches_to_a_view(ui: &Ui) -> Result<()> {
    let label = ui.label("右クリックしてください")?;
    let popup = ui.popup_menu()?;
    popup.set_items(&PopupItem::list(["コピー"]));
    assert!(label.native_view().menu().is_none());

    popup.attach(&label);
    let attached = label.native_view().menu().expect("ビューのメニュー");
    assert!(
        std::ptr::eq(&*attached, &*popup.native_menu()),
        "取り付けたメニューそのものであること"
    );

    // 出していないメニューを閉じても落ちない。
    popup.close();
    Ok(())
}

/// 見出し・本文・中身のウィジェットが `NSAlert` へそのまま渡ること。
///
/// `open()` は `runModal` に入って戻らなくなるため、テストでは
/// `native_alert()` で組み立てだけを確かめる (表示はしない)。
fn dialog_configuration_reaches_the_alert(ui: &Ui) -> Result<()> {
    let dialog = ui.dialog("削除しますか")?;
    assert_eq!(dialog.title(), "削除しますか");
    dialog.set_message("この操作は元に戻せません。");
    assert_eq!(dialog.message(), "この操作は元に戻せません。");
    assert!(!dialog.is_open(), "作っただけでは出ていないこと");

    let field = ui.text_input("メモ")?;
    dialog.set_child(&field);

    let alert = dialog.native_alert();
    assert_eq!(alert.messageText().to_string(), "削除しますか");
    assert_eq!(
        alert.informativeText().to_string(),
        "この操作は元に戻せません。"
    );

    let accessory = alert.accessoryView().expect("中身が accessoryView に載る");
    let text_field = accessory
        .downcast::<NSTextField>()
        .expect("実体は NSTextField であること");
    assert_eq!(text_field.stringValue().to_string(), "メモ");
    // `NSAlert` は frame を見て場所を空けるので、大きさが入っていること。
    assert!(
        text_field.frame().size.width > 0.0 && text_field.frame().size.height > 0.0,
        "Auto Layout の子にも frame が入ること: {:?}",
        text_field.frame().size
    );

    // `NSAlert` にレイアウトさせても、中身が潰れずウィンドウの中に載ること。
    // (表示はしない。`layout()` はレイアウトを走らせるだけ。)
    alert.layout();
    let alert_window = alert.window();
    let laid_out = text_field.frame();
    assert!(
        laid_out.size.width > 0.0 && laid_out.size.height > 0.0,
        "レイアウト後も大きさを持つこと: {:?}",
        laid_out.size
    );
    assert!(
        unsafe { text_field.superview() }.is_some(),
        "accessoryView がアラートのビュー階層に入ること"
    );
    let content = alert_window
        .contentView()
        .expect("アラートに contentView がある");
    assert!(
        laid_out.size.width <= content.frame().size.width,
        "中身がアラートの幅からはみ出さないこと: {:?} / {:?}",
        laid_out.size,
        content.frame().size
    );

    // 設定は消えず、何度でも組み立て直せる。
    let again = dialog.native_alert();
    assert_eq!(again.messageText().to_string(), "削除しますか");
    Ok(())
}

/// macOS の並び (左から 副操作・取り消し・主操作) になること。
///
/// `NSAlert` は先に足したものを右端かつ既定のボタンにするので、
/// 足す順は 主操作・取り消し・副操作 になる。
fn dialog_buttons_follow_the_macos_order(ui: &Ui) -> Result<()> {
    let dialog = ui.dialog("保存しますか")?;
    dialog.set_buttons(
        DialogButtons::new()
            .primary("保存")
            .secondary("保存しない")
            .cancel("キャンセル"),
    );
    assert_eq!(
        dialog.buttons().label(DialogResponse::Secondary),
        Some("保存しない"),
        "設定した組み合わせが読み出せること"
    );

    let alert = dialog.native_alert();
    let titles: Vec<String> = alert
        .buttons()
        .iter()
        .map(|button| button.title().to_string())
        .collect();
    assert_eq!(titles, ["保存", "キャンセル", "保存しない"]);

    let buttons = alert.buttons();
    let cancel = buttons.objectAtIndex(1);
    assert_eq!(
        cancel.keyEquivalent().to_string(),
        "\u{1b}",
        "取り消しは Esc で閉じられること"
    );
    let primary = buttons.objectAtIndex(0);
    assert_eq!(
        primary.keyEquivalent().to_string(),
        "\r",
        "先頭が Return の既定ボタンになること"
    );
    Ok(())
}

/// ボタンを 1 つも指定しないと「OK」だけが出ること。
fn dialog_without_buttons_shows_ok(ui: &Ui) -> Result<()> {
    let dialog = ui.dialog("完了しました")?;
    assert!(dialog.buttons().is_empty(), "既定ではボタンを持たないこと");

    let alert = dialog.native_alert();
    let titles: Vec<String> = alert
        .buttons()
        .iter()
        .map(|button| button.title().to_string())
        .collect();
    assert_eq!(titles, ["OK"]);
    Ok(())
}

/// 出していないダイアログへの `close()` が何も壊さないこと。
///
/// `close()` は modal を中断するので、出していないときに呼ぶと
/// AppKit 側が例外を投げうる。呼ばないことを確かめる。
fn dialog_is_closed_until_opened(ui: &Ui) -> Result<()> {
    let dialog = ui.dialog("何か")?;
    assert!(!dialog.is_open());
    dialog.close();
    dialog.close();
    assert!(!dialog.is_open());

    // 通知は設定できるが、閉じていないので呼ばれない。
    let called = Rc::new(RefCell::new(Vec::new()));
    dialog.on_response({
        let called = called.clone();
        move |response| called.borrow_mut().push(response)
    });
    dialog.close();
    assert!(
        called.borrow().is_empty(),
        "閉じていないので通知されないこと"
    );
    Ok(())
}

/// トーストがウィンドウの `contentView` へ重なり、消すと外れること。
fn toast_is_placed_over_the_window(ui: &Ui) -> Result<()> {
    let window = ui.window("トースト", 360.0, 240.0)?;
    let root = ui.stack(Orientation::Vertical)?;
    window.set_child(&root);

    let toast = ui.toast("保存しました")?;
    assert_eq!(toast.message(), "保存しました");
    assert!(!toast.is_visible(), "作っただけでは出ていないこと");
    assert!(toast.native_view().is_none());

    toast.show();
    assert!(toast.is_visible());
    let view = toast.native_view().expect("出ている間はビューがあること");
    let content = window
        .native_window()
        .contentView()
        .expect("ウィンドウの中身があること");
    assert!(
        contains_subview(&content, &view),
        "ウィンドウの中身へ重なっていること"
    );

    // 制約から実際の大きさが出ること。frame から作られる制約を外し忘れると
    // 0 × 0 のまま重なり、出ているのに見えない状態になる。
    content.setFrameSize(NSSize::new(400.0, 300.0));
    content.layoutSubtreeIfNeeded();
    let frame = view.frame();
    assert!(
        frame.size.width > 0.0 && frame.size.height > 0.0,
        "中身の大きさを持つこと: {frame:?}"
    );
    assert!(
        frame.size.width < 400.0,
        "ウィンドウからはみ出さないこと: {frame:?}"
    );
    assert_eq!(
        frame.origin.y, 24.0,
        "下端から一定の余白を空けて置かれること"
    );
    let center = frame.origin.x + frame.size.width / 2.0;
    assert!(
        (center - 200.0).abs() < 1.0,
        "左右の中央に置かれること: {frame:?}"
    );

    // 出したまま文字を変えても、載っているのは 1 つだけ。
    toast.set_message("書き出しました");
    assert_eq!(toast.message(), "書き出しました");
    let rebuilt = toast.native_view().expect("作り直しても出ていること");
    assert_eq!(
        toast_labels(&rebuilt),
        ["書き出しました"],
        "新しい文字が出ていること"
    );
    assert!(contains_subview(&content, &rebuilt));
    assert_eq!(
        count_toasts(&content),
        1,
        "作り直しても重なるのは 1 つだけであること"
    );

    toast.dismiss();
    assert!(!toast.is_visible());
    assert!(toast.native_view().is_none());
    assert_eq!(count_toasts(&content), 0, "消すとビューが外れること");
    Ok(())
}

/// 操作ボタンを押すと `on_action` と `on_dismiss` が呼ばれ、消えること。
fn toast_action_notifies_and_dismisses(ui: &Ui) -> Result<()> {
    let window = ui.window("トースト", 360.0, 240.0)?;
    window.set_child(&ui.stack(Orientation::Vertical)?);

    let toast = ui.toast("削除しました")?;
    toast.set_action("元に戻す");
    assert_eq!(toast.action(), "元に戻す");
    // 押されるまでは消えないトーストにしておく。
    toast.set_timeout(0.0);
    assert!(toast.spec().is_persistent());

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
    let view = toast.native_view().expect("出ている間はビューがあること");
    let button = toast_button(&view).expect("操作ボタンが並んでいること");
    assert_eq!(button.title().to_string(), "元に戻す");
    unsafe { button.performClick(None) };

    assert_eq!(
        *seen.borrow(),
        ["action", "dismiss"],
        "押された通知のあとに、消えた通知が届くこと"
    );
    assert!(!toast.is_visible(), "押すと消えること");
    Ok(())
}

/// 新しいトーストが前のものを置き換え、置き換えられたほうは通知しないこと。
fn toast_replaces_the_previous_one(ui: &Ui) -> Result<()> {
    let window = ui.window("トースト", 360.0, 240.0)?;
    window.set_child(&ui.stack(Orientation::Vertical)?);
    let content = window
        .native_window()
        .contentView()
        .expect("ウィンドウの中身があること");

    let first = ui.toast("1 つめ")?;
    first.set_timeout(0.0);
    let dismissed = Rc::new(RefCell::new(0));
    first.on_dismiss({
        let dismissed = dismissed.clone();
        move || *dismissed.borrow_mut() += 1
    });
    first.show();

    let second = ui.toast("2 つめ")?;
    second.set_timeout(0.0);
    second.show();

    assert!(!first.is_visible(), "前のものは消えること");
    assert!(second.is_visible());
    assert_eq!(
        *dismissed.borrow(),
        0,
        "アプリ自身の操作なので、消えた通知は届かないこと"
    );
    assert_eq!(
        count_toasts(&content),
        1,
        "同時に出るのは 1 つだけであること"
    );

    second.dismiss();
    assert_eq!(count_toasts(&content), 0);
    Ok(())
}

/// 指定した時間が過ぎるとトーストが自分から消え、`on_dismiss` が届くこと。
fn toast_dismisses_itself_after_the_timeout(ui: &Ui) -> Result<()> {
    let window = ui.window("トースト", 360.0, 240.0)?;
    window.set_child(&ui.stack(Orientation::Vertical)?);

    let toast = ui.toast("しばらくしたら消える")?;
    toast.set_timeout(0.05);
    assert_eq!(toast.timeout(), 0.05);
    let dismissed = Rc::new(RefCell::new(0));
    toast.on_dismiss({
        let dismissed = dismissed.clone();
        move || *dismissed.borrow_mut() += 1
    });

    toast.show();
    assert!(toast.is_visible());
    assert_eq!(*dismissed.borrow(), 0, "まだ時間が来ていないこと");

    // NSTimer はランループの上で数えるので、回して時間を進める。
    pump_until(5.0, || !toast.is_visible());
    assert!(!toast.is_visible(), "時間が来たら自分から消えること");
    assert_eq!(*dismissed.borrow(), 1, "消えた通知が 1 回だけ届くこと");
    Ok(())
}

/// トーストとして重ねてあるビューか。naui が付けた名前で見分ける。
fn is_toast_view(view: &NSView) -> bool {
    view.identifier()
        .is_some_and(|id| id.to_string() == "naui.toast")
}

/// `parent` の子に `view` があるか。
fn contains_subview(parent: &NSView, view: &NSView) -> bool {
    parent
        .subviews()
        .iter()
        .any(|sub| std::ptr::eq(&*sub as *const NSView, view as *const NSView))
}

/// `parent` に重なっているトーストの数。
fn count_toasts(parent: &NSView) -> usize {
    parent
        .subviews()
        .iter()
        .filter(|sub| is_toast_view(sub))
        .count()
}

/// トーストに並んでいる文字。
fn toast_labels(view: &NSView) -> Vec<String> {
    toast_row(view)
        .iter()
        .filter_map(|item| {
            item.downcast_ref::<NSTextField>()
                .map(|f| f.stringValue().to_string())
        })
        .collect()
}

/// トーストの操作ボタン。置いていなければ `None`。
fn toast_button(view: &NSView) -> Option<Retained<NSButton>> {
    toast_row(view)
        .iter()
        .find_map(|item| item.downcast_ref::<NSButton>().map(|b| b.retain()))
}

/// トーストの中で文字とボタンを並べている行。
///
/// 重ねてあるビュー (影) → 背景 (角丸) → 行、の順に入っている。
fn toast_row(view: &NSView) -> Retained<objc2_foundation::NSArray<NSView>> {
    let surface = view.subviews().objectAtIndex(0);
    let row = surface.subviews().objectAtIndex(0);
    row.subviews()
}

/// `NSToolbar` に項目が並び、区切りは AppKit の space 項目へ写る。
fn toolbar_items_map_to_native(ui: &Ui) -> Result<()> {
    let toolbar = ui.toolbar()?;
    assert!(toolbar.is_empty());

    toolbar.set_items(&[
        ToolbarItem::new(ToolbarIcon::New, "新規"),
        ToolbarItem::separator(),
        ToolbarItem::new(ToolbarIcon::Save, "保存").enabled(false),
    ]);
    assert_eq!(toolbar.len(), 3, "区切りも 1 項目として数える");

    let native = toolbar.native_toolbar();
    assert_eq!(native.items().len(), 3, "NSToolbar にも 3 つ並ぶ");
    assert_eq!(
        native.items().objectAtIndex(1).itemIdentifier().to_string(),
        "NSToolbarSpaceItem",
        "区切りは AppKit の space 項目"
    );

    let first = toolbar.native_item(0).expect("先頭は項目");
    assert_eq!(first.label().to_string(), "新規");
    assert!(first.isEnabled());
    assert!(
        first.image().is_some(),
        "SF Symbols のアイコンが載っていること"
    );
    assert_eq!(
        first.toolTip().map(|t| t.to_string()).as_deref(),
        Some("新規"),
        "ラベルはツールチップに出る"
    );
    assert!(toolbar.native_item(1).is_none(), "区切りに項目は無い");
    assert!(!toolbar.native_item(2).expect("3 番目は項目").isEnabled());
    assert!(toolbar.native_item(9).is_none(), "範囲外は None");

    assert!(toolbar.is_item_enabled(0));
    assert!(!toolbar.is_item_enabled(1), "区切りは押せない");
    assert!(!toolbar.is_item_enabled(2));

    // 項目ごとの指定と全体の指定は AND を取る。
    toolbar.set_item_enabled(2, true);
    assert!(toolbar.native_item(2).expect("項目").isEnabled());
    toolbar.set_enabled(false);
    assert!(!toolbar.is_item_enabled(0));
    assert!(!toolbar.native_item(0).expect("項目").isEnabled());
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
    assert_eq!(native.items().len(), 0);
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

    // AppKit が使うのと同じ target/action の経路も確かめる。
    let item = toolbar.native_item(0).expect("項目");
    let target = item.target().expect("target が設定されていること");
    assert_eq!(
        item.action(),
        Some(sel!(invoke:)),
        "トランポリンの invoke: が action になっている"
    );
    // AppKit が action を送るのと同じ形で呼ぶ (invoke: は戻り値を持たない)。
    unsafe {
        let _: () = msg_send![&*target, invoke: &*item];
    }
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

/// ツールバーは `NSWindow` に取り付き、外すと消える。
fn toolbar_attaches_to_a_window(ui: &Ui) -> Result<()> {
    let window = ui.window("ツールバー", 400.0, 300.0)?;
    let native_window = window.native_window();
    assert!(native_window.toolbar().is_none(), "最初は付いていない");

    let toolbar = ui.toolbar()?;
    toolbar.set_items(&[
        ToolbarItem::new(ToolbarIcon::New, "新規"),
        ToolbarItem::new(ToolbarIcon::Open, "開く"),
    ]);
    window.set_toolbar(&toolbar);

    let attached = native_window.toolbar().expect("取り付いていること");
    assert_eq!(attached, toolbar.native_toolbar(), "同じ NSToolbar である");
    assert_eq!(attached.items().len(), 2);

    // タイトル文字を出したままだと、先頭を占めて項目が右端へ寄ってしまう。
    assert_eq!(
        native_window.titleVisibility(),
        NSWindowTitleVisibility::Hidden,
        "ツールバーを付けるとタイトル文字は隠れる"
    );
    assert_eq!(
        window.title(),
        "ツールバー",
        "隠れるのは表示だけで、タイトル自体は残る"
    );

    window.clear_toolbar();
    assert!(native_window.toolbar().is_none(), "外すと消える");
    assert_eq!(
        native_window.titleVisibility(),
        NSWindowTitleVisibility::Visible,
        "外すとタイトル文字も戻る"
    );
    window.close();
    Ok(())
}

/// 通知の中からツールバーを組み替えても、二重借用にならない。
fn toolbar_callback_is_reentrant(ui: &Ui) -> Result<()> {
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

/// 記号名を間違えると `NSImage` が nil になり、その項目だけ絵が出なくなる。
fn toolbar_icons_exist_as_sf_symbols(_ui: &Ui) -> Result<()> {
    let mut missing = Vec::new();
    for icon in ToolbarIcon::ALL {
        let name = NSString::from_str(icon.sf_symbol());
        if NSImage::imageWithSystemSymbolName_accessibilityDescription(&name, None).is_none() {
            missing.push(icon.sf_symbol());
        }
    }
    assert!(missing.is_empty(), "実在しない SF Symbols: {missing:?}");
    Ok(())
}

/// ツリーが NSOutlineView の行として並び、開くと子の行が増える。
fn tree_rows_follow_the_expansion(ui: &Ui) -> Result<()> {
    let tree = ui.tree()?;
    tree.set_items(&sample_tree());
    assert_eq!(tree.len(), 6, "子孫まで数えること");
    assert!(!tree.is_empty());

    let outline = tree.native_outline_view();
    // 何も開いていないので、見えているのは根の 2 つだけ。
    assert_eq!(outline.numberOfRows(), 2);
    assert!(!tree.is_expanded(&[0]));

    tree.set_expanded(&[0], true);
    assert!(tree.is_expanded(&[0]));
    assert_eq!(outline.numberOfRows(), 4, "src の子 2 つが増えること");
    assert_eq!(outline.levelForRow(1), 1, "子は 1 段下がること");

    // 孫まで開くと、間の枝もまとめて開く。
    tree.set_expanded(&[1, 0], true);
    assert!(tree.is_expanded(&[1]), "祖先も開かれること");
    assert_eq!(outline.numberOfRows(), 6, "すべての項目が見えること");

    tree.collapse_all();
    assert_eq!(outline.numberOfRows(), 2);
    assert!(!tree.is_expanded(&[0]));

    tree.expand_all();
    assert_eq!(outline.numberOfRows(), 6, "すべての行が見えること");

    // 作り直すと、TreeItem::expanded のとおりに戻る。
    tree.set_items(&[TreeItem::new("親")
        .expanded(true)
        .children(TreeItem::list(["子"]))]);
    assert_eq!(tree.len(), 2);
    assert!(tree.is_expanded(&[0]));
    assert_eq!(tree.native_outline_view().numberOfRows(), 2);
    Ok(())
}

/// ツリーの選択が NSOutlineView と往復し、`set_selected` は通知しない。
fn tree_selection_round_trips(ui: &Ui) -> Result<()> {
    let tree = ui.tree()?;
    tree.set_items(&sample_tree());
    assert_eq!(tree.selected(), None, "作った直後は何も選ばれていないこと");

    let seen: Rc<RefCell<Vec<Vec<usize>>>> = Rc::new(RefCell::new(Vec::new()));
    tree.on_select({
        let seen = seen.clone();
        move |path| seen.borrow_mut().push(path.to_vec())
    });

    // 閉じた枝の中を選ぶと、見えるように祖先が開く。
    tree.select(&[0, 1]);
    assert_eq!(tree.selected(), Some(vec![0, 1]));
    assert!(tree.is_expanded(&[0]));
    assert_eq!(*seen.borrow(), vec![vec![0, 1]]);

    let outline = tree.native_outline_view();
    assert_eq!(outline.selectedRow(), 2, "ネイティブ側も選ばれていること");

    // `set_selected` は通知しない。
    tree.set_selected(&[1]);
    assert_eq!(tree.selected(), Some(vec![1]));
    assert_eq!(*seen.borrow(), vec![vec![0, 1]], "通知は 1 回だけ");

    // 無いパスを選ぶと選択が外れ、空のパスで通知される。
    tree.select(&[9]);
    assert_eq!(tree.selected(), None);
    assert_eq!(*seen.borrow(), vec![vec![0, 1], Vec::new()]);

    // clear_selection は通知しない。
    tree.set_selected(&[1]);
    tree.clear_selection();
    assert_eq!(tree.selected(), None);
    assert_eq!(seen.borrow().len(), 2);

    // 作り直すと選択は外れる。
    tree.set_selected(&[1]);
    tree.set_items(&sample_tree());
    assert_eq!(tree.selected(), None);
    assert_eq!(seen.borrow().len(), 2, "作り直しも通知しない");
    Ok(())
}

/// 選べない枝は、その子孫までまとめて選べない。
fn tree_skips_disabled_branches(ui: &Ui) -> Result<()> {
    let tree = ui.tree()?;
    tree.set_items(&[
        TreeItem::new("有効").child(TreeItem::new("子")),
        TreeItem::new("無効")
            .enabled(false)
            .child(TreeItem::new("孫")),
    ]);
    tree.expand_all();

    let seen: Rc<RefCell<Vec<Vec<usize>>>> = Rc::new(RefCell::new(Vec::new()));
    tree.on_select({
        let seen = seen.clone();
        move |path| seen.borrow_mut().push(path.to_vec())
    });

    tree.select(&[1]);
    assert_eq!(tree.selected(), None, "無効な枝は選べないこと");
    tree.select(&[1, 0]);
    assert_eq!(tree.selected(), None, "無効な枝の中身も選べないこと");
    assert_eq!(*seen.borrow(), vec![Vec::<usize>::new(), Vec::new()]);

    tree.select(&[0, 0]);
    assert_eq!(tree.selected(), Some(vec![0, 0]));

    // AppKit 自身にも「この項目は選べない」と伝わっている。
    let outline = tree.native_outline_view();
    let delegate = unsafe { outline.delegate() }.expect("デリゲートがあること");
    let enabled = outline.itemAtRow(0).expect("1 行目の項目があること");
    let disabled = outline.itemAtRow(2).expect("3 行目の項目があること");
    unsafe {
        assert!(delegate.outlineView_shouldSelectItem(&outline, &enabled));
        assert!(!delegate.outlineView_shouldSelectItem(&outline, &disabled));
    }
    Ok(())
}

/// 開閉の通知は、ネイティブ側の操作でも naui の `expand` でも届く。
fn tree_expansion_notifies(ui: &Ui) -> Result<()> {
    let tree = ui.tree()?;
    tree.set_items(&sample_tree());

    let seen: Rc<RefCell<Vec<(Vec<usize>, bool)>>> = Rc::new(RefCell::new(Vec::new()));
    tree.on_expand({
        let seen = seen.clone();
        move |path, expanded| seen.borrow_mut().push((path.to_vec(), expanded))
    });

    tree.expand(&[0]);
    tree.collapse(&[0]);
    assert_eq!(
        *seen.borrow(),
        vec![(vec![0], true), (vec![0], false)],
        "naui からの開閉が 1 回ずつ通知されること"
    );

    // `set_expanded` と一括の開閉は通知しない。
    tree.set_expanded(&[0], true);
    tree.expand_all();
    tree.collapse_all();
    assert_eq!(seen.borrow().len(), 2);

    // naui を通さず、AppKit 側から開く。
    let outline = tree.native_outline_view();
    let item = outline.itemAtRow(1).expect("2 行目の項目があること");
    unsafe { outline.expandItem(Some(&item)) };
    assert_eq!(
        seen.borrow().last(),
        Some(&(vec![1], true)),
        "ネイティブ側の開閉もそのまま届くこと"
    );
    assert!(tree.is_expanded(&[1]));

    // 葉は開けないので、通知も起きない。
    tree.expand(&[1, 0, 0]);
    assert_eq!(seen.borrow().len(), 3);
    Ok(())
}

/// 項目の中身は、AppKit がデリゲートに作らせた NSTextField になる。
fn tree_rows_are_native_views(ui: &Ui) -> Result<()> {
    let tree = ui.tree()?;
    tree.set_items(&[TreeItem::new("プロジェクト")
        .detail("3 ファイル")
        .expanded(true)
        .child(TreeItem::new("main.rs"))]);
    tree.set_sizing(Sizing::fixed(260.0, 140.0));

    let stack = ui.stack(Orientation::Vertical)?;
    stack.append(&tree);
    let root = stack.native_view();
    root.setFrameSize(NSSize::new(400.0, 300.0));
    root.layoutSubtreeIfNeeded();

    let frame = tree.native_view().frame();
    assert!(
        (frame.size.width - 260.0).abs() < 1e-6 && (frame.size.height - 140.0).abs() < 1e-6,
        "指定した大きさになること: {frame:?}"
    );

    let outline = tree.native_outline_view();
    let cell = outline
        .viewAtColumn_row_makeIfNecessary(0, 0, true)
        .expect("1 行目のビューが作られること")
        .downcast::<objc2_app_kit::NSTableCellView>()
        .expect("行は NSTableCellView であること");
    let field = unsafe { cell.textField() }.expect("行に文字が入っていること");
    assert_eq!(field.stringValue().to_string(), "プロジェクト");
    assert_eq!(cell.subviews().len(), 2, "補助の文字が 2 行目に出ること");

    // 子の行も同じように作られる。
    let child = outline
        .viewAtColumn_row_makeIfNecessary(0, 1, true)
        .expect("2 行目のビューが作られること")
        .downcast::<objc2_app_kit::NSTableCellView>()
        .expect("行は NSTableCellView であること");
    let field = unsafe { child.textField() }.expect("行に文字が入っていること");
    assert_eq!(field.stringValue().to_string(), "main.rs");
    Ok(())
}

/// 通知の中から同じツリーを触っても壊れない (リストと同じ形)。
fn tree_callback_can_touch_the_tree(ui: &Ui) -> Result<()> {
    let tree = ui.tree()?;
    tree.set_items(&sample_tree());
    tree.expand_all();

    let seen: Rc<RefCell<Vec<Vec<usize>>>> = Rc::new(RefCell::new(Vec::new()));
    tree.on_select({
        let seen = seen.clone();
        let tree = tree.clone();
        move |path| {
            seen.borrow_mut().push(path.to_vec());
            // 1 回目の通知でだけ、中身ごと差し替える。
            if seen.borrow().len() == 1 {
                tree.set_items(&[TreeItem::new("別の木").child(TreeItem::new("葉"))]);
                tree.set_selected(&[0]);
            }
        }
    });

    // naui を通さず、AppKit 側から選択を起こす。
    let rows = objc2_foundation::NSMutableIndexSet::new();
    rows.addIndex(1);
    tree.native_outline_view()
        .selectRowIndexes_byExtendingSelection(&rows, false);

    assert_eq!(*seen.borrow(), vec![vec![0, 0]], "通知は 1 回だけ");
    assert_eq!(tree.len(), 2, "コールバックの中の差し替えが効くこと");
    assert_eq!(tree.selected(), Some(vec![0]));
    Ok(())
}

/// テストで使う木。src (2 つの葉) と docs (guide > intro)。
fn sample_tree() -> Vec<TreeItem> {
    vec![
        TreeItem::new("src").children([TreeItem::new("main.rs"), TreeItem::new("lib.rs")]),
        TreeItem::new("docs").child(TreeItem::new("guide").child(TreeItem::new("intro.md"))),
    ]
}

/// 親を閉じても、その中の開閉は覚えられていて、開き直すと元に戻る。
///
/// AppKit (Finder と同じ) の動きに合わせている。開き直したときに AppKit が
/// もう一度出す通知は、naui から見た状態が変わっていないので出さない。
fn tree_remembers_expansion_inside_a_closed_branch(ui: &Ui) -> Result<()> {
    let tree = ui.tree()?;
    tree.set_items(&sample_tree());
    tree.set_expanded(&[1, 0], true);
    let outline = tree.native_outline_view();
    assert_eq!(
        outline.numberOfRows(),
        4,
        "docs > guide > intro.md まで開くこと"
    );

    let seen: Rc<RefCell<Vec<(Vec<usize>, bool)>>> = Rc::new(RefCell::new(Vec::new()));
    tree.on_expand({
        let seen = seen.clone();
        move |path, expanded| seen.borrow_mut().push((path.to_vec(), expanded))
    });

    tree.collapse(&[1]);
    assert_eq!(outline.numberOfRows(), 2);
    assert!(
        tree.is_expanded(&[1, 0]),
        "見えなくなっても、中の開閉は覚えていること"
    );

    tree.expand(&[1]);
    assert_eq!(outline.numberOfRows(), 4, "中の開閉ごと戻ること");
    assert_eq!(
        *seen.borrow(),
        vec![(vec![1], false), (vec![1], true)],
        "通知は操作した枝の分だけ"
    );
    Ok(())
}

// ------------------------------------------------------------ 日付ピッカー

/// ネイティブ側の編集を真似る。
///
/// `NSDatePicker` には `performClick` にあたるものが無いので、AppKit が
/// 利用者の操作でするのと同じ「値を書いてから action を送る」を手で行う。
fn edit_natively(picker: &NSDatePicker, value: DateTime) {
    let calendar = NSCalendar::initWithCalendarIdentifier(NSCalendar::alloc(), unsafe {
        NSCalendarIdentifierGregorian
    })
    .expect("グレゴリオ暦");
    let components = NSDateComponents::new();
    components.setYear(value.year as isize);
    components.setMonth(value.month as isize);
    components.setDay(value.day as isize);
    components.setHour(value.hour as isize);
    components.setMinute(value.minute as isize);
    components.setSecond(0);
    let date = calendar
        .dateFromComponents(&components)
        .expect("NSDate へ変換");
    picker.setDateValue(&date);
    let action = picker.action();
    let target = picker.target();
    unsafe { picker.sendAction_to(action, target.as_deref()) };
}

/// 時刻ピッカーのスピナーを回したときと同じ経路。日付は naui が使うものと
/// 同じ 1970-01-01 へそろえる。
fn edit_time_natively(picker: &NSDatePicker, value: Time) {
    let (year, month, day) = DateTime::TIME_ORIGIN;
    edit_natively(
        picker,
        DateTime::new(year, month, day, value.hour, value.minute),
    );
}

/// 数字の欄へ打ち込む。`controlTextDidChange:` は AppKit が編集中に送るので、
/// テストからは同じ通知をデリゲートへ直接渡す。
fn type_into_field(field: &NSTextField, text: &str) {
    field.setStringValue(&NSString::from_str(text));
    let Some(delegate) = field.delegate() else {
        return;
    };
    let name = NSString::from_str("NSControlTextDidChangeNotification");
    let notification =
        unsafe { NSNotification::notificationWithName_object(&name, Some(field.as_ref())) };
    unsafe {
        let _: () = msg_send![&*delegate, controlTextDidChange: &*notification];
    }
}

/// 検索欄で Enter を押す (AppKit が編集中のフィールドエディタへ投げる
/// `insertNewline:` と同じ経路)。
fn press_return_in_field(field: &NSTextField) {
    let Some(delegate) = field.delegate() else {
        return;
    };
    let mtm = MainThreadMarker::from(field);
    let editor = NSTextView::new(mtm);
    let control: &objc2_app_kit::NSControl = field.as_ref();
    let handled: bool = unsafe {
        msg_send![
            &*delegate,
            control: control,
            textView: &*editor,
            doCommandBySelector: sel!(insertNewline:),
        ]
    };
    assert!(!handled, "既定の確定は AppKit へ任せること");
}

/// 一覧から候補を選ぶ。`selectItemAtIndex:` が出す通知に加え、届かなかった
/// 場合に備えて同じ通知をデリゲートへ直接渡す (二重に届いても 1 回にまとまる)。
fn pick_natively(combo: &NSComboBox, index: isize) {
    combo.selectItemAtIndex(index);
    let Some(delegate) = combo.delegate() else {
        return;
    };
    let name = NSString::from_str("NSComboBoxSelectionDidChangeNotification");
    let notification =
        unsafe { NSNotification::notificationWithName_object(&name, Some(combo.as_ref())) };
    unsafe {
        let _: () = msg_send![&*delegate, comboBoxSelectionDidChange: &*notification];
    }
}

/// 欄を確定する (Enter・欄を離れたときと同じ経路)。
fn commit_field(control: &NSTextField) {
    let action = control.action();
    let target = control.target();
    unsafe { control.sendAction_to(action, target.as_deref()) };
}

/// 上下のボタンを動かす (`NSStepper` が値を動かしてから action を送る)。
fn step_natively(stepper: &NSStepper, value: f64) {
    stepper.setDoubleValue(value);
    let action = stepper.action();
    let target = stepper.target();
    unsafe { stepper.sendAction_to(action, target.as_deref()) };
}

/// 数値入力は欄と上下のボタンの両方へ値を書き、ボタンの操作を通知する。
fn number_input_value_round_trips(ui: &Ui) -> Result<()> {
    let number = ui.number_input(3.0)?;
    assert_eq!(number.value(), 3.0);
    assert_eq!(number.native_field().stringValue().to_string(), "3");
    assert_eq!(number.native_stepper().doubleValue(), 3.0);

    let seen = Rc::new(RefCell::new(Vec::new()));
    number.on_change({
        let seen = seen.clone();
        move |value| seen.borrow_mut().push(value)
    });

    // 既定は整数なので、小数は丸めて入る。
    number.set_value(2.4);
    assert_eq!(number.value(), 2.0);
    assert_eq!(number.native_field().stringValue().to_string(), "2");
    assert!(seen.borrow().is_empty(), "set_value は通知しない");

    step_natively(&number.native_stepper(), 3.0);
    assert_eq!(number.value(), 3.0);
    assert_eq!(*seen.borrow(), vec![3.0]);
    assert_eq!(
        number.native_field().stringValue().to_string(),
        "3",
        "ボタンで動かしたら欄も追いかけること"
    );

    // 値が動かなければ知らせない。
    step_natively(&number.native_stepper(), 3.0);
    assert_eq!(seen.borrow().len(), 1);

    number.set_enabled(false);
    assert!(!number.native_field().isEnabled());
    assert!(!number.native_stepper().isEnabled());
    Ok(())
}

/// 小数桁・刻み・範囲は `NSStepper` にも naui の値にも効く。
fn number_input_applies_the_spec(ui: &Ui) -> Result<()> {
    let number = ui.number_input(0.0)?;
    number.set_decimals(2);
    number.set_step(0.5);
    number.set_range(Some(0.0), Some(10.0));

    let stepper = number.native_stepper();
    assert_eq!(stepper.minValue(), 0.0);
    assert_eq!(stepper.maxValue(), 10.0);
    assert_eq!(stepper.increment(), 0.5);

    number.set_value(1.239);
    assert_eq!(number.value(), 1.24, "小数桁へ丸める");
    assert_eq!(number.native_field().stringValue().to_string(), "1.24");

    number.set_value(12.345);
    assert_eq!(number.value(), 10.0, "上限で止まる");
    assert_eq!(
        number.native_field().stringValue().to_string(),
        "10.00",
        "桁は必ず埋めること"
    );

    number.set_value(-1.0);
    assert_eq!(number.value(), 0.0, "下限で止まる");

    // 範囲を外すと自由に入る。
    number.set_range(None, None);
    number.set_value(-30.5);
    assert_eq!(number.value(), -30.5);
    assert_eq!(number.spec().min, None);
    Ok(())
}

/// 打っている間は通知だけ、確定で表示が値へそろう。
fn number_input_notifies_while_typing(ui: &Ui) -> Result<()> {
    let number = ui.number_input(0.0)?;
    number.set_range(None, Some(100.0));
    let field = number.native_field();

    let seen = Rc::new(RefCell::new(Vec::new()));
    number.on_change({
        let seen = seen.clone();
        move |value| seen.borrow_mut().push(value)
    });

    type_into_field(&field, "12");
    assert_eq!(number.value(), 12.0);
    assert_eq!(*seen.borrow(), vec![12.0]);
    assert_eq!(
        field.stringValue().to_string(),
        "12",
        "打っている間は表示を書き換えないこと"
    );

    // 数として読めないものは、確定まで放っておく。
    type_into_field(&field, "12ab");
    assert_eq!(number.value(), 12.0);
    assert_eq!(seen.borrow().len(), 1);
    commit_field(&field);
    assert_eq!(field.stringValue().to_string(), "12", "元の値へ戻す");

    // 範囲の外は通知の時点で端へ寄り、確定で表示もそろう。
    type_into_field(&field, "999");
    assert_eq!(number.value(), 100.0);
    assert_eq!(*seen.borrow(), vec![12.0, 100.0]);
    commit_field(&field);
    assert_eq!(field.stringValue().to_string(), "100");
    assert_eq!(seen.borrow().len(), 2, "確定だけでは重ねて通知しないこと");

    // 空欄も読めない扱い。確定で元へ戻る。
    type_into_field(&field, "");
    commit_field(&field);
    assert_eq!(number.value(), 100.0);
    assert_eq!(field.stringValue().to_string(), "100");
    Ok(())
}

/// パスワード入力は `NSSecureTextField` そのもので、文字列は往復する。
fn password_input_round_trips(ui: &Ui) -> Result<()> {
    let password = ui.password_input()?;
    let view = password.native_view();
    let field = view
        .downcast_ref::<NSSecureTextField>()
        .expect("伏せ字の欄であること");
    assert!(field.isEditable(), "打ち込めること");
    assert!(field.isBezeled(), "1 行入力と同じ枠を持つこと");

    assert_eq!(password.text(), "");
    password.set_text("ひみつ");
    assert_eq!(password.text(), "ひみつ");
    password.set_placeholder("パスワード");

    let seen = Rc::new(RefCell::new(Vec::new()));
    password.on_change({
        let seen = seen.clone();
        move |text| seen.borrow_mut().push(text.to_string())
    });
    type_into_field(field.as_ref(), "あい");
    assert_eq!(password.text(), "あい");
    assert_eq!(*seen.borrow(), vec!["あい".to_string()]);

    password.set_text("");
    assert_eq!(password.text(), "");
    assert_eq!(seen.borrow().len(), 1, "set_text は通知しない");

    password.set_enabled(false);
    assert!(!field.isEnabled());
    Ok(())
}

/// 検索入力は `NSSearchField` そのもので、文字列は往復する。
fn search_input_round_trips(ui: &Ui) -> Result<()> {
    let search = ui.search_input()?;
    let view = search.native_view();
    let field = view
        .downcast_ref::<NSSearchField>()
        .expect("検索の欄であること");
    assert!(field.isEditable(), "打ち込めること");

    assert_eq!(search.text(), "");
    search.set_text("なう");
    assert_eq!(search.text(), "なう");
    search.set_placeholder("検索");
    assert_eq!(
        field
            .placeholderString()
            .map(|s| s.to_string())
            .unwrap_or_default(),
        "検索"
    );

    let seen = Rc::new(RefCell::new(Vec::new()));
    search.on_change({
        let seen = seen.clone();
        move |text| seen.borrow_mut().push(text.to_string())
    });
    type_into_field(field.as_ref(), "なうい");
    assert_eq!(search.text(), "なうい");
    assert_eq!(*seen.borrow(), vec!["なうい".to_string()]);

    search.set_text("");
    assert_eq!(search.text(), "");
    assert_eq!(seen.borrow().len(), 1, "set_text は通知しない");

    search.set_enabled(false);
    assert!(!field.isEnabled());
    Ok(())
}

/// 確定の通知は Enter のときだけで、打っている間は飛ばない。
fn search_input_notifies_on_return(ui: &Ui) -> Result<()> {
    let search = ui.search_input()?;
    let view = search.native_view();
    let field = view
        .downcast_ref::<NSSearchField>()
        .expect("検索の欄であること");

    let typed = Rc::new(RefCell::new(Vec::new()));
    let submitted = Rc::new(RefCell::new(Vec::new()));
    search.on_change({
        let typed = typed.clone();
        move |text| typed.borrow_mut().push(text.to_string())
    });
    search.on_search({
        let submitted = submitted.clone();
        move |text| submitted.borrow_mut().push(text.to_string())
    });

    type_into_field(field.as_ref(), "ねこ");
    assert_eq!(*typed.borrow(), vec!["ねこ".to_string()]);
    assert!(submitted.borrow().is_empty(), "打つだけでは確定しない");

    press_return_in_field(field.as_ref());
    assert_eq!(*submitted.borrow(), vec!["ねこ".to_string()]);
    assert_eq!(typed.borrow().len(), 1, "確定は打鍵の通知を増やさない");

    // 確定は何度でも起きる。
    type_into_field(field.as_ref(), "いぬ");
    press_return_in_field(field.as_ref());
    assert_eq!(
        *submitted.borrow(),
        vec!["ねこ".to_string(), "いぬ".to_string()]
    );
    Ok(())
}

/// 種別ごとに `datePickerElements` が変わる。
fn date_picker_shows_the_elements_of_its_mode(ui: &Ui) -> Result<()> {
    let date = ui.date_picker(DatePickerMode::Date)?;
    assert_eq!(date.mode(), DatePickerMode::Date);
    assert_eq!(
        date.native_picker().datePickerElements(),
        NSDatePickerElementFlags::YearMonthDay
    );

    let time = ui.date_picker(DatePickerMode::Time)?;
    assert_eq!(
        time.native_picker().datePickerElements(),
        NSDatePickerElementFlags::HourMinute
    );

    let both = ui.date_picker(DatePickerMode::DateTime)?;
    assert_eq!(
        both.native_picker().datePickerElements(),
        NSDatePickerElementFlags::YearMonthDay | NSDatePickerElementFlags::HourMinute
    );

    // 日付を選ばせるときだけ、編集中にカレンダーを重ねて出す。
    assert!(date.native_picker().presentsCalendarOverlay());
    assert!(both.native_picker().presentsCalendarOverlay());
    assert!(
        !time.native_picker().presentsCalendarOverlay(),
        "時刻だけの表示に暦は出さない"
    );

    // 作った直後は現在日時。暦は固定しているので西暦で返る。
    assert!(
        (2000..=9999).contains(&date.value().year),
        "初期値が西暦で入る: {}",
        date.value()
    );
    assert!(date.value().is_valid());
    Ok(())
}

/// 値の書き込みと、ネイティブ側の編集の両方向。
fn date_picker_value_round_trips(ui: &Ui) -> Result<()> {
    let picker = ui.date_picker(DatePickerMode::DateTime)?;
    let seen = Rc::new(RefCell::new(Vec::new()));
    picker.on_change({
        let seen = seen.clone();
        move |value| seen.borrow_mut().push(value)
    });

    picker.set_value(DateTime::new(2026, 8, 22, 9, 30));
    assert_eq!(picker.value(), DateTime::new(2026, 8, 22, 9, 30));
    assert!(seen.borrow().is_empty(), "set_value は通知しない");

    // 暦として成り立たない値は丸める。
    picker.set_value(DateTime::new(2026, 11, 31, 25, 70));
    assert_eq!(picker.value(), DateTime::new(2026, 11, 30, 23, 59));

    edit_natively(&picker.native_picker(), DateTime::new(2027, 1, 5, 18, 45));
    assert_eq!(picker.value(), DateTime::new(2027, 1, 5, 18, 45));
    assert_eq!(*seen.borrow(), vec![DateTime::new(2027, 1, 5, 18, 45)]);

    // 同じ値で通知が来ても、値が動いていなければ知らせない。
    edit_natively(&picker.native_picker(), DateTime::new(2027, 1, 5, 18, 45));
    assert_eq!(seen.borrow().len(), 1);

    picker.set_enabled(false);
    assert!(!picker.native_picker().isEnabled());
    Ok(())
}

/// 日付だけの表示は時刻を、時刻だけの表示は日付を保つ。
fn date_picker_keeps_the_part_it_does_not_show(ui: &Ui) -> Result<()> {
    let date_only = ui.date_picker(DatePickerMode::Date)?;
    date_only.set_value(DateTime::new(2026, 8, 22, 9, 30));
    edit_natively(&date_only.native_picker(), DateTime::new(2027, 1, 5, 0, 0));
    assert_eq!(
        date_only.value(),
        DateTime::new(2027, 1, 5, 9, 30),
        "日付を選んでも時刻は残る"
    );

    let time_only = ui.date_picker(DatePickerMode::Time)?;
    time_only.set_value(DateTime::new(2026, 8, 22, 9, 30));
    edit_natively(
        &time_only.native_picker(),
        DateTime::new(1970, 1, 1, 18, 45),
    );
    assert_eq!(
        time_only.value(),
        DateTime::new(2026, 8, 22, 18, 45),
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
    assert!(picker.native_picker().minDate().is_some());
    assert!(picker.native_picker().maxDate().is_some());

    let seen = Rc::new(RefCell::new(Vec::new()));
    picker.on_change({
        let seen = seen.clone();
        move |value| seen.borrow_mut().push(value)
    });

    edit_natively(&picker.native_picker(), DateTime::date(2030, 5, 5));
    assert_eq!(
        picker.value(),
        DateTime::new(2026, 12, 31, 9, 30),
        "上限で止まり、時刻は動かない"
    );
    assert_eq!(*seen.borrow(), vec![DateTime::new(2026, 12, 31, 9, 30)]);

    // 範囲の外にある値を渡すと、通知せずに端へ寄る。
    picker.set_value(DateTime::date(1990, 1, 1));
    assert_eq!(picker.value(), DateTime::new(2026, 1, 1, 0, 0));
    assert_eq!(seen.borrow().len(), 1);

    // 範囲を外すと自由に選べる。
    picker.set_range(None, None);
    assert!(picker.native_picker().minDate().is_none());
    edit_natively(&picker.native_picker(), DateTime::date(1990, 1, 1));
    assert_eq!(picker.value(), DateTime::date(1990, 1, 1));

    // 時刻だけの表示では日付を見ない。
    let time_only = ui.date_picker(DatePickerMode::Time)?;
    time_only.set_value(DateTime::new(2026, 8, 22, 6, 0));
    time_only.set_range(Some(DateTime::time(9, 0)), Some(DateTime::time(18, 0)));
    assert_eq!(
        time_only.value(),
        DateTime::new(2026, 8, 22, 9, 0),
        "日付はそのままに、時刻だけが下限へ寄る"
    );
    assert!(
        time_only.native_picker().minDate().is_none(),
        "日付での制限はネイティブへ渡さない"
    );
    Ok(())
}

/// 通知の最中に同じピッカーを操作し、通知先も差し替えられる。
fn date_picker_callback_is_reentrant(ui: &Ui) -> Result<()> {
    let picker = ui.date_picker(DatePickerMode::DateTime)?;
    picker.set_value(DateTime::new(2026, 8, 22, 9, 30));
    let seen = Rc::new(RefCell::new(Vec::new()));
    picker.on_change({
        let seen = seen.clone();
        let picker = picker.clone();
        move |value| {
            seen.borrow_mut().push(value);
            // 通知の中から値を読み書きしても borrow が衝突しない。
            let _ = picker.value();
            picker.set_value(DateTime::new(2026, 12, 31, 0, 0));
            picker.on_change({
                let seen = seen.clone();
                move |value| seen.borrow_mut().push(value)
            });
        }
    });

    edit_natively(&picker.native_picker(), DateTime::new(2027, 1, 5, 18, 45));
    assert_eq!(*seen.borrow(), vec![DateTime::new(2027, 1, 5, 18, 45)]);
    assert_eq!(picker.value(), DateTime::new(2026, 12, 31, 0, 0));

    edit_natively(&picker.native_picker(), DateTime::new(2028, 2, 29, 8, 0));
    assert_eq!(
        *seen.borrow(),
        vec![
            DateTime::new(2027, 1, 5, 18, 45),
            DateTime::new(2028, 2, 29, 8, 0)
        ],
        "差し替えた通知先が呼ばれる"
    );
    Ok(())
}

/// 時刻ピッカーは `NSDatePicker` の時分だけを出したもの。値はネイティブと
/// 往復し、`set_value` は通知しない。
fn time_picker_value_round_trips(ui: &Ui) -> Result<()> {
    let picker = ui.time_picker()?;
    let native = picker.native_picker();
    assert_eq!(
        native.datePickerElements(),
        NSDatePickerElementFlags::HourMinute,
        "時分だけを出すこと"
    );
    assert!(
        !native.presentsCalendarOverlay(),
        "時刻だけの表示に暦は出さない"
    );
    // 作った直後は現在時刻。
    assert!(picker.value().is_valid(), "{}", picker.value());

    let seen = Rc::new(RefCell::new(Vec::new()));
    picker.on_change({
        let seen = seen.clone();
        move |value| seen.borrow_mut().push(value)
    });

    picker.set_value(Time::new(9, 30));
    assert_eq!(picker.value(), Time::new(9, 30));
    assert!(seen.borrow().is_empty(), "set_value は通知しない");

    // 時計として成り立たない値は丸める。
    picker.set_value(Time::new(25, 70));
    assert_eq!(picker.value(), Time::new(23, 59));

    // 利用者がスピナーを回したときの経路 (target/action)。
    edit_time_natively(&native, Time::new(18, 45));
    assert_eq!(picker.value(), Time::new(18, 45));
    assert_eq!(*seen.borrow(), vec![Time::new(18, 45)]);

    // 同じ値で通知が来ても、値が動いていなければ知らせない。
    edit_time_natively(&native, Time::new(18, 45));
    assert_eq!(seen.borrow().len(), 1);

    picker.set_enabled(false);
    assert!(!native.isEnabled());
    Ok(())
}

/// 下限・上限の外へは出ない。日付を 1970-01-01 へ固定しているので、
/// `NSDatePicker` にも範囲をそのまま渡せる。
fn time_picker_stays_inside_the_range(ui: &Ui) -> Result<()> {
    let picker = ui.time_picker()?;
    picker.set_value(Time::new(12, 0));
    picker.set_range(Some(Time::new(9, 0)), Some(Time::new(18, 0)));
    assert_eq!(picker.value(), Time::new(12, 0));
    let native = picker.native_picker();
    assert!(native.minDate().is_some());
    assert!(native.maxDate().is_some());

    let seen = Rc::new(RefCell::new(Vec::new()));
    picker.on_change({
        let seen = seen.clone();
        move |value| seen.borrow_mut().push(value)
    });

    edit_time_natively(&native, Time::new(22, 0));
    assert_eq!(picker.value(), Time::new(18, 0), "上限で止まること");
    assert_eq!(*seen.borrow(), vec![Time::new(18, 0)]);

    // 範囲の外にある値を渡すと、通知せずに端へ寄る。
    picker.set_value(Time::new(1, 0));
    assert_eq!(picker.value(), Time::new(9, 0));
    assert_eq!(seen.borrow().len(), 1);

    // 範囲を外すと自由に選べる。
    picker.set_range(None, None);
    assert!(native.minDate().is_none());
    assert!(native.maxDate().is_none());
    edit_time_natively(&native, Time::new(1, 0));
    assert_eq!(picker.value(), Time::new(1, 0));

    // 通知の中から同じピッカーを操作しても borrow が衝突しない。
    picker.on_change({
        let picker = picker.clone();
        let seen = seen.clone();
        move |value| {
            seen.borrow_mut().push(value);
            let _ = picker.value();
            picker.set_value(Time::MIDNIGHT);
        }
    });
    edit_time_natively(&native, Time::new(6, 15));
    assert_eq!(picker.value(), Time::MIDNIGHT);
    assert_eq!(
        *seen.borrow(),
        vec![Time::new(18, 0), Time::new(1, 0), Time::new(6, 15)],
        "利用者の操作はどれも 1 回だけ届くこと"
    );
    Ok(())
}

/// ギャラリーのように、縦に長い中身をスクロールへ載せてタブに入れると、
/// 下まで送れる (ウィンドウの高さで切れて終わりにならない)。
fn tall_tab_content_scrolls(ui: &Ui) -> Result<()> {
    let window = ui.window("t", 320.0, 240.0)?;
    let pane = ui.stack(Orientation::Vertical)?;
    for i in 0..40 {
        pane.append(&ui.label(&format!("行 {i}"))?);
    }
    let scroll = ui.scroll()?;
    scroll.set_policy(ScrollPolicy::Never, ScrollPolicy::Auto);
    scroll.set_child(&pane);
    scroll.set_sizing(Sizing::fill());
    let tabs = ui.tabs()?;
    tabs.add_tab("t", &scroll);
    tabs.set_sizing(Sizing::fill());
    window.set_child(&tabs);
    window.show();
    window
        .native_window()
        .contentView()
        .unwrap()
        .layoutSubtreeIfNeeded();

    let native = scroll
        .native_view()
        .downcast::<NSScrollView>()
        .expect("NSScrollView");
    let clip = native.contentView();
    let document = unsafe { native.documentView() }.expect("中身");
    let visible = clip.bounds().size.height;
    let content = document.frame().size.height;
    assert!(visible > 0.0, "タブの中でスクロールが領域を持つ");
    assert!(
        content > visible,
        "中身のほうが高い ({content} > {visible})"
    );
    assert_eq!(
        document.frame().size.width,
        clip.bounds().size.width,
        "横は送らないので幅はクリップに合わせる"
    );

    // 下端まで送れる。
    let hidden = content - visible;
    clip.scrollToPoint(NSPoint::new(0.0, hidden));
    unsafe { native.reflectScrolledClipView(&clip) };
    assert_eq!(clip.bounds().origin.y, hidden, "最後の行まで届く");
    window.close();
    Ok(())
}

/// スクローラが場所を取っても、中身の幅は「見えている幅」に収まる。
///
/// 重ね表示 (overlay) のスクローラは中身に重なるだけで場所を取らないが、
/// 「常に表示」の設定や、ポインティングデバイスの無い環境では場所を取る
/// 様式 (legacy) が選ばれる。そのとき中身がスクローラのぶんはみ出すと、
/// 横へ送らない設定では二度と見えない。
///
/// 利用者の設定に左右されずに確かめたいので、様式をこの場で指定する。
fn scroll_content_fits_beside_a_legacy_scroller(ui: &Ui) -> Result<()> {
    let window = ui.window("t", 320.0, 240.0)?;
    let pane = ui.stack(Orientation::Vertical)?;
    for i in 0..40 {
        pane.append(&ui.label(&format!("行 {i}"))?);
    }
    let scroll = ui.scroll()?;
    scroll.set_policy(ScrollPolicy::Never, ScrollPolicy::Auto);
    scroll.set_child(&pane);
    scroll.set_sizing(Sizing::fill());
    window.set_child(&scroll);

    let native = scroll
        .native_view()
        .downcast::<NSScrollView>()
        .expect("NSScrollView");
    native.setScrollerStyle(NSScrollerStyle::Legacy);
    window.show();
    window
        .native_window()
        .contentView()
        .unwrap()
        .layoutSubtreeIfNeeded();

    let clip = native.contentView();
    let document = unsafe { native.documentView() }.expect("中身");
    let visible = clip.bounds().size.width;
    assert!(
        visible < native.frame().size.width,
        "スクローラが場所を取っている ({visible} < {})",
        native.frame().size.width
    );
    assert_eq!(
        document.frame().size.width,
        visible,
        "中身の幅はスクローラを除いた見える幅に合う"
    );
    window.close();
    Ok(())
}

/// 複数列のグリッドでは、`Fill` の子はそのセルの取り分だけを望む。
///
/// グリッド全体の幅をそのまま望むと、同じ行にあるほかのセルの中身
/// (compression resistance がこの希望より弱いもの) を押し潰しかねない。
fn grid_fill_column_takes_only_its_share(ui: &Ui) -> Result<()> {
    let grid = ui.grid()?;
    grid.set_column_track(0, Track::Auto);
    grid.set_column_track(1, Track::FILL);
    grid.set_column_track(2, Track::Auto);
    grid.set_spacing(8.0, 0.0);

    let leading = ui.checkbox("")?;
    let middle = ui.stack(Orientation::Vertical)?;
    middle.set_align(Align::Start);
    middle.append(&ui.label("Wi-Fi")?);
    middle.set_sizing(Sizing::fill_width());
    let trailing = ui.button("オプション…")?;
    grid.attach(&leading, GridCell::new(0, 0));
    grid.attach(&middle, GridCell::new(1, 0));
    grid.attach(&trailing, GridCell::new(2, 0));

    let root = grid.native_view();
    root.setFrameSize(NSSize::new(600.0, 60.0));
    root.layoutSubtreeIfNeeded();

    let leading_width = leading.native_view().frame().size.width;
    let middle_width = middle.native_view().frame().size.width;
    let trailing_width = trailing.native_view().frame().size.width;

    // 望むのは「グリッドの幅 − ほかの列」。定数が引かれていないと、1 つの
    // セルの子がグリッド全体の幅を要求してしまう。
    let constraints = root.constraints();
    let grow = (0..constraints.len())
        .map(|index| constraints.objectAtIndex(index))
        .find(|constraint| {
            constraint
                .identifier()
                .is_some_and(|id| id.to_string() == "naui.grid.grow.width.1.0")
        })
        .expect("Fill のセルへ伸びる希望が張られていること");
    assert!(
        grow.constant() <= -(leading_width + trailing_width),
        "ほかの列の幅が差し引かれていること: {} / {leading_width} + {trailing_width}",
        grow.constant()
    );

    // それでいて、余りは Fill の列が受け取る。
    assert!(
        middle_width > 400.0,
        "Fill の列が余りを受け取ること: {middle_width}"
    );
    assert!(
        trailing_width > 60.0,
        "ほかの列は中身の幅を保つこと: {trailing_width}"
    );
    assert!(
        leading_width + middle_width + trailing_width <= 600.0,
        "グリッドからはみ出さないこと: {leading_width} + {middle_width} + {trailing_width}"
    );
    Ok(())
}

/// 幅を決め打ちした列は、中身が短くても `Fill` の子に食われない。
///
/// 取り分を中身の幅だけで見積もると、固定幅の列に載せたものが短いときに
/// `Fill` の列へ余分な幅を渡してしまい、後続の列と重なる。
fn grid_fill_column_leaves_fixed_columns(ui: &Ui) -> Result<()> {
    let grid = ui.grid()?;
    grid.set_column_track(0, Track::Fixed(120.0));
    grid.set_column_track(1, Track::FILL);
    grid.set_column_track(2, Track::Auto);
    grid.set_spacing(8.0, 0.0);

    // 120pt の列に、それより狭い中身を置く。
    let leading = ui.label("短い")?;
    let middle = ui.stack(Orientation::Vertical)?;
    middle.set_align(Align::Start);
    middle.append(&ui.label("本文")?);
    middle.set_sizing(Sizing::fill_width());
    let trailing = ui.button("オプション…")?;
    grid.attach(&leading, GridCell::new(0, 0));
    grid.attach(&middle, GridCell::new(1, 0));
    grid.attach(&trailing, GridCell::new(2, 0));

    let root = grid.native_view();
    root.setFrameSize(NSSize::new(600.0, 60.0));
    root.layoutSubtreeIfNeeded();

    let leading_width = leading.native_view().frame().size.width;
    let middle_frame = middle.native_view().frame();
    let trailing_frame = trailing.native_view().frame();
    assert!(
        leading_width < 120.0,
        "固定幅より中身が狭いこと (この検査の前提): {leading_width}"
    );

    // 差し引くのは中身の幅ではなく、列に指定した 120pt。
    let constraints = root.constraints();
    let grow = (0..constraints.len())
        .map(|index| constraints.objectAtIndex(index))
        .find(|constraint| {
            constraint
                .identifier()
                .is_some_and(|id| id.to_string() == "naui.grid.grow.width.1.0")
        })
        .expect("Fill のセルへ伸びる希望が張られていること");
    assert!(
        grow.constant() <= -(120.0 + trailing_frame.size.width),
        "固定幅の列がそのまま差し引かれること: {} / 120 + {}",
        grow.constant(),
        trailing_frame.size.width
    );

    assert!(
        middle_frame.origin.x >= 120.0,
        "Fill の列が固定幅の列へ食い込まないこと: {middle_frame:?}"
    );
    assert!(
        middle_frame.origin.x + middle_frame.size.width <= trailing_frame.origin.x + 0.5,
        "Fill の列が後続の列と重ならないこと: {middle_frame:?} / {trailing_frame:?}"
    );
    assert!(
        trailing_frame.origin.x + trailing_frame.size.width <= 600.5,
        "グリッドからはみ出さないこと: {trailing_frame:?}"
    );
    assert!(
        middle_frame.size.width > 300.0,
        "それでも余りは Fill の列が受け取ること: {middle_frame:?}"
    );
    Ok(())
}

/// `Auto` の行に置いた子は、縦の余りを受け取らない。
///
/// NSStackView のようにコンテナ自身の hugging priority が低い子は、これが
/// 無いと `Fill` 行より先に余りを吸ってしまい、`Fill` 行が中身の高さ
/// (タブなら見出しだけ) まで潰れる。
fn grid_fill_row_keeps_the_rest(ui: &Ui) -> Result<()> {
    let grid = ui.grid()?;
    grid.set_column_track(0, Track::FILL);
    grid.set_row_track(0, Track::Auto);
    grid.set_row_track(1, Track::FILL);

    let header = ui.stack(Orientation::Vertical)?;
    header.append(&ui.label("見出し")?);
    header.set_sizing(Sizing::fill_width());
    grid.attach(&header, GridCell::new(0, 0));

    let pane = ui.stack(Orientation::Vertical)?;
    for i in 0..30 {
        pane.append(&ui.label(&format!("行 {i}"))?);
    }
    let scroll = ui.scroll()?;
    scroll.set_policy(ScrollPolicy::Never, ScrollPolicy::Auto);
    scroll.set_child(&pane);
    scroll.set_sizing(Sizing::fill());
    grid.attach(&scroll, GridCell::new(0, 1));

    let vertical = NSLayoutConstraintOrientation::Vertical;
    assert_eq!(
        header
            .native_view()
            .contentHuggingPriorityForOrientation(vertical),
        999.0,
        "Auto の子は縦にも内容の高さを保つ"
    );
    assert_eq!(
        scroll
            .native_view()
            .contentHuggingPriorityForOrientation(vertical),
        1.0,
        "Fill の子は余りを受け取る"
    );

    // hugging priority だけでは、どの行が余りを受け取るかが AppKit 任せで
    // 安定しない。`Fill` のセルには「グリッドいっぱいまで伸びたい」という
    // 最弱の希望も張っておく。
    let root = grid.native_view();
    let constraints = root.constraints();
    let grow = (0..constraints.len())
        .map(|index| constraints.objectAtIndex(index))
        .find(|constraint| {
            constraint
                .identifier()
                .is_some_and(|id| id.to_string() == "naui.grid.grow.height.0.1")
        })
        .expect("Fill のセルへ伸びる希望が張られていること");
    // 弱すぎると余りが `Auto` 行に残り、強すぎると `Auto` 行の中身を潰す。
    // 中身を守る compression resistance (750) より必ず弱くしておく。
    assert!(
        (251.0..750.0).contains(&grow.priority()),
        "希望の優先度が中庸であること: {}",
        grow.priority()
    );

    root.setFrameSize(NSSize::new(500.0, 600.0));
    root.layoutSubtreeIfNeeded();
    let header_height = header.native_view().frame().size.height;
    let scroll_height = scroll.native_view().frame().size.height;
    assert!(
        header_height < 100.0,
        "見出しは中身の高さのまま: {header_height}"
    );
    assert!(
        scroll_height > 400.0,
        "Fill 行が残りの高さを受け取る: {scroll_height}"
    );
    Ok(())
}

// ----------------------------------------------------------------- 非同期

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

/// 別スレッドから送った値が、メインキュー経由でラベルへ届く。
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
        "ランループを回すまでは届かないこと (必ず後回しになる)"
    );

    pump(0.05);
    assert_eq!(label.text(), "完了", "メインキュー経由で届くこと");
    Ok(())
}

/// spawn した処理がランループの上で進み、完了する。
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

    pump(0.05);
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
    pump(0.05);

    assert_eq!(label.text(), "待機中", "cancel した処理は走らないこと");
    Ok(())
}
