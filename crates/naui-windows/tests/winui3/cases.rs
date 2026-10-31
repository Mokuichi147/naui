//! The body of the behavior checks. Compiled only on Windows (see
//! `tests/winui3.rs`).

use std::cell::{Cell, RefCell};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use naui_core::{
    Align, ButtonRole, Color, GridCell, MenuItem, MenuShortcut, MenuSpec, Orientation, Point, Rect,
    Result, ScrollPolicy, SidebarItem, SidebarSection, Sizing, TextColor, TextStyle, ToolbarIcon,
    WindowPlacement, WindowState, DEFAULT_SIDEBAR_WIDTH,
};
use naui_windows::{run_for_test, MenuEntry, Ui, Widget};

use crate::automation;
use naui_winui3::Microsoft::UI::Xaml::Controls::{
    Button as XamlButton, Canvas as XamlCanvas, CheckBox as XamlCheckBox, ComboBox as XamlComboBox,
    Grid, Image as XamlImage, MenuFlyoutItem, ProgressRing, ScrollViewer, Slider as XamlSlider,
    StackPanel, TextBlock, TextBox, ToggleSwitch,
};
use naui_winui3::Microsoft::UI::Xaml::Media::Imaging::BitmapImage;
use naui_winui3::Microsoft::UI::Xaml::Media::SolidColorBrush;
use naui_winui3::Microsoft::UI::Xaml::{
    Application, FrameworkElement, GridUnitType, HorizontalAlignment, RoutedEventHandler, Style,
    UIElement, VerticalAlignment,
};
use windows::Foundation::{IPropertyValue, PropertyValue};
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
/// up. After allowing time for every case, it always ends.
const WATCHDOG: Duration = Duration::from_secs(180);

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

/// `size` and `on_resize` are the size of the content row, excluding the title bar
/// band naui draws and the menu bar (the same as macOS's `contentView` and the area
/// below GTK's header bar). `Window.Bounds` includes what is behind the title bar,
/// so as is it differs.
///
/// Messages are pumped until the row has an actual size. In environments where it
/// never gets one (CI), it passes without checking.
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
const SCROLL_LAYOUT_TIMEOUT: Duration = Duration::from_secs(3);

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
        let children = native::<Grid>(&table).Children().expect("表の子");
        let scroll = (0..children.Size().expect("表の子の数"))
            .find_map(|i| children.GetAt(i).ok()?.cast::<ScrollViewer>().ok())
            .expect("表の ScrollViewer");
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
