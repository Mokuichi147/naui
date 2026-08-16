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

use miui_core::{
    FileFilter, FilePickerMode, Fit, GridCell, NavItem, Orientation, Padding, PlaybackState, Result,
    ScrollPolicy, Sizing, Theme, Track,
};
use miui_macos::{run_for_test, Ui, Widget};
use objc2::rc::Retained;
use objc2_app_kit::{NSButton, NSImageScaling, NSImageView, NSLayoutConstraint, NSView};
use objc2_foundation::{NSDate, NSRunLoop, NSSize};

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
        ("大きさの指定が制約として反映される", sizing_constraints),
        (
            "大きさを指定し直しても制約が積み上がらない",
            sizing_is_replaced,
        ),
        ("交差軸の Fill が親の幅に合わせて広がる", stack_fill_cross),
        ("スペーサーが余りを吸って後続を端へ寄せる", spacer_pushes),
        ("グリッドが行と列を広げて子を置く", grid_places_children),
        ("スクロールが中身を保持する", scroll_keeps_child),
        ("グリッドの同じ行が縦中央でそろう", grid_row_alignment),
        (
            "ファイル選択がボタンとして構成され設定を保つ",
            file_picker_configuration,
        ),
        (
            "ファイル選択のモードが NSOpenPanel へ反映される",
            file_picker_panel,
        ),
        (
            "編集メニューが貼り付けをレスポンダチェーンへ配送する",
            menu_bar_provides_edit_shortcuts,
        ),
        ("画像がローカルのファイルから読み込まれる", image_loads_a_local_file),
        (
            "収め方が NSImageView の imageScaling になる",
            image_fit_maps_to_native_scaling,
        ),
        ("音声が最後まで再生され Ended が届く", audio_plays_to_the_end),
        ("繰り返し再生が末尾で止まらない", audio_loops_back_to_the_start),
        ("再生位置の指定が AVPlayer に届く", media_seek_moves_the_position),
        ("音量と消音がネイティブと往復する", media_volume_round_trips),
        (
            "メディアのハンドルを捨てても落ちない",
            media_handles_can_be_dropped,
        ),
        (
            "再生位置が定期的にクロージャへ届く",
            media_reports_position_while_playing,
        ),
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

