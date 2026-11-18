//! The testing actions for typing and answering dialogs, run with `naui::testing` on
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
//! where a `TextBox` has no template (so no UI Automation Value pattern) and a
//! `ContentDialog` cannot be shown.

use std::cell::RefCell;
use std::rc::Rc;

use naui::{testing, DialogButtons, DialogResponse, Orientation, Result, Ui, Window};

naui::test_cases!(
    typing_notifies_like_the_user,
    typing_and_enter_do_nothing_while_disabled,
    respond_answers_a_dialog_opened_by_a_click,
    respond_answers_a_sheet,
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

/// A shown window. Windows types into a field only once its window is shown.
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
    // Windows は `TextChanged` を後から出すので、届くまで待つ。
    testing::wait_until(&ui, || last_is(&changes, "Ada")).await?;
    field.submit();
    assert_eq!(*submits.borrow(), ["Ada"]);

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
