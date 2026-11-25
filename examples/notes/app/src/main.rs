//! A notes app split into crates. This one, the app crate, is the only one that knows
//! all the others, and it is kept thin:
//!
//! - Its `Message` wraps each screen's messages and its own commands (the toolbar and
//!   the menu), and `update` turns them into the core's, which
//!   `notes_core::Notes::update` handles. The current time is added here, so the core
//!   reads no clock.
//! - It turns the `Request` the core returns into an effect that calls the store
//!   service.
//! - Its `build` attaches the sidebar, the toolbar and the menu to a window and places
//!   the other screens in it, and `render` asks each screen to render.
//! - It keeps the window open until the notes are written.
//! - It keeps the translation of the language shown, and the strings themselves in
//!   `locales/`. The screens look them up by their IDs.
//!
//! See `docs/large-apps.md` (Structuring large apps) for how the crates are
//! divided and what each change rebuilds.

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, SystemTime};

use naui::{
    CloseResponse, MenuItem, MenuSpec, Orientation, Padding, Settings, Shortcut, Sizing, TextColor,
    TextStyle, ToolbarIcon, ToolbarItem, Ui,
};
use naui_app::{Dispatch, Effect, Program};
use notes_core::{Failure, Notes, Problem, Request, Status};
use notes_l10n::{Locales, Translations};
use notes_store::Store;

/// How long editing has to pause before the notes are written.
const PAUSE: Duration = Duration::from_millis(500);

/// Under this key each change's wait replaces the previous one, so typing writes once.
const SAVE: &str = "save";

/// How often the ages next to the notes ("5 minutes ago") are brought up to date.
const TICK: Duration = Duration::from_secs(30);

/// Every string the app and its screens show, in each language. English comes first:
/// it has every string, and is shown for what another language lacks. The files are
/// embedded, so nothing needs to be next to the executable.
const LOCALES: &Locales = &[
    ("en-US", include_str!("../locales/en-US/notes.ftl")),
    ("ja", include_str!("../locales/ja/notes.ftl")),
];

/// The app's messages: the screens' own, the app's commands, and the core's for what
/// comes back from the effects.
#[derive(Debug)]
enum Message {
    /// The OS's preferred languages, on starting and when they change.
    Languages(Vec<String>),
    Started,
    Tick,
    Command(Command),
    Sidebar(notes_sidebar::Message),
    List(notes_list::Message),
    Editor(notes_editor::Message),
    Notes(notes_core::Message),
}

/// What the toolbar and the menu do. Both belong to the window, so to the app.
#[derive(Clone, Copy, Debug)]
enum Command {
    NewNote,
    DeleteNote,
}

/// The toolbar's items, in order.
const TOOLBAR: [Command; 2] = [Command::NewNote, Command::DeleteNote];

/// `Program` is naui-app's trait and `Notes` is the core's type, so neither can be
/// implemented for the other here: the app wraps the model in a type of its own.
struct App {
    notes: Notes,
    store: Store,
    /// The strings in the language shown. The language is the app's to keep, not the
    /// core's, which has no words.
    tr: Translations,
    /// Counts the changes of language, for `render` to set the strings again.
    language: u64,
}

struct View {
    window: naui::Window,
    toolbar: naui::Toolbar,
    menu_bar: naui::MenuBar,
    sidebar: notes_sidebar::View,
    list: notes_list::View,
    editor: notes_editor::View,
    problem: naui::Label,
    notice: naui::Stack,
    /// [`Notes::can_close`] as of the last render. Asking the window to close sends a
    /// message, which runs `update` and `render` right away, inside the request: the
    /// request reads this first, so that a window that can close is not also closed
    /// from `render` while it is asking.
    can_close: Rc<Cell<bool>>,
    /// [`App::language`] as of the last render, `None` before the first.
    language: Cell<Option<u64>>,
}

impl App {
    fn new(store: Store) -> Self {
        Self {
            notes: Notes::new(),
            store,
            // Until `build` sends the OS's languages, the first one (English).
            tr: Translations::negotiate(&[] as &[&str], LOCALES),
            language: 0,
        }
    }