/// 大きさの指定が AppKit の制約になり、値まで届く。
fn sizing_constraints(ui: &Ui) -> Result<()> {
    let button = ui.button("固定")?;
    button.set_sizing(Sizing::fixed(160.0, 44.0).min_width(80.0));

    let view = button.native_view();
    let sizes = miui_constraints(&view);
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

/// 指定し直しても制約は積み上がらず、AppKit 内部の制約も壊さない。
fn sizing_is_replaced(ui: &Ui) -> Result<()> {
    let label = ui.label("文字")?;
    let view = label.native_view();
    // AppKit が内部で付ける制約 (intrinsic content size 用) の本数。
    let before = view.constraints().len();

    // 文字幅より広い値を順に指定する (狭めると圧縮抵抗と競合するため)。
    label.set_sizing(Sizing::fixed(200.0, 20.0));
    label.set_sizing(Sizing::fixed(220.0, 30.0));
    label.set_sizing(Sizing::fixed(240.0, 40.0));

    let mine = miui_constraints(&view);
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

/// 縦スタックの中で幅 Fill を指定した子は、余白を除いた親の幅まで広がる。
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

/// スペーサーが縦の余りをすべて受け取り、後ろの子を下端へ寄せる。
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

/// グリッドは必要な行と列を自分で増やし、子を保持する。
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
    assert!((x - 140.0).abs() <= 5.0, "固定した列幅と余白が効くこと: {x}");
    Ok(())
}

/// スクロールは中身のハンドルを捨てても保持し続ける。
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

/// miui が付けた大きさの制約だけを取り出す。
fn miui_constraints(view: &NSView) -> Vec<Retained<NSLayoutConstraint>> {
    let all = view.constraints();
    (0..all.len())
        .map(|i| all.objectAtIndex(i))
        .filter(|c| {
            c.identifier()
                .is_some_and(|id| id.to_string() == "miui.sizing")
        })
        .collect()
}

/// 同じ行に高さの違うものを置いても、縦中央でそろう。
///
/// NSGridView の既定は上ぞろえなので、ラベル (16pt) と入力欄 (24pt) を
/// 並べるとラベルだけが上にずれる。
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

/// ファイル選択は AppKit のボタンとして構成され、設定を保つ。
///
/// `NSOpenPanel` はアプリモーダルで、出すと閉じられるまで戻らない。
/// 自動テストからは開けないので、**ボタンの実体と設定の保持まで**を確かめる。
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

/// モードと絞り込みが、実際の `NSOpenPanel` の設定になる。
///
/// `native_panel()` は組み立てるだけで表示しないので、
/// イベントループを回さずに中身を確かめられる。
fn file_picker_panel(ui: &Ui) -> Result<()> {
    let picker = ui.file_picker("選ぶ")?;

    let panel = picker.native_panel();
    assert!(panel.canChooseFiles(), "既定はファイルを選ぶ");
    assert!(!panel.canChooseDirectories());
    assert!(!panel.allowsMultipleSelection());

    picker.set_mode(FilePickerMode::Files);
    let panel = picker.native_panel();
    assert!(panel.canChooseFiles());
    assert!(panel.allowsMultipleSelection(), "複数選べること");

    picker.set_mode(FilePickerMode::Folder);
    let panel = picker.native_panel();
    assert!(!panel.canChooseFiles(), "フォルダーだけを選ばせること");
    assert!(panel.canChooseDirectories());
    assert!(!panel.allowsMultipleSelection());

    // 絞り込みは拡張子の並びとして渡り、フォルダーのときは渡らない。
    picker.set_filters(&[
        FileFilter::new("画像", ["*.PNG", "jpg"]),
        FileFilter::new("文書", ["txt"]),
    ]);
    #[allow(deprecated)]
    let folder_types = picker.native_panel().allowedFileTypes();
    assert!(folder_types.is_none(), "フォルダー選択では絞り込まないこと");

    picker.set_mode(FilePickerMode::File);
    #[allow(deprecated)]
    let types = picker
        .native_panel()
        .allowedFileTypes()
        .expect("拡張子が設定されること");
    let types: Vec<String> = types.iter().map(|t| t.to_string()).collect();
    assert_eq!(types, ["png", "jpg", "txt"]);
    Ok(())
}

/// メインメニューに編集項目があり、⌘V がレスポンダチェーンへ流れる。
///
/// macOS では ⌘C / ⌘V はメインメニューのキー等価として配送される。
/// メニューが無いと、テキスト入力にフォーカスがあっても貼り付けができない。
fn menu_bar_provides_edit_shortcuts(_ui: &Ui) -> Result<()> {
    let mtm = objc2::MainThreadMarker::new().expect("メインスレッド");
    miui_macos::install_menu_bar_for_test(mtm, "miui test");

    let app = objc2_app_kit::NSApplication::sharedApplication(mtm);
    let main = app.mainMenu().expect("メインメニューがあること");
    let edit = (0..main.numberOfItems())
        .filter_map(|i| main.itemAtIndex(i).and_then(|item| item.submenu()))
        .find(|menu| menu.title().to_string() == "編集")
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
    miui_macos::install_menu_bar_for_test(mtm, "miui test");
    assert!(
        app.mainMenu().is_some_and(|m| m == again),
        "既にメニューがあれば作り直さないこと"
    );
    Ok(())
}

// --------------------------------------------------------------- メディア

/// 2x2 の PNG。画像の読み込みを、実ファイルを介して確かめるために埋め込む。
const PNG_2X2: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x02, 0x08, 0x02, 0x00, 0x00, 0x00, 0xfd, 0xd4, 0x9a,
    0x73, 0x00, 0x00, 0x00, 0x10, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0xf8, 0xcf, 0x00, 0x04,
    0xff, 0x19, 0x20, 0x14, 0x00, 0x1b, 0xf2, 0x03, 0xfd, 0xd6, 0x96, 0xf2, 0x2b, 0x00, 0x00, 0x00,
    0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
];

/// テストが使う一時ファイル。落ちても残らないよう Drop で消す。
struct Fixture {
    dir: std::path::PathBuf,
}

impl Fixture {
    /// テストごとに別のディレクトリを作る。
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("miui-{}-{}", std::process::id(), name));
        std::fs::create_dir_all(&dir).expect("一時ディレクトリの作成");
        Self { dir }
    }

    fn write(&self, name: &str, bytes: &[u8]) -> String {
        let path = self.dir.join(name);
        std::fs::write(&path, bytes).expect("一時ファイルの書き出し");
        path.to_str().expect("UTF-8 のパス").to_string()
    }

    /// 0.5 秒の無音 WAV。AVFoundation が実際に再生できる最小のメディア。
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

/// ランループを回して、AVFoundation の非同期な読み込みと再生を進める。
///
/// アセットの読み込みも再生位置の更新もランループの上で起きるため、
/// これを回さないと長さは決まらず、再生も進まない。
fn pump(seconds: f64) {
    let until = NSDate::dateWithTimeIntervalSinceNow(seconds);
    NSRunLoop::currentRunLoop().runUntilDate(&until);
}

