//! Images, video and audio.
//!
//! Video and audio are handled directly by WinUI 3's **MediaPlayerElement** and
//! **Windows.Media.Playback.MediaPlayer**.
//!
//! Images use WinUI's standard `Image` and `BitmapImage`, both created directly
//! from the [`naui_winui3`] projection. WinUI does both loading and drawing. Exactly
//! one `Image` is placed in the host `Grid`, and when the location or fitting
//! changes its properties are rewritten instead of recreating it.
//!
//! ## The natural size of an image or a video
//!
//! The natural size of an image or a video is its pixel size, as with GTK's
//! `GtkPicture`, AppKit's `NSImageView` and the Web's `<img>`. It can be made larger
//! or smaller by the `Sizing`. WinUI's `Image` and `MediaPlayerElement` alone would
//! report a different one: with `Uniform` and an unbounded height (inside a vertical
//! `Scroll`), they ask for the available width and the height that keeps the aspect
//! ratio, so a small image in a wide pane grows out of the viewport, and a video
//! does not get shorter than its width allows when the window is made shorter.
//!
//! So the `Image` or `MediaPlayerElement` is put on a `Canvas`, which leaves it out
//! of measuring, and is given the host's size whenever that changes (a [`Frame`]).
//! An empty `Border` beside it is given the pixel size once the image or the video
//! is opened, and that is what the host asks for.
//!
//! **On a `Fill` axis the picture asks for nothing** (for its maximum, if it has
//! one) and fits into the space it is given, as on the other backends. Asking for its pixel size there, a large picture
//! would push a parent that can grow (a vertical `Scroll`) out to its own height.
//! Which axes are `Fill` is looked at again on every `set_sizing`
//! ([`on_sizing_change`](crate::layout::on_sizing_change)), not on `SizeChanged`:
//! from `Auto` to `Fill`, the host keeps the size the pixels gave it, so its actual
//! size does not change and `SizeChanged` never comes.
//!
//! ## Playback state notifications and threads
//!
//! `MediaPlaybackSession`'s `PlaybackStateChanged` and `MediaPlayer`'s
//! `MediaEnded` **fire on the playback pipeline's thread, not the UI thread**.
//! Calling Rust closures directly from there would break structures that assume the
//! UI thread, so they are handed back to the UI thread with
//! `DispatcherQueue::TryEnqueue` before notifying.
//!
//! An `Rc` cannot be carried across threads for that (its reference count is not
//! atomic). Widgets are registered by number in a list on the UI thread side, and
//! **only the number and the state cross threads**. The video's pixel size
//! (`NaturalVideoSizeChanged`) comes back the same way.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ops::Deref;
use std::rc::{Rc, Weak};

use naui_core::media::is_url;
use naui_core::{Fit, PlaybackState, Result};
use naui_winui3::Microsoft::UI::Dispatching::{DispatcherQueue, DispatcherQueueHandler};
use naui_winui3::Microsoft::UI::Xaml::Automation::AutomationProperties;
use naui_winui3::Microsoft::UI::Xaml::Controls::{
    Border, Canvas, Grid, Image as XamlImage, MediaPlayerElement, UIElementCollection,
};
use naui_winui3::Microsoft::UI::Xaml::Media::Imaging::BitmapImage;
use naui_winui3::Microsoft::UI::Xaml::Media::Stretch;
use naui_winui3::Microsoft::UI::Xaml::{
    FrameworkElement, RoutedEventHandler, SizeChangedEventHandler, UIElement,
};
use windows::Foundation::{TimeSpan, TypedEventHandler, Uri};
use windows::Media::Core::MediaSource;
use windows::Media::Playback::{
    IMediaPlaybackSource, MediaPlaybackSession, MediaPlaybackState, MediaPlayer,
    MediaPlayerFailedEventArgs,
};
use windows::Storage::StorageFile;
use windows_core::{IInspectable, Interface, HSTRING};

use crate::to_error;
use crate::ui_thread::UiThreadCell;
use crate::widgets::impl_widget;
use crate::widgets::Widget;
use crate::Slot;

/// Ticks per second of a TimeSpan (in 100-nanosecond units).
const TICKS_PER_SECOND: f64 = 10_000_000.0;

/// Turns the fitting into XAML's `Stretch`.
fn stretch(fit: Fit) -> Stretch {
    match fit {
        Fit::Contain => Stretch::Uniform,
        Fit::Cover => Stretch::UniformToFill,
        Fit::Fill => Stretch::Fill,
        Fit::None => Stretch::None,
    }
}

/// Turns a media location into the string passed to `Windows.Foundation.Uri`.
///
/// Paths become `file://` with [`file_uri`]. URLs are passed as is, except that for
/// `file:` only, [`unescape_non_ascii`] turns non-ASCII percent-encoding back into
/// raw characters, because URLs made by [`naui_core::media::file_url`] and
/// standards-conforming ones like `file:///C:/%E5%86%99.png` written by users
/// cannot be read either, for the same reason as [`file_uri`].
fn source_uri(source: &str) -> String {
    if !is_url(source) {
        file_uri(source)
    } else if source
        .get(..5)
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case("file:"))
    {
        unescape_non_ascii(source)
    } else {
        source.to_string()
    }
}

