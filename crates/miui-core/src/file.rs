//! ファイルとフォルダーの選択にかかわる値型。
//!
//! ダイアログそのものは各バックエンドが OS のネイティブなものを出す
//! (NSOpenPanel / IFileOpenDialog / `<input type="file">`)。ここにあるのは
//! 「何を選ばせるか」「何が選ばれたか」を表す、環境に依存しない型だけ。

use std::path::{Path, PathBuf};

/// 何を選ばせるか。
///
/// 4 環境すべてで同じ意味になるものだけを持つ。「フォルダーを複数」や
/// 「ファイルとフォルダーを混ぜて」は Windows のダイアログにも
/// `<input type="file">` にも無いため、含めていない。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FilePickerMode {
    /// ファイルを 1 つだけ選ぶ (既定)。
    #[default]
    File,
    /// ファイルを複数選ぶ。
    Files,
    /// フォルダーを 1 つ選ぶ。
    Folder,
}

impl FilePickerMode {
    /// 複数選べるかどうか。
    pub fn allows_multiple(self) -> bool {
        matches!(self, FilePickerMode::Files)
    }

    /// フォルダーを選ぶモードかどうか。
    pub fn is_folder(self) -> bool {
        matches!(self, FilePickerMode::Folder)
    }
}

/// 拡張子による絞り込み。
///
/// 拡張子は `png` の形に正規化して保持する (`.png` や `*.png` と書いても同じ)。
/// 各環境が要求する書き方はバックエンドが組み立てる。
///
/// ```
/// # use miui_core::FileFilter;
/// let filter = FileFilter::new("画像", ["*.PNG", ".jpg"]);
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
    /// 表示名と拡張子の並びから作る。空文字や `*` だけの要素は捨てる。
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

    /// ダイアログの種類欄に出る表示名。
    pub fn label(&self) -> &str {
        &self.label
    }

    /// 正規化済みの拡張子 (`png` の形)。
    pub fn extensions(&self) -> &[String] {
        &self.extensions
    }

    pub fn is_empty(&self) -> bool {
        self.extensions.is_empty()
    }

    /// `*.png;*.jpg` の形。Windows の `COMDLG_FILTERSPEC` が要求する書き方。
    pub fn glob_pattern(&self) -> String {
        self.extensions
            .iter()
            .map(|e| format!("*.{e}"))
            .collect::<Vec<_>>()
            .join(";")
    }

    /// `.png,.jpg` の形。Web の `accept` 属性が要求する書き方。
    pub fn accept_list(&self) -> String {
        self.extensions
            .iter()
            .map(|e| format!(".{e}"))
            .collect::<Vec<_>>()
            .join(",")
    }
}

/// 絞り込みの並びを、`<input type="file">` の `accept` 属性ひとつにまとめる。
///
/// ブラウザには「種類を選ぶ欄」が無く、受け付ける拡張子を 1 つの属性に
/// 並べる形しか無いため、すべての絞り込みを連結する。
///
/// ```
/// # use miui_core::{accept_attribute, FileFilter};
/// let filters = [
///     FileFilter::new("画像", ["png", "jpg"]),
///     FileFilter::new("文書", ["txt"]),
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

/// 選ばれたファイル、またはフォルダー 1 つ。
///
/// `path` がある環境 (macOS / Windows / Linux) では絶対パスが入る。
/// **Web ではブラウザがパスを渡さないため、`path` は常に `None`**。
///
/// 代わりに [`source`](Self::source) を使うと、どの環境でも
/// 「そのまま [`Ui::image`] / [`Ui::video`] / [`Ui::audio`] へ渡せる場所」が
/// 得られる。ネイティブでは絶対パス、Web ではブラウザが作る
/// `blob:` URL になる。
///
/// [`Ui::image`]: https://docs.rs/miui/latest/miui/struct.Ui.html#method.image
/// [`Ui::video`]: https://docs.rs/miui/latest/miui/struct.Ui.html#method.video
/// [`Ui::audio`]: https://docs.rs/miui/latest/miui/struct.Ui.html#method.audio
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FileEntry {
    name: String,
    path: Option<PathBuf>,
    /// メディアや画像に渡せる場所。ネイティブはパス、Web は blob URL。
    source: Option<String>,
}

impl FileEntry {
    /// パスから作る。表示名はパスの末尾から取る。
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

    /// 表示名だけから作る (パスも中身も渡さない場合)。
    pub fn from_name(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            path: None,
            source: None,
        }
    }

    /// 表示名と、中身を指す場所から作る。
    ///
    /// Web バックエンドが `URL.createObjectURL` で作った `blob:` URL を
    /// 渡すために使う。パスは持たない。
    pub fn from_name_and_source(name: impl Into<String>, source: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            path: None,
            source: Some(source.into()),
        }
    }

    /// 画面に出せる名前。パスがある環境では末尾の要素。
    pub fn name(&self) -> &str {
        &self.name
    }

    /// 絶対パス。Web では常に `None`。
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// そのまま `ui.image` / `ui.video` / `ui.audio` へ渡せる場所。
    ///
    /// ネイティブでは絶対パス、Web ではブラウザが作る `blob:` URL。
    /// フォルダーを選んだときや、場所を取れなかったときは `None`。
    ///
    /// **Web の `blob:` URL は、同じ [`FilePicker`] で次に選び直すまで
    /// 有効**。選び直すと以前の URL は破棄される。
    ///
    /// [`FilePicker`]: https://docs.rs/miui/latest/miui/struct.FilePicker.html
    pub fn source(&self) -> Option<&str> {
        self.source.as_deref()
    }
}

/// `*.PNG` や `.png` を `png` にそろえる。中身が無ければ捨てる。
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
        assert_eq!(entry.path().map(|p| p.to_str().unwrap()), Some("/tmp/写真 1.png"));
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
