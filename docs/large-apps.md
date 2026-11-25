# Structuring large apps

[naui-app](../crates/naui-app) gives an app a structure inside one crate: a model,
typed messages, `update` and `render`, split into components. As the app grows, so
does the time it takes to build after each change, because Cargo compiles a crate
as a whole. Cargo does reuse the build of crates that did not change, so splitting
the app into several crates by feature means a change rebuilds only the crates it
touches and the ones depending on them.

This guide splits an app into four kinds of crates:

1. [An app crate](#the-app-crate) that puts the rest together
2. [A core crate](#the-core-crate) with the models and `update`, without naui
3. [A crate per screen](#screen-crates) with the widgets and their messages
4. [Services](#services) for the work outside the UI, such as files

with [translations](#translations) beside them, and then measures
[what each change rebuilds](#what-each-change-rebuilds), which decides
[where to draw the boundaries](#where-to-draw-the-boundaries).

The complete, runnable version is [`examples/notes`](../examples/notes): a notes app
that keeps each note as a Markdown file in a folder, with a sidebar of folders, a
list of notes and an editor. It is in English and Japanese, following the OS's
language.

```sh
cargo run -p notes
```

It runs on the desktop platforms. On the Web there is no file system and no
threads, so the store and `Effect::thread` would have to be replaced there.

## The crates

```text
examples/notes/
  app/              notes          main, Program, the window, the toolbar and the menu,
                                   and every string in each language (locales/)
  core/             notes-core     models, messages and update
  l10n/             notes-l10n     looking the strings up in the user's language
  screens/sidebar/  notes-sidebar  the folders, the folder bar and its prompts, and their messages
  screens/list/     notes-list     the search and the list of notes, and their messages
  screens/editor/   notes-editor   the folder and text of the selected note, and their messages
  services/store/   notes-store    reading and writing the notes folder
```

They depend on each other like this. Only the app crate knows all of them, and the
screens and the service know only the core:

```text
                      notes (app)
          /          |          |          \
  notes-sidebar  notes-list  notes-editor  notes-store
          \          |          |          /
                      notes-core
```

The app and the three screens also depend on `notes-l10n`, which depends on neither
the core nor naui.

| Crate | Depends on | Changes when |
| --- | --- | --- |
| `notes` | all the others, `naui`, `naui-app` | the window, the commands, what a screen's message does, how a request runs, or a string in any language |
| `notes-sidebar`, `notes-list`, `notes-editor` | `notes-core`, `notes-l10n`, `naui`, `naui-app` | a screen's widgets and the messages they send |
| `notes-store` | `notes-core` | how files are read and written |
| `notes-l10n` | `fluent` | how a language is chosen and a string looked up |
| `notes-core` | nothing | what can be done to the notes |

In the example they are members of naui's workspace. In an app of your own, give
them a workspace of their own and keep the versions of shared dependencies in one
place:

```toml
# Cargo.toml at the root of the app
[workspace]
resolver = "2"
members = ["app", "core", "screens/*", "services/*"]

[workspace.dependencies]
naui = "0.20"
naui-app = "0.20"
notes-core = { path = "core" }
```

```toml
# screens/list/Cargo.toml
[dependencies]
naui.workspace = true
naui-app.workspace = true
notes-core.workspace = true
```

## The core crate

The core holds the models, the messages and `update`, and **depends on nothing**,
naui included:

```sh
$ cargo tree -p notes-core
notes-core v0.20.0 (examples/notes/core)
```

This is what makes it the core:

- Its tests build no UI. From an empty target directory, `cargo test -p
  notes-core` took 1.1 s, while building `notes` took 17.8 s, almost all of it
  naui and the platform crates under it. The tests run on any machine, including a
  Linux CI runner without GTK.
- Nothing about widgets can leak into the model, so `update` stays the plain Rust
  that naui-app's `update` is meant to be.

Leaving out naui also leaves out naui-app, which depends on it, and that shapes the
rest of the design.

### `update` returns requests, not effects

`naui_app::Effect` is out of reach, so the core's `update` returns a plain value
saying what the app is to do, and the app crate turns it into an effect:

```rust
/// What `Notes::update` asks the app to do. It does nothing by itself.
pub enum Request {
    /// Read the notes folder, and send back `Message::Loaded`.
    Load,
    /// Send `Message::Save` once editing pauses.
    SaveLater,
    /// Make these changes, and send back `Message::Saved` with a result for each.
    Write { ops: Vec<Op>, revision: u64 },
}

/// A change to the files, for the store to make.
pub enum Op {
    Put { folder: String, file: String, text: String, replaces: Option<(String, String)> },
    Trash { folder: String, file: String },
    MakeFolder { folder: String },
    RenameFolder { from: String, to: String },
    TrashFolder { folder: String },
}

impl Notes {
    pub fn update(&mut self, message: Message) -> Option<Request> { /* … */ }
}
```

A request says what is wanted, not how: whether `Load` runs on a thread or in an
async runtime, and where the folder is, is the app's business. Tests check
requests with `assert_eq!`, as they would check any value.

The more a request decides, the more the core's tests cover. Here the core chooses
every file name: a note made in the app is named after its first line, a name is
never taken from another file (ignoring case, as macOS and Windows do), and a file
read from the folder keeps its name until its title is changed in the app. Those
rules are tested without touching a file, and the store is left with writing what
it is told and refusing to write over a file that is not the note's own.

The current time comes in messages too (`Message::New(SystemTime)`,
`Message::Tick(SystemTime)`), added by the app, so `update` reads no clock and the
order of the notes in a test is exact.

The core has no words either, since the words depend on the language shown. What it
would say, it returns as values for the screens and the app to put in words:

- A note's age is an `Age` (`Minutes(5)`, `Yesterday`…), which the list shows as
  "5 minutes ago" or "5 分前".
- What went wrong is a `Problem` (`Read(reason)`, or `Save(Failure::NameTaken(name))`
  and so on), and the system's reason is passed on as the system gave it.
- An empty title stays empty, and the list shows it as "New Note".
- A folder made without a name gets one from the sidebar screen, "New Folder" in the
  language shown, since that name is written to the disk too.

What the requests leave to the app, the core cannot count on: in particular, two
writes running on two threads can finish in either order, and the older text
would be left in the file. So the order is kept in the model, where the core's own
tests cover it:

- The core asks for one `Write` at a time. When it is time to save while a write
  runs, it waits for `Message::Saved` and asks for the next write then. A note
  renamed or deleted meanwhile is renamed or trashed under the name that write
  gave it, and a note written while its folder is renamed is found in the renamed
  folder.
- `Message::Saved` has a result for each op, and the core marks a note written only
  if its op succeeded and the note did not change since. A write that fails halfway
  is followed by one with only what failed.
- Closing does not wait for the pause: `Message::Close` asks for the write at once,
  and `Notes::can_close` and `Notes::should_close` tell the app when the window may
  close. If that write fails, the window stays open to show why, and the next request
  to close closes it without the change.

### Messages say what happens to the notes

The core's messages are named for what they do to the notes, not for the screen
or the button that asks for it:

```rust
pub enum Message {
    Started(SystemTime),
    Tick(SystemTime),
    Loaded(Result<Library, String>),
    SelectFolder(Option<String>),
    SetSearch(String),
    Select(Option<NoteId>),
    New(SystemTime),
    SetText(String, SystemTime),
    Delete,
    CreateFolder(String),
    Move(String),
    RenameFolder { from: String, to: String },
    DeleteFolder(String),
    Save,
    Saved { revision: u64, results: Vec<Result<(), Failure>> },
    Close,
}
```

The screens have messages of their own, in their own crates, and the
[app crate](#the-app-crate) turns them into these. A button is then a change to its
screen, and a message the core does not have yet is a change to what can be done to
the notes. Were the screens' messages kept in the core, adding a button would
change the core, and [rebuild every crate but one](#what-each-change-rebuilds).

The screens also do not send positions: a row's position changes with the search
and a sidebar item's with the folders, so `Select` takes the note's id and
`SelectFolder` the folder's name, and each screen keeps what it shows to turn the
position picked into one.

### `Program` lives in the app crate

`Program` is naui-app's trait and `Notes` is the core's type. Rust's orphan rule
allows `impl Program for Notes` only in one of those two crates: the core does not
depend on naui-app, and neither a screen crate nor the app crate is allowed to
write it. The app crate wraps the model in a type of its own instead (see
[below](#the-app-crate)).

## Screen crates

A screen crate holds widgets and the messages they send: a message type, a
function that builds the widgets and connects their callbacks to it, and a `render`
that sets them from the core's model.

```rust
// notes-list
pub enum Message {
    SearchChanged(String),
    Picked(Option<NoteId>),
}

pub fn build(ui: &Ui, dispatch: &Dispatch<Message>) -> naui::Result<View> {
    // …
    search.on_change(dispatch.with(|text: &str| Message::SearchChanged(text.to_owned())));
    // …
}

// The app places the view itself: `left.append(&list)`.
impl naui::AsWidget for View {
    fn as_widget(&self) -> &dyn naui::Widget { &self.root }
}

impl View {
    /// Sets the strings that do not depend on the notes, when the language changes.
    pub fn translate(&self, tr: &Translations) {
        self.search.set_placeholder(&tr.get("list-search"));
    }

    pub fn render(&self, notes: &Notes, tr: &Translations) {
        self.search.set_text(notes.search());
        // …
    }
}
```

The screen reads the whole model through `&Notes`, but only through the core's
public methods (`visible`, `selected`, `folders`), which are what it depends on. A
screen gets a dispatch for its own messages only, so the sidebar has no way to
delete a note.

A screen hands the app what the app places. Most implement `naui::AsWidget`,
returning their root widget, so the app passes the view itself to a container;
the sidebar is attached to the window instead of placed, so `notes-sidebar`'s view
gives its `naui::Sidebar` (`View::sidebar`) and the app calls `Window::set_sidebar`
with it.

Dialogs that ask about one screen's things belong to that screen. The sidebar
screen has the sheets that ask for a folder's name and the dialog that confirms
deleting one, and sends what they decide (`FolderNamed`, `FolderRenamed`,
`FolderDeleted`); the core makes the actual name, one no other folder or file has.
They are opened from the sidebar's New Folder item, from the folder bar the screen
gives the app to place above the list, and from the app's Folder menu, which
reaches them through a handle the screen hands out (`View::prompts`), so the menu
stays the app's and the prompts stay the screen's.

`render` runs after every message, so a screen keeps what it showed last and
changes only what differs. The list builds its rows when they come into view
(`List::set_row_builder`), and while the same notes stay in the same order (as
while typing in the editor, which changes a title and an age) it only rebuilds the
rows in view (`List::refresh`), so the selection and the scrolling stay.

The screens are not `Component`s. A component is a `Program` with its own model,
and the models here are in the core, which cannot implement naui-app's traits.
What components give is then done by the app:

- **Mounting.** The screens are always there, so the app builds them once in
  `build`. `render` receives neither the `Ui` nor a `Dispatch`, so for a screen
  that comes and goes the app's view keeps clones of both, along with a
  `RefCell<Option<View>>`: `render` builds the screen and places its root when the
  model calls for it, and removes the root and drops the view when it no longer
  does.
- **Cancelling a child's effects.** Without `Scoped`, the app uses effect keys
  (`Effect::key`, `Effect::cancel`), and the core ignores results that arrive too
  late, as `revision` does above.

A part of a screen whose state concerns no one else (a dialog's draft, an expanded
section) can still be a `Component` defined in the screen crate. The core cannot
hold its model, so the model sits beside the core's in the app's wrapper, and its
messages are one more case of the app's message type. Keep the state the rest of
the app reads in the core.

## Services

A service does the work outside the UI: files, databases, the network. It takes
plain values and returns plain results, and knows nothing of messages, effects or
widgets, so it can run on another thread and be tested on its own:

```rust
// notes-store
impl Store {
    pub fn load(&self) -> Result<Library, String> { /* … */ }
    pub fn apply(&self, ops: &[Op]) -> Vec<Result<(), Failure>> { /* … */ }
}
```

Its own tests use real folders: notes in subfolders, a file that is not UTF-8
(skipped and left as it is), a rename onto a file that is not the note's, a folder
made and a note moved into it, and the trash.

`notes-store` depends on the core for `Library` and `Op`, so a change in the core
rebuilds it. A service that takes only standard types (strings, byte buffers) does
not even depend on the core, and is rebuilt only when it changes itself.

## Translations

A screen shows strings, but the strings are not in the screen crates. They are in
the app crate, one Fluent file per language (`app/locales/en-US/notes.ftl`,
`app/locales/ja/notes.ftl`), each screen's starting with its name
(`sidebar-new-folder`, `list-search`). The screens look them up by their IDs in a
`Translations` the app passes to their `render`:

```rust
// notes-list
fn age(tr: &Translations, age: Age) -> String {
    let (id, count) = match age {
        Age::JustNow => return tr.get("list-just-now"),
        Age::Minutes(count) => ("list-minutes-ago", count),
        // …
    };
    tr.get_args(id, [("count", count.into())])
}
```

`Translations` is in a crate of its own, `notes-l10n`, which chooses the languages
from the OS's preferred ones and looks a string up in them, falling back to English
for what a translation lacks. It holds no strings, depends on neither the core nor
naui, and seldom changes. So changing a string, in any language, rebuilds only the
app crate (see [the table](#what-each-change-rebuilds)), and translators work in one
folder. Kept in each screen's crate instead, the strings would rebuild that screen
and the app; kept in `notes-l10n`, every screen.

The language is the app's state, like the notes, and changes the screen through a
message. `build` sends the OS's languages, and `Ui::on_preferred_languages_change`
sends them again when they change:

```rust
dispatch.send(Message::Languages(ui.preferred_languages()));
ui.on_preferred_languages_change(dispatch.with(|languages: &[String]| {
    Message::Languages(languages.to_vec())
}));
```

`update` keeps the translation in the app's wrapper, not in the core, which has no
words, and counts the changes. `render` then sets every string again: the strings
that depend on the notes (an age, the dialog that names the folder to delete) on
every render, and the others (the menus, the toolbar, the sidebar's "All Notes")
only when the language has changed, through each screen's `translate`. naui-app
renders after every message anyway, so this replaces the list of bindings that
[Translating apps](translating.md) keeps for an app without it.

A string a translation lacks shows in English without a word, so the app crate's
tests check that every language has the same IDs (`notes_l10n::mismatches`).

## The app crate

The app crate is the only one that knows all the others, and is kept thin. It
wraps the core's model to implement `Program`, turns the screens' messages and its
own commands into the core's and the core's requests into effects that call the
service, and puts the screens in a window:

```rust
enum Message {
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
enum Command {
    NewNote,
    DeleteNote,
}

struct App {
    notes: Notes,
    store: Store,
    tr: Translations,
    language: u64,
}

impl App {
    fn to_core(message: Message) -> notes_core::Message {
        use notes_core::Message as Notes;
        let now = SystemTime::now();
        match message {
            Message::Command(Command::NewNote) => Notes::New(now),
            Message::Command(Command::DeleteNote) => Notes::Delete,
            Message::Sidebar(notes_sidebar::Message::Picked(folder)) => Notes::SelectFolder(folder),
            Message::Sidebar(notes_sidebar::Message::FolderNamed(name)) => Notes::CreateFolder(name),
            Message::Editor(notes_editor::Message::FolderPicked(folder)) => Notes::Move(folder),
            Message::Editor(notes_editor::Message::TextChanged(text)) => Notes::SetText(text, now),
            // … the others the same way
            Message::Notes(message) => message,
        }
    }

    fn effect(&self, request: Option<Request>) -> Effect<Message> {
        use notes_core::Message::{Loaded, Save, Saved};
        let store = self.store.clone();
        let effect = match request {
            None => Effect::none(),
            Some(Request::Load) => Effect::thread(move || Loaded(store.load())),
            Some(Request::SaveLater) => Effect::after(PAUSE, Save).key(SAVE),
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
        let request = self.notes.update(Self::to_core(message));
        self.effect(request)
    }

    fn render(&self, view: &View) {
        if view.language.get() != Some(self.language) {
            view.language.set(Some(self.language));
            view.translate(&self.tr); // the window's strings, then each screen's
        }
        view.sidebar.render(&self.notes, &self.tr);
        view.list.render(&self.notes, &self.tr);
        view.editor.render(&self.notes, &self.tr);
        // …
    }
}

fn build(ui: &Ui, dispatch: &Dispatch<Message>) -> naui::Result<View> {
    // …
    let sidebar = notes_sidebar::build(ui, &dispatch.wrap(Message::Sidebar))?;
    let list = notes_list::build(ui, &dispatch.wrap(Message::List))?;
    let editor = notes_editor::build(ui, &dispatch.wrap(Message::Editor))?;
    window.set_sidebar(sidebar.sidebar());
    toolbar.on_activate(dispatch.map(|index: usize| Message::Command(TOOLBAR[index])));
    // …
}
```

`Dispatch::wrap` gives each screen a dispatch of its own message type, whose
messages arrive in the app's queue wrapped as `Message::Sidebar`, `Message::List`
or `Message::Editor`. Everything still goes through one queue, so the guarantee
against re-entering `update` holds across crates.

`to_core` is where a message gets its meaning. The toolbar and the menu send the
same `Command`, which becomes the same core message, and a new button that does
something the core already does (a "Clear" button sending `SetSearch` with an
empty search) is connected here, without changing the core.

Closing goes through the core too. The window's `on_close_request` keeps the window
open while `Notes::can_close` is false and sends `Close`, and `render` closes the
window once `Notes::should_close` says the notes are written. The request has to
answer at once, and a message sent from it runs `update` and `render` before it
returns, so it reads whether the window can close from a value `render` left in the
view, rather than sending a message and letting `render` close the window while the
request is still asking:

```rust
window.on_close_request(move || {
    if can_close.get() {
        CloseResponse::Close
    } else {
        dispatch.send(Message::Notes(notes_core::Message::Close));
        CloseResponse::KeepOpen
    }
});
```

The app crate's own tests are about what it adds: that the screens' messages and
the commands reach the core, and which effect each request becomes (`Effect::keys`
and `Effect::is_none` tell them apart without a window).

## What each change rebuilds

Each row changes one thing in `examples/notes`, then runs `cargo build -p notes`
(Rust 1.99, the default `dev` profile) and lists the crates Cargo compiled:

| Change | Compiled again |
| --- | --- |
| A private detail of the sidebar | `notes-sidebar`, `notes` |
| A private detail of the list screen | `notes-list`, `notes` |
| A private detail of the editor screen | `notes-editor`, `notes` |
| A private detail of the store | `notes-store`, `notes` |
| A private detail of the app | `notes` |
| A string in one language (`app/locales/*/notes.ftl`) | `notes` |
| A new button in the list screen, with a message of its own that the app turns into `SetSearch` | `notes-list`, `notes` |
| A private detail of `notes-l10n` | `notes-l10n`, the three screens, `notes` |
| The body of a function in the core | all but `notes-l10n` |
| A comment in the core | all but `notes-l10n` |
| A new public function in the core | all but `notes-l10n` |

naui, naui-app and the platform crates under them were not compiled again in any
of these: dependencies the app does not change stay built, split or not.

Note the last three rows. **Cargo does not look at what changed**: once a crate is
compiled again, for any reason, every crate depending on it is compiled again too,
whether its public API changed or only a comment did. So the boundaries are not
about keeping public APIs stable; they are about which crates change at all, and
how many crates depend on those.

Crates that do not depend on each other (the three screens and the store) are
compiled in parallel, so a wide, shallow graph also makes a full rebuild of the
app's own crates faster than a deep chain.

## Where to draw the boundaries

From the table above:

- **What changes often goes in the leaves.** Screens, the window and its commands,
  and the details of services change most, and few crates (only the app) depend on
  them.
- **Keep the core small and slow to change.** Almost everything depends on it, so
  every change to it rebuilds almost everything. A type only one screen uses belongs in that
  screen's crate, not in the core: the screens' messages are the first of these,
  since they change with every button.
- **Screens do not depend on each other.** They meet in the app crate. Widget
  helpers shared by several screens make a crate that all of them depend on: it
  becomes a second core, and the same rule applies to it.
- **Services depend on as little as they can.** Standard types at the boundary
  keep a service out of the core's rebuilds.
- **Data that changes often but is read at run time goes where few crates depend
  on it.** The translations are only strings to the screens, so they sit in the
  app crate, and the crate the screens do depend on holds only the lookup.
- **Split the core by feature once it grows.** Per-feature crates (the notes, the
  settings, the account) that depend on a small shared crate keep a change to one
  feature's model from rebuilding the screens of the others. The app crate's
  message type then combines the features' messages, as it combines the screens'
  here.
- **Do not split too finely.** Each crate costs a `Cargo.toml`, a separate rustc
  invocation and its own metadata, and crates that always change together rebuild
  together anyway. Splitting pays off when one crate takes long enough to build
  that waiting for it is noticeable, and parts of it change independently.

Start with one crate structured with naui-app, and move parts out along these
lines when the build time asks for it: the model and `update` into a core first,
since that is also what makes them testable without naui, then the screens that
change the most.
