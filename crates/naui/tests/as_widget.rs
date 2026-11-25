//! Types made outside naui go into every container through `AsWidget`, and the
//! common methods reach the widget they return. Run with `naui::testing` on every
//! platform's real event loop.
//!
//! ```sh
//! cargo test -p naui --test as_widget                                   # macOS, Linux, Windows
//! cargo test -p naui --test as_widget --target wasm32-unknown-unknown   # the Web, in a browser
//! ```
//!
//! What is placed is a [`Probe`]: a type of the test's own holding a `Canvas`,
//! whose size stays zero until it is laid out in a shown window. A probe is
//! dropped right after it is passed, so a size above zero also shows that the
//! container keeps the widget returned rather than the value passed.

use std::cell::RefCell;
use std::rc::Rc;

use naui::{
    testing, AsWidget, Canvas, DialogButtons, DialogResponse, EventResponse, FileEntry, GridCell,
    Key, KeyEvent, Length, ListRow, Modifiers, Orientation, Result, Shortcut, Sizing, TableCells,
    TableColumn, Ui, Widget, Window,
};

naui::test_cases!(
    own_types_go_into_layout_containers,
    own_types_go_into_list_and_table_rows,
    own_types_go_into_sheets_dialogs_and_popovers,
    common_methods_reach_the_inner_widget,
);

/// A type made outside naui: what it places is the `Canvas` inside it.
struct Probe {
    canvas: Canvas,
}

impl AsWidget for Probe {
    fn as_widget(&self) -> &dyn Widget {
        &self.canvas
    }
}

/// A probe, and a handle on its `Canvas` to look at once the probe is gone.
fn probe(ui: &Ui) -> Result<(Probe, Canvas)> {
    let canvas = ui.canvas()?;
    canvas.set_sizing(Sizing::fixed(40.0, 16.0));
    Ok((
        Probe {
            canvas: canvas.clone(),
        },
        canvas,
    ))
}

/// A type of your own made of another one: `as_widget` passes the inner one on.
struct Screen<T: AsWidget> {
    inner: T,
}

impl<T: AsWidget> AsWidget for Screen<T> {
    fn as_widget(&self) -> &dyn Widget {
        self.inner.as_widget()
    }
}

/// A screen holding a vertical stack, as a screen crate's view does.
struct Column {
    root: naui::Stack,
}

impl AsWidget for Column {
    fn as_widget(&self) -> &dyn Widget {
        &self.root
    }
}

/// Waits until every canvas is laid out in the shown window.
async fn wait_laid_out(ui: &Ui, what: &str, canvases: &[(&str, Canvas)]) -> Result<()> {
    let laid_out = || canvases.iter().all(|(_, canvas)| canvas.size().0 > 0.0);
    if testing::wait_until(ui, laid_out).await.is_err() {
        let sizes: Vec<String> = canvases
            .iter()
            .map(|(name, canvas)| format!("{name} {:?}", canvas.size()))
            .collect();
        panic!("{what}: not all laid out: {sizes:#?}");
    }
    Ok(())
}

fn shown_window(ui: &Ui, title: &str, width: f64, height: f64) -> Result<Window> {
    let window = ui.window(title, width, height)?;
    window.show();
    Ok(window)
}

