//! Behavior checks against real GTK4 / libadwaita controls.
//!
//! They trigger **native-side operations** such as `emit_clicked` or changing a
//! `GtkAdjustment`'s value, and check that they reach Rust closures and that the
//! native state changes. Size specifications are checked with the results of
//! `gtk_widget_measure`.
//!
//! GTK4 can only be touched on the main thread, but Rust's standard test harness
//! runs each test on a separate thread (even with `--test-threads=1`). So
//! `harness = false` is set and a custom runner runs on the main thread.
//!
//! A display (Wayland or X11) is required. Without one, `gtk_init` fails.

use std::cell::{Cell, RefCell};
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::task::{Context, Poll};

use adw::prelude::*;
use gtk::gio;
use gtk::glib;
use gtk::pango;
use naui_core::{
    Align, ButtonRole, Color, DatePickerMode, DateTime, DialogButtons, DialogResponse,
    EventResponse, FileFilter, FilePickerMode, Fit, GridCell, Length, ListItem, MenuItem,
    MenuShortcut, MenuSpec, NavItem, Orientation, Padding, Pane, PlaybackState, Point,
    PointerPhase, PopupItem, Rect, Result, ScrollPolicy, SelectionMode, SidebarItem,
    SidebarSection, Sizing, SortOrder, TableColumn, TableRow, TextColor, TextStyle, Theme, Time,
    ToolbarIcon, ToolbarItem, Track, TreeItem, WindowPlacement, WindowState, DEFAULT_SIDEBAR_WIDTH,
};
use naui_gtk::{run_for_test, ListRow, TableCells, Ui, Widget};

/// One test case.
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
            "描画面が on_draw の命令を cairo で画素に落とす",
            canvas_paints_recorded_commands,
        ),
        (
            "描画面がポインターの押下・移動・解放を面の座標で通知する",
            canvas_reports_pointer_events,
        ),
        (
            "描画面がホイールを論理ピクセルで通知し、Handled なら伝播を止める",
            canvas_reports_wheel,
        ),
        (
            "描画面が Handled で受け終えたら外側の慣性スクロールを取り消す",
            canvas_cancels_outer_kinetic_after_handled,
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
        (
            "1 行入力とパスワード入力が Enter で確定を通知する",
            text_inputs_notify_on_activate,
        ),
        (
            "数値入力が Enter で値を確定してから通知する",
            number_input_commits_before_submitting,
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
            "狭い場所を配られても中身を潰さない",
            a_narrow_allocation_does_not_squash_the_content,
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
            "Fill で最小より細くされても中身を最小より細く測らない",
            fill_does_not_measure_content_below_its_minimum,
        ),
        (
            "グリッドの Fill の列が余りを受け取る",
            grid_fill_track_expands_child,
        ),
        (
            "グリッドの Fill の列と行が重みで余りを分ける",
            grid_fill_weights_split_the_room,
        ),
        (
            "グリッドの Fill の列は中身の最小を下回らず、残りを分け直す",
            grid_fill_keeps_the_content_minimum,
        ),
        (
            "グリッドの余りは Fill の列だけが受け取り、自然な幅で偏らない",
            grid_fill_takes_the_rest_from_auto_columns,
        ),
        (
            "グリッドの Fill の列は場所が足りる限り中身を切らない",
            grid_fill_does_not_cut_off_content_while_there_is_room,
        ),
        (
            "グリッドをまたぐ子が足りない分は Fill の重みで配る",
            grid_spanning_child_spreads_by_weight,
        ),
        (
            "重なるスパンの拡張要求が追加順によらず Auto の行と列へ届く",
            grid_overlapping_spans_expand_independently_of_attach_order,
        ),
        (
            "グリッドの折り返す文字は配られる幅で高さを測る",
            grid_measures_wrapping_text_at_its_share,
        ),
        (
            "右から左の文字ではグリッドの列が反転する",
            grid_mirrors_columns_right_to_left,
        ),
        (
            "グリッドの中身に合わせる子がマスの先頭に置かれる",
            grid_places_auto_children_at_the_start,
        ),
        (
            "グリッドの大きさを決めた子もマスの先頭に置かれる",
            grid_places_sized_children_at_the_start,
        ),
        (
            "グリッドの固定幅の列が子の幅を決める",
            grid_fixed_track_sets_width,
        ),
        (
            "追加したあとの set_sizing でもスタックの寄せ方が残る",
            stack_align_survives_later_sizing,
        ),
        (
            "スタックの向きを変えると最初からその向きで作ったのと同じ置き方になる",
            stack_switches_orientation,
        ),
        (
            "しきい値を持つスタックが自分の幅で向きを切り替え、境目では遊びを持つ",
            stack_breakpoint_follows_its_width,
        ),
        (
            "一覧と詳細が幅に応じて並び、1 区画のときは戻るボタンで一覧へ戻る",
            adaptive_panes_follow_their_width,
        ),
        (
            "置いたあとの set_sizing や列の変更でもグリッドの列の設定が残る",
            grid_track_survives_later_changes,
        ),
        (
            "別のコンテナへ移した子は前の親の設定を持ち越さない",
            moved_child_drops_the_old_placement,
        ),
        (
            "ウィンドウに入れたあとの set_sizing でも中身が窓いっぱいに残る",
            window_child_fills_after_later_sizing,
        ),
        (
            "移した子はスクロール位置を保ち、フォーカスは外れる",
            moved_widget_keeps_scroll_but_not_focus,
        ),
        ("どのコンテナから外しても再追加でフォーカスを持ち越さない", containers_release_focus_before_detaching),
        (
            "スクロールが中身とポリシーを保つ",
            scroll_keeps_child_and_policy,
        ),
        (
            "スクロールの中で高さを指定しない中身が縮まない",
            scroll_gives_the_child_its_natural_height,
        ),
        (
            "隠したものは Stack の中で場所を空けない",
            hidden_widget_leaves_the_stack_layout,
        ),
        (
            "フォーカスが入力欄と複数行入力の中へ移る",
            request_focus_reaches_the_editor,
        ),
        (
            "フォーカスの出入りが持ち主の変わったときだけ通知される",
            focus_change_follows_the_focus_widget,
        ),
        (
            "ウィジェットを捨てると on_focus_change の通知先も解放される",
            focus_change_handler_is_freed_with_the_widget,
        ),
        ("ツールチップが中身のコントロールへ届く", tooltip_reaches_the_widget),
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
        ("タイマーがメインループで呼ばれ、止められる", timers_fire_on_the_main_loop),
        (
            "不確定の進捗は値を覚えたまま pulse に切り替わる",
            progress_bar_switches_to_indeterminate,
        ),
        (
            "スピナーは AdwSpinner (1.6 より前は GtkSpinner) で、止めると描かずに場所を残す",
            spinner_stops_without_giving_up_its_space,
        ),
        (
            "スピナーはネイティブの最小より小さい場所にも収まる",
            spinner_fits_below_native_minimum,
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
            "表示中に帯を付けてすぐ set_size しても中身が指定どおりになる",
            window_set_size_right_after_adding_bars,
        ),
        (
            "閉じる確認で KeepOpen を返すと閉じない",
            window_close_request_can_keep_it_open,
        ),
        (
            "ウィンドウの位置は取れず、動かしても何も起きない",
            window_position_is_unavailable,
        ),
        (
            "ウィンドウの最大化・全画面が往復し通知される",
            window_state_round_trips,
        ),
        (
            "コンポジタが応える前に続けて頼んでも最後の状態になる",
            window_state_requests_in_a_row,
        ),
        (
            "表示する前に頼んだ状態で表示される",
            window_state_requested_before_show,
        ),
        (
            "保存した大きさと状態は最大化の間も通常の大きさを指し、位置は持たない",
            window_placement_keeps_the_normal_size,
        ),
        (
            "最小化して元に戻せる (X11)",
            window_minimize_round_trips,
        ),
        (
            "表示する前に頼んだ最小化は表示してから当たる (X11)",
            window_minimize_requested_before_show,
        ),
        (
            "大きさを変えられるかどうかがネイティブと往復する",
            window_resizable_round_trips,
        ),
        (
            "読み取り専用と最小の大きさがネイティブへ届く",
            read_only_and_min_size,
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
            "中身を足した直後でも、次のレイアウトの後に末尾まで送る",
            scroll_to_end_after_appending,
        ),
        (
            "表示前に頼んだ末尾送りが表示後に効く",
            scroll_to_end_before_shown,
        ),
        (
            "隠れたタブの中で足して頼んだ末尾送りがタブを戻すと効く",
            scroll_to_end_in_hidden_tab,
        ),
        (
            "スクロールの通知は位置が変わったときだけ届く",
            scroll_notifies_only_moves,
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
            "ラベルの段階と役割が libadwaita のスタイルクラスへ写る",
            label_style_and_color_map_to_style_classes,
        ),
        (
            "ボタンの役割が libadwaita のスタイルクラスへ写る",
            button_role_maps_to_style_classes,
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
            "サイドバーが navigation-sidebar の GtkListBox に項目と見出しを並べ、仕切りで幅を変えられる",
            sidebar_rows_map_to_a_list_box,
        ),
        (
            "サイドバーの選択が通し番号で往復し、利用者の選択だけを通知する",
            sidebar_selection_round_trips,
        ),
        (
            "サイドバーが GtkPaned として取り付き外れる",
            sidebar_attaches_to_a_window,
        ),
        (
            "サイドバーの通知中に内容と通知先を差し替えられる",
            sidebar_callback_is_reentrant,
        ),
        (
            "サイドバーボタンが開いている間はサイドバー、閉じたら中身の左上に入り、開閉を通知する",
            sidebar_toggle_button_collapses_and_notifies,
        ),
        (
            "サイドバーがウィンドウを狭めると畳まれ、広げると開き直す",
            sidebar_collapses_when_the_window_narrows,
        ),
        (
            "サイドバーを自動で畳む予約は、行う前に窓が戻ったら取り消される",
            sidebar_drops_a_stale_auto_collapse,
        ),
        (
            "中身に最小幅があっても、サイドバーは元の幅で畳むかを決め、幅を保つ",
            sidebar_collapses_beside_content_with_a_min_width,
        ),
        (
            "すべてのアイコンがテーマに実在する",
            toolbar_icons_exist_in_the_theme,
        ),
        (
            "ツールバーの通知内で組み替えと差し替えができる",
            toolbar_callback_is_reentrant_and_replaceable,
        ),
        (
            "メニューバーの見出しと項目が GMenu と GAction になる",
            menu_bar_menus_map_to_native,
        ),
        (
            "メニューバーの項目が経路で通知する",
            menu_bar_activation_notifies,
        ),
        (
            "メニューバーのサブメニューとチェックが GMenu と操作になる",
            menu_bar_submenus_and_checks_map_to_native,
        ),
        (
            "メニューバーのファンクションキーがアクセラレータになる",
            menu_bar_function_keys_become_accelerators,
        ),
        (
            "メニューバーがウィンドウに取り付き、操作が引ける",
            menu_bar_attaches_to_the_window,
        ),
        (
            "メニューバーの通知内で操作と差し替えができる",
            menu_bar_callback_is_reentrant_and_replaceable,
        ),
        (
            "別のウィンドウのメニューバーは同じショートカットを上書きしない",
            menu_bar_shortcuts_are_kept_per_menu_bar,
        ),
        ("メニューの選択が 1 つだけ点く", menu_selection_is_exclusive),
        ("選べない項目は選ばれない", nav_skips_disabled_items),
        ("パンくずが末尾を現在地にする", breadcrumbs_last_is_current),
        (
            "パンくずのリンクは右クリックのメニューを出さない",
            breadcrumbs_have_no_context_menu,
        ),
        ("ページ送りが範囲内に収まる", pagination_steps),
        ("タブが中身ごと切り替わる", tabs_selection),
        ("タブを外して空にできる", tabs_remove_and_clear),
        ("タブを閉じるフォーカス通知内でタブ一覧を変更できる", tabs_remove_after_focus_callback),
        (
            "スタックが子を差し込み・外し・空にできる",
            stack_inserts_and_removes_children,
        ),
        (
            "グリッドがマス単位で子を外せる",
            grid_removes_children_by_cell,
        ),
        ("リンクのクリックがクロージャへ届く", link_click),
        ("リストの行が GtkListBox に並ぶ", list_rows_are_native),
        (
            "リストの補助の文字が 2 行目に出る",
            list_detail_makes_a_second_line,
        ),
        (
            "リストの長い文字が行の幅に収まる",
            list_long_text_stays_inside_the_row,
        ),
        (
            "リストの行の中身が行の幅いっぱいに置かれる",
            list_row_content_fills_the_row,
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
            "clone した Ui で行を後から作れる",
            ui_clone_builds_rows_from_a_callback,
        ),
        (
            "行のクリックがクロージャへ届く",
            list_row_activation_notifies,
        ),
        (
            "リストのダブルクリックと Enter が行の起動を通知する",
            list_row_activate_notifies,
        ),
        (
            "リストの右クリックが行と位置を通知する",
            list_context_menu_notifies,
        ),
        (
            "複数選択でクリックの修飾キーが GTK4 へ届く",
            list_multiple_selection_reaches_the_modifier_keys,
        ),
        ("リストが選べない行を飛ばす", list_skips_disabled_rows),
        (
            "一覧・表・ツリーはフォーカスが入っただけでは行を選ばない",
            focus_entering_rows_does_not_select,
        ),
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
        (
            "テーブルが見えている行だけを組み立てる",
            table_builds_only_the_visible_rows,
        ),
        (
            "テーブルのセルにウィジェットを置ける",
            table_widget_cells_are_native,
        ),
        (
            "テーブルの行の高さを決められる",
            table_row_height_can_be_fixed,
        ),
        (
            "行を絞っていても見出しの並べ替えが届く",
            table_sorting_works_while_windowed,
        ),
        (
            "組み立てる行でも行数と並べ替えに耐える",
            table_builder_rows_scale_and_sort,
        ),
        (
            "ウィジェットのセルでも列の幅がスクロールで動かない",
            table_widget_cells_keep_the_column_width,
        ),
        (
            "選べるかどうかを行を組み立てずに答えられる",
            table_row_selectable_answers_without_building,
        ),
        (
            "ツリーの行の中身が行の幅いっぱいに置かれる",
            tree_row_content_fills_the_row,
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
            "ポップオーバーがウィジェットに付いて中身を出し、閉じても通知しない",
            popover_shows_its_content_beside_the_widget,
        ),
        (
            "利用者がポップオーバーを閉じると on_close が呼ばれる",
            popover_dismissed_by_the_user_notifies,
        ),
        (
            "ポップオーバーの向きが GtkPopover へ届く",
            popover_position_reaches_the_native,
        ),
        (
            "ポップオーバーを別のウィジェットへ移しても通知しない",
            popover_moves_between_widgets,
        ),
        (
            "スクロールに付けたポップオーバーが中身の伸びに追従する",
            popover_follows_its_content_on_a_scroll,
        ),
        (
            "画面に出ていないウィジェットにはポップオーバーを開かない",
            popover_needs_a_shown_widget,
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
            "ポップアップのサブメニューとチェックが GMenu と操作になる",
            popup_submenus_and_checks,
        ),
        (
            "画像がローカルのファイルから読み込まれる",
            image_loads_a_local_file,
        ),
        (
            "Fill の画像はその軸で画素の大きさを求めない",
            fill_image_does_not_ask_for_its_size,
        ),
        (
            "Fill の動画はその軸で映像の大きさを求めない",
            fill_video_does_not_ask_for_its_size,
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
            "シートが AdwDialog になり、ボタンを見出しの帯に並べる",
            sheet_builds_an_adw_dialog,
        ),
        (
            "シートのボタンと Esc が役割を通知して閉じる",
            sheet_buttons_and_escape_respond,
        ),
        (
            "シートのボタンの有効・無効が開いている間も届く",
            sheet_button_enabled_reaches_the_buttons,
        ),
        (
            "シートの中身を差し替えたときのフォーカス通知の中で閉じ・差し替えられる",
            sheet_set_child_allows_changes_in_the_focus_notification,
        ),
        (
            "開いた直後に閉じたシートは残らない",
            sheet_closed_right_after_opening_does_not_stay,
        ),
        (
            "開いた直後に閉じたダイアログは残らない",
            dialog_closed_right_after_opening_does_not_stay,
        ),
        (
            "同じ処理の中で開き直したダイアログは前のダイアログの応答を受けない",
            dialog_reopened_in_the_same_turn_ignores_the_old_response,
        ),
        (
            "同じ処理の中で開き直したシートは前のシートの閉じを受けない",
            sheet_reopened_in_the_same_turn_ignores_the_old_close,
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
            "優先言語は gettext と同じ環境変数から BCP 47 で読める",
            preferred_languages_come_from_the_environment,
        ),
        (
            "落とされたファイルがウィジェットと窓の受け口へ届く",
            file_drop_reaches_the_drop_targets,
        ),
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
    let mut skipped = 0;
    for (name, case) in cases {
        take_skip();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            run_for_test(*case).expect("テスト用の起動")
        }));
        match (result, take_skip()) {
            (Ok(()), None) => println!("ok   ... {name}"),
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
    if failed > 0 {
        std::process::exit(1);
    }
}

thread_local! {
    /// Where **what the test could not verify** is recorded ([`skip`]).
    static SKIP_REASON: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// Records something that could not be verified because of the environment.
///
/// A test with a record is counted as SKIP rather than success and left in the
/// output, but unlike the macOS tests it does not make the exit code fail. CI's
/// Xvfb has no window manager, so nobody honors maximize or fullscreen requests
/// after showing, and failing would make CI fail every time.
fn skip(reason: &str) {
    SKIP_REASON.with(|slot| *slot.borrow_mut() = Some(reason.to_string()));
}

/// Takes out the record and resets it to empty.
fn take_skip() -> Option<String> {
    SKIP_REASON.with(|slot| slot.borrow_mut().take())
}

// --------------------------------------------------------------- 小さな道具

/// A container for counting "how many times it was called and what arrived last".
fn recorder<T: Clone + 'static>() -> (Rc<RefCell<Vec<T>>>, impl FnMut(T) + 'static) {
    let log = Rc::new(RefCell::new(Vec::new()));
    let sink = log.clone();
    (log, move |value| sink.borrow_mut().push(value))
}

/// The (minimum, natural) width.
fn measure_width(widget: &impl IsA<gtk::Widget>) -> (i32, i32) {
    let (min, nat, _, _) = widget.as_ref().measure(gtk::Orientation::Horizontal, -1);
    (min, nat)
}

/// The numbers the indicator position was based on.
///
/// The answer depends on font metrics, so this helps tell environment differences
/// apart when it fails (no font with Japanese glyphs, a default font so small there
/// is no room to lower the indicator, and so on).
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

/// The (minimum, natural) height.
fn measure_height(widget: &impl IsA<gtk::Widget>) -> (i32, i32) {
    let (min, nat, _, _) = widget.as_ref().measure(gtk::Orientation::Vertical, -1);
    (min, nat)
}

/// The (minimum, natural) height for a given width.
fn measure_height_for(widget: &impl IsA<gtk::Widget>, width: i32) -> (i32, i32) {
    let (min, nat, _, _) = widget.as_ref().measure(gtk::Orientation::Vertical, width);
    (min, nat)
}

/// The container (`SizeBin`) a widget is wrapped in when put into a container.
fn bin_of(widget: &dyn Widget) -> gtk::Widget {
    widget
        .native_widget()
        .ancestor(naui_gtk::SizeBin::static_type())
        .expect("SizeBin に包まれている")
}

/// Collects the `GtkLabel`s inside a widget in order, descending through nesting.
///
/// Row contents are built from naui's `Stack` and `Label`, so the text is not
/// directly under the row. Only the sequence of text matters here, so the depth
/// does not.
fn labels_in(widget: &impl IsA<gtk::Widget>) -> Vec<gtk::Label> {
    let mut found = Vec::new();
    for child in children(widget) {
        match child.downcast::<gtk::Label>() {
            Ok(label) => found.push(label),
            Err(other) => found.extend(labels_in(&other)),
        }
    }
    found
}

/// Collects the children of a `GtkBox` and the like, in order.
fn children(widget: &impl IsA<gtk::Widget>) -> Vec<gtk::Widget> {
    let mut out = Vec::new();
    let mut child = widget.as_ref().first_child();
    while let Some(current) = child {
        child = current.next_sibling();
        out.push(current);
    }
    out
}

/// A 1x1 PNG. Written out to check image loading with a real file.
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

/// The switch is a `GtkSwitch` itself, with the label placed next to it.
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

/// The drawing surface is a `GtkDrawingArea`, and the commands recorded by
/// `on_draw` are painted with cairo. The widget's snapshot is drawn into a cairo
/// image, and pixels inside and outside the painted rectangle are read.
fn canvas_paints_recorded_commands(ui: &Ui) -> Result<()> {
    let canvas = ui.canvas()?;
    canvas.set_sizing(
        Sizing::new()
            .width(Length::Fixed(120.0))
            .height(Length::Fixed(80.0)),
    );
    let (sizes, sink) = recorder::<(f64, f64)>();
    canvas.on_draw({
        let mut sink = sink;
        move |painter| {
            sink((painter.width(), painter.height()));
            painter.fill_rect(painter.bounds(), Color::WHITE);
            painter.fill_rect(Rect::new(10.0, 10.0, 40.0, 20.0), Color::rgb(0xff, 0, 0));
            painter.set_text_align(Align::End);
            painter.text("naui", Point::new(110.0, 50.0), 12.0, Color::BLACK);
        }
    });

    let window = ui.window("canvas", 200.0, 160.0)?;
    window.set_child(&canvas);
    window.show();
    let area = canvas.native_area();
    tick(&area);
    assert_eq!(canvas.size(), (120.0, 80.0), "指定した大きさになること");

    // 描画をレンダーノードへ取り、cairo の画像へ落として画素を読む。
    let snapshot = gtk::Snapshot::new();
    bin_of(&canvas).snapshot_child(&area, &snapshot);
    let node = snapshot.to_node().expect("描いたものがあること");
    let mut surface =
        gtk::cairo::ImageSurface::create(gtk::cairo::Format::ARgb32, 120, 80).expect("画像の生成");
    let cr = gtk::cairo::Context::new(&surface).expect("コンテキストの生成");
    node.draw(&cr);
    drop(cr);
    surface.flush();
    let stride = surface.stride() as usize;
    let data = surface.data().expect("画素の読み出し");
    // ARGB32 はネイティブのエンディアンで 1 画素 4 バイト (BGRA の並び)。
    let read = |x: usize, y: usize| -> (u8, u8, u8) {
        let i = y * stride + x * 4;
        (data[i + 2], data[i + 1], data[i])
    };

    assert!(
        sizes.borrow().iter().all(|&size| size == (120.0, 80.0)),
        "面の大きさで呼ばれること: {:?}",
        sizes.borrow()
    );
    assert!(!sizes.borrow().is_empty(), "描くときに呼ばれること");
    assert_eq!(read(20, 15), (255, 0, 0), "矩形の中は赤いこと");
    assert_eq!(read(80, 60), (255, 255, 255), "矩形の外は白いこと");

    // 右寄せの文字は x=110 より左にだけ描かれる。
    let dark = |x: usize, y: usize| {
        let (r, g, b) = read(x, y);
        u32::from(r) + u32::from(g) + u32::from(b) < 600
    };
    let found = (85..110).any(|x| (50..64).any(|y| dark(x, y)));
    assert!(found, "文字が右寄せで描かれていること");
    assert!(
        (50..64).all(|y| !dark(114, y)),
        "右端より右には描かないこと"
    );
    window.close();
    Ok(())
}

/// Pointer presses, moves and releases arrive with positions relative to the
/// surface's top left.
///
/// GTK4 has no public API for feeding pointer events to a widget
/// (`gtk_test_widget_click` is gone in GTK4, and apps cannot create `GdkEvent`s).
/// So after checking which controllers are attached, their signals are emitted to
/// see how the notifications are mapped.
///
/// - `GtkGestureClick`: emits `pressed` at the moment of pressing (without waiting
///   for drag detection). Set not to distinguish buttons
/// - `GtkGestureDrag`: movement while pressed. Delivered even outside the surface
/// - `GtkEventControllerMotion`: movement while not pressed (hover). Silent while
///   pressed so as not to duplicate `drag-update`
fn canvas_reports_pointer_events(ui: &Ui) -> Result<()> {
    let canvas = ui.canvas()?;
    let (log, sink) = recorder::<naui_core::PointerEvent>();
    canvas.on_pointer(sink);

    let area = canvas.native_area();
    let mut click = None;
    let mut drag = None;
    let mut motion = None;
    // `observe_controllers` の項目の型は `GObject` として申告されるので、
    // 1 つずつ取り出して見分ける。
    let controllers = area.observe_controllers();
    for index in 0..controllers.n_items() {
        let Some(c) = controllers.item(index) else {
            continue;
        };
        if let Ok(g) = c.clone().downcast::<gtk::GestureClick>() {
            click = Some(g);
        } else if let Ok(d) = c.clone().downcast::<gtk::GestureDrag>() {
            drag = Some(d);
        } else if let Ok(m) = c.downcast::<gtk::EventControllerMotion>() {
            motion = Some(m);
        }
    }
    let click = click.expect("押下と解放を受ける GtkGestureClick が付いていること");
    let drag = drag.expect("押している間の移動を受ける GtkGestureDrag が付いていること");
    let motion = motion.expect("ホバーを受ける GtkEventControllerMotion が付いていること");
    assert_eq!(
        click.button(),
        0,
        "どのボタンでも受け、右ボタンや中ボタンでも届くこと"
    );
    assert_eq!(drag.button(), 0, "ドラッグもどのボタンでも受けること");

    // 押していない間の移動はホバーとして届く。
    motion.emit_by_name::<()>("motion", &[&5.0f64, &6.0f64]);

    // 押してから動かす。`drag-update` は始点からのずれで届く (シグナルを直接
    // 起こしたときは始点を持っていないので 0 として扱われる)。面の外
    // (負の座標) へ出た移動も届く。
    click.emit_by_name::<()>("pressed", &[&1i32, &30.0f64, &20.0f64]);
    drag.emit_by_name::<()>("drag-begin", &[&30.0f64, &20.0f64]);
    drag.emit_by_name::<()>("drag-update", &[&50.0f64, &40.0f64]);
    motion.emit_by_name::<()>("motion", &[&51.0f64, &41.0f64]);
    drag.emit_by_name::<()>("drag-update", &[&-10.0f64, &-10.0f64]);
    drag.emit_by_name::<()>("drag-end", &[&-10.0f64, &-10.0f64]);
    click.emit_by_name::<()>("released", &[&1i32, &-10.0f64, &-10.0f64]);

    // 離したあとの移動はまたホバーとして届く。
    motion.emit_by_name::<()>("motion", &[&7.0f64, &8.0f64]);

    // 親のスクロールなどにシーケンスを奪われると `drag-end` は来ず `cancel` に
    // なる。そのあともホバーが止まらないこと。
    drag.emit_by_name::<()>("drag-begin", &[&1.0f64, &1.0f64]);
    motion.emit_by_name::<()>("motion", &[&2.0f64, &2.0f64]);
    drag.emit_by_name::<()>("cancel", &[&None::<gtk::gdk::EventSequence>]);
    motion.emit_by_name::<()>("motion", &[&3.0f64, &4.0f64]);

    let seen = log.borrow();
    let phases: Vec<_> = seen.iter().map(|e| (e.phase, e.point)).collect();
    assert_eq!(
        phases,
        vec![
            (PointerPhase::Move, Point::new(5.0, 6.0)),
            (PointerPhase::Down, Point::new(30.0, 20.0)),
            (PointerPhase::Move, Point::new(50.0, 40.0)),
            // 押している間の `motion` (51, 41) は二重になるので出ない。
            (PointerPhase::Move, Point::new(-10.0, -10.0)),
            (PointerPhase::Up, Point::new(-10.0, -10.0)),
            (PointerPhase::Move, Point::new(7.0, 8.0)),
            // 取り消された間の `motion` (2, 2) は出ず、取り消し後は届く。
            (PointerPhase::Move, Point::new(3.0, 4.0)),
        ],
        "{seen:?}"
    );
    Ok(())
}

/// The wheel arrives from `GtkEventControllerScroll`, with notches converted to
/// logical pixels. The position is the one last seen while hovering, and returning
/// `Handled` stops propagation.
fn canvas_reports_wheel(ui: &Ui) -> Result<()> {
    let canvas = ui.canvas()?;
    let (log, mut sink) = recorder::<naui_core::PointerEvent>();
    let response = Rc::new(Cell::new(EventResponse::Continue));
    canvas.on_wheel({
        let response = response.clone();
        move |event| {
            sink(event);
            response.get()
        }
    });
    let pointer = Rc::new(RefCell::new(Vec::new()));
    canvas.on_pointer({
        let pointer = pointer.clone();
        move |event| pointer.borrow_mut().push(event)
    });

    let area = canvas.native_area();
    let mut scroll = None;
    let mut motion = None;
    let controllers = area.observe_controllers();
    for index in 0..controllers.n_items() {
        let Some(c) = controllers.item(index) else {
            continue;
        };
        if let Ok(s) = c.clone().downcast::<gtk::EventControllerScroll>() {
            scroll = Some(s);
        } else if let Ok(m) = c.downcast::<gtk::EventControllerMotion>() {
            motion = Some(m);
        }
    }
    let scroll = scroll.expect("ホイールを受ける GtkEventControllerScroll が付いていること");
    let motion = motion.expect("ホバーを受ける GtkEventControllerMotion が付いていること");
    assert!(
        scroll
            .flags()
            .contains(gtk::EventControllerScrollFlags::BOTH_AXES),
        "縦と横の両方を受けること: {:?}",
        scroll.flags()
    );

    // シグナルを直接起こしたときの単位は刻み (`GDK_SCROLL_UNIT_WHEEL`)。
    motion.emit_by_name::<()>("motion", &[&5.0f64, &6.0f64]);
    let stopped = scroll.emit_by_name::<bool>("scroll", &[&0.5f64, &1.0f64]);
    assert!(!stopped, "Continue なら外側のスクロールへ回すこと");
    response.set(EventResponse::Handled);
    let stopped = scroll.emit_by_name::<bool>("scroll", &[&0.0f64, &-2.0f64]);
    assert!(stopped, "Handled なら伝播を止めること");

    let seen = log.borrow();
    let wheels: Vec<_> = seen
        .iter()
        .map(|e| (e.phase, e.point, e.button, e.wheel_x, e.wheel_y))
        .collect();
    let step = naui_core::WHEEL_STEP;
    assert_eq!(
        wheels,
        vec![
            (
                PointerPhase::Wheel,
                Point::new(5.0, 6.0),
                None,
                0.5 * step,
                step
            ),
            (
                PointerPhase::Wheel,
                Point::new(5.0, 6.0),
                None,
                0.0,
                -2.0 * step
            ),
        ]
    );
    assert!(
        pointer
            .borrow()
            .iter()
            .all(|e| e.phase == PointerPhase::Move),
        "ホイールは on_pointer へは届かないこと: {:?}",
        pointer.borrow()
    );
    Ok(())
}