    /// Turns what happened on a screen, or a command, into what it does to the notes.
    /// This is where a new button that does something the core already does is
    /// connected, without changing the core.
    fn to_core(message: Message) -> notes_core::Message {
        use notes_core::Message as Notes;
        let now = SystemTime::now();
        match message {
            Message::Languages(_) => unreachable!("`update` keeps the languages"),
            Message::Started => Notes::Started(now),
            Message::Tick => Notes::Tick(now),
            Message::Command(Command::NewNote) => Notes::New(now),
            Message::Command(Command::DeleteNote) => Notes::Delete,
            Message::Sidebar(message) => match message {
                notes_sidebar::Message::Picked(folder) => Notes::SelectFolder(folder),
                notes_sidebar::Message::FolderNamed(name) => Notes::CreateFolder(name),
                notes_sidebar::Message::FolderRenamed { from, to } => {
                    Notes::RenameFolder { from, to }
                }
                notes_sidebar::Message::FolderDeleted(folder) => Notes::DeleteFolder(folder),
            },
            Message::List(message) => match message {
                notes_list::Message::SearchChanged(search) => Notes::SetSearch(search),
                notes_list::Message::Picked(id) => Notes::Select(id),
            },
            Message::Editor(message) => match message {
                notes_editor::Message::TextChanged(text) => Notes::SetText(text, now),
                notes_editor::Message::FolderPicked(folder) => Notes::Move(folder),
            },
            Message::Notes(message) => message,
        }
    }

    /// Turns what the core asked for into an effect. This is the one place where the
    /// core meets naui-app and the service meets the UI thread.
    fn effect(&self, request: Option<Request>) -> Effect<Message> {
        use notes_core::Message::{Loaded, Save, Saved};
        let store = self.store.clone();
        let effect = match request {
            None => Effect::none(),
            Some(Request::Load) => Effect::thread(move || Loaded(store.load())),
            Some(Request::SaveLater) => Effect::after(PAUSE, Save).key(SAVE),
            // The core asks for one write at a time, so this one is not keyed: a later
            // change must not cancel it.
            Some(Request::Write { ops, revision }) => Effect::thread(move || Saved {
                revision,
                results: store.apply(&ops),
            }),
        };
        effect.map(Message::Notes)
    }
}

impl Program for App {
    type Message = Message;
    type View = View;

    fn update(&mut self, message: Message) -> Effect<Message> {
        if let Message::Languages(languages) = message {
            self.tr = Translations::negotiate(&languages, LOCALES);
            self.language += 1;
            return Effect::none();
        }
        let ticks = matches!(message, Message::Started)
            .then(|| Effect::every(TICK, || Message::Tick))
            .unwrap_or_else(Effect::none);
        let request = self.notes.update(Self::to_core(message));
        self.effect(request).and(ticks)
    }

    fn render(&self, view: &View) {
        let notes = &self.notes;
        let tr = &self.tr;
        // The strings that do not depend on the notes are set only when the language
        // changes: setting the menus again on every keystroke would be wasted.
        if view.language.get() != Some(self.language) {
            view.language.set(Some(self.language));
            view.translate(tr);
        }
        view.sidebar.render(notes, tr);
        view.list.render(notes, tr);
        view.editor.render(notes, tr);
        let loaded = notes.is_loaded();
        let selected = notes.selected().is_some();
        for (index, command) in TOOLBAR.into_iter().enumerate() {
            let enabled = match command {
                Command::NewNote => loaded,
                Command::DeleteNote => selected,
            };
            view.toolbar.set_item_enabled(index, enabled);
        }
        view.menu_bar.set_item_enabled(&[0, 0], loaded);
        view.menu_bar.set_item_enabled(&[0, 2], selected);
        let folder = notes.folder().is_some();
        view.menu_bar.set_item_enabled(&[1, 0], loaded);
        view.menu_bar.set_item_enabled(&[1, 1], folder);
        view.menu_bar.set_item_enabled(&[1, 2], folder);

        let problem = match notes.status() {
            Status::Failed(problem) => Some(match problem {
                Problem::Read(reason) => {
                    tr.get_args("app-problem-read", [("reason", reason.into())])
                }
                Problem::Save(Failure::NameTaken(name)) => {
                    tr.get_args("app-problem-name-taken", [("name", name.into())])
                }
                Problem::Save(Failure::Io(reason)) => {
                    tr.get_args("app-problem-save", [("reason", reason.into())])
                }
            }),
            _ => match notes.skipped() {
                [] => None,
                [file] => Some(tr.get_args("app-skipped-file", [("file", file.as_str().into())])),
                files => Some(tr.get_args("app-skipped-files", [("count", files.len().into())])),
            },
        };
        view.notice.set_visible(problem.is_some());
        view.problem.set_text(problem.as_deref().unwrap_or(""));

        view.can_close.set(notes.can_close());
        if notes.should_close() {
            view.window.close();
        }
    }
}

