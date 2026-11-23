//! The notes of the folder shown, the latest first, with a field that searches them.
//!
//! A screen crate holds widgets and the messages they send: `build` makes the widgets
//! and connects their callbacks to [`Message`], and [`View::render`] sets them from the
//! core's model. The model and `update` stay in `notes-core`, and the app turns this
//! screen's messages into the core's, so a new widget here rebuilds nothing but this
//! crate and the app.
//!
//! The strings are looked up by their IDs in the [`Translations`] the app passes in,
//! and the core's values, such as a note's [`Age`], are put in words here.

use std::cell::RefCell;
use std::rc::Rc;

use naui::{Align, ListRow, Orientation, Padding, Sizing, TextColor, TextStyle, Ui};
use naui_app::Dispatch;
use notes_core::{Age, NoteId, Notes};
use notes_l10n::Translations;

/// What happens on this screen. The app decides what each one does to the notes.
#[derive(Debug)]
pub enum Message {
    SearchChanged(String),
    /// A note was picked in the list, or the selection was cleared.
    Picked(Option<NoteId>),
}

/// What a row shows.
#[derive(Clone, PartialEq)]
struct Row {
    id: NoteId,
    title: String,
    detail: String,
}

pub struct View {
    ui: Ui,
    root: naui::Stack,
    search: naui::SearchInput,
    list: naui::List,
    /// The rows shown. The list builds a row from it when the row comes into view,
    /// and a picked row is turned into its note's id here, so the message does not
    /// depend on the rows staying the same.
    rows: Rc<RefCell<Vec<Row>>>,
}

pub fn build(ui: &Ui, dispatch: &Dispatch<Message>) -> naui::Result<View> {
    let root = ui.stack(Orientation::Vertical)?;
    root.set_spacing(8.0);
    root.set_padding(Padding::all(8.0));
    root.set_sizing(Sizing::fill());

    let search = ui.search_input()?;
    search.set_sizing(Sizing::fill_width());
    search.on_change(dispatch.with(|text: &str| Message::SearchChanged(text.to_owned())));

    let rows: Rc<RefCell<Vec<Row>>> = Rc::default();
    let list = ui.list()?;
    list.set_sizing(Sizing::fill());
    list.on_select(dispatch.with({
        let rows = Rc::clone(&rows);
        move |selection: &[usize]| {
            let picked = selection
                .first()
                .and_then(|&row| rows.borrow().get(row).map(|row| row.id));
            Message::Picked(picked)
        }
    }));

    root.append(&search);
    root.append(&list);
    let view = View {
        ui: ui.clone(),
        root,
        search,
        list,
        rows,
    };
    view.set_rows(Vec::new());
    Ok(view)
}

/// The app places the view itself (`stack.append(&list)`), which places its root.
impl naui::AsWidget for View {
    fn as_widget(&self) -> &dyn naui::Widget {
        &self.root
    }
}

impl View {
    /// Sets the strings that do not depend on the notes: called when the language
    /// changes, before `render`.
    pub fn translate(&self, tr: &Translations) {
        self.search.set_placeholder(&tr.get("list-search"));
    }

    pub fn render(&self, notes: &Notes, tr: &Translations) {
        self.search.set_text(notes.search());
        let rows: Vec<Row> = notes
            .visible()
            .iter()
            .map(|note| {
                let age = age(tr, notes_core::age(notes.now(), note.modified()));
                Row {
                    id: note.id(),
                    title: match note.title() {
                        "" => tr.get("list-untitled"),
                        title => title.to_owned(),
                    },
                    detail: match note.preview() {
                        preview if preview.is_empty() => age,
                        preview => format!("{age}  {preview}"),
                    },
                }
            })
            .collect();
        let same_notes = {
            let shown = self.rows.borrow();
            shown.len() == rows.len() && shown.iter().zip(&rows).all(|(a, b)| a.id == b.id)
        };
        if !same_notes {
            // Other notes, or in another order: new rows, and the selection set again.
            self.set_rows(rows);
        } else if *self.rows.borrow() != rows {
            // The same notes with other titles or ages, as while typing: only the rows
            // in view are built again, and the selection and scrolling stay.
            *self.rows.borrow_mut() = rows;
            self.list.refresh();
        }
        // A selected note the search hides has no row.
        let selected = notes.selected().map(|note| note.id());
        let selection: Vec<usize> = self
            .rows
            .borrow()
            .iter()
            .position(|row| Some(row.id) == selected)
            .into_iter()
            .collect();
        if self.list.selection() != selection {
            self.list.set_selection(&selection);
        }
    }

    fn set_rows(&self, rows: Vec<Row>) {
        let count = rows.len();
        let ui = self.ui.clone();
        *self.rows.borrow_mut() = rows;
        let shown = Rc::clone(&self.rows);
        self.list.set_row_builder(count, move |index| {
            let shown = shown.borrow();
            let row = &shown[index];
            let stack = ui.stack(Orientation::Vertical)?;
            stack.set_spacing(2.0);
            stack.set_padding(Padding::symmetric(4.0, 6.0));
            stack.set_align(Align::Start);
            stack.set_sizing(Sizing::fill_width());
            let title = ui.label(&row.title)?;
            title.set_style(TextStyle::Heading);
            let detail = ui.label(&row.detail)?;
            detail.set_style(TextStyle::Caption);
            detail.set_color(TextColor::Secondary);
            // One line each, cut off with "…" where the list ends.
            for label in [&title, &detail] {
                label.set_wrap(false);
                label.set_sizing(Sizing::fill_width());
                stack.append(label);
            }
            Ok(ListRow::new(&stack))
        });
    }
}

/// A note's age in words: "Just now", "5 minutes ago", "Yesterday"…
fn age(tr: &Translations, age: Age) -> String {
    let (id, count) = match age {
        Age::JustNow => return tr.get("list-just-now"),
        Age::Yesterday => return tr.get("list-yesterday"),
        Age::Minutes(count) => ("list-minutes-ago", count),
        Age::Hours(count) => ("list-hours-ago", count),
        Age::Days(count) => ("list-days-ago", count),
        Age::Weeks(count) => ("list-weeks-ago", count),
        Age::Months(count) => ("list-months-ago", count),
        Age::Years(count) => ("list-years-ago", count),
    };
    tr.get_args(id, [("count", count.into())])
}
