//! Images, video and audio.
//!
//! Images are handled by **NSImageView**, and video and audio by **AVKit's
//! AVPlayerView**. Opening files, decoding and drawing the playback bar are all the
//! job of AppKit / AVFoundation; naui only passes URLs and sets properties.
//!
//! Playback state notifications are made by observing AVPlayer's
//! `timeControlStatus` with KVO. They arrive through the same path not only when
//! `play()` is called but **also when the user clicks AVPlayerView's playback
//! bar**.

use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::rc::{Rc, Weak};

use block2::RcBlock;
use naui_core::{Fit, PlaybackState, Sizing};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol};
use objc2::{
    define_class, msg_send, sel, AnyThread, ClassType, DefinedClass, MainThreadMarker,
    MainThreadOnly, Message,
};
use objc2_app_kit::{
    NSAccessibility, NSCompositingOperation, NSImage, NSImageScaling, NSImageView, NSView,
};
use objc2_av_foundation::{
    AVLayerVideoGravity, AVLayerVideoGravityResize, AVLayerVideoGravityResizeAspect,
    AVLayerVideoGravityResizeAspectFill, AVPlayer, AVPlayerActionAtItemEnd, AVPlayerItem,
    AVPlayerItemDidPlayToEndTimeNotification, AVPlayerTimeControlStatus,
};
use objc2_av_kit::{AVPlayerView, AVPlayerViewControlsStyle};
use objc2_core_media::{CMTime, CMTimeFlags};
use objc2_foundation::{
    NSNotification, NSNotificationCenter, NSObjectNSKeyValueObserverRegistration, NSPoint, NSRect,
    NSSize, NSString, NSURL,
};

use crate::widgets::{impl_widget, Widget};

/// The AVPlayer property observed with KVO.
const TIME_CONTROL_STATUS: &str = "timeControlStatus";

/// The time resolution of the CMTime created by `seek` (how many parts a second is
/// divided into).
///
/// 600 divides evenly by 24 / 25 / 30 / 60 fps and is the value conventionally used
/// in AVFoundation.
const TIMESCALE: i32 = 600;

/// The interval (seconds) at which the playback position is reported.
///
/// Four times a second, roughly matching how often HTMLMediaElement's `timeupdate`
/// fires.
const POSITION_INTERVAL: f64 = 0.25;

/// Turns a media location into an NSURL.
///
/// If `is_url` is true it is interpreted as is; otherwise it is taken as a local
/// file path. Paths are passed to `fileURLWithPath:`, leaving the encoding to
/// Foundation.
fn to_url(source: &str) -> Option<Retained<NSURL>> {
    if source.is_empty() {
        return None;
    }
    if naui_core::media::is_url(source) {
        NSURL::URLWithString(&NSString::from_str(source))
    } else {
        Some(NSURL::fileURLWithPath(&NSString::from_str(source)))
    }
}

// ------------------------------------------------------------------ Image

struct ImageInner {
    native: Retained<NauiImageView>,
    source: RefCell<String>,
}

// `NSImageView` に無い `Fit::Cover` を、表示領域いっぱいに拡大して描く画像ビュー。
define_class!(
    #[unsafe(super(NSImageView))]
    #[thread_kind = MainThreadOnly]
    #[name = "NauiImageView"]
    #[ivars = Cell<Fit>]
    struct NauiImageView;

    unsafe impl NSObjectProtocol for NauiImageView {}

    impl NauiImageView {
        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, dirty_rect: NSRect) {
            if self.ivars().get() != Fit::Cover {
                let _: () = unsafe { msg_send![super(self), drawRect: dirty_rect] };
                return;
            }

            let Some(image) = self.as_super().image() else {
                return;
            };
            let bounds = self.as_super().bounds();
            let Some(destination) = cover_rect(bounds, image.size()) else {
                return;
            };
            let source = NSRect::new(NSPoint::new(0.0, 0.0), image.size());
            image.drawInRect_fromRect_operation_fraction(
                destination,
                source,
                NSCompositingOperation::SourceOver,
                1.0,
            );
        }
    }
);

