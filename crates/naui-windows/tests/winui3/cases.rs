//! The body of the behavior checks. Compiled only on Windows (see
//! `tests/winui3.rs`).

use std::cell::{Cell, RefCell};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use naui_core::{
    Align, Color, GridCell, MenuItem, MenuShortcut, MenuSpec, Orientation, Point, Rect, Result,
    ScrollPolicy, SidebarItem, SidebarSection, Sizing, TextColor, TextStyle, ToolbarIcon,
    WindowState, DEFAULT_SIDEBAR_WIDTH,
};
use naui_windows::{run_for_test, Ui, Widget};

use crate::automation;
use naui_winui3::Microsoft::UI::Xaml::Controls::{
    Button as XamlButton, Canvas as XamlCanvas, CheckBox as XamlCheckBox, ComboBox as XamlComboBox,
    Grid, Image as XamlImage, ScrollViewer, Slider as XamlSlider, StackPanel, TextBlock, TextBox,
    ToggleSwitch,
};
use naui_winui3::Microsoft::UI::Xaml::Markup::XamlReader;
use naui_winui3::Microsoft::UI::Xaml::Media::Imaging::BitmapImage;
use naui_winui3::Microsoft::UI::Xaml::Media::SolidColorBrush;
use naui_winui3::Microsoft::UI::Xaml::{
    FrameworkElement, GridUnitType, HorizontalAlignment, RoutedEventHandler, UIElement,
    VerticalAlignment,
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
        "大きさを変えられるかどうかが OverlappedPresenter と往復する",
        window_resizable_round_trips,
    ),
    (
        "表示する前に大きさを固定しても表示した後に効いている",
        window_resizable_requested_before_show,
    ),
    (
        "TextArea の改行は \\n で読める",
        text_area_reports_newlines_as_lf,
    ),
    (
        "描画面の命令が XAML の Path と TextBlock になり、読み込める",
        canvas_commands_become_xaml_shapes,
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
        "メニューバーの項目がインデックスの組で通知する",
        menu_bar_activation_notifies,
    ),
    (
        "サイドバーが NavigationView の項目・見出し・区切りになる",
        sidebar_items_map_to_native,
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
        "スクロールの位置を測り、送れる範囲に丸めて送る",
        scroll_measures_and_moves,
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
];

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
    Ok(())
}

