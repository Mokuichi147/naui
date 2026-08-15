//! 画像・動画・音声。
//!
//! 画像は **NSImageView**、動画と音声は **AVKit の AVPlayerView** が担う。
//! ファイルを開くのも、デコードも、再生バーの描画も AppKit / AVFoundation の
//! 仕事で、miui 側は URL を渡してプロパティを設定するだけ。
//!
//! 再生状態の通知は、AVPlayer の `timeControlStatus` を KVO で監視して行う。
//! `play()` を呼んだときだけでなく、**AVPlayerView の再生バーをユーザーが
//! 押したときにも同じ経路で届く**。

use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::rc::{Rc, Weak};

use miui_core::{Fit, PlaybackState, Sizing};
use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol};
use objc2::{
    define_class, msg_send, sel, AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, Message,
};
use objc2_app_kit::{NSAccessibility, NSImage, NSImageScaling, NSImageView, NSView};
use objc2_av_foundation::{
    AVLayerVideoGravity, AVLayerVideoGravityResize, AVLayerVideoGravityResizeAspect,
    AVLayerVideoGravityResizeAspectFill, AVPlayer, AVPlayerActionAtItemEnd, AVPlayerItem,
    AVPlayerItemDidPlayToEndTimeNotification, AVPlayerTimeControlStatus,
};
use objc2_av_kit::{AVPlayerView, AVPlayerViewControlsStyle};
use objc2_core_media::{CMTime, CMTimeFlags};
use objc2_foundation::{
    NSNotification, NSNotificationCenter, NSObjectNSKeyValueObserverRegistration, NSString, NSURL,
};

use crate::widgets::{impl_widget, Widget};

/// KVO で監視する AVPlayer のプロパティ。
const TIME_CONTROL_STATUS: &str = "timeControlStatus";

/// `seek` で作る CMTime の時間分解能 (1 秒を何分割するか)。
///
/// 600 は 24 / 25 / 30 / 60 fps のいずれも割り切れる、AVFoundation で
/// 慣例的に使われる値。
const TIMESCALE: i32 = 600;

/// 再生位置を知らせる間隔 (秒)。
///
/// 4 回/秒。HTMLMediaElement の `timeupdate` が出る間隔とおおよそそろえてある。
const POSITION_INTERVAL: f64 = 0.25;

/// メディアの場所を NSURL にする。
///
/// `is_url` が真ならそのまま解釈し、そうでなければローカルのファイルパスと
/// みなす。パスは `fileURLWithPath:` に渡し、エンコードは Foundation に任せる。
fn to_url(source: &str) -> Option<Retained<NSURL>> {
    if source.is_empty() {
        return None;
    }
    if miui_core::media::is_url(source) {
        NSURL::URLWithString(&NSString::from_str(source))
    } else {
        Some(NSURL::fileURLWithPath(&NSString::from_str(source)))
    }
}

// ------------------------------------------------------------------ Image

struct ImageInner {
    native: Retained<NSImageView>,
    source: RefCell<String>,
}

/// 画像表示 (NSImageView)。
///
/// 読み込みは `NSImage` が行う。**同期的に読むため、リモートの URL を
/// 指定すると読み終わるまで UI が止まる。** ローカルのファイルを渡すか、
/// あらかじめ手元へ落としてから渡すこと。
#[derive(Clone)]
pub struct Image(Rc<ImageInner>);
impl_widget!(Image);

impl Image {
    pub(crate) fn new(mtm: MainThreadMarker, source: &str) -> Self {
        let native = NSImageView::new(mtm);
        // Fit::None は画像を原寸で描画するため、表示領域より大きい画像が
        // 周囲のビューへはみ出さないよう NSImageView の境界で切り取る。
        native.setClipsToBounds(true);
        let this = Self(Rc::new(ImageInner {
            native,
            source: RefCell::new(String::new()),
        }));
        this.set_fit(Fit::default());
        this.set_source(source);
        this
    }

    /// いま指定されている場所 (渡した文字列そのまま)。
    pub fn source(&self) -> String {
        self.0.source.borrow().clone()
    }

    /// 表示する画像の場所。ファイルパスと URL のどちらでもよい。
    ///
    /// 空文字列を渡すと画像を外す。読み込めなかった場合も画像は外れる
    /// (NSImage が nil を返すため)。
    pub fn set_source(&self, source: &str) {
        *self.0.source.borrow_mut() = source.to_string();
        let image =
            to_url(source).and_then(|url| NSImage::initWithContentsOfURL(NSImage::alloc(), &url));
        self.0.native.setImage(image.as_deref());
    }

    /// 読み込めているか。
    pub fn is_loaded(&self) -> bool {
        self.0.native.image().is_some()
    }