impl NauiImageView {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(Cell::new(Fit::default()));
        let this: Retained<Self> = unsafe { msg_send![super(this), init] };
        this.as_super().setClipsToBounds(true);
        this
    }

    fn set_fit(&self, fit: Fit) {
        self.ivars().set(fit);
        // Contain / Fill / None は NSImageView の描画へ戻す。Cover だけは
        // drawRect で独自に描くが、ネイティブ側の状態も一貫させておく。
        self.as_super().setImageScaling(match fit {
            Fit::Contain | Fit::Cover => NSImageScaling::ScaleProportionallyUpOrDown,
            Fit::Fill => NSImageScaling::ScaleAxesIndependently,
            Fit::None => NSImageScaling::ScaleNone,
        });
        self.as_super().as_super().as_super().setNeedsDisplay(true);
    }
}

/// Computes a draw rect that covers the display area while keeping the image's
/// aspect ratio.
fn cover_rect(bounds: NSRect, image: NSSize) -> Option<NSRect> {
    let width = bounds.size.width;
    let height = bounds.size.height;
    if width <= 0.0 || height <= 0.0 || image.width <= 0.0 || image.height <= 0.0 {
        return None;
    }

    let scale = (width / image.width).max(height / image.height);
    let size = NSSize::new(image.width * scale, image.height * scale);
    Some(NSRect::new(
        NSPoint::new(
            bounds.origin.x + (width - size.width) / 2.0,
            bounds.origin.y + (height - size.height) / 2.0,
        ),
        size,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cover_rect_fills_the_bounds_while_preserving_aspect_ratio() {
        let bounds = NSRect::new(NSPoint::new(10.0, 20.0), NSSize::new(400.0, 200.0));
        let rect = cover_rect(bounds, NSSize::new(200.0, 400.0)).expect("有効な矩形");

        // 縦長画像を横長の領域へ cover すると、上下がはみ出す。
        assert_eq!(rect.size.width, 400.0);
        assert_eq!(rect.size.height, 800.0);
        assert_eq!(rect.origin.x, 10.0);
        assert_eq!(rect.origin.y, -280.0);
    }

    #[test]
    fn cover_rect_rejects_empty_sizes() {
        let bounds = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(100.0, 100.0));
        assert!(cover_rect(bounds, NSSize::new(0.0, 100.0)).is_none());
        assert!(cover_rect(bounds, NSSize::new(100.0, 0.0)).is_none());
    }
}

/// Image display (NSImageView).
///
/// Loading is done by `NSImage`. **It loads synchronously, so specifying a remote
/// URL freezes the UI until it has loaded.** Pass a local file, or download it
/// beforehand and pass that.
#[derive(Clone)]
pub struct Image(Rc<ImageInner>);
impl Image {
    pub(crate) fn new(mtm: MainThreadMarker, source: &str) -> Self {
        let native = NauiImageView::new(mtm);
        let this = Self(Rc::new(ImageInner {
            native,
            source: RefCell::new(String::new()),
        }));
        this.set_fit(Fit::default());
        this.set_source(source);
        this
    }

    /// The location currently specified (exactly the string that was passed).
    pub fn source(&self) -> String {
        self.0.source.borrow().clone()
    }

    /// The location of the image to show. Either a file path or a URL.
    ///
    /// Passing an empty string removes the image. The image is also removed if it could
    /// not be loaded (because NSImage returns nil).
    pub fn set_source(&self, source: &str) {
        *self.0.source.borrow_mut() = source.to_string();
        let image =
            to_url(source).and_then(|url| NSImage::initWithContentsOfURL(NSImage::alloc(), &url));
        self.0.native.as_super().setImage(image.as_deref());
    }

    /// Whether it has loaded.
    pub fn is_loaded(&self) -> bool {
        self.0.native.as_super().image().is_some()
    }

    /// How it is fitted into the display area.
    ///
    /// NSImageView has no "fill even if cropped" setting, so only [`Fit::Cover`] draws
    /// it scaled up in `drawRect`, keeping the aspect ratio.
    pub fn set_fit(&self, fit: Fit) {
        self.0.native.set_fit(fit);
    }

    /// Text describing the image's content. Read aloud by VoiceOver.
    pub fn set_alt(&self, text: &str) {
        self.0
            .native
            .as_super()
            .setAccessibilityLabel(Some(&NSString::from_str(text)));
    }
}

impl Widget for Image {
    fn native_view(&self) -> Retained<NSView> {
        self.0.native.clone().into_super().into_super().into_super()
    }

    fn boxed_clone(&self) -> Box<dyn Widget> {
        Box::new(self.clone())
    }
}

crate::widgets::impl_sizing!(Image);

// --------------------------------------------------------------- 再生の中身

/// The closure called when the playback state changes.
///
/// So that touching the same widget from inside the closure does not double-borrow,
/// it is taken out of the RefCell only for the duration of the call.
type StateCallback = RefCell<Option<Box<dyn FnMut(PlaybackState)>>>;

/// The closure called when the playback position advances.
type PositionCallback = RefCell<Option<Box<dyn FnMut(f64)>>>;

/// The playback object shared by video and audio.
///
/// A pair of AVPlayerView (screen) and AVPlayer (playback); both widgets hold it in
/// an `Rc`. The only difference is whether the picture is used.
struct PlaybackInner {
    native: Retained<AVPlayerView>,
    player: Retained<AVPlayer>,
    source: RefCell<String>,
    looping: Cell<bool>,
    autoplay: Cell<bool>,
    state: Cell<PlaybackState>,
    callback: StateCallback,
    position_callback: PositionCallback,
    /// The receiver of KVO and notifications. Held so that Drop can remove the
    /// registrations.
    observer: RefCell<Option<Retained<PlaybackObserver>>>,
    /// The observer that periodically reports the playback position. Held so that Drop
    /// can remove it.
    ///
    /// AVFoundation states explicitly that freeing it without calling
    /// `removeTimeObserver:` is undefined behavior, so it is always removed before
    /// being let go.
    time_observer: RefCell<Option<Retained<AnyObject>>>,
}

impl PlaybackInner {
    fn new(mtm: MainThreadMarker) -> Rc<Self> {
        let player = unsafe { AVPlayer::new(mtm) };
        // 末尾で止まったことを自分で扱うため、AVPlayer には何もさせない。
        unsafe { player.setActionAtItemEnd(AVPlayerActionAtItemEnd::Pause) };
        let native = unsafe { AVPlayerView::new(mtm) };
        unsafe {
            native.setPlayer(Some(&player));
            native.setControlsStyle(AVPlayerViewControlsStyle::Default);
        }

        let this = Rc::new(Self {
            native,
            player,
            source: RefCell::new(String::new()),
            looping: Cell::new(false),
            autoplay: Cell::new(false),
            state: Cell::new(PlaybackState::Idle),
            callback: RefCell::new(None),
            position_callback: RefCell::new(None),
            observer: RefCell::new(None),
            time_observer: RefCell::new(None),
        });

        // 再生位置は KVO では観測できない (AVFoundation のドキュメントが
        // 明記している)。定期的に呼ばれるブロックで受け取る。
        // キューに None を渡すとメインキューになるので、
        // ブロックはメインスレッドで走る = Rc を触ってよい。
        let weak = Rc::downgrade(&this);
        let block = RcBlock::new(move |time: CMTime| {
            let Some(inner) = weak.upgrade() else {
                return;
            };
            inner.emit_position(seconds(time).unwrap_or(0.0));
        });
        let token = unsafe {
            this.player
                .addPeriodicTimeObserverForInterval_queue_usingBlock(
                    CMTime::with_seconds(POSITION_INTERVAL, TIMESCALE),
                    None,
                    &block,
                )
        };
        *this.time_observer.borrow_mut() = Some(token);

        let observer = PlaybackObserver::new(mtm, Rc::downgrade(&this));
        unsafe {
            this.player.addObserver_forKeyPath_options_context(
                &observer,
                &NSString::from_str(TIME_CONTROL_STATUS),
                objc2_foundation::NSKeyValueObservingOptions::New,
                std::ptr::null_mut(),
            );
            // 項目を差し替えても購読し直さずに済むよう、送り主は限定しない。
            // どの項目からの通知かは受け取ってから確かめる。
            NSNotificationCenter::defaultCenter().addObserver_selector_name_object(
                &observer,
                sel!(itemDidPlayToEnd:),
                Some(AVPlayerItemDidPlayToEndTimeNotification),
                None,
            );
        }
        *this.observer.borrow_mut() = Some(observer);
        this
    }

    fn set_source(&self, source: &str) {
        *self.source.borrow_mut() = source.to_string();
        let item = to_url(source).map(|url| {
            let mtm = MainThreadMarker::from(&*self.native);
            unsafe { AVPlayerItem::playerItemWithURL(&url, mtm) }
        });
        unsafe {
            self.player
                .replaceCurrentItemWithPlayerItem(item.as_deref())
        };
        self.emit(PlaybackState::Idle);
        if self.autoplay.get() && item.is_some() {
            self.play();
        }
    }

    fn play(&self) {
        // 末尾まで再生し終えた後の `play()` は、AVPlayer では何も起こらない。
        // 先頭へ戻してから再生し、他のバックエンドと同じ挙動にそろえる。
        if self.state.get() == PlaybackState::Ended {
            self.seek(0.0);
        }
        unsafe { self.player.play() };
    }

    fn pause(&self) {
        unsafe { self.player.pause() };
    }

    fn seek(&self, seconds: f64) {
        let time = unsafe { CMTime::with_seconds(seconds.max(0.0), TIMESCALE) };
        unsafe { self.player.seekToTime(time) };
        // 先頭へ戻したら「再生し終えた」ではなくなる。
        if self.state.get() == PlaybackState::Ended {
            self.emit(PlaybackState::Paused);
        }
    }

    fn position(&self) -> f64 {
        seconds(unsafe { self.player.currentTime() }).unwrap_or(0.0)
    }

    fn duration(&self) -> Option<f64> {
        let item = unsafe { self.player.currentItem() }?;
        seconds(unsafe { item.duration() })
    }

    /// Called from KVO and notifications; maps AVPlayer's state onto naui's state.
    fn sync_state(&self) {
        // メディアが無いときの AVPlayer は「一時停止」を名乗るが、
        // 止めたのではなく、まだ何も渡していないだけ。
        if unsafe { self.player.currentItem() }.is_none() {
            self.emit(PlaybackState::Idle);
            return;
        }
        let status = unsafe { self.player.timeControlStatus() };
        let state = if status == AVPlayerTimeControlStatus::Playing {
            PlaybackState::Playing
        } else if status == AVPlayerTimeControlStatus::WaitingToPlayAtSpecifiedRate {
            PlaybackState::Buffering
        } else if self.state.get() == PlaybackState::Ended {
            // 末尾で止まったことによる Paused。Ended のままにしておく。
            PlaybackState::Ended
        } else {
            PlaybackState::Paused
        };
        self.emit(state);
    }

    /// The notification that playback reached the end. Only those from the item
    /// currently playing are handled.
    fn item_did_play_to_end(&self, notification: &NSNotification) {
        let Some(sender) = notification.object() else {
            return;
        };
        let Some(current) = (unsafe { self.player.currentItem() }) else {
            return;
        };
        if !std::ptr::eq(
            &*sender as *const AnyObject,
            &*current as *const AVPlayerItem as _,
        ) {
            return;
        }
        if self.looping.get() {
            // 末尾では止まらない設定にしてあるので、鳴らしたまま先頭へ戻す。
            // `play()` は取りこぼしの保険 (何かの拍子に止まっていたとき用)。
            self.seek(0.0);
            unsafe { self.player.play() };
            return;
        }
        self.emit(PlaybackState::Ended);
    }

    /// Notifies the playback position.
    fn emit_position(&self, seconds: f64) {
        // 状態の通知と同じく、呼び出しの間だけ取り出して再入を避ける。
        let Some(mut f) = self.position_callback.borrow_mut().take() else {
            return;
        };
        f(seconds);
        let mut slot = self.position_callback.borrow_mut();
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
        let Some(mut f) = self.callback.borrow_mut().take() else {
            return;
        };
        f(state);
        // 呼び出し中に差し替えられていたら、新しいほうを残す。
        let mut slot = self.callback.borrow_mut();
        if slot.is_none() {
            *slot = Some(f);
        }
    }
}

impl Drop for PlaybackInner {
    fn drop(&mut self) {
        // 定期観測は、外さずに解放すると未定義動作になる。
        if let Some(token) = self.time_observer.borrow_mut().take() {
            unsafe { self.player.removeTimeObserver(&token) };
        }
        // KVO を張ったまま AVPlayer を解放すると AppKit が異常終了する。
        let Some(observer) = self.observer.borrow_mut().take() else {
            return;
        };
        unsafe {
            self.player
                .removeObserver_forKeyPath(&observer, &NSString::from_str(TIME_CONTROL_STATUS));
            NSNotificationCenter::defaultCenter().removeObserver(&observer);
        }
    }
}

/// Converts a CMTime to seconds. `None` if the time is not defined.
///
/// The duration is `indefinite` until loading finishes, so it is `None` meanwhile.
fn seconds(time: CMTime) -> Option<f64> {
    if !time.flags.contains(CMTimeFlags::Valid) || time.timescale == 0 {
        return None;
    }
    // Indefinite / ±Infinity は値ではなく状態を表す。
    if time.flags.intersects(CMTimeFlags::ImpliedValueFlagsMask) {
        return None;
    }
    Some(time.value as f64 / time.timescale as f64)
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "NauiPlaybackObserver"]
    #[ivars = Weak<PlaybackInner>]
    /// Relays AVPlayer's KVO and end-of-playback notifications to the Rust side.
    ///
    /// It holds the widget weakly. A strong reference would create a cycle, since the
    /// widget holds the observer, and it would never be freed.
    struct PlaybackObserver;

    unsafe impl NSObjectProtocol for PlaybackObserver {}

    impl PlaybackObserver {
        #[unsafe(method(observeValueForKeyPath:ofObject:change:context:))]
        fn observe_value(
            &self,
            _key_path: Option<&NSString>,
            _object: Option<&AnyObject>,
            _change: Option<&AnyObject>,
            _context: *mut c_void,
        ) {
            if let Some(inner) = self.ivars().upgrade() {
                inner.sync_state();
            }
        }

        #[unsafe(method(itemDidPlayToEnd:))]
        fn item_did_play_to_end(&self, notification: &NSNotification) {
            if let Some(inner) = self.ivars().upgrade() {
                inner.item_did_play_to_end(notification);
            }
        }
    }
);

