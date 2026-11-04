//! A checklist split into components with naui-app.
//!
//! - [`entry::Entry`], the field for a new item, is always there: it is mounted once
//!   in `build` and kept in a [`Mounted`].
//! - [`item::Item`]s come and go: [`Children`] mounts them, unmounts them and keeps
//!   them in order, following the items in the model.
//!
//! Each component has its own model, messages, `update` and widgets. Their messages
//! are wrapped into the app's (`Message::Entry`, `Message::Item`), so the app's
//! `update` sees them first: it adds an item on the entry's `Submit` and removes one
//! on the item's `Remove`, and hands the rest on to the component.

mod entry;
mod item;

use entry::Entry;
use item::Item;
use naui::{Orientation, Padding, Settings, Sizing, Ui};
use naui_app::{Change, Children, Dispatch, Effect, Mounted, Program};

struct Checklist {
    entry: Entry,
    items: Vec<Item>,
    next_id: u64,
}

enum Message {
    Entry(entry::Message),
    Item(u64, item::Message),
    ClearDone,
}

struct View {
    entry: Mounted<Entry>,
    items: Children<u64, Item>,
    /// Holds only the items' widgets, so a `Change`'s position is a position in it.
    list: naui::Stack,
    summary: naui::Label,
    clear: naui::Button,
}

impl Checklist {
    fn new(items: &[&str]) -> Self {
        let mut checklist = Self {
            entry: Entry::default(),
            items: Vec::new(),
            next_id: 0,
        };
        for text in items {
            checklist.add(text);
        }
        checklist
    }

    fn add(&mut self, text: &str) {
        self.items.push(Item::new(self.next_id, text));
        self.next_id += 1;
    }
}

impl Program for Checklist {
    type Message = Message;
    type View = View;

    fn update(&mut self, message: Message) -> Effect<Message> {
        match message {
            Message::Entry(entry::Message::Submit) => {
                if let Some(text) = self.entry.take() {
                    self.add(&text);
                }
                Effect::none()
            }
            Message::Entry(message) => self.entry.update(message).map(Message::Entry),
            Message::Item(id, item::Message::Remove) => {
                self.items.retain(|item| item.id != id);
                Effect::none()
            }
            // An item removed a moment ago can still have a message in the queue, so
            // a missing id is not an error.
            Message::Item(id, message) => match self.items.iter_mut().find(|item| item.id == id) {
                Some(item) => item
                    .update(message)
                    .map(move |message| Message::Item(id, message)),
                None => Effect::none(),
            },
            Message::ClearDone => {
                self.items.retain(|item| !item.done);
                Effect::none()
            }
        }
    }

    fn render(&self, view: &View) {
        view.entry.render(&self.entry);
        let items = self.items.iter().map(|item| (item.id, item));
        let synced = view.items.sync(items, |change| match change {
            Change::Insert { index, root } => view.list.insert(index, root),
            Change::Remove { index, .. } => view.list.remove(index),
        });
        if let Err(error) = synced {
            eprintln!("could not show an item: {error}");
        }
        let done = self.items.iter().filter(|item| item.done).count();
        view.summary.set_text(&match self.items.len() {
            0 => "Nothing to do".to_owned(),
            total => format!("{done} of {total} done"),
        });
        view.clear.set_enabled(done > 0);
    }
}

fn build(ui: &Ui, dispatch: &Dispatch<Message>) -> naui::Result<View> {
    let window = ui.window("Checklist", 380.0, 440.0)?;
    let root = ui.stack(Orientation::Vertical)?;
    root.set_spacing(8.0);
    root.set_padding(Padding::all(16.0));

    let entry = Mounted::<Entry>::new(ui, dispatch, Message::Entry)?;

    let list = ui.stack(Orientation::Vertical)?;
    list.set_spacing(4.0);
    list.set_sizing(Sizing::fill_width());
    let scroll = ui.scroll()?;
    scroll.set_sizing(Sizing::fill());
    scroll.set_child(&list);
    let items = Children::new(ui, dispatch, Message::Item);

    let footer = ui.stack(Orientation::Horizontal)?;
    footer.set_sizing(Sizing::fill_width());
    let summary = ui.label("")?;
    let clear = ui.button("Clear done")?;
    clear.on_click(dispatch.to(|| Message::ClearDone));
    footer.append(&summary);
    footer.append(&ui.spacer()?);
    footer.append(&clear);

    root.append(entry.root());
    root.append(&scroll);
    root.append(&footer);
    window.set_child(&root);
    window.show();
    Ok(View {
        entry,
        items,
        list,
        summary,
        clear,
    })
}

fn main() -> naui::Result<()> {
    let checklist = Checklist::new(&["Read the guide", "Split the app into components"]);
    naui_app::run(Settings::new("checklist"), checklist, build)
}

/// The app's `update`, children included, is plain Rust: no window is needed.
#[cfg(test)]
mod tests {
    use super::*;

    fn texts(checklist: &Checklist) -> Vec<&str> {
        checklist
            .items
            .iter()
            .map(|item| item.text.as_str())
            .collect()
    }

    #[test]
    fn submitting_the_entry_adds_an_item() {
        let mut checklist = Checklist::new(&[]);
        let _ = checklist.update(Message::Entry(entry::Message::DraftChanged(
            "  Buy milk ".into(),
        )));
        let _ = checklist.update(Message::Entry(entry::Message::Submit));
        assert_eq!(texts(&checklist), ["Buy milk"]);
        // 空の入力では増えない。
        let _ = checklist.update(Message::Entry(entry::Message::Submit));
        assert_eq!(checklist.items.len(), 1);
    }

    #[test]
    fn an_items_messages_reach_that_item() {
        let mut checklist = Checklist::new(&["a", "b", "c"]);
        let _ = checklist.update(Message::Item(1, item::Message::Toggled(true)));
        let _ = checklist.update(Message::Item(0, item::Message::Remove));
        assert_eq!(texts(&checklist), ["b", "c"]);
        assert!(checklist.items[0].done);
        let _ = checklist.update(Message::ClearDone);
        assert_eq!(texts(&checklist), ["c"]);
    }

    #[test]
    fn a_message_for_a_removed_item_is_ignored() {
        let mut checklist = Checklist::new(&["a"]);
        let _ = checklist.update(Message::Item(0, item::Message::Remove));
        let _ = checklist.update(Message::Item(0, item::Message::Toggled(true)));
        assert!(checklist.items.is_empty());
    }
}
