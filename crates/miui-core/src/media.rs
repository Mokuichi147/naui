//! メディア (画像・動画・音声) の値型。
//!
//! ここにあるのは「どう収めたいか」「いま再生中か」を表す値と、
//! 指定された場所を各バックエンドが解釈するための小さな判定だけ。
//! **デコードも再生も miui は行わない。** 実際に画像を開いて描くのは
//! NSImage / ブラウザ / WinRT で、音を鳴らすのは AVPlayer / HTMLMediaElement /
//! Windows.Media.Playback.MediaPlayer である。

/// 表示領域に対する映像の収め方。
///
/// 画像 (`Image`) と動画 (`Video`) の映像面に効く。
///
/// | 値 | macOS (NSImageView) | Web (CSS) | Windows (XAML) |
/// | --- | --- | --- | --- |
/// | [`Fit::Contain`] | `ProportionallyUpOrDown` | `object-fit: contain` | `Stretch="Uniform"` |
/// | [`Fit::Cover`] | `ProportionallyUpOrDown` + クリップ | `object-fit: cover` | `Stretch="UniformToFill"` |
/// | [`Fit::Fill`] | `AxesIndependently` | `object-fit: fill` | `Stretch="Fill"` |
/// | [`Fit::None`] | `None` | `object-fit: none` | `Stretch="None"` |
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Fit {
    /// 縦横比を保ったまま、はみ出さないように収める (既定)。
    #[default]
    Contain,
    /// 縦横比を保ったまま領域を埋める。はみ出した部分は切り取られる。
    Cover,
    /// 縦横比を無視して領域いっぱいに引き伸ばす。
    Fill,
    /// 拡大も縮小もしない (原寸)。
    None,
}

/// 再生の状態。
///
/// [`Video`] / [`Audio`] の `on_state_change` で通知される。
/// アプリ側から `play()` を呼んだときだけでなく、**ネイティブの再生バーを
/// ユーザーが操作したときにも届く**。
///
/// [`Video`]: https://docs.rs/miui/latest/miui/struct.Video.html
/// [`Audio`]: https://docs.rs/miui/latest/miui/struct.Audio.html
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlaybackState {
    /// まだ再生されていない、または読み込み前 (既定)。
    #[default]
    Idle,
    /// 再生しようとしているが、データが足りず待っている。
    Buffering,
    /// 再生中。
    Playing,
    /// 一時停止中。
    Paused,
    /// 最後まで再生し終えた。
    Ended,
}

impl PlaybackState {
    /// 音が出ている (映像が進んでいる) 状態か。
    pub fn is_playing(self) -> bool {
        matches!(self, PlaybackState::Playing)
    }
}

/// メディアの指定文字列が、すでに URL かどうかを判定する。
///
/// RFC 3986 のスキーム (`ALPHA *( ALPHA / DIGIT / "+" / "-" / "." )` の後に `:`)
/// が付いていれば URL とみなす。**ただし 1 文字のスキームは URL としない。**
/// Windows のドライブレター (`C:\...`) をスキームと取り違えないためで、
/// 1 文字のスキームは実在しない。
///
/// ```
/// # use miui_core::media::is_url;
/// assert!(is_url("https://example.com/photo.jpg"));
/// assert!(is_url("file:///Users/me/photo.jpg"));
/// assert!(!is_url("/Users/me/photo.jpg"));
/// // ドライブレターはスキームではない。
/// assert!(!is_url(r"C:\Users\me\photo.jpg"));
/// ```
pub fn is_url(source: &str) -> bool {
    let Some(end) = source.find(':') else {
        return false;
    };
    let scheme = &source[..end];
    if scheme.len() < 2 {
        return false;
    }
    let mut chars = scheme.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
}

/// ローカルのファイルパスを `file://` URL にする。
///
/// 区切りは `/` にそろえ、URL で意味を持つ文字と非 ASCII をパーセント
/// エンコードする。UNC パス (`\\server\share`) はホスト名付きの
/// `file://server/share` になる。
///
/// ```
/// # use miui_core::media::file_url;
/// assert_eq!(file_url("/Users/me/a b.png"), "file:///Users/me/a%20b.png");
/// assert_eq!(file_url(r"C:\Users\me\a.png"), "file:///C:/Users/me/a.png");
/// assert_eq!(file_url(r"\\server\share\a.png"), "file://server/share/a.png");
/// ```
pub fn file_url(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    // UNC パスの先頭 `//` は「ホスト名が続く」の意味なので、`file:` を足すだけ。
    // それ以外は「ホスト名が空」を表す `file://` の後ろに絶対パスを置く。
    let mut url = String::from(if normalized.starts_with("//") {
        "file:"
    } else if normalized.starts_with('/') {
        "file://"
    } else {
        "file:///"
    });
    for byte in normalized.bytes() {
        if is_safe_in_path(byte) {
            url.push(byte as char);
        } else {
            url.push('%');
            url.push(HEX[(byte >> 4) as usize] as char);
            url.push(HEX[(byte & 0x0f) as usize] as char);
        }
    }
    url
}

