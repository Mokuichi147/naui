//! Reading and writing the notes folder: a service.
//!
//! Like the core, it does not depend on naui. It is called from other threads (the
//! app runs it with `Effect::thread`), so it takes plain values and returns plain
//! results, and knows nothing of messages or effects.
//!
//! The folder holds one Markdown file per note, and its subfolders are the folders of
//! the app. The core decides every name; this crate only refuses to write over a file
//! that is not the note's own, and moves deleted notes to [`TRASH`] instead of
//! removing them.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use notes_core::{Failure, Library, Op, StoredNote};

/// The folder, inside the notes folder, that deleted notes are moved to. Its name
/// starts with a dot, so it is not shown as a folder of notes.
pub const TRASH: &str = ".trash";

/// A notes folder.
#[derive(Clone, Debug)]
pub struct Store {
    root: PathBuf,
}

impl Store {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Reads the notes: the `.md` files in the folder and in its subfolders. A folder
    /// that does not exist yet holds none. A file that cannot be read is skipped, and
    /// listed in [`Library::skipped`].
    pub fn load(&self) -> Result<Library, String> {
        let mut library = Library::default();
        let top = match entries(&self.root) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(library),
            Err(error) => return Err(self.problem(&self.root, error)),
        };
        let mut folders = Vec::new();
        for (name, path, is_dir) in top {
            if is_dir {
                if !name.starts_with('.') {
                    folders.push(name);
                }
            } else {
                read_note(&mut library, "", &name, &path);
            }
        }
        folders.sort_by_key(|name| name.to_lowercase());
        for folder in &folders {
            let path = self.root.join(folder);
            let inside = entries(&path).map_err(|error| self.problem(&path, error))?;
            for (name, path, is_dir) in inside {
                if !is_dir {
                    read_note(&mut library, folder, &name, &path);
                }
            }
        }
        library.folders = folders;
        Ok(library)
    }

    /// Makes the changes in order, and returns a result for each. A change that fails
    /// does not stop the others.
    pub fn apply(&self, ops: &[Op]) -> Vec<Result<(), Failure>> {
        ops.iter().map(|op| self.apply_one(op)).collect()
    }

    fn apply_one(&self, op: &Op) -> Result<(), Failure> {
        match op {
            Op::Put {
                folder,
                file,
                text,
                replaces,
            } => {
                let dir = self.root.join(folder);
                let target = dir.join(file);
                // The note's own file is written over. Any other name must be free;
                // on macOS and Windows that includes the same name in another case.
                let own = replaces
                    .as_ref()
                    .is_some_and(|(old_folder, old)| old_folder == folder && old == file);
                if !own && target.exists() {
                    let name = target.strip_prefix(&self.root).unwrap_or(&target);
                    return Err(Failure::NameTaken(name.display().to_string()));
                }
                fs::create_dir_all(&dir).map_err(|error| self.failure(&dir, error))?;
                // Written next to it first, then put in its place, so a write cut short
                // leaves the previous text whole.
                let partial = dir.join(format!(".{file}.partial"));
                fs::write(&partial, text).map_err(|error| self.failure(&target, error))?;
                fs::rename(&partial, &target).map_err(|error| self.failure(&target, error))?;
                if let Some((old_folder, old)) = replaces.as_ref().filter(|_| !own) {
                    let old = self.root.join(old_folder).join(old);
                    match fs::remove_file(&old) {
                        Err(error) if error.kind() != io::ErrorKind::NotFound => {
                            return Err(self.failure(&old, error));
                        }
                        _ => {}
                    }
                }
                Ok(())
            }
            Op::MakeFolder { folder } => {
                let dir = self.root.join(folder);
                fs::create_dir_all(&dir).map_err(|error| self.failure(&dir, error))
            }
            Op::RenameFolder { from, to } => {
                let old = self.root.join(from);
                let new = self.root.join(to);
                // Another folder or file already called `to` is never replaced. On macOS
                // and Windows a change of case finds the folder itself, which is fine.
                let same = from.to_lowercase() == to.to_lowercase();
                if !same && new.exists() {
                    return Err(Failure::NameTaken(to.clone()));
                }
                if old.exists() {
                    fs::rename(&old, &new).map_err(|error| self.failure(&old, error))
                } else {
                    // Made in the app and not written yet.
                    fs::create_dir_all(&new).map_err(|error| self.failure(&new, error))
                }
            }
            Op::TrashFolder { folder } => {
                let dir = self.root.join(folder);
                let empty = match fs::read_dir(&dir) {
                    Ok(mut entries) => entries.next().is_none(),
                    Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
                    Err(error) => return Err(self.failure(&dir, error)),
                };
                if empty {
                    return fs::remove_dir(&dir).map_err(|error| self.failure(&dir, error));
                }
                let trash = self.root.join(TRASH);
                fs::create_dir_all(&trash).map_err(|error| self.failure(&trash, error))?;
                let to = free_name(&trash, folder);
                fs::rename(&dir, &to).map_err(|error| self.failure(&dir, error))
            }
            Op::Trash { folder, file } => {
                let from = self.root.join(folder).join(file);
                let trash = self.root.join(TRASH);
                fs::create_dir_all(&trash).map_err(|error| self.failure(&trash, error))?;
                let to = free_name(&trash, file);
                match fs::rename(&from, &to) {
                    Err(error) if error.kind() != io::ErrorKind::NotFound => {
                        Err(self.failure(&from, error))
                    }
                    _ => Ok(()),
                }
            }
        }
    }

    fn failure(&self, path: &Path, error: io::Error) -> Failure {
        Failure::Io(self.problem(path, error))
    }

    /// What went wrong, naming the file from the notes folder down.
    fn problem(&self, path: &Path, error: io::Error) -> String {
        let path = path.strip_prefix(&self.root).unwrap_or(path);
        format!("{}: {error}", path.display())
    }
}