/// 状態の変化を記録するクロージャを付ける。
fn record_states(seen: &Rc<RefCell<Vec<PlaybackState>>>) -> impl FnMut(PlaybackState) + 'static {
    let seen = seen.clone();
    move |state| seen.borrow_mut().push(state)
}

/// 画像がローカルのファイルから NSImage として読み込まれる。
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
    image.set_source("/miui/does/not/exist.png");
    assert!(!image.is_loaded());
    assert!(native.image().is_none());

    // 空文字列でも落ちない。
    image.set_source("");
    assert!(!image.is_loaded());
    Ok(())
}

/// 収め方の指定が NSImageView の imageScaling になる。
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
    assert_eq!(native.imageScaling(), NSImageScaling::ScaleAxesIndependently);
    image.set_fit(Fit::None);
    assert_eq!(native.imageScaling(), NSImageScaling::ScaleNone);
    // Cover は imageScaling の値を共有するが、MiuiImageView::drawRect で独自に描画する。
    image.set_fit(Fit::Cover);
    assert_eq!(
        native.imageScaling(),
        NSImageScaling::ScaleProportionallyUpOrDown
    );
    Ok(())
}

/// 音声を最後まで再生し、状態と長さが AVPlayer から届く。
fn audio_plays_to_the_end(ui: &Ui) -> Result<()> {
    let fixture = Fixture::new("audio");
    let audio = ui.audio(&fixture.silent_wav("a.wav"))?;
    let seen = Rc::new(RefCell::new(Vec::new()));
    audio.on_state_change(record_states(&seen));

    // 読み込みが終わるまで長さは決まらない。
    assert_eq!(audio.duration(), None, "読み込み前の長さは None であること");
    pump(0.5);
    let duration = audio.duration().expect("読み込み後は長さが決まること");
    assert!(
        (duration - 0.5).abs() < 0.05,
        "WAV の長さ 0.5 秒が返ること: {duration}"
    );

    audio.play();
    pump(1.0);
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
    pump(0.2);
    assert!(
        audio.position() < 0.4,
        "先頭へ戻って再生されること: {}",
        audio.position()
    );
    Ok(())
}

/// 繰り返しを指定すると、末尾で止まらずに先頭へ戻る。
fn audio_loops_back_to_the_start(ui: &Ui) -> Result<()> {
    let fixture = Fixture::new("loop");
    let audio = ui.audio(&fixture.silent_wav("a.wav"))?;
    audio.set_loop(true);
    assert!(audio.is_loop());
    let seen = Rc::new(RefCell::new(Vec::new()));
    audio.on_state_change(record_states(&seen));

    pump(0.5);
    audio.play();
    // 0.5 秒のメディアを 1.2 秒ぶん回すので、必ず末尾を越える。
    pump(1.2);

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

/// 再生位置を指定すると、AVPlayer の現在位置が動く。
fn media_seek_moves_the_position(ui: &Ui) -> Result<()> {
    let fixture = Fixture::new("seek");
    let video = ui.video(&fixture.silent_wav("a.wav"))?;
    pump(0.5);

    video.seek(0.25);
    pump(0.2);
    let position = video.position();
    assert!(
        (position - 0.25).abs() < 0.1,
        "指定した位置へ移ること: {position}"
    );

    // 負の値は先頭として扱う。
    video.seek(-5.0);
    pump(0.2);
    assert!(video.position() < 0.1, "先頭へ戻ること: {}", video.position());
    Ok(())
}

/// 音量と消音が AVPlayer と往復し、範囲外は丸められる。
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

/// ハンドルを捨てても、KVO を張ったままの AVPlayer で異常終了しない。
///
/// KVO の観測者を登録したまま AVPlayer を解放すると AppKit が落ちる。
/// Drop で確実に外していることを、実際に解放して確かめる。
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

/// 再生中、再生位置が定期的にクロージャへ届く。
///
/// シークバーを再生に追従させるための通知。AVPlayer の
/// `addPeriodicTimeObserverForInterval:` から来る。
fn media_reports_position_while_playing(ui: &Ui) -> Result<()> {
    let fixture = Fixture::new("position");
    let audio = ui.audio(&fixture.silent_wav("a.wav"))?;
    let seen = Rc::new(RefCell::new(Vec::new()));
    audio.on_position_change({
        let seen = seen.clone();
        move |seconds| seen.borrow_mut().push(seconds)
    });

    pump(0.5);
    audio.play();
    pump(1.0);

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
