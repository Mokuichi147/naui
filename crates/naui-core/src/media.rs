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

/// Whether the bytes are an SVG document rather than a raster image.
///
/// Raster formats start with a fixed signature that every platform's decoder
/// recognizes by itself, but SVG is text, and some decoders (WinUI's `BitmapImage`,
/// a browser loading a `Blob` without a type) need to be told. This looks past a
/// byte order mark, whitespace, an XML declaration, comments and a `DOCTYPE` for an
/// `<svg` start tag within the first few kilobytes. UTF-8 and, with a byte order
/// mark (which XML requires of it), UTF-16 in either byte order are read.
///
/// ```
/// # use naui_core::media::is_svg;
/// assert!(is_svg(br#"<svg xmlns="http://www.w3.org/2000/svg"/>"#));
/// assert!(is_svg(b"<?xml version=\"1.0\"?>\n<!-- icon -->\n<svg/>"));
/// assert!(is_svg(b"\xFF\xFE<\0s\0v\0g\0/\0>\0"));
/// assert!(!is_svg(b"\x89PNG\r\n\x1a\n"));
/// ```
pub fn is_svg(bytes: &[u8]) -> bool {
    // UTF-16 は UTF-8 の倍ほどの長さになるので、倍まで読んで同じだけの文字を見る。
    match decode_utf16(&bytes[..bytes.len().min(SVG_LOOK * 2)]) {
        Some(text) => svg_root(&text).is_some(),
        None => svg_root(bytes).is_some(),
    }
}

/// How many bytes from the start [`is_svg`] looks through for the root element.
const SVG_LOOK: usize = 4096;

/// How many bytes of a UTF-16 document [`svg_size`] turns into UTF-8: the part where
/// the root element starts ([`SVG_LOOK`] characters) and room for its start tag.
const SVG_TAG_LOOK: usize = 64 * 1024;

/// UTF-16 text with a byte order mark, as UTF-8. `None` without one of the marks.
///
/// SVG is looked into as UTF-8 bytes; the few documents written in UTF-16 are
/// turned into it first. A unit cut off at the end is dropped, and unpaired
/// surrogates become U+FFFD.
fn decode_utf16(bytes: &[u8]) -> Option<Vec<u8>> {
    let (little, body) = match bytes {
        [0xFF, 0xFE, body @ ..] => (true, body),
        [0xFE, 0xFF, body @ ..] => (false, body),
        _ => return None,
    };
    let units = body.as_chunks::<2>().0.iter().map(|&pair| {
        if little {
            u16::from_le_bytes(pair)
        } else {
            u16::from_be_bytes(pair)
        }
    });
    let text: String = char::decode_utf16(units)
        .map(|c| c.unwrap_or(char::REPLACEMENT_CHARACTER))
        .collect();
    Some(text.into_bytes())
}

/// `bytes` from the root `<svg` start tag on, found the way [`is_svg`] describes.
/// `bytes` must be UTF-8 (or ASCII-compatible); see [`decode_utf16`].
fn svg_root(bytes: &[u8]) -> Option<&[u8]> {
    let head = &bytes[..bytes.len().min(SVG_LOOK)];
    let mut rest = head.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(head);
    loop {
        rest = trim_ascii_start(rest);
        if rest.starts_with(b"<svg") {
            let is_tag = rest
                .get(4)
                .is_none_or(|next| next.is_ascii_whitespace() || matches!(next, b'>' | b'/'));
            // rest は head の末尾側なので、長さの差がそのまま bytes の中の位置になる。
            return is_tag.then(|| &bytes[head.len() - rest.len()..]);
        }
        // 宣言・コメント・DOCTYPE は読み飛ばす。それ以外の要素が先に来たら SVG ではない。
        let len = if rest.starts_with(b"<?") {
            skip_past(rest, b"?>")?
        } else if rest.starts_with(b"<!--") {
            skip_past(rest, b"-->")?
        } else if rest.starts_with(b"<!") {
            doctype_len(rest)?
        } else {
            return None;
        };
        rest = &rest[len..];
    }
}

