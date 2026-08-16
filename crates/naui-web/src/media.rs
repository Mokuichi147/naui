//! 画像・動画・音声。
//!
//! `<img>` / `<video>` / `<audio>` をそのまま使う。デコードも再生も
//! 再生バーの描画もブラウザの仕事で、naui 側は `src` と属性を書くだけ。
//!
//! 再生状態は HTMLMediaElement のイベント (`playing` / `waiting` / `pause` /
//! `ended`) を購読して通知する。アプリから [`Video::play`] を呼んだときだけで
//! なく、**ブラウザの再生バーをユーザーが操作したときにも同じ経路で届く**。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use naui_core::{Fit, PlaybackState, Result};
use wasm_bindgen::JsCast;
use web_sys::{Document, Element, HtmlElement, HtmlImageElement, HtmlMediaElement};

use crate::widgets::{create, impl_widget, Listener, Widget};

/// 収め方を CSS の `object-fit` の値にする。
fn object_fit(fit: Fit) -> &'static str {
    match fit {
        Fit::Contain => "contain",
        Fit::Cover => "cover",
        Fit::Fill => "fill",
        Fit::None => "none",
    }
}

// ------------------------------------------------------------------ Image

struct ImageInner {
    element: HtmlImageElement,
    /// 渡された文字列そのまま。`src` は絶対 URL に解決されてしまうため。
    source: RefCell<String>,
}

/// 画像表示 (`<img>`)。
///
/// 読み込みはブラウザが非同期に行う。**指定した直後は
/// [`is_loaded`](Self::is_loaded) がまだ偽になる。**
#[derive(Clone)]
pub struct Image(Rc<ImageInner>);
impl_widget!(Image, element);

impl Image {
    pub(crate) fn new(doc: &Document, source: &str) -> Result<Self> {
        let element: HtmlImageElement = create(doc, "img")?.unchecked_into();
        let this = Self(Rc::new(ImageInner {
            element,
            source: RefCell::new(String::new()),
        }));
        this.set_fit(Fit::default());
        this.set_source(source);
        Ok(this)
    }

    /// いま指定されている場所 (渡した文字列そのまま)。
    pub fn source(&self) -> String {
        self.0.source.borrow().clone()
    }

    /// 表示する画像の場所。相対 URL はページからの相対として解決される。
    ///
    /// 空文字列を渡すと画像を外す。
    pub fn set_source(&self, source: &str) {
        *self.0.source.borrow_mut() = source.to_string();
        if source.is_empty() {
            let _ = self.0.element.remove_attribute("src");
        } else {
            self.0.element.set_src(source);
        }
    }

    /// 読み込みが終わっているか。
    ///
    /// ブラウザの読み込みは非同期なので、[`set_source`](Self::set_source) の
    /// 直後は偽になる。
    pub fn is_loaded(&self) -> bool {
        self.0.element.complete() && self.0.element.natural_width() > 0
    }

    /// 表示領域への収め方 (CSS の `object-fit`)。
    pub fn set_fit(&self, fit: Fit) {
        let _ = self
            .0
            .element
            .style()
            .set_property("object-fit", object_fit(fit));
    }

    /// 画像の内容を表す文字列 (`alt`)。読み上げと、読み込めなかったときの表示に使われる。
    pub fn set_alt(&self, text: &str) {
        self.0.element.set_alt(text);
    }
}

// --------------------------------------------------------------- 再生の中身

/// 差し替えできる 1 本のクロージャと、いまの状態。
///
/// イベントごとに別のリスナーを張るので、そのすべてから共有する。
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

    /// 状態が変わっていれば記録して通知する。同じ状態の連続では呼ばない。
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

/// 再生位置を知らせる、差し替えできる 1 本のクロージャ。
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

/// 動画と音声で共有する再生の実体。
///
/// `<video>` と `<audio>` はどちらも HTMLMediaElement なので、
/// 映像面を持つかどうか以外はまったく同じ操作になる。
struct MediaInner {
    element: HtmlMediaElement,
    source: RefCell<String>,
    handler: StateHandler,
    position: PositionHandler,
    /// 状態を通知するためのイベント購読。ハンドルが生きている間だけ有効。
    listeners: RefCell<Vec<Listener>>,
}

impl MediaInner {
    fn new(doc: &Document, tag: &'static str) -> Result<Rc<Self>> {
        let element: HtmlMediaElement = create(doc, tag)?.unchecked_into();
        // ブラウザ標準の再生バーを出す。
        element.set_controls(true);
        let this = Rc::new(Self {
            element,
            source: RefCell::new(String::new()),
            handler: StateHandler::default(),
            position: PositionHandler::default(),
            listeners: RefCell::new(Vec::new()),
        });
        this.subscribe()?;
        Ok(this)
    }

