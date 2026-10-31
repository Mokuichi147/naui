//! Behavior checks against real AppKit controls.
//!
//! They trigger native-side operations such as `performClick`, and check that they
//! reach Rust closures and that the native state changes.
//!
//! AppKit can only be touched on the main thread, but Rust's standard test harness
//! runs each test on a separate thread (even with `--test-threads=1`). So
//! `harness = false` is set and a custom runner runs on the main thread.

use std::cell::{Cell, RefCell};
use std::future::Future;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::pin::Pin;
use std::rc::Rc;
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use naui_core::{
    Align, ButtonRole, Color, DatePickerMode, DateTime, DialogButtons, DialogResponse,
    EventResponse, FileFilter, FilePickerMode, Fit, GridCell, Length, ListItem, MenuItem,
    MenuShortcut, MenuSpec, NavItem, Orientation, Padding, PlaybackState, Point, PointerButton,
    PointerPhase, PopupItem, Rect, Result, ScrollPolicy, SelectionMode, SidebarItem,
    SidebarSection, Sizing, SortOrder, TableColumn, TableRow, TextColor, TextStyle, Theme, Time,
    ToolbarIcon, ToolbarItem, Track, TreeItem, WindowPlacement, WindowState, DEFAULT_SIDEBAR_WIDTH,
};
use naui_macos::{run_for_test, ListRow, TableCells, Ui, Widget};
use objc2::rc::{autoreleasepool, Retained};
use objc2::runtime::{NSObjectProtocol, ProtocolObject};
use objc2::sel;
use objc2::{
    define_class, msg_send, AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, Message,
};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSButton, NSColor, NSColorSpace, NSComboBox,
    NSControlStateValueOff, NSControlStateValueOn, NSDatePicker, NSDatePickerElementFlags, NSEvent,
    NSEventMask, NSEventModifierFlags, NSEventType, NSImage, NSImageScaling, NSImageView,
    NSLayoutConstraint, NSLayoutConstraintOrientation, NSOutlineViewDelegate, NSResponder,
    NSScrollView, NSScrollerStyle, NSSearchField, NSSecureTextField, NSSegmentedControl, NSStepper,
    NSSwitch, NSTableViewDelegate, NSTextField, NSTextInputClient, NSTextView, NSTintProminence,
    NSUserInterfaceItemIdentification, NSView, NSWindow, NSWindowStyleMask,
    NSWindowTitleVisibility,
};
use objc2_foundation::{
    NSCalendar, NSCalendarIdentifierGregorian, NSDate, NSDateComponents, NSDefaultRunLoopMode,
    NSNotFound, NSNotification, NSPoint, NSRange, NSRect, NSRunLoop, NSSize, NSString,
};

/// One test case.
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
        (
            "描画面が on_draw の命令を AppKit で画素に落とす",
            canvas_paints_recorded_commands,
        ),
        (
            "描画面がマウスの押下・移動・解放を面の座標で通知する",
            canvas_reports_pointer_events,
        ),
        (
            "描画面がホイールを論理ピクセルで通知し、Continue なら外側へ回す",
            canvas_reports_wheel,
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
            "リストの長い文字が行の幅に収まる",
            list_long_text_stays_inside_the_row,
        ),
        (
            "選べない行は本文も補助も無効な色になる",
            list_disabled_row_dims_both_lines,
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
            "リストのダブルクリックと Return が行の起動を通知する",
            list_row_activate_notifies,
        ),
        (
            "リストの右クリックが行と位置を通知する",
            list_context_menu_notifies,
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
        (
            "テーブルが見えている行だけを組み立てる",
            table_builds_only_the_rows_it_shows,
        ),
        (
            "テーブルのセルにウィジェットを置ける",
            table_widget_cells_are_native,
        ),
        (
            "テーブルの行の高さを決められる",
            table_row_height_follows_the_setting,
        ),
        (
            "選べるかどうかを行を組み立てずに答えられる",
            table_row_selectable_answers_without_building,
        ),
        (
            "行を絞っていても見出しの並べ替えが届く",
            table_sorting_works_while_windowed,
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
        (
            "パンくずが文字の左端で同じ列のラベルとそろう",
            breadcrumbs_align_with_labels,
        ),
        (
            "パンくずのフォーカスの受け取りがボタンと同じ",
            breadcrumbs_take_focus_like_a_button,
        ),
        (
            "パンくずをクリックしてもフォーカスの枠が残らない",
            breadcrumbs_click_leaves_no_focus_ring,
        ),
        ("ページ送りが範囲内に収まる", pagination_steps),
        ("リンクのクリックがクロージャへ届く", link_click),
        ("テーマを実行中に切り替えられる", theme_switch),
        (
            "OS の優先言語を読め、変わると知らせる",
            preferred_languages_follow_the_os,
        ),
        (
            "優先言語の知らせの中の panic でアプリが落ちない",
            preferred_languages_survive_a_panicking_closure,
        ),
        (
            "落とされたファイルがポインターの下のウィジェットか窓へ届く",
            file_drop_reaches_the_widget_under_the_pointer,
        ),
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
            "追加したあとの set_sizing も交差軸の Fill に効く",
            stack_fill_cross_after_append,
        ),
        (
            "追加したあとの set_sizing も主軸の Fill に効く",
            stack_fill_main_after_append,
        ),
        (
            "置いたあとの set_sizing もグリッドのマスに効く",
            grid_fill_after_attach,
        ),
        (
            "積んだあとで間隔を変えても子どうしの間に効く",
            stack_spacing_after_append,
        ),
        (
            "Stack へ差し込んでも外しても向きを変えても入力欄のフォーカスと変換中の文字が残る",
            stack_insert_keeps_focus,
        ),
        (
            "Stack の向きを変えると最初からその向きで作ったのと同じ並びになる",
            stack_switches_orientation,
        ),
        (
            "向きを変えて同じ向きの Stack に入った Stack は余白を外側へ譲る",
            nested_stack_switched_to_the_same_direction,
        ),
        (
            "別の Stack へ移した子は新しい親に合わせて広がる",
            moved_child_follows_the_new_stack,
        ),
        (
            "移した子はスクロール位置を保ち、フォーカスは外れて変換中の文字は確定する",
            moved_widget_keeps_scroll_but_not_focus,
        ),
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
            "上限付きの子がいても Grid の Fill 行は余りを受け取る",
            grid_fill_row_takes_the_rest_around_a_capped_child,
        ),
        (
            "上限付きと上限なしの Fill の行・列が並んでも、上限付きは上限まで広がる",
            grid_capped_and_plain_fill_tracks_share_the_room,
        ),
        (
            "上限付きと上限なしの Fill の行・列は、狭いときウィンドウを押し広げない",
            grid_capped_and_plain_fill_tracks_fit_a_small_window,
        ),
        (
            "入れ子の縦 Stack が余りの高さを吸わない",
            nested_stack_does_not_take_the_slack,
        ),
        (
            "主軸に Fill を指定した入れ子の Stack は余りを受け取る",
            nested_stack_with_fill_takes_the_slack,
        ),
        (
            "グリッドの Auto 行が後からの高さ変化に追従する",
            grid_auto_row_follows_later_growth,
        ),
        (
            "グリッドの Auto 行が後から足した一覧の行に追従する",
            grid_auto_row_follows_added_list_rows,
        ),
        (
            "グリッドの Auto 行が折り返しラベルの行数に追従する",
            grid_auto_row_follows_wrapped_label,
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
            "グリッドの Auto 列が再レイアウトで余りを受け取らない",
            grid_auto_column_keeps_content_width,
        ),
        (
            "Gallery のメディア欄が高さ変更後にラベル幅を広げない",
            gallery_media_status_does_not_expand,
        ),
        ("グリッドの子を置き換える", grid_replaces_child),
        ("スクロールが中身を保持する", scroll_keeps_child),
        (
            "隠したものは Stack の中で場所を空けない",
            hidden_widget_leaves_the_stack_layout,
        ),
        (
            "折りたたみを開いてもアプリが隠した中身は隠れたまま",
            expander_keeps_the_app_hidden_child,
        ),
        (
            "フォーカスが入力欄と複数行入力の中へ移る",
            request_focus_reaches_the_editor,
        ),
        (
            "フォーカスの出入りが持ち主の変わったときだけ通知される",
            focus_change_follows_the_first_responder,
        ),
        (
            "捨てた入力欄の on_focus_change の通知先が解放される",
            focus_change_handler_is_freed_with_the_widget,
        ),
        (
            "通知のクロージャが弱参照で欄自身を触れ、外して手放せば解放される",
            weak_widget_breaks_the_callback_cycle,
        ),
        ("ツールチップが NSView へ届く", tooltip_reaches_the_view),
        (
            "複数行入力の on_key_down が Shift なしの Enter を止める",
            text_area_key_down_can_take_enter,
        ),
        (
            "変換中のキーは on_key_down に届かない",
            key_down_skips_composition,
        ),
        (
            "ウィジェットが Continue を返したキーだけがウィンドウへ届く",
            window_key_down_gets_what_the_widget_left,
        ),
        (
            "タイマーがメインループで呼ばれ、止められる",
            timers_fire_on_the_main_loop,
        ),
        (
            "不確定の進捗は値を覚えたまま動きに切り替わる",
            progress_bar_switches_to_indeterminate,
        ),
        (
            "スピナーは回る形の NSProgressIndicator で、止めると描かずに場所を残す",
            spinner_is_a_spinning_indicator,
        ),
        ("ラベルを選べるようにできる", label_can_be_selectable),
        (
            "クリップボードへ書いた文字を読み戻せる",
            clipboard_round_trips_text,
        ),
        ("URL として読めないものは開かない", open_url_rejects_garbage),
        (
            "ウィンドウの大きさを読み、変わったら通知する",
            window_reports_its_size,
        ),
        (
            "ツールバーとサイドバーを付け外ししても中身の大きさは変わらない",
            window_keeps_its_size_across_toolbar_and_sidebar,
        ),
        (
            "閉じる確認で KeepOpen を返すと閉じない",
            window_close_request_can_keep_it_open,
        ),
        (
            "ウィンドウの位置がネイティブと往復する",
            window_position_round_trips,
        ),
        (
            "ウィンドウの最大化・最小化・全画面が往復し通知される",
            window_state_round_trips,
        ),
        (
            "最大化・最小化・全画面の間の位置は通常へ戻ったときの位置",
            window_position_while_not_normal,
        ),
        (
            "表示する前に頼んだ状態で表示される",
            window_state_requested_before_show,
        ),
        (
            "保存した大きさ・位置・状態は最大化や全画面の間も通常の枠を指す",
            window_placement_while_not_normal,
        ),
        (
            "保存した大きさ・位置・状態を表示する前に当てると、その枠で出る",
            window_placement_restores_before_show,
        ),
        (
            "状態の通知の中から頼んだ状態も通知される",
            window_state_change_inside_the_notification,
        ),
        (
            "大きさを変えられるかどうかが styleMask と往復する",
            window_resizable_round_trips,
        ),
        (
            "読み上げ名・読み取り専用・最小の大きさがネイティブへ届く",
            accessible_label_read_only_and_min_size,
        ),
        (
            "一覧・表・ツリーで指定した行まで送れる",
            rows_can_be_scrolled_into_view,
        ),
        (
            "スクロールの位置を測り、指定した位置と末尾へ送る",
            scroll_measures_and_moves,
        ),
        (
            "中身を足した直後でも末尾まで送れる",
            scroll_to_end_after_appending,
        ),
        (
            "表示前に頼んだ末尾送りが表示後に効く",
            scroll_to_end_before_shown,
        ),
        (
            "スクロールの通知は位置が変わったときだけ届く",
            scroll_notifies_only_moves,
        ),
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
            "ラベルの段階と役割が AppKit のフォントと色へ写る",
            label_style_and_color_map_to_appkit,
        ),
        (
            "ボタンの役割が NSButton の属性へ写り、既定へ戻せる",
            button_role_maps_to_appkit,
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
            "ポップオーバーがウィジェットの横に中身を出し、閉じても通知しない",
            popover_shows_its_content_beside_the_widget,
        ),
        (
            "利用者がポップオーバーを閉じると on_close が呼ばれる",
            popover_dismissed_by_the_user_notifies,
        ),
        (
            "ポップオーバーが指定した側に出る",
            popover_follows_the_position,
        ),
        (
            "画面に出ていないウィジェットにはポップオーバーを開かない",
            popover_needs_a_shown_widget,
        ),
        (
            "幅の決まらない中身のポップオーバーにも幅がある",
            popover_gives_width_to_content_without_one,
        ),
        (
            "表示中に中身が伸びるとポップオーバーも伸びる",
            popover_grows_with_its_content,
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
            "標準メニューの項目名が OS の言語に従う",
            menu_bar_follows_the_os_language,
        ),
        (
            "画像がローカルのファイルから読み込まれる",
            image_loads_a_local_file,
        ),
        (
            "Fill の画像は大きくても縦の Scroll を押し広げない",
            fill_image_does_not_push_a_scroll,
        ),
        (
            "上限付き Fill の画像は上限の大きさを求める",
            capped_fill_image_asks_for_its_maximum,
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
            "サイドバーがソースリストの NSTableView に項目と見出しを並べる",
            sidebar_rows_map_to_a_source_list,
        ),
        (
            "サイドバーの選択が通し番号で往復し、利用者の選択だけを通知する",
            sidebar_selection_round_trips,
        ),
        (
            "サイドバーが NSSplitViewController として取り付き外れる",
            sidebar_attaches_to_a_window,
        ),
        (
            "サイドバーの幅と開閉が NSSplitViewItem に届く",
            sidebar_width_and_collapse,
        ),
        (
            "サイドバーを付けたウィンドウではトーストが中身の区画に重なる",
            sidebar_toast_goes_to_the_content_pane,
        ),
        (
            "サイドバーを付けても子の大きさへウィンドウが縮まない",
            sidebar_child_does_not_shrink_the_window,
        ),
        (
            "サイドバーを閉じている間に置いた幅が、開き直したときに反映される",
            sidebar_width_set_while_collapsed_applies_on_reopen,
        ),
        (
            "サイドバーを付けるとツールバーの先頭にサイドバーボタンが入る",
            sidebar_adds_the_toggle_button_to_the_toolbar,
        ),
        (
            "サイドバーボタンで開閉すると on_collapse が呼ばれる",
            sidebar_toggle_notifies_collapse,
        ),
        (
            "サイドバーがウィンドウを狭めると畳まれ、広げると開き直す",
            sidebar_collapses_when_the_window_narrows,
        ),
        (
            "サイドバーの自動の開閉はライブリサイズで AppKit が決める",
            sidebar_collapses_during_live_resize,
        ),
        (
            "サイドバーの通知中に内容と通知先を差し替えられる",
            sidebar_callback_is_reentrant,
        ),
        (
            "メニューバーの見出しと項目が NSMenu になる",
            menu_bar_menus_map_to_native,
        ),
        (
            "メニューバーはアプリメニューと編集メニューを挟んで並べる",
            menu_bar_keeps_the_standard_menus,
        ),
        (
            "メニューバーの項目が経路で通知する",
            menu_bar_activation_notifies,
        ),
        (
            "メニューバーのサブメニューとチェックが NSMenu へ届く",
            menu_bar_submenus_and_checks_map_to_native,
        ),
        (
            "メニューバーのファンクションキーがキー等価として働く",
            menu_bar_function_keys_are_key_equivalents,
        ),
        (
            "メニューバーがアプリのメインメニューになる",
            menu_bar_attaches_to_the_application,
        ),
        (
            "メニューバーの通知内で組み替えと差し替えができる",
            menu_bar_callback_is_reentrant,
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
            "1 行入力とパスワード入力が Enter で確定を通知する",
            text_inputs_notify_on_return,
        ),
        (
            "読み取り専用の 1 行入力は Enter で確定しない",
            read_only_text_input_does_not_submit,
        ),
        (
            "数値入力が Enter で値を確定してから通知する",
            number_input_commits_before_submitting,
        ),
        (
            "ポップアップメニューの選択がクロージャへ届く",
            popup_menu_selection_notifies,
        ),
        (
            "ポップアップメニューのサブメニューとチェックが NSMenu へ届く",
            popup_menu_submenus_and_checks,
        ),
        (
            "ポップアップメニューが取り付け先のビューのメニューになる",
            popup_menu_attaches_to_a_view,
        ),
    ];

    let mut failed = 0;
    let mut skipped = 0;
    for (name, case) in cases {
        take_skip();
        let result = catch_unwind(AssertUnwindSafe(|| {
            run_for_test(*case).expect("テスト用の起動")
        }));
        match (result, take_skip()) {
            (Ok(()), None) => println!("ok   ... {name}"),
            // 走らせられなかった確認があるものは、成功として数えない。
            (Ok(()), Some(reason)) => {
                println!("SKIP ... {name} ({reason})");
                skipped += 1;
            }
            (Err(_), _) => {
                println!("FAIL ... {name}");
                failed += 1;
            }
        }
    }

    println!(
        "\n{} 件中 {} 件成功 / {} 件スキップ / {} 件失敗",
        cases.len(),
        cases.len() - failed - skipped,
        skipped,
        failed
    );
    if skipped > 0 {
        // スキップは「確かめられなかった」こと。緑で通すと、走っていない確認が
        // あることに気付けないので、失敗と同じ扱いにする。
        println!("確かめられなかったテストがあるので、成功としては終わらない。");
    }
    if failed > 0 || skipped > 0 {
        std::process::exit(1);
    }
}

thread_local! {
    /// Where **what the test could not verify** is recorded ([`skip`]).
    static SKIP_REASON: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// Records something that could not be verified because of the environment.
///
/// A test with a record is counted as SKIP rather than success, and **the exit code
/// is not 0 either**. Passing silently would turn green without verifying anything,
/// and CI would never notice.
fn skip(reason: &str) {
    SKIP_REASON.with(|slot| *slot.borrow_mut() = Some(reason.to_string()));
}

/// Takes out the record and resets it to empty.
fn take_skip() -> Option<String> {
    SKIP_REASON.with(|slot| slot.borrow_mut().take())
}

/// A button click reaches the Rust closure.
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

/// A check box changes its native state and notifies the new value.
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

/// A switch changes its native state and notifies the new value.
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

/// The switch is an `NSSwitch` itself, with the label placed next to it. The label
/// takes the leftover width, and the switch keeps its own size.
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

/// The color picker is an `NSColorWell` itself, and its value round-trips with the
/// native side. `set_value` does not notify, and `pick` notifies exactly once, like
/// a user action.
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

/// The color panel also returns catalog colors (`systemBlue` and so on). They are
/// converted to sRGB before reading components, so they can be received as a
/// [`Color`] as is.
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

/// The drawing surface is an `NSView`'s `drawRect:`, and the commands recorded by
/// `on_draw` are painted with `NSBezierPath`. The view's drawing is copied into a
/// bitmap, and pixels inside and outside the painted rectangle are read.
fn canvas_paints_recorded_commands(ui: &Ui) -> Result<()> {
    let canvas = ui.canvas()?;
    canvas.set_sizing(
        Sizing::new()
            .width(Length::Fixed(120.0))
            .height(Length::Fixed(80.0)),
    );
    let sizes = Rc::new(RefCell::new(Vec::new()));
    canvas.on_draw({
        let sizes = sizes.clone();
        move |painter| {
            sizes.borrow_mut().push((painter.width(), painter.height()));
            painter.fill_rect(Rect::new(0.0, 0.0, 120.0, 80.0), Color::WHITE);
            painter.fill_rect(
                Rect::new(10.0, 10.0, 40.0, 20.0),
                Color::rgb(0xff, 0x00, 0x00),
            );
            painter.set_text_align(Align::End);
            painter.text("naui", Point::new(110.0, 50.0), 12.0, Color::BLACK);
        }
    });

    let window = ui.window("canvas", 200.0, 160.0)?;
    window.set_child(&canvas);
    window.show();
    let view = canvas.native_view();
    view.layoutSubtreeIfNeeded();
    assert_eq!(canvas.size(), (120.0, 80.0), "指定した大きさになること");

    let bounds = view.bounds();
    let bitmap = view
        .bitmapImageRepForCachingDisplayInRect(bounds)
        .expect("ビットマップを用意できること");
    unsafe { view.cacheDisplayInRect_toBitmapImageRep(bounds, &bitmap) };
    // ウィンドウを出したときにも 1 回描かれるので、回数ではなく大きさを見る。
    let drawn = sizes.borrow().len();
    assert!(drawn >= 1, "描くときに呼ばれること");
    assert!(
        sizes.borrow().iter().all(|&size| size == (120.0, 80.0)),
        "面の大きさで呼ばれること: {:?}",
        sizes.borrow()
    );

    // 左上原点なので、(20, 15) は赤い矩形の中、(80, 60) は白い地。
    // Retina では 1 論理ピクセルが複数の画素になるので、倍率を掛けて読む。
    let scale = bitmap.pixelsWide() as f64 / bounds.size.width;
    let read = |x: f64, y: f64| -> Color {
        let color = bitmap
            .colorAtX_y((x * scale) as isize, (y * scale) as isize)
            .expect("画素が読めること");
        let srgb = color
            .colorUsingColorSpace(&NSColorSpace::sRGBColorSpace())
            .expect("sRGB へ変換できること");
        Color::from_unit(
            srgb.redComponent(),
            srgb.greenComponent(),
            srgb.blueComponent(),
        )
    };
    // ビットマップの色空間はディスプレイのものなので、値は sRGB へ戻しても
    // 少しずれる (#ff0000 が #f14b2d になる)。赤が勝っていることだけを見る。
    let inside = read(20.0, 15.0);
    assert!(
        inside.r > 180 && inside.g < 120 && inside.b < 120,
        "矩形の中は赤いこと: {inside}"
    );
    let outside = read(80.0, 60.0);
    assert!(
        outside.r > 200 && outside.g > 200 && outside.b > 200,
        "矩形の外は白いこと: {outside}"
    );
    // 右寄せの文字は x=110 より左に描かれる。文字の行 (y=50..62) の
    // 右端付近に暗い画素がある一方、x=112 より右には無い。
    let dark = |x: f64, y: f64| {
        let c = read(x, y);
        u32::from(c.r) + u32::from(c.g) + u32::from(c.b) < 600
    };
    let mut found = false;
    for x in 85..110 {
        for y in 50..64 {
            if dark(f64::from(x), f64::from(y)) {
                found = true;
            }
        }
    }
    assert!(found, "文字が右寄せで描かれていること");
    for y in 50..64 {
        assert!(!dark(114.0, f64::from(y)), "右端より右には描かないこと");
    }

    // 描き直しを頼むと次の描画でまた呼ばれる。
    canvas.redraw();
    unsafe { view.cacheDisplayInRect_toBitmapImageRep(bounds, &bitmap) };
    assert!(sizes.borrow().len() > drawn, "描き直しで呼ばれること");
    window.close();
    Ok(())
}

/// Mouse presses, drags and releases arrive converted from window coordinates to
/// surface coordinates.
fn canvas_reports_pointer_events(ui: &Ui) -> Result<()> {
    use objc2_core_graphics::{CGEvent, CGEventType, CGMouseButton};

    let canvas = ui.canvas()?;
    canvas.set_sizing(
        Sizing::new()
            .width(Length::Fixed(100.0))
            .height(Length::Fixed(60.0)),
    );
    let seen = Rc::new(RefCell::new(Vec::new()));
    canvas.on_pointer({
        let seen = seen.clone();
        move |event| seen.borrow_mut().push(event)
    });

    let window = ui.window("canvas", 100.0, 60.0)?;
    window.set_child(&canvas);
    window.show();
    let view = canvas.native_view();
    view.layoutSubtreeIfNeeded();
    let native_window = window.native_window();

    // 面の (30, 20) をウィンドウ座標へ直して、本物の NSEvent を作る。
    let send = |kind: NSEventType, x: f64, y: f64| {
        let point = view.convertPoint_toView(NSPoint::new(x, y), None);
        let event = NSEvent::mouseEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_clickCount_pressure(
            kind,
            point,
            NSEventModifierFlags::empty(),
            0.0,
            native_window.windowNumber(),
            None,
            0,
            1,
            1.0,
        )
        .expect("マウスイベント");
        match kind {
            NSEventType::LeftMouseDown => view.mouseDown(&event),
            NSEventType::LeftMouseDragged => view.mouseDragged(&event),
            NSEventType::LeftMouseUp => view.mouseUp(&event),
            NSEventType::RightMouseDown => view.rightMouseDown(&event),
            NSEventType::RightMouseUp => view.rightMouseUp(&event),
            NSEventType::OtherMouseDown => view.otherMouseDown(&event),
            _ => view.otherMouseUp(&event),
        }
    };
    send(NSEventType::LeftMouseDown, 30.0, 20.0);
    send(NSEventType::LeftMouseDragged, 50.0, 40.0);
    send(NSEventType::LeftMouseUp, 50.0, 40.0);

    {
        let seen = seen.borrow();
        assert_eq!(seen.len(), 3);
        assert_eq!(seen[0].phase, PointerPhase::Down);
        assert_eq!(seen[1].phase, PointerPhase::Move);
        assert_eq!(seen[2].phase, PointerPhase::Up);
        let near = |a: Point, x: f64, y: f64| (a.x - x).abs() < 0.5 && (a.y - y).abs() < 0.5;
        assert!(
            near(seen[0].point, 30.0, 20.0),
            "左上原点で届くこと: {:?}",
            seen[0]
        );
        assert!(near(seen[2].point, 50.0, 40.0), "{:?}", seen[2]);
    }

    // 左ボタンは主ボタン。動きにはボタンが付かない。
    {
        let seen = seen.borrow();
        let buttons: Vec<_> = seen.iter().map(|e| e.button).collect();
        assert_eq!(
            buttons,
            vec![
                Some(PointerButton::Primary),
                None,
                Some(PointerButton::Primary)
            ]
        );
    }

    // 右ボタンは別のメソッドへ届くが、同じ通知にボタンを付けて寄せる。
    seen.borrow_mut().clear();
    send(NSEventType::RightMouseDown, 10.0, 10.0);
    send(NSEventType::RightMouseUp, 10.0, 10.0);

    // 左右以外は `otherMouseDown:` へまとめて届くので、ボタンの番号で見分ける。
    // `mouseEventWithType` では番号を決められないので CGEvent から作る
    // (位置はスクリーン座標になるので、ここでは見ない)。
    let other = |kind: CGEventType, number: u32| {
        let cg =
            CGEvent::new_mouse_event(None, kind, NSPoint::new(0.0, 0.0), CGMouseButton(number))
                .expect("マウスの CGEvent");
        let event = NSEvent::eventWithCGEvent(&cg).expect("マウスの NSEvent");
        if kind == CGEventType::OtherMouseDown {
            view.otherMouseDown(&event);
        } else {
            view.otherMouseUp(&event);
        }
    };
    other(CGEventType::OtherMouseDown, 2);
    other(CGEventType::OtherMouseUp, 2);
    other(CGEventType::OtherMouseDown, 3);
    other(CGEventType::OtherMouseUp, 3);
    other(CGEventType::OtherMouseDown, 4);
    let events: Vec<_> = seen.borrow().iter().map(|e| (e.phase, e.button)).collect();
    assert_eq!(
        events,
        vec![
            (PointerPhase::Down, Some(PointerButton::Secondary)),
            (PointerPhase::Up, Some(PointerButton::Secondary)),
            (PointerPhase::Down, Some(PointerButton::Middle)),
            (PointerPhase::Up, Some(PointerButton::Middle)),
            (PointerPhase::Down, Some(PointerButton::Back)),
            (PointerPhase::Up, Some(PointerButton::Back)),
            (PointerPhase::Down, Some(PointerButton::Forward)),
        ],
        "右・中・側面のボタンも見分けて届くこと"
    );
    window.close();
    Ok(())
}

define_class!(
    /// A responder that counts how many times `scrollWheel:` arrived. Inserted as the
    /// drawing surface's next responder to see whether a `Continue` wheel was passed
    /// outward.
    #[unsafe(super(NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "NauiTestWheelSink"]
    #[ivars = Cell<usize>]
    struct WheelSink;

    unsafe impl NSObjectProtocol for WheelSink {}

    impl WheelSink {
        #[unsafe(method(scrollWheel:))]
        fn scroll_wheel(&self, _event: &NSEvent) {
            self.ivars().set(self.ivars().get() + 1);
        }
    }
);

/// The wheel arrives normalized to logical pixels; with `Continue` it is passed to
/// the next responder (usually the outer `NSScrollView`), and with `Handled` it is
/// not.
///
/// Whether the outer `NSScrollView` actually scrolls with synthesized events
/// depends on the environment (it moved locally but not on the CI runner), so
/// whether it was passed on is observed with a recording responder.
fn canvas_reports_wheel(ui: &Ui) -> Result<()> {
    use objc2_core_graphics::{CGEvent, CGScrollEventUnit};

    let canvas = ui.canvas()?;
    canvas.set_sizing(
        Sizing::new()
            .width(Length::Fixed(100.0))
            .height(Length::Fixed(60.0)),
    );
    let seen = Rc::new(RefCell::new(Vec::new()));
    let response = Rc::new(Cell::new(EventResponse::Continue));
    canvas.on_wheel({
        let seen = seen.clone();
        let response = response.clone();
        move |event| {
            seen.borrow_mut().push(event);
            response.get()
        }
    });
    let window = ui.window("wheel", 200.0, 100.0)?;
    window.set_child(&canvas);
    window.show();
    let view = canvas.native_view();

    // 次のレスポンダーは弱参照なので、テストの終わりまで持っておき、戻してから捨てる。
    let mtm = MainThreadMarker::new().expect("メインスレッド");
    let sink: Retained<WheelSink> =
        unsafe { msg_send![super(WheelSink::alloc(mtm).set_ivars(Cell::new(0))), init] };
    // 弱参照の付け替えなので unsafe。`sink` も元の値もこの関数の間は生きている。
    let original = unsafe { view.nextResponder() };
    unsafe { view.setNextResponder(Some(&sink)) };

    // 本物のホイールの NSEvent は CGEvent からしか作れない。正の量は上へ
    // (中身が下へ動く向き) なので、負の量で下へ読み進める。
    let wheel = |unit: CGScrollEventUnit, amount: i32| {
        let cg = CGEvent::new_scroll_wheel_event2(None, unit, 1, amount, 0, 0)
            .expect("ホイールの CGEvent");
        let event = NSEvent::eventWithCGEvent(&cg).expect("ホイールの NSEvent");
        view.scrollWheel(&event);
    };

    // 細かく測れる量 (ピクセル) はそのまま、向きは「下へ読み進めると正」。
    wheel(CGScrollEventUnit::Pixel, -30);
    {
        let seen = seen.borrow();
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].phase, PointerPhase::Wheel);
        assert_eq!(seen[0].button, None);
        assert_eq!(
            (seen[0].wheel_x, seen[0].wheel_y),
            (0.0, 30.0),
            "{:?}",
            seen[0]
        );
    }
    assert_eq!(
        sink.ivars().get(),
        1,
        "Continue なら次のレスポンダーへ回すこと"
    );

    // 刻みのあるホイールの 1 刻み (行) は WHEEL_STEP に直す。
    response.set(EventResponse::Handled);
    wheel(CGScrollEventUnit::Line, -1);
    {
        let seen = seen.borrow();
        assert_eq!(seen.len(), 2);
        assert_eq!(
            (seen[1].wheel_x, seen[1].wheel_y),
            (0.0, naui_core::WHEEL_STEP),
            "{:?}",
            seen[1]
        );
    }
    assert_eq!(
        sink.ivars().get(),
        1,
        "Handled なら次のレスポンダーへは回さないこと"
    );

    unsafe { view.setNextResponder(original.as_deref()) };
    window.close();
    Ok(())
}

/// The text of labels and text inputs is reflected to the native side.
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

/// Multi-line input keeps text including newlines as is.
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

/// A notification arrives on every keystroke, and the placeholder appears and
/// disappears depending on whether there is content.
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

/// Clicking on the placeholder still hit-tests through to the NSTextView below.
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

/// The height follows `set_sizing`, and the wrapping width follows the parent.
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

/// Takes out the placeholder label overlaid on multi-line input.
fn placeholder_label(text_view: &NSTextView) -> Option<Retained<NSTextField>> {
    let view: &NSView = text_view.as_ref();
    let subviews = view.subviews();
    (0..subviews.len())
        .map(|index| subviews.objectAtIndex(index))
        .find_map(|subview| subview.downcast::<NSTextField>().ok())
}

/// Inserts text through the same path as an IME or a physical keyboard.
fn type_into(text_view: &NSTextView, text: &str) {
    let string = NSString::from_str(text);
    unsafe {
        text_view.insertText_replacementRange(&string, NSRange::new(NSNotFound as usize, 0));
    }
}

/// A slider is clamped to its range.
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

/// A progress bar stays within 0.0..=1.0.
fn progress_clamp(ui: &Ui) -> Result<()> {
    let bar = ui.progress_bar()?;
    bar.set_value(0.25);
    assert!((bar.value() - 0.25).abs() < 1e-9);
    bar.set_value(5.0);
    assert!((bar.value() - 1.0).abs() < 1e-9);
    Ok(())
}

/// Items, the unselected state and whether notifications happen match the
/// NSPopUpButton.
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