    /// 表示領域への収め方。
    ///
    /// NSImageView に「切り取ってでも埋める」設定は無いため、
    /// [`Fit::Cover`] は [`Fit::Contain`] と同じ拡縮になる。
    pub fn set_fit(&self, fit: Fit) {
        self.0.native.setImageScaling(match fit {
            Fit::Contain | Fit::Cover => NSImageScaling::ScaleProportionallyUpOrDown,
            Fit::Fill => NSImageScaling::ScaleAxesIndependently,
            Fit::None => NSImageScaling::ScaleNone,
        });
    }

    /// 画像の内容を表す文字列。VoiceOver が読み上げる。
    pub fn set_alt(&self, text: &str) {
        self.0
            .native
            .setAccessibilityLabel(Some(&NSString::from_str(text)));
    }
}

// --------------------------------------------------------------- 再生の中身

/// 再生状態が変わったときのクロージャ。
///
/// クロージャの中から同じウィジェットを触っても二重借用にならないよう、
/// 呼び出しの間だけ RefCell から取り出す。
type StateCallback = RefCell<Option<Box<dyn FnMut(PlaybackState)>>>;

/// 再生位置が進んだときのクロージャ。
type PositionCallback = RefCell<Option<Box<dyn FnMut(f64)>>>;

/// 動画と音声で共有する再生の実体。
///
/// AVPlayerView (画面) と AVPlayer (再生) の組で、どちらのウィジェットも
/// これを `Rc` で持つ。違いは映像面を使うかどうかだけ。
struct PlaybackInner {
    native: Retained<AVPlayerView>,
    player: Retained<AVPlayer>,
    source: RefCell<String>,
    looping: Cell<bool>,
    autoplay: Cell<bool>,
    state: Cell<PlaybackState>,
    callback: StateCallback,
    position_callback: PositionCallback,
    /// KVO と通知の受け口。Drop で登録を外すため保持する。
    observer: RefCell<Option<Retained<PlaybackObserver>>>,
    /// 定期的に再生位置を知らせる観測者。Drop で外すため保持する。
    ///
    /// AVFoundation は「`removeTimeObserver:` を呼ばずに解放すると
    /// 未定義動作」と明記しているので、必ず手放す前に外す。
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
            this.player.addPeriodicTimeObserverForInterval_queue_usingBlock(
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
        unsafe { self.player.replaceCurrentItemWithPlayerItem(item.as_deref()) };
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

    /// KVO と通知から呼ばれ、AVPlayer の状態を miui の状態へ写す。
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

    /// 再生し終えた通知。いま再生している項目からのものだけを扱う。
    fn item_did_play_to_end(&self, notification: &NSNotification) {
        let Some(sender) = notification.object() else {
            return;
        };
        let Some(current) = (unsafe { self.player.currentItem() }) else {
            return;
        };
        if !std::ptr::eq(&*sender as *const AnyObject, &*current as *const AVPlayerItem as _) {
            return;
        }
        if self.looping.get() {
            self.seek(0.0);
            unsafe { self.player.play() };
            return;
        }
        self.emit(PlaybackState::Ended);
    }

    /// 再生位置を通知する。
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

    /// 状態が変わっていれば記録して通知する。同じ状態の連続では呼ばない。
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
            self.player.removeObserver_forKeyPath(
                &observer,
                &NSString::from_str(TIME_CONTROL_STATUS),
            );
            NSNotificationCenter::defaultCenter().removeObserver(&observer);
        }
    }
}

/// CMTime を秒にする。時間が定まっていなければ `None`。
///
/// 読み込みが終わるまで長さは `indefinite` なので、そのあいだは `None` になる。
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
    #[name = "MiuiPlaybackObserver"]
    #[ivars = Weak<PlaybackInner>]
    /// AVPlayer の KVO と再生終了通知を Rust 側へ中継する。
    ///
    /// ウィジェットを弱参照で持つ。強参照にすると、ウィジェットが
    /// 観測者を保持しているため循環して解放されなくなる。
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