/// The outer `GtkScrolledWindow` accumulates a history of amounts in the capture
/// phase and starts momentum scrolling when the fingers lift, even if the child
/// stopped them with `Handled`. If the last one was received with `Handled`,
/// `scroll-end` turns `kinetic-scrolling` off and back on to cancel that momentum
/// (turning it back on leaves the setting unchanged). If the last one was passed
/// outward, it is left alone.
fn canvas_cancels_outer_kinetic_after_handled(ui: &Ui) -> Result<()> {
    let canvas = ui.canvas()?;
    let response = Rc::new(Cell::new(EventResponse::Handled));
    canvas.on_wheel({
        let response = response.clone();
        move |_| response.get()
    });
    let scroll = ui.scroll()?;
    scroll.set_child(&canvas);
    let native: gtk::ScrolledWindow = scroll
        .native_widget()
        .downcast()
        .expect("GtkScrolledWindow");
    assert!(native.is_kinetic_scrolling(), "既定で慣性が有効なこと");
    let toggles = Rc::new(Cell::new(0));
    native.connect_kinetic_scrolling_notify({
        let toggles = toggles.clone();
        move |_| toggles.set(toggles.get() + 1)
    });

    let area = canvas.native_area();
    let controllers = area.observe_controllers();
    let controller = (0..controllers.n_items())
        .filter_map(|index| controllers.item(index))
        .find_map(|c| c.downcast::<gtk::EventControllerScroll>().ok())
        .expect("ホイールを受ける GtkEventControllerScroll が付いていること");
    let gesture = |responses: &[EventResponse]| {
        controller.emit_by_name::<()>("scroll-begin", &[]);
        for r in responses {
            response.set(*r);
            controller.emit_by_name::<bool>("scroll", &[&0.0f64, &1.0f64]);
        }
        controller.emit_by_name::<()>("scroll-end", &[]);
    };

    gesture(&[EventResponse::Handled, EventResponse::Handled]);
    assert_eq!(toggles.get(), 2, "Handled で終えたら切って戻すこと");
    assert!(native.is_kinetic_scrolling(), "設定は元へ戻ること");

    gesture(&[EventResponse::Handled, EventResponse::Continue]);
    assert_eq!(toggles.get(), 2, "最後を外側へ回したなら慣性を残すこと");

    native.set_kinetic_scrolling(false);
    toggles.set(0);
    gesture(&[EventResponse::Handled]);
    assert_eq!(toggles.get(), 0, "慣性を切ってある外側には触らないこと");
    assert!(!native.is_kinetic_scrolling());
    Ok(())
}

/// The color picker is a `GtkColorDialogButton` itself, and its value round-trips
/// with the native side.
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

/// GTK4 places the indicator at the center of the label's "line box", but Japanese
/// lines get a larger ascent, which pushes the glyphs down. naui adds a top margin
/// to the indicator to re-align it with the glyph center. The margin stays within
/// the line box, so the check box's height does not change.
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
    assert_eq!(native.selected(), 3, "末尾の非表示の未選択項目");
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
    assert_eq!(native.selected(), 3);
    combo.set_items(&["", "別"]);
    assert_eq!(combo.len(), 2);
    assert_eq!(combo.selected(), None);
    combo.set_selected(0);
    assert_eq!(combo.selected(), Some(0), "空文字の項目も未選択と区別する");
    combo.set_items::<&str>(&[]);
    assert!(combo.is_empty());
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
    combo.clear_selection();
    native.set_selected(0);
    assert_eq!(combo.selected(), Some(0), "解除後も先頭の項目を選べる");
    assert_eq!(log.borrow().as_slice(), [2, 0, 0]);
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

/// Even through the native click path, exactly one is ever on.
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

/// Number input exchanges values with the `GtkSpinButton` and notifies on user
/// actions.
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

/// Decimal places, step and range apply both to the `GtkSpinButton` and to naui's
/// value.
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

/// Values readable at the time of typing are notified, and confirmation (`update`)
/// brings the display and the value into line.
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

