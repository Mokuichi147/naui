//! The selected note: the folder it is in, and its text.
//!
//! Like the other screens, it holds widgets and the messages they send, and reads the
//! core's model. It knows nothing of the list: the two meet only in the app crate.
//! The note's title is its first line, so there is no field for it.
//!
//! The strings are looked up by their IDs in the [`Translations`] the app passes in.

use std::cell::RefCell;
use std::rc::Rc;

use naui::{Align, Orientation, Padding, Sizing, Ui};
use naui_app::Dispatch;
use notes_core::Notes;
use notes_l10n::Translations;

/// What happens on this screen. The app decides what each one does to the notes.
#[derive(Debug)]
pub enum Message {
    TextChanged(String),
    /// A folder was picked for the note, `""` for none.
    FolderPicked(String),
}

pub struct View {
    root: naui::Stack,
    label: naui::Label,
    folder: naui::ComboBox,
    text: naui::TextArea,
    /// What the folder field calls the notes folder itself, in the language shown.
    no_folder: RefCell<String>,
    /// The folders the field offers, after "No Folder". The field tells which one was
    /// picked by its number, which is turned into the folder's name here.
    folders: Rc<RefCell<Vec<String>>>,
}

pub fn build(ui: &Ui, dispatch: &Dispatch<Message>) -> naui::Result<View> {
    let root = ui.stack(Orientation::Vertical)?;
    root.set_spacing(8.0);
    root.set_padding(Padding {
        top: 8.0,
        ..Padding::default()
    });
    root.set_sizing(Sizing::fill());

    let folders: Rc<RefCell<Vec<String>>> = Rc::default();
    let row = ui.stack(Orientation::Horizontal)?;
    row.set_spacing(8.0);
    row.set_align(Align::Center);
    row.set_padding(Padding {
        start: 4.0,
        ..Padding::default()
    });
    let folder = ui.combo_box()?;
    folder.on_select(dispatch.map({
        let folders = Rc::clone(&folders);
        move |index: usize| {
            let name = match index {
                0 => String::new(),
                index => folders.borrow().get(index - 1).cloned().unwrap_or_default(),
            };
            Message::FolderPicked(name)
        }
    }));
    let label = ui.label("")?;
    row.append(&label);
    row.append(&folder);

    let text = ui.text_area("")?;
    text.set_sizing(Sizing::fill());
    text.on_change(dispatch.with(|text: &str| Message::TextChanged(text.to_owned())));

    root.append(&row);
    root.append(&text);
    // The strings are set by `translate`, which the app calls before the first render.
    Ok(View {
        root,
        label,
        folder,
        text,
        no_folder: RefCell::default(),
        folders,
    })
}

impl View {
    pub fn root(&self) -> &dyn naui::Widget {
        &self.root
    }

    /// Sets the strings that do not depend on the notes: called when the language
    /// changes, before `render`.
    pub fn translate(&self, tr: &Translations) {
        self.label.set_text(&tr.get("editor-folder"));
        self.text.set_accessible_label(Some(&tr.get("editor-note")));
        *self.no_folder.borrow_mut() = tr.get("editor-no-folder");
        let folders = self.folders.borrow().clone();
        self.set_folders(&folders);
    }

    pub fn render(&self, notes: &Notes, tr: &Translations) {
        let note = notes.selected();
        // The same text is not set again, so typing keeps its place.
        self.text.set_text(note.map_or("", |note| note.text()));
        self.text.set_enabled(note.is_some());
        self.text.set_placeholder(&match note {
            Some(_) => String::new(),
            None if notes.is_loaded() && notes.visible().is_empty() => tr.get("editor-no-notes"),
            None => tr.get("editor-none-selected"),
        });

        if *self.folders.borrow() != notes.folders() {
            self.set_folders(notes.folders());
        }
        let index = note.map_or(0, |note| {
            notes
                .folders()
                .iter()
                .position(|f| f == note.folder())
                .map_or(0, |index| index + 1)
        });
        if self.folder.selected() != Some(index) {
            self.folder.set_selected(index);
        }
        self.folder.set_enabled(note.is_some());
    }

    /// Offers "No Folder" and `folders`. `render` selects the note's folder again.
    fn set_folders(&self, folders: &[String]) {
        *self.folders.borrow_mut() = folders.to_vec();
        let no_folder = self.no_folder.borrow();
        let items: Vec<&str> = std::iter::once(no_folder.as_str())
            .chain(folders.iter().map(String::as_str))
            .collect();
        self.folder.set_items(&items);
    }
}
