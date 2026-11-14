//! Images, video and audio (`GtkPicture` / `GtkMediaFile` / `GtkMediaControls`).
//!
//! naui does no decoding or playback. GTK4's GStreamer-based media playback
//! (`GtkMediaFile`) plays it as is, and the picture is drawn into a `GtkPicture` as
//! a `GdkPaintable`.
//!
//! Video is built from "`GtkPicture` + `GtkMediaControls`" rather than `GtkVideo`
//! because `GtkVideo` does not let the fitting ([`Fit`]) or showing/hiding the
//! playback bar be changed from outside. Both show a `GtkMediaFile`, so playback
//! itself is handled the same.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::gdk;
use gtk::gio;
use gtk::glib;
use gtk::prelude::*;
use naui_core::media::source_url;
use naui_core::{Fit, PlaybackState};

use crate::bin::SizeBin;
use crate::callback::Notifier;
use crate::widgets::{impl_widget, Widget};

/// Maps naui's fitting onto `GtkPicture`'s `content-fit`.
///
/// GTK4 has nothing that exactly matches [`Fit::None`] (actual size), so
/// `ScaleDown`, which **does not scale up but does scale down**, is used.
fn to_content_fit(fit: Fit) -> gtk::ContentFit {
    match fit {
        Fit::Contain => gtk::ContentFit::Contain,
        Fit::Cover => gtk::ContentFit::Cover,
        Fit::Fill => gtk::ContentFit::Fill,
        Fit::None => gtk::ContentFit::ScaleDown,
    }
}

/// Creates a `GFile` from a location string.
fn to_file(source: &str) -> gio::File {
    gio::File::for_uri(&source_url(source))
}

/// Whether a content type is SVG (plain or gzip-compressed).
///
/// A content type is not always a MIME type (GIO on macOS uses UTIs), so the MIME
/// types are converted before comparing.
fn is_svg_type(content_type: &str) -> bool {
    ["image/svg+xml", "image/svg+xml-compressed"]
        .into_iter()
        .filter_map(gio::content_type_from_mime_type)
        .any(|svg| gio::content_type_is_a(content_type, &svg))
}

/// Whether the contents of an image file are an SVG.
///
/// Asks gdk-pixbuf, which draws it: GIO's guess from contents needs the MIME database
/// (shared-mime-info). Only the start is handed over, which is what gdk-pixbuf looks at
/// to choose the format, so closing it early fails and that is ignored.
fn is_svg_bytes(bytes: &[u8]) -> bool {
    const SNIFF_BYTES: usize = 4096;
    let loader = gtk::gdk_pixbuf::PixbufLoader::new();
    let _ = loader.write(&bytes[..bytes.len().min(SNIFF_BYTES)]);
    let _ = loader.close();
    loader
        .format()
        .and_then(|format| format.name())
        .is_some_and(|name| name == "svg")
}

// ------------------------------------------------------------ SvgPaintable

// glib のサブクラス化マクロが `unsafe impl` を作るため、この中だけ許可する。
#[allow(unsafe_code)]
mod imp {
    use std::cell::{Cell, OnceCell, RefCell};

    use gtk::gdk;
    use gtk::gdk::subclass::prelude::*;
    use gtk::glib;
    use gtk::prelude::*;

    #[derive(Default)]
    pub struct SvgPaintable {
        /// The SVG file's contents.
        pub bytes: OnceCell<glib::Bytes>,
        /// The SVG's own size (logical pixels).
        pub size: Cell<(i32, i32)>,
        /// Device pixels per logical pixel of the widget it is shown on.
        pub scale: Cell<i32>,
        /// The last rasterization and its size in device pixels.
        pub cache: RefCell<Option<((i32, i32), gdk::Texture)>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for SvgPaintable {
        const NAME: &'static str = "NauiSvgPaintable";
        type Type = super::SvgPaintable;
        type Interfaces = (gdk::Paintable,);
    }

    impl ObjectImpl for SvgPaintable {}

    impl PaintableImpl for SvgPaintable {
        fn flags(&self) -> gdk::PaintableFlags {
            gdk::PaintableFlags::SIZE
        }