/// Number input fits in the given width and is not squashed below the width of its
/// field and buttons.
fn number_input_keeps_its_buttons_inside(ui: &Ui) -> Result<()> {
    /// A common width for number fields (logical pixels).
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

/// Search input is a `GtkSearchEntry` itself, and text round-trips.
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

/// The confirmation notification comes only on `activate` (Enter), not while
/// typing.
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

/// Single-line input and password input also report confirmation on `activate`
/// (Enter), like the search field. Not counted while read-only.
fn text_inputs_notify_on_activate(ui: &Ui) -> Result<()> {
    let input = ui.text_input("ねこ")?;
    let password = ui.password_input()?;
    let input_native: gtk::Entry = input.native_widget().downcast().expect("GtkEntry");
    let password_native: gtk::PasswordEntry = password
        .native_widget()
        .downcast()
        .expect("GtkPasswordEntry");

    let (submitted, sink) = recorder::<String>();
    let sink = Rc::new(RefCell::new(sink));
    input.on_submit({
        let sink = sink.clone();
        move |text: &str| (sink.borrow_mut())(format!("入力:{text}"))
    });
    password.on_submit({
        let sink = sink.clone();
        move |text: &str| (sink.borrow_mut())(format!("伏せ字:{text}"))
    });
    password.set_text("ひみつ");

    input_native.emit_activate();
    password_native.emit_activate();
    assert_eq!(
        submitted.borrow().as_slice(),
        ["入力:ねこ", "伏せ字:ひみつ"]
    );

    input.set_read_only(true);
    input_native.emit_activate();
    assert_eq!(submitted.borrow().len(), 2, "読み取り専用なら確定しない");
    Ok(())
}

/// Enter reports after the value is confirmed (after `gtk_spin_button_update`).
fn number_input_commits_before_submitting(ui: &Ui) -> Result<()> {
    let number = ui.number_input(5.0)?;
    number.set_range(None, Some(100.0));
    let native: gtk::SpinButton = number.native_widget().downcast().expect("GtkSpinButton");
    let text: gtk::Text = native
        .delegate()
        .and_then(|delegate| delegate.downcast().ok())
        .expect("GtkSpinButton の中の GtkText");

    let (log, sink) = recorder::<String>();
    let sink = Rc::new(RefCell::new(sink));
    number.on_change({
        let sink = sink.clone();
        move |value| (sink.borrow_mut())(format!("変更:{value}"))
    });
    number.on_submit({
        let sink = sink.clone();
        move |value| (sink.borrow_mut())(format!("確定:{value}"))
    });

    native.set_text("12");
    text.emit_activate();
    assert_eq!(log.borrow().as_slice(), ["変更:12", "確定:12"]);

    // 範囲の外は端へ寄せ、表示も直してから知らせる。
    native.set_text("999");
    text.emit_activate();
    assert_eq!(log.borrow()[2..], ["変更:100", "確定:100"]);
    assert_eq!(native.text().as_str(), "100");
    Ok(())
}

/// Password input is a `GtkPasswordEntry` itself, and text round-trips.
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

/// When a full-width field is placed in a row too narrow for it.
///
/// Axes with `Fill` report a minimum of 0, so GTK4 allots less space than the
/// content can shrink to. Passing that on as is makes `GtkEntry` pass a negative
/// width ("the allotted width minus its border") to the inner `GtkText`, and GTK4
/// warns and stops drawing the content (this happens when the window is shrunk).
/// The content is placed at its minimum, and whatever overflows is hidden with
/// `overflow`.
fn a_narrow_allocation_does_not_squash_the_content(ui: &Ui) -> Result<()> {
    let row = ui.stack(Orientation::Horizontal)?;
    row.set_sizing(Sizing::fill_width());
    let input = ui.text_input("")?;
    input.set_sizing(Sizing::fill_width());
    let button = ui.button("読み込む")?;
    row.append(&input);
    row.append(&button);

    let bin = bin_of(&row);
    let native = input.native_widget();
    let height = measure_height(&bin).1;

    // 潰れ方は「中で**負の大きさ**が出る」こと。`GtkEntry` は配られた幅から
    // 枠の分を引いて中の `GtkText` へ渡すので、狭いと `-18px` のような値になり、
    // GTK4 の警告とともに中身が描かれなくなる。
    //
    // 0px は「入る場所が無い」ぶんには正しい (何も描かれないだけ) ので、
    // 見るのは負にならないことだけ。直す前は、下のどの幅でも入力欄が
    // -18px になっていた (Linux の CI で実測)。
    for width in [0, 1, 40, 120] {
        bin.queue_resize();
        bin.allocate(width, height, -1, None);
        assert!(
            native.width() >= 0 && native.height() >= 0,
            "行へ幅 {width}px を配ったら、入力欄が {}x{}px になった",
            native.width(),
            native.height()
        );
    }

    // 入れ物へ直接配ったときも同じ (行の割り振りを通らない経路)。
    // 直す前はここでも -18px / -17px になっていた。
    let input_bin = bin_of(&input);
    for width in [0, 1, 40] {
        input_bin.queue_resize();
        input_bin.allocate(width, height, -1, None);
        assert!(
            native.width() >= 0 && native.height() >= 0,
            "入れ物へ幅 {width}px を配ったら、入力欄が {}x{}px になった",
            native.width(),
            native.height()
        );
    }
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

    let native = grid.native_widget();
    assert_eq!(children(&native).len(), 3);

    // 2 列 2 行なので、すき間は横にも縦にも 1 つずつ。
    let width = measure_width(&native);
    let height = measure_height_for(&native, -1);
    grid.set_spacing(12.0, 8.0);
    assert_eq!(measure_width(&native), (width.0 + 12, width.1 + 12));
    assert_eq!(
        measure_height_for(&native, -1),
        (height.0 + 8, height.1 + 8)
    );
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

    let native = grid.native_widget();
    let placed = children(&native);
    assert_eq!(placed.len(), 1);
    assert_eq!(placed[0], bin_of(&second));
    Ok(())
}

/// `Fill` lowers the minimum to 0, so a parent may ask for the height at a width
/// below the content's minimum (a window made narrower than a row of radio buttons).
/// The content is measured at its minimum instead, as GTK4 warns otherwise
/// ("Trying to measure … but it needs at least …"). Baselines that fall outside the
/// lowered minimum are dropped for the same reason.
fn fill_does_not_measure_content_below_its_minimum(ui: &Ui) -> Result<()> {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    static WATCHING: AtomicBool = AtomicBool::new(false);
    static WARNINGS: AtomicUsize = AtomicUsize::new(0);

    let grid = ui.grid()?;
    grid.set_sizing(Sizing::fill());
    let row = ui.stack(Orientation::Horizontal)?;
    for title in ["Contain", "Cover", "Fill", "None"] {
        row.append(&ui.button(title)?);
    }
    grid.attach(&row, GridCell::new(0, 0));
    let bin = bin_of(&grid);
    let native = grid.native_widget();
    let content_minimum = measure_width(&native).0;
    assert!(content_minimum > 0);
    assert_eq!(measure_width(&bin).0, 0, "Fill の最小は 0");

    // 警告は構造化ログで出るので、書き出し先を差し替えて数える。書き出し先は
    // 1 つのプロセスで 1 度しか設定できないので、ほかの記録はそのまま流す。
    glib::log_set_writer_func(|level, fields| {
        if WATCHING.load(Ordering::SeqCst) && level == glib::LogLevel::Warning {
            WARNINGS.fetch_add(1, Ordering::SeqCst);
        }
        glib::log_writer_default(level, fields)
    });
    WATCHING.store(true, Ordering::SeqCst);
    let narrow = measure_height_for(&bin, content_minimum / 2);
    WATCHING.store(false, Ordering::SeqCst);

    assert_eq!(WARNINGS.load(Ordering::SeqCst), 0, "GTK4 が警告しない");
    assert_eq!(
        narrow.1,
        measure_height_for(&native, content_minimum).1,
        "中身は最小の幅で測る"
    );
    Ok(())
}

fn grid_fill_track_expands_child(ui: &Ui) -> Result<()> {
    let grid = ui.grid()?;
    let field = ui.text_input("")?;
    grid.attach(&field, GridCell::new(1, 0));
    grid.set_column_track(1, Track::FILL);

    let bin = bin_of(&field);
    assert!(bin.hexpands(), "Fill の列に入った子が余りを受け取る");
    // 広がるのは列だけで、中身に合わせる子はマスの先頭に置いたまま伸ばさない。
    // ほかの環境 (NSGridView の Leading、WinUI の Left、CSS の start) と同じ。
    assert_eq!(bin.halign(), gtk::Align::Start);

    // 自分で Fill を指定した子は、マスの幅まで伸びる。
    let wide = ui.text_input("")?;
    wide.set_sizing(Sizing::fill_width());
    grid.attach(&wide, GridCell::new(1, 1));
    assert_eq!(bin_of(&wide).halign(), gtk::Align::Fill);
    Ok(())
}

/// Lays a grid out at the given size, as its parent would, and returns where each
/// child's container (`SizeBin`) ended up: `(x, y, width, height)`.
fn lay_out_grid(
    grid: &naui_gtk::Grid,
    width: i32,
    height: i32,
    children: &[&dyn Widget],
) -> Vec<(f32, f32, f32, f32)> {
    let native = grid.native_widget();
    // 親と同じく、先に測ってから配る (測らずに配ると GTK4 が警告する)。
    native.measure(gtk::Orientation::Horizontal, -1);
    native.measure(gtk::Orientation::Vertical, width);
    native.size_allocate(&gtk::Allocation::new(0, 0, width, height), -1);
    children
        .iter()
        .map(|child| {
            let bounds = bin_of(*child)
                .compute_bounds(&native)
                .expect("グリッドの中の位置");
            (bounds.x(), bounds.y(), bounds.width(), bounds.height())
        })
        .collect()
}

/// `Fill` columns and rows split the room by weight, as CSS's `fr` (Web) and
/// `Star` (Windows) do. `GtkGrid` split it evenly.
fn grid_fill_weights_split_the_room(ui: &Ui) -> Result<()> {
    let grid = ui.grid()?;
    grid.set_spacing(8.0, 4.0);
    grid.set_column_track(0, Track::Fill(1.0));
    grid.set_column_track(1, Track::Fill(3.0));
    grid.set_row_track(0, Track::Fill(2.0));
    grid.set_row_track(1, Track::Fill(1.0));
    let cells: Vec<naui_gtk::Stack> = (0..4)
        .map(|_| {
            let cell = ui.stack(Orientation::Vertical)?;
            cell.set_sizing(Sizing::fill());
            Ok(cell)
        })
        .collect::<Result<_>>()?;
    for (index, cell) in cells.iter().enumerate() {
        grid.attach(cell, GridCell::new(index % 2, index / 2));
    }
    let widgets: Vec<&dyn Widget> = cells.iter().map(|cell| cell as &dyn Widget).collect();
    let placed = lay_out_grid(&grid, 408, 304, &widgets);
    // 横は 408 - 8 = 400 を 1 : 3、縦は 304 - 4 = 300 を 2 : 1。
    assert_eq!(placed[0], (0.0, 0.0, 100.0, 200.0), "{placed:?}");
    assert_eq!(placed[1], (108.0, 0.0, 300.0, 200.0), "{placed:?}");
    assert_eq!(placed[2], (0.0, 204.0, 100.0, 100.0), "{placed:?}");
    assert_eq!(placed[3], (108.0, 204.0, 300.0, 100.0), "{placed:?}");

    // 重みをあとから変えても分け直す。
    grid.set_column_track(1, Track::Fill(1.0));
    let placed = lay_out_grid(&grid, 408, 304, &widgets);
    assert_eq!(placed[0].2, 200.0, "{placed:?}");
    assert_eq!(placed[1].2, 200.0, "{placed:?}");
    Ok(())
}

/// A `Fill` column never gets less than its content needs. It keeps that, and the
/// other `Fill` columns share what is left by their weights (as CSS's `fr`).
fn grid_fill_keeps_the_content_minimum(ui: &Ui) -> Result<()> {
    let grid = ui.grid()?;
    grid.set_column_track(0, Track::Fill(1.0));
    grid.set_column_track(1, Track::Fill(1.0));
    grid.set_column_track(2, Track::Fill(2.0));
    let wide = ui.stack(Orientation::Vertical)?;
    wide.set_sizing(Sizing::fill().min_width(180.0));
    let middle = ui.stack(Orientation::Vertical)?;
    middle.set_sizing(Sizing::fill());
    let last = ui.stack(Orientation::Vertical)?;
    last.set_sizing(Sizing::fill());
    grid.attach(&wide, GridCell::new(0, 0));
    grid.attach(&middle, GridCell::new(1, 0));
    grid.attach(&last, GridCell::new(2, 0));
    // 1 : 1 : 2 なら 100 : 100 : 200 だが、1 列目は 180 要る。残り 220 を 1 : 2。
    let placed = lay_out_grid(&grid, 400, 40, &[&wide, &middle, &last]);
    let widths: Vec<f32> = placed.iter().map(|p| p.2).collect();
    assert_eq!(widths[0], 180.0, "{placed:?}");
    assert!((widths[1] - 73.0).abs() <= 1.0, "{placed:?}");
    assert!((widths[2] - 147.0).abs() <= 1.0, "{placed:?}");
    assert_eq!(
        widths.iter().sum::<f32>(),
        400.0,
        "画素を余らせない: {placed:?}"
    );
    Ok(())
}

/// The leftover goes only to the `Fill` columns: an `Auto` column keeps its
/// natural width even when a child in it grows, and two `Fill` columns of equal
/// weight get the same width whatever their content's natural width. `GtkGrid`
/// gave each its natural width plus an even share of the rest.
fn grid_fill_takes_the_rest_from_auto_columns(ui: &Ui) -> Result<()> {
    let grid = ui.grid()?;
    grid.set_column_track(0, Track::Auto);
    grid.set_column_track(1, Track::FILL);
    grid.set_column_track(2, Track::FILL);
    let name = ui.label("名前")?;
    let growing = ui.stack(Orientation::Vertical)?;
    growing.set_sizing(Sizing::fill().min_width(40.0));
    let short = ui.label("短い")?;
    let long = ui.label("こちらはずっと長い文字です")?;
    let below = ui.stack(Orientation::Vertical)?;
    below.set_sizing(Sizing::fill());
    let other = ui.stack(Orientation::Vertical)?;
    other.set_sizing(Sizing::fill());
    grid.attach(&name, GridCell::new(0, 0));
    grid.attach(&growing, GridCell::new(0, 1));
    grid.attach(&short, GridCell::new(1, 0));
    grid.attach(&long, GridCell::new(2, 0));
    grid.attach(&below, GridCell::new(1, 1));
    grid.attach(&other, GridCell::new(2, 1));

    let auto = measure_width(&bin_of(&name)).1.max(40);
    let placed = lay_out_grid(&grid, 600, 80, &[&growing, &below, &other]);
    assert_eq!(
        placed[0].2, auto as f32,
        "Auto の列は余りを受け取らない: {placed:?}"
    );
    assert_eq!(placed[1].2, placed[2].2, "同じ重みの列は同じ幅: {placed:?}");
    assert_eq!(placed[1].2 + placed[2].2, (600 - auto) as f32, "{placed:?}");
    Ok(())
}

/// A `Fill` child reports a minimum of 0, but while there is room its column is
/// not made narrower than the content can shrink to: the column keeps that and the
/// others share the rest by weight. The grid's own minimum still leaves it out, so
/// the content does not push the window wider.
fn grid_fill_does_not_cut_off_content_while_there_is_room(ui: &Ui) -> Result<()> {
    let grid = ui.grid()?;
    grid.set_column_track(0, Track::FILL);
    grid.set_column_track(1, Track::FILL);
    let short = ui.button("短い")?;
    short.set_sizing(Sizing::fill());
    let long = ui.button("こちらはずっと長い文字のボタンです")?;
    long.set_sizing(Sizing::fill());
    grid.attach(&short, GridCell::new(0, 0));
    grid.attach(&long, GridCell::new(1, 0));

    let short_minimum = measure_width(&short.native_widget()).0;
    let long_minimum = measure_width(&long.native_widget()).0;
    let width = long_minimum + short_minimum + 20;
    assert!(width < long_minimum * 2, "等分では切れる幅で確かめる");
    let placed = lay_out_grid(&grid, width, 40, &[&short, &long]);
    assert_eq!(placed[1].2, long_minimum as f32, "{placed:?}");
    assert_eq!(placed[0].2, (short_minimum + 20) as f32, "{placed:?}");

    // 広ければ重みどおり。
    let width = long_minimum * 3;
    let placed = lay_out_grid(&grid, width, 40, &[&short, &long]);
    assert!((placed[0].2 - placed[1].2).abs() <= 1.0, "{placed:?}");

    assert!(
        measure_width(&grid.native_widget()).0 < long_minimum,
        "グリッドの最小は中身で広がらない"
    );
    Ok(())
}

/// What a spanning child lacks is spread over the `Fill` lines it crosses by their
/// weights, not evenly: across `Fill` 1 : 3, a child of 400 lets 600 split 150 :
/// 450. Spread evenly, both lines would need 200 and the split would be 200 : 400.
fn grid_spanning_child_spreads_by_weight(ui: &Ui) -> Result<()> {
    let fills = || -> Result<(naui_gtk::Stack, naui_gtk::Stack)> {
        let first = ui.stack(Orientation::Vertical)?;
        first.set_sizing(Sizing::fill());
        let second = ui.stack(Orientation::Vertical)?;
        second.set_sizing(Sizing::fill());
        Ok((first, second))
    };

    let columns = ui.grid()?;
    columns.set_column_track(0, Track::Fill(1.0));
    columns.set_column_track(1, Track::Fill(3.0));
    let wide = ui.stack(Orientation::Vertical)?;
    wide.set_sizing(Sizing::fixed(400.0, 20.0));
    let (left, right) = fills()?;
    columns.attach(&wide, GridCell::new(0, 0).span(2, 1));
    columns.attach(&left, GridCell::new(0, 1));
    columns.attach(&right, GridCell::new(1, 1));
    let placed = lay_out_grid(&columns, 600, 100, &[&left, &right]);
    assert_eq!(placed[0].2, 150.0, "{placed:?}");
    assert_eq!(placed[1].2, 450.0, "{placed:?}");

    // 既存の最小 180 に不足分 220 の 1/4 を足すと、不要な最小 235 が
    // 残る。180 を下限にまたぐ幅 400 を配り、180 : 220 とする。
    left.set_sizing(Sizing::fill().min_width(180.0));
    for (width, expected) in [
        (400, [180.0, 220.0]),
        (600, [180.0, 420.0]),
        (800, [200.0, 600.0]),
    ] {
        let placed = lay_out_grid(&columns, width, 100, &[&left, &right]);
        assert_eq!([placed[0].2, placed[1].2], expected, "{placed:?}");
    }
    columns.set_spacing(8.0, 0.0);
    let placed = lay_out_grid(&columns, 608, 100, &[&left, &right]);
    assert_eq!([placed[0].2, placed[1].2], [180.0, 420.0], "{placed:?}");
    assert_eq!(placed[1].0, 188.0, "{placed:?}");

    let rows = ui.grid()?;
    rows.set_row_track(0, Track::Fill(1.0));
    rows.set_row_track(1, Track::Fill(3.0));
    let tall = ui.stack(Orientation::Vertical)?;
    tall.set_sizing(Sizing::fixed(20.0, 400.0));
    let (top, bottom) = fills()?;
    rows.attach(&tall, GridCell::new(0, 0).span(1, 2));
    rows.attach(&top, GridCell::new(1, 0));
    rows.attach(&bottom, GridCell::new(1, 1));
    let placed = lay_out_grid(&rows, 100, 600, &[&top, &bottom]);
    assert_eq!(placed[0].3, 150.0, "{placed:?}");
    assert_eq!(placed[1].3, 450.0, "{placed:?}");
    top.set_sizing(Sizing::fill().min_height(180.0));
    for (height, expected) in [
        (400, [180.0, 220.0]),
        (600, [180.0, 420.0]),
        (800, [200.0, 600.0]),
    ] {
        let placed = lay_out_grid(&rows, 100, height, &[&top, &bottom]);
        assert_eq!([placed[0].3, placed[1].3], expected, "{placed:?}");
    }
    rows.set_spacing(0.0, 8.0);
    let placed = lay_out_grid(&rows, 100, 608, &[&top, &bottom]);
    assert_eq!([placed[0].3, placed[1].3], [180.0, 420.0], "{placed:?}");
    assert_eq!(placed[1].1, 188.0, "{placed:?}");
    Ok(())
}

/// Spanning children request expansion independently; expansion supplied by a
/// single-cell child takes precedence within each span, as in `GtkGrid`.
fn grid_overlapping_spans_expand_independently_of_attach_order(ui: &Ui) -> Result<()> {
    for vertical in [false, true] {
        for reverse in [false, true] {
            for single in [None, Some(0), Some(1), Some(2)] {
                let grid = ui.grid()?;
                let first = ui.stack(Orientation::Vertical)?;
                let second = ui.stack(Orientation::Vertical)?;
                first.set_sizing(Sizing::fill());
                second.set_sizing(Sizing::fill());
                let (first_cell, second_cell) = if vertical {
                    (
                        GridCell::new(0, 0).span(1, 2),
                        GridCell::new(1, 1).span(1, 2),
                    )
                } else {
                    (
                        GridCell::new(0, 0).span(2, 1),
                        GridCell::new(1, 1).span(2, 1),
                    )
                };
                if reverse {
                    grid.attach(&second, second_cell);
                    grid.attach(&first, first_cell);
                } else {
                    grid.attach(&first, first_cell);
                    grid.attach(&second, second_cell);
                }
                if let Some(index) = single {
                    let child = ui.stack(Orientation::Vertical)?;
                    child.set_sizing(Sizing::fill());
                    grid.attach(
                        &child,
                        if vertical {
                            GridCell::new(2, index)
                        } else {
                            GridCell::new(index, 2)
                        },
                    );
                }
                let placed = lay_out_grid(&grid, 300, 300, &[&first, &second]);
                let extents: Vec<_> = placed
                    .iter()
                    .map(|frame| {
                        if vertical {
                            (frame.1, frame.3)
                        } else {
                            (frame.0, frame.2)
                        }
                    })
                    .collect();
                let expected = if single == Some(1) {
                    // 両スパンとも単独の子がある中央のトラックだけで拡張できる。
                    [(0.0, 300.0), (0.0, 300.0)]
                } else {
                    // 3 トラックへ 100 ずつ配るので、両スパンとも 200 になる。
                    [(0.0, 200.0), (100.0, 200.0)]
                };
                assert_eq!(
                    extents, expected,
                    "縦 {vertical}、逆順 {reverse}、単独の拡張要求 {single:?}: {placed:?}"
                );
            }
        }
    }
    Ok(())
}

/// A wrapping label is measured for the width its `Fill` column actually gets, so
/// the row is as tall as the text needs at that width.
fn grid_measures_wrapping_text_at_its_share(ui: &Ui) -> Result<()> {
    let grid = ui.grid()?;
    grid.set_column_track(0, Track::Fill(1.0));
    grid.set_column_track(1, Track::Fill(3.0));
    let text = "折り返す文字。".repeat(12);
    let narrow = ui.label(&text)?;
    narrow.set_wrap(true);
    let wide = ui.label(&text)?;
    wide.set_wrap(true);
    grid.attach(&narrow, GridCell::new(0, 0));
    grid.attach(&wide, GridCell::new(1, 0));

    let native = grid.native_widget();
    assert_eq!(native.request_mode(), gtk::SizeRequestMode::HeightForWidth);
    let row = measure_height_for(&native, 400).0;
    let at_share = measure_height_for(&bin_of(&narrow), 100).0;
    assert_eq!(row, at_share, "1 列目の幅 100 で測った高さが行の高さ");
    assert!(
        at_share > measure_height_for(&bin_of(&wide), 300).0,
        "狭い列ほど行が多い"
    );
    Ok(())
}

/// In right-to-left text the first column is on the right, as in `GtkGrid`.
fn grid_mirrors_columns_right_to_left(ui: &Ui) -> Result<()> {
    let grid = ui.grid()?;
    grid.set_column_track(0, Track::Fill(1.0));
    grid.set_column_track(1, Track::Fill(3.0));
    let first = ui.stack(Orientation::Vertical)?;
    first.set_sizing(Sizing::fill());
    let second = ui.stack(Orientation::Vertical)?;
    second.set_sizing(Sizing::fill());
    grid.attach(&first, GridCell::new(0, 0));
    grid.attach(&second, GridCell::new(1, 0));
    grid.native_widget().set_direction(gtk::TextDirection::Rtl);
    let placed = lay_out_grid(&grid, 400, 40, &[&first, &second]);
    assert_eq!((placed[0].0, placed[0].2), (300.0, 100.0), "{placed:?}");
    assert_eq!((placed[1].0, placed[1].2), (0.0, 300.0), "{placed:?}");
    Ok(())
}

/// Children that fit their content go to the start of the cell horizontally and to
/// the center vertically, the same as on the other backends. This includes
/// children spanning several columns, which do not take a column's setting.
fn grid_places_auto_children_at_the_start(ui: &Ui) -> Result<()> {
    let grid = ui.grid()?;
    grid.set_column_track(0, Track::Auto);
    grid.set_column_track(1, Track::FILL);
    grid.set_column_track(2, Track::Auto);
    let name = ui.label("Button")?;
    let buttons = ui.stack(Orientation::Horizontal)?;
    buttons.append(&ui.button("Click")?);
    grid.attach(&name, GridCell::new(0, 0));
    grid.attach(&buttons, GridCell::new(1, 0).span(2, 1));

    for bin in [bin_of(&name), bin_of(&buttons)] {
        assert_eq!(bin.halign(), gtk::Align::Start);
        assert_eq!(bin.valign(), gtk::Align::Center);
    }
    Ok(())
}

/// Children with a size of their own (`Fixed`, or `Fill` with a maximum) also go
/// to the start of the cell, as on the other backends. Only `Fill` without a
/// maximum stretches. The placement holds when the size is set after attaching.
fn grid_places_sized_children_at_the_start(ui: &Ui) -> Result<()> {
    let grid = ui.grid()?;
    grid.set_column_track(0, Track::FILL);

    let fixed = ui.text_input("")?;
    fixed.set_sizing(Sizing::new().width(Length::Fixed(240.0)));
    grid.attach(&fixed, GridCell::new(0, 0));
    let capped = ui.text_input("")?;
    capped.set_sizing(Sizing::fill_width().max_width(300.0));
    grid.attach(&capped, GridCell::new(0, 1));
    for bin in [bin_of(&fixed), bin_of(&capped)] {
        assert_eq!(bin.halign(), gtk::Align::Start);
        assert_eq!(bin.valign(), gtk::Align::Center);
    }

    // 置いた後に大きさを決めても、マスの先頭のまま。
    let later = ui.text_input("")?;
    grid.attach(&later, GridCell::new(0, 2));
    later.set_sizing(Sizing::new().width(Length::Fixed(240.0)));
    assert_eq!(bin_of(&later).halign(), gtk::Align::Start);
    // 置いた後に Fill にすれば、マスの幅まで伸びる。
    later.set_sizing(Sizing::fill_width());
    assert_eq!(bin_of(&later).halign(), gtk::Align::Fill);
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

/// Specifying the size after appending keeps the stack's alignment. `set_sizing`
/// sets the same `halign` as the stack does, and used to put back its own.
fn stack_align_survives_later_sizing(ui: &Ui) -> Result<()> {
    let stack = ui.stack(Orientation::Vertical)?;
    stack.set_align(Align::Start);
    let label = ui.label("あ")?;
    stack.append(&label);

    label.set_sizing(Sizing::new().height(Length::Fixed(30.0)));
    assert_eq!(bin_of(&label).halign(), gtk::Align::Start);
    label.set_sizing(Sizing::fill_width());
    assert_eq!(bin_of(&label).halign(), gtk::Align::Fill);
    label.set_sizing(Sizing::AUTO);
    assert_eq!(
        bin_of(&label).halign(),
        gtk::Align::Start,
        "Fill をやめればスタックの寄せ方へ戻る"
    );
    Ok(())
}

/// A `Stack` with a breakpoint lays its children out side by side once it is at least
/// that wide (its padding included) and one above the other below it, switching after
/// the paint that showed the new width. Going back takes a few pixels less, and a field
/// being typed into keeps its focus through the switch.
fn stack_breakpoint_follows_its_width(ui: &Ui) -> Result<()> {
    use std::time::Duration;

    let window = ui.window("しきい値", 700.0, 240.0)?;
    let root = ui.stack(Orientation::Vertical)?;
    root.set_sizing(Sizing::fill());
    let form = ui.stack(Orientation::Vertical)?;
    form.set_sizing(Sizing::fill_width());
    form.set_padding(Padding::all(10.0));
    let field = ui.text_input("")?;
    let first = ui.button("前")?;
    let last = ui.button("後")?;
    form.append(&field);
    form.append(&first);
    form.append(&last);
    root.append(&form);
    window.set_child(&root);

    form.set_breakpoint(Some(naui_core::Breakpoint::new(
        500.0,
        Orientation::Horizontal,
    )));
    assert_eq!(
        form.current_orientation(),
        Orientation::Vertical,
        "並ぶ前は自分の向きのまま"
    );
    window.show();
    spin_until(Duration::from_secs(3), || {
        form.current_orientation() == Orientation::Horizontal
    });
    let native: gtk::Box = form.native_widget().downcast().expect("GtkBox");
    assert_eq!(
        native.orientation(),
        gtk::Orientation::Horizontal,
        "しきい値より広ければ横に並ぶ: 幅 {}",
        native.width()
    );
    assert_eq!(form.orientation(), Orientation::Vertical);
    assert!(field.request_focus());
    let entry = field.native_widget();

    let resize = |width: f64, expected: Orientation, message: &str| {
        window.set_size(width, 240.0);
        spin_until(Duration::from_secs(2), || {
            form.current_orientation() == expected
        });
        // 変わらないはずの所では、描き直しと予約が済むまで待ってから見る。
        spin_until(Duration::from_millis(300), || false);
        assert_eq!(
            form.current_orientation(),
            expected,
            "{message} (窓 {width}、スタック {})",
            native.width() + 20
        );
    };
    resize(480.0, Orientation::Vertical, "しきい値より狭ければ縦に積む");
    assert!(
        entry.has_focus() || entry.focus_child().is_some(),
        "向きが切り替わっても入力欄のフォーカスが外れない"
    );
    resize(496.0, Orientation::Vertical, "しきい値に届くまでは縦のまま");
    resize(500.0, Orientation::Horizontal, "ちょうどしきい値で横に並ぶ");
    resize(
        494.0,
        Orientation::Horizontal,
        "しきい値のすぐ下では戻らない",
    );
    resize(490.0, Orientation::Vertical, "遊びを越えて狭まれば縦に戻る");

    resize(700.0, Orientation::Horizontal, "広げれば横に戻る");
    form.set_breakpoint(None);
    assert_eq!(
        native.orientation(),
        gtk::Orientation::Vertical,
        "しきい値を外すと自分の向きへすぐ戻る"
    );
    // しきい値の向きの側にいる間に自分の向きを変えても、並びはしきい値の向きのまま。
    form.set_breakpoint(Some(naui_core::Breakpoint::new(
        400.0,
        Orientation::Vertical,
    )));
    form.set_orientation(Orientation::Horizontal);
    assert_eq!(form.orientation(), Orientation::Horizontal);
    assert_eq!(form.current_orientation(), Orientation::Vertical);
    resize(
        380.0,
        Orientation::Horizontal,
        "狭まれば新しい自分の向きになる",
    );
    window.close();
    Ok(())
}

/// `AdaptivePanes` keeps the `AdwNavigationSplitView` side by side (the list at its
/// fixed width) while at least the breakpoint wide and collapses it below, switching
/// after the paint that changed its width. While only the detail is shown, naui's
/// back button and libadwaita's own going back (`show-content` turning off) both
/// return to the list and notify.
fn adaptive_panes_follow_their_width(ui: &Ui) -> Result<()> {
    use std::time::Duration;

    let window = ui.window("一覧と詳細", 800.0, 300.0)?;
    let panes = ui.adaptive_panes()?;
    let list = ui.stack(Orientation::Vertical)?;
    list.append(&ui.label("一覧")?);
    let detail = ui.stack(Orientation::Vertical)?;
    detail.append(&ui.text_input("詳細")?);
    panes.set_list(&list);
    panes.set_detail(&detail);
    window.set_child(&panes);
    let backs = Rc::new(Cell::new(0));
    panes.on_back({
        let backs = backs.clone();
        move || backs.set(backs.get() + 1)
    });

    let native = panes.native_split_view();
    let back = panes.native_back_button();
    assert!(!panes.is_collapsed(), "並ぶ前は 2 区画");
    assert_eq!(panes.breakpoint(), naui_core::DEFAULT_PANES_BREAKPOINT);
    window.show();
    spin_until(Duration::from_millis(300), || false);
    assert!(!native.is_collapsed() && !panes.is_collapsed());
    assert!(!back.is_visible());
    let list_width = list.native_widget().width();
    assert!(
        (f64::from(list_width) - naui_core::DEFAULT_LIST_PANE_WIDTH).abs() <= 1.0,
        "一覧が決まった幅で並ぶ: {list_width}"
    );
    assert!(detail.native_widget().is_mapped(), "詳細も出ている");

    panes.show(Pane::Detail);
    assert!(native.shows_content());
    assert!(!back.is_visible(), "2 区画のときは戻るボタンを出さない");

    let resize = |width: f64, collapsed: bool, message: &str| {
        window.set_size(width, 300.0);
        spin_until(Duration::from_secs(2), || panes.is_collapsed() == collapsed);
        // 変わらないはずの所では、描き直しと予約が済むまで待ってから見る。
        spin_until(Duration::from_millis(300), || false);
        assert_eq!(
            panes.is_collapsed(),
            collapsed,
            "{message} (窓 {width}、分割 {})",
            native.width()
        );
        assert_eq!(native.is_collapsed(), collapsed, "{message}");
    };
    resize(500.0, true, "しきい値より狭ければ畳む");
    assert!(
        native.shows_content() && back.is_visible(),
        "広いときに選んだ詳細が戻るボタン付きで出る"
    );
    spin_until(Duration::from_secs(1), || !list.native_widget().is_mapped());
    assert!(!list.native_widget().is_mapped(), "一覧は隠れる");

    back.emit_clicked();
    assert_eq!(backs.get(), 1, "戻るボタンで on_back が呼ばれる");
    assert_eq!(panes.visible_pane(), Pane::List);
    assert!(!native.shows_content() && !back.is_visible());

    panes.show(Pane::Detail);
    assert!(native.shows_content() && back.is_visible());
    assert_eq!(backs.get(), 1, "show では on_back を呼ばない");
    // libadwaita 自身の「戻る」(Escape やスワイプ) は show-content を下ろす。
    native.set_show_content(false);
    assert_eq!(backs.get(), 2, "ネイティブの戻るでも on_back が呼ばれる");
    assert_eq!(panes.visible_pane(), Pane::List);
    assert!(!back.is_visible());

    resize(636.0, true, "しきい値に届くまでは畳んだまま");
    resize(640.0, false, "ちょうどしきい値で並ぶ");
    resize(634.0, false, "しきい値のすぐ下では畳まない");
    resize(630.0, true, "遊びを越えて狭まれば畳む");

    panes.set_breakpoint(400.0);
    assert!(
        !panes.is_collapsed() && !native.is_collapsed(),
        "しきい値を変えるといまの幅ですぐ決め直す"
    );
    panes.set_list_width(200.0);
    assert_eq!(panes.list_width(), 200.0);
    assert_eq!(native.min_sidebar_width(), 200.0);
    assert_eq!(native.max_sidebar_width(), 200.0);
    window.close();
    Ok(())
}

/// Switching a `Stack`'s orientation turns the `GtkBox` and places each child again
/// for the new cross axis, ending up the same as a stack made in that direction:
/// the alignment moves from `halign` to `valign`, and nothing the previous direction
/// set is left over.
fn stack_switches_orientation(ui: &Ui) -> Result<()> {
    let build = |orientation: Orientation| -> Result<(naui_gtk::Stack, gtk::Widget, gtk::Widget)> {
        let stack = ui.stack(orientation)?;
        stack.set_align(Align::End);
        let label = ui.label("あ")?;
        let wide = ui.button("広がる")?;
        wide.set_sizing(Sizing::fill_width());
        stack.append(&label);
        stack.append(&wide);
        Ok((stack, bin_of(&label), bin_of(&wide)))
    };
    let placement = |bin: &gtk::Widget| (bin.halign(), bin.valign(), bin.hexpands());

    let (stack, label, wide) = build(Orientation::Vertical)?;
    let native: gtk::Box = stack.native_widget().downcast().expect("GtkBox");
    assert_eq!(label.halign(), gtk::Align::End);

    stack.set_orientation(Orientation::Horizontal);
    assert_eq!(stack.orientation(), Orientation::Horizontal);
    assert_eq!(native.orientation(), gtk::Orientation::Horizontal);
    assert_eq!(
        label.valign(),
        gtk::Align::End,
        "寄せ方が新しい交差軸に効く"
    );
    let (_, made_label, made_wide) = build(Orientation::Horizontal)?;
    assert_eq!(
        placement(&label),
        placement(&made_label),
        "横で作ったのと同じ置き方になる"
    );
    assert_eq!(placement(&wide), placement(&made_wide));

    stack.set_orientation(Orientation::Vertical);
    assert_eq!(native.orientation(), gtk::Orientation::Vertical);
    let (_, made_label, made_wide) = build(Orientation::Vertical)?;
    assert_eq!(
        placement(&label),
        placement(&made_label),
        "縦へ戻せば縦で作ったのと同じ置き方になる"
    );
    assert_eq!(placement(&wide), placement(&made_wide));
    Ok(())
}

/// A grid column's settings stay on its children when their size is specified
/// after attaching, and changing the column replaces the old setting rather than
/// piling onto it.
fn grid_track_survives_later_changes(ui: &Ui) -> Result<()> {
    let grid = ui.grid()?;
    grid.set_column_track(0, Track::Fixed(96.0));
    let label = ui.label("名前")?;
    grid.attach(&label, GridCell::new(0, 0));
    let natural = measure_width(&ui.label("名前")?.native_widget());

    label.set_sizing(Sizing::new().height(Length::Fixed(30.0)));
    assert_eq!(
        measure_width(&bin_of(&label)),
        (96, 96),
        "あとから高さを決めても列の固定幅のまま"
    );

    grid.set_column_track(0, Track::Auto);
    assert_eq!(
        measure_width(&bin_of(&label)),
        natural,
        "列を Auto に戻せば中身の幅へ戻る"
    );

    grid.set_column_track(0, Track::FILL);
    label.set_sizing(Sizing::new().height(Length::Fixed(30.0)));
    assert!(
        bin_of(&label).hexpands(),
        "あとから大きさを決めても Fill の列は広がる"
    );
    assert_eq!(bin_of(&label).halign(), gtk::Align::Start);
    Ok(())
}

/// A child moved from a `Grid` into a `Stack` takes the stack's alignment and leaves
/// the grid column's width behind.
fn moved_child_drops_the_old_placement(ui: &Ui) -> Result<()> {
    let grid = ui.grid()?;
    grid.set_column_track(0, Track::Fixed(96.0));
    let label = ui.label("移る")?;
    grid.attach(&label, GridCell::new(0, 0));
    assert_eq!(measure_width(&bin_of(&label)), (96, 96));
    let natural = measure_width(&ui.label("移る")?.native_widget());

    grid.remove(GridCell::new(0, 0));
    let stack = ui.stack(Orientation::Vertical)?;
    stack.set_align(Align::End);
    stack.append(&label);
    let bin = bin_of(&label);
    assert_eq!(measure_width(&bin), natural, "列の固定幅を持ち越さない");
    assert_eq!(bin.halign(), gtk::Align::End, "新しい親の寄せ方になる");
    assert_eq!(bin.valign(), gtk::Align::Center);
    Ok(())
}

/// The window's content still fills the window when its size is specified after
/// `set_child`.
fn window_child_fills_after_later_sizing(ui: &Ui) -> Result<()> {
    let window = ui.window("中身", 320.0, 240.0)?;
    let root = ui.stack(Orientation::Vertical)?;
    window.set_child(&root);
    root.set_sizing(Sizing::new().min_height(100.0));
    let bin = bin_of(&root);
    assert_eq!(bin.halign(), gtk::Align::Fill);
    assert_eq!(bin.valign(), gtk::Align::Fill);
    window.close();
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

/// By default, a `GtkScrolledWindow`'s `GtkViewport` squashes the content to its
/// minimum size when there is not enough room. A `List` without a height has a
/// minimum (one row) different from its natural size (all rows), so in scrollable
/// directions it is placed at its natural size.
fn scroll_gives_the_child_its_natural_height(ui: &Ui) -> Result<()> {
    let list = ui.list()?;
    list.set_sizing(Sizing::fill_width());
    list.set_items(&ListItem::list(["東京", "大阪", "札幌", "那覇", "福岡"]));
    let bin = bin_of(&list);
    let (minimum, natural) = measure_height(&bin);
    assert!(
        minimum < natural,
        "この検証は最小と自然な大きさが違う中身で行う: {minimum} / {natural}"
    );

    let pane = ui.stack(Orientation::Vertical)?;
    pane.append(&list);
    let scroll = ui.scroll()?;
    scroll.set_policy(ScrollPolicy::Never, ScrollPolicy::Auto);
    scroll.set_child(&pane);
    scroll.set_sizing(Sizing::fill());

    // 中身より低いウィンドウ。ここで潰れると一覧が 1 行分まで縮む。
    let window = ui.window("スクロール", 400.0, 120.0)?;
    window.set_child(&scroll);
    window.show();
    tick(&bin);
    assert_eq!(
        bin.height(),
        natural,
        "送れる向きでは自然な高さのまま置かれること (最小は {minimum})"
    );
    window.close();
    Ok(())
}

/// What a widget moved to another container keeps: a `Scroll` keeps its position,
/// while a field loses the focus (GTK4 moves it out of a widget that is unparented).
fn moved_widget_keeps_scroll_but_not_focus(ui: &Ui) -> Result<()> {
    // Keep a visible destination and change its alignment before moving the field.
    let window = ui.window("移動", 240.0, 300.0)?;
    let root = ui.stack(Orientation::Vertical)?;
    let first = ui.stack(Orientation::Vertical)?;
    let second = ui.stack(Orientation::Vertical)?;
    root.set_align(Align::Start);
    second.set_align(Align::End);
    second.append(&ui.label("移動先")?);
    root.append(&first);
    root.append(&second);
    let field = ui.text_input("入力中の文字")?;
    first.append(&field);
    let pane = ui.stack(Orientation::Vertical)?;
    for index in 0..40 {
        pane.append(&ui.label(&format!("行 {index}"))?);
    }
    let scroll = ui.scroll()?;
    scroll.set_child(&pane);
    scroll.set_sizing(
        Sizing::new()
            .width(Length::Fixed(200.0))
            .height(Length::Fixed(80.0)),
    );
    first.append(&scroll);
    window.set_child(&root);
    window.show();
    settle(&scroll);
    assert!(field.request_focus());
    pump();
    // フォーカスはウィンドウが持つ「いま入力を受ける子」で見る。
    let focused = || {
        let field = field.native_widget();
        field
            .root()
            .and_then(|root| root.focus())
            .is_some_and(|focus| focus == field || focus.is_ancestor(&field))
    };
    let had_focus = focused();
    let lost_focus = Rc::new(Cell::new(0));
    field.on_focus_change({
        let lost_focus = lost_focus.clone();
        let first = first.downgrade();
        let notice = ui.label("フォーカスが外れた")?;
        move |has_focus| {
            if !has_focus {
                lost_focus.set(lost_focus.get() + 1);
                if let Some(first) = first.upgrade() {
                    first.append(&notice);
                }
            }
        }
    });
    // Unrelated insertions and removals must leave the field focused.
    first.insert(0, &ui.label("見出し")?);
    first.remove(0);
    assert_eq!(focused(), had_focus);
    scroll.scroll_to(0.0, 120.0);
    settle(&scroll);
    assert_eq!(scroll.metrics().y, 120.0);

    first.remove(1);
    first.remove(0);
    second.append(&field);
    second.append(&scroll);
    if had_focus {
        assert!(!focused(), "再追加の直後からフォーカスを失うこと");
        assert!(lost_focus.get() > 0, "フォーカスを失った通知が届くこと");
        assert_eq!(first.len(), 1, "通知内でも元のコンテナを操作できること");
    }
    settle(&scroll);
    assert_eq!(field.text(), "入力中の文字");
    assert_eq!(scroll.metrics().y, 120.0, "スクロール位置が残ること");
    // ウィンドウがフォーカスを持てない環境では、移す前から持っていない。
    if had_focus {
        assert!(!focused(), "移した入力欄からはフォーカスが外れること");
    }
    window.close();
    Ok(())
}

/// Cover direct fields and nested fields across every container removal path.
fn containers_release_focus_before_detaching(ui: &Ui) -> Result<()> {
    for mode in 0..10 {
        let window = ui.window("取り外し", 440.0, 260.0)?;
        let root = ui.stack(Orientation::Vertical)?;
        let destination = ui.stack(Orientation::Vertical)?;
        destination.append(&ui.label("移動先")?);
        let pane = ui.stack(Orientation::Vertical)?;
        let field = ui.text_input("保持する文字列")?;
        pane.append(&field);
        let replacement = ui.label("差し替え")?;
        let detach: Box<dyn Fn()> = match mode {
            0 | 1 => {
                let source = ui.stack(Orientation::Vertical)?;
                source.append(&pane);
                root.append(&source);
                Box::new(move || {
                    if mode == 0 {
                        source.remove(0)
                    } else {
                        source.clear()
                    }
                })
            }
            2 | 3 => {
                let source = ui.grid()?;
                source.attach(&pane, GridCell::new(0, 0));
                root.append(&source);
                Box::new(move || {
                    if mode == 2 {
                        source.remove(GridCell::new(0, 0))
                    } else {
                        source.clear()
                    }
                })
            }
            4 => {
                let source = ui.scroll()?;
                source.set_child(&pane);
                root.append(&source);
                Box::new(move || source.set_child(&replacement))
            }
            5 => {
                let source = ui.expander("開閉")?;
                source.set_expanded(true);
                source.set_child(&pane);
                root.append(&source);
                Box::new(move || source.set_child(&replacement))
            }
            6 => {
                let source = ui.split_view(Orientation::Horizontal)?;
                source.set_start(&pane);
                source.set_end(&replacement);
                root.append(&source);
                let replacement = ui.label("新しい区画")?;
                Box::new(move || source.set_start(&replacement))
            }
            7 | 8 => {
                let source = ui.tabs()?;
                source.add_tab("入力", &pane);
                root.append(&source);
                Box::new(move || {
                    if mode == 7 {
                        source.remove_tab(0)
                    } else {
                        source.clear()
                    }
                })
            }
            _ => {
                window.set_child(&pane);
                let window = window.clone();
                let root = root.clone();
                Box::new(move || window.set_child(&root))
            }
        };
        root.append(&destination);
        if mode != 9 {
            window.set_child(&root);
        }
        window.show();
        tick(&field.native_widget());
        assert!(field.request_focus(), "shown mode {mode}");
        let focused = || {
            field
                .native_widget()
                .root()
                .and_then(|root| root.focus())
                .is_some_and(|focus| {
                    focus == field.native_widget() || focus.is_ancestor(&field.native_widget())
                })
        };
        assert!(focused(), "focused before mode {mode}");
        detach();
        destination.append(&pane);
        assert!(!focused(), "immediately after mode {mode}");
        tick(&field.native_widget());
        assert!(!focused(), "after layout mode {mode}");
        assert_eq!(field.text(), "保持する文字列");
        assert!(field.request_focus(), "explicit refocus mode {mode}");
        assert!(focused());
        window.close();
    }
    Ok(())
}

/// Puts a scroll view holding tall content into a window 100 high.
fn tall_scroll(
    ui: &Ui,
    rows: usize,
) -> Result<(naui_gtk::Window, naui_gtk::Scroll, naui_gtk::Stack)> {
    let window = ui.window("スクロール", 200.0, 100.0)?;
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

/// Advances frames until the requested destination is forgotten (after the next
/// layout).
fn settle(scroll: &naui_gtk::Scroll) {
    for _ in 0..3 {
        tick(&scroll.native_widget());
    }
}

fn scroll_measures_and_moves(ui: &Ui) -> Result<()> {
    let (window, scroll, _pane) = tall_scroll(ui, 40)?;
    window.show();
    settle(&scroll);

    let m = scroll.metrics();
    assert_eq!((m.x, m.y), (0.0, 0.0), "最初は先頭: {m:?}");
    assert!(m.viewport_height > 0.0, "{m:?}");
    assert!(
        m.content_height > m.viewport_height,
        "はみ出している: {m:?}"
    );

    scroll.scroll_to(0.0, 30.0);
    assert_eq!(
        scroll.metrics().y,
        30.0,
        "大きさが決まっていればその場で動く"
    );

    scroll.scroll_to_end();
    let end = scroll.metrics();
    assert_eq!(end.y, end.max_y(), "末尾へ: {end:?}");
    assert_eq!(end.distance_to_end(), 0.0);

    scroll.scroll_to(-50.0, 1e9);
    let clamped = scroll.metrics();
    assert_eq!((clamped.x, clamped.y), (0.0, clamped.max_y()));
    window.close();
    Ok(())
}

/// Calls "append, then scroll to the end", as in a chat, back to back without
/// waiting for layout.
fn scroll_to_end_after_appending(ui: &Ui) -> Result<()> {
    let (window, scroll, pane) = tall_scroll(ui, 20)?;
    window.show();
    settle(&scroll);
    scroll.scroll_to_end();
    let before = scroll.metrics();

    for i in 0..20 {
        pane.append(&ui.label(&format!("追加 {i}"))?);
    }
    scroll.scroll_to_end();
    settle(&scroll);
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
    settle(&scroll);
    let m = scroll.metrics();
    assert!(m.max_y() > 0.0, "{m:?}");
    assert_eq!(m.distance_to_end(), 0.0, "表示後に末尾にいる: {m:?}");
    window.close();
    Ok(())
}

/// Moves a scroll view that has been shown once into a background tab, then appends
/// and requests scrolling to the end while hidden. Frames keep advancing while
/// hidden, but the destination is remembered until it is shown again.
fn scroll_to_end_in_hidden_tab(ui: &Ui) -> Result<()> {
    let window = ui.window("タブ", 200.0, 140.0)?;
    let tabs = ui.tabs()?;
    let pane = ui.stack(Orientation::Vertical)?;
    for i in 0..40 {
        pane.append(&ui.label(&format!("行 {i}"))?);
    }
    let scroll = ui.scroll()?;
    scroll.set_child(&pane);
    scroll.set_sizing(Sizing::fill());
    tabs.add_tab("前", &ui.label("別のタブ")?);
    tabs.add_tab("スクロール", &scroll);
    tabs.set_selected(1);
    window.set_child(&tabs);
    window.show();
    settle(&scroll);
    scroll.scroll_to_end();
    settle(&scroll);
    let before = scroll.metrics();

    tabs.set_selected(0);
    settle(&scroll);
    for i in 0..20 {
        pane.append(&ui.label(&format!("追加 {i}"))?);
    }
    scroll.scroll_to_end();
    // 隠れている間にフレームを進める。数えていれば行き先を忘れる。
    for _ in 0..10 {
        tick(&tabs.native_widget());
    }

    tabs.set_selected(1);
    settle(&scroll);
    let after = scroll.metrics();
    assert!(
        after.content_height > before.content_height,
        "中身が伸びている: {before:?} → {after:?}"
    );
    assert_eq!(
        after.distance_to_end(),
        0.0,
        "戻したら末尾にいる: {after:?}"
    );
    window.close();
    Ok(())
}

fn scroll_notifies_only_moves(ui: &Ui) -> Result<()> {
    let (window, scroll, _pane) = tall_scroll(ui, 40)?;
    window.show();
    settle(&scroll);
    let seen = Rc::new(RefCell::new(Vec::new()));
    scroll.on_scroll({
        let seen = seen.clone();
        move |m: naui_core::ScrollMetrics| seen.borrow_mut().push(m.y)
    });

    scroll.scroll_to(0.0, 25.0);
    scroll.scroll_to(0.0, 25.0); // 同じ位置へは動かない
    settle(&scroll);
    assert_eq!(*seen.borrow(), vec![25.0]);

    // 幅だけ変えても、位置が変わらなければ届かない。
    window.set_size(260.0, 100.0);
    settle(&scroll);
    assert_eq!(*seen.borrow(), vec![25.0]);

    // 利用者の操作 (ネイティブ側での移動) でも届く。
    let native: gtk::ScrolledWindow = scroll
        .native_widget()
        .downcast()
        .expect("GtkScrolledWindow");
    native.vadjustment().set_value(40.0);
    assert_eq!(*seen.borrow(), vec![25.0, 40.0]);
    window.close();
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

/// `set_role` maps to libadwaita's style classes, and switching roles takes off the
/// previous one.
fn button_role_maps_to_style_classes(ui: &Ui) -> Result<()> {
    let button = ui.button("保存")?;
    let native: gtk::Button = button.native_widget().downcast().expect("GtkButton");
    let classes = || {
        ["suggested-action", "destructive-action"]
            .into_iter()
            .filter(|class| native.has_css_class(class))
            .collect::<Vec<_>>()
    };
    assert!(classes().is_empty(), "既定ではクラスを付けないこと");

    button.set_role(ButtonRole::Primary);
    assert_eq!(classes(), ["suggested-action"]);
    button.set_role(ButtonRole::Destructive);
    assert_eq!(classes(), ["destructive-action"], "前の役割が外れること");
    button.set_role(ButtonRole::Normal);
    assert!(classes().is_empty(), "既定へ戻すとクラスが外れること");
    Ok(())
}

/// `set_style` / `set_color` map to libadwaita style classes, and the text size
/// actually changes.
///
/// naui holds neither point sizes nor colors, so what is checked is "which classes
/// are attached" and "how the theme draws them".
fn label_style_and_color_map_to_style_classes(ui: &Ui) -> Result<()> {
    let label = ui.label("見出し")?;
    let native: gtk::Label = label.native_widget().downcast().expect("GtkLabel");

    // 既定 (`Body` / `Default`) はクラスを付けない。
    for style in TextStyle::ALL {
        if let Some(class) = style.style_class() {
            assert!(!native.has_css_class(class), "既定で {class} が付いている");
        }
    }

    // 段階を切り替えると、前の段階のクラスは外れる。
    label.set_style(TextStyle::Title);
    assert!(native.has_css_class("title-2"), "title-2 が付くこと");
    label.set_style(TextStyle::Caption);
    assert!(!native.has_css_class("title-2"), "前の段階が外れること");
    assert!(native.has_css_class("caption"), "caption が付くこと");
    label.set_style(TextStyle::Body);
    for style in TextStyle::ALL {
        if let Some(class) = style.style_class() {
            assert!(
                !native.has_css_class(class),
                "本文へ戻すと {class} が外れること"
            );
        }
    }

    // 色も同じ決まり。
    label.set_color(TextColor::Danger);
    assert!(native.has_css_class("error"), "error が付くこと");
    label.set_color(TextColor::Secondary);
    assert!(!native.has_css_class("error"), "前の役割が外れること");
    assert!(native.has_css_class("dim-label"), "dim-label が付くこと");
    label.set_color(TextColor::Default);
    for color in TextColor::ALL {
        if let Some(class) = color.style_class() {
            assert!(
                !native.has_css_class(class),
                "既定へ戻すと {class} が外れること"
            );
        }
    }

    // 段階が上がるほど、テーマは大きく (または太く) 描く。CSS はウィジェットが
    // 画面へ付いてから解決されるので、ウィンドウへ入れて測る。
    let window = ui.window("文字づかい", 320.0, 120.0)?;
    window.set_child(&label);
    window.show();

    // 見るのは、テーマが決めた級数と太さの組。libadwaita は `.title-2` と
    // `.title-3` に同じ級数を与えて太さだけを変えるので、大きさだけでは
    // 段階の違いを見分けられない。
    let mut fonts = Vec::new();
    for style in [
        TextStyle::Caption,
        TextStyle::Body,
        TextStyle::Heading,
        TextStyle::Subtitle,
        TextStyle::Title,
        TextStyle::LargeTitle,
    ] {
        label.set_style(style);
        tick(&native);
        let font = native
            .pango_context()
            .font_description()
            .expect("テーマが決めた書体");
        fonts.push((style, font.size(), font.weight()));
    }
    for pair in fonts.windows(2) {
        let (before, before_size, before_weight) = pair[0];
        let (after, after_size, after_weight) = pair[1];
        assert!(
            after_size >= before_size,
            "段階が上がって小さくならないこと: {before:?} {before_size} / {after:?} {after_size}"
        );
        assert!(
            (after_size, after_weight) != (before_size, before_weight),
            "段階ごとに違う描き方になること: {before:?} と {after:?} が同じ ({after_size}, {after_weight:?})"
        );
    }
    window.close();
    Ok(())
}

/// A container wrapping wrapping content reports that "the height is decided once
/// the width is decided".
///
/// Without this, the parent asks for the height without passing a width and allots
/// only **the height of one unwrapped line**. That is why descriptive text placed
/// in a narrow pane (a `SplitView` and the like) appeared to overflow and overlap by
/// the wrapped amount.
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

/// Takes out only the items' toggle buttons.
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
    let mut items = NavItem::list(["ホーム", "書類", "2026"]);
    items[1].enabled = false;
    breadcrumbs.set_items(&items);
    assert_eq!(breadcrumbs.len(), 3);
    assert_eq!(breadcrumbs.selected(), Some(2), "末尾がいまいる場所");

    // 項目はボタンではなくラベルで、間に区切りのラベルが入る。
    let native: gtk::Box = breadcrumbs.native_widget().downcast().expect("GtkBox");
    let labels: Vec<gtk::Label> = children(&native)
        .into_iter()
        .map(|child| child.downcast().expect("子はどれも GtkLabel"))
        .collect();
    let texts: Vec<String> = labels.iter().map(|l| l.text().to_string()).collect();
    assert_eq!(texts, ["ホーム", "›", "書類", "›", "2026"]);
    // 選べる祖先はリンク、選べない項目と現在地はリンクにしない。
    assert!(
        labels[0].label().contains("<a href"),
        "{}",
        labels[0].label()
    );
    assert!(
        !labels[2].label().contains("<a href"),
        "{}",
        labels[2].label()
    );
    assert!(!labels[2].is_sensitive(), "選べない項目は無効");
    // 現在地は太字にもしない (選び直しで文字幅が変わらないように)。
    assert_eq!(labels[4].label(), "2026");

    let (log, sink) = recorder::<usize>();
    breadcrumbs.on_select(sink);
    let handled: bool = labels[0].emit_by_name("activate-link", &[&"0"]);
    assert!(handled, "リンクの既定の処理 (URI を開く) へは渡さない");
    // リンクを押している最中は何も書き換えない。GTK4 がその場でラベルを
    // 作り直されると落ちるため、選び直しは次の周回へ回している。
    assert_eq!(breadcrumbs.selected(), Some(2), "押した時点では動かない");
    assert!(log.borrow().is_empty(), "通知も次の周回");
    pump();
    assert_eq!(log.borrow().as_slice(), [0]);
    assert_eq!(breadcrumbs.selected(), Some(0));
    assert_eq!(labels[0].label(), "ホーム");
    assert!(
        labels[4].label().contains("<a href"),
        "{}",
        labels[4].label()
    );

    // 選べない項目は選ばれない。
    breadcrumbs.select(1);
    assert_eq!(breadcrumbs.selected(), Some(0));
    assert_eq!(log.borrow().len(), 1);

    // 押したあとで階層が作り直されたら、待っていた選び直しは捨てる。
    let handled: bool = labels[4].emit_by_name("activate-link", &[&"2"]);
    assert!(handled);
    breadcrumbs.set_items(&NavItem::list(["ホーム", "書類", "2026"]));
    pump();
    assert_eq!(log.borrow().len(), 1, "作り直す前の項目は選ばれない");
    assert_eq!(breadcrumbs.selected(), Some(2), "作り直した末尾のまま");

    // 記号はマークアップとして解釈しない。
    breadcrumbs.set_items(&NavItem::list(["<a>&", "末尾"]));
    let first: gtk::Label = native
        .first_child()
        .expect("子")
        .downcast()
        .expect("GtkLabel");
    assert_eq!(first.text(), "<a>&");
    Ok(())
}

fn breadcrumbs_have_no_context_menu(ui: &Ui) -> Result<()> {
    let breadcrumbs = ui.breadcrumbs()?;
    breadcrumbs.set_items(&NavItem::list(["ホーム", "書類"]));
    let window = ui.window("パンくず", 320.0, 80.0)?;
    window.set_child(&breadcrumbs);
    window.show();
    let native: gtk::Box = breadcrumbs.native_widget().downcast().expect("GtkBox");
    let link: gtk::Label = native
        .first_child()
        .expect("子")
        .downcast()
        .expect("GtkLabel");
    tick(&link);

    // 比べる相手: 素のリンク入りラベルは、menu.popup でメニュー (GtkPopoverMenu) を子に作る。
    let plain = gtk::Label::new(None);
    plain.set_markup("<a href=\"0\">素</a>");
    native.append(&plain);
    tick(&plain);
    plain
        .activate_action("menu.popup", None)
        .expect("menu.popup がある");
    assert!(
        plain.first_child().is_some(),
        "素のラベルはメニューを作ること (比較の前提)"
    );
    native.remove(&plain);

    // キー操作の経路: menu.popup を起こしてもメニューは作られない。
    let _ = link.activate_action("menu.popup", None);
    assert!(link.first_child().is_none(), "キー操作でメニューが出ない");

    // 右クリックの経路: 捕捉段階で右ボタンを取るジェスチャーがある。
    let controllers = link.observe_controllers();
    let claims_secondary = (0..controllers.n_items())
        .filter_map(|i| controllers.item(i))
        .filter_map(|item| item.downcast::<gtk::GestureClick>().ok())
        .any(|gesture| {
            gesture.button() == gtk::gdk::BUTTON_SECONDARY
                && gesture.propagation_phase() == gtk::PropagationPhase::Capture
        });
    assert!(claims_secondary, "右クリックを先に取るジェスチャーがある");
    window.close();
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

/// Tabs can be removed later. Reselection is not notified.
fn tabs_remove_and_clear(ui: &Ui) -> Result<()> {
    let tabs = ui.tabs()?;
    let first = ui.label("1 枚目")?;
    tabs.add_tab("あ", &first);
    tabs.add_tab("い", &ui.label("2 枚目")?);
    tabs.add_tab("う", &ui.label("3 枚目")?);
    assert_eq!(tabs.len(), 3);

    let (log, sink) = recorder::<usize>();
    tabs.on_select(sink);

    tabs.set_selected(2);
    assert_eq!(tabs.selected(), Some(2));

    // 選択より前のタブを外すと、選択は同じタブへ付いていく。
    tabs.remove_tab(0);
    assert_eq!(tabs.len(), 2);
    assert_eq!(tabs.selected(), Some(1));
    assert!(log.borrow().is_empty(), "外したことは通知しないこと");

    let native: gtk::Notebook = tabs.native_widget().downcast().expect("GtkNotebook");
    assert_eq!(native.n_pages(), 2, "GtkNotebook 側もページが減ること");
    assert!(
        bin_of(&first).parent().is_none(),
        "外したタブの中身は GtkNotebook から抜けること"
    );

    // 選択中のタブを外しても通知はしない。
    tabs.remove_tab(1);
    assert_eq!(tabs.len(), 1);
    assert_eq!(tabs.selected(), Some(0));
    assert!(log.borrow().is_empty());

    // 範囲外は何もしない。
    tabs.remove_tab(5);
    assert_eq!(tabs.len(), 1);

    tabs.clear();
    assert!(tabs.is_empty());
    assert_eq!(tabs.selected(), None);
    assert_eq!(native.n_pages(), 0);
    assert!(log.borrow().is_empty());

    // 空にした後もふつうに足せる。
    tabs.add_tab("え", &ui.label("新しい 1 枚目")?);
    assert_eq!(tabs.len(), 1);
    assert_eq!(tabs.selected(), Some(0));
    Ok(())
}

/// Focus loss is synchronous: callbacks can clear, replace or reorder pages
/// before the outer removal resumes. Follow the page, not its old index.
fn tabs_remove_after_focus_callback(ui: &Ui) -> Result<()> {
    for mode in 0..5 {
        let window = ui.window("タブの再入", 480.0, 240.0)?;
        let tabs = ui.tabs()?;
        let before = ui.label("前")?;
        let field = ui.text_input("削除する入力欄")?;
        let after = ui.label("後")?;
        let last = ui.label("最後")?;
        let replacement = ui.label("新しいページ")?;
        tabs.add_tab("前", &before);
        tabs.add_tab("入力", &field);
        tabs.add_tab("後", &after);
        tabs.add_tab("最後", &last);
        tabs.set_selected(1);
        window.set_child(&tabs);
        window.show();
        tick(&field.native_widget());
        assert!(field.request_focus());
        let called = Rc::new(Cell::new(0));
        field.on_focus_change({
            let tabs = tabs.downgrade();
            let replacement = replacement.clone();
            let called = called.clone();
            move |focused| {
                if focused {
                    return;
                }
                called.set(called.get() + 1);
                let tabs = tabs.upgrade().expect("タブが生存");
                match mode {
                    0 => tabs.clear(),
                    1 => tabs.remove_tab(0),
                    2 => {
                        tabs.remove_tab(1);
                        tabs.add_tab("新規", &replacement);
                    }
                    3 => {
                        tabs.set_selected(3);
                        tabs.remove_tab(0);
                    }
                    _ => {
                        tabs.clear();
                        tabs.add_tab("新規", &replacement);
                    }
                }
            }
        });
        let (log, sink) = recorder::<usize>();
        tabs.on_select(sink);
        // Close the real window even when the old implementation panics, so the
        // failure does not leave a window interfering with subsequent tests.
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            tabs.remove_tab(1);
            assert_eq!(called.get(), 1, "focus callback mode {mode}");
            let native: gtk::Notebook = tabs.native_widget().downcast().expect("GtkNotebook");
            let expected: Vec<gtk::Widget> = match mode {
                0 => vec![],
                1 | 3 => vec![bin_of(&after), bin_of(&last)],
                2 => vec![
                    bin_of(&before),
                    bin_of(&after),
                    bin_of(&last),
                    bin_of(&replacement),
                ],
                _ => vec![bin_of(&replacement)],
            };
            assert_eq!(tabs.len(), expected.len(), "page count mode {mode}");
            for (index, page) in expected.iter().enumerate() {
                assert_eq!(
                    native.nth_page(Some(index as u32)).as_ref(),
                    Some(page),
                    "page identity mode {mode}"
                );
            }
            let selected = match mode {
                0 => None,
                1 | 4 => Some(0),
                _ => Some(1),
            };
            assert_eq!(
                tabs.selected(),
                selected,
                "selection after callback mode {mode}"
            );
            // Rebuilding an empty notebook adds and selects a new first page.
            // Keep that callback intact; removal itself must not add a notice.
            let notices: &[usize] = if mode == 4 { &[0] } else { &[] };
            assert_eq!(
                log.borrow().as_slice(),
                notices,
                "selection notices mode {mode}"
            );
            assert!(bin_of(&field).parent().is_none());
            // Exercise the Rust child list again, as well as checking native pages.
            field.on_focus_change(|_| {});
            tabs.clear();
            tabs.add_tab("再利用", &field);
            tabs.remove_tab(0);
            assert!(tabs.is_empty());
        }));
        window.close();
        if let Err(panic) = outcome {
            std::panic::resume_unwind(panic);
        }
    }
    Ok(())
}

/// A `Stack` can insert, remove and clear children later.
fn stack_inserts_and_removes_children(ui: &Ui) -> Result<()> {
    let stack = ui.stack(Orientation::Vertical)?;
    let first = ui.label("A")?;
    stack.append(&first);
    stack.append(&ui.label("C")?);
    stack.insert(1, &ui.label("B")?);
    assert_eq!(stack.len(), 3);

    let native: gtk::Box = stack.native_widget().downcast().expect("GtkBox");
    let labels = |native: &gtk::Box| -> Vec<String> {
        children(native)
            .iter()
            .filter_map(|bin| bin.first_child())
            .filter_map(|child| child.downcast::<gtk::Label>().ok())
            .map(|label| label.text().to_string())
            .collect()
    };
    assert_eq!(labels(&native), ["A", "B", "C"]);

    // 範囲外の index は末尾へ足す。
    stack.insert(99, &ui.label("D")?);
    assert_eq!(labels(&native), ["A", "B", "C", "D"]);

    stack.remove(1);
    assert_eq!(stack.len(), 3);
    assert_eq!(labels(&native), ["A", "C", "D"]);

    // 範囲外の index は何もしない。
    stack.remove(9);
    assert_eq!(stack.len(), 3);

    stack.clear();
    assert!(stack.is_empty());
    assert_eq!(children(&native).len(), 0);
    assert!(
        bin_of(&first).parent().is_none(),
        "外した子は GtkBox から抜けること"
    );

    // 空にした後もふつうに積める。
    stack.append(&ui.label("F")?);
    assert_eq!(labels(&native), ["F"]);
    Ok(())
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
    assert!(bin_of(&name).parent().is_none());
    assert!(bin_of(&field).parent().is_some());

    // 何も無いマスを指定しても何も起きない。
    grid.remove(GridCell::new(0, 0));
    assert_eq!(grid.len(), 1);

    // replace は「そのマスだけ」差し替える。
    grid.attach(&ui.label("表示名")?, GridCell::new(0, 0));
    grid.replace(&ui.label("別名")?, GridCell::new(0, 0));
    assert_eq!(grid.len(), 2, "replace は他のマスの子を外さないこと");

    grid.clear();
    assert!(grid.is_empty());
    assert!(bin_of(&field).parent().is_none());

    // 行と列の指定は残るので、そのまま置き直せる。
    grid.attach(&ui.label("再")?, GridCell::new(0, 0));
    assert_eq!(grid.len(), 1);
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
    let row_labels = |row: &gtk::Widget| -> Vec<gtk::Label> {
        let content = row
            .clone()
            .downcast::<gtk::ListBoxRow>()
            .expect("GtkListBoxRow")
            .child()
            .expect("中身");
        labels_in(&content)
    };
    let texts = |row: &gtk::Widget| -> Vec<String> {
        row_labels(row)
            .into_iter()
            .map(|label| label.text().to_string())
            .collect()
    };
    assert_eq!(texts(&rows[0]), ["東京", "13,960,000 人"]);
    assert_eq!(texts(&rows[1]), ["札幌"]);

    // 補助の文字は `TextStyle::Caption` と `TextColor::Secondary` で小さく淡く
    // なる。naui 側で級数も色も持たないので、当たっているクラスで見る。
    let detail = row_labels(&rows[0]).pop().expect("補助の文字");
    assert!(detail.has_css_class("caption"), "補助は小さくなること");
    assert!(detail.has_css_class("dim-label"), "補助は淡くなること");
    Ok(())
}

/// A row's content is placed across the row's full width.
///
/// The `SizeBin` wrapping the content defaults to `Center` (fit the content), so
/// putting it into a `GtkListBoxRow` as is would center the text in the row. GTK4
/// allots the sizes, so it is shown on screen and checked with the allotted width.
fn list_row_content_fills_the_row(ui: &Ui) -> Result<()> {
    let list = ui.list()?;
    list.set_sizing(Sizing::fill());
    list.set_items(&[ListItem::new("東京").detail("13,960,000 人")]);

    let window = ui.window("一覧", 400.0, 200.0)?;
    window.set_child(&list);
    window.show();
    let native = list_box_of(&list);
    tick(&native);

    let row = children(&native)
        .remove(0)
        .downcast::<gtk::ListBoxRow>()
        .expect("GtkListBoxRow");
    let content = row.child().expect("行の中身");
    assert!(row.width() > 0, "行に幅が配られていること");
    assert_eq!(
        content.width(),
        row.width() - content.margin_start() - content.margin_end(),
        "中身が行の幅いっぱいに置かれること (行の幅 {})",
        row.width()
    );
    window.close();
    Ok(())
}

/// A row's text spreads to the full width of the row, and overflow is cut off with
/// an ellipsis.
///
/// `GtkLabel` cuts the end with `PangoEllipsizeMode::End`, but **that works only
/// once the width is decided**. At the content's width, a long label would push the
/// row wider.
fn list_long_text_stays_inside_the_row(ui: &Ui) -> Result<()> {
    let list = ui.list()?;
    let long = "これは行の幅にはとても収まらない、ずいぶん長いラベルの文字列です";
    list.set_items(&[ListItem::new(long).detail(long)]);
    list.set_sizing(Sizing::fill());

    let window = ui.window("一覧", 240.0, 200.0)?;
    window.set_child(&list);
    window.show();
    let native = list_box_of(&list);
    // 配られた幅を測るので、フレームを 1 つ進めてから見る。
    tick(&native);

    let row = children(&native).into_iter().next().expect("1 行目");
    let row_width = row.width();
    assert!(row_width > 0, "行に幅が配られていること");

    let labels = labels_in(&row);
    assert_eq!(labels.len(), 2, "文字が 2 本あること");
    for (index, label) in labels.iter().enumerate() {
        assert!(
            label.width() <= row_width,
            "{index} 本目の文字が行からはみ出している: 文字 {} / 行 {row_width}",
            label.width()
        );
        // 幅いっぱいまでは使う (内容の幅のまま縮こまっていない)。
        assert!(
            label.width() * 2 > row_width,
            "{index} 本目の文字が行の幅を使っていない: 文字 {} / 行 {row_width}",
            label.width()
        );
    }
    window.close();
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

/// Rows for settings screens can hold any layout and controls, not just labels.
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

/// `Ui` can be cloned, so widgets can be created even inside callbacks.
///
/// A screen that adds a row on each click builds the row's content inside the
/// notification and passes it to `set_rows`.
fn ui_clone_builds_rows_from_a_callback(ui: &Ui) -> Result<()> {
    let list = ui.list()?;
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

    assert!(list.is_empty());
    let native_button: gtk::Button = add.native_widget().downcast().expect("GtkButton");
    native_button.emit_clicked();
    native_button.emit_clicked();
    assert_eq!(list.len(), 2, "コールバックの中で作った行が載ること");

    // 後から足した行も、他の行と同じように GtkListBoxRow になる。
    let native = list_box_of(&list);
    let native_rows = children(&native);
    assert_eq!(native_rows.len(), 2, "GtkListBox 側も 2 行になること");
    assert!(native_rows[1].is::<gtk::ListBoxRow>());

    // 選択もふつうの行と同じに動く。
    list.select(1);
    assert_eq!(list.selected(), Some(1));
    let selected: Vec<i32> = native
        .selected_rows()
        .iter()
        .map(|row| row.index())
        .collect();
    assert_eq!(selected, vec![1], "GtkListBox 側の選択も動くこと");
    Ok(())
}

/// A click on the row itself reaches the closure.
///
/// Buttons and check boxes inside the row receive their own clicks, so on the GTK
/// side too they do not fire twice together with the row's activation.
/// The notification `GtkListBox` emits when a row is clicked with the mouse.
///
/// GTK4's `GtkGestureClick` only emits `row-activated` and does not go through
/// `GtkListBoxRow::activate` (the keyboard side).
fn activate_row(list: &gtk::ListBox, row: &gtk::ListBoxRow) {
    list.emit_by_name::<()>("row-activated", &[row]);
}

/// The `GtkGestureClick` attached to the row that receives the given button.
fn row_gesture(row: &gtk::ListBoxRow, button: u32) -> gtk::GestureClick {
    let controllers = row.observe_controllers();
    (0..controllers.n_items())
        .filter_map(|index| controllers.item(index))
        .filter_map(|item| item.downcast::<gtk::GestureClick>().ok())
        .find(|gesture| gesture.button() == button)
        .expect("行のクリックを受けるジェスチャー")
}

/// Shows a list with rows in a window, for checking activation and the context
/// menu.
fn shown_list_for_events(ui: &Ui) -> Result<(naui_gtk::Window, naui_gtk::List)> {
    let window = ui.window("行の操作", 320.0, 240.0)?;
    let list = ui.list()?;
    list.set_items(&[
        ListItem::new("札幌"),
        ListItem::new("仙台"),
        ListItem::new("東京").detail("13,960,000 人"),
        ListItem::new("大阪").enabled(false),
    ]);
    list.set_sizing(Sizing::fill());
    window.set_child(&list);
    window.show();
    for _ in 0..3 {
        tick(&list.native_widget());
    }
    Ok((window, list))
}

/// Double-clicks, and Enter while a row has focus, reach `on_row_activate`.
fn list_row_activate_notifies(ui: &Ui) -> Result<()> {
    let (window, list) = shown_list_for_events(ui)?;
    let (log, sink) = recorder::<usize>();
    list.on_row_activate(sink);
    let native = list_box_of(&list);
    let row = |index: i32| native.row_at_index(index).expect("行");

    // 1 回目の押下では起動しない。2 回目 (n_press = 2) で起動する。
    let double = row_gesture(&row(2), gtk::gdk::BUTTON_PRIMARY);
    double.emit_by_name::<()>("pressed", &[&1i32, &10.0f64, &5.0f64]);
    assert!(log.borrow().is_empty(), "1 回のクリックでは起動しないこと");
    double.emit_by_name::<()>("pressed", &[&2i32, &10.0f64, &5.0f64]);
    assert_eq!(
        log.borrow().as_slice(),
        [2],
        "ダブルクリックした行が届くこと"
    );

    // 選べない行は GTK が押下を届けない。届いたとしても起動しない。
    row_gesture(&row(3), gtk::gdk::BUTTON_PRIMARY)
        .emit_by_name::<()>("pressed", &[&2i32, &10.0f64, &5.0f64]);
    assert_eq!(log.borrow().as_slice(), [2], "選べない行では起動しないこと");

    // Enter は、フォーカスのある行を起動する。
    let enter = {
        let controllers = native.observe_controllers();
        (0..controllers.n_items())
            .filter_map(|index| controllers.item(index))
            .filter_map(|item| item.downcast::<gtk::EventControllerKey>().ok())
            .find(|keys| keys.propagation_phase() == gtk::PropagationPhase::Capture)
            .expect("Enter の受け口")
    };
    let press = |key: gtk::gdk::Key, state: gtk::gdk::ModifierType| {
        let _: bool = enter.emit_by_name("key-pressed", &[&key, &0u32, &state]);
    };
    assert!(row(1).grab_focus(), "行にフォーカスを移せること");
    press(gtk::gdk::Key::Return, gtk::gdk::ModifierType::empty());
    assert_eq!(
        log.borrow().as_slice(),
        [2, 1],
        "Enter でフォーカスの行が届くこと"
    );
    press(gtk::gdk::Key::Return, gtk::gdk::ModifierType::CONTROL_MASK);
    press(gtk::gdk::Key::a, gtk::gdk::ModifierType::empty());
    assert_eq!(
        log.borrow().len(),
        2,
        "修飾キー付きの Enter や他のキーでは起動しないこと"
    );

    window.close();
    Ok(())
}

/// Right-clicks reach `on_context_menu`, with positions in the same coordinates as
/// `PopupMenu::open_at(&list, ..)` (the `GtkListBox`'s coordinates).
fn list_context_menu_notifies(ui: &Ui) -> Result<()> {
    let (window, list) = shown_list_for_events(ui)?;
    let (log, mut sink) = recorder::<(usize, Point)>();
    list.on_context_menu(move |index, at| sink((index, at)));
    let native = list_box_of(&list);
    let row = native.row_at_index(2).expect("3 行目");

    // 窓を出したときにフォーカスの行が選ばれることがあるので、前後で比べる。
    let before = list.selection();
    let gesture = row_gesture(&row, gtk::gdk::BUTTON_SECONDARY);
    gesture.emit_by_name::<()>("pressed", &[&1i32, &7.0f64, &4.0f64]);
    let expected = row
        .compute_point(&native, &gtk::graphene::Point::new(7.0, 4.0))
        .expect("行の位置");
    {
        let log = log.borrow();
        assert_eq!(log.len(), 1, "右クリックが 1 回だけ届くこと");
        assert_eq!(log[0].0, 2, "右クリックした行のインデックスが届くこと");
        assert!(
            (log[0].1.x - f64::from(expected.x())).abs() < 0.5
                && (log[0].1.y - f64::from(expected.y())).abs() < 0.5,
            "位置がリストの座標であること: {:?}",
            log[0].1
        );
        assert!(log[0].1.y > 0.0, "3 行目は上端より下にあること");
    }
    assert_eq!(
        list.selection(),
        before,
        "右クリックでは選択が変わらないこと"
    );

    row_gesture(
        &native.row_at_index(3).expect("4 行目"),
        gtk::gdk::BUTTON_SECONDARY,
    )
    .emit_by_name::<()>("pressed", &[&1i32, &7.0f64, &4.0f64]);
    assert_eq!(log.borrow().len(), 1, "選べない行では届かないこと");

    window.close();
    Ok(())
}

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

    // マウスのクリックは `GtkListBoxRow` の `activate` を出さない。
    // `GtkListBox` が `row-activated` を出すだけなので、そちらで確かめる。
    activate_row(&native, &first);
    assert_eq!(log.borrow().as_slice(), [true]);
    assert!(check.is_checked(), "行のクリックからチェックが変わること");

    // 2 回目も同じクロージャが呼ばれる (取り出したまま失われない)。
    activate_row(&native, &first);
    assert_eq!(log.borrow().as_slice(), [true, false]);

    // キーボード (Enter / Space) は行の `activate` から同じ経路へ入る。
    first.emit_activate();
    assert_eq!(log.borrow().as_slice(), [true, false, true]);

    let second = native.row_at_index(1).expect("2 行目");
    activate_row(&native, &second);
    assert_eq!(text_log.borrow().len(), 1);
    assert_eq!(log.borrow().len(), 3, "他の行は呼ばれないこと");

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

/// The headings' `GtkBox` and the `GtkListBox` the rows are in.
fn table_parts(table: &naui_gtk::Table) -> (gtk::Box, gtk::ListBox) {
    let root: gtk::Box = table.native_widget().downcast().expect("GtkBox");
    let header = root
        .first_child()
        .expect("見出し")
        .downcast::<gtk::Box>()
        .expect("見出しは GtkBox");
    (header, table.native_list_box())
}

/// Collects the `GtkLabel`s inside, in order.
fn table_labels(container: &impl IsA<gtk::Widget>) -> Vec<gtk::Label> {
    children(container)
        .into_iter()
        .filter_map(|child| child.downcast::<gtk::Label>().ok())
        .collect()
}

/// Even with built rows (`TableCells`), only the visible rows are created when
/// there are many, and sorting by heading still works.
fn table_builder_rows_scale_and_sort(ui: &Ui) -> Result<()> {
    const ROWS: usize = 20_000;
    let built = Rc::new(Cell::new(0usize));
    let descending = Rc::new(Cell::new(false));

    let table = ui.table()?;
    table.set_columns(&[
        TableColumn::new("番号").sortable(true),
        TableColumn::new("操作"),
    ]);
    table.set_sizing(Sizing::fill());
    let window = ui.window("大きな表", 400.0, 200.0)?;
    window.set_child(&table);
    window.show();

    table.set_row_builder(ROWS, {
        let ui = ui.clone();
        let built = built.clone();
        let descending = descending.clone();
        move |index| {
            built.set(built.get() + 1);
            // 並べ替えはアプリの仕事。ここでは向きを反転するだけ。
            let value = match descending.get() {
                true => ROWS - 1 - index,
                false => index,
            };
            let open = ui.button("開く")?;
            Ok(TableCells::new().text(value.to_string()).cell(&open))
        }
    });
    tick(&table.native_list_box());

    let list = table.native_list_box();
    let realized = children(&list).len();
    assert!(
        realized > 0 && realized < 200,
        "見えている分だけを組み立てること: {realized} 行"
    );
    assert!(
        built.get() < 400,
        "組み立てを呼ぶのも見えている行の分だけであること: {} 回",
        built.get()
    );
    assert!(
        find_button(&children(&list)[0]).is_some(),
        "セルの中がボタンであること"
    );

    // 見出しを押すと、組み立てる中身のほうが入れ替わる。
    table.on_sort({
        let table = table.clone();
        let descending = descending.clone();
        move |_column, order| {
            descending.set(!order.is_ascending());
            // 行数は変わらないので `refresh` で組み立て直す。
            table.refresh();
        }
    });
    let (header, _) = table_parts(&table);
    let button = children(&header)
        .into_iter()
        .find_map(|child| child.downcast::<gtk::Button>().ok())
        .expect("押せる見出し");
    button.emit_clicked();
    button.emit_clicked();

    assert_eq!(
        table_cells(&children(&list)[0])[0],
        (ROWS - 1).to_string(),
        "並べ替えた結果が出ること"
    );
    assert!(
        built.get() < 800,
        "並べ替えでも作り直すのは見えている行だけであること: {} 回",
        built.get()
    );
    window.close();
    Ok(())
}

/// Even with built rows, whether a row can be selected is answered without creating
/// it.
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

    // 画面に出る分はここまでで組み立てられている。以降は増えないはず。
    let realized = built.get();

    // 述語が無いと、まだ作っていない行は選べる扱いになる。
    table.set_selection(&[LOCKED]);
    assert_eq!(table.selection(), vec![LOCKED], "述語が無ければ選べる扱い");

    // 述語を渡すと、行を組み立てずに落とせる。
    table.clear_selection();
    let seen = Rc::new(RefCell::new(Vec::new()));
    table.set_row_selectable({
        let seen = seen.clone();
        move |index| {
            seen.borrow_mut().push(index);
            index != LOCKED
        }
    });
    table.set_selection(&[LOCKED]);
    assert!(table.selection().is_empty(), "選べない行は取り除かれること");
    table.set_selection(&[LOCKED - 1]);
    assert_eq!(table.selection(), vec![LOCKED - 1]);
    assert!(seen.borrow().contains(&LOCKED), "述語が呼ばれること");
    assert_eq!(
        built.get(),
        realized,
        "選択のために行を組み立て直さないこと"
    );
    Ok(())
}

/// In a table whose rows are limited, the width of widgets in cells does not move
/// the column width.
///
/// Column widths are decided only by the built rows, so if the content's natural
/// width became the column width, the columns would move every time scrolling shows
/// other rows.
fn table_widget_cells_keep_the_column_width(ui: &Ui) -> Result<()> {
    const ROWS: usize = 5_000;
    let table = ui.table()?;
    // どちらの列も幅を指定しない (余りを分け合う)。
    table.set_columns(&TableColumn::list(["操作", "名前"]));
    table.set_sizing(Sizing::fill());
    let window = ui.window("列の幅", 400.0, 200.0)?;
    window.set_child(&table);
    window.show();

    table.set_row_builder(ROWS, {
        let ui = ui.clone();
        move |index| {
            // 先頭には出さず、ずっと下の行だけラベルを長くする。
            let label = match index >= 3_000 {
                true => "とても長いラベルの操作ボタン",
                false => "実行",
            };
            let button = ui.button(label)?;
            Ok(TableCells::new()
                .cell(&button)
                .text(format!("項目 {index}")))
        }
    });
    tick(&table.native_list_box());

    let (header, list) = table_parts(&table);
    let column = children(&header)[0].clone();
    let before = column.width();
    assert!(before > 0, "列の幅が決まっていること");

    // 長いラベルの行が出るところまでスクロールする。
    let scroller = scroller_of(&list).expect("GtkScrolledWindow");
    let adjustment = scroller.vadjustment();
    adjustment.set_value(table.row_height() * 3_500.0);
    tick(&list);
    let long_label_shown = children(&list).iter().any(|row| {
        find_button(row).is_some_and(|b| b.label().is_some_and(|l| l.contains("とても長い")))
    });
    assert!(long_label_shown, "長いラベルの行が出ていること");

    assert_eq!(
        column.width(),
        before,
        "スクロールしても列の幅が動かないこと"
    );
    window.close();
    Ok(())
}

/// Even in a table with many rows, sorting by clicking a heading still works.
///
/// Sorting is the app's job, so this checks that what is passed again with
/// `set_rows` after the notification is reflected in the rows on screen.
fn table_sorting_works_while_windowed(ui: &Ui) -> Result<()> {
    const ROWS: usize = 10_000;
    let table = ui.table()?;
    table.set_columns(&[
        TableColumn::new("番号").sortable(true),
        TableColumn::new("名前"),
    ]);
    let rows: Vec<TableRow> = (0..ROWS)
        .map(|i| TableRow::new([i.to_string(), format!("項目 {i}")]))
        .collect();
    table.set_rows(&rows);

    let (log, sink) = recorder::<(usize, bool)>();
    let sink = Rc::new(RefCell::new(sink));
    table.on_sort({
        let table = table.clone();
        let rows = rows.clone();
        move |column, order| {
            (sink.borrow_mut())((column, order.is_ascending()));
            let sorted: Vec<TableRow> = match order.is_ascending() {
                true => rows.clone(),
                false => rows.iter().rev().cloned().collect(),
            };
            table.set_rows(&sorted);
        }
    });

    let list = table.native_list_box();
    let first_cell = || table_cells(&children(&list)[0])[0].clone();
    assert_eq!(first_cell(), "0");

    // 見出しのボタンを押す (利用者の操作と同じ経路)。
    let (header, _) = table_parts(&table);
    let button = children(&header)
        .into_iter()
        .find_map(|child| child.downcast::<gtk::Button>().ok())
        .expect("押せる見出し");
    button.emit_clicked();
    assert_eq!(*log.borrow(), vec![(0, true)]);
    button.emit_clicked();
    assert_eq!(*log.borrow(), vec![(0, true), (0, false)]);
    assert_eq!(
        first_cell(),
        (ROWS - 1).to_string(),
        "並べ替えた結果が画面の行に出ること"
    );

    // 向きの指標は見出しの文字に付く。
    let title = table_labels(&button).first().map(|l| l.text().to_string());
    assert_eq!(title.as_deref(), Some("番号 ▼"));
    Ok(())
}

/// In a table with many rows, `GtkListBoxRow`s are created only for the rows on
/// screen. The scrollable height still covers all rows, and scrolling swaps the
/// contents.
fn table_builds_only_the_visible_rows(ui: &Ui) -> Result<()> {
    const ROWS: usize = 20_000;
    let table = ui.table()?;
    table.set_columns(&TableColumn::list(["番号", "名前"]));
    table.set_sizing(Sizing::fill());

    let window = ui.window("大きな表", 400.0, 200.0)?;
    window.set_child(&table);
    window.show();

    let started = std::time::Instant::now();
    table.set_rows(
        &(0..ROWS)
            .map(|i| TableRow::new([i.to_string(), format!("行 {i}")]))
            .collect::<Vec<_>>(),
    );
    assert!(
        started.elapsed() < std::time::Duration::from_secs(2),
        "{ROWS} 行でも待たされないこと: {:?}",
        started.elapsed()
    );
    tick(&table.native_list_box());
    assert_eq!(table.len(), ROWS);

    let list = table.native_list_box();
    let realized = children(&list).len();
    assert!(
        realized > 0 && realized < 200,
        "見えている分だけを作ること: {realized} 行"
    );
    assert_eq!(table_cells(&children(&list)[0]), ["0", "行 0"]);

    // 詰め物のぶん、中身の高さは全行分ある。
    let height = table.row_height();
    assert!(height > 0.0, "行の高さを測れていること: {height}");
    let scroller = scroller_of(&list).expect("GtkScrolledWindow");
    let adjustment = scroller.vadjustment();
    let expected = ROWS as f64 * height;
    assert!(
        (adjustment.upper() - expected).abs() < expected * 0.05,
        "全行分の高さがあること: {} / {expected}",
        adjustment.upper()
    );

    // 途中までスクロールすると、その辺りの行に入れ替わる。
    adjustment.set_value(10_000.0 * height);
    tick(&list);
    let rows = children(&list);
    assert!(rows.len() < 200, "作る量は増えないこと");
    let first = table_cells(&rows[0])[0].parse::<usize>().expect("番号");
    assert!(
        (9_900..=10_000).contains(&first),
        "スクロール先の行を作ること: {first} 行目から"
    );

    // 窓の外の行を選んでも覚えている。
    table.set_selected(19_999);
    assert_eq!(table.selection(), vec![19_999]);
    adjustment.set_value(adjustment.upper());
    tick(&list);
    assert_eq!(
        table.selection(),
        vec![19_999],
        "戻ってきても選択は残ること"
    );
    let selected: Vec<i32> = list.selected_rows().iter().map(|row| row.index()).collect();
    assert_eq!(selected.len(), 1, "見えている行としても選ばれること");
    window.close();
    Ok(())
}

/// Widgets can be placed in cells. Clicking a row becomes an activation, and for
/// unselectable rows only the selection does not happen.
fn table_widget_cells_are_native(ui: &Ui) -> Result<()> {
    let (log, sink) = recorder::<usize>();
    let pressed = Rc::new(Cell::new(0));

    let table = ui.table()?;
    table.set_columns(&TableColumn::list(["都市", "操作"]));
    {
        let ui = ui.clone();
        let sink = Rc::new(RefCell::new(sink));
        let pressed = pressed.clone();
        table.set_row_builder(3, move |index| {
            let button = ui.button("開く")?;
            button.on_click({
                let pressed = pressed.clone();
                move || pressed.set(pressed.get() + 1)
            });
            let cells = TableCells::new()
                .text(format!("都市 {index}"))
                .cell(&button)
                .selectable(index != 1);
            cells.on_activate({
                let sink = sink.clone();
                move || (sink.borrow_mut())(index)
            });
            Ok(cells)
        });
    }

    let list = table.native_list_box();
    let rows = children(&list);
    assert_eq!(rows.len(), 3);
    // 2 列目はボタンそのものが入る。
    let button = rows[0]
        .clone()
        .downcast::<gtk::ListBoxRow>()
        .expect("GtkListBoxRow")
        .child()
        .map(|content| children(&content))
        .unwrap_or_default()
        .into_iter()
        .find_map(|cell| find_button(&cell))
        .expect("セルの中がボタンであること");
    button.emit_clicked();
    assert_eq!(pressed.get(), 1, "セルのボタンがそのまま押せること");
    assert!(log.borrow().is_empty(), "行の activation は起きないこと");

    // 行そのものを押すと activation が出る。
    let row = rows[0]
        .clone()
        .downcast::<gtk::ListBoxRow>()
        .expect("GtkListBoxRow");
    list.emit_by_name::<()>("row-activated", &[&row]);
    assert_eq!(*log.borrow(), vec![0]);

    // 選べない行は、選択の対象から外れる。
    table.set_selection(&[0, 1]);
    assert_eq!(table.selection(), vec![0]);
    Ok(())
}

/// Finds the `GtkScrolledWindow` holding the widget by walking up its parents.
fn scroller_of(widget: &impl IsA<gtk::Widget>) -> Option<gtk::ScrolledWindow> {
    let mut parent = widget.as_ref().parent();
    while let Some(current) = parent {
        if let Ok(scroller) = current.clone().downcast::<gtk::ScrolledWindow>() {
            return Some(scroller);
        }
        parent = current.parent();
    }
    None
}

/// Finds a button inside the table.
fn find_button(widget: &gtk::Widget) -> Option<gtk::Button> {
    if let Ok(button) = widget.clone().downcast::<gtk::Button>() {
        return Some(button);
    }
    children(widget)
        .into_iter()
        .find_map(|child| find_button(&child))
}

/// Fixing the row height redraws the spacers at that height too.
fn table_row_height_can_be_fixed(ui: &Ui) -> Result<()> {
    let table = ui.table()?;
    table.set_columns(&TableColumn::list(["番号"]));
    table.set_rows(
        &(0..1_000)
            .map(|i| TableRow::new([i.to_string()]))
            .collect::<Vec<_>>(),
    );
    table.set_row_height(40.0);
    assert_eq!(table.row_height(), 40.0);

    let list = table.native_list_box();
    let row = children(&list)[0].clone();
    let (_, natural, _, _) = row.measure(gtk::Orientation::Vertical, -1);
    assert_eq!(natural, 40, "指定した高さになること");

    table.set_row_height(0.0);
    assert!(
        table.row_height() > 0.0,
        "0 以下は測った高さへ戻すこと: {}",
        table.row_height()
    );
    Ok(())
}

/// The text of the cells in one row.
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

/// Keyboard focus moving into a `List`, `Table` or `Tree` (the first focus the
/// window gives on showing, or Shift+Tab) focuses a row without selecting it, so
/// nothing is notified. `GtkListBoxRow` itself selects the row it focuses; naui's
/// rows leave that to clicks and arrow keys, which still select.
fn focus_entering_rows_does_not_select(ui: &Ui) -> Result<()> {
    let list = ui.list()?;
    list.set_items(&ListItem::list(["札幌", "東京", "那覇"]));
    let table = ui.table()?;
    table.set_columns(&[TableColumn::new("番号"), TableColumn::new("名前")]);
    table.set_rows(&[TableRow::new(["1", "札幌"]), TableRow::new(["2", "東京"])]);
    let tree = ui.tree()?;
    tree.set_items(&[TreeItem::new("main.rs"), TreeItem::new("lib.rs")]);

    let log = Rc::new(RefCell::new(Vec::<Vec<usize>>::new()));
    let notify = || {
        let log = log.clone();
        move |selection: &[usize]| log.borrow_mut().push(selection.to_vec())
    };
    list.on_select(notify());
    table.on_select(notify());
    tree.on_select(notify());

    let check =
        |name: &str, widget: &dyn Fn(&naui_gtk::Stack), native: gtk::ListBox| -> Result<()> {
            log.borrow_mut().clear();
            let window = ui.window(name, 320.0, 240.0)?;
            let stack = ui.stack(Orientation::Vertical)?;
            widget(&stack);
            let after = ui.button("後ろ")?;
            stack.append(&after);
            window.set_child(&stack);
            window.show();
            spin_until(std::time::Duration::from_millis(300), || false);
            assert!(
                native.focus_child().is_some(),
                "{name}: 最初のフォーカスが行に入る"
            );
            assert!(
                native.selected_row().is_none(),
                "{name}: 最初のフォーカスでは選ばない"
            );

            after.native_widget().grab_focus();
            window
                .native_window()
                .child_focus(gtk::DirectionType::TabBackward);
            assert!(
                native.focus_child().is_some(),
                "{name}: Shift+Tab で行に入る"
            );
            assert!(
                native.selected_row().is_none(),
                "{name}: Shift+Tab でも選ばない"
            );
            assert!(
                log.borrow().is_empty(),
                "{name}: 通知しない: {:?}",
                log.borrow()
            );

            // 矢印キー (`move-cursor`) では今までどおり選ぶ。
            native.emit_by_name::<()>(
                "move-cursor",
                &[&gtk::MovementStep::DisplayLines, &-1i32, &false, &false],
            );
            assert!(native.selected_row().is_some(), "{name}: 矢印キーで選ぶ");
            assert_eq!(log.borrow().len(), 1, "{name}: 矢印キーの選択は通知する");
            window.close();
            Ok(())
        };
    check("一覧", &|stack| stack.append(&list), list_box_of(&list))?;
    check("表", &|stack| stack.append(&table), table.native_list_box())?;
    check("ツリー", &|stack| stack.append(&tree), tree_box_of(&tree))?;
    Ok(())
}

// --------------------------------------------------------------------- ツリー

fn tree_box_of(tree: &naui_gtk::Tree) -> gtk::ListBox {
    tree.native_widget().downcast().expect("GtkListBox")
}

/// Tree rows also use everything right of the expander button as the text area.
///
/// `SizeBin`'s default alignment fits the content, so as is the text would not use
/// the rest of the row's width. Without a decided width, `PangoEllipsizeMode::End`
/// does not work either, and a long label pushes the row wider (the same reason as
/// `List`).
fn tree_row_content_fills_the_row(ui: &Ui) -> Result<()> {
    let tree = ui.tree()?;
    tree.set_sizing(Sizing::fill());
    tree.set_items(&[TreeItem::new("プロジェクト")
        .detail("3 ファイル")
        .expanded(true)
        .child(TreeItem::new("main.rs"))]);

    let window = ui.window("ツリー", 400.0, 200.0)?;
    window.set_child(&tree);
    window.show();
    let native = tree_box_of(&tree);
    // 配られた幅を測るので、フレームを 1 つ進めてから見る。
    tick(&native);

    let row = children(&native)
        .remove(0)
        .downcast::<gtk::ListBoxRow>()
        .expect("GtkListBoxRow");
    let line = row.child().expect("行の中身");
    assert!(line.width() > 0, "行に幅が配られていること");

    // 行は [開閉ボタン, 文字の縦並び] の順。文字の領域には、中身の自然な幅より
    // 広く配られる (開閉ボタンの右の余りを使う)。寄せ方が「中身の大きさに
    // 合わせる」ままだと、配られた幅と自然な幅が一致する。
    //
    // 残り幅そのものを計算しないのは、行の間隔や余白の取り方に縛られないため。
    let parts = children(&line);
    let content = parts.last().expect("文字の縦並び");
    let (_, natural) = measure_width(content);
    assert!(
        content.width() > natural,
        "文字の領域が行の残り幅を使っていない: 領域 {} / 中身 {natural} (行 {})",
        content.width(),
        line.width()
    );

    // 中の文字も、その領域に合わせて広がる。
    for (index, label) in labels_in(content).iter().enumerate() {
        let (_, natural) = measure_width(label);
        assert!(
            label.width() > natural,
            "{index} 本目の文字が領域の幅を使っていない: 文字 {} / 中身 {natural}",
            label.width()
        );
    }
    window.close();
    Ok(())
}

/// The tree used in tests: src (two leaves) and docs (guide > intro.md).
fn sample_tree() -> Vec<TreeItem> {
    vec![
        TreeItem::new("src").children([TreeItem::new("main.rs"), TreeItem::new("lib.rs")]),
        TreeItem::new("docs").child(TreeItem::new("guide").child(TreeItem::new("intro.md"))),
    ]
}

/// Returns the text of the currently visible rows, from the top.
///
/// Rows are built in advance for every item, and those inside collapsed branches are
/// hidden with `set_visible(false)`.
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
            Some(labels_in(&content).first()?.text().to_string())
        })
        .collect()
}

