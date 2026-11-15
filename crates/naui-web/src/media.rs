//! Images, video and audio.
//!
//! `<img>` / `<video>` / `<audio>` are used as they are. Decoding, playback and
//! drawing the playback bar are the browser's job; naui only writes `src` and
//! attributes. `<img>` and `<video>` sit in a frame `<div>`, which is the widget's
//! element ([`frame`]).
//!
//! The playback state is notified by subscribing to HTMLMediaElement's events
//! (`playing` / `waiting` / `pause` / `ended`). It arrives through the same path
//! not only when the app calls [`Video::play`] but **also when the user operates
//! the browser's playback bar**.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use naui_core::{Fit, PlaybackState, Result};
use wasm_bindgen::JsCast;
use web_sys::{Document, Element, HtmlElement, HtmlImageElement, HtmlMediaElement};

use crate::to_error;
use crate::widgets::{create, impl_widget, Listener, Widget};

/// Turns the fitting into a CSS `object-fit` value.
fn object_fit(fit: Fit) -> &'static str {
    match fit {
        Fit::Contain => "contain",
        Fit::Cover => "cover",
        Fit::Fill => "fill",
        Fit::None => "none",
    }
}

/// Puts an image or a video into the frame `<div>` that is the widget's element.
///
/// A `Fill` picture does not ask its parent for its natural size: the frame gets
/// size containment ([`apply_sizing`](crate::layout::apply_sizing)). Containing the
/// `<img>` / `<video>` itself would be enough for the layout, but WebKit then takes
/// the contained size (zero) as the picture's natural size, and `object-fit`
/// distorts it (https://bugs.webkit.org/show_bug.cgi?id=276681).
///
/// The frame is a grid of one cell, stretched over by the picture. Without
/// containment the frame is as large as the picture would be (its pixel size, or the
/// height its width and aspect ratio give); with it, the picture fills what the
/// parent gives. Stretching in a grid needs no percentage, which WebKit resolves
/// against the wrong box when the frame is in a grid whose height comes from its
/// content.
fn frame(doc: &Document, picture: &HtmlElement) -> Result<HtmlElement> {
    let frame: HtmlElement = create(doc, "div")?.unchecked_into();
    let _ = frame.set_attribute(crate::layout::PICTURE_FRAME_ATTR, "");
    let style = frame.style();
    let _ = style.set_property("display", "inline-grid");
    let _ = style.set_property("grid-template", "minmax(0, 1fr) / minmax(0, 1fr)");
    let style = picture.style();
    let _ = style.set_property("place-self", "stretch");
    let _ = style.set_property("min-width", "0");
    let _ = style.set_property("min-height", "0");
    frame
        .append_child(picture)
        .map_err(|e| to_error("put the picture into its frame", e))?;
    Ok(frame)
}

// ------------------------------------------------------------------ Image

struct ImageInner {
    /// The frame placed into the parent ([`frame`]).
    frame: HtmlElement,
    element: HtmlImageElement,
    /// Exactly the string that was passed, since `src` gets resolved to an absolute
    /// URL.
    source: RefCell<String>,
    /// The `blob:` URL made for [`Image::set_bytes`] / [`Image::set_pixels`].
    /// Revoked when replaced and when the image is dropped.
    object_url: RefCell<Option<String>>,
    /// `on_file_drop`.
    file_drop: crate::file_drop::FileDrop,
}

impl Drop for ImageInner {
    fn drop(&mut self) {
        if let Some(url) = self.object_url.get_mut().take() {
            let _ = web_sys::Url::revoke_object_url(&url);
        }
    }
}

/// Image display (`<img>` in a frame `<div>`).
///
/// The browser loads it asynchronously. **Right after it is set,
/// [`is_loaded`](Self::is_loaded) is still false.**
#[derive(Clone)]
pub struct Image(Rc<ImageInner>);
impl_widget!(Image, frame);

impl Image {
    pub(crate) fn new(doc: &Document, source: &str) -> Result<Self> {
        let element: HtmlImageElement = create(doc, "img")?.unchecked_into();
        let this = Self(Rc::new(ImageInner {
            frame: frame(doc, &element)?,
            element,
            source: RefCell::new(String::new()),
            object_url: RefCell::new(None),
            file_drop: Default::default(),
        }));
        this.set_fit(Fit::default());
        this.set_source(source);
        Ok(this)
    }

    /// The location currently specified (exactly the string that was passed).
    pub fn source(&self) -> String {
        self.0.source.borrow().clone()
    }

    /// The location of the image to show. Relative URLs are resolved relative to the
    /// page.
    ///
    /// Passing an empty string removes the image.
    pub fn set_source(&self, source: &str) {
        *self.0.source.borrow_mut() = source.to_string();
        if source.is_empty() {
            let _ = self.0.element.remove_attribute("src");
        } else {
            self.0.element.set_src(source);
        }
        self.revoke_object_url();
    }