        fn intrinsic_width(&self) -> i32 {
            self.size.get().0
        }

        fn intrinsic_height(&self) -> i32 {
            self.size.get().1
        }

        fn snapshot(&self, snapshot: &gdk::Snapshot, width: f64, height: f64) {
            // 描く大きさの実画素で描き直す。同じ大きさなら前のものを使い回す。
            let scale = f64::from(self.scale.get().max(1));
            let pixels = |logical: f64| {
                ((logical * scale).ceil() as i32).clamp(1, super::SvgPaintable::MAX_PIXELS)
            };
            let size = (pixels(width), pixels(height));
            let cached = self
                .cache
                .borrow()
                .as_ref()
                .filter(|(cached, _)| *cached == size)
                .map(|(_, texture)| texture.clone());
            let texture = match cached {
                Some(texture) => Some(texture),
                None => {
                    let texture = self.obj().rasterize(size.0, size.1);
                    if let Some(texture) = &texture {
                        *self.cache.borrow_mut() = Some((size, texture.clone()));
                    }
                    texture
                }
            };
            if let Some(texture) = texture {
                texture.snapshot(snapshot, width, height);
            }
        }
    }
}

glib::wrapper! {
    /// An SVG drawn again at the size it is shown at, so it stays sharp when enlarged.
    ///
    /// `gdk_texture_new_from_bytes` turns an SVG into pixels once at its own size,
    /// and `GtkPicture` then stretches those pixels.
    pub struct SvgPaintable(ObjectSubclass<imp::SvgPaintable>)
        @implements gdk::Paintable;
}

impl SvgPaintable {
    /// The largest side, in device pixels, it is rasterized at.
    const MAX_PIXELS: i32 = 8192;

    /// Reads an SVG. `Err` if it cannot be read.
    fn new(bytes: &glib::Bytes) -> Result<Self, glib::Error> {
        let pixbuf = gtk::gdk_pixbuf::Pixbuf::from_stream(
            &gio::MemoryInputStream::from_bytes(bytes),
            None::<&gio::Cancellable>,
        )?;
        let paintable: Self = glib::Object::new();
        let imp = gtk::subclass::prelude::ObjectSubclassIsExt::imp(&paintable);
        let _ = imp.bytes.set(bytes.clone());
        imp.size.set((pixbuf.width(), pixbuf.height()));
        imp.scale.set(1);
        Ok(paintable)
    }

    /// Sets how many device pixels a logical pixel is on the widget it is shown on.
    fn set_scale(&self, scale: i32) {
        let imp = gtk::subclass::prelude::ObjectSubclassIsExt::imp(self);
        if imp.scale.replace(scale) != scale {
            self.invalidate_contents();
        }
    }

    /// Turns the SVG into `width` × `height` pixels.
    fn rasterize(&self, width: i32, height: i32) -> Option<gdk::Texture> {
        let imp = gtk::subclass::prelude::ObjectSubclassIsExt::imp(self);
        let bytes = imp.bytes.get()?;
        // 縦横比は GtkPicture が合わせ方に沿って決めた大きさに任せる (Fill は引き伸ばす)。
        let pixbuf = gtk::gdk_pixbuf::Pixbuf::from_stream_at_scale(
            &gio::MemoryInputStream::from_bytes(bytes),
            width,
            height,
            false,
            None::<&gio::Cancellable>,
        )
        .map_err(|error| eprintln!("naui-gtk: could not draw the SVG: {error}"))
        .ok()?;
        let format = if pixbuf.has_alpha() {
            gdk::MemoryFormat::R8g8b8a8
        } else {
            gdk::MemoryFormat::R8g8b8
        };
        let texture = gdk::MemoryTexture::new(
            pixbuf.width(),
            pixbuf.height(),
            format,
            &pixbuf.read_pixel_bytes(),
            pixbuf.rowstride() as usize,
        );
        Some(texture.upcast())
    }
}

// ------------------------------------------------------------------ Image

struct ImageInner {
    native: gtk::Picture,
    bin: SizeBin,
    source: RefCell<String>,
}

/// An image (`GtkPicture`).
#[derive(Clone)]
pub struct Image(Rc<ImageInner>);
impl_widget!(Image);

impl Image {
    pub(crate) fn new(source: &str) -> Self {
        let native = gtk::Picture::new();
        native.set_content_fit(gtk::ContentFit::Contain);
        // SVG は画面の倍率に合わせた実画素で描く。
        native.connect_scale_factor_notify(|native| {
            if let Some(svg) = native.paintable().and_downcast::<SvgPaintable>() {
                svg.set_scale(native.scale_factor());
            }
        });
        let bin = SizeBin::wrap(&native);
        bin.mark_fill_ignores_natural();
        let image = Self(Rc::new(ImageInner {
            native,
            bin,
            source: RefCell::new(String::new()),
        }));
        image.set_source(source);
        image
    }