/// The expander button placed in a row. Leaf rows have none.
///
/// `index` is the position when **every item** is laid out depth-first (invisible
/// rows count too).
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

/// A shown window with two buttons to open popovers from.
fn popover_window(ui: &Ui) -> Result<(naui_gtk::Window, naui_gtk::Button, naui_gtk::Button)> {
    let window = ui.window("ポップオーバー", 360.0, 240.0)?;
    let root = ui.stack(Orientation::Horizontal)?;
    let first = ui.button("設定")?;
    let second = ui.button("表示")?;
    root.append(&first);
    root.append(&second);
    window.set_child(&root);
    window.show();
    tick(&first.native_widget());
    Ok((window, first, second))
}

/// Counts `on_close` calls.
fn count_closes(popover: &naui_gtk::Popover) -> Rc<Cell<usize>> {
    let closed = Rc::new(Cell::new(0));
    popover.on_close({
        let closed = closed.clone();
        move || closed.set(closed.get() + 1)
    });
    closed
}

fn popover_shows_its_content_beside_the_widget(ui: &Ui) -> Result<()> {
    let (window, button, _) = popover_window(ui)?;
    let popover = ui.popover()?;
    let body = ui.label("中身")?;
    popover.set_child(&body);
    let closed = count_closes(&popover);
    assert!(!popover.is_open(), "作っただけでは閉じていること");

    popover.open(&button);
    assert!(popover.is_open());
    let native = popover.native_popover();
    assert!(native.is_visible(), "GtkPopover が出ていること");
    assert_eq!(
        native.parent(),
        button.native_widget().parent(),
        "押したウィジェットを包む SizeBin に付くこと"
    );
    assert!(
        body.native_widget().is_ancestor(&native),
        "中身がポップオーバーの中に入ること"
    );

    popover.close();
    assert!(!popover.is_open());
    assert!(!native.is_visible());
    pump();
    assert_eq!(closed.get(), 0, "アプリが閉じたときは通知しないこと");
    assert!(
        native.parent().is_none(),
        "閉じたらウィジェットから外れること"
    );

    // 外れたあとも開き直せること。
    popover.open(&button);
    assert!(popover.is_open());
    popover.close();
    window.close();
    Ok(())
}

