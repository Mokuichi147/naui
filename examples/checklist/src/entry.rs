//! The field for a new item: a component that is always there.

use naui::{Orientation, Sizing, Ui};
use naui_app::{Component, Dispatch, Program};

#[derive(Default)]
pub struct Entry {
    draft: String,
}

pub enum Message {
    DraftChanged(String),
    /// Enter, or the Add button. The parent acts on it: it takes the draft with
    /// [`Entry::take`] and adds an item.
    Submit,
}

pub struct View {
    root: naui::Stack,
    field: naui::TextInput,
    add: naui::Button,
}

impl Entry {
    /// The draft, trimmed, leaving the field empty. `None` if there is nothing in it.
    pub fn take(&mut self) -> Option<String> {
        let text = std::mem::take(&mut self.draft).trim().to_owned();
        (!text.is_empty()).then_some(text)
    }
}

impl Program for Entry {
    type Message = Message;
    type View = View;

    fn update(&mut self, message: Message) {
        match message {
            Message::DraftChanged(text) => self.draft = text,
            Message::Submit => {}
        }
    }

    fn render(&self, view: &View) {
        view.field.set_text(&self.draft);
        view.add.set_enabled(!self.draft.trim().is_empty());
    }
}

impl Component for Entry {
    fn mount(ui: &Ui, dispatch: &Dispatch<Message>) -> naui::Result<View> {
        let root = ui.stack(Orientation::Horizontal)?;
        root.set_spacing(8.0);
        root.set_sizing(Sizing::fill_width());
        let field = ui.text_input("")?;
        field.set_placeholder("Something to do");
        field.set_sizing(Sizing::fill_width());
        field.on_change(dispatch.with(|text: &str| Message::DraftChanged(text.to_owned())));
        field.on_submit(dispatch.with(|_: &str| Message::Submit));
        let add = ui.button("Add")?;
        add.on_click(dispatch.to(|| Message::Submit));
        root.append(&field);
        root.append(&add);
        Ok(View { root, field, add })
    }

    fn root(view: &View) -> &dyn naui::Widget {
        &view.root
    }
}