/// Text, suggestions and whether notifications happen match the NSComboBox.
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
    combo.set_text("神戸");
    assert_eq!(combo.text(), "神戸", "選択を外しても書いた文字列は残る");
    assert_eq!(native.indexOfSelectedItem(), -1, "一覧側の選択も外れる");

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

/// Even in the middle of a notification, the same field can be operated on and the
/// callback replaced.
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

/// `NSComboBox` does not narrow its list, so apps that want to narrow it swap the
/// suggestions inside `on_change`. Checks that this procedure described in the
/// guide actually works.
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

/// Even in the middle of a notification, the same combo box can be operated on and
/// the callback replaced.
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

/// Items, the unselected state and whether notifications happen match radio-type
/// NSButtons.
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

/// Even through AppKit's target/action path, exactly one is on.
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

/// Even in the middle of a notification, the same radio group can be operated on and
/// the callback replaced.
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

/// Children added to a stack keep their callbacks alive even after the handles are
/// dropped. A `Stack` can insert, remove and clear children later.
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

/// Takes out the text of the labels laid out in an `NSStackView`, in order.
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

/// A `Grid` can remove children by cell.
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

/// Removing a merged cell (a child with a span) leaves the merge itself in place.
///
/// `NSGridView` has no API to unmerge, and rows or columns cannot be removed either
/// (AppKit throws an exception when removing rows or columns crossing a merge). So
/// naui decides how it looks by **treating a merged range as "one cell" all the
/// way**.
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

/// Whether two positions point at the same `NSGridCell` (= are merged).
fn shares_cell(grid: &naui_macos::Grid, left: (isize, isize), right: (isize, isize)) -> bool {
    let native: Retained<objc2_app_kit::NSGridView> = grid
        .native_view()
        .downcast()
        .expect("NSGridView であること");
    let first = native.cellAtColumnIndex_rowIndex(left.0, left.1);
    let second = native.cellAtColumnIndex_rowIndex(right.0, right.1);
    Retained::as_ptr(&first) == Retained::as_ptr(&second)
}

/// Tabs can be removed later. Reselection is not notified.
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

/// The nav bar round-trips with the NSSegmentedControl's selection and notifies
/// selections.
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

/// The dock holds items, and disabled items can be laid out too.
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

/// Tabs hold their content widgets and switch on selection.
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

/// In a menu, only the selected button is pressed in.
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

/// Breadcrumbs treat the last item as the current position, and notify the position
/// on click.
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

/// Pagination stops at the ends.
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

/// A link calls its closure on a native click.
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

/// The theme follows the system by default and can be switched to a fixed theme at
/// runtime.
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

/// The preferred languages are `NSLocale.preferredLanguages`, and a change reported
/// by `NSCurrentLocaleDidChangeNotification` reaches the closure, even when posted
/// on another thread. naui's own strings stay as chosen at startup.
fn preferred_languages_follow_the_os(ui: &Ui) -> Result<()> {
    use objc2_foundation::{NSCurrentLocaleDidChangeNotification, NSLocale, NSNotificationCenter};

    fn post() {
        unsafe {
            NSNotificationCenter::defaultCenter()
                .postNotificationName_object(NSCurrentLocaleDidChangeNotification, None);
        }
    }

    let os: Vec<String> = NSLocale::preferredLanguages()
        .iter()
        .map(|tag| tag.to_string())
        .collect();
    assert_eq!(ui.preferred_languages(), os);
    let strings = naui_core::ui_strings();

    let seen = Rc::new(RefCell::new(Vec::<Vec<String>>::new()));
    let sink = seen.clone();
    ui.on_preferred_languages_change(move |tags| sink.borrow_mut().push(tags.to_vec()));

    // 地域だけが変わったときと同じく、言語が変わらなければ呼ばれない。
    post();
    pump(0.2);
    assert!(seen.borrow().is_empty(), "{:?}", seen.borrow());

    let changed = vec!["fr-FR".to_string(), "ja-JP".to_string()];
    naui_macos::override_preferred_languages_for_test(Some(changed.clone()));
    post();
    pump_until(2.0, || !seen.borrow().is_empty());
    assert_eq!(seen.borrow().as_slice(), std::slice::from_ref(&changed));
    assert_eq!(ui.preferred_languages(), changed);
    assert!(
        std::ptr::eq(naui_core::ui_strings(), strings),
        "naui の文字列は起動時の言語のまま"
    );

    // 通知は別のスレッドで来ることもある。
    let other = vec!["de-DE".to_string()];
    naui_macos::override_preferred_languages_for_test(Some(other.clone()));
    std::thread::spawn(post).join().unwrap();
    pump_until(2.0, || seen.borrow().len() == 2);
    assert_eq!(seen.borrow().last(), Some(&other));

    naui_macos::override_preferred_languages_for_test(None);
    post();
    pump_until(2.0, || seen.borrow().len() == 3);
    assert_eq!(seen.borrow().last(), Some(&os));
    assert_eq!(ui.preferred_languages(), os);
    Ok(())
}

/// A panic in the closure stops at the main queue's boundary instead of aborting
/// the process, and the languages are still taken.
fn preferred_languages_survive_a_panicking_closure(ui: &Ui) -> Result<()> {
    use objc2_foundation::{NSCurrentLocaleDidChangeNotification, NSNotificationCenter};

    let called = Rc::new(Cell::new(false));
    let sink = called.clone();
    ui.on_preferred_languages_change(move |_| {
        sink.set(true);
        panic!("知らせの中の panic (テストで意図したもの)");
    });
    let changed = vec!["it-IT".to_string()];
    naui_macos::override_preferred_languages_for_test(Some(changed.clone()));
    unsafe {
        NSNotificationCenter::defaultCenter()
            .postNotificationName_object(NSCurrentLocaleDidChangeNotification, None);
    }
    pump_until(2.0, || called.get());
    naui_macos::override_preferred_languages_for_test(None);
    // ここへ戻ってこられれば、プロセスは落ちていない。
    assert!(called.get(), "クロージャが呼ばれること");
    assert_eq!(ui.preferred_languages(), changed);
    Ok(())
}

/// A window goes through creation, configuration and closing (without double
/// frees).
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

/// Size specifications become AppKit constraints, values included.
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

/// Specifying again does not pile up constraints, nor break AppKit's internal
/// constraints.
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

/// In a vertical stack, a child with width Fill stretches to the parent's width
/// minus padding.
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

/// Cross-axis `Fill` works even when `set_sizing` is called after appending.
///
/// A child whose width comes from its content (a button) grows through hugging alone,
/// but a nested `Stack` or a `Scroll` grows only through the cross-axis constraint,
/// which used to be placed only at `append`.
fn stack_fill_cross_after_append(ui: &Ui) -> Result<()> {
    let stack = ui.stack(Orientation::Vertical)?;
    stack.set_padding(Padding::all(10.0));
    let inner = ui.stack(Orientation::Vertical)?;
    inner.append(&ui.label("あとから広げる")?);
    let scroll = ui.scroll()?;
    scroll.set_child(&ui.label("中身")?);
    scroll.set_sizing(Sizing::new().height(Length::Fixed(50.0)));
    stack.append(&inner);
    stack.append(&scroll);

    let root = stack.native_view();
    root.setFrameSize(NSSize::new(300.0, 200.0));
    root.layoutSubtreeIfNeeded();
    let before = inner.native_view().frame().size.width;
    assert!(before < 280.0, "指定前は中身の幅であること: {before}");

    inner.set_sizing(Sizing::fill_width());
    scroll.set_sizing(Sizing::fill_width().height(Length::Fixed(50.0)));
    root.layoutSubtreeIfNeeded();
    for (name, width) in [
        ("Stack", inner.native_view().frame().size.width),
        ("Scroll", scroll.native_view().frame().size.width),
    ] {
        assert!(
            (width - 280.0).abs() < 1e-6,
            "あとから指定しても {name} が左右の余白を除いた幅になること: {width}"
        );
    }

    // `Fill` をやめれば中身の幅へ戻る。
    inner.set_sizing(Sizing::AUTO);
    root.layoutSubtreeIfNeeded();
    let restored = inner.native_view().frame().size.width;
    assert!(
        (restored - before).abs() < 1e-6,
        "指定を戻せば中身の幅へ戻ること: {before} / {restored}"
    );
    Ok(())
}

/// Main-axis `Fill` takes the leftover space even when `set_sizing` is called after
/// appending.
fn stack_fill_main_after_append(ui: &Ui) -> Result<()> {
    let stack = ui.stack(Orientation::Horizontal)?;
    stack.set_spacing(12.0);
    let fill = ui.button("伸びる")?;
    let fixed = ui.button("固定")?;
    stack.append(&fill);
    stack.append(&fixed);

    let root = stack.native_view();
    root.setFrameSize(NSSize::new(500.0, 60.0));
    root.layoutSubtreeIfNeeded();
    let before = fill.native_view().frame().size.width;

    fill.set_sizing(Sizing::fill_width());
    root.layoutSubtreeIfNeeded();
    let fill_frame = fill.native_view().frame();
    let fixed_frame = fixed.native_view().frame();
    assert!(
        fill_frame.size.width > fixed_frame.size.width + 150.0,
        "あとから指定した主軸の Fill が余白を受け取ること: {fill_frame:?} / {fixed_frame:?}"
    );

    fill.set_sizing(Sizing::AUTO);
    root.layoutSubtreeIfNeeded();
    let restored = fill.native_view().frame().size.width;
    assert!(
        (restored - before).abs() < 1e-6,
        "指定を戻せば余白を受け取らなくなること: {before} / {restored}"
    );
    Ok(())
}

/// A child grows within its grid cell even when `set_sizing` is called after
/// attaching.
fn grid_fill_after_attach(ui: &Ui) -> Result<()> {
    let grid = ui.grid()?;
    grid.set_column_track(0, Track::FILL);
    let inner = ui.stack(Orientation::Vertical)?;
    inner.append(&ui.label("あとから広げる")?);
    grid.attach(&inner, GridCell::new(0, 0));

    let root = grid.native_view();
    root.setFrameSize(NSSize::new(400.0, 60.0));
    root.layoutSubtreeIfNeeded();
    let before = inner.native_view().frame().size.width;
    assert!(before < 300.0, "指定前は中身の幅であること: {before}");

    inner.set_sizing(Sizing::fill_width());
    root.layoutSubtreeIfNeeded();
    let filled = inner.native_view().frame().size.width;
    assert!(
        (filled - 400.0).abs() < 1e-6,
        "あとから指定してもマスいっぱいに広がること: {filled}"
    );

    inner.set_sizing(Sizing::AUTO);
    root.layoutSubtreeIfNeeded();
    let restored = inner.native_view().frame().size.width;
    assert!(
        (restored - before).abs() < 1e-6,
        "指定を戻せば中身の幅へ戻ること: {before} / {restored}"
    );
    Ok(())
}

/// Calling `set_spacing` after appending changes the gaps between the children
/// already there.
fn stack_spacing_after_append(ui: &Ui) -> Result<()> {
    let stack = ui.stack(Orientation::Vertical)?;
    let first = ui.label("1")?;
    let second = ui.label("2")?;
    let third = ui.label("3")?;
    stack.append(&first);
    stack.append(&second);
    stack.append(&third);
    stack.set_spacing(20.0);

    let root = stack.native_view();
    root.setFrameSize(NSSize::new(200.0, 200.0));
    root.layoutSubtreeIfNeeded();
    let gap = |upper: &naui_macos::Label, lower: &naui_macos::Label| {
        let a = upper.native_view().frame();
        let b = lower.native_view().frame();
        // 反転していない座標系でも反転した座標系でも、間の長さを取る。
        let (top, bottom) = if a.origin.y < b.origin.y {
            (a, b)
        } else {
            (b, a)
        };
        bottom.origin.y - (top.origin.y + top.size.height)
    };
    for (name, value) in [
        ("1 と 2", gap(&first, &second)),
        ("2 と 3", gap(&second, &third)),
    ] {
        assert!(
            (value - 20.0).abs() < 1e-6,
            "{name} の間が指定した間隔になること: {value}"
        );
    }
    Ok(())
}

/// Inserting into or removing from a `Stack` leaves the other children in the view
/// hierarchy. Taking them out would cost a field being typed into its focus and its
/// IME composition.
fn stack_insert_keeps_focus(ui: &Ui) -> Result<()> {
    let window = ui.window("フォーカスの保持", 320.0, 160.0)?;
    let root = ui.stack(Orientation::Vertical)?;
    root.set_sizing(Sizing::fill());
    let field = ui.text_input("")?;
    field.set_sizing(Sizing::fill_width());
    root.append(&field);
    window.set_child(&root);
    window.show();

    let field_view: Retained<NSTextField> = field
        .native_view()
        .downcast()
        .expect("NSTextField であること");
    assert!(field.request_focus());
    let editor = field_view
        .currentEditor()
        .and_then(|editor| editor.downcast::<NSTextView>().ok())
        .expect("入力欄が編集中になること");
    // IME が変換中の文字 (marked text) を置いた状態にする。
    unsafe {
        editor.setMarkedText_selectedRange_replacementRange(
            &NSString::from_str("かな"),
            NSRange::new(2, 0),
            NSRange::new(NSNotFound as usize, 0),
        )
    };
    assert!(unsafe { editor.hasMarkedText() });

    root.insert(0, &ui.label("見出し")?);
    assert!(
        field_view.currentEditor().is_some(),
        "前へ差し込んでも入力欄のフォーカスが外れないこと"
    );
    root.append(&ui.label("末尾")?);
    root.remove(0);
    assert!(
        field_view.currentEditor().is_some(),
        "ほかの子を外しても入力欄のフォーカスが外れないこと"
    );
    assert!(
        unsafe { editor.hasMarkedText() },
        "ほかの子を出し入れしても変換中の文字が残ること"
    );

    root.set_orientation(Orientation::Horizontal);
    root.native_view().layoutSubtreeIfNeeded();
    assert!(
        field_view.currentEditor().is_some(),
        "向きを変えても入力欄のフォーカスが外れないこと"
    );
    assert!(
        unsafe { editor.hasMarkedText() },
        "向きを変えても変換中の文字が残ること"
    );
    window.close();
    Ok(())
}

/// Switching a `Stack` between vertical and horizontal gives the layout of one made
/// that way: `Fill` on the width stretches a child across a vertical stack and hands
/// it the leftover width of a horizontal one, and the alignment applies to the new
/// cross axis. Switching back restores every frame.
fn stack_switches_orientation(ui: &Ui) -> Result<()> {
    let stack = ui.stack(Orientation::Vertical)?;
    stack.set_padding(Padding::all(10.0));
    stack.set_spacing(8.0);
    stack.set_align(Align::Start);
    let first = ui.button("前")?;
    let wide = ui.button("広がる")?;
    wide.set_sizing(Sizing::fill_width());
    let last = ui.button("後")?;
    stack.append(&first);
    stack.append(&wide);
    stack.append(&last);

    let root = stack.native_view();
    root.setFrameSize(NSSize::new(400.0, 200.0));
    root.layoutSubtreeIfNeeded();
    let frames = || {
        [
            &first.native_view(),
            &wide.native_view(),
            &last.native_view(),
        ]
        .map(|view| view.frame())
    };
    let vertical = frames();
    assert!(
        (vertical[1].size.width - 380.0).abs() < 1e-6,
        "縦並びでは幅の Fill が左右の余白を除いた幅になること: {:?}",
        vertical[1]
    );

    stack.set_orientation(Orientation::Horizontal);
    assert_eq!(stack.orientation(), Orientation::Horizontal);
    root.layoutSubtreeIfNeeded();
    let [a, b, c] = frames();
    for (name, left, right) in [("前と広がる", a, b), ("広がると後", b, c)] {
        let gap = right.origin.x - (left.origin.x + left.size.width);
        assert!(
            (gap - 8.0).abs() < 1e-6,
            "{name} が間隔を空けて横に並ぶこと: {left:?} / {right:?}"
        );
    }
    assert!(
        (a.origin.x - 10.0).abs() < 1e-6 && (c.origin.x + c.size.width - 390.0).abs() < 1e-6,
        "幅の Fill が横の余りを受け取って端まで埋めること: {a:?} / {c:?}"
    );
    assert!(
        (a.size.width - vertical[0].size.width).abs() < 1e-6,
        "Auto の子は中身の幅のままであること: {a:?}"
    );
    // `Align::Start` は横並びでは上端。
    let top = |frame: NSRect| {
        if root.isFlipped() {
            frame.origin.y
        } else {
            200.0 - (frame.origin.y + frame.size.height)
        }
    };
    for frame in [a, b, c] {
        assert!(
            (top(frame) - 10.0).abs() < 1e-6,
            "揃えが新しい交差軸 (縦) に効くこと: {frame:?}"
        );
    }

    stack.set_orientation(Orientation::Vertical);
    root.layoutSubtreeIfNeeded();
    let restored = frames();
    for (before, after) in vertical.iter().zip(restored.iter()) {
        assert!(
            (before.origin.x - after.origin.x).abs() < 1e-6
                && (before.origin.y - after.origin.y).abs() < 1e-6
                && (before.size.width - after.size.width).abs() < 1e-6
                && (before.size.height - after.size.height).abs() < 1e-6,
            "縦へ戻すと元の位置と大きさへ戻ること: {before:?} / {after:?}"
        );
    }
    Ok(())
}

/// A child taken out of one `Stack` and appended to another follows the new parent:
/// cross-axis `Fill` stretches it to the new stack, and later `set_sizing` calls reach
/// the new stack rather than the old one.
fn moved_child_follows_the_new_stack(ui: &Ui) -> Result<()> {
    let first = ui.stack(Orientation::Vertical)?;
    let second = ui.stack(Orientation::Vertical)?;
    second.set_padding(Padding::all(20.0));
    let inner = ui.stack(Orientation::Vertical)?;
    inner.append(&ui.label("移る")?);
    inner.set_sizing(Sizing::fill_width());
    first.append(&inner);

    let first_root = first.native_view();
    first_root.setFrameSize(NSSize::new(300.0, 100.0));
    first_root.layoutSubtreeIfNeeded();
    assert!(
        (inner.native_view().frame().size.width - 300.0).abs() < 1e-6,
        "移す前は前の親の幅になること"
    );

    first.remove(0);
    second.append(&inner);
    let second_root = second.native_view();
    second_root.setFrameSize(NSSize::new(200.0, 100.0));
    second_root.layoutSubtreeIfNeeded();
    let moved = inner.native_view().frame().size.width;
    assert!(
        (moved - 160.0).abs() < 1e-6,
        "移した先の幅から左右の余白を除いた幅になること: {moved}"
    );
    assert!(first.is_empty(), "前の親には残らないこと");

    inner.set_sizing(Sizing::AUTO);
    second_root.layoutSubtreeIfNeeded();
    let restored = inner.native_view().frame().size.width;
    assert!(
        restored < 150.0,
        "移したあとの set_sizing が新しい親に効くこと: {restored}"
    );
    Ok(())
}

/// What a widget moved to another container keeps: a `Scroll` keeps its position,
/// while a field loses the focus, and AppKit commits the text being composed when it
/// does (the documented behavior; the other platforms differ only in what happens to
/// the composition).
fn moved_widget_keeps_scroll_but_not_focus(ui: &Ui) -> Result<()> {
    let window = ui.window("移動", 320.0, 300.0)?;
    let root = ui.stack(Orientation::Vertical)?;
    root.set_sizing(Sizing::fill());
    let first = ui.stack(Orientation::Vertical)?;
    let second = ui.stack(Orientation::Vertical)?;
    first.set_sizing(Sizing::fill_width());
    second.set_sizing(Sizing::fill_width());
    root.append(&first);
    root.append(&second);
    let field = ui.text_input("")?;
    field.set_sizing(Sizing::fill_width());
    first.append(&field);
    let scroll = ui.scroll()?;
    scroll.set_sizing(Sizing::fill_width().height(Length::Fixed(80.0)));
    let tall = ui.stack(Orientation::Vertical)?;
    for index in 0..30 {
        tall.append(&ui.label(&format!("行 {index}"))?);
    }
    scroll.set_child(&tall);
    first.append(&scroll);
    window.set_child(&root);
    window.show();
    root.native_view().layoutSubtreeIfNeeded();

    assert!(field.request_focus());
    let field_view: Retained<NSTextField> = field
        .native_view()
        .downcast()
        .expect("NSTextField であること");
    let editor = field_view
        .currentEditor()
        .and_then(|editor| editor.downcast::<NSTextView>().ok())
        .expect("入力欄が編集中になること");
    unsafe {
        editor.setMarkedText_selectedRange_replacementRange(
            &NSString::from_str("かな"),
            NSRange::new(2, 0),
            NSRange::new(NSNotFound as usize, 0),
        )
    };
    scroll.scroll_to(0.0, 200.0);
    assert_eq!(scroll.metrics().y, 200.0);

    first.remove(1);
    first.remove(0);
    second.append(&field);
    second.append(&scroll);
    root.native_view().layoutSubtreeIfNeeded();
    assert_eq!(scroll.metrics().y, 200.0, "スクロール位置が残ること");
    assert!(
        field_view.currentEditor().is_none(),
        "移した入力欄からはフォーカスが外れること"
    );
    assert_eq!(field.text(), "かな", "変換中の文字は確定して残ること");
    window.close();
    Ok(())
}

/// An `Auto` child in a horizontal stack does not take the leftover main-axis space
/// on its own.
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

/// In a horizontal stack, a child with width `Fill` explicitly takes the leftover.
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

/// A maximum on cross-axis `Fill` does not clash with the parent's width.
///
/// Cross-axis `Fill` means "match the parent's width", but a maximum takes
/// precedence. If required constraints clashed, AppKit would arbitrarily drop one.
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

/// `Fill` with a maximum grows up to the maximum when there is room, and is smaller
/// when it is tight.
///
/// To mean the same as CSS and WinUI's `Stretch`, the maximum is also treated as
/// "the size you want under normal conditions". Horizontal and vertical are treated
/// the same, and `Sizing` alone is enough to specify it.
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

/// A spacer takes all the vertical leftover and pushes the children after it to the
/// bottom.
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

/// A grid adds the rows and columns it needs and holds its children.
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

/// An `Auto` child placed in a `Track::Fill` column keeps its content width even
/// after relayout.
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

/// Shrinks and grows the height in the same Stack -> Tabs -> Stack -> Grid hierarchy
/// as the Gallery.
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

/// Replacing in a grid leaves no previous child in the parent.
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

/// An expander expands or collapses each time the heading is clicked and notifies
/// the new state.
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

/// While collapsed, the content leaves the layout and takes no space.
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

/// Even without a setting, the content is placed across the expander's full width
/// (the same as `Scroll`).
///
/// If the content's width were decided by its own content, aligning it left or right
/// inside would look shifted (it would not look the same on the four platforms).
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

/// A `Label` is one line by default and wraps only with `set_wrap(true)`.
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

/// `set_role` maps to `NSButton`'s properties. The role changes only the look, so
/// the Return key equivalent (which makes the window's default button) stays empty.
fn button_role_maps_to_appkit(ui: &Ui) -> Result<()> {
    let button = ui.button("保存")?;
    let native: Retained<NSButton> = button
        .native_view()
        .downcast()
        .expect("ボタンは NSButton であること");
    // `tintProminence` は macOS 26 から。CI の macOS 15 では代わりに `bezelColor` を見る。
    let has_prominence = native.respondsToSelector(sel!(setTintProminence:));
    let assert_normal = |when: &str| {
        assert!(!native.hasDestructiveAction(), "{when}: 破壊的でないこと");
        assert!(native.bezelColor().is_none(), "{when}: 地の色が無いこと");
        if has_prominence {
            assert_eq!(
                native.tintProminence(),
                NSTintProminence::Automatic,
                "{when}: 強調が既定であること"
            );
        }
    };
    assert_normal("作った直後");

    button.set_role(ButtonRole::Primary);
    assert!(!native.hasDestructiveAction(), "主ボタンは破壊的でないこと");
    if has_prominence {
        assert_eq!(native.tintProminence(), NSTintProminence::Primary);
    } else {
        assert_eq!(
            native.bezelColor().as_deref(),
            Some(&*NSColor::controlAccentColor()),
            "macOS 26 より前はアクセント色の地にすること"
        );
    }
    assert_eq!(
        native.keyEquivalent().to_string(),
        "",
        "Return のキー割り当ては付けないこと"
    );

    button.set_role(ButtonRole::Destructive);
    assert!(native.hasDestructiveAction(), "破壊的な操作であること");
    if has_prominence {
        assert_eq!(native.tintProminence(), NSTintProminence::Secondary);
        assert_eq!(
            native.bezelColor().as_deref(),
            Some(&*NSColor::systemRedColor()),
            "警告パネルと同じ赤の地にすること"
        );
    } else {
        assert!(
            native.bezelColor().is_none(),
            "主ボタンの地の色が残らないこと"
        );
    }

    button.set_role(ButtonRole::Normal);
    assert_normal("既定へ戻した後");
    Ok(())
}

/// `set_style` / `set_color` map to AppKit fonts and colors. Checks with the real
/// fonts and `NSColor`s that naui holds no point sizes or color values itself.
fn label_style_and_color_map_to_appkit(ui: &Ui) -> Result<()> {
    let label = ui.label("見出し")?;
    let native: Retained<NSTextField> = label
        .native_view()
        .downcast()
        .expect("ラベルは NSTextField であること");
    let font = || native.font().expect("フォントが決まっていること");

    // 大きい段階ほど級数も大きい。値は AppKit が決めるので、順序だけを見る。
    let ramp = [
        TextStyle::Caption,
        TextStyle::Body,
        TextStyle::Subtitle,
        TextStyle::Title,
        TextStyle::LargeTitle,
    ];
    let mut sizes: Vec<(TextStyle, f64)> = Vec::new();
    for style in ramp {
        label.set_style(style);
        sizes.push((style, font().pointSize()));
    }
    for pair in sizes.windows(2) {
        assert!(
            pair[1].1 > pair[0].1,
            "段階が上がるほど大きくなること: {:?} {} / {:?} {}",
            pair[0].0,
            pair[0].1,
            pair[1].0,
            pair[1].1
        );
    }

    // `Heading` は本文と同じ級数で、太さだけが違う。
    label.set_style(TextStyle::Body);
    let body = font();
    label.set_style(TextStyle::Heading);
    let heading = font();
    assert!(
        (heading.pointSize() - body.pointSize()).abs() < 0.5,
        "見出しは本文と同じ級数であること: {} / {}",
        body.pointSize(),
        heading.pointSize()
    );
    assert_ne!(
        heading.fontName().to_string(),
        body.fontName().to_string(),
        "見出しは本文と違う太さであること"
    );

    // 色は役割ごとに違う NSColor が来る。どれも AppKit が持つ色そのもの。
    label.set_color(TextColor::Default);
    assert_eq!(
        native.textColor().expect("色が決まっていること"),
        NSColor::labelColor(),
        "既定は labelColor であること"
    );
    label.set_color(TextColor::Secondary);
    assert_eq!(
        native.textColor().expect("色が決まっていること"),
        NSColor::secondaryLabelColor(),
        "補足は secondaryLabelColor であること"
    );
    label.set_color(TextColor::Danger);
    assert_eq!(
        native.textColor().expect("色が決まっていること"),
        NSColor::systemRedColor(),
        "危険は systemRedColor であること"
    );

    // 役割ごとに違う色へ落ちる (どれか 2 つが同じ色だと区別が付かない)。
    let mut seen: Vec<Retained<NSColor>> = Vec::new();
    for color in TextColor::ALL {
        label.set_color(color);
        let current = native.textColor().expect("色が決まっていること");
        assert!(
            !seen.contains(&current),
            "{color:?} が別の役割と同じ色になっている"
        );
        seen.push(current);
    }
    Ok(())
}

/// The split view is an NSSplitView itself, laying out two panes.
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

/// The divider is placed at the specified position, and the end side takes the
/// extra space when it grows.
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

/// The divider moves only within the minimum sizes, and a notification arrives when
/// it moves.
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

/// Puts a scroll view holding tall content into a window 100 high.
fn tall_scroll(
    ui: &Ui,
    rows: usize,
) -> Result<(naui_macos::Window, naui_macos::Scroll, naui_macos::Stack)> {
    let window = ui.window("t", 200.0, 100.0)?;
    let pane = ui.stack(Orientation::Vertical)?;
    for i in 0..rows {
        pane.append(&ui.label(&format!("行 {i}"))?);
    }
    let scroll = ui.scroll()?;
    scroll.set_child(&pane);
    scroll.set_sizing(Sizing::fill());
    window.set_child(&scroll);
    Ok((window, scroll, pane))
}

fn scroll_measures_and_moves(ui: &Ui) -> Result<()> {
    let (window, scroll, _pane) = tall_scroll(ui, 40)?;
    window.show();

    let m = scroll.metrics();
    assert_eq!((m.x, m.y), (0.0, 0.0), "最初は先頭: {m:?}");
    assert!(m.viewport_height > 0.0, "{m:?}");
    assert!(
        m.content_height > m.viewport_height,
        "はみ出している: {m:?}"
    );
    assert_eq!(m.distance_to_end(), m.max_y());

    scroll.scroll_to(0.0, 30.0);
    assert_eq!(scroll.metrics().y, 30.0);

    scroll.scroll_to_end();
    let end = scroll.metrics();
    assert_eq!(end.y, end.max_y(), "末尾へ: {end:?}");
    assert_eq!(end.distance_to_end(), 0.0);

    // 範囲の外は丸める。
    scroll.scroll_to(-50.0, 1e9);
    let clamped = scroll.metrics();
    assert_eq!((clamped.x, clamped.y), (0.0, clamped.max_y()));

    // 末尾が見えていることは、ネイティブの側でも確かめる。
    let native = scroll
        .native_view()
        .downcast::<NSScrollView>()
        .expect("NSScrollView");
    assert_eq!(native.contentView().bounds().origin.y, clamped.max_y());
    window.close();
    Ok(())
}

/// Calls "append, then scroll to the end", as in a chat, back to back without
/// waiting for layout.
fn scroll_to_end_after_appending(ui: &Ui) -> Result<()> {
    let (window, scroll, pane) = tall_scroll(ui, 20)?;
    window.show();
    scroll.scroll_to_end();
    let before = scroll.metrics();

    for i in 0..20 {
        pane.append(&ui.label(&format!("追加 {i}"))?);
    }
    scroll.scroll_to_end();
    let after = scroll.metrics();
    assert!(
        after.content_height > before.content_height,
        "中身が伸びている: {before:?} → {after:?}"
    );
    assert_eq!(after.distance_to_end(), 0.0, "{after:?}");
    window.close();
    Ok(())
}

fn scroll_to_end_before_shown(ui: &Ui) -> Result<()> {
    let (window, scroll, _pane) = tall_scroll(ui, 40)?;
    scroll.scroll_to_end();
    window.show();
    window
        .native_window()
        .contentView()
        .unwrap()
        .layoutSubtreeIfNeeded();
    let m = scroll.metrics();
    assert!(m.max_y() > 0.0, "{m:?}");
    assert_eq!(m.distance_to_end(), 0.0, "表示後に末尾にいる: {m:?}");
    window.close();
    Ok(())
}

fn scroll_notifies_only_moves(ui: &Ui) -> Result<()> {
    let (window, scroll, _pane) = tall_scroll(ui, 40)?;
    window.show();
    let seen = Rc::new(RefCell::new(Vec::new()));
    scroll.on_scroll({
        let seen = seen.clone();
        move |m: naui_core::ScrollMetrics| seen.borrow_mut().push(m.y)
    });

    scroll.scroll_to(0.0, 25.0);
    scroll.scroll_to(0.0, 25.0); // 同じ位置へは動かない
    assert_eq!(*seen.borrow(), vec![25.0]);

    // 幅だけ変えても、位置が変わらなければ届かない。
    window.set_size(260.0, 100.0);
    window
        .native_window()
        .contentView()
        .unwrap()
        .layoutSubtreeIfNeeded();
    assert_eq!(*seen.borrow(), vec![25.0]);

    // 利用者の操作 (ネイティブ側での移動) でも届く。
    let native = scroll
        .native_view()
        .downcast::<NSScrollView>()
        .expect("NSScrollView");
    let clip = native.contentView();
    clip.scrollToPoint(NSPoint::new(0.0, 40.0));
    native.reflectScrolledClipView(&clip);
    assert_eq!(*seen.borrow(), vec![25.0, 40.0]);
    window.close();
    Ok(())
}

fn hidden_widget_leaves_the_stack_layout(ui: &Ui) -> Result<()> {
    let stack = ui.stack(Orientation::Vertical)?;
    let first = ui.label("1 行目")?;
    let middle = ui.button("真ん中")?;
    let last = ui.label("3 行目")?;
    stack.append(&first);
    stack.append(&middle);
    stack.append(&last);
    let view = stack.native_view();
    view.layoutSubtreeIfNeeded();
    let shown = view.fittingSize().height;
    assert!(middle.is_visible(), "既定は表示");

    middle.set_visible(false);
    view.layoutSubtreeIfNeeded();
    let hidden = view.fittingSize().height;
    assert!(!middle.is_visible());
    assert!(middle.native_view().isHidden());
    assert!(
        hidden + 10.0 < shown,
        "隠すと場所を空けない: {shown} -> {hidden}"
    );

    middle.set_visible(true);
    view.layoutSubtreeIfNeeded();
    assert!(middle.is_visible());
    assert!(
        (view.fittingSize().height - shown).abs() < 1.0,
        "戻すと元の高さ"
    );
    Ok(())
}

