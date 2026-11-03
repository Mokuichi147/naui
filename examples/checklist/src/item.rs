//! One item of the list: a component mounted and unmounted as items come and go.

use naui::{Orientation, Sizing, Ui};
use naui_app::{Component, Dispatch, Program};

pub struct Item {
    /// What identifies it among the mounted items. Not its position, which changes
    /// as items before it are removed.
    pub id: u64,
    pub text: String,
    pub done: bool,
}

pub enum Message {
    Toggled(bool),
    /// The parent acts on it: it removes the item from its list.
    Remove,
}

pub struct View {
    root: naui::Stack,
    check: naui::Checkbox,
    label: naui::Label,
}

impl Item {
    pub fn new(id: u64, text: impl Into<String>) -> Self {
        Self {
            id,
            text: text.into(),
            done: false,
        }
    }
}

impl Program for Item {
    type Message = Message;
    type View = View;

    fn update(&mut self, message: Message) {
        match message {
            Message::Toggled(done) => self.done = done,
            Message::Remove => {}
        }
    }

    fn render(&self, view: &View) {
        view.check.set_checked(self.done);
        view.label.set_text(&self.text);
    }
}

impl Component for Item {
    fn mount(ui: &Ui, dispatch: &Dispatch<Message>) -> naui::Result<View> {
        let root = ui.stack(Orientation::Horizontal)?;
        root.set_spacing(8.0);
        root.set_sizing(Sizing::fill_width());
        let check = ui.checkbox("")?;
        check.set_accessible_label(Some("Done"));
        check.on_toggle(dispatch.map(Message::Toggled));
        let label = ui.label("")?;
        label.set_sizing(Sizing::fill_width());
        let remove = ui.button("Remove")?;
        remove.on_click(dispatch.to(|| Message::Remove));
        root.append(&check);
        root.append(&label);
        root.append(&remove);
        Ok(View { root, check, label })
    }

    fn root(view: &View) -> &dyn naui::Widget {
        &view.root
    }
}