/// 動画と音声に共通の再生 API を生やす。
///
/// 2 つの違いは映像面を使うかどうかだけなので、操作はすべて同じ形にそろえる。
macro_rules! impl_playback {
    ($t:ty) => {
        impl $t {
            /// いま指定されている場所 (渡した文字列そのまま)。
            pub fn source(&self) -> String {
                self.0.source.borrow().clone()
            }

            /// 再生するメディアの場所。ファイルパスと URL のどちらでもよい。
            ///
            /// 呼ぶと再生は止まり、状態は [`PlaybackState::Idle`] に戻る。
            /// 空文字列を渡すとメディアを外す。
            pub fn set_source(&self, source: &str) {
                self.0.set_source(source);
            }

            /// 再生を始める。
            ///
            /// 最後まで再生し終えた後に呼ぶと、先頭へ戻してから再生する。
            pub fn play(&self) {
                self.0.play();
            }

            /// 一時停止する。
            pub fn pause(&self) {
                self.0.pause();
            }

            /// いまの再生状態。
            pub fn state(&self) -> PlaybackState {
                self.0.state.get()
            }

            /// 再生中か。
            pub fn is_playing(&self) -> bool {
                self.0.state.get().is_playing()
            }

            /// 再生位置を秒で指定する。負の値は先頭として扱う。
            pub fn seek(&self, seconds: f64) {
                self.0.seek(seconds);
            }

            /// いまの再生位置 (秒)。
            pub fn position(&self) -> f64 {
                self.0.position()
            }

            /// メディアの長さ (秒)。**読み込みが終わるまでは `None`。**
            ///
            /// 長さが決まらない配信 (ライブなど) でも `None` を返す。
            pub fn duration(&self) -> Option<f64> {
                self.0.duration()
            }

            /// 音量 (0.0..=1.0)。範囲外は丸める。
            pub fn set_volume(&self, volume: f64) {
                unsafe { self.0.player.setVolume(volume.clamp(0.0, 1.0) as f32) };
            }

            pub fn volume(&self) -> f64 {
                unsafe { self.0.player.volume() as f64 }
            }

            /// 消音する。音量の値は保ったまま音だけ止まる。
            pub fn set_muted(&self, muted: bool) {
                unsafe { self.0.player.setMuted(muted) };
            }

            pub fn is_muted(&self) -> bool {
                unsafe { self.0.player.isMuted() }
            }

            /// 最後まで再生したら先頭へ戻って繰り返す。
            pub fn set_loop(&self, looping: bool) {
                self.0.looping.set(looping);
            }

            pub fn is_loop(&self) -> bool {
                self.0.looping.get()
            }

            /// メディアを指定したときに自動で再生を始める。
            ///
            /// すでに場所が指定されていて、まだ一度も再生していなければ、
            /// この呼び出しで再生が始まる。
            pub fn set_autoplay(&self, autoplay: bool) {
                self.0.autoplay.set(autoplay);
                if autoplay
                    && self.0.state.get() == PlaybackState::Idle
                    && !self.0.source.borrow().is_empty()
                {
                    self.0.play();
                }
            }

            /// ネイティブの再生バーを出すかどうか (既定は出す)。
            pub fn set_controls(&self, controls: bool) {
                let style = if controls {
                    AVPlayerViewControlsStyle::Default
                } else {
                    AVPlayerViewControlsStyle::None
                };
                unsafe { self.0.native.setControlsStyle(style) };
            }

            /// 再生状態が変わったときに呼ばれる。設定し直すと以前のものは外れる。
            ///
            /// アプリから [`play`](Self::play) を呼んだときだけでなく、
            /// **AVPlayerView の再生バーをユーザーが操作したときにも届く**。
            pub fn on_state_change(&self, f: impl FnMut(PlaybackState) + 'static) {
                *self.0.callback.borrow_mut() = Some(Box::new(f));
            }

            /// 再生位置が進むたびに、その位置 (秒) で呼ばれる。
            ///
            /// シークバーの表示を再生に追従させるためのもの。間隔はネイティブ側が
            /// 決めるが、およそ 4 回/秒で、再生の開始・停止・シークの直後にも届く。
            /// **再生していない間は呼ばれない。**
            pub fn on_position_change(&self, f: impl FnMut(f64) + 'static) {
                *self.0.position_callback.borrow_mut() = Some(Box::new(f));
            }
        }
    };
}

// ------------------------------------------------------------------ Video

/// 動画 (AVKit の AVPlayerView)。
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

    /// 映像の収め方。AVPlayerLayer の videoGravity に写す。
    ///
    /// AVFoundation に「原寸のまま置く」は無いため、[`Fit::None`] は
    /// [`Fit::Contain`] と同じ扱いになる。
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

/// 音声 (AVKit の AVPlayerView。映像面を持たないので再生バーだけが見える)。
///
/// AppKit に「音声プレイヤー」という単独のコントロールは無い。映像トラックの
/// 無いメディアを AVPlayerView に載せると、再生バーだけが表示される。
#[derive(Clone)]
pub struct Audio(Rc<PlaybackInner>);
impl_widget!(Audio);
impl_playback!(Audio);

impl Audio {
    /// 再生バーが見える最低限の高さ。AVPlayerView の inline スタイルの実測値。
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
