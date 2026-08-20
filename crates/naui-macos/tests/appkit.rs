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

use naui_core::{
    Align, DialogButtons, DialogResponse, FileFilter, FilePickerMode, Fit, GridCell, ListItem,
    NavItem, Orientation, Padding, PlaybackState, PopupItem, Result, ScrollPolicy, SelectionMode,
    Sizing, Theme, Track, TreeItem,
};
use naui_macos::{run_for_test, Ui, Widget};
use objc2::rc::Retained;
use objc2_app_kit::{
    NSButton, NSImageScaling, NSImageView, NSLayoutConstraint, NSOutlineViewDelegate,
    NSSegmentedControl, NSTableViewDelegate, NSTextField, NSTextInputClient, NSTextView, NSView,
};
use objc2_foundation::{NSDate, NSNotFound, NSRange, NSRunLoop, NSSize, NSString};

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
        ("複数行入力が改行込みで往復する", text_area_round_trip),
        (
            "複数行入力の打鍵が通知され、プレースホルダーが消える",
            text_area_notifies_while_typing,
        ),
        (
            "複数行入力のプレースホルダーがクリックを通す",
            text_area_placeholder_passes_clicks,
        ),
        (
            "複数行入力が指定した高さに収まり、折り返し幅が追従する",
            text_area_follows_the_given_size,
        ),
        ("スライダーが範囲でクランプされる", slider_clamp),
        ("進捗バーが 0..1 に収まる", progress_clamp),
        (
            "コンボボックスの選択がネイティブと往復する",
            combo_box_selection_round_trips,
        ),
        (
            "コンボボックスの通知中に内容と通知先を差し替えられる",
            combo_box_callback_is_reentrant,
        ),
        ("スタックが子を生かし続ける", stack_keeps_children),
        ("ウィンドウを設定して閉じられる", window_lifecycle),
        ("ナビバーの選択がネイティブと往復する", navbar_selection),
        ("ドックが等幅の項目を持つ", dock_items),
        ("タブが中身ごと切り替わる", tabs_selection),
        ("メニューの選択が 1 つだけ点く", menu_selection),
        ("リストの行の文字が縦中央にそろう", list_rows_are_vertically_centered),
        ("リストの補助の文字が 2 行目に出る", list_detail_makes_a_second_line),
        ("リストの選択がネイティブと往復する", list_selection_round_trips),
        ("リストの複数選択が 0 件にもなる", list_multiple_selection),
        ("リストが選べない行を飛ばす", list_skips_disabled_rows),
        ("リストの行が NSTableView に描かれる", list_rows_are_native_views),
        (
            "NSTableView 側の選択がクロージャへ届く",
            list_native_selection_notifies,
        ),
        (
            "リストの通知の中からリストを操作できる",
            list_callback_can_touch_the_list,
        ),
        ("ツリーの行が展開に追従する", tree_rows_follow_the_expansion),
        (
            "ツリーの選択がネイティブと往復する",
            tree_selection_round_trips,
        ),
        ("ツリーが選べない枝を飛ばす", tree_skips_disabled_branches),
        ("ツリーの開閉が通知される", tree_expansion_notifies),
        (
            "閉じた枝の中の開閉が保たれる",
            tree_remembers_expansion_inside_a_closed_branch,
        ),
        (
            "ツリーの行が NSOutlineView に描かれる",
            tree_rows_are_native_views,
        ),
        (
            "ツリーの通知の中からツリーを操作できる",
            tree_callback_can_touch_the_tree,
        ),
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
        (
            "Stack の Auto の子が余白で横に広がらない",
            stack_auto_does_not_fill,
        ),
        (
            "Stack の主軸の Fill は余白を受け取る",
            stack_fill_main,
        ),
        (
            "上限付きの Fill が空間のあるときは上限まで広がる",
            fill_with_max_prefers_the_max,
        ),
        (
            "交差軸の Fill に上限を付けても親の幅と衝突しない",
            stack_fill_cross_respects_max,
        ),
        ("スペーサーが余りを吸って後続を端へ寄せる", spacer_pushes),
        ("グリッドが行と列を広げて子を置く", grid_places_children),
        (
            "グリッドの Auto の子が再レイアウトで横に広がらない",
            grid_auto_does_not_fill,
        ),
        (
            "Gallery のメディア欄が高さ変更後にラベル幅を広げない",
            gallery_media_status_does_not_expand,
        ),
        ("グリッドの子を置き換える", grid_replaces_child),
        ("スクロールが中身を保持する", scroll_keeps_child),
        ("グリッドの同じ行が縦中央でそろう", grid_row_alignment),
        (
            "ファイル選択がボタンとして構成され設定を保つ",
            file_picker_configuration,
        ),
        (
            "ダイアログの設定が NSAlert へ届く",
            dialog_configuration_reaches_the_alert,
        ),
        (
            "ダイアログのボタンが役割の順に並ぶ",
            dialog_buttons_follow_the_macos_order,
        ),
        (
            "ボタンを指定しないダイアログに OK が出る",
            dialog_without_buttons_shows_ok,
        ),
        (
            "出していないダイアログは閉じても何も起きない",
            dialog_is_closed_until_opened,
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
        (
            "動画の表示領域が読み込み前から高さを保つ",
            video_display_reserves_height,
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
        (
            "ポップアップメニューが項目と区切り線を NSMenu に写す",
            popup_menu_items_map_to_native,
        ),
        (
            "ポップアップメニューの選択がクロージャへ届く",
            popup_menu_selection_notifies,
        ),
        (
            "ポップアップメニューが取り付け先のビューのメニューになる",
            popup_menu_attaches_to_a_view,
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

/// 複数行入力は改行を含む文字列をそのまま保つ。
fn text_area_round_trip(ui: &Ui) -> Result<()> {
    let area = ui.text_area("あ\nい")?;
    assert_eq!(area.text(), "あ\nい");

    area.set_text("一行目\n二行目\n三行目");
    assert_eq!(area.text(), "一行目\n二行目\n三行目");

    // NSScrollView に載っているので、外から見えるビューはスクロールビュー。
    let view = area.native_view();
    assert!(
        view.downcast_ref::<objc2_app_kit::NSScrollView>().is_some(),
        "複数行入力はスクロールビューごと 1 つのウィジェットであること"
    );

    let text_view = area.native_text_view();
    let default_color = text_view.textColor();
    area.set_enabled(false);
    assert!(!text_view.isEditable(), "無効なら編集できないこと");
    area.set_enabled(true);
    assert!(text_view.isEditable());
    assert_eq!(
        text_view.textColor(),
        default_color,
        "有効へ戻したら NSTextView の既定の文字色へ戻ること"
    );
    Ok(())
}

/// 打鍵のたびに通知が届き、プレースホルダーは中身の有無で出入りする。
fn text_area_notifies_while_typing(ui: &Ui) -> Result<()> {
    let area = ui.text_area("")?;
    area.set_placeholder("本文");
    let text_view = area.native_text_view();
    let placeholder = placeholder_label(&text_view).expect("プレースホルダーが重なっていること");
    assert_eq!(placeholder.stringValue().to_string(), "本文");
    assert!(!placeholder.isHidden(), "空のときは出ていること");

    let seen = Rc::new(RefCell::new(Vec::new()));
    area.on_change({
        let seen = seen.clone();
        move |text| seen.borrow_mut().push(text.to_string())
    });

    for chunk in ["あ", "\n", "い"] {
        type_into(&text_view, chunk);
    }
    assert_eq!(area.text(), "あ\nい");
    assert_eq!(
        *seen.borrow(),
        vec!["あ".to_string(), "あ\n".to_string(), "あ\nい".to_string()],
        "改行の入力でも通知されること"
    );
    assert!(placeholder.isHidden(), "入力後は隠れていること");

    area.set_text("");
    assert!(!placeholder.isHidden(), "空に戻したら出ること");
    assert_eq!(
        seen.borrow().len(),
        3,
        "set_text では通知しないこと (1 行入力と同じ)"
    );
    Ok(())
}

/// プレースホルダーの上をクリックしても、当たり判定は下の NSTextView へ通る。
fn text_area_placeholder_passes_clicks(ui: &Ui) -> Result<()> {
    let area = ui.text_area("")?;
    area.set_placeholder("本文");
    let text_view = area.native_text_view();
    // レイアウトを確定させてから、ラベルの中心を叩く。
    let view: &NSView = text_view.as_ref();
    view.layoutSubtreeIfNeeded();
    let placeholder = placeholder_label(&text_view).expect("プレースホルダーが重なっていること");
    let frame = placeholder.frame();
    let point = objc2_foundation::NSPoint::new(
        frame.origin.x + frame.size.width / 2.0,
        frame.origin.y + frame.size.height / 2.0,
    );
    let hit = view.hitTest(point);
    assert!(
        hit.is_some_and(|hit| hit.downcast_ref::<NSTextView>().is_some()),
        "プレースホルダーではなく NSTextView が受け取ること"
    );
    Ok(())
}

/// 高さは `set_sizing` の指定どおりになり、折り返しの幅は親に追従する。
fn text_area_follows_the_given_size(ui: &Ui) -> Result<()> {
    let stack = ui.stack(Orientation::Vertical)?;
    stack.set_sizing(Sizing::fill());
    let area = ui.text_area("あ")?;
    area.set_sizing(
        Sizing::new()
            .width(naui_core::Length::Fill)
            .height(naui_core::Length::Fixed(96.0)),
    );
    stack.append(&area);

    let root = stack.native_view();
    root.setFrameSize(NSSize::new(400.0, 400.0));
    root.layoutSubtreeIfNeeded();

    let frame = area.native_view().frame();
    assert!(
        (frame.size.height - 96.0).abs() < 1e-6 && (frame.size.width - 400.0).abs() < 1e-6,
        "指定した高さと親の幅になること: {frame:?}"
    );

    // 中の NSTextView は、枠のぶんだけ内側でスクロールビューに追従する。
    // 折り返しはこの幅で起きるので、親が広がれば折り返しも変わる。
    let inner = area.native_text_view().frame();
    assert!(
        inner.size.width > 0.0 && inner.size.width <= frame.size.width,
        "折り返しの幅がスクロールビューに収まること: {inner:?}"
    );

    root.setFrameSize(NSSize::new(240.0, 400.0));
    root.layoutSubtreeIfNeeded();
    let narrow = area.native_text_view().frame();
    assert!(
        narrow.size.width < inner.size.width,
        "親が狭くなれば折り返しの幅も狭くなること: {inner:?} -> {narrow:?}"
    );
    Ok(())
}

/// 複数行入力に重ねたプレースホルダーのラベルを取り出す。
fn placeholder_label(text_view: &NSTextView) -> Option<Retained<NSTextField>> {
    let view: &NSView = text_view.as_ref();
    let subviews = view.subviews();
    (0..subviews.len())
        .map(|index| subviews.objectAtIndex(index))
        .find_map(|subview| subview.downcast::<NSTextField>().ok())
}

/// IME や物理キーボードと同じ経路で文字を入れる。
fn type_into(text_view: &NSTextView, text: &str) {
    let string = NSString::from_str(text);
    unsafe {
        text_view.insertText_replacementRange(&string, NSRange::new(NSNotFound as usize, 0));
    }
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

/// 項目・未選択状態・通知の有無が NSPopUpButton と一致する。
fn combo_box_selection_round_trips(ui: &Ui) -> Result<()> {
    let combo = ui.combo_box()?;
    assert!(combo.is_empty());
    assert_eq!(combo.selected(), None);

    combo.set_items(&["東京", "大阪", "札幌"]);
    assert_eq!(combo.len(), 3);
    assert_eq!(combo.selected(), None, "項目の作り直し後は未選択");
    let native = combo.native_combo_box();
    assert_eq!(native.numberOfItems(), 3);
    assert_eq!(native.itemTitleAtIndex(1).to_string(), "大阪");

    let seen = Rc::new(RefCell::new(Vec::new()));
    combo.on_select({
        let seen = seen.clone();
        move |index| seen.borrow_mut().push(index)
    });

    combo.set_selected(1);
    assert_eq!(combo.selected(), Some(1));
    assert!(seen.borrow().is_empty(), "set_selected は通知しない");
    combo.set_selected(99);
    assert_eq!(combo.selected(), Some(1), "範囲外は無視する");

    combo.clear_selection();
    assert_eq!(combo.selected(), None);
    assert!(seen.borrow().is_empty(), "clear_selection は通知しない");

    combo.select(2);
    assert_eq!(combo.selected(), Some(2));
    assert_eq!(*seen.borrow(), vec![2]);
    combo.select(99);
    assert_eq!(*seen.borrow(), vec![2], "範囲外は通知もしない");

    // AppKit の target/action から届く実際の通知経路も確かめる。
    native.selectItemAtIndex(0);
    let target = native.target();
    unsafe { native.sendAction_to(native.action(), target.as_deref()) };
    assert_eq!(*seen.borrow(), vec![2, 0]);

    combo.set_enabled(false);
    assert!(!native.isEnabled());
    combo.set_items(&["那覇"]);
    assert_eq!(combo.selected(), None);
    assert_eq!(*seen.borrow(), vec![2, 0], "再構築は通知しない");
    Ok(())
}

/// 通知の最中でも同じコンボボックスを操作し、コールバックを差し替えられる。
fn combo_box_callback_is_reentrant(ui: &Ui) -> Result<()> {
    let combo = ui.combo_box()?;
    combo.set_items(&["春", "夏", "秋"]);

    let first = Rc::new(RefCell::new(Vec::new()));
    let replacement = Rc::new(RefCell::new(Vec::new()));
    combo.on_select({
        let combo = combo.clone();
        let first = first.clone();
        let replacement = replacement.clone();
        move |index| {
            first.borrow_mut().push(index);
            combo.set_items(&["朝", "昼"]);
            combo.set_selected(1);
            combo.on_select({
                let replacement = replacement.clone();
                move |index| replacement.borrow_mut().push(index)
            });
        }
    });

    combo.select(0);
    assert_eq!(*first.borrow(), vec![0]);
    assert_eq!(combo.len(), 2);
    assert_eq!(combo.selected(), Some(1));

    combo.select(0);
    assert_eq!(*first.borrow(), vec![0], "古い通知先は外れること");
    assert_eq!(*replacement.borrow(), vec![0]);
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
    let control = (0..subviews.len())
        .find_map(|index| {
            subviews
                .objectAtIndex(index)
                .downcast::<objc2_app_kit::NSButton>()
                .ok()
        })
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
    let navbar = ui.navbar("naui")?;
    assert_eq!(navbar.title(), "naui");
    navbar.set_items(&NavItem::list(["ホーム", "検索", "設定"]));
    assert_eq!(navbar.len(), 3);

    let view = navbar.native_view();
    view.setFrameSize(NSSize::new(500.0, 40.0));
    view.layoutSubtreeIfNeeded();
    let subviews = view.subviews();
    let title = (0..subviews.len())
        .find_map(|index| subviews.objectAtIndex(index).downcast::<NSTextField>().ok())
        .expect("見出しがあること");
    let segments = (0..subviews.len())
        .find_map(|index| {
            subviews
                .objectAtIndex(index)
                .downcast::<NSSegmentedControl>()
                .ok()
        })
        .expect("セグメントがあること");
    assert!(title.frame().size.width < 200.0, "見出しが余白で広がらないこと");
    assert!(
        segments.frame().size.width < 250.0,
        "セグメントが余白で広がらないこと"
    );

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
    let link = ui.link("naui", "")?;
    assert_eq!(link.text(), "naui");
    assert_eq!(link.href(), "");

    let hits = Rc::new(RefCell::new(0));
    link.on_click({
        let hits = hits.clone();
        move || *hits.borrow_mut() += 1
    });

    link.click();
    link.click();
    assert_eq!(*hits.borrow(), 2);

    link.set_text("naui のリポジトリ");
    assert_eq!(link.text(), "naui のリポジトリ");
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
    let sizes = naui_constraints(&view);
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

    let mine = naui_constraints(&view);
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

/// 横スタックの `Auto` の子は、余った主軸の空間を勝手に受け取らない。
fn stack_auto_does_not_fill(ui: &Ui) -> Result<()> {
    let stack = ui.stack(Orientation::Horizontal)?;
    stack.set_spacing(12.0);
    let first = ui.button("短")?;
    let second = ui.button("少し長い項目")?;
    stack.append(&first);
    stack.append(&second);

    let root = stack.native_view();
    root.setFrameSize(NSSize::new(500.0, 60.0));
    root.layoutSubtreeIfNeeded();

    let first_frame = first.native_view().frame();
    let second_frame = second.native_view().frame();
    assert!(
        first_frame.size.width < 150.0,
        "Auto の先頭項目が余白で広がらないこと: {first_frame:?}"
    );
    assert!(
        second_frame.size.width < 250.0,
        "Auto の後続項目が余白で広がらないこと: {second_frame:?}"
    );
    assert!(
        first_frame.origin.x.abs() < 1e-6,
        "余った空間で先頭項目が中央へ移動しないこと: {first_frame:?}"
    );
    assert!(
        (second_frame.origin.x - first_frame.origin.x - first_frame.size.width - 12.0).abs()
            < 1e-6,
        "指定した間隔だけで項目が並ぶこと: {first_frame:?} / {second_frame:?}"
    );
    Ok(())
}

/// 横スタックで幅 `Fill` を指定した子は、明示的に余りを受け取る。
fn stack_fill_main(ui: &Ui) -> Result<()> {
    let stack = ui.stack(Orientation::Horizontal)?;
    stack.set_spacing(12.0);
    let fill = ui.button("伸びる")?;
    fill.set_sizing(Sizing::fill_width());
    let fixed = ui.button("固定")?;
    stack.append(&fill);
    stack.append(&fixed);

    let root = stack.native_view();
    root.setFrameSize(NSSize::new(500.0, 60.0));
    root.layoutSubtreeIfNeeded();

    let fill_frame = fill.native_view().frame();
    let fixed_frame = fixed.native_view().frame();
    assert!(
        fill_frame.size.width > fixed_frame.size.width + 150.0,
        "主軸の Fill だけが余白を受け取ること: {fill_frame:?} / {fixed_frame:?}"
    );
    Ok(())
}

/// 交差軸の `Fill` に上限を付けても、親の幅と衝突しない。
///
/// 交差軸の `Fill` は「親の幅に合わせる」だが、上限があるならそちらが優先。
/// 必須制約どうしがぶつかると、AppKit がどちらかを勝手に落としてしまう。
fn stack_fill_cross_respects_max(ui: &Ui) -> Result<()> {
    // 縦並びの交差軸は横。
    let vertical = ui.stack(Orientation::Vertical)?;
    let capped = ui.button("上限まで")?;
    capped.set_sizing(Sizing::fill_width().max_width(200.0));
    vertical.append(&capped);

    let root = vertical.native_view();
    root.setFrameSize(NSSize::new(640.0, 200.0));
    root.layoutSubtreeIfNeeded();
    // 親に合わせる制約は必須ではない (必須だと上限とぶつかり、AppKit が
    // どちらを落とすか分からなくなる)。
    let all = root.constraints();
    let ours: Vec<_> = (0..all.len())
        .map(|i| all.objectAtIndex(i))
        .filter(|c| c.identifier().is_none())
        .collect();
    assert!(
        ours.iter().any(|c| c.priority() < 1000.0),
        "親に合わせる制約は必須より下であること"
    );

    let frame = capped.native_view().frame();
    assert!(
        (frame.size.width - 200.0).abs() < 1e-6,
        "交差軸の上限が親の幅より優先されること: {frame:?}"
    );

    // 横並びの交差軸は縦。
    let horizontal = ui.stack(Orientation::Horizontal)?;
    let short = ui.button("上限まで")?;
    short.set_sizing(Sizing::fill_height().max_height(24.0));
    horizontal.append(&short);

    let root = horizontal.native_view();
    root.setFrameSize(NSSize::new(300.0, 200.0));
    root.layoutSubtreeIfNeeded();
    let frame = short.native_view().frame();
    assert!(
        (frame.size.height - 24.0).abs() < 1e-6,
        "交差軸の上限が親の高さより優先されること: {frame:?}"
    );
    Ok(())
}

/// 上限付きの `Fill` は、空間があれば上限まで広がり、狭ければそれ以下になる。
///
/// CSS や WinUI の `Stretch` と同じ意味になるよう、上限を「通常時に確保したい
/// 大きさ」としても扱う。横も縦も同じ扱いで、指定は `Sizing` だけで済む。
fn fill_with_max_prefers_the_max(ui: &Ui) -> Result<()> {
    let stack = ui.stack(Orientation::Horizontal)?;
    let capped = ui.button("上限まで")?;
    capped.set_sizing(Sizing::fill_width().max_width(200.0));
    stack.append(&capped);
    stack.append(&ui.spacer()?);

    // 上限 (必須) と、上限まで広がりたい希望 (弱い) の 2 本。
    let constraints = naui_constraints(&capped.native_view());
    assert_eq!(constraints.len(), 2, "上限と希望の 2 本が付くこと");
    assert!(
        constraints.iter().any(|c| c.priority() < 100.0),
        "希望のほうは弱い優先度であること"
    );

    let root = stack.native_view();
    root.setFrameSize(NSSize::new(640.0, 60.0));
    root.layoutSubtreeIfNeeded();
    let wide = capped.native_view().frame();
    assert!(
        (wide.size.width - 200.0).abs() < 1e-6,
        "空間があるときは上限まで広がること: {wide:?}"
    );

    root.setFrameSize(NSSize::new(120.0, 60.0));
    root.layoutSubtreeIfNeeded();
    let narrow = capped.native_view().frame();
    assert!(
        narrow.size.width < wide.size.width,
        "狭いときは上限より小さくなること: {wide:?} -> {narrow:?}"
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

/// `Track::Fill` の列に置いた `Auto` の子は、再レイアウト後も内容幅を保つ。
fn grid_auto_does_not_fill(ui: &Ui) -> Result<()> {
    let grid = ui.grid()?;
    grid.set_column_track(0, Track::FILL);
    grid.set_row_track(0, Track::Auto);
    let status = ui.label("種類: 画像 (同梱のサンプル)")?;
    grid.attach(&status, GridCell::new(0, 0));

    let root = grid.native_view();
    root.setFrameSize(NSSize::new(500.0, 860.0));
    root.layoutSubtreeIfNeeded();
    let initial = status.native_view().frame();

    root.setFrameSize(NSSize::new(500.0, 300.0));
    root.layoutSubtreeIfNeeded();
    let shrunk = status.native_view().frame();

    root.setFrameSize(NSSize::new(500.0, 860.0));
    root.layoutSubtreeIfNeeded();
    let resized = status.native_view().frame();
    assert!(
        resized.size.width < 300.0,
        "再レイアウト後もラベルが列幅へ広がらないこと: {initial:?} / {resized:?}"
    );
    assert!(
        (initial.size.width - resized.size.width).abs() < 1e-6,
        "高さ変更でラベルの幅が変わらないこと: {initial:?} / {resized:?}"
    );
    assert!(
        (shrunk.size.width - resized.size.width).abs() < 1e-6,
        "縮小中もラベルの幅が変わらないこと: {shrunk:?} / {resized:?}"
    );
    Ok(())
}

/// Gallery と同じ Stack -> Tabs -> Stack -> Grid の階層で、高さの縮小・拡大を行う。
fn gallery_media_status_does_not_expand(ui: &Ui) -> Result<()> {
    let media = ui.stack(Orientation::Vertical)?;
    media.set_spacing(12.0);
    media.set_padding(Padding::all(12.0));
    media.set_align(Align::Start);
    let description = ui.label("選んだファイルの種類に合わせて表示形式が切り替わります。")?;
    let description_row = ui.stack(Orientation::Horizontal)?;
    description_row.append(&description);
    media.append(&description_row);

    let image_pane = ui.grid()?;
    image_pane.set_spacing(0.0, 8.0);
    image_pane.set_padding(Padding::all(8.0));
    image_pane.set_sizing(Sizing::fill());
    image_pane.set_column_track(0, Track::FILL);
    image_pane.set_row_track(0, Track::FILL);
    let image = ui.image(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../examples/gallery/assets/sample.png"
    ))?;
    image.set_sizing(Sizing::fill().max_height(315.0));
    image_pane.attach(&image, GridCell::new(0, 0));
    let selector = ui.navbar("収め方")?;
    selector.set_items(&NavItem::list(["contain", "cover", "fill", "none"]));
    selector.set_sizing(Sizing::fill_width());
    image_pane.attach(&selector, GridCell::new(0, 1));

    let forms = ui.grid()?;
    forms.set_sizing(Sizing::fill());
    forms.set_column_track(0, Track::FILL);
    forms.set_row_track(0, Track::FILL);
    forms.attach(&image_pane, GridCell::new(0, 0));

    let video_pane = ui.stack(Orientation::Vertical)?;
    video_pane.set_spacing(8.0);
    video_pane.set_padding(Padding::all(8.0));
    video_pane.set_align(Align::Start);
    video_pane.set_sizing(Sizing::fill());
    let video_frame = ui.grid()?;
    video_frame.set_sizing(Sizing::fill().max_height(315.0));
    video_frame.set_column_track(0, Track::FILL);
    video_frame.set_row_track(0, Track::FILL);
    let video = ui.video("")?;
    video.set_sizing(Sizing::fill().max_height(315.0));
    video_frame.attach(&video, GridCell::new(0, 0));
    video_pane.append(&video_frame);
    let video_controls = ui.stack(Orientation::Vertical)?;
    video_controls.append(&ui.label("状態: 未再生")?);
    video_pane.append(&video_controls);

    let status = ui.label("種類: 画像 (同梱のサンプル)")?;
    let status_row = ui.stack(Orientation::Horizontal)?;
    status_row.append(&status);
    let field = ui.text_input("")?;
    field.set_sizing(Sizing::fill_width());
    let row = ui.stack(Orientation::Horizontal)?;
    row.set_spacing(8.0);
    let load = ui.button("読み込む")?;
    let pick = ui.button("選ぶ")?;
    row.append(&field);
    row.append(&load);
    row.append(&pick);
    row.set_sizing(Sizing::fill_width());
    media.append(&row);
    media.append(&status_row);
    media.append(&forms);

    let root = ui.stack(Orientation::Vertical)?;
    root.set_spacing(12.0);
    root.set_padding(Padding::all(24.0));
    root.append(&ui.label("naui")?);
    root.append(&ui.label("ルート: メディア")?);
    let tabs = ui.tabs()?;
    let home = ui.label("ホーム")?;
    tabs.add_tab("ホーム", &home);
    tabs.add_tab("メディア", &media);
    tabs.set_sizing(Sizing::fill());
    root.append(&tabs);
    root.append(&ui.label("下端")?);

    let window = ui.window("media layout", 680.0, 860.0)?;
    window.set_child(&root);
    window.show();
    let native_window = window.native_window();
    let content = native_window.contentView().expect("コンテンツビューがあること");
    content.setFrameSize(NSSize::new(680.0, 860.0));
    content.layoutSubtreeIfNeeded();
    pump(0.05);
    tabs.select(1);
    content.layoutSubtreeIfNeeded();
    pump(0.05);
    let initial = status.native_view().frame();
    let description_initial = description.native_view().frame();
    let row_initial = row.native_view().frame();
    native_window.setContentSize(NSSize::new(680.0, 350.0));
    content.setFrameSize(NSSize::new(680.0, 350.0));
    content.layoutSubtreeIfNeeded();
    pump(0.05);
    let shrunk = status.native_view().frame();
    let description_shrunk = description.native_view().frame();
    let row_shrunk = row.native_view().frame();
    native_window.setContentSize(NSSize::new(680.0, 860.0));
    content.setFrameSize(NSSize::new(680.0, 860.0));
    content.layoutSubtreeIfNeeded();
    pump(0.05);
    let resized = status.native_view().frame();
    let description_resized = description.native_view().frame();
    let row_resized = row.native_view().frame();
    let image_frame = image.native_view().frame();
    forms.replace(&video_pane, GridCell::new(0, 0));
    content.layoutSubtreeIfNeeded();
    pump(0.05);
    let video_frame_after_replace = video.native_view().frame();
    forms.replace(&image_pane, GridCell::new(0, 0));
    content.layoutSubtreeIfNeeded();
    pump(0.05);
    assert!(
        resized.size.width < 300.0,
        "高さ変更後もステータスが列幅へ広がらないこと: {initial:?} / {shrunk:?} / {resized:?}"
    );
    assert!(
        (initial.size.width - resized.size.width).abs() < 1e-6,
        "縮小してから戻してもステータス幅が変わらないこと: {initial:?} / {shrunk:?} / {resized:?}"
    );
    assert!(
        description_resized.size.width < 500.0,
        "高さ変更後も説明文が列幅へ広がらないこと: {description_initial:?} / {description_shrunk:?} / {description_resized:?}"
    );
    assert!(
        (description_initial.size.width - description_resized.size.width).abs() < 1e-6,
        "縮小してから戻しても説明文の幅が変わらないこと: {description_initial:?} / {description_shrunk:?} / {description_resized:?}"
    );
    assert!(
        row_resized.size.height < 100.0,
        "高さ変更後もパス入力行がメディア表示の余白を受け取らないこと: {row_initial:?} / {row_shrunk:?} / {row_resized:?}"
    );
    assert!(
        (row_initial.size.height - row_resized.size.height).abs() < 1e-6,
        "縮小してから戻してもパス入力行の高さが変わらないこと: {row_initial:?} / {row_shrunk:?} / {row_resized:?}"
    );
    assert!(
        image_frame.size.width > 500.0,
        "Auto のラベルがあっても画像の Fill は列いっぱいになること: {image_frame:?}"
    );
    assert!(
        video_frame_after_replace.size.height > 200.0,
        "画像から動画へ差し替えた直後も動画表示欄が潰れないこと: {video_frame_after_replace:?}"
    );
    window.close();
    Ok(())
}

/// グリッドの差し替えで、前の子が親に残らない。
fn grid_replaces_child(ui: &Ui) -> Result<()> {
    let grid = ui.grid()?;
    let photo = ui.label("写真")?;
    let video = ui.label("動画")?;
    let cell = GridCell::new(0, 0);

    grid.attach(&photo, cell);
    assert!(unsafe { photo.native_view().superview() }.is_some());

    grid.replace(&video, cell);

    assert_eq!(grid.len(), 1);
    assert!(unsafe { photo.native_view().superview() }.is_none());
    assert!(unsafe { video.native_view().superview() }.is_some());
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

/// naui が付けた大きさの制約だけを取り出す。
fn naui_constraints(view: &NSView) -> Vec<Retained<NSLayoutConstraint>> {
    let all = view.constraints();
    (0..all.len())
        .map(|i| all.objectAtIndex(i))
        .filter(|c| {
            c.identifier()
                .is_some_and(|id| id.to_string() == "naui.sizing")
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

/// メインメニューに編集項目があり、⌘V がレスポンダチェーンへ流れる。
///
/// macOS では ⌘C / ⌘V はメインメニューのキー等価として配送される。
/// メニューが無いと、テキスト入力にフォーカスがあっても貼り付けができない。
fn menu_bar_provides_edit_shortcuts(_ui: &Ui) -> Result<()> {
    let mtm = objc2::MainThreadMarker::new().expect("メインスレッド");
    naui_macos::install_menu_bar_for_test(mtm, "naui test");

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
    naui_macos::install_menu_bar_for_test(mtm, "naui test");
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
        let dir = std::env::temp_dir().join(format!("naui-{}-{}", std::process::id(), name));
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
    image.set_source("/naui/does/not/exist.png");
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
    // Cover は imageScaling の値を共有するが、NauiImageView::drawRect で独自に描画する。
    image.set_fit(Fit::Cover);
    assert_eq!(
        native.imageScaling(),
        NSImageScaling::ScaleProportionallyUpOrDown
    );
    Ok(())
}

/// AVPlayerView に動画の intrinsic size がまだ無くても、表示領域を確保する。
///
/// 上限付きの `Fill` は「空間があれば上限まで、狭ければそれ以下」を表す。
/// Gallery 側に macOS だけの指定を書かずに済むよう、この意味は `Sizing` から
/// 導く (バックエンド共通の約束)。
fn video_display_reserves_height(ui: &Ui) -> Result<()> {
    let pane = ui.stack(Orientation::Vertical)?;
    pane.set_spacing(8.0);
    pane.set_padding(Padding::all(8.0));
    pane.set_align(Align::Start);
    pane.set_sizing(Sizing::fill());
    let video = ui.video("")?;
    video.set_sizing(Sizing::fill().max_height(315.0));
    let media_frame = ui.grid()?;
    media_frame.set_sizing(Sizing::fill().max_height(315.0));
    media_frame.set_column_track(0, Track::FILL);
    media_frame.set_row_track(0, Track::FILL);
    media_frame.attach(&video, GridCell::new(0, 0));
    pane.append(&media_frame);
    let controls = ui.stack(Orientation::Vertical)?;
    controls.set_spacing(8.0);
    let buttons = ui.stack(Orientation::Horizontal)?;
    buttons.set_spacing(8.0);
    buttons.append(&ui.button("再生")?);
    buttons.append(&ui.button("一時停止")?);
    controls.append(&buttons);
    controls.append(&ui.label("状態: 未再生")?);
    controls.append(&ui.label("位置: -")?);
    controls.append(&ui.slider(0.0, 1.0)?);
    controls.append(&ui.slider(0.0, 1.0)?);
    controls.append(&ui.label("音量: 100%")?);
    let toggles = ui.stack(Orientation::Horizontal)?;
    toggles.set_spacing(12.0);
    toggles.append(&ui.checkbox("消音")?);
    toggles.append(&ui.checkbox("繰り返し")?);
    controls.append(&toggles);
    pane.append(&controls);

    let root = pane.native_view();
    root.setFrameSize(NSSize::new(640.0, 860.0));
    root.layoutSubtreeIfNeeded();
    let initial = video.native_view().frame();
    assert!(
        (initial.size.height - 315.0).abs() < 1e-6,
        "通常の高さでは動画表示領域を上限まで確保すること: {initial:?}"
    );
    root.setFrameSize(NSSize::new(640.0, 300.0));
    root.layoutSubtreeIfNeeded();
    let narrow = video.native_view().frame();
    assert!(
        narrow.size.height < initial.size.height,
        "ウィンドウが狭いときは動画表示領域も縮められること: {initial:?} -> {narrow:?}"
    );
    assert!(
        narrow.size.height > 0.0,
        "動画表示領域が完全には潰れないこと: {narrow:?}"
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


/// リストの選択が NSTableView と往復し、`set_selected` は通知しない。
fn list_selection_round_trips(ui: &Ui) -> Result<()> {
    let list = ui.list()?;
    list.set_items(&ListItem::list(["東京", "大阪", "札幌"]));
    assert_eq!(list.len(), 3);
    assert_eq!(list.selected(), None, "作った直後は何も選ばれていないこと");
    assert_eq!(list.selection_mode(), SelectionMode::Single);

    let seen: Rc<RefCell<Vec<Vec<usize>>>> = Rc::new(RefCell::new(Vec::new()));
    list.on_select({
        let seen = seen.clone();
        move |indices| seen.borrow_mut().push(indices.to_vec())
    });

    list.select(1);
    assert_eq!(list.selected(), Some(1));
    assert_eq!(*seen.borrow(), vec![vec![1]]);

    // ネイティブ側の選択も動いていること。
    let table = list.native_table();
    assert_eq!(table.selectedRow(), 1);
    assert_eq!(table.numberOfRows(), 3);

    // 単一選択では 2 行目を選ぶと 1 行目は外れる。
    list.select(2);
    assert_eq!(list.selection(), vec![2]);

    // `set_selected` は通知しない。
    list.set_selected(0);
    assert_eq!(list.selected(), Some(0));
    assert_eq!(*seen.borrow(), vec![vec![1], vec![2]], "通知は 2 回だけ");

    // 行を作り直すと選択は外れる。
    list.set_items(&ListItem::list(["那覇"]));
    assert_eq!(list.len(), 1);
    assert_eq!(list.selected(), None);
    assert_eq!(*seen.borrow(), vec![vec![1], vec![2]], "作り直しも通知しない");
    Ok(())
}

/// 複数選択では複数行が選ばれ、選択が 0 件にもなる。
fn list_multiple_selection(ui: &Ui) -> Result<()> {
    let list = ui.list()?;
    list.set_items(&ListItem::list(["赤", "青", "緑", "黄"]));
    list.set_selection_mode(SelectionMode::Multiple);
    assert!(list.selection_mode().is_multiple());
    assert!(
        list.native_table().allowsMultipleSelection(),
        "NSTableView 側も複数選択になっていること"
    );

    let seen: Rc<RefCell<Vec<Vec<usize>>>> = Rc::new(RefCell::new(Vec::new()));
    list.on_select({
        let seen = seen.clone();
        move |indices| seen.borrow_mut().push(indices.to_vec())
    });

    // 並びは昇順にそろい、重複と範囲外は落ちる。
    list.select_many(&[3, 0, 3, 99]);
    assert_eq!(list.selection(), vec![0, 3]);
    assert_eq!(*seen.borrow(), vec![vec![0, 3]]);
    assert_eq!(list.selected(), Some(0), "selected は先頭の行を返すこと");

    // 0 件へも戻せる。
    list.select_many(&[]);
    assert_eq!(list.selection(), Vec::<usize>::new());
    assert_eq!(*seen.borrow(), vec![vec![0, 3], vec![]]);

    // clear_selection は通知しない。
    list.set_selection(&[1, 2]);
    assert_eq!(list.selection(), vec![1, 2]);
    list.clear_selection();
    assert!(list.selection().is_empty());
    assert_eq!(seen.borrow().len(), 2, "set_selection と clear は通知しない");

    // 単一選択へ戻すと選択は外れ、以後は 1 行だけになる。
    list.set_selection_mode(SelectionMode::Single);
    assert!(list.selection().is_empty());
    list.set_selection(&[1, 2]);
    assert_eq!(list.selection(), vec![1], "単一選択では先頭の 1 件だけ");
    Ok(())
}

/// 選べない行は、プログラムからもネイティブからも選ばれない。
fn list_skips_disabled_rows(ui: &Ui) -> Result<()> {
    let list = ui.list()?;
    list.set_items(&[
        ListItem::new("下書き"),
        ListItem::new("送信中").enabled(false),
        ListItem::new("送信済み"),
    ]);

    let seen: Rc<RefCell<Vec<Vec<usize>>>> = Rc::new(RefCell::new(Vec::new()));
    list.on_select({
        let seen = seen.clone();
        move |indices| seen.borrow_mut().push(indices.to_vec())
    });

    list.select(1);
    assert!(list.selection().is_empty(), "選べない行は選ばれないこと");
    assert_eq!(*seen.borrow(), vec![Vec::<usize>::new()]);

    list.select(2);
    assert_eq!(list.selection(), vec![2]);

    // AppKit 自身にも「この行は選べない」と伝わっている。
    let table = list.native_table();
    let delegate = unsafe { table.delegate() }.expect("デリゲートがあること");
    assert!(!delegate.tableView_shouldSelectRow(&table, 1));
    assert!(delegate.tableView_shouldSelectRow(&table, 2));
    Ok(())
}

/// 行の中身は AppKit がデリゲートに作らせた NSTextField になる。
fn list_rows_are_native_views(ui: &Ui) -> Result<()> {
    let list = ui.list()?;
    list.set_items(&ListItem::list(["日本語の行", "ASCII row"]));
    list.set_sizing(Sizing::fixed(240.0, 120.0));

    // 大きさの制約は親のレイアウトで効くので、スタックに入れてから測る。
    let stack = ui.stack(Orientation::Vertical)?;
    stack.append(&list);
    let root = stack.native_view();
    root.setFrameSize(NSSize::new(400.0, 300.0));
    root.layoutSubtreeIfNeeded();

    // スクロールビューは大きさの指定どおりになり、表は幅いっぱいに広がる。
    let frame = list.native_view().frame();
    assert!(
        (frame.size.width - 240.0).abs() < 1e-6 && (frame.size.height - 120.0).abs() < 1e-6,
        "指定した大きさになること: {frame:?}"
    );
    let table = list.native_table();
    assert!(
        table.frame().size.width > 0.0,
        "NSTableView がスクロールの中で幅を持つこと"
    );

    let cell = table
        .viewAtColumn_row_makeIfNecessary(0, 0, true)
        .expect("1 行目のビューが作られること")
        .downcast::<objc2_app_kit::NSTableCellView>()
        .expect("行は NSTableCellView であること");
    let field = unsafe { cell.textField() }.expect("行に文字が入っていること");
    assert_eq!(field.stringValue().to_string(), "日本語の行");
    Ok(())
}

/// AppKit 側で選択が変わったときも、そのままクロージャへ届く。
///
/// `select` は naui が通知を出す経路なので、ここでは naui を通さずに
/// `NSTableView` を直接動かし、デリゲート経由の通知を確かめる。
fn list_native_selection_notifies(ui: &Ui) -> Result<()> {
    let list = ui.list()?;
    list.set_items(&ListItem::list(["朝", "昼", "夜"]));
    list.set_selection_mode(SelectionMode::Multiple);

    let seen: Rc<RefCell<Vec<Vec<usize>>>> = Rc::new(RefCell::new(Vec::new()));
    list.on_select({
        let seen = seen.clone();
        move |indices| seen.borrow_mut().push(indices.to_vec())
    });

    let table = list.native_table();
    let rows = objc2_foundation::NSMutableIndexSet::new();
    rows.addIndex(0);
    rows.addIndex(2);
    table.selectRowIndexes_byExtendingSelection(&rows, false);

    assert_eq!(
        *seen.borrow(),
        vec![vec![0, 2]],
        "ネイティブ側の選択がそのまま届くこと"
    );
    assert_eq!(list.selection(), vec![0, 2]);
    Ok(())
}

/// 通知の中から同じリストを触っても、AppKit ごと壊れない。
///
/// `on_select` の中で行を作り直すと、AppKit がデリゲートを呼んでいる最中に
/// `reloadData` が走る。ギャラリーの「選択に合わせて表示を変える」形が
/// まさにこれなので、実際にやって確かめる。
fn list_callback_can_touch_the_list(ui: &Ui) -> Result<()> {
    let list = ui.list()?;
    list.set_items(&ListItem::list(["春", "夏", "秋", "冬"]));

    let seen: Rc<RefCell<Vec<Vec<usize>>>> = Rc::new(RefCell::new(Vec::new()));
    list.on_select({
        let seen = seen.clone();
        let list = list.clone();
        move |indices| {
            seen.borrow_mut().push(indices.to_vec());
            // 1 回目の通知でだけ、中身ごと差し替える。
            if seen.borrow().len() == 1 {
                list.set_items(&ListItem::list(["朝", "昼"]));
                list.set_selected(1);
            }
        }
    });

    // naui を通さず、AppKit 側から選択を起こす。
    let rows = objc2_foundation::NSMutableIndexSet::new();
    rows.addIndex(2);
    list.native_table()
        .selectRowIndexes_byExtendingSelection(&rows, false);

    assert_eq!(*seen.borrow(), vec![vec![2]], "通知は 1 回だけ");
    assert_eq!(list.len(), 2, "コールバックの中の差し替えが効くこと");
    assert_eq!(list.selected(), Some(1));
    Ok(())
}

/// 行の文字が、行の高さの縦中央に来る。
///
/// 文字だけの `NSTextField` を行にすると、行の高さ (Inset スタイルでは 24pt) に
/// 対して文字の高さ (16pt 前後) が足りず、AppKit が上寄せで描いてしまう。
/// 選択の帯は行いっぱいに出るので、そのままだと帯と文字がずれる。
fn list_rows_are_vertically_centered(ui: &Ui) -> Result<()> {
    let list = ui.list()?;
    list.set_items(&ListItem::list(["札幌", "仙台", "東京"]));
    list.set_sizing(Sizing::fixed(240.0, 180.0));
    let stack = ui.stack(Orientation::Vertical)?;
    stack.append(&list);
    let root = stack.native_view();
    root.setFrameSize(NSSize::new(400.0, 300.0));
    root.layoutSubtreeIfNeeded();

    let table = list.native_table();
    let cell = table
        .viewAtColumn_row_makeIfNecessary(0, 1, true)
        .expect("行のビューが作られること");
    let subviews = cell.subviews();
    assert_eq!(subviews.len(), 1, "行の中身は文字 1 つ");
    let field = subviews.objectAtIndex(0).frame();
    let cell_height = cell.frame().size.height;

    assert!(
        field.size.height < cell_height,
        "文字は行より低いこと (縦中央ぞろえが要る状況であること): {field:?} / {cell_height}"
    );
    let field_center = field.origin.y + field.size.height / 2.0;
    assert!(
        (field_center - cell_height / 2.0).abs() < 0.5,
        "文字が行の縦中央にあること: 文字の中心 {field_center} / 行の中心 {}",
        cell_height / 2.0
    );
    Ok(())
}

/// `detail` を付けた行だけが 2 行になり、そのぶん高くなる。
///
/// 行の高さを決めるのは AppKit (`usesAutomaticRowHeights`) なので、
/// naui 側は制約を張るだけ。高さの数値ではなく「1 行より高い」ことを見る。
fn list_detail_makes_a_second_line(ui: &Ui) -> Result<()> {
    let list = ui.list()?;
    list.set_items(&[
        ListItem::new("東京"),
        ListItem::new("大阪").detail("2,750,000 人"),
    ]);
    list.set_sizing(Sizing::fixed(240.0, 180.0));
    let stack = ui.stack(Orientation::Vertical)?;
    stack.append(&list);
    let root = stack.native_view();
    root.setFrameSize(NSSize::new(400.0, 300.0));
    root.layoutSubtreeIfNeeded();

    let table = list.native_table();
    let plain = table
        .viewAtColumn_row_makeIfNecessary(0, 0, true)
        .expect("1 行目のビュー");
    let detailed = table
        .viewAtColumn_row_makeIfNecessary(0, 1, true)
        .expect("2 行目のビュー");
    assert_eq!(plain.subviews().len(), 1, "detail が無い行は文字 1 本");
    assert_eq!(detailed.subviews().len(), 2, "detail がある行は文字 2 本");

    let plain_height = table.rectOfRow(0).size.height;
    let detailed_height = table.rectOfRow(1).size.height;
    assert!(
        detailed_height > plain_height,
        "2 行になった行のほうが高いこと: {detailed_height} <= {plain_height}"
    );

    // 補助の文字は本文の下に来る。NSTableCellView は反転していないので、
    // 画面の下ほど y が小さい (反転していれば逆になる)。
    let title = detailed.subviews().objectAtIndex(0).frame();
    let sub = detailed.subviews().objectAtIndex(1).frame();
    let title_is_above = if detailed.isFlipped() {
        title.origin.y + title.size.height <= sub.origin.y
    } else {
        sub.origin.y + sub.size.height <= title.origin.y
    };
    assert!(
        title_is_above,
        "補助の文字が本文の下にあること: 本文 {title:?} / 補助 {sub:?}"
    );
    assert!(
        sub.size.height < title.size.height,
        "補助の文字のほうが小さいこと: {} / {}",
        sub.size.height,
        title.size.height
    );
    Ok(())
}

/// 項目と区切り線が、そのまま NSMenu の中身になる。
fn popup_menu_items_map_to_native(ui: &Ui) -> Result<()> {
    let popup = ui.popup_menu()?;
    assert!(popup.is_empty());

    popup.set_items(&[
        PopupItem::new("コピー"),
        PopupItem::separator(),
        PopupItem::new("削除").enabled(false),
    ]);
    assert_eq!(popup.len(), 3, "区切り線も数に入ること");

    let menu = popup.native_menu();
    assert_eq!(menu.numberOfItems(), 3);
    assert!(
        !menu.autoenablesItems(),
        "AppKit の自動有効化を切らないと enabled(false) が無視される"
    );

    let copy = menu.itemAtIndex(0).expect("1 番目の項目");
    assert_eq!(copy.title().to_string(), "コピー");
    assert!(copy.isEnabled());
    assert!(menu.itemAtIndex(1).expect("2 番目の項目").isSeparatorItem());
    let remove = menu.itemAtIndex(2).expect("3 番目の項目");
    assert!(!remove.isEnabled(), "選べない項目は無効のままであること");

    // 作り直すと以前の項目は残らない。
    popup.set_items(&PopupItem::list(["貼り付け"]));
    assert_eq!(popup.len(), 1);
    assert_eq!(popup.native_menu().numberOfItems(), 1);
    Ok(())
}

/// 項目を選ぶと、区切り線を含めた並びの位置がクロージャへ届く。
fn popup_menu_selection_notifies(ui: &Ui) -> Result<()> {
    let popup = ui.popup_menu()?;
    popup.set_items(&[
        PopupItem::new("コピー"),
        PopupItem::separator(),
        PopupItem::new("貼り付け"),
        PopupItem::new("削除").enabled(false),
    ]);

    let seen = Rc::new(RefCell::new(Vec::new()));
    popup.on_select({
        let seen = seen.clone();
        move |index| seen.borrow_mut().push(index)
    });

    popup.select(2);
    assert_eq!(*seen.borrow(), vec![2], "区切り線を数えた位置で届くこと");

    // 区切り線・選べない項目・範囲外は通知しない。
    popup.select(1);
    popup.select(3);
    popup.select(9);
    assert_eq!(*seen.borrow(), vec![2]);
    Ok(())
}

/// 取り付けたウィジェットのビューが、そのメニューを持つようになる。
fn popup_menu_attaches_to_a_view(ui: &Ui) -> Result<()> {
    let label = ui.label("右クリックしてください")?;
    let popup = ui.popup_menu()?;
    popup.set_items(&PopupItem::list(["コピー"]));
    assert!(label.native_view().menu().is_none());

    popup.attach(&label);
    let attached = label.native_view().menu().expect("ビューのメニュー");
    assert!(
        std::ptr::eq(&*attached, &*popup.native_menu()),
        "取り付けたメニューそのものであること"
    );

    // 出していないメニューを閉じても落ちない。
    popup.close();
    Ok(())
}

/// 見出し・本文・中身のウィジェットが `NSAlert` へそのまま渡ること。
///
/// `open()` は `runModal` に入って戻らなくなるため、テストでは
/// `native_alert()` で組み立てだけを確かめる (表示はしない)。
fn dialog_configuration_reaches_the_alert(ui: &Ui) -> Result<()> {
    let dialog = ui.dialog("削除しますか")?;
    assert_eq!(dialog.title(), "削除しますか");
    dialog.set_message("この操作は元に戻せません。");
    assert_eq!(dialog.message(), "この操作は元に戻せません。");
    assert!(!dialog.is_open(), "作っただけでは出ていないこと");

    let field = ui.text_input("メモ")?;
    dialog.set_child(&field);

    let alert = dialog.native_alert();
    assert_eq!(alert.messageText().to_string(), "削除しますか");
    assert_eq!(
        alert.informativeText().to_string(),
        "この操作は元に戻せません。"
    );

    let accessory = alert.accessoryView().expect("中身が accessoryView に載る");
    let text_field = accessory
        .downcast::<NSTextField>()
        .expect("実体は NSTextField であること");
    assert_eq!(text_field.stringValue().to_string(), "メモ");
    // `NSAlert` は frame を見て場所を空けるので、大きさが入っていること。
    assert!(
        text_field.frame().size.width > 0.0 && text_field.frame().size.height > 0.0,
        "Auto Layout の子にも frame が入ること: {:?}",
        text_field.frame().size
    );

    // `NSAlert` にレイアウトさせても、中身が潰れずウィンドウの中に載ること。
    // (表示はしない。`layout()` はレイアウトを走らせるだけ。)
    alert.layout();
    let alert_window = alert.window();
    let laid_out = text_field.frame();
    assert!(
        laid_out.size.width > 0.0 && laid_out.size.height > 0.0,
        "レイアウト後も大きさを持つこと: {:?}",
        laid_out.size
    );
    assert!(
        unsafe { text_field.superview() }.is_some(),
        "accessoryView がアラートのビュー階層に入ること"
    );
    let content = alert_window.contentView().expect("アラートに contentView がある");
    assert!(
        laid_out.size.width <= content.frame().size.width,
        "中身がアラートの幅からはみ出さないこと: {:?} / {:?}",
        laid_out.size,
        content.frame().size
    );

    // 設定は消えず、何度でも組み立て直せる。
    let again = dialog.native_alert();
    assert_eq!(again.messageText().to_string(), "削除しますか");
    Ok(())
}

/// macOS の並び (左から 副操作・取り消し・主操作) になること。
///
/// `NSAlert` は先に足したものを右端かつ既定のボタンにするので、
/// 足す順は 主操作・取り消し・副操作 になる。
fn dialog_buttons_follow_the_macos_order(ui: &Ui) -> Result<()> {
    let dialog = ui.dialog("保存しますか")?;
    dialog.set_buttons(
        DialogButtons::new()
            .primary("保存")
            .secondary("保存しない")
            .cancel("キャンセル"),
    );
    assert_eq!(
        dialog.buttons().label(DialogResponse::Secondary),
        Some("保存しない"),
        "設定した組み合わせが読み出せること"
    );

    let alert = dialog.native_alert();
    let titles: Vec<String> = alert
        .buttons()
        .iter()
        .map(|button| button.title().to_string())
        .collect();
    assert_eq!(titles, ["保存", "キャンセル", "保存しない"]);

    let buttons = alert.buttons();
    let cancel = buttons.objectAtIndex(1);
    assert_eq!(
        cancel.keyEquivalent().to_string(),
        "\u{1b}",
        "取り消しは Esc で閉じられること"
    );
    let primary = buttons.objectAtIndex(0);
    assert_eq!(
        primary.keyEquivalent().to_string(),
        "\r",
        "先頭が Return の既定ボタンになること"
    );
    Ok(())
}

/// ボタンを 1 つも指定しないと「OK」だけが出ること。
fn dialog_without_buttons_shows_ok(ui: &Ui) -> Result<()> {
    let dialog = ui.dialog("完了しました")?;
    assert!(dialog.buttons().is_empty(), "既定ではボタンを持たないこと");

    let alert = dialog.native_alert();
    let titles: Vec<String> = alert
        .buttons()
        .iter()
        .map(|button| button.title().to_string())
        .collect();
    assert_eq!(titles, ["OK"]);
    Ok(())
}

/// 出していないダイアログへの `close()` が何も壊さないこと。
///
/// `close()` は modal を中断するので、出していないときに呼ぶと
/// AppKit 側が例外を投げうる。呼ばないことを確かめる。
fn dialog_is_closed_until_opened(ui: &Ui) -> Result<()> {
    let dialog = ui.dialog("何か")?;
    assert!(!dialog.is_open());
    dialog.close();
    dialog.close();
    assert!(!dialog.is_open());

    // 通知は設定できるが、閉じていないので呼ばれない。
    let called = Rc::new(RefCell::new(Vec::new()));
    dialog.on_response({
        let called = called.clone();
        move |response| called.borrow_mut().push(response)
    });
    dialog.close();
    assert!(called.borrow().is_empty(), "閉じていないので通知されないこと");
    Ok(())
}

/// ツリーが NSOutlineView の行として並び、開くと子の行が増える。
fn tree_rows_follow_the_expansion(ui: &Ui) -> Result<()> {
    let tree = ui.tree()?;
    tree.set_items(&sample_tree());
    assert_eq!(tree.len(), 6, "子孫まで数えること");
    assert!(!tree.is_empty());

    let outline = tree.native_outline_view();
    // 何も開いていないので、見えているのは根の 2 つだけ。
    assert_eq!(outline.numberOfRows(), 2);
    assert!(!tree.is_expanded(&[0]));

    tree.set_expanded(&[0], true);
    assert!(tree.is_expanded(&[0]));
    assert_eq!(outline.numberOfRows(), 4, "src の子 2 つが増えること");
    assert_eq!(outline.levelForRow(1), 1, "子は 1 段下がること");

    // 孫まで開くと、間の枝もまとめて開く。
    tree.set_expanded(&[1, 0], true);
    assert!(tree.is_expanded(&[1]), "祖先も開かれること");
    assert_eq!(outline.numberOfRows(), 6, "すべての項目が見えること");

    tree.collapse_all();
    assert_eq!(outline.numberOfRows(), 2);
    assert!(!tree.is_expanded(&[0]));

    tree.expand_all();
    assert_eq!(outline.numberOfRows(), 6, "すべての行が見えること");

    // 作り直すと、TreeItem::expanded のとおりに戻る。
    tree.set_items(&[TreeItem::new("親")
        .expanded(true)
        .children(TreeItem::list(["子"]))]);
    assert_eq!(tree.len(), 2);
    assert!(tree.is_expanded(&[0]));
    assert_eq!(tree.native_outline_view().numberOfRows(), 2);
    Ok(())
}

/// ツリーの選択が NSOutlineView と往復し、`set_selected` は通知しない。
fn tree_selection_round_trips(ui: &Ui) -> Result<()> {
    let tree = ui.tree()?;
    tree.set_items(&sample_tree());
    assert_eq!(tree.selected(), None, "作った直後は何も選ばれていないこと");

    let seen: Rc<RefCell<Vec<Vec<usize>>>> = Rc::new(RefCell::new(Vec::new()));
    tree.on_select({
        let seen = seen.clone();
        move |path| seen.borrow_mut().push(path.to_vec())
    });

    // 閉じた枝の中を選ぶと、見えるように祖先が開く。
    tree.select(&[0, 1]);
    assert_eq!(tree.selected(), Some(vec![0, 1]));
    assert!(tree.is_expanded(&[0]));
    assert_eq!(*seen.borrow(), vec![vec![0, 1]]);

    let outline = tree.native_outline_view();
    assert_eq!(outline.selectedRow(), 2, "ネイティブ側も選ばれていること");

    // `set_selected` は通知しない。
    tree.set_selected(&[1]);
    assert_eq!(tree.selected(), Some(vec![1]));
    assert_eq!(*seen.borrow(), vec![vec![0, 1]], "通知は 1 回だけ");

    // 無いパスを選ぶと選択が外れ、空のパスで通知される。
    tree.select(&[9]);
    assert_eq!(tree.selected(), None);
    assert_eq!(*seen.borrow(), vec![vec![0, 1], Vec::new()]);

    // clear_selection は通知しない。
    tree.set_selected(&[1]);
    tree.clear_selection();
    assert_eq!(tree.selected(), None);
    assert_eq!(seen.borrow().len(), 2);

    // 作り直すと選択は外れる。
    tree.set_selected(&[1]);
    tree.set_items(&sample_tree());
    assert_eq!(tree.selected(), None);
    assert_eq!(seen.borrow().len(), 2, "作り直しも通知しない");
    Ok(())
}

/// 選べない枝は、その子孫までまとめて選べない。
fn tree_skips_disabled_branches(ui: &Ui) -> Result<()> {
    let tree = ui.tree()?;
    tree.set_items(&[
        TreeItem::new("有効").child(TreeItem::new("子")),
        TreeItem::new("無効")
            .enabled(false)
            .child(TreeItem::new("孫")),
    ]);
    tree.expand_all();

    let seen: Rc<RefCell<Vec<Vec<usize>>>> = Rc::new(RefCell::new(Vec::new()));
    tree.on_select({
        let seen = seen.clone();
        move |path| seen.borrow_mut().push(path.to_vec())
    });

    tree.select(&[1]);
    assert_eq!(tree.selected(), None, "無効な枝は選べないこと");
    tree.select(&[1, 0]);
    assert_eq!(tree.selected(), None, "無効な枝の中身も選べないこと");
    assert_eq!(*seen.borrow(), vec![Vec::<usize>::new(), Vec::new()]);

    tree.select(&[0, 0]);
    assert_eq!(tree.selected(), Some(vec![0, 0]));

    // AppKit 自身にも「この項目は選べない」と伝わっている。
    let outline = tree.native_outline_view();
    let delegate = unsafe { outline.delegate() }.expect("デリゲートがあること");
    let enabled = outline.itemAtRow(0).expect("1 行目の項目があること");
    let disabled = outline.itemAtRow(2).expect("3 行目の項目があること");
    unsafe {
        assert!(delegate.outlineView_shouldSelectItem(&outline, &enabled));
        assert!(!delegate.outlineView_shouldSelectItem(&outline, &disabled));
    }
    Ok(())
}

/// 開閉の通知は、ネイティブ側の操作でも naui の `expand` でも届く。
fn tree_expansion_notifies(ui: &Ui) -> Result<()> {
    let tree = ui.tree()?;
    tree.set_items(&sample_tree());

    let seen: Rc<RefCell<Vec<(Vec<usize>, bool)>>> = Rc::new(RefCell::new(Vec::new()));
    tree.on_expand({
        let seen = seen.clone();
        move |path, expanded| seen.borrow_mut().push((path.to_vec(), expanded))
    });

    tree.expand(&[0]);
    tree.collapse(&[0]);
    assert_eq!(
        *seen.borrow(),
        vec![(vec![0], true), (vec![0], false)],
        "naui からの開閉が 1 回ずつ通知されること"
    );

    // `set_expanded` と一括の開閉は通知しない。
    tree.set_expanded(&[0], true);
    tree.expand_all();
    tree.collapse_all();
    assert_eq!(seen.borrow().len(), 2);

    // naui を通さず、AppKit 側から開く。
    let outline = tree.native_outline_view();
    let item = outline.itemAtRow(1).expect("2 行目の項目があること");
    unsafe { outline.expandItem(Some(&item)) };
    assert_eq!(
        seen.borrow().last(),
        Some(&(vec![1], true)),
        "ネイティブ側の開閉もそのまま届くこと"
    );
    assert!(tree.is_expanded(&[1]));

    // 葉は開けないので、通知も起きない。
    tree.expand(&[1, 0, 0]);
    assert_eq!(seen.borrow().len(), 3);
    Ok(())
}

/// 項目の中身は、AppKit がデリゲートに作らせた NSTextField になる。
fn tree_rows_are_native_views(ui: &Ui) -> Result<()> {
    let tree = ui.tree()?;
    tree.set_items(&[TreeItem::new("プロジェクト")
        .detail("3 ファイル")
        .expanded(true)
        .child(TreeItem::new("main.rs"))]);
    tree.set_sizing(Sizing::fixed(260.0, 140.0));

    let stack = ui.stack(Orientation::Vertical)?;
    stack.append(&tree);
    let root = stack.native_view();
    root.setFrameSize(NSSize::new(400.0, 300.0));
    root.layoutSubtreeIfNeeded();

    let frame = tree.native_view().frame();
    assert!(
        (frame.size.width - 260.0).abs() < 1e-6 && (frame.size.height - 140.0).abs() < 1e-6,
        "指定した大きさになること: {frame:?}"
    );

    let outline = tree.native_outline_view();
    let cell = outline
        .viewAtColumn_row_makeIfNecessary(0, 0, true)
        .expect("1 行目のビューが作られること")
        .downcast::<objc2_app_kit::NSTableCellView>()
        .expect("行は NSTableCellView であること");
    let field = unsafe { cell.textField() }.expect("行に文字が入っていること");
    assert_eq!(field.stringValue().to_string(), "プロジェクト");
    assert_eq!(cell.subviews().len(), 2, "補助の文字が 2 行目に出ること");

    // 子の行も同じように作られる。
    let child = outline
        .viewAtColumn_row_makeIfNecessary(0, 1, true)
        .expect("2 行目のビューが作られること")
        .downcast::<objc2_app_kit::NSTableCellView>()
        .expect("行は NSTableCellView であること");
    let field = unsafe { child.textField() }.expect("行に文字が入っていること");
    assert_eq!(field.stringValue().to_string(), "main.rs");
    Ok(())
}

/// 通知の中から同じツリーを触っても壊れない (リストと同じ形)。
fn tree_callback_can_touch_the_tree(ui: &Ui) -> Result<()> {
    let tree = ui.tree()?;
    tree.set_items(&sample_tree());
    tree.expand_all();

    let seen: Rc<RefCell<Vec<Vec<usize>>>> = Rc::new(RefCell::new(Vec::new()));
    tree.on_select({
        let seen = seen.clone();
        let tree = tree.clone();
        move |path| {
            seen.borrow_mut().push(path.to_vec());
            // 1 回目の通知でだけ、中身ごと差し替える。
            if seen.borrow().len() == 1 {
                tree.set_items(&[TreeItem::new("別の木").child(TreeItem::new("葉"))]);
                tree.set_selected(&[0]);
            }
        }
    });

    // naui を通さず、AppKit 側から選択を起こす。
    let rows = objc2_foundation::NSMutableIndexSet::new();
    rows.addIndex(1);
    tree.native_outline_view()
        .selectRowIndexes_byExtendingSelection(&rows, false);

    assert_eq!(*seen.borrow(), vec![vec![0, 0]], "通知は 1 回だけ");
    assert_eq!(tree.len(), 2, "コールバックの中の差し替えが効くこと");
    assert_eq!(tree.selected(), Some(vec![0]));
    Ok(())
}

/// テストで使う木。src (2 つの葉) と docs (guide > intro)。
fn sample_tree() -> Vec<TreeItem> {
    vec![
        TreeItem::new("src").children([TreeItem::new("main.rs"), TreeItem::new("lib.rs")]),
        TreeItem::new("docs").child(TreeItem::new("guide").child(TreeItem::new("intro.md"))),
    ]
}

/// 親を閉じても、その中の開閉は覚えられていて、開き直すと元に戻る。
///
/// AppKit (Finder と同じ) の動きに合わせている。開き直したときに AppKit が
/// もう一度出す通知は、naui から見た状態が変わっていないので出さない。
fn tree_remembers_expansion_inside_a_closed_branch(ui: &Ui) -> Result<()> {
    let tree = ui.tree()?;
    tree.set_items(&sample_tree());
    tree.set_expanded(&[1, 0], true);
    let outline = tree.native_outline_view();
    assert_eq!(
        outline.numberOfRows(),
        4,
        "docs > guide > intro.md まで開くこと"
    );

    let seen: Rc<RefCell<Vec<(Vec<usize>, bool)>>> = Rc::new(RefCell::new(Vec::new()));
    tree.on_expand({
        let seen = seen.clone();
        move |path, expanded| seen.borrow_mut().push((path.to_vec(), expanded))
    });

    tree.collapse(&[1]);
    assert_eq!(outline.numberOfRows(), 2);
    assert!(
        tree.is_expanded(&[1, 0]),
        "見えなくなっても、中の開閉は覚えていること"
    );

    tree.expand(&[1]);
    assert_eq!(outline.numberOfRows(), 4, "中の開閉ごと戻ること");
    assert_eq!(
        *seen.borrow(),
        vec![(vec![1], false), (vec![1], true)],
        "通知は操作した枝の分だけ"
    );
    Ok(())
}