    /// Shows an image from the contents of an image file held in memory.
    /// [`source`](Self::source) becomes empty.
    ///
    /// The bytes become a `Blob` shown through a `blob:` URL, so the browser decodes
    /// them as it would a file. SVG is given the type `image/svg+xml`, which the
    /// browser needs to read it as an image; raster formats are recognized by their
    /// signature. Loading is asynchronous, as with [`set_source`](Self::set_source).
    pub fn set_bytes(&self, bytes: &[u8]) {
        let kind = if naui_core::media::is_svg(bytes) {
            "image/svg+xml"
        } else {
            ""
        };
        self.show_blob(bytes, kind);
    }

    /// Shows pixels the app made itself: `width` × `height` pixels, rows from top to
    /// bottom, each pixel 4 bytes of red, green, blue and alpha (straight alpha).
    /// [`source`](Self::source) becomes empty.
    ///
    /// The pixels are packed into an uncompressed PNG and shown like
    /// [`set_bytes`](Self::set_bytes). Returns `Err` without changing the image when
    /// `rgba` is not exactly `width * height * 4` bytes.
    pub fn set_pixels(&self, width: u32, height: u32, rgba: &[u8]) -> Result<()> {
        let png = naui_core::media::encode_png(width, height, rgba)?;
        self.show_blob(&png, "image/png");
        Ok(())
    }

    fn show_blob(&self, bytes: &[u8], kind: &str) {
        self.0.source.borrow_mut().clear();
        let parts = js_sys::Array::of1(&js_sys::Uint8Array::from(bytes));
        let options = web_sys::BlobPropertyBag::new();
        options.set_type(kind);
        let url = web_sys::Blob::new_with_u8_array_sequence_and_options(&parts, &options)
            .and_then(|blob| web_sys::Url::create_object_url_with_blob(&blob));
        match url {
            Ok(url) => {
                self.0.element.set_src(&url);
                self.revoke_object_url();
                *self.0.object_url.borrow_mut() = Some(url);
            }
            Err(error) => {
                web_sys::console::error_2(&"naui-web: could not show the image:".into(), &error);
                let _ = self.0.element.remove_attribute("src");
                self.revoke_object_url();
            }
        }
    }

    /// Lets the browser free the `blob:` URL that was shown before.
    ///
    /// Revoking does not affect an image that has already started loading from it.
    fn revoke_object_url(&self) {
        if let Some(url) = self.0.object_url.borrow_mut().take() {
            let _ = web_sys::Url::revoke_object_url(&url);
        }
    }

    /// Whether loading has finished.
    ///
    /// Browser loading is asynchronous, so it is false right after
    /// [`set_source`](Self::set_source).
    pub fn is_loaded(&self) -> bool {
        self.0.element.complete() && self.0.element.natural_width() > 0
    }

    /// How it is fitted into the display area (CSS `object-fit`).
    pub fn set_fit(&self, fit: Fit) {
        let _ = self
            .0
            .element
            .style()
            .set_property("object-fit", object_fit(fit));
    }

    /// Text describing the image's content (`alt`). Used for screen readers and shown
    /// when it could not be loaded.
    pub fn set_alt(&self, text: &str) {
        self.0.element.set_alt(text);
    }
}

// --------------------------------------------------------------- 再生の中身

/// A single replaceable closure and the current state.
///
/// Separate listeners are attached per event, so it is shared by all of them.
#[derive(Clone, Default)]
struct StateHandler(Rc<StateInner>);

#[derive(Default)]
struct StateInner {
    state: Cell<PlaybackState>,
    callback: RefCell<Option<Box<dyn FnMut(PlaybackState)>>>,
}

impl StateHandler {
    fn state(&self) -> PlaybackState {
        self.0.state.get()
    }

    fn set(&self, f: impl FnMut(PlaybackState) + 'static) {
        *self.0.callback.borrow_mut() = Some(Box::new(f));
    }

    /// If the state changed, records and notifies it. Not called for repeats of the
    /// same state.
    fn emit(&self, state: PlaybackState) {
        if self.0.state.get() == state {
            return;
        }
        self.0.state.set(state);
        // クロージャの中から同じウィジェットを触っても二重借用にならないよう、
        // 呼び出しの間だけ取り出す。
        let Some(mut f) = self.0.callback.borrow_mut().take() else {
            return;
        };
        f(state);
        let mut slot = self.0.callback.borrow_mut();
        if slot.is_none() {
            *slot = Some(f);
        }
    }
}

