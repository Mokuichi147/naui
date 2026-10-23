//! Value types for opening and saving files.
//!
//! The dialogs themselves are the OS's native ones, shown by each backend
//! (NSOpenPanel / NSSavePanel / IFileOpenDialog / IFileSaveDialog /
//! `<input type="file">` / `showSaveFilePicker`). All that is here are
//! platform-independent types describing "what to pick", "what was picked" and
//! "where to write".

use std::path::{Path, PathBuf};

/// What to let the user pick.
///
/// Only options that mean the same on all four platforms. "Multiple folders" and
/// "files and folders mixed" are available in neither the Windows dialog nor
/// `<input type="file">`, so they are not included.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FilePickerMode {
    /// Pick exactly one file (the default).
    #[default]
    File,
    /// Pick multiple files.
    Files,
    /// Pick one folder.
    Folder,
}

impl FilePickerMode {
    /// Whether multiple items can be picked.
    pub fn allows_multiple(self) -> bool {
        matches!(self, FilePickerMode::Files)
    }

    /// Whether this mode picks a folder.
    pub fn is_folder(self) -> bool {
        matches!(self, FilePickerMode::Folder)
    }
}

/// A filter by file extension.
///
/// Extensions are normalized to the form `png` (writing `.png` or `*.png` gives the
/// same result). Backends build the notation each platform requires.
///
/// ```
/// # use naui_core::FileFilter;
/// let filter = FileFilter::new("Images", ["*.PNG", ".jpg"]);
/// assert_eq!(filter.extensions(), ["png", "jpg"]);
/// assert_eq!(filter.glob_pattern(), "*.png;*.jpg");
/// assert_eq!(filter.accept_list(), ".png,.jpg");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FileFilter {
    label: String,
    extensions: Vec<String>,
}

impl FileFilter {
    /// Creates one from a display name and a list of extensions. Empty entries and
    /// entries that are just `*` are dropped.
    pub fn new<I, S>(label: impl Into<String>, extensions: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        Self {
            label: label.into(),
            extensions: extensions
                .into_iter()
                .filter_map(|e| normalize_extension(e.as_ref()))
                .collect(),
        }
    }

    /// The display name shown in the dialog's file type field.
    pub fn label(&self) -> &str {
        &self.label
    }

    /// The normalized extensions (in the form `png`).
    pub fn extensions(&self) -> &[String] {
        &self.extensions
    }

    pub fn is_empty(&self) -> bool {
        self.extensions.is_empty()
    }

    /// The form `*.png;*.jpg`, as Windows's `COMDLG_FILTERSPEC` requires.
    pub fn glob_pattern(&self) -> String {
        self.extensions
            .iter()
            .map(|e| format!("*.{e}"))
            .collect::<Vec<_>>()
            .join(";")
    }

    /// The form `.png,.jpg`, as the Web's `accept` attribute requires.
    pub fn accept_list(&self) -> String {
        self.extensions
            .iter()
            .map(|e| format!(".{e}"))
            .collect::<Vec<_>>()
            .join(",")
    }
}

/// Combines a list of filters into a single `accept` attribute for
/// `<input type="file">`.
///
/// Browsers have no "file type" field; the only option is to list the accepted
/// extensions in one attribute, so all filters are concatenated.
///
/// ```
/// # use naui_core::{accept_attribute, FileFilter};
/// let filters = [
///     FileFilter::new("Images", ["png", "jpg"]),
///     FileFilter::new("Documents", ["txt"]),
/// ];
/// assert_eq!(accept_attribute(&filters), ".png,.jpg,.txt");
/// ```
pub fn accept_attribute(filters: &[FileFilter]) -> String {
    filters
        .iter()
        .filter(|f| !f.is_empty())
        .map(|f| f.accept_list())
        .collect::<Vec<_>>()
        .join(",")
}

/// One picked file or folder.
///
/// On platforms that have paths (macOS / Windows / Linux) `path` holds an absolute
/// path. **On the Web the browser does not hand out paths, so `path` is always
/// `None`.**
///
/// Use [`source`](Self::source) instead to get, on every platform, "a location you
/// can pass straight to [`Ui::image`] / [`Ui::video`] / [`Ui::audio`]". Natively
/// this is the absolute path; on the Web it is a `blob:` URL created by the
/// browser.
///
/// [`Ui::image`]: https://docs.rs/naui/latest/naui/struct.Ui.html#method.image
/// [`Ui::video`]: https://docs.rs/naui/latest/naui/struct.Ui.html#method.video
/// [`Ui::audio`]: https://docs.rs/naui/latest/naui/struct.Ui.html#method.audio
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FileEntry {
    name: String,
    path: Option<PathBuf>,
    /// A location that can be passed to media and images. A path natively, a blob URL
    /// on the Web.
    source: Option<String>,
}