/// Turns runs of `%XX` representing non-ASCII UTF-8 back into raw characters.
///
/// ASCII encodings such as `%20` and `%23` are left as they are, so as not to change
/// their meaning as URL delimiters. Runs that cannot be read as UTF-8 are left alone
/// too.
fn unescape_non_ascii(url: &str) -> String {
    let bytes = url.as_bytes();
    let mut out = String::with_capacity(url.len());
    let mut i = 0;
    while i < bytes.len() {
        // `%80`〜`%FF` が続くあいだを 1 つの塊として集める。
        let start = i;
        let mut run = Vec::new();
        while let Some(byte) = url
            .get(i..i + 3)
            .and_then(|escape| escape.strip_prefix('%'))
            .and_then(|hex| u8::from_str_radix(hex, 16).ok())
            .filter(|byte| *byte >= 0x80)
        {
            run.push(byte);
            i += 3;
        }
        if run.is_empty() {
            let c = url[i..].chars().next().expect("i is on a char boundary");
            out.push(c);
            i += c.len_utf8();
        } else {
            match String::from_utf8(run) {
                Ok(text) => out.push_str(&text),
                Err(_) => out.push_str(&url[start..i]),
            }
        }
    }
    out
}

/// Turns a local file path into a `file://` that `Windows.Foundation.Uri` can read.
///
/// Unlike [`naui_core::media::file_url`], **non-ASCII is placed raw, not
/// encoded**. `Windows.Foundation.Uri` does not decode `%E5%86%99` as UTF-8 but turns
/// it back one byte per character (`å†™`), so a path containing Japanese becomes a
/// different path, and both `BitmapImage` and `MediaSource` give up loading without
/// an error. Raw non-ASCII is accepted as an IRI, so only ASCII with meaning in URLs
/// (spaces, `%`, `#`, `?` and so on) is encoded.
fn file_uri(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    // UNC パスの先頭 `//` は「ホスト名が続く」の意味なので、`file:` を足すだけ。
    let mut uri = String::from(if normalized.starts_with("//") {
        "file:"
    } else if normalized.starts_with('/') {
        "file://"
    } else {
        "file:///"
    });
    for c in normalized.chars() {
        if !c.is_ascii()
            || c.is_ascii_alphanumeric()
            || matches!(c, '-' | '.' | '_' | '~' | '/' | ':' | '@')
        {
            uri.push(c);
        } else {
            uri.push_str(&format!("%{:02X}", c as u8));
        }
    }
    uri
}

// ------------------------------------------------------------------ Frame

/// What a host `Grid` holds so that an image or a video asks for its pixel size
/// rather than WinUI's (see the module documentation).
struct Frame {
    /// An empty element as large as the picture's pixels, so that the host asks for
    /// the natural size. Sized automatically (zero) until the picture is opened.
    natural: Border,
    /// Holds the picture without letting it take part in measuring.
    stage: Canvas,
}

impl Frame {
    /// Builds the parts and puts them into `host`.
    fn new(host: &Grid) -> Result<Self> {
        let natural = Border::new().map_err(|e| to_error("create the size element", e))?;
        let stage = Canvas::new().map_err(|e| to_error("create the stage", e))?;
        natural
            .SetIsHitTestVisible(false)
            .map_err(|e| to_error("set up the size element", e))?;
        host.Children()
            .and_then(|children| {
                children.Append(&natural.cast::<UIElement>()?)?;
                children.Append(&stage.cast::<UIElement>()?)
            })
            .map_err(|e| to_error("build the host", e))?;
        Ok(Self { natural, stage })
    }

    /// Gives the picture on the `Canvas` the host's size.
    fn fit(&self, host: &Grid, picture: &FrameworkElement) {
        let _ = picture.SetWidth(host.ActualWidth().unwrap_or(0.0));
        let _ = picture.SetHeight(host.ActualHeight().unwrap_or(0.0));
    }

    /// Makes the host ask for the picture's pixel size (`None` until it is opened),
    /// except on `Fill` axes.
    ///
    /// A `Fill` axis asks for its maximum if it has one, since the maximum doubles as
    /// the size wanted under normal conditions (as on the other backends), and for
    /// nothing otherwise. Asking for nothing with a maximum too, the picture
    /// collapsed to zero where the natural size decides (an `Auto` row, for example).
    fn ask_for(&self, host: &Grid, pixels: Option<(u32, u32)>) {
        let Ok(host) = host.cast::<FrameworkElement>() else {
            return;
        };
        // WinUI では NaN が「中身に合わせる」を表す。空の Border なので 0 になる。
        let natural = |horizontal: bool, pixels: Option<u32>| {
            if crate::layout::wants_fill(&host, horizontal) {
                let max = if horizontal {
                    host.MaxWidth()
                } else {
                    host.MaxHeight()
                };
                max.ok().filter(|max| max.is_finite()).unwrap_or(f64::NAN)
            } else {
                pixels.map_or(f64::NAN, f64::from)
            }
        };
        let _ = self
            .natural
            .SetWidth(natural(true, pixels.map(|(width, _)| width)));
        let _ = self
            .natural
            .SetHeight(natural(false, pixels.map(|(_, height)| height)));
    }
}

// ------------------------------------------------------------------ Image

struct ImageInner {
    /// The host that receives the size specification. Holds the [`Frame`].
    native: Grid,
    frame: Frame,
    /// Whether the image shown has been opened (its pixel size is known).
    opened: Cell<bool>,
    /// The WinUI `Image` shown. Removed from the frame's `stage` when the location is
    /// empty.
    image: XamlImage,
    source: RefCell<String>,
    fit: Cell<Fit>,
    alt: RefCell<String>,
}