impl PlaybackObserver {
    fn new(mtm: MainThreadMarker, inner: Weak<PlaybackInner>) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(inner);
        unsafe { msg_send![super(this), init] }
    }
}

/// Adds the playback API shared by video and audio.
///
/// The only difference between the two is whether the picture is used, so every
/// operation has the same shape.
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
                self.0.pause();
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
                self.0.position()
            }

            /// The length of the media (seconds). **`None` until loading finishes.**
            ///
            /// Also returns `None` for streams whose length is undefined (live streams and so
            /// on).
            pub fn duration(&self) -> Option<f64> {
                self.0.duration()
            }

            /// The volume (0.0..=1.0). Out-of-range values are clamped.
            pub fn set_volume(&self, volume: f64) {
                unsafe { self.0.player.setVolume(volume.clamp(0.0, 1.0) as f32) };
            }

            pub fn volume(&self) -> f64 {
                unsafe { self.0.player.volume() as f64 }
            }

            /// Mutes it. Only the sound stops; the volume value is kept.
            pub fn set_muted(&self, muted: bool) {
                unsafe { self.0.player.setMuted(muted) };
            }

            pub fn is_muted(&self) -> bool {
                unsafe { self.0.player.isMuted() }
            }

            /// After playing to the end, goes back to the start and repeats.
            pub fn set_loop(&self, looping: bool) {
                self.0.looping.set(looping);
                // 繰り返すなら末尾で止めさせない。止めてしまうと、先頭へ
                // 戻して鳴らし直すまでの間だけ「一時停止」が通知され、
                // 繰り返しているのに再生が途切れたように見える。
                let action = if looping {
                    AVPlayerActionAtItemEnd::None
                } else {
                    AVPlayerActionAtItemEnd::Pause
                };
                unsafe { self.0.player.setActionAtItemEnd(action) };
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
                if autoplay
                    && self.0.state.get() == PlaybackState::Idle
                    && !self.0.source.borrow().is_empty()
                {
                    self.0.play();
                }
            }

            /// Whether to show the native playback bar (shown by default).
            pub fn set_controls(&self, controls: bool) {
                let style = if controls {
                    AVPlayerViewControlsStyle::Default
                } else {
                    AVPlayerViewControlsStyle::None
                };
                unsafe { self.0.native.setControlsStyle(style) };
            }

            /// Called when the playback state changes. Setting it again removes the previous
            /// one.
            ///
            /// It arrives not only when the app calls [`play`](Self::play) but **also when the
            /// user operates AVPlayerView's playback bar**.
            pub fn on_state_change(&self, f: impl FnMut(PlaybackState) + 'static) {
                *self.0.callback.borrow_mut() = Some(Box::new(f));
            }

            /// Called with the position (seconds) every time the playback position advances.
            ///
            /// For keeping a seek bar in step with playback. The native side decides the
            /// interval, but it is about four times a second, and it also arrives right after
            /// playback starts, stops or seeks. **It is not called while not playing.**
            pub fn on_position_change(&self, f: impl FnMut(f64) + 'static) {
                *self.0.position_callback.borrow_mut() = Some(Box::new(f));
            }
        }
    };
}