fn expander_keeps_the_app_hidden_child(ui: &Ui) -> Result<()> {
    let expander = ui.expander("詳細")?;
    let body = ui.label("中身")?;
    expander.set_child(&body);

    // たたんでいる間に表示へ戻しても、中身は出ない。
    body.set_visible(false);
    body.set_visible(true);
    assert!(body.is_visible());
    assert!(body.native_view().isHidden(), "たたんでいる間は隠れたまま");

    body.set_visible(false);
    expander.set_expanded(true);
    assert!(!body.is_visible());
    assert!(
        body.native_view().isHidden(),
        "開いてもアプリが隠したものは隠れたまま"
    );

    body.set_visible(true);
    assert!(!body.native_view().isHidden(), "開いていれば出る");
    expander.set_expanded(false);
    assert!(body.native_view().isHidden());
    assert!(body.is_visible(), "アプリの指定は表示のまま");
    Ok(())
}

fn request_focus_reaches_the_editor(ui: &Ui) -> Result<()> {
    let window = ui.window("t", 320.0, 240.0)?;
    let root = ui.stack(Orientation::Vertical)?;
    let label = ui.label("見出し")?;
    let input = ui.text_input("")?;
    let area = ui.text_area("")?;
    area.set_sizing(Sizing::fixed(200.0, 60.0));
    let inner = ui.stack(Orientation::Vertical)?;
    let nested = ui.text_input("")?;
    inner.append(&ui.label("中")?);
    inner.append(&nested);
    root.append(&label);
    root.append(&input);
    root.append(&area);
    root.append(&inner);

    assert!(!input.request_focus(), "ウィンドウに載る前は移せない");
    window.set_child(&root);
    window.show();
    let native = window.native_window();

    assert!(!label.request_focus(), "ラベルは受け取らない");

    assert!(input.request_focus());
    let editor = input
        .native_view()
        .downcast::<NSTextField>()
        .expect("NSTextField")
        .currentEditor();
    assert!(editor.is_some(), "入力欄が編集中になる");

    assert!(area.request_focus());
    let responder = native.firstResponder().expect("first responder");
    assert!(
        responder.downcast::<NSTextView>().is_ok(),
        "複数行入力は中の NSTextView が受け取る"
    );

    assert!(inner.request_focus(), "コンテナは中の最初の入力欄へ");
    assert!(nested
        .native_view()
        .downcast::<NSTextField>()
        .expect("NSTextField")
        .currentEditor()
        .is_some());

    input.set_visible(false);
    assert!(!input.request_focus(), "隠れていると移せない");
    window.close();
    Ok(())
}

fn focus_change_follows_the_first_responder(ui: &Ui) -> Result<()> {
    let window = ui.window("t", 320.0, 300.0)?;
    let root = ui.stack(Orientation::Vertical)?;
    let name = ui.text_input("")?;
    let memo = ui.text_area("")?;
    memo.set_sizing(Sizing::fixed(200.0, 60.0));
    let count = ui.number_input(1.0)?;
    let other = ui.text_input("")?;
    root.append(&name);
    root.append(&memo);
    root.append(&count);
    root.append(&other);
    window.set_child(&root);
    window.show();

    // 通知先を置く前からフォーカスがある欄も、そこから出たことは伝わる。
    assert!(name.request_focus());
    let log = Rc::new(RefCell::new(Vec::new()));
    let record = |label: &'static str| {
        let log = log.clone();
        move |focused: bool| log.borrow_mut().push(format!("{label}:{focused}"))
    };
    name.on_focus_change(record("名前"));
    memo.on_focus_change(record("メモ"));
    count.on_focus_change(record("個数"));
    assert!(log.borrow().is_empty(), "置いただけでは通知しない");

    assert!(memo.request_focus());
    assert!(count.request_focus());
    assert_eq!(
        *log.borrow(),
        ["名前:false", "メモ:true", "メモ:false", "個数:true"]
    );

    // 通知先の無い欄へ移れば、出たことだけが伝わる。
    assert!(other.request_focus());
    assert_eq!(log.borrow()[4..], ["個数:false"]);

    // 同じ欄へ移し直しても、持ち主は変わらない。
    assert!(name.request_focus());
    assert!(name.request_focus());
    assert_eq!(log.borrow()[5..], ["名前:true"]);
    window.close();
    Ok(())
}

fn focus_change_handler_is_freed_with_the_widget(ui: &Ui) -> Result<()> {
    let window = ui.window("フォーカス", 320.0, 200.0)?;
    let root = ui.stack(Orientation::Vertical)?;
    let kept = ui.text_input("")?;
    root.append(&kept);
    window.set_child(&root);
    window.show();

    // 通知先が捕まえている状態。解放されれば数が 1 へ戻る。
    let state = Rc::new(());
    let capture = || {
        let state = state.clone();
        move |_: bool| {
            let _ = &state;
        }
    };

    // 表示中の木へ入れてフォーカスを持たせてから外す。自動解放に回された
    // ぶんはプールを抜けたところで、フォーカスを持っていた欄を AppKit が
    // 掴んでいるぶんは実行ループが回ったところで手放される。
    autoreleasepool(|_| {
        let input = ui.text_input("")?;
        root.append(&input);
        input.on_focus_change(capture());
        // 差し替えても、前の通知先は残らない。
        input.on_focus_change(capture());
        assert!(input.request_focus());
        assert_eq!(Rc::strong_count(&state), 2);
        assert!(kept.request_focus());
        root.remove(1);
        drop(input);
        Ok::<_, naui_core::Error>(())
    })?;
    pump_until(2.0, || Rc::strong_count(&state) == 1);
    assert_eq!(Rc::strong_count(&state), 1, "外して捨てた欄の通知先");

    // 木へ入れないまま捨てる。
    autoreleasepool(|_| {
        let area = ui.text_area("")?;
        area.on_focus_change(capture());
        drop(area);
        Ok::<_, naui_core::Error>(())
    })?;
    assert_eq!(Rc::strong_count(&state), 1, "木へ入れていない欄の通知先");

    window.close();
    Ok(())
}

fn weak_widget_breaks_the_callback_cycle(ui: &Ui) -> Result<()> {
    let window = ui.window("t", 320.0, 200.0)?;
    let root = ui.stack(Orientation::Vertical)?;
    let mail = ui.text_input(" a@example.com ")?;
    let other = ui.text_input("")?;
    root.append(&mail);
    root.append(&other);
    window.set_child(&root);
    window.show();

    // 通知の中で欄自身を触る。
    let weak = mail.downgrade();
    mail.on_focus_change({
        let weak = weak.clone();
        move |focused| {
            if let Some(mail) = weak.upgrade() {
                if !focused {
                    mail.set_text(mail.text().trim());
                }
            }
        }
    });
    assert!(mail.request_focus());
    assert!(other.request_focus());
    assert_eq!(mail.text(), "a@example.com");

    // 画面から外してハンドルを手放せば、通知が弱参照を持っていても解放される。
    root.remove(0);
    drop(mail);
    assert!(weak.upgrade().is_none(), "外して手放した欄が残っている");
    window.close();
    Ok(())
}

fn tooltip_reaches_the_view(ui: &Ui) -> Result<()> {
    let button = ui.button("保存")?;
    button.set_tooltip(Some("変更を保存します"));
    assert_eq!(
        button
            .native_view()
            .toolTip()
            .map(|t| t.to_string())
            .as_deref(),
        Some("変更を保存します")
    );
    button.set_tooltip(None);
    assert!(button.native_view().toolTip().is_none());

    let stack = ui.stack(Orientation::Vertical)?;
    stack.set_tooltip(Some("まとめ"));
    assert_eq!(
        stack
            .native_view()
            .toolTip()
            .map(|t| t.to_string())
            .as_deref(),
        Some("まとめ")
    );
    Ok(())
}

/// Queues one key in the window's queue and advances until it is delivered (going
/// through the local monitor).
fn press_key(
    app: &NSApplication,
    window: &NSWindow,
    characters: &str,
    key_code: u16,
    flags: NSEventModifierFlags,
) {
    let text = NSString::from_str(characters);
    let event = NSEvent::keyEventWithType_location_modifierFlags_timestamp_windowNumber_context_characters_charactersIgnoringModifiers_isARepeat_keyCode(
        NSEventType::KeyDown,
        NSPoint::new(0.0, 0.0),
        flags,
        0.0,
        window.windowNumber(),
        None,
        &text,
        &text,
        false,
        key_code,
    )
    .expect("キーイベント");
    unsafe { app.postEvent_atStart(&event, false) };
    deliver_events(app);
}

fn text_area_key_down_can_take_enter(ui: &Ui) -> Result<()> {
    let mtm = MainThreadMarker::new().expect("メインスレッド");
    let app = NSApplication::sharedApplication(mtm);
    let window = ui.window("キー", 320.0, 200.0)?;
    let area = ui.text_area("")?;
    area.set_sizing(Sizing::fill());
    window.set_child(&area);
    window.show();
    assert!(area.request_focus());

    let seen = Rc::new(RefCell::new(Vec::new()));
    area.on_key_down({
        let seen = seen.clone();
        move |event: &naui_core::KeyEvent| {
            seen.borrow_mut().push((event.key, event.modifiers.shift));
            if event.key == naui_core::Key::Enter && !event.modifiers.shift {
                naui_core::EventResponse::Handled
            } else {
                naui_core::EventResponse::Continue
            }
        }
    });
    let native = window.native_window();
    press_key(&app, &native, "\r", 36, NSEventModifierFlags::empty());
    press_key(&app, &native, "\r", 36, NSEventModifierFlags::Shift);
    press_key(&app, &native, "A", 0, NSEventModifierFlags::Shift);
    press_key(&app, &native, "", 122, NSEventModifierFlags::empty()); // F1
    assert_eq!(
        *seen.borrow(),
        vec![
            (naui_core::Key::Enter, false),
            (naui_core::Key::Enter, true),
            (naui_core::Key::Character('a'), true),
            (naui_core::Key::F(1), false),
        ]
    );
    // 止めた Enter は改行にならず、Continue のキーは入力欄が受け取る。
    assert_eq!(area.text(), "\nA", "Shift+Enter と A だけが入る");
    window.close();
    Ok(())
}

fn key_down_skips_composition(ui: &Ui) -> Result<()> {
    let mtm = MainThreadMarker::new().expect("メインスレッド");
    let app = NSApplication::sharedApplication(mtm);
    let window = ui.window("キー", 320.0, 200.0)?;
    let area = ui.text_area("")?;
    area.set_sizing(Sizing::fill());
    window.set_child(&area);
    window.show();
    assert!(area.request_focus());
    let count = Rc::new(Cell::new(0));
    area.on_key_down({
        let count = count.clone();
        move |_| {
            count.set(count.get() + 1);
            naui_core::EventResponse::Handled
        }
    });
    let native = window.native_window();
    let text_view = native
        .firstResponder()
        .and_then(|r| r.downcast::<NSTextView>().ok())
        .expect("NSTextView");
    // IME が変換中の文字 (marked text) を置いた状態にする。
    let marked = NSString::from_str("かな");
    unsafe {
        text_view.setMarkedText_selectedRange_replacementRange(
            &marked,
            NSRange::new(2, 0),
            NSRange::new(NSNotFound as usize, 0),
        )
    };
    assert!(unsafe { text_view.hasMarkedText() });
    press_key(&app, &native, "\r", 36, NSEventModifierFlags::empty());
    assert_eq!(count.get(), 0, "変換中の Enter は届かない");

    unsafe { text_view.unmarkText() };
    press_key(&app, &native, "\r", 36, NSEventModifierFlags::empty());
    assert_eq!(count.get(), 1, "確定後は届く");
    window.close();
    Ok(())
}

fn window_key_down_gets_what_the_widget_left(ui: &Ui) -> Result<()> {
    let mtm = MainThreadMarker::new().expect("メインスレッド");
    let app = NSApplication::sharedApplication(mtm);
    let window = ui.window("キー", 320.0, 200.0)?;
    let input = ui.text_input("")?;
    window.set_child(&input);
    window.show();
    assert!(input.request_focus());
    input.on_key_down(|event: &naui_core::KeyEvent| {
        if event.key == naui_core::Key::Enter {
            naui_core::EventResponse::Handled
        } else {
            naui_core::EventResponse::Continue
        }
    });
    let seen = Rc::new(RefCell::new(Vec::new()));
    window.on_key_down({
        let seen = seen.clone();
        move |event: &naui_core::KeyEvent| {
            seen.borrow_mut().push(event.key);
            naui_core::EventResponse::Continue
        }
    });
    let native = window.native_window();
    press_key(&app, &native, "x", 7, NSEventModifierFlags::empty());
    press_key(&app, &native, "\r", 36, NSEventModifierFlags::empty());
    press_key(&app, &native, "\u{1b}", 53, NSEventModifierFlags::empty());
    assert_eq!(
        *seen.borrow(),
        vec![naui_core::Key::Character('x'), naui_core::Key::Escape]
    );
    window.close();
    Ok(())
}

fn timers_fire_on_the_main_loop(ui: &Ui) -> Result<()> {
    let tasks = ui.tasks();
    let once = Rc::new(Cell::new(0));
    tasks.after(Duration::from_millis(20), {
        let once = once.clone();
        move || once.set(once.get() + 1)
    });
    let ticks = Rc::new(Cell::new(0));
    let handle: Rc<RefCell<Option<naui_core::Timer>>> = Rc::default();
    let timer = tasks.every(Duration::from_millis(10), {
        let ticks = ticks.clone();
        let handle = handle.clone();
        move || {
            ticks.set(ticks.get() + 1);
            if ticks.get() == 3 {
                if let Some(timer) = handle.borrow().as_ref() {
                    timer.cancel();
                }
            }
        }
    });
    *handle.borrow_mut() = Some(timer.clone());
    let cancelled = Rc::new(Cell::new(false));
    tasks
        .after(Duration::from_millis(10), {
            let cancelled = cancelled.clone();
            move || cancelled.set(true)
        })
        .cancel();
    assert_eq!(once.get(), 0, "その場では呼ばない");
    pump_until(2.0, || once.get() == 1 && !timer.is_active());
    assert_eq!(once.get(), 1);
    assert_eq!(ticks.get(), 3, "3 回目で止めた");
    pump(0.05);
    assert_eq!(ticks.get(), 3, "止めた後は呼ばれない");
    assert!(!cancelled.get(), "cancel したものは呼ばれない");
    Ok(())
}

fn progress_bar_switches_to_indeterminate(ui: &Ui) -> Result<()> {
    let progress = ui.progress_bar()?;
    progress.set_value(0.4);
    progress.set_indeterminate(true);
    assert!(progress.is_indeterminate());
    let native = progress
        .native_view()
        .downcast::<objc2_app_kit::NSProgressIndicator>()
        .expect("NSProgressIndicator");
    assert!(native.isIndeterminate());
    progress.set_value(0.7);
    progress.set_indeterminate(false);
    assert!(!native.isIndeterminate());
    assert_eq!(progress.value(), 0.7, "不確定の間に置いた値が出る");
    Ok(())
}

fn spinner_is_a_spinning_indicator(ui: &Ui) -> Result<()> {
    use objc2_app_kit::{NSProgressIndicator, NSProgressIndicatorStyle};
    let stack = ui.stack(Orientation::Vertical)?;
    let spinner = ui.spinner()?;
    assert!(spinner.is_spinning(), "作った時点で回っている");
    stack.append(&spinner);
    let native = spinner
        .native_view()
        .downcast::<NSProgressIndicator>()
        .expect("NSProgressIndicator");
    assert_eq!(native.style(), NSProgressIndicatorStyle::Spinning);
    assert!(native.isIndeterminate());
    assert!(!native.isDisplayedWhenStopped(), "止めたら描かない");

    let root = stack.native_view();
    root.setFrameSize(NSSize::new(200.0, 200.0));
    root.layoutSubtreeIfNeeded();
    let size = native.frame().size;
    assert!(
        (size.width - 32.0).abs() < 1e-6 && (size.height - 32.0).abs() < 1e-6,
        "既定は 32 x 32 (通常の大きさ): {size:?}"
    );

    spinner.set_spinning(false);
    assert!(!spinner.is_spinning());
    root.layoutSubtreeIfNeeded();
    assert_eq!(native.frame().size, size, "止めても場所は残る");
    spinner.set_spinning(true);
    assert!(spinner.is_spinning());

    spinner.set_sizing(
        Sizing::new()
            .width(naui_core::Length::Fixed(16.0))
            .height(naui_core::Length::Fixed(16.0)),
    );
    root.layoutSubtreeIfNeeded();
    let small = native.frame().size;
    assert!(
        (small.width - 16.0).abs() < 1e-6 && (small.height - 16.0).abs() < 1e-6,
        "固定の大きさに従う: {small:?}"
    );
    Ok(())
}

fn label_can_be_selectable(ui: &Ui) -> Result<()> {
    let label = ui.label("コピーできる文字")?;
    assert!(!label.is_selectable(), "既定は選べない");
    label.set_selectable(true);
    let native = label
        .native_view()
        .downcast::<NSTextField>()
        .expect("NSTextField");
    assert!(native.isSelectable());
    assert!(!native.isEditable(), "選べても書き換えられない");
    Ok(())
}

fn clipboard_round_trips_text(ui: &Ui) -> Result<()> {
    use objc2_app_kit::{NSPasteboard, NSPasteboardTypeString};
    // 利用者のクリップボードを書き換えるので、終わったら戻す。
    let pasteboard = NSPasteboard::generalPasteboard();
    let saved = pasteboard
        .stringForType(unsafe { NSPasteboardTypeString })
        .map(|text| text.to_string());

    let clipboard = ui.clipboard();
    let written = Rc::new(RefCell::new(None));
    clipboard.set_text("naui のテスト ✂", {
        let written = written.clone();
        move |result| *written.borrow_mut() = Some(result.is_ok())
    });
    assert!(
        written.borrow().is_none(),
        "書き込みの結果もその場では呼ばない"
    );
    pump_until(2.0, || written.borrow().is_some());
    assert_eq!(*written.borrow(), Some(true), "書き込めた");
    let read = Rc::new(RefCell::new(None));
    clipboard.read_text({
        let read = read.clone();
        move |text| *read.borrow_mut() = Some(text)
    });
    assert!(read.borrow().is_none(), "その場では呼ばない");
    pump_until(2.0, || read.borrow().is_some());
    let got = read.borrow().clone();

    pasteboard.clearContents();
    if let Some(saved) = saved {
        pasteboard.setString_forType(&NSString::from_str(&saved), unsafe {
            NSPasteboardTypeString
        });
    }
    assert_eq!(got, Some(Some("naui のテスト ✂".to_string())));
    Ok(())
}

define_class!(
    /// Stands in for the `NSDraggingInfo` AppKit passes during a drag. Only the two
    /// things naui reads are answered: the pasteboard and the pointer position.
    #[unsafe(super(objc2_foundation::NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "NauiTestDraggingInfo"]
    #[ivars = (Retained<objc2_app_kit::NSPasteboard>, Cell<NSPoint>)]
    struct FakeDraggingInfo;

    unsafe impl NSObjectProtocol for FakeDraggingInfo {}

    impl FakeDraggingInfo {
        #[unsafe(method_id(draggingPasteboard))]
        fn dragging_pasteboard(&self) -> Retained<objc2_app_kit::NSPasteboard> {
            self.ivars().0.clone()
        }

        #[unsafe(method(draggingLocation))]
        fn dragging_location(&self) -> NSPoint {
            self.ivars().1.get()
        }
    }
);

/// Files dropped from Finder go to the innermost widget under the pointer that has
/// `on_file_drop`, and otherwise to the window's. A real drag cannot be synthesized,
/// so the messages AppKit sends to the window are sent directly.
fn file_drop_reaches_the_widget_under_the_pointer(ui: &Ui) -> Result<()> {
    use objc2_app_kit::{NSDragOperation, NSPasteboard, NSPasteboardWriting};
    use objc2_foundation::{NSArray, NSURL};

    let window = ui.window("ファイルの受け取り", 320.0, 200.0)?;
    let stack = ui.stack(Orientation::Vertical)?;
    let outer = ui.stack(Orientation::Vertical)?;
    let target = ui.label("ここへ落とす")?;
    outer.append(&target);
    let plain = ui.button("受け取らない")?;
    stack.append(&outer);
    stack.append(&plain);
    window.set_child(&stack);
    window.show();
    pump(0.1);

    let to_target = Rc::new(RefCell::new(Vec::new()));
    let to_outer = Rc::new(Cell::new(0));
    let to_window = Rc::new(RefCell::new(Vec::new()));
    target.on_file_drop({
        let seen = to_target.clone();
        move |files| seen.borrow_mut().extend(files.iter().cloned())
    });
    outer.on_file_drop({
        let seen = to_outer.clone();
        move |_| seen.set(seen.get() + 1)
    });

    let mtm = MainThreadMarker::new().expect("main thread");
    let pasteboard = NSPasteboard::pasteboardWithUniqueName();
    let urls: Vec<Retained<ProtocolObject<dyn NSPasteboardWriting>>> =
        ["/tmp/naui 写真.png", "/tmp/メモ.txt"]
            .iter()
            .map(|path| {
                ProtocolObject::from_retained(NSURL::fileURLWithPath(&NSString::from_str(path)))
            })
            .collect();
    pasteboard.clearContents();
    assert!(pasteboard.writeObjects(&NSArray::from_retained_slice(&urls)));
    let info =
        FakeDraggingInfo::alloc(mtm).set_ivars((pasteboard.clone(), Cell::new(NSPoint::ZERO)));
    let info: Retained<FakeDraggingInfo> = unsafe { msg_send![super(info), init] };
    let native = window.native_window();

    let center_of = |view: &NSView| {
        let bounds = view.bounds();
        view.convertPoint_toView(
            NSPoint::new(bounds.size.width / 2.0, bounds.size.height / 2.0),
            None,
        )
    };
    let operation_at = |point: NSPoint| -> NSDragOperation {
        info.ivars().1.set(point);
        unsafe { msg_send![&*native, draggingEntered: &*info] }
    };
    let drop_at = |point: NSPoint| -> bool {
        info.ivars().1.set(point);
        unsafe { msg_send![&*native, performDragOperation: &*info] }
    };

    let on_target = center_of(&target.native_view());
    let on_plain = center_of(&plain.native_view());
    assert_eq!(
        operation_at(on_target),
        NSDragOperation::Copy,
        "受け取るウィジェットの上ではコピー"
    );
    assert_eq!(
        operation_at(on_plain),
        NSDragOperation::None,
        "受け取り手がいなければ断る"
    );
    assert!(!drop_at(on_plain));

    assert!(drop_at(on_target));
    let names: Vec<String> = to_target
        .borrow()
        .iter()
        .map(|f| f.name().to_string())
        .collect();
    assert_eq!(names, ["naui 写真.png", "メモ.txt"]);
    assert_eq!(
        to_target.borrow()[0].path().and_then(|p| p.to_str()),
        Some("/tmp/naui 写真.png"),
        "絶対パスを渡す"
    );
    assert_eq!(to_outer.get(), 0, "内側が受け取ったら外側へは届かない");

    window.on_file_drop({
        let seen = to_window.clone();
        move |files| {
            seen.borrow_mut()
                .extend(files.iter().map(|f| f.name().to_string()))
        }
    });
    assert_eq!(
        operation_at(on_plain),
        NSDragOperation::Copy,
        "窓が受け取る"
    );
    assert!(drop_at(on_plain));
    assert_eq!(to_window.borrow().len(), 2);
    assert_eq!(
        to_target.borrow().len(),
        2,
        "ほかのウィジェットへは届かない"
    );

    // ファイルでない中身 (文字列だけ) は受け取らない。
    pasteboard.clearContents();
    pasteboard.setString_forType(&NSString::from_str("文字"), unsafe {
        objc2_app_kit::NSPasteboardTypeString
    });
    assert_eq!(operation_at(on_target), NSDragOperation::None);
    window.close();
    Ok(())
}

/// Strings without a scheme are rejected without being passed to `NSWorkspace`.
/// Passing them made Finder show a "The application can't be opened. (-50)" dialog,
/// leaving one behind every time the tests ran.
fn open_url_rejects_garbage(ui: &Ui) -> Result<()> {
    assert!(ui.open_url("").is_err());
    assert!(ui.open_url("example.com").is_err());
    Ok(())
}

fn window_reports_its_size(ui: &Ui) -> Result<()> {
    let window = ui.window("大きさ", 320.0, 200.0)?;
    window.set_child(&ui.label("中身")?);
    assert_eq!(window.size(), (320.0, 200.0));
    let seen = Rc::new(RefCell::new(Vec::new()));
    window.on_resize({
        let seen = seen.clone();
        move |size| seen.borrow_mut().push(size)
    });
    window.show();
    window.set_size(400.0, 260.0);
    pump_until(1.0, || !seen.borrow().is_empty());
    assert_eq!(window.size(), (400.0, 260.0));
    assert_eq!(seen.borrow().last(), Some(&(400.0, 260.0)));
    window.close();
    Ok(())
}

/// Attaching a toolbar makes AppKit grow the content by the title bar's height, and
/// a sidebar (`fullSizeContentView`) keeps the frame and shrinks the content. In
/// both cases the content keeps the size decided with `ui.window` or `set_size`,
/// and the frame changes instead.
fn window_keeps_its_size_across_toolbar_and_sidebar(ui: &Ui) -> Result<()> {
    let sidebar = ui.sidebar()?;
    sidebar.set_items(&[SidebarItem::new("一般")]);
    let toolbar = ui.toolbar()?;
    toolbar.set_items(&[ToolbarItem::new(ToolbarIcon::New, "新規")]);
    // 子のラベルは縦に縮もうとするので、縦の Stack に入れて区画に合わせる。
    let content = || -> Result<naui_macos::Stack> {
        let stack = ui.stack(Orientation::Vertical)?;
        stack.append(&ui.label("中身")?);
        Ok(stack)
    };

    // 表示する前に付けても、ui.window で指定した大きさになる。
    // 枠が画面に収まらないと、表示するときに AppKit が縮めてしまうので、
    // CI の小さな画面 (1024×768、Dock とメニューバーを除くと高さ 674) にも
    // 収まる大きさにする。
    let window = ui.window("付けてから出す", 600.0, 400.0)?;
    window.set_child(&content()?);
    window.set_toolbar(&toolbar);
    assert_eq!(window.size(), (600.0, 400.0), "ツールバーを付けた後");
    window.set_sidebar(&sidebar);
    assert_eq!(window.size(), (600.0, 400.0), "サイドバーも付けた後");
    window.show();
    pump(0.3);
    let native = window.native_window();
    assert_eq!(
        window.size(),
        (600.0, 400.0),
        "表示した後: frame={:?} layout={:?} screen={:?} zoomed={}",
        native.frame(),
        native.contentLayoutRect(),
        native.screen().map(|screen| screen.visibleFrame()),
        native.isZoomed()
    );
    window.close();

    // 表示中に付け外ししても変わらず、上端も動かない。
    let window = ui.window("表示中に付け外し", 400.0, 300.0)?;
    window.set_child(&content()?);
    window.show();
    pump(0.3);
    let top = |window: &naui_macos::Window| {
        let frame = window.native_window().frame();
        frame.origin.y + frame.size.height
    };
    let first_top = top(&window);
    let steps: [(&str, &dyn Fn()); 4] = [
        ("ツールバーを付ける", &|| {
            window.set_toolbar(&toolbar)
        }),
        ("サイドバーを付ける", &|| {
            window.set_sidebar(&sidebar)
        }),
        ("サイドバーを外す", &|| window.clear_sidebar()),
        ("ツールバーを外す", &|| window.clear_toolbar()),
    ];
    for (step, change) in steps {
        change();
        pump(0.1);
        assert_eq!(window.size(), (400.0, 300.0), "{step}");
        assert!(
            (top(&window) - first_top).abs() < 0.5,
            "{step}: 上端が動かない"
        );
    }
    window.close();
    Ok(())
}

fn window_close_request_can_keep_it_open(ui: &Ui) -> Result<()> {
    let window = ui.window("閉じる確認", 320.0, 200.0)?;
    window.set_child(&ui.label("中身")?);
    window.show();
    let asked = Rc::new(Cell::new(0));
    let keep = Rc::new(Cell::new(true));
    window.on_close_request({
        let asked = asked.clone();
        let keep = keep.clone();
        move || {
            asked.set(asked.get() + 1);
            if keep.get() {
                naui_core::CloseResponse::KeepOpen
            } else {
                naui_core::CloseResponse::Close
            }
        }
    });
    let native = window.native_window();
    // タイトルバーの閉じるボタンと同じ経路 (`performClose:`)。
    native.performClose(None);
    assert_eq!(asked.get(), 1);
    assert!(window.is_visible(), "KeepOpen なら閉じない");
    keep.set(false);
    native.performClose(None);
    assert_eq!(asked.get(), 2);
    assert!(!window.is_visible(), "Close なら閉じる");

    // プログラムからの close は確かめない。
    window.show();
    keep.set(true);
    window.close();
    assert_eq!(asked.get(), 2);
    assert!(!window.is_visible());
    Ok(())
}

/// The position is the top left of the window frame, counted right and down from
/// the primary screen's top left. It round-trips whether set before or after
/// showing, and later `set_size` calls do not recenter it.
fn window_position_round_trips(ui: &Ui) -> Result<()> {
    let near = |a: Option<(f64, f64)>, b: (f64, f64)| {
        a.is_some_and(|a| (a.0 - b.0).abs() < 1.0 && (a.1 - b.1).abs() < 1.0)
    };
    let window = ui.window("位置", 320.0, 200.0)?;
    window.set_child(&ui.label("中身")?);
    window.set_position(120.0, 140.0);
    assert!(
        near(window.position(), (120.0, 140.0)),
        "表示の前: {:?}",
        window.position()
    );
    window.show();
    pump(0.2);
    assert!(
        near(window.position(), (120.0, 140.0)),
        "表示の後: {:?}",
        window.position()
    );

    // AppKit の座標 (左下から上へ) と向きが逆になっていること。
    let mtm = MainThreadMarker::new().expect("メインスレッド");
    let screen_height = objc2_app_kit::NSScreen::screens(mtm)
        .firstObject()
        .map(|screen| screen.frame().size.height)
        .expect("主画面");
    let frame = window.native_window().frame();
    assert!(
        (screen_height - (frame.origin.y + frame.size.height) - 140.0).abs() < 1.0,
        "枠の上端が主画面の上から 140: frame={frame:?} screen_height={screen_height}"
    );

    window.set_size(360.0, 220.0);
    assert!(
        near(window.position(), (120.0, 140.0)),
        "set_size で中央へ寄せ直さない: {:?}",
        window.position()
    );
    window.set_position(200.0, 160.0);
    pump(0.1);
    assert!(
        near(window.position(), (200.0, 160.0)),
        "表示中に動かす: {:?}",
        window.position()
    );
    window.close();
    Ok(())
}

/// Maximize (`zoom:`), minimize (`miniaturize:`) and fullscreen
/// (`toggleFullScreen:`) round-trip with `state`, and `on_state_change` is called on
/// each change.
fn window_state_round_trips(ui: &Ui) -> Result<()> {
    let window = ui.window("状態", 320.0, 200.0)?;
    window.set_child(&ui.label("中身")?);
    let seen = Rc::new(RefCell::new(Vec::new()));
    window.on_state_change({
        let seen = seen.clone();
        move |state| seen.borrow_mut().push(state)
    });
    window.show();
    pump(0.2);
    assert_eq!(window.state(), WindowState::Normal);
    let native = window.native_window();

    window.set_state(WindowState::Maximized);
    pump_until(2.0, || window.state() == WindowState::Maximized);
    assert_eq!(
        window.state(),
        WindowState::Maximized,
        "zoom={}",
        native.isZoomed()
    );
    assert!(native.isZoomed());
    window.set_state(WindowState::Normal);
    pump_until(2.0, || window.state() == WindowState::Normal);
    assert_eq!(window.state(), WindowState::Normal);
    assert_eq!(
        seen.borrow().as_slice(),
        [WindowState::Maximized, WindowState::Normal]
    );

    window.set_state(WindowState::Minimized);
    pump_until(3.0, || window.state() == WindowState::Minimized);
    if window.state() == WindowState::Minimized {
        window.set_state(WindowState::Normal);
        pump_until(3.0, || window.state() == WindowState::Normal);
        assert_eq!(window.state(), WindowState::Normal, "最小化から戻す");
        assert!(!native.isMiniaturized());
    } else {
        // Dock の無いセッションでは miniaturize: が効かない。
        skip("最小化できない環境 (Dock が無い)");
    }

    seen.borrow_mut().clear();
    window.set_state(WindowState::Fullscreen);
    pump_until(5.0, || window.state() == WindowState::Fullscreen);
    if window.state() == WindowState::Fullscreen {
        assert!(native.styleMask().contains(NSWindowStyleMask::FullScreen));
        // 全画面から最大化へは、全画面を出終わってから zoom する。
        window.set_state(WindowState::Maximized);
        pump_until(5.0, || window.state() == WindowState::Maximized);
        assert_eq!(window.state(), WindowState::Maximized, "全画面から最大化へ");
        window.set_state(WindowState::Normal);
        pump_until(2.0, || window.state() == WindowState::Normal);
        assert_eq!(window.state(), WindowState::Normal);
        assert_eq!(
            seen.borrow().as_slice(),
            [
                WindowState::Fullscreen,
                WindowState::Normal,
                WindowState::Maximized,
                WindowState::Normal
            ],
            "全画面を出たところでいったん Normal が届く"
        );
    } else {
        // 画面を占有できないセッション (CI の一部) では全画面へ入れない。
        skip("全画面へ入れない環境");
    }
    window.close();
    Ok(())
}