/// `Window::set_child`, `Stack`, `Grid`, `Flow`, `Scroll`, `Group`, `Expander`,
/// `SplitView`, `AdaptivePanes`, `Tabs` and `PopupMenu::attach` take a type of
/// your own, a `&dyn Widget` and a `&dyn AsWidget`.
async fn own_types_go_into_layout_containers(ui: Ui) -> Result<()> {
    let window = shown_window(&ui, "as_widget layout", 900.0, 760.0)?;
    let column = ui.stack(Orientation::Vertical)?;
    column.set_sizing(Sizing::fill());
    // 自前の型を包んだ自前の型を窓に置く。
    window.set_child(&Screen {
        inner: Column {
            root: column.clone(),
        },
    });
    let mut placed = Vec::new();

    // Stack: append / insert、`&dyn Widget` と `&dyn AsWidget` の値も。
    let stack = ui.stack(Orientation::Horizontal)?;
    let (p, c) = probe(&ui)?;
    stack.append(&p);
    placed.push(("Stack::append", c));
    let (p, c) = probe(&ui)?;
    stack.insert(0, &p);
    placed.push(("Stack::insert", c));
    let (_, c) = probe(&ui)?;
    let as_dyn_widget: &dyn Widget = &c;
    stack.append(as_dyn_widget);
    placed.push(("Stack::append(&dyn Widget)", c.clone()));
    let (p, c) = probe(&ui)?;
    let boxed: Box<dyn AsWidget> = Box::new(p);
    stack.append(boxed.as_ref());
    drop(boxed);
    placed.push(("Stack::append(&dyn AsWidget)", c));
    assert_eq!(stack.len(), 4);
    column.append(&stack);

    // Grid: attach / replace。
    let grid = ui.grid()?;
    let (p, c) = probe(&ui)?;
    grid.attach(&p, GridCell::new(0, 0));
    placed.push(("Grid::attach", c));
    let (p, _) = probe(&ui)?;
    grid.attach(&p, GridCell::new(1, 0));
    let (p, c) = probe(&ui)?;
    grid.replace(&p, GridCell::new(1, 0));
    placed.push(("Grid::replace", c));
    assert_eq!(grid.len(), 2, "replace takes the place of what was there");
    column.append(&grid);

    // Flow: append / insert。
    let flow = ui.flow()?;
    flow.set_sizing(Sizing::fill_width());
    let (p, c) = probe(&ui)?;
    flow.append(&p);
    placed.push(("Flow::append", c));
    let (p, c) = probe(&ui)?;
    flow.insert(0, &p);
    placed.push(("Flow::insert", c));
    assert_eq!(flow.len(), 2);
    column.append(&flow);

    // 子を 1 つ持つコンテナ。
    let scroll = ui.scroll()?;
    // Scroll / AdaptivePanes / Tabs は、幅を Fill・高さを Fixed にすると macOS で
    // 縦の Stack ごと中身の幅まで縮む (この変更とは別の既存の不具合) ので、固定の
    // 大きさにする。
    scroll.set_sizing(Sizing::fixed(200.0, 40.0));
    let (p, c) = probe(&ui)?;
    scroll.set_child(&p);
    placed.push(("Scroll::set_child", c));
    column.append(&scroll);

    let group = ui.group("Group")?;
    let (p, c) = probe(&ui)?;
    group.set_child(&p);
    placed.push(("Group::set_child", c));
    column.append(&group);

    let expander = ui.expander("Expander")?;
    expander.set_expanded(true);
    let (p, c) = probe(&ui)?;
    expander.set_child(&p);
    placed.push(("Expander::set_child", c));
    column.append(&expander);

    let split = ui.split_view(Orientation::Horizontal)?;
    split.set_sizing(Sizing::fill_width().height(Length::Fixed(60.0)));
    let (p, c) = probe(&ui)?;
    split.set_start(&p);
    placed.push(("SplitView::set_start", c));
    let (p, c) = probe(&ui)?;
    split.set_end(&p);
    placed.push(("SplitView::set_end", c));
    column.append(&split);

    let panes = ui.adaptive_panes()?;
    // 折りたたまず、両方の区画を並べる。
    panes.set_breakpoint(0.0);
    panes.set_sizing(Sizing::fixed(400.0, 60.0));
    let (p, c) = probe(&ui)?;
    panes.set_list(&p);
    placed.push(("AdaptivePanes::set_list", c));
    let (p, c) = probe(&ui)?;
    panes.set_detail(&p);
    placed.push(("AdaptivePanes::set_detail", c));
    column.append(&panes);

    let tabs = ui.tabs()?;
    tabs.set_sizing(Sizing::fixed(300.0, 90.0));
    let (p, c) = probe(&ui)?;
    tabs.add_tab("Tab", &p);
    placed.push(("Tabs::add_tab", c));
    assert_eq!(tabs.len(), 1);
    column.append(&tabs);

    // PopupMenu::attach は右クリックで開く印を付けるだけで、見える変化が無い。
    // 自前の型を渡せて、その中のウィジェットが置かれたままであることを見る。
    let menu = ui.popup_menu()?;
    menu.set_items(&[naui::PopupItem::new("Item")]);
    let (p, c) = probe(&ui)?;
    column.append(&p);
    menu.attach(&p);
    placed.push(("PopupMenu::attach", c));

    wait_laid_out(&ui, "layout containers", &placed).await?;
    window.close();
    Ok(())
}