/// Image display (WinUI's `Image`).
///
/// WinUI loads it asynchronously, so right after [`set_source`](Self::set_source) it
/// is not shown yet.
#[derive(Clone)]
pub struct Image(Rc<ImageInner>);
impl_widget!(Image, native);

impl Image {
    pub(crate) fn new(source: &str) -> Result<Self> {
        let native = Grid::new().map_err(|e| to_error("create the Image host", e))?;
        let frame = Frame::new(&native)?;
        let image = XamlImage::new().map_err(|e| to_error("create the Image", e))?;
        image
            .SetStretch(stretch(Fit::default()))
            .map_err(|e| to_error("set the Image alignment", e))?;
        let this = Self(Rc::new(ImageInner {
            native,
            frame,
            opened: Cell::new(false),
            image,
            source: RefCell::new(String::new()),
            fit: Cell::new(Fit::default()),
            alt: RefCell::new(String::new()),
        }));
        this.track_size()?;
        this.set_source(source);
        Ok(this)
    }

    /// Keeps the `Image` as large as the host, and the host's natural size as large
    /// as the image's pixels.
    fn track_size(&self) -> Result<()> {
        let target = UiThreadCell::new(Rc::downgrade(&self.0));
        let resized = SizeChangedEventHandler::new(move |_, _| {
            if let Some(inner) = target.try_with_mut(|weak| weak.upgrade()).flatten() {
                Image(inner).fit_to_host();
            }
            Ok(())
        });
        self.0
            .native
            .SizeChanged(&resized)
            .map_err(|e| to_error("subscribe to Image size changes", e))?;

        let target = UiThreadCell::new(Rc::downgrade(&self.0));
        let opened = RoutedEventHandler::new(move |_, _| {
            if let Some(inner) = target.try_with_mut(|weak| weak.upgrade()).flatten() {
                inner.opened.set(true);
                Image(inner).take_natural_size();
            }
            Ok(())
        });
        self.0
            .image
            .ImageOpened(&opened)
            .map_err(|e| to_error("subscribe to Image loading", e))?;

        // `Fill` かどうかで求める大きさが変わるので、`set_sizing` のたびに求め直す。
        // 実寸が変わらない切り替えもあるので、SizeChanged は待たない。
        // 画像が開く前でも、上限付きの `Fill` は上限を求める。
        let target = Rc::downgrade(&self.0);
        crate::layout::on_sizing_change(&self.native_element(), move || {
            if let Some(inner) = target.upgrade() {
                Image(inner).take_natural_size();
            }
        });
        Ok(())
    }

    /// Gives the `Image` on the `Canvas` the host's size.
    fn fit_to_host(&self) {
        if let Ok(image) = self.0.image.cast::<FrameworkElement>() {
            self.0.frame.fit(&self.0.native, &image);
        }
    }

    /// Makes the opened image's pixel size the host's natural size, except on `Fill`
    /// axes (see [`Frame::ask_for`]). Before it is opened, no pixel size is asked
    /// for (not the previous image's either).
    fn take_natural_size(&self) {
        let pixels = |value: windows_core::Result<i32>| value.unwrap_or(0).max(0) as u32;
        let size = self
            .0
            .image
            .Source()
            .ok()
            .and_then(|source| source.cast::<BitmapImage>().ok())
            .filter(|_| self.0.opened.get())
            .map(|bitmap| (pixels(bitmap.PixelWidth()), pixels(bitmap.PixelHeight())));
        self.0.frame.ask_for(&self.0.native, size);
    }

    /// Forgets the natural size of the image that was shown.
    fn forget_natural_size(&self) {
        self.0.opened.set(false);
        self.take_natural_size();
    }

    /// The location currently specified (exactly the string that was passed).
    pub fn source(&self) -> String {
        self.0.source.borrow().clone()
    }

    /// The location of the image to show. Either a file path or a URL.
    ///
    /// Passing an empty string removes the image.
    pub fn set_source(&self, source: &str) {
        *self.0.source.borrow_mut() = source.to_string();
        self.apply_source();
    }

    /// Whether loading could be requested (whether the element could be built).
    ///
    /// WinUI's `Image` does not report load completion back to Rust, so **this returns
    /// "whether an element to show is in place", not "whether loading has finished"**.
    pub fn is_loaded(&self) -> bool {
        self.0
            .frame
            .stage
            .Children()
            .and_then(|children| children.Size())
            .is_ok_and(|size| size > 0)
    }

    /// How it is fitted into the display area (the `Image`'s `Stretch`).
    pub fn set_fit(&self, fit: Fit) {
        self.0.fit.set(fit);
        let _ = self.0.image.SetStretch(stretch(fit));
    }

    /// Text describing the image's content. Read aloud by Narrator.
    pub fn set_alt(&self, text: &str) {
        *self.0.alt.borrow_mut() = text.to_string();
        let _ = AutomationProperties::SetName(&self.0.image, &HSTRING::from(text));
    }

    /// Removes the image from the `Canvas` and drops the load request too.
    ///
    /// Gone through both when the location is empty and when loading could not be
    /// requested. This putting in and out is what [`is_loaded`](Self::is_loaded) looks
    /// at.
    fn take_down(&self, children: &UIElementCollection) {
        let _ = children.Clear();
        let _ = self.0.image.SetSource(None);
    }

