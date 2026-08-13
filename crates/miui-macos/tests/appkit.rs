//! AppKit の実コントロールに対する動作確認。
//!
//! `performClick` などネイティブ側の操作を発生させ、Rust のクロージャへ
//! 届くこと・ネイティブの状態が変わることを確かめる。
//!
//! AppKit はメインスレッドでしか触れないが、Rust の標準テストハーネスは
//! 各テストを別スレッドで走らせる (`--test-threads=1` でも同じ)。
//! そのため `harness = false` にして、自前のランナーをメインスレッドで回す。

use std::cell::RefCell;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::rc::Rc;

use miui_core::{Orientation, Padding, Result};
use miui_macos::{run_for_test, Ui, Widget};

/// テストケース 1 件。
type Case = (&'static str, fn(&Ui) -> Result<()>);

fn main() {
    let cases: &[Case] = &[
        ("ボタンのクリックがクロージャへ届く", button_click),
        ("チェックボックスが反転し新しい値を通知する", checkbox_toggle),
        ("文字列がネイティブと往復する (日本語含む)", text_round_trip),
        ("スライダーが範囲でクランプされる", slider_clamp),
        ("進捗バーが 0..1 に収まる", progress_clamp),
        ("スタックが子を生かし続ける", stack_keeps_children),
        ("ウィンドウを設定して閉じられる", window_lifecycle),
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

/// スタックへ追加した子は、ハンドルを捨ててもコールバックが生き続ける。
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
    assert_eq!(subviews.len(), 1, "NSStackView に 1 つ並んでいること");
    let control = subviews
        .objectAtIndex(0)
        .downcast::<objc2_app_kit::NSButton>()
        .expect("NSButton であること");
    unsafe { control.performClick(None) };

    assert_eq!(
        *hits.borrow(),
        1,
        "ハンドルを捨てた後もクロージャが呼ばれること"
    );
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
