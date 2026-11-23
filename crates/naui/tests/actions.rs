//! The testing actions (typing, answering dialogs, a toast's buttons, the close
//! button, pointer events, choosing and dropping files), run with `naui::testing` on
//! every platform's real event loop.
//!
//! ```sh
//! cargo test -p naui --test actions                                   # macOS, Linux, Windows
//! cargo test -p naui --test actions --target wasm32-unknown-unknown   # the Web, in a browser
//! ```
//!
//! Each backend's own integration tests check the native side in more detail. These
//! check what an app's tests rely on, the same way on all four platforms. Windows
//! needs this event loop: its integration tests run before the window is on screen,
//! where a `TextBox` raises no `TextChanged` and a `ContentDialog` cannot be shown.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use naui::{
    testing, CloseResponse, DialogButtons, DialogResponse, EventResponse, FileEntry, Modifiers,
    Orientation, Point, PointerButton, PointerEvent, PointerPhase, Result, Ui, Window,
};

naui::test_cases!(
    typing_notifies_like_the_user,
    typing_and_enter_do_nothing_while_disabled,
    respond_answers_a_dialog_opened_by_a_click,
    respond_answers_a_sheet,
    click_action_clicks_a_toast_button,
    request_close_asks_before_closing,
    canvas_receives_the_pointer_events_sent,
    choose_picks_files_without_the_dialog,
    drop_files_goes_where_a_real_drop_would,
    open_url_is_recorded_instead_of_opened,
);

/// Records every value a notification passes.
fn recorder<T: 'static>() -> (Rc<RefCell<Vec<T>>>, impl FnMut(T) + 'static) {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let sink = {
        let seen = seen.clone();
        move |value: T| seen.borrow_mut().push(value)
    };
    (seen, sink)
}

/// Records every string a text notification passes.
fn text_recorder() -> (Rc<RefCell<Vec<String>>>, impl FnMut(&str) + 'static) {
    let (seen, mut sink) = recorder::<String>();
    (seen, move |text: &str| sink(text.to_string()))
}

/// Whether the last string recorded is `text`.
fn last_is(seen: &Rc<RefCell<Vec<String>>>, text: &str) -> bool {
    seen.borrow().last().map(String::as_str) == Some(text)
}

/// A shown window. On Windows, UI Automation reaches a field only once its window
/// is drawn.
fn shown_window(ui: &Ui, title: &str) -> Result<Window> {
    let window = ui.window(title, 480.0, 520.0)?;
    window.show();
    Ok(window)
}

