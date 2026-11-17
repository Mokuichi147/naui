# notes

An example of a naui app split into crates by feature: a notes app with a sidebar
of folders, a list of notes and an editor, which keeps each note as a Markdown file.

```sh
cargo run -p notes
```

It is in English and Japanese, following the OS's language. On macOS and Windows
it also changes language while running when the OS's does; Linux decides
the language from the environment at startup. To start it in Japanese on macOS:

```sh
cargo run -p notes -- -AppleLanguages '(ja)'
```

The notes are kept in `Notes` in the home folder. Set `NOTES_DIR` to use another
folder:

- Each `.md` file is a note, and each subfolder is a folder in the sidebar (one
  level; folders whose names start with a dot are not shown). New Folder, the last
  item in the sidebar, makes one (named "New Folder" in the language shown if no
  name is typed), and the Folder field above a note moves the note
  to another folder. A new note goes in the folder shown.
- While a folder is shown, the bar above the list names it and has Rename and
  Delete; the Folder menu has them too. Deleting a folder moves it, with its notes,
  to `.trash` in the notes folder, after asking.
- A note's title is its first line. A note made in the app is named after its title,
  and renamed when the title changes. A file that was already there keeps its name
  until its title is changed in the app. A name already used by another file is
  never taken.
- Only the notes edited in the app are written. Deleting a note moves its file to
  `.trash` in the notes folder.
- Files that cannot be read are listed under the list and left as they are. If the
  folder itself cannot be read, the app shows why and writes nothing.
- Closing the window writes what is not written yet first. If that fails, the window
  stays open to show why, and closing it again gives up the change.

The app does not show the trash, or notice changes made by other programs while it
runs. Editing the same
note in another program at the same time loses one of the two edits. It runs on the
desktop platforms only, since it reads the files on another thread.

| Directory | Crate | Holds |
| --- | --- | --- |
| [`app`](app) | `notes` | `main`, the `Program` that turns the screens' messages and the commands into the core's, the window with its toolbar and menu, and every string in each language ([`locales`](app/locales)) |
| [`core`](core) | `notes-core` | the models, what can be done to them, and `update`, without naui |
| [`l10n`](l10n) | `notes-l10n` | choosing the language and looking the strings up in it, with Fluent and without naui |
| [`screens/sidebar`](screens/sidebar) | `notes-sidebar` | the folders, the folder bar, and the prompts that make, rename and delete folders |
| [`screens/list`](screens/list) | `notes-list` | the search and the list of notes, and the messages they send |
| [`screens/editor`](screens/editor) | `notes-editor` | the folder and text of the selected note, and the messages they send |
| [`services/store`](services/store) | `notes-store` | reading and writing the notes folder |

The core's tests build no naui at all:

```sh
cargo test -p notes-core -p notes-store -p notes-l10n
```

How the crates are divided, and what each change rebuilds, is in
[Structuring large apps](../../docs/large-apps.md).
