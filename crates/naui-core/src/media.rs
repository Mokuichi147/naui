//! Value types for media (images, video, audio).
//!
//! All that is here are values describing "how you want it fitted" and "whether it
//! is playing now", plus small checks each backend uses to interpret the given
//! location. **naui does no decoding or playback.** Images are opened and drawn by
//! NSImage / the browser / WinRT, and sound is played by AVPlayer /
//! HTMLMediaElement / Windows.Media.Playback.MediaPlayer.

/// How the picture is fitted into the display area.
///
/// Applies to the picture of images (`Image`) and video (`Video`).
///
/// | Value | macOS (NSImageView) | Web (CSS) | Windows (XAML) |
/// | --- | --- | --- | --- |
/// | [`Fit::Contain`] | `ProportionallyUpOrDown` | `object-fit: contain` | `Stretch="Uniform"` |
/// | [`Fit::Cover`] | `ProportionallyUpOrDown` + clipping | `object-fit: cover` | `Stretch="UniformToFill"` |
/// | [`Fit::Fill`] | `AxesIndependently` | `object-fit: fill` | `Stretch="Fill"` |
/// | [`Fit::None`] | `None` | `object-fit: none` | `Stretch="None"` |
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Fit {
    /// Fits without overflowing, keeping the aspect ratio (default).
    #[default]
    Contain,
    /// Fills the area, keeping the aspect ratio. Whatever overflows is cropped.
    Cover,
    /// Stretches to fill the area, ignoring the aspect ratio.
    Fill,
    /// Neither scales up nor down (actual size).
    None,
}

/// The playback state.
///
/// Reported by `on_state_change` of [`Video`] / [`Audio`]. It arrives not only when
/// the app calls `play()` but **also when the user operates the native playback
/// bar**.
///
/// [`Video`]: https://docs.rs/naui/latest/naui/struct.Video.html
/// [`Audio`]: https://docs.rs/naui/latest/naui/struct.Audio.html
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlaybackState {
    /// Not played yet, or not loaded yet (default).
    #[default]
    Idle,
    /// Trying to play, but waiting for more data.
    Buffering,
    /// Playing.
    Playing,
    /// Paused.
    Paused,
    /// Played to the end.
    Ended,
}

impl PlaybackState {
    /// Whether sound is coming out (the picture is advancing).
    pub fn is_playing(self) -> bool {
        matches!(self, PlaybackState::Playing)
    }
}

/// Determines whether a media location string is already a URL.
///
/// It counts as a URL if it starts with an RFC 3986 scheme
/// (`ALPHA *( ALPHA / DIGIT / "+" / "-" / "." )` followed by `:`). **However,
/// one-letter schemes are not treated as URLs.** This keeps Windows drive letters
/// (`C:\...`) from being mistaken for schemes; no one-letter schemes exist.
///
/// ```
/// # use naui_core::media::is_url;
/// assert!(is_url("https://example.com/photo.jpg"));
/// assert!(is_url("file:///Users/me/photo.jpg"));
/// assert!(!is_url("/Users/me/photo.jpg"));
/// // A drive letter is not a scheme.
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

/// Turns a local file path into a `file://` URL.
///
/// Separators are normalized to `/`, and characters that mean something in URLs and
/// non-ASCII are percent-encoded. UNC paths (`\\server\share`) become
/// `file://server/share` with a host name.
///
/// ```
/// # use naui_core::media::file_url;
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

/// Whether the character can appear as is in the path part of a URL.
///
/// RFC 3986's unreserved characters plus the path separator and the `:` and `@`
/// allowed within a segment. Encoding everything else always yields a valid URL.
fn is_safe_in_path(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~' | b'/' | b':' | b'@')
}

/// Normalizes a media location into a URL string to hand to the native side.
///
/// URLs are returned as is; anything else is taken as a local file path and turned
/// into a `file://` URL. The Web backend, which has no concept of file paths, does
/// not use this and lets the browser resolve it as a relative URL.
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
        assert_eq!(
            file_url(r"\\server\share\a.png"),
            "file://server/share/a.png"
        );
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
    fn source_url_passes_urls_through() {
        assert_eq!(
            source_url("https://example.com/a.png"),
            "https://example.com/a.png"
        );
        assert_eq!(source_url("/tmp/a.png"), "file:///tmp/a.png");
    }
}