/// While maximized, minimized or in fullscreen, `position` returns the position for
/// returning to normal, and `set_position` decides that position without changing
/// the state. While maximized or in fullscreen the visible frame does not move
/// either (while minimized, `frame` is the return target itself).
fn window_position_while_not_normal(ui: &Ui) -> Result<()> {
    let near = |a: Option<(f64, f64)>, b: (f64, f64)| {
        a.is_some_and(|a| (a.0 - b.0).abs() < 1.0 && (a.1 - b.1).abs() < 1.0)
    };
    let window = ui.window("状態の間の位置", 320.0, 200.0)?;
    window.set_child(&ui.label("中身")?);
    window.set_position(120.0, 140.0);
    window.show();
    pump(0.2);
    let native = window.native_window();
    let mut normal = (120.0, 140.0);
    let mut skipped = Vec::new();
    for (index, (state, limit)) in [
        (WindowState::Maximized, 2.0),
        (WindowState::Minimized, 3.0),
        (WindowState::Fullscreen, 10.0),
    ]
    .into_iter()
    .enumerate()
    {
        window.set_state(state);
        pump_until(limit, || window.state() == state);
        if window.state() != state {
            // Dock の無いセッションでは最小化が、画面を占有できないセッションでは
            // 全画面が効かない。CI のランナーでは全画面へ入り終わるのに 6 秒ほど
            // かかることがあるので、全画面は長めに待つ。
            skipped.push(format!("{state:?}"));
            continue;
        }
        assert!(
            near(window.position(), normal),
            "{state:?} の間は元の位置: {:?}",
            window.position()
        );
        let shown = native.frame();
        normal = (200.0 + 20.0 * index as f64, 160.0 + 10.0 * index as f64);
        window.set_position(normal.0, normal.1);
        pump(0.2);
        assert_eq!(window.state(), state, "{state:?} のまま");
        if state != WindowState::Minimized {
            assert_eq!(native.frame(), shown, "{state:?} の枠は動かない");
        }
        assert!(
            near(window.position(), normal),
            "{state:?} の間に決めた位置: {:?}",
            window.position()
        );
        window.set_state(WindowState::Normal);
        pump_until(limit, || window.state() == WindowState::Normal);
        pump(0.2);
        assert_eq!(window.state(), WindowState::Normal);
        assert!(
            near(window.position(), normal),
            "{state:?} から戻ると決めた位置: {:?}",
            window.position()
        );
    }
    if !skipped.contains(&"Fullscreen".to_string()) {
        // 全画面の間に決めた位置は、全画面から最大化へ移っても元に戻す先になる。
        window.set_state(WindowState::Fullscreen);
        pump_until(10.0, || window.state() == WindowState::Fullscreen);
        window.set_position(260.0, 190.0);
        window.set_state(WindowState::Maximized);
        pump_until(10.0, || window.state() == WindowState::Maximized);
        assert_eq!(window.state(), WindowState::Maximized);
        assert!(
            near(window.position(), (260.0, 190.0)),
            "全画面から最大化: {:?}",
            window.position()
        );
        // 最大化の間に決めた位置は、全画面を挟んでも残る。
        window.set_position(280.0, 200.0);
        window.set_state(WindowState::Fullscreen);
        pump_until(10.0, || window.state() == WindowState::Fullscreen);
        assert!(
            near(window.position(), (280.0, 200.0)),
            "最大化から全画面: {:?}",
            window.position()
        );
        window.set_state(WindowState::Normal);
        pump_until(10.0, || window.state() == WindowState::Normal);
        pump(0.2);
        assert_eq!(window.state(), WindowState::Normal);
        assert!(
            near(window.position(), (280.0, 200.0)),
            "全画面から戻る: {:?}",
            window.position()
        );
    }
    window.close();
    if !skipped.is_empty() {
        skip(&format!("{} にできない環境", skipped.join("・")));
    }
    Ok(())
}

/// `set_state` before showing is remembered and applied when shown. Changing the
/// same window's state from inside the notification does not lose that change's
/// notification.
///
/// On macOS 27, a `zoom:` requested inside the notification is not applied on the
/// spot (it is still zoomed right after returning), and the restoration arrives
/// later through `windowDidResize:`. The mechanism of deferring and reporting state
/// changes made inside the notification is itself verified by the unit tests of
/// naui-core's `StateHandler`.
fn window_state_change_inside_the_notification(ui: &Ui) -> Result<()> {
    let window = ui.window("通知の中の状態", 320.0, 200.0)?;
    window.set_child(&ui.label("中身")?);
    let seen = Rc::new(RefCell::new(Vec::new()));
    window.on_state_change({
        let seen = seen.clone();
        let weak = window.downgrade();
        move |state| {
            seen.borrow_mut().push(state);
            if state == WindowState::Maximized {
                if let Some(window) = weak.upgrade() {
                    window.set_state(WindowState::Normal);
                }
            }
        }
    });
    window.show();
    pump(0.2);
    window.set_state(WindowState::Maximized);
    pump_until(2.0, || seen.borrow().len() >= 2);
    assert_eq!(
        seen.borrow().as_slice(),
        [WindowState::Maximized, WindowState::Normal]
    );
    assert_eq!(window.state(), WindowState::Normal);
    window.close();
    Ok(())
}

fn window_state_requested_before_show(ui: &Ui) -> Result<()> {
    let window = ui.window("表示前の状態", 320.0, 200.0)?;
    window.set_child(&ui.label("中身")?);
    window.set_state(WindowState::Maximized);
    assert_eq!(
        window.state(),
        WindowState::Maximized,
        "表示の前は頼んだ状態"
    );
    assert!(!window.native_window().isZoomed());
    window.show();
    pump_until(2.0, || window.native_window().isZoomed());
    assert_eq!(window.state(), WindowState::Maximized);
    window.close();
    Ok(())
}

/// `placement` keeps the normal frame while maximized, minimized or in fullscreen,
/// and `set_size` there decides the size for returning without leaving the state.
fn window_placement_while_not_normal(ui: &Ui) -> Result<()> {
    let near = |a: (f64, f64), b: (f64, f64)| (a.0 - b.0).abs() < 1.0 && (a.1 - b.1).abs() < 1.0;
    let window = ui.window("保存する枠", 320.0, 200.0)?;
    window.set_child(&ui.label("中身")?);
    window.set_position(120.0, 140.0);
    window.show();
    pump(0.2);
    let placement = window.placement();
    assert!(near(placement.size, (320.0, 200.0)), "{placement:?}");
    assert!(
        placement
            .position
            .is_some_and(|position| near(position, (120.0, 140.0))),
        "{placement:?}"
    );
    assert_eq!(placement.state, WindowState::Normal);

    let native = window.native_window();
    let mut normal = (320.0, 200.0);
    let mut skipped = Vec::new();
    for (index, (state, limit)) in [
        (WindowState::Maximized, 2.0),
        (WindowState::Fullscreen, 10.0),
    ]
    .into_iter()
    .enumerate()
    {
        window.set_state(state);
        pump_until(limit, || window.state() == state);
        if window.state() != state {
            skipped.push(format!("{state:?}"));
            continue;
        }
        let placement = window.placement();
        assert_eq!(placement.state, state);
        assert!(
            near(placement.size, normal),
            "{state:?} の間は元の大きさ: {placement:?}"
        );
        assert!(
            placement
                .position
                .is_some_and(|position| near(position, (120.0, 140.0))),
            "{state:?} の間は元の位置: {placement:?}"
        );
        let shown = native.frame();
        normal = (360.0 + 20.0 * index as f64, 240.0 + 10.0 * index as f64);
        window.set_size(normal.0, normal.1);
        pump(0.2);
        assert_eq!(window.state(), state, "{state:?} のまま");
        assert_eq!(native.frame(), shown, "{state:?} の枠は変わらない");
        assert!(
            near(window.placement().size, normal),
            "{state:?} の間に決めた大きさ: {:?}",
            window.placement()
        );
        window.set_state(WindowState::Normal);
        pump_until(limit, || window.state() == WindowState::Normal);
        pump(0.3);
        assert_eq!(window.state(), WindowState::Normal);
        assert!(
            near(window.size(), normal),
            "{state:?} から戻ると決めた大きさ: {:?}",
            window.size()
        );
        assert!(
            window
                .position()
                .is_some_and(|position| near(position, (120.0, 140.0))),
            "{state:?} から戻っても位置は動かない: {:?}",
            window.position()
        );
    }

    // 最大化したまま最小化したら、戻す先の最大化を保存する。
    window.set_state(WindowState::Maximized);
    pump_until(2.0, || window.state() == WindowState::Maximized);
    window.set_state(WindowState::Minimized);
    pump_until(3.0, || window.state() == WindowState::Minimized);
    if window.state() == WindowState::Minimized {
        let placement = window.placement();
        assert_eq!(placement.state, WindowState::Maximized, "{placement:?}");
        assert!(near(placement.size, normal), "{placement:?}");
    } else {
        skipped.push("Minimized".to_string());
    }
    window.close();
    if !skipped.is_empty() {
        skip(&format!("{} にできない環境", skipped.join("・")));
    }
    Ok(())
}

/// A placement read back from text and applied before showing puts the window in
/// that frame and state, and a position on no connected screen is skipped.
fn window_placement_restores_before_show(ui: &Ui) -> Result<()> {
    let near = |a: (f64, f64), b: (f64, f64)| (a.0 - b.0).abs() < 1.0 && (a.1 - b.1).abs() < 1.0;
    let saved =
        WindowPlacement::parse("size=380x260 position=150,170 state=maximized").expect("読める");
    let window = ui.window("戻す枠", 320.0, 200.0)?;
    window.set_child(&ui.label("中身")?);
    window.set_placement(&saved);
    let before = window.placement();
    assert!(
        near(before.size, saved.size)
            && before
                .position
                .is_some_and(|position| near(position, (150.0, 170.0)))
            && before.state == WindowState::Maximized,
        "表示の前: {before:?}"
    );
    window.show();
    pump_until(2.0, || window.native_window().isZoomed());
    assert_eq!(window.state(), WindowState::Maximized);
    let placement = window.placement();
    assert_eq!(placement.state, WindowState::Maximized);
    assert!(near(placement.size, saved.size), "{placement:?}");
    window.set_state(WindowState::Normal);
    pump_until(2.0, || window.state() == WindowState::Normal);
    pump(0.2);
    assert!(near(window.size(), saved.size), "{:?}", window.size());
    assert!(
        window
            .position()
            .is_some_and(|position| near(position, (150.0, 170.0))),
        "{:?}",
        window.position()
    );
    window.close();

    // つながっていない画面の位置は使わず、中央に出す。
    let window = ui.window("外れた画面", 320.0, 200.0)?;
    window.set_child(&ui.label("中身")?);
    let centered = window.position().expect("位置");
    window.set_placement(&WindowPlacement {
        size: (320.0, 200.0),
        position: Some((-100_000.0, -100_000.0)),
        state: WindowState::Normal,
    });
    assert!(
        window
            .position()
            .is_some_and(|position| near(position, centered)),
        "{:?}",
        window.position()
    );
    window.close();
    Ok(())
}

fn window_resizable_round_trips(ui: &Ui) -> Result<()> {
    let window = ui.window("大きさの固定", 320.0, 200.0)?;
    window.set_child(&ui.label("中身")?);
    let native = window.native_window();
    assert!(window.is_resizable());
    assert!(native.styleMask().contains(NSWindowStyleMask::Resizable));
    window.set_resizable(false);
    assert!(!window.is_resizable());
    assert!(!native.styleMask().contains(NSWindowStyleMask::Resizable));
    window.show();
    // 固定しても、プログラムからは大きさを変えられる。
    window.set_size(360.0, 240.0);
    assert_eq!(window.size(), (360.0, 240.0));
    window.set_resizable(true);
    assert!(window.is_resizable());
    assert!(native.styleMask().contains(NSWindowStyleMask::Resizable));
    window.close();
    Ok(())
}

fn accessible_label_read_only_and_min_size(ui: &Ui) -> Result<()> {
    use objc2_app_kit::NSAccessibility;
    let button = ui.button("⚙")?;
    button.set_accessible_label(Some("設定"));
    assert_eq!(
        button
            .native_view()
            .accessibilityLabel()
            .map(|t| t.to_string())
            .as_deref(),
        Some("設定")
    );
    button.set_accessible_label(None);
    assert!(button.native_view().accessibilityLabel().is_none());

    let input = ui.text_input("ログ")?;
    input.set_read_only(true);
    assert!(input.is_read_only());
    let field = input
        .native_view()
        .downcast::<NSTextField>()
        .expect("NSTextField");
    assert!(
        !field.isEditable() && field.isSelectable(),
        "選べるが書けない"
    );
    let area = ui.text_area("ログ")?;
    area.set_read_only(true);
    assert!(area.is_read_only());
    area.set_read_only(false);
    assert!(!area.is_read_only());

    let window = ui.window("最小", 320.0, 200.0)?;
    window.set_min_size(240.0, 120.0);
    let min = window.native_window().contentMinSize();
    assert_eq!((min.width, min.height), (240.0, 120.0));
    Ok(())
}

/// The range of visible rows of `table` (first, one past the last).
fn visible_rows(table: &objc2_app_kit::NSTableView) -> (usize, usize) {
    let range = table.rowsInRect(table.visibleRect());
    (range.location, range.location + range.length)
}

fn rows_can_be_scrolled_into_view(ui: &Ui) -> Result<()> {
    let window = ui.window("行へ送る", 320.0, 240.0)?;
    let root = ui.stack(Orientation::Horizontal)?;
    let labels: Vec<String> = (0..100).map(|i| format!("行 {i}")).collect();

    let list = ui.list()?;
    list.set_items(&ListItem::list(labels.iter().map(String::as_str)));
    list.set_sizing(Sizing::fixed(100.0, 200.0));
    let table = ui.table()?;
    table.set_columns(&[TableColumn::new("名前")]);
    table.set_rows(&TableRow::list(labels.iter().map(|label| [label.as_str()])));
    table.set_sizing(Sizing::fixed(100.0, 200.0));
    let tree = ui.tree()?;
    tree.set_items(&[
        TreeItem::new("上").children(TreeItem::list(labels.iter().map(String::as_str)))
    ]);
    tree.set_sizing(Sizing::fixed(100.0, 200.0));
    root.append(&list);
    root.append(&table);
    root.append(&tree);
    window.set_child(&root);
    window.show();
    window
        .native_window()
        .contentView()
        .unwrap()
        .layoutSubtreeIfNeeded();

    let list_view = list.native_table();
    assert_eq!(visible_rows(&list_view).0, 0, "最初は先頭");
    list.scroll_to_row(80);
    let (first, end) = visible_rows(&list_view);
    assert!(first <= 80 && 80 < end, "80 行目が見える: {first}..{end}");
    list.scroll_to_row(first);
    assert_eq!(visible_rows(&list_view).0, first, "見えていれば動かさない");
    list.scroll_to_row(1000); // 範囲外は何もしない
    assert!(list.selected().is_none(), "選択は変わらない");

    let table_view = table.native_table();
    table.scroll_to_row(90);
    let (first, end) = visible_rows(&table_view);
    assert!(
        first <= 90 && 90 < end,
        "表の 90 行目が見える: {first}..{end}"
    );

    assert!(!tree.is_expanded(&[0]));
    tree.scroll_to_item(&[0, 70]);
    assert!(tree.is_expanded(&[0]), "祖先を開いてから送る");
    let outline = tree.native_outline_view();
    let (first, end) = visible_rows(&outline);
    // 0 行目は「上」なので、[0, 70] は 71 行目。
    assert!(
        first <= 71 && 71 < end,
        "ツリーの項目が見える: {first}..{end}"
    );
    window.close();
    Ok(())
}

/// A scroll view keeps holding its content even after the handle is dropped.
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

/// Takes out only the size constraints naui added.
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

/// Items of different heights in the same row are aligned at the vertical center.
///
/// NSGridView defaults to top alignment, so placing a label (16pt) next to a text
/// field (24pt) shifts only the label upward.
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

/// File picking is set up as an AppKit button and keeps its settings.
///
/// `NSOpenPanel` is app-modal and does not return until it is closed once shown. It
/// cannot be opened from automated tests, so this checks **up to the real button
/// and that the settings are kept**.
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

/// Saving is set up as a button, and the settings reach the `NSSavePanel`.
///
/// `NSSavePanel` is also app-modal and does not return until it is closed once
/// shown. It cannot be opened from automated tests, so this checks **the panel's
/// contents before it is shown**.
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

/// A default name without an extension gets the first extension of the filters.
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

/// The main menu has Edit items, and ⌘V flows into the responder chain.
///
/// On macOS, ⌘C / ⌘V are dispatched as key equivalents of the main menu. Without a
/// menu, pasting does not work even when a text field has focus.
fn menu_bar_provides_edit_shortcuts(_ui: &Ui) -> Result<()> {
    let mtm = objc2::MainThreadMarker::new().expect("メインスレッド");
    naui_macos::install_menu_bar_for_test(mtm, "naui test");

    let app = objc2_app_kit::NSApplication::sharedApplication(mtm);
    let main = app.mainMenu().expect("メインメニューがあること");
    let edit = (0..main.numberOfItems())
        .filter_map(|i| main.itemAtIndex(i).and_then(|item| item.submenu()))
        .find(|menu| menu.title().to_string() == naui_core::ui_strings().edit_menu)
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

/// The standard menus' titles come from the table chosen by
/// `NSLocale.preferredLanguages`, not from a fixed language.
fn menu_bar_follows_the_os_language(_ui: &Ui) -> Result<()> {
    let mtm = objc2::MainThreadMarker::new().expect("メインスレッド");
    naui_macos::install_menu_bar_for_test(mtm, "naui test");

    let preferred: Vec<String> = objc2_foundation::NSLocale::preferredLanguages()
        .iter()
        .map(|tag| tag.to_string())
        .collect();
    let text = naui_core::UiStrings::for_languages(&preferred);
    assert_eq!(
        naui_core::ui_strings(),
        text,
        "起動時に OS の優先言語 {preferred:?} から表を選んでいること"
    );

    let app = objc2_app_kit::NSApplication::sharedApplication(mtm);
    let main = app.mainMenu().expect("メインメニューがあること");
    let titles = |menu: &objc2_app_kit::NSMenu| -> Vec<String> {
        (0..menu.numberOfItems())
            .filter_map(|i| menu.itemAtIndex(i))
            .map(|item| item.title().to_string())
            .collect()
    };
    let submenu = |index: isize| {
        main.itemAtIndex(index)
            .and_then(|item| item.submenu())
            .expect("サブメニューがあること")
    };
    assert_eq!(titles(&submenu(0)), [text.quit("naui test")]);
    let edit = submenu(main.numberOfItems() - 1);
    assert_eq!(edit.title().to_string(), text.edit_menu);
    assert_eq!(
        titles(&edit),
        [
            text.undo,
            text.redo,
            text.cut,
            text.copy,
            text.paste,
            text.select_all
        ]
    );
    Ok(())
}

// --------------------------------------------------------------- メディア

/// A 2x2 PNG. Embedded to check image loading through a real file.
const PNG_2X2: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x02, 0x08, 0x02, 0x00, 0x00, 0x00, 0xfd, 0xd4, 0x9a,
    0x73, 0x00, 0x00, 0x00, 0x10, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0xf8, 0xcf, 0x00, 0x04,
    0xff, 0x19, 0x20, 0x14, 0x00, 0x1b, 0xf2, 0x03, 0xfd, 0xd6, 0x96, 0xf2, 0x2b, 0x00, 0x00, 0x00,
    0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
];

/// Temporary files used by a test. Removed on Drop so nothing is left behind even
/// on failure.
struct Fixture {
    dir: std::path::PathBuf,
}

impl Fixture {
    /// Creates a separate directory per test.
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

    /// A 0.5-second silent WAV. The smallest media AVFoundation can actually play.
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

/// Runs the run loop to advance AVFoundation's asynchronous loading and playback.
///
/// Both asset loading and playback position updates happen on the run loop, so
/// without running it the duration is never decided and playback does not advance.
fn pump(seconds: f64) {
    let until = NSDate::dateWithTimeIntervalSinceNow(seconds);
    NSRunLoop::currentRunLoop().runUntilDate(&until);
}

/// Runs the run loop until the condition is met, up to the limit.
///
/// How fast loading, playback and timers progress depends on the machine, so a
/// fixed-time `pump` can fall short on a slow CI runner. Wait for things with this
/// instead. If the condition is not met by the limit it simply returns, so the
/// caller checks.
fn pump_until(limit: f64, mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs_f64(limit);
    while !ready() && Instant::now() < deadline {
        pump(0.05);
    }
}

/// Attaches a closure that records state changes.
fn record_states(seen: &Rc<RefCell<Vec<PlaybackState>>>) -> impl FnMut(PlaybackState) + 'static {
    let seen = seen.clone();
    move |state| seen.borrow_mut().push(state)
}

/// An image is loaded from a local file as an NSImage.
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

/// A `Fill` image asks for no size on that axis and fits into what it is given; an
/// `Auto` axis still asks for the image's size.
///
/// Asking for its pixel size, a large image inside a vertical `Scroll` pushed the
/// content to its own height, and the rows after it left the visible area.
fn fill_image_does_not_push_a_scroll(ui: &Ui) -> Result<()> {
    let fixture = Fixture::new("large-image");
    let path = fixture.write("a.png", PNG_2X2);
    let large = NSSize::new(3000.0, 2000.0);
    let image = ui.image(&path)?;
    image.set_sizing(Sizing::fill());
    let view = image.native_view();
    let native = view
        .downcast_ref::<NSImageView>()
        .expect("実体が NSImageView であること");
    // NSImageView は画像の大きさ (ポイント) を自然な大きさにする。
    native.image().expect("NSImage があること").setSize(large);
    view.invalidateIntrinsicContentSize();
    let none = unsafe { objc2_app_kit::NSViewNoIntrinsicMetric };
    let fill = view.intrinsicContentSize();
    assert_eq!(
        (fill.width, fill.height),
        (none, none),
        "Fill の軸は大きさを求めない"
    );
    image.set_sizing(Sizing::fill_width());
    let auto_height = view.intrinsicContentSize();
    assert_eq!(
        (auto_height.width, auto_height.height),
        (none, 2000.0),
        "Auto の軸は画像の大きさを求める"
    );
    image.set_sizing(Sizing::fill());

    let pane = ui.stack(Orientation::Vertical)?;
    pane.set_align(Align::Start);
    pane.append(&ui.label("見出し")?);
    pane.append(&image);
    let footer = ui.label("下端")?;
    pane.append(&footer);
    let scroll = ui.scroll()?;
    scroll.set_policy(ScrollPolicy::Never, ScrollPolicy::Auto);
    scroll.set_child(&pane);
    scroll.set_sizing(Sizing::fill());
    let window = ui.window("大きな画像", 400.0, 300.0)?;
    window.set_child(&scroll);
    window.show();
    let content = window
        .native_window()
        .contentView()
        .expect("コンテンツビューがあること");
    content.layoutSubtreeIfNeeded();
    pump(0.05);
    content.layoutSubtreeIfNeeded();
    let metrics = scroll.metrics();
    let frame = view.frame();
    window.close();
    assert_eq!(
        metrics.content_height, metrics.viewport_height,
        "中身は表示領域に収まる: {metrics:?}"
    );
    assert!(
        frame.size.height > 100.0 && frame.size.height < 300.0,
        "画像は余りを受け取る: {frame:?}"
    );
    Ok(())
}

/// A `Fill` image with a maximum asks for its maximum (the maximum doubles as the
/// size wanted under normal conditions), so in an `Auto` row it keeps that height
/// even when a `Fill` row takes the rest. Asking for nothing on a `Fill` axis, it
/// collapsed to zero there.
fn capped_fill_image_asks_for_its_maximum(ui: &Ui) -> Result<()> {
    let fixture = Fixture::new("capped-image");
    let path = fixture.write("a.png", PNG_2X2);
    let image = ui.image(&path)?;
    image.set_sizing(Sizing::fill().max_height(100.0));
    let rest = ui.stack(Orientation::Vertical)?;
    rest.set_sizing(Sizing::fill());

    let grid = ui.grid()?;
    grid.set_column_track(0, Track::FILL);
    grid.set_row_track(0, Track::Auto);
    grid.set_row_track(1, Track::FILL);
    grid.attach(&image, GridCell::new(0, 0));
    grid.attach(&rest, GridCell::new(0, 1));
    grid.set_sizing(Sizing::fixed(300.0, 300.0));

    let root = grid.native_view();
    root.setFrameSize(NSSize::new(300.0, 300.0));
    root.layoutSubtreeIfNeeded();
    root.layoutSubtreeIfNeeded();
    let frame = image.native_view().frame();
    let rest_frame = rest.native_view().frame();
    assert_eq!(
        frame.size.height, 100.0,
        "Auto 行でも上限の高さを保つ: {frame:?} / {rest_frame:?}"
    );
    assert!(
        rest_frame.size.height > 150.0,
        "残りは Fill の行が受け取る: {rest_frame:?}"
    );
    let (top, bottom) = if frame.origin.y < rest_frame.origin.y {
        (frame, rest_frame)
    } else {
        (rest_frame, frame)
    };
    assert!(
        top.origin.y + top.size.height <= bottom.origin.y + 0.5,
        "行どうしが重ならない: {frame:?} / {rest_frame:?}"
    );
    Ok(())
}

/// The fitting setting becomes the NSImageView's imageScaling.
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

/// Even before the AVPlayerView has the video's intrinsic size, the display area is
/// reserved.
///
/// `Fill` with a maximum means "up to the maximum when there is room, less when it
/// is tight". So that the Gallery need not contain macOS-only settings, this meaning
/// is derived from `Sizing` (a promise shared by every backend).
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

/// Audio plays to the end, and the state and duration arrive from the AVPlayer.
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

/// With looping, it goes back to the start instead of stopping at the end.
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

    // 先頭へ戻った直後は、まだ読み込み待ちを名乗っていることがある。
    // 落ち着くまで回してから、再生が続いていることを確かめる。
    pump_until(5.0, || audio.state() == PlaybackState::Playing);
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

/// Setting the playback position moves the AVPlayer's current position.
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

/// Volume and mute round-trip with the AVPlayer, and out-of-range values are
/// clamped.
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

/// Dropping the handles does not crash with an AVPlayer that still has KVO
/// attached.
///
/// Freeing an AVPlayer while a KVO observer is still registered makes AppKit crash.
/// This actually frees it to check that Drop reliably removes the observer.
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

/// While playing, the playback position periodically reaches the closure.
///
/// The notification for keeping a seek bar in step with playback. It comes from the
/// AVPlayer's `addPeriodicTimeObserverForInterval:`.
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

/// A list's selection round-trips with the NSTableView, and `set_selected` does not
/// notify.
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

/// With multiple selection several rows are selected, and the selection can be
/// empty.
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

/// Rows that cannot be selected are selected neither from code nor natively.
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

/// Queues a real click in the window's event queue and advances until it is
/// delivered.
///
/// Unlike calling `mouseDown:` directly, it goes through AppKit's hit-testing and
/// tracking loop as is. Things whose behavior depends on "which view receives it",
/// like `NSTableView`, can only be verified through this path.
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

/// Delivers every event pending in the queue.
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

/// A click on the row itself reaches `on_activate`, and clicking a control inside
/// the row directly does not fire twice.
///
/// AppKit passes clicks on display-only labels and icons to the `NSTableView`
/// (hit-testing returns the table rather than the cell), so naui receives row clicks
/// through the table's action. Real clicks never reach the cell's `mouseDown:`, so
/// here real events are delivered to check it.
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
        Some(sel!(nauiRowClicked:)),
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

    if !native.isKeyWindow() {
        skip("ウィンドウが key にならず、本物のクリックを配送できなかった");
    }
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

/// Shows a list with rows in the key window, for checking activation and the
/// context menu.
///
/// The returned `bool` is whether the window became key (whether real events can be
/// delivered).
fn shown_list_for_events(
    ui: &Ui,
    title: &str,
) -> Result<(naui_macos::Window, naui_macos::List, bool)> {
    let mtm = MainThreadMarker::new().expect("メインスレッド");
    let app = NSApplication::sharedApplication(mtm);
    let window = ui.window(title, 480.0, 320.0)?;
    let list = ui.list()?;
    list.set_items(&[
        ListItem::new("札幌"),
        ListItem::new("仙台"),
        ListItem::new("東京").detail("13,960,000 人"),
        ListItem::new("大阪").enabled(false),
    ]);
    list.set_sizing(Sizing::fill());
    window.set_child(&list);

    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    #[allow(deprecated)]
    app.activateIgnoringOtherApps(true);
    window.show();
    let native = window.native_window();
    // ウィンドウが key になっても、アプリのアクティブ化が済むまでは、最初の
    // マウスの押下がアクティブ化に使われてビューへ届かないことがある。両方を待つ。
    let ready = |native: &NSWindow| native.isKeyWindow() && app.isActive();
    for _ in 0..60 {
        if ready(&native) {
            break;
        }
        pump(0.05);
        deliver_events(&app);
    }
    let content_view = native.contentView().expect("contentView");
    content_view.layoutSubtreeIfNeeded();
    unsafe { content_view.display() };
    let key = ready(&native);
    Ok((window, list, key))
}

/// Returns the center of row `row` in window coordinates.
fn row_center_in_window(table: &objc2_app_kit::NSTableView, row: isize) -> NSPoint {
    let rect = table.rectOfRow(row);
    let center = NSPoint::new(
        rect.origin.x + rect.size.width / 2.0,
        rect.origin.y + rect.size.height / 2.0,
    );
    table.convertPoint_toView(center, None)
}

/// Creates a mouse event at window coordinate `point`.
fn mouse_event(
    window: &NSWindow,
    kind: NSEventType,
    point: NSPoint,
    clicks: isize,
) -> Retained<NSEvent> {
    NSEvent::mouseEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_clickCount_pressure(
        kind,
        point,
        NSEventModifierFlags::empty(),
        0.0,
        window.windowNumber(),
        None,
        0,
        clicks,
        1.0,
    )
    .expect("マウスイベント")
}

/// A double-click reaches `on_row_activate` through `doubleAction`, and Return also
/// activates the selected row. Not called for rows that cannot be selected.
fn list_row_activate_notifies(ui: &Ui) -> Result<()> {
    let mtm = MainThreadMarker::new().expect("メインスレッド");
    let app = NSApplication::sharedApplication(mtm);
    let (window, list, key) = shown_list_for_events(ui, "行の起動")?;
    let activated: Rc<RefCell<Vec<usize>>> = Rc::new(RefCell::new(Vec::new()));
    list.on_row_activate({
        let activated = activated.clone();
        move |index| activated.borrow_mut().push(index)
    });
    let table = list.native_table();
    assert_eq!(
        table.doubleAction(),
        Some(sel!(nauiRowDoubleClicked:)),
        "ダブルクリックを doubleAction で受けていること"
    );
    let native = window.native_window();

    // Return は選ばれている行を起動する。first responder の keyDown: へ届ける。
    list.set_selected(1);
    native.makeFirstResponder(Some(&table));
    press_key(&app, &native, "\r", 36, NSEventModifierFlags::empty());
    assert_eq!(
        *activated.borrow(),
        vec![1],
        "Return で選んだ行が起動されること"
    );
    // 修飾キー付きの Return は起動にしない。
    press_key(&app, &native, "\r", 36, NSEventModifierFlags::Command);
    assert_eq!(activated.borrow().len(), 1, "⌘Return では起動しないこと");

    if key {
        // 1 回目と 2 回目のクリック (clickCount 2) を本物のイベントとして配送する。
        // 4 つとも積んでから配送する。`NSTableView` は押下を受けると、
        // 解放が来るまで自分の追跡ループでイベントを待つので、1 つずつ
        // 積んでは配送すると、まだ積んでいない解放を待ったまま止まる。
        let double_click = |row: isize| {
            let point = row_center_in_window(&table, row);
            for clicks in [1, 2] {
                for kind in [NSEventType::LeftMouseDown, NSEventType::LeftMouseUp] {
                    let event = mouse_event(&native, kind, point, clicks);
                    unsafe { app.postEvent_atStart(&event, false) };
                }
            }
            deliver_events(&app);
            pump(0.05);
            deliver_events(&app);
        };
        double_click(2);
        assert_eq!(
            *activated.borrow(),
            vec![1, 2],
            "ダブルクリックした行が起動されること"
        );
        double_click(3);
        assert_eq!(
            *activated.borrow(),
            vec![1, 2],
            "選べない行のダブルクリックでは起動しないこと"
        );
    } else {
        skip("ウィンドウが key にならず、本物のダブルクリックを配送できなかった");
    }

    window.close();
    app.setActivationPolicy(NSApplicationActivationPolicy::Prohibited);
    Ok(())
}