/// `enter_text` replaces the text so that `on_change` is called with it, and
/// `submit` / `search` call `on_submit` / `on_search`, on every text field.
async fn typing_notifies_like_the_user(ui: Ui) -> Result<()> {
    let window = ui.window("typing", 480.0, 520.0)?;
    let column = ui.stack(Orientation::Vertical)?;
    let field = ui.text_input("before")?;
    let password = ui.password_input()?;
    let search = ui.search_input()?;
    let area = ui.text_area("first line")?;
    let number = ui.number_input(1.0)?;
    let combo = ui.editable_combo_box()?;
    combo.set_items(&["Apple", "Banana"]);
    column.append(&field);
    column.append(&password);
    column.append(&search);
    column.append(&area);
    column.append(&number);
    column.append(&combo);
    window.set_child(&column);
    window.show();
    // Windows の `NumberBox` と `ComboBox` は、テンプレートが当たってから入力欄を探す。
    testing::settle(&ui).await;

    let (changes, sink) = text_recorder();
    field.on_change(sink);
    let (submits, sink) = text_recorder();
    field.on_submit(sink);
    field.enter_text("Ada");
    assert_eq!(field.text(), "Ada");
    testing::wait_until(&ui, || last_is(&changes, "Ada")).await?;
    field.submit();
    assert_eq!(*submits.borrow(), ["Ada"]);
    // 続けて打っても、打つたびに 1 度だけ呼ぶ (Windows では、あとから来る
    // `TextChanged` で二重に呼ばない)。
    field.enter_text("Grace");
    field.enter_text("Hopper");
    testing::settle(&ui).await;
    assert_eq!(*changes.borrow(), ["Ada", "Grace", "Hopper"]);

    let (changes, sink) = text_recorder();
    password.on_change(sink);
    let (submits, sink) = text_recorder();
    password.on_submit(sink);
    password.enter_text("secret");
    testing::wait_until(&ui, || last_is(&changes, "secret")).await?;
    password.submit();
    assert_eq!(password.text(), "secret");
    assert_eq!(*submits.borrow(), ["secret"]);

    let (changes, sink) = text_recorder();
    search.on_change(sink);
    let (searches, sink) = text_recorder();
    search.on_search(sink);
    search.enter_text("naui");
    testing::wait_until(&ui, || last_is(&changes, "naui")).await?;
    search.search();
    testing::wait_until(&ui, || last_is(&searches, "naui")).await?;

    let (changes, sink) = text_recorder();
    area.on_change(sink);
    area.enter_text("one\ntwo");
    testing::wait_until(&ui, || last_is(&changes, "one\ntwo")).await?;
    assert_eq!(area.text(), "one\ntwo");

    number.set_range(Some(0.0), Some(10.0));
    let (values, sink) = recorder::<f64>();
    number.on_change(sink);
    let (submits, sink) = recorder::<f64>();
    number.on_submit(sink);
    number.enter_text("4");
    testing::wait_until(&ui, || values.borrow().last() == Some(&4.0)).await?;
    assert_eq!(number.value(), 4.0);
    number.submit();
    assert_eq!(*submits.borrow(), [4.0]);

    let (changes, sink) = text_recorder();
    combo.on_change(sink);
    combo.enter_text("Ap");
    testing::wait_until(&ui, || last_is(&changes, "Ap")).await?;
    assert_eq!(
        combo.text(),
        "Ap",
        "nothing is completed from the suggestions"
    );
    combo.enter_text("Banana");
    testing::wait_until(&ui, || last_is(&changes, "Banana")).await?;
    assert_eq!(combo.selected(), Some(1));

    window.close();
    Ok(())
}

/// A user cannot type into or confirm a disabled or read-only field, and neither
/// can `enter_text` and `submit`.
async fn typing_and_enter_do_nothing_while_disabled(ui: Ui) -> Result<()> {
    let window = shown_window(&ui, "disabled")?;
    let column = ui.stack(Orientation::Vertical)?;
    let field = ui.text_input("kept")?;
    let area = ui.text_area("kept")?;
    let number = ui.number_input(3.0)?;
    column.append(&field);
    column.append(&area);
    column.append(&number);
    window.set_child(&column);
    testing::settle(&ui).await;

    let (changes, sink) = text_recorder();
    field.on_change(sink);
    let (submits, sink) = text_recorder();
    field.on_submit(sink);
    field.set_read_only(true);
    field.enter_text("typed");
    field.submit();
    field.set_read_only(false);
    field.set_enabled(false);
    field.enter_text("typed");
    field.submit();

    let (area_changes, sink) = text_recorder();
    area.on_change(sink);
    area.set_read_only(true);
    area.enter_text("typed");
    area.set_read_only(false);
    area.set_enabled(false);
    area.enter_text("typed");

    let (values, sink) = recorder::<f64>();
    number.on_change(sink);
    let (number_submits, sink) = recorder::<f64>();
    number.on_submit(sink);
    number.set_enabled(false);
    number.enter_text("7");
    number.submit();

    // 届くとしても後からなので、間を置いてから何も起きていないことを見る。
    testing::settle(&ui).await;
    assert_eq!(field.text(), "kept");
    assert!(changes.borrow().is_empty() && submits.borrow().is_empty());
    assert_eq!(area.text(), "kept");
    assert!(area_changes.borrow().is_empty());
    assert_eq!(number.value(), 3.0);
    assert!(values.borrow().is_empty() && number_submits.borrow().is_empty());
    window.close();
    Ok(())
}