/// The drawing surface places a `Canvas` inside a `Grid` and loads the `on_draw`
/// commands as XAML `Path`s and `TextBlock`s. It is not on screen, so its size is not
/// decided; the XAML built by `xaml_for_test` with a given size is read by WinUI,
/// and the number and kinds of elements are checked.
fn canvas_commands_become_xaml_shapes(ui: &Ui) -> Result<()> {
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
            painter.set_dash(&[4.0, 2.0]);
            painter.line(
                Point::new(0.0, 0.0),
                Point::new(50.0, 30.0),
                Color::BLACK,
                2.0,
            );
            painter.set_text_align(Align::Center);
            painter.text("naui <&>", Point::new(60.0, 40.0), 12.0, Color::BLACK);
            // `{` で始まる文字はマークアップ拡張と読まれて面ごと壊れるので、
            // 文字のまま通ることを見る。
            painter.text("{Binding}", Point::new(60.0, 60.0), 12.0, Color::BLACK);
        }
    });

    let xaml = canvas.xaml_for_test(120.0, 80.0);
    assert_eq!(calls.get(), 1, "面の大きさで on_draw が呼ばれること");
    let scene: XamlCanvas = XamlReader::Load(&HSTRING::from(xaml.as_str()))
        .and_then(|element| element.cast::<XamlCanvas>())
        .unwrap_or_else(|e| panic!("組み立てた XAML を WinUI が読めること: {e}\n{xaml}"));
    let children = scene.Children().expect("子");
    assert_eq!(
        children.Size().unwrap_or(0),
        4,
        "塗り・線・文字 2 つの 4 要素"
    );

    let root: FrameworkElement = scene.cast().expect("FrameworkElement");
    assert!(
        (root.Width().unwrap_or(0.0) - 120.0).abs() < 0.5
            && (root.Height().unwrap_or(0.0) - 80.0).abs() < 0.5,
        "面の大きさが Canvas に付くこと"
    );
    let text: TextBlock = root
        .FindName(&HSTRING::from("t2"))
        .and_then(|value| value.cast())
        .expect("文字の TextBlock");
    assert_eq!(
        text.Text().map(|t| t.to_string()).as_deref(),
        Ok("naui <&>")
    );
    assert!(
        (text.FontSize().unwrap_or(0.0) - 12.0).abs() < 0.01,
        "文字の大きさが FontSize に写ること"
    );
    let braced: TextBlock = root
        .FindName(&HSTRING::from("t3"))
        .and_then(|value| value.cast())
        .expect("`{` で始まる文字の TextBlock");
    assert_eq!(
        braced.Text().map(|t| t.to_string()).as_deref(),
        Ok("{Binding}"),
        "`{{}}` の印は表示には出ず、文字がそのまま残ること"
    );
    Ok(())
}

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

    let first = menu_bar.native_item(0, 0).expect("先頭は項目");
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
    assert!(menu_bar.native_item(0, 1).is_none(), "区切り線に項目は無い");

    let save = menu_bar.native_item(0, 2).expect("3 番目は項目");
    assert!(!save.IsEnabled().expect("有効かどうか"));
    assert_eq!(
        save.KeyboardAcceleratorTextOverride()
            .expect("右端の表示")
            .to_string(),
        "Ctrl+Shift+S"
    );
    assert!(menu_bar.native_item(9, 0).is_none(), "範囲外は None");
    assert!(menu_bar.native_item(0, 9).is_none(), "範囲外は None");
    assert!(menu_bar.native_flyout(9).is_none(), "範囲外は None");

    // ショートカットを指定しなければ右端には何も出ない。
    assert_eq!(
        menu_bar
            .native_item(1, 0)
            .expect("項目")
            .KeyboardAcceleratorTextOverride()
            .expect("右端の表示")
            .to_string(),
        ""
    );

    assert!(menu_bar.is_item_enabled(0, 0));
    assert!(!menu_bar.is_item_enabled(0, 1), "区切り線は押せない");
    assert!(!menu_bar.is_item_enabled(0, 2));

    // 項目ごとの指定と全体の指定は AND を取る。
    menu_bar.set_item_enabled(0, 2, true);
    assert!(menu_bar
        .native_item(0, 2)
        .expect("項目")
        .IsEnabled()
        .expect("有効かどうか"));
    menu_bar.set_enabled(false);
    assert!(!menu_bar.is_item_enabled(0, 0));
    assert!(!menu_bar
        .native_item(0, 0)
        .expect("項目")
        .IsEnabled()
        .expect("有効かどうか"));
    menu_bar.set_enabled(true);
    assert!(
        menu_bar.is_item_enabled(0, 2),
        "全体を戻すと項目ごとの指定が残る"
    );

    // 区切り線への set_item_enabled は無視する。
    menu_bar.set_item_enabled(0, 1, true);
    assert!(!menu_bar.is_item_enabled(0, 1));

    menu_bar.set_menus(&[]);
    assert!(menu_bar.is_empty());
    assert!(menu_bar.native_item(0, 0).is_none());
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
    menu_bar.on_activate(move |menu, item| sink.borrow_mut().push((menu, item)));

    menu_bar.activate(0, 0);
    assert_eq!(seen.borrow().as_slice(), [(0, 0)].as_slice());
    menu_bar.activate(0, 1);
    menu_bar.activate(0, 2);
    menu_bar.activate(9, 0);
    menu_bar.activate(0, 9);
    assert_eq!(
        seen.borrow().as_slice(),
        [(0, 0)].as_slice(),
        "区切り線・押せない項目・範囲外は通知しない"
    );

    menu_bar.activate(1, 0);
    assert_eq!(seen.borrow().as_slice(), [(0, 0), (1, 0)].as_slice());

    menu_bar.set_item_enabled(0, 2, true);
    menu_bar.activate(0, 2);
    assert_eq!(
        seen.borrow().as_slice(),
        [(0, 0), (1, 0), (0, 2)].as_slice()
    );

    menu_bar.set_enabled(false);
    menu_bar.activate(0, 0);
    assert_eq!(
        seen.borrow().as_slice(),
        [(0, 0), (1, 0), (0, 2)].as_slice(),
        "無効なメニューバーは通知しない"
    );
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
    assert_eq!(
        native.PaneDisplayMode().expect("表示形式"),
        NavigationViewPaneDisplayMode::Left
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
            let bitmap = native::<Grid>(&image)
                .Children()
                .and_then(|children| children.GetAt(0))
                .and_then(|element| element.cast::<XamlImage>())
                .and_then(|element| element.Source())
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