    /// Passes the current location to the `Image` and matches putting it in and out of
    /// the `Canvas`.
    ///
    /// When the location is empty or loading could not be requested, it is removed with
    /// [`take_down`](Self::take_down).
    fn apply_source(&self) {
        let Ok(children) = self.0.frame.stage.Children() else {
            return;
        };
        // 新しい画像が開くまでは、前の画像の大きさを求めない。
        self.forget_natural_size();
        let source = self.0.source.borrow();
        if source.is_empty() {
            self.take_down(&children);
            return;
        }

        // WinUI はファイルパスではなく URI を要求する。
        let uri = match Uri::CreateUri(&HSTRING::from(source_uri(&source))) {
            Ok(uri) => uri,
            Err(error) => {
                eprintln!("naui-windows: cannot turn the Image location into a URI: {error}");
                self.take_down(&children);
                return;
            }
        };
        let bitmap = match BitmapImage::CreateInstanceWithUriSource(&uri) {
            Ok(bitmap) => bitmap,
            Err(error) => {
                eprintln!("naui-windows: failed to start loading the Image: {error}");
                self.take_down(&children);
                return;
            }
        };
        if let Err(error) = self.0.image.SetSource(&bitmap) {
            eprintln!("naui-windows: failed to replace the Image: {error}");
            self.take_down(&children);
            return;
        }
        if children.Size().is_ok_and(|size| size > 0) {
            return;
        }
        let Ok(element) = self.0.image.cast::<UIElement>() else {
            eprintln!("naui-windows: failed to convert to the Image element");
            return;
        };
        if let Err(error) = children.Append(&element) {
            eprintln!("naui-windows: failed to place the Image: {error}");
            return;
        }
        // ホストの大きさが決まったあとで置いたときは SizeChanged が来ないので、
        // ここで合わせる。
        self.fit_to_host();
    }
}

// --------------------------------------------------------------- 再生の中身

/// The playback object shared by video and audio.
struct MediaInner {
    native: MediaPlayerElement,
    player: MediaPlayer,
    /// Its own number in the list on the UI thread side.
    id: u64,
    source: RefCell<String>,
    looping: Cell<bool>,
    autoplay: Cell<bool>,
    state: Cell<PlaybackState>,
    callback: RefCell<Slot<dyn FnMut(PlaybackState)>>,
    position_callback: RefCell<Slot<dyn FnMut(f64)>>,
    /// Told the video's pixel size (width, height) when it changes, and `(0, 0)`
    /// when the location changes. Set by [`Video`] only.
    natural_size_callback: RefCell<Slot<dyn FnMut(u32, u32)>>,
}

thread_local! {
    /// The list of playback widgets alive on the UI thread.
    ///
    /// Used to look up the object by number when notifications from the playback
    /// pipeline's thread are received again on the UI thread. They are weak references,
    /// so nothing happens if the widget has already been freed.
    static PLAYBACKS: RefCell<HashMap<u64, Weak<MediaInner>>> =
        RefCell::new(HashMap::new());

    /// The next number to hand out.
    static NEXT_ID: Cell<u64> = const { Cell::new(0) };
}

fn next_id() -> u64 {
    NEXT_ID.with(|id| {
        id.set(id.get() + 1);
        id.get()
    })
}

/// Delivers the state to the widget with the number, on the UI thread.
fn deliver(id: u64, state: PlaybackState) {
    if let Some(inner) = lookup(id) {
        inner.emit(state);
    }
}

/// Delivers the playback position to the widget with the number, on the UI thread.
fn deliver_position(id: u64, seconds: f64) {
    if let Some(inner) = lookup(id) {
        inner.emit_position(seconds);
    }
}

/// Delivers the video's pixel size to the widget with the number, on the UI thread.
fn deliver_natural_size(id: u64, width: u32, height: u32) {
    if let Some(inner) = lookup(id) {
        inner.emit_natural_size(width, height);
    }
}

fn lookup(id: u64) -> Option<Rc<MediaInner>> {
    PLAYBACKS.with(|map| map.borrow().get(&id).and_then(Weak::upgrade))
}

/// Maps WinRT's playback state onto naui's playback state.
fn map_state(state: MediaPlaybackState) -> PlaybackState {
    match state {
        MediaPlaybackState::Playing => PlaybackState::Playing,
        MediaPlaybackState::Paused => PlaybackState::Paused,
        MediaPlaybackState::Opening | MediaPlaybackState::Buffering => PlaybackState::Buffering,
        _ => PlaybackState::Idle,
    }
}

fn to_seconds(span: TimeSpan) -> f64 {
    span.Duration as f64 / TICKS_PER_SECOND
}

fn to_time_span(seconds: f64) -> TimeSpan {
    TimeSpan {
        Duration: (seconds.max(0.0) * TICKS_PER_SECOND) as i64,
    }
}