/// The answer is given before the click that opens the dialog: on macOS that click
/// returns only once the dialog has been answered, and elsewhere the answer comes a
/// moment after it opens.
async fn respond_answers_a_dialog_opened_by_a_click(ui: Ui) -> Result<()> {
    let window = shown_window(&ui, "dialog")?;
    let opener = ui.button("Delete…")?;
    window.set_child(&opener);
    let dialog = ui.dialog("Delete it?")?;
    dialog.set_buttons(
        DialogButtons::new()
            .primary("Delete")
            .secondary("Later")
            .cancel("Keep"),
    );
    let (responses, sink) = recorder::<DialogResponse>();
    dialog.on_response(sink);
    opener.on_click({
        let dialog = dialog.clone();
        move || dialog.open()
    });

    for (round, response) in [
        DialogResponse::Primary,
        DialogResponse::Secondary,
        DialogResponse::Cancel,
    ]
    .into_iter()
    .enumerate()
    {
        dialog.respond(response);
        opener.click();
        testing::wait_until(&ui, || responses.borrow().len() == round + 1).await?;
        assert_eq!(responses.borrow()[round], response);
        assert!(!dialog.is_open());
        // 閉じ終えるまで間を置いてから、次の回で開き直す。
        testing::settle(&ui).await;
    }
    window.close();
    Ok(())
}

/// A sheet is answered the same way, and a disabled or missing button cannot be
/// clicked, so the sheet stays open.
async fn respond_answers_a_sheet(ui: Ui) -> Result<()> {
    let window = shown_window(&ui, "sheet")?;
    window.set_child(&ui.label("body")?);
    let sheet = ui.sheet("New folder")?;
    sheet.set_child(&ui.text_input("")?);
    sheet.set_buttons(DialogButtons::new().primary("Create").cancel("Cancel"));
    let (responses, sink) = recorder::<DialogResponse>();
    sheet.on_response(sink);

    sheet.respond(DialogResponse::Primary);
    sheet.open();
    testing::wait_until(&ui, || !responses.borrow().is_empty()).await?;
    assert_eq!(*responses.borrow(), [DialogResponse::Primary]);
    assert!(!sheet.is_open());
    testing::settle(&ui).await;

    sheet.set_button_enabled(DialogResponse::Primary, false);
    sheet.open();
    testing::settle(&ui).await;
    sheet.respond(DialogResponse::Primary);
    sheet.respond(DialogResponse::Secondary);
    testing::settle(&ui).await;
    assert!(
        sheet.is_open(),
        "a disabled or missing button cannot be clicked"
    );
    sheet.respond(DialogResponse::Cancel);
    testing::wait_until(&ui, || !sheet.is_open()).await?;
    assert_eq!(
        *responses.borrow(),
        [DialogResponse::Primary, DialogResponse::Cancel]
    );
    window.close();
    Ok(())
}

/// `click_action` clicks a toast's button only while it is on screen, so
/// `on_action` and then `on_dismiss` are called.
async fn click_action_clicks_a_toast_button(ui: Ui) -> Result<()> {
    let window = shown_window(&ui, "toast")?;
    window.set_child(&ui.label("body")?);
    let toast = ui.toast("Deleted")?;
    toast.set_actions(&["Undo", "Open"]);
    toast.set_timeout(0.0);
    let (seen, mut sink) = recorder::<String>();
    let seen_dismiss = seen.clone();
    toast.on_action(move |index| sink(format!("action {index}")));
    toast.on_dismiss(move || seen_dismiss.borrow_mut().push("dismiss".to_string()));

    toast.click_action(0);
    testing::settle(&ui).await;
    assert!(
        seen.borrow().is_empty(),
        "a toast not on screen has no buttons to click"
    );

    toast.show();
    testing::wait_until(&ui, || toast.is_visible()).await?;
    toast.click_action(2);
    testing::settle(&ui).await;
    assert!(seen.borrow().is_empty(), "there is no third button");
    assert!(toast.is_visible());

    toast.click_action(1);
    testing::wait_until(&ui, || seen.borrow().len() == 2).await?;
    assert_eq!(*seen.borrow(), ["action 1", "dismiss"]);
    testing::wait_until(&ui, || !toast.is_visible()).await?;
    window.close();
    Ok(())
}