impl View {
    /// Sets the window's strings, and asks each screen to set its own.
    fn translate(&self, tr: &Translations) {
        self.window.set_title(&tr.get("app-title"));
        // Setting the items again enables them all: `render` disables them after.
        self.toolbar.set_items(&[
            ToolbarItem::new(ToolbarIcon::New, tr.get("app-new-note")),
            ToolbarItem::new(ToolbarIcon::Delete, tr.get("app-delete")),
        ]);
        // The folders' commands open the sidebar screen's prompts, as its own items do.
        self.menu_bar.set_menus(&[
            MenuSpec::new(
                tr.get("app-menu-file"),
                [
                    MenuItem::new(tr.get("app-new-note")).shortcut(Shortcut::new('n')),
                    MenuItem::separator(),
                    MenuItem::new(tr.get("app-menu-delete-note")),
                ],
            ),
            MenuSpec::new(
                tr.get("app-menu-folder"),
                [
                    MenuItem::new(tr.get("app-menu-new-folder"))
                        .shortcut(Shortcut::new('n').shift(true)),
                    MenuItem::new(tr.get("app-menu-rename-folder")),
                    MenuItem::new(tr.get("app-menu-delete-folder")),
                ],
            ),
        ]);
        self.sidebar.translate(tr);
        self.list.translate(tr);
        self.editor.translate(tr);
    }
}

fn build(ui: &Ui, dispatch: &Dispatch<Message>) -> naui::Result<View> {
    // The strings are set by the first render, in the OS's language.
    let window = ui.window("", 960.0, 620.0)?;
    window.set_min_size(640.0, 400.0);

    // Each screen gets a dispatch for its own messages only.
    let sidebar = notes_sidebar::build(ui, &dispatch.wrap(Message::Sidebar))?;
    let list = notes_list::build(ui, &dispatch.wrap(Message::List))?;
    let editor = notes_editor::build(ui, &dispatch.wrap(Message::Editor))?;
    window.set_sidebar(sidebar.sidebar());

    let toolbar = ui.toolbar()?;
    toolbar.on_activate(dispatch.map(|index: usize| Message::Command(TOOLBAR[index])));
    window.set_toolbar(&toolbar);

    let menu_bar = ui.menu_bar()?;
    let commands = dispatch.clone();
    let prompts = sidebar.prompts();
    menu_bar.on_activate(move |path: &[usize]| match path {
        [0, 0] => commands.send(Message::Command(Command::NewNote)),
        [0, 2] => commands.send(Message::Command(Command::DeleteNote)),
        [1, 0] => prompts.new_folder(),
        [1, 1] => prompts.rename_folder(),
        [1, 2] => prompts.delete_folder(),
        _ => {}
    });
    window.set_menu_bar(&menu_bar);

    // Shown under the list when something could not be read or written.
    let problem = ui.label("")?;
    problem.set_style(TextStyle::Caption);
    problem.set_color(TextColor::Danger);
    problem.set_wrap(true);
    problem.set_sizing(Sizing::fill_width());
    let notice = ui.stack(Orientation::Vertical)?;
    notice.set_padding(Padding {
        start: 16.0,
        end: 16.0,
        top: 0.0,
        bottom: 12.0,
    });
    notice.set_sizing(Sizing::fill_width());
    notice.append(&problem);
    notice.set_visible(false);
    let left = ui.stack(Orientation::Vertical)?;
    left.set_sizing(Sizing::fill());
    left.append(sidebar.folder_bar());
    left.append(&list);
    left.append(&notice);

    let split = ui.split_view(Orientation::Horizontal)?;
    split.set_start(&left);
    split.set_end(&editor);
    split.set_min_sizes(220.0, 300.0);
    split.set_position(280.0);
    split.set_sizing(Sizing::fill());
    window.set_child(&split);

    // Closing waits until the notes are written: the core asks for the write, and
    // `render` closes the window once it is done.
    let can_close = Rc::new(Cell::new(true));
    window.on_close_request({
        let dispatch = dispatch.clone();
        let can_close = Rc::clone(&can_close);
        move || {
            if can_close.get() {
                CloseResponse::Close
            } else {
                dispatch.send(Message::Notes(notes_core::Message::Close));
                CloseResponse::KeepOpen
            }
        }
    });
    // The languages come in a message, like anything else that changes the screen.
    dispatch.send(Message::Languages(ui.preferred_languages()));
    ui.on_preferred_languages_change(
        dispatch.with(|languages: &[String]| Message::Languages(languages.to_vec())),
    );
    window.show();

    // Loading is an effect like any other, asked for by a message.
    dispatch.send(Message::Started);
    Ok(View {
        window,
        toolbar,
        menu_bar,
        sidebar,
        list,
        editor,
        problem,
        notice,
        can_close,
        language: Cell::new(None),
    })
}