    pub fn source(&self) -> String {
        self.0.source.borrow().clone()
    }

    /// Replaces the image. `source` is a file path or a URL.
    ///
    /// An SVG is read here and drawn at the size it is shown at; other formats are
    /// left to `GtkPicture`.
    pub fn set_source(&self, source: &str) {
        *self.0.source.borrow_mut() = source.to_string();
        if source.is_empty() {
            self.0.native.set_paintable(None::<&gtk::gdk::Paintable>);
            return;
        }
        let file = to_file(source);
        let (content_type, _) = gio::content_type_guess(file.basename(), &[]);
        if !is_svg_type(&content_type) {
            self.0.native.set_file(Some(&file));
            return;
        }
        let svg = file
            .load_bytes(None::<&gio::Cancellable>)
            .and_then(|(bytes, _)| SvgPaintable::new(&bytes));
        match svg {
            Ok(svg) => self.show_svg(&svg),
            Err(error) => {
                eprintln!("naui-gtk: could not read the image: {error}");
                self.0.native.set_paintable(None::<&gdk::Paintable>);
            }
        }
    }

    /// Shows an SVG at the widget's current scale.
    fn show_svg(&self, svg: &SvgPaintable) {
        svg.set_scale(self.0.native.scale_factor());
        self.0.native.set_paintable(Some(svg));
    }

    /// Shows an image from the contents of an image file (PNG, JPEG, SVG and the
    /// other formats GTK4 and gdk-pixbuf can read), such as one embedded with
    /// `include_bytes!`. [`source`](Self::source) becomes empty.
    ///
    /// It is decoded right away (`gdk_texture_new_from_bytes`); if it cannot be,
    /// nothing is shown and [`is_loaded`](Self::is_loaded) is `false`. An SVG is
    /// drawn again at the size it is shown at, so it stays sharp when enlarged.
    pub fn set_bytes(&self, bytes: &[u8]) {
        self.0.source.borrow_mut().clear();
        let is_svg = is_svg_bytes(bytes);
        let bytes = glib::Bytes::from(bytes);
        if is_svg {
            if let Ok(svg) = SvgPaintable::new(&bytes) {
                self.show_svg(&svg);
                return;
            }
        }
        match gdk::Texture::from_bytes(&bytes) {
            Ok(texture) => self.0.native.set_paintable(Some(&texture)),
            Err(error) => {
                eprintln!("naui-gtk: could not read the image: {error}");
                self.0.native.set_paintable(None::<&gdk::Paintable>);
            }
        }
    }

    /// Shows pixels the app made itself: `width` × `height` pixels, rows from top to
    /// bottom, each pixel 4 bytes of red, green, blue and alpha (straight alpha).
    /// [`source`](Self::source) becomes empty.
    ///
    /// Returns `Err` without changing the image when `rgba` is not exactly
    /// `width * height * 4` bytes. The pixels are copied into a `GdkMemoryTexture`.
    pub fn set_pixels(&self, width: u32, height: u32, rgba: &[u8]) -> naui_core::Result<()> {
        naui_core::media::check_pixels(width, height, rgba)?;
        let too_large = || naui_core::Error::new("show the pixels", "the image is too large");
        let w = i32::try_from(width).map_err(|_| too_large())?;
        let h = i32::try_from(height).map_err(|_| too_large())?;
        let stride = (width as usize).checked_mul(4).ok_or_else(too_large)?;
        let texture = gdk::MemoryTexture::new(
            w,
            h,
            gdk::MemoryFormat::R8g8b8a8,
            &glib::Bytes::from(rgba),
            stride,
        );
        self.0.source.borrow_mut().clear();
        self.0.native.set_paintable(Some(&texture));
        Ok(())
    }