impl FileEntry {
    /// Creates one from a path. The display name is taken from the end of the path.
    pub fn from_path(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.to_string_lossy().into_owned());
        Self {
            source: path.to_str().map(str::to_string),
            name,
            path: Some(path),
        }
    }

    /// Creates one from just a display name (when neither a path nor contents are
    /// passed).
    pub fn from_name(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            path: None,
            source: None,
        }
    }

    /// Creates one from a display name and a location pointing at the contents.
    ///
    /// Used by the Web backend to pass `blob:` URLs created with
    /// `URL.createObjectURL`. It has no path.
    pub fn from_name_and_source(name: impl Into<String>, source: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            path: None,
            source: Some(source.into()),
        }
    }

    /// A name that can be shown on screen. On platforms with paths, the last component.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The absolute path. Always `None` on the Web.
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// A location you can pass straight to `ui.image` / `ui.video` / `ui.audio`.
    ///
    /// Natively the absolute path; on the Web a `blob:` URL created by the browser.
    /// `None` when a folder was picked or the location could not be obtained.
    ///
    /// **A `blob:` URL on the Web stays valid until the next pick with the same
    /// [`FilePicker`].** Picking again discards the previous URLs.
    ///
    /// [`FilePicker`]: https://docs.rs/naui/latest/naui/struct.FilePicker.html
    pub fn source(&self) -> Option<&str> {
        self.source.as_deref()
    }
}

/// Takes one default extension from a list of filters.
///
/// A save dialog writes with a single "file type", so the default is the first
/// extension of the first usable filter. Used for Windows's `SetDefaultExtension`
/// and when adding an extension to a name.
///
/// ```
/// # use naui_core::{default_extension, FileFilter};
/// let filters = [
///     FileFilter::new("Empty", [] as [&str; 0]),
///     FileFilter::new("Images", ["png", "jpg"]),
/// ];
/// assert_eq!(default_extension(&filters), Some("png"));
/// assert_eq!(default_extension(&[]), None);
/// ```
pub fn default_extension(filters: &[FileFilter]) -> Option<&str> {
    filters
        .iter()
        .find_map(|filter| filter.extensions().first())
        .map(String::as_str)
}

/// Adds a filter's extension to a default file name.
///
/// If the name already ends with the extension of any filter it is returned as is.
/// Otherwise [`default_extension`] is appended. Nothing is added to an empty name
/// (the dialog's own default is used).
///
/// This exists for places that "don't add the extension for you", such as GTK's
/// `set_initial_name` and browser download names.
///
/// ```
/// # use naui_core::{with_default_extension, FileFilter};
/// let filters = [FileFilter::new("Images", ["png", "jpg"])];
/// assert_eq!(with_default_extension("notes", &filters), "notes.png");
/// assert_eq!(with_default_extension("photo.JPG", &filters), "photo.JPG");
/// assert_eq!(with_default_extension("notes", &[]), "notes");
/// assert_eq!(with_default_extension("", &filters), "");
/// ```
pub fn with_default_extension(name: &str, filters: &[FileFilter]) -> String {
    let name = name.trim();
    if name.is_empty() {
        return String::new();
    }
    let Some(default) = default_extension(filters) else {
        return name.to_string();
    };
    let has_known_extension = filters
        .iter()
        .flat_map(|filter| filter.extensions())
        .any(|extension| ends_with_extension(name, extension));
    if has_known_extension {
        return name.to_string();
    }
    format!("{name}.{default}")
}

/// Whether `name` ends with `.<extension>` (case-insensitive).
fn ends_with_extension(name: &str, extension: &str) -> bool {
    let suffix = format!(".{extension}");
    let Some(head) = name.len().checked_sub(suffix.len()) else {
        return false;
    };
    // 拡張子だけの名前 (`.png`) は拡張子付きとみなさない。
    if head == 0 {
        return false;
    }
    // 日本語の名前では区切りが文字の途中に来ることがある。
    name.is_char_boundary(head) && name[head..].eq_ignore_ascii_case(&suffix)
}