/// A right-click reaches `on_context_menu` through `rightMouseDown:`, with the
/// position from the list's top left (the same coordinates as
/// `PopupMenu::open_at`).
fn list_context_menu_notifies(ui: &Ui) -> Result<()> {
    let mtm = MainThreadMarker::new().expect("メインスレッド");
    let app = NSApplication::sharedApplication(mtm);
    let (window, list, _key) = shown_list_for_events(ui, "文脈メニュー")?;
    let seen: Rc<RefCell<Vec<(usize, Point)>>> = Rc::new(RefCell::new(Vec::new()));
    list.on_context_menu({
        let seen = seen.clone();
        move |index, at| seen.borrow_mut().push((index, at))
    });
    let table = list.native_table();
    let native = window.native_window();

    // 期待する位置: 行の中央を、リストのビューの左上からの座標へ直したもの。
    let outer = list.native_view();
    let expected = |row: isize| {
        let local = outer.convertPoint_fromView(row_center_in_window(&table, row), None);
        let y = if outer.isFlipped() {
            local.y
        } else {
            outer.bounds().size.height - local.y
        };
        (local.x, y)
    };

    // 右クリックは、作ったイベントをテーブルの `rightMouseDown:` へ直接渡す。
    // macOS 26 では、`mouseEventWithType:` で作った右クリックを待ち行列から
    // 取り出すと、ウィンドウ内の位置がずれてウィンドウの外を指す (左クリック
    // はずれない)。そのため本物の配送の経路は CI で確かめられない。
    // (ラベルの上の右クリックがテーブルまで上がることは macOS 27 で確かめた)
    let right_click = |row: isize| {
        let point = row_center_in_window(&table, row);
        let event = mouse_event(&native, NSEventType::RightMouseDown, point, 1);
        table.rightMouseDown(&event);
    };

    right_click(2);
    {
        let seen = seen.borrow();
        assert_eq!(seen.len(), 1, "右クリックが 1 回だけ通知されること");
        let (index, at) = seen[0];
        assert_eq!(index, 2, "右クリックした行のインデックスが届くこと");
        let (x, y) = expected(2);
        assert!(
            (at.x - x).abs() < 1.0 && (at.y - y).abs() < 1.0,
            "位置がリストの左上からの座標であること: {at:?} / ({x}, {y})"
        );
    }
    assert!(
        list.selection().is_empty(),
        "右クリックでは選択が変わらないこと"
    );

    right_click(3);
    assert_eq!(
        seen.borrow().len(),
        1,
        "選べない行の右クリックでは通知しないこと"
    );

    window.close();
    app.setActivationPolicy(NSApplicationActivationPolicy::Prohibited);
    Ok(())
}

/// Rows for settings screens can hold any layout and controls, not just labels.
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

/// `Ui` can be cloned, so widgets can be created even inside callbacks.
///
/// Row contents are not always all ready inside `build`. A screen that adds a row
/// on each click needs to create new widgets inside the notification and pass them
/// to [`List::set_rows`].
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

/// Row contents become NSTextFields that AppKit had the delegate create.
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
/// The total width of the columns (including the spacing between them) and the
/// width of the table itself.
fn table_widths(table: &naui_macos::Table) -> (f64, f64) {
    let native = table.native_table();
    let columns = native.tableColumns();
    let total: f64 = (0..columns.len())
        .map(|i| columns.objectAtIndex(i).width())
        .sum();
    let spacing = native.intercellSpacing().width * columns.len() as f64;
    (total + spacing, native.frame().size.width)
}

/// Even after removing columns and adding them back, the columns without a width
/// keep taking the leftover.
///
/// AppKit's automatic column resizing cannot distribute all of the leftover when
/// some columns have a fixed width (leaving blank space on the table's right). This
/// checks that naui redistributes it.
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

/// Sorting by clicking a heading round-trips with AppKit.
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

/// Swaps the columns of a table on screen many times, as the Gallery's "hide / show
/// area" does.
///
/// **The key is to have the cell views created before swapping columns.** Letting
/// AppKit compute row heights (`usesAutomaticRowHeights`) makes the next
/// `addTableColumn:` throw a "no common ancestor" exception, breaking the table
/// there.
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

/// Columns and rows become the `NSTableView`'s columns and rows as is.
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

/// Columns with a width are fixed, and only those without share the leftover. Text
/// alignment applies to both cells and headings.
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

/// However many rows there are, only the rows on screen are built.
///
/// Counts which rows the function passed to `set_row_builder` is called for, and
/// checks that it is not called for every row, nor once per column for the same row.
fn table_builds_only_the_rows_it_shows(ui: &Ui) -> Result<()> {
    const ROWS: usize = 500_000;
    let built: Rc<RefCell<Vec<usize>>> = Rc::new(RefCell::new(Vec::new()));

    let table = ui.table()?;
    table.set_columns(&TableColumn::list(["番号", "名前"]));
    let started = Instant::now();
    table.set_row_builder(ROWS, {
        let built = built.clone();
        move |index| {
            built.borrow_mut().push(index);
            Ok(TableCells::new()
                .text(index.to_string())
                .text(format!("行 {index}")))
        }
    });
    assert!(
        started.elapsed() < Duration::from_millis(500),
        "{ROWS} 行でも渡すだけなら待たされないこと: {:?}",
        started.elapsed()
    );
    assert_eq!(table.len(), ROWS);
    let native = table.native_table();
    assert_eq!(native.numberOfRows() as usize, ROWS);
    assert!(
        built.borrow().is_empty(),
        "まだ画面に出ていないので、1 行も組み立てないこと"
    );

    // 画面に出た行だけが組み立てられる。同じ行の 2 列目は組み立て直さない。
    let cell = native
        .viewAtColumn_row_makeIfNecessary(0, 42, true)
        .expect("42 行目 1 列目のビューが作られること")
        .downcast::<objc2_app_kit::NSTableCellView>()
        .expect("セルは NSTableCellView であること");
    let field = unsafe { cell.textField() }.expect("セルに文字が入っていること");
    assert_eq!(field.stringValue().to_string(), "42");
    let cell = native
        .viewAtColumn_row_makeIfNecessary(1, 42, true)
        .expect("42 行目 2 列目のビューが作られること")
        .downcast::<objc2_app_kit::NSTableCellView>()
        .expect("セルは NSTableCellView であること");
    let field = unsafe { cell.textField() }.expect("セルに文字が入っていること");
    assert_eq!(field.stringValue().to_string(), "行 42");
    assert_eq!(
        *built.borrow(),
        vec![42],
        "組み立てるのは見えた行だけで、列ごとには作り直さないこと"
    );

    // 中身が変わったら `refresh` で組み立て直す。
    table.refresh();
    let cell = native
        .viewAtColumn_row_makeIfNecessary(0, 42, true)
        .expect("セルが作られること");
    assert!(cell
        .downcast_ref::<objc2_app_kit::NSTableCellView>()
        .is_some());
    assert_eq!(
        *built.borrow(),
        vec![42, 42],
        "`refresh` は組み立て直すこと"
    );
    Ok(())
}

/// Widgets placed in cells go in as native views as is. Rows with
/// `selectable(false)` cannot be selected from code either.
fn table_widget_cells_are_native(ui: &Ui) -> Result<()> {
    let pressed = Rc::new(Cell::new(0));
    let table = ui.table()?;
    table.set_columns(&TableColumn::list(["都市", "操作"]));
    let ui = ui.clone();
    let pressed_for_button = pressed.clone();
    table.set_row_builder(3, move |index| {
        let button = ui.button("開く")?;
        button.on_click({
            let pressed = pressed_for_button.clone();
            move || pressed.set(pressed.get() + 1)
        });
        Ok(TableCells::new()
            .text(format!("都市 {index}"))
            .cell(&button)
            // 2 行目だけは行として選べない (中のボタンだけを使う行)。
            .selectable(index != 1))
    });

    let native = table.native_table();
    let cell = native
        .viewAtColumn_row_makeIfNecessary(1, 0, true)
        .expect("ウィジェットのセルが作られること");
    let button = cell
        .subviews()
        .iter()
        .find_map(|view| view.downcast_ref::<NSButton>().map(|b| b.retain()))
        .expect("セルの中がボタンであること");
    assert_eq!(button.title().to_string(), "開く");
    unsafe { button.performClick(None) };
    assert_eq!(pressed.get(), 1, "セルのボタンがそのまま押せること");

    // 組み立て済みの行は、選べるかどうかもそのとおりになる。
    let _ = native.viewAtColumn_row_makeIfNecessary(0, 1, true);
    table.set_selection(&[0, 1]);
    assert_eq!(table.selection(), vec![0], "選べない行は落ちること");
    Ok(())
}

/// The row height can be set. Passing 0 or less returns to the default.
fn table_row_height_follows_the_setting(ui: &Ui) -> Result<()> {
    let table = ui.table()?;
    table.set_columns(&TableColumn::list(["都市"]));
    table.set_rows(&TableRow::list([["東京"], ["大阪"]]));
    let default = table.row_height();
    assert!(default > 0.0, "既定の行の高さが決まっていること");

    table.set_row_height(64.0);
    assert_eq!(table.row_height(), 64.0);
    assert_eq!(table.native_table().rowHeight(), 64.0);

    table.set_row_height(0.0);
    assert_eq!(table.row_height(), default, "0 以下は既定へ戻すこと");
    Ok(())
}

/// Even in tables with many rows, or with built rows, sorting by heading still
/// works.
///
/// Sorting is the app's job, so this checks that what is passed again with
/// `set_rows` / `refresh` after the notification is reflected in the rows on
/// screen.
fn table_sorting_works_while_windowed(ui: &Ui) -> Result<()> {
    const ROWS: usize = 50_000;
    let table = ui.table()?;
    table.set_columns(&[
        TableColumn::new("番号").sortable(true),
        TableColumn::new("名前"),
    ]);

    // 昇順・降順を切り替えるだけの並べ替え。
    let descending = Rc::new(Cell::new(false));
    let row_text = |index: usize, descending: bool| match descending {
        true => ROWS - 1 - index,
        false => index,
    };
    table.set_row_builder(ROWS, {
        let descending = descending.clone();
        move |index| {
            let value = row_text(index, descending.get());
            Ok(TableCells::new()
                .text(value.to_string())
                .text(format!("項目 {value}")))
        }
    });
    let seen: Rc<RefCell<Vec<(usize, SortOrder)>>> = Rc::new(RefCell::new(Vec::new()));
    table.on_sort({
        let seen = seen.clone();
        let table = table.clone();
        let descending = descending.clone();
        move |column, order| {
            seen.borrow_mut().push((column, order));
            descending.set(!order.is_ascending());
            // 並べ替えたデータを出し直す (行数は変わらないので `refresh`)。
            table.refresh();
        }
    });

    let native = table.native_table();
    let first_cell = || {
        native
            .viewAtColumn_row_makeIfNecessary(0, 0, true)
            .and_then(|cell| cell.downcast::<objc2_app_kit::NSTableCellView>().ok())
            .and_then(|cell| unsafe { cell.textField() })
            .map(|field| field.stringValue().to_string())
            .unwrap_or_default()
    };
    assert_eq!(first_cell(), "0");

    // 利用者が見出しを押したときと同じ経路。
    let prototype = native
        .tableColumns()
        .objectAtIndex(0)
        .sortDescriptorPrototype()
        .expect("押せる列であること");
    let descriptor = objc2_foundation::NSSortDescriptor::sortDescriptorWithKey_ascending(
        prototype.key().as_deref(),
        false,
    );
    native.setSortDescriptors(&objc2_foundation::NSArray::from_retained_slice(&[
        descriptor,
    ]));
    assert_eq!(*seen.borrow(), vec![(0, SortOrder::Descending)]);
    assert_eq!(table.sort(), Some((0, SortOrder::Descending)));
    assert_eq!(
        first_cell(),
        (ROWS - 1).to_string(),
        "並べ替えた結果が画面の行に出ること"
    );

    // 文字だけの行 (`set_rows`) でも同じ。行数は多いままにする。
    let rows: Vec<TableRow> = (0..ROWS)
        .map(|index| TableRow::new([index.to_string(), format!("項目 {index}")]))
        .collect();
    table.set_rows(&rows);
    assert_eq!(first_cell(), "0");
    assert_eq!(
        table.sort(),
        Some((0, SortOrder::Descending)),
        "行を入れ替えても指標は残ること"
    );
    let reversed: Vec<TableRow> = rows.iter().rev().cloned().collect();
    table.set_rows(&reversed);
    assert_eq!(first_cell(), (ROWS - 1).to_string());
    Ok(())
}

/// Even with built rows, whether a row can be selected is answered without creating
/// it.
///
/// Until the function is given, "rows not yet built can be selected" is assumed, so
/// a `set_selection` pointing at rows off screen stays as is.
fn table_row_selectable_answers_without_building(ui: &Ui) -> Result<()> {
    const ROWS: usize = 10_000;
    const LOCKED: usize = 5_000;
    let built = Rc::new(Cell::new(0usize));

    let table = ui.table()?;
    table.set_columns(&TableColumn::list(["番号"]));
    table.set_row_builder(ROWS, {
        let built = built.clone();
        move |index| {
            built.set(built.get() + 1);
            Ok(TableCells::new()
                .text(index.to_string())
                .selectable(index != LOCKED))
        }
    });

    // 述語が無いと、まだ作っていない行は選べる扱いになる。
    table.set_selection(&[LOCKED]);
    assert_eq!(table.selection(), vec![LOCKED], "述語が無ければ選べる扱い");

    // 述語を渡すと、行を組み立てずに落とせる。
    table.clear_selection();
    table.set_row_selectable(|index| index != LOCKED);
    table.set_selection(&[LOCKED]);
    assert!(
        table.selection().is_empty(),
        "選べない行は取り除かれること: {:?}",
        table.selection()
    );
    // 選べる行はそのまま通る。
    table.set_selection(&[LOCKED - 1]);
    assert_eq!(table.selection(), vec![LOCKED - 1]);
    assert_eq!(built.get(), 0, "選択のために行を組み立てないこと");

    // AppKit 側にも「この行は選べない」と伝わる。
    let native = table.native_table();
    let delegate = unsafe { native.delegate() }.expect("デリゲートがあること");
    assert!(!delegate.tableView_shouldSelectRow(&native, LOCKED as isize));
    assert!(delegate.tableView_shouldSelectRow(&native, (LOCKED - 1) as isize));
    Ok(())
}

/// The selection round-trips by row index, and notifications arrive as they are.
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

/// Rows that cannot be selected can be selected neither by naui nor by AppKit.
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

/// Replacing only the columns keeps the rows' contents and just rearranges them.
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

/// When the selection changes on the AppKit side, it reaches the closure as is.
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

/// When the selection changes on the AppKit side, it reaches the closure as is.
///
/// `select` is the path where naui sends the notification, so here the
/// `NSTableView` is moved directly without going through naui, checking the
/// notification via the delegate.
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

/// Touching the same list from inside the notification does not break it, AppKit
/// included.
///
/// Rebuilding the rows inside `on_select` runs `reloadData` while AppKit is in the
/// middle of calling the delegate. The Gallery's "change the display to match the
/// selection" is exactly this, so it is actually done to check.
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

/// A row's text sits at the vertical center of the row's height.
///
/// If a text-only `NSTextField` were the row, the text height (around 16pt) would
/// fall short of the row height (24pt in the Inset style), and AppKit would draw it
/// top-aligned. The selection highlight fills the row, so the highlight and the text
/// would be misaligned.
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

/// Only rows with `detail` become two lines and correspondingly taller.
///
/// Collects the `NSTextField`s inside a view in order, descending through nesting.
///
/// Row contents are built from naui's `Stack` and `Label`, so the text is not
/// directly under the cell. Only the number and order of the text matter here, so
/// the depth does not.
fn text_fields(view: &NSView) -> Vec<Retained<NSTextField>> {
    let mut found = Vec::new();
    for subview in view.subviews().iter() {
        match subview.downcast::<NSTextField>() {
            Ok(field) => found.push(field),
            Err(other) => found.extend(text_fields(&other)),
        }
    }
    found
}

/// AppKit (`usesAutomaticRowHeights`) decides the row height, so naui only places
/// constraints. This checks "taller than one line" rather than a height value.
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
    let plain_fields = text_fields(&plain);
    let detailed_fields = text_fields(&detailed);
    assert_eq!(plain_fields.len(), 1, "detail が無い行は文字 1 本");
    assert_eq!(detailed_fields.len(), 2, "detail がある行は文字 2 本");

    let plain_height = table.rectOfRow(0).size.height;
    let detailed_height = table.rectOfRow(1).size.height;
    assert!(
        detailed_height > plain_height,
        "2 行になった行のほうが高いこと: {detailed_height} <= {plain_height}"
    );

    // 補助の文字は本文の下に来る。y の向きは入れ物しだいなので、
    // 文字が乗っている親の向きで見分ける。
    let title = detailed_fields[0].frame();
    let sub = detailed_fields[1].frame();
    // SAFETY: 行のビューは組み立て済みで、親をたどるだけ。
    let holder = unsafe { detailed_fields[0].superview() }.expect("文字は入れ物に乗っている");
    let title_is_above = if holder.isFlipped() {
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

/// A row's text spreads to the full width of the row, and overflow is cut off with
/// an ellipsis.
///
/// Without a decided width, `NSTextField` keeps demanding its natural width, and a
/// long label sticks out of the row. This checks with the real frame that the
/// `Sizing` passed to the `Stack` holding the text takes effect.
fn list_long_text_stays_inside_the_row(ui: &Ui) -> Result<()> {
    let list = ui.list()?;
    let long = "これは行の幅にはとても収まらない、ずいぶん長いラベルの文字列です";
    list.set_items(&[ListItem::new(long).detail(long)]);
    list.set_sizing(Sizing::fixed(200.0, 120.0));
    let stack = ui.stack(Orientation::Vertical)?;
    stack.append(&list);
    let root = stack.native_view();
    root.setFrameSize(NSSize::new(400.0, 300.0));
    root.layoutSubtreeIfNeeded();
    root.layoutSubtreeIfNeeded();

    let table = list.native_table();
    let row = table
        .viewAtColumn_row_makeIfNecessary(0, 0, true)
        .expect("1 行目のビュー");
    let row_width = row.frame().size.width;
    assert!(row_width > 0.0, "行に幅が配られていること");

    for (index, field) in text_fields(&row).into_iter().enumerate() {
        let width = field.frame().size.width;
        // NSTextField は左右に 2pt の差し込みを持つので、そのぶんだけ大きく出る
        // (折り返しのテストと同じ)。幅が配られていなければ、自然な幅
        // (この文字列なら数百 pt) を要求したままになる。
        assert!(
            width <= row_width + 6.0,
            "{index} 本目の文字が行からはみ出している: 文字 {width} / 行 {row_width}"
        );
        // 幅いっぱいまでは使う (自然な幅のまま縮こまっていない)。
        assert!(
            width > row_width * 0.5,
            "{index} 本目の文字が行の幅を使っていない: 文字 {width} / 行 {row_width}"
        );
    }
    Ok(())
}

/// Rows that cannot be selected draw both the main and secondary text in the
/// disabled color.
///
/// `NSTableView` has no notion of disabled rows, so it can only be expressed by
/// color. The secondary text's role is `Secondary`, but **in rows that cannot be
/// selected the disabled color wins over it**. Applying them in the wrong order
/// would bring only the secondary text back to a normal color.
fn list_disabled_row_dims_both_lines(ui: &Ui) -> Result<()> {
    let list = ui.list()?;
    list.set_items(&[
        ListItem::new("東京").detail("13,960,000 人"),
        ListItem::new("大阪").detail("2,750,000 人").enabled(false),
    ]);
    list.set_sizing(Sizing::fixed(240.0, 160.0));
    let stack = ui.stack(Orientation::Vertical)?;
    stack.append(&list);
    let root = stack.native_view();
    root.setFrameSize(NSSize::new(400.0, 300.0));
    root.layoutSubtreeIfNeeded();

    let table = list.native_table();
    let color_of = |row: isize| -> Vec<Retained<NSColor>> {
        let view = table
            .viewAtColumn_row_makeIfNecessary(0, row, true)
            .expect("行のビュー");
        text_fields(&view)
            .into_iter()
            .map(|field| field.textColor().expect("色が決まっていること"))
            .collect()
    };

    // 選べる行は、本文と補助でそれぞれの役割の色になる。
    let enabled = color_of(0);
    assert_eq!(enabled.len(), 2, "文字が 2 本あること");
    assert_eq!(enabled[0], NSColor::labelColor(), "本文は labelColor");
    assert_eq!(
        enabled[1],
        NSColor::secondaryLabelColor(),
        "補助は secondaryLabelColor"
    );

    // 選べない行は、どちらも無効な色になる。
    let disabled = color_of(1);
    assert_eq!(disabled.len(), 2, "文字が 2 本あること");
    for (index, color) in disabled.iter().enumerate() {
        assert_eq!(
            color,
            &NSColor::disabledControlTextColor(),
            "{index} 本目が無効な色になっていない"
        );
    }
    Ok(())
}

/// Items and separators become the NSMenu's contents as is.
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

/// Choosing an item delivers its position in the sequence including separators to
/// the closure.
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
        move |path| seen.borrow_mut().push(path.to_vec())
    });

    popup.select(&[2]);
    assert_eq!(
        *seen.borrow(),
        vec![vec![2]],
        "区切り線を数えた位置で届くこと"
    );

    // 区切り線・選べない項目・範囲外は通知しない。
    popup.select(&[1]);
    popup.select(&[3]);
    popup.select(&[9]);
    popup.select(&[]);
    assert_eq!(*seen.borrow(), vec![vec![2]]);
    Ok(())
}

/// A submenu becomes an `NSMenuItem` holding another `NSMenu`, and choosing a check
/// item flips its `state` before the notification.
fn popup_menu_submenus_and_checks(ui: &Ui) -> Result<()> {
    let popup = ui.popup_menu()?;
    popup.set_items(&[
        PopupItem::new("折り返す").checked(true),
        PopupItem::separator(),
        PopupItem::submenu(
            "並べ替え",
            [
                PopupItem::new("名前"),
                PopupItem::new("日付").checked(false),
            ],
        ),
        PopupItem::submenu("閉じた", ["中"]).enabled(false),
    ]);
    assert_eq!(popup.len(), 4, "上の段だけを数える");

    let wrap = popup.native_item(&[0]).expect("項目");
    assert_eq!(wrap.state(), NSControlStateValueOn);
    let sort = popup.native_item(&[2]).expect("サブメニューの見出し");
    let submenu = sort.submenu().expect("NSMenu を持つこと");
    assert_eq!(submenu.numberOfItems(), 2);
    assert!(!submenu.autoenablesItems());
    assert_eq!(
        popup.native_item(&[2, 1]).expect("項目").state(),
        NSControlStateValueOff
    );
    assert!(
        !popup.native_item(&[3, 0]).expect("項目").isEnabled(),
        "押せないサブメニューの中も止める"
    );

    let seen = Rc::new(RefCell::new(Vec::new()));
    popup.on_select({
        let seen = seen.clone();
        let popup = popup.clone();
        move |path| {
            seen.borrow_mut()
                .push((path.to_vec(), popup.is_item_checked(path)))
        }
    });

    popup.select(&[2, 1]);
    popup.select(&[0]);
    popup.select(&[2, 0]);
    assert_eq!(
        *seen.borrow(),
        vec![(vec![2, 1], true), (vec![0], false), (vec![2, 0], false)],
        "印は通知の前に反転している"
    );
    assert_eq!(wrap.state(), NSControlStateValueOff);
    assert_eq!(
        popup.native_item(&[2, 1]).expect("項目").state(),
        NSControlStateValueOn
    );

    // 見出し・押せないサブメニューの中は選べない。
    popup.select(&[2]);
    popup.select(&[3, 0]);
    assert_eq!(seen.borrow().len(), 3);

    popup.set_item_checked(&[0], true);
    assert!(popup.is_item_checked(&[0]));
    assert_eq!(wrap.state(), NSControlStateValueOn);
    assert_eq!(seen.borrow().len(), 3, "set_item_checked は通知しない");
    // 印の無い項目には付かない。
    popup.set_item_checked(&[2, 0], true);
    assert!(!popup.is_item_checked(&[2, 0]));
    Ok(())
}

/// The view of the widget it is attached to comes to hold the menu.
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

/// The title, body text and content widget are passed to the `NSAlert` as is.
///
/// `open()` enters `runModal` and never returns, so the test only checks the
/// building with `native_alert()` (without showing it).
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

/// The macOS order (from the left: secondary, cancel, primary).
///
/// `NSAlert` makes the button added first the rightmost and the default, so they
/// are added in the order primary, cancel, secondary.
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

/// With no buttons specified, only "OK" is shown.
fn dialog_without_buttons_shows_ok(ui: &Ui) -> Result<()> {
    let dialog = ui.dialog("完了しました")?;
    assert!(dialog.buttons().is_empty(), "既定ではボタンを持たないこと");

    let alert = dialog.native_alert();
    let titles: Vec<String> = alert
        .buttons()
        .iter()
        .map(|button| button.title().to_string())
        .collect();
    assert_eq!(titles, [naui_core::ui_strings().ok]);
    Ok(())
}

/// `close()` on a dialog that is not shown breaks nothing.
///
/// `close()` aborts the modal, so calling it when nothing is shown could make
/// AppKit throw. This checks that it is not called.
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

/// The toast is overlaid on the window's `contentView` and removed when dismissed.
/// A window with a button to open popovers from, shown on screen (`NSPopover` does
/// nothing for views in windows that are not).
fn popover_window(ui: &Ui, title: &str) -> Result<(naui_macos::Window, naui_macos::Button)> {
    let window = ui.window(title, 360.0, 320.0)?;
    let root = ui.stack(Orientation::Vertical)?;
    root.set_padding(Padding::all(120.0));
    let button = ui.button("設定")?;
    root.append(&button);
    window.set_child(&root);
    window.show();
    window.native_window().layoutIfNeeded();
    Ok((window, button))
}

/// The screen frame of the window `NSPopover` shows its content in.
fn popover_frame(popover: &naui_macos::Popover) -> Option<NSRect> {
    let controller = popover.native_popover().contentViewController()?;
    controller.view().window().map(|window| window.frame())
}

fn popover_shows_its_content_beside_the_widget(ui: &Ui) -> Result<()> {
    let (window, button) = popover_window(ui, "ポップオーバー")?;
    let popover = ui.popover()?;
    let body = ui.label("中身")?;
    popover.set_child(&body);
    let closed = Rc::new(Cell::new(0));
    popover.on_close({
        let closed = closed.clone();
        move || closed.set(closed.get() + 1)
    });
    assert!(!popover.is_open(), "作っただけでは閉じていること");

    popover.open(&button);
    assert!(popover.is_open());
    let native = popover.native_popover();
    assert!(native.isShown(), "NSPopover が出ていること");
    let content = native
        .contentViewController()
        .expect("中身のビューコントローラがあること")
        .view();
    assert!(
        contains_subview(&content, &body.native_view()),
        "中身がポップオーバーの中に入ること"
    );
    content.layoutSubtreeIfNeeded();
    // 制約は揃え用の矩形どうしで張られる (ラベルの frame はそれより左右に広い)。
    let size = content.frame().size;
    let view = body.native_view();
    let label = view.alignmentRectForFrame(view.frame());
    assert!(
        (label.origin.x - 12.0).abs() < 1.0
            && (size.width - label.size.width - 24.0).abs() < 1.0
            && (size.height - label.size.height - 24.0).abs() < 1.0,
        "中身の周りに余白を取ること: {size:?} / {label:?}"
    );

    popover.close();
    assert!(!popover.is_open());
    pump_until(2.0, || !native.isShown());
    assert!(!native.isShown(), "NSPopover が閉じること");
    assert_eq!(closed.get(), 0, "アプリが閉じたときは通知しないこと");
    window.close();
    Ok(())
}

fn popover_dismissed_by_the_user_notifies(ui: &Ui) -> Result<()> {
    let (window, button) = popover_window(ui, "ポップオーバー")?;
    let popover = ui.popover()?;
    popover.set_child(&ui.label("中身")?);
    let closed = Rc::new(Cell::new(0));
    popover.on_close({
        let closed = closed.clone();
        move || closed.set(closed.get() + 1)
    });

    popover.open(&button);
    assert!(popover.is_open());
    popover.dismiss();
    pump_until(2.0, || closed.get() > 0);
    assert_eq!(closed.get(), 1, "利用者が閉じたら 1 度だけ通知すること");
    assert!(!popover.is_open());

    // 閉じてから開き直せること。
    popover.open(&button);
    assert!(popover.is_open());
    popover.close();
    pump(0.3);
    assert_eq!(closed.get(), 1);
    window.close();
    Ok(())
}

fn popover_follows_the_position(ui: &Ui) -> Result<()> {
    let (window, button) = popover_window(ui, "ポップオーバーの向き")?;
    let view = button.native_view();
    let on_screen = window
        .native_window()
        .convertRectToScreen(view.convertRect_toView(view.bounds(), None));
    let popover = ui.popover()?;
    popover.set_child(&ui.label("中身")?);
    // 開いて動きが落ち着いてから、出た窓の枠を読む。
    let frame_at = |position| {
        popover.set_position(position);
        popover.open(&button);
        pump(0.3);
        let frame = popover_frame(&popover).expect("ポップオーバーの窓があること");
        popover.close();
        pump(0.3);
        frame
    };
    assert_eq!(popover.position(), naui_core::PopoverPosition::Bottom);

    let mid_x = |r: NSRect| r.origin.x + r.size.width / 2.0;
    let mid_y = |r: NSRect| r.origin.y + r.size.height / 2.0;
    // 画面の座標は上が大きい。
    let below = frame_at(naui_core::PopoverPosition::Bottom);
    assert!(
        mid_y(below) < mid_y(on_screen),
        "下に出ること: {below:?} / {on_screen:?}"
    );
    let above = frame_at(naui_core::PopoverPosition::Top);
    assert!(
        mid_y(above) > mid_y(on_screen),
        "上に出ること: {above:?} / {on_screen:?}"
    );
    let left = frame_at(naui_core::PopoverPosition::Left);
    assert!(
        mid_x(left) < mid_x(on_screen),
        "左に出ること: {left:?} / {on_screen:?}"
    );
    let right = frame_at(naui_core::PopoverPosition::Right);
    assert!(
        mid_x(right) > mid_x(on_screen),
        "右に出ること: {right:?} / {on_screen:?}"
    );
    window.close();
    Ok(())
}

fn popover_needs_a_shown_widget(ui: &Ui) -> Result<()> {
    let popover = ui.popover()?;
    popover.set_child(&ui.label("中身")?);
    let loose = ui.button("どこにもない")?;
    popover.open(&loose);
    assert!(
        !popover.is_open(),
        "ウィンドウに無いウィジェットには開かないこと"
    );

    let window = ui.window("出していない", 200.0, 120.0)?;
    let hidden = ui.button("隠れている")?;
    window.set_child(&hidden);
    popover.open(&hidden);
    assert!(
        !popover.is_open(),
        "画面に出ていないウィンドウでは開かないこと"
    );
    popover.close();
    window.close();
    Ok(())
}

fn popover_gives_width_to_content_without_one(ui: &Ui) -> Result<()> {
    let (window, button) = popover_window(ui, "ポップオーバーの幅")?;
    let popover = ui.popover()?;
    let field = ui.text_input("")?;
    popover.set_child(&field);
    popover.open(&button);
    let content = popover
        .native_popover()
        .contentViewController()
        .expect("中身のビューコントローラがあること")
        .view();
    content.layoutSubtreeIfNeeded();
    let width = field.native_view().frame().size.width;
    assert!(width >= 200.0, "入力欄に幅があること: {width}");
    popover.close();
    window.close();
    Ok(())
}

fn popover_grows_with_its_content(ui: &Ui) -> Result<()> {
    let (window, button) = popover_window(ui, "ポップオーバーの伸び")?;
    let popover = ui.popover()?;
    let expander = ui.expander("詳細")?;
    let body = ui.label("一行目\n二行目\n三行目\n四行目")?;
    expander.set_child(&body);
    popover.set_child(&expander);
    popover.open(&button);
    pump(0.3);
    let before = popover_frame(&popover).expect("ポップオーバーの窓があること");

    expander.set_expanded(true);
    pump_until(2.0, || {
        popover_frame(&popover).is_some_and(|f| f.size.height > before.size.height + 20.0)
    });
    let after = popover_frame(&popover).expect("ポップオーバーの窓があること");
    assert!(
        after.size.height > before.size.height + 20.0,
        "開いた中身のぶん高くなること: {before:?} -> {after:?}"
    );
    popover.close();
    window.close();
    Ok(())
}

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

/// Clicking the action button calls `on_action` and `on_dismiss`, and it
/// disappears.
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

/// A new toast replaces the previous one, and the replaced one does not notify.
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

/// After the specified time passes, the toast disappears by itself and
/// `on_dismiss` arrives.
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

/// Whether the view is overlaid as a toast. Identified by the name naui gave it.
fn is_toast_view(view: &NSView) -> bool {
    view.identifier()
        .is_some_and(|id| id.to_string() == "naui.toast")
}

/// Whether `view` is among `parent`'s children.
fn contains_subview(parent: &NSView, view: &NSView) -> bool {
    parent
        .subviews()
        .iter()
        .any(|sub| std::ptr::eq(&*sub as *const NSView, view as *const NSView))
}