impl MediaInner {
    fn new() -> Result<Rc<Self>> {
        let native =
            MediaPlayerElement::new().map_err(|e| to_error("create the MediaPlayerElement", e))?;
        let player = MediaPlayer::new().map_err(|e| to_error("create the MediaPlayer", e))?;
        // Windows App SDK 2.2.4 の標準 MediaTransportControls は、
        // MediaPlayerElement を visual tree へ追加したときに XAML 内部
        // 例外 (0xc000027b) を起こす環境がある。操作 UI はアプリ側で
        // 用意するため、ここでは常に無効にしておく。
        native
            .SetAreTransportControlsEnabled(false)
            .map_err(|e| to_error("set the transport controls", e))?;
        native
            .SetMediaPlayer(&player)
            .map_err(|e| to_error("assign the MediaPlayer", e))?;
        // 場所を指定するまで勝手に鳴らさない。
        player
            .SetAutoPlay(false)
            .map_err(|e| to_error("set autoplay", e))?;

        let this = Rc::new(Self {
            native,
            player,
            id: next_id(),
            source: RefCell::new(String::new()),
            looping: Cell::new(false),
            autoplay: Cell::new(false),
            state: Cell::new(PlaybackState::Idle),
            callback: RefCell::new(None),
            position_callback: RefCell::new(None),
            natural_size_callback: RefCell::new(None),
        });
        PLAYBACKS.with(|map| map.borrow_mut().insert(this.id, Rc::downgrade(&this)));
        this.subscribe()?;
        Ok(this)
    }

    /// Makes notifications from the playback pipeline receivable via the UI thread.
    fn subscribe(&self) -> Result<()> {
        let queue = DispatcherQueue::GetForCurrentThread()
            .map_err(|e| to_error("get the UI thread's DispatcherQueue", e))?;
        let session = self
            .player
            .PlaybackSession()
            .map_err(|e| to_error("get the playback session", e))?;

        // スレッドをまたぐのは番号と状態だけ。Rc は渡さない。
        let id = self.id;
        let state_queue = queue.clone();
        let handler =
            TypedEventHandler::<MediaPlaybackSession, IInspectable>::new(move |session, _args| {
                let Ok(session) = session.ok() else {
                    return Ok(());
                };
                let state = match session.PlaybackState() {
                    Ok(state) => map_state(state),
                    Err(error) => {
                        eprintln!("naui-windows: failed to get the playback state: {error}");
                        return Ok(());
                    }
                };
                let _ = state_queue.TryEnqueue(&DispatcherQueueHandler::new(move || {
                    deliver(id, state);
                    Ok(())
                }));
                Ok(())
            });
        session
            .PlaybackStateChanged(&handler)
            .map_err(|e| to_error("subscribe to playback state changes", e))?;

        // 再生位置も同じ経路で UI スレッドへ渡す。運ぶのは番号と秒数だけ。
        let position_queue = queue.clone();
        let position =
            TypedEventHandler::<MediaPlaybackSession, IInspectable>::new(move |session, _args| {
                let Ok(session) = session.ok() else {
                    return Ok(());
                };
                let seconds = match session.Position() {
                    Ok(position) => to_seconds(position),
                    Err(error) => {
                        eprintln!("naui-windows: failed to get the playback position: {error}");
                        return Ok(());
                    }
                };
                let _ = position_queue.TryEnqueue(&DispatcherQueueHandler::new(move || {
                    deliver_position(id, seconds);
                    Ok(())
                }));
                Ok(())
            });
        session
            .PositionChanged(&position)
            .map_err(|e| to_error("subscribe to playback position changes", e))?;

        // 動画の画素の大きさも同じ経路で渡す。運ぶのは番号と大きさだけ。
        let size_queue = queue.clone();
        let natural_size =
            TypedEventHandler::<MediaPlaybackSession, IInspectable>::new(move |session, _args| {
                let Ok(session) = session.ok() else {
                    return Ok(());
                };
                let width = session.NaturalVideoWidth().unwrap_or(0);
                let height = session.NaturalVideoHeight().unwrap_or(0);
                let _ = size_queue.TryEnqueue(&DispatcherQueueHandler::new(move || {
                    deliver_natural_size(id, width, height);
                    Ok(())
                }));
                Ok(())
            });
        session
            .NaturalVideoSizeChanged(&natural_size)
            .map_err(|e| to_error("subscribe to video size changes", e))?;

        // MediaPlaybackState に「最後まで再生した」は無いので、別の通知で補う。
        let ended = TypedEventHandler::<MediaPlayer, IInspectable>::new(move |_player, _args| {
            let _ = queue.TryEnqueue(&DispatcherQueueHandler::new(move || {
                // 繰り返し再生では MediaPlayer 自身が先頭へ戻すため、
                // 「終わった」とは扱わない。
                let looping = PLAYBACKS.with(|map| {
                    map.borrow()
                        .get(&id)
                        .and_then(Weak::upgrade)
                        .is_some_and(|inner| inner.looping.get())
                });
                if !looping {
                    deliver(id, PlaybackState::Ended);
                }
                Ok(())
            }));
            Ok(())
        });
        self.player
            .MediaEnded(&ended)
            .map_err(|e| to_error("subscribe to playback ending", e))?;

        // MediaPlayer の失敗イベントが持つ HRESULT を記録する。これを
        // 登録しておかないと、WinRT の非同期メディアエラーが
        // 0xc000027b (stowed exception) としてしか見えないことがある。
        let failed = TypedEventHandler::<MediaPlayer, MediaPlayerFailedEventArgs>::new(
            move |_player, args| {
                let Ok(args) = args.ok() else {
                    return Ok(());
                };
                let error = args
                    .Error()
                    .map(|error| format!("{error:?}"))
                    .unwrap_or_else(|error| format!("unavailable: {error}"));
                let code = args
                    .ExtendedErrorCode()
                    .map(|code| format!("{code:?}"))
                    .unwrap_or_else(|error| format!("unavailable: {error}"));
                let message = args
                    .ErrorMessage()
                    .map(|message| message.to_string_lossy())
                    .unwrap_or_default();
                eprintln!(
                    "naui-windows: MediaPlayer playback failed: error={error}, extended_error={code}, message={message}"
                );
                Ok(())
            },
        );
        self.player
            .MediaFailed(&failed)
            .map_err(|e| to_error("subscribe to playback failures", e))?;
        Ok(())
    }