    /// Whether it has loaded. `false` if loading failed.
    pub fn is_loaded(&self) -> bool {
        self.0.native.paintable().is_some()
    }

    pub fn set_fit(&self, fit: Fit) {
        self.0.native.set_content_fit(to_content_fit(fit));
    }

    /// Alternative text used by screen readers and the like.
    pub fn set_alt(&self, text: &str) {
        self.0.native.set_alternative_text(Some(text));
    }
}

// --------------------------------------------------------------- Playback

/// The playback state shared by video and audio.
pub(crate) struct PlaybackInner {
    /// What is playing now. Recreated by `set_source`.
    stream: RefCell<Option<gtk::MediaFile>>,
    /// The playback bar. Below the picture for video; for audio it is the widget itself.
    controls: gtk::MediaControls,
    source: RefCell<String>,
    /// Whether playback has ever started. [`PlaybackState::Idle`] if not.
    started: Cell<bool>,
    autoplay: Cell<bool>,
    looping: Cell<bool>,
    volume: Cell<f64>,
    muted: Cell<bool>,
    on_state: Notifier<PlaybackState>,
    on_position: Notifier<f64>,
    /// The surface the picture is shown on. Audio has none.
    picture: Option<gtk::Picture>,
}

impl PlaybackInner {
    fn new(picture: Option<gtk::Picture>) -> Rc<Self> {
        let controls = gtk::MediaControls::new(None::<&gtk::MediaStream>);
        // 再生バーはテープの進む向きを表すので、右から左の配置でも反転させない
        // (再生ボタンが左、位置のバーは左から右へ伸びる)。macOS の AVPlayerView も
        // 右から左のアプリでそう描く。
        crate::bin::pin_left_to_right(&controls);
        Rc::new(Self {
            stream: RefCell::new(None),
            controls,
            source: RefCell::new(String::new()),
            started: Cell::new(false),
            autoplay: Cell::new(false),
            looping: Cell::new(false),
            volume: Cell::new(1.0),
            muted: Cell::new(false),
            on_state: Notifier::default(),
            on_position: Notifier::default(),
            picture,
        })
    }

    /// Applies volume and mute to the native side.
    ///
    /// When muted, `GtkMediaControls` moves the volume slider to 0 and writes that 0
    /// back to the `GtkMediaStream`. **The volume stays at 0 even after unmuting**, so
    /// at that point naui writes back the volume it holds. Without this, there would be
    /// no sound even after unmuting.
    fn apply_audio(&self) {
        let Some(stream) = self.stream.borrow().clone() else {
            return;
        };
        let muted = self.muted.get();
        stream.set_muted(muted);
        if !muted {
            stream.set_volume(self.volume.get());
        }
    }

    /// Cleanup when a state-related property changes.
    ///
    /// Autoplay starts "once loaded", so it waits until ready and starts from here.
    fn after_state_change(&self) {
        let stream = self.stream.borrow().clone();
        if let Some(stream) = stream {
            if self.autoplay.get()
                && !self.started.get()
                && stream.is_prepared()
                && !stream.is_playing()
            {
                self.started.set(true);
                stream.play();
            }
        }
        self.on_state.emit(self.state());
    }

