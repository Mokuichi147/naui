//! The folders: making, picking, renaming and deleting them.
//!
//! - The sidebar lists all the notes, each subfolder of the notes folder, and New
//!   Folder, which asks for a name.
//! - The folder bar, placed by the app above the list, names the folder shown and
//!   has Rename and Delete for it.
//! - [`Prompts`] opens the sheets and the dialog that ask for a name or a
//!   confirmation, for the bar and for the app's menu alike.
//!
//! The sidebar is not placed in a layout but attached to the window, so this screen
//! hands the app its [`naui::Sidebar`] instead of a root widget. Otherwise it is like
//! the other screens: widgets and the messages they send, a `render` that sets them
//! from the core's model, and a `translate` that sets the strings that do not depend
//! on it.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use naui::{
    Align, DialogButtons, DialogResponse, Orientation, Padding, SidebarItem, SidebarSection,
    Sizing, TextStyle, ToolbarIcon, Ui,
};
use naui_app::Dispatch;
use notes_core::Notes;
use notes_l10n::Translations;

/// What happens on this screen. The app decides what each one does to the notes.
#[derive(Debug)]
pub enum Message {
    /// A folder was picked, or `None` for all the notes.
    Picked(Option<String>),
    /// A name was given for a new folder: what was typed, or "New Folder" in the
    /// language shown.
    FolderNamed(String),
    /// A new name was given for a folder.
    FolderRenamed { from: String, to: String },
    /// Deleting a folder was confirmed.
    FolderDeleted(String),
}

/// The folder shown, as of the last render, for the prompts to act on, and what the
/// dialog that deletes it says.
#[derive(Default)]
struct Shown {
    folder: Option<String>,
    delete_title: String,
    delete_message: String,
}

/// A sheet that asks for a folder's name.
#[derive(Clone)]
struct NameSheet {
    sheet: naui::Sheet,
    label: naui::Label,
    name: naui::TextInput,
}

impl NameSheet {
    fn new(ui: &Ui) -> naui::Result<Self> {
        let sheet = ui.sheet("")?;
        let form = ui.stack(Orientation::Vertical)?;
        form.set_spacing(8.0);
        form.set_align(Align::Fill);
        form.set_sizing(Sizing::fill_width());
        let label = ui.label("")?;
        form.append(&label);
        let name = ui.text_input("")?;
        name.set_sizing(Sizing::fill_width());
        form.append(&name);
        sheet.set_child(&form);
        Ok(Self { sheet, label, name })
    }

    fn translate(&self, tr: &Translations, title: &str, button: &str) {
        self.sheet.set_title(&tr.get(title));
        self.label.set_text(&tr.get("sidebar-name"));
        self.name.set_placeholder(&tr.get("sidebar-default-name"));
        self.sheet.set_buttons(
            DialogButtons::new()
                .primary(tr.get(button))
                .cancel(tr.get("sidebar-cancel")),
        );
    }

    fn open(&self, name: &str) {
        self.name.set_text(name);
        self.sheet.open();
    }
}

/// Opens what asks for a folder's name or for a confirmation. Clone it to open them
/// from the app's menu too.
#[derive(Clone)]
pub struct Prompts {
    new: NameSheet,
    rename: NameSheet,
    delete: naui::Dialog,
    shown: Rc<RefCell<Shown>>,
}

impl Prompts {
    pub fn new_folder(&self) {
        self.new.open("");
    }

    /// Asks for a new name for the folder shown. Nothing for all the notes.
    pub fn rename_folder(&self) {
        if let Some(folder) = &self.shown.borrow().folder {
            self.rename.open(folder);
        }
    }

    /// Asks whether to delete the folder shown. Nothing for all the notes.
    pub fn delete_folder(&self) {
        {
            let shown = self.shown.borrow();
            if shown.folder.is_none() {
                return;
            }
            self.delete.set_title(&shown.delete_title);
            self.delete.set_message(&shown.delete_message);
        }
        self.delete.open();
    }
}