fn popover_dismissed_by_the_user_notifies(ui: &Ui) -> Result<()> {
    let (window, button, _) = popover_window(ui)?;
    let popover = ui.popover()?;
    popover.set_child(&ui.label("中身")?);
    let closed = count_closes(&popover);

    popover.open(&button);
    popover.dismiss();
    pump();
    assert_eq!(closed.get(), 1, "利用者が閉じたら 1 度だけ通知すること");
    assert!(!popover.is_open());
    window.close();
    Ok(())
}

fn popover_position_reaches_the_native(ui: &Ui) -> Result<()> {
    let popover = ui.popover()?;
    assert_eq!(popover.position(), naui_core::PopoverPosition::Bottom);
    assert_eq!(
        popover.native_popover().position(),
        gtk::PositionType::Bottom
    );
    for (position, native) in [
        (naui_core::PopoverPosition::Top, gtk::PositionType::Top),
        (naui_core::PopoverPosition::Left, gtk::PositionType::Left),
        (naui_core::PopoverPosition::Right, gtk::PositionType::Right),
        (
            naui_core::PopoverPosition::Bottom,
            gtk::PositionType::Bottom,
        ),
    ] {
        popover.set_position(position);
        assert_eq!(popover.position(), position);
        assert_eq!(popover.native_popover().position(), native);
    }
    Ok(())
}

fn popover_moves_between_widgets(ui: &Ui) -> Result<()> {
    let (window, first, second) = popover_window(ui)?;
    let popover = ui.popover()?;
    popover.set_child(&ui.label("中身")?);
    let closed = count_closes(&popover);

    popover.open(&first);
    popover.open(&second);
    pump();
    assert!(popover.is_open(), "移したあとも開いていること");
    let native = popover.native_popover();
    assert!(native.is_visible());
    assert_eq!(native.parent(), second.native_widget().parent());
    assert_eq!(closed.get(), 0, "移すだけなら通知しないこと");
    popover.close();
    window.close();
    Ok(())
}

fn popover_follows_its_content_on_a_scroll(ui: &Ui) -> Result<()> {
    let window = ui.window("ポップオーバー", 360.0, 240.0)?;
    let scroll = ui.scroll()?;
    scroll.set_child(&ui.label("本文")?);
    window.set_child(&scroll);
    window.show();
    tick(&scroll.native_widget());

    let popover = ui.popover()?;
    let body = ui.label("中身")?;
    popover.set_child(&body);
    popover.open(&scroll);
    let native = popover.native_popover();
    tick(&native);
    tick(&native);
    let (width, height) = (native.width(), native.height());
    assert!(width > 0 && height > 0, "開いた時点で大きさがあること");

    // GtkScrolledWindow は子のポップオーバーを置き直さないので、付け先が
    // 割り当てを受け持たないと、中身が伸びたときに 0×0 へ潰れる。
    body.set_text("ずっと長くなった中身\n2 行目\n3 行目");
    tick(&native);
    tick(&scroll.native_widget());
    tick(&native);
    assert!(
        native.width() > width && native.height() > height,
        "中身に合わせて伸びること ({width}×{height} → {}×{})",
        native.width(),
        native.height()
    );
    popover.close();
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
    assert!(popover.native_popover().parent().is_none());
    Ok(())
}

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

    let (log, sink) = recorder::<Vec<usize>>();
    let mut sink = sink;
    popup.on_select(move |path| sink(path.to_vec()));

    // GTK4 のメニューは操作 (GAction) で動く。項目を選んだのと同じ経路。
    popup
        .native_popover()
        .activate_action("naui-popup.item1", None)
        .expect("メニューの操作");
    assert_eq!(log.borrow().as_slice(), [vec![1]]);
    Ok(())
}

fn popup_skips_separator_and_disabled(ui: &Ui) -> Result<()> {
    let popup = ui.popup_menu()?;
    popup.set_items(&[
        PopupItem::new("コピー"),
        PopupItem::separator(),
        PopupItem::new("削除").enabled(false),
    ]);
    let (log, sink) = recorder::<Vec<usize>>();
    let mut sink = sink;
    popup.on_select(move |path| sink(path.to_vec()));

    popup.select(&[1]);
    popup.select(&[2]);
    popup.select(&[9]);
    popup.select(&[]);
    assert!(
        log.borrow().is_empty(),
        "区切り線と選べない項目は通知しない"
    );

    popup.select(&[0]);
    assert_eq!(log.borrow().as_slice(), [vec![0]]);
    Ok(())
}