/// A single replaceable closure that reports the playback position.
#[derive(Clone, Default)]
struct PositionHandler(Rc<RefCell<Option<Box<dyn FnMut(f64)>>>>);

impl PositionHandler {
    fn set(&self, f: impl FnMut(f64) + 'static) {
        *self.0.borrow_mut() = Some(Box::new(f));
    }

    fn emit(&self, seconds: f64) {
        // 状態の通知と同じく、呼び出しの間だけ取り出して再入を避ける。
        let Some(mut f) = self.0.borrow_mut().take() else {
            return;
        };
        f(seconds);
        let mut slot = self.0.borrow_mut();
        if slot.is_none() {
            *slot = Some(f);
        }
    }
}

/// The playback object shared by video and audio.
///
/// `<video>` and `<audio>` are both HTMLMediaElements, so apart from whether they
/// have a picture, every operation is exactly the same.
struct MediaInner {
    /// The element placed into the parent: the frame around a video ([`frame`]), or
    /// the `<audio>` itself.
    frame: HtmlElement,
    element: HtmlMediaElement,
    source: RefCell<String>,
    handler: StateHandler,
    position: PositionHandler,
    /// The event subscriptions for notifying the state. Valid only while the handle is
    /// alive.
    listeners: RefCell<Vec<Listener>>,
    /// `on_file_drop`.
    file_drop: crate::file_drop::FileDrop,
}

impl MediaInner {
    fn new(doc: &Document, tag: &'static str) -> Result<Rc<Self>> {
        let element: HtmlMediaElement = create(doc, tag)?.unchecked_into();
        // ブラウザ標準の再生バーを出す。
        element.set_controls(true);
        // 再生バーはテープの進む向きを表すので、右から左の配置でも反転させない。
        let _ = element.set_attribute("dir", "ltr");
        let frame = if tag == "video" {
            frame(doc, &element)?
        } else {
            element.clone().unchecked_into()
        };
        let this = Rc::new(Self {
            frame,
            element,
            source: RefCell::new(String::new()),
            handler: StateHandler::default(),
            position: PositionHandler::default(),
            listeners: RefCell::new(Vec::new()),
            file_drop: Default::default(),
        });
        this.subscribe()?;
        Ok(this)
    }

    /// Maps HTMLMediaElement's events onto naui's playback state.
    fn subscribe(&self) -> Result<()> {
        let mut listeners = Vec::new();
        // `play` は「再生を始めようとした」で、まだ音は出ていない。
        // 実際に進み始めると `playing` が来る。
        for (event, state) in [
            ("play", PlaybackState::Buffering),
            ("waiting", PlaybackState::Buffering),
            ("playing", PlaybackState::Playing),
            ("ended", PlaybackState::Ended),
        ] {
            let handler = self.handler.clone();
            listeners.push(Listener::attach(self.element.as_ref(), event, move || {
                handler.emit(state)
            })?);
        }
        // 末尾に届いたときの `pause` は「一時停止」ではない。
        // 後から来る `ended` に任せる。
        let handler = self.handler.clone();
        let element = self.element.clone();
        listeners.push(Listener::attach(
            self.element.as_ref(),
            "pause",
            move || {
                if !element.ended() {
                    handler.emit(PlaybackState::Paused);
                }
            },
        )?);
        // 再生位置の追従。`timeupdate` の間隔はブラウザが決める
        // (仕様上は 4 回/秒あたりが目安)。シーク直後にも届く。
        let position = self.position.clone();
        let element = self.element.clone();
        listeners.push(Listener::attach(
            self.element.as_ref(),
            "timeupdate",
            move || position.emit(element.current_time()),
        )?);

        *self.listeners.borrow_mut() = listeners;
        Ok(())
    }

    fn set_source(&self, source: &str) {
        *self.source.borrow_mut() = source.to_string();
        if source.is_empty() {
            let _ = self.element.remove_attribute("src");
        } else {
            self.element.set_src(source);
        }
        // 新しいメディアとして読み直させる。
        self.element.load();
        self.handler.emit(PlaybackState::Idle);
    }