/// How far `bytes` runs up to and including the first `end`.
fn skip_past(bytes: &[u8], end: &[u8]) -> Option<usize> {
    let at = bytes.windows(end.len()).position(|window| window == end)?;
    Some(at + end.len())
}

/// The length of the `<!DOCTYPE ...>` (or other `<!...>` declaration) `bytes` begins
/// with.
///
/// The internal subset (`[...]`) holds declarations of its own, so a `>` counts as the
/// end only outside it and outside quotes. Comments and processing instructions in
/// the subset are skipped whole, since they may hold quotes and brackets.
fn doctype_len(bytes: &[u8]) -> Option<usize> {
    let mut quote = None;
    let mut in_subset = false;
    let mut i = 2;
    while let Some(&byte) = bytes.get(i) {
        match (quote, byte) {
            (Some(open), _) if byte == open => quote = None,
            (Some(_), _) => {}
            (None, b'"' | b'\'') => quote = Some(byte),
            (None, b'<') if in_subset && bytes[i..].starts_with(b"<!--") => {
                i += skip_past(&bytes[i..], b"-->")?;
                continue;
            }
            (None, b'<') if in_subset && bytes[i..].starts_with(b"<?") => {
                i += skip_past(&bytes[i..], b"?>")?;
                continue;
            }
            (None, b'[') => in_subset = true,
            (None, b']') => in_subset = false,
            (None, b'>') if !in_subset => return Some(i + 1),
            _ => {}
        }
        i += 1;
    }
    None
}

/// The size an SVG document gives itself, in CSS pixels: its root element's `width`
/// and `height`, with the aspect ratio of the `viewBox` filling in whichever is
/// missing or relative (`em`, `ex`, `%`). Absolute units (`in`, `cm`, `mm`, `Q`,
/// `pt`, `pc`) are converted at CSS's 96 pixels to the inch. Without either, the
/// default size of 300 × 150 is used, as a browser shows such an SVG: the `viewBox`'s
/// aspect ratio fitted inside it, or, with no `viewBox` either, as it is. `None`
/// only if the bytes are not SVG or its start tag is cut off.
///
/// For platforms whose SVG image source does not report a natural size (WinUI's
/// `SvgImageSource`). Only the root start tag is read.
///
/// ```
/// # use naui_core::media::svg_size;
/// assert_eq!(svg_size(br#"<svg width="24" height="16"/>"#), Some((24.0, 16.0)));
/// assert_eq!(svg_size(br#"<svg viewBox="0 0 48 32" width="24px"/>"#), Some((24.0, 16.0)));
/// assert_eq!(svg_size(br#"<svg viewBox="0 0 100 100"/>"#), Some((150.0, 150.0)));
/// assert_eq!(svg_size(b"<svg/>"), Some((300.0, 150.0)));
/// assert_eq!(svg_size(b"\x89PNG"), None);
/// ```
pub fn svg_size(bytes: &[u8]) -> Option<(f64, f64)> {
    // UTF-16 は根の開始タグに要る先頭だけを直す。文書全体は写さない。
    let decoded = decode_utf16(&bytes[..bytes.len().min(SVG_TAG_LOOK)]);
    let bytes = decoded.as_deref().unwrap_or(bytes);
    // 先頭のコメントの中の `<svg` ではなく、根の要素の開始タグを読む。
    let attributes = start_tag_attributes(svg_root(bytes)?)?;
    let attribute = |name: &str| {
        attributes
            .iter()
            .find(|(key, _)| *key == name.as_bytes())
            .and_then(|(_, value)| std::str::from_utf8(value).ok())
            .map(str::trim)
    };

    let pixels = |name: &str| {
        let value = attribute(name)?;
        let unit_at = value
            .rfind(|c: char| !c.is_ascii_alphabetic())
            .map_or(0, |at| at + 1);
        let (number, unit) = value.split_at(unit_at);
        // CSS の絶対単位は 1in = 96px で換算する。em・ex・% は文書の外で決まるので読まない。
        let scale = match unit.to_ascii_lowercase().as_str() {
            "" | "px" => 1.0,
            "in" => 96.0,
            "cm" => 96.0 / 2.54,
            "mm" => 96.0 / 25.4,
            "q" => 96.0 / 101.6,
            "pt" => 96.0 / 72.0,
            "pc" => 96.0 / 6.0,
            _ => return None,
        };
        number
            .trim()
            .parse::<f64>()
            .ok()
            .map(|v| v * scale)
            .filter(|v| v.is_finite() && *v > 0.0)
    };
    let view_box = attribute("viewBox").and_then(|value| {
        let numbers: Vec<f64> = value
            .split(|c: char| c.is_ascii_whitespace() || c == ',')
            .filter(|part| !part.is_empty())
            .map(str::parse)
            .collect::<std::result::Result<_, _>>()
            .ok()?;
        match numbers[..] {
            [_, _, w, h] if w > 0.0 && h > 0.0 => Some((w, h)),
            _ => None,
        }
    });
    match (pixels("width"), pixels("height"), view_box) {
        (Some(w), Some(h), _) => Some((w, h)),
        (Some(w), None, Some((vw, vh))) => Some((w, w * vh / vw)),
        (None, Some(h), Some((vw, vh))) => Some((h * vw / vh, h)),
        // viewBox は縦横比だけを決める。大きさは 300×150 にその比で収める
        // (CSS の既定の大きさに contain で合わせる)。
        (None, None, Some((vw, vh))) => {
            let (dw, dh) = DEFAULT_SVG_SIZE;
            Some(if vw / vh > dw / dh {
                (dw, dw * vh / vw)
            } else {
                (dh * vw / vh, dh)
            })
        }
        // 縦横比も無いときは、CSS の置換要素の既定の大きさ 300×150 で埋める。
        (Some(w), None, None) => Some((w, DEFAULT_SVG_SIZE.1)),
        (None, Some(h), None) => Some((DEFAULT_SVG_SIZE.0, h)),
        (None, None, None) => Some(DEFAULT_SVG_SIZE),
    }
}