/// A submenu becomes a submenu link with a `submenu-action`, and a check item an
/// action with a boolean state that flips before the notification.
fn popup_submenus_and_checks(ui: &Ui) -> Result<()> {
    let popup = ui.popup_menu()?;
    popup.set_items(&[
        PopupItem::new("折り返す").checked(true),
        PopupItem::submenu(
            "並べ替え",
            [
                PopupItem::new("名前"),
                PopupItem::new("日付").checked(false),
            ],
        ),
        PopupItem::submenu("閉じた", ["中"]).enabled(false),
    ]);
    assert_eq!(popup.len(), 3);

    let wrap = popup.native_action(&[0]).expect("操作");
    assert_eq!(wrap.state().and_then(|v| v.get::<bool>()), Some(true));
    assert!(popup
        .native_action(&[1, 0])
        .expect("操作")
        .state()
        .is_none());
    assert!(!popup
        .native_action(&[2])
        .expect("submenu-action")
        .is_enabled());
    assert!(
        !popup.native_action(&[2, 0]).expect("操作").is_enabled(),
        "押せないサブメニューの中も止める"
    );

    // 節の中の 2 番目がサブメニューへのリンクと submenu-action を持つ。
    let model = popup.native_popover().menu_model().expect("GMenu");
    let section = model
        .item_link(0, gio::MENU_LINK_SECTION)
        .expect("最初の節");
    assert!(section.item_link(1, gio::MENU_LINK_SUBMENU).is_some());
    let action = section
        .item_attribute_value(1, "submenu-action", None)
        .and_then(|v| v.get::<String>());
    assert_eq!(action.as_deref(), Some("naui-popup.item1"));

    let seen = Rc::new(RefCell::new(Vec::new()));
    popup.on_select({
        let seen = seen.clone();
        let popup = popup.clone();
        move |path| {
            seen.borrow_mut()
                .push((path.to_vec(), popup.is_item_checked(path)))
        }
    });
    popup
        .native_popover()
        .activate_action("naui-popup.item1-1", None)
        .expect("メニューの操作");
    popup.select(&[0]);
    popup.select(&[1]);
    popup.select(&[2, 0]);
    assert_eq!(
        *seen.borrow(),
        vec![(vec![1, 1], true), (vec![0], false)],
        "印は通知の前に反転し、見出しと押せない中身は選べない"
    );
    assert_eq!(wrap.state().and_then(|v| v.get::<bool>()), Some(false));

    popup.set_item_checked(&[0], true);
    assert!(popup.is_item_checked(&[0]));
    assert_eq!(wrap.state().and_then(|v| v.get::<bool>()), Some(true));
    assert_eq!(seen.borrow().len(), 2, "set_item_checked は通知しない");
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

/// A `Fill` image asks for no natural size on that axis and fits into the leftover
/// space; an `Auto` axis still asks for the pixel size, and a maximum still counts.
///
/// A vertical `Scroll` places its content at the natural size, so asking for the
/// pixel size, a large image pushed the content to the image's height.
fn fill_image_does_not_ask_for_its_size(ui: &Ui) -> Result<()> {
    let image = ui.image("")?;
    let native: gtk::Picture = image.native_widget().downcast().expect("GtkPicture");
    let (width, height) = (1500, 1000);
    let pixels = glib::Bytes::from_owned(vec![0u8; (width * height * 3) as usize]);
    let texture = gtk::gdk::MemoryTexture::new(
        width,
        height,
        gtk::gdk::MemoryFormat::R8g8b8,
        &pixels,
        (width * 3) as usize,
    );
    native.set_paintable(Some(&texture));
    let bin = bin_of(&image);
    assert_eq!(measure_height(&bin).1, height, "Auto は画素の高さを求める");

    image.set_sizing(Sizing::fill());
    assert_eq!(measure_width(&bin), (0, 0), "Fill の幅は大きさを求めない");
    assert_eq!(
        measure_height(&bin),
        (0, 0),
        "Fill の高さは大きさを求めない"
    );

    image.set_sizing(Sizing::fill_width());
    assert_eq!(
        measure_height(&bin).1,
        height,
        "Auto の高さは画素の高さを求める"
    );

    image.set_sizing(Sizing::fill().max_height(315.0));
    assert_eq!(measure_height(&bin).1, 315, "上限は自然な大きさを兼ねる");
    Ok(())
}

fn fill_video_does_not_ask_for_its_size(ui: &Ui) -> Result<()> {
    let video = ui.video("")?;
    let native: gtk::Box = video.native_widget().downcast().expect("GtkBox");
    let picture: gtk::Picture = native
        .first_child()
        .and_then(|child| child.downcast().ok())
        .expect("GtkPicture");
    // 開いた動画の代わりに、同じ縦横比の絵を映像面へ置く。
    let (width, height) = (1600, 900);
    let pixels = glib::Bytes::from_owned(vec![0u8; (width * height * 3) as usize]);
    let texture = gtk::gdk::MemoryTexture::new(
        width,
        height,
        gtk::gdk::MemoryFormat::R8g8b8,
        &pixels,
        (width * 3) as usize,
    );
    picture.set_paintable(Some(&texture));
    let bin = bin_of(&video);
    assert!(
        measure_height(&bin).1 >= height,
        "Auto は映像の高さを求める"
    );

    video.set_sizing(Sizing::fill());
    assert_eq!(
        measure_height(&bin),
        (0, 0),
        "Fill の高さは大きさを求めない"
    );

    video.set_sizing(Sizing::fill().max_height(315.0));
    assert_eq!(measure_height(&bin).1, 315, "上限は自然な大きさを兼ねる");
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

/// Saving is set up as a button and keeps its settings.
///
/// `GtkFileDialog`'s save cannot be inspected until shown, so this checks **up to
/// the real button and that the settings are kept**.
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

/// The toast's settings reach the `AdwToast`, and dismissing removes it.
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

/// The action button and dismissal each reach their closures.
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

/// A new toast replaces the previous one, and the replaced one does not notify.
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

/// libadwaita 1.5 opens the sheet inside `AdwDialog` on its second frame after it is
/// mapped and ignores closing before then, so this checks that naui closes it once
/// it opens. 1.6 and later close it on the first try.
fn dialog_closed_right_after_opening_does_not_stay(ui: &Ui) -> Result<()> {
    let window = ui.window("naui", 320.0, 200.0)?;
    window.show();

    let dialog = ui.dialog("すぐ閉じる")?;
    dialog.set_buttons(DialogButtons::new().primary("消す").cancel("やめる"));
    let (log, sink) = recorder::<DialogResponse>();
    dialog.on_response(sink);
    // 同じイベント処理の中で開いて閉じる。
    dialog.open();
    let native = dialog.native_dialog().expect("AdwAlertDialog");
    dialog.close();
    assert!(!dialog.is_open());
    assert_eq!(log.borrow().as_slice(), [DialogResponse::Cancel]);
    // 閉じ終えると AdwAlertDialog は窓から外れる。1.5 が開く 2 フレーム目より十分に待つ。
    let gone = || native.parent().is_none();
    spin_until(std::time::Duration::from_secs(2), gone);
    assert!(gone(), "開く前に閉じたダイアログが窓に残らない");
    assert_eq!(log.borrow().len(), 1, "取り消しは 1 回だけ届く");

    // 閉じたあとも、開き直して使える。
    dialog.open();
    let reopened = dialog.native_dialog().expect("開き直した AdwAlertDialog");
    reopened.emit_by_name::<()>("response", &[&"primary"]);
    assert_eq!(
        log.borrow().as_slice(),
        [DialogResponse::Cancel, DialogResponse::Primary]
    );
    window.close();
    Ok(())
}

/// On libadwaita 1.5 the first dialog closes a few frames later, after the second
/// one is open. Its late response must not be taken for the second one's.
fn dialog_reopened_in_the_same_turn_ignores_the_old_response(ui: &Ui) -> Result<()> {
    let window = ui.window("naui", 320.0, 200.0)?;
    window.show();

    let dialog = ui.dialog("開き直す")?;
    dialog.set_buttons(DialogButtons::new().primary("消す").cancel("やめる"));
    let (log, sink) = recorder::<DialogResponse>();
    dialog.on_response(sink);
    // 同じイベント処理の中で開き、閉じ、開き直す。
    dialog.open();
    let first = dialog.native_dialog().expect("AdwAlertDialog");
    dialog.close();
    dialog.open();
    let second = dialog.native_dialog().expect("開き直した AdwAlertDialog");
    assert_ne!(first, second);
    assert_eq!(log.borrow().as_slice(), [DialogResponse::Cancel]);

    let gone = || first.parent().is_none();
    spin_until(std::time::Duration::from_secs(2), gone);
    assert!(gone(), "前のダイアログは窓に残らない");
    assert_eq!(
        log.borrow().as_slice(),
        [DialogResponse::Cancel],
        "前のダイアログが遅れて閉じても取り消しは重ならない"
    );
    assert!(dialog.is_open(), "開き直したダイアログは開いたまま");
    assert_eq!(dialog.native_dialog().as_ref(), Some(&second));

    // 開き直したダイアログの応答は届く。
    second.emit_by_name::<()>("response", &[&"primary"]);
    assert_eq!(
        log.borrow().as_slice(),
        [DialogResponse::Cancel, DialogResponse::Primary]
    );
    assert!(!dialog.is_open());
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

/// The `GtkButton`s in `widget` and below it, in tree order.
fn buttons_in(widget: &gtk::Widget) -> Vec<gtk::Button> {
    let mut found = Vec::new();
    let mut child = widget.first_child();
    while let Some(current) = child {
        match current.clone().downcast::<gtk::Button>() {
            Ok(button) => found.push(button),
            Err(other) => found.extend(buttons_in(&other)),
        }
        child = current.next_sibling();
    }
    found
}

/// The sheet's buttons with their labels.
fn sheet_buttons(sheet: &naui_gtk::Sheet) -> Vec<(String, gtk::Button)> {
    let native = sheet.native_dialog().expect("AdwDialog");
    buttons_in(native.upcast_ref())
        .into_iter()
        .filter_map(|button| button.label().map(|label| (label.to_string(), button)))
        .collect()
}

/// Waits until the sheet has opened, as the user sees it.
///
/// libadwaita 1.5 opens the sheet inside `AdwDialog` on its second frame after it is
/// mapped, and closing it before then does nothing, so `closed` never comes (1.6
/// and later close it anyway). The user can press Esc only once it is shown, so wait
/// for the focus it takes when it opens.
fn wait_until_shown(native: &adw::Dialog) {
    let shown = || {
        native
            .root()
            .and_then(|root| root.focus())
            .is_some_and(|focus| focus.is_ancestor(native))
    };
    spin_until(std::time::Duration::from_secs(2), shown);
    assert!(shown(), "シートが開ききる");
}

fn sheet_builds_an_adw_dialog(ui: &Ui) -> Result<()> {
    let window = ui.window("naui", 480.0, 360.0)?;
    window.show();

    let sheet = ui.sheet("新しいフォルダ")?;
    let field = ui.text_input("名称未設定")?;
    sheet.set_child(&field);
    sheet.set_buttons(
        DialogButtons::new()
            .primary("作成")
            .secondary("あとで")
            .cancel("キャンセル"),
    );
    let (log, sink) = recorder::<DialogResponse>();
    sheet.on_response(sink);
    assert!(!sheet.is_open());
    assert!(sheet.native_dialog().is_none());

    sheet.open();
    assert!(sheet.is_open());
    let native = sheet.native_dialog().expect("AdwDialog");
    assert_eq!(native.title().to_string(), "新しいフォルダ");
    assert!(native.follows_content_size(), "中身の大きさに従う");
    assert!(
        field.native_widget().is_ancestor(&native),
        "中身がダイアログの中に入る"
    );

    let buttons = sheet_buttons(&sheet);
    let header = buttons[0]
        .1
        .ancestor(adw::HeaderBar::static_type())
        .and_downcast::<adw::HeaderBar>()
        .expect("ボタンが見出しの帯にある");
    assert!(
        !header.shows_end_title_buttons(),
        "取り消しがあれば閉じるボタンは出さない"
    );
    let labels: Vec<_> = buttons.iter().map(|(label, _)| label.as_str()).collect();
    // 帯の中では、始端に取り消し、終端から主・副の順に詰める。
    assert_eq!(labels, ["キャンセル", "あとで", "作成"]);
    let primary = &buttons[2].1;
    assert!(
        primary.has_css_class("suggested-action"),
        "主ボタンの見た目"
    );
    assert!(!buttons[1].1.has_css_class("suggested-action"));
    assert_eq!(
        native.default_widget().as_ref(),
        Some(primary.upcast_ref::<gtk::Widget>()),
        "主ボタンが既定のボタン"
    );

    sheet.close();
    assert!(!sheet.is_open());
    assert!(sheet.native_dialog().is_none());
    assert!(log.borrow().is_empty(), "アプリが閉じたときは通知しない");

    // 取り消しが無ければ、閉じるボタンを出す。
    sheet.set_buttons(DialogButtons::new().primary("作成"));
    sheet.open();
    let header = sheet_buttons(&sheet)[0]
        .1
        .ancestor(adw::HeaderBar::static_type())
        .and_downcast::<adw::HeaderBar>()
        .expect("見出しの帯");
    assert!(header.shows_end_title_buttons());
    sheet.close();
    window.close();
    Ok(())
}

fn sheet_buttons_and_escape_respond(ui: &Ui) -> Result<()> {
    let window = ui.window("naui", 480.0, 360.0)?;
    window.show();

    let sheet = ui.sheet("確認")?;
    let body = ui.label("中身")?;
    sheet.set_child(&body);
    sheet.set_buttons(DialogButtons::new().primary("作成").cancel("やめる"));
    let log = Rc::new(RefCell::new(Vec::new()));
    let reopened = Rc::new(Cell::new(false));
    sheet.on_response({
        let log = log.clone();
        let reopened = reopened.clone();
        let sheet = sheet.clone();
        move |response| {
            log.borrow_mut().push(response);
            // 1 回目の通知の中で開き直す。
            if log.borrow().len() == 1 {
                sheet.open();
                reopened.set(sheet.is_open());
            }
        }
    });

    sheet.open();
    let first = sheet.native_dialog().expect("AdwDialog");
    wait_until_shown(&first);
    let (_, primary) = sheet_buttons(&sheet).pop().expect("主ボタン");
    primary.emit_clicked();
    assert_eq!(log.borrow().as_slice(), [DialogResponse::Primary]);
    assert!(reopened.get(), "通知の中で開き直せる");
    let second = sheet.native_dialog().expect("開き直した AdwDialog");
    assert_ne!(first, second, "開くたびに作り直す");
    pump();
    assert!(
        body.native_widget().is_ancestor(&second),
        "中身が新しいダイアログへ移る"
    );

    // Esc は AdwDialog が閉じる。取り消しとして届く。
    wait_until_shown(&second);
    second.close();
    pump();
    assert_eq!(
        log.borrow().as_slice(),
        [DialogResponse::Primary, DialogResponse::Cancel]
    );
    assert!(!sheet.is_open());
    // 閉じ終えたら中身を手放す。
    assert!(body.size_bin().parent().is_none(), "中身はどこにも属さない");

    sheet.on_response(|_| {});
    window.close();
    Ok(())
}

fn sheet_button_enabled_reaches_the_buttons(ui: &Ui) -> Result<()> {
    let window = ui.window("naui", 480.0, 360.0)?;
    window.show();

    let sheet = ui.sheet("名前")?;
    sheet.set_buttons(DialogButtons::new().primary("作成").cancel("やめる"));
    sheet.set_button_enabled(DialogResponse::Primary, false);
    sheet.set_button_enabled(DialogResponse::Cancel, false);
    assert!(!sheet.is_button_enabled(DialogResponse::Primary));
    assert!(sheet.is_button_enabled(DialogResponse::Cancel));
    sheet.open();
    let sensitive = |sheet: &naui_gtk::Sheet| -> Vec<(String, bool)> {
        sheet_buttons(sheet)
            .into_iter()
            .map(|(label, button)| (label, button.is_sensitive()))
            .collect()
    };
    assert_eq!(
        sensitive(&sheet),
        [("やめる".to_string(), true), ("作成".to_string(), false)]
    );
    sheet.set_button_enabled(DialogResponse::Primary, true);
    assert_eq!(
        sensitive(&sheet),
        [("やめる".to_string(), true), ("作成".to_string(), true)],
        "開いている間の切り替えも届く"
    );
    sheet.close();
    window.close();
    Ok(())
}

fn sheet_set_child_allows_changes_in_the_focus_notification(ui: &Ui) -> Result<()> {
    let window = ui.window("naui", 480.0, 360.0)?;
    window.show();

    let sheet = ui.sheet("名前")?;
    let field = ui.text_input("")?;
    let (log, sink) = recorder::<DialogResponse>();
    sheet.on_response(sink);
    let focus_field = |sheet: &naui_gtk::Sheet| {
        sheet.set_child(&field);
        sheet.open();
        wait_until_shown(&sheet.native_dialog().expect("AdwDialog"));
        assert!(field.request_focus(), "入力欄にフォーカスを移せる");
    };

    // 差し替えで外れたフォーカスの通知の中で閉じる。
    focus_field(&sheet);
    let lost = Rc::new(Cell::new(0));
    field.on_focus_change({
        let lost = lost.clone();
        let sheet = sheet.clone();
        move |has_focus| {
            if !has_focus {
                lost.set(lost.get() + 1);
                sheet.close();
            }
        }
    });
    let other = ui.label("差し替え")?;
    sheet.set_child(&other);
    assert_eq!(lost.get(), 1, "フォーカスが外れた通知が届く");
    assert!(!sheet.is_open(), "通知の中で閉じられる");
    assert!(log.borrow().is_empty(), "close では応答は届かない");
    assert!(
        other.size_bin().parent().is_none(),
        "閉じたシートには置かない"
    );

    // 通知の中でさらに差し替えれば、そちらが残る。
    focus_field(&sheet);
    let third = ui.label("通知の中で差し替え")?;
    field.on_focus_change({
        let sheet = sheet.clone();
        let third = third.clone();
        move |has_focus| {
            if !has_focus {
                sheet.set_child(&third);
            }
        }
    });
    sheet.set_child(&other);
    let native = sheet.native_dialog().expect("開いたまま");
    assert!(
        third.native_widget().is_ancestor(&native),
        "通知の中の中身が残る"
    );
    assert!(other.size_bin().parent().is_none(), "外側の中身は置かない");

    field.on_focus_change(|_| {});
    sheet.close();
    window.close();
    Ok(())
}

/// libadwaita 1.5 ignores closing before the sheet has opened (see
/// `wait_until_shown`), so this checks that naui closes it once it opens.
/// On libadwaita 1.5 the first sheet closes a few frames later, after the second one
/// is open. Its late `closed` must not close the second one or take its content.
fn sheet_reopened_in_the_same_turn_ignores_the_old_close(ui: &Ui) -> Result<()> {
    let window = ui.window("naui", 480.0, 360.0)?;
    window.show();

    let sheet = ui.sheet("開き直す")?;
    let body = ui.label("中身")?;
    sheet.set_child(&body);
    sheet.set_buttons(DialogButtons::new().primary("作成").cancel("やめる"));
    let (log, sink) = recorder::<DialogResponse>();
    sheet.on_response(sink);
    // 同じイベント処理の中で開き、閉じ、開き直す。
    sheet.open();
    let first = sheet.native_dialog().expect("AdwDialog");
    sheet.close();
    sheet.open();
    let second = sheet.native_dialog().expect("開き直した AdwDialog");
    assert_ne!(first, second);

    let gone = || first.parent().is_none();
    spin_until(std::time::Duration::from_secs(2), gone);
    assert!(gone(), "前のシートは窓に残らない");
    assert!(
        log.borrow().is_empty(),
        "前のシートが遅れて閉じても通知しない"
    );
    assert!(sheet.is_open(), "開き直したシートは開いたまま");
    assert_eq!(sheet.native_dialog().as_ref(), Some(&second));
    assert!(
        body.native_widget().is_ancestor(&second),
        "中身は開き直したシートに残る"
    );

    // 開き直したシートは Esc で取り消しとして閉じる。
    wait_until_shown(&second);
    second.close();
    spin_until(std::time::Duration::from_secs(2), || !sheet.is_open());
    assert_eq!(log.borrow().as_slice(), [DialogResponse::Cancel]);
    window.close();
    Ok(())
}

fn sheet_closed_right_after_opening_does_not_stay(ui: &Ui) -> Result<()> {
    let window = ui.window("naui", 480.0, 360.0)?;
    window.show();

    let sheet = ui.sheet("すぐ閉じる")?;
    sheet.set_buttons(DialogButtons::new().primary("作成").cancel("やめる"));
    let (log, sink) = recorder::<DialogResponse>();
    sheet.on_response(sink);
    // 同じイベント処理の中で開いて閉じる。
    sheet.open();
    let native = sheet.native_dialog().expect("AdwDialog");
    sheet.close();
    assert!(!sheet.is_open());
    // 閉じ終えると AdwDialog は窓から外れる。1.5 が開く 2 フレーム目より十分に待つ。
    let gone = || native.parent().is_none();
    spin_until(std::time::Duration::from_secs(2), gone);
    assert!(gone(), "開く前に閉じたシートが窓に残らない");
    assert!(log.borrow().is_empty(), "close では応答は届かない");

    // 閉じたあとも、開き直して使える。
    sheet.open();
    let reopened = sheet.native_dialog().expect("開き直した AdwDialog");
    wait_until_shown(&reopened);
    reopened.close();
    spin_until(std::time::Duration::from_secs(2), || !sheet.is_open());
    assert_eq!(log.borrow().as_slice(), [DialogResponse::Cancel]);
    window.close();
    Ok(())
}

/// The preferred languages are glib's language names in BCP 47 (`ja_JP.UTF-8`
/// becomes `ja-JP`), with none under the C locale, and naui's own strings come from
/// the same list. There is no notification of a change on Linux.
fn preferred_languages_come_from_the_environment(ui: &Ui) -> Result<()> {
    let locale = ["LC_ALL", "LC_MESSAGES", "LANG"]
        .into_iter()
        .filter_map(|name| std::env::var(name).ok())
        .find(|value| !value.is_empty())
        .unwrap_or_default();
    let is_c = locale.is_empty() || locale == "POSIX" || locale.split('.').next() == Some("C");
    let expected = if is_c {
        Vec::new()
    } else {
        naui_core::language_tags_from_locale_names(glib::language_names().iter())
    };
    assert_eq!(ui.preferred_languages(), expected, "LANG={locale:?}");
    assert!(ui
        .preferred_languages()
        .iter()
        .all(|tag| !tag.contains(['_', '.', '@'])));
    assert_eq!(
        naui_core::ui_strings(),
        naui_core::UiStrings::for_languages(if is_c {
            vec!["C".to_string()]
        } else {
            expected
        })
    );
    ui.on_preferred_languages_change(|_| panic!("Linux では呼ばれない"));
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

/// The toolbar is placed in a `GtkBox` in the sequence including separators, and
/// separators have no button.
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

/// The toolbar goes into the header bar and disappears when detached.
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

/// Rebuilding the toolbar from inside the notification does not double-borrow.
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

/// A wrong name would turn just that item into the "missing image" picture.
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

/// The number of children laid out in a `GtkBox`.
fn children_of(container: &gtk::Box) -> Vec<gtk::Widget> {
    let mut children = Vec::new();
    let mut child = container.first_child();
    while let Some(widget) = child {
        child = widget.next_sibling();
        children.push(widget);
    }
    children
}

/// For each kind, only the controls it needs are laid out.
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

/// Both directions: writing the value, and operations on the native side.
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

/// A date-only display keeps the time, and a time-only display keeps the date.
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

/// It never goes outside the minimum and maximum.
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

/// The time picker is `GtkSpinButton`s for hours and minutes separated by `:`. The
/// value round-trips with the native side, and `set_value` does not notify.
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

/// It never goes outside the minimum and maximum. `GtkSpinButton` has no time
/// range, so the clamped result is written back to the spin buttons' display too.
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

/// Flushes idle sources. Does not start the main loop.
///
/// `run_for_test` does not enter the main loop, so to make posted work progress the
/// main context must be iterated by hand.
fn pump() {
    let context = glib::MainContext::default();
    // 入っている分だけ回す。取りこぼしと無限ループの両方を避けるため上限を付ける。
    for _ in 0..64 {
        if !context.iteration(false) {
            break;
        }
    }
}

/// Advances one frame, until CSS resolution and layout are done.
///
/// GTK4 does not decide point sizes the moment a style class is added; it resolves
/// them together in the frame clock's layout phase and then allots space. `pump`
/// only runs what is pending and does not advance frames, so use this **before
/// measuring CSS results or allotted sizes**.
///
/// The wait gives up after 2 seconds. Failing with the measured values makes the
/// reason clearer than hanging without progress.
fn tick(widget: &impl IsA<gtk::Widget>) {
    let Some(clock) = widget.as_ref().frame_clock() else {
        pump();
        return;
    };
    let laid_out = Rc::new(Cell::new(false));
    let handler = clock.connect_layout({
        let laid_out = laid_out.clone();
        move |_| laid_out.set(true)
    });
    clock.request_phase(gtk::gdk::FrameClockPhase::LAYOUT);
    let context = glib::MainContext::default();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while !laid_out.get() && std::time::Instant::now() < deadline {
        // 次のティックまでは回すものが無いので、空回りのあいだは少し休む。
        if !context.iteration(false) {
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }
    clock.disconnect(handler);
}

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

/// A value sent from another thread reaches a label via the main context.
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

/// Spawned work progresses on the main context and completes.
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
    pump();

    assert_eq!(label.text(), "待機中", "cancel した処理は走らないこと");
    Ok(())
}

/// Headings become submenus of the `GMenu`, and items become `GSimpleAction`s.
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

    // メニューモデルには見出しの数だけ submenu が並ぶ。
    let model = menu_bar
        .native_menu_bar()
        .menu_model()
        .expect("メニューモデル");
    assert_eq!(model.n_items(), 2);

    let first = menu_bar.native_action(&[0, 0]).expect("先頭は操作を持つ");
    assert_eq!(first.name(), "m0-0");
    assert!(first.is_enabled());
    assert!(
        menu_bar.native_action(&[0, 1]).is_none(),
        "区切り線に操作は無い"
    );
    assert!(!menu_bar
        .native_action(&[0, 2])
        .expect("3 番目は操作を持つ")
        .is_enabled());
    assert!(menu_bar.native_action(&[9, 0]).is_none(), "範囲外は None");
    assert!(menu_bar.native_action(&[0, 9]).is_none(), "範囲外は None");

    assert!(menu_bar.is_item_enabled(&[0, 0]));
    assert!(!menu_bar.is_item_enabled(&[0, 1]), "区切り線は押せない");
    assert!(!menu_bar.is_item_enabled(&[0, 2]));

    // 項目ごとの指定と全体の指定は AND を取る。
    menu_bar.set_item_enabled(&[0, 2], true);
    assert!(menu_bar.native_action(&[0, 2]).expect("操作").is_enabled());
    menu_bar.set_enabled(false);
    assert!(!menu_bar.is_item_enabled(&[0, 0]));
    assert!(
        !menu_bar.native_action(&[0, 0]).expect("操作").is_enabled(),
        "全体を無効にすると、アクセラレータも効かないように操作ごと落とす"
    );
    assert!(!menu_bar.native_menu_bar().is_sensitive());
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
    assert!(menu_bar.native_action(&[0, 0]).is_none());
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

    // GTK4 側の操作から届く実際の通知経路も確かめる。
    menu_bar
        .native_action(&[1, 0])
        .expect("操作")
        .activate(None);
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

/// A submenu becomes a submenu link whose `submenu-action` decides whether it opens,
/// and a check item an action with a boolean state.
fn menu_bar_submenus_and_checks_map_to_native(ui: &Ui) -> Result<()> {
    let menu_bar = ui.menu_bar()?;
    menu_bar.set_menus(&[MenuSpec::new(
        "表示",
        [
            MenuItem::new("サイドバー").checked(true),
            MenuItem::submenu(
                "拡大",
                [MenuItem::new("拡大"), MenuItem::submenu("深い", ["さらに"])],
            ),
        ],
    )]);

    let sidebar = menu_bar.native_action(&[0, 0]).expect("操作");
    assert_eq!(sidebar.state().and_then(|v| v.get::<bool>()), Some(true));
    let zoom = menu_bar.native_action(&[0, 1]).expect("submenu-action");
    assert!(zoom.is_enabled());
    assert_eq!(
        menu_bar
            .native_action_name(&[0, 1, 1, 0])
            .map(|n| n.ends_with(".m0-1-1-0")),
        Some(true)
    );

    // 見出しの下の節の 2 番目がサブメニューへのリンクになっている。
    let model = menu_bar
        .native_menu_bar()
        .menu_model()
        .expect("メニューモデル");
    let view = model.item_link(0, gio::MENU_LINK_SUBMENU).expect("見出し");
    let section = view.item_link(0, gio::MENU_LINK_SECTION).expect("節");
    assert!(section.item_link(1, gio::MENU_LINK_SUBMENU).is_some());
    let action = section
        .item_attribute_value(1, "submenu-action", None)
        .and_then(|v| v.get::<String>())
        .expect("submenu-action");
    assert!(action.ends_with(".m0-1"), "{action}");

    let seen = Rc::new(RefCell::new(Vec::new()));
    menu_bar.on_activate({
        let seen = seen.clone();
        let menu_bar = menu_bar.clone();
        move |path| {
            seen.borrow_mut()
                .push((path.to_vec(), menu_bar.is_item_checked(path)))
        }
    });
    // GTK4 から届くのと同じ経路。
    sidebar.activate(None);
    assert_eq!(
        *seen.borrow(),
        vec![(vec![0, 0], false)],
        "通知の前に反転する"
    );
    assert_eq!(sidebar.state().and_then(|v| v.get::<bool>()), Some(false));
    menu_bar.activate(&[0, 0]);
    menu_bar.activate(&[0, 1]);
    menu_bar.activate(&[0, 1, 1, 0]);
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
    assert_eq!(sidebar.state().and_then(|v| v.get::<bool>()), Some(false));
    assert_eq!(seen.borrow().len(), 3, "set_item_checked は通知しない");

    // サブメニューを止めると、開けなくなり、中の操作もアクセラレータごと止まる。
    menu_bar.set_item_enabled(&[0, 1], false);
    assert!(!zoom.is_enabled());
    assert!(!menu_bar
        .native_action(&[0, 1, 1, 0])
        .expect("操作")
        .is_enabled());
    assert!(!menu_bar.is_item_enabled(&[0, 1, 0]));
    menu_bar.set_item_enabled(&[0, 1], true);
    assert!(menu_bar
        .native_action(&[0, 1, 1, 0])
        .expect("操作")
        .is_enabled());
    Ok(())
}

/// Function keys are registered as GTK4 accelerators, with Ctrl only when asked.
/// Invalid shortcuts are not registered.
fn menu_bar_function_keys_become_accelerators(ui: &Ui) -> Result<()> {
    let window = ui.window("ファンクションキー", 400.0, 300.0)?;
    let app = window
        .native_window()
        .application()
        .expect("GtkApplication");
    let menu_bar = ui.menu_bar()?;
    menu_bar.set_menus(&[MenuSpec::new(
        "表示",
        [
            MenuItem::new("再読み込み").shortcut(MenuShortcut::function(5)),
            MenuItem::submenu(
                "開発",
                [MenuItem::new("調べる")
                    .shortcut(MenuShortcut::function(12).primary(true).shift(true))],
            ),
            MenuItem::new("記号").shortcut(MenuShortcut::new('/')),
        ],
    )]);
    window.set_menu_bar(&menu_bar);

    let name = |path: &[usize]| menu_bar.native_action_name(path).expect("操作の名前");
    assert_eq!(app.accels_for_action(&name(&[0, 0])), vec!["F5"]);
    assert_eq!(
        app.accels_for_action(&name(&[0, 1, 0])),
        // GTK4 は修飾キーを自分の順に並べ直して返す。
        vec!["<Shift><Control>F12"],
        "サブメニューの中の項目も登録する"
    );
    assert!(
        app.accels_for_action(&name(&[0, 2])).is_empty(),
        "記号のショートカットはどの環境でも登録しない"
    );

    window.clear_menu_bar();
    assert!(app.accels_for_action(&name(&[0, 1, 0])).is_empty());
    window.close();
    Ok(())
}

/// Once attached to a window, its action group can be looked up from the window.
///
/// Accelerators are invoked by searching the action groups reachable from the
/// focused widget. Being reachable from the window is exactly the condition for
/// shortcuts registered with the `GtkApplication` to work.
fn menu_bar_attaches_to_the_window(ui: &Ui) -> Result<()> {
    let window = ui.window("メニューバー", 400.0, 300.0)?;
    let native = window.native_window();
    let app = native.application().expect("GtkApplication");

    let menu_bar = ui.menu_bar()?;
    menu_bar.set_menus(&[MenuSpec::new(
        "ファイル",
        [MenuItem::new("開く").shortcut(MenuShortcut::new('o'))],
    )]);
    let name = menu_bar.native_action_name(&[0, 0]).expect("操作の名前");
    assert!(
        name.starts_with("naui-menubar-") && name.ends_with(".m0-0"),
        "メニューバーごとの名前空間に入る: {name}"
    );
    assert!(
        app.accels_for_action(&name).is_empty(),
        "取り付ける前はアクセラレータへ登録しない"
    );

    window.set_menu_bar(&menu_bar);
    assert_eq!(
        app.accels_for_action(&name),
        vec!["<Control>o"],
        "取り付けるとアプリのアクセラレータへ登録する"
    );
    let mount = menu_bar.native_menu_bar();
    assert_eq!(
        mount
            .ancestor(adw::ApplicationWindow::static_type())
            .as_ref(),
        Some(native.upcast_ref::<gtk::Widget>()),
        "ウィンドウの中に入っていること"
    );

    let seen = Rc::new(Cell::new(0));
    menu_bar.on_activate({
        let seen = seen.clone();
        move |_path| seen.set(seen.get() + 1)
    });
    WidgetExt::activate_action(&native, &name, None).expect("ウィンドウから操作を引けること");
    assert_eq!(seen.get(), 1);

    window.clear_menu_bar();
    assert!(
        mount
            .ancestor(adw::ApplicationWindow::static_type())
            .is_none(),
        "外すとウィンドウから消える"
    );
    assert!(
        WidgetExt::activate_action(&native, &name, None).is_err(),
        "外すと操作も引けなくなる"
    );
    assert!(
        app.accels_for_action(&name).is_empty(),
        "外すとアクセラレータの登録も外す"
    );

    // 外している間に組み替えても、登録はしない。
    menu_bar.set_menus(&[MenuSpec::new(
        "ファイル",
        [MenuItem::new("保存").shortcut(MenuShortcut::new('s'))],
    )]);
    assert!(app.accels_for_action(&name).is_empty());

    // 付け直すと、そのときの項目で登録し直す。
    window.set_menu_bar(&menu_bar);
    assert_eq!(app.accels_for_action(&name), vec!["<Control>s"]);

    // 取り付けたまま作り直すと、以前のアクセラレータは残らない。
    menu_bar.set_menus(&[]);
    assert!(
        app.accels_for_action(&name).is_empty(),
        "項目が消えたらアクセラレータも外す"
    );
    window.clear_menu_bar();
    window.close();
    Ok(())
}

/// Rebuilding the menu bar from inside the notification does not double-borrow.
fn menu_bar_callback_is_reentrant_and_replaceable(ui: &Ui) -> Result<()> {
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

/// Even when the same shortcut is used in the menu bars of two windows, neither
/// overwrites the other's registration, and each window can look up only its own
/// action.
fn menu_bar_shortcuts_are_kept_per_menu_bar(ui: &Ui) -> Result<()> {
    let spec = [MenuSpec::new(
        "ファイル",
        [MenuItem::new("開く").shortcut(MenuShortcut::new('o'))],
    )];

    let first_window = ui.window("1 つ目", 400.0, 300.0)?;
    let first = ui.menu_bar()?;
    first.set_menus(&spec);
    first_window.set_menu_bar(&first);

    let second_window = ui.window("2 つ目", 400.0, 300.0)?;
    let second = ui.menu_bar()?;
    second.set_menus(&spec);
    second_window.set_menu_bar(&second);

    let first_name = first.native_action_name(&[0, 0]).expect("操作の名前");
    let second_name = second.native_action_name(&[0, 0]).expect("操作の名前");
    assert_ne!(first_name, second_name, "メニューバーごとに名前空間が違う");

    let app = first_window
        .native_window()
        .application()
        .expect("GtkApplication");
    assert_eq!(
        app.accels_for_action(&first_name),
        vec!["<Control>o"],
        "あとから作ったメニューバーに上書きされない"
    );
    assert_eq!(app.accels_for_action(&second_name), vec!["<Control>o"]);

    let seen = Rc::new(RefCell::new(Vec::new()));
    first.on_activate({
        let seen = seen.clone();
        move |_path| seen.borrow_mut().push(1)
    });
    second.on_activate({
        let seen = seen.clone();
        move |_path| seen.borrow_mut().push(2)
    });

    // それぞれのウィンドウから引けるのは、自分のメニューバーの操作だけ。
    let first_native = first_window.native_window();
    let second_native = second_window.native_window();
    WidgetExt::activate_action(&first_native, &first_name, None).expect("1 つ目の操作");
    WidgetExt::activate_action(&second_native, &second_name, None).expect("2 つ目の操作");
    assert_eq!(*seen.borrow(), vec![1, 2]);
    assert!(
        WidgetExt::activate_action(&first_native, &second_name, None).is_err(),
        "1 つ目のウィンドウから 2 つ目の操作は引けない"
    );
    assert!(
        WidgetExt::activate_action(&second_native, &first_name, None).is_err(),
        "2 つ目のウィンドウから 1 つ目の操作は引けない"
    );

    first_window.clear_menu_bar();
    second_window.clear_menu_bar();
    first_window.close();
    second_window.close();
    Ok(())
}

fn sidebar_fixture() -> Vec<SidebarSection> {
    vec![
        SidebarSection::untitled([
            SidebarItem::new("一般").icon(ToolbarIcon::Settings),
            SidebarItem::new("検索"),
        ]),
        SidebarSection::new(
            "場所",
            [
                SidebarItem::new("書類").icon(ToolbarIcon::Open),
                SidebarItem::new("共有").enabled(false),
            ],
        ),
    ]
}

/// Collects a `GtkListBox`'s rows from the top.
fn list_box_rows(list: &gtk::ListBox) -> Vec<gtk::ListBoxRow> {
    let mut rows = Vec::new();
    let mut index = 0;
    while let Some(row) = list.row_at_index(index) {
        rows.push(row);
        index += 1;
    }
    rows
}

/// The text in the row's content (heading or item name).
fn row_text(row: &gtk::ListBoxRow) -> Option<String> {
    let child = row.child()?;
    if let Some(label) = child.downcast_ref::<gtk::Label>() {
        return Some(label.text().to_string());
    }
    let mut next = child.first_child();
    while let Some(widget) = next {
        if let Some(label) = widget.downcast_ref::<gtk::Label>() {
            return Some(label.text().to_string());
        }
        next = widget.next_sibling();
    }
    None
}

fn sidebar_rows_map_to_a_list_box(ui: &Ui) -> Result<()> {
    let sidebar = ui.sidebar()?;
    assert!(sidebar.is_empty());
    sidebar.set_sections(&sidebar_fixture());
    assert_eq!(sidebar.len(), 4, "見出しと区切りは数えない");

    let list = sidebar.native_list_box();
    assert!(list.has_css_class("navigation-sidebar"));
    let rows = list_box_rows(&list);
    // 一般・検索 | 区切り | 場所 (見出し) | 書類・共有
    assert_eq!(rows.len(), 6);
    assert!(rows[2].child().is_some_and(|c| c.is::<gtk::Separator>()));
    assert!(!rows[2].is_selectable() && !rows[2].is_activatable());
    assert_eq!(row_text(&rows[3]).as_deref(), Some("場所"));
    assert!(!rows[3].is_selectable(), "見出しは選べない");
    assert_eq!(row_text(&rows[0]).as_deref(), Some("一般"));
    assert!(
        rows[0]
            .child()
            .and_then(|c| c.first_child())
            .is_some_and(|c| c.is::<gtk::Image>()),
        "アイコンを指定した項目は GtkImage を持つ"
    );
    assert!(!rows[5].is_selectable(), "選べない項目は選べない");
    assert!(!rows[5].is_sensitive());

    // 幅は GtkPaned の仕切りの位置。利用者が動かせる。
    let paned = sidebar.native_paned();
    assert_eq!(paned.position(), DEFAULT_SIDEBAR_WIDTH as i32);
    assert!(!paned.resizes_start_child(), "ウィンドウを広げても幅を保つ");
    assert!(!paned.shrinks_start_child(), "下限より狭くはならない");
    let resized = Rc::new(RefCell::new(Vec::new()));
    sidebar.on_resize({
        let resized = resized.clone();
        move |width| resized.borrow_mut().push(width)
    });
    sidebar.set_width(160.0);
    assert_eq!(paned.position(), 160);
    assert_eq!(sidebar.width(), 160.0);
    sidebar.set_width(40.0);
    assert_eq!(
        sidebar.width(),
        naui_core::SIDEBAR_MIN_WIDTH,
        "下限で止まる"
    );
    sidebar.set_width(160.0);
    assert!(resized.borrow().is_empty(), "set_width では通知しない");
    // 利用者が仕切りを動かしたときと同じ経路 (position プロパティ)。
    paned.set_position(240);
    assert_eq!(sidebar.width(), 240.0);
    assert_eq!(*resized.borrow(), [240.0], "利用者が動かしたら通知する");

    sidebar.set_collapsed(true);
    assert!(sidebar.is_collapsed());
    assert!(
        !paned.start_child().is_some_and(|c| c.get_visible()),
        "区画が隠れる"
    );
    sidebar.set_collapsed(false);
    assert!(paned.start_child().is_some_and(|c| c.get_visible()));
    assert_eq!(paned.position(), 240, "開き直すと元の幅");

    sidebar.set_items(&SidebarItem::list(["春", "夏"]));
    assert_eq!(
        list_box_rows(&list).len(),
        2,
        "まとまり 1 つなら区切りは無い"
    );
    Ok(())
}

fn sidebar_selection_round_trips(ui: &Ui) -> Result<()> {
    let sidebar = ui.sidebar()?;
    sidebar.set_sections(&sidebar_fixture());
    let seen = Rc::new(RefCell::new(Vec::new()));
    sidebar.on_select({
        let seen = seen.clone();
        move |index| seen.borrow_mut().push(index)
    });
    let list = sidebar.native_list_box();
    assert_eq!(sidebar.selected(), None);

    sidebar.set_selected(2);
    assert_eq!(sidebar.selected(), Some(2));
    assert_eq!(list.selected_row().map(|r| r.index()), Some(4));
    assert!(seen.borrow().is_empty(), "set_selected は通知しない");
    sidebar.set_selected(3);
    sidebar.set_selected(9);
    assert_eq!(sidebar.selected(), Some(2), "選べない・範囲外は無視");

    sidebar.select(1);
    assert_eq!(*seen.borrow(), [1]);
    sidebar.select(1);
    assert_eq!(*seen.borrow(), [1, 1], "同じ項目でも通知する");

    // 利用者が選んだときと同じく、一覧の側から選ぶ。
    list.select_row(list.row_at_index(0).as_ref());
    assert_eq!(sidebar.selected(), Some(0));
    assert_eq!(*seen.borrow(), [1, 1, 0]);

    sidebar.set_selected(2);
    sidebar.set_sections(&sidebar_fixture());
    assert_eq!(sidebar.selected(), Some(2), "同じ番号が選べれば残る");
    assert_eq!(list.selected_row().map(|r| r.index()), Some(4));
    sidebar.set_items(&SidebarItem::list(["ひとつ"]));
    assert_eq!(sidebar.selected(), None, "無くなった番号の選択は外れる");
    sidebar.set_selected(0);
    sidebar.clear_selection();
    assert_eq!(sidebar.selected(), None);
    assert!(list.selected_row().is_none());
    assert_eq!(seen.borrow().len(), 3, "差し替えと解除は通知しない");
    Ok(())
}

fn sidebar_attaches_to_a_window(ui: &Ui) -> Result<()> {
    let window = ui.window("サイドバー", 640.0, 400.0)?;
    let content = ui.stack(Orientation::Vertical)?;
    window.set_child(&content);
    let native = window.native_window();
    let view = native.content().expect("中身");
    assert!(view.is::<adw::ToolbarView>());

    let sidebar = ui.sidebar()?;
    sidebar.set_items(&SidebarItem::list(["一般"]));
    window.set_sidebar(&sidebar);
    let paned = sidebar.native_paned();
    assert_eq!(
        native.content().as_ref(),
        Some(paned.upcast_ref::<gtk::Widget>()),
        "ウィンドウの中身が GtkPaned になる"
    );
    assert_eq!(
        paned.end_child(),
        Some(view.clone()),
        "元の中身は右の区画へ"
    );
    assert!(paned
        .start_child()
        .is_some_and(|pane| pane.is::<adw::ToolbarView>()));
    // ウィンドウのボタンは、ウィンドウの端に接するほうにだけ出す。
    assert!(!sidebar.native_header_bar().shows_end_title_buttons());
    assert!(!window.native_header_bar().shows_start_title_buttons());

    // 付けたままでも、子とトーストは右の区画のトースト置き場に入る。
    let other = ui.stack(Orientation::Vertical)?;
    window.set_child(&other);
    assert!(window.native_toast_overlay().child().is_some());
    window.show();
    let toast = ui.toast("保存しました")?;
    toast.show();
    assert!(toast.is_visible(), "右の区画のトースト置き場が見つかる");
    toast.dismiss();

    window.clear_sidebar();
    assert_eq!(native.content(), Some(view), "外すと元の中身へ戻る");
    assert!(paned.end_child().is_none());
    assert!(
        window.native_header_bar().shows_start_title_buttons(),
        "外すと中身のヘッダーバーが両端のボタンを出し直す"
    );
    window.clear_sidebar(); // 付いていなければ何もしない
    window.close();
    Ok(())
}

/// When the window gets too narrow for the sidebar and
/// `SIDEBAR_COLLAPSE_CONTENT_WIDTH` of content, the sidebar collapses after the paint
/// and expands again when it widens. What the user chose is kept.
fn sidebar_collapses_when_the_window_narrows(ui: &Ui) -> Result<()> {
    use naui_core::SIDEBAR_COLLAPSE_CONTENT_WIDTH;
    use std::time::Duration;

    let window = ui.window("サイドバーの自動の開閉", 800.0, 400.0)?;
    window.set_child(&ui.stack(Orientation::Vertical)?);
    let sidebar = ui.sidebar()?;
    sidebar.set_items(&SidebarItem::list(["一般"]));
    assert!(sidebar.auto_collapse(), "既定で自動で畳む");
    window.set_sidebar(&sidebar);
    window.show();
    let paned = sidebar.native_paned();
    spin_until(Duration::from_secs(2), || paned.width() >= 700);
    assert!(!sidebar.is_collapsed(), "広いうちは開いている");
    let seen = Rc::new(RefCell::new(Vec::new()));
    sidebar.on_collapse({
        let seen = seen.clone();
        move |collapsed| seen.borrow_mut().push(collapsed)
    });
    let narrow = DEFAULT_SIDEBAR_WIDTH + SIDEBAR_COLLAPSE_CONTENT_WIDTH - 60.0;
    let wait = |len: usize| spin_until(Duration::from_secs(3), || seen.borrow().len() >= len);

    window.set_size(narrow, 400.0);
    wait(1);
    assert!(
        sidebar.is_collapsed(),
        "狭めると畳まれる: paned={}",
        paned.width()
    );
    assert_eq!(*seen.borrow(), [true], "畳んだことが届く");
    assert!(
        window.native_header_bar().shows_start_title_buttons(),
        "畳んだ姿はボタンで閉じたときと同じ"
    );

    window.set_size(800.0, 400.0);
    wait(2);
    assert!(!sidebar.is_collapsed(), "広げると開き直す");
    assert_eq!(*seen.borrow(), [true, false]);
    assert_eq!(
        f64::from(paned.position()),
        DEFAULT_SIDEBAR_WIDTH,
        "元の幅で開く"
    );

    // 狭いうちに利用者が開いたら開いたまま。
    window.set_size(narrow, 400.0);
    wait(3);
    sidebar.native_toggle_button().set_active(true);
    pump();
    assert_eq!(*seen.borrow(), [true, false, true, false]);
    window.set_size(narrow - 20.0, 400.0);
    spin_until(Duration::from_millis(500), || false);
    assert!(!sidebar.is_collapsed(), "狭いまま動かしても閉じない");

    // 利用者が閉じたものは、狭めて広げても開かない。
    window.set_size(800.0, 400.0);
    spin_until(Duration::from_millis(500), || false);
    sidebar.native_toggle_button().set_active(false);
    pump();
    window.set_size(narrow, 400.0);
    spin_until(Duration::from_millis(500), || false);
    window.set_size(800.0, 400.0);
    spin_until(Duration::from_millis(500), || false);
    assert!(sidebar.is_collapsed(), "利用者が閉じたまま");
    assert_eq!(seen.borrow().len(), 5, "利用者の操作だけが届く");

    // 自動で畳んだあとアプリが畳み直したら、見た目は変わらなくても
    // アプリが決めたものになり、広げても開かない。
    sidebar.set_collapsed(false);
    window.set_size(narrow, 400.0);
    wait(6);
    assert!(
        sidebar.is_collapsed(),
        "開き直したものもまた狭めると畳まれる"
    );
    sidebar.set_collapsed(true);
    window.set_size(800.0, 400.0);
    spin_until(Duration::from_millis(500), || false);
    assert!(sidebar.is_collapsed(), "アプリが畳んだまま");
    assert_eq!(seen.borrow().len(), 6);

    // 切ると畳まない。
    sidebar.set_collapsed(false);
    sidebar.set_auto_collapse(false);
    assert!(!sidebar.auto_collapse());
    window.set_size(narrow, 400.0);
    spin_until(Duration::from_millis(500), || false);
    assert!(!sidebar.is_collapsed(), "切っていれば畳まない");
    assert_eq!(seen.borrow().len(), 6);
    window.clear_sidebar();
    window.close();
    Ok(())
}

/// The automatic collapse is decided after a paint and done in the next main loop
/// iteration. If the width comes back in between, the scheduled collapse is cancelled
/// rather than collapsing the sidebar in a wide window, where it would never expand
/// again.
fn sidebar_drops_a_stale_auto_collapse(ui: &Ui) -> Result<()> {
    use naui_core::SIDEBAR_COLLAPSE_CONTENT_WIDTH;
    use std::time::Duration;

    let window = ui.window("サイドバーの古い予約", 800.0, 400.0)?;
    window.set_child(&ui.stack(Orientation::Vertical)?);
    let sidebar = ui.sidebar()?;
    sidebar.set_items(&SidebarItem::list(["一般"]));
    window.set_sidebar(&sidebar);
    window.show();
    let paned = sidebar.native_paned();
    spin_until(Duration::from_secs(2), || paned.width() >= 700);
    let wide = paned.width();
    let seen = Rc::new(RefCell::new(Vec::new()));
    sidebar.on_collapse({
        let seen = seen.clone();
        move |collapsed| seen.borrow_mut().push(collapsed)
    });

    // サイドバーの後につなぐので、狭い幅で描いて畳むのを予約した直後に呼ばれる。
    // そこで窓を戻し、描き直し (GDK_PRIORITY_REDRAW = 120) は通すが idle (200) は
    // 待たせる source を挟む。予約を行う前に、元の幅で描き直された形になる。
    let threshold = (DEFAULT_SIDEBAR_WIDTH + SIDEBAR_COLLAPSE_CONTENT_WIDTH) as i32;
    let clock = paned.frame_clock().expect("表示した後は時計がある");
    let done = Rc::new(Cell::new(false));
    let held = Rc::new(Cell::new(false));
    let handler = clock.connect_after_paint({
        let paned = paned.clone();
        let window = window.clone();
        let done = done.clone();
        let held = held.clone();
        move |_| {
            if done.get() || paned.width() >= threshold {
                return;
            }
            done.set(true);
            window.set_size(800.0, 400.0);
            let deadline = std::time::Instant::now() + Duration::from_secs(2);
            let paned = paned.clone();
            let held = held.clone();
            glib::idle_add_local_full(glib::Priority::from(150), move || {
                if paned.width() < wide && std::time::Instant::now() < deadline {
                    return glib::ControlFlow::Continue;
                }
                held.set(paned.width() >= wide);
                glib::ControlFlow::Break
            });
        }
    });
    let narrow = DEFAULT_SIDEBAR_WIDTH + SIDEBAR_COLLAPSE_CONTENT_WIDTH - 60.0;
    window.set_size(narrow, 400.0);
    spin_until(Duration::from_secs(2), || done.get());
    clock.disconnect(handler);
    assert!(done.get(), "狭い幅で描かれた");
    spin_until(Duration::from_secs(3), || held.get());
    assert!(held.get(), "予約を行う前に元の幅で描き直された");
    spin_until(Duration::from_millis(500), || false);
    assert!(
        !sidebar.is_collapsed(),
        "広い窓では畳まない: paned={}, 届いた通知={:?}",
        paned.width(),
        seen.borrow()
    );

    // 判断の状態も崩れていない。狭めれば畳まれ、広げれば開き直す。
    seen.borrow_mut().clear();
    window.set_size(narrow, 400.0);
    spin_until(Duration::from_secs(3), || !seen.borrow().is_empty());
    assert!(sidebar.is_collapsed(), "狭めると畳まれる");
    window.set_size(800.0, 400.0);
    spin_until(Duration::from_secs(3), || seen.borrow().len() >= 2);
    assert!(!sidebar.is_collapsed(), "広げると開き直す");
    assert_eq!(*seen.borrow(), [true, false]);
    window.clear_sidebar();
    window.close();
    Ok(())
}

/// When the content has a minimum width, `GtkPaned` narrows the sidebar before the
/// window gets too narrow for both. That is the layout's doing, not the user's: the
/// width is kept, collapsing is decided with it, and the sidebar is back at it once
/// there is room.
fn sidebar_collapses_beside_content_with_a_min_width(ui: &Ui) -> Result<()> {
    use std::time::Duration;

    let window = ui.window("中身に最小幅", 800.0, 400.0)?;
    let content = ui.stack(Orientation::Vertical)?;
    content.native_widget().set_size_request(400, -1);
    window.set_child(&content);
    let sidebar = ui.sidebar()?;
    sidebar.set_items(&SidebarItem::list(["一般"]));
    window.set_sidebar(&sidebar);
    window.show();
    let paned = sidebar.native_paned();
    spin_until(Duration::from_secs(2), || paned.width() >= 700);
    let seen = Rc::new(RefCell::new(Vec::new()));
    sidebar.on_collapse({
        let seen = seen.clone();
        move |collapsed| seen.borrow_mut().push(collapsed)
    });
    let resized = Rc::new(RefCell::new(Vec::new()));
    sidebar.on_resize({
        let resized = resized.clone();
        move |width| resized.borrow_mut().push(width)
    });

    // サイドバーと中身の最小幅の和 (620) より狭いが、畳む幅 (580) よりは広い。
    // GtkPaned がサイドバーを詰めても、それは利用者が決めた幅ではない。
    window.set_size(600.0, 400.0);
    spin_until(Duration::from_secs(2), || paned.width() < 620);
    spin_until(Duration::from_millis(300), || false);
    assert!(!sidebar.is_collapsed(), "まだ畳まない");
    assert!(
        paned.position() < DEFAULT_SIDEBAR_WIDTH as i32,
        "中身に押されて詰まる: position={}",
        paned.position()
    );
    assert_eq!(sidebar.width(), DEFAULT_SIDEBAR_WIDTH, "幅は保つ");
    assert!(resized.borrow().is_empty(), "幅の通知は来ない");

    // 元の幅で決めるので、畳む幅を切れば畳まれる。
    window.set_size(560.0, 400.0);
    spin_until(Duration::from_secs(3), || !seen.borrow().is_empty());
    assert!(
        sidebar.is_collapsed(),
        "畳まれる: paned={}, width={}",
        paned.width(),
        sidebar.width()
    );

    // 広げると元の幅で開き直す。
    window.set_size(800.0, 400.0);
    spin_until(Duration::from_secs(3), || seen.borrow().len() >= 2);
    assert!(!sidebar.is_collapsed(), "開き直す");
    assert_eq!(*seen.borrow(), [true, false]);
    tick(&paned);
    assert_eq!(
        paned.position(),
        DEFAULT_SIDEBAR_WIDTH as i32,
        "元の幅で開く"
    );

    // 詰まっただけのものも、場所ができれば元の幅へ戻る。
    window.set_size(600.0, 400.0);
    spin_until(Duration::from_secs(2), || paned.width() < 620);
    spin_until(Duration::from_millis(300), || false);
    assert!(paned.position() < DEFAULT_SIDEBAR_WIDTH as i32);
    window.set_size(800.0, 400.0);
    spin_until(Duration::from_secs(2), || {
        paned.width() >= 700 && paned.position() == DEFAULT_SIDEBAR_WIDTH as i32
    });
    assert_eq!(
        paned.position(),
        DEFAULT_SIDEBAR_WIDTH as i32,
        "元の幅へ戻る"
    );
    assert!(resized.borrow().is_empty(), "幅の通知は来ない");
    window.clear_sidebar();
    window.close();
    Ok(())
}

fn sidebar_callback_is_reentrant(ui: &Ui) -> Result<()> {
    let sidebar = ui.sidebar()?;
    sidebar.set_items(&SidebarItem::list(["春", "夏"]));
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
    sidebar.select(0);
    assert_eq!(*seen.borrow(), [1, 100]);
    Ok(())
}

fn sidebar_toggle_button_collapses_and_notifies(ui: &Ui) -> Result<()> {
    let window = ui.window("サイドバーボタン", 640.0, 400.0)?;
    window.set_child(&ui.stack(Orientation::Vertical)?);
    let toolbar = ui.toolbar()?;
    toolbar.set_items(&[ToolbarItem::new(ToolbarIcon::New, "新規")]);
    window.set_toolbar(&toolbar);

    let sidebar = ui.sidebar()?;
    window.set_sidebar(&sidebar);
    pump();
    let toggle = sidebar.native_toggle_button();
    let side_header = sidebar.native_header_bar();
    let content_header = window.native_header_bar();
    assert_eq!(toggle.icon_name().as_deref(), Some("sidebar-show-symbolic"));
    // ツールチップは起動時に選んだ表に従う (表の選び方は naui-gtk の単体テスト)。
    assert_eq!(
        toggle.tooltip_text().as_deref(),
        Some(naui_core::ui_strings().sidebar)
    );
    assert!(
        toggle.is_ancestor(&side_header),
        "開いている間はサイドバーのヘッダーバーの左端"
    );
    assert!(toggle.is_active(), "開いている間は押し込まれている");

    let seen = Rc::new(RefCell::new(Vec::new()));
    sidebar.on_collapse({
        let seen = seen.clone();
        move |collapsed| seen.borrow_mut().push(collapsed)
    });
    toggle.set_active(false); // 利用者がボタンを押したのと同じ
    pump();
    assert!(sidebar.is_collapsed());
    assert_eq!(*seen.borrow(), [true]);
    assert!(
        toggle.is_ancestor(&content_header),
        "閉じたら中身のヘッダーバーの左端へ移る"
    );
    // ツールバーより左に来る。
    let slot = toggle.parent().expect("置き場");
    assert_eq!(
        slot.next_sibling().as_ref(),
        Some(toolbar.native_box().upcast_ref::<gtk::Widget>()),
        "サイドバーボタンの右隣がツールバー"
    );
    assert!(
        content_header.shows_start_title_buttons(),
        "閉じたら中身のヘッダーバーが左側のウィンドウのボタンも出す"
    );

    toggle.set_active(true);
    pump();
    assert!(!sidebar.is_collapsed());
    assert_eq!(*seen.borrow(), [true, false]);
    assert!(toggle.is_ancestor(&side_header), "開いたら戻る");

    sidebar.set_collapsed(true);
    pump();
    assert!(!toggle.is_active(), "set_collapsed もボタンに映る");
    sidebar.set_collapsed(false);
    pump();
    assert_eq!(seen.borrow().len(), 2, "set_collapsed では通知しない");

    // 閉じている間に置いた幅が、ボタンで開き直したときに反映され、その後の
    // 利用者の仕切りの操作も通知される。
    let paned = sidebar.native_paned();
    let resized = Rc::new(RefCell::new(Vec::new()));
    sidebar.on_resize({
        let resized = resized.clone();
        move |width| resized.borrow_mut().push(width)
    });
    sidebar.set_width(260.0);
    sidebar.set_collapsed(true);
    pump();
    sidebar.set_width(300.0);
    toggle.set_active(true); // 利用者がボタンで開いたのと同じ
    pump();
    assert!(!sidebar.is_collapsed());
    assert_eq!(paned.position(), 300, "開き直すと閉じている間に置いた幅");
    assert!(
        resized.borrow().is_empty(),
        "set_width と開き直しでは通知しない"
    );
    paned.set_position(250); // 利用者が仕切りを動かしたのと同じ経路
    assert_eq!(*resized.borrow(), [250.0], "開き直したあとの操作も通知する");

    window.clear_sidebar();
    window.clear_toolbar();
    window.close();
    Ok(())
}

// ------------------------------------------------- 表示・フォーカス・ツールチップ

fn hidden_widget_leaves_the_stack_layout(ui: &Ui) -> Result<()> {
    let stack = ui.stack(Orientation::Vertical)?;
    let middle = ui.button("真ん中")?;
    stack.append(&ui.label("1 行目")?);
    stack.append(&middle);
    stack.append(&ui.label("3 行目")?);
    let root = bin_of(&stack);
    let (_, shown) = measure_height(&root);
    assert!(middle.is_visible(), "既定は表示");

    middle.set_visible(false);
    assert!(!middle.is_visible());
    assert!(!bin_of(&middle).is_visible(), "入れ物ごと隠す");
    let (_, hidden) = measure_height(&root);
    assert!(
        hidden + 10 < shown,
        "隠すと場所を空けない: {shown} -> {hidden}"
    );

    middle.set_visible(true);
    assert!(middle.is_visible());
    assert_eq!(measure_height(&root).1, shown, "戻すと元の高さ");
    Ok(())
}

fn focus_change_follows_the_focus_widget(ui: &Ui) -> Result<()> {
    let window = ui.window("フォーカス", 320.0, 300.0)?;
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
    tick(&bin_of(&root));

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
    tick(&bin_of(&root));

    // 通知先が捕まえている状態。解放されれば数が 1 へ戻る。
    let state = Rc::new(());
    let capture = || {
        let state = state.clone();
        move |_: bool| {
            let _ = &state;
        }
    };

    // 表示中の木へ入れてフォーカスを持たせてから外す。
    let input = ui.text_input("")?;
    root.append(&input);
    tick(&bin_of(&root));
    input.on_focus_change(capture());
    // 差し替えても、前の通知先は残らない。
    input.on_focus_change(capture());
    assert!(input.request_focus());
    assert_eq!(Rc::strong_count(&state), 2);
    root.remove(1);
    drop(input);
    tick(&bin_of(&root));
    // フォーカスが別の欄へ移っても、表示中の窓の持ち主から外れる。
    assert!(kept.request_focus());
    assert_eq!(Rc::strong_count(&state), 1, "外して捨てた欄の通知先");

    // 木へ入れないまま捨てる。
    let area = ui.text_area("")?;
    area.on_focus_change(capture());
    drop(area);
    assert_eq!(Rc::strong_count(&state), 1, "木へ入れていない欄の通知先");

    window.close();
    Ok(())
}

fn request_focus_reaches_the_editor(ui: &Ui) -> Result<()> {
    let window = ui.window("フォーカス", 320.0, 240.0)?;
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
    window.set_child(&root);

    assert!(!input.request_focus(), "表示前は移せない");
    window.show();
    tick(&bin_of(&root));

    let focused = |widget: &gtk::Widget| {
        widget
            .root()
            .and_then(|root| root.focus())
            .is_some_and(|focus| focus == *widget || focus.is_ancestor(widget))
    };

    assert!(!label.request_focus(), "ラベルは受け取らない");
    assert!(input.request_focus());
    assert!(focused(&input.native_widget()), "入力欄が受け取る");
    assert!(area.request_focus());
    let focus = area
        .native_widget()
        .root()
        .and_then(|root| root.focus())
        .expect("フォーカス");
    assert!(
        focus.downcast_ref::<gtk::TextView>().is_some(),
        "複数行入力は中の GtkTextView が受け取る: {}",
        focus.type_().name()
    );
    assert!(inner.request_focus(), "コンテナは中の最初の入力欄へ");
    assert!(focused(&nested.native_widget()));

    input.set_visible(false);
    assert!(!input.request_focus(), "隠れていると移せない");
    window.close();
    Ok(())
}

fn tooltip_reaches_the_widget(ui: &Ui) -> Result<()> {
    let button = ui.button("保存")?;
    button.set_tooltip(Some("変更を保存します"));
    assert_eq!(
        button.native_widget().tooltip_text().as_deref(),
        Some("変更を保存します")
    );
    button.set_tooltip(None);
    assert_eq!(button.native_widget().tooltip_text(), None);
    Ok(())
}

// ------------------------------------------------------------- on_key_down

/// Passes one key to the key receiver attached to the window. `true` if it was
/// stopped.
///
/// GTK4 has no public API for creating real key events, so the signal of the
/// capture-phase `GtkEventControllerKey` naui attached is emitted directly.
fn press_key(
    window: &naui_gtk::Window,
    keyval: gtk::gdk::Key,
    state: gtk::gdk::ModifierType,
) -> bool {
    let native = window.native_window();
    let controllers = native.observe_controllers();
    let controller = (0..controllers.n_items())
        .filter_map(|index| controllers.item(index))
        .filter_map(|item| item.downcast::<gtk::EventControllerKey>().ok())
        .find(|controller| controller.propagation_phase() == gtk::PropagationPhase::Capture)
        .expect("naui のキーの受け口");
    let stopped: bool = controller.emit_by_name("key-pressed", &[&keyval, &0u32, &state]);
    controller.emit_by_name::<()>("key-released", &[&keyval, &0u32, &state]);
    stopped
}

fn key_down_window(ui: &Ui) -> Result<(naui_gtk::Window, naui_gtk::TextArea)> {
    let window = ui.window("キー", 320.0, 200.0)?;
    let area = ui.text_area("")?;
    area.set_sizing(Sizing::fill());
    window.set_child(&area);
    window.show();
    tick(&area.native_widget());
    assert!(area.request_focus());
    Ok((window, area))
}

fn text_area_key_down_can_take_enter(ui: &Ui) -> Result<()> {
    use gtk::gdk::{Key as K, ModifierType as M};
    let (window, area) = key_down_window(ui)?;
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
    assert!(press_key(&window, K::Return, M::empty()), "Enter は止める");
    assert!(
        !press_key(&window, K::Return, M::SHIFT_MASK),
        "Shift+Enter は通す"
    );
    // 修飾キーを単独で押しても届かない。
    assert!(!press_key(&window, K::Shift_L, M::empty()));
    assert!(!press_key(&window, K::A, M::SHIFT_MASK));
    assert!(!press_key(&window, K::F5, M::empty()));
    assert_eq!(
        *seen.borrow(),
        vec![
            (naui_core::Key::Enter, false),
            (naui_core::Key::Enter, true),
            (naui_core::Key::Character('a'), true),
            (naui_core::Key::F(5), false),
        ]
    );
    window.close();
    Ok(())
}

fn key_down_skips_composition(ui: &Ui) -> Result<()> {
    use gtk::gdk::{Key as K, ModifierType as M};
    let (window, area) = key_down_window(ui)?;
    let count = Rc::new(Cell::new(0));
    area.on_key_down({
        let count = count.clone();
        move |_| {
            count.set(count.get() + 1);
            naui_core::EventResponse::Handled
        }
    });
    // 最初のキーで preedit-changed の購読が付く。
    press_key(&window, K::x, M::empty());
    assert_eq!(count.get(), 1);

    let view = gtk::prelude::GtkWindowExt::focus(&window.native_window())
        .and_downcast::<gtk::TextView>()
        .expect("GtkTextView にフォーカス");
    view.emit_by_name::<()>("preedit-changed", &[&"かな"]);
    assert!(
        !press_key(&window, K::Return, M::empty()),
        "変換中は IME へ通す"
    );
    assert_eq!(count.get(), 1, "変換中の Enter は届かない");

    view.emit_by_name::<()>("preedit-changed", &[&""]);
    assert!(press_key(&window, K::Return, M::empty()));
    assert_eq!(count.get(), 2, "確定後は届く");
    window.close();
    Ok(())
}

fn window_key_down_gets_what_the_widget_left(ui: &Ui) -> Result<()> {
    use gtk::gdk::{Key as K, ModifierType as M};
    let window = ui.window("キー", 320.0, 200.0)?;
    let input = ui.text_input("")?;
    window.set_child(&input);
    window.show();
    tick(&input.native_widget());
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
    press_key(&window, K::x, M::empty());
    press_key(&window, K::Return, M::empty());
    press_key(&window, K::Escape, M::empty());
    assert_eq!(
        *seen.borrow(),
        vec![naui_core::Key::Character('x'), naui_core::Key::Escape]
    );
    window.close();
    Ok(())
}

// ------------------------------------------------- アプリ向けの基本機能

/// Iterates the main loop until the condition is met (up to the limit).
fn spin_until(limit: std::time::Duration, mut ready: impl FnMut() -> bool) {
    let context = glib::MainContext::default();
    let deadline = std::time::Instant::now() + limit;
    while !ready() && std::time::Instant::now() < deadline {
        if !context.iteration(false) {
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }
}

fn timers_fire_on_the_main_loop(ui: &Ui) -> Result<()> {
    use std::time::Duration;
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
    assert_eq!(once.get(), 0, "その場では呼ばない");
    spin_until(Duration::from_secs(2), || {
        once.get() == 1 && !timer.is_active()
    });
    assert_eq!(once.get(), 1);
    assert_eq!(ticks.get(), 3, "3 回目で止めた");
    Ok(())
}

fn progress_bar_switches_to_indeterminate(ui: &Ui) -> Result<()> {
    let progress = ui.progress_bar()?;
    progress.set_value(0.4);
    progress.set_indeterminate(true);
    assert!(progress.is_indeterminate());
    progress.set_value(0.7);
    progress.set_indeterminate(false);
    assert!(!progress.is_indeterminate());
    let native: gtk::ProgressBar = progress.native_widget().downcast().expect("GtkProgressBar");
    assert_eq!(native.fraction(), 0.7, "不確定の間に置いた値が出る");
    Ok(())
}

fn spinner_stops_without_giving_up_its_space(ui: &Ui) -> Result<()> {
    let spinner = ui.spinner()?;
    assert!(spinner.is_spinning(), "作った時点で回っている");
    let native = spinner.native_widget();
    let adw_spinner = (adw::major_version(), adw::minor_version()) >= (1, 6);
    let expected = if adw_spinner {
        "AdwSpinner"
    } else {
        "GtkSpinner"
    };
    assert_eq!(native.type_().name(), expected);
    assert_eq!(native.accessible_role(), gtk::AccessibleRole::ProgressBar);
    spinner.set_accessible_label(Some("Loading"));
    if std::env::var("GTK_A11Y").as_deref() != Ok("none") {
        assert!(gtk::test_accessible_has_property(
            &native,
            gtk::AccessibleProperty::Label
        ));
        assert!(gtk::test_accessible_has_state(
            &native,
            gtk::AccessibleState::Busy
        ));
    }
    let size = native.measure(gtk::Orientation::Horizontal, -1);
    assert_eq!((size.0, size.1), (16, 16), "既定は 16 x 16");

    spinner.set_spinning(false);
    assert!(!spinner.is_spinning());
    match native.downcast_ref::<gtk::Spinner>() {
        Some(gtk_spinner) => assert!(!gtk_spinner.is_spinning()),
        None => assert!(!native.is_child_visible(), "AdwSpinner は写さずに止める"),
    }
    assert!(native.is_visible(), "場所は残る");
    assert_eq!(
        spinner
            .size_bin()
            .measure(gtk::Orientation::Horizontal, -1)
            .1,
        16,
        "止めても大きさは変わらない"
    );

    spinner.set_spinning(true);
    assert!(spinner.is_spinning());
    match native.downcast_ref::<gtk::Spinner>() {
        Some(gtk_spinner) => assert!(gtk_spinner.is_spinning()),
        None => assert!(native.is_child_visible()),
    }
    Ok(())
}

fn spinner_fits_below_native_minimum(ui: &Ui) -> Result<()> {
    let spinner = ui.spinner()?;
    let native = spinner.native_widget();
    let bin = spinner.size_bin();
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    let next = gtk::Label::new(Some("next"));
    spinner.set_sizing(Sizing::fixed(8.0, 8.0));
    row.append(&bin);
    row.append(&next);
    let window = gtk::Window::builder()
        .title("spinner")
        .default_width(200)
        .default_height(80)
        .child(&row)
        .build();
    window.present();
    tick(&native);
    let bounds = native.compute_bounds(&row).expect("spinner bounds");
    let next_bounds = next.compute_bounds(&row).expect("neighbor bounds");
    assert_eq!((bin.width(), bin.height()), (8, 8));
    assert!(
        (bounds.width() - 8.0).abs() < 0.01 && (bounds.height() - 8.0).abs() < 0.01,
        "native bounds: {bounds:?}"
    );
    assert!(
        bounds.x() + bounds.width() <= next_bounds.x(),
        "spinner {bounds:?} overlaps neighbor {next_bounds:?}"
    );
    // Check the rendered pixels as well as the transformed allocation: merely
    // clipping the original 16×16 spinner would discard part of its shape.
    let snapshot = gtk::Snapshot::new();
    bin.snapshot_child(&native, &snapshot);
    let node = snapshot.to_node().expect("spinner is drawn");
    let mut surface = gtk::cairo::ImageSurface::create(gtk::cairo::Format::ARgb32, 16, 16)
        .expect("image surface");
    let cr = gtk::cairo::Context::new(&surface).expect("cairo context");
    node.draw(&cr);
    drop(cr);
    surface.flush();
    let stride = surface.stride() as usize;
    let pixels = surface.data().expect("image pixels");
    let alpha = |x: usize, y: usize| pixels[y * stride + x * 4 + 3];
    assert!(
        (0..8).any(|x| (0..8).any(|y| alpha(x, y) > 0)),
        "spinner draws inside its allocation"
    );
    assert!(
        (0..16).all(|x| (0..16).all(|y| (x < 8 && y < 8) || alpha(x, y) == 0)),
        "spinner draws outside its allocation"
    );
    drop(pixels);
    spinner.set_sizing(Sizing::fixed(8.0, 20.0));
    tick(&native);
    let bounds = native.compute_bounds(&bin).expect("narrow bounds");
    assert!(
        (bounds.width() - 8.0).abs() < 0.01 && bounds.height() <= 20.0,
        "narrow bounds: {bounds:?}"
    );
    spinner.set_sizing(Sizing::fixed(0.0, 8.0));
    tick(&native);
    assert_eq!(bin.width(), 0, "zero width is kept");
    assert_eq!(
        bin.overflow(),
        gtk::Overflow::Hidden,
        "zero allocation clips the child"
    );
    spinner.set_sizing(Sizing::new());
    tick(&native);
    let bounds = native.compute_bounds(&bin).expect("default bounds");
    assert_eq!((bounds.width(), bounds.height()), (16.0, 16.0));
    window.close();
    Ok(())
}

fn label_can_be_selectable(ui: &Ui) -> Result<()> {
    let label = ui.label("コピーできる文字")?;
    assert!(!label.is_selectable(), "既定は選べない");
    label.set_selectable(true);
    let native: gtk::Label = label.native_widget().downcast().expect("GtkLabel");
    assert!(native.is_selectable());
    Ok(())
}

fn clipboard_round_trips_text(ui: &Ui) -> Result<()> {
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
    spin_until(std::time::Duration::from_secs(2), || {
        written.borrow().is_some()
    });
    assert_eq!(*written.borrow(), Some(true), "書き込めた");
    let read = Rc::new(RefCell::new(None));
    clipboard.read_text({
        let read = read.clone();
        move |text| *read.borrow_mut() = Some(text)
    });
    assert!(read.borrow().is_none(), "その場では呼ばない");
    spin_until(std::time::Duration::from_secs(2), || {
        read.borrow().is_some()
    });
    assert_eq!(*read.borrow(), Some(Some("naui のテスト ✂".to_string())));
    Ok(())
}

fn open_url_rejects_garbage(ui: &Ui) -> Result<()> {
    assert!(ui.open_url("これは URL ではない").is_err());
    Ok(())
}

/// GTK4 has no API for window positions (dropped on X11 too, to match Wayland).
fn window_position_is_unavailable(ui: &Ui) -> Result<()> {
    let window = ui.window("位置", 320.0, 200.0)?;
    window.set_child(&ui.label("中身")?);
    assert_eq!(window.position(), None);
    window.set_position(120.0, 140.0);
    window.show();
    assert_eq!(window.position(), None, "動かしても取れないまま");
    window.close();
    Ok(())
}

/// Maximize and fullscreen round-trip with `state`, and `on_state_change` is called
/// on each change.
///
/// Both are requests to the compositor (window manager), so in an environment where
/// nobody responds (CI's Xvfb) nothing changes. In that case it is SKIPped.
/// Minimizing is not checked here, because Wayland has no way to know about it or
/// to restore it.
fn window_state_round_trips(ui: &Ui) -> Result<()> {
    use std::time::Duration;
    let window = ui.window("状態", 320.0, 200.0)?;
    window.set_child(&ui.label("中身")?);
    let (seen, sink) = recorder();
    window.on_state_change(sink);
    window.show();
    spin_until(Duration::from_secs(1), || {
        window.native_window().is_realized()
    });
    assert_eq!(window.state(), WindowState::Normal);
    let native = window.native_window();

    window.set_state(WindowState::Maximized);
    spin_until(Duration::from_secs(2), || {
        window.state() == WindowState::Maximized
    });
    if window.state() != WindowState::Maximized {
        skip("ウィンドウマネージャが最大化に応えない環境 (CI の Xvfb)");
        window.close();
        return Ok(());
    }
    assert!(native.is_maximized());
    window.set_state(WindowState::Normal);
    spin_until(Duration::from_secs(2), || {
        window.state() == WindowState::Normal
    });
    assert_eq!(window.state(), WindowState::Normal);
    assert!(!native.is_maximized());

    window.set_state(WindowState::Fullscreen);
    spin_until(Duration::from_secs(3), || {
        window.state() == WindowState::Fullscreen
    });
    assert_eq!(window.state(), WindowState::Fullscreen);
    assert!(native.is_fullscreen());
    // 全画面から最大化へ移ると、全画面は解ける。
    window.set_state(WindowState::Maximized);
    spin_until(Duration::from_secs(3), || {
        window.state() == WindowState::Maximized
    });
    assert_eq!(window.state(), WindowState::Maximized, "全画面から最大化へ");
    assert!(!native.is_fullscreen());
    window.set_state(WindowState::Normal);
    spin_until(Duration::from_secs(3), || {
        window.state() == WindowState::Normal
    });
    assert_eq!(window.state(), WindowState::Normal);
    assert_eq!(
        seen.borrow().first(),
        Some(&WindowState::Maximized),
        "変わるたびに届く: {:?}",
        seen.borrow()
    );
    assert_eq!(seen.borrow().last(), Some(&WindowState::Normal));
    assert!(
        seen.borrow().contains(&WindowState::Fullscreen),
        "全画面も届く: {:?}",
        seen.borrow()
    );
    window.close();
    Ok(())
}

/// Even when states are requested back to back before the compositor responds, the
/// window ends up in the last state requested.
///
/// `is_maximized` / `is_fullscreen` do not change until the compositor responds, so
/// looking only at them would fail to withdraw unanswered requests and an earlier
/// state would land later.
fn window_state_requests_in_a_row(ui: &Ui) -> Result<()> {
    use std::time::Duration;
    let sequences: &[&[WindowState]] = &[
        &[WindowState::Fullscreen, WindowState::Normal],
        &[WindowState::Maximized, WindowState::Normal],
        &[WindowState::Fullscreen, WindowState::Maximized],
        &[
            WindowState::Maximized,
            WindowState::Fullscreen,
            WindowState::Normal,
        ],
    ];
    if is_unresponsive(ui)? {
        skip("ウィンドウマネージャが最大化に応えない環境 (CI の Xvfb)");
        return Ok(());
    }
    for sequence in sequences {
        let window = ui.window("続けて頼む", 320.0, 200.0)?;
        window.set_child(&ui.label("中身")?);
        window.show();
        spin_until(Duration::from_secs(1), || {
            !toplevel_state(&window).is_empty()
        });
        for &state in *sequence {
            window.set_state(state);
        }
        let last = *sequence.last().unwrap();
        // 先の頼みが後から当たっていないことも確かめるため、待ち切ってから比べる。
        spin_until(Duration::from_secs(3), || false);
        assert_eq!(window.state(), last, "{sequence:?}");
        window.close();
    }
    Ok(())
}

/// Whether this is an environment where nobody honors maximize after showing (X11
/// without a window manager).
fn is_unresponsive(ui: &Ui) -> Result<bool> {
    let window = ui.window("応えるか", 320.0, 200.0)?;
    window.set_child(&ui.label("中身")?);
    window.show();
    window.set_state(WindowState::Maximized);
    spin_until(std::time::Duration::from_secs(2), || {
        toplevel_state(&window).contains(gtk::gdk::ToplevelState::MAXIMIZED)
    });
    let unresponsive = !toplevel_state(&window).contains(gtk::gdk::ToplevelState::MAXIMIZED);
    window.close();
    Ok(unresponsive)
}

/// `set_state` before showing is remembered and applied when shown.
///
/// GTK4 passes a maximize before showing as the state to present the surface with.
/// On Wayland the surface state right after showing is empty, so the requested
/// state is returned until the compositor's state arrives.
///
/// Unlike maximizing after showing, this is not SKIPped even on X11 without a
/// window manager (CI's Xvfb). GDK's X11 backend does not ask the window manager
/// to `maximize` a surface that has not been shown yet; it sets MAXIMIZED on the
/// surface state itself (and puts it in `_NET_WM_STATE` when showing). So it is
/// shown maximized even with nobody to respond (confirmed to pass on CI's Xvfb).
fn window_state_requested_before_show(ui: &Ui) -> Result<()> {
    let window = ui.window("表示前の状態", 320.0, 200.0)?;
    window.set_child(&ui.label("中身")?);
    window.set_state(WindowState::Maximized);
    assert_eq!(
        window.state(),
        WindowState::Maximized,
        "表示の前は頼んだ状態"
    );
    window.show();
    assert_eq!(
        window.state(),
        WindowState::Maximized,
        "コンポジタから届く前も頼んだ状態"
    );
    spin_until(std::time::Duration::from_secs(2), || {
        toplevel_state(&window).contains(gtk::gdk::ToplevelState::MAXIMIZED)
    });
    assert!(window.native_window().is_maximized());
    assert_eq!(window.state(), WindowState::Maximized);
    window.close();
    Ok(())
}

/// A placement applied before showing shows the window maximized, and while
/// maximized `placement` reports the size it returns to (GTK4's default size), which
/// `set_size` there also decides. The position is never kept.
///
/// Maximizing before showing is honored even without a window manager (see
/// [`window_state_requested_before_show`]); un-maximizing is SKIPped there.
fn window_placement_keeps_the_normal_size(ui: &Ui) -> Result<()> {
    use std::time::Duration;
    let saved =
        WindowPlacement::parse("size=380x260 position=150,170 state=maximized").expect("読める");
    let window = ui.window("保存する枠", 320.0, 200.0)?;
    window.set_child(&ui.label("中身")?);
    window.set_placement(&saved);
    let expected = WindowPlacement {
        position: None,
        ..saved
    };
    assert_eq!(window.placement(), expected, "表示の前");
    window.show();
    spin_until(Duration::from_secs(2), || {
        toplevel_state(&window).contains(gtk::gdk::ToplevelState::MAXIMIZED)
    });
    tick(&window.native_window());
    assert_eq!(window.state(), WindowState::Maximized);
    assert_eq!(window.placement(), expected, "最大化の間");

    window.set_size(400.0, 280.0);
    tick(&window.native_window());
    assert_eq!(window.state(), WindowState::Maximized, "最大化のまま");
    assert_eq!(window.placement().size, (400.0, 280.0));

    window.set_state(WindowState::Normal);
    spin_until(Duration::from_secs(3), || {
        window.state() == WindowState::Normal && window.size() == (400.0, 280.0)
    });
    if window.state() != WindowState::Normal {
        skip("ウィンドウマネージャが最大化の解除に応えない環境 (CI の Xvfb)");
        window.close();
        return Ok(());
    }
    assert_eq!(window.size(), (400.0, 280.0), "戻ると決めた大きさ");
    assert_eq!(
        window.placement(),
        WindowPlacement {
            size: (400.0, 280.0),
            position: None,
            state: WindowState::Normal,
        }
    );
    window.close();
    Ok(())
}

/// Minimizing round-trips with `state`, and `on_state_change` is called on each
/// change.
///
/// X11 window managers sometimes do not restore with `unminimize` alone, so when
/// restoring it is also asked to come to the front. On Wayland there is no way to
/// know about minimization, and on X11 without a window manager (CI's Xvfb) nobody
/// responds, so it is SKIPped.
fn window_minimize_round_trips(ui: &Ui) -> Result<()> {
    use std::time::Duration;
    if is_wayland() {
        skip("Wayland では最小化を知る仕組みが無い");
        return Ok(());
    }
    let window = ui.window("最小化", 320.0, 200.0)?;
    window.set_child(&ui.label("中身")?);
    let (seen, sink) = recorder();
    window.on_state_change(sink);
    window.show();
    spin_until(Duration::from_secs(1), || {
        !toplevel_state(&window).is_empty()
    });

    window.set_state(WindowState::Minimized);
    spin_until(Duration::from_secs(3), || {
        window.state() == WindowState::Minimized
    });
    if window.state() != WindowState::Minimized {
        skip("ウィンドウマネージャが最小化に応えない環境 (CI の Xvfb)");
        window.close();
        return Ok(());
    }
    window.set_state(WindowState::Normal);
    spin_until(Duration::from_secs(3), || {
        window.state() == WindowState::Normal
    });
    assert_eq!(window.state(), WindowState::Normal, "最小化から戻る");
    assert_eq!(
        *seen.borrow(),
        [WindowState::Minimized, WindowState::Normal]
    );
    window.close();
    Ok(())
}

/// A minimize requested before showing is ignored by window managers if requested
/// before the window is shown, so it is applied after showing, once the
/// compositor's first state arrives.
fn window_minimize_requested_before_show(ui: &Ui) -> Result<()> {
    use std::time::Duration;
    let window = ui.window("表示前の最小化", 320.0, 200.0)?;
    window.set_child(&ui.label("中身")?);
    let (seen, sink) = recorder();
    window.on_state_change(sink);
    window.set_state(WindowState::Minimized);
    window.show();
    assert_eq!(
        window.state(),
        WindowState::Minimized,
        "コンポジタから届く前も頼んだ状態"
    );
    if is_wayland() {
        skip("Wayland では最小化を知る仕組みが無い");
        window.close();
        return Ok(());
    }
    spin_until(Duration::from_secs(3), || {
        toplevel_state(&window).contains(gtk::gdk::ToplevelState::MINIMIZED)
    });
    if !toplevel_state(&window).contains(gtk::gdk::ToplevelState::MINIMIZED) {
        skip("ウィンドウマネージャが最小化に応えない環境 (CI の Xvfb)");
        window.close();
        return Ok(());
    }
    assert_eq!(window.state(), WindowState::Minimized);
    assert_eq!(*seen.borrow(), [WindowState::Minimized]);
    window.set_state(WindowState::Normal);
    spin_until(Duration::from_secs(3), || {
        window.state() == WindowState::Normal
    });
    assert_eq!(window.state(), WindowState::Normal, "最小化から戻る");
    window.close();
    Ok(())
}

/// The state the compositor delivered for the window's surface. Empty before
/// realize.
fn toplevel_state(window: &naui_gtk::Window) -> gtk::gdk::ToplevelState {
    window
        .native_window()
        .surface()
        .and_then(|surface| surface.dynamic_cast::<gtk::gdk::Toplevel>().ok())
        .map_or(gtk::gdk::ToplevelState::empty(), |toplevel| {
            toplevel.state()
        })
}

fn is_wayland() -> bool {
    gtk::gdk::Display::default()
        .is_some_and(|display| display.type_().name() == "GdkWaylandDisplay")
}

fn window_resizable_round_trips(ui: &Ui) -> Result<()> {
    let window = ui.window("大きさの固定", 320.0, 200.0)?;
    window.set_child(&ui.label("中身")?);
    assert!(window.is_resizable());
    window.set_resizable(false);
    assert!(!window.is_resizable());
    assert!(!window.native_window().is_resizable());
    window.show();
    // 固定しても、ui.window で指定した大きさで出る。
    spin_until(std::time::Duration::from_secs(2), || {
        window.size() == (320.0, 200.0)
    });
    assert_eq!(window.size(), (320.0, 200.0));
    window.set_resizable(true);
    assert!(window.is_resizable());
    assert!(window.native_window().is_resizable());
    window.close();
    Ok(())
}

fn window_reports_its_size(ui: &Ui) -> Result<()> {
    let window = ui.window("大きさ", 320.0, 200.0)?;
    window.set_child(&ui.label("中身")?);
    assert_eq!(window.size(), (320.0, 200.0), "表示前は指定した大きさ");
    let seen = Rc::new(RefCell::new(Vec::new()));
    window.on_resize({
        let seen = seen.clone();
        move |size| seen.borrow_mut().push(size)
    });
    window.set_size(400.0, 260.0);
    spin_until(std::time::Duration::from_secs(1), || {
        !seen.borrow().is_empty()
    });
    assert_eq!(
        seen.borrow().last(),
        Some(&(400.0, 260.0)),
        "表示前は指定した大きさが届く"
    );

    // 出ているウィンドウでは、中身へ配られた後の大きさが届く。指定した
    // 大きさはヘッダーバーを除いた中身の大きさなので、並べ終わった中身が
    // 指定どおりになる (並ぶ前の見積もりは、並んだ後に合わせ直す)。
    window.show();
    let settled = |seen: &RefCell<Vec<(f64, f64)>>| {
        let size = window.size();
        window.native_header_bar().height() > 0
            && seen.borrow().last() == Some(&size)
            && (size.0 - 400.0).abs() < 1.0
            && (size.1 - 260.0).abs() < 1.0
    };
    spin_until(std::time::Duration::from_secs(2), || settled(&seen));
    assert!(
        settled(&seen),
        "表示後の中身の大きさが届く: {:?}",
        seen.borrow()
    );
    let shown = window.size();

    // ウィンドウの大きさは同じでも、メニューバーを付けると中身は低くなる。
    let menu_bar = ui.menu_bar()?;
    menu_bar.set_menus(&[MenuSpec::new("ファイル", ["新規"])]);
    seen.borrow_mut().clear();
    window.set_menu_bar(&menu_bar);
    spin_until(std::time::Duration::from_secs(2), || {
        seen.borrow().last().is_some_and(|size| size.1 < shown.1)
    });
    let after = seen.borrow().clone();
    assert!(
        after
            .last()
            .is_some_and(|size| size.1 < shown.1 && size.0 == shown.0),
        "メニューバーの分だけ低くなったことが届く: {shown:?} → {after:?}"
    );
    assert_eq!(after.last(), Some(&window.size()), "届くのは今の大きさ");
    assert!(
        !after.contains(&shown),
        "変わる前の大きさは届かない: {after:?}"
    );

    seen.borrow_mut().clear();
    window.clear_menu_bar();
    spin_until(std::time::Duration::from_secs(2), || {
        seen.borrow().last() == Some(&shown)
    });
    assert_eq!(seen.borrow().last(), Some(&shown), "外すと元の高さに戻る");
    window.close();
    Ok(())
}

fn window_set_size_right_after_adding_bars(ui: &Ui) -> Result<()> {
    // The header (title and window buttons) can itself need more than 320px.
    // This test checks refitting after bar changes, above that native minimum.
    let window = ui.window("帯と大きさ", 400.0, 200.0)?;
    window.set_child(&ui.label("中身")?);
    window.show();
    spin_until(std::time::Duration::from_secs(2), || {
        window.native_header_bar().height() > 0 && window.size() == (400.0, 200.0)
    });
    assert_eq!(window.size(), (400.0, 200.0), "表示後の中身");

    // 付けたばかりのメニューバーとツールバーはまだ並んでいない。同じ周回の
    // `set_size` は、並び直した後の帯の高さで合わせ直される。
    let menu_bar = ui.menu_bar()?;
    menu_bar.set_menus(&[MenuSpec::new("ファイル", ["新規"])]);
    window.set_menu_bar(&menu_bar);
    let toolbar = ui.toolbar()?;
    toolbar.set_items(&[ToolbarItem::new(ToolbarIcon::New, "新規")]);
    window.set_toolbar(&toolbar);
    window.set_size(500.0, 300.0);
    let settled = || menu_bar.native_menu_bar().height() > 0 && window.size() == (500.0, 300.0);
    spin_until(std::time::Duration::from_secs(2), settled);
    assert!(
        settled(),
        "メニューバーとツールバーを付けても中身は指定どおり: {:?}",
        window.size()
    );

    // 外した直後も同じ。
    window.clear_menu_bar();
    window.clear_toolbar();
    window.set_size(420.0, 260.0);
    spin_until(std::time::Duration::from_secs(2), || {
        window.size() == (420.0, 260.0)
    });
    assert_eq!(
        window.size(),
        (420.0, 260.0),
        "帯を外しても中身は指定どおり"
    );
    window.close();
    Ok(())
}

fn window_close_request_can_keep_it_open(ui: &Ui) -> Result<()> {
    let window = ui.window("閉じる確認", 320.0, 200.0)?;
    window.set_child(&ui.label("中身")?);
    window.show();
    let asked = Rc::new(Cell::new(0));
    window.on_close_request({
        let asked = asked.clone();
        move || {
            asked.set(asked.get() + 1);
            naui_core::CloseResponse::KeepOpen
        }
    });
    // タイトルバーの閉じるボタンと同じく `close-request` を起こす。
    let stopped: bool = window.native_window().emit_by_name("close-request", &[]);
    assert!(stopped, "KeepOpen なら止める");
    assert_eq!(asked.get(), 1);

    // プログラムからの close は確かめない。
    window.close();
    assert_eq!(asked.get(), 1);
    Ok(())
}

fn read_only_and_min_size(ui: &Ui) -> Result<()> {
    let input = ui.text_input("ログ")?;
    input.set_read_only(true);
    assert!(input.is_read_only());
    let entry: gtk::Entry = input.native_widget().downcast().expect("GtkEntry");
    assert!(!entry.is_editable());
    let area = ui.text_area("ログ")?;
    area.set_read_only(true);
    let view: gtk::TextView = area.native_widget().downcast().expect("GtkTextView");
    assert!(!view.is_editable());
    assert!(!view.is_cursor_visible(), "カーソルを出さない");

    // 読み上げ名の中身は GTK4 から読み戻せないので、付いているかどうかを見る。
    // 文字のボタンは中のラベルへ `LABELLED_BY` を張っていて (張らない版の
    // GTK 4.14 もある)、そちらが `LABEL` より優先されるので、付けている間は
    // 外れていなければならない。
    // `GTK_A11Y=none` では読み上げの属性そのものが持たれないので確かめない。
    if std::env::var("GTK_A11Y").as_deref() != Ok("none") {
        let button = ui.button("⚙")?;
        let native = button.native_widget();
        let has_label =
            |w: &gtk::Widget| gtk::test_accessible_has_property(w, gtk::AccessibleProperty::Label);
        let labelled_by = |w: &gtk::Widget| {
            gtk::test_accessible_has_relation(w, gtk::AccessibleRelation::LabelledBy)
        };
        let from_child = labelled_by(&native);
        button.set_accessible_label(Some("設定"));
        assert!(has_label(&native));
        assert!(!labelled_by(&native), "付けた名前が中のラベルに隠されない");
        button.set_text("⚙ ");
        assert!(!labelled_by(&native), "文字を変えても付けた名前のまま");
        button.set_accessible_label(None);
        assert!(!has_label(&native));
        if from_child {
            assert!(labelled_by(&native), "外すと中のラベルの文字に戻る");
        }
        let search = ui.text_input("")?;
        search.set_accessible_label(Some("検索語"));
        assert!(has_label(&search.native_widget()));
    }

    let window = ui.window("最小", 320.0, 200.0)?;
    window.set_min_size(240.0, 120.0);
    window.set_child(&ui.label("中身")?);
    let content = window.native_window().content().expect("中身");
    let (min_width, _, _, _) = content.measure(gtk::Orientation::Horizontal, -1);
    assert!(min_width >= 240, "中身は 240 より狭くならない: {min_width}");
    Ok(())
}

/// The visible range (top, bottom) of the first scroll area inside the widget.
fn visible_range(widget: &gtk::Widget) -> (f64, f64) {
    fn find(widget: &gtk::Widget) -> Option<gtk::ScrolledWindow> {
        if let Some(scroller) = widget.downcast_ref::<gtk::ScrolledWindow>() {
            return Some(scroller.clone());
        }
        let mut child = widget.first_child();
        while let Some(current) = child {
            if let Some(found) = find(&current) {
                return Some(found);
            }
            child = current.next_sibling();
        }
        None
    }
    let scroller = scroller_of(widget)
        .or_else(|| find(widget))
        .expect("GtkScrolledWindow");
    let adjustment = scroller.vadjustment();
    (
        adjustment.value(),
        adjustment.value() + adjustment.page_size(),
    )
}

fn rows_can_be_scrolled_into_view(ui: &Ui) -> Result<()> {
    let window = ui.window("行へ送る", 480.0, 240.0)?;
    let root = ui.stack(Orientation::Horizontal)?;
    let labels: Vec<String> = (0..100).map(|i| format!("行 {i}")).collect();
    let list = ui.list()?;
    list.set_items(&ListItem::list(labels.iter().map(String::as_str)));
    list.set_sizing(Sizing::fixed(140.0, 200.0));
    let table = ui.table()?;
    table.set_columns(&[TableColumn::new("名前")]);
    table.set_rows(&TableRow::list(labels.iter().map(|label| [label.as_str()])));
    table.set_sizing(Sizing::fixed(140.0, 200.0));
    let tree = ui.tree()?;
    tree.set_items(&[
        TreeItem::new("上").children(TreeItem::list(labels.iter().map(String::as_str)))
    ]);
    tree.set_sizing(Sizing::fixed(140.0, 200.0));
    root.append(&list);
    root.append(&table);
    root.append(&tree);
    window.set_child(&root);
    window.show();
    for _ in 0..3 {
        tick(&bin_of(&root));
    }

    let list_widget = list.native_widget();
    assert_eq!(visible_range(&list_widget).0, 0.0, "最初は先頭");
    list.scroll_to_row(80);
    for _ in 0..2 {
        tick(&bin_of(&root));
    }
    let row = list_widget
        .downcast_ref::<gtk::ListBox>()
        .and_then(|list| list.row_at_index(80))
        .expect("80 行目");
    // ビューポート基準の位置は、見えている範囲の中での位置。
    let viewport = scroller_of(&list_widget)
        .and_then(|s| s.child())
        .expect("ビューポート");
    let bounds = row.compute_bounds(&viewport).expect("行の位置");
    let (top, bottom) = visible_range(&list_widget);
    assert!(top > 0.0, "下へ送られている");
    assert!(
        bounds.y() >= -1.0 && f64::from(bounds.y() + bounds.height()) <= bottom - top + 1.0,
        "80 行目が見える: {:?} / 見える高さ {}",
        (bounds.y(), bounds.height()),
        bottom - top
    );
    // もう一度送っても、見えていれば動かさない。
    list.scroll_to_row(80);
    assert_eq!(visible_range(&list_widget).0, top, "見えていれば動かさない");

    table.scroll_to_row(90);
    let (top, bottom) = visible_range(&table.native_widget());
    let height = table.row_height();
    assert!(
        90.0 * height >= top - 1.0 && 91.0 * height <= bottom + 1.0,
        "表の 90 行目が見える: {top}..{bottom} (行の高さ {height})"
    );

    tree.scroll_to_item(&[0, 70]);
    assert!(tree.is_expanded(&[0]), "祖先を開いてから送る");
    for _ in 0..2 {
        tick(&bin_of(&root));
    }
    for _ in 0..5 {
        tick(&bin_of(&root));
    }
    let (top, _) = visible_range(&tree.native_widget());
    assert!(top > 0.0, "下へ送られている: {top}");
    window.close();
    Ok(())
}

/// The `GtkDropTarget` naui put on a widget. Only one, even after setting the
/// closure again.
fn file_drop_target(widget: &gtk::Widget) -> gtk::DropTarget {
    let controllers = widget.observe_controllers();
    let targets: Vec<gtk::DropTarget> = (0..controllers.n_items())
        .filter_map(|i| controllers.item(i))
        .filter_map(|object| object.downcast::<gtk::DropTarget>().ok())
        .collect();
    assert_eq!(targets.len(), 1, "受け口は 1 つだけ");
    targets[0].clone()
}

/// Drops files as GTK does: the drop target's `drop` with a `GdkFileList`.
fn drop_files(target: &gtk::DropTarget, paths: &[&str]) -> bool {
    let files: Vec<gio::File> = paths.iter().map(gio::File::for_path).collect();
    let list = gtk::gdk::FileList::from_array(&files);
    // シグナルの引数は、値そのものを包んだ GValue。
    let value = glib::BoxedValue(list.to_value());
    target.emit_by_name::<bool>("drop", &[&value, &10.0f64, &10.0f64])
}

/// Files dropped from a file manager reach the closure as absolute paths. GTK itself
/// picks the innermost target under the pointer, so what is checked here is that
/// widgets and the window get a target for file lists that hands them over.
fn file_drop_reaches_the_drop_targets(ui: &Ui) -> Result<()> {
    let window = ui.window("ファイルの受け取り", 320.0, 200.0)?;
    let label = ui.label("ここへ落とす")?;
    window.set_child(&label);

    let seen = Rc::new(RefCell::new(Vec::new()));
    label.on_file_drop(|_| panic!("差し替えたので呼ばれない"));
    label.on_file_drop({
        let seen = seen.clone();
        move |files| seen.borrow_mut().extend(files.iter().cloned())
    });
    let bin = label.native_widget().parent().expect("SizeBin");
    let target = file_drop_target(&bin);
    assert!(target
        .formats()
        .is_some_and(|formats| formats.contains_type(gtk::gdk::FileList::static_type())));
    assert_eq!(target.actions(), gtk::gdk::DragAction::COPY);

    assert!(drop_files(
        &target,
        &["/tmp/naui 写真.png", "/tmp/メモ.txt"]
    ));
    let names: Vec<String> = seen.borrow().iter().map(|f| f.name().to_string()).collect();
    assert_eq!(names, ["naui 写真.png", "メモ.txt"]);
    assert_eq!(
        seen.borrow()[0].path().and_then(|p| p.to_str()),
        Some("/tmp/naui 写真.png"),
        "絶対パスを渡す"
    );

    let to_window = Rc::new(Cell::new(0));
    window.on_file_drop({
        let to_window = to_window.clone();
        move |files| to_window.set(to_window.get() + files.len())
    });
    let window_target = file_drop_target(window.native_window().upcast_ref());
    assert!(drop_files(&window_target, &["/tmp/別.txt"]));
    assert_eq!(to_window.get(), 1);
    assert_eq!(seen.borrow().len(), 2, "ほかの受け口へは届かない");
    window.close();
    Ok(())
}
