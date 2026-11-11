//! The body of the behavior checks. Compiled only on Windows (see
//! `tests/winui3.rs`).

use std::cell::{Cell, RefCell};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use naui_core::{
    Align, ButtonRole, Color, GridCell, LayoutDirection, MenuItem, MenuShortcut, MenuSpec,
    Orientation, Point, Rect, Result, RichText, ScrollPolicy, SidebarItem, SidebarSection, Sizing,
    TextColor, TextStyle, Theme, ToolbarIcon, WindowPlacement, WindowState, DEFAULT_SIDEBAR_WIDTH,
};
use naui_windows::{run_for_test, MenuEntry, Ui, Widget};

use crate::automation;
use naui_winui3::Microsoft::UI::Windowing::TitleBarTheme;
use naui_winui3::Microsoft::UI::Xaml::Controls::{
    Border, Button as XamlButton, Canvas as XamlCanvas, CheckBox as XamlCheckBox,
    ComboBox as XamlComboBox, Grid, Image as XamlImage, ListView, ListViewItem, MenuFlyoutItem,
    ProgressRing, ScrollViewer, Slider as XamlSlider, StackPanel, TextBlock, TextBox, ToggleSwitch,
};
use naui_winui3::Microsoft::UI::Xaml::Documents::{Hyperlink, Inline, Run};
use naui_winui3::Microsoft::UI::Xaml::Media::Imaging::{BitmapImage, SvgImageSource};
use naui_winui3::Microsoft::UI::Xaml::Media::SolidColorBrush;
use naui_winui3::Microsoft::UI::Xaml::{
    Application, FrameworkElement, GridUnitType, HorizontalAlignment, RoutedEventHandler, Style,
    UIElement, VerticalAlignment, Visibility,
};
use windows::Foundation::{IPropertyValue, PropertyValue};
use windows::Win32::Foundation::E_NOINTERFACE;
use windows_core::{Interface, HSTRING};

/// One test case.
type Case = (&'static str, fn(&Ui) -> Result<()>);

/// The order to run in. Cases that create windows are put last, so they run after
/// the other cases' results are in (in environments without a screen, this is where
/// things break first).
const CASES: &[Case] = &[
    ("ボタンのクリックがクロージャへ届く", button_click),
    (
        "ボタンのラベルがネイティブと往復する",
        button_label_round_trips,
    ),
    (
        "ボタンの役割が AccentButtonStyle へ写り、既定へ戻せる",
        button_role_maps_to_accent_style,
    ),
    (
        "チェックボックスが反転し新しい値を通知する",
        checkbox_toggle,
    ),
    (
        "スイッチが切り替わり新しい値を通知する",
        toggle_switches_and_notifies,
    ),
    ("文字列がネイティブと往復する (日本語含む)", text_round_trip),
    ("ラベルの文字列がネイティブと往復する", label_round_trips),
    (
        "ラベルの段階と役割が type ramp とテーマリソースへ写る",
        label_style_and_color_map_to_the_type_ramp,
    ),
    ("スライダーが範囲でクランプされる", slider_clamp),
    ("進捗バーが 0..1 に収まる", progress_clamp),
    (
        "コンボボックスの選択がネイティブと往復する",
        combo_box_selection_round_trips,
    ),
    (
        "ラジオグループの選択が 1 つだけ点いて通知する",
        radio_group_selects_one,
    ),
    (
        "ツリーの行が中身を行の幅いっぱいに置く",
        tree_rows_stretch_their_content,
    ),
    (
        "スタックの寄せ方が幅の Fill を壊さない",
        stack_alignment_keeps_fill_width,
    ),
    (
        "スタックの寄せ方がサイズ変更で維持される",
        stack_alignment_survives_sizing,
    ),
    (
        "スクロールの非スクロール軸が内容を広げる",
        scroll_stretches_non_scrolling_content,
    ),
    (
        "スクロール内容の Stretch がサイズ変更で維持される",
        scroll_stretch_survives_sizing,
    ),
    (
        "区切り線がテーマの線色の Border になり、引いた向きいっぱいに伸びる",
        separator_is_a_themed_border_line,
    ),
    (
        "グループが見出しとカードになり、カードの中に中身を広げる",
        group_puts_a_heading_above_a_card,
    ),
    (
        "親への追加失敗がレイアウト状態を汚さない",
        failed_parent_operations_keep_layout_state,
    ),
    (
        "Grid への追加失敗が既存の配置を汚さない",
        failed_grid_attach_keeps_existing_state,
    ),
    (
        "利用者の Tag をレイアウトが上書きしない",
        layout_preserves_user_tag,
    ),
    (
        "Stack から外すと配置を次の親へ戻せる",
        removing_stack_child_restores_alignment,
    ),
    ("スタックが子を生かし続ける", stack_keeps_children),
    (
        "スタックの主軸の Fill は Star の行、それ以外と隠した子は行を持たない",
        stack_gives_fill_children_star_tracks,
    ),
    (
        "スタックから外した子は行の番号を次の親へ持ち越さない",
        removing_stack_child_resets_its_track,
    ),
    (
        "スタックの向きを変えると最初からその向きで作ったのと同じ行と配置になる",
        stack_switches_orientation,
    ),
    (
        "Grid で結合していた子をスタックへ移すと結合が外れる",
        grid_span_does_not_follow_into_a_stack,
    ),
    (
        "Grid のマスの縦の寄せ方は set_sizing のあとも残り、外すと残らない",
        grid_cell_placement_follows_the_child,
    ),
    (
        "Grid から外した上限付き Fill の子もマスの配置を残さない",
        removing_capped_grid_child_resets_placement,
    ),
    (
        "リストに行の起動と文脈メニューの受け口を付けられる",
        list_accepts_row_activate_and_context_menu,
    ),
    (
        "行の多いリストが見えている行だけを組み立てる",
        list_builds_only_the_rows_it_needs,
    ),
    (
        "リストの組み立てる範囲が表示位置へ付いてくる",
        list_rows_follow_scrolling,
    ),
    (
        "行を組み立てる関数の中で行を差し替えても新しい行にそろう",
        list_builder_can_replace_the_rows,
    ),
    (
        "窓に残る行は作り直さず、中のコントロールがフォーカスを失わない",
        list_keeps_rows_that_stay_in_the_window,
    ),
    (
        "行を組み立てる関数の中の refresh は組み立てをやり直す",
        list_refresh_inside_the_builder_starts_over,
    ),
    (
        "選択を動かすキーはリストの選べない行を飛ばし、Ctrl は止まれる",
        list_keys_pass_over_rows_that_cannot_be_selected,
    ),
    (
        "Tab はリストの画面の中の行へ入り、次のキーで表示が飛ばない",
        list_tab_focuses_a_row_on_screen,
    ),
    (
        "組み立ての途中で失敗しても、残った行を管理し続ける",
        list_keeps_the_rows_a_failed_build_left,
    ),
    (
        "Page Up / Down はリストを 1 ページずつ動かす",
        list_page_keys_move_by_a_page,
    ),
    (
        "矢印キーはリストの無効の行を飛ばし、単一選択は Shift でも 1 行",
        list_arrow_keys_pass_over_disabled_rows,
    ),
    (
        "Shift の範囲選択は窓の外へ出た起点から伸びる",
        list_shift_range_keeps_its_anchor_outside_the_window,
    ),
    (
        "表の組み立ての途中で失敗しても、残った行を管理し続ける",
        table_keeps_the_rows_a_failed_build_left,
    ),
    (
        "Page Up / Down は表を 1 ページずつ動かす",
        table_page_keys_move_by_a_page,
    ),
    (
        "表の Shift の範囲選択は窓の外へ出た起点から伸びる",
        table_shift_range_keeps_its_anchor_outside_the_window,
    ),
    (
        "選択を動かすキーは表の選べない行を飛ばし、Ctrl は止まれる",
        table_keys_pass_over_rows_that_cannot_be_selected,
    ),
    (
        "リストのビルダーだけが知る長い選べない行もキーで越える",
        list_keys_pass_over_long_runs_only_the_builder_knows,
    ),
    (
        "表のビルダーだけが知る長い選べない行もキーで越える",
        table_keys_pass_over_long_runs_only_the_builder_knows,
    ),
    (
        "高さの違う行でもリストの窓の再計算は止まる",
        list_window_settles_with_rows_of_many_heights,
    ),
    (
        "高さの違う行でも表の窓の再計算は止まる",
        table_window_settles_with_rows_of_many_heights,
    ),
    (
        "高さの違うリストの行のフォーカスは実際の位置で決まる",
        list_focus_follows_the_real_place_of_rows,
    ),
    (
        "高さの違う表の行のフォーカスは実際の位置で決まる",
        table_focus_follows_the_real_place_of_rows,
    ),
    (
        "高さの違うリストの行でも Page は 1 画面ずつ動く",
        list_page_keys_go_by_the_real_height_of_rows,
    ),
    (
        "高さの違う表の行でも Page は 1 画面ずつ動く",
        table_page_keys_go_by_the_real_height_of_rows,
    ),
    (
        "スクロールのあとリストの行の余白を Shift クリックできる",
        list_shift_click_in_the_margin_after_a_scroll,
    ),
    (
        "スクロールのあと表の行の余白を Shift クリックできる",
        table_shift_click_in_the_margin_after_a_scroll,
    ),
    (
        "先に選べる行が無くても、リストの見えている行は組み立てたまま",
        list_keeps_its_rows_when_no_row_ahead_can_be_selected,
    ),
    (
        "先に選べる行が無くても、表の見えている行は組み立てたまま",
        table_keeps_its_rows_when_no_row_ahead_can_be_selected,
    ),
    (
        "表のフォーカスが画面の外にあっても表示が動かない",
        table_view_stays_when_the_focus_is_off_screen,
    ),
    (
        "表へ Tab で入ると画面の中の行へフォーカスが付く",
        table_tab_focuses_a_row_on_screen,
    ),
    (
        "表の行を組み立てる関数の中の refresh は組み立てをやり直す",
        table_refresh_inside_the_builder_starts_over,
    ),
    (
        "表の矢印キーは組み立てた行の端を越える",
        table_arrow_keys_go_past_the_rows_built,
    ),
    (
        "表のキーは自分でスクロールするかどうかで決まる",
        table_keys_depend_on_whether_it_scrolls,
    ),
    (
        "表の単一選択は修飾キーがあっても 1 行",
        table_single_selection_stays_one_row,
    ),
    (
        "表の窓を動かしても残る行は組み立て直さない",
        table_keeps_rows_that_stay_in_the_window,
    ),
    (
        "表の行を組み立てる関数の中で行を差し替えられる",
        table_builder_can_replace_the_rows,
    ),
    (
        "行の多いリストで指定より高い行があっても窓の位置がずれない",
        list_rows_taller_than_the_height_set_keep_the_window_right,
    ),
    (
        "行の多いリストで画面の外の選択が残る",
        list_keeps_the_selection_outside_the_window,
    ),
    (
        "Auto の高さのリストは全行を行ごとの高さで組み立てる",
        list_auto_height_builds_every_row,
    ),
    (
        "リストの行は全行の中の位置と全行数を支援技術へ伝える",
        list_rows_report_their_place_among_all_rows,
    ),
    (
        "隠すと Visibility が Collapsed になり、戻すと Visible",
        set_visible_maps_to_visibility,
    ),
    (
        "ツールチップが ToolTipService へ届き、None で外れる",
        tooltip_maps_to_tool_tip_service,
    ),
    (
        "ビジュアルツリーに載る前はフォーカスを移せない",
        request_focus_needs_the_visual_tree,
    ),
    (
        "文字入力系の 6 つに on_key_down を付けられる",
        text_widgets_accept_key_handlers,
    ),
    (
        "ファイルを落とせるよう窓の根が AllowDrop になる",
        file_drop_makes_the_root_a_drop_target,
    ),
    (
        "子のない窓でも on_file_drop で根が AllowDrop になる",
        file_drop_works_without_a_child,
    ),
    (
        "1 行入力・パスワード入力・数値入力に on_submit を付けられる",
        text_widgets_accept_submit_handlers,
    ),
    (
        "文字入力系の 6 つに on_focus_change を付けられる",
        text_widgets_accept_focus_handlers,
    ),
    (
        "捨てた入力欄の on_focus_change の通知先が解放される",
        focus_change_handler_is_freed_with_the_widget,
    ),
    (
        "通知のクロージャが弱参照で欄自身を触れ、外して手放せば解放される",
        weak_widget_breaks_the_callback_cycle,
    ),
    (
        "タイマーが DispatcherQueue で呼ばれ、止められる",
        timers_fire_on_the_dispatcher_queue,
    ),
    (
        "不確定の進捗は値を覚えたまま帯の往復に切り替わる",
        progress_bar_switches_to_indeterminate,
    ),
    (
        "スピナーは ProgressRing で、IsActive で止まり既定の大きさを保つ",
        spinner_is_a_progress_ring,
    ),
    (
        "スピナーは広い Grid の先頭に置かれ、サイズ変更に追従する",
        spinner_starts_at_grid_cell,
    ),
    (
        "スピナーは親 Stack の Fill と向きの変更に追従する",
        spinner_follows_stack_fill,
    ),
    (
        "ウィンドウ直下のスピナーは中身の領域に追従する",
        spinner_fills_window_content,
    ),
    (
        "ウィンドウへの追加失敗はスピナーの親を変えない",
        failed_window_attach_keeps_spinner_parent,
    ),
    (
        "ウィンドウの子の Auto と固定サイズは差し替え後も保たれる",
        window_child_sizing_survives_replacement,
    ),
    (
        "別のウィンドウへ移った子の配置は古い窓を閉じても残る",
        moved_window_child_keeps_new_parent,
    ),
    (
        "ラベルを選べるようにすると IsTextSelectionEnabled が立つ",
        label_can_be_selectable,
    ),
    ("リッチテキストが Inlines になる", rich_text_builds_inlines),
    (
        "リッチテキストは表示中と同じ内容なら何もしない",
        rich_text_same_content_keeps_the_inlines,
    ),
    (
        "リッチテキストからふつうの文字へ戻せる",
        rich_text_goes_back_to_plain_text,
    ),
    ("URL として読めないものは開かない", open_url_rejects_garbage),
    (
        "読み上げ名と読み取り専用がネイティブへ届く",
        accessible_label_and_read_only,
    ),
    (
        "一覧・表・ツリーの行へ送っても選択は変わらない",
        scroll_to_row_keeps_the_selection,
    ),
    (
        "アプリの言語がウィンドウの中身に付き、中の要素が継ぐ",
        app_language_is_set_on_the_window_content,
    ),
    (
        "右から左の配置がウィンドウの中身に付き、描画面・絵・数値入力・動画は左から右のまま",
        right_to_left_layout_is_set_on_the_window_content,
    ),
    (
        "ウィンドウの大きさはタイトルバーとメニューバーを除いた中身の行",
        window_size_is_the_content_row,
    ),
    (
        "短いラベルのスイッチとラジオが既定の最小幅を占めない",
        short_labels_drop_the_minimum_width,
    ),
    (
        "上限付き Fill の子はマスの横は先頭・縦は中央に置かれる",
        grid_places_a_capped_fill_child_like_an_auto_one,
    ),
    (
        "Grid に置いたあとで上限を変えても、外しても、付けても追従する",
        grid_follows_a_changed_maximum_width,
    ),
    (
        "スタックの主軸の Fill と Spacer が余りを受け取り、隠した子は間を空けない",
        stack_hands_leftover_space_to_fill_and_spacer,
    ),
    (
        "ui.window と set_size の大きさは size と同じ中身の大きさ",
        window_set_size_is_the_content_size,
    ),
    (
        "ウィンドウの位置が AppWindow と往復する",
        window_position_round_trips,
    ),
    (
        "タイトルバーのボタンがウィンドウのテーマに従う",
        title_bar_buttons_follow_the_theme,
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
        "中身の無いウィンドウも、変えた大きさを最大化の間に保存する",
        window_placement_without_child,
    ),
    (
        "大きさを変えられるかどうかが OverlappedPresenter と往復する",
        window_resizable_round_trips,
    ),
    (
        "表示する前に大きさを固定しても表示した後に効いている",
        window_resizable_requested_before_show,
    ),
    (
        "プロセスとウィンドウが Per-Monitor V2 で、倍率がウィンドウの DPI と合う",
        windows_are_per_monitor_v2_dpi_aware,
    ),
    (
        "TextArea の改行は \\n で読める",
        text_area_reports_newlines_as_lf,
    ),
    (
        "描画面の命令が Direct2D で画素になる",
        canvas_commands_become_pixels,
    ),
    (
        "表示した描画面は物理画素の Composition の面に描かれる",
        canvas_draws_onto_a_composition_surface,
    ),
    (
        "別のウィンドウへ移した描画面は移った先の XamlRoot を見る",
        canvas_follows_the_window_it_moves_to,
    ),
    (
        "捨てた描画面は XamlRoot.Changed の購読を外す",
        canvas_unsubscribes_from_the_root_when_dropped,
    ),
    (
        "ラベルの付くウィジェットが読み上げ名を持つ",
        widgets_expose_accessible_names,
    ),
    (
        "メニューバーの見出しと項目が MenuFlyout になる",
        menu_bar_menus_map_to_native,
    ),
    (
        "メニューバーの項目が経路で通知する",
        menu_bar_activation_notifies,
    ),
    (
        "メニューバーのサブメニューとチェックが MenuFlyoutSubItem と ToggleMenuFlyoutItem になる",
        menu_bar_submenus_and_checks_map_to_native,
    ),
    (
        "サイドバーが NavigationView の項目・見出し・区切りになる",
        sidebar_items_map_to_native,
    ),
    (
        "自動で畳むかを切り替えても、畳んだサイドバーは開かない",
        sidebar_auto_collapse_toggle_keeps_collapsed,
    ),
    (
        "しきい値を持つスタックが自分の幅で向きを切り替え、境目では遊びを持つ",
        stack_breakpoint_follows_its_width,
    ),
    (
        "一覧と詳細が TwoPaneView に載り、1 区画のときは戻るボタンで一覧へ戻る",
        adaptive_panes_follow_their_width,
    ),
    (
        "自動で畳んだサイドバーを set_collapsed で畳み直すと、広げても開かない",
        sidebar_set_collapsed_overrides_auto_expand,
    ),
    (
        "サイドバーの選択が通し番号で往復し、select だけが通知する",
        sidebar_selection_round_trips,
    ),
    (
        "日本語を含むパスの画像がパスでも file: URL でも読み込める",
        image_loads_from_japanese_path,
    ),
    (
        "画像の自然な大きさは画素の大きさで、Scroll の中でも幅に合わせて大きくならない",
        image_natural_size_is_its_pixels,
    ),
    (
        "画像がメモリ上のバイト列・画素・SVG を WinUI に読ませる",
        image_takes_bytes_pixels_and_svg,
    ),
    ("UTF-16 の SVG も SvgImageSource が開く", svg_opens_in_utf16),
    (
        "ネットワーク上の SVG・SVGZ を取ってきて出し、文書から大きさを求める",
        network_svg_takes_its_size,
    ),
    (
        "Fill の動画は Scroll の中でも表示領域の余りに収まり、縮めると付いていく",
        video_fits_the_leftover_and_follows_it,
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
        "画像の高さを Auto から Fill に変えると、画素の高さを求めるのをすぐやめる",
        image_from_auto_to_fill_drops_the_pixel_height,
    ),
    (
        "スクロールの位置を測り、送れる範囲に丸めて送る",
        scroll_measures_and_moves,
    ),
    (
        "別の Stack へ移した Scroll がスクロール位置を保つ",
        moved_scroll_keeps_its_position,
    ),
    (
        "足した直後の scroll_to_end が新しい末尾へ届く",
        scroll_to_end_after_appending,
    ),
    (
        "表示前に頼んだ scroll_to_end が表示後に効く",
        scroll_to_end_before_shown,
    ),
    (
        "スクロールの通知は位置が変わったときだけ届き、通知の中の送りも届く",
        scroll_notifies_only_moves,
    ),
    (
        "行の多い表は、スクロールした先の行を組み立てる",
        table_rows_follow_scrolling,
    ),
    (
        "表の行は全行の中の位置と全行数を支援技術へ伝える",
        table_rows_report_their_place_among_all_rows,
    ),
    (
        "行の多い表は、窓が動いてもフォーカスのある行を保つ",
        table_keeps_the_focused_row_across_rebuilds,
    ),
    (
        "行の多い表は、単一選択で Ctrl を押していても窓の外の行を残さない",
        table_single_selection_drops_rows_outside_the_window,
    ),
    (
        "編集できるコンボボックスの Enter で文字が残り、Esc で書いた値へ戻る",
        editable_combo_box_commits_and_reverts_text,
    ),
    (
        "OS の優先言語を読め、WM_SETTINGCHANGE の intl で変わると知らせる",
        preferred_languages_follow_settingchange,
    ),
    (
        "ポップオーバーの向きが Flyout の Placement へ届く",
        popover_position_reaches_the_flyout,
    ),
    (
        "画面に出ていないウィジェットにはポップオーバーを開かない",
        popover_needs_a_shown_widget,
    ),
    (
        "ポップオーバーが Flyout で開き、閉じても通知せず、利用者が閉じると通知する",
        popover_opens_and_notifies_only_user_closes,
    ),
    (
        "シートの設定が ContentDialog へ届く",
        sheet_configuration_reaches_the_content_dialog,
    ),
];

fn sheet_configuration_reaches_the_content_dialog(ui: &Ui) -> Result<()> {
    use naui_core::{DialogButtons, DialogResponse};
    use naui_winui3::Microsoft::UI::Xaml::Controls::ContentDialogButton;

    let sheet = ui.sheet("新しいフォルダ")?;
    let native = sheet.native_dialog();
    assert_eq!(sheet.title(), "新しいフォルダ");
    // null の見出しは、投影では Err として返る。
    assert!(native.Title().is_ok(), "見出しが出ること");
    sheet.set_title("");
    assert!(native.Title().is_err(), "空の見出しは出さないこと");

    let field = ui.text_input("名称未設定")?;
    sheet.set_child(&field);
    let content = native
        .Content()
        .expect("Content")
        .cast::<StackPanel>()
        .expect("中身を並べる StackPanel");
    let children = content.Children().expect("Children");
    let mut found = false;
    for i in 0..children.Size().expect("Size") {
        if children.GetAt(i).ok() == Some(field.native_element()) {
            found = true;
        }
    }
    assert!(found, "中身が ContentDialog の中に入ること");

    sheet.set_buttons(
        DialogButtons::new()
            .primary("作成")
            .secondary("あとで")
            .cancel("キャンセル"),
    );
    assert_eq!(native.PrimaryButtonText().expect("主").to_string(), "作成");
    assert_eq!(
        native.SecondaryButtonText().expect("副").to_string(),
        "あとで"
    );
    assert_eq!(
        native.CloseButtonText().expect("取り消し").to_string(),
        "キャンセル"
    );
    assert_eq!(
        native.DefaultButton().expect("既定"),
        ContentDialogButton::Primary
    );

    sheet.set_button_enabled(DialogResponse::Primary, false);
    sheet.set_button_enabled(DialogResponse::Cancel, false);
    assert!(!native.IsPrimaryButtonEnabled().expect("主の有効"));
    assert!(native.IsSecondaryButtonEnabled().expect("副の有効"));
    assert!(sheet.is_button_enabled(DialogResponse::Cancel));
    // ボタンを差し替えても、有効・無効は残る。
    sheet.set_buttons(DialogButtons::new().primary("保存").cancel("やめる"));
    assert!(!native.IsPrimaryButtonEnabled().expect("主の有効"));
    sheet.set_button_enabled(DialogResponse::Primary, true);
    assert!(native.IsPrimaryButtonEnabled().expect("主の有効"));

    // 主ボタンが無ければ、取り消しが既定のボタンになる。
    sheet.set_buttons(DialogButtons::new());
    assert_eq!(
        native.DefaultButton().expect("既定"),
        ContentDialogButton::Close
    );
    assert!(!sheet.is_open());
    sheet.close();
    Ok(())
}

fn popover_position_reaches_the_flyout(ui: &Ui) -> Result<()> {
    use naui_core::PopoverPosition;
    use naui_winui3::Microsoft::UI::Xaml::Controls::Primitives::FlyoutPlacementMode;

    let popover = ui.popover()?;
    let placement = || popover.native_flyout().Placement().expect("Placement");
    assert_eq!(popover.position(), PopoverPosition::Bottom);
    assert_eq!(placement(), FlyoutPlacementMode::Bottom);
    for (position, mode) in [
        (PopoverPosition::Top, FlyoutPlacementMode::Top),
        (PopoverPosition::Left, FlyoutPlacementMode::Left),
        (PopoverPosition::Right, FlyoutPlacementMode::Right),
        (PopoverPosition::Bottom, FlyoutPlacementMode::Bottom),
    ] {
        popover.set_position(position);
        assert_eq!(popover.position(), position);
        assert_eq!(placement(), mode);
    }
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
    assert!(!popover.native_flyout().IsOpen().unwrap_or(true));
    Ok(())
}

/// The maximum time to wait for a window's elements to load, and for a flyout to
/// open or close.
const POPOVER_TIMEOUT: Duration = Duration::from_secs(3);

fn popover_opens_and_notifies_only_user_closes(ui: &Ui) -> Result<()> {
    use naui_core::PopoverPosition;
    use naui_winui3::Microsoft::UI::Xaml::Controls::Primitives::FlyoutPlacementMode;

    let window = ui.window("ポップオーバー", 360.0, 240.0)?;
    let root = ui.stack(Orientation::Horizontal)?;
    let first = ui.button("設定")?;
    let second = ui.button("表示")?;
    root.append(&first);
    root.append(&second);
    window.set_child(&root);
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        window.show();
        let button = native::<FrameworkElement>(&first);
        pump_until(POPOVER_TIMEOUT, || button.IsLoaded().unwrap_or(false));
        if !button.IsLoaded().unwrap_or(false) {
            println!("     (ウィンドウの要素が読み込まれない環境なので、確かめずに通す)");
            return;
        }

        let popover = ui.popover().expect("ポップオーバー");
        let body = ui.label("中身").expect("ラベル");
        popover.set_child(&body);
        let closed = Rc::new(Cell::new(0));
        popover.on_close({
            let closed = closed.clone();
            move || closed.set(closed.get() + 1)
        });
        let flyout = popover.native_flyout();
        assert_eq!(
            flyout.Content().ok(),
            Some(body.native_element()),
            "中身が Flyout の Content になること"
        );
        let is_open = || flyout.IsOpen().unwrap_or(false);
        let shown_at = |widget: &dyn Widget| {
            is_open() && flyout.Target().ok().as_ref() == Some(&native::<FrameworkElement>(widget))
        };
        // Flyout は閉じ終えてから `Closed` を出す。その知らせと、そのあとに出し直す分を
        // 待つ。
        let settle = || pump_until(Duration::from_millis(800), || false);

        popover.open(&first);
        assert!(popover.is_open());
        pump_until(POPOVER_TIMEOUT, || shown_at(&first));
        assert!(shown_at(&first), "Flyout が開くこと");

        // 開いたまま別のウィジェットへ移しても、閉じたことにはしない。
        popover.open(&second);
        assert!(popover.is_open());
        pump_until(POPOVER_TIMEOUT, || shown_at(&second));
        settle();
        assert!(popover.is_open(), "移したあとも開いていること");
        assert!(shown_at(&second), "移した先の横に出ること");
        assert_eq!(closed.get(), 0, "移すだけなら通知しないこと");

        // 同じウィジェットへもう一度開いても、閉じずにそのまま。
        popover.open(&second);
        assert!(shown_at(&second), "同じ場所なら閉じないこと");

        // 開いている間に向きを変えると、新しい向きで出し直す。
        popover.set_position(PopoverPosition::Right);
        pump_until(POPOVER_TIMEOUT, || {
            flyout.Placement().ok() == Some(FlyoutPlacementMode::Right) && shown_at(&second)
        });
        settle();
        assert!(popover.is_open());
        assert!(shown_at(&second), "向きを変えても開いていること");
        assert_eq!(closed.get(), 0, "向きを変えるだけなら通知しないこと");

        popover.close();
        assert!(!popover.is_open());
        assert!(!is_open(), "Flyout が閉じること");
        // 閉じ終える前に開き直しても、閉じた知らせを利用者が閉じたものと取り違えない。
        popover.open(&first);
        pump_until(POPOVER_TIMEOUT, || shown_at(&first));
        settle();
        assert!(shown_at(&first), "閉じかけから開き直せること");
        assert_eq!(closed.get(), 0, "アプリが閉じたときは通知しないこと");

        // 移している途中で閉じたら、移した先で開き直さない。
        popover.open(&second);
        popover.close();
        settle();
        assert!(!popover.is_open());
        assert!(!is_open(), "移す途中で閉じたら開き直さないこと");
        assert_eq!(closed.get(), 0);

        // 移している途中で利用者が閉じたら、開き直さずに 1 度だけ通知する。
        popover.open(&first);
        pump_until(POPOVER_TIMEOUT, || shown_at(&first));
        popover.open(&second);
        popover.dismiss();
        settle();
        assert!(!popover.is_open());
        assert!(!is_open(), "移す途中で利用者が閉じたら開き直さないこと");
        assert_eq!(closed.get(), 1, "移す途中で利用者が閉じたら通知すること");

        popover.open(&first);
        pump_until(POPOVER_TIMEOUT, || shown_at(&first));
        popover.dismiss();
        pump_until(POPOVER_TIMEOUT, || closed.get() > 1);
        settle();
        assert_eq!(closed.get(), 2, "利用者が閉じたら 1 度だけ通知すること");
        assert!(!popover.is_open());
        assert!(!is_open());

        // `on_close` の中から同じウィジェットへ開き直すと、閉じ終えたあとで出る。
        let reopened = Rc::new(Cell::new(false));
        popover.on_close({
            let popover = popover.clone();
            let first = first.clone();
            let reopened = reopened.clone();
            move || {
                if !reopened.replace(true) {
                    popover.open(&first);
                }
            }
        });
        popover.open(&first);
        pump_until(POPOVER_TIMEOUT, || shown_at(&first));
        popover.dismiss();
        pump_until(POPOVER_TIMEOUT, || reopened.get());
        pump_until(POPOVER_TIMEOUT, || shown_at(&first));
        settle();
        assert!(reopened.get());
        assert!(popover.is_open());
        assert!(shown_at(&first), "on_close から開き直せること");
        // 開き直したあとも、ふつうに閉じて開ける。
        popover.close();
        settle();
        assert!(!is_open(), "開き直したあとも閉じられること");
        popover.open(&second);
        pump_until(POPOVER_TIMEOUT, || shown_at(&second));
        assert!(shown_at(&second), "開き直したあとも開けること");
        popover.close();
        settle();
    }));
    window.close();
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
    Ok(())
}

/// Work to verify later. Called after the event loop has gone around once.
type Deferred = Box<dyn FnOnce() -> Result<()>>;

/// Cases whose setup and verification are one loop iteration apart.
///
/// Showing a window does not set `Window.Visible` on the spot. Whether it appeared
/// can be seen only after WinUI has gone through a round of messages. So the setup
/// shows it, and verification happens after the loop has turned.
type AsyncCase = (&'static str, fn(&Ui) -> Result<Deferred>);

const ASYNC_CASES: &[AsyncCase] = &[
    ("ウィンドウが出て、閉じると消える", window_lifecycle),
    (
        "トーストはメニューバーの行ではなく中身の行に重なる",
        toast_overlays_the_content_below_the_menu_bar,
    ),
    (
        "サイドバーは中身の行に入り、子をその右の区画へ移す",
        sidebar_takes_the_content_row,
    ),
];

/// If it enters `Application::Start` and never returns, CI stays stuck until it gives
/// up. After allowing time for every case, it always ends. The cases sending real input
/// wait for it to arrive, and together take a few minutes.
const WATCHDOG: Duration = Duration::from_secs(600);

pub(crate) fn run() {
    std::thread::spawn(|| {
        std::thread::sleep(WATCHDOG);
        eprintln!(
            "\nnaui: {} 秒たってもアプリが終わりませんでした",
            WATCHDOG.as_secs()
        );
        std::process::exit(1);
    });

    let failed = Arc::new(AtomicUsize::new(0));
    let counter = failed.clone();
    let outcome = run_for_test(move |ui| {
        for (name, case) in CASES {
            if let Ok(only) = std::env::var("NAUI_ONLY") {
                if !name.contains(only.as_str()) {
                    continue;
                }
            }
            report(name, catch_unwind(AssertUnwindSafe(|| case(ui))), &counter);
        }

        // 2 段構えのケースは、まず仕込みだけ済ませる。
        let mut deferred = Vec::new();
        for (name, setup) in ASYNC_CASES {
            if std::env::var("NAUI_ONLY").is_ok() {
                continue;
            }
            match catch_unwind(AssertUnwindSafe(|| setup(ui))) {
                Ok(Ok(check)) => deferred.push((*name, check)),
                other => report(name, other.map(|result| result.map(|_| ())), &counter),
            }
        }

        // ここで積んだ仕事は、WinUI が仕込みの間に積んだものより後ろに並ぶ。
        // 同じ DispatcherQueue なので、一巡してから呼ばれる。`run_for_test`
        // が畳む仕事を積むのはこの後なので、集計まで済ませてから終わる。
        let mut deferred = Some(deferred);
        let later = counter.clone();
        let checks = ui.tasks().channel(move |()| {
            let Some(deferred) = deferred.take() else {
                return;
            };
            for (name, check) in deferred {
                report(name, catch_unwind(AssertUnwindSafe(check)), &later);
            }
            // 結果はここで出し切る。この先はアプリの後片づけなので、そこで
            // 転んでも何が通って何が落ちたかは読める。
            let total = CASES.len() + ASYNC_CASES.len();
            println!(
                "\n{total} 件中 {} 件成功",
                total - later.load(Ordering::Relaxed)
            );
        });
        checks.send(())?;
        Ok(())
    });

    if let Err(error) = outcome {
        eprintln!("アプリを起こせませんでした: {error}");
        std::process::exit(1);
    }
    if failed.load(Ordering::Relaxed) > 0 {
        std::process::exit(1);
    }
}

/// Reports one result. Counts it if it failed.
///
/// Cases' `assert!`s are caught with `catch_unwind`. Even if one fails, the rest still
/// run (the app cannot be restarted, so stopping would leave everything after it
/// unrun).
fn report(name: &str, outcome: std::thread::Result<Result<()>>, failed: &Arc<AtomicUsize>) {
    match outcome {
        Ok(Ok(())) => println!("ok   ... {name}"),
        Ok(Err(error)) => {
            println!("FAIL ... {name}: {error}");
            failed.fetch_add(1, Ordering::Relaxed);
        }
        Err(_) => {
            println!("FAIL ... {name}");
            failed.fetch_add(1, Ordering::Relaxed);
        }
    }
}

// ------------------------------------------------------------------ 補助

/// Clicks through the same path as an actual click (the Invoke pattern).
///
/// The details of the path are in [`crate::automation`]. It goes through the peer
/// WinUI provides per control, so it is WinUI itself that raises `Click`.
fn invoke(widget: &dyn Widget) {
    automation::invoke(&widget.native_element());
}

/// Toggles on/off through the same path as an actual click (the Toggle pattern).
fn toggle(widget: &dyn Widget) {
    automation::toggle(&widget.native_element());
}

/// Selects an item through the same path as an actual selection (the SelectionItem
/// pattern).
fn select(element: &UIElement) {
    automation::select(element);
}

/// The name passed to assistive technology.
fn accessible_name(widget: &dyn Widget) -> String {
    automation::accessible_name(&widget.native_element())
}

/// Takes it out as a native control. Fails the test if the type differs.
fn native<T: Interface>(widget: &dyn Widget) -> T {
    widget
        .native_element()
        .cast::<T>()
        .expect("期待した WinUI のコントロールではありません")
}

// ------------------------------------------------------------ 各ケース

fn button_click(ui: &Ui) -> Result<()> {
    let button = ui.button("押す")?;

    let first = Rc::new(Cell::new(0));
    let counter = first.clone();
    button.on_click(move || counter.set(counter.get() + 1));
    invoke(&button);
    assert_eq!(first.get(), 1, "クリックが 1 回だけ届くこと");

    // 付け替えたら、古い通知先は外れる。
    let second = Rc::new(Cell::new(0));
    let counter = second.clone();
    button.on_click(move || counter.set(counter.get() + 1));
    invoke(&button);
    assert_eq!(first.get(), 1, "古い通知先へは届かないこと");
    assert_eq!(second.get(), 1, "新しい通知先へ届くこと");
    Ok(())
}

fn button_label_round_trips(ui: &Ui) -> Result<()> {
    let button = ui.button("はじめ")?;
    assert_eq!(native_button_text(&button), "はじめ");
    button.set_text("あと");
    assert_eq!(native_button_text(&button), "あと");
    Ok(())
}

/// A button's content is a `TextBlock`. Reads the native-side text.
fn native_button_text(button: &naui_windows::Button) -> String {
    native::<XamlButton>(button)
        .Content()
        .expect("Button の中身")
        .cast::<TextBlock>()
        .expect("Button の中身は TextBlock")
        .Text()
        .expect("TextBlock の文字列")
        .to_string()
}

fn checkbox_toggle(ui: &Ui) -> Result<()> {
    let checkbox = ui.checkbox("同意する")?;
    assert!(!checkbox.is_checked(), "はじめは切れていること");

    let seen = Rc::new(RefCell::new(Vec::new()));
    let sink = seen.clone();
    checkbox.on_toggle(move |checked| sink.borrow_mut().push(checked));

    toggle(&checkbox);
    assert!(checkbox.is_checked(), "反転すること");
    assert!(
        native::<XamlCheckBox>(&checkbox)
            .IsChecked()
            .and_then(|value| value.Value())
            .unwrap_or(false),
        "ネイティブの CheckBox も点いていること"
    );

    toggle(&checkbox);
    assert!(!checkbox.is_checked(), "もう一度で戻ること");
    assert_eq!(
        seen.borrow().as_slice(),
        [true, false].as_slice(),
        "変わったあとの値が順に届くこと"
    );

    // プログラムからの `set_checked` は通知しない (4 環境で同じ約束)。
    checkbox.set_checked(true);
    assert!(checkbox.is_checked(), "点くこと");
    checkbox.set_checked(false);
    assert_eq!(
        seen.borrow().len(),
        2,
        "set_checked では on_toggle を呼ばないこと"
    );
    Ok(())
}

/// One pixel of what `pixels_for_test` returns: (blue, green, red, alpha),
/// premultiplied.
fn pixel(pixels: &(u32, u32, Vec<u8>), x: u32, y: u32) -> (u8, u8, u8, u8) {
    let (width, _, bytes) = pixels;
    let at = ((y * width + x) * 4) as usize;
    (bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3])
}

/// The columns holding any visible pixel within rows `top..bottom`.
fn inked_columns(pixels: &(u32, u32, Vec<u8>), top: u32, bottom: u32) -> Option<(u32, u32)> {
    let (width, _, _) = pixels;
    let inked: Vec<u32> = (0..*width)
        .filter(|x| (top..bottom).any(|y| pixel(pixels, *x, y).3 > 0))
        .collect();
    Some((*inked.first()?, *inked.last()?))
}

/// The drawing surface replays the `on_draw` commands with Direct2D. It is not on
/// screen here, so the same replay is drawn into an offscreen bitmap and its pixels
/// are checked: fills keep their color and opacity, dashes leave gaps, text is drawn
/// and centered on its point, and the display scale multiplies the pixels.
fn canvas_commands_become_pixels(ui: &Ui) -> Result<()> {
    let canvas = ui.canvas()?;
    assert!(
        native::<Grid>(&canvas).Background().is_ok(),
        "土台の Grid に当たり判定のための背景があること"
    );
    let calls = Rc::new(Cell::new(0));
    canvas.on_draw({
        let calls = calls.clone();
        move |painter| {
            calls.set(calls.get() + 1);
            painter.fill_rect(Rect::new(10.0, 10.0, 40.0, 20.0), Color::rgb(0xff, 0, 0));
            painter.set_opacity(0.5);
            painter.fill_rect(Rect::new(60.0, 10.0, 20.0, 20.0), Color::rgb(0, 0, 0xff));
            painter.set_opacity(1.0);
            // 長方形でない塗り (ジオメトリを作る側) も描けること。
            painter.fill_polygon(
                &[
                    Point::new(100.0, 10.0),
                    Point::new(110.0, 30.0),
                    Point::new(90.0, 30.0),
                ],
                Color::rgb(0, 0x80, 0),
            );
            painter.set_dash(&[4.0, 4.0]);
            painter.line(
                Point::new(0.0, 40.5),
                Point::new(120.0, 40.5),
                Color::BLACK,
                1.0,
            );
            painter.set_dash(&[]);
            painter.set_text_align(Align::Center);
            painter.text("naui <&>", Point::new(60.0, 50.0), 14.0, Color::BLACK);
            // `{` で始まる文字も文字のまま描けること (XAML で描いていたころは
            // マークアップ拡張と読まれて面ごと壊れた)。
            painter.text("{Binding}", Point::new(60.0, 64.0), 10.0, Color::BLACK);
        }
    });

    let Some(pixels) = canvas.pixels_for_test(120.0, 80.0, 1.0) else {
        println!("     (Direct2D を使えない環境なので、確かめずに通す)");
        return Ok(());
    };
    assert_eq!(calls.get(), 1, "面の大きさで on_draw が呼ばれること");
    assert_eq!((pixels.0, pixels.1), (120, 80), "面の大きさの画素");
    assert_eq!(pixel(&pixels, 30, 20), (0, 0, 0xff, 0xff), "不透明の赤");
    assert_eq!(
        pixel(&pixels, 5, 5),
        (0, 0, 0, 0),
        "描いていないところは透明"
    );
    let (blue, _, _, alpha) = pixel(&pixels, 70, 20);
    assert!(
        (126..=129).contains(&alpha) && blue == alpha,
        "半透明の青は乗算済みで半分: {:?}",
        pixel(&pixels, 70, 20)
    );
    assert_eq!(pixel(&pixels, 100, 25).1, 0x80, "三角形の中は緑");
    assert_eq!(pixel(&pixels, 2, 40).3, 0xff, "破線の線のところ");
    assert_eq!(pixel(&pixels, 6, 40).3, 0, "破線の隙間");
    let (left, right) = inked_columns(&pixels, 50, 64).expect("文字が描かれること");
    let center = f64::from(left + right) / 2.0;
    assert!(
        (center - 60.0).abs() <= 2.0 && right - left > 20,
        "中央揃えの文字は点を中心に置かれる: {left}..{right}"
    );
    assert!(
        inked_columns(&pixels, 64, 80).is_some(),
        "`{{` で始まる文字も描かれること"
    );

    let doubled = canvas
        .pixels_for_test(120.0, 80.0, 2.0)
        .expect("倍率 2 でも描けること");
    assert_eq!(
        (doubled.0, doubled.1),
        (240, 160),
        "倍率の分だけ画素が増える"
    );
    assert_eq!(
        pixel(&doubled, 60, 40),
        (0, 0, 0xff, 0xff),
        "座標も倍率で伸びる"
    );
    assert_eq!(pixel(&doubled, 15, 15), (0, 0, 0, 0));

    // 機器が持てる大きさ (ここでは 16384 画素) を超える面は、倍率を下げて
    // 全体を描く。右端の印まで描けていること。
    let wide = ui.canvas()?;
    wide.on_draw(|painter| {
        let right = painter.width();
        painter.fill_rect(
            Rect::new(right - 1000.0, 0.0, 1000.0, 10.0),
            Color::rgb(0xff, 0, 0),
        );
    });
    let wide = wide
        .pixels_for_test(20000.0, 10.0, 1.0)
        .expect("大きすぎる面も描けること");
    assert!(wide.0 <= 16384, "面は機器の上限に収める: {} 画素", wide.0);
    assert_eq!(
        pixel(&wide, wide.0 - 10, 5),
        (0, 0, 0xff, 0xff),
        "倍率を下げて右端まで描くこと"
    );
    Ok(())
}

/// Shown on screen, the surface is a `SpriteVisual` placed over the `Grid` whose
/// brush shows a drawing surface covering the surface in physical pixels.
fn canvas_draws_onto_a_composition_surface(ui: &Ui) -> Result<()> {
    use naui_winui3::Microsoft::UI::Composition::{
        CompositionDrawingSurface, CompositionSurfaceBrush, SpriteVisual,
    };
    use naui_winui3::Microsoft::UI::Xaml::Hosting::ElementCompositionPreview;

    let window = ui.window("描画面", 200.0, 150.0)?;
    let canvas = ui.canvas()?;
    canvas.set_sizing(Sizing::fixed(120.0, 80.0));
    let calls = Rc::new(Cell::new(0));
    canvas.on_draw({
        let calls = calls.clone();
        move |painter| {
            calls.set(calls.get() + 1);
            painter.fill_rect(painter.bounds(), Color::rgb(0, 0x80, 0xff));
        }
    });
    window.set_child(&canvas);
    window.show();
    let element = native::<UIElement>(&canvas);
    let visual = || {
        ElementCompositionPreview::GetElementChildVisual(&element)
            .ok()
            .and_then(|visual| visual.cast::<SpriteVisual>().ok())
    };
    pump_until(CANVAS_TIMEOUT, || calls.get() > 0 && visual().is_some());
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        if canvas.size().0 <= 0.0 {
            println!("     (面に実寸が付かない環境なので、確かめずに通す)");
            return;
        }
        let visual = visual().expect("面の上に SpriteVisual が置かれること");
        let scale = element
            .XamlRoot()
            .and_then(|root| root.RasterizationScale())
            .expect("倍率");
        let surface: CompositionDrawingSurface = visual
            .Brush()
            .and_then(|brush| brush.cast::<CompositionSurfaceBrush>())
            .and_then(|brush| brush.Surface())
            .and_then(|surface| surface.cast())
            .expect("ブラシが描画用の面を映すこと");
        let pixels = surface.SizeInt32().expect("面の画素数");
        assert_eq!(
            (pixels.Width, pixels.Height),
            ((120.0 * scale).ceil() as i32, (80.0 * scale).ceil() as i32),
            "面は物理画素で作る (倍率 {scale})"
        );
        let size = visual.Size().expect("見た目の大きさ");
        assert!(
            (f64::from(size.X) - f64::from(pixels.Width) / scale).abs() < 0.01
                && (f64::from(size.Y) - f64::from(pixels.Height) / scale).abs() < 0.01,
            "見た目は面の 1 画素が画面の 1 画素になる大きさ: {size:?}"
        );

        // GPU のリセットを Direct3D の知らせで真似る。アプリが描き直しを頼まなくても、
        // デバイスを作り直して面を描き直すこと (動かないグラフが消えたままにならない)。
        let before = calls.get();
        if !canvas.signal_device_removed_for_test() {
            println!("     (デバイスの除去を知らせられない環境なので、復旧は確かめない)");
            return;
        }
        pump_until(CANVAS_TIMEOUT, || calls.get() > before);
        assert!(
            calls.get() > before,
            "デバイスの除去が知らされたら、頼まれなくても描き直すこと"
        );
        let after = surface.SizeInt32().expect("復旧後の面の画素数");
        assert_eq!(
            (after.Width, after.Height),
            (pixels.Width, pixels.Height),
            "復旧後も同じ面に描くこと"
        );
        assert!(
            !canvas.signal_device_removed_for_test() || {
                // 作り直したデバイスにも知らせが付いていること。二度目も描き直す。
                let second = calls.get();
                pump_until(CANVAS_TIMEOUT, || calls.get() > second);
                calls.get() > second
            },
            "作り直したデバイスの除去でも描き直すこと"
        );
    }));
    window.close();
    match outcome {
        Ok(()) => Ok(()),
        Err(panic) => std::panic::resume_unwind(panic),
    }
}

/// Moved to another window, a surface watches the new window's `XamlRoot.Changed`
/// and no longer the old one's.
///
/// Both `SizeChanged` (which has so far always come with the move) and `Loaded`
/// lead there, so this does not tell the two apart. Redrawing at a different scale
/// needs two displays with different scales and is not checked.
fn canvas_follows_the_window_it_moves_to(ui: &Ui) -> Result<()> {
    let first = ui.window("描画面の移動元", 200.0, 150.0)?;
    let second = ui.window("描画面の移動先", 200.0, 150.0)?;
    first.show();
    second.show();
    let baseline = naui_windows::Canvas::root_watches_for_test();
    let outcome = catch_unwind(AssertUnwindSafe(|| -> Result<()> {
        let canvas = ui.canvas()?;
        canvas.set_sizing(Sizing::fixed(80.0, 60.0));
        let drawn = Rc::new(Cell::new(false));
        canvas.on_draw({
            let drawn = drawn.clone();
            move |painter| {
                drawn.set(true);
                painter.fill_rect(painter.bounds(), Color::BLACK);
            }
        });
        first.set_child(&canvas);
        pump_until(CANVAS_TIMEOUT, || drawn.get());
        if !drawn.get() {
            println!("     (面に実寸が付かない環境なので、確かめずに通す)");
            return Ok(());
        }
        assert!(
            canvas.watches_current_root_for_test(),
            "移す前の窓を見ている"
        );

        first.set_child(&ui.label("空")?);
        second.set_child(&canvas);
        pump_until(CANVAS_TIMEOUT, || canvas.watches_current_root_for_test());
        assert!(
            canvas.watches_current_root_for_test(),
            "移った先の窓の XamlRoot.Changed を購読し直すこと"
        );
        assert_eq!(
            naui_windows::Canvas::root_watches_for_test(),
            baseline + 1,
            "移す前の窓の購読は外すこと"
        );
        Ok(())
    }));
    first.close();
    second.close();
    match outcome {
        Ok(result) => result,
        Err(panic) => std::panic::resume_unwind(panic),
    }
}

/// A surface watches its window's `XamlRoot.Changed` to notice scale changes. The
/// `XamlRoot` outlives it, so dropping the surface must unsubscribe; otherwise
/// remaking surfaces in one window piles up delegates.
fn canvas_unsubscribes_from_the_root_when_dropped(ui: &Ui) -> Result<()> {
    let window = ui.window("描画面の作り直し", 200.0, 150.0)?;
    window.show();
    let baseline = naui_windows::Canvas::root_watches_for_test();
    let outcome = catch_unwind(AssertUnwindSafe(|| -> Result<()> {
        for round in 0..3 {
            let canvas = ui.canvas()?;
            canvas.set_sizing(Sizing::fixed(80.0, 60.0));
            let drawn = Rc::new(Cell::new(false));
            canvas.on_draw({
                let drawn = drawn.clone();
                move |painter| {
                    drawn.set(true);
                    painter.fill_rect(painter.bounds(), Color::BLACK);
                }
            });
            window.set_child(&canvas);
            pump_until(CANVAS_TIMEOUT, || drawn.get());
            if !drawn.get() {
                println!("     (面に実寸が付かない環境なので、確かめずに通す)");
                return Ok(());
            }
            assert_eq!(
                naui_windows::Canvas::root_watches_for_test(),
                baseline + 1,
                "{round} 回目: 描いた面は XamlRoot.Changed を 1 つだけ購読する"
            );
            window.set_child(&ui.label("空")?);
            drop(canvas);
            assert_eq!(
                naui_windows::Canvas::root_watches_for_test(),
                baseline,
                "{round} 回目: 捨てた面は購読を外す"
            );
        }
        Ok(())
    }));
    window.close();
    match outcome {
        Ok(result) => result,
        Err(panic) => std::panic::resume_unwind(panic),
    }
}

/// The maximum time to wait for a shown surface to be drawn.
const CANVAS_TIMEOUT: Duration = Duration::from_secs(3);

fn toggle_switches_and_notifies(ui: &Ui) -> Result<()> {
    let switch = ui.toggle("通知")?;
    assert!(!switch.is_on(), "はじめは切れていること");

    let seen = Rc::new(RefCell::new(Vec::new()));
    let sink = seen.clone();
    switch.on_toggle(move |on| sink.borrow_mut().push(on));

    toggle(&switch);
    assert!(switch.is_on(), "入ること");
    assert!(
        native::<ToggleSwitch>(&switch).IsOn().unwrap_or(false),
        "ネイティブの ToggleSwitch も入っていること"
    );
    assert_eq!(
        seen.borrow().as_slice(),
        [true].as_slice(),
        "入ったことが届くこと"
    );

    // プログラムからの `set_on` は通知しない (4 環境で同じ約束)。
    switch.set_on(false);
    assert!(!switch.is_on(), "切れること");
    assert_eq!(
        seen.borrow().len(),
        1,
        "set_on では on_toggle を呼ばないこと"
    );
    Ok(())
}

fn text_round_trip(ui: &Ui) -> Result<()> {
    let input = ui.text_input("はじめ")?;
    assert_eq!(input.text(), "はじめ");

    // 打鍵は UI オートメーションの Value パターンでは起こせない。TextBox の
    // peer が Value を返すのは既定テンプレートが当たってからで、画面に出て
    // いないコントロールにはまだ当たっていない (`GetPattern` が空を返す)。
    // キーを打ったとき WinUI 自身が行うのと同じ、`TextBox.Text` の書き換えで
    // 代える。ここから先は利用者が打ったときと同じ経路を通る。
    native::<TextBox>(&input)
        .SetText(&HSTRING::from("こんにちは naui"))
        .expect("TextBox への書き込み");
    assert_eq!(input.text(), "こんにちは naui", "打った文字が読めること");

    input.set_text("差し替え");
    assert_eq!(
        native::<TextBox>(&input)
            .Text()
            .expect("TextBox の文字列")
            .to_string(),
        "差し替え",
        "ネイティブの TextBox にも届くこと"
    );

    // 表示中と同じ文字列は書き直さないので、カーソルの位置も変わらない。
    let text_box = native::<TextBox>(&input);
    text_box.SetSelectionStart(2).expect("カーソルの位置");
    input.set_text("差し替え");
    assert_eq!(
        text_box.SelectionStart().expect("カーソルの位置"),
        2,
        "同じ文字列の set_text はカーソルを動かさないこと"
    );
    Ok(())
}

fn label_round_trips(ui: &Ui) -> Result<()> {
    let label = ui.label("見出し")?;
    assert_eq!(label.text(), "見出し");
    label.set_text("差し替え");
    assert_eq!(label.text(), "差し替え");
    assert_eq!(
        native::<TextBlock>(&label)
            .Text()
            .expect("TextBlock の文字列")
            .to_string(),
        "差し替え",
        "ネイティブの TextBlock にも届くこと"
    );
    Ok(())
}

/// `set_role` gives `Primary` the theme's `AccentButtonStyle`. WinUI has no style for
/// destructive actions, so `Destructive` clears the `Style` the same as `Normal`.
fn button_role_maps_to_accent_style(ui: &Ui) -> Result<()> {
    let button = ui.button("保存")?;
    let native = native::<XamlButton>(&button);
    let accent = Application::Current()
        .and_then(|app| app.Resources())
        .and_then(|resources| {
            resources.Lookup(&PropertyValue::CreateString(&HSTRING::from(
                "AccentButtonStyle",
            ))?)
        })
        .and_then(|value| value.cast::<Style>())
        .expect("テーマに AccentButtonStyle があること");
    // `Style` が空なら既定のボタンの見た目になる。
    let style = || native.Style().ok();
    assert_eq!(style(), None, "作った直後は Style が空であること");

    button.set_role(ButtonRole::Primary);
    assert_eq!(
        style(),
        Some(accent.clone()),
        "主ボタンは AccentButtonStyle"
    );
    button.set_role(ButtonRole::Destructive);
    assert_eq!(style(), None, "破壊的な操作は既定の見た目に戻ること");
    button.set_role(ButtonRole::Primary);
    button.set_role(ButtonRole::Normal);
    assert_eq!(style(), None, "既定へ戻すと Style が空になること");
    Ok(())
}

/// `set_style` / `set_color` map to WinUI's type ramp and theme resources.
///
/// naui holds neither point sizes nor colors, so what is checked is "whether the
/// values Fluent decides come in" and "whether both take effect when a level and a
/// role are layered".
fn label_style_and_color_map_to_the_type_ramp(ui: &Ui) -> Result<()> {
    let label = ui.label("見出し")?;
    let native = native::<TextBlock>(&label);
    let font_size = || native.FontSize().expect("FontSize");

    // 段階が上がるほど級数も上がる。値を決めるのは type ramp のほう。
    let mut sizes: Vec<(TextStyle, f64)> = Vec::new();
    for style in [
        TextStyle::Caption,
        TextStyle::Body,
        TextStyle::Subtitle,
        TextStyle::Title,
        TextStyle::LargeTitle,
    ] {
        label.set_style(style);
        sizes.push((style, font_size()));
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
    let body_size = font_size();
    let body_weight = native.FontWeight().expect("FontWeight").Weight;
    label.set_style(TextStyle::Heading);
    assert!(
        (font_size() - body_size).abs() < 0.5,
        "見出しは本文と同じ級数であること: {body_size} / {}",
        font_size()
    );
    assert!(
        native.FontWeight().expect("FontWeight").Weight > body_weight,
        "見出しは本文より太いこと"
    );

    // 色は役割ごとに違うブラシへ落ちる。段階の上へ重ねるので、
    // 色を変えても級数はそのまま残る (`Style` の `BasedOn`)。
    label.set_style(TextStyle::Title);
    let title_size = font_size();
    let mut seen = Vec::new();
    for color in TextColor::ALL {
        label.set_color(color);
        assert!(
            (font_size() - title_size).abs() < 0.5,
            "{color:?} にしても段階が残ること: {title_size} / {}",
            font_size()
        );
        let brush = native
            .Foreground()
            .expect("Foreground")
            .cast::<SolidColorBrush>()
            .expect("テーマリソースは SolidColorBrush であること");
        let current = brush.Color().expect("ブラシの色");
        assert!(
            !seen.contains(&current),
            "{color:?} が別の役割と同じ色になっている: {current:?}"
        );
        seen.push(current);
    }
    Ok(())
}

fn slider_clamp(ui: &Ui) -> Result<()> {
    let slider = ui.slider(0.0, 10.0)?;
    slider.set_value(5.0);
    assert!((slider.value() - 5.0).abs() < 0.001, "範囲内はそのまま");

    slider.set_value(99.0);
    assert!((slider.value() - 10.0).abs() < 0.001, "上限で止まること");
    slider.set_value(-99.0);
    assert!((slider.value() - 0.0).abs() < 0.001, "下限で止まること");

    let xaml = native::<XamlSlider>(&slider);
    assert!(
        (xaml.Minimum().unwrap_or(-1.0) - 0.0).abs() < 0.001
            && (xaml.Maximum().unwrap_or(-1.0) - 10.0).abs() < 0.001,
        "ネイティブの Slider にも範囲が入っていること"
    );
    Ok(())
}

fn progress_clamp(ui: &Ui) -> Result<()> {
    let progress = ui.progress_bar()?;
    // 進捗バーの中身は Grid + Border 2 つで、2 つ目が伸び縮みする前景。
    // `value()` は Rust 側の控えを返すだけなので、ネイティブの幅も見る。
    let fill = native::<Grid>(&progress)
        .Children()
        .and_then(|children| children.GetAt(1))
        .expect("前景の Border")
        .cast::<FrameworkElement>()
        .expect("Border は FrameworkElement");

    progress.set_value(1.0);
    let full = fill.Width().expect("前景の幅");
    assert!(full > 0.0, "いっぱいのときに幅があること");

    progress.set_value(0.5);
    assert!((progress.value() - 0.5).abs() < 0.001);
    assert!(
        (fill.Width().expect("前景の幅") - full / 2.0).abs() < 0.5,
        "半分なら前景も半分の幅になること"
    );

    progress.set_value(2.0);
    assert!((progress.value() - 1.0).abs() < 0.001, "上限で止まること");
    assert!(
        (fill.Width().expect("前景の幅") - full).abs() < 0.5,
        "上限を超えても前景は伸びないこと"
    );

    progress.set_value(-2.0);
    assert!((progress.value() - 0.0).abs() < 0.001, "下限で止まること");
    assert!(
        fill.Width().expect("前景の幅").abs() < 0.5,
        "下限では前景が消えること"
    );
    Ok(())
}

fn combo_box_selection_round_trips(ui: &Ui) -> Result<()> {
    let combo = ui.combo_box()?;
    combo.set_items(&["東京", "大阪", "札幌"]);
    assert_eq!(combo.len(), 3);
    assert_eq!(combo.selected(), None, "はじめは未選択であること");

    let seen = Rc::new(RefCell::new(Vec::new()));
    let sink = seen.clone();
    combo.on_select(move |index| sink.borrow_mut().push(index));

    combo.select(1);
    assert_eq!(combo.selected(), Some(1));
    assert_eq!(
        native::<XamlComboBox>(&combo).SelectedIndex().unwrap_or(-1),
        1,
        "ネイティブの ComboBox にも入っていること"
    );
    assert_eq!(
        seen.borrow().as_slice(),
        [1].as_slice(),
        "選んだことが届くこと"
    );

    // `set_selected` は通知しない。
    combo.set_selected(2);
    assert_eq!(combo.selected(), Some(2));
    assert_eq!(seen.borrow().len(), 1, "set_selected では通知しないこと");
    Ok(())
}

fn radio_group_selects_one(ui: &Ui) -> Result<()> {
    let radio = ui.radio_group()?;
    radio.set_items(&["小", "中", "大"]);
    assert_eq!(radio.len(), 3);

    let seen = Rc::new(RefCell::new(Vec::new()));
    let sink = seen.clone();
    radio.on_select(move |index| sink.borrow_mut().push(index));

    let buttons = radio.native_buttons();
    select(
        &buttons[1]
            .cast::<UIElement>()
            .expect("RadioButton の要素化"),
    );
    assert_eq!(radio.selected(), Some(1));
    assert_eq!(
        seen.borrow().as_slice(),
        [1].as_slice(),
        "選んだことが届くこと"
    );

    // 排他は WinUI が `GroupName` を見て行う。naui は何もしない。
    select(
        &buttons[2]
            .cast::<UIElement>()
            .expect("RadioButton の要素化"),
    );
    assert_eq!(radio.selected(), Some(2), "あとから選んだほうが残ること");
    assert!(
        !buttons[1]
            .IsChecked()
            .and_then(|value| value.Value())
            .unwrap_or(false),
        "前に選んでいたものは消えること"
    );
    assert_eq!(seen.borrow().as_slice(), [1, 2].as_slice());
    Ok(())
}

/// Tree rows have a `Style` that places their content across the row's full width.
///
/// WinUI's `TreeViewItem` binds the content's horizontal position to
/// `HorizontalContentAlignment` (the default template's `ContentPresenter`). Without
/// this, the text spreads only to the content's width and does not use the rest of
/// the row's width.
///
/// It is not on screen, so the allotted width cannot be measured. This checks only
/// that the `Style` is applied to the rows.
fn tree_rows_stretch_their_content(ui: &Ui) -> Result<()> {
    let tree = ui.tree()?;
    let style = tree
        .native_tree_view()
        .ItemContainerStyle()
        .expect("行の Style が当たっていること");
    assert_eq!(
        style
            .TargetType()
            .expect("TargetType")
            .Name
            .to_string()
            .rsplit('.')
            .next()
            .unwrap_or_default(),
        "TreeViewItem",
        "行そのものへ当たる Style であること"
    );

    // `Style::Setters` は投影に入っていないので、中身の `Stretch` そのものは
    // ここでは見られない (投影を増やすと下流のコンパイルを壊すため増やさない)。
    // 当たっていること自体が消える回帰は、これで捕まえられる。
    Ok(())
}

/// `Align` is applied to the children on the cross axis, not to the Stack's `Grid`
/// itself.
///
/// Setting the `Grid` itself to `Left` would shrink even a Stack with
/// `fill_width()` to its content width, and the elements inside could not receive the
/// parent's width even with Stretch.
fn stack_alignment_keeps_fill_width(ui: &Ui) -> Result<()> {
    let stack = ui.stack(Orientation::Vertical)?;
    stack.set_align(Align::Start);

    let normal = ui.label("通常")?;
    let fill = ui.label("幅いっぱい")?;
    fill.set_sizing(Sizing::fill_width());
    stack.append(&normal);
    stack.append(&fill);

    let panel = native::<Grid>(&stack);
    assert_eq!(
        panel.HorizontalAlignment().expect("Stack の Grid の横配置"),
        HorizontalAlignment::Stretch,
        "Stack の Grid 自身は親の幅を受け取ること"
    );
    assert_eq!(
        native::<TextBlock>(&normal)
            .HorizontalAlignment()
            .expect("通常ラベルの横配置"),
        HorizontalAlignment::Left,
        "通常の子は Stack の Align に従うこと"
    );
    assert_eq!(
        native::<TextBlock>(&fill)
            .HorizontalAlignment()
            .expect("Fill ラベルの横配置"),
        HorizontalAlignment::Stretch,
        "子自身の Fill が Stack の Align より優先されること"
    );

    stack.set_align(Align::Center);
    assert_eq!(
        native::<TextBlock>(&normal)
            .HorizontalAlignment()
            .expect("通常ラベルの横配置"),
        HorizontalAlignment::Center,
        "後から寄せ方を変えても既存の子へ反映されること"
    );
    assert_eq!(
        native::<TextBlock>(&fill)
            .HorizontalAlignment()
            .expect("Fill ラベルの横配置"),
        HorizontalAlignment::Stretch,
        "後から寄せ方を変えても Fill は広がること"
    );
    Ok(())
}

/// Even when the alignment and size are changed after adding children, the Stack's
/// cross-axis alignment is kept. Sizing overwrites the native Alignment, so this
/// checks for regressions when the call order is reversed.
fn stack_alignment_survives_sizing(ui: &Ui) -> Result<()> {
    let stack = ui.stack(Orientation::Vertical)?;
    let child = ui.label("後からサイズ変更")?;
    stack.append(&child);
    stack.set_align(Align::End);

    child.set_sizing(Sizing::fixed(120.0, 24.0));
    assert_eq!(
        native::<TextBlock>(&child)
            .HorizontalAlignment()
            .expect("サイズ固定後の横配置"),
        HorizontalAlignment::Right,
        "固定サイズを後から指定しても Stack の End を保つこと"
    );

    child.set_sizing(Sizing::AUTO);
    assert_eq!(
        native::<TextBlock>(&child)
            .HorizontalAlignment()
            .expect("Auto 後の横配置"),
        HorizontalAlignment::Right,
        "Auto を後から指定しても Stack の End を保つこと"
    );
    Ok(())
}

/// With the `ScrollViewer`'s default (top-left alignment), content that does not
/// scroll horizontally cannot use the viewport's width. With naui's default policy,
/// the horizontal axis is Stretch.
fn scroll_stretches_non_scrolling_content(ui: &Ui) -> Result<()> {
    let scroll = ui.scroll()?;
    let child = ui.stack(Orientation::Vertical)?;
    // 明示的な Auto が先に入っていても、スクロールしない軸は埋める。
    child.set_sizing(Sizing::AUTO);
    scroll.set_child(&child);

    let scroll_native = native::<ScrollViewer>(&scroll);
    assert_eq!(
        scroll_native
            .HorizontalContentAlignment()
            .expect("横の内容配置"),
        HorizontalAlignment::Stretch,
        "横へ送らない内容はビューポート幅へ広がること"
    );
    assert_eq!(
        native::<Grid>(&child)
            .HorizontalAlignment()
            .expect("スクロール内容の横配置"),
        HorizontalAlignment::Stretch,
        "中身自身も横へ広がること"
    );
    Ok(())
}

/// A separator is a `Border` drawing one 1 px edge in the theme's divider color, and
/// it stretches along its direction.
fn separator_is_a_themed_border_line(ui: &Ui) -> Result<()> {
    let line = ui.separator(Orientation::Horizontal)?;
    let border = native::<Border>(&line);
    let edge = border.BorderThickness().expect("BorderThickness");
    assert_eq!(
        (edge.Left, edge.Top, edge.Right, edge.Bottom),
        (0.0, 1.0, 0.0, 0.0)
    );
    assert!(border.BorderBrush().is_ok(), "線の色があること");
    assert_eq!(
        border.HorizontalAlignment().expect("横配置"),
        HorizontalAlignment::Stretch,
        "横線は幅いっぱいに伸びること"
    );

    let upright = ui.separator(Orientation::Vertical)?;
    let border = native::<Border>(&upright);
    let edge = border.BorderThickness().expect("BorderThickness");
    assert_eq!(
        (edge.Left, edge.Top, edge.Right, edge.Bottom),
        (1.0, 0.0, 0.0, 0.0)
    );
    assert_eq!(
        border.VerticalAlignment().expect("縦配置"),
        VerticalAlignment::Stretch,
        "縦線は高さいっぱいに伸びること"
    );
    Ok(())
}

/// A group is a heading above a card, and its content stretches inside the card.
fn group_puts_a_heading_above_a_card(ui: &Ui) -> Result<()> {
    let group = ui.group("表示")?;
    assert_eq!(group.text(), "表示");
    let heading = group.native_heading();
    assert_eq!(heading.Text().expect("見出し").to_string(), "表示");
    assert_eq!(
        heading.Visibility().expect("見出しの表示"),
        Visibility::Visible
    );
    // 支援技術には、見出しを名前に持つグループとして見える。
    assert_eq!(
        automation::control_type(&group.native_element()),
        Some(automation::CONTROL_TYPE_GROUP),
        "グループとして支援技術に見えること"
    );
    assert_eq!(
        accessible_name(&group),
        "表示",
        "見出しがグループの名前になること"
    );
    {
        use naui_winui3::Microsoft::UI::Xaml::Automation::AutomationProperties;
        use naui_winui3::Microsoft::UI::Xaml::DependencyObject;
        let object = group
            .native_element()
            .cast::<DependencyObject>()
            .expect("DependencyObject");
        assert!(
            AutomationProperties::GetLabeledBy(&object).expect("LabeledBy")
                == heading.cast::<UIElement>().expect("UIElement"),
            "見出しがグループのラベルとして関連付けられること"
        );
    }
    group.set_text("詳細");
    assert_eq!(
        accessible_name(&group),
        "詳細",
        "見出しを変えると名前も変わること"
    );
    group.set_accessible_label(Some("表示の設定"));
    group.set_text("表示");
    assert_eq!(
        accessible_name(&group),
        "表示の設定",
        "指定した読み上げ名は見出しより優先されること"
    );
    group.set_accessible_label(None);
    assert_eq!(accessible_name(&group), "表示");

    let content = ui.stack(Orientation::Vertical)?;
    group.set_child(&content);
    let card = group.native_card();
    let child = card.Child().expect("カードの中身");
    assert!(
        child
            == native::<Grid>(&content)
                .cast::<UIElement>()
                .expect("UIElement"),
        "中身がカードに入ること"
    );
    assert_eq!(
        native::<Grid>(&content)
            .HorizontalAlignment()
            .expect("中身の横配置"),
        HorizontalAlignment::Stretch,
        "中身はカードの幅いっぱいに広がること"
    );
    let padding = card.Padding().expect("Padding");
    assert_eq!(padding.Left, naui_core::DEFAULT_GROUP_PADDING.left);

    group.set_padding(naui_core::Padding::all(4.0));
    assert_eq!(card.Padding().expect("Padding").Top, 4.0);

    group.set_text("");
    assert_eq!(group.text(), "");
    assert_eq!(
        heading.Visibility().expect("見出しの表示"),
        Visibility::Collapsed
    );
    Ok(())
}

/// Even when Sizing is changed after the Scroll's content is placed, Stretch on the
/// axis where horizontal scrolling is disabled is kept. Checks that the parent's
/// state can be reapplied after the child's Sizing.
fn scroll_stretch_survives_sizing(ui: &Ui) -> Result<()> {
    let scroll = ui.scroll()?;
    let child = ui.stack(Orientation::Vertical)?;
    scroll.set_child(&child);
    child.set_sizing(Sizing::AUTO);

    let scroll_native = native::<ScrollViewer>(&scroll);
    assert_eq!(
        scroll_native
            .HorizontalContentAlignment()
            .expect("横の内容配置"),
        HorizontalAlignment::Stretch,
        "横へ送らない内容はビューポート幅へ広がること"
    );
    assert_eq!(
        native::<Grid>(&child)
            .HorizontalAlignment()
            .expect("サイズ変更後のスクロール内容の横配置"),
        HorizontalAlignment::Stretch,
        "Sizing を後から指定しても非スクロール軸の Stretch を保つこと"
    );
    Ok(())
}

/// Even when an element that already belongs to another parent cannot be added, the
/// failed parent's state is not recorded. Checks that the actual parent's state
/// remains after Sizing.
fn failed_parent_operations_keep_layout_state(ui: &Ui) -> Result<()> {
    let first_stack = ui.stack(Orientation::Vertical)?;
    first_stack.set_align(Align::End);
    let second_stack = ui.stack(Orientation::Vertical)?;
    second_stack.set_align(Align::Start);
    let stack_child = ui.label("既存の Stack 子")?;
    first_stack.append(&stack_child);
    // すでに親があるため、ネイティブ側の Append は失敗する。
    second_stack.append(&stack_child);
    stack_child.set_sizing(Sizing::AUTO);
    assert_eq!(
        native::<TextBlock>(&stack_child)
            .HorizontalAlignment()
            .expect("追加失敗後の Stack 子の横配置"),
        HorizontalAlignment::Right,
        "失敗した Stack の寄せ方を後から再適用しないこと"
    );

    let first_scroll = ui.scroll()?;
    first_scroll.set_policy(ScrollPolicy::Never, ScrollPolicy::Auto);
    let second_scroll = ui.scroll()?;
    second_scroll.set_policy(ScrollPolicy::Always, ScrollPolicy::Auto);
    let scroll_child = ui.stack(Orientation::Vertical)?;
    first_scroll.set_child(&scroll_child);
    // 水平スクロールを許す別の Scroll への SetContent は失敗する。
    second_scroll.set_child(&scroll_child);
    scroll_child.set_sizing(Sizing::AUTO);
    assert_eq!(
        native::<Grid>(&scroll_child)
            .HorizontalAlignment()
            .expect("追加失敗後の Scroll 内容の横配置"),
        HorizontalAlignment::Stretch,
        "失敗した Scroll のポリシーを後から再適用しないこと"
    );
    Ok(())
}

/// Even when an element that already belongs to another parent cannot be added to a
/// Grid, neither the Grid's attached properties nor the Alignment set by the
/// existing parent change.
fn failed_grid_attach_keeps_existing_state(ui: &Ui) -> Result<()> {
    let first_grid = ui.grid()?;
    let second_grid = ui.grid()?;
    let grid_child = ui.label("既存の Grid 子")?;
    let original_cell = GridCell::new(2, 3).span(2, 2);
    first_grid.attach(&grid_child, original_cell);

    // すでに親があるため、ネイティブ側の Append は失敗する。
    second_grid.attach(&grid_child, GridCell::new(0, 0));
    let grid_native = native::<TextBlock>(&grid_child)
        .cast::<FrameworkElement>()
        .expect("Grid 子の FrameworkElement 変換");
    assert_eq!(
        Grid::GetColumn(&grid_native).expect("失敗後の Grid 列"),
        original_cell.column as i32
    );
    assert_eq!(
        Grid::GetRow(&grid_native).expect("失敗後の Grid 行"),
        original_cell.row as i32
    );
    assert_eq!(
        Grid::GetColumnSpan(&grid_native).expect("失敗後の Grid 列 span"),
        original_cell.column_span as i32
    );
    assert_eq!(
        Grid::GetRowSpan(&grid_native).expect("失敗後の Grid 行 span"),
        original_cell.row_span as i32
    );

    let stack = ui.stack(Orientation::Horizontal)?;
    stack.set_align(Align::End);
    let stack_child = ui.label("既存の Stack 子")?;
    stack.append(&stack_child);
    let third_grid = ui.grid()?;
    third_grid.attach(&stack_child, GridCell::new(0, 0));
    assert_eq!(
        native::<TextBlock>(&stack_child)
            .VerticalAlignment()
            .expect("失敗後の Stack 子の縦配置"),
        VerticalAlignment::Bottom,
        "失敗した Grid の中央寄せを既存の Stack 子へ残さないこと"
    );
    Ok(())
}

/// A Tag set through WinUI's escape hatch is not lost by Sizing or by parent
/// container operations.
fn layout_preserves_user_tag(ui: &Ui) -> Result<()> {
    let stack = ui.stack(Orientation::Vertical)?;
    let stack_child = ui.label("Tag を持つ Stack 子")?;
    let stack_native = native::<TextBlock>(&stack_child)
        .cast::<FrameworkElement>()
        .expect("Stack 子の FrameworkElement 変換");
    let user_tag =
        PropertyValue::CreateString(&HSTRING::from("user-stack-tag")).expect("Stack 子の Tag");
    stack_native.SetTag(&user_tag).expect("Stack 子の Tag 設定");
    stack.append(&stack_child);
    stack.set_align(Align::End);
    stack_child.set_sizing(Sizing::AUTO);
    assert_eq!(
        stack_native
            .Tag()
            .expect("Stack 子の Tag 読み出し")
            .cast::<IPropertyValue>()
            .expect("Stack 子の Tag 型")
            .GetString()
            .expect("Stack 子の Tag 文字列")
            .to_string(),
        "user-stack-tag"
    );
    stack.remove(0);

    let scroll = ui.scroll()?;
    let scroll_child = ui.stack(Orientation::Vertical)?;
    let scroll_native = native::<Grid>(&scroll_child)
        .cast::<FrameworkElement>()
        .expect("Scroll 内容の FrameworkElement 変換");
    let user_tag =
        PropertyValue::CreateString(&HSTRING::from("user-scroll-tag")).expect("Scroll 内容の Tag");
    scroll_native
        .SetTag(&user_tag)
        .expect("Scroll 内容の Tag 設定");
    scroll.set_child(&scroll_child);
    scroll_child.set_sizing(Sizing::AUTO);
    assert_eq!(
        scroll_native
            .Tag()
            .expect("Scroll 内容の Tag 読み出し")
            .cast::<IPropertyValue>()
            .expect("Scroll 内容の Tag 型")
            .GetString()
            .expect("Scroll 内容の Tag 文字列")
            .to_string(),
        "user-scroll-tag"
    );
    Ok(())
}

/// The Alignment a Stack set on the cross axis is restored when the child is
/// removed. Otherwise the previous Stack's alignment would linger on the element when
/// it is next placed in a Grid.
fn removing_stack_child_restores_alignment(ui: &Ui) -> Result<()> {
    let stack = ui.stack(Orientation::Vertical)?;
    stack.set_align(Align::End);
    let child = ui.label("親を移る")?;
    stack.append(&child);
    stack.remove(0);

    let grid = ui.grid()?;
    grid.attach(&child, GridCell::new(0, 0));
    assert_eq!(
        native::<TextBlock>(&child)
            .HorizontalAlignment()
            .expect("Grid へ移した子の横配置"),
        HorizontalAlignment::Stretch,
        "Stack の Right が次の親へ持ち越されないこと"
    );
    Ok(())
}

fn stack_keeps_children(ui: &Ui) -> Result<()> {
    let stack = ui.stack(Orientation::Vertical)?;
    {
        // Rust 側のハンドルはここで落ちる。ネイティブの子は残るはず。
        let label = ui.label("残る")?;
        stack.append(&label);
        stack.append(&ui.button("押す")?);
        stack.append(&ui.checkbox("入れる")?);
    }
    assert_eq!(stack.len(), 3);

    let children = native::<Grid>(&stack).Children().expect("子の一覧");
    assert_eq!(children.Size().unwrap_or(0), 3, "ネイティブ側にも 3 つ");
    let first = children
        .GetAt(0)
        .expect("先頭の子")
        .cast::<TextBlock>()
        .expect("先頭は TextBlock");
    assert_eq!(
        first.Text().expect("文字列").to_string(),
        "残る",
        "ハンドルを落としても中身が生きていること"
    );
    Ok(())
}

/// The kinds of a Stack's main-axis tracks, in order.
fn stack_tracks(stack: &Grid, vertical: bool) -> Vec<GridUnitType> {
    if vertical {
        let definitions = stack.RowDefinitions().expect("行の定義");
        (0..definitions.Size().expect("行の数"))
            .map(|i| {
                definitions
                    .GetAt(i)
                    .expect("行")
                    .Height()
                    .expect("高さ")
                    .GridUnitType
            })
            .collect()
    } else {
        let definitions = stack.ColumnDefinitions().expect("列の定義");
        (0..definitions.Size().expect("列の数"))
            .map(|i| {
                definitions
                    .GetAt(i)
                    .expect("列")
                    .Width()
                    .expect("幅")
                    .GridUnitType
            })
            .collect()
    }
}

/// A Stack is a `Grid` with one track per shown child: `Star` for `Fill` on the main
/// axis (a `Spacer` included), `Auto` otherwise. A hidden child shares the track
/// before it, so it takes neither leftover space nor the spacing around a track. A
/// `Fill` given after adding, showing a child again and removing one all lay it out
/// again.
fn stack_gives_fill_children_star_tracks(ui: &Ui) -> Result<()> {
    const AUTO: GridUnitType = GridUnitType::Auto;
    const STAR: GridUnitType = GridUnitType::Star;

    let stack = ui.stack(Orientation::Horizontal)?;
    let first = ui.label("先頭")?;
    let spacer = ui.spacer()?;
    let hidden = ui.button("隠す")?;
    hidden.set_sizing(Sizing::fill());
    hidden.set_visible(false);
    let last = ui.button("末尾")?;
    stack.append(&first);
    stack.append(&spacer);
    stack.append(&hidden);
    stack.append(&last);

    let grid = native::<Grid>(&stack);
    let columns = || {
        [&first as &dyn Widget, &spacer, &hidden, &last]
            .map(|widget| Grid::GetColumn(&native::<FrameworkElement>(widget)).expect("列"))
    };
    assert_eq!(
        stack_tracks(&grid, false),
        [AUTO, STAR, AUTO],
        "Spacer だけが Star で、隠した子は列を持たないこと"
    );
    assert_eq!(columns(), [0, 1, 1, 2], "隠した子は前の子の列に入ること");
    assert_eq!(
        stack_tracks(&grid, true),
        [],
        "交差軸は暗黙の 1 本のままのこと"
    );

    hidden.set_visible(true);
    assert_eq!(
        stack_tracks(&grid, false),
        [AUTO, STAR, STAR, AUTO],
        "見せ直すと Fill の子が自分の Star の列を持つこと"
    );
    assert_eq!(columns(), [0, 1, 2, 3]);

    first.set_sizing(Sizing::fill_width());
    last.set_sizing(Sizing::fill_height());
    assert_eq!(
        stack_tracks(&grid, false),
        [STAR, STAR, STAR, AUTO],
        "追加の後の主軸の Fill は Star にし、交差軸だけの Fill は Auto のままのこと"
    );

    stack.remove(1);
    assert_eq!(
        stack_tracks(&grid, false),
        [STAR, STAR, AUTO],
        "外すと列を詰めること"
    );

    let column = ui.stack(Orientation::Vertical)?;
    let body = ui.label("本文")?;
    body.set_sizing(Sizing::fill_height());
    column.append(&ui.label("見出し")?);
    column.append(&body);
    let column_grid = native::<Grid>(&column);
    assert_eq!(
        stack_tracks(&column_grid, true),
        [AUTO, STAR],
        "縦のスタックでは高さの Fill が Star の行になること"
    );
    assert_eq!(stack_tracks(&column_grid, false), []);
    Ok(())
}

/// Switching a Stack's orientation moves its tracks to the other axis, gives back
/// the alignment the previous cross axis overrode and applies `Align` to the new one,
/// ending up as a Stack made in that direction would.
fn stack_switches_orientation(ui: &Ui) -> Result<()> {
    const AUTO: GridUnitType = GridUnitType::Auto;
    const STAR: GridUnitType = GridUnitType::Star;

    let build = |orientation: Orientation| -> Result<_> {
        let stack = ui.stack(orientation)?;
        stack.set_align(Align::End);
        let label = ui.label("前")?;
        let wide = ui.button("広がる")?;
        wide.set_sizing(Sizing::fill_width());
        stack.append(&label);
        stack.append(&wide);
        Ok((stack, label, wide))
    };
    let placement = |widget: &dyn Widget| {
        let element = native::<FrameworkElement>(widget);
        (
            element.HorizontalAlignment().expect("横配置"),
            element.VerticalAlignment().expect("縦配置"),
            Grid::GetRow(&element).expect("行"),
            Grid::GetColumn(&element).expect("列"),
        )
    };

    let (stack, label, wide) = build(Orientation::Vertical)?;
    let grid = native::<Grid>(&stack);
    assert_eq!(
        stack_tracks(&grid, true),
        [AUTO, AUTO],
        "縦並びでは幅の Fill は行を Star にしないこと"
    );

    stack.set_orientation(Orientation::Horizontal);
    assert_eq!(stack.orientation(), Orientation::Horizontal);
    assert_eq!(
        stack_tracks(&grid, false),
        [AUTO, STAR],
        "横並びでは幅の Fill が Star の列を持つこと"
    );
    assert_eq!(stack_tracks(&grid, true), [], "前の主軸の行を残さないこと");
    // Keep the reference Stack alive: dropping it restores its children's alignment
    // and clears their parent layout, so they would no longer be a valid reference.
    let (_horizontal_stack, made_label, made_wide) = build(Orientation::Horizontal)?;
    assert_eq!(
        placement(&label),
        placement(&made_label),
        "横で作ったのと同じ配置になること"
    );
    assert_eq!(placement(&wide), placement(&made_wide));
    assert_eq!(
        native::<FrameworkElement>(&label)
            .VerticalAlignment()
            .expect("縦配置"),
        VerticalAlignment::Bottom,
        "揃えが新しい交差軸に効くこと"
    );

    stack.set_orientation(Orientation::Vertical);
    assert_eq!(stack_tracks(&grid, true), [AUTO, AUTO]);
    assert_eq!(stack_tracks(&grid, false), [], "前の主軸の列を残さないこと");
    let (_vertical_stack, made_label, made_wide) = build(Orientation::Vertical)?;
    assert_eq!(
        placement(&label),
        placement(&made_label),
        "縦へ戻せば縦で作ったのと同じ配置になること"
    );
    assert_eq!(placement(&wide), placement(&made_wide));
    Ok(())
}

/// A child taken out of a Stack does not keep its row number. Otherwise, in a `Grid`
/// that places its children without setting a row (a window's content row, for
/// example), it would land in a row that is not there.
fn removing_stack_child_resets_its_track(ui: &Ui) -> Result<()> {
    let stack = ui.stack(Orientation::Vertical)?;
    stack.append(&ui.label("一")?);
    let child = ui.label("二")?;
    stack.append(&child);
    let element = native::<FrameworkElement>(&child);
    assert_eq!(Grid::GetRow(&element).expect("行"), 1);

    stack.remove(1);
    assert_eq!(
        Grid::GetRow(&element).expect("外した後の行"),
        0,
        "remove で行を戻すこと"
    );

    stack.append(&child);
    assert_eq!(Grid::GetRow(&element).expect("戻した後の行"), 1);
    stack.clear();
    assert_eq!(
        Grid::GetRow(&element).expect("clear の後の行"),
        0,
        "clear で行を戻すこと"
    );
    assert_eq!(
        stack_tracks(&native::<Grid>(&stack), true),
        [],
        "clear で行の定義も無くなること"
    );
    Ok(())
}

/// A child that spanned cells in a `Grid` takes one track in a Stack. A span of 2
/// would also cover the next child's column, so the two would overlap.
fn grid_span_does_not_follow_into_a_stack(ui: &Ui) -> Result<()> {
    let grid = ui.grid()?;
    let child = ui.button("2 列にまたがる")?;
    grid.attach(&child, GridCell::new(0, 0).span(2, 2));
    grid.remove(GridCell::new(0, 0));

    let stack = ui.stack(Orientation::Horizontal)?;
    let next = ui.button("後ろ")?;
    stack.append(&child);
    stack.append(&next);
    let element = native::<FrameworkElement>(&child);
    assert_eq!(
        (
            Grid::GetColumnSpan(&element).expect("列の結合"),
            Grid::GetRowSpan(&element).expect("行の結合"),
        ),
        (1, 1),
        "Grid の結合を Stack へ持ち込まないこと"
    );
    assert_eq!(
        (
            Grid::GetColumn(&element).expect("移した子の列"),
            Grid::GetColumn(&native::<FrameworkElement>(&next)).expect("後ろの子の列"),
        ),
        (0, 1),
        "移した子と後ろの子が別の列に入ること"
    );
    Ok(())
}

/// A grid cell centers an `Auto` child vertically. That holds when the child's size
/// is specified after attaching (`apply_sizing` sets the alignment too), and is not
/// carried along when the child leaves the grid: in a `Scroll` or a window it would
/// stay centered instead of taking the top.
fn grid_cell_placement_follows_the_child(ui: &Ui) -> Result<()> {
    let grid = ui.grid()?;
    let child = ui.label("マスの中")?;
    grid.attach(&child, GridCell::new(1, 1));
    let element = native::<FrameworkElement>(&child);
    child.set_sizing(Sizing::fixed(80.0, 20.0));
    assert_eq!(
        element.VerticalAlignment().expect("縦の寄せ方"),
        VerticalAlignment::Center,
        "置いたあとに大きさを決めてもマスの中央に置くこと"
    );

    grid.remove(GridCell::new(1, 1));
    assert_eq!(
        element.VerticalAlignment().expect("外したあとの縦の寄せ方"),
        VerticalAlignment::Top,
        "外したらマスのための寄せ方を残さないこと"
    );
    assert_eq!(
        (
            Grid::GetColumn(&element).expect("列"),
            Grid::GetRow(&element).expect("行"),
        ),
        (0, 0),
        "外したらマスの位置を残さないこと"
    );
    Ok(())
}

/// Adding a maximum after attaching wraps the child, but its old cell properties
/// are still on the child itself. Both removal paths must reset those properties
/// even when the child has to be released from a wrapper first.
fn removing_capped_grid_child_resets_placement(ui: &Ui) -> Result<()> {
    for clear in [false, true] {
        for capped in [false, true] {
            for fill_height in [false, true] {
                let grid = ui.grid()?;
                let child = ui.label("移す子")?;
                let cell = GridCell::new(1, 2).span(2, 3);
                grid.attach(&child, cell);
                let sizing = if fill_height {
                    Sizing::fill()
                } else {
                    Sizing::fill_width()
                };
                child.set_sizing(if capped {
                    sizing.max_width(80.0)
                } else {
                    sizing
                });

                if clear {
                    grid.clear();
                } else {
                    grid.remove(cell);
                }
                let element = native::<FrameworkElement>(&child);
                assert_eq!(
                    (
                        Grid::GetColumn(&element).expect("列"),
                        Grid::GetRow(&element).expect("行"),
                        Grid::GetColumnSpan(&element).expect("列の結合"),
                        Grid::GetRowSpan(&element).expect("行の結合"),
                    ),
                    (0, 0, 1, 1),
                    "外した子にマスの配置を残さない: clear={clear}, capped={capped}, fill_height={fill_height}"
                );
                assert_eq!(
                    element.VerticalAlignment().expect("縦の寄せ方"),
                    if fill_height {
                        VerticalAlignment::Stretch
                    } else {
                        VerticalAlignment::Top
                    },
                    "外した子自身の Sizing を保つこと"
                );
                assert!(grid.is_empty(), "元の Grid から外れていること");
                // A raw panel does not repair the cell properties for us. Appending
                // also proves that the former wrapper no longer owns the child.
                let other = Grid::new().expect("移動先の Grid");
                let children = other.Children().expect("移動先の子一覧");
                children
                    .Append(&child.native_element())
                    .expect("外した子をほかの親へ追加できること");
                children.Clear().expect("移動先から外す");
            }
        }
    }
    Ok(())
}

/// The space actually handed out: a `Spacer` pushes the button after it to the
/// right edge, a vertical `Spacer` pushes the last label to the bottom, and a hidden
/// child leaves no doubled spacing.
///
/// Messages are pumped until it is laid out. In environments where it never is (CI),
/// it passes without checking.
fn stack_hands_leftover_space_to_fill_and_spacer(ui: &Ui) -> Result<()> {
    const SPACING: f64 = 10.0;

    let window = ui.window("余り", 400.0, 300.0)?;
    let root = ui.stack(Orientation::Vertical)?;
    root.set_sizing(Sizing::fill());
    root.set_spacing(SPACING);

    let bar = ui.stack(Orientation::Horizontal)?;
    bar.set_sizing(Sizing::fill_width());
    let start = ui.button("左")?;
    let end = ui.button("右")?;
    bar.append(&start);
    bar.append(&ui.spacer()?);
    bar.append(&end);

    let above = ui.label("上")?;
    let hidden = ui.label("隠す")?;
    hidden.set_visible(false);
    let below = ui.label("下")?;
    let footer = ui.label("足")?;
    root.append(&bar);
    root.append(&above);
    root.append(&hidden);
    root.append(&below);
    root.append(&ui.spacer()?);
    root.append(&footer);
    window.set_child(&root);
    window.show();

    let element = |widget: &dyn Widget| native::<FrameworkElement>(widget);
    let (bar_element, end_element) = (element(&bar), element(&end));
    let (root_element, footer_element) = (element(&root), element(&footer));
    let (above_element, below_element) = (element(&above), element(&below));
    // 親の中での位置 (x, y) と大きさ (幅, 高さ)。
    let bounds = |element: &FrameworkElement| {
        let offset = element
            .cast::<UIElement>()
            .and_then(|element| element.ActualOffset())
            .expect("位置");
        (
            f64::from(offset.X),
            f64::from(offset.Y),
            element.ActualWidth().unwrap_or(0.0),
            element.ActualHeight().unwrap_or(0.0),
        )
    };
    let laid_out = || {
        [&root_element, &end_element, &footer_element]
            .iter()
            .all(|element| element.ActualWidth().unwrap_or(0.0) > 0.0)
    };
    pump_until(SCROLL_LAYOUT_TIMEOUT, laid_out);

    let outcome = catch_unwind(AssertUnwindSafe(|| {
        if !laid_out() {
            println!("     (中身に実寸が付かない環境なので、確かめずに通す)");
            return;
        }
        let (end_x, _, end_width, _) = bounds(&end_element);
        let bar_width = bar_element.ActualWidth().unwrap_or(0.0);
        assert!(
            (end_x + end_width - bar_width).abs() < 1.0,
            "Spacer の後のボタンが右端に付くこと: {} / {bar_width}",
            end_x + end_width
        );

        let (_, footer_y, _, footer_height) = bounds(&footer_element);
        let root_height = root_element.ActualHeight().unwrap_or(0.0);
        assert!(
            (footer_y + footer_height - root_height).abs() < 1.0,
            "縦の Spacer の後のラベルが下端に付くこと: {} / {root_height}",
            footer_y + footer_height
        );

        let (_, above_y, _, above_height) = bounds(&above_element);
        let (_, below_y, _, _) = bounds(&below_element);
        let gap = below_y - (above_y + above_height);
        assert!(
            (gap - SPACING).abs() < 0.5,
            "隠した子の両側に間隔を重ねないこと: {gap}"
        );
    }));
    window.close();
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
    Ok(())
}

fn widgets_expose_accessible_names(ui: &Ui) -> Result<()> {
    // WinUI は Content に置いた TextBlock の文字列を読み上げ名にする。
    // naui はラベルをそこへ入れているので、支援技術から名前が読める。
    let button = ui.button("保存")?;
    assert_eq!(accessible_name(&button), "保存", "ボタンの読み上げ名");

    let checkbox = ui.checkbox("同意する")?;
    assert_eq!(
        accessible_name(&checkbox),
        "同意する",
        "チェックボックスの読み上げ名"
    );

    let label = ui.label("見出し")?;
    assert_eq!(accessible_name(&label), "見出し", "ラベルの読み上げ名");
    Ok(())
}

/// The window root has three rows, "title bar → menu bar → content", and toasts are
/// overlaid on the content row (the third). If the overlay target kept an old number
/// when a row was added, the toast would go into the menu bar row (`Auto`) and push
/// the content down.
///
/// The overlay target is decided right when the toast is shown, so the check is done
/// inside the setup. When a window closes, the cleanup (the `AppWindow`'s `Closing`)
/// removes the content of every window, so the root `Grid` cannot be read after a
/// round (after other cases close their windows). For the same reason, this window
/// is not closed either, and is left to the app's exit.
fn toast_overlays_the_content_below_the_menu_bar(ui: &Ui) -> Result<Deferred> {
    let window = ui.window("トースト", 320.0, 240.0)?;
    let stack = ui.stack(Orientation::Vertical)?;
    stack.append(&ui.label("中身")?);
    window.set_child(&stack);
    let menu_bar = ui.menu_bar()?;
    menu_bar.set_menus(&[MenuSpec::new("ファイル", ["新規"])]);
    window.set_menu_bar(&menu_bar);
    window.show();

    // 重ね先は「最後に出したウィンドウ」なので、出してから重ねる。
    let toast = ui.toast("保存しました")?;
    toast.show();

    let rows = window
        .native_window()
        .Content()
        .expect("ウィンドウの中身")
        .cast::<Grid>()
        .expect("根は Grid")
        .Children()
        .expect("根の子");
    // 行ごとの子。XAML は子を行の順に並べている。
    let row = |index: u32| -> Vec<UIElement> {
        let children = rows
            .GetAt(index)
            .expect("行")
            .cast::<Grid>()
            .expect("行は Grid")
            .Children()
            .expect("行の子");
        (0..children.Size().expect("子の数"))
            .map(|i| children.GetAt(i).expect("子"))
            .collect()
    };

    let toast_element = toast.native_element();
    assert!(toast.is_visible(), "トーストが出ていること");
    // 中身に重なるので、下が透けない背景を持つこと (InfoBar の既定はほぼ透明)。
    let background = toast_element
        .cast::<naui_winui3::Microsoft::UI::Xaml::Controls::Control>()
        .and_then(|control| control.Background())
        .and_then(|brush| brush.cast::<SolidColorBrush>())
        .and_then(|brush| brush.Color())
        .expect("トーストの背景は単色のブラシ");
    assert_eq!(background.A, 0xff, "トーストの背景は不透明: {background:?}");
    assert!(
        row(2).contains(&toast_element),
        "トーストは中身の行に重なること"
    );
    assert!(
        !row(1).contains(&toast_element),
        "メニューバーの行には入らないこと"
    );
    let panel = menu_bar
        .native_panel()
        .cast::<UIElement>()
        .expect("StackPanel の要素化");
    assert_eq!(
        row(1),
        vec![panel],
        "メニューバーの行にはメニューバーだけが入ること"
    );

    toast.dismiss();
    window.clear_menu_bar();
    assert_eq!(row(1).len(), 0, "外すとメニューバーの行は空になること");

    // 確かめることはもう無い。ウィンドウは `Ui` が持ち、アプリの終了で畳まれる。
    Ok(Box::new(|| Ok(())))
}

fn window_lifecycle(ui: &Ui) -> Result<Deferred> {
    let window = ui.window("テスト", 320.0, 240.0)?;
    assert_eq!(window.title(), "テスト");
    window.set_title("別の題");
    assert_eq!(window.title(), "別の題");
    assert!(!window.is_visible(), "出すまでは見えないこと");

    let stack = ui.stack(Orientation::Vertical)?;
    stack.append(&ui.label("中身")?);
    window.set_child(&stack);
    window.show();

    Ok(Box::new(move || {
        // 出したことを先に確かめる。ここを飛ばすと、はじめから見えていない
        // ウィンドウを閉じただけでも「閉じたら見えない」が成り立ってしまう。
        assert!(window.is_visible(), "出したら見えること");
        window.close();
        assert!(!window.is_visible(), "閉じたら見えないこと");
        Ok(())
    }))
}

/// The system draws the title bar's buttons, so `RequestedTheme` on the XAML root does
/// not reach them; the theme has to be handed to `AppWindowTitleBar.PreferredTheme`.
/// The colors themselves can only be seen on a real screen, so this checks that the
/// theme is handed over both when creating a window and when switching at runtime.
///
/// Older runtimes that naui still supports have no `PreferredTheme`. There the buttons
/// stay in the OS theme's colors, so only check that switching the theme still works.
fn title_bar_buttons_follow_the_theme(ui: &Ui) -> Result<()> {
    let read_preferred = |window: &naui_windows::Window| {
        window
            .native_window()
            .AppWindow()
            .and_then(|window| window.TitleBar())
            .and_then(|bar| bar.PreferredTheme())
    };
    let preferred =
        |window: &naui_windows::Window| read_preferred(window).expect("タイトルバーのテーマ");

    let window = ui.window("テーマ", 320.0, 240.0)?;
    if matches!(read_preferred(&window), Err(error) if error.code() == E_NOINTERFACE) {
        println!("     (PreferredTheme が無いランタイムなので、ボタンのテーマは確かめずに通す)");
        window.set_theme(Theme::Light)?;
        window.set_theme(Theme::System)?;
        window.close();
        return Ok(());
    }
    assert_eq!(preferred(&window), TitleBarTheme::UseDefaultAppMode);
    // 中身を載せるとタイトルバーの中まで広げるので、そのあとの切り替えを確かめる。
    window.set_child(&ui.label("中身")?);
    for (theme, expected) in [
        (Theme::Light, TitleBarTheme::Light),
        (Theme::Dark, TitleBarTheme::Dark),
        (Theme::System, TitleBarTheme::UseDefaultAppMode),
    ] {
        window.set_theme(theme)?;
        assert_eq!(preferred(&window), expected, "{theme:?} へ切り替えたとき");
    }
    window.close();

    // はじめから Light のアプリで作ったウィンドウも、最初から従う。
    ui.set_theme(Theme::Light)?;
    let created = ui.window("はじめから", 320.0, 240.0)?;
    let theme = preferred(&created);
    ui.set_theme(Theme::System)?;
    created.close();
    assert_eq!(theme, TitleBarTheme::Light);
    Ok(())
}

/// `size` and `on_resize` are the size of the content row, excluding the title bar
/// band naui draws and the menu bar (the same as macOS's `contentView` and the area
/// below GTK's header bar). `Window.Bounds` includes what is behind the title bar,
/// so as is it differs.
///
/// Messages are pumped until the row has an actual size. In environments where it
/// never gets one (CI), it passes without checking.
/// The app's language is set on the window's content, which every element inside
/// inherits (`FrameworkElement.Language`).
fn app_language_is_set_on_the_window_content(ui: &Ui) -> Result<()> {
    naui_core::set_app_language(Some("ja-JP"));
    let window = ui.window("言語", 200.0, 120.0)?;
    let stack = ui.stack(Orientation::Vertical)?;
    let label = ui.label("今日は")?;
    stack.append(&label);
    window.set_child(&stack);
    naui_core::set_app_language(None);
    let root = window
        .native_window()
        .Content()
        .expect("ウィンドウの中身")
        .cast::<FrameworkElement>()
        .expect("根は FrameworkElement");
    assert_eq!(root.Language().expect("Language").to_string(), "ja-JP");
    assert_eq!(
        native::<FrameworkElement>(&label)
            .Language()
            .expect("Language")
            .to_string(),
        "ja-JP",
        "中の要素は根の言語を継ぐこと"
    );
    window.close();
    Ok(())
}

/// A right-to-left layout is `FlowDirection` on the window's content, which every
/// element inside inherits. XAML mirrors by a transform on the rendering, so the
/// elements that draw for themselves (`Canvas`, `Image`, `Video`) and `NumberInput`
/// are given `LeftToRight` back.
fn right_to_left_layout_is_set_on_the_window_content(ui: &Ui) -> Result<()> {
    use naui_winui3::Microsoft::UI::Xaml::FlowDirection;

    naui_core::set_app_layout_direction(Some(LayoutDirection::Rtl));
    let built = (|| -> Result<_> {
        let window = ui.window("書字方向", 400.0, 200.0)?;
        let stack = ui.stack(Orientation::Vertical)?;
        let row = ui.stack(Orientation::Horizontal)?;
        let first = ui.button("一")?;
        let second = ui.button("二")?;
        row.append(&first);
        row.append(&second);
        stack.append(&row);
        let number = ui.number_input(-5.0)?;
        let canvas = ui.canvas()?;
        let image = ui.image("")?;
        let video = ui.video("")?;
        stack.append(&number);
        stack.append(&canvas);
        stack.append(&image);
        stack.append(&video);
        window.set_child(&stack);
        Ok((window, first, second, number, canvas, image, video))
    })();
    assert_eq!(ui.layout_direction(), LayoutDirection::Rtl);
    naui_core::set_app_layout_direction(None);
    let (window, first, second, number, canvas, image, video) = built?;

    let root = window
        .native_window()
        .Content()
        .expect("ウィンドウの中身")
        .cast::<FrameworkElement>()
        .expect("根は FrameworkElement");
    let direction = |element: &FrameworkElement| element.FlowDirection().expect("FlowDirection");
    assert_eq!(direction(&root), FlowDirection::RightToLeft);
    assert_eq!(
        direction(&native::<FrameworkElement>(&first)),
        FlowDirection::RightToLeft,
        "中の要素は根の向きを継ぐこと"
    );
    for (name, element) in [
        ("数値入力", native::<FrameworkElement>(&number)),
        ("描画面", native::<FrameworkElement>(&canvas)),
        ("画像", native::<FrameworkElement>(&image)),
        ("動画", native::<FrameworkElement>(&video)),
    ] {
        assert_eq!(
            direction(&element),
            FlowDirection::LeftToRight,
            "{name}は左から右のまま"
        );
    }

    // 横に並べた最初の子が右に来る (反転は描画の変換なので、窓から見た位置で比べる)。
    window.show();
    let first_element = native::<FrameworkElement>(&first);
    let laid_out = || first_element.ActualWidth().unwrap_or(0.0) > 0.0;
    pump_until(SCROLL_LAYOUT_TIMEOUT, laid_out);
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        if !laid_out() {
            println!("     (実寸が付かない環境なので、位置は確かめずに通す)");
            return;
        }
        let x = |element: &FrameworkElement| {
            element
                .TransformToVisual(None)
                .and_then(|transform| {
                    transform.TransformPoint(windows::Foundation::Point { X: 0.0, Y: 0.0 })
                })
                .expect("窓から見た位置")
                .X
        };
        let first_x = x(&first_element);
        let second_x = x(&native::<FrameworkElement>(&second));
        assert!(
            first_x > second_x,
            "最初の子が右に来ること: 一 {first_x} / 二 {second_x}"
        );
    }));
    window.close();
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
    Ok(())
}

fn window_size_is_the_content_row(ui: &Ui) -> Result<()> {
    let window = ui.window("大きさ", 420.0, 300.0)?;
    let reported = Rc::new(Cell::new(None));
    window.on_resize({
        let reported = reported.clone();
        move |size| reported.set(Some(size))
    });
    let stack = ui.stack(Orientation::Vertical)?;
    stack.append(&ui.label("中身")?);
    window.set_child(&stack);
    let menu_bar = ui.menu_bar()?;
    menu_bar.set_menus(&[MenuSpec::new("ファイル", ["新規"])]);
    window.set_menu_bar(&menu_bar);
    window.show();
    let content_row = window
        .native_window()
        .Content()
        .expect("ウィンドウの中身")
        .cast::<Grid>()
        .expect("根は Grid")
        .Children()
        .expect("根の子")
        .GetAt(2)
        .expect("中身の行")
        .cast::<FrameworkElement>()
        .expect("行は FrameworkElement");
    let measure = || {
        (
            content_row.ActualWidth().unwrap_or(0.0),
            content_row.ActualHeight().unwrap_or(0.0),
        )
    };
    // メニューバーが並んで中身の行が縮み、その通知が届くまで待つ。
    pump_until(SCROLL_LAYOUT_TIMEOUT, || {
        measure().1 > 0.0 && reported.get() == Some(window.size())
    });

    let outcome = catch_unwind(AssertUnwindSafe(|| {
        let row = measure();
        if row.1 <= 0.0 {
            println!("     (中身の行に実寸が付かない環境なので、確かめずに通す)");
            return;
        }
        let size = window.size();
        let near =
            |a: (f64, f64), b: (f64, f64)| (a.0 - b.0).abs() < 1.0 && (a.1 - b.1).abs() < 1.0;
        assert!(
            near(size, row),
            "size {size:?} が中身の行 {row:?} と同じこと"
        );
        let bounds = window.native_window().Bounds().expect("Bounds");
        assert!(
            f64::from(bounds.Height) - size.1 > 32.0,
            "タイトルバーとメニューバーの分だけ Bounds より低いこと"
        );
        let last = reported.get().expect("on_resize が呼ばれていること");
        assert!(
            near(last, size),
            "最後の通知 {last:?} が size {size:?} と同じこと"
        );
    }));
    window.close();
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
    Ok(())
}

/// A switch and radio buttons with short labels do not take WinUI's default minimum
/// width (`ToggleSwitch` 154 / `RadioButton` 120).
///
/// With the minimum left in place, the control after a switch, and the items of a
/// horizontal radio group, end up far apart. naui removes it with an implicit style
/// based on Fluent's default style, the same as for `CheckBox`. Without the base, the
/// switch falls back to a template that keeps 154 even with `MinWidth` = 0, so the
/// laid-out width is checked rather than the property. Horizontal radio items are
/// separated by WinUI's `RadioButtonsColumnSpacing` (7).
///
/// The style is applied once the controls are in a shown window, so messages are
/// pumped until they are laid out. In environments where they never load (CI), it
/// passes without checking.
fn short_labels_drop_the_minimum_width(ui: &Ui) -> Result<()> {
    let window = ui.window("最小幅", 420.0, 200.0)?;
    let stack = ui.stack(Orientation::Vertical)?;
    let toggle = ui.toggle("On")?;
    let radio = ui.radio_group()?;
    radio.set_items(&["A", "B"]);
    radio.set_orientation(Orientation::Horizontal);
    stack.append(&toggle);
    stack.append(&radio);
    window.set_child(&stack);
    window.show();

    let switch = native::<ToggleSwitch>(&toggle);
    let buttons: Vec<FrameworkElement> = {
        let panel = native::<StackPanel>(&radio)
            .Children()
            .expect("ラジオグループの子");
        (0..panel.Size().unwrap_or(0))
            .map(|index| {
                panel
                    .GetAt(index)
                    .expect("ラジオボタン")
                    .cast::<FrameworkElement>()
                    .expect("ラジオボタンは FrameworkElement")
            })
            .collect()
    };
    let laid_out = |element: &FrameworkElement| {
        element.IsLoaded().unwrap_or(false) && element.ActualWidth().unwrap_or(0.0) > 0.0
    };
    let switch_element: FrameworkElement = switch.cast().expect("ToggleSwitch は FrameworkElement");
    pump_until(SCROLL_LAYOUT_TIMEOUT, || {
        laid_out(&switch_element) && buttons.iter().all(laid_out)
    });

    let outcome = catch_unwind(AssertUnwindSafe(|| {
        if !laid_out(&switch_element) {
            println!("     (コントロールが読み込まれない環境なので、確かめずに通す)");
            return;
        }
        assert_eq!(buttons.len(), 2, "項目の数だけラジオボタンがあること");
        // MinWidth の値でなく、実際に占める幅で確かめる。Fluent の既定スタイル
        // を引き継げていないと、MinWidth が 0 でもテンプレートが 154 を取る。
        let switch_width = switch.ActualWidth().unwrap_or(f64::NAN);
        assert!(
            switch_width > 0.0 && switch_width < 154.0,
            "ToggleSwitch が既定の最小幅 154 より狭いこと: {switch_width}"
        );
        for button in &buttons {
            let width = button.ActualWidth().unwrap_or(f64::NAN);
            assert!(
                width > 0.0 && width < 120.0,
                "RadioButton が既定の最小幅 120 より狭いこと: {width}"
            );
        }
        // 横に並べた項目の間は、WinUI の RadioButtons と同じ 7 だけ空く。
        let left = |element: &FrameworkElement| {
            element
                .cast::<UIElement>()
                .and_then(|element| element.ActualOffset())
                .map(|offset| f64::from(offset.X))
                .unwrap_or(f64::NAN)
        };
        let first_end = left(&buttons[0]) + buttons[0].ActualWidth().unwrap_or(f64::NAN);
        let gap = left(&buttons[1]) - first_end;
        assert!(
            (gap - 7.0).abs() < 0.5,
            "横に並べたラジオの項目の間が 7 空くこと: {gap}"
        );
    }));
    window.close();
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
    Ok(())
}

/// A `Fill` child with a maximum grows up to it and then sits where an `Auto` child
/// would: at the start of the cell horizontally, in the middle vertically. This is
/// how GTK, macOS and the Web place it. WinUI alone would center a `Stretch` element
/// stopped by `MaxWidth`, so naui puts it into a one-column Grid.
///
/// Removing it from the cell also takes it out of that Grid, so it can be placed
/// again. Messages are pumped until it is laid out; in environments where it never
/// is (CI), the sizes and positions pass without checking.
fn grid_places_a_capped_fill_child_like_an_auto_one(ui: &Ui) -> Result<()> {
    let window = ui.window("上限付きの Fill", 600.0, 400.0)?;
    let grid = ui.grid()?;
    grid.set_column_track(0, naui_core::Track::FILL);
    grid.set_row_track(0, naui_core::Track::FILL);
    grid.set_sizing(Sizing::fill());
    let capped = ui.stack(Orientation::Vertical)?;
    capped.set_sizing(Sizing::fill().max_width(200.0).max_height(100.0));
    let cell = GridCell::new(0, 0);
    grid.attach(&capped, cell);
    window.set_child(&grid);
    window.show();

    let child = native::<FrameworkElement>(&capped);
    let root = native::<Grid>(&grid);
    let laid_out = || child.ActualWidth().unwrap_or(0.0) > 0.0;
    pump_until(SCROLL_LAYOUT_TIMEOUT, laid_out);

    let outcome = catch_unwind(AssertUnwindSafe(|| {
        if !laid_out() {
            println!("     (実寸が付かない環境なので、大きさと位置は確かめずに通す)");
            return;
        }
        let size = (
            child.ActualWidth().unwrap_or(f64::NAN),
            child.ActualHeight().unwrap_or(f64::NAN),
        );
        assert_eq!(size, (200.0, 100.0), "上限まで広がる");
        let origin = child
            .TransformToVisual(&root)
            .and_then(|transform| {
                transform.TransformPoint(windows::Foundation::Point { X: 0.0, Y: 0.0 })
            })
            .expect("グリッドから見た位置");
        assert!(
            f64::from(origin.X).abs() < 0.5,
            "横はマスの先頭: {origin:?}"
        );
        // The cell's height can be odd, and layout rounding then moves the middle to a
        // whole pixel (149.5 becomes 150), so allow up to one pixel.
        let middle = (root.ActualHeight().unwrap_or(f64::NAN) - 100.0) / 2.0;
        assert!(
            (f64::from(origin.Y) - middle).abs() <= 1.0,
            "縦はマスの中央 ({middle}): {origin:?}"
        );
    }));

    grid.remove(cell);
    // 包みから出ていれば、ほかのパネルへそのまま足せる。
    let element = native::<UIElement>(&capped);
    let released = StackPanel::new()
        .and_then(|panel| panel.Children())
        .map(|children| {
            let appended = children.Append(&element).is_ok();
            let _ = children.Clear();
            appended
        })
        .unwrap_or(false);
    let other = ui.grid()?;
    other.attach(&capped, cell);
    // ウィンドウに載っていないグリッドの中では Parent が null を返すので、
    // 包みの Grid の中身を直接見る。
    let identity = |element: &UIElement| element.cast::<windows_core::IUnknown>().ok();
    let placed_again = other.len() == 1
        && native::<Grid>(&other)
            .Children()
            .and_then(|children| children.GetAt(0))
            .and_then(|wrapper| wrapper.cast::<Grid>())
            .and_then(|wrapper| wrapper.Children())
            .and_then(|children| children.GetAt(0))
            .is_ok_and(|inner| identity(&inner) == identity(&element));
    window.close();
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
    assert!(released, "外すと包みからも出る");
    assert!(placed_again, "外したあとは別のグリッドへ置ける");
    Ok(())
}

/// `set_sizing` after the child is placed in a Grid replaces the maximum width it
/// was placed with: a new maximum, no maximum, and a maximum given to a child placed
/// without one.
///
/// Whether the child is wrapped is checked everywhere. Messages are pumped until it
/// is laid out; in environments where it never is (CI), the widths and positions
/// pass without checking.
fn grid_follows_a_changed_maximum_width(ui: &Ui) -> Result<()> {
    let window = ui.window("上限の変更", 600.0, 400.0)?;
    let grid = ui.grid()?;
    grid.set_column_track(0, naui_core::Track::FILL);
    grid.set_row_track(0, naui_core::Track::FILL);
    grid.set_sizing(Sizing::fill());
    let capped = ui.stack(Orientation::Vertical)?;
    capped.set_sizing(Sizing::fill().max_width(200.0));
    grid.attach(&capped, GridCell::new(0, 0));
    window.set_child(&grid);
    window.show();

    let child = native::<FrameworkElement>(&capped);
    let element = native::<UIElement>(&capped);
    let root = native::<Grid>(&grid);
    let identity = |element: &UIElement| element.cast::<windows_core::IUnknown>().ok();
    let wrapped = || {
        root.Children()
            .and_then(|children| children.GetAt(0))
            .is_ok_and(|placed| identity(&placed) != identity(&element))
    };
    let width = || child.ActualWidth().unwrap_or(0.0);
    let left = || {
        child
            .TransformToVisual(&root)
            .and_then(|transform| {
                transform.TransformPoint(windows::Foundation::Point { X: 0.0, Y: 0.0 })
            })
            .map(|origin| f64::from(origin.X))
            .unwrap_or(f64::NAN)
    };
    pump_until(SCROLL_LAYOUT_TIMEOUT, || width() > 0.0);
    let laid_out = width() > 0.0;

    let outcome = catch_unwind(AssertUnwindSafe(|| {
        if !laid_out {
            println!("     (実寸が付かない環境なので、幅と位置は確かめずに通す)");
        }
        assert!(wrapped(), "上限付きで置くと包まれる");

        capped.set_sizing(Sizing::fill().max_width(400.0));
        assert!(wrapped(), "上限を変えても包まれたまま");
        if laid_out {
            pump_until(SCROLL_LAYOUT_TIMEOUT, || width() == 400.0);
            assert_eq!(width(), 400.0, "新しい上限まで広がる");
            assert!(left().abs() < 0.5, "横はマスの先頭: {}", left());
        }

        capped.set_sizing(Sizing::fill());
        assert!(!wrapped(), "上限を外すと包みから出る");
        if laid_out {
            let cell = root.ActualWidth().unwrap_or(f64::NAN);
            pump_until(SCROLL_LAYOUT_TIMEOUT, || width() == cell);
            assert_eq!(width(), cell, "上限を外すとマスいっぱいに広がる");
        }

        capped.set_sizing(Sizing::fill().max_width(150.0));
        assert!(wrapped(), "置いたあとで上限を付けると包まれる");
        if laid_out {
            pump_until(SCROLL_LAYOUT_TIMEOUT, || width() == 150.0);
            assert_eq!(width(), 150.0, "付けた上限まで広がる");
            assert!(left().abs() < 0.5, "横はマスの先頭: {}", left());
        }
    }));
    window.close();
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
    Ok(())
}

/// The width and height given to `ui.window` and `set_size` are the same content size
/// as the `size` of [`window_size_is_the_content_row`] (excluding the title bar, menu
/// bar and frame). The outer frame is passed to `AppWindow.Resize`, so the outer
/// frame is larger by that much.
///
/// The menu bar's height is not known until it is laid out, so it is refitted once
/// after showing. Messages are pumped until the row has an actual size; in
/// environments where it never gets one (CI), it passes without checking.
fn window_set_size_is_the_content_size(ui: &Ui) -> Result<()> {
    let window = ui.window("大きさの指定", 900.0, 640.0)?;
    assert_eq!(
        window.size(),
        (900.0, 640.0),
        "表示の前は ui.window の大きさ"
    );
    let stack = ui.stack(Orientation::Vertical)?;
    stack.append(&ui.label("中身")?);
    window.set_child(&stack);
    let menu_bar = ui.menu_bar()?;
    menu_bar.set_menus(&[MenuSpec::new("ファイル", ["新規"])]);
    window.set_menu_bar(&menu_bar);
    window.show();
    let content_row = window
        .native_window()
        .Content()
        .expect("ウィンドウの中身")
        .cast::<Grid>()
        .expect("根は Grid")
        .Children()
        .expect("根の子")
        .GetAt(2)
        .expect("中身の行")
        .cast::<FrameworkElement>()
        .expect("行は FrameworkElement");
    let measure = || {
        (
            content_row.ActualWidth().unwrap_or(0.0),
            content_row.ActualHeight().unwrap_or(0.0),
        )
    };
    let near = |a: (f64, f64), b: (f64, f64)| (a.0 - b.0).abs() < 1.0 && (a.1 - b.1).abs() < 1.0;
    pump_until(SCROLL_LAYOUT_TIMEOUT, || near(measure(), (900.0, 640.0)));

    let outcome = catch_unwind(AssertUnwindSafe(|| {
        if measure().1 <= 0.0 {
            println!("     (中身の行に実寸が付かない環境なので、確かめずに通す)");
            return;
        }
        assert!(
            near(window.size(), (900.0, 640.0)),
            "表示した中身 {:?} が ui.window の 900×640 であること",
            window.size()
        );
        let scale = content_row
            .XamlRoot()
            .and_then(|root| root.RasterizationScale())
            .expect("RasterizationScale");
        let outer = window
            .native_window()
            .AppWindow()
            .and_then(|app_window| app_window.Size())
            .expect("AppWindow.Size");
        assert!(
            f64::from(outer.Width) > 900.0 * scale
                && f64::from(outer.Height) - 640.0 * scale > 32.0,
            "外枠 {}×{} が中身より枠・タイトルバー・メニューバーの分だけ大きいこと",
            outer.Width,
            outer.Height
        );

        // 表示した後に変えても、中身の大きさになる。
        window.set_size(500.0, 400.0);
        pump_until(SCROLL_LAYOUT_TIMEOUT, || near(measure(), (500.0, 400.0)));
        assert!(
            near(window.size(), (500.0, 400.0)),
            "set_size 後の中身 {:?} が 500×400 であること",
            window.size()
        );
    }));
    window.close();
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
    Ok(())
}

/// The position is the outer frame's top left counted right and down from the
/// primary screen's top left (DIP). `AppWindow.Position` is in physical pixels, so it
/// is the value multiplied by `RasterizationScale`.
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
    pump_until(Duration::from_millis(300), || false);
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        assert!(
            near(window.position(), (120.0, 140.0)),
            "表示の後: {:?}",
            window.position()
        );
        window.set_position(200.0, 160.0);
        assert!(
            near(window.position(), (200.0, 160.0)),
            "表示中に動かす: {:?}",
            window.position()
        );
        let native = window
            .native_window()
            .AppWindow()
            .and_then(|app_window| app_window.Position())
            .expect("AppWindow.Position");
        let (x, _) = window.position().expect("位置");
        assert!(
            f64::from(native.X) >= x - 1.0,
            "物理ピクセルは DIP 以上: {} / {x}",
            native.X
        );
    }));
    window.close();
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
    Ok(())
}

/// Maximize and minimize (`OverlappedPresenter`) and fullscreen
/// (`FullScreenPresenter`) round-trip with `state`, and `on_state_change` is called
/// on each change.
///
/// In sessions without a screen, maximizing may not work, in which case it passes
/// without checking (the same as the other window cases).
fn window_state_round_trips(ui: &Ui) -> Result<()> {
    let window = ui.window("状態", 320.0, 200.0)?;
    window.set_child(&ui.label("中身")?);
    let seen = Rc::new(RefCell::new(Vec::new()));
    window.on_state_change({
        let seen = seen.clone();
        move |state| seen.borrow_mut().push(state)
    });
    window.show();
    pump_until(Duration::from_millis(300), || false);
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        assert_eq!(window.state(), WindowState::Normal);
        let steps = [
            WindowState::Maximized,
            WindowState::Normal,
            WindowState::Minimized,
            WindowState::Normal,
            WindowState::Fullscreen,
            // 全画面から出るときは、最初の OverlappedPresenter を付け直す。
            WindowState::Maximized,
            WindowState::Normal,
        ];
        for (index, step) in steps.into_iter().enumerate() {
            window.set_state(step);
            pump_until(SCROLL_LAYOUT_TIMEOUT, || window.state() == step);
            if index == 0 && window.state() != step {
                println!("     (最大化が効かない環境なので、確かめずに通す)");
                return;
            }
            assert_eq!(window.state(), step, "{index} 番目");
        }
        // 全画面から出るときは、OverlappedPresenter を付け直したところで
        // いったん Normal が届くことがある。
        let seen = seen.borrow();
        assert_eq!(seen.first(), Some(&WindowState::Maximized), "{seen:?}");
        assert_eq!(seen.last(), Some(&WindowState::Normal), "{seen:?}");
        for state in [WindowState::Minimized, WindowState::Fullscreen] {
            assert!(seen.contains(&state), "{state:?} も届く: {seen:?}");
        }
        assert!(
            seen.windows(2).all(|pair| pair[0] != pair[1]),
            "同じ状態を続けて知らせない: {seen:?}"
        );
        let presenter = window
            .native_window()
            .AppWindow()
            .and_then(|app_window| app_window.Presenter())
            .and_then(|presenter| presenter.Kind())
            .expect("Presenter.Kind");
        assert_eq!(
            presenter,
            naui_winui3::Microsoft::UI::Windowing::AppWindowPresenterKind::Overlapped
        );
    }));
    window.close();
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
    Ok(())
}

/// While maximized, minimized or in fullscreen, `position` returns the position for
/// returning to normal, and `set_position` decides that position without changing
/// the state or the visible frame.
fn window_position_while_not_normal(ui: &Ui) -> Result<()> {
    let near = |a: Option<(f64, f64)>, b: (f64, f64)| {
        a.is_some_and(|a| (a.0 - b.0).abs() < 1.0 && (a.1 - b.1).abs() < 1.0)
    };
    let window = ui.window("状態の間の位置", 320.0, 200.0)?;
    window.set_child(&ui.label("中身")?);
    window.set_position(120.0, 140.0);
    window.show();
    pump_until(Duration::from_millis(300), || false);
    let native_position = || {
        window
            .native_window()
            .AppWindow()
            .and_then(|app_window| app_window.Position())
            .map(|position| (position.X, position.Y))
            .expect("AppWindow.Position")
    };
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        let mut normal = (120.0, 140.0);
        for (index, state) in [
            WindowState::Maximized,
            WindowState::Minimized,
            WindowState::Fullscreen,
        ]
        .into_iter()
        .enumerate()
        {
            window.set_state(state);
            pump_until(SCROLL_LAYOUT_TIMEOUT, || window.state() == state);
            if index == 0 && window.state() != state {
                println!("     (最大化が効かない環境なので、確かめずに通す)");
                return;
            }
            assert_eq!(window.state(), state);
            assert!(
                near(window.position(), normal),
                "{state:?} の間は元の位置: {:?}",
                window.position()
            );
            let shown = native_position();
            normal = (200.0 + 20.0 * index as f64, 160.0 + 10.0 * index as f64);
            window.set_position(normal.0, normal.1);
            pump_until(Duration::from_millis(200), || false);
            assert_eq!(window.state(), state, "{state:?} のまま");
            assert_eq!(native_position(), shown, "{state:?} の枠は動かない");
            assert!(
                near(window.position(), normal),
                "{state:?} の間に決めた位置: {:?}",
                window.position()
            );
            window.set_state(WindowState::Normal);
            pump_until(SCROLL_LAYOUT_TIMEOUT, || {
                window.state() == WindowState::Normal
            });
            assert_eq!(window.state(), WindowState::Normal);
            assert!(
                near(window.position(), normal),
                "{state:?} から戻ると決めた位置: {:?}",
                window.position()
            );
        }
        // 全画面の間に決めた位置は、全画面から最大化へ移っても元に戻す先になる。
        window.set_state(WindowState::Fullscreen);
        pump_until(SCROLL_LAYOUT_TIMEOUT, || {
            window.state() == WindowState::Fullscreen
        });
        window.set_position(260.0, 190.0);
        window.set_state(WindowState::Maximized);
        pump_until(SCROLL_LAYOUT_TIMEOUT, || {
            window.state() == WindowState::Maximized
        });
        assert_eq!(window.state(), WindowState::Maximized);
        assert!(
            near(window.position(), (260.0, 190.0)),
            "全画面から最大化: {:?}",
            window.position()
        );
        window.set_state(WindowState::Normal);
        pump_until(SCROLL_LAYOUT_TIMEOUT, || {
            window.state() == WindowState::Normal
        });
        assert!(
            near(window.position(), (260.0, 190.0)),
            "最大化から戻る: {:?}",
            window.position()
        );
    }));
    window.close();
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
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
    pump_until(SCROLL_LAYOUT_TIMEOUT, || {
        near(window.size(), (320.0, 200.0))
    });
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        let placement = window.placement();
        assert!(near(placement.size, (320.0, 200.0)), "{placement:?}");
        assert!(
            placement
                .position
                .is_some_and(|position| near(position, (120.0, 140.0))),
            "{placement:?}"
        );
        assert_eq!(placement.state, WindowState::Normal);

        let mut normal = (320.0, 200.0);
        for (index, state) in [
            WindowState::Maximized,
            WindowState::Minimized,
            WindowState::Fullscreen,
        ]
        .into_iter()
        .enumerate()
        {
            window.set_state(state);
            pump_until(SCROLL_LAYOUT_TIMEOUT, || window.state() == state);
            if index == 0 && window.state() != state {
                println!("     (最大化が効かない環境なので、確かめずに通す)");
                return;
            }
            assert_eq!(window.state(), state);
            let placement = window.placement();
            assert!(
                near(placement.size, normal),
                "{state:?} の間は元の大きさ: {placement:?}"
            );
            normal = (360.0 + 20.0 * index as f64, 240.0 + 10.0 * index as f64);
            window.set_size(normal.0, normal.1);
            pump_until(Duration::from_millis(200), || false);
            assert_eq!(window.state(), state, "{state:?} のまま");
            assert!(
                near(window.placement().size, normal),
                "{state:?} の間に決めた大きさ: {:?}",
                window.placement()
            );
            window.set_state(WindowState::Normal);
            pump_until(SCROLL_LAYOUT_TIMEOUT, || {
                window.state() == WindowState::Normal && near(window.size(), normal)
            });
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
        pump_until(SCROLL_LAYOUT_TIMEOUT, || {
            window.state() == WindowState::Maximized
        });
        window.set_state(WindowState::Minimized);
        pump_until(SCROLL_LAYOUT_TIMEOUT, || {
            window.state() == WindowState::Minimized
        });
        assert_eq!(window.state(), WindowState::Minimized);
        let placement = window.placement();
        assert_eq!(placement.state, WindowState::Maximized, "{placement:?}");
        assert!(near(placement.size, normal), "{placement:?}");
    }));
    window.close();
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
    Ok(())
}

/// A placement read back from text and applied before showing puts the window in
/// that frame and state, and a position on no connected monitor is skipped.
fn window_placement_restores_before_show(ui: &Ui) -> Result<()> {
    let near = |a: (f64, f64), b: (f64, f64)| (a.0 - b.0).abs() < 1.0 && (a.1 - b.1).abs() < 1.0;
    let saved =
        WindowPlacement::parse("size=380x260 position=150,170 state=maximized").expect("読める");
    let window = ui.window("戻す枠", 320.0, 200.0)?;
    window.set_child(&ui.label("中身")?);
    window.set_placement(&saved);
    let outcome = catch_unwind(AssertUnwindSafe(|| {
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
        pump_until(SCROLL_LAYOUT_TIMEOUT, || {
            window.state() == WindowState::Maximized
        });
        if window.state() != WindowState::Maximized {
            println!("     (最大化が効かない環境なので、確かめずに通す)");
            return;
        }
        let placement = window.placement();
        assert!(near(placement.size, saved.size), "{placement:?}");
        window.set_state(WindowState::Normal);
        pump_until(SCROLL_LAYOUT_TIMEOUT, || {
            window.state() == WindowState::Normal && near(window.size(), saved.size)
        });
        assert!(near(window.size(), saved.size), "{:?}", window.size());
        assert!(
            window
                .position()
                .is_some_and(|position| near(position, (150.0, 170.0))),
            "{:?}",
            window.position()
        );
    }));
    window.close();
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }

    // つながっていない画面の位置は使わない。
    let window = ui.window("外れた画面", 320.0, 200.0)?;
    window.set_child(&ui.label("中身")?);
    let before = window.position();
    window.set_placement(&WindowPlacement {
        size: (320.0, 200.0),
        position: Some((-100_000.0, -100_000.0)),
        state: WindowState::Normal,
    });
    let after = window.position();
    window.close();
    assert_eq!(after, before);
    Ok(())
}

/// Without a child there is no content layer to wait for, so a size changed in the
/// normal state is still what `placement` reports once maximized.
fn window_placement_without_child(ui: &Ui) -> Result<()> {
    let near = |a: (f64, f64), b: (f64, f64)| (a.0 - b.0).abs() < 1.0 && (a.1 - b.1).abs() < 1.0;
    let window = ui.window("中身の無い枠", 320.0, 200.0)?;
    window.show();
    pump_until(SCROLL_LAYOUT_TIMEOUT, || {
        near(window.size(), (320.0, 200.0))
    });
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        window.set_size(640.0, 480.0);
        pump_until(SCROLL_LAYOUT_TIMEOUT, || {
            near(window.size(), (640.0, 480.0))
        });
        assert!(near(window.size(), (640.0, 480.0)), "{:?}", window.size());
        window.set_state(WindowState::Maximized);
        pump_until(SCROLL_LAYOUT_TIMEOUT, || {
            window.state() == WindowState::Maximized
        });
        if window.state() != WindowState::Maximized {
            println!("     (最大化が効かない環境なので、確かめずに通す)");
            return;
        }
        let placement = window.placement();
        assert!(near(placement.size, (640.0, 480.0)), "{placement:?}");
    }));
    window.close();
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
    Ok(())
}

/// `set_state` before showing is remembered and applied when shown.
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
    pump_until(SCROLL_LAYOUT_TIMEOUT, || {
        window.state() == WindowState::Maximized
    });
    let state = window.state();
    window.close();
    if state != WindowState::Maximized {
        println!("     (最大化が効かない環境なので、確かめずに通す)");
        return Ok(());
    }
    Ok(())
}

/// `set_resizable(false)` before showing still takes effect after showing.
///
/// Before the first show, `AppWindow.Presenter` may be empty, and even then the
/// requested value is not dropped but applied in `show`. Even if it is not empty in
/// this environment, that the value is kept across showing can still be checked.
fn window_resizable_requested_before_show(ui: &Ui) -> Result<()> {
    use naui_winui3::Microsoft::UI::Windowing::OverlappedPresenter;
    let window = ui.window("表示前の大きさの固定", 320.0, 200.0)?;
    window.set_child(&ui.label("中身")?);
    window.set_resizable(false);
    assert!(!window.is_resizable(), "表示の前");
    window.show();
    pump_until(Duration::from_millis(300), || false);
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        assert!(!window.is_resizable(), "表示の後");
        let presenter = window
            .native_window()
            .AppWindow()
            .and_then(|app_window| app_window.Presenter())
            .and_then(|presenter| presenter.cast::<OverlappedPresenter>())
            .expect("OverlappedPresenter");
        assert_eq!(presenter.IsResizable().ok(), Some(false));
        assert_eq!(presenter.IsMaximizable().ok(), Some(false));
    }));
    window.close();
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
    Ok(())
}

fn window_resizable_round_trips(ui: &Ui) -> Result<()> {
    use naui_winui3::Microsoft::UI::Windowing::OverlappedPresenter;
    let window = ui.window("大きさの固定", 320.0, 200.0)?;
    window.set_child(&ui.label("中身")?);
    let presenter = window
        .native_window()
        .AppWindow()
        .and_then(|app_window| app_window.Presenter())
        .and_then(|presenter| presenter.cast::<OverlappedPresenter>())
        .expect("OverlappedPresenter");
    assert!(window.is_resizable());
    window.set_resizable(false);
    assert!(!window.is_resizable());
    assert_eq!(presenter.IsResizable().ok(), Some(false));
    assert_eq!(
        presenter.IsMaximizable().ok(),
        Some(false),
        "最大化のボタンも消える"
    );
    window.set_resizable(true);
    assert!(window.is_resizable());
    assert_eq!(presenter.IsResizable().ok(), Some(true));
    assert_eq!(presenter.IsMaximizable().ok(), Some(true));
    window.close();
    Ok(())
}

/// `run` makes the process Per-Monitor V2.
///
/// The test executable has no DPI manifest, so unless naui chooses, it stays DPI
/// unaware (`DPI_AWARENESS_UNAWARE`), and on a screen at 125% or so the OS stretches
/// a screen drawn at 96 DPI and text is blurred. `RasterizationScale` and
/// `GetDpiForWindow` then always return 1.0 / 96.
///
/// It also checks that the window (HWND) is created with the same awareness and that
/// XAML's scale matches that window's DPI. Whether the scale is other than 1 depends
/// on the screen settings, so it is not assumed here.
fn windows_are_per_monitor_v2_dpi_aware(ui: &Ui) -> Result<()> {
    use windows::Win32::UI::HiDpi::{
        AreDpiAwarenessContextsEqual, GetDpiForWindow, GetThreadDpiAwarenessContext,
        GetWindowDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
    };

    // SAFETY: どちらも UI スレッドの状態を読むだけ。
    let thread_is_v2 = unsafe {
        AreDpiAwarenessContextsEqual(
            GetThreadDpiAwarenessContext(),
            DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        )
    };
    assert!(
        thread_is_v2.as_bool(),
        "UI スレッドが Per-Monitor V2 であること"
    );

    let window = ui.window("表示倍率", 320.0, 240.0)?;
    let label = ui.label("倍率")?;
    window.set_child(&label);
    window.show();
    let native = window
        .native_window()
        .cast::<naui_winui3::IWindowNative>()
        .expect("IWindowNative");
    // SAFETY: HWND は window が閉じるまで有効で、UI スレッドから読んでいる。
    let hwnd = unsafe { native.WindowHandle() }.expect("HWND");
    let element = label.native_element().cast::<FrameworkElement>().ok();
    let scale = || {
        element
            .as_ref()
            .and_then(|element| element.XamlRoot().ok())
            .and_then(|root| root.RasterizationScale().ok())
    };
    pump_until(SCROLL_LAYOUT_TIMEOUT, || scale().is_some());

    let outcome = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: 上の HWND の状態を読むだけ。
        let window_is_v2 = unsafe {
            AreDpiAwarenessContextsEqual(
                GetWindowDpiAwarenessContext(hwnd),
                DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
            )
        };
        assert!(
            window_is_v2.as_bool(),
            "ウィンドウが Per-Monitor V2 で作られていること"
        );
        let Some(scale) = scale() else {
            println!("     (XamlRoot が付かない環境なので、倍率は確かめずに通す)");
            return;
        };
        // SAFETY: 上の HWND を渡すだけ。
        let dpi = unsafe { GetDpiForWindow(hwnd) };
        println!("     (このウィンドウの DPI は {dpi}、倍率は {scale})");
        assert!(
            (scale - f64::from(dpi) / 96.0).abs() < 0.01,
            "RasterizationScale {scale} が GetDpiForWindow {dpi} / 96 と合うこと"
        );
    }));
    window.close();
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
    Ok(())
}

// -------------------------------------------------------------- メニューバー

/// The number of headings laid out in the `StackPanel`.
fn title_count(menu_bar: &naui_windows::MenuBar) -> u32 {
    menu_bar
        .native_panel()
        .Children()
        .expect("見出しの入れ物")
        .Size()
        .expect("見出しの数")
}

/// The `MenuFlyoutItem` of an ordinary item. `None` for separators, out of range
/// and the other kinds.
fn flyout_item(menu_bar: &naui_windows::MenuBar, path: &[usize]) -> Option<MenuFlyoutItem> {
    match menu_bar.native_item(path)? {
        MenuEntry::Item(item) => Some(item),
        _ => None,
    }
}

/// Headings become `Button` + `MenuFlyout`, and items become `MenuFlyoutItem`s.
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
    assert_eq!(title_count(&menu_bar), 2, "見出しの数だけボタンが並ぶ");

    let flyout = menu_bar.native_flyout(0).expect("メニュー");
    assert_eq!(
        flyout.Items().expect("項目").Size().expect("項目の数"),
        3,
        "区切り線も 1 つとして並ぶ"
    );

    let first = flyout_item(&menu_bar, &[0, 0]).expect("先頭は項目");
    assert_eq!(first.Text().expect("文字").to_string(), "新規");
    assert!(first.IsEnabled().expect("有効かどうか"));
    assert_eq!(
        first
            .KeyboardAcceleratorTextOverride()
            .expect("右端の表示")
            .to_string(),
        "Ctrl+N",
        "ショートカットの表示は naui が添える"
    );
    assert!(
        menu_bar.native_item(&[0, 1]).is_none(),
        "区切り線に項目は無い"
    );

    let save = flyout_item(&menu_bar, &[0, 2]).expect("3 番目は項目");
    assert!(!save.IsEnabled().expect("有効かどうか"));
    assert_eq!(
        save.KeyboardAcceleratorTextOverride()
            .expect("右端の表示")
            .to_string(),
        "Ctrl+Shift+S"
    );
    assert!(menu_bar.native_item(&[9, 0]).is_none(), "範囲外は None");
    assert!(menu_bar.native_item(&[0, 9]).is_none(), "範囲外は None");
    assert!(menu_bar.native_flyout(9).is_none(), "範囲外は None");

    // ショートカットを指定しなければ右端には何も出ない。
    assert_eq!(
        flyout_item(&menu_bar, &[1, 0])
            .expect("項目")
            .KeyboardAcceleratorTextOverride()
            .expect("右端の表示")
            .to_string(),
        ""
    );

    assert!(menu_bar.is_item_enabled(&[0, 0]));
    assert!(!menu_bar.is_item_enabled(&[0, 1]), "区切り線は押せない");
    assert!(!menu_bar.is_item_enabled(&[0, 2]));

    // 項目ごとの指定と全体の指定は AND を取る。
    menu_bar.set_item_enabled(&[0, 2], true);
    assert!(flyout_item(&menu_bar, &[0, 2])
        .expect("項目")
        .IsEnabled()
        .expect("有効かどうか"));
    menu_bar.set_enabled(false);
    assert!(!menu_bar.is_item_enabled(&[0, 0]));
    assert!(!flyout_item(&menu_bar, &[0, 0])
        .expect("項目")
        .IsEnabled()
        .expect("有効かどうか"));
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
    assert_eq!(title_count(&menu_bar), 0);
    Ok(())
}

/// The clicked item arrives as a pair of (heading, item) indices.
///
/// `MenuFlyoutItem`'s `Click` cannot be triggered without showing the menu
/// (`MenuFlyout` provides peers for its items only once it is on screen). This looks
/// only at naui's path; the wiring from real clicks is verified by the integration
/// tests on macOS / GTK / Web, and on Windows by running the Gallery.
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
    let sink = seen.clone();
    menu_bar.on_activate(move |path| sink.borrow_mut().push(path.to_vec()));

    menu_bar.activate(&[0, 0]);
    assert_eq!(seen.borrow().as_slice(), [vec![0, 0]].as_slice());
    menu_bar.activate(&[0, 1]);
    menu_bar.activate(&[0, 2]);
    menu_bar.activate(&[9, 0]);
    menu_bar.activate(&[0, 9]);
    assert_eq!(
        seen.borrow().as_slice(),
        [vec![0, 0]].as_slice(),
        "区切り線・押せない項目・範囲外は通知しない"
    );

    menu_bar.activate(&[1, 0]);
    assert_eq!(
        seen.borrow().as_slice(),
        [vec![0, 0], vec![1, 0]].as_slice()
    );

    menu_bar.set_item_enabled(&[0, 2], true);
    menu_bar.activate(&[0, 2]);
    assert_eq!(
        seen.borrow().as_slice(),
        [vec![0, 0], vec![1, 0], vec![0, 2]].as_slice()
    );

    menu_bar.set_enabled(false);
    menu_bar.activate(&[0, 0]);
    assert_eq!(
        seen.borrow().as_slice(),
        [vec![0, 0], vec![1, 0], vec![0, 2]].as_slice(),
        "無効なメニューバーは通知しない"
    );
    Ok(())
}

/// A check item becomes a `ToggleMenuFlyoutItem` and a submenu a `MenuFlyoutSubItem`
/// holding its items. Function keys show their own names at the right edge.
fn menu_bar_submenus_and_checks_map_to_native(ui: &Ui) -> Result<()> {
    let menu_bar = ui.menu_bar()?;
    menu_bar.set_menus(&[MenuSpec::new(
        "表示",
        [
            MenuItem::new("サイドバー").checked(true),
            MenuItem::submenu(
                "拡大",
                [
                    MenuItem::new("拡大"),
                    MenuItem::new("開発")
                        .shortcut(MenuShortcut::function(12).primary(true).shift(true)),
                ],
            ),
            MenuItem::new("全画面").shortcut(MenuShortcut::function(11)),
        ],
    )]);

    let Some(MenuEntry::Toggle(sidebar)) = menu_bar.native_item(&[0, 0]) else {
        panic!("印のある項目は ToggleMenuFlyoutItem になること");
    };
    assert!(sidebar.IsChecked().expect("印"));
    let Some(MenuEntry::Submenu(zoom)) = menu_bar.native_item(&[0, 1]) else {
        panic!("サブメニューは MenuFlyoutSubItem になること");
    };
    assert_eq!(zoom.Text().expect("文字").to_string(), "拡大");
    assert_eq!(zoom.Items().expect("項目").Size().expect("数"), 2);
    let develop = flyout_item(&menu_bar, &[0, 1, 1]).expect("サブメニューの中の項目");
    assert_eq!(
        develop
            .KeyboardAcceleratorTextOverride()
            .expect("右端の表示")
            .to_string(),
        "Ctrl+Shift+F12"
    );
    assert_eq!(
        flyout_item(&menu_bar, &[0, 2])
            .expect("項目")
            .KeyboardAcceleratorTextOverride()
            .expect("右端の表示")
            .to_string(),
        "F11"
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
    menu_bar.activate(&[0, 0]);
    assert!(!sidebar.IsChecked().expect("印"), "選ぶと印が反転する");
    menu_bar.activate(&[0, 1]);
    menu_bar.activate(&[0, 1, 0]);
    assert_eq!(
        *seen.borrow(),
        vec![(vec![0, 0], false), (vec![0, 1, 0], false)],
        "サブメニューの見出しは選べない"
    );

    menu_bar.set_item_checked(&[0, 0], true);
    assert!(sidebar.IsChecked().expect("印"));
    assert_eq!(seen.borrow().len(), 2, "set_item_checked は通知しない");

    // サブメニューを止めると、中の項目も止まる。
    menu_bar.set_item_enabled(&[0, 1], false);
    assert!(!zoom.IsEnabled().expect("有効かどうか"));
    assert!(!develop.IsEnabled().expect("有効かどうか"));
    assert!(!menu_bar.is_item_enabled(&[0, 1, 0]));
    menu_bar.set_item_enabled(&[0, 1], true);
    assert!(develop.IsEnabled().expect("有効かどうか"));
    Ok(())
}

// -------------------------------------------------------------- サイドバー

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

fn sidebar_items_map_to_native(ui: &Ui) -> Result<()> {
    use naui_winui3::Microsoft::UI::Xaml::Controls::{
        NavigationViewItem, NavigationViewItemHeader, NavigationViewItemSeparator,
        NavigationViewPaneDisplayMode,
    };

    let sidebar = ui.sidebar()?;
    assert!(sidebar.is_empty());
    sidebar.set_sections(&sidebar_fixture());
    assert_eq!(sidebar.len(), 4, "見出しと区切りは数えない");

    let native = sidebar.native_navigation_view();
    let collapse = naui_core::SIDEBAR_COLLAPSE_CONTENT_WIDTH;
    assert!(sidebar.auto_collapse(), "既定で自動で畳む");
    assert_eq!(
        native.PaneDisplayMode().expect("表示形式"),
        NavigationViewPaneDisplayMode::Auto,
        "狭くなったら畳む判断は NavigationView に任せる"
    );
    assert_eq!(
        native.ExpandedModeThresholdWidth().expect("しきい値"),
        DEFAULT_SIDEBAR_WIDTH + collapse,
        "中身がこれより狭くなると帯になる"
    );
    assert_eq!(
        native.CompactModeThresholdWidth().expect("しきい値"),
        0.0,
        "帯ごと消える形 (LeftMinimal) にはしない"
    );
    sidebar.set_auto_collapse(false);
    assert!(!sidebar.auto_collapse());
    assert_eq!(
        native.PaneDisplayMode().expect("表示形式"),
        NavigationViewPaneDisplayMode::Left,
        "切ると左のペインのまま"
    );
    sidebar.set_auto_collapse(true);
    assert_eq!(
        native.PaneDisplayMode().expect("表示形式"),
        NavigationViewPaneDisplayMode::Auto
    );
    assert!(!native.IsSettingsVisible().expect("設定項目"));
    assert!(
        native.IsPaneToggleButtonVisible().expect("畳むボタン"),
        "標準の畳むボタンで開閉できる"
    );

    let menu = native.MenuItems().expect("項目");
    // 一般・検索 | 区切り | 場所 (見出し) | 書類・共有
    assert_eq!(menu.Size().expect("項目数"), 6);
    let at = |i: u32| menu.GetAt(i).expect("項目");
    assert!(at(2).cast::<NavigationViewItemSeparator>().is_ok());
    let header = at(3)
        .cast::<NavigationViewItemHeader>()
        .expect("見出しは NavigationViewItemHeader");
    let title = header
        .Content()
        .expect("見出しの中身")
        .cast::<IPropertyValue>()
        .expect("文字")
        .GetString()
        .expect("文字列");
    assert_eq!(title.to_string(), "場所");

    let first = at(0).cast::<NavigationViewItem>().expect("項目");
    let label = first
        .Content()
        .expect("項目の中身")
        .cast::<TextBlock>()
        .expect("文字は TextBlock");
    assert_eq!(
        label.TextTrimming().expect("省略"),
        naui_winui3::Microsoft::UI::Xaml::TextTrimming::CharacterEllipsis,
        "入りきらない文字は末尾を省略記号にする (字の途中で断ち切らない)"
    );
    // 空の参照は Err で返る。
    assert!(
        first.Icon().is_ok(),
        "アイコンを指定した項目は FontIcon を持つ"
    );
    let plain = at(1).cast::<NavigationViewItem>().expect("項目");
    assert!(plain.Icon().is_err(), "アイコンの無い項目は文字だけ");
    let shared = at(5).cast::<NavigationViewItem>().expect("項目");
    assert!(!shared.IsEnabled().expect("有効"), "選べない項目は無効");
    assert!(!shared.SelectsOnInvoked().expect("選ばれるか"));

    assert_eq!(native.OpenPaneLength().expect("幅"), DEFAULT_SIDEBAR_WIDTH);
    let divider = sidebar.native_divider();
    let divider_left = || divider.Margin().expect("仕切りの余白").Left;
    assert_eq!(
        divider_left(),
        DEFAULT_SIDEBAR_WIDTH - 3.0,
        "仕切りのつかみ代はペインの右端をまたぐ"
    );
    let resized = Rc::new(RefCell::new(Vec::new()));
    sidebar.on_resize({
        let resized = resized.clone();
        move |width| resized.borrow_mut().push(width)
    });
    sidebar.set_width(160.0);
    assert_eq!(native.OpenPaneLength().expect("幅"), 160.0);
    assert_eq!(divider_left(), 157.0, "仕切りもペインの端へ動く");
    assert_eq!(
        native.ExpandedModeThresholdWidth().expect("しきい値"),
        160.0 + collapse,
        "畳むしきい値も幅に付いてくる"
    );
    assert_eq!(sidebar.width(), 160.0);
    sidebar.set_width(-1.0);
    assert_eq!(sidebar.width(), 160.0, "おかしな幅は無視する");
    sidebar.set_width(40.0);
    assert_eq!(
        sidebar.width(),
        naui_core::SIDEBAR_MIN_WIDTH,
        "下限より狭くはならない"
    );
    sidebar.set_width(160.0);
    assert!(resized.borrow().is_empty(), "set_width では通知しない");

    let seen = Rc::new(RefCell::new(Vec::new()));
    sidebar.on_collapse({
        let seen = seen.clone();
        move |collapsed| seen.borrow_mut().push(collapsed)
    });
    assert!(!sidebar.is_collapsed());
    sidebar.set_collapsed(true);
    assert!(sidebar.is_collapsed());
    assert!(
        !native.IsPaneOpen().expect("ペイン"),
        "畳むとアイコンだけの帯になる (Windows の作法)"
    );
    assert!(native.IsPaneVisible().expect("ペイン"), "帯は残る");
    assert_eq!(
        sidebar.native_divider().Visibility().expect("仕切り"),
        naui_winui3::Microsoft::UI::Xaml::Visibility::Collapsed,
        "畳んだ帯の幅は変えられないので仕切りを隠す"
    );
    sidebar.set_collapsed(false);
    assert!(native.IsPaneOpen().expect("ペイン"));
    assert!(seen.borrow().is_empty(), "set_collapsed では通知しない");

    // 閉じている間に置いた幅が、開き直したときに使われる。
    sidebar.set_collapsed(true);
    sidebar.set_width(300.0);
    assert_eq!(sidebar.width(), 300.0);
    sidebar.set_collapsed(false);
    assert!(native.IsPaneOpen().expect("ペイン"));
    assert_eq!(
        native.OpenPaneLength().expect("幅"),
        300.0,
        "開き直すと閉じている間に置いた幅"
    );
    sidebar.set_width(160.0);

    sidebar.set_items(&SidebarItem::list(["春", "夏"]));
    assert_eq!(
        menu.Size().expect("項目数"),
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
    let native = sidebar.native_navigation_view();
    let menu = native.MenuItems().expect("項目");
    let same = |a: windows_core::IInspectable, b: windows_core::IInspectable| {
        a.cast::<windows_core::IUnknown>()
            .expect("IUnknown")
            .as_raw()
            == b.cast::<windows_core::IUnknown>()
                .expect("IUnknown")
                .as_raw()
    };

    sidebar.set_selected(2);
    assert_eq!(sidebar.selected(), Some(2));
    assert!(
        same(
            native.SelectedItem().expect("選択"),
            menu.GetAt(4).expect("書類")
        ),
        "書類の項目 (見出しの後ろ) が選ばれる"
    );
    assert!(seen.borrow().is_empty(), "set_selected は通知しない");
    sidebar.set_selected(3);
    sidebar.set_selected(9);
    assert_eq!(sidebar.selected(), Some(2), "選べない・範囲外は無視");

    sidebar.select(1);
    assert_eq!(*seen.borrow(), [1], "select は通知する");
    sidebar.select(1);
    assert_eq!(*seen.borrow(), [1, 1], "同じ項目でも通知する");

    sidebar.set_selected(2);
    sidebar.set_sections(&sidebar_fixture());
    assert_eq!(sidebar.selected(), Some(2), "同じ番号が選べれば残る");
    sidebar.set_items(&SidebarItem::list(["ひとつ"]));
    assert_eq!(sidebar.selected(), None, "無くなった番号の選択は外れる");
    sidebar.set_selected(0);
    sidebar.clear_selection();
    assert_eq!(sidebar.selected(), None);
    assert_eq!(seen.borrow().len(), 2, "差し替えと解除は通知しない");
    Ok(())
}

/// The sidebar goes at the head of the content row (`CONTENT_ROW`), and the child
/// moves to its `Content`. Removing it returns the child to the content row.
///
/// As in the toast case, the window is not closed and is left to the app's exit.
fn sidebar_takes_the_content_row(ui: &Ui) -> Result<Deferred> {
    let window = ui.window("サイドバー", 480.0, 320.0)?;
    let stack = ui.stack(Orientation::Vertical)?;
    stack.append(&ui.label("中身")?);
    window.set_child(&stack);

    let rows = window
        .native_window()
        .Content()
        .expect("ウィンドウの中身")
        .cast::<Grid>()
        .expect("根は Grid")
        .Children()
        .expect("根の子");
    let first_in_content_row = || -> UIElement {
        rows.GetAt(2)
            .expect("行")
            .cast::<Grid>()
            .expect("行は Grid")
            .Children()
            .expect("行の子")
            .GetAt(0)
            .expect("先頭の子")
    };
    let child = stack.native_element();
    assert_eq!(first_in_content_row(), child, "最初は子がそのまま入る");

    let sidebar = ui.sidebar()?;
    sidebar.set_items(&SidebarItem::list(["一般"]));
    window.set_sidebar(&sidebar);
    // 中身の行には、NavigationView と仕切りを重ねた Grid が入る。
    let host = first_in_content_row()
        .cast::<Grid>()
        .expect("サイドバーの Grid")
        .Children()
        .expect("子");
    let navigation = sidebar
        .native_navigation_view()
        .cast::<UIElement>()
        .expect("要素化");
    let divider = sidebar
        .native_divider()
        .cast::<UIElement>()
        .expect("要素化");
    assert_eq!(
        host.GetAt(0).expect("先頭"),
        navigation,
        "サイドバーが中身の行に入る"
    );
    assert_eq!(
        host.GetAt(1).expect("2 番目"),
        divider,
        "仕切りがその上に重なる"
    );
    let content = sidebar
        .native_navigation_view()
        .Content()
        .expect("右の区画")
        .cast::<UIElement>()
        .expect("要素");
    assert_eq!(content, child, "子は右の区画へ移る");

    // 付けたまま子を差し替えても、右の区画に置かれる。
    let other = ui.stack(Orientation::Vertical)?;
    window.set_child(&other);
    let content = sidebar
        .native_navigation_view()
        .Content()
        .expect("右の区画")
        .cast::<UIElement>()
        .expect("要素");
    assert_eq!(content, other.native_element());

    window.clear_sidebar();
    let rows = window
        .native_window()
        .Content()
        .expect("ウィンドウの中身")
        .cast::<Grid>()
        .expect("根は Grid")
        .Children()
        .expect("根の子");
    let head = rows
        .GetAt(2)
        .expect("行")
        .cast::<Grid>()
        .expect("行は Grid")
        .Children()
        .expect("行の子")
        .GetAt(0)
        .expect("先頭の子");
    assert_eq!(head, other.native_element(), "外すと子が中身の行へ戻る");
    assert!(
        sidebar.native_navigation_view().Content().is_err(),
        "サイドバーは中身を手放す (空の参照は Err で返る)"
    );
    window.clear_sidebar(); // 付いていなければ何もしない
    Ok(Box::new(|| Ok(())))
}

/// `AdaptivePanes` is a `TwoPaneView` set up from naui's values: the breakpoint as
/// `MinWideModeWidth`, the list's width as `Pane1Length`, never stacking the panes,
/// and `PanePriority` following `show`. It shows one pane below the breakpoint, even
/// when first laid out that narrow, and the back button, which has an accessible
/// name, returns to the list and notifies. Focus leaves the pane that is hidden.
fn adaptive_panes_follow_their_width(ui: &Ui) -> Result<()> {
    use naui_core::Pane;
    use naui_winui3::Microsoft::UI::Xaml::Automation::AutomationProperties;
    use naui_winui3::Microsoft::UI::Xaml::Controls::{
        Control, TwoPaneViewMode, TwoPaneViewPriority, TwoPaneViewTallModeConfiguration,
    };
    use naui_winui3::Microsoft::UI::Xaml::{DependencyObject, FocusState, Visibility};

    let window = ui.window("一覧と詳細", 900.0, 400.0)?;
    let outer = ui.stack(Orientation::Vertical)?;
    outer.set_sizing(Sizing::fixed(800.0, 300.0));
    let panes = ui.adaptive_panes()?;
    panes.set_sizing(Sizing::fill());
    let list = ui.button("一覧")?;
    let detail = ui.label("詳細")?;
    panes.set_list(&list);
    panes.set_detail(&detail);
    outer.append(&panes);
    window.set_child(&outer);
    let backs = Rc::new(Cell::new(0));
    panes.on_back({
        let backs = backs.clone();
        move || backs.set(backs.get() + 1)
    });

    let native = panes.native_two_pane_view();
    let back = panes.native_back_button();
    assert_eq!(
        native.MinWideModeWidth().expect("MinWideModeWidth"),
        naui_core::DEFAULT_PANES_BREAKPOINT
    );
    assert_eq!(
        native.Pane1Length().expect("Pane1Length").Value,
        naui_core::DEFAULT_LIST_PANE_WIDTH
    );
    assert_eq!(
        native
            .TallModeConfiguration()
            .expect("TallModeConfiguration"),
        TwoPaneViewTallModeConfiguration::SinglePane,
        "縦に積む並びは使わないこと"
    );
    assert!(!panes.is_collapsed(), "並ぶ前は 2 区画であること");
    assert_eq!(
        back.Visibility().expect("Visibility"),
        Visibility::Collapsed
    );

    panes.show(Pane::Detail);
    assert_eq!(
        native.PanePriority().expect("PanePriority"),
        TwoPaneViewPriority::Pane2,
        "show で PanePriority が変わること"
    );
    panes.set_breakpoint(700.0);
    assert_eq!(native.MinWideModeWidth().expect("MinWideModeWidth"), 700.0);
    panes.set_list_width(200.0);
    assert_eq!(native.Pane1Length().expect("Pane1Length").Value, 200.0);
    assert_eq!(panes.list_width(), 200.0);

    panes.go_back();
    assert_eq!(backs.get(), 1, "go_back で on_back が呼ばれること");
    assert_eq!(
        native.PanePriority().expect("PanePriority"),
        TwoPaneViewPriority::Pane1
    );

    assert_eq!(
        AutomationProperties::GetName(&back.cast::<DependencyObject>().expect("DependencyObject"))
            .expect("Name")
            .to_string(),
        naui_core::ui_strings().back,
        "戻るボタンは読み上げられる名前を持つこと"
    );

    // 最初の配置から狭い。TwoPaneView の Mode は SinglePane から変わらないので
    // ModeChanged は来ない。
    outer.set_sizing(Sizing::fixed(500.0, 300.0));
    window.show();
    pump_until(SCROLL_LAYOUT_TIMEOUT, || panes.is_collapsed());
    assert_eq!(native.Mode().expect("Mode"), TwoPaneViewMode::SinglePane);
    assert!(panes.is_collapsed(), "最初から狭くても 1 区画と分かること");
    assert_eq!(
        back.Visibility().expect("Visibility"),
        Visibility::Collapsed
    );

    // 一覧にフォーカスがあるまま詳細へ移ると、フォーカスは戻るボタンへ移る
    // (詳細はラベルだけで、フォーカスを受けない)。
    // 窓が前面に無くても (GotFocus が届かなくても) 移ること。
    let list_control = list.native_element().cast::<Control>().expect("Control");
    pump_until(FOCUS_TIMEOUT, || list.request_focus());
    assert_ne!(
        list_control.FocusState().expect("FocusState"),
        FocusState::Unfocused
    );

    panes.show(Pane::Detail);
    assert_eq!(
        back.Visibility().expect("Visibility"),
        Visibility::Visible,
        "詳細だけのときは戻るボタンが出ること"
    );
    assert_ne!(
        back.FocusState().expect("FocusState"),
        FocusState::Unfocused,
        "隠れた一覧からフォーカスが戻るボタンへ移ること"
    );

    automation::invoke(&back.cast::<UIElement>().expect("UIElement"));
    pump_until(Duration::from_millis(200), || backs.get() == 2);
    assert_eq!(backs.get(), 2, "戻るボタンで on_back が呼ばれること");
    assert_eq!(panes.visible_pane(), Pane::List);
    assert_eq!(
        back.Visibility().expect("Visibility"),
        Visibility::Collapsed
    );
    assert_ne!(
        list_control.FocusState().expect("FocusState"),
        FocusState::Unfocused,
        "戻ると、隠れた戻るボタンからフォーカスが一覧へ移ること"
    );

    // 戻るボタンにフォーカスがあるまま広げると、ボタンだけが隠れる。フォーカスは
    // 詳細が受けないので一覧へ。
    panes.show(Pane::Detail);
    assert_ne!(
        back.FocusState().expect("FocusState"),
        FocusState::Unfocused
    );
    outer.set_sizing(Sizing::fixed(800.0, 300.0));
    pump_until(SCROLL_LAYOUT_TIMEOUT, || !panes.is_collapsed());
    assert!(!panes.is_collapsed(), "広げれば 2 区画に戻ること");
    assert_eq!(
        back.Visibility().expect("Visibility"),
        Visibility::Collapsed
    );
    assert_ne!(
        list_control.FocusState().expect("FocusState"),
        FocusState::Unfocused,
        "隠れた戻るボタンからフォーカスが一覧へ移ること"
    );
    window.close();
    Ok(())
}

/// A Stack with a breakpoint lays its children out side by side once it is at least
/// that wide (its padding included) and one above the other below it, switching on the
/// dispatcher after the layout pass that changed its width. Going back takes a few
/// pixels less. The width comes from a fixed-width parent, so the window's frame does
/// not matter. Where the window is never laid out, it passes without checking.
fn stack_breakpoint_follows_its_width(ui: &Ui) -> Result<()> {
    let window = ui.window("しきい値", 900.0, 400.0)?;
    let outer = ui.stack(Orientation::Vertical)?;
    outer.set_sizing(Sizing::fixed(700.0, 300.0));
    let form = ui.stack(Orientation::Vertical)?;
    form.set_sizing(Sizing::fill_width());
    form.set_padding(naui_core::Padding::all(10.0));
    let first = ui.button("前")?;
    let last = ui.button("後")?;
    form.append(&first);
    form.append(&last);
    outer.append(&form);
    window.set_child(&outer);
    form.set_breakpoint(Some(naui_core::Breakpoint::new(
        500.0,
        Orientation::Horizontal,
    )));
    assert_eq!(
        form.current_orientation(),
        Orientation::Vertical,
        "並ぶ前は自分の向きのままであること"
    );
    window.show();
    let grid = native::<Grid>(&form);
    pump_until(SCROLL_LAYOUT_TIMEOUT, || {
        form.current_orientation() == Orientation::Horizontal
    });
    if grid.ActualWidth().unwrap_or(0.0) <= 0.0 {
        println!("     (並べられない環境なので、確かめずに通す)");
        window.close();
        return Ok(());
    }
    assert_eq!(
        form.current_orientation(),
        Orientation::Horizontal,
        "しきい値より広ければ横に並ぶこと: 幅 {:?}",
        grid.ActualWidth()
    );
    assert_eq!(form.orientation(), Orientation::Vertical);
    assert_eq!(
        Grid::GetColumn(&native::<FrameworkElement>(&last)).expect("列"),
        1,
        "子が横の列に並ぶこと"
    );

    for (width, expected, message) in [
        (480.0, Orientation::Vertical, "しきい値より狭ければ縦に積む"),
        (496.0, Orientation::Vertical, "しきい値に届くまでは縦のまま"),
        (500.0, Orientation::Horizontal, "ちょうどしきい値で横に並ぶ"),
        (
            494.0,
            Orientation::Horizontal,
            "しきい値のすぐ下では戻らない",
        ),
        (490.0, Orientation::Vertical, "遊びを越えて狭まれば縦に戻る"),
    ] {
        outer.set_sizing(Sizing::fixed(width, 300.0));
        pump_until(SCROLL_LAYOUT_TIMEOUT, || {
            grid.ActualWidth()
                .is_ok_and(|actual| (actual - width).abs() < 0.5)
                && form.current_orientation() == expected
        });
        // 変わらないはずの所では、予約が走るまで待ってから見る。
        pump_until(Duration::from_millis(200), || false);
        assert_eq!(
            form.current_orientation(),
            expected,
            "{message} (幅 {width}、実際 {:?})",
            grid.ActualWidth()
        );
    }

    form.set_breakpoint(None);
    assert_eq!(
        form.current_orientation(),
        Orientation::Vertical,
        "しきい値を外すと自分の向きへすぐ戻ること"
    );
    window.close();
    Ok(())
}

/// `NavigationView` forgets that the pane was closed when `PaneDisplayMode` changes,
/// and reopens it if it is wide enough. This only happens once its template is
/// applied, so messages are pumped until it is loaded; in environments where it never
/// is (CI), the case passes without checking.
fn sidebar_auto_collapse_toggle_keeps_collapsed(ui: &Ui) -> Result<()> {
    let window = ui.window("自動で畳む", 900.0, 400.0)?;
    window.set_child(&ui.label("中身")?);
    let sidebar = ui.sidebar()?;
    sidebar.set_items(&SidebarItem::list(["一般", "詳細"]));
    window.set_sidebar(&sidebar);
    window.show();
    let native = sidebar.native_navigation_view();
    pump_until(SCROLL_LAYOUT_TIMEOUT, || native.IsLoaded().unwrap_or(false));
    if !native.IsLoaded().unwrap_or(false) {
        println!("     (読み込まれない環境なので、確かめずに通す)");
        window.close();
        return Ok(());
    }

    let seen = Rc::new(RefCell::new(Vec::new()));
    sidebar.on_collapse({
        let seen = Rc::clone(&seen);
        move |collapsed| seen.borrow_mut().push(collapsed)
    });
    // 開閉の通知はアニメーションの後に届くので、切り替えるたびに少し回す。
    let toggle = |auto: bool| {
        sidebar.set_auto_collapse(auto);
        pump_until(Duration::from_millis(300), || false);
        sidebar.is_collapsed()
    };
    sidebar.set_collapsed(true);
    let collapsed_off = toggle(false);
    let collapsed_on = toggle(true);
    sidebar.set_collapsed(false);
    let open_off = !toggle(false);
    let open_on = !toggle(true);
    window.close();

    assert!(collapsed_off, "切っても畳んだまま");
    assert!(collapsed_on, "入れ直しても畳んだまま");
    assert!(open_off, "開いていれば開いたまま");
    assert!(open_on);
    assert!(seen.borrow().is_empty(), "切り替えでは通知しない");
    Ok(())
}

/// A sidebar collapsed automatically expands again when the window widens, unless it
/// was collapsed with `set_collapsed` in between. Closing the already-closed pane does
/// not change `IsPaneOpen`, so `NavigationView` would not learn that it was closed on
/// purpose. As above, it passes without checking where it is never loaded.
fn sidebar_set_collapsed_overrides_auto_expand(ui: &Ui) -> Result<()> {
    let window = ui.window("自動で開き直す", 900.0, 400.0)?;
    window.set_child(&ui.label("中身")?);
    let sidebar = ui.sidebar()?;
    sidebar.set_items(&SidebarItem::list(["一般", "詳細"]));
    window.set_sidebar(&sidebar);
    window.show();
    let native = sidebar.native_navigation_view();
    pump_until(SCROLL_LAYOUT_TIMEOUT, || native.IsLoaded().unwrap_or(false));
    if !native.IsLoaded().unwrap_or(false) {
        println!("     (読み込まれない環境なので、確かめずに通す)");
        window.close();
        return Ok(());
    }

    let seen = Rc::new(RefCell::new(Vec::new()));
    sidebar.on_collapse({
        let seen = Rc::clone(&seen);
        move |collapsed| seen.borrow_mut().push(collapsed)
    });
    let resize = |width: f64| {
        window.set_size(width, 400.0);
        pump_until(SCROLL_LAYOUT_TIMEOUT, || {
            native
                .ActualWidth()
                .is_ok_and(|actual| (actual - width).abs() < 40.0)
        });
        // 開閉の通知はアニメーションの後に届く。
        pump_until(Duration::from_millis(300), || false);
        sidebar.is_collapsed()
    };
    let narrowed = resize(400.0);
    let widened = !resize(900.0);
    let notified = seen.borrow().clone();
    let narrowed_again = resize(400.0);
    seen.borrow_mut().clear();
    sidebar.set_collapsed(true);
    pump_until(Duration::from_millis(300), || false);
    let silent = seen.borrow().is_empty();
    let stays = resize(900.0);
    window.close();

    assert!(narrowed, "狭めると畳む");
    assert!(widened, "広げると開き直す");
    assert_eq!(notified, [true, false], "自動の開閉は通知する");
    assert!(narrowed_again);
    assert!(silent, "set_collapsed では通知しない");
    assert!(stays, "set_collapsed で畳み直したものは、広げても開かない");
    Ok(())
}

// ------------------------------------------------------------------ 画像

/// The maximum time to wait for loading. It is a 1-pixel BMP, so it normally
/// finishes instantly.
const IMAGE_LOAD_TIMEOUT: Duration = Duration::from_secs(10);

/// A real file at a path containing Japanese can be loaded both by path and by
/// `file:` URL.
///
/// `BitmapImage` loading is asynchronous, and `SetSource` succeeds even if the
/// location cannot be read. It waits until `ImageOpened` arrives and the pixel size
/// can be obtained.
fn image_loads_from_japanese_path(ui: &Ui) -> Result<()> {
    let dir = std::env::temp_dir().join(format!("naui 画像テスト {}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("一時フォルダを作れない");
    let path = dir.join("写真 #1.bmp");
    std::fs::write(&path, bmp_2x1()).expect("画像を書き出せない");
    let path = path.to_str().expect("一時パスが UTF-8 でない").to_string();

    // 画面のツリーに入っていない `BitmapImage` は読み込みを始めないので、
    // ウィンドウに載せて出す。
    let window = ui.window("画像", 160.0, 120.0)?;
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        for source in [path.clone(), naui_core::media::file_url(&path)] {
            let image = ui.image(&source)?;
            window.set_child(&image);
            window.show();
            let bitmap = xaml_image(&image)
                .Source()
                .and_then(|bitmap| bitmap.cast::<BitmapImage>())
                .expect("Image の中身が BitmapImage でない");
            let opened = Arc::new(AtomicBool::new(false));
            let flag = opened.clone();
            bitmap
                .ImageOpened(&RoutedEventHandler::new(move |_, _| {
                    flag.store(true, Ordering::Relaxed);
                    Ok(())
                }))
                .expect("ImageOpened を購読できない");

            pump_until(IMAGE_LOAD_TIMEOUT, || opened.load(Ordering::Relaxed));
            assert!(
                opened.load(Ordering::Relaxed),
                "{} 秒待っても ImageOpened が届かない ({source})",
                IMAGE_LOAD_TIMEOUT.as_secs()
            );
            assert_eq!(
                (bitmap.PixelWidth().ok(), bitmap.PixelHeight().ok()),
                (Some(2), Some(1)),
                "読めた画素の大きさ ({source})"
            );
        }
        Ok(())
    }));
    window.close();
    let _ = std::fs::remove_dir_all(&dir);
    match outcome {
        Ok(result) => result,
        Err(panic) => std::panic::resume_unwind(panic),
    }
}

const RED_SVG: &str = r##"<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" width="24" height="16" viewBox="0 0 24 16">
  <rect width="24" height="16" fill="#ff0000"/>
</svg>"##;

/// The empty `Border` in an image's host whose size is the natural size asked for.
fn natural_size(image: &naui_windows::Image) -> (f64, f64) {
    let children = native::<Grid>(image).Children().expect("ホストの子");
    let natural = (0..children.Size().expect("ホストの子の数"))
        .filter_map(|index| children.GetAt(index).ok())
        .find_map(|element| {
            element
                .cast::<naui_winui3::Microsoft::UI::Xaml::Controls::Border>()
                .ok()
        })
        .expect("大きさを求める Border");
    (
        natural.Width().unwrap_or(f64::NAN),
        natural.Height().unwrap_or(f64::NAN),
    )
}

/// An SVG on the network (served over HTTP on the loopback here) is fetched and shown
/// like bytes: it gets its natural size from its document, and an SVGZ served
/// without `Content-Encoding` is unpacked. A document arriving after the image has
/// been replaced is not shown.
fn network_svg_takes_its_size(ui: &Ui) -> Result<()> {
    use std::io::{Read, Write};

    const BODY: &str =
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="20"><rect/></svg>"#;
    let svgz = {
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(BODY.as_bytes()).expect("gzip に書けない");
        encoder.finish().expect("gzip を閉じられない")
    };
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("待ち受けられない");
    let base = format!("http://{}", listener.local_addr().expect("待ち受けの番地"));
    // .svgz には gzip のまま (Content-Encoding を付けずに) 返す。
    // プロセスが終わるまで応え続ける。
    std::thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let mut request = Vec::new();
            let mut buffer = [0; 1024];
            while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                match stream.read(&mut buffer) {
                    Ok(0) | Err(_) => break,
                    Ok(read) => request.extend_from_slice(&buffer[..read]),
                }
            }
            let line = request.split(|&byte| byte == b'\r').next().unwrap_or(&[]);
            let body: &[u8] = if line.windows(5).any(|window| window == b".svgz") {
                &svgz
            } else {
                BODY.as_bytes()
            };
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: image/svg+xml\r\nContent-Length: {}\r\n\
                 Connection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(body);
        }
    });

    for path in ["icon.svg", "icon.svgz"] {
        let url = format!("{base}/{path}");
        let image = ui.image(&url)?;
        pump_until(IMAGE_LOAD_TIMEOUT, || natural_size(&image) == (40.0, 20.0));
        assert_eq!(
            natural_size(&image),
            (40.0, 20.0),
            "文書から大きさを求める ({path})"
        );
        assert!(
            xaml_image(&image)
                .Source()
                .and_then(|source| source.cast::<SvgImageSource>())
                .is_ok(),
            "取ってきた SVG を SvgImageSource で読む ({path})"
        );
        assert_eq!(image.source(), url, "取ってきても場所はそのまま");
    }

    // 取りに行っているあいだに別の画像へ替えたら、届いた文書は出さない。
    let image = ui.image(&format!("{base}/icon.svg"))?;
    image.set_bytes(RED_SVG.as_bytes());
    pump_until(Duration::from_secs(2), || false);
    assert_eq!(
        natural_size(&image),
        (24.0, 16.0),
        "前の SVG で上書きしない"
    );
    Ok(())
}

/// An SVG written in UTF-16 (either byte order, with its mark) is recognized as SVG
/// and actually opened by `SvgImageSource`, as UTF-8 is.
///
/// The load status `SetSourceAsync` ends with is read directly: a broken document
/// ends in `InvalidFormat`, which tells that the check can fail.
fn svg_opens_in_utf16(ui: &Ui) -> Result<()> {
    use naui_winui3::Microsoft::UI::Xaml::Media::Imaging::SvgImageSourceLoadStatus;
    use windows::Storage::Streams::{DataWriter, InMemoryRandomAccessStream};

    let utf16 = |little: bool| -> Vec<u8> {
        let doc = RED_SVG.replace("UTF-8", "UTF-16");
        let bom: &[u8] = if little { b"\xFF\xFE" } else { b"\xFE\xFF" };
        bom.iter()
            .copied()
            .chain(doc.encode_utf16().flat_map(move |unit| {
                if little {
                    unit.to_le_bytes()
                } else {
                    unit.to_be_bytes()
                }
            }))
            .collect()
    };
    let cases: [(&str, Vec<u8>, bool); 4] = [
        ("UTF-8", RED_SVG.as_bytes().to_vec(), true),
        ("UTF-16LE", utf16(true), true),
        ("UTF-16BE", utf16(false), true),
        (
            "壊れた SVG",
            b"<svg width='24' height='16'><rect".to_vec(),
            false,
        ),
    ];

    // 画面のツリーに入っていない画像は読み込みを始めないので、ウィンドウに載せて出す。
    let window = ui.window("SVG", 160.0, 120.0)?;
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        for (name, bytes, expect_open) in &cases {
            let image = ui.image("")?;
            window.set_child(&image);
            window.show();
            image.set_bytes(bytes);
            let svg = xaml_image(&image)
                .Source()
                .and_then(|source| source.cast::<SvgImageSource>())
                .unwrap_or_else(|_| panic!("{name} を SvgImageSource で読まない"));
            assert_eq!(natural_size(&image), (24.0, 16.0), "{name} の大きさ");

            // naui が渡したのと同じバイト列を、結果を受け取れる形でもう一度読ませる。
            let stream = InMemoryRandomAccessStream::new().expect("ストリームを作れない");
            let writer = DataWriter::CreateDataWriter(&stream).expect("DataWriter を作れない");
            writer.WriteBytes(bytes).expect("書けない");
            writer
                .StoreAsync()
                .and_then(|store| store.join())
                .expect("書き込みが終わらない");
            writer.DetachStream().expect("ストリームを外せない");
            stream.Seek(0).expect("先頭へ戻れない");
            let load = svg
                .SetSourceAsync(&stream)
                .expect("SetSourceAsync を呼べない");
            // AsyncStatus (windows-future) の 0 が Started。それ以外になれば終わっている。
            pump_until(IMAGE_LOAD_TIMEOUT, || {
                load.Status().is_ok_and(|status| status.0 != 0)
            });
            let status = load.GetResults().expect("読み込みの結果が届かない");
            assert_eq!(
                status == SvgImageSourceLoadStatus::Success,
                *expect_open,
                "{name} の読み込みの結果: {status:?}"
            );
        }
        Ok(())
    }));
    window.close();
    match outcome {
        Ok(result) => result,
        Err(panic) => std::panic::resume_unwind(panic),
    }
}

/// Bytes become a `BitmapImage` read from a stream, SVG an `SvgImageSource` whose
/// natural size comes from the document, and pixels are checked against their size.
fn image_takes_bytes_pixels_and_svg(ui: &Ui) -> Result<()> {
    let image = ui.image("")?;
    image.set_bytes(&bmp_solid(4, 2));
    assert!(image.is_loaded(), "バイト列の画像が置かれる");
    assert_eq!(image.source(), "");
    assert!(
        xaml_image(&image)
            .Source()
            .and_then(|source| source.cast::<BitmapImage>())
            .is_ok(),
        "ラスター画像は BitmapImage で読む"
    );

    image.set_bytes(RED_SVG.as_bytes());
    assert!(
        xaml_image(&image)
            .Source()
            .and_then(|source| source.cast::<SvgImageSource>())
            .is_ok(),
        "SVG は SvgImageSource で読む"
    );
    assert_eq!(natural_size(&image), (24.0, 16.0), "SVG の大きさを求める");
    // 寸法も viewBox も持たない SVG は、ブラウザと同じ 300×150 で出す (Auto が 0 にならない)。
    image.set_bytes(
        br#"<svg xmlns="http://www.w3.org/2000/svg"><rect width="10" height="10"/></svg>"#,
    );
    assert_eq!(
        natural_size(&image),
        (300.0, 150.0),
        "寸法の無い SVG の大きさ"
    );

    image.set_pixels(3, 2, &[0, 0, 255, 255].repeat(6))?;
    assert!(
        xaml_image(&image)
            .Source()
            .and_then(|source| source.cast::<BitmapImage>())
            .is_ok(),
        "画素は PNG にして BitmapImage で読む"
    );
    assert!(image.set_pixels(3, 3, &[0; 4]).is_err());

    // 名前の # は URL のフラグメントではない。パスでも file: の URL でも SVG として読む。
    let path = std::env::temp_dir().join(format!("naui-svg#{}.svg", std::process::id()));
    std::fs::write(&path, RED_SVG).expect("SVG を書き出せない");
    image.set_source(path.to_str().expect("UTF-8 のパス"));
    assert!(
        xaml_image(&image)
            .Source()
            .and_then(|source| source.cast::<SvgImageSource>())
            .is_ok(),
        "SVG ファイルも SvgImageSource で読む"
    );
    assert_eq!(natural_size(&image), (24.0, 16.0));

    // file: の URL で渡しても、手元のファイルとして大きさを求める。
    image.set_source("");
    image.set_source(&naui_core::media::file_url(
        path.to_str().expect("UTF-8 のパス"),
    ));
    let _ = std::fs::remove_file(&path);
    assert_eq!(
        natural_size(&image),
        (24.0, 16.0),
        "file: の SVG の大きさを求める"
    );

    // gzip で包んだ .svgz は展開してから SvgImageSource に渡す。
    let svgz = {
        use std::io::Write;
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder
            .write_all(RED_SVG.as_bytes())
            .expect("gzip に書けない");
        encoder.finish().expect("gzip を閉じられない")
    };
    let path = std::env::temp_dir().join(format!("naui-svg-{}.svgz", std::process::id()));
    std::fs::write(&path, &svgz).expect("SVGZ を書き出せない");
    image.set_source(path.to_str().expect("UTF-8 のパス"));
    let _ = std::fs::remove_file(&path);
    assert!(
        xaml_image(&image)
            .Source()
            .and_then(|source| source.cast::<SvgImageSource>())
            .is_ok(),
        "SVGZ ファイルも SvgImageSource で読む"
    );
    assert_eq!(natural_size(&image), (24.0, 16.0), "SVGZ の大きさを求める");

    image.set_bytes(&svgz);
    assert_eq!(natural_size(&image), (24.0, 16.0), "SVGZ のバイト列も読む");
    Ok(())
}

/// The WinUI `Image` inside naui's `Image`. It sits on a `Canvas` in the host `Grid`.
fn xaml_image(image: &naui_windows::Image) -> XamlImage {
    let children = native::<Grid>(image).Children().expect("ホストの子");
    (0..children.Size().expect("ホストの子の数"))
        .filter_map(|index| children.GetAt(index).ok())
        .find_map(|element| element.cast::<XamlCanvas>().ok())
        .and_then(|stage| stage.Children().and_then(|c| c.GetAt(0)).ok())
        .and_then(|element| element.cast::<XamlImage>().ok())
        .expect("Image の中身が WinUI の Image でない")
}

/// An image's natural size is its pixel size, as on the other backends. In a
/// vertical `Scroll`, a `Fill`-width image keeps its pixel height instead of growing
/// to the width with the aspect ratio, and a `Fill` image takes the viewport's
/// leftover height.
///
/// Messages are pumped until it is laid out; in environments where it never is
/// (CI), the sizes pass without checking.
fn image_natural_size_is_its_pixels(ui: &Ui) -> Result<()> {
    let dir = std::env::temp_dir().join(format!("naui 画像の大きさ {}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("一時フォルダを作れない");
    let path = dir.join("40x20.bmp");
    std::fs::write(&path, bmp_solid(40, 20)).expect("画像を書き出せない");
    let path = path.to_str().expect("一時パスが UTF-8 でない").to_string();

    let window = ui.window("画像の大きさ", 300.0, 240.0)?;
    let pane = ui.stack(Orientation::Vertical)?;
    pane.set_sizing(Sizing::fill());
    let image = ui.image(&path)?;
    image.set_sizing(Sizing::fill_width());
    pane.append(&image);
    let scroll = ui.scroll()?;
    scroll.set_child(&pane);
    scroll.set_sizing(Sizing::fill());
    window.set_child(&scroll);

    let host = native::<FrameworkElement>(&image);
    let inner = xaml_image(&image);
    let size = |element: &FrameworkElement| {
        (
            element.ActualWidth().unwrap_or(f64::NAN),
            element.ActualHeight().unwrap_or(f64::NAN),
        )
    };
    let inner_element = inner.cast::<FrameworkElement>().expect("Image は要素");
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        if !show_and_measure(&window, &scroll) {
            return;
        }
        let opened = || size(&host).1 == 20.0;
        pump_until(IMAGE_LOAD_TIMEOUT, opened);
        let viewport = scroll.metrics();
        assert_eq!(
            size(&host),
            (viewport.viewport_width, 20.0),
            "横は Fill、縦は画素の高さ"
        );
        assert_eq!(
            size(&inner_element),
            size(&host),
            "WinUI の Image はホストと同じ大きさ"
        );

        image.set_sizing(Sizing::fill());
        let tall = || size(&host).1 == viewport.viewport_height;
        pump_until(SCROLL_LAYOUT_TIMEOUT, tall);
        assert_eq!(
            size(&host),
            (viewport.viewport_width, viewport.viewport_height),
            "縦も Fill なら表示領域の余りを受け取る"
        );
        pump_until(SCROLL_LAYOUT_TIMEOUT, || {
            size(&inner_element) == size(&host)
        });
        assert_eq!(
            size(&inner_element),
            size(&host),
            "WinUI の Image はホストの大きさに付いていく"
        );
    }));
    window.close();
    let _ = std::fs::remove_dir_all(&dir);
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
    Ok(())
}

/// A `Fill` video sits in the space a vertical `Scroll` leaves it, and follows when
/// the window gets shorter, instead of keeping the height its width allows.
///
/// The `MediaPlayerElement` sits on a `Canvas` in the host `Grid` and is given the
/// host's size. No video is loaded (CI has none to play), so the pixel size is not
/// involved; the Gallery's Media page is where a real video was checked. Sizes pass
/// without checking where nothing is laid out (CI).
fn video_fits_the_leftover_and_follows_it(ui: &Ui) -> Result<()> {
    use naui_winui3::Microsoft::UI::Xaml::Controls::MediaPlayerElement;

    let window = ui.window("動画の大きさ", 320.0, 300.0)?;
    let pane = ui.stack(Orientation::Vertical)?;
    pane.set_sizing(Sizing::fill());
    let video = ui.video("")?;
    video.set_sizing(Sizing::fill());
    pane.append(&video);
    pane.append(&ui.label("操作の行")?);
    let scroll = ui.scroll()?;
    scroll.set_child(&pane);
    scroll.set_sizing(Sizing::fill());
    window.set_child(&scroll);

    let host = native::<Grid>(&video);
    let children = host.Children().expect("ホストの子");
    let player = (0..children.Size().expect("ホストの子の数"))
        .filter_map(|index| children.GetAt(index).ok())
        .find_map(|element| element.cast::<XamlCanvas>().ok())
        .and_then(|stage| stage.Children().and_then(|c| c.GetAt(0)).ok())
        .and_then(|element| element.cast::<MediaPlayerElement>().ok())
        .expect("MediaPlayerElement が Canvas に載っていない");
    let host = host.cast::<FrameworkElement>().expect("ホストは要素");
    let player = player.cast::<FrameworkElement>().expect("動画は要素");
    let size = |element: &FrameworkElement| {
        (
            element.ActualWidth().unwrap_or(f64::NAN),
            element.ActualHeight().unwrap_or(f64::NAN),
        )
    };

    let outcome = catch_unwind(AssertUnwindSafe(|| {
        if !show_and_measure(&window, &scroll) {
            return;
        }
        let fits = || {
            let viewport = scroll.metrics();
            let content = native::<FrameworkElement>(&pane)
                .ActualHeight()
                .unwrap_or(f64::NAN);
            (content - viewport.viewport_height).abs() < 0.5
                && size(&player) == size(&host)
                && size(&host).1 > 0.0
        };
        pump_until(SCROLL_LAYOUT_TIMEOUT, fits);
        assert!(fits(), "動画は表示領域の余りに収まる: {:?}", size(&host));
        let tall = size(&host).1;

        window.set_size(320.0, 200.0);
        pump_until(SCROLL_LAYOUT_TIMEOUT, || fits() && size(&host).1 < tall);
        assert!(
            fits() && size(&host).1 < tall,
            "縮めると動画も低くなる: {tall} → {:?}",
            size(&host)
        );
    }));
    window.close();
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
    Ok(())
}

/// Puts a scroll view holding tall content into a small window (not shown yet).
fn tall_scroll(
    ui: &Ui,
    rows: usize,
) -> Result<(
    naui_windows::Window,
    naui_windows::Scroll,
    naui_windows::Stack,
)> {
    let window = ui.window("スクロール", 240.0, 160.0)?;
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

/// Shows the window and pumps messages until the scroll view has an actual size.
///
/// In WinUI 3 on CI, the ScrollViewer gets no template and its actual size stays 0.
/// In that case it returns `false`, and the case passes without checking (it is
/// checked on real machines).
fn show_and_measure(window: &naui_windows::Window, scroll: &naui_windows::Scroll) -> bool {
    window.show();
    pump_until(SCROLL_LAYOUT_TIMEOUT, || {
        scroll.metrics().viewport_height > 0.0
    });
    let measured = scroll.metrics().viewport_height > 0.0;
    if !measured {
        println!("     (ScrollViewer に実寸が付かない環境なので、確かめずに通す)");
    }
    measured
}

/// The maximum time to wait for the scroll view to get an actual size.
///
/// It is long because a window opened right after keys moved through the rows of a
/// `ListView` is laid out late, by seconds (see `docs/known-limitations.md`). A case that
/// gives up waiting passes without checking anything.
const SCROLL_LAYOUT_TIMEOUT: Duration = Duration::from_secs(20);

fn scroll_measures_and_moves(ui: &Ui) -> Result<()> {
    let (window, scroll, _pane) = tall_scroll(ui, 40)?;
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        if !show_and_measure(&window, &scroll) {
            return;
        }
        let m = scroll.metrics();
        assert_eq!((m.x, m.y), (0.0, 0.0), "最初は先頭: {m:?}");
        assert!(
            m.content_height > m.viewport_height,
            "はみ出している: {m:?}"
        );

        // `ChangeView` の直後でも、新しい位置を返す。
        scroll.scroll_to(0.0, 30.0);
        assert_eq!(scroll.metrics().y, 30.0);

        scroll.scroll_to_end();
        let end = scroll.metrics();
        assert_eq!(end.y, end.max_y(), "末尾へ: {end:?}");
        assert_eq!(end.distance_to_end(), 0.0);

        scroll.scroll_to(-50.0, 1e9);
        let clamped = scroll.metrics();
        assert_eq!((clamped.x, clamped.y), (0.0, clamped.max_y()));
    }));
    window.close();
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
    Ok(())
}

/// A `Scroll` moved to another `Stack` keeps its position, as on the other
/// platforms (where the tests check it). On CI the ScrollViewer gets no size, so this
/// is checked on real machines only.
fn moved_scroll_keeps_its_position(ui: &Ui) -> Result<()> {
    let window = ui.window("移動", 240.0, 320.0)?;
    let root = ui.stack(Orientation::Vertical)?;
    let first = ui.stack(Orientation::Vertical)?;
    let second = ui.stack(Orientation::Vertical)?;
    root.append(&first);
    root.append(&second);
    let pane = ui.stack(Orientation::Vertical)?;
    for i in 0..40 {
        pane.append(&ui.label(&format!("行 {i}"))?);
    }
    let scroll = ui.scroll()?;
    scroll.set_child(&pane);
    scroll.set_sizing(Sizing::fixed(200.0, 100.0));
    first.append(&scroll);
    window.set_child(&root);
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        if !show_and_measure(&window, &scroll) {
            return;
        }
        scroll.scroll_to(0.0, 120.0);
        assert_eq!(scroll.metrics().y, 120.0);
        first.remove(0);
        second.append(&scroll);
        pump_until(SCROLL_LAYOUT_TIMEOUT, || {
            scroll.metrics().viewport_height > 0.0
        });
        assert_eq!(
            scroll.metrics().y,
            120.0,
            "移してもスクロール位置が残ること"
        );
    }));
    window.close();
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
    Ok(())
}

/// Calls "append, then scroll to the end", as in a chat, back to back without
/// pumping messages.
fn scroll_to_end_after_appending(ui: &Ui) -> Result<()> {
    let (window, scroll, pane) = tall_scroll(ui, 20)?;
    let outcome = catch_unwind(AssertUnwindSafe(|| -> Result<()> {
        if !show_and_measure(&window, &scroll) {
            return Ok(());
        }
        scroll.scroll_to_end();
        let before = scroll.metrics();
        for round in 0..3 {
            // Gallery の「行を足す」と同じく、足す前の位置で追従を決める。
            assert!(
                scroll.metrics().distance_to_end() < 1.0,
                "{round} 回目の前に末尾から外れた"
            );
            for i in 0..10 {
                pane.append(&ui.label(&format!("追加 {round}-{i}"))?);
            }
            scroll.scroll_to_end();
        }
        let after = scroll.metrics();
        assert!(
            after.content_height > before.content_height,
            "中身が伸びている: {before:?} → {after:?}"
        );
        assert_eq!(after.distance_to_end(), 0.0, "{after:?}");
        Ok(())
    }));
    window.close();
    outcome.unwrap_or_else(|panic| std::panic::resume_unwind(panic))
}

fn scroll_to_end_before_shown(ui: &Ui) -> Result<()> {
    let (window, scroll, _pane) = tall_scroll(ui, 40)?;
    scroll.scroll_to_end();
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        if !show_and_measure(&window, &scroll) {
            return;
        }
        let m = scroll.metrics();
        assert!(m.max_y() > 0.0, "{m:?}");
        assert_eq!(m.distance_to_end(), 0.0, "表示後に末尾にいる: {m:?}");
    }));
    window.close();
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
    Ok(())
}

/// `ViewChanged` arrives later, so messages are pumped while waiting.
fn scroll_notifies_only_moves(ui: &Ui) -> Result<()> {
    let (window, scroll, _pane) = tall_scroll(ui, 40)?;
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        if !show_and_measure(&window, &scroll) {
            return;
        }
        let seen = Rc::new(RefCell::new(Vec::new()));
        scroll.on_scroll({
            let seen = seen.clone();
            move |m: naui_core::ScrollMetrics| seen.borrow_mut().push(m.y)
        });
        let settle = || pump_until(Duration::from_millis(300), || false);

        scroll.scroll_to(0.0, 25.0);
        scroll.scroll_to(0.0, 25.0); // 同じ位置へは動かない
        settle();
        assert_eq!(*seen.borrow(), vec![25.0]);

        // 幅だけ変えても、位置が変わらなければ届かない。
        window.set_size(300.0, 160.0);
        settle();
        assert_eq!(*seen.borrow(), vec![25.0]);

        // 通知の中から送った先も、最後の位置として届く。
        seen.borrow_mut().clear();
        scroll.on_scroll({
            let seen = seen.clone();
            let scroll = scroll.clone();
            move |m: naui_core::ScrollMetrics| {
                seen.borrow_mut().push(m.y);
                if m.y == 55.0 {
                    scroll.scroll_to(0.0, 50.0);
                }
            }
        });
        scroll.scroll_to(0.0, 55.0);
        settle();
        assert_eq!(*seen.borrow(), vec![55.0, 50.0]);
        assert_eq!(scroll.metrics().y, 50.0);
    }));
    window.close();
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
    Ok(())
}

/// The `ListView` has only the rows around the screen, so assistive technology is told
/// each row's place among all the rows and how many there are.
fn table_rows_report_their_place_among_all_rows(ui: &Ui) -> Result<()> {
    use naui_core::TableColumn;
    use naui_windows::TableCells;
    use naui_winui3::Microsoft::UI::Xaml::Automation::AutomationProperties;

    let window = ui.window("表の行の位置", 240.0, 160.0)?;
    let table = ui.table()?;
    table.set_columns(&[TableColumn::new("名前")]);
    table.set_sizing(Sizing::fill());
    // 組み立てた順の番号。末尾の行数分が、いま `ListView` にある行。
    let fill = |count: usize| {
        table.set_row_builder(count, |_| Ok(TableCells::default()));
    };
    // どの行も位置と全行数を伝えていることを確かめ、行の数と組み立てた先頭の番号を返す。
    let check = |count: usize| {
        let items = table.native_list_view().Items().expect("行");
        let size = items.Size().expect("行の数") as usize;
        let mut first = 0;
        for slot in 0..size {
            let row = items
                .GetAt(slot as u32)
                .and_then(|item| item.cast::<ListViewItem>())
                .expect("行");
            let position = AutomationProperties::GetPositionInSet(&row).expect("位置") as usize;
            if slot == 0 {
                first = position - 1;
            }
            assert_eq!(position, first + slot + 1, "全行の中の位置であること");
            assert_eq!(
                AutomationProperties::GetSizeOfSet(&row).expect("全行数"),
                count as i32,
                "組み立てた行の数ではなく全行数であること"
            );
        }
        (size, first)
    };
    fill(1000);
    window.set_child(&table);
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        window.show();
        pump_until(SCROLL_LAYOUT_TIMEOUT, || check(1000).0 < 200);
        let (size, first) = check(1000);
        if size >= 200 {
            println!("     (表に実寸が付かない環境なので、確かめずに通す)");
            return;
        }
        assert_eq!(first, 0, "最初は先頭から組み立てている");

        // 送っても、窓の中の位置にならない。
        table.scroll_to_row(800);
        pump_until(Duration::from_secs(2), || check(1000).1 > 0);
        let (size, first) = check(1000);
        assert!(first > 0, "組み立てる範囲が付いてくること");
        assert!(size < 200, "絞られていること: {size} 行");

        // 行数が変わったら、新しい全行数を伝える。
        fill(500);
        check(500);
    }));
    window.close();
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
    Ok(())
}

/// A `Table` with many rows builds only the rows around the screen, and the built
/// range follows scrolling.
///
/// The table's `ScrollViewer` is moved directly (not with `scroll_to_row`, which
/// moves the range itself), so this checks that the range follows `ViewChanged`.
fn table_rows_follow_scrolling(ui: &Ui) -> Result<()> {
    use naui_core::{TableColumn, TableRow};

    let window = ui.window("表のスクロール", 240.0, 160.0)?;
    let table = ui.table()?;
    table.set_columns(&[TableColumn::new("名前")]);
    let labels: Vec<String> = (0..1000).map(|i| format!("行 {i}")).collect();
    table.set_rows(&TableRow::list(labels.iter().map(|l| [l.as_str()])));
    table.set_sizing(Sizing::fill());
    window.set_child(&table);
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        window.show();
        let scroll = table_scroller(&table);
        pump_until(SCROLL_LAYOUT_TIMEOUT, || {
            scroll.ViewportHeight().unwrap_or(0.0) > 0.0
        });
        if scroll.ViewportHeight().unwrap_or(0.0) <= 0.0 {
            println!("     (ScrollViewer に実寸が付かない環境なので、確かめずに通す)");
            return;
        }
        let body = scroll
            .Content()
            .and_then(|content| content.cast::<StackPanel>())
            .expect("表の本体");
        let top_spacer = body
            .Children()
            .and_then(|children| children.GetAt(0))
            .and_then(|spacer| spacer.cast::<FrameworkElement>())
            .expect("先頭の詰め物");
        let leading = || top_spacer.Height().unwrap_or(0.0);
        assert_eq!(leading(), 0.0, "最初は先頭から組み立てている");

        let target = scroll.ExtentHeight().expect("表の全体の高さ") / 2.0;
        let offset = PropertyValue::CreateDouble(target)
            .and_then(|value| value.cast::<windows::Foundation::IReference<f64>>())
            .expect("送り先");
        scroll
            .ChangeViewWithOptionalAnimation(None, &offset, None, true)
            .expect("表のスクロール");
        pump_until(Duration::from_secs(2), || leading() > 0.0);
        let offset = scroll.VerticalOffset().expect("表の位置");
        assert!(offset > 0.0, "表が送られている");
        assert!(
            leading() > 0.0 && leading() <= offset,
            "組み立てる範囲が表示位置へ付いてくる (詰め物 {}, 位置 {offset})",
            leading()
        );
    }));
    window.close();
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
    Ok(())
}

/// The row with the focus stays focused when the window moves and rows are rebuilt,
/// as long as it is still on screen.
fn table_keeps_the_focused_row_across_rebuilds(ui: &Ui) -> Result<()> {
    use naui_core::{TableColumn, TableRow};
    use naui_winui3::Microsoft::UI::Xaml::Controls::Control;
    use naui_winui3::Microsoft::UI::Xaml::FocusState;

    // 窓が動く分 (`ROW_WINDOW_OVERSCAN` の半分) より多くの行が見える高さにする。
    let window = ui.window("表のフォーカス", 240.0, 640.0)?;
    let table = ui.table()?;
    table.set_columns(&[TableColumn::new("名前")]);
    let labels: Vec<String> = (0..5_000).map(|i| format!("行 {i}")).collect();
    table.set_rows(&TableRow::list(labels.iter().map(|l| [l.as_str()])));
    table.set_sizing(Sizing::fill());
    window.set_child(&table);
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        window.show();
        let children = native::<Grid>(&table).Children().expect("表の子");
        let scroll = (0..children.Size().expect("表の子の数"))
            .find_map(|i| children.GetAt(i).ok()?.cast::<ScrollViewer>().ok())
            .expect("表の ScrollViewer");
        pump_until(SCROLL_LAYOUT_TIMEOUT, || {
            scroll.ViewportHeight().unwrap_or(0.0) > 0.0 && table.row_height() > 0.0
        });
        if scroll.ViewportHeight().unwrap_or(0.0) <= 0.0 || table.row_height() <= 0.0 {
            println!("     (ScrollViewer に実寸が付かない環境なので、確かめずに通す)");
            return;
        }
        let body = scroll
            .Content()
            .and_then(|content| content.cast::<StackPanel>())
            .expect("表の本体");
        let top_spacer = body
            .Children()
            .and_then(|children| children.GetAt(0))
            .and_then(|spacer| spacer.cast::<FrameworkElement>())
            .expect("先頭の詰め物");
        let items = table.native_list_view().Items().expect("Items");
        let height = table.row_height();
        let first_built = || (top_spacer.Height().unwrap_or(0.0) / height).round() as usize;
        let scroll_to = |row: usize| {
            let offset = PropertyValue::CreateDouble(row as f64 * height)
                .and_then(|value| value.cast::<windows::Foundation::IReference<f64>>())
                .expect("送り先");
            scroll
                .ChangeViewWithOptionalAnimation(None, &offset, None, true)
                .expect("表のスクロール");
        };
        let item = |offset: usize| {
            items
                .GetAt(offset as u32)
                .and_then(|item| item.cast::<ListViewItem>())
                .expect("行")
        };
        // 窓の中でフォーカスのある行の、全体での番号。
        let focused = || {
            (0..items.Size().expect("行数") as usize).find_map(|offset| {
                let state = item(offset).cast::<Control>().ok()?.FocusState().ok()?;
                (state != FocusState::Unfocused).then_some(first_built() + offset)
            })
        };

        scroll_to(100);
        pump_until(Duration::from_secs(2), || first_built() > 0);
        let visible = (scroll.ViewportHeight().expect("表の高さ") / height) as usize;
        assert!(
            visible > 8,
            "窓を動かしても見えたままにできる高さ: {visible} 行"
        );
        // 見えている範囲の下のほうの行。下へ送っても画面に残る。
        let target = 100 + visible - 1;
        let target_item = item(target - first_built());
        let control = target_item.cast::<Control>().expect("Control");
        pump_until(FOCUS_TIMEOUT, || {
            control.Focus(FocusState::Programmatic).unwrap_or(false)
        });
        assert_eq!(focused(), Some(target), "行にフォーカスを置けること");

        let moved_from = first_built();
        scroll_to(100 + 7);
        pump_until(Duration::from_secs(2), || first_built() != moved_from);
        assert_ne!(first_built(), moved_from, "窓が動いていること");
        assert_eq!(focused(), Some(target), "組み立て直しても同じ行にあること");
        let offset = scroll.VerticalOffset().expect("表の位置");
        assert!(
            (offset - 107.0 * height).abs() <= height,
            "フォーカスを戻しても位置が飛ばないこと: {offset}"
        );
    }));
    window.close();
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
    Ok(())
}

/// With single selection, a row off screen must not stay selected next to the row
/// just picked, even with Ctrl held. Multiple selection keeps it.
fn table_single_selection_drops_rows_outside_the_window(ui: &Ui) -> Result<()> {
    use naui_core::{SelectionMode, TableColumn, TableRow};
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetKeyboardState, SetKeyboardState, VK_CONTROL,
    };

    let window = ui.window("表の単一選択", 240.0, 160.0)?;
    let table = ui.table()?;
    table.set_columns(&[TableColumn::new("名前")]);
    let labels: Vec<String> = (0..5_000).map(|i| format!("行 {i}")).collect();
    table.set_rows(&TableRow::list(labels.iter().map(|l| [l.as_str()])));
    table.set_sizing(Sizing::fill());
    window.set_child(&table);

    // `GetKeyState` はこのスレッドの表を読むので、Ctrl を押した状態を表へ書く。
    let mut keys = [0u8; 256];
    // SAFETY: 配列はこの関数の中で生きており、大きさは 256 バイトで決まっている。
    unsafe { GetKeyboardState(&mut keys) }.expect("キーの状態");
    let released = keys;
    keys[usize::from(VK_CONTROL.0)] |= 0x80;

    let outcome = catch_unwind(AssertUnwindSafe(|| {
        window.show();
        pump_until(SCROLL_LAYOUT_TIMEOUT, || table.row_height() > 0.0);
        let items = table.native_list_view().Items().expect("Items");
        let pick_row_one = || {
            items
                .GetAt(1)
                .and_then(|item| item.cast::<ListViewItem>())
                .and_then(|item| item.SetIsSelected(true))
                .expect("行を選ぶ");
            // 窓に載っていても、通知が同じ呼び出しの中で届くとは限らない。
            pump_until(Duration::from_secs(2), || table.selection().contains(&1));
        };
        // SAFETY: 同上。
        unsafe { SetKeyboardState(&keys) }.expect("Ctrl を押す");

        table.set_selected(4_000);
        pick_row_one();
        assert_eq!(table.selection(), vec![1], "1 行だけになること");

        table.set_selection_mode(SelectionMode::Multiple);
        table.set_selection(&[4_000]);
        pick_row_one();
        assert_eq!(table.selection(), vec![1, 4_000], "足すだけなら残ること");
    }));
    // SAFETY: 同上。
    unsafe { SetKeyboardState(&released) }.expect("Ctrl を離す");
    window.close();
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
    Ok(())
}

/// The preferred languages are `GlobalizationPreferences.Languages`, and a window
/// that receives `WM_SETTINGCHANGE` with `"intl"` has them read again. Other areas
/// and a list that did not change do not reach the closure, and naui's own strings
/// stay as chosen at startup.
fn preferred_languages_follow_settingchange(ui: &Ui) -> Result<()> {
    use windows::Win32::Foundation::{LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{SendMessageW, WM_SETTINGCHANGE};

    let os = ui.preferred_languages();
    assert!(
        !os.is_empty(),
        "GlobalizationPreferences.Languages が何か返すこと"
    );
    let strings = naui_core::ui_strings();

    let window = ui.window("言語", 320.0, 240.0)?;
    let native = window
        .native_window()
        .cast::<naui_winui3::IWindowNative>()
        .expect("IWindowNative");
    // SAFETY: このウィンドウが生きている間の HWND を UI スレッドで使う。
    let hwnd = unsafe { native.WindowHandle() }.expect("HWND");
    let send = |area: &str| {
        let area: Vec<u16> = area.encode_utf16().chain([0]).collect();
        // SAFETY: area は SendMessageW が戻るまで生きている。
        unsafe {
            SendMessageW(
                hwnd,
                WM_SETTINGCHANGE,
                Some(WPARAM(0)),
                Some(LPARAM(area.as_ptr() as isize)),
            );
        }
    };

    let seen = Rc::new(RefCell::new(Vec::<Vec<String>>::new()));
    let sink = seen.clone();
    ui.on_preferred_languages_change(move |tags| sink.borrow_mut().push(tags.to_vec()));
    let changed = vec!["fr-FR".to_string(), "ja-JP".to_string()];
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        naui_windows::override_preferred_languages_for_test(Some(changed.clone()));
        send("Environment");
        pump_until(Duration::from_millis(300), || !seen.borrow().is_empty());
        assert!(seen.borrow().is_empty(), "intl 以外では読み直さない");

        send("intl");
        pump_until(Duration::from_secs(2), || !seen.borrow().is_empty());
        assert_eq!(seen.borrow().as_slice(), std::slice::from_ref(&changed));
        assert_eq!(ui.preferred_languages(), changed);
        assert!(
            std::ptr::eq(naui_core::ui_strings(), strings),
            "naui の文字列は起動時の言語のまま"
        );

        send("intl");
        pump_until(Duration::from_millis(300), || seen.borrow().len() > 1);
        assert_eq!(seen.borrow().len(), 1, "変わっていなければ呼ばれない");

        naui_windows::override_preferred_languages_for_test(None);
        send("intl");
        pump_until(Duration::from_secs(2), || seen.borrow().len() == 2);
        assert_eq!(seen.borrow().last(), Some(&os));
    }));
    naui_windows::override_preferred_languages_for_test(None);
    window.close();
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
    Ok(())
}

/// Pumps the UI thread's messages until the condition holds or time runs out.
///
/// Image load completion arrives by being queued onto the UI thread's queue from
/// another thread. Cases run inside `OnLaunched`, so without pumping here it would
/// never arrive.
fn pump_until(timeout: Duration, mut done: impl FnMut() -> bool) {
    use windows::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, PeekMessageW, TranslateMessage, MSG, PM_REMOVE,
    };

    let deadline = std::time::Instant::now() + timeout;
    while !done() && std::time::Instant::now() < deadline {
        let mut message = MSG::default();
        // SAFETY: MSG はこの関数の中で生きており、UI スレッドから呼んでいる。
        unsafe {
            if PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            } else {
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    }
}

/// A 2×1 24-bit BMP, red on the left and blue on the right.
///
/// Unlike PNG it has no checksum, so building it by hand does not break it.
fn bmp_2x1() -> Vec<u8> {
    const HEADER: u32 = 14 + 40;
    // 1 行 6 バイトを 4 バイト境界にそろえて 8 バイト。
    const PIXELS: u32 = 8;
    let mut bmp = Vec::new();
    bmp.extend_from_slice(b"BM");
    bmp.extend_from_slice(&(HEADER + PIXELS).to_le_bytes());
    bmp.extend_from_slice(&0u32.to_le_bytes());
    bmp.extend_from_slice(&HEADER.to_le_bytes());
    bmp.extend_from_slice(&40u32.to_le_bytes()); // BITMAPINFOHEADER の大きさ
    bmp.extend_from_slice(&2i32.to_le_bytes()); // 幅
    bmp.extend_from_slice(&1i32.to_le_bytes()); // 高さ (正なので下から上)
    bmp.extend_from_slice(&1u16.to_le_bytes()); // プレーン数
    bmp.extend_from_slice(&24u16.to_le_bytes()); // 1 画素のビット数
    bmp.extend_from_slice(&0u32.to_le_bytes()); // 無圧縮
    bmp.extend_from_slice(&PIXELS.to_le_bytes());
    bmp.extend_from_slice(&[0; 16]); // 解像度と色数は使わない
    bmp.extend_from_slice(&[0, 0, 255, 255, 0, 0, 0, 0]); // BGR, BGR, 詰め物
    bmp
}

/// A `Fill` image asks for no size and fits into the viewport's leftover height, even
/// when its pixels are taller than the viewport. Asking for its pixel size, it pushed
/// the content of a vertical `Scroll` out to its own height.
///
/// Messages are pumped until it is laid out; in environments where it never is
/// (CI), the sizes pass without checking.
fn fill_image_does_not_push_a_scroll(ui: &Ui) -> Result<()> {
    let dir = std::env::temp_dir().join(format!("naui 大きな画像 {}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("一時フォルダを作れない");
    let path = dir.join("300x1000.bmp");
    std::fs::write(&path, bmp_solid(300, 1000)).expect("画像を書き出せない");
    let path = path.to_str().expect("一時パスが UTF-8 でない").to_string();

    let window = ui.window("大きな画像", 300.0, 240.0)?;
    let pane = ui.stack(Orientation::Vertical)?;
    pane.set_sizing(Sizing::fill());
    let image = ui.image(&path)?;
    image.set_sizing(Sizing::fill());
    pane.append(&image);
    let scroll = ui.scroll()?;
    scroll.set_child(&pane);
    scroll.set_sizing(Sizing::fill());
    window.set_child(&scroll);

    let host = native::<FrameworkElement>(&image);
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        if !show_and_measure(&window, &scroll) {
            return;
        }
        let opened = || {
            xaml_image(&image)
                .Source()
                .ok()
                .and_then(|source| source.cast::<BitmapImage>().ok())
                .and_then(|bitmap| bitmap.PixelHeight().ok())
                .is_some_and(|height| height == 1000)
        };
        pump_until(IMAGE_LOAD_TIMEOUT, opened);
        assert!(opened(), "画像が開くこと");
        let viewport = scroll.metrics().viewport_height;
        let fits = || host.ActualHeight().unwrap_or(f64::NAN) == viewport;
        pump_until(SCROLL_LAYOUT_TIMEOUT, fits);
        let metrics = scroll.metrics();
        assert_eq!(
            metrics.content_height, metrics.viewport_height,
            "中身は表示領域に収まる: {metrics:?}"
        );
        assert_eq!(
            host.ActualHeight().unwrap_or(f64::NAN),
            viewport,
            "画像は表示領域の余りを受け取る"
        );
    }));
    window.close();
    let _ = std::fs::remove_dir_all(&dir);
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
    Ok(())
}

/// Changing an opened image's height from `Auto` to `Fill` stops asking for its
/// pixel height at once. The host keeps its size until it is measured again, so
/// waiting for `SizeChanged` to drop the pixel height would wait forever, and the
/// image would keep pushing the content of a vertical `Scroll` out.
///
/// Messages are pumped until it is laid out; in environments where it never is
/// (CI), the sizes pass without checking.
fn image_from_auto_to_fill_drops_the_pixel_height(ui: &Ui) -> Result<()> {
    let dir = std::env::temp_dir().join(format!("naui 高さの切り替え {}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("一時フォルダを作れない");
    let path = dir.join("300x1000.bmp");
    std::fs::write(&path, bmp_solid(300, 1000)).expect("画像を書き出せない");
    let path = path.to_str().expect("一時パスが UTF-8 でない").to_string();

    let window = ui.window("高さの切り替え", 300.0, 240.0)?;
    let pane = ui.stack(Orientation::Vertical)?;
    pane.set_sizing(Sizing::fill());
    let image = ui.image(&path)?;
    image.set_sizing(Sizing::fill_width());
    pane.append(&image);
    let scroll = ui.scroll()?;
    scroll.set_child(&pane);
    scroll.set_sizing(Sizing::fill());
    window.set_child(&scroll);

    let host = native::<FrameworkElement>(&image);
    let height = || host.ActualHeight().unwrap_or(f64::NAN);
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        if !show_and_measure(&window, &scroll) {
            return;
        }
        pump_until(IMAGE_LOAD_TIMEOUT, || height() == 1000.0);
        assert_eq!(height(), 1000.0, "Auto の高さは画素の高さ");

        image.set_sizing(Sizing::fill());
        let viewport = scroll.metrics().viewport_height;
        pump_until(SCROLL_LAYOUT_TIMEOUT, || height() == viewport);
        assert_eq!(height(), viewport, "Fill にすると表示領域の余りに収まる");
        let metrics = scroll.metrics();
        assert_eq!(
            metrics.content_height, metrics.viewport_height,
            "中身は表示領域に収まる: {metrics:?}"
        );
    }));
    window.close();
    let _ = std::fs::remove_dir_all(&dir);
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
    Ok(())
}

/// A `Fill` image with a maximum asks for its maximum (the maximum doubles as the
/// size wanted under normal conditions), so in an `Auto` row it keeps that height
/// even when a `Fill` row takes the rest. Asking for nothing on a `Fill` axis, it
/// collapsed to zero there.
///
/// Messages are pumped until it is laid out; in environments where it never is
/// (CI), the sizes pass without checking.
fn capped_fill_image_asks_for_its_maximum(ui: &Ui) -> Result<()> {
    let dir = std::env::temp_dir().join(format!("naui 上限付きの画像 {}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("一時フォルダを作れない");
    let path = dir.join("40x20.bmp");
    std::fs::write(&path, bmp_solid(40, 20)).expect("画像を書き出せない");
    let path = path.to_str().expect("一時パスが UTF-8 でない").to_string();

    let window = ui.window("上限付きの画像", 400.0, 400.0)?;
    let image = ui.image(&path)?;
    image.set_sizing(Sizing::fill().max_height(100.0));
    let rest = ui.stack(Orientation::Vertical)?;
    rest.set_sizing(Sizing::fill());
    let grid = ui.grid()?;
    grid.set_column_track(0, naui_core::Track::FILL);
    grid.set_row_track(0, naui_core::Track::Auto);
    grid.set_row_track(1, naui_core::Track::FILL);
    grid.attach(&image, GridCell::new(0, 0));
    grid.attach(&rest, GridCell::new(0, 1));
    grid.set_sizing(Sizing::fixed(300.0, 300.0));
    let scroll = ui.scroll()?;
    scroll.set_child(&grid);
    scroll.set_sizing(Sizing::fill());
    window.set_child(&scroll);

    let host = native::<FrameworkElement>(&image);
    let rest_element = native::<FrameworkElement>(&rest);
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        if !show_and_measure(&window, &scroll) {
            return;
        }
        let height = |element: &FrameworkElement| element.ActualHeight().unwrap_or(f64::NAN);
        pump_until(IMAGE_LOAD_TIMEOUT, || height(&host) == 100.0);
        assert_eq!(height(&host), 100.0, "Auto 行でも上限の高さを保つ");
        assert_eq!(height(&rest_element), 200.0, "残りは Fill の行が受け取る");
    }));
    window.close();
    let _ = std::fs::remove_dir_all(&dir);
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
    Ok(())
}

/// An uncompressed 24-bit BMP of the given size, all one gray.
fn bmp_solid(width: u32, height: u32) -> Vec<u8> {
    const HEADER: u32 = 14 + 40;
    // 1 行を 4 バイト境界にそろえる。
    let row = (width * 3).div_ceil(4) * 4;
    let pixels = row * height;
    let mut bmp = Vec::new();
    bmp.extend_from_slice(b"BM");
    bmp.extend_from_slice(&(HEADER + pixels).to_le_bytes());
    bmp.extend_from_slice(&0u32.to_le_bytes());
    bmp.extend_from_slice(&HEADER.to_le_bytes());
    bmp.extend_from_slice(&40u32.to_le_bytes()); // BITMAPINFOHEADER の大きさ
    bmp.extend_from_slice(&(width as i32).to_le_bytes());
    bmp.extend_from_slice(&(height as i32).to_le_bytes());
    bmp.extend_from_slice(&1u16.to_le_bytes()); // プレーン数
    bmp.extend_from_slice(&24u16.to_le_bytes()); // 1 画素のビット数
    bmp.extend_from_slice(&0u32.to_le_bytes()); // 無圧縮
    bmp.extend_from_slice(&pixels.to_le_bytes());
    bmp.extend_from_slice(&[0; 16]); // 解像度と色数は使わない
    bmp.resize(bmp.len() + pixels as usize, 0x80);
    bmp
}

fn set_visible_maps_to_visibility(ui: &Ui) -> Result<()> {
    use naui_winui3::Microsoft::UI::Xaml::Visibility;

    let button = ui.button("t")?;
    assert!(button.is_visible(), "既定は表示");
    button.set_visible(false);
    assert!(!button.is_visible());
    assert_eq!(
        button.native_element().Visibility().expect("Visibility"),
        Visibility::Collapsed,
        "場所も取らない Collapsed にする"
    );
    button.set_visible(true);
    assert!(button.is_visible());
    assert_eq!(
        button.native_element().Visibility().expect("Visibility"),
        Visibility::Visible
    );
    Ok(())
}

fn tooltip_maps_to_tool_tip_service(ui: &Ui) -> Result<()> {
    use naui_winui3::Microsoft::UI::Xaml::Controls::ToolTipService;
    use naui_winui3::Microsoft::UI::Xaml::DependencyObject;

    let button = ui.button("保存")?;
    let object = button
        .native_element()
        .cast::<DependencyObject>()
        .expect("DependencyObject");
    button.set_tooltip(Some("変更を保存します"));
    let tip = ToolTipService::GetToolTip(&object)
        .expect("ToolTip")
        .cast::<IPropertyValue>()
        .and_then(|value| value.GetString())
        .expect("文字列の ToolTip");
    assert_eq!(tip.to_string(), "変更を保存します");

    button.set_tooltip(None);
    let cleared = ToolTipService::GetToolTip(&object)
        .ok()
        .and_then(|value| value.cast::<IPropertyValue>().ok())
        .and_then(|value| value.GetString().ok());
    assert_eq!(cleared, None, "None で外れる");
    Ok(())
}

/// There is no public API for triggering double-taps or context menu requests, so
/// this checks that rows can be rebuilt with the receivers attached and that they are
/// not called by selection operations (actual delivery is checked on real machines).
fn list_accepts_row_activate_and_context_menu(ui: &Ui) -> Result<()> {
    use naui_core::ListItem;

    let list = ui.list()?;
    let activated = Rc::new(RefCell::new(Vec::new()));
    let menus = Rc::new(RefCell::new(Vec::new()));
    list.on_row_activate({
        let activated = activated.clone();
        move |index| activated.borrow_mut().push(index)
    });
    list.on_context_menu({
        let menus = menus.clone();
        move |index, at: Point| menus.borrow_mut().push((index, at))
    });
    // 受け口を付けたあとでも行を作り直せる (行ごとの購読が付き直る)。
    list.set_items(&[
        ListItem::new("札幌"),
        ListItem::new("東京").detail("13,960,000 人"),
        ListItem::new("大阪").enabled(false),
    ]);
    let items = list
        .native_list_view()
        .Items()
        .expect("Items")
        .Size()
        .expect("Size");
    assert_eq!(items, 3);
    list.set_items(&[ListItem::new("那覇")]);
    assert_eq!(list.len(), 1);

    // 選択は起動でも文脈メニューでもない。
    list.select(0);
    assert_eq!(list.selection(), vec![0]);
    assert!(activated.borrow().is_empty());
    assert!(menus.borrow().is_empty());

    // 差し替えられる。
    list.on_row_activate(|_| {});
    list.on_context_menu(|_, _| {});
    Ok(())
}

/// Even before the list has a size, it builds only a foothold, not every row, and the
/// function is asked only for those rows.
fn list_builds_only_the_rows_it_needs(ui: &Ui) -> Result<()> {
    use naui_core::ROW_WINDOW_THRESHOLD;
    use naui_windows::ListRow;

    const ROWS: usize = 100_000;
    const LOCKED: usize = 50_000;
    let built = Rc::new(Cell::new(0usize));
    let list = ui.list()?;
    list.set_row_builder(ROWS, {
        let ui = ui.clone();
        let built = built.clone();
        move |index| {
            built.set(built.get() + 1);
            let row = ListRow::new(&ui.label(&index.to_string())?);
            Ok(row.selectable(index != LOCKED))
        }
    });
    assert_eq!(list.len(), ROWS);
    let items = list
        .native_list_view()
        .Items()
        .expect("Items")
        .Size()
        .expect("Size") as usize;
    assert_eq!(items, ROW_WINDOW_THRESHOLD, "先頭だけを組み立てること");
    assert_eq!(built.get(), ROW_WINDOW_THRESHOLD);

    // 組み立てずに答える関数があれば、画面の外の行も取り除ける。
    list.set_selection(&[LOCKED]);
    assert_eq!(list.selection(), vec![LOCKED], "述語が無ければ選べる扱い");
    list.clear_selection();
    list.set_row_selectable(|index| index != LOCKED);
    list.set_selection(&[LOCKED]);
    assert!(list.selection().is_empty(), "選べない行は取り除かれること");
    assert_eq!(
        built.get(),
        ROW_WINDOW_THRESHOLD,
        "選択のために組み立てない"
    );

    // `refresh` は組み立て直す。行数と選択は残る。
    list.set_selection(&[LOCKED - 1]);
    list.refresh();
    assert_eq!(built.get(), ROW_WINDOW_THRESHOLD * 2);
    assert_eq!(list.selection(), vec![LOCKED - 1]);
    assert_eq!(list.len(), ROWS);

    // 行の高さの指定。
    list.set_row_height(48.0);
    assert_eq!(list.row_height(), 48.0);
    list.set_row_height(0.0);
    assert_eq!(list.row_height(), 0.0, "0 以下で指定を外すこと");

    // 組み立ての中からリストを触っても二重借用にならない。
    let weak = list.downgrade();
    let label_ui = ui.clone();
    list.set_row_builder(3, move |index| {
        let list = weak.upgrade().expect("リストが生きていること");
        assert_eq!(list.len(), 3);
        Ok(ListRow::new(&label_ui.label(&index.to_string())?))
    });
    assert_eq!(list.len(), 3);
    Ok(())
}

/// Moving the window leaves the rows in both ranges alone: the builder is not called for
/// them, and a control inside one keeps the keyboard focus.
fn list_keeps_rows_that_stay_in_the_window(ui: &Ui) -> Result<()> {
    use naui_core::ROW_WINDOW_OVERSCAN;
    use naui_windows::ListRow;
    use naui_winui3::Microsoft::UI::Xaml::Controls::Control;
    use naui_winui3::Microsoft::UI::Xaml::FocusState;
    use std::collections::HashMap;

    const ROWS: usize = 1_000;
    const TARGET: usize = 100;
    let window = ui.window("窓に残る行", 240.0, 160.0)?;
    let list = ui.list()?;
    let inputs: Rc<RefCell<HashMap<usize, naui_windows::TextInput>>> = Rc::default();
    let builds: Rc<RefCell<HashMap<usize, usize>>> = Rc::default();
    {
        let ui = ui.clone();
        let inputs = inputs.clone();
        let builds = builds.clone();
        list.set_row_builder(ROWS, move |index| {
            *builds.borrow_mut().entry(index).or_default() += 1;
            let input = ui.text_input(&format!("行 {index}"))?;
            inputs.borrow_mut().insert(index, input.clone());
            Ok(ListRow::new(&input))
        });
    }
    list.set_sizing(Sizing::fill());
    window.set_child(&list);
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        window.show();
        let scroll = list_scroller(&list);
        pump_until(SCROLL_LAYOUT_TIMEOUT, || {
            scroll.ViewportHeight().unwrap_or(0.0) > 0.0 && list.row_height() > 0.0
        });
        if scroll.ViewportHeight().unwrap_or(0.0) <= 0.0 {
            println!("     (ScrollViewer に実寸が付かない環境なので、確かめずに通す)");
            return;
        }
        list.scroll_to_row(TARGET);
        pump_until(SCROLL_LAYOUT_TIMEOUT, || {
            inputs.borrow().contains_key(&TARGET)
        });
        let input = inputs.borrow().get(&TARGET).cloned().expect("送った先の行");
        pump_until(FOCUS_TIMEOUT, || input.request_focus());
        let focused = || {
            input
                .native_element()
                .cast::<Control>()
                .and_then(|control| control.FocusState())
                .is_ok_and(|state| state != FocusState::Unfocused)
        };
        if !focused() {
            println!("     (フォーカスを受けられない環境なので、確かめずに通す)");
            return;
        }
        let built = builds.borrow()[&TARGET];

        // 窓が先へ動いても、入力欄の行は窓の中に残っている。
        let next = TARGET + ROW_WINDOW_OVERSCAN;
        list.scroll_to_row(next);
        pump_until(SCROLL_LAYOUT_TIMEOUT, || {
            builds.borrow().contains_key(&next)
        });
        assert!(builds.borrow().contains_key(&next), "窓が動いていること");
        assert!(focused(), "窓が動いても入力欄にフォーカスが残ること");
        assert_eq!(
            builds.borrow()[&TARGET],
            built,
            "残る行は組み立て直さないこと"
        );
    }));
    window.close();
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
    Ok(())
}

/// The `ScrollViewer` of a list.
fn list_scroller(list: &naui_windows::List) -> ScrollViewer {
    native::<Border>(list)
        .Child()
        .and_then(|child| child.cast::<ScrollViewer>())
        .expect("リストの ScrollViewer")
}

/// The `ScrollViewer` of a table.
fn table_scroller(table: &naui_windows::Table) -> ScrollViewer {
    let children = native::<Grid>(table).Children().expect("表の子");
    (0..children.Size().expect("表の子の数"))
        .find_map(|i| children.GetAt(i).ok()?.cast::<ScrollViewer>().ok())
        .expect("表の ScrollViewer")
}

fn list_rows_follow_scrolling(ui: &Ui) -> Result<()> {
    use naui_core::ListItem;

    let window = ui.window("リストのスクロール", 240.0, 160.0)?;
    let list = ui.list()?;
    let labels: Vec<String> = (0..1000).map(|i| format!("行 {i}")).collect();
    list.set_items(&ListItem::list(labels.iter().map(String::as_str)));
    list.set_sizing(Sizing::fill());
    window.set_child(&list);
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        window.show();
        let scroll = list_scroller(&list);
        pump_until(SCROLL_LAYOUT_TIMEOUT, || {
            scroll.ViewportHeight().unwrap_or(0.0) > 0.0
        });
        if scroll.ViewportHeight().unwrap_or(0.0) <= 0.0 {
            println!("     (ScrollViewer に実寸が付かない環境なので、確かめずに通す)");
            return;
        }
        let body = scroll
            .Content()
            .and_then(|content| content.cast::<StackPanel>())
            .expect("リストの本体");
        let top_spacer = body
            .Children()
            .and_then(|children| children.GetAt(0))
            .and_then(|spacer| spacer.cast::<FrameworkElement>())
            .expect("先頭の詰め物");
        let leading = || top_spacer.Height().unwrap_or(0.0);
        assert_eq!(leading(), 0.0, "最初は先頭から組み立てている");
        let rows = || {
            list.native_list_view()
                .Items()
                .and_then(|items| items.Size())
                .expect("行の数") as usize
        };
        pump_until(SCROLL_LAYOUT_TIMEOUT, || rows() < 200);
        assert!(rows() < 200, "見えている分に絞られること: {} 行", rows());

        list.scroll_to_row(800);
        pump_until(Duration::from_secs(2), || leading() > 0.0);
        let offset = scroll.VerticalOffset().expect("リストの位置");
        assert!(offset > 0.0, "リストが送られている");
        assert!(
            leading() > 0.0 && leading() <= offset,
            "組み立てる範囲が表示位置へ付いてくる (詰め物 {}, 位置 {offset})",
            leading()
        );
        assert!(rows() < 200, "送っても絞られていること: {} 行", rows());
    }));
    window.close();
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
    Ok(())
}

/// Replacing the rows from inside the function that builds a row leaves only the new
/// rows, in the new window, however far the build had got.
fn list_builder_can_replace_the_rows(ui: &Ui) -> Result<()> {
    use naui_core::ListItem;
    use naui_windows::ListRow;

    /// What the old builder does when it reaches the row it was waiting for.
    type Replace = Rc<dyn Fn(&naui_windows::List)>;

    const ROWS: usize = 1_000;
    let window = ui.window("行の差し替え", 240.0, 160.0)?;
    let list = ui.list()?;
    list.set_sizing(Sizing::fill());
    window.set_child(&list);
    let slot = Rc::new(RefCell::new(None::<Replace>));
    let fill = |list: &naui_windows::List| {
        let weak = list.downgrade();
        let ui = ui.clone();
        let slot = slot.clone();
        list.set_row_builder(ROWS, move |index| {
            // 窓の途中の行で差し替える。
            if index % 7 == 3 {
                let replace = slot.borrow_mut().take();
                if let (Some(replace), Some(list)) = (replace, weak.upgrade()) {
                    replace(&list);
                }
            }
            Ok(ListRow::new(&ui.label(&format!("古い {index}"))?))
        });
    };
    fill(&list);

    let outcome = catch_unwind(AssertUnwindSafe(|| {
        window.show();
        let scroll = list_scroller(&list);
        pump_until(SCROLL_LAYOUT_TIMEOUT, || {
            scroll.ViewportHeight().unwrap_or(0.0) > 0.0
        });
        if scroll.ViewportHeight().unwrap_or(0.0) <= 0.0 {
            println!("     (ScrollViewer に実寸が付かない環境なので、確かめずに通す)");
            return;
        }
        let rows = || {
            list.native_list_view()
                .Items()
                .and_then(|items| items.Size())
                .expect("行の数") as usize
        };

        // 別の組み立て関数へ。組み立てられるのは新しい窓の行だけで、1 回ずつ。
        let new_built = Rc::new(RefCell::new(Vec::new()));
        *slot.borrow_mut() = Some(Rc::new({
            let ui = ui.clone();
            let new_built = new_built.clone();
            move |list: &naui_windows::List| {
                let ui = ui.clone();
                let new_built = new_built.clone();
                list.set_row_builder(2_000, move |index| {
                    new_built.borrow_mut().push(index);
                    Ok(ListRow::new(&ui.label(&format!("新しい {index}"))?))
                });
            }
        }));
        list.refresh();
        assert_eq!(list.len(), 2_000);
        assert!(rows() > 0, "行があること");
        assert_eq!(new_built.borrow().first(), Some(&0));
        let built = new_built.borrow().clone();
        assert_eq!(
            built,
            (0..built.len()).collect::<Vec<_>>(),
            "新しい関数は新しい窓の行だけを、1 回ずつ組み立てること"
        );

        // 窓が動く途中に、行の少ない固定の行へ。
        fill(&list);
        list.set_selection(&[1]);
        *slot.borrow_mut() = Some(Rc::new(|list: &naui_windows::List| {
            list.set_items(&ListItem::list(["東京", "大阪", "京都"]));
        }));
        list.scroll_to_row(800);
        pump_until(Duration::from_secs(2), || list.len() == 3 && rows() == 3);
        assert_eq!(list.len(), 3);
        assert_eq!(rows(), 3, "新しい行の数だけになること");
        assert!(list.selection().is_empty(), "選択は外れること");

        // 組み立てるたびに差し替える関数でも止まること。
        fn keep_replacing(list: &naui_windows::List, ui: &Ui) {
            let weak = list.downgrade();
            let ui = ui.clone();
            list.set_row_builder(ROWS, move |index| {
                if let Some(list) = weak.upgrade() {
                    keep_replacing(&list, &ui);
                }
                Ok(ListRow::new(&ui.label(&index.to_string())?))
            });
        }
        keep_replacing(&list, ui);
        assert_eq!(list.len(), ROWS);
    }));
    window.close();
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
    Ok(())
}

/// The height set is a minimum. A row that needs more grows, so the spacers must use the
/// real height rather than the one set.
fn list_rows_taller_than_the_height_set_keep_the_window_right(ui: &Ui) -> Result<()> {
    use naui_core::ListItem;

    const ROWS: usize = 1_000;
    let window = ui.window("指定より高い行", 240.0, 160.0)?;
    let list = ui.list()?;
    list.set_row_height(1.0);
    let labels: Vec<String> = (0..ROWS).map(|i| format!("行 {i}")).collect();
    list.set_items(&ListItem::list(labels.iter().map(String::as_str)));
    list.set_sizing(Sizing::fill());
    window.set_child(&list);
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        window.show();
        let scroll = list_scroller(&list);
        pump_until(SCROLL_LAYOUT_TIMEOUT, || {
            scroll.ViewportHeight().unwrap_or(0.0) > 0.0 && list.row_height() > 1.0
        });
        if scroll.ViewportHeight().unwrap_or(0.0) <= 0.0 {
            println!("     (ScrollViewer に実寸が付かない環境なので、確かめずに通す)");
            return;
        }
        let height = list.row_height();
        assert!(height > 1.0, "中身に合わせて伸びること: {height}");
        let expected = height * ROWS as f64;
        pump_until(SCROLL_LAYOUT_TIMEOUT, || {
            (scroll.ExtentHeight().unwrap_or(0.0) - expected).abs() < 2.0
        });
        let extent = scroll.ExtentHeight().expect("全体の高さ");
        assert!(
            (extent - expected).abs() < 2.0,
            "スクロールの範囲が実際の高さの全行分であること: {extent} / {expected}"
        );

        // 送った先で高さを変えても、組み立てる範囲が表示位置へ付いてくる。
        let body = scroll
            .Content()
            .and_then(|content| content.cast::<StackPanel>())
            .expect("リストの本体");
        let top_spacer = body
            .Children()
            .and_then(|children| children.GetAt(0))
            .and_then(|spacer| spacer.cast::<FrameworkElement>())
            .expect("先頭の詰め物");
        let leading = || top_spacer.Height().unwrap_or(0.0);
        list.scroll_to_row(800);
        pump_until(Duration::from_secs(2), || leading() > 0.0);
        list.set_row_height(height * 2.0);
        pump_until(Duration::from_secs(2), || {
            leading() <= scroll.VerticalOffset().unwrap_or(0.0)
        });
        let offset = scroll.VerticalOffset().expect("リストの位置");
        assert!(
            leading() <= offset,
            "高さを変えても行が画面にあること (詰め物 {}, 位置 {offset})",
            leading()
        );
    }));
    window.close();
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
    Ok(())
}

/// An `Auto` height is as tall as all the rows, so every row is built at its own height.
/// Only a height the app limited makes the list build what is on screen.
fn list_auto_height_builds_every_row(ui: &Ui) -> Result<()> {
    use naui_core::{ListItem, ROW_WINDOW_THRESHOLD};

    const ROWS: usize = 300;
    let items: Vec<ListItem> = (0..ROWS)
        .map(|index| match index % 2 {
            0 => ListItem::new(format!("行 {index}")),
            _ => ListItem::new(format!("行 {index}")).detail("補助の文字"),
        })
        .collect();
    let window = ui.window("Auto の高さのリスト", 240.0, 160.0)?;
    let list = ui.list()?;
    list.set_items(&items);
    window.set_child(&list);
    let rows = || {
        list.native_list_view()
            .Items()
            .and_then(|items| items.Size())
            .expect("行の数") as usize
    };
    assert_eq!(
        rows(),
        ROW_WINDOW_THRESHOLD,
        "大きさが分かるまでは先頭だけを組み立てる"
    );
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        window.show();
        let scroll = list_scroller(&list);
        pump_until(SCROLL_LAYOUT_TIMEOUT, || {
            scroll.ViewportHeight().unwrap_or(0.0) > 0.0 && rows() == ROWS
        });
        if scroll.ViewportHeight().unwrap_or(0.0) <= 0.0 {
            println!("     (ScrollViewer に実寸が付かない環境なので、確かめずに通す)");
            return;
        }
        assert_eq!(rows(), ROWS, "Auto の高さでは全行を組み立てること");
        let height = |index: u32| {
            list.native_list_view()
                .Items()
                .and_then(|items| items.GetAt(index))
                .and_then(|item| item.cast::<ListViewItem>())
                .and_then(|item| item.ActualHeight())
                .expect("行の高さ")
        };
        pump_until(Duration::from_secs(2), || {
            height(0) > 0.0 && height(1) > height(0)
        });
        assert!(
            height(0) > 0.0 && height(1) > height(0),
            "行ごとの高さのままであること: {} / {}",
            height(0),
            height(1)
        );

        // 指定を付けると絞り、外すと全行へ戻る。実際の大きさが変わらなくても追従する。
        list.set_sizing(Sizing::fill());
        pump_until(SCROLL_LAYOUT_TIMEOUT, || rows() < ROW_WINDOW_THRESHOLD);
        assert!(rows() < ROW_WINDOW_THRESHOLD, "絞られること: {} 行", rows());
        list.set_sizing(Sizing::AUTO);
        pump_until(SCROLL_LAYOUT_TIMEOUT, || rows() == ROWS);
        assert_eq!(rows(), ROWS, "指定を外すと全行へ戻ること");
    }));
    window.close();
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
    Ok(())
}

/// The `ListView` has only the rows around the screen, so assistive technology is told
/// each row's place among all the rows and how many there are.
fn list_rows_report_their_place_among_all_rows(ui: &Ui) -> Result<()> {
    use naui_windows::ListRow;
    use naui_winui3::Microsoft::UI::Xaml::Automation::AutomationProperties;

    let window = ui.window("リストの行の位置", 240.0, 160.0)?;
    let list = ui.list()?;
    list.set_sizing(Sizing::fill());
    let fill = |count: usize| {
        let ui = ui.clone();
        list.set_row_builder(count, move |index| {
            Ok(ListRow::new(&ui.label(&format!("行 {index}"))?))
        });
    };
    // 行の番号は中の文字から読む。窓に残る行は組み立て直さないので、組み立てた順の
    // 記録からは、いま `ListView` にある行が分からない。
    let index_of = |row: &ListViewItem| {
        row.Content()
            .and_then(|content| content.cast::<Grid>())
            .and_then(|host| host.Children())
            .and_then(|children| children.GetAt(0))
            .and_then(|label| label.cast::<TextBlock>())
            .and_then(|label| label.Text())
            .expect("行の文字")
            .to_string()
            .trim_start_matches("行 ")
            .parse::<usize>()
            .expect("行の番号")
    };
    // どの行も位置と全行数を伝えていることを確かめ、行の数と先頭の番号を返す。
    let check = |count: usize| {
        let items = list.native_list_view().Items().expect("行");
        let size = items.Size().expect("行の数") as usize;
        let mut first = 0;
        for slot in 0..size {
            let row = items
                .GetAt(slot as u32)
                .and_then(|item| item.cast::<ListViewItem>())
                .expect("行");
            let index = index_of(&row);
            if slot == 0 {
                first = index;
            }
            assert_eq!(index, first + slot, "番号の順に並んでいること");
            assert_eq!(
                AutomationProperties::GetPositionInSet(&row).expect("位置"),
                index as i32 + 1,
                "全行の中の位置であること"
            );
            assert_eq!(
                AutomationProperties::GetSizeOfSet(&row).expect("全行数"),
                count as i32,
                "組み立てた行の数ではなく全行数であること"
            );
        }
        (size, first)
    };
    fill(1000);
    window.set_child(&list);
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        window.show();
        let scroll = list_scroller(&list);
        pump_until(SCROLL_LAYOUT_TIMEOUT, || {
            scroll.ViewportHeight().unwrap_or(0.0) > 0.0
        });
        if scroll.ViewportHeight().unwrap_or(0.0) <= 0.0 {
            println!("     (ScrollViewer に実寸が付かない環境なので、確かめずに通す)");
            return;
        }
        let (_, first) = check(1000);
        assert_eq!(first, 0, "最初は先頭から組み立てている");

        // 送っても、窓の中の位置にならない。
        list.scroll_to_row(800);
        pump_until(Duration::from_secs(2), || check(1000).1 > 0);
        let (size, first) = check(1000);
        assert!(first > 0, "組み立てる範囲が付いてくること");
        assert!(size < 200, "絞られていること: {size} 行");

        // 行数が変わったら、新しい全行数を伝える。
        fill(500);
        check(500);
    }));
    window.close();
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
    Ok(())
}

/// The selection is held by naui, so it covers rows the `ListView` does not have.
fn list_keeps_the_selection_outside_the_window(ui: &Ui) -> Result<()> {
    use naui_core::{ListItem, SelectionMode};

    const ROWS: usize = 5_000;
    let labels: Vec<String> = (0..ROWS).map(|i| format!("行 {i}")).collect();
    let window = ui.window("窓の外の選択", 240.0, 160.0)?;
    let list = ui.list()?;
    list.set_selection_mode(SelectionMode::Multiple);
    list.set_items(&ListItem::list(labels.iter().map(String::as_str)));
    list.set_sizing(Sizing::fill());
    window.set_child(&list);
    let seen = Rc::new(RefCell::new(Vec::new()));
    list.on_select({
        let seen = seen.clone();
        move |indices| seen.borrow_mut().push(indices.to_vec())
    });

    let outcome = catch_unwind(AssertUnwindSafe(|| {
        // `ListViewItem` の `IsSelected` の変更が `SelectionChanged` になるのは、
        // `ListView` が表示されてから。
        window.show();
        let scroll = list_scroller(&list);
        pump_until(SCROLL_LAYOUT_TIMEOUT, || {
            scroll.ViewportHeight().unwrap_or(0.0) > 0.0
        });
        if scroll.ViewportHeight().unwrap_or(0.0) <= 0.0 {
            println!("     (ScrollViewer に実寸が付かない環境なので、確かめずに通す)");
            return;
        }
        let items = list.native_list_view().Items().expect("Items");
        let item = |index: u32| {
            items
                .GetAt(index)
                .and_then(|item| item.cast::<ListViewItem>())
                .expect("行")
        };

        // まだ組み立てていない行も選べる。組み立ててある行は `ListView` でも選ばれる。
        list.set_selection(&[1, 4_000]);
        assert_eq!(list.selection(), vec![1, 4_000]);
        assert!(item(1).IsSelected().expect("IsSelected"));
        assert!(!item(0).IsSelected().expect("IsSelected"));
        assert!(seen.borrow().is_empty(), "通知しない経路であること");

        // ネイティブ側で選ばれると、窓の外の扱いは修飾キーで決まる (ここでは押されていない)。
        item(0).SetIsSelected(true).expect("SetIsSelected");
        pump_until(Duration::from_secs(2), || !seen.borrow().is_empty());
        assert_eq!(seen.borrow().last(), Some(&vec![0, 1]));
        assert_eq!(list.selection(), vec![0, 1], "選び直しは窓の外を落とすこと");

        // 通知する経路も同じ。
        list.select_many(&[2, 4_500]);
        assert_eq!(list.selection(), vec![2, 4_500]);
        assert_eq!(seen.borrow().last(), Some(&vec![2, 4_500]));

        // 単一選択では 1 行だけ。
        list.set_selection_mode(SelectionMode::Single);
        assert!(list.selection().is_empty());
        list.set_selected(4_500);
        assert_eq!(list.selection(), vec![4_500]);
        let before = seen.borrow().len();
        item(0).SetIsSelected(true).expect("SetIsSelected");
        pump_until(Duration::from_secs(2), || seen.borrow().len() > before);
        assert_eq!(list.selection(), vec![0], "窓の外の行を残さないこと");
    }));
    window.close();
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
    Ok(())
}

fn request_focus_needs_the_visual_tree(ui: &Ui) -> Result<()> {
    let input = ui.text_input("")?;
    assert!(!input.request_focus(), "載る前は移せない");
    let label = ui.label("見出し")?;
    assert!(!label.request_focus());
    Ok(())
}

/// There is no public API for triggering keys, so this checks only that they can be
/// attached (actual delivery is checked on real machines).
fn text_widgets_accept_key_handlers(ui: &Ui) -> Result<()> {
    use naui_core::{EventResponse, KeyEvent};
    let handler = |_: &KeyEvent| EventResponse::Continue;
    ui.text_input("")?.on_key_down(handler);
    ui.text_area("")?.on_key_down(handler);
    ui.password_input()?.on_key_down(handler);
    ui.search_input()?.on_key_down(handler);
    ui.number_input(1.0)?.on_key_down(handler);
    ui.editable_combo_box()?.on_key_down(handler);
    Ok(())
}

/// A drag cannot be started from code, so this checks only that the window's root
/// becomes a drop target and that closures can be attached (delivery is checked on
/// real machines).
fn file_drop_makes_the_root_a_drop_target(ui: &Ui) -> Result<()> {
    let window = ui.window("ファイルの受け取り", 320.0, 200.0)?;
    let label = ui.label("ここへ落とす")?;
    label.on_file_drop(|_| {});
    label.on_file_drop(|_| {});
    window.on_file_drop(|_| {});
    window.set_child(&label);
    let root = window.native_window().Content().expect("ウィンドウの中身");
    assert!(root.AllowDrop().expect("AllowDrop"), "根がドロップを受ける");
    // 閉じずに残すと、終わりの `Application::Exit` のあともアプリが終わらない。
    window.close();
    Ok(())
}

/// `on_file_drop` does not require a child: a window without one gets an empty root
/// that takes drops, and a later `set_child` keeps the new root a drop target.
fn file_drop_works_without_a_child(ui: &Ui) -> Result<()> {
    let window = ui.window("子のない窓", 320.0, 200.0)?;
    window.on_file_drop(|_| {});
    let root = window.native_window().Content().expect("受け口の根");
    assert!(
        root.AllowDrop().expect("AllowDrop"),
        "子が無くても根がドロップを受ける"
    );
    window.on_file_drop(|_| {});
    let again = window.native_window().Content().expect("受け口の根");
    assert_eq!(again, root, "付け直しても根は作り直さない");
    window.set_child(&ui.label("あとから置く")?);
    let replaced = window.native_window().Content().expect("ウィンドウの中身");
    assert_ne!(replaced, root, "set_child が根を置き換える");
    assert!(
        replaced.AllowDrop().expect("AllowDrop"),
        "置き換えた根もドロップを受ける"
    );
    window.close();
    Ok(())
}

/// There is no public API for triggering Enter, so this checks only that they can be
/// attached and that attaching them does not change how text and values are handled
/// (actual delivery is checked on real machines).
fn text_widgets_accept_submit_handlers(ui: &Ui) -> Result<()> {
    let input = ui.text_input("ねこ")?;
    input.on_submit(|_: &str| {});
    assert_eq!(input.text(), "ねこ");
    let password = ui.password_input()?;
    password.on_submit(|_: &str| {});
    password.set_text("ひみつ");
    assert_eq!(password.text(), "ひみつ");
    let number = ui.number_input(3.0)?;
    number.on_submit(|_: f64| {});
    assert_eq!(number.value(), 3.0);
    Ok(())
}

/// Focus moves (`GotFocus`) arrive asynchronously, so this checks only that handlers
/// can be attached and reattached (actual delivery is checked on real machines).
fn text_widgets_accept_focus_handlers(ui: &Ui) -> Result<()> {
    let handler = |_: bool| {};
    let input = ui.text_input("")?;
    input.on_focus_change(handler);
    input.on_focus_change(handler);
    ui.text_area("")?.on_focus_change(handler);
    ui.password_input()?.on_focus_change(handler);
    ui.search_input()?.on_focus_change(handler);
    ui.number_input(1.0)?.on_focus_change(handler);
    ui.editable_combo_box()?.on_focus_change(handler);
    Ok(())
}

/// The handler is held by the widget, so dropping it frees the handler (without
/// waiting for the next registration).
fn focus_change_handler_is_freed_with_the_widget(ui: &Ui) -> Result<()> {
    // 通知先が捕まえている状態。解放されれば数が 1 へ戻る。
    let state = Rc::new(());
    let capture = || {
        let state = state.clone();
        move |_: bool| {
            let _ = &state;
        }
    };

    let input = ui.text_input("")?;
    input.on_focus_change(capture());
    // 差し替えても、前の通知先は残らない。
    input.on_focus_change(capture());
    assert_eq!(Rc::strong_count(&state), 2);
    drop(input);
    assert_eq!(Rc::strong_count(&state), 1, "1 行入力の通知先");

    // 表示中の木から外して捨てる。
    let root = ui.stack(Orientation::Vertical)?;
    let area = ui.text_area("")?;
    root.append(&area);
    area.on_focus_change(capture());
    root.remove(0);
    drop(area);
    assert_eq!(
        Rc::strong_count(&state),
        1,
        "外して捨てた複数行入力の通知先"
    );

    ui.password_input()?.on_focus_change(capture());
    ui.search_input()?.on_focus_change(capture());
    ui.number_input(1.0)?.on_focus_change(capture());
    ui.editable_combo_box()?.on_focus_change(capture());
    assert_eq!(Rc::strong_count(&state), 1, "その場で捨てた欄の通知先");
    Ok(())
}

/// The maximum time to wait for a focus move to arrive.
const FOCUS_TIMEOUT: Duration = Duration::from_secs(5);

/// Focus moves (`GotFocus` / `LostFocus`) arrive asynchronously, so messages are
/// pumped while waiting.
fn weak_widget_breaks_the_callback_cycle(ui: &Ui) -> Result<()> {
    let window = ui.window("弱参照", 320.0, 200.0)?;
    let root = ui.stack(Orientation::Vertical)?;
    let mail = ui.text_input(" a@example.com ")?;
    let other = ui.text_input("")?;
    root.append(&mail);
    root.append(&other);
    window.set_child(&root);
    window.show();

    // 通知の中で欄自身を触る。
    let seen = Rc::new(RefCell::new(Vec::new()));
    let weak = mail.downgrade();
    mail.on_focus_change({
        let weak = weak.clone();
        let seen = seen.clone();
        move |focused| {
            seen.borrow_mut().push(focused);
            if let Some(mail) = weak.upgrade() {
                if !focused {
                    mail.set_text(mail.text().trim());
                }
            }
        }
    });
    pump_until(FOCUS_TIMEOUT, || mail.request_focus());
    pump_until(FOCUS_TIMEOUT, || seen.borrow().contains(&true));
    assert!(other.request_focus());
    pump_until(FOCUS_TIMEOUT, || seen.borrow().contains(&false));
    assert_eq!(*seen.borrow(), [true, false]);
    assert_eq!(mail.text(), "a@example.com");

    // 画面から外してハンドルを手放せば、通知が弱参照を持っていても解放される。
    root.remove(0);
    drop(mail);
    assert!(weak.upgrade().is_none(), "外して手放した欄が残っている");

    // 文字入力でないウィジェットにもあり、写しも同じものを指す。
    let label = ui.label("見出し")?;
    label.set_selectable(true);
    let weak_label = label.downgrade();
    assert_eq!(
        weak_label.clone().upgrade().map(|l| l.is_selectable()),
        Some(true)
    );
    drop(label);
    assert!(weak_label.upgrade().is_none());
    window.close();
    Ok(())
}

fn timers_fire_on_the_dispatcher_queue(ui: &Ui) -> Result<()> {
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
    pump_until(Duration::from_secs(3), || {
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
    assert_eq!(progress.value(), 0.7);
    Ok(())
}

fn spinner_is_a_progress_ring(ui: &Ui) -> Result<()> {
    let spinner = ui.spinner()?;
    assert!(spinner.is_spinning(), "作った時点で回っている");
    let ring = native::<ProgressRing>(&spinner);
    assert!(ring.IsActive().expect("IsActive"));
    let element: FrameworkElement = ring.cast().expect("FrameworkElement");
    let size = |element: &FrameworkElement| {
        (
            element.Width().expect("Width"),
            element.Height().expect("Height"),
        )
    };
    assert_eq!(size(&element), (32.0, 32.0), "既定スタイルと同じ 32 x 32");

    spinner.set_spinning(false);
    assert!(!spinner.is_spinning());
    assert!(!ring.IsActive().expect("IsActive"));
    spinner.set_spinning(true);
    assert!(ring.IsActive().expect("IsActive"));

    spinner.set_sizing(Sizing::fixed(16.0, 16.0));
    assert_eq!(size(&element), (16.0, 16.0), "固定の大きさに従う");
    spinner.set_sizing(Sizing::new());
    assert_eq!(
        size(&element),
        (32.0, 32.0),
        "指定を外すと既定の大きさに戻る"
    );
    spinner.set_sizing(Sizing::fill_width());
    let (width, height) = size(&element);
    assert!(width.is_nan(), "Fill の軸は親に合わせる: {width}");
    assert_eq!(height, 32.0, "Fill でない軸は既定のまま");
    Ok(())
}

/// An auto-sized spinner starts at the left of a wide cell. A change of sizing
/// after attaching must preserve that placement, and Fill must still stretch.
fn spinner_starts_at_grid_cell(ui: &Ui) -> Result<()> {
    let window = ui.window("Spinner placement", 400.0, 150.0)?;
    let grid = ui.grid()?;
    grid.set_column_track(0, naui_core::Track::FILL);
    grid.set_sizing(Sizing::fill());
    let spinner = ui.spinner()?;
    grid.attach(&spinner, GridCell::new(0, 0));
    let child = native::<FrameworkElement>(&spinner);
    let root = native::<Grid>(&grid);

    // This also catches the regression on machines where windows cannot be laid out.
    assert_eq!(
        child.HorizontalAlignment().expect("横の寄せ方"),
        HorizontalAlignment::Left,
        "既定のスピナーはマスの先頭に置く"
    );
    window.set_child(&grid);
    window.show();
    let width = || child.ActualWidth().unwrap_or(0.0);
    let origin = || {
        child
            .TransformToVisual(&root)
            .and_then(|transform| {
                transform.TransformPoint(windows::Foundation::Point { X: 0.0, Y: 0.0 })
            })
            .map(|point| f64::from(point.X))
            .unwrap_or(f64::NAN)
    };
    pump_until(SCROLL_LAYOUT_TIMEOUT, || width() > 0.0);
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        if width() <= 0.0 {
            println!("     (実寸が付かない環境なので、大きさと位置は確かめずに通す)");
            return;
        }
        assert!((width() - 32.0).abs() < 0.5, "既定の幅: {}", width());
        assert!(origin().abs() < 0.5, "既定の位置: {}", origin());

        spinner.set_sizing(Sizing::fixed(16.0, 16.0));
        pump_until(SCROLL_LAYOUT_TIMEOUT, || (width() - 16.0).abs() < 0.5);
        assert!((width() - 16.0).abs() < 0.5, "固定幅: {}", width());
        assert!(origin().abs() < 0.5, "固定幅の位置: {}", origin());

        spinner.set_sizing(Sizing::fill_width());
        pump_until(SCROLL_LAYOUT_TIMEOUT, || {
            (width() - root.ActualWidth().unwrap_or(0.0)).abs() < 0.5
        });
        assert!(
            (width() - root.ActualWidth().unwrap_or(0.0)).abs() < 0.5,
            "Fill はマスいっぱい: {}",
            width()
        );
        assert!(origin().abs() < 0.5, "Fill の位置: {}", origin());

        spinner.set_sizing(Sizing::new());
        pump_until(SCROLL_LAYOUT_TIMEOUT, || (width() - 32.0).abs() < 0.5);
        assert!((width() - 32.0).abs() < 0.5, "Auto に戻した幅: {}", width());
        assert!(origin().abs() < 0.5, "Auto に戻した位置: {}", origin());
    }));
    window.close();
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
    Ok(())
}

/// An Auto spinner follows its Stack's cross-axis alignment on both axes.
fn spinner_follows_stack_fill(ui: &Ui) -> Result<()> {
    let window = ui.window("Spinner in a Stack", 400.0, 160.0)?;
    let stack = ui.stack(Orientation::Vertical)?;
    stack.set_sizing(Sizing::fill());
    let spinner = ui.spinner()?;
    stack.append(&spinner);
    let child = native::<FrameworkElement>(&spinner);
    let parent = native::<FrameworkElement>(&stack);
    stack.set_align(Align::Fill);
    assert!(
        child.Width().expect("Width").is_nan(),
        "親の Fill で Auto の固定幅を外す"
    );
    stack.set_align(Align::Center);
    assert_eq!(
        child.Width().expect("Width"),
        32.0,
        "Fill を外すと既定幅に戻る"
    );
    window.set_child(&stack);
    window.show();

    let width = || child.ActualWidth().unwrap_or(0.0);
    let height = || child.ActualHeight().unwrap_or(0.0);
    let parent_width = || parent.ActualWidth().unwrap_or(0.0);
    let parent_height = || parent.ActualHeight().unwrap_or(0.0);
    pump_until(SCROLL_LAYOUT_TIMEOUT, || {
        width() > 0.0 && parent_width() > 32.0
    });
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        if width() <= 0.0 || parent_width() <= 32.0 {
            println!("     (実寸が付かない環境なので、配置は確かめずに通す)");
            return;
        }
        assert!((width() - 32.0).abs() < 0.5, "既定の幅: {}", width());
        assert!((height() - 32.0).abs() < 0.5, "既定の高さ: {}", height());

        stack.set_align(Align::Fill);
        pump_until(SCROLL_LAYOUT_TIMEOUT, || {
            (width() - parent_width()).abs() < 0.5
        });
        assert!(
            (width() - parent_width()).abs() < 0.5,
            "縦 Stack の Fill: 子の幅 {}、親の幅 {}",
            width(),
            parent_width()
        );
        assert!((height() - 32.0).abs() < 0.5, "Auto の高さ: {}", height());

        stack.set_align(Align::Start);
        pump_until(SCROLL_LAYOUT_TIMEOUT, || (width() - 32.0).abs() < 0.5);
        assert!(
            (width() - 32.0).abs() < 0.5,
            "Start に戻した幅: {}",
            width()
        );

        spinner.set_sizing(Sizing::fill_width());
        pump_until(SCROLL_LAYOUT_TIMEOUT, || {
            (width() - parent_width()).abs() < 0.5
        });
        assert!(
            (width() - parent_width()).abs() < 0.5,
            "子自身の Fill: {}",
            width()
        );
        spinner.set_sizing(Sizing::new());
        pump_until(SCROLL_LAYOUT_TIMEOUT, || (width() - 32.0).abs() < 0.5);
        assert!(
            (width() - 32.0).abs() < 0.5,
            "子を Auto に戻した幅: {}",
            width()
        );

        stack.set_align(Align::Fill);
        spinner.set_spinning(false);
        pump_until(SCROLL_LAYOUT_TIMEOUT, || {
            (width() - parent_width()).abs() < 0.5
        });
        assert!(
            (width() - parent_width()).abs() < 0.5,
            "停止中の幅: {}",
            width()
        );
        spinner.set_spinning(true);
        spinner.set_sizing(Sizing::fixed(16.0, 16.0));
        pump_until(SCROLL_LAYOUT_TIMEOUT, || (width() - 16.0).abs() < 0.5);
        assert!((width() - 16.0).abs() < 0.5, "固定幅: {}", width());
        spinner.set_sizing(Sizing::new().max_width(16.0));
        pump_until(SCROLL_LAYOUT_TIMEOUT, || (width() - 16.0).abs() < 0.5);
        assert!((width() - 16.0).abs() < 0.5, "最大幅: {}", width());

        spinner.set_sizing(Sizing::new());
        pump_until(SCROLL_LAYOUT_TIMEOUT, || {
            (width() - parent_width()).abs() < 0.5
        });
        assert!(
            (width() - parent_width()).abs() < 0.5,
            "Auto に戻した幅: {}",
            width()
        );

        stack.set_sizing(Sizing::fixed(16.0, 64.0));
        pump_until(SCROLL_LAYOUT_TIMEOUT, || {
            (parent_width() - 16.0).abs() < 0.5
        });
        assert!(
            (width() - 16.0).abs() < 0.5,
            "小さい親に合わせた幅: {}",
            width()
        );
        stack.set_sizing(Sizing::fill());
        pump_until(SCROLL_LAYOUT_TIMEOUT, || parent_width() > 32.0);

        stack.set_orientation(Orientation::Horizontal);
        pump_until(SCROLL_LAYOUT_TIMEOUT, || {
            (height() - parent_height()).abs() < 0.5
        });
        assert!((width() - 32.0).abs() < 0.5, "横 Stack の幅: {}", width());
        assert!(
            (height() - parent_height()).abs() < 0.5,
            "横 Stack の Fill: 子の高さ {}、親の高さ {}",
            height(),
            parent_height()
        );

        stack.remove(0);
        assert_eq!(child.Width().expect("Width"), 32.0, "外すと既定幅に戻る");
        assert_eq!(child.Height().expect("Height"), 32.0, "外すと既定高に戻る");

        let filled = ui.stack(Orientation::Vertical).expect("Stack");
        filled.set_align(Align::Fill);
        filled.append(&spinner);
        assert!(
            child.Width().expect("Width").is_nan(),
            "先に親を Fill にして追加しても親の幅に合わせる"
        );
        filled.remove(0);
        assert_eq!(child.Width().expect("Width"), 32.0, "外すと既定幅に戻る");

        let scroll = ui.scroll().expect("Scroll");
        scroll.set_policy(ScrollPolicy::Never, ScrollPolicy::Auto);
        scroll.set_sizing(Sizing::fill());
        scroll.set_child(&spinner);
        window.set_child(&scroll);
        pump_until(SCROLL_LAYOUT_TIMEOUT, || width() > 32.0);
        assert!(
            width() > 32.0,
            "Scroll の非スクロール軸は広がる: {}",
            width()
        );
        scroll.set_policy(ScrollPolicy::Auto, ScrollPolicy::Auto);
        pump_until(SCROLL_LAYOUT_TIMEOUT, || (width() - 32.0).abs() < 0.5);
        assert!(
            (width() - 32.0).abs() < 0.5,
            "スクロール軸は既定幅: {}",
            width()
        );
        scroll.set_policy(ScrollPolicy::Never, ScrollPolicy::Auto);
        pump_until(SCROLL_LAYOUT_TIMEOUT, || width() > 32.0);
        assert!(width() > 32.0, "非スクロール軸に戻すと広がる: {}", width());
    }));
    window.close();
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
    Ok(())
}

/// An Auto spinner as window content fills the content area, including after a
/// size change or sidebar transition. Fixed and capped sizes remain explicit.
fn spinner_fills_window_content(ui: &Ui) -> Result<()> {
    let window = ui.window("Spinner as window content", 420.0, 200.0)?;
    let spinner = ui.spinner()?;
    let element = native::<FrameworkElement>(&spinner);
    window.set_child(&spinner);

    let outcome = catch_unwind(AssertUnwindSafe(|| {
        assert!(
            element.Width().expect("Width").is_nan(),
            "ウィンドウの幅に合わせる"
        );
        assert!(
            element.Height().expect("Height").is_nan(),
            "ウィンドウの高さに合わせる"
        );
        assert_eq!(
            element.HorizontalAlignment().unwrap(),
            HorizontalAlignment::Stretch
        );
        assert_eq!(
            element.VerticalAlignment().unwrap(),
            VerticalAlignment::Stretch
        );
        window.show();
        let content = window
            .native_window()
            .Content()
            .expect("ウィンドウの中身")
            .cast::<Grid>()
            .expect("根は Grid")
            .Children()
            .expect("根の子")
            .GetAt(2)
            .expect("中身の行")
            .cast::<FrameworkElement>()
            .expect("行は FrameworkElement");
        let size = || {
            (
                element.ActualWidth().unwrap_or(0.0),
                element.ActualHeight().unwrap_or(0.0),
            )
        };
        let content_size = || {
            (
                content.ActualWidth().unwrap_or(0.0),
                content.ActualHeight().unwrap_or(0.0),
            )
        };
        let near =
            |a: (f64, f64), b: (f64, f64)| (a.0 - b.0).abs() < 1.0 && (a.1 - b.1).abs() < 1.0;
        pump_until(SCROLL_LAYOUT_TIMEOUT, || {
            content_size().0 > 32.0 && content_size().1 > 32.0
        });
        if content_size().0 <= 32.0 || content_size().1 <= 32.0 {
            println!("     (実寸が付かない環境なので、表示後の配置は確かめずに通す)");
            return;
        }
        pump_until(SCROLL_LAYOUT_TIMEOUT, || near(size(), content_size()));
        assert!(
            near(size(), content_size()),
            "中身の領域 {:?} にスピナー {:?} が収まる",
            content_size(),
            size()
        );

        window.set_size(320.0, 140.0);
        pump_until(SCROLL_LAYOUT_TIMEOUT, || {
            near(size(), content_size()) && (content_size().0 - 320.0).abs() < 1.0
        });
        assert!(
            near(size(), content_size()),
            "窓のサイズ変更に追従: {:?} / {:?}",
            size(),
            content_size()
        );
        assert!(
            (content_size().0 - 320.0).abs() < 1.0,
            "窓の幅も指定どおり変わる: {:?}",
            content_size()
        );

        spinner.set_sizing(Sizing::fixed(16.0, 24.0));
        pump_until(SCROLL_LAYOUT_TIMEOUT, || near(size(), (16.0, 24.0)));
        assert!(
            near(size(), (16.0, 24.0)),
            "明示した固定サイズ: {:?}",
            size()
        );
        spinner.set_sizing(Sizing::new().width(naui_core::Length::Fixed(16.0)));
        pump_until(SCROLL_LAYOUT_TIMEOUT, || {
            near(size(), (16.0, content_size().1))
        });
        assert!(
            near(size(), (16.0, content_size().1)),
            "幅だけ固定: {:?}",
            size()
        );
        spinner.set_sizing(Sizing::new().height(naui_core::Length::Fixed(24.0)));
        pump_until(SCROLL_LAYOUT_TIMEOUT, || {
            near(size(), (content_size().0, 24.0))
        });
        assert!(
            near(size(), (content_size().0, 24.0)),
            "高さだけ固定: {:?}",
            size()
        );
        spinner.set_sizing(Sizing::new().max_width(16.0).max_height(20.0));
        pump_until(SCROLL_LAYOUT_TIMEOUT, || near(size(), (16.0, 20.0)));
        assert!(
            near(size(), (16.0, 20.0)),
            "上限のある Auto サイズ: {:?}",
            size()
        );
        spinner.set_sizing(Sizing::new());
        pump_until(SCROLL_LAYOUT_TIMEOUT, || near(size(), content_size()));
        assert!(
            near(size(), content_size()),
            "Auto を指定し直しても中身いっぱい: {:?}",
            size()
        );

        window.set_child(&spinner);
        pump_until(SCROLL_LAYOUT_TIMEOUT, || {
            element.ActualWidth().unwrap_or(0.0) > 32.0
        });
        assert!(element.Width().unwrap().is_nan(), "同じ子の再設定後も Fill");

        window.set_size(600.0, 200.0);
        let sidebar = ui.sidebar().expect("Sidebar");
        window.set_sidebar(&sidebar);
        window.set_child(&spinner);
        let content = window
            .native_window()
            .Content()
            .unwrap()
            .cast::<Grid>()
            .unwrap()
            .Children()
            .unwrap()
            .GetAt(2)
            .unwrap()
            .cast::<FrameworkElement>()
            .unwrap();
        let left = || {
            element
                .TransformToVisual(&content)
                .and_then(|transform| {
                    transform.TransformPoint(windows::Foundation::Point { X: 0.0, Y: 0.0 })
                })
                .map(|point| f64::from(point.X))
                .unwrap_or(f64::NAN)
        };
        pump_until(SCROLL_LAYOUT_TIMEOUT, || left() > 32.0 && size().0 > 32.0);
        assert!(left() > 32.0, "サイドバーの右側に置かれる: {}", left());
        assert!(
            (left() + size().0 - content.ActualWidth().unwrap()).abs() < 1.0,
            "サイドバーの右側の幅を埋める: 左 {}、子 {:?}",
            left(),
            size()
        );
        // NavigationView's border can take one logical pixel from the content pane.
        assert!(
            (size().1 - content.ActualHeight().unwrap()).abs() <= 1.0,
            "サイドバーの右側の高さを埋める: {:?}",
            size()
        );
        window.clear_sidebar();
        pump_until(SCROLL_LAYOUT_TIMEOUT, || {
            near(
                size(),
                (
                    content.ActualWidth().unwrap(),
                    content.ActualHeight().unwrap(),
                ),
            )
        });
        assert!(
            near(
                size(),
                (
                    content.ActualWidth().unwrap(),
                    content.ActualHeight().unwrap()
                )
            ),
            "サイドバーを外すと中身いっぱい: {:?}",
            size()
        );

        let replacement = ui.label("Replacement").expect("Label");
        window.set_child(&replacement);
        assert_eq!(element.Width().unwrap(), 32.0, "差し替えると既定幅に戻る");
        assert_eq!(element.Height().unwrap(), 32.0, "差し替えると既定高に戻る");
        assert_eq!(
            element.HorizontalAlignment().unwrap(),
            HorizontalAlignment::Left
        );
        assert_eq!(element.VerticalAlignment().unwrap(), VerticalAlignment::Top);
        let stack = ui.stack(Orientation::Vertical).expect("Stack");
        stack.set_align(Align::Fill);
        stack.append(&spinner);
        assert_eq!(stack.len(), 1, "差し替えたスピナーを別の親へ置ける");
        assert!(element.Width().unwrap().is_nan(), "新しい親の Fill に従う");
        stack.remove(0);
        window.set_child(&spinner);
        assert!(
            element.Width().unwrap().is_nan(),
            "再びウィンドウの幅に合わせる"
        );
    }));
    window.close();
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
    assert_eq!(element.Width().unwrap(), 32.0, "窓を閉じると既定幅に戻る");
    assert_eq!(element.Height().unwrap(), 32.0, "窓を閉じると既定高に戻る");
    Ok(())
}

fn failed_window_attach_keeps_spinner_parent(ui: &Ui) -> Result<()> {
    for with_sidebar in [false, true] {
        let window = ui.window("Failed window attachment", 420.0, 200.0)?;
        let sidebar = ui.sidebar()?;
        if with_sidebar {
            window.set_sidebar(&sidebar);
        }
        let original = ui.spinner()?;
        window.set_child(&original);
        let original_element = native::<FrameworkElement>(&original);

        let stack = ui.stack(Orientation::Vertical)?;
        stack.set_align(Align::End);
        let spinner = ui.spinner()?;
        stack.append(&spinner);
        let child = native::<FrameworkElement>(&spinner);
        window.set_child(&spinner);
        spinner.set_sizing(Sizing::new());
        assert_eq!(child.Width().unwrap(), 32.0, "別の親にいる子の幅を変えない");
        assert_eq!(
            child.HorizontalAlignment().unwrap(),
            HorizontalAlignment::Right,
            "別の親の配置を変えない"
        );
        assert!(
            original_element.Width().unwrap().is_nan(),
            "元のウィンドウの子を維持する"
        );
        let mounted = if with_sidebar {
            sidebar
                .native_navigation_view()
                .Content()
                .unwrap()
                .cast::<UIElement>()
                .unwrap()
        } else {
            window
                .native_window()
                .Content()
                .unwrap()
                .cast::<Grid>()
                .unwrap()
                .Children()
                .unwrap()
                .GetAt(2)
                .unwrap()
                .cast::<Grid>()
                .unwrap()
                .Children()
                .unwrap()
                .GetAt(0)
                .unwrap()
        };
        assert_eq!(
            mounted,
            original.native_element(),
            "元の子はウィンドウ内に残る"
        );
        window.close();
    }
    Ok(())
}

fn window_child_sizing_survives_replacement(ui: &Ui) -> Result<()> {
    let window = ui.window("Window content sizing", 420.0, 200.0)?;
    let label = ui.label("Window content")?;
    label.set_sizing(Sizing::new());
    let element = native::<FrameworkElement>(&label);
    window.set_child(&label);
    assert_eq!(
        element.HorizontalAlignment().unwrap(),
        HorizontalAlignment::Stretch
    );
    assert_eq!(
        element.VerticalAlignment().unwrap(),
        VerticalAlignment::Stretch
    );

    label.set_sizing(Sizing::fixed(50.0, 30.0));
    assert_eq!(
        element.HorizontalAlignment().unwrap(),
        HorizontalAlignment::Left
    );
    assert_eq!(element.VerticalAlignment().unwrap(), VerticalAlignment::Top);
    label.set_sizing(Sizing::new().max_width(100.0));
    assert_eq!(
        element.HorizontalAlignment().unwrap(),
        HorizontalAlignment::Left
    );
    assert_eq!(
        element.VerticalAlignment().unwrap(),
        VerticalAlignment::Stretch
    );
    label.set_sizing(Sizing::new());
    assert_eq!(
        element.HorizontalAlignment().unwrap(),
        HorizontalAlignment::Stretch
    );
    assert_eq!(
        element.VerticalAlignment().unwrap(),
        VerticalAlignment::Stretch
    );

    window.set_child(&ui.label("Replacement")?);
    assert_eq!(
        element.HorizontalAlignment().unwrap(),
        HorizontalAlignment::Left
    );
    assert_eq!(element.VerticalAlignment().unwrap(), VerticalAlignment::Top);
    window.set_child(&label);
    assert_eq!(
        element.HorizontalAlignment().unwrap(),
        HorizontalAlignment::Stretch
    );
    window.close();
    assert_eq!(
        element.HorizontalAlignment().unwrap(),
        HorizontalAlignment::Left
    );
    assert_eq!(element.VerticalAlignment().unwrap(), VerticalAlignment::Top);
    Ok(())
}

fn moved_window_child_keeps_new_parent(ui: &Ui) -> Result<()> {
    let first = ui.window("First window", 300.0, 200.0)?;
    let second = ui.window("Second window", 300.0, 200.0)?;
    let spinner = ui.spinner()?;
    first.set_child(&spinner);
    // The native escape hatch can detach a child without changing naui's old handle.
    let root = first
        .native_window()
        .Content()
        .unwrap()
        .cast::<Grid>()
        .unwrap();
    let content = root
        .Children()
        .unwrap()
        .GetAt(2)
        .unwrap()
        .cast::<Grid>()
        .unwrap();
    content.Children().unwrap().Clear().unwrap();
    second.set_child(&spinner);
    let element = native::<FrameworkElement>(&spinner);
    assert!(element.Width().unwrap().is_nan());
    first.close();
    assert!(
        element.Width().unwrap().is_nan(),
        "古い窓は新しい親の配置を解除しない"
    );
    second.close();
    assert_eq!(
        element.Width().unwrap(),
        32.0,
        "新しい窓を閉じたら既定幅に戻る"
    );
    Ok(())
}

fn sample_rich_text() -> RichText {
    RichText::new()
        .text("a ")
        .bold("b ")
        .colored("c ", TextColor::Danger)
        .link("d", "https://example.com/d")
}

/// The inlines of a label, in order.
fn inlines_of(label: &naui_windows::Label) -> Vec<Inline> {
    let inlines = native::<TextBlock>(label)
        .Inlines()
        .expect("TextBlock の Inlines");
    let count = inlines.Size().expect("Inlines の数");
    (0..count)
        .map(|index| inlines.GetAt(index).expect("Inline"))
        .collect()
}

/// `Run`s are built through `XamlReader`, so this also checks that WinUI accepts the
/// XAML of a bold and colored run (a `{ThemeResource}` that does not resolve fails
/// here).
fn rich_text_builds_inlines(ui: &Ui) -> Result<()> {
    let label = ui.label("")?;
    label.set_rich_text(&sample_rich_text());
    assert_eq!(label.text(), "a b c d");

    let inlines = inlines_of(&label);
    assert_eq!(inlines.len(), 4, "断片ごとに Inline ができること");
    for (index, expected) in ["a ", "b ", "c "].into_iter().enumerate() {
        let run: Run = inlines[index].cast().expect("Run");
        assert_eq!(run.Text().expect("Run の文字列").to_string(), expected);
    }
    let link: Hyperlink = inlines[3].cast().expect("リンクは Hyperlink");
    assert!(
        link.NavigateUri().is_err(),
        "開くかどうかは on_link の答えで決めるので、WinUI に任せない"
    );
    let inside = link.Inlines().expect("Hyperlink の Inlines");
    assert_eq!(inside.Size().expect("数"), 1);
    let run: Run = inside.GetAt(0).expect("Inline").cast().expect("Run");
    assert_eq!(run.Text().expect("Run の文字列").to_string(), "d");

    // 段階と色は `Style` で当たるので、中身に関わらず何度でも変えられる。
    label.set_style(TextStyle::Title);
    label.set_color(TextColor::Secondary);
    assert_eq!(inlines_of(&label).len(), 4, "段階と色で中身は変わらない");
    Ok(())
}

/// `render` sets the same content every time, so the inlines, which hold the selection
/// and the hover state, must stay the same objects.
fn rich_text_same_content_keeps_the_inlines(ui: &Ui) -> Result<()> {
    let label = ui.label("")?;
    label.set_rich_text(&sample_rich_text());
    let before = inlines_of(&label);

    // 作り方が違っても、見せるものが同じなら同じ内容。
    label.set_rich_text(
        &RichText::new()
            .text("a")
            .text(" ")
            .bold("b ")
            .colored("c ", TextColor::Danger)
            .link("d", "https://example.com/d"),
    );
    assert_eq!(
        before,
        inlines_of(&label),
        "同じオブジェクトのままであること"
    );

    label.set_rich_text(&RichText::new().bold("a b c d"));
    assert_ne!(before, inlines_of(&label), "内容が変われば書き換わる");
    assert_eq!(inlines_of(&label).len(), 1);
    Ok(())
}

fn rich_text_goes_back_to_plain_text(ui: &Ui) -> Result<()> {
    let label = ui.label("")?;
    let text_block = native::<TextBlock>(&label);

    // 文字が同じでも、飾りは外れる。
    label.set_rich_text(&RichText::new().bold("abc").link("d", "x:y"));
    label.set_text("abcd");
    assert_eq!(label.text(), "abcd");
    assert_eq!(text_block.Text().expect("文字列").to_string(), "abcd");
    let inlines = inlines_of(&label);
    assert!(
        inlines
            .iter()
            .all(|inline| inline.cast::<Hyperlink>().is_err()),
        "リンクが残らないこと"
    );

    // 飾りの無い RichText は set_text と同じ。
    label.set_rich_text(&RichText::new().bold("abcd"));
    label.set_rich_text(&RichText::new().text("plain"));
    assert_eq!(label.text(), "plain");
    assert_eq!(text_block.Text().expect("文字列").to_string(), "plain");
    Ok(())
}

fn label_can_be_selectable(ui: &Ui) -> Result<()> {
    let label = ui.label("コピーできる文字")?;
    assert!(!label.is_selectable(), "既定は選べない");
    label.set_selectable(true);
    let native = native::<TextBlock>(&label);
    assert!(native
        .IsTextSelectionEnabled()
        .expect("IsTextSelectionEnabled"));
    Ok(())
}

fn open_url_rejects_garbage(ui: &Ui) -> Result<()> {
    assert!(ui.open_url("").is_err());
    Ok(())
}

fn accessible_label_and_read_only(ui: &Ui) -> Result<()> {
    use naui_winui3::Microsoft::UI::Xaml::Automation::AutomationProperties;
    use naui_winui3::Microsoft::UI::Xaml::DependencyObject;
    let button = ui.button("⚙")?;
    button.set_accessible_label(Some("設定"));
    let object = button
        .native_element()
        .cast::<DependencyObject>()
        .expect("DependencyObject");
    assert_eq!(
        AutomationProperties::GetName(&object)
            .expect("Name")
            .to_string(),
        "設定"
    );
    let input = ui.text_input("ログ")?;
    input.set_read_only(true);
    assert!(native::<TextBox>(&input).IsReadOnly().expect("IsReadOnly"));
    Ok(())
}

/// On CI rows get no actual size, so this only checks that scrolling does not break
/// anything and the selection does not change (whether it scrolls to a visible
/// position is checked on real machines).
fn scroll_to_row_keeps_the_selection(ui: &Ui) -> Result<()> {
    use naui_core::{ListItem, TableColumn, TableRow, TreeItem};
    let labels: Vec<String> = (0..50).map(|i| format!("行 {i}")).collect();
    let list = ui.list()?;
    list.set_items(&ListItem::list(labels.iter().map(String::as_str)));
    list.scroll_to_row(40);
    list.scroll_to_row(1000);
    assert!(list.selected().is_none());

    let table = ui.table()?;
    table.set_columns(&[TableColumn::new("名前")]);
    table.set_rows(&TableRow::list(labels.iter().map(|l| [l.as_str()])));
    table.scroll_to_row(40);
    assert!(table.selected().is_none());

    let tree = ui.tree()?;
    tree.set_items(&[
        TreeItem::new("上").children(TreeItem::list(labels.iter().map(String::as_str)))
    ]);
    tree.scroll_to_item(&[0, 30]);
    assert!(tree.is_expanded(&[0]), "祖先を開く");
    assert!(tree.selected().is_none());
    Ok(())
}

/// `TextBox` stores newlines as `\r`, but `text()` returns `\n`, the same as the
/// other platforms.
fn text_area_reports_newlines_as_lf(ui: &Ui) -> Result<()> {
    let area = ui.text_area("一\n二")?;
    assert_eq!(area.text(), "一\n二");
    area.set_text("a\r\nb\rc");
    assert_eq!(area.text(), "a\nb\nc");

    // 改行の書き方だけが違う文字列は同じ文字列として扱い、書き直さない。
    // 書き直すとカーソルが先頭へ戻る。
    let text_box = native::<TextBox>(&area);
    text_box.SetSelectionStart(2).expect("カーソルの位置");
    area.set_text("a\r\nb\rc");
    assert_eq!(
        text_box.SelectionStart().expect("カーソルの位置"),
        2,
        "\\r\\n を含む同じ文字列の set_text はカーソルを動かさないこと"
    );
    Ok(())
}

/// Typing a string not among the suggestions and pressing Enter does not erase it,
/// and a string matching a suggestion selects that suggestion. Cancelling with Esc
/// returns to the last confirmed string (including ones written from code).
///
/// Confirming and cancelling with Enter and Esc cannot be triggered through UI
/// Automation, so real keys are sent with `SendInput`. Keys go to the foreground
/// window, so when the foreground is not this test's window (another app is in
/// front, or the template's input field is missing), it passes without sending.
fn editable_combo_box_commits_and_reverts_text(ui: &Ui) -> Result<()> {
    use naui_winui3::Microsoft::UI::Xaml::FocusState;
    use windows::Win32::UI::Input::KeyboardAndMouse::{VIRTUAL_KEY, VK_ESCAPE, VK_RETURN};

    let window = ui.window("編集できるコンボボックス", 240.0, 120.0)?;
    let combo = ui.editable_combo_box()?;
    combo.set_items(&["ab", "z"]);
    let seen = Rc::new(RefCell::new(Vec::<String>::new()));
    combo.on_change({
        let seen = seen.clone();
        move |text: &str| seen.borrow_mut().push(text.to_string())
    });
    window.set_child(&combo);
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        window.show();
        let native = native::<XamlComboBox>(&combo);
        let part = || {
            native
                .GetTemplateChild(&HSTRING::from("EditableText"))
                .and_then(|part| part.cast::<TextBox>())
        };
        pump_until(KEY_INPUT_TIMEOUT, || part().is_ok());
        let Ok(text_box) = part() else {
            println!("     (テンプレートの入力欄が無い環境なので、確かめずに通す)");
            return;
        };
        // 前のケースのウィンドウが閉じると、前面はほかのアプリへ移っている。
        if bring_to_front(&window).is_none() {
            return;
        }
        if !text_box.Focus(FocusState::Programmatic).unwrap_or(false) {
            println!("     (入力欄へフォーカスを移せない環境なので、確かめずに通す)");
            return;
        }
        // 1 文字目の `TextChanged` は遅れて届くことがあるので、文字列と通知の
        // 両方がそろうまで待つ。
        let settle = |text: &str| {
            pump_until(KEY_INPUT_TIMEOUT, || {
                combo.text() == text && seen.borrow().last().map(String::as_str) == Some(text)
            });
        };
        let key = |c: u8| VIRTUAL_KEY(u16::from(c));

        // 候補に無い文字列。WinUI が `TextSubmitted` を出す。
        if !type_keys(&[key(b'Q')]) {
            return;
        }
        settle("q");
        assert_eq!(combo.text(), "q", "打った文字が入る");
        if !type_keys(&[VK_RETURN]) {
            return;
        }
        assert_eq!(combo.text(), "q", "Enter の後も残る");
        assert_eq!(combo.selected(), None);
        assert_eq!(*seen.borrow(), ["q"], "確定で余計な通知が出ない");

        // プログラムから書いた文字列が、Esc の戻り先になる。
        for written in ["foo", ""] {
            combo.set_text(written);
            let _ = text_box.SelectAll();
            if !type_keys(&[key(b'W')]) {
                return;
            }
            settle("w");
            if !type_keys(&[VK_ESCAPE]) {
                return;
            }
            settle(written);
            assert_eq!(combo.text(), written, "Esc で set_text の値へ戻る");
        }
        assert_eq!(*seen.borrow(), ["q", "w", "foo", "w", ""]);

        // 候補と同じ文字列。打ち直すと、その候補が選ばれる。
        let _ = text_box.SelectAll();
        if !type_keys(&[key(b'Z')]) {
            return;
        }
        settle("z");
        if !type_keys(&[VK_RETURN]) {
            return;
        }
        assert_eq!(combo.text(), "z");
        assert_eq!(combo.selected(), Some(1), "一致する候補が選ばれる");
        assert_eq!(
            native.SelectedIndex().unwrap_or(-1),
            1,
            "ネイティブの選択も候補を指す"
        );
        assert_eq!(*seen.borrow(), ["q", "w", "foo", "w", "", "z"]);
    }));
    window.close();
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
    Ok(())
}

/// The maximum time to wait for key input to take effect.
const KEY_INPUT_TIMEOUT: Duration = Duration::from_secs(3);

/// Presses and releases real keys in order, and pumps messages until they arrive.
///
/// If the foreground window does not belong to this process, it sends nothing and
/// returns `false` (so as not to type into another app). The case then passes without
/// checking.
fn type_keys(keys: &[windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY]) -> bool {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP,
    };
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

    let mut owner = 0;
    // SAFETY: `owner` はこの関数の中で生きている。
    unsafe { GetWindowThreadProcessId(GetForegroundWindow(), Some(&mut owner)) };
    if owner != std::process::id() {
        println!("     (テストのウィンドウが前面に無いので、キーを送らずに通す)");
        return false;
    }
    let key = |vk, flags| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    let inputs: Vec<INPUT> = keys
        .iter()
        .flat_map(|&vk| [key(vk, KEYBD_EVENT_FLAGS(0)), key(vk, KEYEVENTF_KEYUP)])
        .collect();
    // SAFETY: `inputs` は送り終わるまで生きている。
    let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
    if sent as usize != inputs.len() {
        println!("     (キーを送れない環境なので、確かめずに通す)");
        return false;
    }
    pump_until(Duration::from_millis(300), || false);
    true
}

/// Whether the foreground window belongs to this process, so real input stays in it.
fn foreground_is_ours() -> bool {
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

    let mut owner = 0;
    // SAFETY: `owner` はこの関数の中で生きている。
    unsafe { GetWindowThreadProcessId(GetForegroundWindow(), Some(&mut owner)) };
    owner == std::process::id()
}

/// Brings the window to the front for real input. `None` if it does not get there, and
/// the case then passes without checking.
fn bring_to_front(window: &naui_windows::Window) -> Option<windows::Win32::Foundation::HWND> {
    use windows::Win32::UI::WindowsAndMessaging::{
        SetForegroundWindow, SetWindowPos, HWND_TOPMOST, SWP_NOMOVE, SWP_NOSIZE,
    };

    let native = window
        .native_window()
        .cast::<naui_winui3::IWindowNative>()
        .expect("IWindowNative");
    // SAFETY: HWND は window が閉じるまで有効で、UI スレッドから読んでいる。
    let hwnd = unsafe { native.WindowHandle() }.expect("HWND");
    // SAFETY: 上の HWND を渡すだけ。前面に出せなければ下で分かる。
    let _ = unsafe { SetForegroundWindow(hwnd) };
    // ほかのウィンドウが重なると、押す位置が他へ渡ってしまう。
    // SAFETY: 上の HWND を渡すだけ。ウィンドウごとテストの最後に閉じる。
    let _ = unsafe {
        SetWindowPos(
            hwnd,
            Some(HWND_TOPMOST),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE,
        )
    };
    pump_until(Duration::from_millis(300), || false);
    if !foreground_is_ours() {
        println!("     (テストのウィンドウを前面に出せないので、入力を送らずに通す)");
        return None;
    }
    Some(hwnd)
}

/// Sends one input event and lets it arrive. Sent one at a time with messages pumped in
/// between, like a person's, because `ListView` reads the modifier keys as it handles them.
fn send_input(input: windows::Win32::UI::Input::KeyboardAndMouse::INPUT) {
    use windows::Win32::UI::Input::KeyboardAndMouse::{SendInput, INPUT};

    // SAFETY: `input` は送り終わるまで生きている。
    unsafe { SendInput(&[input], std::mem::size_of::<INPUT>() as i32) };
    pump_until(Duration::from_millis(60), || false);
}

/// A key press or release, for [`send_input`].
fn key_input(
    key: windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY,
    flags: windows::Win32::UI::Input::KeyboardAndMouse::KEYBD_EVENT_FLAGS,
) -> windows::Win32::UI::Input::KeyboardAndMouse::INPUT {
    use windows::Win32::UI::Input::KeyboardAndMouse::{INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT};

    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: key,
                wScan: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

/// A mouse event at the pointer, for [`send_input`].
fn mouse_input(
    flags: windows::Win32::UI::Input::KeyboardAndMouse::MOUSE_EVENT_FLAGS,
    data: u32,
) -> windows::Win32::UI::Input::KeyboardAndMouse::INPUT {
    use windows::Win32::UI::Input::KeyboardAndMouse::{INPUT, INPUT_0, INPUT_MOUSE, MOUSEINPUT};

    INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx: 0,
                dy: 0,
                mouseData: data,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

/// Runs `act` with the modifier keys held.
fn with_modifiers(
    modifiers: &[windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY],
    act: impl FnOnce(),
) {
    use windows::Win32::UI::Input::KeyboardAndMouse::{KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP};

    for &modifier in modifiers {
        send_input(key_input(modifier, KEYBD_EVENT_FLAGS(0)));
    }
    act();
    pump_until(Duration::from_millis(150), || false);
    for &modifier in modifiers {
        send_input(key_input(modifier, KEYEVENTF_KEYUP));
    }
    pump_until(Duration::from_millis(100), || false);
}

/// Presses a navigation key with real input. `None`, without sending anything, once the
/// foreground is not this process.
///
/// The key is marked extended, or Windows takes it for the numeric keypad's and lifts
/// Shift around it.
fn press_nav_key(
    modifiers: &[windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY],
    key: windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY,
) -> Option<()> {
    use windows::Win32::UI::Input::KeyboardAndMouse::{KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP};

    if !foreground_is_ours() {
        println!("     (テストのウィンドウが前面に無いので、キーを送らずに通す)");
        return None;
    }
    with_modifiers(modifiers, || {
        send_input(key_input(key, KEYEVENTF_EXTENDEDKEY));
        send_input(key_input(key, KEYEVENTF_KEYUP | KEYEVENTF_EXTENDEDKEY));
    });
    Some(())
}

/// Presses Ctrl + A with real input. `None`, without sending anything, once the foreground
/// is not this process.
fn press_ctrl_a() -> Option<()> {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, VIRTUAL_KEY, VK_CONTROL,
    };

    if !foreground_is_ours() {
        println!("     (テストのウィンドウが前面に無いので、キーを送らずに通す)");
        return None;
    }
    let key_a = VIRTUAL_KEY(u16::from(b'A'));
    with_modifiers(&[VK_CONTROL], || {
        send_input(key_input(key_a, KEYBD_EVENT_FLAGS(0)));
        send_input(key_input(key_a, KEYEVENTF_KEYUP));
    });
    Some(())
}

/// What the real-input cases use, which a `List` and a `Table` both have: they are built
/// the same way, so the same scenario has to hold on both.
trait RowsForInput {
    fn list_view(&self) -> ListView;
    fn scroller(&self) -> ScrollViewer;
    fn row_height(&self) -> f64;
    fn selection(&self) -> Vec<usize>;
    fn set_selection(&self, indices: &[usize]);
    fn scroll_to_row(&self, index: usize);
    fn on_select(&self, f: impl FnMut(&[usize]) + 'static);
    fn refresh(&self);
}

macro_rules! rows_for_input {
    ($widget:ty, $scroller:path) => {
        impl RowsForInput for $widget {
            fn list_view(&self) -> ListView {
                self.native_list_view()
            }
            fn scroller(&self) -> ScrollViewer {
                $scroller(self)
            }
            fn row_height(&self) -> f64 {
                self.row_height()
            }
            fn selection(&self) -> Vec<usize> {
                self.selection()
            }
            fn set_selection(&self, indices: &[usize]) {
                self.set_selection(indices)
            }
            fn scroll_to_row(&self, index: usize) {
                self.scroll_to_row(index)
            }
            fn on_select(&self, f: impl FnMut(&[usize]) + 'static) {
                self.on_select(f)
            }
            fn refresh(&self) {
                self.refresh()
            }
        }
    };
}
rows_for_input!(naui_windows::List, list_scroller);
rows_for_input!(naui_windows::Table, table_scroller);

/// The built `ListViewItem` of a row of the whole list.
fn built_list_row(list: &impl RowsForInput, index: usize) -> Option<ListViewItem> {
    use naui_winui3::Microsoft::UI::Xaml::Automation::AutomationProperties;

    let items = list.list_view().Items().ok()?;
    (0..items.Size().ok()?).find_map(|slot| {
        let row = items.GetAt(slot).ok()?.cast::<ListViewItem>().ok()?;
        (AutomationProperties::GetPositionInSet(&row).ok()? == index as i32 + 1).then_some(row)
    })
}

/// Which row of the whole list has the keyboard focus.
fn focused_list_row(list: &impl RowsForInput) -> Option<usize> {
    use naui_winui3::Microsoft::UI::Xaml::Automation::AutomationProperties;
    use naui_winui3::Microsoft::UI::Xaml::Controls::Control;
    use naui_winui3::Microsoft::UI::Xaml::FocusState;

    let items = list.list_view().Items().ok()?;
    (0..items.Size().ok()?).find_map(|slot| {
        let row = items.GetAt(slot).ok()?.cast::<ListViewItem>().ok()?;
        let state = row.cast::<Control>().ok()?.FocusState().ok()?;
        let position = AutomationProperties::GetPositionInSet(&row).ok()?;
        (state != FocusState::Unfocused).then_some(position as usize - 1)
    })
}

/// Clicks a row on screen with real input. `None`, without clicking, if the point is not
/// in this process's window. A row out of view fails the case: its point could be anywhere
/// on the screen.
fn click_list_row(
    hwnd: windows::Win32::Foundation::HWND,
    list: &impl RowsForInput,
    index: usize,
    modifiers: &[windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY],
) -> Option<()> {
    use windows::Win32::UI::Input::KeyboardAndMouse::{MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP};

    aim_at_list_row(hwnd, list, index)?;
    with_modifiers(modifiers, || {
        send_input(mouse_input(MOUSEEVENTF_LEFTDOWN, 0));
        send_input(mouse_input(MOUSEEVENTF_LEFTUP, 0));
    });
    Some(())
}

/// Puts the pointer over a row on screen. `None`, without moving it, if the point is not in
/// this process's window. A row out of view fails the case: its point could be anywhere on
/// the screen.
fn aim_at_list_row(
    hwnd: windows::Win32::Foundation::HWND,
    list: &impl RowsForInput,
    index: usize,
) -> Option<()> {
    aim_at_row_point(hwnd, list, index, 40.0)
}

/// [`aim_at_list_row`], at a distance from the row's left edge.
fn aim_at_row_point(
    hwnd: windows::Win32::Foundation::HWND,
    list: &impl RowsForInput,
    index: usize,
    x: f32,
) -> Option<()> {
    use windows::Foundation::Point as XamlPoint;
    use windows::Win32::Foundation::POINT;
    use windows::Win32::Graphics::Gdi::ClientToScreen;
    use windows::Win32::UI::HiDpi::GetDpiForWindow;
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowThreadProcessId, SetCursorPos, WindowFromPoint,
    };

    let row = built_list_row(list, index).unwrap_or_else(|| {
        let first = list.list_view().Items().ok().and_then(|items| items.GetAt(0).ok());
        let first = first.and_then(|item| item.cast::<ListViewItem>().ok()).map(|row| naui_winui3::Microsoft::UI::Xaml::Automation::AutomationProperties::GetPositionInSet(&row));
        panic!("{index} 行目が組み立てられていない: 先頭 {first:?} 位置 {:?}", list.scroller().VerticalOffset())
    });
    let scroll = list.scroller();
    let middle = (row.ActualHeight().expect("行の高さ") / 2.0) as f32;
    let row_at = || {
        row.TransformToVisual(None)
            .and_then(|to| to.TransformPoint(XamlPoint { X: x, Y: middle }))
            .expect("行の位置")
    };
    let view = scroll
        .TransformToVisual(None)
        .and_then(|to| to.TransformPoint(XamlPoint { X: 0.0, Y: 0.0 }))
        .expect("表示領域の位置");
    let viewport = scroll.ViewportHeight().expect("表示領域の高さ") as f32;
    // 直前のケースでキーが行を動かしていると、行の位置はスクロールに遅れて付いてくる。
    pump_until(SCROLL_LAYOUT_TIMEOUT, || {
        let at = row_at();
        at.Y > view.Y && at.Y < view.Y + viewport
    });
    let at = row_at();
    assert!(
        at.Y > view.Y && at.Y < view.Y + viewport,
        "見えている行だけを押す: {index} 行目は {} (表示領域 {}〜{})",
        at.Y,
        view.Y,
        view.Y + viewport
    );
    // SAFETY: HWND は window が閉じるまで有効で、UI スレッドから読んでいる。
    let scale = unsafe { GetDpiForWindow(hwnd) } as f32 / 96.0;
    let mut point = POINT {
        x: (at.X * scale) as i32,
        y: (at.Y * scale) as i32,
    };
    // SAFETY: `point` はこの関数の中で生きている。
    let _ = unsafe { ClientToScreen(hwnd, &mut point) };
    let mut owner = 0;
    // SAFETY: `owner` はこの関数の中で生きている。
    unsafe { GetWindowThreadProcessId(WindowFromPoint(point), Some(&mut owner)) };
    if !foreground_is_ours() || owner != std::process::id() {
        println!("     (押す位置がテストのウィンドウの上に無いので、押さずに通す)");
        return None;
    }
    // SAFETY: 引数は値だけ。
    let _ = unsafe { SetCursorPos(point.x, point.y) };
    pump_until(Duration::from_millis(150), || false);
    Some(())
}

/// Clicks a row to start a case. The first click after coming to the front can be lost, so
/// it is tried again, and if none arrives the case passes without checking.
fn first_click_on_list_row(
    hwnd: windows::Win32::Foundation::HWND,
    list: &impl RowsForInput,
    index: usize,
) -> Option<()> {
    for _ in 0..3 {
        click_list_row(hwnd, list, index, &[])?;
        if list.selection() == [index] {
            return Some(());
        }
        pump_until(Duration::from_millis(300), || false);
    }
    println!("     (クリックが届かない環境なので、確かめずに通す)");
    None
}

/// Clicks a row and waits for the selection to become `expected`. A sent click can be slow
/// to arrive or get lost, so it is tried again before the case fails.
fn click_list_row_expecting(
    hwnd: windows::Win32::Foundation::HWND,
    list: &impl RowsForInput,
    index: usize,
    modifiers: &[windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY],
    expected: &[usize],
    what: &str,
) -> Option<()> {
    let scroll = list.scroller();
    let before = scroll.VerticalOffset().expect("位置");
    for attempt in 0..3 {
        if attempt > 0 {
            println!("     ({what}: クリックが反映されないので、もう一度押す)");
        }
        click_list_row(hwnd, list, index, modifiers)?;
        pump_until(Duration::from_secs(1), || list.selection() == expected);
        // フォーカスの自動スクロールで飛ぶと、押下と解放の間ならクリックごと失われる。
        let now = scroll.VerticalOffset().expect("位置");
        assert!(
            (now - before).abs() < 1.0,
            "{what}: クリックで表示が動かないこと: {before} → {now}"
        );
        if list.selection() == expected {
            return Some(());
        }
    }
    assert_eq!(list.selection(), expected, "{what}");
    Some(())
}

/// Shows a list of many rows for real input, runs `scenario`, and puts the pointer back.
fn with_shown_list_for_input(
    ui: &Ui,
    rows: usize,
    scenario: impl FnOnce(windows::Win32::Foundation::HWND, &naui_windows::List) -> Option<()>,
) -> Result<()> {
    let window = ui.window("リストへの実際の入力", 300.0, 240.0)?;
    let list = ui.list()?;
    list.set_selection_mode(naui_core::SelectionMode::Multiple);
    list.set_items(&naui_core::ListItem::list(
        (0..rows).map(|index| format!("行 {index}")),
    ));
    list.set_sizing(Sizing::fill());
    window.set_child(&list);
    run_shown_for_input(&window, &list, scenario)
}

/// Shows a table of many rows for real input, runs `scenario`, and puts the pointer back.
fn with_shown_table_for_input(
    ui: &Ui,
    rows: usize,
    scenario: impl FnOnce(windows::Win32::Foundation::HWND, &naui_windows::Table) -> Option<()>,
) -> Result<()> {
    let window = ui.window("表への実際の入力", 300.0, 240.0)?;
    let table = ui.table()?;
    table.set_selection_mode(naui_core::SelectionMode::Multiple);
    table.set_columns(&[
        naui_core::TableColumn::new("名前"),
        naui_core::TableColumn::new("値"),
    ]);
    table.set_row_builder(rows, |index| {
        Ok(naui_windows::TableCells::new()
            .text(format!("行 {index}"))
            .text(format!("{index}")))
    });
    table.set_sizing(Sizing::fill());
    window.set_child(&table);
    run_shown_for_input(&window, &table, scenario)
}

/// Shows the window, brings it to the front, runs `scenario`, and puts the pointer back.
fn run_shown_for_input<R: RowsForInput>(
    window: &naui_windows::Window,
    rows: &R,
    scenario: impl FnOnce(windows::Win32::Foundation::HWND, &R) -> Option<()>,
) -> Result<()> {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::UI::WindowsAndMessaging::{GetCursorPos, SetCursorPos};

    let mut pointer = POINT::default();
    // SAFETY: `pointer` はこの関数の中で生きている。
    let _ = unsafe { GetCursorPos(&mut pointer) };
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        window.show();
        let scroll = rows.scroller();
        pump_until(SCROLL_LAYOUT_TIMEOUT, || {
            scroll.ViewportHeight().unwrap_or(0.0) > 0.0 && rows.row_height() > 0.0
        });
        if scroll.ViewportHeight().unwrap_or(0.0) <= 0.0 {
            println!("     (ScrollViewer に実寸が付かない環境なので、確かめずに通す)");
            return;
        }
        if let Some(hwnd) = bring_to_front(window) {
            pump_until(Duration::from_millis(500), || false);
            let _ = scenario(hwnd, rows);
        }
    }));
    // SAFETY: 引数は値だけ。
    let _ = unsafe { SetCursorPos(pointer.x, pointer.y) };
    window.close();
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
    Ok(())
}

/// Page Up and Page Down move by what is on screen, whether or not every row is built:
/// `ListView` scrolls nothing itself, so it would take all the rows it has as the page.
fn list_page_keys_move_by_a_page(ui: &Ui) -> Result<()> {
    for rows in [1_000, 100] {
        with_shown_list_for_input(ui, rows, |hwnd, list| page_keys_scenario(hwnd, list, rows))?;
    }
    Ok(())
}

/// [`list_page_keys_move_by_a_page`] on a `Table`, built the same way.
fn table_page_keys_move_by_a_page(ui: &Ui) -> Result<()> {
    for rows in [1_000, 100] {
        with_shown_table_for_input(ui, rows, |hwnd, table| {
            page_keys_scenario(hwnd, table, rows)
        })?;
    }
    Ok(())
}

fn page_keys_scenario(
    hwnd: windows::Win32::Foundation::HWND,
    list: &impl RowsForInput,
    rows: usize,
) -> Option<()> {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        VK_CONTROL, VK_DOWN, VK_END, VK_HOME, VK_NEXT, VK_PRIOR, VK_SHIFT,
    };

    let scroll = list.scroller();
    let seen: Rc<RefCell<Vec<Vec<usize>>>> = Rc::default();
    {
        let seen = seen.clone();
        list.on_select(move |picked: &[usize]| seen.borrow_mut().push(picked.to_vec()));
    }
    let height = list.row_height();
    let page = (scroll.ViewportHeight().expect("高さ") / height).floor() as usize;
    let on_screen = |index: usize| {
        let top = scroll.VerticalOffset().expect("位置");
        let bottom = top + scroll.ViewportHeight().expect("高さ");
        index as f64 * height >= top - 1.0 && (index + 1) as f64 * height <= bottom + 1.0
    };

    first_click_on_list_row(hwnd, list, 1)?;

    // 組み立てた行の端を越えて、1 ページずつ進む。
    let mut at = 1;
    for _ in 0..8 {
        press_nav_key(&[], VK_NEXT)?;
        at += page;
        assert_eq!(list.selection(), [at], "{rows} 行: Page Down の選択");
        assert_eq!(focused_list_row(list), Some(at), "{rows} 行: フォーカス");
        assert!(on_screen(at), "{rows} 行: {at} 行目が見えていること");
        assert_eq!(seen.borrow().last().expect("通知"), &[at], "通知");
    }
    for _ in 0..3 {
        press_nav_key(&[], VK_PRIOR)?;
        at -= page;
        assert_eq!(list.selection(), [at], "{rows} 行: Page Up の選択");
        assert_eq!(focused_list_row(list), Some(at), "{rows} 行: フォーカス");
        assert!(on_screen(at), "{rows} 行: {at} 行目が見えていること");
    }

    // 矢印キーは `ListView` のまま、移った行から続く。
    press_nav_key(&[], VK_DOWN)?;
    at += 1;
    assert_eq!(list.selection(), [at], "{rows} 行: 移った行から続く");

    // Shift は移った行までの範囲、Ctrl はフォーカスだけ。
    press_nav_key(&[VK_SHIFT], VK_NEXT)?;
    let range: Vec<usize> = (at..=at + page).collect();
    assert_eq!(list.selection(), range, "{rows} 行: Shift + Page Down");
    assert_eq!(focused_list_row(list), Some(at + page), "{rows} 行");
    press_nav_key(&[VK_CONTROL], VK_NEXT)?;
    assert_eq!(
        list.selection(),
        range,
        "{rows} 行: Ctrl で選択は変わらない"
    );
    assert_eq!(focused_list_row(list), Some(at + page * 2), "{rows} 行");

    // 端では止まり、同じ選択を通知し直さない。
    list.scroll_to_row(rows - 1);
    pump_until(Duration::from_millis(500), || false);
    let what = format!("{rows} 行: 最後の手前の行");
    click_list_row_expecting(hwnd, list, rows - 2, &[], &[rows - 2], &what)?;
    press_nav_key(&[], VK_NEXT)?;
    assert_eq!(list.selection(), [rows - 1], "{rows} 行: 最後の行で止まる");
    let notified = seen.borrow().len();
    press_nav_key(&[], VK_NEXT)?;
    assert_eq!(list.selection(), [rows - 1]);
    assert_eq!(
        seen.borrow().len(),
        notified,
        "{rows} 行: 変わらなければ黙る"
    );

    list.scroll_to_row(0);
    pump_until(Duration::from_millis(500), || false);
    let what = format!("{rows} 行: 先頭の次の行");
    click_list_row_expecting(hwnd, list, 1, &[], &[1], &what)?;
    press_nav_key(&[], VK_PRIOR)?;
    assert_eq!(list.selection(), [0], "{rows} 行: 最初の行で止まる");

    // Home / End は全行の端へ動き、移った行を見せる。
    press_nav_key(&[], VK_END)?;
    assert_eq!(list.selection(), [rows - 1], "{rows} 行: End の選択");
    assert_eq!(focused_list_row(list), Some(rows - 1), "{rows} 行: End");
    assert!(on_screen(rows - 1), "{rows} 行: End の行が見えていること");
    press_nav_key(&[], VK_HOME)?;
    assert_eq!(list.selection(), [0], "{rows} 行: Home の選択");
    assert_eq!(focused_list_row(list), Some(0), "{rows} 行: Home");
    assert!(on_screen(0), "{rows} 行: Home の行が見えていること");

    // Ctrl + A は組み立てていない行も選ぶ。
    press_ctrl_a()?;
    let every: Vec<usize> = (0..rows).collect();
    assert_eq!(list.selection(), every, "{rows} 行: Ctrl + A");
    assert_eq!(seen.borrow().last().expect("通知"), &every, "通知");
    assert_eq!(
        focused_list_row(list),
        Some(0),
        "{rows} 行: フォーカスは動かない"
    );
    assert!(on_screen(0), "{rows} 行: 表示は動かない");
    Some(())
}

/// A Shift range starts from the row the last selection ended on, even after that row has
/// left the range built: `ListView` remembers it only as a place among the rows it has.
fn list_shift_range_keeps_its_anchor_outside_the_window(ui: &Ui) -> Result<()> {
    with_shown_list_for_input(ui, SHIFT_RANGE_ROWS, |hwnd, list| {
        shift_range_scenario(hwnd, list)
    })
}

/// [`list_shift_range_keeps_its_anchor_outside_the_window`] on a `Table`.
fn table_shift_range_keeps_its_anchor_outside_the_window(ui: &Ui) -> Result<()> {
    with_shown_table_for_input(ui, SHIFT_RANGE_ROWS, |hwnd, table| {
        shift_range_scenario(hwnd, table)
    })
}

/// How many rows [`shift_range_scenario`] runs over.
const SHIFT_RANGE_ROWS: usize = 1_000;

fn shift_range_scenario(
    hwnd: windows::Win32::Foundation::HWND,
    list: &impl RowsForInput,
) -> Option<()> {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_WHEEL, VK_CONTROL, VK_DOWN, VK_END,
        VK_HOME, VK_NEXT, VK_PRIOR, VK_SHIFT, VK_SPACE, VK_UP,
    };
    use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

    const ROWS: usize = SHIFT_RANGE_ROWS;
    let span = |from: usize, to: usize| (from..=to).collect::<Vec<usize>>();
    {
        let scroll = list.scroller();
        let seen: Rc<RefCell<Vec<Vec<usize>>>> = Rc::default();
        {
            let seen = seen.clone();
            list.on_select(move |picked: &[usize]| seen.borrow_mut().push(picked.to_vec()));
        }
        let go_to = |index: usize| {
            list.scroll_to_row(index);
            pump_until(Duration::from_millis(500), || false);
        };

        // 起点が窓の中にある間も、同じ結果になる。
        first_click_on_list_row(hwnd, list, 2)?;
        click_list_row_expecting(hwnd, list, 4, &[VK_SHIFT], &span(2, 4), "窓の中の範囲")?;

        // 起点の行が窓から外れたあと。
        go_to(ROWS - 1);
        assert!(built_list_row(list, 2).is_none(), "起点の行が窓の外にある");
        click_list_row_expecting(
            hwnd,
            list,
            ROWS - 2,
            &[VK_SHIFT],
            &span(2, ROWS - 2),
            "窓の外の起点からの範囲",
        )?;
        assert_eq!(
            seen.borrow().last().expect("通知"),
            &span(2, ROWS - 2),
            "利用者へ渡す選択も同じ"
        );
        assert_eq!(focused_list_row(list), Some(ROWS - 2), "押した行へ移る");

        // 起点は動かない。
        click_list_row_expecting(
            hwnd,
            list,
            ROWS - 4,
            &[VK_SHIFT],
            &span(2, ROWS - 4),
            "縮める",
        )?;

        // キーで動かしても同じ起点から伸びる。
        press_nav_key(&[VK_SHIFT], VK_UP)?;
        assert_eq!(list.selection(), span(2, ROWS - 5), "Shift + ↑");
        press_nav_key(&[VK_SHIFT], VK_DOWN)?;
        assert_eq!(list.selection(), span(2, ROWS - 4), "Shift + ↓");
        press_nav_key(&[VK_SHIFT], VK_PRIOR)?;
        let page = (scroll.ViewportHeight().expect("高さ") / list.row_height()).floor() as usize;
        assert_eq!(
            list.selection(),
            span(2, ROWS - 4 - page),
            "Shift + Page Up"
        );

        // Shift + Home / End は、組み立てていない行を越えて全行の端まで。
        press_nav_key(&[VK_SHIFT], VK_END)?;
        assert_eq!(list.selection(), span(2, ROWS - 1), "Shift + End");
        assert_eq!(
            focused_list_row(list),
            Some(ROWS - 1),
            "Shift + End のフォーカス"
        );
        press_nav_key(&[VK_SHIFT], VK_HOME)?;
        assert_eq!(list.selection(), span(0, 2), "Shift + Home");
        assert_eq!(focused_list_row(list), Some(0), "Shift + Home のフォーカス");

        // 手前へ戻って選び直すと、起点が変わる。上へ伸ばしても順序は昇順。
        go_to(ROWS - 1);
        click_list_row_expecting(hwnd, list, ROWS - 3, &[], &[ROWS - 3], "選び直し")?;
        go_to(5);
        click_list_row_expecting(hwnd, list, 5, &[VK_SHIFT], &span(5, ROWS - 3), "上へ伸ばす")?;

        // Ctrl で足し引きした行が次の起点になる。
        let without_seven: Vec<usize> = span(5, ROWS - 3)
            .into_iter()
            .filter(|&row| row != 7)
            .collect();
        click_list_row_expecting(hwnd, list, 7, &[VK_CONTROL], &without_seven, "Ctrl で引く")?;
        click_list_row_expecting(
            hwnd,
            list,
            9,
            &[VK_SHIFT],
            &span(7, 9),
            "Ctrl で触った行が起点",
        )?;

        // コードから選んだ行も起点になる。
        list.set_selection(&[ROWS - 10]);
        click_list_row_expecting(
            hwnd,
            list,
            8,
            &[VK_SHIFT],
            &span(8, ROWS - 10),
            "set_selection の行が起点",
        )?;

        // ホイールのあと、ポインターを動かさずに押した行も分かる。
        click_list_row_expecting(hwnd, list, 7, &[], &[7], "ホイールの前の選択")?;
        let pointer_at = || {
            let mut at = POINT::default();
            // SAFETY: `at` はこの関数の中で生きている。
            let _ = unsafe { GetCursorPos(&mut at) };
            (at.x, at.y)
        };
        let aimed = pointer_at();
        let start = scroll.VerticalOffset().expect("位置");
        for _ in 0..60 {
            if !foreground_is_ours() {
                return None;
            }
            if built_list_row(list, 7).is_none() {
                break;
            }
            send_input(mouse_input(MOUSEEVENTF_WHEEL, (-480_i32) as u32));
        }
        pump_until(Duration::from_millis(500), || false);
        // ホイールもこの先のクリックも、ポインターの下へ届く。
        if pointer_at() != aimed {
            println!("     (ポインターがほかから動かされたので、確かめずに通す)");
            return None;
        }
        assert!(
            built_list_row(list, 7).is_none(),
            "ホイールで窓が動いている: 位置 {start} → {} ポインター {aimed:?}",
            scroll.VerticalOffset().expect("位置")
        );
        // ホイールのスクロールは少しのあいだ続くので、止まってから押す行を決める。
        let mut settled = scroll.VerticalOffset().expect("位置");
        pump_until(Duration::from_secs(3), || {
            pump_until(Duration::from_millis(300), || false);
            let now = scroll.VerticalOffset().expect("位置");
            let still = (now - settled).abs() < 0.5;
            settled = now;
            still
        });
        let mut pointer = POINT::default();
        // SAFETY: `pointer` はこの関数の中で生きている。
        let _ = unsafe { GetCursorPos(&mut pointer) };
        let under_pointer = |row: &ListViewItem| {
            use windows::Foundation::Point as XamlPoint;
            use windows::Win32::Graphics::Gdi::ClientToScreen;
            use windows::Win32::UI::HiDpi::GetDpiForWindow;
            let top = row
                .TransformToVisual(None)
                .and_then(|to| to.TransformPoint(XamlPoint { X: 0.0, Y: 0.0 }))
                .expect("行の位置");
            // SAFETY: HWND は window が閉じるまで有効で、UI スレッドから読んでいる。
            let scale = unsafe { GetDpiForWindow(hwnd) } as f32 / 96.0;
            let mut origin = POINT::default();
            // SAFETY: `origin` はこの関数の中で生きている。
            let _ = unsafe { ClientToScreen(hwnd, &mut origin) };
            let y = (pointer.y - origin.y) as f32 / scale;
            y >= top.Y && y < top.Y + row.ActualHeight().unwrap_or(0.0) as f32
        };
        let target = (0..ROWS)
            .find(|&index| built_list_row(list, index).is_some_and(|row| under_pointer(&row)))
            .expect("ポインターの下の行");
        for _ in 0..3 {
            with_modifiers(&[VK_SHIFT], || {
                send_input(mouse_input(MOUSEEVENTF_LEFTDOWN, 0));
                send_input(mouse_input(MOUSEEVENTF_LEFTUP, 0));
            });
            pump_until(Duration::from_secs(1), || {
                list.selection() == span(7, target)
            });
            if list.selection() == span(7, target) {
                break;
            }
        }
        if pointer_at() != aimed {
            println!("     (ポインターがほかから動かされたので、確かめずに通す)");
            return None;
        }
        assert_eq!(list.selection(), span(7, target), "ポインターの下の行まで");
        // 選択は押した時点で変わる。フォーカスはそのあとで届くことがあるので待つ。
        pump_until(Duration::from_secs(1), || {
            focused_list_row(list) == Some(target)
        });
        assert_eq!(
            focused_list_row(list),
            Some(target),
            "押した行へ移る (ホイールのあと)"
        );

        // 押した行から、キーでも同じ起点のまま伸びる。
        press_nav_key(&[VK_SHIFT], VK_NEXT)?;
        assert_eq!(
            list.selection(),
            span(7, target + page),
            "Shift + Page Down"
        );

        // Space は `ListView` の覚えている位置ではなく、フォーカスの行に効く。
        let cursor = target + page;
        press_nav_key(&[], VK_SPACE)?;
        assert_eq!(list.selection(), [cursor], "Space はその行だけを選ぶ");
        press_nav_key(&[VK_CONTROL], VK_DOWN)?;
        assert_eq!(list.selection(), [cursor], "Ctrl + ↓ はフォーカスだけ");
        press_nav_key(&[VK_CONTROL], VK_SPACE)?;
        assert_eq!(
            list.selection(),
            [cursor, cursor + 1],
            "Ctrl + Space で足す"
        );
        press_nav_key(&[VK_CONTROL], VK_SPACE)?;
        assert_eq!(list.selection(), [cursor], "Ctrl + Space で引く");
        Some(())
    }
}

/// Once scrolling has taken the focused row out of the rows built, XAML puts the focus on a
/// row off screen. That must not move the view, by itself or when a press comes: a press
/// and its release have to land on the same row, or the click is lost.
fn table_view_stays_when_the_focus_is_off_screen(ui: &Ui) -> Result<()> {
    use naui_winui3::Microsoft::UI::Xaml::Automation::AutomationProperties;
    use naui_winui3::Microsoft::UI::Xaml::Controls::Control;
    use naui_winui3::Microsoft::UI::Xaml::FocusState;
    use windows::Win32::Foundation::POINT;
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_WHEEL,
    };
    use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

    with_shown_table_for_input(ui, 1_000, |hwnd, table| {
        let scroll = table.scroller();
        let height = table.row_height();
        let position = || scroll.VerticalOffset().expect("位置");
        let first_in_view = || (position() / height).ceil() as usize + 1;
        // 送ったクリックは表示と関係なく失われることがあるので、押し直してよい。押している
        // 間に表示が動くことだけは許さない。解放が別の行へ届いてしまう。
        let hold_and_release = |index: usize, what: &str| -> Option<()> {
            for attempt in 0..3 {
                if attempt > 0 {
                    println!("     ({what}: クリックが反映されないので、もう一度押す)");
                }
                aim_at_list_row(hwnd, table, index)?;
                let before = position();
                send_input(mouse_input(MOUSEEVENTF_LEFTDOWN, 0));
                pump_until(Duration::from_millis(400), || false);
                let during = position();
                send_input(mouse_input(MOUSEEVENTF_LEFTUP, 0));
                assert!(
                    (during - before).abs() < 1.0,
                    "{what}: 押している間に表示が動かないこと: {before} → {during}"
                );
                pump_until(Duration::from_secs(1), || table.selection() == [index]);
                if table.selection() == [index] {
                    break;
                }
            }
            assert_eq!(table.selection(), [index], "{what}: クリックが届くこと");
            assert_eq!(focused_list_row(table), Some(index), "{what}: フォーカス");
            Some(())
        };

        first_click_on_list_row(hwnd, table, 2)?;

        // XAML がするのと同じく、組み立て済みで画面の外にある行へフォーカスを移す。
        table.scroll_to_row(900);
        pump_until(Duration::from_millis(500), || false);
        let off_screen = table
            .list_view()
            .Items()
            .and_then(|items| items.GetAt(0))
            .and_then(|item| item.cast::<ListViewItem>())
            .expect("組み立てた先頭の行");
        let index = AutomationProperties::GetPositionInSet(&off_screen).expect("位置") as usize - 1;
        let before = position();
        assert!(
            ((index + 1) as f64) * height <= before,
            "{index} 行目は表示 {before} より上にあること"
        );
        for state in [
            FocusState::Programmatic,
            FocusState::Keyboard,
            FocusState::Pointer,
        ] {
            // 同じ行のままだと動きが起きないので、いったん見えている行へ移す。
            built_list_row(table, first_in_view())
                .expect("見えている行")
                .cast::<Control>()
                .and_then(|row| row.Focus(FocusState::Programmatic))
                .expect("フォーカス");
            pump_until(Duration::from_millis(200), || false);
            let focused = off_screen
                .cast::<Control>()
                .and_then(|row| row.Focus(state))
                .expect("フォーカス");
            assert!(focused, "画面の外の行がフォーカスを受けること");
            pump_until(Duration::from_secs(1), || false);
            assert!(
                (position() - before).abs() < 1.0,
                "{state:?} のフォーカスで表示が動かないこと: {before} → {}",
                position()
            );
        }
        hold_and_release(first_in_view(), "画面の外にフォーカスがあるとき")?;

        // ホイールでフォーカスの行を窓の外へ出す。端に着かないよう、先の行を残して始める。
        table.scroll_to_row(300);
        pump_until(Duration::from_millis(500), || false);
        hold_and_release(first_in_view(), "ホイールの前")?;
        for round in 0..2 {
            let what = format!("ホイール {round} 回目");
            let left = table.selection()[0];
            // ホイールは表の上にあるポインターにしか効かない。
            aim_at_list_row(hwnd, table, first_in_view())?;
            let start = position();
            let pointer = || {
                let mut at = POINT::default();
                // SAFETY: `at` はこの関数の中で生きている。
                let _ = unsafe { GetCursorPos(&mut at) };
                (at.x, at.y)
            };
            let aimed = pointer();
            for _ in 0..60 {
                if !foreground_is_ours() {
                    println!("     (テストのウィンドウが前面に無いので、ホイールを送らずに通す)");
                    return None;
                }
                if built_list_row(table, left).is_none() {
                    break;
                }
                send_input(mouse_input(MOUSEEVENTF_WHEEL, (-480_i32) as u32));
            }
            if pointer() != aimed {
                println!("     (ポインターがほかから動かされたので、確かめずに通す)");
                return None;
            }
            assert!(
                built_list_row(table, left).is_none(),
                "{what}: フォーカスの行から離れられること: 位置 {start} → {} ポインター {aimed:?}",
                position()
            );
            pump_until(Duration::from_millis(1_500), || false);
            let settled = position();
            pump_until(Duration::from_millis(500), || false);
            assert!(
                (position() - settled).abs() < 1.0,
                "{what}: 遅れて表示が動かないこと: {settled} → {}",
                position()
            );
            hold_and_release(first_in_view(), &what)?;
        }

        // `scroll_to_row` でフォーカスの行を窓の外へ出す。
        for round in 0..4 {
            let what = format!("scroll_to_row {round} 回目");
            table.scroll_to_row(if round % 2 == 0 { 300 } else { 900 });
            pump_until(Duration::from_millis(500), || false);
            hold_and_release(first_in_view() + round % 3, &what)?;
        }
        Some(())
    })
}

/// The height of a row of [`list_window_settles_with_rows_of_many_heights`]. Taken from the
/// first row built, a height would put the window where the first row has the other height,
/// over and over.
fn height_that_moves_the_window(index: usize) -> f64 {
    match index % 1_000 >= 900 {
        true => 72.0,
        false => 20.0,
    }
}

/// Rows of many heights do not keep the window moving: the rows' height only settles if it
/// cannot go back and forth with the rows built.
fn list_window_settles_with_rows_of_many_heights(ui: &Ui) -> Result<()> {
    use naui_windows::ListRow;

    let window = ui.window("高さの違うリストの行", 300.0, 240.0)?;
    let list = ui.list()?;
    {
        let ui = ui.clone();
        list.set_row_builder(10_000, move |index| {
            let label = ui.label(&format!("行 {index}"))?;
            label.set_sizing(Sizing::fixed(100.0, height_that_moves_the_window(index)));
            Ok(ListRow::new(&label))
        });
    }
    list.set_sizing(Sizing::fill());
    window.set_child(&list);
    window_settles_scenario(&window, &list)
}

/// [`list_window_settles_with_rows_of_many_heights`] on a `Table`.
fn table_window_settles_with_rows_of_many_heights(ui: &Ui) -> Result<()> {
    use naui_windows::TableCells;

    let window = ui.window("高さの違う表の行", 300.0, 240.0)?;
    let table = ui.table()?;
    table.set_columns(&[naui_core::TableColumn::new("名前")]);
    {
        let ui = ui.clone();
        table.set_row_builder(10_000, move |index| {
            let label = ui.label(&format!("行 {index}"))?;
            label.set_sizing(Sizing::fixed(100.0, height_that_moves_the_window(index)));
            Ok(TableCells::new().cell(&label))
        });
    }
    table.set_sizing(Sizing::fill());
    window.set_child(&table);
    window_settles_scenario(&window, &table)
}

fn window_settles_scenario(window: &naui_windows::Window, list: &impl RowsForInput) -> Result<()> {
    use naui_winui3::Microsoft::UI::Xaml::Automation::AutomationProperties;

    let outcome = catch_unwind(AssertUnwindSafe(|| {
        window.show();
        let scroll = list.scroller();
        pump_until(SCROLL_LAYOUT_TIMEOUT, || {
            scroll.ViewportHeight().unwrap_or(0.0) > 0.0 && list.row_height() > 0.0
        });
        if scroll.ViewportHeight().unwrap_or(0.0) <= 0.0 {
            println!("     (ScrollViewer に実寸が付かない環境なので、確かめずに通す)");
            return;
        }
        let state = || {
            let items = list.list_view().Items().expect("行");
            let first = items
                .GetAt(0)
                .and_then(|item| item.cast::<ListViewItem>())
                .and_then(|row| AutomationProperties::GetPositionInSet(&row))
                .expect("位置");
            (
                first,
                items.Size().expect("行数"),
                list.row_height().to_bits(),
            )
        };
        // 高さの低い行の位置から、窓の先頭が高い行へ来るところへ送る。
        for target in [1_000, 3_000, 40, 7_000] {
            list.scroll_to_row(target);
            pump_until(Duration::from_millis(500), || false);
            let settled = state();
            pump_until(Duration::from_millis(500), || false);
            assert_eq!(
                state(),
                settled,
                "{target} 行目へ送ったあと、窓が動き続けないこと"
            );
            assert!(
                settled.1 < 200,
                "{target} 行目: 絞られていること: {}",
                settled.1
            );
        }
    }));
    window.close();
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
    Ok(())
}

/// The height of a row of [`list_focus_follows_the_real_place_of_rows`].
fn alternating_row_height(index: usize) -> f64 {
    match index % 2 == 1 {
        true => 100.0,
        false => 20.0,
    }
}

/// Where a row is, when rows differ in height, is known only from the row itself: an
/// estimate from a row height takes rows on screen for rows off it, and the focus is lost.
fn list_focus_follows_the_real_place_of_rows(ui: &Ui) -> Result<()> {
    use naui_windows::ListRow;

    let list = ui.list()?;
    {
        let ui = ui.clone();
        list.set_row_builder(100, move |index| {
            let label = ui.label(&format!("行 {index}"))?;
            label.set_sizing(Sizing::fixed(100.0, alternating_row_height(index)));
            Ok(ListRow::new(&label))
        });
    }
    list.set_sizing(Sizing::fill());
    real_place_scenario(ui, &list)
}

/// [`list_focus_follows_the_real_place_of_rows`] on a `Table`.
fn table_focus_follows_the_real_place_of_rows(ui: &Ui) -> Result<()> {
    use naui_windows::TableCells;

    let table = ui.table()?;
    table.set_columns(&[naui_core::TableColumn::new("名前")]);
    {
        let ui = ui.clone();
        table.set_row_builder(100, move |index| {
            let label = ui.label(&format!("行 {index}"))?;
            label.set_sizing(Sizing::fixed(100.0, alternating_row_height(index)));
            Ok(TableCells::new().cell(&label))
        });
    }
    table.set_sizing(Sizing::fill());
    real_place_scenario(ui, &table)
}

/// Page Up and Page Down move by what is on screen, also when rows differ in height: a page
/// counted in the tallest row's height would be far shorter than the screen.
fn list_page_keys_go_by_the_real_height_of_rows(ui: &Ui) -> Result<()> {
    use naui_windows::ListRow;

    let window = ui.window("高さの違うリストの Page", 300.0, 320.0)?;
    let list = ui.list()?;
    {
        let ui = ui.clone();
        list.set_row_builder(100, move |index| {
            let label = ui.label(&format!("行 {index}"))?;
            label.set_sizing(Sizing::fixed(100.0, alternating_row_height(index)));
            Ok(ListRow::new(&label))
        });
    }
    list.set_sizing(Sizing::fill());
    window.set_child(&list);
    run_shown_for_input(&window, &list, real_height_pages_scenario)
}

/// [`list_page_keys_go_by_the_real_height_of_rows`] on a `Table`.
fn table_page_keys_go_by_the_real_height_of_rows(ui: &Ui) -> Result<()> {
    use naui_windows::TableCells;

    let window = ui.window("高さの違う表の Page", 300.0, 320.0)?;
    let table = ui.table()?;
    table.set_columns(&[naui_core::TableColumn::new("名前")]);
    {
        let ui = ui.clone();
        table.set_row_builder(100, move |index| {
            let label = ui.label(&format!("行 {index}"))?;
            label.set_sizing(Sizing::fixed(100.0, alternating_row_height(index)));
            Ok(TableCells::new().cell(&label))
        });
    }
    table.set_sizing(Sizing::fill());
    window.set_child(&table);
    run_shown_for_input(&window, &table, real_height_pages_scenario)
}

fn real_height_pages_scenario(
    hwnd: windows::Win32::Foundation::HWND,
    list: &impl RowsForInput,
) -> Option<()> {
    use windows::Foundation::Point as XamlPoint;
    use windows::Win32::UI::Input::KeyboardAndMouse::{VK_NEXT, VK_PRIOR};

    let scroll = list.scroller();
    let viewport = scroll.ViewportHeight().expect("高さ");
    let position = || scroll.VerticalOffset().expect("位置");
    let top_of = |index: usize| {
        let row = built_list_row(list, index).expect("組み立てた行");
        let at = row
            .TransformToVisual(&scroll)
            .and_then(|to| to.TransformPoint(XamlPoint { X: 0.0, Y: 0.0 }))
            .expect("行の位置");
        f64::from(at.Y) + position()
    };
    let height_of = |index: usize| {
        built_list_row(list, index)
            .expect("組み立てた行")
            .ActualHeight()
            .expect("行の高さ")
    };

    list.scroll_to_row(20);
    pump_until(Duration::from_millis(500), || false);
    click_list_row_expecting(hwnd, list, 20, &[], &[20], "高さの違う行")?;

    // Page Down: 今の行の上端から 1 画面下にある行へ、表示も 1 画面ぶん進む。
    let from = list.selection()[0];
    let below = top_of(from) + viewport;
    let expected = (from..100)
        .find(|&index| top_of(index) + height_of(index) > below)
        .expect("1 画面下の行");
    let before = position();
    press_nav_key(&[], VK_NEXT)?;
    assert_eq!(list.selection(), [expected], "Page Down の行");
    let moved = position() - before;
    assert!(
        (moved - viewport).abs() <= 1.0,
        "Page Down で表示が 1 画面ぶん進むこと: {moved} (表示 {viewport})"
    );

    // Page Up: 上端が 1 画面ぶん上か、それより下にある最初の行へ。
    let from = list.selection()[0];
    let above = top_of(from) - viewport;
    let expected = (0..from)
        .find(|&index| top_of(index) >= above)
        .expect("1 画面上の行");
    let before = position();
    press_nav_key(&[], VK_PRIOR)?;
    assert_eq!(list.selection(), [expected], "Page Up の行");
    let moved = before - position();
    assert!(
        (moved - viewport).abs() <= 1.0,
        "Page Up で表示が 1 画面ぶん戻ること: {moved} (表示 {viewport})"
    );
    Some(())
}

/// A Shift click reaches the row under the pointer even in the row's margin, which
/// `ListViewItem` keeps to itself, right after a scroll with the pointer left still: no event
/// says which row the pointer is on then.
fn list_shift_click_in_the_margin_after_a_scroll(ui: &Ui) -> Result<()> {
    with_shown_list_for_input(ui, 1_000, shift_click_in_margin_scenario)
}

/// [`list_shift_click_in_the_margin_after_a_scroll`] on a `Table`.
fn table_shift_click_in_the_margin_after_a_scroll(ui: &Ui) -> Result<()> {
    with_shown_table_for_input(ui, 1_000, shift_click_in_margin_scenario)
}

fn shift_click_in_margin_scenario(
    hwnd: windows::Win32::Foundation::HWND,
    list: &impl RowsForInput,
) -> Option<()> {
    use naui_winui3::Microsoft::UI::Xaml::Automation::AutomationProperties;
    use windows::Foundation::Point as XamlPoint;
    use windows::Win32::Foundation::POINT;
    use windows::Win32::Graphics::Gdi::ClientToScreen;
    use windows::Win32::UI::HiDpi::GetDpiForWindow;
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_WHEEL, VK_SHIFT,
    };
    use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

    // 行の左端の余白。文字の入れ物より左なので、行の中の `PointerPressed` は来ない。
    const MARGIN: f32 = 4.0;
    let scroll = list.scroller();
    let pointer_at = || {
        let mut at = POINT::default();
        // SAFETY: `at` はこの関数の中で生きている。
        let _ = unsafe { GetCursorPos(&mut at) };
        at
    };
    let click = |what: &str, expected: &dyn Fn() -> bool, modifiers: &[_]| {
        for _ in 0..3 {
            with_modifiers(modifiers, || {
                send_input(mouse_input(MOUSEEVENTF_LEFTDOWN, 0));
                send_input(mouse_input(MOUSEEVENTF_LEFTUP, 0));
            });
            pump_until(Duration::from_secs(1), expected);
            if expected() {
                return;
            }
            println!("     ({what}: クリックが反映されないので、もう一度押す)");
        }
    };

    aim_at_row_point(hwnd, list, 2, MARGIN)?;
    click("余白の行", &|| list.selection() == [2], &[]);
    assert_eq!(list.selection(), [2], "余白を押して選べること");

    // ポインターを動かさずにスクロールする。
    let aimed = pointer_at();
    for _ in 0..60 {
        if !foreground_is_ours() {
            println!("     (テストのウィンドウが前面に無いので、ホイールを送らずに通す)");
            return None;
        }
        if built_list_row(list, 2).is_none() {
            break;
        }
        send_input(mouse_input(MOUSEEVENTF_WHEEL, (-480_i32) as u32));
    }
    let mut settled = scroll.VerticalOffset().expect("位置");
    pump_until(Duration::from_secs(3), || {
        pump_until(Duration::from_millis(300), || false);
        let now = scroll.VerticalOffset().expect("位置");
        let still = (now - settled).abs() < 0.5;
        settled = now;
        still
    });
    let moved_elsewhere = |at: POINT| at.x != aimed.x || at.y != aimed.y;
    if moved_elsewhere(pointer_at()) {
        println!("     (ポインターがほかから動かされたので、確かめずに通す)");
        return None;
    }
    assert!(built_list_row(list, 2).is_none(), "起点の行が窓の外にある");

    // ポインターの下の行を、行そのものの位置で求める。
    // SAFETY: HWND は window が閉じるまで有効で、UI スレッドから読んでいる。
    let scale = unsafe { GetDpiForWindow(hwnd) } as f32 / 96.0;
    let mut origin = POINT::default();
    // SAFETY: `origin` はこの関数の中で生きている。
    let _ = unsafe { ClientToScreen(hwnd, &mut origin) };
    let y = (aimed.y - origin.y) as f32 / scale;
    let row_under_pointer = || {
        let items = list.list_view().Items().ok()?;
        (0..items.Size().ok()?).find_map(|slot| {
            let row = items.GetAt(slot).ok()?.cast::<ListViewItem>().ok()?;
            let top = row
                .TransformToVisual(None)
                .and_then(|to| to.TransformPoint(XamlPoint { X: 0.0, Y: 0.0 }))
                .ok()?;
            let bottom = top.Y + row.ActualHeight().ok()? as f32;
            (y >= top.Y && y < bottom)
                .then(|| AutomationProperties::GetPositionInSet(&row).expect("位置") as usize - 1)
        })
    };
    let target = row_under_pointer().expect("ポインターの下の行");

    let range: Vec<usize> = (2..=target).collect();
    click(
        "余白の Shift クリック",
        &|| list.selection() == range,
        &[VK_SHIFT],
    );
    if moved_elsewhere(pointer_at()) {
        println!("     (ポインターがほかから動かされたので、確かめずに通す)");
        return None;
    }
    let picked = list.selection();
    assert_eq!(
        (
            picked.first().copied(),
            picked.last().copied(),
            picked.len()
        ),
        (Some(2), Some(target), range.len()),
        "余白の Shift クリックの範囲"
    );
    assert_eq!(focused_list_row(list), Some(target), "押した行へ移る");

    // ポインターのイベントが何も起きないスクロールのあとでも、同じ。
    click("選び直し", &|| list.selection() == [target], &[]);
    assert_eq!(list.selection(), [target], "選び直せること");
    for (round, to) in [400, 800, 200, 600].into_iter().enumerate() {
        let anchor = list.selection()[0];
        list.scroll_to_row(to);
        // 行の位置はスクロールに遅れて付いてくる。特に GPU の無い環境で遅い。
        pump_until(SCROLL_LAYOUT_TIMEOUT, || {
            row_under_pointer().is_some_and(|row| row.abs_diff(to) <= 10)
        });
        let under = row_under_pointer().expect("ポインターの下の行");
        assert!(
            under.abs_diff(to) <= 10,
            "{round} 回目: {to} 行目の近くへ送られること: {under} 行目 位置 {:?}",
            scroll.VerticalOffset()
        );
        let range: Vec<usize> = (anchor.min(under)..=anchor.max(under)).collect();
        click(
            "スクロールのあとの Shift クリック",
            &|| list.selection() == range,
            &[VK_SHIFT],
        );
        if moved_elsewhere(pointer_at()) {
            println!("     (ポインターがほかから動かされたので、確かめずに通す)");
            return None;
        }
        let picked = list.selection();
        assert_eq!(
            (
                picked.first().copied(),
                picked.last().copied(),
                picked.len()
            ),
            (range.first().copied(), range.last().copied(), range.len()),
            "{round} 回目: {anchor} 行目から {under} 行目までの範囲"
        );
        assert_eq!(
            focused_list_row(list),
            Some(under),
            "{round} 回目: 押した行へ移る"
        );
        click("次の起点", &|| list.selection() == [under], &[]);
    }

    // 窓が動かないところだと、`ListView` の範囲は押した行より後ろまで続く。押した行が範囲の
    // どちらの端かは、ポインターの乗っている行でしか分からない。
    let anchor = list.selection()[0];
    let height = list.row_height();
    let view = scroll.VerticalOffset().expect("位置");
    let offset = PropertyValue::CreateDouble(view - height * 2.0)
        .and_then(|value| value.cast::<windows::Foundation::IReference<f64>>())
        .expect("送り先");
    scroll
        .ChangeViewWithOptionalAnimation(None, &offset, None, true)
        .expect("スクロール");
    pump_until(SCROLL_LAYOUT_TIMEOUT, || {
        row_under_pointer().is_some_and(|row| row < anchor)
    });
    let under = row_under_pointer().expect("ポインターの下の行");
    assert!(
        under < anchor,
        "起点より上の行を押すこと: {under} / {anchor} 位置 {view} → {:?}",
        scroll.VerticalOffset()
    );
    let range: Vec<usize> = (under..=anchor).collect();
    click(
        "上へ戻った Shift クリック",
        &|| list.selection() == range,
        &[VK_SHIFT],
    );
    if moved_elsewhere(pointer_at()) {
        println!("     (ポインターがほかから動かされたので、確かめずに通す)");
        return None;
    }
    assert_eq!(list.selection(), range, "上へ戻った Shift クリックの範囲");
    assert_eq!(focused_list_row(list), Some(under), "上へ戻った行へ移る");
    Some(())
}

fn real_place_scenario(ui: &Ui, list: &(impl RowsForInput + Widget)) -> Result<()> {
    use naui_winui3::Microsoft::UI::Xaml::Controls::Control;
    use naui_winui3::Microsoft::UI::Xaml::FocusState;
    use windows::Foundation::Point as XamlPoint;
    use windows::Win32::UI::Input::KeyboardAndMouse::{VK_DOWN, VK_TAB};

    let window = ui.window("高さの違う行のフォーカス", 300.0, 320.0)?;
    let stack = ui.stack(Orientation::Vertical)?;
    let button = ui.button("手前")?;
    stack.set_sizing(Sizing::fill());
    stack.append(&button);
    stack.append(list);
    window.set_child(&stack);
    run_shown_for_input(&window, list, |hwnd, list| {
        let scroll = list.scroller();
        // 行そのものの位置で、見えているかを確かめる。
        let visible = |index: usize| {
            let Some(row) = built_list_row(list, index) else {
                return false;
            };
            let top = row
                .TransformToVisual(&scroll)
                .and_then(|to| to.TransformPoint(XamlPoint { X: 0.0, Y: 0.0 }))
                .expect("行の位置");
            let bottom = f64::from(top.Y) + row.ActualHeight().expect("行の高さ");
            bottom > 0.0 && f64::from(top.Y) < scroll.ViewportHeight().expect("高さ")
        };
        let target = 50;
        list.scroll_to_row(target);
        pump_until(Duration::from_millis(500), || false);
        assert!(visible(target), "{target} 行目が見えていること");
        click_list_row_expecting(hwnd, list, target, &[], &[target], "高さの違う行")?;
        assert_eq!(focused_list_row(list), Some(target), "押した行へ移る");

        // 組み立て直しても、見えている行のフォーカスは戻る。
        list.refresh();
        pump_until(Duration::from_millis(500), || false);
        assert_eq!(
            focused_list_row(list),
            Some(target),
            "組み立て直したあともフォーカスが残ること"
        );
        press_nav_key(&[], VK_DOWN)?;
        assert_eq!(list.selection(), [target + 1], "次のキーが効くこと");
        assert!(visible(target + 1), "移った行が見えていること");

        // 手前のボタンから Tab で入っても、見えている行へ付く。
        list.scroll_to_row(20);
        pump_until(Duration::from_millis(500), || false);
        let _ = native::<XamlButton>(&button).Focus(FocusState::Keyboard);
        pump_until(FOCUS_TIMEOUT, || {
            native::<Control>(&button).FocusState() != Ok(FocusState::Unfocused)
        });
        press_nav_key(&[], VK_TAB)?;
        pump_until(SCROLL_LAYOUT_TIMEOUT, || {
            focused_list_row(list).is_some_and(visible)
        });
        let Some(focused) = focused_list_row(list) else {
            println!("     (Tab で行へ入らない環境なので、確かめずに通す)");
            return None;
        };
        assert!(
            visible(focused),
            "Tab で見えている行へ入ること: {focused} 行目"
        );
        Some(())
    })
}

/// The arrow keys carry on past the rows built, as the rows follow.
fn table_arrow_keys_go_past_the_rows_built(ui: &Ui) -> Result<()> {
    use windows::Win32::UI::Input::KeyboardAndMouse::{VK_DOWN, VK_UP};

    with_shown_table_for_input(ui, 1_000, |hwnd, table| {
        let scroll = table.scroller();
        let height = table.row_height();
        let on_screen = |index: usize| {
            let top = scroll.VerticalOffset().expect("位置");
            let bottom = top + scroll.ViewportHeight().expect("高さ");
            index as f64 * height >= top - 1.0 && (index + 1) as f64 * height <= bottom + 1.0
        };
        first_click_on_list_row(hwnd, table, 0)?;
        let built = table.list_view().Items().expect("行").Size().expect("行数") as usize;
        for at in 1..=built + 6 {
            press_nav_key(&[], VK_DOWN)?;
            assert_eq!(table.selection(), [at], "↓");
            assert_eq!(focused_list_row(table), Some(at), "↓ のフォーカス");
            assert!(on_screen(at), "{at} 行目が見えていること");
        }
        press_nav_key(&[], VK_UP)?;
        assert_eq!(table.selection(), [built + 5], "↑");
        Some(())
    })
}

/// A table with an `Auto` height whose parent gives it no more room than that scrolls
/// itself, so the keys are the same as with a height set. One with nowhere to scroll leaves
/// them to `ListView`.
fn table_keys_depend_on_whether_it_scrolls(ui: &Ui) -> Result<()> {
    use naui_core::{TableColumn, TableRow};
    use windows::Win32::UI::Input::KeyboardAndMouse::{VK_DOWN, VK_NEXT};

    let window = ui.window("Auto の高さの表", 300.0, 240.0)?;
    let table = ui.table()?;
    table.set_columns(&[TableColumn::new("名前")]);
    let labels: Vec<String> = (0..1_000).map(|index| format!("行 {index}")).collect();
    table.set_rows(&TableRow::list(labels.iter().map(|l| [l.as_str()])));
    window.set_child(&table);
    run_shown_for_input(&window, &table, |hwnd, table| {
        let scroll = table.scroller();
        assert!(scroll.ScrollableHeight().expect("高さ") > 0.0, "自分で動く");
        let page = (scroll.ViewportHeight().expect("高さ") / table.row_height()).floor() as usize;
        first_click_on_list_row(hwnd, table, 1)?;
        press_nav_key(&[], VK_NEXT)?;
        assert_eq!(table.selection(), [1 + page], "Auto の高さの Page Down");
        Some(())
    })?;

    let window = ui.window("スクロールしない表", 300.0, 400.0)?;
    let table = ui.table()?;
    table.set_columns(&[TableColumn::new("名前")]);
    table.set_rows(&TableRow::list(labels[..4].iter().map(|l| [l.as_str()])));
    table.set_sizing(Sizing::fill());
    window.set_child(&table);
    run_shown_for_input(&window, &table, |hwnd, table| {
        let scroll = table.scroller();
        assert_eq!(scroll.ScrollableHeight().expect("高さ"), 0.0, "動かない");
        first_click_on_list_row(hwnd, table, 0)?;
        press_nav_key(&[], VK_DOWN)?;
        assert_eq!(table.selection(), [1], "↓");
        press_nav_key(&[], VK_NEXT)?;
        assert_eq!(table.selection(), [3], "Page Down は最後の行まで");
        Some(())
    })
}

/// A disabled row is passed over, and a single selection stays one row whatever the
/// modifier keys, even with the row selected before out of the rows built.
fn table_single_selection_stays_one_row(ui: &Ui) -> Result<()> {
    use naui_core::{TableColumn, TableRow};
    use windows::Win32::UI::Input::KeyboardAndMouse::{VK_CONTROL, VK_DOWN, VK_SHIFT, VK_UP};

    let window = ui.window("単一選択の表", 300.0, 240.0)?;
    let table = ui.table()?;
    table.set_columns(&[TableColumn::new("名前")]);
    let rows: Vec<TableRow> = (0..1_000)
        .map(|index| TableRow::new([format!("行 {index}")]).enabled(index != 3))
        .collect();
    table.set_rows(&rows);
    table.set_sizing(Sizing::fill());
    window.set_child(&table);
    run_shown_for_input(&window, &table, |hwnd, table| {
        first_click_on_list_row(hwnd, table, 1)?;
        press_nav_key(&[], VK_DOWN)?;
        assert_eq!(table.selection(), [2], "↓");
        press_nav_key(&[], VK_DOWN)?;
        assert_eq!(table.selection(), [4], "無効の行を飛ばす");
        press_nav_key(&[], VK_UP)?;
        assert_eq!(table.selection(), [2], "手前へも飛ばす");
        press_nav_key(&[VK_SHIFT], VK_DOWN)?;
        assert_eq!(table.selection(), [4], "単一選択では Shift でも 1 行");

        table.scroll_to_row(800);
        pump_until(Duration::from_millis(500), || false);
        assert!(built_list_row(table, 4).is_none(), "選んだ行が窓の外にある");
        click_list_row_expecting(hwnd, table, 799, &[VK_CONTROL], &[799], "Ctrl クリック")
    })
}

/// [`list_keeps_rows_that_stay_in_the_window`] on a `Table`.
fn table_keeps_rows_that_stay_in_the_window(ui: &Ui) -> Result<()> {
    use naui_core::ROW_WINDOW_OVERSCAN;
    use naui_windows::TableCells;
    use naui_winui3::Microsoft::UI::Xaml::Controls::Control;
    use naui_winui3::Microsoft::UI::Xaml::FocusState;
    use std::collections::HashMap;

    const ROWS: usize = 1_000;
    const TARGET: usize = 100;
    let window = ui.window("表の窓に残る行", 240.0, 160.0)?;
    let table = ui.table()?;
    table.set_columns(&[naui_core::TableColumn::new("名前")]);
    let inputs: Rc<RefCell<HashMap<usize, naui_windows::TextInput>>> = Rc::default();
    let builds: Rc<RefCell<HashMap<usize, usize>>> = Rc::default();
    {
        let ui = ui.clone();
        let inputs = inputs.clone();
        let builds = builds.clone();
        table.set_row_builder(ROWS, move |index| {
            *builds.borrow_mut().entry(index).or_default() += 1;
            let input = ui.text_input(&format!("行 {index}"))?;
            inputs.borrow_mut().insert(index, input.clone());
            Ok(TableCells::new().cell(&input))
        });
    }
    table.set_sizing(Sizing::fill());
    window.set_child(&table);
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        window.show();
        let scroll = table_scroller(&table);
        pump_until(SCROLL_LAYOUT_TIMEOUT, || {
            scroll.ViewportHeight().unwrap_or(0.0) > 0.0 && table.row_height() > 0.0
        });
        if scroll.ViewportHeight().unwrap_or(0.0) <= 0.0 {
            println!("     (ScrollViewer に実寸が付かない環境なので、確かめずに通す)");
            return;
        }
        table.scroll_to_row(TARGET);
        pump_until(SCROLL_LAYOUT_TIMEOUT, || {
            inputs.borrow().contains_key(&TARGET)
        });
        let input = inputs.borrow().get(&TARGET).cloned().expect("送った先の行");
        pump_until(FOCUS_TIMEOUT, || input.request_focus());
        let focused = || {
            input
                .native_element()
                .cast::<Control>()
                .and_then(|control| control.FocusState())
                .is_ok_and(|state| state != FocusState::Unfocused)
        };
        if !focused() {
            println!("     (フォーカスを受けられない環境なので、確かめずに通す)");
            return;
        }
        let built = builds.borrow()[&TARGET];

        // 窓が先へ動いても、入力欄の行は窓の中に残っている。
        let next = TARGET + ROW_WINDOW_OVERSCAN;
        table.scroll_to_row(next);
        pump_until(SCROLL_LAYOUT_TIMEOUT, || {
            builds.borrow().contains_key(&next)
        });
        assert!(builds.borrow().contains_key(&next), "窓が動いていること");
        assert!(focused(), "窓が動いても入力欄にフォーカスが残ること");
        assert_eq!(
            builds.borrow()[&TARGET],
            built,
            "残る行は組み立て直さないこと"
        );
    }));
    window.close();
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
    Ok(())
}

/// Replacing the rows from inside the function that builds a row leaves only the new rows,
/// however far the build had got. The rows kept from before must not stay behind.
fn table_builder_can_replace_the_rows(ui: &Ui) -> Result<()> {
    use naui_windows::TableCells;
    use naui_winui3::Microsoft::UI::Xaml::Automation::AutomationProperties;

    /// What the old builder does when it reaches a row, once.
    type Replace = Box<dyn FnOnce()>;

    let window = ui.window("表の行の差し替え", 240.0, 160.0)?;
    let table = ui.table()?;
    table.set_columns(&[naui_core::TableColumn::new("名前")]);
    table.set_sizing(Sizing::fill());
    window.set_child(&table);
    let replace: Rc<RefCell<Option<Replace>>> = Rc::default();
    {
        let replace = replace.clone();
        table.set_row_builder(1_000, move |index| {
            let pending = replace.borrow_mut().take();
            if let Some(pending) = pending {
                pending();
            }
            Ok(TableCells::new().text(format!("古い {index}")))
        });
    }
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        window.show();
        let scroll = table_scroller(&table);
        pump_until(SCROLL_LAYOUT_TIMEOUT, || {
            scroll.ViewportHeight().unwrap_or(0.0) > 0.0 && table.row_height() > 0.0
        });
        if scroll.ViewportHeight().unwrap_or(0.0) <= 0.0 {
            println!("     (ScrollViewer に実寸が付かない環境なので、確かめずに通す)");
            return;
        }
        pump_until(Duration::from_millis(300), || false);
        *replace.borrow_mut() = Some(Box::new({
            let table = table.clone();
            move || {
                table.set_row_builder(2_000, |index| {
                    Ok(TableCells::new().text(format!("新しい {index}")))
                });
            }
        }));
        // 窓が動いて増える行の組み立ての中で、差し替わる。
        let built = table.list_view().Items().expect("行").Size().expect("行数") as usize;
        table.scroll_to_row(built + 2);
        pump_until(SCROLL_LAYOUT_TIMEOUT, || table.len() == 2_000);
        assert_eq!(table.len(), 2_000, "差し替わること");
        pump_until(Duration::from_millis(300), || false);

        let items = table.list_view().Items().expect("行");
        let count = items.Size().expect("行数");
        assert!(count > 0, "行があること");
        for slot in 0..count {
            let row = items
                .GetAt(slot)
                .and_then(|item| item.cast::<ListViewItem>())
                .expect("行");
            assert_eq!(
                AutomationProperties::GetSizeOfSet(&row).expect("全行数"),
                2_000,
                "古い行が残らないこと"
            );
            let index = AutomationProperties::GetPositionInSet(&row).expect("位置") - 1;
            let text = row
                .Content()
                .and_then(|content| content.cast::<Grid>())
                .and_then(|grid| grid.Children())
                .and_then(|children| children.GetAt(0))
                .and_then(|cell| cell.cast::<TextBlock>())
                .and_then(|cell| cell.Text())
                .unwrap_or_default()
                .to_string();
            assert_eq!(text, format!("新しい {index}"), "新しい行の中身");
        }
    }));
    window.close();
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
    Ok(())
}

/// What `ListView` did along the arrows is kept where naui moves the cursor: a disabled row
/// is passed over, and a single selection follows the cursor even with Shift.
fn list_arrow_keys_pass_over_disabled_rows(ui: &Ui) -> Result<()> {
    use naui_core::ListItem;
    use windows::Win32::Foundation::POINT;
    use windows::Win32::UI::Input::KeyboardAndMouse::{VK_DOWN, VK_SHIFT, VK_UP};
    use windows::Win32::UI::WindowsAndMessaging::{GetCursorPos, SetCursorPos};

    let window = ui.window("矢印キーと無効の行", 300.0, 240.0)?;
    let list = ui.list()?;
    let items: Vec<ListItem> = (0..60)
        .map(|index| ListItem::new(format!("行 {index}")).enabled(index != 3))
        .collect();
    list.set_items(&items);
    list.set_sizing(Sizing::fill());
    window.set_child(&list);
    let mut pointer = POINT::default();
    // SAFETY: `pointer` はこの関数の中で生きている。
    let _ = unsafe { GetCursorPos(&mut pointer) };
    let outcome = catch_unwind(AssertUnwindSafe(|| -> Option<()> {
        window.show();
        pump_until(SCROLL_LAYOUT_TIMEOUT, || list.row_height() > 0.0);
        let hwnd = bring_to_front(&window)?;
        pump_until(Duration::from_millis(500), || false);

        first_click_on_list_row(hwnd, &list, 1)?;
        press_nav_key(&[], VK_DOWN)?;
        assert_eq!(list.selection(), [2], "↓");
        press_nav_key(&[], VK_DOWN)?;
        assert_eq!(list.selection(), [4], "無効の行を飛ばす");
        press_nav_key(&[], VK_UP)?;
        assert_eq!(list.selection(), [2], "手前へも飛ばす");
        press_nav_key(&[VK_SHIFT], VK_DOWN)?;
        assert_eq!(list.selection(), [4], "単一選択では Shift でも 1 行");
        Some(())
    }));
    // SAFETY: 引数は値だけ。
    let _ = unsafe { SetCursorPos(pointer.x, pointer.y) };
    window.close();
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
    Ok(())
}

/// How many rows [`keys_over_unselectable_rows_scenario`] runs over.
const UNSELECTABLE_ROWS: usize = 1_000;

/// Whether a row of [`keys_over_unselectable_rows_scenario`] can be selected.
fn is_selectable_in_scenario(index: usize) -> bool {
    index != 0 && index != 3 && !(20..=80).contains(&index) && index != UNSELECTABLE_ROWS - 1
}

/// Moving the selection passes over rows that cannot be selected, such as headings, so it
/// never ends up empty. Ctrl moves only the focus, so it can still stop on them.
fn list_keys_pass_over_rows_that_cannot_be_selected(ui: &Ui) -> Result<()> {
    use naui_windows::ListRow;

    let window = ui.window("選べない行とキー", 300.0, 240.0)?;
    let list = ui.list()?;
    list.set_selection_mode(naui_core::SelectionMode::Multiple);
    {
        let ui = ui.clone();
        list.set_row_builder(UNSELECTABLE_ROWS, move |index| {
            Ok(ListRow::new(&ui.label(&format!("行 {index}"))?)
                .selectable(is_selectable_in_scenario(index)))
        });
    }
    list.set_row_selectable(is_selectable_in_scenario);
    list.set_sizing(Sizing::fill());
    window.set_child(&list);
    run_shown_for_input(&window, &list, keys_over_unselectable_rows_scenario)
}

/// [`list_keys_pass_over_rows_that_cannot_be_selected`] on a `Table`.
fn table_keys_pass_over_rows_that_cannot_be_selected(ui: &Ui) -> Result<()> {
    use naui_windows::TableCells;

    let window = ui.window("表の選べない行とキー", 300.0, 240.0)?;
    let table = ui.table()?;
    table.set_selection_mode(naui_core::SelectionMode::Multiple);
    table.set_columns(&[naui_core::TableColumn::new("名前")]);
    table.set_row_builder(UNSELECTABLE_ROWS, |index| {
        Ok(TableCells::new()
            .text(format!("行 {index}"))
            .selectable(is_selectable_in_scenario(index)))
    });
    table.set_row_selectable(is_selectable_in_scenario);
    table.set_sizing(Sizing::fill());
    window.set_child(&table);
    run_shown_for_input(&window, &table, keys_over_unselectable_rows_scenario)
}

/// Whether a row of [`keys_over_long_unknown_runs_scenario`] can be selected. The runs are
/// longer than the rows built, and nothing says so before they are built.
fn is_selectable_in_long_runs(index: usize) -> bool {
    !(20..=300).contains(&index) && !(350..=480).contains(&index) && index < 900
}

/// Without `set_row_selectable`, a row is known not to be selectable only once built. A run
/// of those longer than the rows built is still passed over, in one key press.
fn list_keys_pass_over_long_runs_only_the_builder_knows(ui: &Ui) -> Result<()> {
    use naui_windows::ListRow;

    let window = ui.window("長く続く選べない行", 300.0, 240.0)?;
    let list = ui.list()?;
    {
        let ui = ui.clone();
        list.set_row_builder(1_000, move |index| {
            Ok(ListRow::new(&ui.label(&format!("行 {index}"))?)
                .selectable(is_selectable_in_long_runs(index)))
        });
    }
    list.set_sizing(Sizing::fill());
    window.set_child(&list);
    run_shown_for_input(&window, &list, keys_over_long_unknown_runs_scenario)
}

/// [`list_keys_pass_over_long_runs_only_the_builder_knows`] on a `Table`.
fn table_keys_pass_over_long_runs_only_the_builder_knows(ui: &Ui) -> Result<()> {
    use naui_windows::TableCells;

    let window = ui.window("表の長く続く選べない行", 300.0, 240.0)?;
    let table = ui.table()?;
    table.set_columns(&[naui_core::TableColumn::new("名前")]);
    table.set_row_builder(1_000, |index| {
        Ok(TableCells::new()
            .text(format!("行 {index}"))
            .selectable(is_selectable_in_long_runs(index)))
    });
    table.set_sizing(Sizing::fill());
    window.set_child(&table);
    run_shown_for_input(&window, &table, keys_over_long_unknown_runs_scenario)
}

/// When no row ahead can be selected, a key moving the selection leaves the view and the rows
/// built where they were. Looking for one builds rows far away, and they must not stay built
/// in place of the rows on screen.
fn list_keeps_its_rows_when_no_row_ahead_can_be_selected(ui: &Ui) -> Result<()> {
    use naui_windows::ListRow;

    let window = ui.window("先に選べる行が無いリスト", 300.0, 240.0)?;
    let list = ui.list()?;
    {
        let ui = ui.clone();
        list.set_row_builder(1_000, move |index| {
            Ok(ListRow::new(&ui.label(&format!("行 {index}"))?).selectable(index < 300))
        });
    }
    list.set_sizing(Sizing::fill());
    window.set_child(&list);
    run_shown_for_input(&window, &list, no_row_ahead_scenario)
}

/// [`list_keeps_its_rows_when_no_row_ahead_can_be_selected`] on a `Table`.
fn table_keeps_its_rows_when_no_row_ahead_can_be_selected(ui: &Ui) -> Result<()> {
    use naui_windows::TableCells;

    let window = ui.window("先に選べる行が無い表", 300.0, 240.0)?;
    let table = ui.table()?;
    table.set_columns(&[naui_core::TableColumn::new("名前")]);
    table.set_row_builder(1_000, |index| {
        Ok(TableCells::new()
            .text(format!("行 {index}"))
            .selectable(index < 300))
    });
    table.set_sizing(Sizing::fill());
    window.set_child(&table);
    run_shown_for_input(&window, &table, no_row_ahead_scenario)
}

fn no_row_ahead_scenario(
    hwnd: windows::Win32::Foundation::HWND,
    list: &impl RowsForInput,
) -> Option<()> {
    use windows::Win32::UI::Input::KeyboardAndMouse::{VK_CONTROL, VK_DOWN, VK_UP};

    let scroll = list.scroller();
    let height = list.row_height();
    // 画面に見えている行が、どれも組み立ててあること。
    let missing_on_screen = || {
        let top = scroll.VerticalOffset().expect("位置");
        let bottom = top + scroll.ViewportHeight().expect("高さ");
        let first = (top / height).floor() as usize;
        let last = ((bottom / height).ceil() as usize).min(999);
        (first..=last)
            .filter(|&index| built_list_row(list, index).is_none())
            .collect::<Vec<usize>>()
    };

    list.scroll_to_row(299);
    pump_until(Duration::from_millis(500), || false);
    click_list_row_expecting(hwnd, list, 299, &[], &[299], "最後の選べる行")?;
    // Ctrl はフォーカスだけなので、選べない行へ移れる。
    press_nav_key(&[VK_CONTROL], VK_DOWN)?;
    assert_eq!(focused_list_row(list), Some(300), "Ctrl + ↓ で選べない行へ");
    let before = scroll.VerticalOffset().expect("位置");

    // その先は最後まで選べない。探しても見つからない。
    press_nav_key(&[], VK_DOWN)?;
    assert_eq!(list.selection(), [299], "選択は変わらない");
    assert_eq!(focused_list_row(list), Some(300), "フォーカスも動かない");
    assert_eq!(
        scroll.VerticalOffset().expect("位置"),
        before,
        "表示は動かない"
    );
    assert_eq!(
        missing_on_screen(),
        Vec::<usize>::new(),
        "キーのすぐあとも、見えている行が組み立ててあること"
    );
    pump_until(Duration::from_millis(500), || false);
    assert_eq!(
        missing_on_screen(),
        Vec::<usize>::new(),
        "見えている行が組み立ててあること"
    );

    // 次のキーもその行から続く。
    press_nav_key(&[], VK_UP)?;
    assert_eq!(list.selection(), [299], "↑ で手前の選べる行へ");
    assert_eq!(focused_list_row(list), Some(299), "↑ のフォーカス");
    Some(())
}

fn keys_over_long_unknown_runs_scenario(
    hwnd: windows::Win32::Foundation::HWND,
    list: &impl RowsForInput,
) -> Option<()> {
    use windows::Win32::UI::Input::KeyboardAndMouse::{VK_DOWN, VK_END, VK_UP};

    let scroll = list.scroller();
    let height = list.row_height();
    let on_screen = |index: usize| {
        let top = scroll.VerticalOffset().expect("位置");
        let bottom = top + scroll.ViewportHeight().expect("高さ");
        index as f64 * height >= top - 1.0 && (index + 1) as f64 * height <= bottom + 1.0
    };
    let check = |expected: usize, what: &str| {
        assert_eq!(list.selection(), [expected], "{what}: 選択");
        assert_eq!(focused_list_row(list), Some(expected), "{what}: フォーカス");
        assert!(
            on_screen(expected),
            "{what}: {expected} 行目が見えていること"
        );
    };

    // 先へ: 組み立てた行の端を越えて続く。
    list.scroll_to_row(19);
    pump_until(Duration::from_millis(500), || false);
    click_list_row_expecting(hwnd, list, 19, &[], &[19], "長い行の手前")?;
    press_nav_key(&[], VK_DOWN)?;
    check(301, "↓ で先の選べる行へ");
    press_nav_key(&[], VK_UP)?;
    check(19, "↑ で手前の選べる行へ");

    // 手前へ: まだ組み立てていない行を越える。
    list.scroll_to_row(481);
    pump_until(Duration::from_millis(500), || false);
    click_list_row_expecting(hwnd, list, 481, &[], &[481], "手前へ越える行")?;
    press_nav_key(&[], VK_UP)?;
    check(349, "↑ で手前の選べる行へ (未構築)");

    // 最後まで選べない行が続くときは、その手前で止まる。
    press_nav_key(&[], VK_END)?;
    check(899, "End は選べる最後の行へ");
    press_nav_key(&[], VK_DOWN)?;
    check(899, "その先に選べる行が無ければ動かない");
    Some(())
}

fn keys_over_unselectable_rows_scenario(
    hwnd: windows::Win32::Foundation::HWND,
    list: &impl RowsForInput,
) -> Option<()> {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        VK_CONTROL, VK_DOWN, VK_END, VK_HOME, VK_NEXT, VK_SHIFT, VK_SPACE, VK_UP,
    };

    const ROWS: usize = UNSELECTABLE_ROWS;
    first_click_on_list_row(hwnd, list, 2)?;
    press_nav_key(&[], VK_DOWN)?;
    assert_eq!(list.selection(), [4], "↓ は選べない行を飛ばす");
    assert_eq!(focused_list_row(list), Some(4), "フォーカスも飛ばす");
    press_nav_key(&[], VK_UP)?;
    assert_eq!(list.selection(), [2], "↑ も選べない行を飛ばす");
    press_nav_key(&[], VK_HOME)?;
    assert_eq!(list.selection(), [1], "Home は選べる最初の行へ");

    // Ctrl はフォーカスだけなので止まれる。その上の Space は選択を変えない。
    press_nav_key(&[VK_CONTROL], VK_DOWN)?;
    press_nav_key(&[VK_CONTROL], VK_DOWN)?;
    assert_eq!(
        focused_list_row(list),
        Some(3),
        "Ctrl は選べない行にも止まる"
    );
    assert_eq!(list.selection(), [1], "選択は変わらない");
    press_nav_key(&[], VK_SPACE)?;
    assert_eq!(list.selection(), [1], "選べない行の上の Space は何もしない");
    press_nav_key(&[], VK_DOWN)?;
    assert_eq!(list.selection(), [4], "そこから次の選べる行へ");

    // Shift も止まらない。範囲に選べない行は入らない。
    click_list_row_expecting(hwnd, list, 2, &[], &[2], "Shift の前の選択")?;
    press_nav_key(&[VK_SHIFT], VK_DOWN)?;
    assert_eq!(list.selection(), [2, 4], "Shift + ↓");
    assert_eq!(focused_list_row(list), Some(4), "Shift + ↓ のフォーカス");

    // 窓より長く続く選べない行も越える。
    list.scroll_to_row(19);
    pump_until(Duration::from_millis(500), || false);
    click_list_row_expecting(hwnd, list, 19, &[], &[19], "長い選べない行の手前")?;
    press_nav_key(&[], VK_DOWN)?;
    assert_eq!(list.selection(), [81], "窓より先の選べる行へ");
    assert_eq!(focused_list_row(list), Some(81), "移った行にフォーカス");
    press_nav_key(&[], VK_UP)?;
    assert_eq!(list.selection(), [19], "手前へも越える");
    press_nav_key(&[], VK_NEXT)?;
    assert!(
        is_selectable_in_scenario(list.selection()[0]),
        "Page Down も選べる行で止まる"
    );

    // 端の行が選べないときは、その手前で止まる。
    press_nav_key(&[], VK_END)?;
    assert_eq!(list.selection(), [ROWS - 2], "End は選べる最後の行へ");
    press_nav_key(&[], VK_DOWN)?;
    assert_eq!(
        list.selection(),
        [ROWS - 2],
        "その先に選べる行が無ければ動かない"
    );
    assert_eq!(
        focused_list_row(list),
        Some(ROWS - 2),
        "フォーカスも動かない"
    );
    Some(())
}

/// Tab lands on a row on screen, so the next key does not pull the view elsewhere: focus
/// changes do not scroll a list that scrolls itself.
fn list_tab_focuses_a_row_on_screen(ui: &Ui) -> Result<()> {
    let list = ui.list()?;
    list.set_items(&naui_core::ListItem::list(
        (0..1_000).map(|index| format!("行 {index}")),
    ));
    list.set_sizing(Sizing::fill());
    tab_focus_scenario(ui, &list)
}

/// [`list_tab_focuses_a_row_on_screen`] on a `Table`.
fn table_tab_focuses_a_row_on_screen(ui: &Ui) -> Result<()> {
    let table = ui.table()?;
    table.set_columns(&[naui_core::TableColumn::new("名前")]);
    table.set_row_builder(1_000, |index| {
        Ok(naui_windows::TableCells::new().text(format!("行 {index}")))
    });
    table.set_sizing(Sizing::fill());
    tab_focus_scenario(ui, &table)
}

fn tab_focus_scenario(ui: &Ui, list: &(impl RowsForInput + Widget)) -> Result<()> {
    use naui_winui3::Microsoft::UI::Xaml::Controls::Control;
    use naui_winui3::Microsoft::UI::Xaml::FocusState;
    use windows::Win32::Foundation::POINT;
    use windows::Win32::UI::Input::KeyboardAndMouse::{VK_DOWN, VK_TAB};
    use windows::Win32::UI::WindowsAndMessaging::{GetCursorPos, SetCursorPos};

    let window = ui.window("Tab で行へ", 300.0, 280.0)?;
    let stack = ui.stack(Orientation::Vertical)?;
    let button = ui.button("手前")?;
    stack.set_sizing(Sizing::fill());
    stack.append(&button);
    stack.append(list);
    window.set_child(&stack);
    let mut pointer = POINT::default();
    // SAFETY: `pointer` はこの関数の中で生きている。
    let _ = unsafe { GetCursorPos(&mut pointer) };
    let outcome = catch_unwind(AssertUnwindSafe(|| -> Option<()> {
        window.show();
        let scroll = list.scroller();
        pump_until(SCROLL_LAYOUT_TIMEOUT, || {
            scroll.ViewportHeight().unwrap_or(0.0) > 0.0 && list.row_height() > 0.0
        });
        if scroll.ViewportHeight().unwrap_or(0.0) <= 0.0 {
            println!("     (ScrollViewer に実寸が付かない環境なので、確かめずに通す)");
            return None;
        }
        bring_to_front(&window)?;
        pump_until(Duration::from_millis(500), || false);
        list.scroll_to_row(500);
        pump_until(Duration::from_millis(500), || false);
        let before = scroll.VerticalOffset().expect("位置");
        let height = list.row_height();
        let on_screen = |index: usize| {
            let top = scroll.VerticalOffset().expect("位置");
            let bottom = top + scroll.ViewportHeight().expect("高さ");
            (index + 1) as f64 * height > top && (index as f64) * height < bottom
        };

        let _ = native::<XamlButton>(&button).Focus(FocusState::Keyboard);
        pump_until(FOCUS_TIMEOUT, || {
            native::<Control>(&button).FocusState() != Ok(FocusState::Unfocused)
        });
        press_nav_key(&[], VK_TAB)?;
        // 直前のケースでキーが行を動かしていると、`GotFocus` も遅れて届く。
        pump_until(SCROLL_LAYOUT_TIMEOUT, || {
            focused_list_row(list).is_some_and(on_screen)
        });
        let Some(focused) = focused_list_row(list) else {
            println!("     (Tab で行へ入らない環境なので、確かめずに通す)");
            return None;
        };
        assert!(
            on_screen(focused),
            "Tab で画面の中の行へ入ること: {focused} 行目"
        );
        let after_tab = scroll.VerticalOffset().expect("位置");
        assert!((after_tab - before).abs() < 1.0, "Tab で表示が動かないこと");

        press_nav_key(&[], VK_DOWN)?;
        let after_key = scroll.VerticalOffset().expect("位置");
        assert!(
            (after_key - before).abs() <= height + 1.0,
            "次のキーで表示が飛ばないこと: {before} → {after_key}"
        );
        assert_eq!(list.selection(), [focused + 1], "その行から続くこと");
        Some(())
    }));
    // SAFETY: 引数は値だけ。
    let _ = unsafe { SetCursorPos(pointer.x, pointer.y) };
    window.close();
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
    Ok(())
}

/// [`list_refresh_inside_the_builder_starts_over`] on a `Table`.
fn table_refresh_inside_the_builder_starts_over(ui: &Ui) -> Result<()> {
    use naui_windows::TableCells;
    use naui_winui3::Microsoft::UI::Xaml::Automation::AutomationProperties;

    let window = ui.window("表の組み立て中の refresh", 240.0, 160.0)?;
    let table = ui.table()?;
    table.set_columns(&[naui_core::TableColumn::new("名前")]);
    let version = Rc::new(Cell::new(1));
    let refresh_once = Rc::new(Cell::new(false));
    {
        let inside = table.clone();
        let version = version.clone();
        let refresh_once = refresh_once.clone();
        table.set_row_builder(1_000, move |index| {
            let built_from = version.get();
            if refresh_once.replace(false) {
                version.set(2);
                inside.refresh();
            }
            Ok(TableCells::new().text(format!("{built_from}:{index}")))
        });
    }
    table.set_sizing(Sizing::fill());
    window.set_child(&table);
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        window.show();
        let scroll = table_scroller(&table);
        pump_until(SCROLL_LAYOUT_TIMEOUT, || {
            scroll.ViewportHeight().unwrap_or(0.0) > 0.0 && table.row_height() > 0.0
        });
        if scroll.ViewportHeight().unwrap_or(0.0) <= 0.0 {
            println!("     (ScrollViewer に実寸が付かない環境なので、確かめずに通す)");
            return;
        }
        refresh_once.set(true);
        table.refresh();
        let items = table.native_list_view().Items().expect("行");
        let count = items.Size().expect("行の数");
        assert!(count > 0, "行があること");
        for slot in 0..count {
            let row = items
                .GetAt(slot)
                .and_then(|item| item.cast::<ListViewItem>())
                .expect("行");
            let index = AutomationProperties::GetPositionInSet(&row).expect("位置") - 1;
            let text = row
                .Content()
                .and_then(|content| content.cast::<Grid>())
                .and_then(|grid| grid.Children())
                .and_then(|children| children.GetAt(0))
                .and_then(|cell| cell.cast::<TextBlock>())
                .and_then(|cell| cell.Text())
                .unwrap_or_default()
                .to_string();
            assert_eq!(text, format!("2:{index}"), "新しい内容で組み立て直すこと");
        }
    }));
    window.close();
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
    Ok(())
}

/// A `refresh` from inside the builder makes the build under way start over, so no row keeps
/// what it was built from before.
fn list_refresh_inside_the_builder_starts_over(ui: &Ui) -> Result<()> {
    use naui_windows::ListRow;
    use naui_winui3::Microsoft::UI::Xaml::Automation::AutomationProperties;

    let window = ui.window("組み立て中の refresh", 240.0, 160.0)?;
    let list = ui.list()?;
    let version = Rc::new(Cell::new(1));
    let refresh_once = Rc::new(Cell::new(false));
    {
        let ui = ui.clone();
        let list_inside = list.clone();
        let version = version.clone();
        let refresh_once = refresh_once.clone();
        list.set_row_builder(1_000, move |index| {
            let built_from = version.get();
            if refresh_once.replace(false) {
                version.set(2);
                list_inside.refresh();
            }
            Ok(ListRow::new(&ui.label(&format!("{built_from}:{index}"))?))
        });
    }
    list.set_sizing(Sizing::fill());
    window.set_child(&list);
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        window.show();
        let scroll = list_scroller(&list);
        pump_until(SCROLL_LAYOUT_TIMEOUT, || {
            scroll.ViewportHeight().unwrap_or(0.0) > 0.0 && list.row_height() > 0.0
        });
        if scroll.ViewportHeight().unwrap_or(0.0) <= 0.0 {
            println!("     (ScrollViewer に実寸が付かない環境なので、確かめずに通す)");
            return;
        }
        refresh_once.set(true);
        list.refresh();
        let items = list.native_list_view().Items().expect("行");
        let count = items.Size().expect("行の数");
        assert!(count > 0, "行があること");
        for slot in 0..count {
            let row = items
                .GetAt(slot)
                .and_then(|item| item.cast::<ListViewItem>())
                .expect("行");
            let index = AutomationProperties::GetPositionInSet(&row).expect("位置") - 1;
            let text = row
                .Content()
                .and_then(|content| content.cast::<Grid>())
                .and_then(|host| host.Children())
                .and_then(|children| children.GetAt(0))
                .and_then(|label| label.cast::<TextBlock>())
                .and_then(|label| label.Text())
                .unwrap_or_default()
                .to_string();
            assert_eq!(text, format!("2:{index}"), "新しい内容で組み立て直すこと");
        }
    }));
    window.close();
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
    Ok(())
}

/// [`list_keeps_the_rows_a_failed_build_left`] on a `Table`.
fn table_keeps_the_rows_a_failed_build_left(ui: &Ui) -> Result<()> {
    use naui_windows::TableCells;
    use naui_winui3::Microsoft::UI::Xaml::Automation::AutomationProperties;
    use std::collections::HashMap;

    const ROWS: usize = 1_000;
    let window = ui.window("表の組み立ての失敗", 240.0, 160.0)?;
    let table = ui.table()?;
    table.set_columns(&[naui_core::TableColumn::new("名前")]);
    // ほかの親に載っているので、行へは入れられない。
    let holder = ui.stack(Orientation::Vertical)?;
    let outsider = ui.label("外")?;
    holder.append(&outsider);
    let first_bad = Rc::new(Cell::new(usize::MAX));
    let reached = Rc::new(Cell::new(false));
    let labels: Rc<RefCell<HashMap<usize, naui_windows::Label>>> = Rc::default();
    {
        let ui = ui.clone();
        let first_bad = first_bad.clone();
        let reached = reached.clone();
        let labels = labels.clone();
        table.set_row_builder(ROWS, move |index| {
            if index >= first_bad.get() {
                reached.set(true);
                return Ok(TableCells::new().cell(&outsider));
            }
            if let Some(label) = labels.borrow().get(&index) {
                return Ok(TableCells::new().cell(label));
            }
            let label = ui.label(&format!("{index}"))?;
            labels.borrow_mut().insert(index, label.clone());
            Ok(TableCells::new().cell(&label))
        });
    }
    table.set_sizing(Sizing::fill());
    window.set_child(&table);
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        window.show();
        let scroll = table_scroller(&table);
        pump_until(SCROLL_LAYOUT_TIMEOUT, || {
            scroll.ViewportHeight().unwrap_or(0.0) > 0.0 && table.row_height() > 0.0
        });
        if scroll.ViewportHeight().unwrap_or(0.0) <= 0.0 {
            println!("     (ScrollViewer に実寸が付かない環境なので、確かめずに通す)");
            return;
        }
        let rows = || table.native_list_view().Items().expect("行");
        let row_at = |slot: u32| {
            rows()
                .GetAt(slot)
                .and_then(|item| item.cast::<ListViewItem>())
                .expect("行")
        };
        pump_until(Duration::from_millis(500), || false);
        let built_end = rows().Size().expect("行の数") as usize;

        // 窓が動いて増える行で失敗する。窓に残る行があるところまでだけ動かす。
        first_bad.set(built_end);
        table.scroll_to_row(built_end + 1);
        pump_until(SCROLL_LAYOUT_TIMEOUT, || reached.get());
        assert!(reached.get(), "増える行を組み立てようとしたこと");
        pump_until(Duration::from_millis(300), || false);
        let first_kept =
            AutomationProperties::GetPositionInSet(&row_at(0)).expect("位置") as usize - 1;

        // 載せられなかった行は空の行になるので、行が欠けず、番号が順に並ぶ。
        for slot in 0..rows().Size().expect("行の数") {
            assert_eq!(
                AutomationProperties::GetPositionInSet(&row_at(slot)).expect("位置") as usize - 1,
                first_kept + slot as usize,
                "行が欠けないこと"
            );
        }
        assert!(
            built_list_row(&table, built_end).is_some(),
            "失敗した行も行として残ること"
        );

        // 残った行は管理されたままなので、選択が届く。
        table.set_selection(&[first_kept]);
        assert!(
            row_at(0).IsSelected().expect("IsSelected"),
            "失敗のあとも残った行へ選択が届くこと ({first_kept} 行目)"
        );

        // 失敗の原因が無くなると、同じウィジェットの行を組み立て直せる。
        first_bad.set(usize::MAX);
        table.refresh();
        pump_until(SCROLL_LAYOUT_TIMEOUT, || {
            built_list_row(&table, built_end).is_some()
        });
        let count = rows().Size().expect("行の数");
        assert!(count > 0, "行があること");
        for slot in 0..count {
            let row = row_at(slot);
            let index = AutomationProperties::GetPositionInSet(&row).expect("位置") - 1;
            let text = row
                .Content()
                .and_then(|content| content.cast::<Grid>())
                .and_then(|host| host.Children())
                .and_then(|children| children.GetAt(0))
                .and_then(|label| label.cast::<TextBlock>())
                .and_then(|label| label.Text())
                .unwrap_or_default()
                .to_string();
            assert_eq!(text, index.to_string(), "{index} 行目の中身があること");
        }
    }));
    window.close();
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
    Ok(())
}

/// A failed build leaves the rows in the `ListView` looked after: their selection still
/// shows, and the widgets the app keeps can go into new rows on the next build.
fn list_keeps_the_rows_a_failed_build_left(ui: &Ui) -> Result<()> {
    use naui_windows::ListRow;
    use naui_winui3::Microsoft::UI::Xaml::Automation::AutomationProperties;
    use std::collections::HashMap;

    const ROWS: usize = 1_000;
    let window = ui.window("組み立ての失敗", 240.0, 160.0)?;
    let list = ui.list()?;
    // ほかの親に載っているので、行へは入れられない。
    let holder = ui.stack(Orientation::Vertical)?;
    let outsider = ui.label("外")?;
    holder.append(&outsider);
    let first_bad = Rc::new(Cell::new(usize::MAX));
    let reached = Rc::new(Cell::new(false));
    let labels: Rc<RefCell<HashMap<usize, naui_windows::Label>>> = Rc::default();
    {
        let ui = ui.clone();
        let first_bad = first_bad.clone();
        let reached = reached.clone();
        let labels = labels.clone();
        list.set_row_builder(ROWS, move |index| {
            if index >= first_bad.get() {
                reached.set(true);
                return Ok(ListRow::new(&outsider));
            }
            if let Some(label) = labels.borrow().get(&index) {
                return Ok(ListRow::new(label));
            }
            let label = ui.label(&format!("{index}"))?;
            labels.borrow_mut().insert(index, label.clone());
            Ok(ListRow::new(&label))
        });
    }
    list.set_sizing(Sizing::fill());
    window.set_child(&list);
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        window.show();
        let scroll = list_scroller(&list);
        pump_until(SCROLL_LAYOUT_TIMEOUT, || {
            scroll.ViewportHeight().unwrap_or(0.0) > 0.0 && list.row_height() > 0.0
        });
        if scroll.ViewportHeight().unwrap_or(0.0) <= 0.0 {
            println!("     (ScrollViewer に実寸が付かない環境なので、確かめずに通す)");
            return;
        }
        let rows = || list.native_list_view().Items().expect("行");
        let row_at = |slot: u32| {
            rows()
                .GetAt(slot)
                .and_then(|item| item.cast::<ListViewItem>())
                .expect("行")
        };
        let built_end = rows().Size().expect("行の数") as usize;
        pump_until(SCROLL_LAYOUT_TIMEOUT, || {
            rows().Size().unwrap_or(0) as usize == built_end
        });

        // 窓が動いて増える行で失敗する。窓に残る行があるところまでだけ動かす。
        first_bad.set(built_end);
        list.scroll_to_row(built_end + 1);
        pump_until(SCROLL_LAYOUT_TIMEOUT, || reached.get());
        assert!(reached.get(), "増える行を組み立てようとしたこと");
        let first_kept =
            AutomationProperties::GetPositionInSet(&row_at(0)).expect("位置") as usize - 1;

        // 載せられなかった行は空の行になるので、行が欠けず、番号が順に並ぶ。
        for slot in 0..rows().Size().expect("行の数") {
            assert_eq!(
                AutomationProperties::GetPositionInSet(&row_at(slot)).expect("位置") as usize - 1,
                first_kept + slot as usize,
                "行が欠けないこと"
            );
        }
        assert!(
            built_list_row(&list, built_end).is_some(),
            "失敗した行も行として残ること"
        );

        // 残った行は管理されたままなので、選択が届く。
        list.set_selection(&[first_kept]);
        assert!(
            row_at(0).IsSelected().expect("IsSelected"),
            "失敗のあとも残った行へ選択が届くこと"
        );

        // 失敗の原因が無くなると、同じウィジェットの行を組み立て直せる。
        first_bad.set(usize::MAX);
        list.refresh();
        pump_until(SCROLL_LAYOUT_TIMEOUT, || {
            built_list_row(&list, built_end).is_some()
        });
        let count = rows().Size().expect("行の数");
        assert!(count > 0, "行があること");
        for slot in 0..count {
            let row = row_at(slot);
            let index = AutomationProperties::GetPositionInSet(&row).expect("位置") - 1;
            let text = row
                .Content()
                .and_then(|content| content.cast::<Grid>())
                .and_then(|host| host.Children())
                .and_then(|children| children.GetAt(0))
                .and_then(|label| label.cast::<TextBlock>())
                .and_then(|label| label.Text())
                .unwrap_or_default()
                .to_string();
            assert_eq!(text, index.to_string(), "{index} 行目の中身があること");
        }
    }));
    window.close();
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
    Ok(())
}
