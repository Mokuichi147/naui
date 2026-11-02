use crate::sys;
use naui_core::{Fit, PlaybackState};

/// Audio (`GtkMediaControls` + `GtkMediaFile`). Has no picture.
#[derive(Clone)]
pub struct Audio(pub(crate) sys::Audio);

impl_widget!(Audio);

impl Audio {
    #[inline]
    pub fn source(&self) -> String {
        self.0.source()
    }

    /// Replaces what to play. `source` is a file path or a URL.
    #[inline]
    pub fn set_source(&self, source: &str) {
        self.0.set_source(source)
    }

    #[inline]
    pub fn play(&self) {
        self.0.play()
    }

    #[inline]
    pub fn pause(&self) {
        self.0.pause()
    }

    #[inline]
    pub fn state(&self) -> PlaybackState {
        self.0.state()
    }

    #[inline]
    pub fn is_playing(&self) -> bool {
        self.0.is_playing()
    }

    /// Sets the playback position in seconds.
    #[inline]
    pub fn seek(&self, seconds: f64) {
        self.0.seek(seconds)
    }

    /// The current playback position (seconds).
    #[inline]
    pub fn position(&self) -> f64 {
        self.0.position()
    }

    /// The length (seconds). `None` if not known yet.
    #[inline]
    pub fn duration(&self) -> Option<f64> {
        self.0.duration()
    }

    /// 0.0..=1.0.
    #[inline]
    pub fn set_volume(&self, volume: f64) {
        self.0.set_volume(volume)
    }

    #[inline]
    pub fn volume(&self) -> f64 {
        self.0.volume()
    }

    #[inline]
    pub fn set_muted(&self, muted: bool) {
        self.0.set_muted(muted)
    }

    #[inline]
    pub fn is_muted(&self) -> bool {
        self.0.is_muted()
    }

    #[inline]
    pub fn set_loop(&self, looping: bool) {
        self.0.set_loop(looping)
    }

    #[inline]
    pub fn is_loop(&self) -> bool {
        self.0.is_loop()
    }

    /// Whether to start playing automatically once loaded.
    #[inline]
    pub fn set_autoplay(&self, autoplay: bool) {
        self.0.set_autoplay(autoplay)
    }

    /// Whether to show the playback bar (shown by default).
    #[inline]
    pub fn set_controls(&self, controls: bool) {
        self.0.set_controls(controls)
    }

    /// Called every time the playback state changes.
    ///
    /// It arrives not only when the app calls `play()` but **also when the user
    /// operates GTK4's playback bar**.
    #[inline]
    pub fn on_state_change(&self, f: impl FnMut(PlaybackState) + 'static) {
        self.0.on_state_change(f)
    }

    /// Called with the position in seconds every time the playback position changes.
    #[inline]
    pub fn on_position_change(&self, f: impl FnMut(f64) + 'static) {
        self.0.on_position_change(f)
    }
}

/// An image (`GtkPicture`).
#[derive(Clone)]
pub struct Image(pub(crate) sys::Image);

impl_widget!(Image);

impl Image {
    #[inline]
    pub fn source(&self) -> String {
        self.0.source()
    }

    /// Replaces the image. `source` is a file path or a URL. SVG files can be shown
    /// too.
    #[inline]
    pub fn set_source(&self, source: &str) {
        self.0.set_source(source)
    }

    /// Shows an image from the contents of an image file held in memory, such as one
    /// embedded with `include_bytes!`. [`source`](Self::source) becomes empty.
    ///
    /// The platform decodes it, so the formats are those it reads: PNG, JPEG and GIF
    /// everywhere, and SVG too (see
    /// [More image sources](crate::guide#images-from-memory-and-svg)).
    ///
    /// ```no_run
    /// # fn build(ui: &naui::Ui) -> naui::Result<()> {
    /// // Usually `include_bytes!("icon.svg")`.
    /// const ICON: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16">
    ///   <circle cx="8" cy="8" r="7" fill="teal"/></svg>"#;
    /// let icon = ui.image("")?;
    /// icon.set_bytes(ICON);
    /// # Ok(())
    /// # }
    /// ```
    #[inline]
    pub fn set_bytes(&self, bytes: &[u8]) {
        self.0.set_bytes(bytes)
    }

