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

use miui_core::{NavItem, Orientation, Padding, Result, Theme};
use miui_macos::{run_for_test, Ui, Widget};

/// テストケース 1 件。
type Case = (&'static str, fn(&Ui) -> Result<()>);

fn main() {
    let cases: &[Case] = &[
        ("ボタンのクリックがクロージャへ届く", button_click),
        (
            "チェックボックスが反転し新しい値を通知する",
            checkbox_toggle,
        ),
        ("文字列がネイティブと往復する (日本語含む)", text_round_trip),
        ("スライダーが範囲でクランプされる", slider_clamp),
        ("進捗バーが 0..1 に収まる", progress_clamp),
        ("スタックが子を生かし続ける", stack_keeps_children),
        ("ウィンドウを設定して閉じられる", window_lifecycle),
        ("ナビバーの選択がネイティブと往復する", navbar_selection),
        ("ドックが等幅の項目を持つ", dock_items),
        ("タブが中身ごと切り替わる", tabs_selection),
        ("メニューの選択が 1 つだけ点く", menu_selection),
        ("パンくずが末尾を現在地にする", breadcrumbs_path),
        ("ページ送りが範囲内に収まる", pagination_steps),
        ("リンクのクリックがクロージャへ届く", link_click),
        ("テーマを実行中に切り替えられる", theme_switch),
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

/// ナビバーは NSSegmentedControl の選択と往復し、選択を通知する。
fn navbar_selection(ui: &Ui) -> Result<()> {
    let navbar = ui.navbar("miui")?;
    assert_eq!(navbar.title(), "miui");
    navbar.set_items(&NavItem::list(["ホーム", "検索", "設定"]));
    assert_eq!(navbar.len(), 3);

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
    let link = ui.link("miui", "")?;
    assert_eq!(link.text(), "miui");
    assert_eq!(link.href(), "");

    let hits = Rc::new(RefCell::new(0));
    link.on_click({
        let hits = hits.clone();
        move || *hits.borrow_mut() += 1
    });

    link.click();
    link.click();
    assert_eq!(*hits.borrow(), 2);

    link.set_text("miui のリポジトリ");
    assert_eq!(link.text(), "miui のリポジトリ");
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