pub struct View {
    sidebar: naui::Sidebar,
    bar: naui::Stack,
    bar_title: naui::Label,
    rename_button: naui::Button,
    delete_button: naui::Button,
    prompts: Prompts,
    /// "All Notes", "Folders" and "New Folder" in the language shown.
    names: RefCell<[String; 3]>,
    /// "New Folder" in the language shown, the name of a folder made without typing
    /// one.
    default_name: Rc<RefCell<String>>,
    /// The folders shown, after "All Notes" and before "New Folder". The sidebar tells
    /// which item was picked by its number, which is turned into the folder's name
    /// here, so the message does not depend on the items staying the same.
    folders: Rc<RefCell<Vec<String>>>,
    /// The number of the item `render` selected last, to go back to once the new
    /// folder's sheet closes.
    index: Rc<Cell<usize>>,
}

pub fn build(ui: &Ui, dispatch: &Dispatch<Message>) -> naui::Result<View> {
    let sidebar = ui.sidebar()?;
    let folders: Rc<RefCell<Vec<String>>> = Rc::default();
    let index = Rc::new(Cell::new(0));
    let shown: Rc<RefCell<Shown>> = Rc::default();
    let default_name: Rc<RefCell<String>> = Rc::default();

    let new = NameSheet::new(ui)?;
    new.sheet.on_response({
        let dispatch = dispatch.clone();
        let name = new.name.clone();
        let default_name = Rc::clone(&default_name);
        let sidebar = sidebar.clone();
        let index = Rc::clone(&index);
        move |response| {
            if response == DialogResponse::Primary {
                let name = match name.text() {
                    typed if typed.trim().is_empty() => default_name.borrow().clone(),
                    typed => typed,
                };
                dispatch.send(Message::FolderNamed(name));
            }
            // "New Folder" is not a place: the selection goes back to the folder shown,
            // which is the new one once it is made.
            sidebar.set_selected(index.get());
        }
    });
    let rename = NameSheet::new(ui)?;
    rename.sheet.on_response({
        let dispatch = dispatch.clone();
        let name = rename.name.clone();
        let shown = Rc::clone(&shown);
        move |response| {
            let from = shown.borrow().folder.clone();
            if let (DialogResponse::Primary, Some(from)) = (response, from) {
                dispatch.send(Message::FolderRenamed {
                    from,
                    to: name.text(),
                });
            }
        }
    });
    let delete = ui.dialog("")?;
    delete.on_response({
        let dispatch = dispatch.clone();
        let shown = Rc::clone(&shown);
        move |response| {
            let folder = shown.borrow().folder.clone();
            if let (DialogResponse::Primary, Some(folder)) = (response, folder) {
                dispatch.send(Message::FolderDeleted(folder));
            }
        }
    });
    let prompts = Prompts {
        new,
        rename,
        delete,
        shown,
    };

    sidebar.on_select({
        let dispatch = dispatch.clone();
        let folders = Rc::clone(&folders);
        let prompts = prompts.clone();
        move |index: usize| {
            let folder = match index {
                0 => None,
                index => match folders.borrow().get(index - 1) {
                    Some(folder) => Some(folder.clone()),
                    None => {
                        prompts.new_folder();
                        return;
                    }
                },
            };
            dispatch.send(Message::Picked(folder));
        }
    });

    // The folder bar: the folder's name, and under it what can be done to it. The
    // name has a line of its own, since the list is narrow and the buttons are wide
    // on some platforms (GTK).
    let bar = ui.stack(Orientation::Vertical)?;
    bar.set_spacing(6.0);
    bar.set_padding(Padding {
        start: 12.0,
        end: 8.0,
        top: 10.0,
        bottom: 0.0,
    });
    bar.set_sizing(Sizing::fill_width());
    let bar_title = ui.label("")?;
    bar_title.set_style(TextStyle::Heading);
    bar_title.set_wrap(false);
    bar_title.set_sizing(Sizing::fill_width());
    let rename_button = ui.button("")?;
    rename_button.on_click({
        let prompts = prompts.clone();
        move || prompts.rename_folder()
    });
    let delete_button = ui.button("")?;
    delete_button.on_click({
        let prompts = prompts.clone();
        move || prompts.delete_folder()
    });
    let buttons = ui.stack(Orientation::Horizontal)?;
    buttons.set_spacing(8.0);
    buttons.set_align(Align::Center);
    buttons.set_sizing(Sizing::fill_width());
    buttons.append(&rename_button);
    buttons.append(&delete_button);
    bar.append(&bar_title);
    bar.append(&buttons);
    bar.set_visible(false);

    // The strings are set by `translate`, which the app calls before the first render.
    Ok(View {
        sidebar,
        bar,
        bar_title,
        rename_button,
        delete_button,
        prompts,
        names: RefCell::default(),
        default_name,
        folders,
        index,
    })
}