/// The size of an SVG that gives none of its own, in CSS pixels: CSS's default
/// object size for replaced elements, which SVG 2 takes for a missing `width` or
/// `height` when there is no `viewBox` to go by.
const DEFAULT_SVG_SIZE: (f64, f64) = (300.0, 150.0);

/// The `name="value"` (or `'value'`) pairs of the start tag `tag` begins with, read
/// in order up to its closing `>` or `/>`. `None` if the tag does not close.
///
/// Read pair by pair, so a `>` or another attribute's name inside a quoted value
/// is taken as part of that value.
fn start_tag_attributes(tag: &[u8]) -> Option<Vec<(&[u8], &[u8])>> {
    // 要素名を飛ばす。
    let name_end = tag
        .iter()
        .position(|byte| byte.is_ascii_whitespace() || matches!(byte, b'>' | b'/'))?;
    let mut rest = &tag[name_end..];
    let mut pairs = Vec::new();
    loop {
        rest = trim_ascii_start(rest);
        if matches!(rest.first()?, b'>' | b'/') {
            return Some(pairs);
        }
        let key_end = rest
            .iter()
            .position(|byte| byte.is_ascii_whitespace() || matches!(byte, b'=' | b'>' | b'/'))?;
        let key = &rest[..key_end];
        // XML の属性は必ず `="..."` か `='...'` を持つ。崩れていたらそこで読むのをやめる。
        let after = trim_ascii_start(&rest[key_end..]);
        let Some(after) = after.strip_prefix(b"=").map(trim_ascii_start) else {
            return Some(pairs);
        };
        let quote = *after.first()?;
        if !matches!(quote, b'"' | b'\'') {
            return Some(pairs);
        }
        let value_end = after[1..].iter().position(|&byte| byte == quote)?;
        pairs.push((key, &after[1..1 + value_end]));
        rest = &after[2 + value_end..];
    }
}

