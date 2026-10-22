use crate::sys;
use naui_core::{Fit, PlaybackState};

/// 音声 (`GtkMediaControls` + `GtkMediaFile`)。映像面を持たない。
#[derive(Clone)]
pub struct Audio(pub(crate) sys::Audio);

impl_widget!(Audio);

impl Audio {
    #[inline]
    pub fn source(&self) -> String {
        self.0.source()
    }

    /// 鳴らすものを差し替える。`source` はファイルパスか URL。
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

    /// 再生位置を秒で指定する。
    #[inline]
    pub fn seek(&self, seconds: f64) {
        self.0.seek(seconds)
    }

    /// いまの再生位置 (秒)。
    #[inline]
    pub fn position(&self) -> f64 {
        self.0.position()
    }

    /// 長さ (秒)。まだ分からなければ `None`。
    #[inline]
    pub fn duration(&self) -> Option<f64> {
        self.0.duration()
    }

    /// 0.0..=1.0。
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

    /// 読み込めたら自動で再生を始めるか。
    #[inline]
    pub fn set_autoplay(&self, autoplay: bool) {
        self.0.set_autoplay(autoplay)
    }

    /// 再生バーを出すか (既定は出す)。
    #[inline]
    pub fn set_controls(&self, controls: bool) {
        self.0.set_controls(controls)
    }

    /// 再生の状態が変わるたびに呼ばれる。
    ///
    /// アプリから `play()` を呼んだときだけでなく、**GTK4 の再生バーを
    /// ユーザーが操作したときにも届く**。
    #[inline]
    pub fn on_state_change(&self, f: impl FnMut(PlaybackState) + 'static) {
        self.0.on_state_change(f)
    }

    /// 再生位置が変わるたびに、その秒数で呼ばれる。
    #[inline]
    pub fn on_position_change(&self, f: impl FnMut(f64) + 'static) {
        self.0.on_position_change(f)
    }
}

/// 画像 (`GtkPicture`)。
#[derive(Clone)]
pub struct Image(pub(crate) sys::Image);

impl_widget!(Image);

impl Image {
    #[inline]
    pub fn source(&self) -> String {
        self.0.source()
    }

    /// 画像を差し替える。`source` はファイルパスか URL。
    #[inline]
    pub fn set_source(&self, source: &str) {
        self.0.set_source(source)
    }

    /// 読み込めているか。読み込みに失敗したときは `false`。
    #[inline]
    pub fn is_loaded(&self) -> bool {
        self.0.is_loaded()
    }

    #[inline]
    pub fn set_fit(&self, fit: Fit) {
        self.0.set_fit(fit)
    }

    /// 読み上げなどに使う代替テキスト。
    #[inline]
    pub fn set_alt(&self, text: &str) {
        self.0.set_alt(text)
    }
}

/// 動画 (`GtkPicture` に `GtkMediaFile` を映し、下に `GtkMediaControls`)。
#[derive(Clone)]
pub struct Video(pub(crate) sys::Video);

impl_widget!(Video);

impl Video {
    #[inline]
    pub fn source(&self) -> String {
        self.0.source()
    }

    /// 鳴らすものを差し替える。`source` はファイルパスか URL。
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

    /// 再生位置を秒で指定する。
    #[inline]
    pub fn seek(&self, seconds: f64) {
        self.0.seek(seconds)
    }

    /// いまの再生位置 (秒)。
    #[inline]
    pub fn position(&self) -> f64 {
        self.0.position()
    }

    /// 長さ (秒)。まだ分からなければ `None`。
    #[inline]
    pub fn duration(&self) -> Option<f64> {
        self.0.duration()
    }

    /// 0.0..=1.0。
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

    /// 読み込めたら自動で再生を始めるか。
    #[inline]
    pub fn set_autoplay(&self, autoplay: bool) {
        self.0.set_autoplay(autoplay)
    }

    /// 再生バーを出すか (既定は出す)。
    #[inline]
    pub fn set_controls(&self, controls: bool) {
        self.0.set_controls(controls)
    }

    /// 再生の状態が変わるたびに呼ばれる。
    ///
    /// アプリから `play()` を呼んだときだけでなく、**GTK4 の再生バーを
    /// ユーザーが操作したときにも届く**。
    #[inline]
    pub fn on_state_change(&self, f: impl FnMut(PlaybackState) + 'static) {
        self.0.on_state_change(f)
    }

    /// 再生位置が変わるたびに、その秒数で呼ばれる。
    #[inline]
    pub fn on_position_change(&self, f: impl FnMut(f64) + 'static) {
        self.0.on_position_change(f)
    }

    /// 表示領域に対する映像の収め方。
    #[inline]
    pub fn set_fit(&self, fit: Fit) {
        self.0.set_fit(fit)
    }
}