    /// Builds the current state from GTK4's properties.
    fn state(&self) -> PlaybackState {
        let Some(stream) = self.stream.borrow().clone() else {
            return PlaybackState::Idle;
        };
        if stream.is_ended() {
            PlaybackState::Ended
        } else if stream.is_playing() {
            // 鳴らそうとしているが、まだ用意ができていない間は待ち。
            if stream.is_prepared() {
                PlaybackState::Playing
            } else {
                PlaybackState::Buffering
            }
        } else if self.started.get() {
            PlaybackState::Paused
        } else {
            PlaybackState::Idle
        }
    }
}

/// Adds the playback API to both video and audio.
macro_rules! impl_playback {
    ($t:ty) => {
        impl $t {
            pub fn source(&self) -> String {
                self.0.playback.source.borrow().clone()
            }

            /// Replaces what to play. `source` is a file path or a URL.
            pub fn set_source(&self, source: &str) {
                crate::media::set_source(&self.0.playback, source);
            }

            pub fn play(&self) {
                if let Some(stream) = self.0.playback.stream.borrow().clone() {
                    self.0.playback.started.set(true);
                    stream.play();
                }
            }

            pub fn pause(&self) {
                if let Some(stream) = self.0.playback.stream.borrow().clone() {
                    stream.pause();
                }
            }

            pub fn state(&self) -> PlaybackState {
                self.0.playback.state()
            }

            pub fn is_playing(&self) -> bool {
                self.state().is_playing()
            }

            /// Sets the playback position in seconds.
            pub fn seek(&self, seconds: f64) {
                if let Some(stream) = self.0.playback.stream.borrow().clone() {
                    stream.seek(crate::media::to_micros(seconds));
                }
            }

            /// The current playback position (seconds).
            pub fn position(&self) -> f64 {
                self.0
                    .playback
                    .stream
                    .borrow()
                    .as_ref()
                    .map(|s| crate::media::to_seconds(s.timestamp()))
                    .unwrap_or(0.0)
            }

            /// The length (seconds). `None` if not known yet.
            pub fn duration(&self) -> Option<f64> {
                self.0
                    .playback
                    .stream
                    .borrow()
                    .as_ref()
                    .map(|s| s.duration())
                    .filter(|&d| d > 0)
                    .map(crate::media::to_seconds)
            }

            /// 0.0..=1.0.
            pub fn set_volume(&self, volume: f64) {
                self.0.playback.volume.set(volume.clamp(0.0, 1.0));
                self.0.playback.apply_audio();
            }

            pub fn volume(&self) -> f64 {
                self.0.playback.volume.get()
            }

            pub fn set_muted(&self, muted: bool) {
                self.0.playback.muted.set(muted);
                self.0.playback.apply_audio();
            }

            pub fn is_muted(&self) -> bool {
                self.0.playback.muted.get()
            }

            pub fn set_loop(&self, looping: bool) {
                self.0.playback.looping.set(looping);
                if let Some(stream) = self.0.playback.stream.borrow().clone() {
                    stream.set_loop(looping);
                }
            }

            pub fn is_loop(&self) -> bool {
                self.0.playback.looping.get()
            }

            /// Whether to start playing automatically once loaded.
            pub fn set_autoplay(&self, autoplay: bool) {
                self.0.playback.autoplay.set(autoplay);
                if autoplay {
                    self.play();
                }
            }

            /// Whether to show the playback bar (shown by default).
            pub fn set_controls(&self, controls: bool) {
                self.0.playback.controls.set_visible(controls);
            }

            /// Called every time the playback state changes.
            ///
            /// It arrives not only when the app calls `play()` but **also when the user
            /// operates GTK4's playback bar**.
            pub fn on_state_change(&self, f: impl FnMut(PlaybackState) + 'static) {
                self.0.playback.on_state.set(f);
            }

            /// Called with the position in seconds every time the playback position changes.
            pub fn on_position_change(&self, f: impl FnMut(f64) + 'static) {
                self.0.playback.on_position.set(f);
            }
        }
    };
}

pub(crate) fn to_micros(seconds: f64) -> i64 {
    (seconds * 1_000_000.0).round() as i64
}

pub(crate) fn to_seconds(micros: i64) -> f64 {
    micros as f64 / 1_000_000.0
}

/// Recreates what to play, and reconnects the state and position notifications.
pub(crate) fn set_source(playback: &Rc<PlaybackInner>, source: &str) {
    *playback.source.borrow_mut() = source.to_string();
    playback.started.set(false);

    if source.is_empty() {
        *playback.stream.borrow_mut() = None;
        playback
            .controls
            .set_media_stream(None::<&gtk::MediaStream>);
        if let Some(picture) = &playback.picture {
            picture.set_paintable(None::<&gtk::gdk::Paintable>);
        }
        return;
    }

    let stream = gtk::MediaFile::for_file(&to_file(source));
    stream.set_loop(playback.looping.get());

    // 状態にかかわるプロパティは、どれが動いても同じ判定をやり直す。
    macro_rules! on_notify {
        ($connect:ident) => {{
            let weak = Rc::downgrade(playback);
            stream.$connect(move |_| {
                if let Some(playback) = weak.upgrade() {
                    playback.after_state_change();
                }
            });
        }};
    }
    on_notify!(connect_playing_notify);
    on_notify!(connect_ended_notify);
    on_notify!(connect_prepared_notify);
    {
        let weak = Rc::downgrade(playback);
        stream.connect_timestamp_notify(move |stream| {
            if let Some(playback) = weak.upgrade() {
                playback.on_position.emit(to_seconds(stream.timestamp()));
            }
        });
    }
    {
        // 再生バーの音量つまみを動かされたら、それを覚えておく。
        // ただし消音の間は `GtkMediaControls` が 0 を書きに来るので拾わない
        // (覚えてしまうと、消音を解いたときに戻す音量が 0 になる)。
        let weak = Rc::downgrade(playback);
        stream.connect_volume_notify(move |stream| {
            if let Some(playback) = weak.upgrade() {
                if !playback.muted.get() {
                    playback.volume.set(stream.volume());
                }
            }
        });
    }

    playback.controls.set_media_stream(Some(&stream));
    if let Some(picture) = &playback.picture {
        picture.set_paintable(Some(&stream));
    }
    *playback.stream.borrow_mut() = Some(stream);
    playback.apply_audio();

    if playback.autoplay.get() {
        if let Some(stream) = playback.stream.borrow().clone() {
            playback.started.set(true);
            stream.play();
        }
    }
}

// ------------------------------------------------------------------ Video

struct VideoInner {
    native: gtk::Box,
    bin: SizeBin,
    picture: gtk::Picture,
    playback: Rc<PlaybackInner>,
}

/// Video (a `GtkMediaFile` shown in a `GtkPicture`, with `GtkMediaControls` below).
#[derive(Clone)]
pub struct Video(Rc<VideoInner>);
impl_widget!(Video);
impl_playback!(Video);

impl Video {
    pub(crate) fn new(source: &str) -> Self {
        let picture = gtk::Picture::new();
        picture.set_content_fit(gtk::ContentFit::Contain);
        // 映像は縦の余りを受け取り、再生バーはその下に自然な高さで置く。
        picture.set_vexpand(true);

        let playback = PlaybackInner::new(Some(picture.clone()));
        let native = gtk::Box::new(gtk::Orientation::Vertical, 0);
        native.append(&picture);
        native.append(&playback.controls);

        let bin = SizeBin::wrap(&native);
        bin.mark_fill_ignores_natural();
        let video = Self(Rc::new(VideoInner {
            native,
            bin,
            picture,
            playback,
        }));
        video.set_source(source);
        video
    }

    /// How the picture is fitted into the display area.
    pub fn set_fit(&self, fit: Fit) {
        self.0.picture.set_content_fit(to_content_fit(fit));
    }
}

// ------------------------------------------------------------------ Audio

struct AudioInner {
    native: gtk::MediaControls,
    bin: SizeBin,
    playback: Rc<PlaybackInner>,
}

/// Audio (`GtkMediaControls` + `GtkMediaFile`). Has no picture.
#[derive(Clone)]
pub struct Audio(Rc<AudioInner>);
impl_widget!(Audio);
impl_playback!(Audio);

impl Audio {
    pub(crate) fn new(source: &str) -> Self {
        let playback = PlaybackInner::new(None);
        let native = playback.controls.clone();
        let bin = SizeBin::wrap(&native);
        let audio = Self(Rc::new(AudioInner {
            native,
            bin,
            playback,
        }));
        audio.set_source(source);
        audio
    }
}