    /// Shows pixels the app made itself: `width` × `height` pixels, rows from top to
    /// bottom, each pixel 4 bytes of red, green, blue and alpha (straight, not
    /// premultiplied, alpha). [`source`](Self::source) becomes empty.
    ///
    /// Returns `Err` without changing the image when `rgba` is not exactly
    /// `width * height * 4` bytes, or either side is 0.
    #[inline]
    pub fn set_pixels(&self, width: u32, height: u32, rgba: &[u8]) -> naui_core::Result<()> {
        self.0.set_pixels(width, height, rgba)
    }

    /// Whether it has loaded. `false` if loading failed.
    #[inline]
    pub fn is_loaded(&self) -> bool {
        self.0.is_loaded()
    }

    #[inline]
    pub fn set_fit(&self, fit: Fit) {
        self.0.set_fit(fit)
    }

    /// Alternative text used by screen readers and the like.
    #[inline]
    pub fn set_alt(&self, text: &str) {
        self.0.set_alt(text)
    }
}

/// Video (a `GtkMediaFile` shown in a `GtkPicture`, with `GtkMediaControls` below).
#[derive(Clone)]
pub struct Video(pub(crate) sys::Video);

impl_widget!(Video);

impl Video {
    #[inline]
    pub fn source(&self) -> String {
        self.0.source()
    }

    /// Replaces what to play. `source` is a file path or a URL.
    #[inline]
    pub fn set_source(&self, source: &str) {
        self.0.set_source(source)
    }

    #[inline]
    pub fn play(&self) {
        self.0.play()
    }

    #[inline]
    pub fn pause(&self) {
        self.0.pause()
    }

    #[inline]
    pub fn state(&self) -> PlaybackState {
        self.0.state()
    }

    #[inline]
    pub fn is_playing(&self) -> bool {
        self.0.is_playing()
    }

    /// Sets the playback position in seconds.
    #[inline]
    pub fn seek(&self, seconds: f64) {
        self.0.seek(seconds)
    }

    /// The current playback position (seconds).
    #[inline]
    pub fn position(&self) -> f64 {
        self.0.position()
    }

    /// The length (seconds). `None` if not known yet.
    #[inline]
    pub fn duration(&self) -> Option<f64> {
        self.0.duration()
    }

    /// 0.0..=1.0.
    #[inline]
    pub fn set_volume(&self, volume: f64) {
        self.0.set_volume(volume)
    }

    #[inline]
    pub fn volume(&self) -> f64 {
        self.0.volume()
    }

    #[inline]
    pub fn set_muted(&self, muted: bool) {
        self.0.set_muted(muted)
    }

    #[inline]
    pub fn is_muted(&self) -> bool {
        self.0.is_muted()
    }

    #[inline]
    pub fn set_loop(&self, looping: bool) {
        self.0.set_loop(looping)
    }

    #[inline]
    pub fn is_loop(&self) -> bool {
        self.0.is_loop()
    }

    /// Whether to start playing automatically once loaded.
    #[inline]
    pub fn set_autoplay(&self, autoplay: bool) {
        self.0.set_autoplay(autoplay)
    }

    /// Whether to show the playback bar (shown by default).
    #[inline]
    pub fn set_controls(&self, controls: bool) {
        self.0.set_controls(controls)
    }

    /// Called every time the playback state changes.
    ///
    /// It arrives not only when the app calls `play()` but **also when the user
    /// operates GTK4's playback bar**.
    #[inline]
    pub fn on_state_change(&self, f: impl FnMut(PlaybackState) + 'static) {
        self.0.on_state_change(f)
    }

    /// Called with the position in seconds every time the playback position changes.
    #[inline]
    pub fn on_position_change(&self, f: impl FnMut(f64) + 'static) {
        self.0.on_position_change(f)
    }

    /// How the picture is fitted into the display area.
    #[inline]
    pub fn set_fit(&self, fit: Fit) {
        self.0.set_fit(fit)
    }
}