/// The number of toasts overlaid on `parent`.
fn count_toasts(parent: &NSView) -> usize {
    parent
        .subviews()
        .iter()
        .filter(|sub| is_toast_view(sub))
        .count()
}

/// The text laid out in the toast.
fn toast_labels(view: &NSView) -> Vec<String> {
    toast_row(view)
        .iter()
        .filter_map(|item| {
            item.downcast_ref::<NSTextField>()
                .map(|f| f.stringValue().to_string())
        })
        .collect()
}

/// The toast's action button. `None` if there is none.
fn toast_button(view: &NSView) -> Option<Retained<NSButton>> {
    toast_row(view)
        .iter()
        .find_map(|item| item.downcast_ref::<NSButton>().map(|b| b.retain()))
}

/// The row laying out the text and the button inside the toast.
///
/// They are nested in the order: overlaid view (shadow) → background (rounded
/// corners) → row.
fn toast_row(view: &NSView) -> Retained<objc2_foundation::NSArray<NSView>> {
    let surface = view.subviews().objectAtIndex(0);
    let row = surface.subviews().objectAtIndex(0);
    row.subviews()
}

/// Items are laid out in the `NSToolbar`, and separators map to AppKit's space
/// items.
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

/// The clicked index arrives as the position in the sequence including separators.
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

/// The toolbar attaches to the `NSWindow` and disappears when detached.
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

/// Rebuilding the toolbar from inside the notification does not double-borrow.
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

/// A wrong symbol name makes `NSImage` nil, leaving just that item without a
/// picture.
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

/// A tree is laid out as NSOutlineView rows, and expanding adds the children's
/// rows.
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

/// A tree's selection round-trips with the NSOutlineView, and `set_selected` does
/// not notify.
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

/// A branch that cannot be selected makes all its descendants unselectable too.
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

/// Expansion notifications arrive both for native operations and for naui's
/// `expand`.
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

/// Item contents become NSTextFields that AppKit had the delegate create.
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
    assert_eq!(
        text_fields(cell.as_ref()).len(),
        2,
        "補助の文字が 2 行目に出ること"
    );

    // 子の行も同じように作られる。
    let child = outline
        .viewAtColumn_row_makeIfNecessary(0, 1, true)
        .expect("2 行目のビューが作られること")
        .downcast::<objc2_app_kit::NSTableCellView>()
        .expect("行は NSTableCellView であること");
    let field = unsafe { child.textField() }.expect("行に文字が入っていること");
    assert_eq!(field.stringValue().to_string(), "main.rs");

    // 高さは AppKit の自動 (`usesAutomaticRowHeights`) ではなく naui が答える。
    // 自動にすると、閉じた枝を開き直したときの復元が親の開閉のデリゲートの
    // 中で走り、AppKit に「再入した」と警告される。
    assert!(
        !outline.usesAutomaticRowHeights(),
        "行の高さを AppKit に測らせていないこと"
    );
    let two_lines = outline.rectOfRow(0).size.height;
    let one_line = outline.rectOfRow(1).size.height;
    assert!(one_line > 0.0, "1 行の項目に高さがあること");
    assert!(
        two_lines > one_line,
        "補助のある項目のほうが高いこと: {two_lines} / {one_line}"
    );
    assert!(
        two_lines + 0.5 >= cell.fittingSize().height,
        "2 行の中身が収まる高さであること: {two_lines} / {}",
        cell.fittingSize().height
    );
    Ok(())
}

/// Touching the same tree from inside the notification does not break it (the same
/// shape as lists).
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

/// The tree used in tests: src (two leaves) and docs (guide > intro).
fn sample_tree() -> Vec<TreeItem> {
    vec![
        TreeItem::new("src").children([TreeItem::new("main.rs"), TreeItem::new("lib.rs")]),
        TreeItem::new("docs").child(TreeItem::new("guide").child(TreeItem::new("intro.md"))),
    ]
}

/// Even when the parent is collapsed, the expansion inside it is remembered and
/// restored when expanded again.
///
/// This matches AppKit's behavior (the same as Finder). The notifications AppKit
/// sends again on re-expanding are not passed on, since the state as seen by naui
/// has not changed.
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

/// Imitates editing on the native side.
///
/// `NSDatePicker` has nothing like `performClick`, so "write the value, then send
/// the action", which AppKit does for user actions, is done by hand.
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

/// The same path as turning the time picker's spinner. The date is set to the same
/// 1970-01-01 naui uses.
fn edit_time_natively(picker: &NSDatePicker, value: Time) {
    let (year, month, day) = DateTime::TIME_ORIGIN;
    edit_natively(
        picker,
        DateTime::new(year, month, day, value.hour, value.minute),
    );
}

/// Types into the digit field. AppKit sends `controlTextDidChange:` during editing,
/// so the test passes the same notification to the delegate directly.
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

/// Presses Enter in the search field (the same path as the `insertNewline:` AppKit
/// sends to the field editor being edited).
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

/// Chooses a suggestion from the list. In addition to the notification
/// `selectItemAtIndex:` sends, the same notification is passed to the delegate
/// directly in case it does not arrive (if it arrives twice it is merged into one).
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

/// Confirms the field (the same path as Enter or leaving the field).
fn commit_field(control: &NSTextField) {
    let action = control.action();
    let target = control.target();
    unsafe { control.sendAction_to(action, target.as_deref()) };
}

/// Moves the up/down buttons (`NSStepper` moves the value, then sends the action).
fn step_natively(stepper: &NSStepper, value: f64) {
    stepper.setDoubleValue(value);
    let action = stepper.action();
    let target = stepper.target();
    unsafe { stepper.sendAction_to(action, target.as_deref()) };
}

/// Number input writes the value into both the field and the up/down buttons, and
/// notifies button operations.
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

/// Decimal places, step and range apply both to the `NSStepper` and to naui's
/// value.
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

/// While typing it only notifies; on confirmation the display matches the value.
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

/// Password input is an `NSSecureTextField` itself, and text round-trips.
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

/// Search input is an `NSSearchField` itself, and text round-trips.
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

/// The confirmation notification comes only on Enter, not while typing.
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

/// Single-line input and password input also report confirmation only on Enter,
/// like the search field.
fn text_inputs_notify_on_return(ui: &Ui) -> Result<()> {
    let input = ui.text_input("")?;
    let password = ui.password_input()?;
    let input_view = input.native_view();
    let password_view = password.native_view();
    let input_field = input_view
        .downcast_ref::<NSTextField>()
        .expect("1 行入力の欄であること");
    let password_field: &NSTextField = password_view
        .downcast_ref::<NSSecureTextField>()
        .expect("伏せ字の欄であること");

    let typed = Rc::new(RefCell::new(Vec::new()));
    let submitted = Rc::new(RefCell::new(Vec::new()));
    input.on_change({
        let typed = typed.clone();
        move |text| typed.borrow_mut().push(text.to_string())
    });
    input.on_submit({
        let submitted = submitted.clone();
        move |text| submitted.borrow_mut().push(format!("入力:{text}"))
    });
    password.on_submit({
        let submitted = submitted.clone();
        move |text| submitted.borrow_mut().push(format!("伏せ字:{text}"))
    });

    type_into_field(input_field, "ねこ");
    assert_eq!(*typed.borrow(), vec!["ねこ".to_string()]);
    assert!(submitted.borrow().is_empty(), "打つだけでは確定しない");

    press_return_in_field(input_field);
    type_into_field(password_field, "ひみつ");
    press_return_in_field(password_field);
    assert_eq!(
        *submitted.borrow(),
        vec!["入力:ねこ".to_string(), "伏せ字:ひみつ".to_string()]
    );
    assert_eq!(typed.borrow().len(), 1, "確定は打鍵の通知を増やさない");

    // 確定は何度でも起き、通知の中から欄を空けられる。
    input.on_submit({
        let submitted = submitted.clone();
        let input = input.clone();
        move |text| {
            submitted.borrow_mut().push(text.to_string());
            input.set_text("");
        }
    });
    type_into_field(input_field, "いぬ");
    press_return_in_field(input_field);
    assert_eq!(submitted.borrow().last().map(String::as_str), Some("いぬ"));
    assert_eq!(input.text(), "");
    Ok(())
}

/// While read-only it cannot be edited, so it does not confirm either.
fn read_only_text_input_does_not_submit(ui: &Ui) -> Result<()> {
    let input = ui.text_input("ログ")?;
    let view = input.native_view();
    let field = view
        .downcast_ref::<NSTextField>()
        .expect("1 行入力の欄であること");
    let submitted = Rc::new(Cell::new(0));
    input.on_submit({
        let submitted = submitted.clone();
        move |_| submitted.set(submitted.get() + 1)
    });

    input.set_read_only(true);
    press_return_in_field(field);
    assert_eq!(submitted.get(), 0, "読み取り専用なら確定しない");

    input.set_read_only(false);
    press_return_in_field(field);
    assert_eq!(submitted.get(), 1, "書き換えられるようになれば確定する");
    Ok(())
}

/// Enter reads what has been typed into the value first, then reports that value.
fn number_input_commits_before_submitting(ui: &Ui) -> Result<()> {
    let number = ui.number_input(5.0)?;
    number.set_range(None, Some(100.0));
    let field = number.native_field();

    let log = Rc::new(RefCell::new(Vec::new()));
    number.on_change({
        let log = log.clone();
        move |value| log.borrow_mut().push(format!("変更:{value}"))
    });
    number.on_submit({
        let log = log.clone();
        move |value| log.borrow_mut().push(format!("確定:{value}"))
    });

    // 読める表示は打った時点で値になっているので、確定はその値で知らせる。
    type_into_field(&field, "12");
    press_return_in_field(&field);
    assert_eq!(*log.borrow(), vec!["変更:12", "確定:12"]);

    // 範囲の外は端へ寄せ、表示も直してから知らせる。
    type_into_field(&field, "999");
    press_return_in_field(&field);
    assert_eq!(field.stringValue().to_string(), "100");
    assert_eq!(log.borrow()[2..], ["変更:100", "確定:100"]);

    // 読めない表示は元の値へ戻してから、その値で知らせる。
    type_into_field(&field, "1x");
    press_return_in_field(&field);
    assert_eq!(field.stringValue().to_string(), "100", "元の値へ戻す");
    assert_eq!(log.borrow()[4..], ["確定:100"]);
    Ok(())
}

/// `datePickerElements` changes with each kind.
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

/// Both directions: writing the value, and editing on the native side.
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

/// A date-only display keeps the time, and a time-only display keeps the date.
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

/// It never goes outside the minimum and maximum.
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

/// In the middle of a notification the same picker can be operated on, and the
/// handler replaced too.
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

/// The time picker is an `NSDatePicker` showing only hours and minutes. The value
/// round-trips with the native side, and `set_value` does not notify.
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

/// It never goes outside the minimum and maximum. The date is fixed to 1970-01-01,
/// so the range can be passed to `NSDatePicker` as is.
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

/// As in the Gallery, putting tall content in a scroll view inside a tab lets it
/// scroll to the bottom (instead of being cut off at the window's height).
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

/// Even when the scroller takes up space, the content's width fits within "the
/// visible width".
///
/// Overlay scrollers just sit on top of the content and take no space, but the
/// "always show" setting, or environments without a pointing device, choose the
/// style that takes space (legacy). If the content then overflowed by the
/// scroller's width, it could never be seen when horizontal scrolling is off.
///
/// To check this regardless of the user's settings, the style is set right here.
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

/// In a multi-column grid, a `Fill` child wants only its cell's share.
///
/// If it wanted the whole grid's width, it could squash the contents of other cells
/// in the same row (those whose compression resistance is weaker than this wish).
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

/// Columns with a fixed width are not eaten by `Fill` children, even when their
/// content is short.
///
/// Estimating the share from content widths alone would give extra width to the
/// `Fill` column when what sits in a fixed-width column is short, overlapping the
/// columns after it.
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

/// In an `Auto | FILL | Auto` grid, only the `Fill` column takes the leftover and the
/// `Auto` columns keep the width of their content.
///
/// NSGridView has no rule for which column receives the leftover, so after a
/// relayout (switching the theme and so on) the left `Auto` column could widen and
/// push the second column to the right. Without a `Fill` child the whole leftover went
/// to the left column, and even with one (such as a slider) the left column widened.
fn grid_auto_column_keeps_content_width(ui: &Ui) -> Result<()> {
    for with_slider in [false, true] {
        let grid = ui.grid()?;
        grid.set_column_track(0, Track::Auto);
        grid.set_column_track(1, Track::FILL);
        grid.set_column_track(2, Track::Auto);
        grid.set_spacing(12.0, 8.0);
        grid.set_sizing(Sizing::fill_width());

        // 「名前 | ウィジェット | 値」の表。値の無い行はウィジェットが 2 列へまたがる。
        let mut labels = Vec::new();
        let mut second = Vec::new();
        let name = ui.label("ボタン")?;
        let buttons = ui.stack(Orientation::Horizontal)?;
        buttons.set_spacing(8.0);
        buttons.append(&ui.button("押す")?);
        buttons.append(&ui.button("取り消す")?);
        grid.attach(&name, GridCell::new(0, 0));
        grid.attach(&buttons, GridCell::new(1, 0).span(2, 1));
        labels.push(name.native_view());
        second.push(buttons.native_view());
        let other = ui.label("リンク")?;
        let link = ui.button("開く")?;
        grid.attach(&other, GridCell::new(0, 1));
        grid.attach(&link, GridCell::new(1, 1).span(2, 1));
        labels.push(other.native_view());
        second.push(link.native_view());
        let mut value = None;
        if with_slider {
            let name = ui.label("音量")?;
            let slider = ui.slider(0.0, 100.0)?;
            slider.set_sizing(Sizing::fill_width());
            let shown = ui.label("50")?;
            grid.attach(&name, GridCell::new(0, 2));
            grid.attach(&slider, GridCell::new(1, 2));
            grid.attach(&shown, GridCell::new(2, 2));
            labels.push(name.native_view());
            second.push(slider.native_view());
            value = Some((slider.native_view(), shown.native_view()));
        }

        let root = ui.stack(Orientation::Vertical)?;
        root.set_padding(Padding::all(16.0));
        root.append(&grid);
        let window = ui.window("grid auto column", 640.0, 240.0)?;
        window.set_child(&root);
        window.show();
        let native_window = window.native_window();
        let content = native_window
            .contentView()
            .expect("コンテンツビューがあること");
        content.layoutSubtreeIfNeeded();
        pump(0.05);

        // 列の幅は枠ではなく、位置合わせの矩形 (ラベルなら左右 2pt 内側) で決まる。
        let alignment = |view: &NSView| view.alignmentRectForFrame(view.frame());
        let label_width = labels
            .iter()
            .map(|view| alignment(view).size.width)
            .fold(0.0, f64::max);
        let expected = label_width + 12.0;
        let starts = || {
            second
                .iter()
                .map(|view| alignment(view).origin.x)
                .collect::<Vec<_>>()
        };
        let initial = starts();

        // 子の大きさが変わったことにして測り直す。
        for view in &second {
            view.invalidateIntrinsicContentSize();
            view.setNeedsLayout(true);
        }
        grid.native_view().setNeedsLayout(true);
        content.layoutSubtreeIfNeeded();
        pump(0.05);
        let relaid = starts();

        // テーマを切り替えて測り直す。
        window.set_theme(Theme::Dark)?;
        pump(0.05);
        content.layoutSubtreeIfNeeded();
        window.set_theme(Theme::Light)?;
        pump(0.05);
        content.layoutSubtreeIfNeeded();
        let themed = starts();
        let grid_width = grid.native_view().frame().size.width;
        let value = value.map(|(slider, shown)| (alignment(&slider), alignment(&shown)));
        window.close();

        for (when, starts) in [
            ("最初のレイアウト", &initial),
            ("再レイアウトのあと", &relaid),
            ("テーマの切り替えのあと", &themed),
        ] {
            assert!(
                starts.iter().all(|x| (x - expected).abs() < 1.0),
                "{when}も 2 列目はラベルの幅 + 列間から始まること \
                 (スライダー: {with_slider}): {starts:?} / {expected}"
            );
        }
        if let Some((slider, shown)) = value {
            // 余りはスライダーが受け取り、値は右端に寄る。
            assert!(
                (slider.origin.x + slider.size.width + 12.0 - shown.origin.x).abs() < 1.0,
                "スライダーが値の手前まで伸びること: {slider:?} / {shown:?}"
            );
            assert!(
                (shown.origin.x + shown.size.width - grid_width).abs() < 1.0,
                "値が右端に寄ること: {shown:?} / {grid_width}"
            );
        }
    }
    Ok(())
}

/// Children placed in `Auto` rows do not take the vertical leftover.
///
/// Without this, children whose container has a low hugging priority, like
/// NSStackView, would absorb the leftover before the `Fill` row, squashing the
/// `Fill` row to its content height (just the headings, for tabs).
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

/// `Fill` rows (and columns) with a capped child and with a plain `Fill` child side
/// by side: when there is room, the capped one still grows up to its maximum and
/// the plain one takes the rest.
///
/// The plain child's "want to grow to fill the grid" wish is far stronger than the
/// capped child's "want to grow up to the maximum". Asking for the whole grid, it
/// squashed the capped track. So, as across columns, it asks only for what the
/// other tracks do not need.
fn grid_capped_and_plain_fill_tracks_share_the_room(ui: &Ui) -> Result<()> {
    for vertical in [true, false] {
        let grid = ui.grid()?;
        grid.set_sizing(Sizing::fill());
        let plain = ui.stack(Orientation::Vertical)?;
        plain.set_sizing(Sizing::fill());
        let capped = ui.stack(Orientation::Vertical)?;
        capped.set_sizing(if vertical {
            Sizing::fill().max_height(100.0)
        } else {
            Sizing::fill().max_width(100.0)
        });
        let second = if vertical {
            grid.set_column_track(0, Track::FILL);
            grid.set_row_track(0, Track::FILL);
            grid.set_row_track(1, Track::FILL);
            GridCell::new(0, 1)
        } else {
            grid.set_row_track(0, Track::FILL);
            grid.set_column_track(0, Track::FILL);
            grid.set_column_track(1, Track::FILL);
            GridCell::new(1, 0)
        };
        grid.attach(&plain, GridCell::new(0, 0));
        grid.attach(&capped, second);

        let root = grid.native_view();
        root.setFrameSize(NSSize::new(400.0, 500.0));
        root.layoutSubtreeIfNeeded();
        let plain_frame = plain.native_view().frame();
        let capped_frame = capped.native_view().frame();
        let (capped_size, plain_size, room) = if vertical {
            (capped_frame.size.height, plain_frame.size.height, 500.0)
        } else {
            (capped_frame.size.width, plain_frame.size.width, 400.0)
        };
        assert_eq!(
            capped_size, 100.0,
            "上限付きは上限まで広がる (縦 {vertical}): {capped_frame:?} / {plain_frame:?}"
        );
        assert!(
            plain_size > room - 150.0,
            "上限なしは残りを受け取る (縦 {vertical}): {capped_frame:?} / {plain_frame:?}"
        );
    }
    Ok(())
}

/// The same tracks in a window (or a vertical `Scroll`) smaller than the maximum: the
/// capped child shrinks to what there is, and the window keeps its size (the
/// `Scroll`'s content stays as tall as its visible area).
///
/// The plain child wished for "the grid − what the other tracks need" with the
/// capped track's whole maximum counted as needed. In a window lower than that it
/// wished for a negative size, and the wish (far stronger than a window keeping its
/// size) grew the window instead. The room kept for a capped track is held to what
/// the grid has.
fn grid_capped_and_plain_fill_tracks_fit_a_small_window(ui: &Ui) -> Result<()> {
    for vertical in [true, false] {
        let grid = ui.grid()?;
        grid.set_sizing(Sizing::fill());
        let plain = ui.stack(Orientation::Vertical)?;
        plain.set_sizing(Sizing::fill());
        let capped = ui.stack(Orientation::Vertical)?;
        capped.set_sizing(if vertical {
            Sizing::fill().max_height(100.0)
        } else {
            Sizing::fill().max_width(100.0)
        });
        let second = if vertical {
            grid.set_column_track(0, Track::FILL);
            grid.set_row_track(0, Track::FILL);
            grid.set_row_track(1, Track::FILL);
            GridCell::new(0, 1)
        } else {
            grid.set_row_track(0, Track::FILL);
            grid.set_column_track(0, Track::FILL);
            grid.set_column_track(1, Track::FILL);
            GridCell::new(1, 0)
        };
        grid.attach(&plain, GridCell::new(0, 0));
        grid.attach(&capped, second);

        let (width, height) = if vertical {
            (400.0, 80.0)
        } else {
            (80.0, 400.0)
        };
        let window = ui.window("狭い", width, height)?;
        window.set_child(&grid);
        window.show();
        let content = window
            .native_window()
            .contentView()
            .expect("コンテンツビューがあること");
        content.layoutSubtreeIfNeeded();
        pump(0.05);
        content.layoutSubtreeIfNeeded();
        let size = content.frame().size;
        let capped_frame = capped.native_view().frame();
        let plain_frame = plain.native_view().frame();
        window.close();
        let (window_size, room, capped_size) = if vertical {
            (size.height, height, capped_frame.size.height)
        } else {
            (size.width, width, capped_frame.size.width)
        };
        assert!(
            (window_size - room).abs() < 0.5,
            "ウィンドウは広がらない (縦 {vertical}): {size:?} / {capped_frame:?} / {plain_frame:?}"
        );
        assert!(
            capped_size <= room + 0.5,
            "上限付きは空いている分まで縮む (縦 {vertical}): {capped_frame:?} / {plain_frame:?}"
        );
    }

    // 縦の Scroll の中でも、中身を伸ばさずに縮む。
    let grid = ui.grid()?;
    grid.set_sizing(Sizing::fill());
    grid.set_column_track(0, Track::FILL);
    grid.set_row_track(0, Track::FILL);
    grid.set_row_track(1, Track::FILL);
    let plain = ui.stack(Orientation::Vertical)?;
    plain.set_sizing(Sizing::fill());
    let capped = ui.stack(Orientation::Vertical)?;
    capped.set_sizing(Sizing::fill().max_height(100.0));
    grid.attach(&plain, GridCell::new(0, 0));
    grid.attach(&capped, GridCell::new(0, 1));
    let scroll = ui.scroll()?;
    scroll.set_policy(ScrollPolicy::Never, ScrollPolicy::Auto);
    scroll.set_child(&grid);
    scroll.set_sizing(Sizing::fill());
    let window = ui.window("狭い Scroll", 400.0, 80.0)?;
    window.set_child(&scroll);
    window.show();
    let content = window
        .native_window()
        .contentView()
        .expect("コンテンツビューがあること");
    content.layoutSubtreeIfNeeded();
    pump(0.05);
    content.layoutSubtreeIfNeeded();
    let metrics = scroll.metrics();
    let capped_frame = capped.native_view().frame();
    window.close();
    assert_eq!(
        metrics.content_height, metrics.viewport_height,
        "Scroll の中身は伸びない: {metrics:?} / {capped_frame:?}"
    );
    assert!(
        capped_frame.size.height < 80.0,
        "上限付きは表示領域に収まるまで縮む: {capped_frame:?}"
    );
    Ok(())
}

/// A `Fill` row takes the leftover even when its child's `Fill` has a maximum. The
/// child grows up to its maximum and sits in the middle of the row, as on GTK and
/// WinUI.
///
/// Placing such a child with `NSGridCellPlacement::Fill` ties the row to the child,
/// so its maximum became the row's maximum. The "want to grow to fill the grid" wish
/// at its usual priority also pulled the grid down to the child.
fn grid_fill_row_takes_the_rest_around_a_capped_child(ui: &Ui) -> Result<()> {
    let grid = ui.grid()?;
    grid.set_column_track(0, Track::FILL);
    grid.set_row_track(0, Track::FILL);
    grid.set_row_track(1, Track::Auto);
    grid.set_sizing(Sizing::fill());
    let capped = ui.stack(Orientation::Vertical)?;
    capped.set_sizing(Sizing::fill().max_width(200.0).max_height(100.0));
    grid.attach(&capped, GridCell::new(0, 0));
    let footer = ui.label("下端")?;
    grid.attach(&footer, GridCell::new(0, 1));

    // gallery の画面と同じく、余りを末尾の受け皿へ回せる縦 Stack に入れる。
    let pane = ui.stack(Orientation::Vertical)?;
    pane.set_align(Align::Start);
    pane.append(&ui.label("見出し")?);
    pane.append(&grid);

    let root = pane.native_view();
    root.setFrameSize(NSSize::new(400.0, 500.0));
    root.layoutSubtreeIfNeeded();
    let grid_frame = grid.native_view().frame();
    let capped_frame = capped.native_view().frame();
    let footer_frame = footer.native_view().frame();
    assert!(
        grid_frame.size.height > 400.0,
        "Fill の行が余りを受け取る: {grid_frame:?}"
    );
    assert_eq!(
        (capped_frame.size.width, capped_frame.size.height),
        (200.0, 100.0),
        "子は上限まで広がる: {capped_frame:?}"
    );
    let gap = (capped_frame.mid().y - footer_frame.mid().y).abs();
    assert!(
        gap > 150.0,
        "子は行の中央に置かれ、下の行へ寄らない: {capped_frame:?} / {footer_frame:?}"
    );

    root.setFrameSize(NSSize::new(400.0, 80.0));
    root.layoutSubtreeIfNeeded();
    let narrow = capped.native_view().frame();
    assert!(
        narrow.size.height < 100.0,
        "狭いときは上限より小さくなる: {narrow:?}"
    );
    Ok(())
}

// ----------------------------------------------------------------- 非同期

/// A future that yields once. The rest runs on the next tick.
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

/// A value sent from another thread reaches a label via the main queue.
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

/// Spawned work progresses on the run loop and completes.
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

/// Cancelled work never runs.
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

/// A vertical `Stack` inside a vertical `Stack` does not absorb the leftover height.
///
/// If it did, a large gap would appear in the middle of a section when the content
/// is shorter than the screen. The leftover always stays at the end of the
/// outermost one.
fn nested_stack_does_not_take_the_slack(ui: &Ui) -> Result<()> {
    let pane = ui.stack(Orientation::Vertical)?;
    pane.set_spacing(10.0);
    let group = ui.stack(Orientation::Vertical)?;
    group.set_spacing(4.0);
    group.append(&ui.label("見出し")?);
    group.append(&ui.label("説明")?);
    group.set_sizing(Sizing::fill_width());
    pane.append(&group);
    let after = ui.label("次の節")?;
    pane.append(&after);

    let root = pane.native_view();
    root.setFrameSize(NSSize::new(400.0, 600.0));
    root.layoutSubtreeIfNeeded();

    let group_frame = group.native_view().frame();
    assert!(
        group_frame.size.height < 100.0,
        "内側の Stack は中身の高さのままであること: {group_frame:?}"
    );
    // AppKit の座標系は左下原点。内側の Stack の下端から間隔だけ空けて次が来る。
    let after_frame = after.native_view().frame();
    let gap = group_frame.origin.y - (after_frame.origin.y + after_frame.size.height);
    assert!(
        (gap - 10.0).abs() < 1.0,
        "次の子が内側の Stack のすぐ下へ来ること: 空き {gap}"
    );
    Ok(())
}

/// A `Stack` that becomes the same direction as the `Stack` it is in, by switching
/// either of them, leaves the leftover space to the outer one, as in
/// [`nested_stack_does_not_take_the_slack`].
fn nested_stack_switched_to_the_same_direction(ui: &Ui) -> Result<()> {
    for switch_inner in [true, false] {
        let pane = ui.stack(if switch_inner {
            Orientation::Vertical
        } else {
            Orientation::Horizontal
        })?;
        pane.set_spacing(10.0);
        let group = ui.stack(if switch_inner {
            Orientation::Horizontal
        } else {
            Orientation::Vertical
        })?;
        group.set_spacing(4.0);
        group.append(&ui.label("見出し")?);
        group.append(&ui.label("説明")?);
        group.set_sizing(Sizing::fill_width());
        pane.append(&group);
        let after = ui.label("次の節")?;
        pane.append(&after);
        if switch_inner {
            group.set_orientation(Orientation::Vertical);
        } else {
            pane.set_orientation(Orientation::Vertical);
        }
        let which = if switch_inner { "内側" } else { "外側" };

        let root = pane.native_view();
        root.setFrameSize(NSSize::new(400.0, 600.0));
        root.layoutSubtreeIfNeeded();
        let group_frame = group.native_view().frame();
        assert!(
            group_frame.size.height < 100.0,
            "{which}の向きを変えても内側の Stack は中身の高さのままであること: {group_frame:?}"
        );
        let after_frame = after.native_view().frame();
        let gap = group_frame.origin.y - (after_frame.origin.y + after_frame.size.height);
        assert!(
            (gap - 10.0).abs() < 1.0,
            "{which}の向きを変えても次の子が内側の Stack のすぐ下へ来ること: 空き {gap}"
        );
    }
    Ok(())
}

/// A `Grid`'s `Auto` row follows its content growing taller after being shown.
///
/// Without it, expanding an Expander or adding rows to a list would squash the
/// row's content into the rows after it. Checked with the same nesting as the
/// Gallery (window > scroll > vertical Stack > grid).
fn grid_auto_row_follows_later_growth(ui: &Ui) -> Result<()> {
    let window = ui.window("追従", 400.0, 400.0)?;
    let grid = ui.grid()?;
    grid.set_spacing(0.0, 8.0);
    grid.set_column_track(0, Track::FILL);
    grid.set_row_track(0, Track::Auto);
    grid.set_row_track(1, Track::Auto);
    grid.set_sizing(Sizing::fill_width());

    let inner = ui.stack(Orientation::Vertical)?;
    inner.set_align(Align::Start);
    inner.append(&ui.checkbox("たたんだ中身")?);
    inner.append(&ui.checkbox("もう 1 行")?);
    let body = ui.expander("詳細")?;
    body.set_child(&inner);
    body.set_sizing(Sizing::fill_width());
    grid.attach(&body, GridCell::new(0, 0));
    let after = ui.label("次の行")?;
    grid.attach(&after, GridCell::new(0, 1));

    let pane = ui.stack(Orientation::Vertical)?;
    pane.set_spacing(12.0);
    pane.set_align(Align::Start);
    pane.append(&grid);
    let scroll = ui.scroll()?;
    scroll.set_policy(ScrollPolicy::Never, ScrollPolicy::Auto);
    scroll.set_child(&pane);
    scroll.set_sizing(Sizing::fill());
    window.set_child(&scroll);

    let content = window
        .native_window()
        .contentView()
        .expect("ウィンドウの中身があること");
    content.setFrameSize(NSSize::new(400.0, 400.0));
    content.layoutSubtreeIfNeeded();
    pump(0.05);
    let collapsed = body.native_view().frame().size.height;

    // 表示のあとで中身が増える。
    body.set_expanded(true);
    content.layoutSubtreeIfNeeded();
    pump(0.05);

    let expanded = body.native_view().frame();
    assert!(
        expanded.size.height > collapsed + 30.0,
        "行の高さが中身に追従すること: {collapsed} -> {expanded:?}"
    );
    let rows: Vec<_> = (0..inner.len()).map(|_| ()).collect();
    let _ = rows;
    let after_frame = after.native_view().frame();
    assert!(
        after_frame.origin.y + after_frame.size.height <= expanded.origin.y + 0.5,
        "後ろの行が押し下げられ、重ならないこと: {expanded:?} / {after_frame:?}"
    );
    Ok(())
}

/// Even when rows are added to a list after showing, the `Grid`'s `Auto` row
/// recomputes its height.
fn grid_auto_row_follows_added_list_rows(ui: &Ui) -> Result<()> {
    let window = ui.window("一覧の追加", 400.0, 400.0)?;
    let grid = ui.grid()?;
    grid.set_spacing(0.0, 8.0);
    grid.set_column_track(0, Track::FILL);
    grid.set_row_track(0, Track::Auto);
    grid.set_row_track(1, Track::Auto);
    grid.set_sizing(Sizing::fill_width());

    let list = ui.list()?;
    list.set_sizing(Sizing::fill_width());
    grid.attach(&list, GridCell::new(0, 0));
    let after = ui.label("次の行")?;
    grid.attach(&after, GridCell::new(0, 1));

    let pane = ui.stack(Orientation::Vertical)?;
    pane.set_spacing(12.0);
    pane.set_align(Align::Start);
    pane.append(&grid);
    let scroll = ui.scroll()?;
    scroll.set_policy(ScrollPolicy::Never, ScrollPolicy::Auto);
    scroll.set_child(&pane);
    scroll.set_sizing(Sizing::fill());
    window.set_child(&scroll);

    let content = window
        .native_window()
        .contentView()
        .expect("ウィンドウの中身があること");
    content.setFrameSize(NSSize::new(400.0, 400.0));
    content.layoutSubtreeIfNeeded();
    pump(0.05);
    let empty = list.native_view().frame().size.height;

    // 表示のあとで行を足す (Gallery の「後から作る行」と同じ経路)。
    let mut rows = Vec::new();
    for index in 1..=3 {
        let row = ui.stack(Orientation::Horizontal)?;
        row.append(&ui.label(&format!("あとから作った行 {index}"))?);
        rows.push(ListRow::new(&row));
    }
    list.set_rows(&rows);
    content.layoutSubtreeIfNeeded();
    pump(0.05);

    let filled = list.native_view().frame();
    assert!(
        filled.size.height > empty + 40.0,
        "足した行のぶんだけ一覧が高くなること: {empty} -> {filled:?}"
    );
    let after_frame = after.native_view().frame();
    assert!(
        after_frame.origin.y + after_frame.size.height <= filled.origin.y + 0.5,
        "後ろの行が押し下げられ、重ならないこと: {filled:?} / {after_frame:?}"
    );
    Ok(())
}