// ------------------------------------------------------------------ Video

/// Video (AVKit's AVPlayerView).
#[derive(Clone)]
pub struct Video(Rc<PlaybackInner>);
impl_widget!(Video);
impl_playback!(Video);

impl Video {
    pub(crate) fn new(mtm: MainThreadMarker, source: &str) -> Self {
        let inner = PlaybackInner::new(mtm);
        let this = Self(inner);
        this.set_fit(Fit::default());
        this.set_source(source);
        this
    }

    /// How the picture is fitted. Mapped onto AVPlayerLayer's videoGravity.
    ///
    /// AVFoundation has no "place at actual size", so [`Fit::None`] is treated the
    /// same as [`Fit::Contain`].
    pub fn set_fit(&self, fit: Fit) {
        let gravity: Option<&AVLayerVideoGravity> = unsafe {
            match fit {
                Fit::Contain | Fit::None => AVLayerVideoGravityResizeAspect,
                Fit::Cover => AVLayerVideoGravityResizeAspectFill,
                Fit::Fill => AVLayerVideoGravityResize,
            }
        };
        if let Some(gravity) = gravity {
            unsafe { self.0.native.setVideoGravity(gravity) };
        }
    }
}

// ------------------------------------------------------------------ Audio

/// Audio (AVKit's AVPlayerView; with no picture, only the playback bar is visible).
///
/// AppKit has no standalone "audio player" control. Media without a video track
/// placed in an AVPlayerView shows only the playback bar.
#[derive(Clone)]
pub struct Audio(Rc<PlaybackInner>);
impl_widget!(Audio);
impl_playback!(Audio);

impl Audio {
    /// The minimum height at which the playback bar is visible. Measured from
    /// AVPlayerView's inline style.
    const CONTROLS_HEIGHT: f64 = 38.0;

    pub(crate) fn new(mtm: MainThreadMarker, source: &str) -> Self {
        let this = Self(PlaybackInner::new(mtm));
        // 映像が無いと AVPlayerView の要求サイズが 0 になり、再生バーごと
        // 潰れてしまう。`set_sizing` を呼べば置き換わる。
        this.set_sizing(Sizing::new().min_height(Self::CONTROLS_HEIGHT));
        this.set_source(source);
        this
    }
}
