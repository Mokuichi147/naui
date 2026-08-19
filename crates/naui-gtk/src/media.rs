//! 画像・動画・音声 (`GtkPicture` / `GtkMediaFile` / `GtkMediaControls`)。
//!
//! デコードも再生も naui は行わない。GStreamer に載った GTK4 のメディア
//! 再生 (`GtkMediaFile`) がそのまま鳴らし、映像は `GdkPaintable` として
//! `GtkPicture` に描かれる。
//!
//! 動画を `GtkVideo` ではなく「`GtkPicture` + `GtkMediaControls`」で組むのは、
//! `GtkVideo` が収め方 ([`Fit`]) と再生バーの出し入れを外から変えられないため。
//! どちらも `GtkMediaFile` を映しているので、再生そのものの扱いは変わらない。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::gio;
use gtk::prelude::*;
use naui_core::media::source_url;
use naui_core::{Fit, PlaybackState};

use crate::bin::SizeBin;
use crate::callback::Notifier;
use crate::widgets::{impl_widget, Widget};

/// naui の収め方を `GtkPicture` の `content-fit` へ写す。
///
/// [`Fit::None`] (原寸) にぴったり合うものが GTK4 に無いため、
/// **拡大はしないが縮小はする** `ScaleDown` を使う。
fn to_content_fit(fit: Fit) -> gtk::ContentFit {
    match fit {
        Fit::Contain => gtk::ContentFit::Contain,
        Fit::Cover => gtk::ContentFit::Cover,
        Fit::Fill => gtk::ContentFit::Fill,
        Fit::None => gtk::ContentFit::ScaleDown,
    }
}

/// 場所の文字列から `GFile` を作る。
fn to_file(source: &str) -> gio::File {
    gio::File::for_uri(&source_url(source))
}

// ------------------------------------------------------------------ Image

struct ImageInner {
    native: gtk::Picture,
    bin: SizeBin,
    source: RefCell<String>,
}

/// 画像 (`GtkPicture`)。
#[derive(Clone)]
pub struct Image(Rc<ImageInner>);
impl_widget!(Image);

impl Image {
    pub(crate) fn new(source: &str) -> Self {
        let native = gtk::Picture::new();
        native.set_content_fit(gtk::ContentFit::Contain);
        let bin = SizeBin::wrap(&native);
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

    /// 画像を差し替える。`source` はファイルパスか URL。
    pub fn set_source(&self, source: &str) {
        *self.0.source.borrow_mut() = source.to_string();
        if source.is_empty() {
            self.0.native.set_paintable(None::<&gtk::gdk::Paintable>);
        } else {
            self.0.native.set_file(Some(&to_file(source)));
        }
    }

    /// 読み込めているか。読み込みに失敗したときは `false`。
    pub fn is_loaded(&self) -> bool {
        self.0.native.paintable().is_some()
    }

    pub fn set_fit(&self, fit: Fit) {
        self.0.native.set_content_fit(to_content_fit(fit));
    }

    /// 読み上げなどに使う代替テキスト。
    pub fn set_alt(&self, text: &str) {
        self.0.native.set_alternative_text(Some(text));
    }
}

// --------------------------------------------------------------- Playback

/// 動画と音声で共有する再生の中身。
pub(crate) struct PlaybackInner {
    /// いま鳴らしているもの。`set_source` で作り直す。
    stream: RefCell<Option<gtk::MediaFile>>,
    /// 再生バー。動画では映像の下、音声ではそれ自体が本体になる。
    controls: gtk::MediaControls,
    source: RefCell<String>,
    /// 一度でも再生を始めたか。まだなら [`PlaybackState::Idle`]。
    started: Cell<bool>,
    autoplay: Cell<bool>,
    looping: Cell<bool>,
    volume: Cell<f64>,
    muted: Cell<bool>,
    on_state: Notifier<PlaybackState>,
    on_position: Notifier<f64>,
    /// 映像を映す面。音声では持たない。
    picture: Option<gtk::Picture>,
}

impl PlaybackInner {
    fn new(picture: Option<gtk::Picture>) -> Rc<Self> {
        let controls = gtk::MediaControls::new(None::<&gtk::MediaStream>);
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

    /// 状態にかかわるプロパティが動いたときの後始末。
    ///
    /// 自動再生は「読み込めた時点」で始めるので、用意ができるのを待って
    /// ここから始める。
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

    /// いまの状態を GTK4 のプロパティから組み立てる。
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

/// 再生の API を動画と音声の両方に生やす。
macro_rules! impl_playback {
    ($t:ty) => {
        impl $t {
            pub fn source(&self) -> String {
                self.0.playback.source.borrow().clone()
            }

            /// 鳴らすものを差し替える。`source` はファイルパスか URL。
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

            /// 再生位置を秒で指定する。
            pub fn seek(&self, seconds: f64) {
                if let Some(stream) = self.0.playback.stream.borrow().clone() {
                    stream.seek(crate::media::to_micros(seconds));
                }
            }

            /// いまの再生位置 (秒)。
            pub fn position(&self) -> f64 {
                self.0
                    .playback
                    .stream
                    .borrow()
                    .as_ref()
                    .map(|s| crate::media::to_seconds(s.timestamp()))
                    .unwrap_or(0.0)
            }

            /// 長さ (秒)。まだ分からなければ `None`。
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

            /// 0.0..=1.0。
            pub fn set_volume(&self, volume: f64) {
                let volume = volume.clamp(0.0, 1.0);
                self.0.playback.volume.set(volume);
                if let Some(stream) = self.0.playback.stream.borrow().clone() {
                    stream.set_volume(volume);
                }
            }

            pub fn volume(&self) -> f64 {
                self.0.playback.volume.get()
            }

            pub fn set_muted(&self, muted: bool) {
                self.0.playback.muted.set(muted);
                if let Some(stream) = self.0.playback.stream.borrow().clone() {
                    stream.set_muted(muted);
                }
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

            /// 読み込めたら自動で再生を始めるか。
            pub fn set_autoplay(&self, autoplay: bool) {
                self.0.playback.autoplay.set(autoplay);
                if autoplay {
                    self.play();
                }
            }

            /// 再生バーを出すか (既定は出す)。
            pub fn set_controls(&self, controls: bool) {
                self.0.playback.controls.set_visible(controls);
            }

            /// 再生の状態が変わるたびに呼ばれる。
            ///
            /// アプリから `play()` を呼んだときだけでなく、**GTK4 の再生バーを
            /// ユーザーが操作したときにも届く**。
            pub fn on_state_change(&self, f: impl FnMut(PlaybackState) + 'static) {
                self.0.playback.on_state.set(f);
            }

            /// 再生位置が変わるたびに、その秒数で呼ばれる。
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

/// 鳴らすものを作り直し、状態と位置の通知をつなぎ直す。
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
    stream.set_volume(playback.volume.get());
    stream.set_muted(playback.muted.get());
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

    playback.controls.set_media_stream(Some(&stream));
    if let Some(picture) = &playback.picture {
        picture.set_paintable(Some(&stream));
    }
    *playback.stream.borrow_mut() = Some(stream);

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

/// 動画 (`GtkPicture` に `GtkMediaFile` を映し、下に `GtkMediaControls`)。
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
        let video = Self(Rc::new(VideoInner {
            native,
            bin,
            picture,
            playback,
        }));
        video.set_source(source);
        video
    }

    /// 表示領域に対する映像の収め方。
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

/// 音声 (`GtkMediaControls` + `GtkMediaFile`)。映像面を持たない。
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