/// When a wrapping label's text is replaced, the `Grid`'s `Auto` row follows the
/// number of lines.
fn grid_auto_row_follows_wrapped_label(ui: &Ui) -> Result<()> {
    let window = ui.window("折り返しの追従", 300.0, 300.0)?;
    let grid = ui.grid()?;
    grid.set_spacing(0.0, 8.0);
    grid.set_column_track(0, Track::FILL);
    grid.set_row_track(0, Track::Auto);
    grid.set_row_track(1, Track::Auto);
    grid.set_sizing(Sizing::fill_width());

    let note = ui.label("短い説明")?;
    note.set_wrap(true);
    note.set_sizing(Sizing::fill_width());
    grid.attach(&note, GridCell::new(0, 0));
    let after = ui.label("次の行")?;
    grid.attach(&after, GridCell::new(0, 1));

    let pane = ui.stack(Orientation::Vertical)?;
    pane.set_align(Align::Start);
    pane.append(&grid);
    window.set_child(&pane);

    let content = window
        .native_window()
        .contentView()
        .expect("ウィンドウの中身があること");
    content.setFrameSize(NSSize::new(300.0, 300.0));
    content.layoutSubtreeIfNeeded();
    pump(0.05);
    let short = note.native_view().frame().size.height;

    note.set_text(
        "これはとても長い説明の文章で、この幅では何行にも折り返して表示されるはずのものです。",
    );
    content.layoutSubtreeIfNeeded();
    pump(0.05);

    let long = note.native_view().frame();
    assert!(
        long.size.height > short + 10.0,
        "折り返した行数のぶんだけ高くなること: {short} -> {long:?}"
    );
    let after_frame = after.native_view().frame();
    assert!(
        after_frame.origin.y + after_frame.size.height <= long.origin.y + 0.5,
        "後ろの行が押し下げられ、重ならないこと: {long:?} / {after_frame:?}"
    );
    Ok(())
}

/// Even when nested, a `Stack` with `Fill` on the main axis takes the leftover as
/// before.
///
/// The flip side of making it "not grow on its own" in
/// [`nested_stack_does_not_take_the_slack`]. It grows only when asked.
fn nested_stack_with_fill_takes_the_slack(ui: &Ui) -> Result<()> {
    let pane = ui.stack(Orientation::Vertical)?;
    pane.set_spacing(10.0);
    pane.append(&ui.label("上")?);
    let body = ui.stack(Orientation::Vertical)?;
    body.append(&ui.label("伸びる区画")?);
    body.set_sizing(Sizing::fill());
    pane.append(&body);
    let after = ui.label("下")?;
    pane.append(&after);

    let root = pane.native_view();
    root.setFrameSize(NSSize::new(400.0, 600.0));
    root.layoutSubtreeIfNeeded();

    let body_frame = body.native_view().frame();
    assert!(
        body_frame.size.height > 400.0,
        "Fill を指定した入れ子の Stack が余りを受け取ること: {body_frame:?}"
    );
    let after_frame = after.native_view().frame();
    assert!(
        after_frame.origin.y < 30.0,
        "後ろの子が下端まで押されること: {after_frame:?}"
    );
    Ok(())
}

/// Breadcrumbs line up with the labels in the same column by **the text's left
/// edge**, not the frame.
///
/// A standard-style `NSPathControl` draws its text with padding inside its frame,
/// so as is it looks shifted right of the labels in the same column.
///
/// Comparing alignment values with each other would only "retrace the same
/// calculation", so **the text actually drawn** is rendered into an image and its
/// left edge measured. If the padding changes with the OS version, appearance or
/// text size, this is where it gets noticed.
fn breadcrumbs_align_with_labels(ui: &Ui) -> Result<()> {
    // 描かせるために、表示はしないウィンドウへ入れておく。
    let window = ui.window("パンくずの位置合わせ", 400.0, 200.0)?;
    let pane = ui.stack(Orientation::Vertical)?;
    pane.set_align(Align::Start);
    let label = ui.label("階層と現在地を表示します。")?;
    pane.append(&label);
    let crumbs = ui.breadcrumbs()?;
    crumbs.set_items(&NavItem::list(["階層 1", "階層 2", "現在地"]));
    pane.append(&crumbs);
    window.set_child(&pane);

    let content = window
        .native_window()
        .contentView()
        .expect("ウィンドウの中身があること");
    content.setFrameSize(NSSize::new(400.0, 200.0));
    content.layoutSubtreeIfNeeded();

    let label_view = label.native_view();
    let crumbs_view = crumbs.native_view();
    // 親から見た「文字の左端」= 枠の位置 + ビューの中で文字が始まる位置。
    let label_text = label_view.frame().origin.x + first_ink_x(&label_view);
    let crumbs_text = crumbs_view.frame().origin.x + first_ink_x(&crumbs_view);
    assert!(
        (label_text - crumbs_text).abs() < 1.5,
        "描かれた文字の左端がそろうこと: ラベル {label_text} / パンくず {crumbs_text}"
    );
    window.close();
    Ok(())
}

/// Renders the view into an image and returns the leftmost position where anything
/// is drawn (pt).
///
/// Neither labels nor breadcrumbs draw a background, so the left edge of the
/// non-transparent pixels is the text's left edge. It measures the same way in both
/// light and dark appearances.
fn first_ink_x(view: &NSView) -> f64 {
    let bounds = view.bounds();
    let rep = view
        .bitmapImageRepForCachingDisplayInRect(bounds)
        .expect("描画用のビットマップを作れること");
    view.cacheDisplayInRect_toBitmapImageRep(bounds, &rep);
    let width = rep.pixelsWide();
    let height = rep.pixelsHigh();
    let scale = width as f64 / bounds.size.width.max(1.0);
    for x in 0..width {
        for y in 0..height {
            // 画素の並び (RGBA / ARGB、前乗せかどうか) は AppKit が決めるので、
            // 生のバイトではなく色として読む。
            let Some(color) = rep.colorAtX_y(x, y) else {
                continue;
            };
            // 縁のぼかしを拾わないよう、はっきり描かれた画素だけを見る。
            if color.alphaComponent() > 0.15 {
                return x as f64 / scale;
            }
        }
    }
    panic!("文字が描かれていること");
}

/// Breadcrumbs do not take keyboard focus from a click alone.
///
/// `NSPathControl` accepts focus by default, so once clicked a blue focus ring stays
/// and never goes away. It is navigation that just changes the screen when clicked,
/// so it accepts focus only when the keyboard navigation setting (System Settings >
/// Keyboard) is on.
fn breadcrumbs_take_focus_like_a_button(ui: &Ui) -> Result<()> {
    let crumbs = ui.breadcrumbs()?;
    crumbs.set_items(&NavItem::list(["階層 1", "現在地"]));
    let mtm = MainThreadMarker::new().expect("メインスレッド");
    let keyboard = NSApplication::sharedApplication(mtm).isFullKeyboardAccessEnabled();
    assert_eq!(
        crumbs.native_view().acceptsFirstResponder(),
        keyboard,
        "キーボード操作の設定が入のときだけフォーカスを受けること"
    );
    Ok(())
}

/// Clicking breadcrumbs does not leave a blue focus ring behind.
///
/// The hands-on version of [`breadcrumbs_take_focus_like_a_button`]. It delivers a
/// real click and checks that the breadcrumbs did not become the window's first
/// responder (the focus ring is drawn only on the first responder).
fn breadcrumbs_click_leaves_no_focus_ring(ui: &Ui) -> Result<()> {
    let mtm = MainThreadMarker::new().expect("メインスレッド");
    let app = NSApplication::sharedApplication(mtm);
    if app.isFullKeyboardAccessEnabled() {
        // キーボード操作が入のときは、フォーカスを受けるのが正しい。
        return Ok(());
    }

    let window = ui.window("パンくずのクリック", 360.0, 120.0)?;
    let root = ui.stack(Orientation::Vertical)?;
    root.set_sizing(Sizing::fill());
    let crumbs = ui.breadcrumbs()?;
    crumbs.set_items(&NavItem::list(["階層 1", "現在地"]));
    root.append(&crumbs);
    window.set_child(&root);

    // アクティブでないアプリのウィンドウは key になれず、クリックが届かない。
    // テストの間だけアクセサリにして、終わったら元へ戻す。
    let policy = app.activationPolicy();
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    #[allow(deprecated)]
    app.activateIgnoringOtherApps(true);
    window.show();
    let native = window.native_window();
    native.makeKeyAndOrderFront(None);
    for _ in 0..40 {
        if native.isKeyWindow() {
            break;
        }
        pump(0.05);
        deliver_events(&app);
    }
    let content_view = native.contentView().expect("contentView");
    content_view.layoutSubtreeIfNeeded();
    content_view.display();

    let view = crumbs.native_view();
    if native.isKeyWindow() {
        // ウィンドウのイベントキューへ積む。ヒットテストと追跡ループを通る。
        click_view(&app, &native, &view);
        let responder = native.firstResponder();
        let focused = responder
            .map(|responder| std::ptr::eq(&*responder as *const _ as *const NSView, &*view))
            .unwrap_or(false);
        assert!(!focused, "クリックでフォーカスを取らないこと");
    } else {
        // ここで `mouseDown:` を直接呼ぶ手もあるが、それだと**直す前でも
        // フォーカスを取らない** (取りに行くのはウィンドウがイベントを配る
        // 経路)。確かめられていないのに緑になるので、SKIP として残す。
        // フォーカスを受け取るかどうか自体は
        // `breadcrumbs_take_focus_like_a_button` が無条件に確かめている。
        skip("ウィンドウが key にならず、本物のクリックを配送できなかった");
    }
    window.close();
    // 後続のテストへ影響しないよう、アプリの状態を戻す。
    app.setActivationPolicy(policy);
    Ok(())
}

// -------------------------------------------------------------- メニューバー

/// Takes out the `NSMenu`s of the headings the app passed, from between naui's
/// standard menus.
///
/// The first is the app menu and the last the Edit menu; the ones in between are the
/// app's.
fn app_menus(bar: &naui_macos::MenuBar) -> Vec<Retained<objc2_app_kit::NSMenu>> {
    let main = bar.native_menu();
    let count = main.numberOfItems();
    (1..count.saturating_sub(1))
        .filter_map(|i| main.itemAtIndex(i).and_then(|item| item.submenu()))
        .collect()
}

/// Headings become `NSMenu`s and items become `NSMenuItem`s.
fn menu_bar_menus_map_to_native(ui: &Ui) -> Result<()> {
    let menu_bar = ui.menu_bar()?;
    assert!(menu_bar.is_empty());

    menu_bar.set_menus(&[
        MenuSpec::new(
            "ファイル",
            [
                MenuItem::new("新規").shortcut(MenuShortcut::new('n')),
                MenuItem::separator(),
                MenuItem::new("保存")
                    .shortcut(MenuShortcut::new('s').shift(true))
                    .enabled(false),
            ],
        ),
        MenuSpec::new("表示", ["拡大", "縮小"]),
    ]);

    assert_eq!(menu_bar.len(), 2, "見出しの数");
    assert_eq!(menu_bar.menu_len(0), 3, "区切り線も 1 項目として数える");
    assert_eq!(menu_bar.menu_len(1), 2);
    assert_eq!(menu_bar.menu_len(9), 0, "範囲外は 0");

    let menus = app_menus(&menu_bar);
    assert_eq!(menus.len(), 2);
    assert_eq!(menus[0].title().to_string(), "ファイル");
    assert_eq!(menus[0].numberOfItems(), 3);
    assert!(
        !menus[0].autoenablesItems(),
        "有効・無効はアプリの指定をそのまま使う"
    );

    let first = menu_bar.native_item(&[0, 0]).expect("先頭は項目");
    assert_eq!(first.title().to_string(), "新規");
    assert!(first.isEnabled());
    assert_eq!(first.keyEquivalent().to_string(), "n");
    assert_eq!(
        first.keyEquivalentModifierMask(),
        NSEventModifierFlags::Command,
        "主修飾キーは ⌘"
    );
    assert!(
        menu_bar.native_item(&[0, 1]).is_none(),
        "区切り線に項目は無い"
    );
    assert!(menus[0].itemAtIndex(1).expect("2 番目").isSeparatorItem());

    let save = menu_bar.native_item(&[0, 2]).expect("3 番目は項目");
    assert!(!save.isEnabled());
    assert_eq!(save.keyEquivalent().to_string(), "s");
    assert_eq!(
        save.keyEquivalentModifierMask(),
        NSEventModifierFlags::Command | NSEventModifierFlags::Shift,
        "⇧ を足すと ⇧⌘ になる"
    );
    assert!(menu_bar.native_item(&[9, 0]).is_none(), "範囲外は None");
    assert!(menu_bar.native_item(&[0, 9]).is_none(), "範囲外は None");

    // ショートカットを指定しなければキー等価は付かない。
    assert_eq!(
        menu_bar
            .native_item(&[1, 0])
            .expect("項目")
            .keyEquivalent()
            .to_string(),
        ""
    );

    assert!(menu_bar.is_item_enabled(&[0, 0]));
    assert!(!menu_bar.is_item_enabled(&[0, 1]), "区切り線は押せない");
    assert!(!menu_bar.is_item_enabled(&[0, 2]));

    // 項目ごとの指定と全体の指定は AND を取る。
    menu_bar.set_item_enabled(&[0, 2], true);
    assert!(menu_bar.native_item(&[0, 2]).expect("項目").isEnabled());
    menu_bar.set_enabled(false);
    assert!(!menu_bar.is_item_enabled(&[0, 0]));
    assert!(!menu_bar.native_item(&[0, 0]).expect("項目").isEnabled());
    menu_bar.set_enabled(true);
    assert!(
        menu_bar.is_item_enabled(&[0, 2]),
        "全体を戻すと項目ごとの指定が残る"
    );

    // 区切り線への set_item_enabled は無視する。
    menu_bar.set_item_enabled(&[0, 1], true);
    assert!(!menu_bar.is_item_enabled(&[0, 1]));

    menu_bar.set_menus(&[]);
    assert!(menu_bar.is_empty());
    assert!(menu_bar.native_item(&[0, 0]).is_none());
    assert!(app_menus(&menu_bar).is_empty());
    Ok(())
}

/// naui's standard menus (the app menu and Edit menu) always remain.
///
/// ⌘C / ⌘V are dispatched as key equivalents of the main menu, so even if the app
/// replaces the menu bar, pasting would stop working if these two disappeared.
fn menu_bar_keeps_the_standard_menus(ui: &Ui) -> Result<()> {
    let menu_bar = ui.menu_bar()?;
    menu_bar.set_menus(&[MenuSpec::new("ファイル", ["新規"])]);

    let main = menu_bar.native_menu();
    assert_eq!(
        main.numberOfItems(),
        3,
        "アプリメニュー + アプリの見出し + 編集メニュー"
    );
    assert!(
        !main.autoenablesItems(),
        "見出しの有効・無効もアプリの指定を使う"
    );

    let titles: Vec<String> = (0..main.numberOfItems())
        .filter_map(|i| main.itemAtIndex(i).and_then(|item| item.submenu()))
        .map(|menu| menu.title().to_string())
        .collect();
    assert_eq!(
        titles[0], "naui",
        "先頭はアプリメニュー (run_for_test の名前)"
    );
    assert_eq!(titles[1], "ファイル");
    assert_eq!(
        titles[2],
        naui_core::ui_strings().edit_menu,
        "末尾は標準の編集メニュー"
    );

    // 編集メニューはレスポンダチェーンへ流す (ターゲットを固定しない)。
    let edit = main
        .itemAtIndex(main.numberOfItems() - 1)
        .and_then(|item| item.submenu())
        .expect("編集メニュー");
    let paste = (0..edit.numberOfItems())
        .filter_map(|i| edit.itemAtIndex(i))
        .find(|item| item.action() == Some(sel!(paste:)))
        .expect("paste: を送る項目があること");
    assert!(unsafe { paste.target() }.is_none());

    // 全体を無効にしても、⌘Q と ⌘V は取り上げない。
    menu_bar.set_enabled(false);
    assert!(
        main.itemAtIndex(0).expect("アプリメニュー").isEnabled(),
        "アプリメニューは無効にしない"
    );
    assert!(
        main.itemAtIndex(2).expect("編集メニュー").isEnabled(),
        "編集メニューは無効にしない"
    );
    assert!(
        !main.itemAtIndex(1).expect("アプリの見出し").isEnabled(),
        "アプリの見出しは無効になる"
    );
    Ok(())
}

/// The clicked item arrives as its path (heading, item).
fn menu_bar_activation_notifies(ui: &Ui) -> Result<()> {
    let menu_bar = ui.menu_bar()?;
    menu_bar.set_menus(&[
        MenuSpec::new(
            "編集",
            [
                MenuItem::new("元に戻す"),
                MenuItem::separator(),
                MenuItem::new("やり直す").enabled(false),
            ],
        ),
        MenuSpec::new("表示", ["拡大"]),
    ]);

    let seen = Rc::new(RefCell::new(Vec::new()));
    menu_bar.on_activate({
        let seen = seen.clone();
        move |path| seen.borrow_mut().push(path.to_vec())
    });

    menu_bar.activate(&[0, 0]);
    assert_eq!(*seen.borrow(), vec![vec![0, 0]]);
    menu_bar.activate(&[0, 1]);
    menu_bar.activate(&[0, 2]);
    menu_bar.activate(&[9, 0]);
    menu_bar.activate(&[0, 9]);
    assert_eq!(
        *seen.borrow(),
        vec![vec![0, 0]],
        "区切り線・押せない項目・範囲外は通知しない"
    );

    // AppKit が使うのと同じ target/action の経路も確かめる。
    let item = menu_bar.native_item(&[1, 0]).expect("項目");
    let target = unsafe { item.target() }.expect("target が設定されていること");
    assert_eq!(
        item.action(),
        Some(sel!(invoke:)),
        "トランポリンの invoke: が action になっている"
    );
    unsafe {
        let _: () = msg_send![&*target, invoke: &*item];
    }
    assert_eq!(*seen.borrow(), vec![vec![0, 0], vec![1, 0]]);

    menu_bar.set_item_enabled(&[0, 2], true);
    menu_bar.activate(&[0, 2]);
    assert_eq!(*seen.borrow(), vec![vec![0, 0], vec![1, 0], vec![0, 2]]);

    menu_bar.set_enabled(false);
    menu_bar.activate(&[0, 0]);
    assert_eq!(
        *seen.borrow(),
        vec![vec![0, 0], vec![1, 0], vec![0, 2]],
        "無効なメニューバーは通知しない"
    );
    Ok(())
}

/// A submenu becomes an `NSMenuItem` holding another `NSMenu`, and a check item's
/// `state` follows the user's choice, `activate` and `set_item_checked`.
fn menu_bar_submenus_and_checks_map_to_native(ui: &Ui) -> Result<()> {
    let menu_bar = ui.menu_bar()?;
    menu_bar.set_menus(&[MenuSpec::new(
        "表示",
        [
            MenuItem::new("サイドバー").checked(true),
            MenuItem::submenu(
                "拡大",
                [
                    MenuItem::new("拡大").shortcut(MenuShortcut::new('i')),
                    MenuItem::submenu("深い", ["さらに"]),
                ],
            ),
        ],
    )]);
    assert_eq!(menu_bar.menu_len(0), 2);

    let sidebar = menu_bar.native_item(&[0, 0]).expect("項目");
    assert_eq!(sidebar.state(), NSControlStateValueOn);
    let zoom = menu_bar.native_item(&[0, 1]).expect("サブメニューの見出し");
    let submenu = zoom.submenu().expect("NSMenu を持つこと");
    assert_eq!(submenu.title().to_string(), "拡大");
    assert_eq!(submenu.numberOfItems(), 2);
    assert!(!submenu.autoenablesItems());
    let deep = menu_bar.native_item(&[0, 1, 1, 0]).expect("3 段目の項目");
    assert_eq!(deep.title().to_string(), "さらに");
    assert_eq!(
        menu_bar
            .native_item(&[0, 1, 0])
            .expect("項目")
            .keyEquivalent()
            .to_string(),
        "i",
        "サブメニューの中にもショートカットが付く"
    );

    let seen = Rc::new(RefCell::new(Vec::new()));
    menu_bar.on_activate({
        let seen = seen.clone();
        let menu_bar = menu_bar.clone();
        move |path| {
            seen.borrow_mut()
                .push((path.to_vec(), menu_bar.is_item_checked(path)))
        }
    });

    // AppKit と同じ target/action の経路で選ぶ。
    let target = unsafe { sidebar.target() }.expect("target");
    unsafe {
        let _: () = msg_send![&*target, invoke: &*sidebar];
    }
    assert_eq!(
        *seen.borrow(),
        vec![(vec![0, 0], false)],
        "通知の前に反転する"
    );
    assert_eq!(sidebar.state(), NSControlStateValueOff);

    menu_bar.activate(&[0, 0]);
    assert_eq!(sidebar.state(), NSControlStateValueOn);
    menu_bar.activate(&[0, 1, 1, 0]);
    menu_bar.activate(&[0, 1]);
    assert_eq!(
        *seen.borrow(),
        vec![
            (vec![0, 0], false),
            (vec![0, 0], true),
            (vec![0, 1, 1, 0], false)
        ],
        "サブメニューの見出しは選べない"
    );

    menu_bar.set_item_checked(&[0, 0], false);
    assert!(!menu_bar.is_item_checked(&[0, 0]));
    assert_eq!(sidebar.state(), NSControlStateValueOff);
    assert_eq!(seen.borrow().len(), 3, "set_item_checked は通知しない");
    menu_bar.set_item_checked(&[0, 1, 0], true);
    assert!(
        !menu_bar.is_item_checked(&[0, 1, 0]),
        "印の無い項目には付かない"
    );

    // サブメニューを止めると、中の項目もキー等価ごと止まる。
    menu_bar.set_item_enabled(&[0, 1], false);
    assert!(!zoom.isEnabled());
    assert!(!menu_bar.native_item(&[0, 1, 0]).expect("項目").isEnabled());
    assert!(!deep.isEnabled());
    assert!(!menu_bar.is_item_enabled(&[0, 1, 1, 0]));
    menu_bar.activate(&[0, 1, 1, 0]);
    assert_eq!(seen.borrow().len(), 3);
    menu_bar.set_item_enabled(&[0, 1], true);
    assert!(deep.isEnabled(), "戻すと中の指定どおりに戻る");
    assert!(menu_bar.is_item_enabled(&[0, 1, 1, 0]));
    Ok(())
}

/// Function keys become AppKit's function-key characters, with ⌘ only when asked,
/// and AppKit matches them as key equivalents.
fn menu_bar_function_keys_are_key_equivalents(ui: &Ui) -> Result<()> {
    let menu_bar = ui.menu_bar()?;
    menu_bar.set_menus(&[MenuSpec::new(
        "表示",
        [
            MenuItem::new("再読み込み").shortcut(MenuShortcut::function(5)),
            MenuItem::new("開発").shortcut(MenuShortcut::function(12).primary(true).shift(true)),
            MenuItem::new("記号").shortcut(MenuShortcut::new('/')),
        ],
    )]);

    let reload = menu_bar.native_item(&[0, 0]).expect("項目");
    assert_eq!(reload.keyEquivalent().to_string(), "\u{F708}");
    assert_eq!(
        reload.keyEquivalentModifierMask(),
        NSEventModifierFlags::empty(),
        "主修飾キーを含めないファンクションキーは単独で効く"
    );
    let develop = menu_bar.native_item(&[0, 1]).expect("項目");
    assert_eq!(develop.keyEquivalent().to_string(), "\u{F70F}");
    assert_eq!(
        develop.keyEquivalentModifierMask(),
        NSEventModifierFlags::Command | NSEventModifierFlags::Shift
    );
    assert_eq!(
        menu_bar
            .native_item(&[0, 2])
            .expect("項目")
            .keyEquivalent()
            .to_string(),
        "",
        "記号のショートカットはどの環境でも登録しない"
    );

    let seen = Rc::new(RefCell::new(Vec::new()));
    menu_bar.on_activate({
        let seen = seen.clone();
        move |path| seen.borrow_mut().push(path.to_vec())
    });

    // F5 を押したときと同じキーイベントを NSMenu に照らし合わせる。
    let f5 = NSString::from_str("\u{F708}");
    let event = NSEvent::keyEventWithType_location_modifierFlags_timestamp_windowNumber_context_characters_charactersIgnoringModifiers_isARepeat_keyCode(
        NSEventType::KeyDown,
        NSPoint::new(0.0, 0.0),
        NSEventModifierFlags::Function,
        0.0,
        0,
        None,
        &f5,
        &f5,
        false,
        0x60, // kVK_F5
    )
    .expect("キーイベント");
    assert!(
        menu_bar.native_menu().performKeyEquivalent(&event),
        "AppKit が F5 をキー等価として受け取ること"
    );
    assert_eq!(*seen.borrow(), vec![vec![0, 0]]);
    Ok(())
}

/// Attaching makes it `NSApplication.mainMenu`, and detaching goes back to the
/// default.
fn menu_bar_attaches_to_the_application(ui: &Ui) -> Result<()> {
    let mtm = objc2::MainThreadMarker::new().expect("メインスレッド");
    let app = NSApplication::sharedApplication(mtm);
    let window = ui.window("メニューバー", 400.0, 300.0)?;

    let menu_bar = ui.menu_bar()?;
    menu_bar.set_menus(&[MenuSpec::new("ファイル", ["新規", "開く"])]);
    window.set_menu_bar(&menu_bar);
    assert!(
        app.mainMenu()
            .is_some_and(|main| main == menu_bar.native_menu()),
        "アプリのメインメニューになること"
    );

    // 取り付けたあとに組み替えても、メインメニューへ映る。
    menu_bar.set_menus(&[MenuSpec::new("編集", ["元に戻す"])]);
    assert!(
        app.mainMenu()
            .is_some_and(|main| main == menu_bar.native_menu()),
        "組み替えても取り付いたまま"
    );
    assert_eq!(app_menus(&menu_bar).len(), 1);

    window.clear_menu_bar();
    let restored = app.mainMenu().expect("既定のメニューへ戻ること");
    assert_ne!(restored, menu_bar.native_menu());
    assert_eq!(
        restored.numberOfItems(),
        2,
        "アプリメニューと編集メニューだけが残る"
    );
    window.close();
    Ok(())
}

/// Rebuilding the menu bar from inside the notification does not double-borrow.
fn menu_bar_callback_is_reentrant(ui: &Ui) -> Result<()> {
    let menu_bar = ui.menu_bar()?;
    menu_bar.set_menus(&[MenuSpec::new("ファイル", ["新規", "開く"])]);

    let seen = Rc::new(RefCell::new(Vec::new()));
    menu_bar.on_activate({
        let seen = seen.clone();
        let menu_bar = menu_bar.clone();
        move |path| {
            seen.borrow_mut().push(path.to_vec());
            // 通知の中でメニューを組み替え、コールバックも差し替える。
            menu_bar.set_menus(&[MenuSpec::new("編集", ["元に戻す"])]);
            menu_bar.set_item_enabled(&[0, 0], true);
            menu_bar.on_activate({
                let seen = seen.clone();
                move |path| {
                    seen.borrow_mut()
                        .push(path.iter().map(|i| i + 10).collect())
                }
            });
        }
    });

    menu_bar.activate(&[0, 1]);
    assert_eq!(*seen.borrow(), vec![vec![0, 1]]);
    assert_eq!(menu_bar.len(), 1);
    assert_eq!(menu_bar.menu_len(0), 1);

    menu_bar.activate(&[0, 0]);
    assert_eq!(
        *seen.borrow(),
        vec![vec![0, 1], vec![10, 10]],
        "差し替えたコールバックが呼ばれる"
    );
    Ok(())
}

/// Sidebar groups: one without a heading and one with a heading.
fn sidebar_fixture() -> Vec<SidebarSection> {
    vec![
        SidebarSection::untitled([
            SidebarItem::new("一般").icon(ToolbarIcon::Settings),
            SidebarItem::new("検索").icon(ToolbarIcon::Search),
        ]),
        SidebarSection::untitled([SidebarItem::new("情報")]),
        SidebarSection::new(
            "場所",
            [
                SidebarItem::new("書類").icon(ToolbarIcon::Open),
                SidebarItem::new("共有").enabled(false),
            ],
        ),
    ]
}

/// Items, gaps and headings are laid out in a source-list-style `NSTableView`.
fn sidebar_rows_map_to_a_source_list(ui: &Ui) -> Result<()> {
    let sidebar = ui.sidebar()?;
    assert!(sidebar.is_empty());
    sidebar.set_sections(&sidebar_fixture());
    assert_eq!(sidebar.len(), 5, "見出しと隙間は数えない");

    let table = sidebar.native_table_view();
    assert_eq!(table.style(), objc2_app_kit::NSTableViewStyle::SourceList);
    assert!(table.headerView().is_none(), "列の見出しは出さない");
    // 一般・検索 | 隙間 | 情報 | 場所 (見出し。前の隙間は省く) | 書類・共有
    assert_eq!(table.numberOfRows(), 7);

    let delegate = unsafe { table.delegate() }.expect("デリゲートがあること");
    let is_group = |row: isize| unsafe { delegate.tableView_isGroupRow(&table, row) };
    assert!(!is_group(0));
    assert!(is_group(4), "見出しはグループ行になる");
    let height = |row: isize| unsafe { delegate.tableView_heightOfRow(&table, row) };
    assert!(height(2) < height(0), "隙間の行は項目より低い");
    let selectable = |row: isize| unsafe { delegate.tableView_shouldSelectRow(&table, row) };
    assert!(selectable(0));
    assert!(!selectable(2), "隙間は選べない");
    assert!(!selectable(4), "見出しは選べない");
    assert!(!selectable(6), "選べない項目は選べない");

    // 項目のセルは NSTableCellView で、文字とアイコンを持つ。
    let cell = |row: isize| {
        table
            .viewAtColumn_row_makeIfNecessary(0, row, true)
            .and_then(|view| view.downcast::<objc2_app_kit::NSTableCellView>().ok())
            .expect("セルがあること")
    };
    let first = cell(0);
    assert_eq!(
        unsafe { first.textField() }.map(|f| f.stringValue().to_string()),
        Some("一般".to_string())
    );
    assert!(
        unsafe { first.imageView() }.is_some(),
        "アイコンを指定した項目は記号を持つ"
    );
    assert!(
        unsafe { cell(3).imageView() }.is_none(),
        "アイコンの無い項目は文字だけ"
    );
    assert_eq!(
        unsafe { cell(4).textField() }.map(|f| f.stringValue().to_string()),
        Some("場所".to_string())
    );

    sidebar.set_items(&SidebarItem::list(["春", "夏"]));
    assert_eq!(sidebar.len(), 2);
    assert_eq!(table.numberOfRows(), 2, "まとまり 1 つなら隙間は無い");
    Ok(())
}

/// The selection is exchanged by running number; `set_selected` does not notify,
/// while `select` and the user's selection do.
fn sidebar_selection_round_trips(ui: &Ui) -> Result<()> {
    let sidebar = ui.sidebar()?;
    sidebar.set_sections(&sidebar_fixture());
    let seen = Rc::new(RefCell::new(Vec::new()));
    sidebar.on_select({
        let seen = seen.clone();
        move |index| seen.borrow_mut().push(index)
    });
    assert_eq!(sidebar.selected(), None, "最初は何も選ばれていない");

    sidebar.set_selected(3);
    assert_eq!(sidebar.selected(), Some(3));
    let table = sidebar.native_table_view();
    assert_eq!(table.selectedRow(), 5, "書類の行 (見出しの後ろ) が選ばれる");
    assert!(seen.borrow().is_empty(), "set_selected は通知しない");

    sidebar.set_selected(4);
    sidebar.set_selected(99);
    assert_eq!(
        sidebar.selected(),
        Some(3),
        "選べない項目と範囲外は無視する"
    );

    sidebar.select(2);
    assert_eq!(sidebar.selected(), Some(2));
    assert_eq!(*seen.borrow(), [2], "select は通知する");
    sidebar.select(2);
    assert_eq!(*seen.borrow(), [2, 2], "同じ項目を選び直しても通知する");

    // 利用者がクリックしたときと同じく、テーブル側から選ぶ。
    table.selectRowIndexes_byExtendingSelection(
        &objc2_foundation::NSIndexSet::indexSetWithIndex(1),
        false,
    );
    assert_eq!(sidebar.selected(), Some(1));
    assert_eq!(*seen.borrow(), [2, 2, 1], "テーブルでの選択も通知する");

    // 項目を差し替えても、同じ番号がまだ選べれば残る。
    sidebar.set_sections(&sidebar_fixture());
    assert_eq!(sidebar.selected(), Some(1));
    sidebar.set_items(&SidebarItem::list(["ひとつ"]));
    assert_eq!(sidebar.selected(), None, "無くなった番号の選択は外れる");
    assert_eq!(seen.borrow().len(), 3, "差し替えでは通知しない");

    sidebar.set_selected(0);
    sidebar.clear_selection();
    assert_eq!(sidebar.selected(), None);
    assert_eq!(seen.borrow().len(), 3, "clear_selection は通知しない");
    Ok(())
}