/// The entries of a folder: name, path and whether it is a folder.
fn entries(dir: &Path) -> io::Result<Vec<(String, PathBuf, bool)>> {
    let mut entries = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        let is_dir = entry.file_type().is_ok_and(|kind| kind.is_dir());
        entries.push((name, entry.path(), is_dir));
    }
    Ok(entries)
}

fn read_note(library: &mut Library, folder: &str, name: &str, path: &Path) {
    library.taken.push((folder.to_owned(), name.to_owned()));
    if name.starts_with('.') || !name.to_lowercase().ends_with(".md") {
        return;
    }
    let read = fs::read_to_string(path).and_then(|text| {
        let modified = fs::metadata(path)?.modified()?;
        Ok((text, modified))
    });
    match read {
        Ok((text, modified)) => library.notes.push(StoredNote {
            folder: folder.to_owned(),
            file: name.to_owned(),
            text,
            modified,
        }),
        Err(_) => library.skipped.push(match folder {
            "" => name.to_owned(),
            folder => format!("{folder}/{name}"),
        }),
    }
}

/// `name` in `dir`, or `name 2`, `name 3`… if it is taken.
fn free_name(dir: &Path, name: &str) -> PathBuf {
    let (stem, extension) = name.rsplit_once('.').unwrap_or((name, ""));
    (1..)
        .map(|n| match (n, extension) {
            (1, _) => dir.join(name),
            (n, "") => dir.join(format!("{stem} {n}")),
            (n, extension) => dir.join(format!("{stem} {n}.{extension}")),
        })
        .find(|path| !path.exists())
        .expect("some number is free")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An empty folder of its own for each test.
    struct Folder(PathBuf);

    impl Folder {
        fn new(name: &str) -> Self {
            let path =
                std::env::temp_dir().join(format!("notes-store-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn write(&self, path: &str, contents: &[u8]) {
            let path = self.0.join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, contents).unwrap();
        }

        fn read(&self, path: &str) -> Option<String> {
            fs::read_to_string(self.0.join(path)).ok()
        }

        fn store(&self) -> Store {
            Store::new(&self.0)
        }
    }

    impl Drop for Folder {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn put(folder: &str, file: &str, text: &str, replaces: Option<&str>) -> Op {
        Op::Put {
            folder: folder.to_owned(),
            file: file.to_owned(),
            text: text.to_owned(),
            replaces: replaces.map(|old| (folder.to_owned(), old.to_owned())),
        }
    }

    #[test]
    fn a_folder_that_does_not_exist_holds_no_notes() {
        let store = Store::new(std::env::temp_dir().join("notes-store-missing-folder"));
        assert_eq!(store.load().unwrap(), Library::default());
    }

    #[test]
    fn notes_are_read_from_the_folder_and_its_subfolders() {
        let folder = Folder::new("load");
        folder.write("Groceries.md", b"Groceries\nmilk");
        folder.write("photo.png", b"not a note");
        folder.write("Work/Plan.md", b"Plan");
        folder.write("archive/Old.MD", b"Old");
        folder.write(".trash/Gone.md", b"Gone");
        folder.write(".hidden.md", b"hidden");
        folder.write("Broken.md", &[0xff, 0xfe]);
        let mut library = folder.store().load().unwrap();
        library.notes.sort_by(|a, b| a.file.cmp(&b.file));
        library.taken.sort();
        assert_eq!(library.folders, ["archive", "Work"]);
        let notes: Vec<(&str, &str, &str)> = library
            .notes
            .iter()
            .map(|note| (note.folder.as_str(), note.file.as_str(), note.text.as_str()))
            .collect();
        assert_eq!(
            notes,
            [
                ("", "Groceries.md", "Groceries\nmilk"),
                ("archive", "Old.MD", "Old"),
                ("Work", "Plan.md", "Plan"),
            ]
        );
        // A file that is not UTF-8 is skipped, and its name stays taken.
        assert_eq!(library.skipped, ["Broken.md"]);
        assert!(library.taken.contains(&(String::new(), "Broken.md".into())));
        assert!(library.taken.contains(&(String::new(), "photo.png".into())));
    }

    #[test]
    fn a_note_is_written_renamed_and_written_over() {
        let folder = Folder::new("put");
        let store = folder.store();
        assert_eq!(
            store.apply(&[put("Work", "Draft.md", "Draft", None)]),
            [Ok(())]
        );
        assert_eq!(folder.read("Work/Draft.md").as_deref(), Some("Draft"));
        let renamed = put("Work", "Final.md", "Final", Some("Draft.md"));
        assert_eq!(store.apply(&[renamed]), [Ok(())]);
        assert_eq!(folder.read("Work/Draft.md"), None);
        let over = put("Work", "Final.md", "Final\nmore", Some("Final.md"));
        assert_eq!(store.apply(&[over]), [Ok(())]);
        assert_eq!(folder.read("Work/Final.md").as_deref(), Some("Final\nmore"));
        // Nothing is left behind.
        let left: Vec<_> = fs::read_dir(folder.0.join("Work")).unwrap().collect();
        assert_eq!(left.len(), 1);
    }

    #[test]
    fn another_file_is_never_written_over() {
        let folder = Folder::new("refuse");
        folder.write("Plan.md", b"someone else's");
        folder.write("Draft.md", b"Plan");
        let results = folder.store().apply(&[
            put("", "Plan.md", "Plan", Some("Draft.md")),
            put("", "New.md", "New", None),
        ]);
        assert_eq!(results[0], Err(Failure::NameTaken("Plan.md".into())));
        // The other change still happens.
        assert_eq!(results[1], Ok(()));
        assert_eq!(folder.read("Plan.md").as_deref(), Some("someone else's"));
        assert_eq!(folder.read("Draft.md").as_deref(), Some("Plan"));
    }

    #[test]
    fn a_folder_is_made_and_a_note_moved_into_it() {
        let folder = Folder::new("move");
        folder.write("Plan.md", b"Plan");
        let ops = [
            Op::MakeFolder {
                folder: "Work".into(),
            },
            Op::Put {
                folder: "Work".into(),
                file: "Plan.md".into(),
                text: "Plan".into(),
                replaces: Some((String::new(), "Plan.md".into())),
            },
        ];
        assert_eq!(folder.store().apply(&ops), [Ok(()), Ok(())]);
        assert_eq!(folder.read("Plan.md"), None);
        assert_eq!(folder.read("Work/Plan.md").as_deref(), Some("Plan"));
        assert_eq!(folder.store().load().unwrap().folders, ["Work"]);
    }

    #[test]
    fn folders_are_renamed_and_trashed_with_their_notes() {
        let folder = Folder::new("folders");
        folder.write("Work/Plan.md", b"Plan");
        folder.write("Home/x.md", b"x");
        folder.write(".trash/Work", b"a file deleted before");
        let rename = |from: &str, to: &str| Op::RenameFolder {
            from: from.into(),
            to: to.into(),
        };
        let store = folder.store();
        assert_eq!(store.apply(&[rename("Work", "Jobs")]), [Ok(())]);
        assert_eq!(folder.read("Jobs/Plan.md").as_deref(), Some("Plan"));
        // Never onto another folder.
        assert_eq!(
            store.apply(&[rename("Jobs", "Home")]),
            [Err(Failure::NameTaken("Home".into()))]
        );
        // A folder made in the app and not on disk yet is made under its new name.
        assert_eq!(store.apply(&[rename("New", "Travel")]), [Ok(())]);
        let trash = |folder: &str| Op::TrashFolder {
            folder: folder.into(),
        };
        assert_eq!(
            store.apply(&[trash("Jobs"), trash("Travel")]),
            [Ok(()), Ok(())]
        );
        // The folder with a note is in the trash; the empty one is just removed.
        assert_eq!(folder.read(".trash/Jobs/Plan.md").as_deref(), Some("Plan"));
        assert!(!folder.0.join("Travel").exists());
        assert!(!folder.0.join(".trash/Travel").exists());
        assert_eq!(store.load().unwrap().folders, ["Home"]);
    }

    #[test]
    fn deleted_notes_go_to_the_trash() {
        let folder = Folder::new("trash");
        folder.write("Work/Plan.md", b"one");
        folder.write(".trash/Plan.md", b"deleted before");
        let trash = Op::Trash {
            folder: "Work".into(),
            file: "Plan.md".into(),
        };
        assert_eq!(folder.store().apply(&[trash]), [Ok(())]);
        assert_eq!(folder.read("Work/Plan.md"), None);
        assert_eq!(
            folder.read(".trash/Plan.md").as_deref(),
            Some("deleted before")
        );
        assert_eq!(folder.read(".trash/Plan 2.md").as_deref(), Some("one"));
        assert!(folder.store().load().unwrap().notes.is_empty());
    }
}
