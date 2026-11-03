//! A to-do list structured with naui-app: one model, typed messages, `update` that
//! changes the model and `render` that sets the widgets to match it.
//!
//! The field's text lives in the model too. `render` sets it back on every update,
//! which is harmless: naui's setters do nothing when the value is already shown, so
//! the caret stays and the IME composition is not broken.
//!
//! The selection lives in the model as well, and `render` sets it on the list, so
//! the row shown as selected is always the one Remove deletes.

use std::cell::RefCell;

use naui::{ListItem, Orientation, Padding, Settings, Sizing, Ui};
use naui_app::{Dispatch, Program};

#[derive(Default)]
struct Todo {
    draft: String,
    items: Vec<String>,
    selected: Option<usize>,
}

enum Message {
    DraftChanged(String),
    Add,
    Select(Option<usize>),
    RemoveSelected,
}

struct View {
    field: naui::TextInput,
    add: naui::Button,
    list: naui::List,
    remove: naui::Button,
    summary: naui::Label,
    /// The items the list shows. `List::set_items` rebuilds the rows, so it is called
    /// only when they change.
    shown: RefCell<Vec<String>>,
}

impl Program for Todo {
    type Message = Message;
    type View = View;

    fn update(&mut self, message: Message) {
        match message {
            Message::DraftChanged(text) => self.draft = text,
            Message::Add => {
                let item = self.draft.trim().to_owned();
                if !item.is_empty() {
                    self.items.push(item);
                    self.draft.clear();
                }
            }
            Message::Select(index) => self.selected = index,
            Message::RemoveSelected => {
                if let Some(index) = self.selected.take() {
                    if index < self.items.len() {
                        self.items.remove(index);
                    }
                }
            }
        }
    }

    fn render(&self, view: &View) {
        view.field.set_text(&self.draft);
        view.add.set_enabled(!self.draft.trim().is_empty());
        if *view.shown.borrow() != self.items {
            let rows: Vec<ListItem> = self.items.iter().map(ListItem::new).collect();
            view.list.set_items(&rows);
            *view.shown.borrow_mut() = self.items.clone();
        }
        // `set_items` clears the selection without notifying, so the model's selection
        // is set back. Adding appends, so the selected row keeps its index.
        let selection: Vec<usize> = self.selected.into_iter().collect();
        if view.list.selection() != selection {
            view.list.set_selection(&selection);
        }
        view.remove.set_enabled(self.selected.is_some());
        view.summary.set_text(&match self.items.len() {
            0 => "Nothing to do".to_owned(),
            1 => "1 item".to_owned(),
            n => format!("{n} items"),
        });
    }
}

fn build(ui: &Ui, dispatch: &Dispatch<Message>) -> naui::Result<View> {
    let window = ui.window("To-do", 360.0, 420.0)?;
    let root = ui.stack(Orientation::Vertical)?;
    root.set_spacing(8.0);
    root.set_padding(Padding::all(16.0));

    let entry = ui.stack(Orientation::Horizontal)?;
    entry.set_spacing(8.0);
    entry.set_sizing(Sizing::fill_width());
    let field = ui.text_input("")?;
    field.set_placeholder("Something to do");
    field.set_sizing(Sizing::fill_width());
    field.on_change(dispatch.with(|text: &str| Message::DraftChanged(text.to_owned())));
    field.on_submit(dispatch.with(|_: &str| Message::Add));
    let add = ui.button("Add")?;
    add.on_click(dispatch.to(|| Message::Add));
    entry.append(&field);
    entry.append(&add);

    let list = ui.list()?;
    list.set_sizing(Sizing::fill());
    list.on_select(dispatch.with(|selected: &[usize]| Message::Select(selected.first().copied())));

    let footer = ui.stack(Orientation::Horizontal)?;
    footer.set_sizing(Sizing::fill_width());
    let summary = ui.label("")?;
    let remove = ui.button("Remove")?;
    remove.on_click(dispatch.to(|| Message::RemoveSelected));
    footer.append(&summary);
    footer.append(&ui.spacer()?);
    footer.append(&remove);

    root.append(&entry);
    root.append(&list);
    root.append(&footer);
    window.set_child(&root);
    window.show();
    Ok(View {
        field,
        add,
        list,
        remove,
        summary,
        shown: RefCell::new(Vec::new()),
    })
}

fn main() -> naui::Result<()> {
    naui_app::run(Settings::new("todo"), Todo::default(), build)
}