/// `request_close` asks `on_close_request` first, as the close button does. The Web
/// has no close button for a window inside the page, so nothing happens there.
async fn request_close_asks_before_closing(ui: Ui) -> Result<()> {
    let window = shown_window(&ui, "close")?;
    window.set_child(&ui.label("unsaved changes")?);
    let answers = Rc::new(RefCell::new(vec![
        CloseResponse::KeepOpen,
        CloseResponse::Close,
    ]));
    let asked = Rc::new(Cell::new(0));
    window.on_close_request({
        let answers = answers.clone();
        let asked = asked.clone();
        move || {
            asked.set(asked.get() + 1);
            answers.borrow_mut().remove(0)
        }
    });

    window.request_close();
    testing::settle(&ui).await;
    if cfg!(target_arch = "wasm32") {
        assert_eq!(
            asked.get(),
            0,
            "on_close_request is never called on the Web"
        );
        assert!(window.is_visible());
        window.close();
        return Ok(());
    }
    assert_eq!(asked.get(), 1);
    assert!(window.is_visible(), "KeepOpen keeps it open");
    window.request_close();
    testing::wait_until(&ui, || !window.is_visible()).await?;
    assert_eq!(asked.get(), 2);

    let bare = shown_window(&ui, "close without asking")?;
    bare.set_child(&ui.label("nothing to save")?);
    bare.request_close();
    testing::wait_until(&ui, || !bare.is_visible()).await?;
    Ok(())
}

/// `send_pointer` and `send_wheel` hand the events to `on_pointer` and `on_wheel`
/// as given.
async fn canvas_receives_the_pointer_events_sent(ui: Ui) -> Result<()> {
    let window = shown_window(&ui, "canvas")?;
    let canvas = ui.canvas()?;
    window.set_child(&canvas);
    let (pointers, sink) = recorder::<PointerEvent>();
    canvas.on_pointer(sink);

    let press = PointerEvent::new(PointerPhase::Down, Point::new(10.0, 20.0))
        .button(Some(PointerButton::Primary))
        .modifiers(Modifiers {
            shift: true,
            ..Modifiers::default()
        });
    let drag = PointerEvent::new(PointerPhase::Move, Point::new(60.0, 45.5));
    let release = PointerEvent::new(PointerPhase::Up, Point::new(60.0, 45.5))
        .button(Some(PointerButton::Primary));
    canvas.send_pointer(press);
    canvas.send_pointer(drag);
    canvas.send_pointer(release);
    assert_eq!(*pointers.borrow(), [press, drag, release]);

    let wheel = PointerEvent::wheel(Point::new(5.0, 5.0), 0.0, 24.0);
    assert_eq!(canvas.send_wheel(wheel), EventResponse::Continue);
    let (wheels, mut sink) = recorder::<PointerEvent>();
    canvas.on_wheel(move |event| {
        sink(event);
        EventResponse::Handled
    });
    assert_eq!(canvas.send_wheel(wheel), EventResponse::Handled);
    canvas.send_pointer(wheel);
    assert_eq!(
        *wheels.borrow(),
        [wheel, wheel],
        "a Wheel event goes to on_wheel"
    );
    assert_eq!(pointers.borrow().len(), 3);
    window.close();
    Ok(())
}

/// The names of `files`.
fn names(files: &[FileEntry]) -> Vec<String> {
    files.iter().map(|file| file.name().to_string()).collect()
}

/// `FilePicker::choose` and `FileSaver::choose` answer as the dialog would, without
/// showing it.
async fn choose_picks_files_without_the_dialog(ui: Ui) -> Result<()> {
    let window = shown_window(&ui, "files")?;
    let column = ui.stack(Orientation::Vertical)?;
    let picker = ui.file_picker("Open…")?;
    let saver = ui.file_saver("Save…")?;
    column.append(&picker);
    column.append(&saver);
    window.set_child(&column);

    let (picks, mut sink) = recorder::<Vec<String>>();
    picker.on_select(move |files| sink(names(files)));
    let files = [
        FileEntry::from_path("/tmp/photo.png"),
        FileEntry::from_path("/tmp/notes.txt"),
    ];
    picker.choose(&files);
    testing::wait_until(&ui, || picks.borrow().len() == 1).await?;
    assert_eq!(picks.borrow()[0], ["photo.png", "notes.txt"]);
    assert_eq!(names(&picker.selection()), ["photo.png", "notes.txt"]);
    picker.choose(&[]);
    picker.set_enabled(false);
    picker.choose(&files[..1]);
    testing::settle(&ui).await;
    assert_eq!(
        picks.borrow().len(),
        1,
        "a cancelled or disabled picker picks nothing"
    );

    let (saves, mut sink) = recorder::<String>();
    saver.on_save(move |entry| sink(entry.name().to_string()));
    let (errors, mut sink) = recorder::<String>();
    saver.on_error(move |error| sink(error.to_string()));
    saver.set_contents(b"saved by a test");
    #[cfg(not(target_arch = "wasm32"))]
    let destination = {
        let path = std::env::temp_dir().join("naui-actions-choose.txt");
        let _ = std::fs::remove_file(&path);
        FileEntry::from_path(path)
    };
    #[cfg(target_arch = "wasm32")]
    let destination = FileEntry::from_name("naui-actions-choose.txt");
    saver.choose(&destination);
    testing::wait_until(&ui, || saves.borrow().len() == 1).await?;
    assert_eq!(saves.borrow()[0], "naui-actions-choose.txt");
    assert!(errors.borrow().is_empty(), "{:?}", errors.borrow());
    #[cfg(not(target_arch = "wasm32"))]
    {
        let path = destination.path().expect("a path");
        assert_eq!(
            std::fs::read(path).ok().as_deref(),
            Some(&b"saved by a test"[..])
        );
        let _ = std::fs::remove_file(path);
    }
    saver.set_enabled(false);
    saver.choose(&destination);
    testing::settle(&ui).await;
    assert_eq!(saves.borrow().len(), 1, "a disabled saver saves nothing");
    window.close();
    Ok(())
}

