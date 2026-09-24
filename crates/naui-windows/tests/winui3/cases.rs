//! 動作確認の本体。Windows でだけコンパイルされる (`tests/winui3.rs` を参照)。

use std::cell::{Cell, RefCell};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use naui_core::{
    Align, Color, GridCell, MenuItem, MenuShortcut, MenuSpec, Orientation, Point, Rect, Result,
    ScrollPolicy, SidebarItem, SidebarSection, Sizing, TextColor, TextStyle, ToolbarIcon,
    DEFAULT_SIDEBAR_WIDTH,
};
use naui_windows::{run_for_test, Ui, Widget};

use crate::automation;
use naui_winui3::Microsoft::UI::Xaml::Controls::{
    Button as XamlButton, Canvas as XamlCanvas, CheckBox as XamlCheckBox, ComboBox as XamlComboBox,
    Grid, ScrollViewer, Slider as XamlSlider, StackPanel, TextBlock, TextBox, ToggleSwitch,
};
use naui_winui3::Microsoft::UI::Xaml::Markup::XamlReader;
use naui_winui3::Microsoft::UI::Xaml::Media::SolidColorBrush;
use naui_winui3::Microsoft::UI::Xaml::{
    FrameworkElement, HorizontalAlignment, UIElement, VerticalAlignment,
};
use windows::Foundation::{IPropertyValue, PropertyValue};
use windows_core::{Interface, HSTRING};

/// テストケース 1 件。
type Case = (&'static str, fn(&Ui) -> Result<()>);

/// 走らせる順。ウィンドウを作るものは、ほかのケースの結果が出そろってから
/// 走るように最後へ置く (画面の無い環境ではここが最初に崩れるため)。
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
];

/// あとで確かめる仕事。イベントループを 1 周まわしてから呼ばれる。
type Deferred = Box<dyn FnOnce() -> Result<()>>;

/// 仕込みと確認が 1 周ぶん離れるケース。
///
/// ウィンドウを出しても `Window.Visible` はその場では立たない。WinUI が
/// メッセージを一巡させてからでないと、出たかどうかを見られない。仕込みで
/// 出し、確認はループが回ってから行う。
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

/// `Application::Start` に入ったきり戻らないと、CI が打ち切るまで詰まる。
/// 全ケースぶんの余裕を取ったうえで、必ず終わらせる。
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
            report(name, catch_unwind(AssertUnwindSafe(|| case(ui))), &counter);
        }

        // 2 段構えのケースは、まず仕込みだけ済ませる。
        let mut deferred = Vec::new();
        for (name, setup) in ASYNC_CASES {
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

/// 1 件ぶんの結果を出す。落ちていたら数える。
///
/// ケースの `assert!` は `catch_unwind` で受け止める。1 つ落ちても残りは
/// 走らせる (アプリを起こし直せないので、打ち切ると以降が全部未実行になる)。
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

/// 実際に押したのと同じ経路 (Invoke パターン) でクリックする。
///
/// 経路の中身は [`crate::automation`]。WinUI がコントロールごとに用意する
/// peer を通すので、`Click` を上げるのは WinUI 自身になる。
fn invoke(widget: &dyn Widget) {
    automation::invoke(&widget.native_element());
}

/// 実際に押したのと同じ経路 (Toggle パターン) で入り切りを反転させる。
fn toggle(widget: &dyn Widget) {
    automation::toggle(&widget.native_element());
}

/// 実際に選んだのと同じ経路 (SelectionItem パターン) で項目を選ぶ。
fn select(element: &UIElement) {
    automation::select(element);
}

/// 支援技術へ渡る読み上げ名。
fn accessible_name(widget: &dyn Widget) -> String {
    automation::accessible_name(&widget.native_element())
}

/// ネイティブのコントロールとして取り出す。型が違えばテストを落とす。
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

/// ボタンの中身は `TextBlock`。ネイティブ側の文字列を読む。
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

/// 描画面は `Grid` の中に `Canvas` を置き、`on_draw` の命令を XAML の
/// `Path` と `TextBlock` にして読み込む。画面に出していないので大きさは
/// 決まらず、`xaml_for_test` で大きさを与えて組み立てた XAML を WinUI に
/// 読ませ、要素の数と種類を確かめる。
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

/// `set_style` / `set_color` は WinUI の type ramp とテーマリソースへ写る。
///
/// 級数も色も naui 側では持たないので、確かめるのは「Fluent が決めた値が
/// 入ってくるか」と「段階と役割を重ねても両方効くか」の 2 つ。
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

/// ツリーの行は、中身を行の幅いっぱいに置く `Style` を持つ。
///
/// WinUI の `TreeViewItem` は中身の横位置を `HorizontalContentAlignment` へ
/// 束ねている (既定のテンプレートの `ContentPresenter`)。これが無いと、文字が
/// 中身の幅までしか広がらず、行の残り幅を使わない。
///
/// 画面へ出していないので配られた幅は測れない。行へ `Style` が当たって
/// いることだけを見る。
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

/// `Align` は StackPanel 自身ではなく、交差軸に置く子へ適用する。
///
/// StackPanel 自身を `Left` にすると、`fill_width()` を指定した Stack まで
/// 内容幅へ縮み、中の要素が Stretch でも親の幅を受け取れない。
fn stack_alignment_keeps_fill_width(ui: &Ui) -> Result<()> {
    let stack = ui.stack(Orientation::Vertical)?;
    stack.set_align(Align::Start);

    let normal = ui.label("通常")?;
    let fill = ui.label("幅いっぱい")?;
    fill.set_sizing(Sizing::fill_width());
    stack.append(&normal);
    stack.append(&fill);

    let panel = native::<StackPanel>(&stack);
    assert_eq!(
        panel.HorizontalAlignment().expect("StackPanel の横配置"),
        HorizontalAlignment::Stretch,
        "StackPanel 自身は親の幅を受け取ること"
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

/// 子を追加してから寄せ方と大きさを変更しても、Stack の交差軸の寄せ方を
/// 保持する。Sizing はネイティブの Alignment を上書きするため、呼び出し順が
/// 逆になったときの回帰を検証する。
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

/// `ScrollViewer` の既定 (左上寄せ) では、横へ送らない中身がビューポートの
/// 幅を使えない。naui の既定ポリシーでは横軸を Stretch にする。
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
        native::<StackPanel>(&child)
            .HorizontalAlignment()
            .expect("スクロール内容の横配置"),
        HorizontalAlignment::Stretch,
        "中身自身も横へ広がること"
    );
    Ok(())
}

/// Scroll の中身を先に置いてから Sizing を変更しても、横スクロールを禁止した
/// 軸の Stretch を保つ。親の状態を子の Sizing 後にも再適用できることを検証する。
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
        native::<StackPanel>(&child)
            .HorizontalAlignment()
            .expect("サイズ変更後のスクロール内容の横配置"),
        HorizontalAlignment::Stretch,
        "Sizing を後から指定しても非スクロール軸の Stretch を保つこと"
    );
    Ok(())
}