fn trim_ascii_start(bytes: &[u8]) -> &[u8] {
    let start = bytes
        .iter()
        .position(|byte| !byte.is_ascii_whitespace())
        .unwrap_or(bytes.len());
    &bytes[start..]
}

/// Whether a file path or URL names an SVG file, judged by its extension
/// (`.svg` / `.svgz`, ignoring case). A URL's query and fragment are ignored; in a
/// file path, `#` and `?` are part of the name.
///
/// ```
/// # use naui_core::media::is_svg_source;
/// assert!(is_svg_source("icons/save.svg"));
/// assert!(is_svg_source("https://example.com/logo.SVG?v=2"));
/// assert!(is_svg_source(r"C:\icons\logo#dark.svg"));
/// assert!(!is_svg_source("photo.png"));
/// ```
pub fn is_svg_source(source: &str) -> bool {
    let path = if is_url(source) {
        source.split(['?', '#']).next().unwrap_or(source)
    } else {
        source
    };
    let lower = path.to_ascii_lowercase();
    lower.ends_with(".svg") || lower.ends_with(".svgz")
}

/// Checks that `rgba` holds exactly `width` × `height` pixels of 4 bytes each.
///
/// The pixel format of `Image::set_pixels`: rows from top to bottom, each pixel red,
/// green, blue and alpha, with straight (not premultiplied) alpha.
pub fn check_pixels(width: u32, height: u32, rgba: &[u8]) -> crate::Result<()> {
    if width == 0 || height == 0 {
        return Err(crate::Error::new(
            "show the pixels",
            format!("the size {width} x {height} has no pixels"),
        ));
    }
    // u32 同士の積の 4 倍は u64 にも収まらないことがある。回り込んだ値で照合しない。
    let Some(expected) = (width as u64)
        .checked_mul(height as u64)
        .and_then(|pixels| pixels.checked_mul(4))
    else {
        return Err(crate::Error::new(
            "show the pixels",
            format!("the size {width} x {height} is too large"),
        ));
    };
    if rgba.len() as u64 != expected {
        return Err(crate::Error::new(
            "show the pixels",
            format!(
                "{width} x {height} pixels need {expected} bytes of RGBA, but {} were given",
                rgba.len()
            ),
        ));
    }
    Ok(())
}

/// Packs RGBA pixels into a PNG file without compressing them.
///
/// Platforms whose image controls take only encoded images (WinUI's `BitmapImage`,
/// `<img>`, `NSImage`'s data initializer) are handed pixels this way, so that they
/// decode and scale them exactly as they would a file. **The data is stored, not
/// compressed** (deflate's stored blocks): it is about as large as the pixels, and
/// writing it is a copy plus two checksums. The pixels are checked with
/// [`check_pixels`] first.
///
/// ```
/// # use naui_core::media::encode_png;
/// let png = encode_png(1, 1, &[255, 0, 0, 255]).unwrap();
/// assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
/// ```
pub fn encode_png(width: u32, height: u32, rgba: &[u8]) -> crate::Result<Vec<u8>> {
    check_pixels(width, height, rgba)?;
    let row = width as usize * 4;
    // 各行の先頭にフィルタの種類 (0 = なし) を置いたものが圧縮前のデータ。
    let mut raw = Vec::with_capacity((row + 1) * height as usize);
    for line in rgba.chunks_exact(row) {
        raw.push(0);
        raw.extend_from_slice(line);
    }

    // zlib: ヘッダ、無圧縮ブロックの並び、Adler-32。
    const BLOCK: usize = 65_535;
    let mut zlib = Vec::with_capacity(raw.len() + raw.len() / BLOCK * 5 + 11);
    zlib.extend_from_slice(&[0x78, 0x01]);
    let mut blocks = raw.chunks(BLOCK).peekable();
    while let Some(block) = blocks.next() {
        zlib.push(u8::from(blocks.peek().is_none()));
        let len = block.len() as u16;
        zlib.extend_from_slice(&len.to_le_bytes());
        zlib.extend_from_slice(&(!len).to_le_bytes());
        zlib.extend_from_slice(block);
    }
    zlib.extend_from_slice(&adler32(&raw).to_be_bytes());

    let mut png = Vec::with_capacity(zlib.len() + 64);
    png.extend_from_slice(b"\x89PNG\r\n\x1a\n");
    let mut header = Vec::with_capacity(13);
    header.extend_from_slice(&width.to_be_bytes());
    header.extend_from_slice(&height.to_be_bytes());
    // 8 bit、色の種類 6 (RGBA)、圧縮 0、フィルタ 0、インターレース無し。
    header.extend_from_slice(&[8, 6, 0, 0, 0]);
    push_chunk(&mut png, b"IHDR", &header);
    push_chunk(&mut png, b"IDAT", &zlib);
    push_chunk(&mut png, b"IEND", &[]);
    Ok(png)
}