/// `drop_files` reaches the widget's own `on_file_drop`, else the nearest enclosing
/// widget's, else the window's.
async fn drop_files_goes_where_a_real_drop_would(ui: Ui) -> Result<()> {
    let window = shown_window(&ui, "drop")?;
    let column = ui.stack(Orientation::Vertical)?;
    let outer = ui.stack(Orientation::Vertical)?;
    let inner = ui.label("Drop here")?;
    let plain = ui.label("Takes nothing itself")?;
    let elsewhere = ui.button("Elsewhere")?;
    outer.append(&inner);
    outer.append(&plain);
    column.append(&outer);
    column.append(&elsewhere);
    window.set_child(&column);

    let (to_inner, mut sink) = recorder::<Vec<String>>();
    inner.on_file_drop(move |files| sink(names(files)));
    let (to_outer, mut sink) = recorder::<Vec<String>>();
    outer.on_file_drop(move |files| sink(names(files)));
    let (to_window, mut sink) = recorder::<Vec<String>>();
    window.on_file_drop(move |files| sink(names(files)));
    let files = [
        FileEntry::from_path("/tmp/photo.png"),
        FileEntry::from_path("/tmp/notes.txt"),
    ];

    inner.drop_files(&files);
    testing::wait_until(&ui, || to_inner.borrow().len() == 1).await?;
    assert_eq!(to_inner.borrow()[0], ["photo.png", "notes.txt"]);
    plain.drop_files(&files[..1]);
    testing::wait_until(&ui, || to_outer.borrow().len() == 1).await?;
    assert_eq!(to_outer.borrow()[0], ["photo.png"]);
    elsewhere.drop_files(&files[1..]);
    window.drop_files(&files);
    testing::wait_until(&ui, || to_window.borrow().len() == 2).await?;
    assert_eq!(to_window.borrow()[0], ["notes.txt"]);
    assert_eq!(to_window.borrow()[1], ["photo.png", "notes.txt"]);
    inner.drop_files(&[]);
    window.drop_files(&[]);
    testing::settle(&ui).await;
    assert_eq!(to_inner.borrow().len(), 1, "an empty drop delivers nothing");
    assert_eq!(to_outer.borrow().len(), 1);
    assert_eq!(to_window.borrow().len(), 2);
    window.close();
    Ok(())
}

/// Inside a test case `Ui::open_url` opens nothing and records the URL; one it
/// cannot read is still an error and is not recorded.
async fn open_url_is_recorded_instead_of_opened(ui: Ui) -> Result<()> {
    assert!(
        testing::opened_urls(&ui).is_empty(),
        "each case starts with none"
    );
    ui.open_url("https://example.com/help")?;
    ui.open_url("mailto:someone@example.com")?;
    assert_eq!(
        testing::opened_urls(&ui),
        ["https://example.com/help", "mailto:someone@example.com"]
    );
    if !cfg!(target_arch = "wasm32") {
        // Web の `window.open` は相対 URL を文書の URL から解決するので、読めない
        // 文字列が少ない。ネイティブでは scheme の無いものを読めない。
        assert!(ui.open_url("").is_err());
        assert_eq!(testing::opened_urls(&ui).len(), 2);
    }
    Ok(())
}