/// Even when the right pane's child tries to shrink (labels, and scroll views
/// containing labels, have a vertical content hugging of 750, stronger than the
/// window size priority of 500), the window does not shrink and the child fills the
/// pane. The same as without a sidebar.
fn sidebar_child_does_not_shrink_the_window(ui: &Ui) -> Result<()> {
    let label = || -> Result<Box<dyn Widget>> { Ok(Box::new(ui.label("中身")?)) };
    let scroll = || -> Result<Box<dyn Widget>> {
        let scroll = ui.scroll()?;
        scroll.set_child(&ui.label("中身")?);
        Ok(Box::new(scroll))
    };
    let cases: [(&str, &dyn Fn() -> Result<Box<dyn Widget>>); 2] =
        [("ラベル", &label), ("スクロール", &scroll)];
    for (name, make) in cases {
        let window = ui.window(name, 640.0, 400.0)?;
        let child = make()?;
        window.set_child(child.as_ref());
        let sidebar = ui.sidebar()?;
        sidebar.set_items(&[SidebarItem::new("一般")]);
        window.set_sidebar(&sidebar);
        let native = window.native_window();
        let frame = native.frame();
        window.show();
        pump(0.8);
        assert_eq!(
            native.frame(),
            frame,
            "{name}: 表示してもウィンドウは縮まない"
        );

        // 子は区画の安全領域 (タイトルバーの下) いっぱいに広がる。
        let view = child.native_view();
        let fills = |when: &str| {
            let placed = view.convertRect_toView(view.bounds(), None);
            let layout = native.contentLayoutRect();
            let right = |rect: NSRect| rect.origin.x + rect.size.width;
            let top = |rect: NSRect| rect.origin.y + rect.size.height;
            assert!(
                (top(placed) - top(layout)).abs() < 0.5
                    && placed.origin.y.abs() < 0.5
                    && (right(placed) - right(layout)).abs() < 0.5
                    && placed.size.width > 200.0,
                "{name} ({when}): 子がタイトルバーの下の区画いっぱいに広がる: \
                 child={placed:?} layout={layout:?}"
            );
        };
        fills("表示後");

        // 利用者が広げた大きさも保ち、子も付いてくる。
        let mut larger = native.frame();
        larger.size.height += 120.0;
        larger.origin.y -= 120.0;
        native.setFrame_display(larger, true);
        pump(0.5);
        assert_eq!(native.frame(), larger, "{name}: 広げた大きさのまま");
        fills("広げた後");

        // ツールバーでタイトルバーの帯が厚くなっても、子の上端が付いてくる。
        let toolbar = ui.toolbar()?;
        toolbar.set_items(&[ToolbarItem::new(ToolbarIcon::New, "新規")]);
        window.set_toolbar(&toolbar);
        pump(0.5);
        fills("ツールバーを付けた後");
        window.close();
    }
    Ok(())
}

/// Attaching makes the `contentViewController` an `NSSplitViewController` and moves
/// the child to the right pane; detaching moves it back to the window's content.
fn sidebar_attaches_to_a_window(ui: &Ui) -> Result<()> {
    use objc2_app_kit::{NSSplitViewController, NSSplitViewItemBehavior, NSWindowStyleMask};

    let window = ui.window("サイドバー", 640.0, 400.0)?;
    let root = ui.stack(Orientation::Vertical)?;
    root.append(&ui.label("本文")?);
    window.set_child(&root);
    let native_window = window.native_window();
    let frame = native_window.frame();

    let sidebar = ui.sidebar()?;
    sidebar.set_sections(&sidebar_fixture());
    window.set_sidebar(&sidebar);

    let controller = native_window
        .contentViewController()
        .and_then(|c| c.downcast::<NSSplitViewController>().ok())
        .expect("contentViewController が NSSplitViewController になる");
    assert_eq!(controller, sidebar.native_split_view_controller());
    let items = controller.splitViewItems();
    assert_eq!(items.len(), 2);
    let first = items.objectAtIndex(0);
    assert_eq!(first.behavior(), NSSplitViewItemBehavior::Sidebar);
    assert!(first.allowsFullHeightLayout(), "タイトルバーの下まで伸びる");
    assert!(native_window
        .styleMask()
        .contains(NSWindowStyleMask::FullSizeContentView));
    assert_eq!(
        window.size(),
        (640.0, 400.0),
        "中身の大きさは変わらない (枠はタイトルバーに重なる分だけ伸びる)"
    );

    let mtm = MainThreadMarker::new().unwrap();
    let host = items.objectAtIndex(1).viewController(mtm).view();
    let root_view = root.native_view();
    // 子は区画の中の台に、枠で置かれる (制約でつなぐとウィンドウが縮むため)。
    assert!(root_view.isDescendantOf(&host), "子は右の区画に移る");

    // 子の上端はタイトルバーを避ける。
    controller.view().layoutSubtreeIfNeeded();
    let inset = host.safeAreaInsets().top;
    assert!(inset > 0.0, "中身の区画はタイトルバーの下まで伸びている");
    let placed = root_view.convertRect_toView(root_view.bounds(), Some(&host));
    let top = placed.origin.y + placed.size.height;
    assert!(
        (host.frame().size.height - top - inset).abs() < 1.0,
        "子の上端は安全領域の上端にそろう: host={:?} child={placed:?} inset={inset}",
        host.frame()
    );

    // 付けたまま子を差し替えても、右の区画に置かれる。
    let other = ui.stack(Orientation::Vertical)?;
    window.set_child(&other);
    assert!(other.native_view().isDescendantOf(&host));
    assert!(!root_view.isDescendantOf(&host), "前の子は外れる");

    window.clear_sidebar();
    assert!(native_window.contentViewController().is_none());
    assert!(!native_window
        .styleMask()
        .contains(NSWindowStyleMask::FullSizeContentView));
    assert_eq!(
        native_window.contentView(),
        Some(other.native_view()),
        "子はウィンドウの中身へ戻る"
    );
    assert_eq!(native_window.frame(), frame, "外すと元の枠へ戻る");
    window.clear_sidebar(); // 付いていなければ何もしない
    window.close();
    Ok(())
}

/// Width and collapse reach the `NSSplitView`'s pane and the `NSSplitViewItem`.
fn sidebar_width_and_collapse(ui: &Ui) -> Result<()> {
    let window = ui.window("サイドバーの幅", 800.0, 400.0)?;
    window.set_child(&ui.stack(Orientation::Vertical)?);
    let sidebar = ui.sidebar()?;
    sidebar.set_items(&SidebarItem::list(["一般"]));
    assert_eq!(sidebar.width(), DEFAULT_SIDEBAR_WIDTH, "付ける前は覚えた幅");
    window.set_sidebar(&sidebar);

    let split = sidebar.native_split_view_controller().splitView();
    let pane = || {
        split.layoutSubtreeIfNeeded();
        split.arrangedSubviews().objectAtIndex(0).frame().size.width
    };
    assert!(
        (pane() - DEFAULT_SIDEBAR_WIDTH).abs() < 1.0,
        "既定の幅で出る: {}",
        pane()
    );

    let resized = Rc::new(RefCell::new(Vec::new()));
    sidebar.on_resize({
        let resized = resized.clone();
        move |width| resized.borrow_mut().push(width)
    });
    sidebar.set_width(260.0);
    assert!((pane() - 260.0).abs() < 1.0, "幅が区画に届く: {}", pane());
    assert!((sidebar.width() - 260.0).abs() < 1.0);
    sidebar.set_width(40.0);
    assert!(
        (sidebar.width() - naui_core::SIDEBAR_MIN_WIDTH).abs() < 1.0,
        "下限より狭くはならない"
    );
    sidebar.set_width(220.0);
    assert!(resized.borrow().is_empty(), "set_width では通知しない");

    // 利用者と同じく、仕切りをつかんで右へ 40 動かす (220 → 260 付近)。
    window.show();
    let app = NSApplication::sharedApplication(MainThreadMarker::new().unwrap());
    drag_divider(&app, &window.native_window(), &split, 40.0);
    let dragged = pane();
    assert!(
        (dragged - 260.0).abs() < 3.0,
        "仕切りで広げられる: {dragged}"
    );
    // 手を離したあと、AppKit は区画を画素の境目へそろえる (257.5 → 258)。
    // その並べ直しはドラッグのイベントの外で起きるので拾わない (拾うと開閉の
    // アニメーション中の幅まで拾ってしまう)。差は半画素までになる。
    assert!(
        (sidebar.width() - dragged).abs() < 1.0,
        "動かした幅を返す: width={} pane={dragged} resized={:?}",
        sidebar.width(),
        resized.borrow()
    );
    assert!(!resized.borrow().is_empty(), "利用者が動かしたら通知する");
    assert!((resized.borrow().last().unwrap() - dragged).abs() < 1.0);
    let count = resized.borrow().len();
    drag_divider(&app, &window.native_window(), &split, -80.0);
    assert!(
        (pane() - (dragged - 80.0)).abs() < 3.0,
        "仕切りで狭められる: {}",
        pane()
    );
    assert!(resized.borrow().len() > count);

    // 下限を越えて端まで寄せると、AppKit の作法どおり閉じる。
    let collapsed = Rc::new(RefCell::new(Vec::new()));
    sidebar.on_collapse({
        let collapsed = collapsed.clone();
        move |value| collapsed.borrow_mut().push(value)
    });
    drag_divider(&app, &window.native_window(), &split, -400.0);
    pump_until(3.0, || sidebar.is_collapsed());
    assert!(sidebar.is_collapsed(), "端まで寄せると閉じる");
    assert_eq!(*collapsed.borrow(), [true], "閉じたことが届く");
    sidebar.set_collapsed(false);
    pump_until(3.0, || {
        !split.isSubviewCollapsed(&split.arrangedSubviews().objectAtIndex(0))
    });
    sidebar.set_width(260.0);
    let fixed = pane();

    assert!(!sidebar.is_collapsed());
    window.show();
    sidebar.set_collapsed(true);
    assert!(sidebar.is_collapsed());
    assert!(
        (sidebar.width() - 260.0).abs() < 1.0,
        "閉じても開いたときの幅を返す"
    );
    // 画面の上でも区画が畳まれ、中身の区画がウィンドウの幅いっぱいになる。
    // 閉じる動きはアニメーションなので、終わるまでランループを回す。
    let sidebar_view = split.arrangedSubviews().objectAtIndex(0);
    let content = || split.arrangedSubviews().objectAtIndex(1).frame().size.width;
    pump_until(3.0, || split.isSubviewCollapsed(&sidebar_view));
    split.layoutSubtreeIfNeeded();
    assert!(
        split.isSubviewCollapsed(&sidebar_view),
        "サイドバーの区画が畳まれる"
    );
    assert!(
        (content() - split.frame().size.width).abs() < 1.0,
        "中身がウィンドウの幅いっぱいになる: {} / {}",
        content(),
        split.frame().size.width
    );
    sidebar.set_collapsed(false);
    assert!(!sidebar.is_collapsed());
    pump_until(3.0, || !split.isSubviewCollapsed(&sidebar_view));
    split.layoutSubtreeIfNeeded();
    assert!(
        (pane() - fixed).abs() < 1.0,
        "開き直すと同じ幅へ戻る: {}",
        pane()
    );

    sidebar.set_width(-1.0);
    sidebar.set_width(f64::NAN);
    assert!(
        (sidebar.width() - 260.0).abs() < 1.0,
        "おかしな幅は無視する"
    );
    window.clear_sidebar();
    window.close();
    Ok(())
}

/// Adding directly to the `NSSplitView` would lay it out as one of the panes, so the
/// toast is overlaid on the right pane.
fn sidebar_toast_goes_to_the_content_pane(ui: &Ui) -> Result<()> {
    let window = ui.window("サイドバーとトースト", 640.0, 400.0)?;
    window.set_child(&ui.stack(Orientation::Vertical)?);
    let sidebar = ui.sidebar()?;
    window.set_sidebar(&sidebar);
    window.show();

    let toast = ui.toast("保存しました")?;
    toast.show();
    let view = toast.native_view().expect("出ていること");
    let mtm = MainThreadMarker::new().unwrap();
    let controller = sidebar.native_split_view_controller();
    let host = controller
        .splitViewItems()
        .objectAtIndex(1)
        .viewController(mtm)
        .view();
    assert!(contains_subview(&host, &view), "中身の区画に重なる");
    assert_eq!(
        controller.splitView().arrangedSubviews().len(),
        2,
        "区画は増えない"
    );
    toast.dismiss();
    window.clear_sidebar();
    window.close();
    Ok(())
}

/// Rebuilding the sidebar from inside the notification does not double-borrow.
fn sidebar_callback_is_reentrant(ui: &Ui) -> Result<()> {
    let sidebar = ui.sidebar()?;
    sidebar.set_items(&SidebarItem::list(["春", "夏", "秋"]));
    let seen = Rc::new(RefCell::new(Vec::new()));
    sidebar.on_select({
        let sidebar = sidebar.clone();
        let seen = seen.clone();
        move |index| {
            seen.borrow_mut().push(index);
            sidebar.set_items(&SidebarItem::list(["冬"]));
            let seen = seen.clone();
            sidebar.on_select(move |index| seen.borrow_mut().push(100 + index));
        }
    });
    sidebar.select(1);
    assert_eq!(sidebar.len(), 1);
    assert_eq!(sidebar.selected(), None, "組み替えで選択は外れる");
    sidebar.select(0);
    assert_eq!(*seen.borrow(), [1, 100], "差し替えた通知先が使われる");
    Ok(())
}

/// Reads the identifiers of the toolbar items in order.
fn toolbar_identifiers(window: &NSWindow) -> Vec<String> {
    window
        .toolbar()
        .map(|toolbar| {
            toolbar
                .items()
                .iter()
                .map(|item| item.itemIdentifier().to_string())
                .collect()
        })
        .unwrap_or_default()
}

/// Without an app toolbar a toolbar with just the button is attached; with one, the
/// button is inserted at its start. Detaching either restores the original.
fn sidebar_adds_the_toggle_button_to_the_toolbar(ui: &Ui) -> Result<()> {
    use objc2_app_kit::{
        NSToolbarSidebarTrackingSeparatorItemIdentifier, NSToolbarToggleSidebarItemIdentifier,
    };
    let toggle = unsafe { NSToolbarToggleSidebarItemIdentifier }.to_string();
    let tracking = unsafe { NSToolbarSidebarTrackingSeparatorItemIdentifier }.to_string();

    let window = ui.window("サイドバーボタン", 640.0, 400.0)?;
    window.set_child(&ui.stack(Orientation::Vertical)?);
    let native = window.native_window();
    let sidebar = ui.sidebar()?;
    window.set_sidebar(&sidebar);
    assert_eq!(
        toolbar_identifiers(&native),
        [toggle.clone(), tracking.clone()],
        "ツールバーが無くてもサイドバーボタンは出る"
    );
    assert_eq!(
        native.titleVisibility(),
        NSWindowTitleVisibility::Visible,
        "ボタンだけのときはタイトルを隠さない"
    );

    let toolbar = ui.toolbar()?;
    toolbar.set_items(&[
        ToolbarItem::new(ToolbarIcon::New, "新規"),
        ToolbarItem::new(ToolbarIcon::Open, "開く"),
    ]);
    window.set_toolbar(&toolbar);
    let ids = toolbar_identifiers(&native);
    assert_eq!(ids.len(), 4);
    assert_eq!(&ids[..2], [toggle.clone(), tracking.clone()], "先頭に入る");
    assert_eq!(toolbar.len(), 2, "naui の項目の数には数えない");
    assert!(
        toolbar.native_item(0).is_some(),
        "項目のインデックスはそのまま"
    );

    // アプリのツールバーを外しても、サイドバーボタンは残る。
    window.clear_toolbar();
    assert_eq!(
        toolbar_identifiers(&native),
        [toggle.clone(), tracking.clone()]
    );
    window.set_toolbar(&toolbar);

    window.clear_sidebar();
    assert_eq!(
        toolbar_identifiers(&native),
        ["naui.item.0", "naui.item.1"],
        "サイドバーを外すとボタンも消える"
    );
    window.clear_toolbar();
    assert!(native.toolbar().is_none());
    window.close();
    Ok(())
}

/// Collapsing with the sidebar button (`toggleSidebar:`) calls `on_collapse`, and
/// `set_collapsed` does not.
fn sidebar_toggle_notifies_collapse(ui: &Ui) -> Result<()> {
    let window = ui.window("サイドバーの開閉", 640.0, 400.0)?;
    window.set_child(&ui.stack(Orientation::Vertical)?);
    let sidebar = ui.sidebar()?;
    sidebar.set_items(&SidebarItem::list(["一般"]));
    window.set_sidebar(&sidebar);
    window.show();
    let seen = Rc::new(RefCell::new(Vec::new()));
    sidebar.on_collapse({
        let seen = seen.clone();
        move |collapsed| seen.borrow_mut().push(collapsed)
    });
    let resized = Rc::new(Cell::new(0));
    sidebar.on_resize({
        let resized = resized.clone();
        move |_| resized.set(resized.get() + 1)
    });

    // サイドバーボタンが送るのと同じ操作。
    let controller = sidebar.native_split_view_controller();
    unsafe { controller.toggleSidebar(None) };
    pump_until(3.0, || !seen.borrow().is_empty());
    assert!(sidebar.is_collapsed());
    assert_eq!(*seen.borrow(), [true], "閉じたことが届く");

    unsafe { controller.toggleSidebar(None) };
    pump_until(3.0, || seen.borrow().len() >= 2);
    assert!(!sidebar.is_collapsed());
    assert_eq!(*seen.borrow(), [true, false], "開いたことも届く");
    pump(0.5);
    assert_eq!(
        resized.get(),
        0,
        "開閉のアニメーションは幅の変更として通知しない"
    );
    assert!(
        (sidebar.width() - DEFAULT_SIDEBAR_WIDTH).abs() < 1.0,
        "開き直すと元の幅: {}",
        sidebar.width()
    );

    sidebar.set_collapsed(true);
    pump(0.5);
    sidebar.set_collapsed(false);
    pump(0.5);
    assert_eq!(seen.borrow().len(), 2, "set_collapsed では通知しない");

    // 通知の中から開閉し直しても、入れ子の通知で落ちない。
    sidebar.on_collapse({
        let sidebar = sidebar.clone();
        let seen = seen.clone();
        move |collapsed| {
            seen.borrow_mut().push(collapsed);
            sidebar.set_collapsed(false);
        }
    });
    unsafe { controller.toggleSidebar(None) };
    pump_until(3.0, || seen.borrow().len() >= 3);
    pump(0.5);
    assert_eq!(seen.borrow()[2], true);
    assert!(!sidebar.is_collapsed(), "通知の中で開き直せる");
    window.clear_sidebar();
    window.close();
    Ok(())
}

/// Narrowing the window from code collapses the sidebar instead of widening the
/// window to fit both panes, and widening it expands the sidebar again. A sidebar the
/// user collapsed stays collapsed.
fn sidebar_collapses_when_the_window_narrows(ui: &Ui) -> Result<()> {
    use naui_core::{SIDEBAR_COLLAPSE_CONTENT_WIDTH, SIDEBAR_MIN_WIDTH};
    use objc2_app_kit::NSSplitViewItemUnspecifiedDimension;

    let window = ui.window("サイドバーの自動の開閉", 800.0, 400.0)?;
    window.set_child(&ui.stack(Orientation::Vertical)?);
    let sidebar = ui.sidebar()?;
    sidebar.set_items(&SidebarItem::list(["一般"]));
    assert!(sidebar.auto_collapse(), "既定で自動で畳む");
    let controller = sidebar.native_split_view_controller();
    let items = controller.splitViewItems();
    assert!(items.objectAtIndex(0).canCollapseFromWindowResize());
    assert_eq!(
        items.objectAtIndex(1).minimumThickness(),
        SIDEBAR_COLLAPSE_CONTENT_WIDTH,
        "中身の区画に最小の幅を持たせる"
    );
    window.set_sidebar(&sidebar);
    window.show();
    pump(0.3);
    let seen = Rc::new(RefCell::new(Vec::new()));
    sidebar.on_collapse({
        let seen = seen.clone();
        move |collapsed| seen.borrow_mut().push(collapsed)
    });
    let narrow = SIDEBAR_MIN_WIDTH + SIDEBAR_COLLAPSE_CONTENT_WIDTH - 60.0;

    window.set_size(narrow, 400.0);
    assert!(sidebar.is_collapsed(), "狭めると畳まれる");
    assert_eq!(
        window.size(),
        (narrow, 400.0),
        "ウィンドウは両方の区画に合わせて広がらない"
    );
    pump_until(2.0, || !seen.borrow().is_empty());
    assert_eq!(*seen.borrow(), [true], "畳んだことは次の周回で届く");
    sidebar.set_auto_collapse(true);
    assert!(sidebar.is_collapsed(), "同じ値を入れ直しても畳んだまま");

    window.set_size(800.0, 400.0);
    pump_until(2.0, || !sidebar.is_collapsed());
    assert!(!sidebar.is_collapsed(), "広げると開き直す");
    assert_eq!(*seen.borrow(), [true, false]);
    let split = controller.splitView();
    split.layoutSubtreeIfNeeded();
    let pane = split.arrangedSubviews().objectAtIndex(0).frame().size.width;
    assert!(
        (pane - DEFAULT_SIDEBAR_WIDTH).abs() < 1.0,
        "元の幅で開く: {pane}"
    );

    // 狭いウィンドウへ付けても、ウィンドウは広がらず畳まれた姿で付く。
    window.clear_sidebar();
    window.set_size(narrow, 400.0);
    window.set_sidebar(&sidebar);
    assert!(sidebar.is_collapsed());
    assert_eq!(window.size(), (narrow, 400.0), "付けても広がらない");
    pump_until(2.0, || seen.borrow().len() >= 3);
    assert_eq!(*seen.borrow(), [true, false, true]);
    window.set_size(800.0, 400.0);
    pump_until(2.0, || !sidebar.is_collapsed());
    assert!(!sidebar.is_collapsed());
    pump(0.2);
    seen.borrow_mut().clear();

    // 利用者が閉じたものは、狭めて広げても開かない。
    unsafe { controller.toggleSidebar(None) };
    pump_until(3.0, || !seen.borrow().is_empty());
    window.set_size(narrow, 400.0);
    window.set_size(800.0, 400.0);
    pump(0.5);
    assert!(sidebar.is_collapsed(), "利用者が閉じたまま");
    assert_eq!(*seen.borrow(), [true], "利用者の操作だけが届く");
    sidebar.set_collapsed(false);
    pump_until(3.0, || {
        !split.isSubviewCollapsed(&split.arrangedSubviews().objectAtIndex(0))
    });

    // 自動の開閉が届く前に set_collapsed で決め直したら、そちらが勝ち、
    // 古い通知は届かない。
    seen.borrow_mut().clear();
    window.set_size(narrow, 400.0);
    window.set_size(800.0, 400.0);
    sidebar.set_collapsed(true);
    pump(0.3);
    assert!(sidebar.is_collapsed(), "開き直しの予約は取り消される");
    assert!(
        seen.borrow().is_empty(),
        "決め直した後に届いた: {:?}",
        seen.borrow()
    );
    sidebar.set_collapsed(false);
    pump(0.1);
    window.set_size(narrow, 400.0);
    sidebar.set_collapsed(false);
    pump(0.3);
    assert!(!sidebar.is_collapsed());
    assert!(
        seen.borrow().is_empty(),
        "畳んだ通知は捨てられる: {:?}",
        seen.borrow()
    );
    window.set_size(800.0, 400.0);
    pump(0.1);

    // 切ると畳まず、中身の区画の最小も外す。
    sidebar.set_auto_collapse(false);
    assert!(!sidebar.auto_collapse());
    assert!(!items.objectAtIndex(0).canCollapseFromWindowResize());
    assert_eq!(items.objectAtIndex(1).minimumThickness(), unsafe {
        NSSplitViewItemUnspecifiedDimension
    });
    window.set_size(narrow, 400.0);
    pump(0.3);
    assert!(!sidebar.is_collapsed(), "切っていれば畳まない");
    assert_eq!(window.size(), (narrow, 400.0));

    // 狭い窓で入れると、窓を広げずに畳む。
    seen.borrow_mut().clear();
    sidebar.set_auto_collapse(true);
    assert!(sidebar.is_collapsed(), "入れると畳まれる");
    pump(0.3);
    assert_eq!(window.size(), (narrow, 400.0), "入れても広がらない");
    assert_eq!(*seen.borrow(), [true]);
    window.clear_sidebar();
    window.close();
    Ok(())
}

/// While the user resizes the window, AppKit itself collapses the sidebar
/// (`canCollapseFromWindowResize`) and expands it again, and naui notifies both.
///
/// AppKit only does this during live resize, which only the window server starts
/// from a real drag of the window edge. The private `_startLiveResize` /
/// `_endLiveResize` reproduce that state; if they ever go away, this is skipped.
fn sidebar_collapses_during_live_resize(ui: &Ui) -> Result<()> {
    use naui_core::{SIDEBAR_COLLAPSE_CONTENT_WIDTH, SIDEBAR_MIN_WIDTH};

    let window = ui.window("ライブリサイズ", 800.0, 400.0)?;
    window.set_child(&ui.stack(Orientation::Vertical)?);
    let sidebar = ui.sidebar()?;
    sidebar.set_items(&SidebarItem::list(["一般"]));
    window.set_sidebar(&sidebar);
    window.show();
    pump(0.3);
    let native = window.native_window();
    let live: bool = unsafe { msg_send![&*native, respondsToSelector: sel!(_startLiveResize)] };
    if !live {
        skip("ライブリサイズを起こせない");
        window.close();
        return Ok(());
    }
    let seen = Rc::new(RefCell::new(Vec::new()));
    sidebar.on_collapse({
        let seen = seen.clone();
        move |collapsed| seen.borrow_mut().push(collapsed)
    });
    let resize = |width: f64| {
        let mut frame = native.frame();
        frame.size.width = width;
        native.setFrame_display(frame, true);
        pump(0.02);
    };
    let threshold = SIDEBAR_MIN_WIDTH + SIDEBAR_COLLAPSE_CONTENT_WIDTH;
    unsafe {
        let _: () = msg_send![&*native, _startLiveResize];
    }
    let mut width = 800.0;
    while width > threshold - 60.0 {
        width -= 10.0;
        resize(width);
    }
    assert!(
        sidebar.is_collapsed(),
        "狭めると畳まれる: frame={:?} split={:?} seen={:?}",
        native.frame().size,
        sidebar
            .native_split_view_controller()
            .splitView()
            .frame()
            .size,
        seen.borrow()
    );
    while width < 800.0 {
        width += 10.0;
        resize(width);
    }
    unsafe {
        let _: () = msg_send![&*native, _endLiveResize];
    }
    pump(0.3);
    assert!(!sidebar.is_collapsed(), "広げると開き直す");
    assert_eq!(*seen.borrow(), [true, false], "どちらも届く");
    assert!(
        (sidebar.width() - DEFAULT_SIDEBAR_WIDTH).abs() < 1.0,
        "覚えている幅は変わらない"
    );

    // コードで狭めて畳んだものは AppKit が開き直さないので、利用者が広げ終えたら
    // naui が開く。
    seen.borrow_mut().clear();
    window.set_size(threshold - 60.0, 400.0);
    assert!(sidebar.is_collapsed());
    pump(0.1);
    unsafe {
        let _: () = msg_send![&*native, _startLiveResize];
    }
    let mut width = native.frame().size.width;
    while width < 800.0 {
        width += 10.0;
        resize(width);
    }
    unsafe {
        let _: () = msg_send![&*native, _endLiveResize];
    }
    pump_until(2.0, || !sidebar.is_collapsed());
    assert!(!sidebar.is_collapsed(), "広げ終えると開く");
    assert_eq!(*seen.borrow(), [true, false]);
    window.clear_sidebar();
    window.close();
    Ok(())
}

/// Grabs the split view's first divider, moves it `dx` horizontally and releases
/// it.
///
/// The events are queued before the press is delivered, so the `NSSplitView`'s
/// tracking loop reads the following events just as in a real drag.
///
/// The press goes straight to the view under the divider rather than through
/// `NSApplication::sendEvent`. `NSWindow` drops a press on a window that is not on
/// screen, and the test's window is not when another app holds a fullscreen Space
/// in front (or the window itself opens into a fullscreen Space), so the drag would
/// silently do nothing depending on what else is on the screen.
fn drag_divider(
    app: &NSApplication,
    window: &NSWindow,
    split: &objc2_app_kit::NSSplitView,
    dx: f64,
) {
    split.layoutSubtreeIfNeeded();
    let first = split.arrangedSubviews().objectAtIndex(0).frame();
    let y = split.bounds().size.height / 2.0;
    let x = first.origin.x + first.size.width + split.dividerThickness() / 2.0;
    let start = split.convertPoint_toView(NSPoint::new(x, y), None);
    let event = |kind, point: NSPoint| {
        NSEvent::mouseEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_clickCount_pressure(
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
        .expect("マウスイベント")
    };
    let steps = 4;
    for step in 1..=steps {
        let point = NSPoint::new(start.x + dx * f64::from(step) / f64::from(steps), start.y);
        unsafe { app.postEvent_atStart(&event(NSEventType::LeftMouseDragged, point), false) };
    }
    let end = NSPoint::new(start.x + dx, start.y);
    unsafe { app.postEvent_atStart(&event(NSEventType::LeftMouseUp, end), false) };
    // ウィンドウの枠のビューから当てて、本物の押下と同じ受け口を選ぶ。
    let frame_view = unsafe { window.contentView().expect("contentView").superview() }
        .expect("ウィンドウの枠のビュー");
    let target = frame_view.hitTest(start).expect("仕切りの上のビュー");
    assert!(
        target.isDescendantOf(split),
        "仕切りの上は NSSplitView: {:?}",
        target.class().name()
    );
    target.mouseDown(&event(NSEventType::LeftMouseDown, start));
    deliver_events(app);
    pump(0.05);
}

/// A width set with `set_width` while collapsed is applied to the pane on
/// re-expanding, and the user's later drags also reach `on_resize`.
///
/// Re-expanding is checked both with `set_collapsed(false)` and the sidebar button
/// (`toggleSidebar:`). When expanding via the button, AppKit restores the width it
/// remembered before collapsing.
fn sidebar_width_set_while_collapsed_applies_on_reopen(ui: &Ui) -> Result<()> {
    let window = ui.window("閉じている間の幅", 800.0, 400.0)?;
    window.set_child(&ui.stack(Orientation::Vertical)?);
    let sidebar = ui.sidebar()?;
    sidebar.set_items(&SidebarItem::list(["一般"]));
    window.set_sidebar(&sidebar);
    window.show();

    let controller = sidebar.native_split_view_controller();
    let split = controller.splitView();
    let pane = || {
        split.layoutSubtreeIfNeeded();
        split.arrangedSubviews().objectAtIndex(0).frame().size.width
    };
    let collapsed = Rc::new(RefCell::new(Vec::new()));
    sidebar.on_collapse({
        let collapsed = collapsed.clone();
        move |value| collapsed.borrow_mut().push(value)
    });
    let resized = Rc::new(RefCell::new(Vec::new()));
    sidebar.on_resize({
        let resized = resized.clone();
        move |width| resized.borrow_mut().push(width)
    });

    // set_collapsed(false) で開き直す。
    sidebar.set_width(260.0);
    sidebar.set_collapsed(true);
    sidebar.set_width(300.0);
    assert!(sidebar.is_collapsed(), "幅を置いても閉じたまま");
    assert_eq!(sidebar.width(), 300.0);
    sidebar.set_collapsed(false);
    assert!(
        (pane() - 300.0).abs() < 1.0,
        "開き直すと新しい幅: {}",
        pane()
    );

    // その後の利用者のドラッグも届く。
    let app = NSApplication::sharedApplication(MainThreadMarker::new().unwrap());
    drag_divider(&app, &window.native_window(), &split, -40.0);
    assert!(
        !resized.borrow().is_empty(),
        "開き直したあとのドラッグも通知する"
    );
    assert!(
        (sidebar.width() - pane()).abs() < 1.0,
        "動かした幅を返す (画素の境目へのそろえ分まで)"
    );

    // サイドバーボタンで開き直しても、閉じている間に置いた幅になる。
    sidebar.set_collapsed(true);
    sidebar.set_width(320.0);
    unsafe { controller.toggleSidebar(None) };
    pump_until(3.0, || !sidebar.is_collapsed());
    pump(0.5);
    assert!(!sidebar.is_collapsed());
    assert!(
        (pane() - 320.0).abs() < 1.0,
        "ボタンで開いても新しい幅: {}",
        pane()
    );
    assert_eq!(
        *collapsed.borrow(),
        [false],
        "閉じている間に幅を置いても開閉は通知せず、ボタンで開いたことだけが届く"
    );
    window.clear_sidebar();
    window.close();
    Ok(())
}