/// The notes folder: `NOTES_DIR` if it is set, or `Notes` in the home folder.
fn notes_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("NOTES_DIR") {
        return dir.into();
    }
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"));
    home.map(PathBuf::from).unwrap_or_default().join("Notes")
}

fn main() -> naui::Result<()> {
    naui_app::run(
        Settings::new("notes"),
        App::new(Store::new(notes_dir())),
        build,
    )
}

/// The core's own tests cover `update`. Here only the app's part is left: that the
/// screens' messages and the commands reach the core, and which effect each request
/// becomes.
#[cfg(test)]
mod tests {
    use super::*;
    use notes_core::Library;

    fn app() -> App {
        let mut app = App::new(Store::new("unused"));
        let _ = app.update(Message::Started);
        let _ = app.notes.update(notes_core::Message::Loaded(Ok(Library {
            folders: vec!["Work".into()],
            ..Library::default()
        })));
        app
    }

    #[test]
    fn starting_loads_on_another_thread_and_starts_the_clock() {
        let mut app = App::new(Store::new("unused"));
        let started = app.update(Message::Started);
        assert!(!started.is_none());
        assert!(started.keys().is_empty());
        assert!(app.notes.now() > SystemTime::UNIX_EPOCH);
    }

    #[test]
    fn the_screens_messages_and_the_commands_reach_the_core() {
        let mut app = app();
        let _ = app.update(Message::Sidebar(notes_sidebar::Message::Picked(Some(
            "Work".into(),
        ))));
        assert_eq!(app.notes.folder(), Some("Work"));
        let _ = app.update(Message::Command(Command::NewNote));
        let id = app.notes.selected().map(|note| note.id());
        assert_eq!(app.notes.selected().unwrap().folder(), "Work");
        let _ = app.update(Message::Editor(notes_editor::Message::TextChanged(
            "Plan".into(),
        )));
        assert_eq!(app.notes.selected().unwrap().title(), "Plan");
        let _ = app.update(Message::List(notes_list::Message::SearchChanged(
            "x".into(),
        )));
        assert_eq!(app.notes.search(), "x");
        let _ = app.update(Message::List(notes_list::Message::Picked(None)));
        assert!(app.notes.selected().is_none());
        let _ = app.update(Message::List(notes_list::Message::Picked(id)));
        let _ = app.update(Message::Command(Command::DeleteNote));
        assert!(app.notes.selected().is_none());
    }

    #[test]
    fn each_change_waits_under_one_key_and_the_write_is_not_cancelled() {
        let mut app = app();
        assert_eq!(
            app.update(Message::Command(Command::NewNote)).keys(),
            [SAVE]
        );
        let typed = app.update(Message::Editor(notes_editor::Message::TextChanged(
            "a".into(),
        )));
        assert_eq!(typed.keys(), [SAVE]);
        // A later change does not cancel a write already asked for.
        let write = app.update(Message::Notes(notes_core::Message::Save));
        assert!(!write.is_none());
        assert!(write.keys().is_empty());
    }

    #[test]
    fn the_languages_are_the_apps_and_change_the_strings() {
        let mut app = app();
        assert_eq!(app.tr.get("app-title"), "Notes");
        assert!(app
            .update(Message::Languages(vec!["ja-JP".into(), "en-US".into()]))
            .is_none());
        assert_eq!(app.tr.get("app-title"), "メモ");
        assert_eq!(app.language, 1);
        // The notes are as they were.
        assert_eq!(app.notes.folders(), ["Work"]);
    }

    #[test]
    fn every_language_has_every_string() {
        assert_eq!(notes_l10n::mismatches(LOCALES), []);
    }

    #[test]
    fn closing_with_a_change_waiting_writes_it_at_once() {
        let mut app = app();
        let _ = app.update(Message::Command(Command::NewNote));
        let write = app.update(Message::Notes(notes_core::Message::Close));
        assert!(!write.is_none());
        assert!(write.keys().is_empty());
        assert!(!app.notes.should_close());
    }
}