    fn set_uri_source(&self, source: &str) -> windows_core::Result<()> {
        Uri::CreateUri(&HSTRING::from(source))
            .and_then(|uri| MediaSource::CreateFromUri(&uri))
            .and_then(|media_source| media_source.cast::<IMediaPlaybackSource>())
            // MediaPlayerElement にはこの player を SetMediaPlayer で割り当て
            // ているため、再生元は Element.Source ではなく player.Source に
            // 設定する。両方へ設定すると WinUI の内部状態が二重になり、
            // メディア要素のテンプレート適用時に stowed exception になる。
            .and_then(|media_source| self.player.SetSource(&media_source))
    }

    fn set_source(&self, source: &str) {
        *self.source.borrow_mut() = source.to_string();
        if source.is_empty() {
            let _ = self.player.SetSource(None);
        } else if is_url(source) {
            if let Err(error) = self.set_uri_source(&source_uri(source)) {
                eprintln!("naui-windows: failed to set the media URL ({source}): {error}");
            }
        } else {
            // ファイル選択で得たパスは、file:// URI に変換して再生するよりも
            // StorageFile として渡す方が、ユーザーが選択したファイルへの
            // アクセス権を Windows のメディアパイプラインへ正しく引き継げる。
            let result = StorageFile::GetFileFromPathAsync(&HSTRING::from(source))
                .and_then(|operation| operation.join())
                .and_then(|file| MediaSource::CreateFromStorageFile(&file))
                .and_then(|media_source| media_source.cast::<IMediaPlaybackSource>())
                .and_then(|media_source| self.player.SetSource(&media_source));
            if let Err(error) = result {
                eprintln!("naui-windows: failed to set the media file ({source}): {error}");
                // 相対パスなど、StorageFile として開けない入力は
                // file:// URI も試す。
                if let Err(fallback_error) = self.set_uri_source(&file_uri(source)) {
                    eprintln!(
                        "naui-windows: the media URL fallback failed too ({source}): {fallback_error}"
                    );
                }
            }
        }
        self.emit(PlaybackState::Idle);
        // 新しい動画が開くまでは、前の動画の大きさを求めない。
        self.emit_natural_size(0, 0);
        if self.autoplay.get() && !source.is_empty() {
            self.play();
        }
    }

    fn play(&self) {
        // 最後まで再生し終えた後の play は、先頭へ戻してから鳴らす。
        if self.state.get() == PlaybackState::Ended {
            self.seek(0.0);
        }
        let _ = self.player.Play();
    }

    fn seek(&self, seconds: f64) {
        if let Ok(session) = self.player.PlaybackSession() {
            let _ = session.SetPosition(to_time_span(seconds));
        }
        if self.state.get() == PlaybackState::Ended {
            self.emit(PlaybackState::Paused);
        }
    }

    /// Notifies the playback position.
    fn emit_position(&self, seconds: f64) {
        let Some(mut f) = self.position_callback.borrow_mut().take() else {
            return;
        };
        f(seconds);
        let mut slot = self.position_callback.borrow_mut();
        if slot.is_none() {
            *slot = Some(f);
        }
    }

    /// Notifies the video's pixel size (`(0, 0)` while none is known).
    fn emit_natural_size(&self, width: u32, height: u32) {
        let Some(mut f) = self.natural_size_callback.borrow_mut().take() else {
            return;
        };
        f(width, height);
        let mut slot = self.natural_size_callback.borrow_mut();
        if slot.is_none() {
            *slot = Some(f);
        }
    }

    /// If the state changed, records and notifies it. Not called for repeats of the
    /// same state.
    fn emit(&self, state: PlaybackState) {
        if self.state.get() == state {
            return;
        }
        self.state.set(state);
        // クロージャの中から同じウィジェットを触っても二重借用にならないよう、
        // 呼び出しの間だけ取り出す。
        let Some(mut f) = self.callback.borrow_mut().take() else {
            return;
        };
        f(state);
        let mut slot = self.callback.borrow_mut();
        if slot.is_none() {
            *slot = Some(f);
        }
    }
}

impl Drop for MediaInner {
    fn drop(&mut self) {
        PLAYBACKS.with(|map| map.borrow_mut().remove(&self.id));
        // MediaPlayer は明示的に閉じないと、再生パイプラインが残る。
        let _ = self.player.Close();
    }
}