/// Normalizes `*.PNG` and `.png` to `png`. Drops it if nothing is left.
fn normalize_extension(raw: &str) -> Option<String> {
    let trimmed = raw
        .trim()
        .trim_start_matches('*')
        .trim_start_matches('.')
        .trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_source_is_the_path_on_native() {
        let entry = FileEntry::from_path("/tmp/写真 1.png");
        assert_eq!(entry.name(), "写真 1.png");
        assert_eq!(
            entry.path().map(|p| p.to_str().unwrap()),
            Some("/tmp/写真 1.png")
        );
        // そのまま ui.image / ui.video へ渡せる場所。
        assert_eq!(entry.source(), Some("/tmp/写真 1.png"));
    }

    #[test]
    fn entry_from_name_has_no_source() {
        // フォルダーのように、中身を指す場所を持たない場合。
        let entry = FileEntry::from_name("写真");
        assert_eq!(entry.name(), "写真");
        assert_eq!(entry.path(), None);
        assert_eq!(entry.source(), None);
    }

    #[test]
    fn entry_can_carry_a_source_without_a_path() {
        // Web はパスを渡せないので、blob URL だけを持たせる。
        let entry = FileEntry::from_name_and_source("clip.mp4", "blob:http://localhost/abc");
        assert_eq!(entry.name(), "clip.mp4");
        assert_eq!(entry.path(), None);
        assert_eq!(entry.source(), Some("blob:http://localhost/abc"));
    }

    #[test]
    fn mode_defaults_to_a_single_file() {
        assert_eq!(FilePickerMode::default(), FilePickerMode::File);
        assert!(!FilePickerMode::File.allows_multiple());
        assert!(FilePickerMode::Files.allows_multiple());
        assert!(FilePickerMode::Folder.is_folder());
        assert!(!FilePickerMode::Folder.allows_multiple());
    }

    #[test]
    fn extensions_are_normalized() {
        let filter = FileFilter::new("画像", ["*.PNG", ".jpg", "gif", " ", "*"]);
        assert_eq!(filter.label(), "画像");
        assert_eq!(filter.extensions(), ["png", "jpg", "gif"]);
        assert!(!filter.is_empty());
        assert!(FileFilter::new("空", [".", ""]).is_empty());
    }

    #[test]
    fn filters_render_per_platform_forms() {
        let filter = FileFilter::new("画像", ["png", "jpg"]);
        assert_eq!(filter.glob_pattern(), "*.png;*.jpg");
        assert_eq!(filter.accept_list(), ".png,.jpg");
        assert_eq!(FileFilter::new("空", [] as [&str; 0]).glob_pattern(), "");
    }

    #[test]
    fn accept_attribute_joins_every_filter() {
        let filters = [
            FileFilter::new("画像", ["png"]),
            FileFilter::new("空", [] as [&str; 0]),
            FileFilter::new("文書", ["txt", "md"]),
        ];
        assert_eq!(accept_attribute(&filters), ".png,.txt,.md");
        assert_eq!(accept_attribute(&[]), "");
    }

    #[test]
    fn default_extension_is_the_first_usable_one() {
        let filters = [
            FileFilter::new("空", [] as [&str; 0]),
            FileFilter::new("画像", ["PNG", "jpg"]),
        ];
        assert_eq!(default_extension(&filters), Some("png"));
        assert_eq!(default_extension(&[]), None);
        assert_eq!(default_extension(&[FileFilter::new("空", ["*"])]), None);
    }

    #[test]
    fn default_extension_is_added_only_when_missing() {
        let filters = [FileFilter::new("画像", ["png", "jpg"])];
        assert_eq!(with_default_extension("メモ", &filters), "メモ.png");
        // 絞り込みのどれかに合っていれば足さない (大文字小文字は無視)。
        assert_eq!(with_default_extension("写真.JPG", &filters), "写真.JPG");
        assert_eq!(with_default_extension("写真.png", &filters), "写真.png");
        // 知らない拡張子は名前の一部として扱う。
        assert_eq!(with_default_extension("写真.bmp", &filters), "写真.bmp.png");
        // 拡張子だけの名前は隠しファイル名なので、拡張子付きとはみなさない。
        assert_eq!(with_default_extension(".png", &filters), ".png.png");
    }

    #[test]
    fn default_extension_leaves_the_name_alone_without_filters() {
        assert_eq!(with_default_extension("メモ", &[]), "メモ");
        // 名前が空ならダイアログ側の既定に任せる。
        assert_eq!(
            with_default_extension("  ", &[FileFilter::new("画像", ["png"])]),
            ""
        );
        // 前後の空白は落とす。
        assert_eq!(
            with_default_extension(" メモ ", &[FileFilter::new("文書", ["txt"])]),
            "メモ.txt"
        );
    }

    #[test]
    fn extension_check_survives_multibyte_names() {
        // 区切りが文字の途中に来ても落ちないこと。
        let filters = [FileFilter::new("画像", ["p"])];
        assert_eq!(with_default_extension("あ", &filters), "あ.p");
        assert_eq!(with_default_extension("aあ", &filters), "aあ.p");
        assert_eq!(with_default_extension("あ.p", &filters), "あ.p");
    }

    #[test]
    fn entry_takes_its_name_from_the_path() {
        let entry = FileEntry::from_path("/tmp/写真/a.png");
        assert_eq!(entry.name(), "a.png");
        assert_eq!(entry.path(), Some(Path::new("/tmp/写真/a.png")));
    }

    #[test]
    fn entry_without_a_path_keeps_only_the_name() {
        let entry = FileEntry::from_name("a.png");
        assert_eq!(entry.name(), "a.png");
        assert_eq!(entry.path(), None);
    }
}
