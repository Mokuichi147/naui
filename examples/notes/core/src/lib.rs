//! The models and `update` of the notes example, in plain Rust.
//!
//! This crate depends on nothing, naui included. The screens, the store and the app
//! depend on it, so it is kept small and its public API steady: a change here
//! rebuilds all of them, while a change in a screen rebuilds only that screen and the
//! app.
//!
//! [`Notes::update`] cannot return a `naui_app::Effect`, since that would bring in
//! naui. It returns a [`Request`] instead, a plain value saying what the app is to do
//! (load, write), and the app crate turns it into an effect. The current time comes in
//! messages too, so `update` reads no clock and its tests are exact.
//!
//! The notes are Markdown files in a folder, one per note, and the folder's
//! subfolders are the folders of the sidebar. The model decides every file name, so
//! the store only does what it is told:
//!
//! - A note made here is named after its title (its first line), and renamed when the
//!   title changes. A file read from the folder keeps its name until its title is
//!   changed here.
//! - A name is never taken from another file, whether a note or not, and comparing
//!   names ignores case, as macOS and Windows do.
//! - Only notes changed here are written, so the dates of the others stay, and so do
//!   edits made to them in other programs.
//! - A deleted note is moved to the trash folder, not removed.
//!
//! Nothing here is in words, so it is the same in every language: what goes wrong is
//! a [`Problem`], a note's age an [`Age`], and an empty title stays empty. The screens
//! and the app put them in words in the language shown.

use std::time::{Duration, SystemTime};

/// What identifies a note while the app runs. Not its file name, which changes with
/// its title.
pub type NoteId = u64;

/// A note as the store read it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredNote {
    /// The subfolder it is in, or `""` for the notes folder itself.
    pub folder: String,
    pub file: String,
    pub text: String,
    pub modified: SystemTime,
}

/// What the store found in the notes folder.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Library {
    /// The subfolders, in the order to show them.
    pub folders: Vec<String>,
    pub notes: Vec<StoredNote>,
    /// Every name in use in each folder, `(folder, name)`, notes or not, so that a new
    /// file name does not take one.
    pub taken: Vec<(String, String)>,
    /// Files that could not be read. They are left as they are.
    pub skipped: Vec<String>,
}

/// A change to the files, for the store to make.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Op {
    /// Write `text` to `file` in `folder`. `replaces` is the note's file so far,
    /// `(folder, file)`, if it has one: when it is another file it is removed after the
    /// write, and any other file already called `file` is never written over.
    Put {
        folder: String,
        file: String,
        text: String,
        replaces: Option<(String, String)>,
    },
    /// Move `file` in `folder` to the trash.
    Trash { folder: String, file: String },
    /// Make the subfolder `folder`.
    MakeFolder { folder: String },
    /// Rename the subfolder `from` to `to`, or make `to` if `from` is not there.
    RenameFolder { from: String, to: String },
    /// Move the subfolder `folder`, with its notes, to the trash.
    TrashFolder { folder: String },
}

/// Why an [`Op`] failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Failure {
    /// The name asked for, from the notes folder down, is another file's, made by
    /// another program perhaps.
    NameTaken(String),
    /// Anything else: no space, no permission… The file, and the system's reason in
    /// the system's words.
    Io(String),
}

/// What went wrong, for the app to put in words.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Problem {
    /// The notes folder could not be read, for the reason the store gave.
    Read(String),
    /// A write failed.
    Save(Failure),
}

/// What [`Notes::update`] asks the app to do. It does nothing by itself.
#[derive(Debug, PartialEq, Eq)]
pub enum Request {
    /// Read the notes folder, and send back [`Message::Loaded`].
    Load,
    /// Send [`Message::Save`] once editing pauses. A later request replaces an earlier
    /// one, so typing saves once, not on every key.
    SaveLater,
    /// Make these changes, and send back [`Message::Saved`] with this revision and a
    /// result for each, in order. There is never more than one write at a time.
    Write { ops: Vec<Op>, revision: u64 },
}

/// What can happen to the notes. The messages are named for what they do, not for the
/// screen that asks for it: the screens have messages of their own, which the app
/// turns into these.
#[derive(Debug)]
pub enum Message {
    /// Sent once when the window is up, with the time.
    Started(SystemTime),
    /// The time, now and then, for the ages shown next to the notes.
    Tick(SystemTime),
    Loaded(Result<Library, String>),
    /// Show the notes of this folder, or of all of them.
    SelectFolder(Option<String>),
    SetSearch(String),
    /// Edit this note, or none.
    Select(Option<NoteId>),
    /// Add an empty note to the folder shown, at this time, and edit it.
    New(SystemTime),
    /// Change the text of the note being edited, at this time.
    SetText(String, SystemTime),
    /// Delete the note being edited.
    Delete,
    /// Make a folder, named after this, show it, and put new notes in it.
    CreateFolder(String),
    /// Move the note being edited to this folder (`""` for the notes folder itself).
    Move(String),
    /// Rename a folder, after what was typed.
    RenameFolder {
        from: String,
        to: String,
    },
    /// Move a folder and its notes to the trash.
    DeleteFolder(String),
    /// Editing paused: time to write.
    Save,
    /// The results of a [`Request::Write`], one for each op.
    Saved {
        revision: u64,
        results: Vec<Result<(), Failure>>,
    },
    /// The window is to close once the notes are written (see [`Notes::can_close`]).
    Close,
}

/// Whether what is on screen is also in the files.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    Loading,
    Saved,
    Unsaved,
    Failed(Problem),
}

#[derive(Clone, Debug)]
pub struct Note {
    id: NoteId,
    folder: String,
    text: String,
    modified: SystemTime,
    /// The name it is written under in `folder`: its file so far, or a new name.
    file: String,
    /// The file it has on disk, `(folder, file)`, if any.
    on_disk: Option<(String, String)>,
    /// The title `file` was made from. Until the title changes, the name stays.
    named_for: String,
    /// The revision of its last change, and the revision written last.
    changed: u64,
    written: u64,
    /// Deleted, and kept until its file is in the trash.
    deleted: bool,
}

impl Note {
    pub fn id(&self) -> NoteId {
        self.id
    }

