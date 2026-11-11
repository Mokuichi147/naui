//! Checks of a right-to-left layout against real AppKit controls.
//!
//! AppKit decides the app's layout direction once, when `NSApplication` is created,
//! so these run in a process of their own that fixes the direction before that, the
//! way `run` does for `Settings::layout_direction(LayoutDirection::Rtl)`.
//!
//! Like `appkit.rs`, `harness = false` runs them on the main thread.

use std::panic::{catch_unwind, AssertUnwindSafe};

use naui_core::{
    Align, GridCell, LayoutDirection, Orientation, Padding, Result, SidebarItem, SidebarSection,
    TableColumn, TableRow,
};
use naui_macos::{run_for_test, Ui, Widget};
use objc2::rc::Retained;
use objc2::MainThreadMarker;
use objc2_app_kit::{
    NSApplication, NSBox, NSSplitViewController, NSTableCellView, NSTextAlignment, NSTextField,
    NSView, NSWritingDirection,
};
use objc2_foundation::NSSize;

/// One test case.
type Case = (&'static str, fn(&Ui) -> Result<()>);

fn main() {
    // NSApplication を作る前に、run と同じ手順で方向を置く。
    naui_macos::apply_layout_direction(Some(LayoutDirection::Rtl));
    naui_core::set_app_layout_direction(Some(LayoutDirection::Rtl));

    let cases: &[Case] = &[
        ("アプリ全体が右から左になる", app_runs_right_to_left),
        ("横の Stack は最初の子を右に置く", stack_starts_at_the_right),
        (
            "Grid の 0 列目は右端に来る",
            grid_columns_start_at_the_right,
        ),
        (
            "Group の余白の start は先頭側 (右) に付く",
            group_padding_is_mirrored,
        ),
        (
            "数値入力は欄が左・ボタンが右のまま、左から右に書く",
            number_input_is_not_mirrored,
        ),
        (
            "表の Start は右揃え、End は左揃えになる",
            table_alignment_is_mirrored,
        ),
        ("サイドバーが右に付く", sidebar_sits_at_the_right),
        ("ラベルと入力欄の文字は右に寄る", text_starts_at_the_right),
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
    println!(
        "\n{} 件中 {} 件成功 / {} 件失敗",
        cases.len(),
        cases.len() - failed,
        failed
    );
    if failed > 0 {
        std::process::exit(1);
    }
}

/// The x of `view`'s frame in `ancestor`'s coordinates.
fn x_in(view: &NSView, ancestor: &NSView) -> f64 {
    view.convertRect_toView(view.bounds(), Some(ancestor))
        .origin
        .x
}

fn app_runs_right_to_left(ui: &Ui) -> Result<()> {
    assert_eq!(ui.layout_direction(), LayoutDirection::Rtl);
    let mtm = MainThreadMarker::new().unwrap();
    assert_eq!(
        NSApplication::sharedApplication(mtm).userInterfaceLayoutDirection(),
        objc2_app_kit::NSUserInterfaceLayoutDirection::RightToLeft
    );
    Ok(())
}

fn stack_starts_at_the_right(ui: &Ui) -> Result<()> {
    let stack = ui.stack(Orientation::Horizontal)?;
    let first = ui.button("一")?;
    let second = ui.button("二")?;
    stack.append(&first);
    stack.append(&second);
    let view = stack.native_view();
    view.setFrameSize(NSSize::new(300.0, 60.0));
    view.layoutSubtreeIfNeeded();
    let first_x = x_in(&first.native_view(), &view);
    let second_x = x_in(&second.native_view(), &view);
    assert!(
        first_x > second_x,
        "最初の子が右に来ること: 一 {first_x} / 二 {second_x}"
    );
    Ok(())
}

fn grid_columns_start_at_the_right(ui: &Ui) -> Result<()> {
    let grid = ui.grid()?;
    let first = ui.label("一")?;
    let second = ui.label("二")?;
    grid.attach(&first, GridCell::new(0, 0));
    grid.attach(&second, GridCell::new(1, 0));
    let view = grid.native_view();
    view.setFrameSize(NSSize::new(300.0, 60.0));
    view.layoutSubtreeIfNeeded();
    let first_x = x_in(&first.native_view(), &view);
    let second_x = x_in(&second.native_view(), &view);
    assert!(
        first_x > second_x,
        "0 列目が右に来ること: 一 {first_x} / 二 {second_x}"
    );
    Ok(())
}

/// `start` is the side where rows start, as with `Stack`'s padding (`NSStackView`'s
/// `edgeInsets`), so the whole layout is mirrored together.
fn group_padding_is_mirrored(ui: &Ui) -> Result<()> {
    let group = ui.group("")?;
    let body = ui.stack(Orientation::Vertical)?;
    body.append(&ui.label("中身")?);
    group.set_child(&body);
    group.set_padding(Padding {
        top: 2.0,
        bottom: 2.0,
        start: 4.0,
        end: 20.0,
    });
    let view = group.native_view();
    let native = view.downcast_ref::<NSBox>().expect("NSBox であること");
    let content = native.contentView().expect("contentView");
    view.setFrameSize(NSSize::new(300.0, 200.0));
    view.layoutSubtreeIfNeeded();
    let inside = content.frame();
    let frame = body.native_view().frame();
    let right_gap = inside.size.width - frame.origin.x - frame.size.width;
    assert!(
        (right_gap - 4.0).abs() < 0.5,
        "start の余白は右に付く: {inside:?} / {frame:?}"
    );
    assert!(
        (frame.origin.x - 20.0).abs() < 0.5,
        "end の余白は左に付く: {frame:?}"
    );
    Ok(())
}

fn number_input_is_not_mirrored(ui: &Ui) -> Result<()> {
    let input = ui.number_input(-5.0)?;
    let view = input.native_view();
    view.setFrameSize(NSSize::new(160.0, 30.0));
    view.layoutSubtreeIfNeeded();
    let field = input.native_field();
    let stepper = input.native_stepper();
    let field_x = x_in(&field, &view);
    let stepper_x = x_in(&stepper, &view);
    assert!(
        field_x < stepper_x,
        "欄が左、ボタンが右のまま: 欄 {field_x} / ボタン {stepper_x}"
    );
    assert_eq!(
        field.baseWritingDirection(),
        NSWritingDirection::LeftToRight
    );
    assert_eq!(field.alignment(), NSTextAlignment::Right);
    Ok(())
}

fn table_alignment_is_mirrored(ui: &Ui) -> Result<()> {
    let table = ui.table()?;
    let mut columns = TableColumn::list(["名前", "数"]);
    columns[1] = columns[1].clone().align(Align::End);
    table.set_columns(&columns);
    table.set_rows(&TableRow::list([["あ", "1"]]));
    let native = table.native_table();
    let alignment = |column: isize| {
        let cell: Retained<NSTableCellView> = native
            .viewAtColumn_row_makeIfNecessary(column, 0, true)
            .expect("セルが作られること")
            .downcast()
            .expect("NSTableCellView であること");
        unsafe { cell.textField() }.expect("セルの文字").alignment()
    };
    assert_eq!(alignment(0), NSTextAlignment::Right, "Start は右揃え");
    assert_eq!(alignment(1), NSTextAlignment::Left, "End は左揃え");
    Ok(())
}

fn sidebar_sits_at_the_right(ui: &Ui) -> Result<()> {
    let window = ui.window("サイドバー", 640.0, 400.0)?;
    let root = ui.stack(Orientation::Vertical)?;
    root.append(&ui.label("本文")?);
    window.set_child(&root);
    let sidebar = ui.sidebar()?;
    sidebar.set_sections(&[SidebarSection::new("", vec![SidebarItem::new("項目")])]);
    window.set_sidebar(&sidebar);
    let native_window = window.native_window();
    let controller = native_window
        .contentViewController()
        .and_then(|c| c.downcast::<NSSplitViewController>().ok())
        .expect("NSSplitViewController");
    controller.view().layoutSubtreeIfNeeded();
    let mtm = MainThreadMarker::new().unwrap();
    let items = controller.splitViewItems();
    let side = items.objectAtIndex(0).viewController(mtm).view();
    let main = items.objectAtIndex(1).viewController(mtm).view();
    let whole = controller.view();
    let side_x = x_in(&side, &whole);
    let main_x = x_in(&main, &whole);
    assert!(
        side_x > main_x,
        "サイドバーが右に来ること: サイドバー {side_x} / 本文 {main_x}"
    );
    window.clear_sidebar();
    native_window.close();
    Ok(())
}

/// Text that fills its row starts at the right. AppKit's natural alignment follows
/// the app's direction, but fields built with `init` or `new` start out aligned to
/// the left, which kept the rows of a `List` on the left.
fn text_starts_at_the_right(ui: &Ui) -> Result<()> {
    let label = ui.label("Inbox")?;
    let password = ui.password_input()?;
    let search = ui.search_input()?;
    let combo = ui.editable_combo_box()?;
    for (name, view) in [
        ("ラベル", label.native_view()),
        ("パスワード入力", password.native_view()),
        ("検索入力", search.native_view()),
        ("自由入力コンボボックス", combo.native_view()),
    ] {
        let field = view
            .downcast_ref::<NSTextField>()
            .unwrap_or_else(|| panic!("{name}は NSTextField"));
        assert_eq!(field.alignment(), NSTextAlignment::Natural, "{name}");
    }

    // 自然な揃えの英字は、右から左のアプリでは右に描かれる。
    let view = label.native_view();
    view.setFrameSize(NSSize::new(300.0, 20.0));
    let rep = view
        .bitmapImageRepForCachingDisplayInRect(view.bounds())
        .expect("画素の入れ物");
    view.cacheDisplayInRect_toBitmapImageRep(view.bounds(), &rep);
    let width = rep.pixelsWide();
    let inked = (0..width).filter(|&x| {
        (0..rep.pixelsHigh()).any(|y| {
            rep.colorAtX_y(x, y)
                .is_some_and(|color| color.alphaComponent() > 0.3)
        })
    });
    let first = inked.min().expect("文字が描かれていること");
    assert!(
        first > width / 2,
        "文字が右半分に描かれること: 描き始め {first} / 幅 {width}"
    );
    Ok(())
}