const HEX: &[u8; 16] = b"0123456789ABCDEF";

/// URL のパス部分にそのまま置ける文字か。
///
/// RFC 3986 の unreserved に、パスの区切りと、セグメント内で許される
/// `:` `@` を足したもの。残りは encode しておけば必ず正しい URL になる。
fn is_safe_in_path(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~' | b'/' | b':' | b'@')
}

/// メディアの種類。
///
/// **拡張子からの推測で、中身は見ない。** どのウィジェットで表示するかを
/// アプリが選ぶための目安として使う。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaKind {
    /// 画像 ([`Image`] で表示する)。
    ///
    /// [`Image`]: https://docs.rs/miui/latest/miui/struct.Image.html
    Image,
    /// 動画 ([`Video`] で表示する)。
    ///
    /// [`Video`]: https://docs.rs/miui/latest/miui/struct.Video.html
    Video,
    /// 音声 ([`Audio`] で表示する)。
    ///
    /// [`Audio`]: https://docs.rs/miui/latest/miui/struct.Audio.html
    Audio,
}

/// [`MediaKind::Image`] とみなす拡張子。
const IMAGE_EXTENSIONS: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "bmp", "webp", "avif", "heic", "heif", "tiff", "tif", "ico", "svg",
];
/// [`MediaKind::Video`] とみなす拡張子。
const VIDEO_EXTENSIONS: &[&str] = &[
    "mp4", "m4v", "mov", "webm", "mkv", "avi", "wmv", "mpg", "mpeg", "ogv", "3gp",
];
/// [`MediaKind::Audio`] とみなす拡張子。
const AUDIO_EXTENSIONS: &[&str] = &[
    "mp3", "m4a", "aac", "wav", "flac", "ogg", "oga", "opus", "aiff", "aif", "wma",
];

impl MediaKind {
    /// ファイル名や場所の拡張子から種類を推測する。
    ///
    /// **中身は見ない。** 拡張子が無い場合や、知らない拡張子の場合は `None`。
    /// 判断できないときにどうするかはアプリが決める。
    ///
    /// クエリやフラグメントの付いた URL も扱えるが、**Web の `blob:` URL には
    /// 拡張子が無い**ので `None` になる。ファイル選択と組み合わせるときは、
    /// 場所ではなく [`FileEntry::name`](crate::FileEntry::name) を渡すこと。
    ///
    /// ```
    /// # use miui_core::media::MediaKind;
    /// assert_eq!(MediaKind::guess("photo.JPG"), Some(MediaKind::Image));
    /// assert_eq!(MediaKind::guess("https://example.com/clip.mp4?t=1"), Some(MediaKind::Video));
    /// assert_eq!(MediaKind::guess("bgm.m4a"), Some(MediaKind::Audio));
    /// assert_eq!(MediaKind::guess("readme"), None);
    /// ```
    pub fn guess(source: &str) -> Option<MediaKind> {
        let extension = extension_of(source)?;
        if IMAGE_EXTENSIONS.contains(&extension.as_str()) {
            Some(MediaKind::Image)
        } else if VIDEO_EXTENSIONS.contains(&extension.as_str()) {
            Some(MediaKind::Video)
        } else if AUDIO_EXTENSIONS.contains(&extension.as_str()) {
            Some(MediaKind::Audio)
        } else {
            None
        }
    }

    /// この種類として扱う拡張子の一覧。ファイル選択の絞り込みに使える。
    pub fn extensions(self) -> &'static [&'static str] {
        match self {
            MediaKind::Image => IMAGE_EXTENSIONS,
            MediaKind::Video => VIDEO_EXTENSIONS,
            MediaKind::Audio => AUDIO_EXTENSIONS,
        }
    }
}

/// 場所の末尾から、小文字にそろえた拡張子を取り出す。
///
/// クエリとフラグメントは落とす。区切りは `/` と `\` の両方を見る
/// (Windows のパスと URL の両方を受けるため)。
fn extension_of(source: &str) -> Option<String> {
    let without_query = source
        .split(['?', '#'])
        .next()
        .filter(|s| !s.is_empty())?;
    let name = without_query
        .rsplit(['/', '\\'])
        .next()
        .filter(|s| !s.is_empty())?;
    // 先頭のドットだけのファイル (`.gitignore`) は拡張子を持たないとみなす。
    let (stem, extension) = name.rsplit_once('.')?;
    if stem.is_empty() || extension.is_empty() {
        return None;
    }
    Some(extension.to_ascii_lowercase())
}

