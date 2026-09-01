//! 動作確認の本体。Windows でだけコンパイルされる (`tests/winui3.rs` を参照)。

use std::cell::{Cell, RefCell};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use naui_core::{Orientation, Result};
use naui_windows::{run_for_test, Ui, Widget};
use naui_winui3::Microsoft::UI::Xaml::Automation::Peers::{
    AutomationPeer, FrameworkElementAutomationPeer, PatternInterface,
};
use naui_winui3::Microsoft::UI::Xaml::Automation::Provider::{
    IInvokeProvider, ISelectionItemProvider, IToggleProvider, IValueProvider,
};
use naui_winui3::Microsoft::UI::Xaml::Controls::{
    Button as XamlButton, CheckBox as XamlCheckBox, ComboBox as XamlComboBox, Slider as XamlSlider,
    StackPanel, TextBlock, TextBox, ToggleSwitch,
};
use naui_winui3::Microsoft::UI::Xaml::UIElement;
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
    ("スタックが子を生かし続ける", stack_keeps_children),
    (
        "ラベルの付くウィジェットが読み上げ名を持つ",
        widgets_expose_accessible_names,
    ),
    ("ウィンドウを設定して閉じられる", window_lifecycle),
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
            // ケースの assert! はここで受け止める。1 つ落ちても残りは走らせる
            // (アプリを起こし直せないので、打ち切ると以降が全部未実行になる)。
            match catch_unwind(AssertUnwindSafe(|| case(ui))) {
                Ok(Ok(())) => println!("ok   ... {name}"),
                Ok(Err(error)) => {
                    println!("FAIL ... {name}: {error}");
                    counter.fetch_add(1, Ordering::Relaxed);
                }
                Err(_) => {
                    println!("FAIL ... {name}");
                    counter.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
        Ok(())
    });

    let failed = failed.load(Ordering::Relaxed);
    println!("\n{} 件中 {} 件成功", CASES.len(), CASES.len() - failed);
    if let Err(error) = outcome {
        eprintln!("アプリを起こせませんでした: {error}");
        std::process::exit(1);
    }
    if failed > 0 {
        std::process::exit(1);
    }
}

// ------------------------------------------------------------------ 補助

/// ネイティブの UI オートメーションから見たこのウィジェット。
///
/// 画面読み上げソフトや自動操作ツールが触るのと同じ口で、WinUI が
/// コントロールごとに用意する peer が応じる。
fn peer(element: &UIElement) -> AutomationPeer {
    FrameworkElementAutomationPeer::CreatePeerForElement(element)
        .expect("AutomationPeer を作れませんでした")
}

/// 実際に押したのと同じ経路 (Invoke パターン) でクリックする。
fn invoke(widget: &dyn Widget) {
    let pattern = peer(&widget.native_element())
        .GetPattern(PatternInterface::Invoke)
        .expect("Invoke パターンを取れませんでした");
    pattern
        .cast::<IInvokeProvider>()
        .expect("IInvokeProvider ではありません")
        .Invoke()
        .expect("Invoke に失敗しました");
}

/// 実際に押したのと同じ経路 (Toggle パターン) で入り切りを反転させる。
fn toggle(widget: &dyn Widget) {
    let pattern = peer(&widget.native_element())
        .GetPattern(PatternInterface::Toggle)
        .expect("Toggle パターンを取れませんでした");
    pattern
        .cast::<IToggleProvider>()
        .expect("IToggleProvider ではありません")
        .Toggle()
        .expect("Toggle に失敗しました");
}

/// 実際に選んだのと同じ経路 (SelectionItem パターン) で項目を選ぶ。
fn select(element: &UIElement) {
    let pattern = peer(element)
        .GetPattern(PatternInterface::SelectionItem)
        .expect("SelectionItem パターンを取れませんでした");
    pattern
        .cast::<ISelectionItemProvider>()
        .expect("ISelectionItemProvider ではありません")
        .Select()
        .expect("Select に失敗しました");
}

/// 実際に打ち込んだのと同じ経路 (Value パターン) で文字列を入れる。
fn type_text(widget: &dyn Widget, text: &str) {
    let pattern = peer(&widget.native_element())
        .GetPattern(PatternInterface::Value)
        .expect("Value パターンを取れませんでした");
    pattern
        .cast::<IValueProvider>()
        .expect("IValueProvider ではありません")
        .SetValue(&HSTRING::from(text))
        .expect("SetValue に失敗しました");
}

/// 支援技術へ渡る読み上げ名。
fn accessible_name(widget: &dyn Widget) -> String {
    peer(&widget.native_element())
        .GetName()
        .map(|name| name.to_string())
        .unwrap_or_default()
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
    let input = ui.text_input("")?;

    let seen = Rc::new(RefCell::new(String::new()));
    let sink = seen.clone();
    input.on_change(move |text| *sink.borrow_mut() = text.to_string());

    type_text(&input, "こんにちは naui");
    assert_eq!(input.text(), "こんにちは naui", "打った文字が読めること");
    assert_eq!(
        native::<TextBox>(&input)
            .Text()
            .expect("TextBox の文字列")
            .to_string(),
        "こんにちは naui",
        "ネイティブの TextBox にも入っていること"
    );
    assert_eq!(
        seen.borrow().as_str(),
        "こんにちは naui",
        "打鍵がクロージャへ届くこと"
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
    progress.set_value(0.5);
    assert!((progress.value() - 0.5).abs() < 0.001);
    progress.set_value(2.0);
    assert!((progress.value() - 1.0).abs() < 0.001, "上限で止まること");
    progress.set_value(-2.0);
    assert!((progress.value() - 0.0).abs() < 0.001, "下限で止まること");
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

fn window_lifecycle(ui: &Ui) -> Result<()> {
    let window = ui.window("テスト", 320.0, 240.0)?;
    assert_eq!(window.title(), "テスト");
    window.set_title("別の題");
    assert_eq!(window.title(), "別の題");

    let stack = ui.stack(Orientation::Vertical)?;
    stack.append(&ui.label("中身")?);
    window.set_child(&stack);

    window.close();
    assert!(!window.is_visible(), "閉じたら見えないこと");
    Ok(())
}