    fn play(&self) {
        // 最後まで再生し終えた後の `play()` は、HTMLMediaElement 自身が
        // 先頭へ戻してから再生する。
        //
        // 返り値の Promise は、ブラウザの自動再生制限で拒否されることがある。
        // その場合は状態が変わらない (Playing が来ない) だけで、
        // アプリ側は `on_state_change` で見分けられる。
        let _ = self.element.play();
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

            /// The location of the media to play. Relative URLs can be used too.
            ///
            /// Calling it stops playback, and the state returns to [`PlaybackState::Idle`].
            /// Passing an empty string removes the media.
            pub fn set_source(&self, source: &str) {
                self.0.set_source(source);
            }

            /// Starts playback.
            ///
            /// Called after playing to the end, it goes back to the start before playing.
            /// **It may be refused by the browser's autoplay restrictions** (no preceding user
            /// action, it makes sound, and so on). When refused, the state does not change.
            pub fn play(&self) {
                self.0.play();
            }

            /// Pauses.
            pub fn pause(&self) {
                let _ = self.0.element.pause();
            }

            /// The current playback state.
            pub fn state(&self) -> PlaybackState {
                self.0.handler.state()
            }

            /// Whether it is playing.
            pub fn is_playing(&self) -> bool {
                self.0.handler.state().is_playing()
            }

            /// Sets the playback position in seconds. Negative values are treated as the start.
            pub fn seek(&self, seconds: f64) {
                self.0.element.set_current_time(seconds.max(0.0));
            }

            /// The current playback position (seconds).
            pub fn position(&self) -> f64 {
                self.0.element.current_time()
            }

            /// The length of the media (seconds). **`None` until loading finishes.**
            ///
            /// Also returns `None` for streams whose length is undefined (live streams and so
            /// on), which HTMLMediaElement represents as NaN and Infinity.
            pub fn duration(&self) -> Option<f64> {
                let duration = self.0.element.duration();
                if duration.is_nan() || duration.is_infinite() {
                    None
                } else {
                    Some(duration)
                }
            }

            /// The volume (0.0..=1.0). Out-of-range values are clamped.
            pub fn set_volume(&self, volume: f64) {
                let _ = self.0.element.set_volume(volume.clamp(0.0, 1.0));
            }

            pub fn volume(&self) -> f64 {
                self.0.element.volume()
            }

            /// Mutes it. Only the sound stops; the volume value is kept.
            pub fn set_muted(&self, muted: bool) {
                self.0.element.set_muted(muted);
            }

            pub fn is_muted(&self) -> bool {
                self.0.element.muted()
            }

            /// After playing to the end, goes back to the start and repeats.
            pub fn set_loop(&self, looping: bool) {
                self.0.element.set_loop(looping);
            }

            pub fn is_loop(&self) -> bool {
                self.0.element.loop_()
            }

            /// Starts playing automatically when media is specified.
            ///
            /// If a location is already specified and it has never played, playback starts with
            /// this call.
            pub fn set_autoplay(&self, autoplay: bool) {
                self.0.element.set_autoplay(autoplay);
                if autoplay
                    && self.0.handler.state() == PlaybackState::Idle
                    && !self.0.source.borrow().is_empty()
                {
                    self.0.play();
                }
            }

            /// Whether to show the browser's standard playback bar (shown by default).
            pub fn set_controls(&self, controls: bool) {
                self.0.element.set_controls(controls);
            }

            /// Called when the playback state changes. Setting it again removes the previous
            /// one.
            ///
            /// It arrives not only when the app calls [`play`](Self::play) but **also when the
            /// user operates the browser's playback bar**.
            pub fn on_state_change(&self, f: impl FnMut(PlaybackState) + 'static) {
                self.0.handler.set(f);
            }

            /// Called with the position (seconds) every time the playback position advances.
            ///
            /// For keeping a seek bar in step with playback. The browser decides the interval,
            /// but it is about four times a second, and it also arrives right after seeking.
            /// **It is not called while not playing.**
            pub fn on_position_change(&self, f: impl FnMut(f64) + 'static) {
                self.0.position.set(f);
            }
        }
    };
}

// ------------------------------------------------------------------ Video

/// Video (`<video>` in a frame `<div>`).
#[derive(Clone)]
pub struct Video(Rc<MediaInner>);
impl_widget!(Video, frame);
impl_playback!(Video);

impl Video {
    pub(crate) fn new(doc: &Document, source: &str) -> Result<Self> {
        let this = Self(MediaInner::new(doc, "video")?);
        this.set_fit(Fit::default());
        this.set_source(source);
        Ok(this)
    }

    /// How the picture is fitted (CSS `object-fit`).
    pub fn set_fit(&self, fit: Fit) {
        let element: &HtmlElement = self.0.element.as_ref();
        let _ = element.style().set_property("object-fit", object_fit(fit));
    }
}

// ------------------------------------------------------------------ Audio

/// Audio (`<audio>`). It has no picture, so only the playback bar is visible.
#[derive(Clone)]
pub struct Audio(Rc<MediaInner>);
impl_widget!(Audio, frame);
impl_playback!(Audio);

impl Audio {
    pub(crate) fn new(doc: &Document, source: &str) -> Result<Self> {
        let this = Self(MediaInner::new(doc, "audio")?);
        this.set_source(source);
        Ok(this)
    }
}