/// Adds the playback API shared by video and audio.
macro_rules! impl_playback {
    ($t:ty) => {
        impl $t {
            /// The location currently specified (exactly the string that was passed).
            pub fn source(&self) -> String {
                self.0.source.borrow().clone()
            }

            /// The location of the media to play. Either a file path or a URL.
            ///
            /// Calling it stops playback, and the state returns to [`PlaybackState::Idle`].
            /// Passing an empty string removes the media.
            pub fn set_source(&self, source: &str) {
                self.0.set_source(source);
            }

            /// Starts playback.
            ///
            /// Called after playing to the end, it goes back to the start before playing.
            pub fn play(&self) {
                self.0.play();
            }

            /// Pauses.
            pub fn pause(&self) {
                let _ = self.0.player.Pause();
            }

            /// The current playback state.
            pub fn state(&self) -> PlaybackState {
                self.0.state.get()
            }

            /// Whether it is playing.
            pub fn is_playing(&self) -> bool {
                self.0.state.get().is_playing()
            }

            /// Sets the playback position in seconds. Negative values are treated as the start.
            pub fn seek(&self, seconds: f64) {
                self.0.seek(seconds);
            }

            /// The current playback position (seconds).
            pub fn position(&self) -> f64 {
                self.0
                    .player
                    .PlaybackSession()
                    .and_then(|session| session.Position())
                    .map(to_seconds)
                    .unwrap_or(0.0)
            }

            /// The length of the media (seconds). **`None` until loading finishes.**
            ///
            /// Also returns `None` for streams whose length is undefined (live streams and so
            /// on).
            pub fn duration(&self) -> Option<f64> {
                let duration = self
                    .0
                    .player
                    .PlaybackSession()
                    .and_then(|session| session.NaturalDuration())
                    .map(to_seconds)
                    .ok()?;
                // 読み込み前と長さの決まらない配信は 0 で返る。
                (duration > 0.0).then_some(duration)
            }

            /// The volume (0.0..=1.0). Out-of-range values are clamped.
            pub fn set_volume(&self, volume: f64) {
                let _ = self.0.player.SetVolume(volume.clamp(0.0, 1.0));
            }

            pub fn volume(&self) -> f64 {
                self.0.player.Volume().unwrap_or(0.0)
            }

            /// Mutes it. Only the sound stops; the volume value is kept.
            pub fn set_muted(&self, muted: bool) {
                let _ = self.0.player.SetIsMuted(muted);
            }

            pub fn is_muted(&self) -> bool {
                self.0.player.IsMuted().unwrap_or(false)
            }

            /// After playing to the end, goes back to the start and repeats.
            pub fn set_loop(&self, looping: bool) {
                self.0.looping.set(looping);
                let _ = self.0.player.SetIsLoopingEnabled(looping);
            }

            pub fn is_loop(&self) -> bool {
                self.0.looping.get()
            }

            /// Starts playing automatically when media is specified.
            ///
            /// If a location is already specified and it has never played, playback starts with
            /// this call.
            pub fn set_autoplay(&self, autoplay: bool) {
                self.0.autoplay.set(autoplay);
                let _ = self.0.player.SetAutoPlay(autoplay);
                if autoplay
                    && self.0.state.get() == PlaybackState::Idle
                    && !self.0.source.borrow().is_empty()
                {
                    self.0.play();
                }
            }

            /// Whether to use WinUI's standard playback bar.
            ///
            /// With Windows App SDK 2.2.4, enabling the standard bar makes the process exit with
            /// `0xc000027b` when the `MediaPlayerElement` is added to the visual tree, so this
            /// backend always disables it. Use it together with a playback UI on the app side.
            pub fn set_controls(&self, controls: bool) {
                if controls {
                    eprintln!(
                        "naui-windows: WinUI's built-in transport controls are disabled for safety"
                    );
                }
                let _ = self.0.native.SetAreTransportControlsEnabled(false);
            }

            /// Called when the playback state changes. Setting it again removes the previous
            /// one.
            ///
            /// It arrives not only when the app calls [`play`](Self::play) but **also when the
            /// user operates the playback bar**.
            pub fn on_state_change(&self, f: impl FnMut(PlaybackState) + 'static) {
                *self.0.callback.borrow_mut() = Some(Box::new(f));
            }

            /// Called with the position (seconds) every time the playback position advances.
            ///
            /// For keeping a seek bar in step with playback. The native side decides the
            /// interval, but it is about four times a second, and it also arrives right after
            /// seeking. **It is not called while not playing.**
            pub fn on_position_change(&self, f: impl FnMut(f64) + 'static) {
                *self.0.position_callback.borrow_mut() = Some(Box::new(f));
            }
        }
    };
}

// ------------------------------------------------------------------ Video

struct VideoInner {
    /// The host that receives the size specification. Holds the [`Frame`], whose
    /// `stage` holds the `MediaPlayerElement`.
    host: Grid,
    frame: Frame,
    /// The video's pixel size, `(0, 0)` while none is known.
    natural_size: Cell<(u32, u32)>,
    media: Rc<MediaInner>,
}

/// The playback methods ([`impl_playback`]) reach the shared playback object
/// through this.
impl Deref for VideoInner {
    type Target = MediaInner;

    fn deref(&self) -> &MediaInner {
        &self.media
    }
}

/// Video (MediaPlayerElement).
#[derive(Clone)]
pub struct Video(Rc<VideoInner>);
impl_widget!(Video, host);
impl_playback!(Video);

impl Video {
    pub(crate) fn new(source: &str) -> Result<Self> {
        let media = MediaInner::new()?;
        let host = Grid::new().map_err(|e| to_error("create the Video host", e))?;
        let frame = Frame::new(&host)?;
        frame
            .stage
            .Children()
            .and_then(|children| children.Append(&media.native.cast::<UIElement>()?))
            .map_err(|e| to_error("place the MediaPlayerElement", e))?;
        let this = Self(Rc::new(VideoInner {
            host,
            frame,
            natural_size: Cell::new((0, 0)),
            media,
        }));
        this.track_size()?;
        this.set_fit(Fit::default());
        this.set_source(source);
        Ok(this)
    }

