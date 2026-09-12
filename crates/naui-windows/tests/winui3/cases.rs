//! 動作確認の本体。Windows でだけコンパイルされる (`tests/winui3.rs` を参照)。

use std::cell::{Cell, RefCell};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use naui_core::{Align, Orientation, Result, Sizing, TextColor, TextStyle};
use naui_windows::{run_for_test, Ui, Widget};

use crate::automation;
use naui_winui3::Microsoft::UI::Xaml::Controls::{
    Button as XamlButton, CheckBox as XamlCheckBox, ComboBox as XamlComboBox, Grid, ScrollViewer,
    Slider as XamlSlider, StackPanel, TextBlock, TextBox, ToggleSwitch,
};
use naui_winui3::Microsoft::UI::Xaml::Media::SolidColorBrush;
use naui_winui3::Microsoft::UI::Xaml::{FrameworkElement, HorizontalAlignment, UIElement};
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
        "スクロールの非スクロール軸が内容を広げる",
        scroll_stretches_non_scrolling_content,
    ),
    ("スタックが子を生かし続ける", stack_keeps_children),
    (
        "ラベルの付くウィジェットが読み上げ名を持つ",
        widgets_expose_accessible_names,
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

const ASYNC_CASES: &[AsyncCase] = &[("ウィンドウが出て、閉じると消える", window_lifecycle)];

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