    pub fn folder(&self) -> &str {
        &self.folder
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn modified(&self) -> SystemTime {
        self.modified
    }

    /// The first line that is not blank, without Markdown's leading `#`.
    pub fn title(&self) -> &str {
        title(&self.text)
    }

    /// The start of the text after the title, on one line, without Markdown's marks
    /// for headings and list items.
    pub fn preview(&self) -> String {
        let mut lines = self
            .text
            .lines()
            .map(|line| {
                let line = line.trim().trim_start_matches('#').trim_start();
                ["- ", "* ", "+ "]
                    .iter()
                    .find_map(|mark| line.strip_prefix(mark))
                    .unwrap_or(line)
                    .trim()
            })
            .filter(|line| !line.is_empty());
        lines.next();
        let preview: String = lines
            .collect::<Vec<_>>()
            .join(" ")
            .chars()
            .take(100)
            .collect();
        preview
    }

    fn is_dirty(&self) -> bool {
        self.changed > self.written
    }
}

fn title(text: &str) -> &str {
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map_or("", |line| line.trim_start_matches('#').trim())
}

/// A write that has been asked for and not answered, one for each op.
#[derive(Debug)]
enum Flight {
    Put {
        id: NoteId,
        folder: String,
        file: String,
    },
    Trash {
        id: NoteId,
    },
    Folder {
        op: Op,
    },
}

pub struct Notes {
    notes: Vec<Note>,
    next_id: NoteId,
    folders: Vec<String>,
    /// Names in use that are not notes' files, `(folder, name)`.
    taken: Vec<(String, String)>,
    /// Changes to folders made in the app and not on disk yet, in order. They are
    /// written before the notes.
    folder_ops: Vec<Op>,
    /// Every folder renamed since the folder was read, `(from, to)`, in order, so
    /// that a write that ends after a rename knows where its file is now.
    renames: Vec<(String, String)>,
    skipped: Vec<String>,
    /// The folder shown, or all of them.
    folder: Option<String>,
    selected: Option<NoteId>,
    search: String,
    now: SystemTime,
    /// Counts the changes, so that a write knows which of them it covers.
    revision: u64,
    /// Whether the folder was read. Until then nothing is written, so files that could
    /// not be read are not replaced.
    loaded: bool,
    problem: Option<Problem>,
    /// The write running, if any. Writes run on other threads, so two at once could
    /// finish in either order: the next one waits for this one instead.
    writing: Option<Vec<Flight>>,
    /// Whether it was time to save while a write was running.
    save_waiting: bool,
    /// Whether the window is waiting for the notes to be written to close.
    closing: bool,
    /// Whether the write before closing failed. The failure is on screen, so asking
    /// to close again closes without the changes.
    close_failed: bool,
}

impl Default for Notes {
    fn default() -> Self {
        Self::new()
    }
}

impl Notes {
    pub fn new() -> Self {
        Self {
            notes: Vec::new(),
            next_id: 0,
            folders: Vec::new(),
            taken: Vec::new(),
            folder_ops: Vec::new(),
            renames: Vec::new(),
            skipped: Vec::new(),
            folder: None,
            selected: None,
            search: String::new(),
            now: SystemTime::UNIX_EPOCH,
            revision: 0,
            loaded: false,
            problem: None,
            writing: None,
            save_waiting: false,
            closing: false,
            close_failed: false,
        }
    }

    #[must_use = "the request is what the app is to do next"]
    pub fn update(&mut self, message: Message) -> Option<Request> {
        match message {
            Message::Started(now) => {
                self.now = now;
                Some(Request::Load)
            }
            Message::Tick(now) => {
                self.now = now;
                None
            }
            Message::Loaded(Ok(library)) => {
                self.load(library);
                None
            }
            Message::Loaded(Err(error)) => {
                self.problem = Some(Problem::Read(error));
                None
            }
            Message::SelectFolder(folder) => {
                if folder.as_ref().is_none_or(|f| self.folders.contains(f)) {
                    self.folder = folder;
                    if !self.is_visible(self.selected) {
                        self.selected = self.visible().first().map(|note| note.id);
                    }
                }
                None
            }
            Message::SetSearch(search) => {
                self.search = search;
                // The note being edited is hidden: the first one found is edited instead.
                // With nothing found, it stays.
                if !self.is_visible(self.selected) {
                    if let Some(first) = self.visible().first() {
                        self.selected = Some(first.id);
                    }
                }
                None
            }
            Message::Select(id) => {
                self.selected = id.filter(|&id| self.note(id).is_some());
                None
            }
            Message::New(_) if !self.loaded => None,
            Message::New(now) => {
                let id = self.next_id;
                self.next_id += 1;
                self.notes.push(Note {
                    id,
                    folder: self.folder.clone().unwrap_or_default(),
                    text: String::new(),
                    modified: now,
                    file: String::new(),
                    on_disk: None,
                    named_for: String::new(),
                    changed: 0,
                    written: 0,
                    deleted: false,
                });
                self.selected = Some(id);
                // A new note would be hidden by most searches.
                self.search.clear();
                self.changed(id)
            }
            Message::SetText(text, now) => {
                let id = self.selected?;
                let note = self.note_mut(id)?;
                if note.text == text {
                    return None;
                }
                note.text = text;
                note.modified = now;
                self.changed(id)
            }
            Message::Delete => {
                let id = self.selected?;
                let visible: Vec<NoteId> = self.visible().iter().map(|note| note.id).collect();
                self.selected = visible.iter().position(|&v| v == id).and_then(|row| {
                    visible
                        .get(row + 1)
                        .or_else(|| row.checked_sub(1).and_then(|row| visible.get(row)))
                        .copied()
                });
                let in_flight = self
                    .writing
                    .iter()
                    .flatten()
                    .any(|flight| matches!(flight, Flight::Put { id: put, .. } if *put == id));
                if self.note(id)?.on_disk.is_none() && !in_flight {
                    // Never written: there is no file to move to the trash.
                    self.notes.retain(|note| note.id != id);
                    return None;
                }
                self.note_mut(id)?.deleted = true;
                self.changed(id)
            }
            // The screen gives a name, "New Folder" in the language shown if none was
            // typed: the core does not know the language.
            Message::CreateFolder(name) if !self.loaded || name.trim().is_empty() => None,
            Message::CreateFolder(name) => {
                let folder = self.folder_name(&name);
                self.folders.push(folder.clone());
                self.folders.sort_by_key(|f| f.to_lowercase());
                self.folder_ops.push(Op::MakeFolder {
                    folder: folder.clone(),
                });
                self.folder = Some(folder);
                self.selected = None;
                self.search.clear();
                self.revision += 1;
                Some(Request::SaveLater)
            }
            Message::Move(folder) => {
                if !folder.is_empty() && !self.folders.contains(&folder) {
                    return None;
                }
                let id = self.selected?;
                let note = self.note_mut(id)?;
                if note.folder == folder {
                    return None;
                }
                note.folder = folder;
                self.changed(id)
            }
            Message::RenameFolder { from, to } => {
                if !self.folders.contains(&from) {
                    return None;
                }
                let to = self.folder_name_except(&to, &from);
                if to == from {
                    return None;
                }
                for folder in &mut self.folders {
                    if *folder == from {
                        *folder = to.clone();
                    }
                }
                self.folders.sort_by_key(|f| f.to_lowercase());
                // The notes go with their folder, on screen at once and on disk with
                // the folder's rename.
                for note in &mut self.notes {
                    if note.folder == from {
                        note.folder = to.clone();
                    }
                    if let Some((folder, _)) = &mut note.on_disk {
                        if *folder == from {
                            *folder = to.clone();
                        }
                    }
                }
                for (folder, _) in &mut self.taken {
                    if *folder == from {
                        *folder = to.clone();
                    }
                }
                if self.folder.as_ref() == Some(&from) {
                    self.folder = Some(to.clone());
                }
                self.renames.push((from.clone(), to.clone()));
                self.folder_ops.push(Op::RenameFolder { from, to });
                self.revision += 1;
                Some(Request::SaveLater)
            }
            Message::DeleteFolder(folder) => {
                if !self.folders.contains(&folder) {
                    return None;
                }
                self.folders.retain(|f| *f != folder);
                // Its notes go to the trash with it, as they are on disk.
                self.notes.retain(|note| note.folder != folder);
                self.taken.retain(|(f, _)| *f != folder);
                if self.folder.as_ref() == Some(&folder) {
                    self.folder = None;
                }
                if !self.is_visible(self.selected) {
                    self.selected = self.visible().first().map(|note| note.id);
                }
                self.folder_ops.push(Op::TrashFolder { folder });
                self.revision += 1;
                Some(Request::SaveLater)
            }
            Message::Save if self.writing.is_some() => {
                self.save_waiting = true;
                None
            }
            Message::Save => self.write(),
            Message::Saved { revision, results } => self.saved(revision, results),
            Message::Close => {
                self.closing = true;
                if self.can_close() {
                    None
                } else if self.writing.is_some() {
                    self.save_waiting = true;
                    None
                } else {
                    self.write()
                }
            }
        }
    }