    /// HTMLMediaElement のイベントを naui の再生状態に写す。
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

/// 動画と音声に共通の再生 API を生やす。
macro_rules! impl_playback {
    ($t:ty) => {
        impl $t {
            /// いま指定されている場所 (渡した文字列そのまま)。
            pub fn source(&self) -> String {
                self.0.source.borrow().clone()
            }

            /// 再生するメディアの場所。相対 URL も使える。
            ///
            /// 呼ぶと再生は止まり、状態は [`PlaybackState::Idle`] に戻る。
            /// 空文字列を渡すとメディアを外す。
            pub fn set_source(&self, source: &str) {
                self.0.set_source(source);
            }

            /// 再生を始める。
            ///
            /// 最後まで再生し終えた後に呼ぶと、先頭へ戻してから再生する。
            /// **ブラウザの自動再生制限で拒否されることがある** (ユーザー操作を
            /// 経ていない、音が出る、など)。拒否されると状態は変わらない。
            pub fn play(&self) {
                self.0.play();
            }

            /// 一時停止する。
            pub fn pause(&self) {
                let _ = self.0.element.pause();
            }

            /// いまの再生状態。
            pub fn state(&self) -> PlaybackState {
                self.0.handler.state()
            }

            /// 再生中か。
            pub fn is_playing(&self) -> bool {
                self.0.handler.state().is_playing()
            }

            /// 再生位置を秒で指定する。負の値は先頭として扱う。
            pub fn seek(&self, seconds: f64) {
                self.0.element.set_current_time(seconds.max(0.0));
            }

            /// いまの再生位置 (秒)。
            pub fn position(&self) -> f64 {
                self.0.element.current_time()
            }

            /// メディアの長さ (秒)。**読み込みが終わるまでは `None`。**
            ///
            /// 長さが決まらない配信 (ライブなど) でも `None` を返す
            /// (HTMLMediaElement では NaN と Infinity で表される)。
            pub fn duration(&self) -> Option<f64> {
                let duration = self.0.element.duration();
                if duration.is_nan() || duration.is_infinite() {
                    None
                } else {
                    Some(duration)
                }
            }

            /// 音量 (0.0..=1.0)。範囲外は丸める。
            pub fn set_volume(&self, volume: f64) {
                let _ = self.0.element.set_volume(volume.clamp(0.0, 1.0));
            }

            pub fn volume(&self) -> f64 {
                self.0.element.volume()
            }

            /// 消音する。音量の値は保ったまま音だけ止まる。
            pub fn set_muted(&self, muted: bool) {
                self.0.element.set_muted(muted);
            }

            pub fn is_muted(&self) -> bool {
                self.0.element.muted()
            }

            /// 最後まで再生したら先頭へ戻って繰り返す。
            pub fn set_loop(&self, looping: bool) {
                self.0.element.set_loop(looping);
            }

            pub fn is_loop(&self) -> bool {
                self.0.element.loop_()
            }

            /// メディアを指定したときに自動で再生を始める。
            ///
            /// すでに場所が指定されていて、まだ一度も再生していなければ、
            /// この呼び出しで再生が始まる。
            pub fn set_autoplay(&self, autoplay: bool) {
                self.0.element.set_autoplay(autoplay);
                if autoplay
                    && self.0.handler.state() == PlaybackState::Idle
                    && !self.0.source.borrow().is_empty()
                {
                    self.0.play();
                }
            }

            /// ブラウザ標準の再生バーを出すかどうか (既定は出す)。
            pub fn set_controls(&self, controls: bool) {
                self.0.element.set_controls(controls);
            }

            /// 再生状態が変わったときに呼ばれる。設定し直すと以前のものは外れる。
            ///
            /// アプリから [`play`](Self::play) を呼んだときだけでなく、
            /// **ブラウザの再生バーをユーザーが操作したときにも届く**。
            pub fn on_state_change(&self, f: impl FnMut(PlaybackState) + 'static) {
                self.0.handler.set(f);
            }

            /// 再生位置が進むたびに、その位置 (秒) で呼ばれる。
            ///
            /// シークバーの表示を再生に追従させるためのもの。間隔はブラウザが
            /// 決めるが、およそ 4 回/秒で、シークの直後にも届く。
            /// **再生していない間は呼ばれない。**
            pub fn on_position_change(&self, f: impl FnMut(f64) + 'static) {
                self.0.position.set(f);
            }
        }
    };
}

// ------------------------------------------------------------------ Video

/// 動画 (`<video>`)。
#[derive(Clone)]
pub struct Video(Rc<MediaInner>);
impl_widget!(Video, element);
impl_playback!(Video);

impl Video {
    pub(crate) fn new(doc: &Document, source: &str) -> Result<Self> {
        let this = Self(MediaInner::new(doc, "video")?);
        this.set_fit(Fit::default());
        this.set_source(source);
        Ok(this)
    }

    /// 映像の収め方 (CSS の `object-fit`)。
    pub fn set_fit(&self, fit: Fit) {
        let element: &HtmlElement = self.0.element.as_ref();
        let _ = element.style().set_property("object-fit", object_fit(fit));
    }
}

// ------------------------------------------------------------------ Audio

/// 音声 (`<audio>`)。映像面を持たないので、再生バーだけが見える。
#[derive(Clone)]
pub struct Audio(Rc<MediaInner>);
impl_widget!(Audio, element);
impl_playback!(Audio);

impl Audio {
    pub(crate) fn new(doc: &Document, source: &str) -> Result<Self> {
        let this = Self(MediaInner::new(doc, "audio")?);
        this.set_source(source);
        Ok(this)
    }
}