/// すでに別の親に属している要素を追加できなくても、失敗した親の状態を
/// 記録しない。実際の親の状態が Sizing 後にも残ることを検証する。
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
        native::<StackPanel>(&scroll_child)
            .HorizontalAlignment()
            .expect("追加失敗後の Scroll 内容の横配置"),
        HorizontalAlignment::Stretch,
        "失敗した Scroll のポリシーを後から再適用しないこと"
    );
    Ok(())
}

/// すでに別の親に属している要素を Grid へ追加できなくても、Grid の添付
/// プロパティや既存の親が設定した Alignment を変更しない。
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

/// WinUI の脱出口から設定した Tag を、Sizing や親コンテナの操作で失わない。
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
    let scroll_native = native::<StackPanel>(&scroll_child)
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

/// Stack が交差軸へ設定した Alignment を、子を外したときに元へ戻す。そう
/// しないと、次に Grid へ置いた要素へ前の Stack の寄せ方が残ってしまう。
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

    let children = native::<StackPanel>(&stack).Children().expect("子の一覧");
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

/// ウィンドウの根は「タイトルバー → メニューバー → 中身」の 3 行で、
/// トーストは中身の行 (3 行目) に重ねる。行を足したときに重ね先が古い番号の
/// ままだと、トーストがメニューバーの行 (`Auto`) に入って中身を押し下げる。
///
/// 重ね先はトーストを出したその場で決まるので、確かめるのは仕込みの中で
/// 済ませる。ウィンドウが閉じると後片づけ (`AppWindow` の `Closing`) が
/// 全ウィンドウの中身を外すので、一巡後 (ほかのケースがウィンドウを
/// 閉じたあと) では根の `Grid` を読めない。同じ理由で、このウィンドウも
/// 閉じずにアプリの終了に任せる。
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

// -------------------------------------------------------------- メニューバー

/// `StackPanel` に並んだ見出しの数。
fn title_count(menu_bar: &naui_windows::MenuBar) -> u32 {
    menu_bar
        .native_panel()
        .Children()
        .expect("見出しの入れ物")
        .Size()
        .expect("見出しの数")
}

/// 見出しは `Button` + `MenuFlyout`、項目は `MenuFlyoutItem` になる。
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

/// 押された項目は (見出し, 項目) のインデックスの組で届く。
///
/// `MenuFlyoutItem` の `Click` は、メニューを出さないと通せない (`MenuFlyout`
/// は画面に出てはじめて項目へ peer を用意する)。ここは naui 側の経路だけを
/// 見て、実際のクリックからの配線は macOS / GTK / Web の統合テストと、
/// Windows は Gallery の実行で確かめている。
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

/// サイドバーは中身の行 (`CONTENT_ROW`) の先頭に入り、子はその `Content` へ
/// 移る。外すと子が中身の行へ戻る。
///
/// トーストのケースと同じく、ウィンドウは閉じずにアプリの終了に任せる。
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