    fn load(&mut self, library: Library) {
        let Library {
            folders,
            notes,
            taken,
            skipped,
        } = library;
        self.notes = notes
            .into_iter()
            .enumerate()
            .map(|(index, stored)| Note {
                id: index as NoteId,
                named_for: title(&stored.text).to_owned(),
                on_disk: Some((stored.folder.clone(), stored.file.clone())),
                folder: stored.folder,
                file: stored.file,
                text: stored.text,
                modified: stored.modified,
                changed: 0,
                written: 0,
                deleted: false,
            })
            .collect();
        self.next_id = self.notes.len() as NoteId;
        self.taken = taken
            .into_iter()
            .filter(|(folder, name)| {
                !self
                    .notes
                    .iter()
                    .any(|note| &note.folder == folder && &note.file == name)
            })
            .collect();
        self.folders = folders;
        self.skipped = skipped;
        self.loaded = true;
        self.problem = None;
        self.selected = self.visible().first().map(|note| note.id);
    }

    fn changed(&mut self, id: NoteId) -> Option<Request> {
        self.revision += 1;
        let revision = self.revision;
        self.note_mut_any(id)?.changed = revision;
        Some(Request::SaveLater)
    }

    /// Asks for the changes not written yet, if there are any.
    fn write(&mut self) -> Option<Request> {
        if !self.loaded {
            return None;
        }
        // Deleted notes that never reached the disk need no write.
        self.notes
            .retain(|note| !(note.deleted && note.on_disk.is_none()));
        let mut ops = Vec::new();
        let mut flights = Vec::new();
        for op in &self.folder_ops {
            ops.push(op.clone());
            flights.push(Flight::Folder { op: op.clone() });
        }
        let dirty: Vec<NoteId> = self
            .notes
            .iter()
            .filter(|note| note.is_dirty())
            .map(|note| note.id)
            .collect();
        for id in dirty {
            let note = self.note_any(id).expect("a dirty note is there");
            if note.deleted {
                let (folder, file) = note
                    .on_disk
                    .clone()
                    .expect("only deleted notes on disk are left");
                ops.push(Op::Trash { folder, file });
                flights.push(Flight::Trash { id });
                continue;
            }
            let same_title = note.title() == note.named_for;
            let file = match &note.on_disk {
                Some((folder, old)) if same_title && *folder == note.folder => old.clone(),
                // Moved to another folder: the name goes with it, if it is free there.
                Some((_, old)) if same_title => {
                    let stem = old.rsplit_once('.').map_or(old.as_str(), |(stem, _)| stem);
                    self.free_name(&note.folder, stem, id)
                }
                old => {
                    let file = self.free_name(&note.folder, &file_stem(note.title()), id);
                    // A name that differs only in case is the same file on macOS and
                    // Windows but not on Linux, so the name is left as it is.
                    match old {
                        Some((folder, old))
                            if *folder == note.folder
                                && old.to_lowercase() == file.to_lowercase() =>
                        {
                            old.clone()
                        }
                        _ => file,
                    }
                }
            };
            let note = self.note_mut_any(id).expect("a dirty note is there");
            note.named_for = title(&note.text).to_owned();
            note.file = file.clone();
            let folder = note.folder.clone();
            ops.push(Op::Put {
                folder: folder.clone(),
                file: file.clone(),
                text: note.text.clone(),
                replaces: note.on_disk.clone(),
            });
            flights.push(Flight::Put { id, folder, file });
        }
        if ops.is_empty() {
            return None;
        }
        self.writing = Some(flights);
        Some(Request::Write {
            ops,
            revision: self.revision,
        })
    }

    fn saved(&mut self, revision: u64, results: Vec<Result<(), Failure>>) -> Option<Request> {
        let flights = self.writing.take().unwrap_or_default();
        let waiting = std::mem::take(&mut self.save_waiting);
        let mut results = results.into_iter();
        let mut failure = None;
        for flight in flights {
            let result = results
                .next()
                .unwrap_or_else(|| Err(Failure::Io("no result for a change".to_owned())));
            match (flight, result) {
                (Flight::Put { id, folder, file }, Ok(())) => {
                    // The folder may have been renamed since the write began.
                    let folder = self.follow(folder);
                    if let Some(note) = self.note_mut_any(id) {
                        note.on_disk = Some((folder, file));
                        note.written = revision;
                    }
                }
                (Flight::Trash { id }, Ok(())) => self.notes.retain(|note| note.id != id),
                (Flight::Folder { op }, Ok(())) => {
                    if let Some(index) = self.folder_ops.iter().position(|o| *o == op) {
                        self.folder_ops.remove(index);
                    }
                }
                (Flight::Put { id, folder, file }, Err(error)) => {
                    let place = Some((self.follow(folder.clone()), file.clone()));
                    let renamed = self.note_mut_any(id).filter(|note| note.on_disk != place);
                    if let Some(note) = renamed {
                        // Not written under its new name: the next write names it
                        // again, and if another program took the name, picks another.
                        note.named_for.clear();
                        note.file.clear();
                        if let Failure::NameTaken(_) = error {
                            self.taken.push((self.follow(folder), file));
                        }
                    }
                    failure.get_or_insert(error);
                }
                (Flight::Trash { .. } | Flight::Folder { .. }, Err(error)) => {
                    failure.get_or_insert(error);
                }
            }
        }
        match failure {
            None => {
                self.problem = None;
                self.close_failed = false;
            }
            Some(error) => {
                self.problem = Some(Problem::Save(error));
                if self.closing {
                    self.closing = false;
                    self.close_failed = true;
                    return None;
                }
            }
        }
        if (waiting || self.closing) && self.is_dirty() {
            self.write()
        } else {
            None
        }
    }