/// `ListRow::new` and `TableCells::cell` take a type of your own, in rows given at
/// once and in rows built when they come into view.
async fn own_types_go_into_list_and_table_rows(ui: Ui) -> Result<()> {
    let window = shown_window(&ui, "as_widget rows", 640.0, 520.0)?;
    let column = ui.stack(Orientation::Vertical)?;
    column.set_sizing(Sizing::fill());
    window.set_child(&column);
    let mut placed = Vec::new();

    let given = ui.list()?;
    given.set_sizing(Sizing::fill_width().height(Length::Fixed(100.0)));
    let (p, c) = probe(&ui)?;
    given.set_rows(&[ListRow::new(&p)]);
    placed.push(("ListRow::new (set_rows)", c));
    column.append(&given);

    // 行を作るたびに作り直す経路。作った Canvas を外から見られるように取っておく。
    let built_canvases = Rc::new(RefCell::new(Vec::new()));
    let built = ui.list()?;
    built.set_sizing(Sizing::fill_width().height(Length::Fixed(100.0)));
    built.set_row_builder(1, {
        let ui = ui.clone();
        let built_canvases = built_canvases.clone();
        move |_| {
            let (p, c) = probe(&ui)?;
            built_canvases.borrow_mut().push(c);
            Ok(ListRow::new(&p))
        }
    });
    column.append(&built);

    let table_canvases = Rc::new(RefCell::new(Vec::new()));
    let table = ui.table()?;
    table.set_sizing(Sizing::fill_width().height(Length::Fixed(120.0)));
    table.set_columns(&[TableColumn::new("Text"), TableColumn::new("Widget")]);
    table.set_row_builder(1, {
        let ui = ui.clone();
        let table_canvases = table_canvases.clone();
        move |_| {
            let (p, c) = probe(&ui)?;
            table_canvases.borrow_mut().push(c);
            Ok(TableCells::new().text("row").cell(&p))
        }
    });
    column.append(&table);

    testing::wait_until(&ui, || {
        !built_canvases.borrow().is_empty() && !table_canvases.borrow().is_empty()
    })
    .await?;
    // 最後に作られた行が、いま見えている行。
    placed.push((
        "ListRow::new (set_row_builder)",
        built_canvases.borrow().last().unwrap().clone(),
    ));
    placed.push((
        "TableCells::cell",
        table_canvases.borrow().last().unwrap().clone(),
    ));
    wait_laid_out(&ui, "rows", &placed).await?;
    window.close();
    Ok(())
}

/// `Sheet::set_child`, `Dialog::set_child`, `Popover::set_child` and
/// `Popover::open` take a type of your own.
async fn own_types_go_into_sheets_dialogs_and_popovers(ui: Ui) -> Result<()> {
    let window = shown_window(&ui, "as_widget overlays", 640.0, 480.0)?;
    let column = ui.stack(Orientation::Vertical)?;
    window.set_child(&column);
    let (anchor, anchor_canvas) = probe(&ui)?;
    column.append(&anchor);

    // Sheet
    let sheet = ui.sheet("Sheet")?;
    sheet.set_buttons(DialogButtons::new().primary("OK").cancel("Cancel"));
    let (p, c) = probe(&ui)?;
    sheet.set_child(&p);
    drop(p);
    sheet.open();
    wait_laid_out(&ui, "sheet", &[("Sheet::set_child", c)]).await?;
    sheet.respond(DialogResponse::Cancel);
    testing::wait_until(&ui, || !sheet.is_open()).await?;
    testing::settle(&ui).await;

    // Dialog
    let dialog = ui.dialog("Dialog")?;
    dialog.set_buttons(DialogButtons::new().primary("OK"));
    let (p, c) = probe(&ui)?;
    dialog.set_child(&p);
    drop(p);
    // 応答した時点の中身の大きさ。
    let size_when_answered = Rc::new(RefCell::new(None));
    dialog.on_response({
        let size_when_answered = size_when_answered.clone();
        let c = c.clone();
        move |_| *size_when_answered.borrow_mut() = Some(c.size())
    });
    if cfg!(target_os = "macos") {
        // macOS の open は答えが出るまで戻らない (runModal)。先に答えを仕込み、
        // 答えた時点で中身が並べ終わっていることを見る。
        dialog.respond(DialogResponse::Primary);
        dialog.open();
        testing::wait_until(&ui, || size_when_answered.borrow().is_some()).await?;
        let answered = size_when_answered.borrow().unwrap();
        assert!(
            answered.0 > 0.0,
            "Dialog::set_child: the content was laid out ({answered:?})"
        );
    } else {
        // ほかの環境は開いている間に見る。答えが届く頃には閉じて、中身の大きさは
        // 0 に戻っていることがある (Web)。
        dialog.open();
        wait_laid_out(&ui, "dialog", &[("Dialog::set_child", c.clone())]).await?;
        dialog.respond(DialogResponse::Primary);
        testing::wait_until(&ui, || size_when_answered.borrow().is_some()).await?;
    }
    testing::settle(&ui).await;

    // Popover: 中身と、開く起点の両方が自前の型。
    wait_laid_out(&ui, "anchor", &[("anchor", anchor_canvas)]).await?;
    let popover = ui.popover()?;
    let (p, c) = probe(&ui)?;
    popover.set_child(&p);
    drop(p);
    popover.open(&anchor);
    testing::wait_until(&ui, || popover.is_open()).await?;
    wait_laid_out(&ui, "popover", &[("Popover::set_child / open", c)]).await?;
    popover.close();
    testing::wait_until(&ui, || !popover.is_open()).await?;

    // PopupMenu::open_at。macOS はメニューを閉じるまで戻らないので開かない
    // (渡せることはコンパイルで見る)。
    let menu = ui.popup_menu()?;
    menu.set_items(&[naui::PopupItem::new("Item")]);
    if !cfg!(target_os = "macos") {
        menu.open_at(&anchor, 4.0, 4.0);
        testing::settle(&ui).await;
        menu.close();
    }
    window.close();
    Ok(())
}