fn push_chunk(png: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    png.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let start = png.len();
    png.extend_from_slice(kind);
    png.extend_from_slice(data);
    let crc = crc32(&png[start..]);
    png.extend_from_slice(&crc.to_be_bytes());
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in bytes {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

fn adler32(bytes: &[u8]) -> u32 {
    const MOD: u32 = 65_521;
    let (mut a, mut b) = (1u32, 0u32);
    // 5552 バイトごとに剰余を取れば u32 からあふれない。
    for chunk in bytes.chunks(5552) {
        for &byte in chunk {
            a += u32::from(byte);
            b += a;
        }
        a %= MOD;
        b %= MOD;
    }
    (b << 16) | a
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn svg_is_recognised_past_prologue() {
        assert!(is_svg(b"<svg/>"));
        assert!(is_svg(b"\xEF\xBB\xBF  <svg width='1'></svg>"));
        assert!(is_svg(
            b"<?xml version='1.0'?><!DOCTYPE svg PUBLIC '-//W3C//DTD SVG 1.1//EN' 'x'><svg>"
        ));
        // UTF-16 は BOM でバイト順を見分けて読む。
        let doc = "<?xml version=\"1.0\" encoding=\"UTF-16\"?>\n<svg width=\"24\" height=\"16\"/>";
        let le: Vec<u8> = b"\xFF\xFE"
            .iter()
            .copied()
            .chain(doc.encode_utf16().flat_map(u16::to_le_bytes))
            .collect();
        let be: Vec<u8> = b"\xFE\xFF"
            .iter()
            .copied()
            .chain(doc.encode_utf16().flat_map(u16::to_be_bytes))
            .collect();
        assert!(is_svg(&le));
        assert!(is_svg(&be));
        assert_eq!(svg_size(&le), Some((24.0, 16.0)));
        assert_eq!(svg_size(&be), Some((24.0, 16.0)));
        // 大きな文書でも、変換するのは先頭だけで足りる。
        let mut large = le.clone();
        large.extend(
            "<rect/>"
                .repeat(100_000)
                .encode_utf16()
                .flat_map(u16::to_le_bytes),
        );
        assert_eq!(svg_size(&large), Some((24.0, 16.0)));
        // BOM の無い UTF-16 と、BOM の後ろが SVG でないものは認めない。
        assert!(!is_svg(&le[2..]));
        assert!(!is_svg(b"\xFF\xFE\x89\0P\0N\0G\0"));
        // 内部サブセットの中の > や引用符・コメントで DOCTYPE を終えない。
        assert!(is_svg(
            br#"<!DOCTYPE svg [<!ENTITY unused "x">]><svg width="24" height="16"/>"#
        ));
        assert!(is_svg(
            b"<!DOCTYPE svg PUBLIC \"-//W3C//DTD SVG 1.1//EN\" \"a>b\" [\n\
              <!ENTITY a 'x]>y'>\n<!-- ] > ' -->\n<?pi ] > ?>\n]>\n<svg>"
        ));
        assert!(!is_svg(br#"<!DOCTYPE svg [<!ENTITY a "x">"#));
        assert!(!is_svg(b"<svgx>"));
        assert!(!is_svg(b"<html><svg/></html>"));
        assert!(!is_svg(b"\xFF\xD8\xFF\xE0"));
        assert!(!is_svg(b"<!-- unterminated"));
        assert!(!is_svg(b""));
    }

    #[test]
    fn svg_size_reads_the_root_element() {
        let svg = br#"<?xml version="1.0"?>
<svg xmlns="http://www.w3.org/2000/svg"
     width='24.5' height="16px" stroke-width="3"><rect width="100" height="100"/></svg>"#;
        assert_eq!(svg_size(svg), Some((24.5, 16.0)));
        // stroke-width の width を取り違えない。寸法が無ければ viewBox は比率だけで、
        // 300×150 に収める。
        assert_eq!(
            svg_size(br#"<svg stroke-width="3" viewBox="0,0,10,20">"#),
            Some((75.0, 150.0))
        );
        assert_eq!(
            svg_size(br#"<svg width="50%" viewBox="0 0 10 20">"#),
            Some((75.0, 150.0))
        );
        assert_eq!(
            svg_size(br#"<svg viewBox="0 0 100 100">"#),
            Some((150.0, 150.0))
        );
        // 300×150 より横長なら幅で収める。
        assert_eq!(
            svg_size(br#"<svg viewBox="0 0 400 100">"#),
            Some((300.0, 75.0))
        );
        assert_eq!(
            svg_size(br#"<svg height="40" viewBox="0 0 10 20">"#),
            Some((20.0, 40.0))
        );
        // 寸法も viewBox も無ければ、足りない方を 300×150 で埋める。
        assert_eq!(svg_size(br#"<svg width="2em">"#), Some((300.0, 150.0)));
        assert_eq!(
            svg_size(br#"<svg xmlns="http://www.w3.org/2000/svg"><rect/></svg>"#),
            Some((300.0, 150.0))
        );
        assert_eq!(svg_size(br#"<svg width="24">"#), Some((24.0, 150.0)));
        assert_eq!(
            svg_size(br#"<svg height="50%" width="1in">"#),
            Some((96.0, 150.0))
        );
        assert_eq!(svg_size(br#"<svg height="16">"#), Some((300.0, 16.0)));
        // 引用符の中の > や属性名らしき文字列は値の一部として読む。
        assert_eq!(
            svg_size(br#"<svg aria-label="A > B" width="24" height="16">"#),
            Some((24.0, 16.0))
        );
        assert_eq!(
            svg_size(br#"<svg data-x='w width="1"' width="24" height="16"/>"#),
            Some((24.0, 16.0))
        );
        assert_eq!(svg_size(br#"<svg width="24" height="16""#), None);
        assert_eq!(
            svg_size(br#"<!DOCTYPE svg [<!ENTITY unused "x">]><svg width="24" height="16"/>"#),
            Some((24.0, 16.0))
        );
        // 物理単位は 1in = 96px で換算する。
        assert_eq!(
            svg_size(br#"<svg width="1in" height="1in" viewBox="0 0 10 10">"#),
            Some((96.0, 96.0))
        );
        assert_eq!(
            svg_size(br#"<svg width="72pt" height="6pc">"#),
            Some((96.0, 96.0))
        );
        let (w, h) = svg_size(br#"<svg width="2.54cm" height="25.4MM">"#).unwrap();
        assert!((w - 96.0).abs() < 1e-9 && (h - 96.0).abs() < 1e-9);
        assert_eq!(
            svg_size(br#"<svg width="0.5in" viewBox="0 0 10 20">"#),
            Some((48.0, 96.0))
        );
        assert_eq!(
            svg_size(br#"<svg width="1e1" height="2E1px">"#),
            Some((10.0, 20.0))
        );
        assert_eq!(svg_size(b"\x89PNG"), None);
        // コメントの中の <svg は根の要素ではない。
        assert_eq!(
            svg_size(br#"<!-- <svg width="1" height="1"> --><svg width="24" height="16"/>"#),
            Some((24.0, 16.0))
        );
    }

    #[test]
    fn svg_sources_are_judged_by_extension() {
        assert!(is_svg_source("a.svgz"));
        assert!(is_svg_source("C:\\icons\\Save.Svg"));
        assert!(is_svg_source("https://example.com/a.svg#frag"));
        assert!(is_svg_source("file:///C:/icons/logo%23dark.svg"));
        // パスの # と ? は名前の一部で、そこから先を捨てない。
        assert!(is_svg_source(r"C:\icons\logo#dark.svg"));
        assert!(is_svg_source("/icons/what?.svg"));
        assert!(!is_svg_source(r"C:\icons\a.svg#b.png"));
        assert!(!is_svg_source("a.svg.png"));
        assert!(!is_svg_source("https://example.com/svg"));
    }

    #[test]
    fn pixels_must_match_the_size() {
        assert!(check_pixels(2, 1, &[0; 8]).is_ok());
        assert!(check_pixels(2, 1, &[0; 7]).is_err());
        assert!(check_pixels(0, 1, &[]).is_err());
        // 2^31 × 2^31 × 4 は u64 で 0 に回る。空のバッファを通さない。
        assert!(check_pixels(1 << 31, 1 << 31, &[]).is_err());
        assert!(check_pixels(u32::MAX, u32::MAX, &[]).is_err());
    }

    #[test]
    fn checksums_match_known_values() {
        assert_eq!(crc32(b"IEND"), 0xAE42_6082);
        assert_eq!(adler32(b"Wikipedia"), 0x11E6_0398);
    }

    #[test]
    fn png_has_valid_structure() {
        let pixels: Vec<u8> = (0..3 * 2 * 4).map(|i| i as u8).collect();
        let png = encode_png(3, 2, &pixels).unwrap();
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
        // IHDR: 長さ 13、幅 3、高さ 2、8 bit RGBA。
        assert_eq!(&png[8..16], b"\0\0\0\x0dIHDR");
        assert_eq!(&png[16..24], &[0, 0, 0, 3, 0, 0, 0, 2]);
        assert_eq!(&png[24..29], &[8, 6, 0, 0, 0]);
        // 最後は空の IEND とその CRC。
        assert_eq!(&png[png.len() - 12..], b"\0\0\0\0IEND\xAE\x42\x60\x82");
        // IDAT の中身は無圧縮ブロック 1 つで、行ごとにフィルタ 0 が付く。
        let idat_len = u32::from_be_bytes(png[33..37].try_into().unwrap()) as usize;
        assert_eq!(&png[37..41], b"IDAT");
        let zlib = &png[41..41 + idat_len];
        assert_eq!(&zlib[..3], &[0x78, 0x01, 0x01]);
        let raw = &zlib[7..zlib.len() - 4];
        assert_eq!(raw.len(), 2 * (1 + 3 * 4));
        assert_eq!(raw[0], 0);
        assert_eq!(&raw[1..13], &pixels[..12]);
        assert_eq!(raw[13], 0);
    }

    #[test]
    fn large_png_splits_into_stored_blocks() {
        let pixels = vec![7u8; 200 * 100 * 4];
        let png = encode_png(200, 100, &pixels).unwrap();
        let idat_len = u32::from_be_bytes(png[33..37].try_into().unwrap()) as usize;
        let zlib = &png[41..41 + idat_len];
        let raw_len: usize = 100 * (1 + 200 * 4);
        // 65535 バイトごとに 5 バイトの見出しが付く。
        let blocks = raw_len.div_ceil(65_535);
        assert_eq!(zlib.len(), 2 + raw_len + blocks * 5 + 4);
        assert_eq!(zlib[2], 0, "最初のブロックは最後ではない");
    }

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