    /// A file name, `stem.md` or `stem 2.md`…, that no other file in `folder` has.
    fn free_name(&self, folder: &str, stem: &str, except: NoteId) -> String {
        let used = |name: &str| {
            let name = name.to_lowercase();
            let same = |file: &str| file.to_lowercase() == name;
            self.taken
                .iter()
                .any(|(f, taken)| f == folder && same(taken))
                || self.notes.iter().any(|note| {
                    note.id != except
                        && ((note.folder == folder && same(&note.file))
                            || note
                                .on_disk
                                .as_ref()
                                .is_some_and(|(f, file)| f == folder && same(file)))
                })
        };
        (1..)
            .map(|n| match n {
                1 => format!("{stem}.md"),
                n => format!("{stem} {n}.md"),
            })
            .find(|name| !used(name))
            .expect("some number is free")
    }

    /// A folder name made from what was typed, that no other folder or file in the
    /// notes folder has.
    fn folder_name(&self, typed: &str) -> String {
        self.folder_name_except(typed, "")
    }

    /// Like [`folder_name`](Self::folder_name), letting `except` keep its name, or
    /// change only its case.
    fn folder_name_except(&self, typed: &str, except: &str) -> String {
        let stem = match typed.trim() {
            "" => return except.to_owned(),
            typed => file_stem(typed),
        };
        let used = |name: &str| {
            let name = name.to_lowercase();
            self.folders
                .iter()
                .any(|f| f != except && f.to_lowercase() == name)
                || self
                    .taken
                    .iter()
                    .any(|(f, taken)| f.is_empty() && taken.to_lowercase() == name)
        };
        (1..)
            .map(|n| match n {
                1 => stem.clone(),
                n => format!("{stem} {n}"),
            })
            .find(|name| !used(name))
            .expect("some number is free")
    }

    /// Where `folder` is now, after the renames made since.
    fn follow(&self, mut folder: String) -> String {
        for (from, to) in &self.renames {
            if *from == folder {
                folder = to.clone();
            }
        }
        folder
    }

    fn note(&self, id: NoteId) -> Option<&Note> {
        self.notes
            .iter()
            .find(|note| note.id == id && !note.deleted)
    }

    fn note_mut(&mut self, id: NoteId) -> Option<&mut Note> {
        self.notes
            .iter_mut()
            .find(|note| note.id == id && !note.deleted)
    }

    fn note_any(&self, id: NoteId) -> Option<&Note> {
        self.notes.iter().find(|note| note.id == id)
    }

    fn note_mut_any(&mut self, id: NoteId) -> Option<&mut Note> {
        self.notes.iter_mut().find(|note| note.id == id)
    }

    fn is_dirty(&self) -> bool {
        !self.folder_ops.is_empty() || self.notes.iter().any(Note::is_dirty)
    }

    fn is_visible(&self, id: Option<NoteId>) -> bool {
        id.is_some_and(|id| self.visible().iter().any(|note| note.id == id))
    }

    /// The notes of the folder shown that the search lets through, the latest first.
    pub fn visible(&self) -> Vec<&Note> {
        let search = self.search.trim().to_lowercase();
        let mut notes: Vec<&Note> = self
            .notes
            .iter()
            .filter(|note| !note.deleted)
            .filter(|note| self.folder.as_ref().is_none_or(|f| &note.folder == f))
            .filter(|note| search.is_empty() || note.text.to_lowercase().contains(&search))
            .collect();
        notes.sort_by(|a, b| b.modified.cmp(&a.modified).then(b.id.cmp(&a.id)));
        notes
    }

    /// How many notes are in `folder`.
    pub fn count_in(&self, folder: &str) -> usize {
        self.notes
            .iter()
            .filter(|note| !note.deleted && note.folder == folder)
            .count()
    }

    /// The subfolders of the notes folder.
    pub fn folders(&self) -> &[String] {
        &self.folders
    }

    /// The folder shown, or `None` for all of them.
    pub fn folder(&self) -> Option<&str> {
        self.folder.as_deref()
    }

    pub fn search(&self) -> &str {
        &self.search
    }

    /// The note being edited. It stays selected when the search finds nothing.
    pub fn selected(&self) -> Option<&Note> {
        self.note(self.selected?)
    }

    /// The time of the last [`Message::Started`] or [`Message::Tick`].
    pub fn now(&self) -> SystemTime {
        self.now
    }

    /// Whether the notes folder has been read, so that notes can be made.
    pub fn is_loaded(&self) -> bool {
        self.loaded
    }

    /// The files that could not be read, and are left as they are.
    pub fn skipped(&self) -> &[String] {
        &self.skipped
    }

    pub fn status(&self) -> Status {
        match &self.problem {
            Some(problem) => Status::Failed(problem.clone()),
            None if !self.loaded => Status::Loading,
            None if self.writing.is_some() || self.is_dirty() => Status::Unsaved,
            None => Status::Saved,
        }
    }

    /// Whether the window can close without losing a change: everything is written,
    /// or nothing can be written (reading failed), or writing failed on closing.
    pub fn can_close(&self) -> bool {
        !self.loaded || self.close_failed || (self.writing.is_none() && !self.is_dirty())
    }

    /// Whether the window was asked to close and now can.
    pub fn should_close(&self) -> bool {
        self.closing && self.can_close()
    }
}

/// A file name, without `.md`, made from a title: characters that some systems do not
/// allow in names become `-`, and names Windows keeps for devices get a `_`.
fn file_stem(title: &str) -> String {
    let stem: String = title
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '-',
            c if c.is_control() => '-',
            c => c,
        })
        .take(60)
        .collect();
    let stem = stem.trim_matches(|c: char| c == '.' || c.is_whitespace());
    if stem.is_empty() {
        return "Untitled".to_owned();
    }
    let device = stem.split('.').next().unwrap_or("").to_ascii_uppercase();
    let reserved = matches!(device.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ((device.starts_with("COM") || device.starts_with("LPT"))
            && device.len() == 4
            && device.as_bytes()[3].is_ascii_digit());
    if reserved {
        format!("{stem}_")
    } else {
        stem.to_owned()
    }
}