    /// How the picture is fitted (the MediaPlayerElement's `Stretch`).
    pub fn set_fit(&self, fit: Fit) {
        let _ = self.0.native.SetStretch(stretch(fit));
    }

    /// Keeps the `MediaPlayerElement` as large as the host, and the host's natural
    /// size as large as the video's pixels.
    fn track_size(&self) -> Result<()> {
        let target = UiThreadCell::new(Rc::downgrade(&self.0));
        let resized = SizeChangedEventHandler::new(move |_, _| {
            if let Some(inner) = target.try_with_mut(|weak| weak.upgrade()).flatten() {
                Video(inner).fit_to_host();
            }
            Ok(())
        });
        self.0
            .host
            .SizeChanged(&resized)
            .map_err(|e| to_error("subscribe to Video size changes", e))?;

        let target = Rc::downgrade(&self.0);
        *self.0.natural_size_callback.borrow_mut() = Some(Box::new(move |width, height| {
            if let Some(inner) = target.upgrade() {
                inner.natural_size.set((width, height));
                Video(inner).take_natural_size();
            }
        }));

        // `Fill` かどうかで求める大きさが変わるので、`set_sizing` のたびに求め直す。
        // 実寸が変わらない切り替えもあるので、SizeChanged は待たない。
        let target = Rc::downgrade(&self.0);
        crate::layout::on_sizing_change(&self.native_element(), move || {
            if let Some(inner) = target.upgrade() {
                Video(inner).take_natural_size();
            }
        });
        Ok(())
    }

    /// Gives the `MediaPlayerElement` on the `Canvas` the host's size.
    fn fit_to_host(&self) {
        if let Ok(element) = self.0.native.cast::<FrameworkElement>() {
            self.0.frame.fit(&self.0.host, &element);
        }
    }

    /// Makes the video's pixel size the host's natural size, except on `Fill` axes
    /// (see the module documentation).
    fn take_natural_size(&self) {
        let size = match self.0.natural_size.get() {
            (0, _) | (_, 0) => None,
            size => Some(size),
        };
        self.0.frame.ask_for(&self.0.host, size);
    }
}

// ------------------------------------------------------------------ Audio

/// Audio (MediaPlayerElement). There is no video track, so only the playback bar is
/// visible.
#[derive(Clone)]
pub struct Audio(Rc<MediaInner>);
impl_widget!(Audio, native);
impl_playback!(Audio);

impl Audio {
    pub(crate) fn new(source: &str) -> Result<Self> {
        let this = Self(MediaInner::new()?);
        this.set_source(source);
        Ok(this)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_uri_keeps_non_ascii_raw() {
        assert_eq!(
            file_uri("C:/Users/太郎/写真.png"),
            "file:///C:/Users/太郎/写真.png"
        );
    }

    #[test]
    fn file_uri_encodes_reserved_ascii() {
        assert_eq!(
            file_uri("C:/a b/c%d#e?.png"),
            "file:///C:/a%20b/c%25d%23e%3F.png"
        );
    }

    #[test]
    fn file_uri_normalises_separators() {
        assert_eq!(file_uri(r"C:\Users\me\a.png"), "file:///C:/Users/me/a.png");
        assert_eq!(
            file_uri(r"\\server\share\a.png"),
            "file://server/share/a.png"
        );
        assert_eq!(file_uri("/tmp/a.png"), "file:///tmp/a.png");
    }

    #[test]
    fn source_uri_passes_urls_through() {
        let url = "https://example.com/%E5%86%99.png";
        assert_eq!(source_uri(url), url);
    }

    /// URLs made by core's `file_url` are also fixed into a form Windows can read.
    #[test]
    fn source_uri_unescapes_utf8_in_file_urls() {
        let url = naui_core::media::file_url("C:/Users/太郎/写真 #1.png");
        assert_eq!(source_uri(&url), "file:///C:/Users/太郎/写真%20%231.png");
        assert_eq!(source_uri("FILE:///C:/%E5%86%99.png"), "FILE:///C:/写.png");
    }

    /// ASCII encodings and runs that cannot be read as UTF-8 are left alone.
    #[test]
    fn unescape_non_ascii_keeps_ascii_and_invalid_escapes() {
        assert_eq!(unescape_non_ascii("file:///a%2Fb%25c"), "file:///a%2Fb%25c");
        assert_eq!(
            unescape_non_ascii("file:///%FF%E5.png"),
            "file:///%FF%E5.png"
        );
        assert_eq!(unescape_non_ascii("file:///%E5%86"), "file:///%E5%86");
        assert_eq!(unescape_non_ascii("file:///写%"), "file:///写%");
    }

    /// A path containing Japanese stays the same path even after passing through
    /// `Windows.Foundation.Uri`.
    ///
    /// Passing it UTF-8 percent-encoded turns it into a different path here, like
    /// `å†™`.
    #[test]
    fn windows_uri_round_trips_japanese_path() {
        let path = "C:/Users/太郎/写真 1.png";
        for source in [path.to_string(), naui_core::media::file_url(path)] {
            let uri = Uri::CreateUri(&HSTRING::from(source_uri(&source))).expect("URI にできない");
            let decoded = Uri::UnescapeComponent(&uri.Path().expect("Path が取れない"))
                .expect("復号できない")
                .to_string();
            assert_eq!(decoded, "/C:/Users/太郎/写真 1.png", "元: {source}");
        }
    }
}