/// A type of your own implementing only `as_widget` gets the operations every
/// widget has (shortcuts included), and they reach the widget inside; so do
/// `&dyn Widget` values.
async fn common_methods_reach_the_inner_widget(ui: Ui) -> Result<()> {
    let window = shown_window(&ui, "as_widget methods", 480.0, 360.0)?;
    let column = ui.stack(Orientation::Vertical)?;
    window.set_child(&column);
    let (probe, canvas) = probe(&ui)?;
    column.append(&probe);

    // set_sizing
    probe.set_sizing(Sizing::fixed(120.0, 50.0));
    testing::wait_until(&ui, || canvas.size() == (120.0, 50.0)).await?;

    // set_visible / is_visible
    assert!(probe.is_visible());
    probe.set_visible(false);
    assert!(!canvas.is_visible());
    assert!(!probe.is_visible());
    probe.set_visible(true);
    assert!(canvas.is_visible());

    // `&dyn Widget` にも同じメソッドがある。
    let as_dyn: &dyn Widget = &canvas;
    as_dyn.set_visible(false);
    assert!(!canvas.is_visible());
    assert!(!as_dyn.is_visible());
    as_dyn.set_visible(true);
    as_dyn.set_sizing(Sizing::fixed(80.0, 30.0));
    testing::wait_until(&ui, || canvas.size() == (80.0, 30.0)).await?;

    // request_focus: 中の入力欄へ移る。
    struct Field {
        input: naui::TextInput,
    }
    impl AsWidget for Field {
        fn as_widget(&self) -> &dyn Widget {
            &self.input
        }
    }
    let field = Field {
        input: ui.text_input("")?,
    };
    column.append(&field);
    testing::wait_until(&ui, || field.request_focus()).await?;

    // on_shortcut / remove_shortcut: 自前の型に付けたショートカットは、中の
    // ウィジェットにフォーカスがある間に効く。
    let fired = Rc::new(RefCell::new(0));
    let escape = Shortcut::from_key(Key::Escape);
    let press_escape = KeyEvent::new(Key::Escape, Modifiers::default());
    field.on_shortcut(escape, {
        let fired = fired.clone();
        move || *fired.borrow_mut() += 1
    });
    assert_eq!(window.send_key(&press_escape), EventResponse::Handled);
    assert_eq!(*fired.borrow(), 1);
    field.remove_shortcut(escape);
    assert_eq!(window.send_key(&press_escape), EventResponse::Continue);
    assert_eq!(*fired.borrow(), 1);

    // set_tooltip / set_accessible_label は読み返す API が無いので、呼べることだけ。
    probe.set_tooltip(Some("tooltip"));
    probe.set_accessible_label(Some("label"));
    probe.set_tooltip(None);
    probe.set_accessible_label(None);

    // on_file_drop / drop_files: 自前の型に付けた閉包へ、自前の型から落とした
    // ファイルが届く。
    let dropped = Rc::new(RefCell::new(Vec::new()));
    probe.on_file_drop({
        let dropped = dropped.clone();
        move |files| {
            dropped
                .borrow_mut()
                .extend(files.iter().map(|file| file.name().to_owned()))
        }
    });
    probe.drop_files(&[FileEntry::from_path("/tmp/photo.png")]);
    testing::wait_until(&ui, || dropped.borrow().len() == 1).await?;
    assert_eq!(*dropped.borrow(), ["photo.png"]);
    // 中のウィジェットへ落としても同じ閉包が受ける (閉包は中のウィジェットに付いた)。
    canvas.drop_files(&[FileEntry::from_path("/tmp/notes.txt")]);
    testing::wait_until(&ui, || dropped.borrow().len() == 2).await?;

    // Web のクラスと属性も、中のウィジェットの要素へ届く。
    #[cfg(target_arch = "wasm32")]
    {
        use naui::web::WebWidgetExt;
        probe.add_class("own-type");
        assert!(canvas.has_class("own-type"));
        assert!(probe.set_attribute("data-testid", "probe"));
        assert_eq!(canvas.attribute("data-testid").as_deref(), Some("probe"));
    }
    window.close();
    Ok(())
}