/// How long ago something was, rounded down to the largest unit, for a screen to put
/// in words ("Just now", "5 minutes ago", "Yesterday"…).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Age {
    JustNow,
    Minutes(u64),
    Hours(u64),
    Yesterday,
    Days(u64),
    Weeks(u64),
    Months(u64),
    Years(u64),
}

/// How long ago `then` was.
pub fn age(now: SystemTime, then: SystemTime) -> Age {
    let seconds = now.duration_since(then).unwrap_or(Duration::ZERO).as_secs();
    match seconds {
        0..60 => Age::JustNow,
        60..3_600 => Age::Minutes(seconds / 60),
        3_600..86_400 => Age::Hours(seconds / 3_600),
        86_400..172_800 => Age::Yesterday,
        172_800..604_800 => Age::Days(seconds / 86_400),
        604_800..2_592_000 => Age::Weeks(seconds / 604_800),
        2_592_000..31_536_000 => Age::Months(seconds / 2_592_000),
        _ => Age::Years(seconds / 31_536_000),
    }
}

/// `update` is plain Rust, and so are its requests: these tests build no naui at all.
#[cfg(test)]
mod tests {
    use super::*;

    fn at(seconds: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000 + seconds)
    }

    fn stored(folder: &str, file: &str, text: &str, modified: u64) -> StoredNote {
        StoredNote {
            folder: folder.to_owned(),
            file: file.to_owned(),
            text: text.to_owned(),
            modified: at(modified),
        }
    }

    fn loaded(folders: &[&str], notes: Vec<StoredNote>) -> Notes {
        let mut model = Notes::new();
        assert_eq!(
            model.update(Message::Started(at(1000))),
            Some(Request::Load)
        );
        let taken = notes
            .iter()
            .map(|note| (note.folder.clone(), note.file.clone()))
            .collect();
        let library = Library {
            folders: folders.iter().map(|&f| f.to_owned()).collect(),
            notes,
            taken,
            skipped: Vec::new(),
        };
        assert_eq!(model.update(Message::Loaded(Ok(library))), None);
        model
    }

    fn titles(notes: &Notes) -> Vec<&str> {
        notes.visible().iter().map(|note| note.title()).collect()
    }

    fn selected_title(notes: &Notes) -> Option<&str> {
        notes.selected().map(Note::title)
    }

    fn write(notes: &mut Notes) -> (Vec<Op>, u64) {
        match notes.update(Message::Save) {
            Some(Request::Write { ops, revision }) => (ops, revision),
            other => panic!("Save asks for a write, not {other:?}"),
        }
    }

    fn saved(revision: u64, count: usize) -> Message {
        Message::Saved {
            revision,
            results: vec![Ok(()); count],
        }
    }

    /// A put whose file so far, if any, is in the same folder.
    fn put(folder: &str, file: &str, text: &str, replaces: Option<&str>) -> Op {
        moved(folder, file, text, replaces.map(|old| (folder, old)))
    }

    fn moved(folder: &str, file: &str, text: &str, replaces: Option<(&str, &str)>) -> Op {
        Op::Put {
            folder: folder.to_owned(),
            file: file.to_owned(),
            text: text.to_owned(),
            replaces: replaces.map(|(f, old)| (f.to_owned(), old.to_owned())),
        }
    }

    fn type_text(notes: &mut Notes, text: &str, time: u64) {
        assert_eq!(
            notes.update(Message::SetText(text.into(), at(time))),
            Some(Request::SaveLater)
        );
    }

    #[test]
    fn titles_and_previews_come_from_the_text() {
        let note = |text: &str| {
            let notes = loaded(&[], vec![stored("", "a.md", text, 0)]);
            let note = notes.selected().unwrap().clone();
            (note.title().to_owned(), note.preview())
        };
        assert_eq!(
            note("\n# Groceries\n\n## Dairy\n- milk\n * eggs \n"),
            ("Groceries".to_owned(), "Dairy milk eggs".to_owned())
        );
        assert_eq!(note("   \n"), (String::new(), String::new()));
    }

    #[test]
    fn the_latest_note_comes_first_and_is_selected() {
        let notes = loaded(
            &[],
            vec![
                stored("", "old.md", "Old", 10),
                stored("", "new.md", "New", 30),
                stored("", "mid.md", "Mid", 20),
            ],
        );
        assert_eq!(titles(&notes), ["New", "Mid", "Old"]);
        assert_eq!(selected_title(&notes), Some("New"));
        assert_eq!(notes.status(), Status::Saved);
    }

    #[test]
    fn folders_and_the_search_choose_the_notes_shown() {
        let mut notes = loaded(
            &["Work"],
            vec![
                stored("", "home.md", "Garden\nplant tomatoes", 30),
                stored("Work", "plan.md", "Plan\nrelease in the garden room", 20),
                stored("Work", "todo.md", "Todo", 10),
            ],
        );
        assert_eq!(titles(&notes), ["Garden", "Plan", "Todo"]);
        // The note selected is not in the folder: its first note is selected instead.
        let _ = notes.update(Message::SelectFolder(Some("Work".into())));
        assert_eq!(titles(&notes), ["Plan", "Todo"]);
        assert_eq!(selected_title(&notes), Some("Plan"));
        let _ = notes.update(Message::SetSearch("GARDEN".into()));
        assert_eq!(titles(&notes), ["Plan"]);
        let _ = notes.update(Message::SelectFolder(None));
        assert_eq!(titles(&notes), ["Garden", "Plan"]);
        // A folder that is not there is not shown.
        let _ = notes.update(Message::SelectFolder(Some("Gone".into())));
        assert_eq!(notes.folder(), None);
    }

    #[test]
    fn a_new_note_goes_in_the_folder_shown_and_is_named_after_its_title() {
        let mut notes = loaded(&["Work"], vec![stored("", "a.md", "A", 0)]);
        let _ = notes.update(Message::SelectFolder(Some("Work".into())));
        assert_eq!(notes.update(Message::New(at(50))), Some(Request::SaveLater));
        assert_eq!(selected_title(&notes), Some(""));
        assert_eq!(notes.status(), Status::Unsaved);
        type_text(&mut notes, "Standup\n- review", 60);
        let (ops, revision) = write(&mut notes);
        assert_eq!(ops, [put("Work", "Standup.md", "Standup\n- review", None)]);
        let _ = notes.update(saved(revision, 1));
        assert_eq!(notes.status(), Status::Saved);
        assert!(notes.can_close());
    }

    #[test]
    fn the_search_selects_the_first_note_found_when_it_hides_the_selected_one() {
        let mut notes = loaded(
            &[],
            vec![
                stored("", "a.md", "Apple", 20),
                stored("", "b.md", "Banana", 10),
            ],
        );
        assert_eq!(selected_title(&notes), Some("Apple"));
        let _ = notes.update(Message::SetSearch("ban".into()));
        assert_eq!(selected_title(&notes), Some("Banana"));
        // With nothing found, the note stays.
        let _ = notes.update(Message::SetSearch("zzz".into()));
        assert_eq!(selected_title(&notes), Some("Banana"));
    }

    #[test]
    fn a_folder_made_in_the_app_is_shown_and_gets_the_new_notes() {
        let mut notes = loaded(&["Work"], vec![stored("Work", "a.md", "A", 0)]);
        // Not a folder, but its name is taken all the same.
        notes.taken.push((String::new(), "Plan".into()));
        assert_eq!(
            notes.update(Message::CreateFolder("travel/2026".into())),
            Some(Request::SaveLater)
        );
        assert_eq!(notes.folder(), Some("travel-2026"));
        assert!(notes.selected().is_none());
        let _ = notes.update(Message::CreateFolder(" work ".into()));
        let _ = notes.update(Message::CreateFolder("Plan".into()));
        // No name: the screen gives one.
        assert_eq!(notes.update(Message::CreateFolder("  ".into())), None);
        let _ = notes.update(Message::CreateFolder("Neuer Ordner".into()));
        assert_eq!(
            notes.folders(),
            ["Neuer Ordner", "Plan 2", "travel-2026", "Work", "work 2"]
        );
        let _ = notes.update(Message::New(at(10)));
        assert_eq!(notes.selected().unwrap().folder(), "Neuer Ordner");
        let (ops, revision) = write(&mut notes);
        let made: Vec<&Op> = ops
            .iter()
            .filter(|op| matches!(op, Op::MakeFolder { .. }))
            .collect();
        assert_eq!(made.len(), 4);
        assert_eq!(
            ops.last(),
            Some(&put("Neuer Ordner", "Untitled.md", "", None))
        );
        let _ = notes.update(saved(revision, ops.len()));
        assert!(notes.can_close());
    }

    #[test]
    fn a_note_moved_to_another_folder_takes_its_name_along() {
        let mut notes = loaded(
            &["Work"],
            vec![
                stored("", "2026-11-01.md", "Meeting", 20),
                stored("Work", "2026-11-01.md", "Other", 10),
            ],
        );
        assert_eq!(selected_title(&notes), Some("Meeting"));
        // Not a folder: nothing happens.
        assert_eq!(notes.update(Message::Move("Gone".into())), None);
        assert_eq!(
            notes.update(Message::Move("Work".into())),
            Some(Request::SaveLater)
        );
        let (ops, revision) = write(&mut notes);
        // The name is taken there, so it gets a number.
        assert_eq!(
            ops,
            [moved(
                "Work",
                "2026-11-01 2.md",
                "Meeting",
                Some(("", "2026-11-01.md"))
            )]
        );
        let _ = notes.update(saved(revision, 1));
        let _ = notes.update(Message::Move(String::new()));
        let (ops, _) = write(&mut notes);
        assert_eq!(
            ops,
            [moved(
                "",
                "2026-11-01 2.md",
                "Meeting",
                Some(("Work", "2026-11-01 2.md"))
            )]
        );
    }

    #[test]
    fn a_renamed_folder_takes_its_notes_along() {
        let mut notes = loaded(
            &["Home", "Work"],
            vec![stored("Work", "plan.md", "Plan", 0)],
        );
        let _ = notes.update(Message::SelectFolder(Some("Work".into())));
        let rename = |from: &str, to: &str| Message::RenameFolder {
            from: from.into(),
            to: to.into(),
        };
        assert_eq!(
            notes.update(rename("Work", "Jobs")),
            Some(Request::SaveLater)
        );
        assert_eq!(notes.folders(), ["Home", "Jobs"]);
        assert_eq!(notes.folder(), Some("Jobs"));
        assert_eq!(notes.selected().unwrap().folder(), "Jobs");
        // Only the folder is renamed on disk: the note goes with it.
        let (ops, revision) = write(&mut notes);
        assert_eq!(
            ops,
            [Op::RenameFolder {
                from: "Work".into(),
                to: "Jobs".into()
            }]
        );
        let _ = notes.update(saved(revision, 1));
        type_text(&mut notes, "Plan\nmore", 10);
        let (ops, revision) = write(&mut notes);
        assert_eq!(ops, [put("Jobs", "plan.md", "Plan\nmore", Some("plan.md"))]);
        let _ = notes.update(saved(revision, 1));
        // Another folder's name gets a number, a change of case does not, and an
        // empty name changes nothing.
        let _ = notes.update(rename("Jobs", "home"));
        assert_eq!(notes.folders(), ["Home", "home 2"]);
        let _ = notes.update(rename("home 2", "Home 2"));
        assert_eq!(notes.folders(), ["Home", "Home 2"]);
        assert_eq!(notes.update(rename("Home 2", " ")), None);
        assert_eq!(notes.update(rename("Gone", "x")), None);
    }

    #[test]
    fn a_note_written_while_its_folder_is_renamed_is_found_in_the_new_one() {
        let mut notes = loaded(&["Work"], vec![stored("Work", "plan.md", "Plan", 0)]);
        type_text(&mut notes, "Plan\nmore", 10);
        let (_, revision) = write(&mut notes);
        let _ = notes.update(Message::RenameFolder {
            from: "Work".into(),
            to: "Jobs".into(),
        });
        // The write began in Work, and the rename waits for it.
        assert_eq!(notes.update(Message::Save), None);
        let Some(Request::Write {
            ops,
            revision: next,
        }) = notes.update(saved(revision, 1))
        else {
            panic!("the rename follows the write");
        };
        assert_eq!(
            ops,
            [Op::RenameFolder {
                from: "Work".into(),
                to: "Jobs".into()
            }]
        );
        let _ = notes.update(saved(next, 1));
        type_text(&mut notes, "Plan\nmore and more", 20);
        let (ops, _) = write(&mut notes);
        assert_eq!(
            ops,
            [put(
                "Jobs",
                "plan.md",
                "Plan\nmore and more",
                Some("plan.md")
            )]
        );
    }

    #[test]
    fn a_deleted_folder_goes_to_the_trash_with_its_notes() {
        let mut notes = loaded(
            &["Work"],
            vec![
                stored("", "inbox.md", "Inbox", 30),
                stored("Work", "plan.md", "Plan", 20),
                stored("Work", "todo.md", "Todo", 10),
            ],
        );
        assert_eq!(notes.count_in("Work"), 2);
        let _ = notes.update(Message::SelectFolder(Some("Work".into())));
        // One note is moved out first, and stays.
        let _ = notes.update(Message::Select(Some(2)));
        let _ = notes.update(Message::Move(String::new()));
        assert_eq!(
            notes.update(Message::DeleteFolder("Work".into())),
            Some(Request::SaveLater)
        );
        assert!(notes.folders().is_empty());
        assert_eq!(notes.folder(), None);
        assert_eq!(titles(&notes), ["Inbox", "Todo"]);
        let (ops, revision) = write(&mut notes);
        assert_eq!(
            ops,
            [
                Op::TrashFolder {
                    folder: "Work".into()
                },
                moved("", "todo.md", "Todo", Some(("Work", "todo.md"))),
            ]
        );
        let _ = notes.update(saved(revision, 2));
        assert!(notes.can_close());
    }

    #[test]
    fn a_name_is_never_taken_from_another_file() {
        let mut notes = loaded(&[], vec![stored("", "Groceries.md", "Groceries", 0)]);
        // Not a note, but its name is taken all the same.
        notes.taken.push((String::new(), "Plan.md".into()));
        let _ = notes.update(Message::New(at(10)));
        type_text(&mut notes, "groceries", 11);
        let _ = notes.update(Message::New(at(12)));
        type_text(&mut notes, "Plan", 13);
        let _ = notes.update(Message::New(at(14)));
        let (ops, _) = write(&mut notes);
        assert_eq!(
            ops,
            [
                put("", "groceries 2.md", "groceries", None),
                put("", "Plan 2.md", "Plan", None),
                put("", "Untitled.md", "", None),
            ]
        );
    }

    #[test]
    fn titles_that_are_not_names_are_made_into_names() {
        assert_eq!(file_stem("a/b: c?"), "a-b- c-");
        assert_eq!(file_stem("..hidden. "), "hidden");
        assert_eq!(file_stem("con"), "con_");
        assert_eq!(file_stem("COM1.txt"), "COM1.txt_");
        assert_eq!(file_stem("Computer"), "Computer");
        assert_eq!(file_stem(" "), "Untitled");
        assert_eq!(file_stem(&"x".repeat(100)).len(), 60);
    }

    #[test]
    fn a_file_keeps_its_name_until_its_title_changes_here() {
        let mut notes = loaded(
            &[],
            vec![stored("", "2026-11-01.md", "# Meeting\nnotes", 0)],
        );
        type_text(&mut notes, "# Meeting\nnotes and more", 10);
        let (ops, revision) = write(&mut notes);
        assert_eq!(
            ops,
            [put(
                "",
                "2026-11-01.md",
                "# Meeting\nnotes and more",
                Some("2026-11-01.md")
            )]
        );
        let _ = notes.update(saved(revision, 1));
        type_text(&mut notes, "# Standup\nnotes and more", 20);
        let (ops, _) = write(&mut notes);
        assert_eq!(
            ops,
            [put(
                "",
                "Standup.md",
                "# Standup\nnotes and more",
                Some("2026-11-01.md")
            )]
        );
    }

    #[test]
    fn only_the_notes_changed_here_are_written() {
        let mut notes = loaded(
            &[],
            vec![stored("", "a.md", "A", 20), stored("", "b.md", "B", 10)],
        );
        assert_eq!(notes.update(Message::Save), None);
        let _ = notes.update(Message::Select(Some(1)));
        type_text(&mut notes, "B\nmore", 30);
        // Typing the same text again changes nothing.
        assert_eq!(
            notes.update(Message::SetText("B\nmore".into(), at(31))),
            None
        );
        let (ops, _) = write(&mut notes);
        assert_eq!(ops, [put("", "b.md", "B\nmore", Some("b.md"))]);
        // The note edited last comes first.
        assert_eq!(titles(&notes), ["B", "A"]);
    }

    #[test]
    fn deleting_moves_the_file_to_the_trash_and_selects_the_next_note() {
        let mut notes = loaded(
            &[],
            vec![
                stored("", "a.md", "A", 30),
                stored("", "b.md", "B", 20),
                stored("", "c.md", "C", 10),
            ],
        );
        let _ = notes.update(Message::Select(Some(1)));
        assert_eq!(notes.update(Message::Delete), Some(Request::SaveLater));
        assert_eq!(titles(&notes), ["A", "C"]);
        assert_eq!(selected_title(&notes), Some("C"));
        let (ops, revision) = write(&mut notes);
        assert_eq!(
            ops,
            [Op::Trash {
                folder: String::new(),
                file: "b.md".into()
            }]
        );
        let _ = notes.update(saved(revision, 1));
        assert!(notes.can_close());
        // The last one: the one before it is selected.
        let _ = notes.update(Message::Delete);
        assert_eq!(selected_title(&notes), Some("A"));
        let _ = notes.update(Message::Delete);
        assert_eq!(notes.selected().map(Note::id), None);
    }

    #[test]
    fn a_new_note_deleted_before_it_is_written_writes_nothing() {
        let mut notes = loaded(&[], Vec::new());
        let _ = notes.update(Message::New(at(10)));
        assert_eq!(notes.update(Message::Delete), None);
        // Nothing to write, so the window can close at once.
        assert!(notes.can_close());
        assert_eq!(notes.update(Message::Save), None);
        assert!(notes.visible().is_empty());
    }

    #[test]
    fn a_title_changed_only_in_case_keeps_the_name() {
        let mut notes = loaded(&[], vec![stored("", "plan.md", "plan", 0)]);
        type_text(&mut notes, "Plan", 10);
        let (ops, _) = write(&mut notes);
        assert_eq!(ops, [put("", "plan.md", "Plan", Some("plan.md"))]);
    }

    #[test]
    fn a_name_that_could_not_be_written_is_not_tried_again() {
        let mut notes = loaded(&[], vec![stored("", "a.md", "A", 0)]);
        type_text(&mut notes, "Plan", 10);
        let (ops, revision) = write(&mut notes);
        assert_eq!(ops, [put("", "Plan.md", "Plan", Some("a.md"))]);
        // Another program made Plan.md meanwhile.
        let _ = notes.update(Message::Saved {
            revision,
            results: vec![Err(Failure::NameTaken("Plan.md".into()))],
        });
        let (ops, _) = write(&mut notes);
        assert_eq!(ops, [put("", "Plan 2.md", "Plan", Some("a.md"))]);
    }

    #[test]
    fn a_note_deleted_during_its_first_write_trashes_the_file_that_write_made() {
        let mut notes = loaded(&[], Vec::new());
        let _ = notes.update(Message::New(at(10)));
        type_text(&mut notes, "Draft", 11);
        let (ops, first) = write(&mut notes);
        assert_eq!(ops, [put("", "Draft.md", "Draft", None)]);
        let _ = notes.update(Message::Delete);
        // It waits for the first write, and then trashes what it wrote.
        assert_eq!(notes.update(Message::Save), None);
        assert_eq!(
            notes.update(saved(first, 1)),
            Some(Request::Write {
                ops: vec![Op::Trash {
                    folder: String::new(),
                    file: "Draft.md".into()
                }],
                revision: notes.revision,
            })
        );
    }

    #[test]
    fn a_title_changed_during_a_write_renames_the_file_that_write_made() {
        let mut notes = loaded(&[], Vec::new());
        let _ = notes.update(Message::New(at(10)));
        type_text(&mut notes, "Draft", 11);
        let (_, first) = write(&mut notes);
        type_text(&mut notes, "Final", 12);
        assert_eq!(notes.update(Message::Save), None);
        let Some(Request::Write { ops, .. }) = notes.update(saved(first, 1)) else {
            panic!("the waiting write follows the first");
        };
        assert_eq!(ops, [put("", "Final.md", "Final", Some("Draft.md"))]);
    }

    #[test]
    fn a_write_that_fails_halfway_writes_again_only_what_failed() {
        let mut notes = loaded(
            &[],
            vec![stored("", "a.md", "A", 20), stored("", "b.md", "B", 10)],
        );
        type_text(&mut notes, "A2", 30);
        let _ = notes.update(Message::Select(Some(1)));
        type_text(&mut notes, "B2", 31);
        let (ops, revision) = write(&mut notes);
        assert_eq!(ops.len(), 2);
        let _ = notes.update(Message::Saved {
            revision,
            results: vec![Ok(()), Err(Failure::Io("disk full".into()))],
        });
        assert_eq!(
            notes.status(),
            Status::Failed(Problem::Save(Failure::Io("disk full".into())))
        );
        assert!(!notes.can_close());
        let (ops, revision) = write(&mut notes);
        assert_eq!(ops, [put("", "B2.md", "B2", Some("b.md"))]);
        let _ = notes.update(saved(revision, 1));
        assert_eq!(notes.status(), Status::Saved);
    }

    #[test]
    fn a_write_overtaken_by_a_change_leaves_the_change_unsaved() {
        let mut notes = loaded(&[], Vec::new());
        let _ = notes.update(Message::New(at(10)));
        let (_, revision) = write(&mut notes);
        // Typing goes on while the write runs.
        type_text(&mut notes, "x", 11);
        assert_eq!(notes.update(saved(revision, 1)), None);
        assert_eq!(notes.status(), Status::Unsaved);
        // The change's own pause ends, and its write includes it.
        let (ops, revision) = write(&mut notes);
        assert_eq!(ops, [put("", "x.md", "x", Some("Untitled.md"))]);
        let _ = notes.update(saved(revision, 1));
        assert_eq!(notes.status(), Status::Saved);
    }

    #[test]
    fn closing_writes_what_is_not_written_yet_and_then_closes() {
        let mut notes = loaded(&[], Vec::new());
        assert!(notes.can_close());
        let _ = notes.update(Message::New(at(10)));
        assert!(!notes.can_close());
        // The pause has not ended: closing writes now instead.
        let Some(Request::Write { revision, .. }) = notes.update(Message::Close) else {
            panic!("closing writes first");
        };
        assert!(!notes.should_close());
        assert_eq!(notes.update(saved(revision, 1)), None);
        assert!(notes.should_close());
    }

    #[test]
    fn closing_during_a_write_writes_the_rest_after_it() {
        let mut notes = loaded(&[], Vec::new());
        let _ = notes.update(Message::New(at(10)));
        let (_, first) = write(&mut notes);
        type_text(&mut notes, "x", 11);
        assert_eq!(notes.update(Message::Close), None);
        let Some(Request::Write { ops, revision }) = notes.update(saved(first, 1)) else {
            panic!("the change made during the first write is written before closing");
        };
        assert_eq!(ops, [put("", "x.md", "x", Some("Untitled.md"))]);
        assert!(!notes.should_close());
        // The pause of the change ends meanwhile: the write already covers it.
        assert_eq!(notes.update(Message::Save), None);
        assert_eq!(notes.update(saved(revision, 1)), None);
        assert!(notes.should_close());
    }

    #[test]
    fn a_failed_write_on_closing_keeps_the_window_until_asked_again() {
        let mut notes = loaded(&[], Vec::new());
        let _ = notes.update(Message::New(at(10)));
        let Some(Request::Write { revision, .. }) = notes.update(Message::Close) else {
            panic!("closing writes first");
        };
        let _ = notes.update(Message::Saved {
            revision,
            results: vec![Err(Failure::Io("disk full".into()))],
        });
        // The window stays, showing why.
        assert!(!notes.should_close());
        assert_eq!(
            notes.status(),
            Status::Failed(Problem::Save(Failure::Io("disk full".into())))
        );
        // Asked again, it closes without the change.
        assert!(notes.can_close());
        assert_eq!(notes.update(Message::Close), None);
        assert!(notes.should_close());
    }

    #[test]
    fn a_folder_that_could_not_be_read_is_not_written() {
        let mut notes = Notes::new();
        let _ = notes.update(Message::Started(at(0)));
        assert_eq!(notes.status(), Status::Loading);
        let _ = notes.update(Message::Loaded(Err("denied".into())));
        assert_eq!(
            notes.status(),
            Status::Failed(Problem::Read("denied".into()))
        );
        assert!(!notes.is_loaded());
        assert_eq!(notes.update(Message::New(at(1))), None);
        assert_eq!(notes.update(Message::Save), None);
        // Nothing to lose either, so the window closes.
        assert!(notes.can_close());
    }

    #[test]
    fn ages_are_rounded_down_to_the_largest_unit() {
        let age = |seconds| age(at(seconds), at(0));
        assert_eq!(age(59), Age::JustNow);
        assert_eq!(age(60), Age::Minutes(1));
        assert_eq!(age(3_599), Age::Minutes(59));
        assert_eq!(age(7_200), Age::Hours(2));
        assert_eq!(age(90_000), Age::Yesterday);
        assert_eq!(age(3 * 86_400), Age::Days(3));
        assert_eq!(age(14 * 86_400), Age::Weeks(2));
        assert_eq!(age(400 * 86_400), Age::Years(1));
        // A file from the future (another machine's clock) is just now.
        assert_eq!(super::age(at(0), at(100)), Age::JustNow);
    }
}