impl View {
    /// Attach it with `Window::set_sidebar`.
    pub fn sidebar(&self) -> &naui::Sidebar {
        &self.sidebar
    }

    /// The folder bar, for the app to place above the list. It is shown only while a
    /// folder is.
    pub fn folder_bar(&self) -> &dyn naui::Widget {
        &self.bar
    }

    pub fn prompts(&self) -> Prompts {
        self.prompts.clone()
    }

    /// Sets the strings that do not depend on the notes: called when the language
    /// changes, before `render`.
    pub fn translate(&self, tr: &Translations) {
        let prompts = &self.prompts;
        prompts
            .new
            .translate(tr, "sidebar-new-folder", "sidebar-create");
        prompts
            .rename
            .translate(tr, "sidebar-rename-title", "sidebar-rename-button");
        prompts.delete.set_buttons(
            DialogButtons::new()
                .primary(tr.get("sidebar-delete-button"))
                .cancel(tr.get("sidebar-cancel")),
        );
        self.rename_button.set_text(&tr.get("sidebar-rename"));
        self.delete_button.set_text(&tr.get("sidebar-delete"));
        *self.default_name.borrow_mut() = tr.get("sidebar-default-name");
        *self.names.borrow_mut() = [
            tr.get("sidebar-all-notes"),
            tr.get("sidebar-folders"),
            tr.get("sidebar-new-folder"),
        ];
        // The items are made again with the new names. `render` selects the folder
        // shown again.
        let folders = self.folders.borrow().clone();
        self.set_folders(&folders);
    }

    pub fn render(&self, notes: &Notes, tr: &Translations) {
        if *self.folders.borrow() != notes.folders() {
            self.set_folders(notes.folders());
        }
        let selected = match notes.folder() {
            None => Some(0),
            Some(folder) => notes
                .folders()
                .iter()
                .position(|f| f == folder)
                .map(|index| index + 1),
        };
        if let Some(index) = selected {
            self.index.set(index);
            if self.sidebar.selected() != Some(index) {
                self.sidebar.set_selected(index);
            }
        }

        let folder = notes.folder().map(str::to_owned);
        self.bar.set_visible(folder.is_some());
        self.bar_title.set_text(folder.as_deref().unwrap_or(""));
        let (delete_title, delete_message) = match &folder {
            None => Default::default(),
            Some(folder) => (
                tr.get_args("sidebar-delete-title", [("folder", folder.as_str().into())]),
                tr.get_args(
                    "sidebar-delete-message",
                    [("count", notes.count_in(folder).into())],
                ),
            ),
        };
        *self.prompts.shown.borrow_mut() = Shown {
            folder,
            delete_title,
            delete_message,
        };
    }

    fn set_folders(&self, folders: &[String]) {
        let [all_notes, folders_title, new_folder] = &*self.names.borrow();
        let items = folders
            .iter()
            .map(|folder| SidebarItem::new(folder).icon(ToolbarIcon::Open))
            .chain([SidebarItem::new(new_folder).icon(ToolbarIcon::Add)]);
        let sections = [
            SidebarSection::untitled([SidebarItem::new(all_notes).icon(ToolbarIcon::Copy)]),
            SidebarSection::new(folders_title, items),
        ];
        *self.folders.borrow_mut() = folders.to_vec();
        self.sidebar.set_sections(&sections);
    }
}