/// メディアの場所を、ネイティブへ渡す URL 文字列にそろえる。
///
/// すでに URL ならそのまま、そうでなければローカルのファイルパスとみなして
/// `file://` URL にする。ファイルパスという概念を持たない Web バックエンドは
/// これを使わず、相対 URL としてブラウザに解決させる。
pub fn source_url(source: &str) -> String {
    if is_url(source) {
        source.to_string()
    } else {
        file_url(source)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_and_state_have_sensible_defaults() {
        assert_eq!(Fit::default(), Fit::Contain);
        assert_eq!(PlaybackState::default(), PlaybackState::Idle);
        assert!(PlaybackState::Playing.is_playing());
        assert!(!PlaybackState::Buffering.is_playing());
    }

    #[test]
    fn schemes_are_recognised_as_urls() {
        assert!(is_url("https://example.com/photo.jpg"));
        assert!(is_url("http://example.com"));
        assert!(is_url("file:///Users/me/photo.jpg"));
        assert!(is_url("data:image/png;base64,AAAA"));
        assert!(is_url("rtsp+tcp://example.com/live"));
    }

    #[test]
    fn paths_are_not_urls() {
        assert!(!is_url("/Users/me/photo.jpg"));
        assert!(!is_url("assets/photo.jpg"));
        assert!(!is_url("photo.jpg"));
        assert!(!is_url(""));
        // ドライブレターを 1 文字スキームと取り違えない。
        assert!(!is_url(r"C:\Users\me\photo.jpg"));
        // スキームは英字で始まる。
        assert!(!is_url("1st:thing"));
    }

    #[test]
    fn file_url_encodes_and_normalises_separators() {
        assert_eq!(file_url("/Users/me/a b.png"), "file:///Users/me/a%20b.png");
        assert_eq!(file_url(r"C:\Users\me\a.png"), "file:///C:/Users/me/a.png");
        assert_eq!(file_url(r"\\server\share\a.png"), "file://server/share/a.png");
        // 相対パスでも `file://` の後ろは絶対パスの形にそろえる。
        assert_eq!(file_url("a.png"), "file:///a.png");
    }

    #[test]
    fn file_url_percent_encodes_non_ascii() {
        assert_eq!(
            file_url("/tmp/写真.png"),
            "file:///tmp/%E5%86%99%E7%9C%9F.png"
        );
        // `?` や `#` はそのまま置くとクエリ・フラグメントになってしまう。
        assert_eq!(file_url("/tmp/a?b#c.png"), "file:///tmp/a%3Fb%23c.png");
    }

    #[test]
    fn media_kind_is_guessed_from_the_extension() {
        assert_eq!(MediaKind::guess("photo.png"), Some(MediaKind::Image));
        // 大文字でも同じ。
        assert_eq!(MediaKind::guess("IMG_0001.JPG"), Some(MediaKind::Image));
        assert_eq!(MediaKind::guess("/tmp/clip.mov"), Some(MediaKind::Video));
        assert_eq!(MediaKind::guess(r"C:\\Users\\me\\bgm.m4a"), Some(MediaKind::Audio));
        // クエリやフラグメントは無視する。
        assert_eq!(
            MediaKind::guess("https://example.com/a/clip.mp4?token=1#t=3"),
            Some(MediaKind::Video)
        );
    }

    #[test]
    fn media_kind_gives_up_when_it_cannot_tell() {
        assert_eq!(MediaKind::guess("readme"), None);
        assert_eq!(MediaKind::guess(""), None);
        assert_eq!(MediaKind::guess("archive.zip"), None);
        // ドットで始まるだけのファイルは拡張子を持たない。
        assert_eq!(MediaKind::guess(".gitignore"), None);
        // Web の blob URL には拡張子が無い。名前のほうを渡す必要がある。
        assert_eq!(MediaKind::guess("blob:http://localhost/9f2c-1a"), None);
    }

    #[test]
    fn media_kind_lists_its_own_extensions() {
        assert!(MediaKind::Image.extensions().contains(&"png"));
        assert!(MediaKind::Video.extensions().contains(&"mp4"));
        assert!(MediaKind::Audio.extensions().contains(&"m4a"));
        // 種類どうしで拡張子が重ならないこと。
        for other in [MediaKind::Video, MediaKind::Audio] {
            for ext in MediaKind::Image.extensions() {
                assert!(!other.extensions().contains(ext), "{ext} が重複している");
            }
        }
    }

    #[test]
    fn source_url_passes_urls_through() {
        assert_eq!(
            source_url("https://example.com/a.png"),
            "https://example.com/a.png"
        );
        assert_eq!(source_url("/tmp/a.png"), "file:///tmp/a.png");
    }
}
