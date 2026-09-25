//! 画像・動画・音声。
//!
//! 動画と音声は WinUI 3 の **MediaPlayerElement** と
//! **Windows.Media.Playback.MediaPlayer** がそのまま担う。
//!
//! 画像は WinUI 標準の `Image` と `BitmapImage` で、どちらも
//! [`naui_winui3`] の投影から直に作る。読み込みも描画も WinUI が行う。
//! 要素はホストの `Grid` に 1 つだけ置き、場所や収め方が変わっても
//! 作り直さずプロパティを書き換える。
//!
//! ## 再生状態の通知とスレッド
//!
//! `MediaPlaybackSession` の `PlaybackStateChanged` と `MediaPlayer` の
//! `MediaEnded` は、**UI スレッドではなく再生パイプラインのスレッドで発生する**。
//! そのまま Rust のクロージャを呼ぶと UI スレッド前提の構造が壊れるため、
//! `DispatcherQueue::TryEnqueue` で UI スレッドへ渡し直してから通知する。
//!
//! そのとき `Rc` をスレッドをまたいで運ぶわけにはいかない (参照カウントが
//! 非アトミックなため)。ウィジェットは UI スレッド側の一覧に番号で登録し、
//! **スレッドをまたぐのは番号と状態だけ**にしている。

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use naui_core::media::is_url;
use naui_core::{Fit, PlaybackState, Result};
use naui_winui3::Microsoft::UI::Dispatching::{DispatcherQueue, DispatcherQueueHandler};
use naui_winui3::Microsoft::UI::Xaml::Automation::AutomationProperties;
use naui_winui3::Microsoft::UI::Xaml::Controls::{
    Grid, Image as XamlImage, MediaPlayerElement, UIElementCollection,
};
use naui_winui3::Microsoft::UI::Xaml::Media::Imaging::BitmapImage;
use naui_winui3::Microsoft::UI::Xaml::Media::Stretch;
use naui_winui3::Microsoft::UI::Xaml::{HorizontalAlignment, UIElement, VerticalAlignment};
use windows::Foundation::{TimeSpan, TypedEventHandler, Uri};
use windows::Media::Core::MediaSource;
use windows::Media::Playback::{
    IMediaPlaybackSource, MediaPlaybackSession, MediaPlaybackState, MediaPlayer,
    MediaPlayerFailedEventArgs,
};
use windows::Storage::StorageFile;
use windows_core::{IInspectable, Interface, HSTRING};

use crate::to_error;
use crate::widgets::impl_widget;
use crate::widgets::Widget;
use crate::Slot;

/// TimeSpan の 1 秒あたりの刻み数 (100 ナノ秒単位)。
const TICKS_PER_SECOND: f64 = 10_000_000.0;

/// 収め方を XAML の `Stretch` にする。
fn stretch(fit: Fit) -> Stretch {
    match fit {
        Fit::Contain => Stretch::Uniform,
        Fit::Cover => Stretch::UniformToFill,
        Fit::Fill => Stretch::Fill,
        Fit::None => Stretch::None,
    }
}

/// メディアの場所を、`Windows.Foundation.Uri` へ渡す文字列にする。
///
/// パスは [`file_uri`] で `file://` にする。URL はそのまま渡すが、`file:` だけは
/// [`unescape_non_ascii`] で非 ASCII の percent-encoding を生の文字に戻す。
/// [`naui_core::media::file_url`] が作る URL や、利用者が標準どおり書いた
/// `file:///C:/%E5%86%99.png` も、[`file_uri`] と同じ理由で読めないため。
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

/// UTF-8 の非 ASCII を表す `%XX` の並びを、生の文字に戻す。
///
/// `%20` `%23` のような ASCII の encode は、URL の区切りとしての意味を
/// 変えないようそのまま残す。UTF-8 として読めない並びも手を付けない。
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
            let c = url[i..].chars().next().expect("i は文字の境目にある");
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

/// ローカルのファイルパスを、`Windows.Foundation.Uri` が読める `file://` にする。
///
/// [`naui_core::media::file_url`] と違い、**非 ASCII は encode せず生のまま置く**。
/// `Windows.Foundation.Uri` は `%E5%86%99` を UTF-8 として復号せず 1 バイト
/// 1 文字 (`å†™`) に戻すため、日本語を含むパスは別のパスになって
/// `BitmapImage` も `MediaSource` もエラー無しに読み込みを諦める。
/// 生の非 ASCII は IRI として受け付けられるので、URL で意味を持つ ASCII
/// (空白・`%`・`#`・`?` など) だけを encode する。
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

// ------------------------------------------------------------------ Image

struct ImageInner {
    /// 大きさの指定を受け取り、画像を出し入れするホスト。
    native: Grid,
    /// 表示する WinUI の `Image`。場所が空のときはホストから外す。
    image: XamlImage,
    source: RefCell<String>,
    fit: Cell<Fit>,
    alt: RefCell<String>,
}

/// 画像表示 (WinUI の `Image`)。
///
/// 読み込みは WinUI が非同期に行うため、[`set_source`](Self::set_source) の
/// 直後にはまだ表示されていない。
#[derive(Clone)]
pub struct Image(Rc<ImageInner>);
impl_widget!(Image, native);

impl Image {
    pub(crate) fn new(source: &str) -> Result<Self> {
        let native = Grid::new().map_err(|e| to_error("Image ホストの生成", e))?;
        let image = XamlImage::new().map_err(|e| to_error("Image の生成", e))?;
        image
            .SetHorizontalAlignment(HorizontalAlignment::Stretch)
            .and_then(|()| image.SetVerticalAlignment(VerticalAlignment::Stretch))
            .and_then(|()| image.SetStretch(stretch(Fit::default())))
            .map_err(|e| to_error("Image の配置設定", e))?;
        let this = Self(Rc::new(ImageInner {
            native,
            image,
            source: RefCell::new(String::new()),
            fit: Cell::new(Fit::default()),
            alt: RefCell::new(String::new()),
        }));
        this.set_source(source);
        Ok(this)
    }

    /// いま指定されている場所 (渡した文字列そのまま)。
    pub fn source(&self) -> String {
        self.0.source.borrow().clone()
    }

    /// 表示する画像の場所。ファイルパスと URL のどちらでもよい。
    ///
    /// 空文字列を渡すと画像を外す。
    pub fn set_source(&self, source: &str) {
        *self.0.source.borrow_mut() = source.to_string();
        self.apply_source();
    }

    /// 読み込みを指示できているか (要素を組み立てられたか)。
    ///
    /// WinUI の `Image` は読み込みの完了を Rust 側へ返さないため、
    /// **「読み終わったか」ではなく「表示する要素が入っているか」を返す**。
    pub fn is_loaded(&self) -> bool {
        self.0
            .native
            .Children()
            .and_then(|children| children.Size())
            .is_ok_and(|size| size > 0)
    }

    /// 表示領域への収め方 (`Image` の `Stretch`)。
    pub fn set_fit(&self, fit: Fit) {
        self.0.fit.set(fit);
        let _ = self.0.image.SetStretch(stretch(fit));
    }

    /// 画像の内容を表す文字列。ナレーターが読み上げる。
    pub fn set_alt(&self, text: &str) {
        *self.0.alt.borrow_mut() = text.to_string();
        let _ = AutomationProperties::SetName(&self.0.image, &HSTRING::from(text));
    }

    /// 画像をホストから外し、読み込みの指示も落とす。
    ///
    /// 場所が空のときと、読み込みを指示できなかったときの両方で通る。
    /// [`is_loaded`](Self::is_loaded) が見ているのはこの出し入れ。
    fn take_down(&self, children: &UIElementCollection) {
        let _ = children.Clear();
        let _ = self.0.image.SetSource(None);
    }

    /// いまの場所を `Image` へ渡し、ホストへの出し入れを合わせる。
    ///
    /// 場所が空のときと、読み込みを指示できなかったときは
    /// [`take_down`](Self::take_down) で外す。
    fn apply_source(&self) {
        let Ok(children) = self.0.native.Children() else {
            return;
        };
        let source = self.0.source.borrow();
        if source.is_empty() {
            self.take_down(&children);
            return;
        }

        // WinUI はファイルパスではなく URI を要求する。
        let uri = match Uri::CreateUri(&HSTRING::from(source_uri(&source))) {
            Ok(uri) => uri,
            Err(error) => {
                eprintln!("naui-windows: Image の場所を URI にできません: {error}");
                self.take_down(&children);
                return;
            }
        };
        let bitmap = match BitmapImage::CreateInstanceWithUriSource(&uri) {
            Ok(bitmap) => bitmap,
            Err(error) => {
                eprintln!("naui-windows: Image の読み込み指示に失敗: {error}");
                self.take_down(&children);
                return;
            }
        };
        if let Err(error) = self.0.image.SetSource(&bitmap) {
            eprintln!("naui-windows: Image の差し替えに失敗: {error}");
            self.take_down(&children);
            return;
        }
        if children.Size().is_ok_and(|size| size > 0) {
            return;
        }
        let Ok(element) = self.0.image.cast::<UIElement>() else {
            eprintln!("naui-windows: Image 要素への変換に失敗");
            return;
        };
        if let Err(error) = children.Append(&element) {
            eprintln!("naui-windows: Image の配置に失敗: {error}");
        }
    }
}

// --------------------------------------------------------------- 再生の中身

/// 動画と音声で共有する再生の実体。
struct MediaInner {
    native: MediaPlayerElement,
    player: MediaPlayer,
    /// UI スレッド側の一覧における自分の番号。
    id: u64,
    source: RefCell<String>,
    looping: Cell<bool>,
    autoplay: Cell<bool>,
    state: Cell<PlaybackState>,
    callback: RefCell<Slot<dyn FnMut(PlaybackState)>>,
    position_callback: RefCell<Slot<dyn FnMut(f64)>>,
}

thread_local! {
    /// UI スレッドで生きている再生ウィジェットの一覧。
    ///
    /// 再生パイプラインのスレッドから届いた通知を UI スレッドで受け直すとき、
    /// 番号から実体を引くために使う。弱参照なので、ウィジェットが先に
    /// 解放されていれば何も起きない。
    static PLAYBACKS: RefCell<HashMap<u64, Weak<MediaInner>>> =
        RefCell::new(HashMap::new());

    /// 次に配る番号。
    static NEXT_ID: Cell<u64> = const { Cell::new(0) };
}

fn next_id() -> u64 {
    NEXT_ID.with(|id| {
        id.set(id.get() + 1);
        id.get()
    })
}

/// UI スレッドで、番号のウィジェットへ状態を届ける。
fn deliver(id: u64, state: PlaybackState) {
    if let Some(inner) = lookup(id) {
        inner.emit(state);
    }
}

/// UI スレッドで、番号のウィジェットへ再生位置を届ける。
fn deliver_position(id: u64, seconds: f64) {
    if let Some(inner) = lookup(id) {
        inner.emit_position(seconds);
    }
}

fn lookup(id: u64) -> Option<Rc<MediaInner>> {
    PLAYBACKS.with(|map| map.borrow().get(&id).and_then(Weak::upgrade))
}

/// WinRT の再生状態を naui の再生状態に写す。
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
            MediaPlayerElement::new().map_err(|e| to_error("MediaPlayerElement の生成", e))?;
        let player = MediaPlayer::new().map_err(|e| to_error("MediaPlayer の生成", e))?;
        // Windows App SDK 2.2.4 の標準 MediaTransportControls は、
        // MediaPlayerElement を visual tree へ追加したときに XAML 内部
        // 例外 (0xc000027b) を起こす環境がある。操作 UI はアプリ側で
        // 用意するため、ここでは常に無効にしておく。
        native
            .SetAreTransportControlsEnabled(false)
            .map_err(|e| to_error("再生バーの設定", e))?;
        native
            .SetMediaPlayer(&player)
            .map_err(|e| to_error("MediaPlayer の割り当て", e))?;
        // 場所を指定するまで勝手に鳴らさない。
        player
            .SetAutoPlay(false)
            .map_err(|e| to_error("自動再生の設定", e))?;

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
        });
        PLAYBACKS.with(|map| map.borrow_mut().insert(this.id, Rc::downgrade(&this)));
        this.subscribe()?;
        Ok(this)
    }

    /// 再生パイプラインからの通知を、UI スレッド経由で受け取れるようにする。
    fn subscribe(&self) -> Result<()> {
        let queue = DispatcherQueue::GetForCurrentThread()
            .map_err(|e| to_error("UI スレッドの DispatcherQueue の取得", e))?;
        let session = self
            .player
            .PlaybackSession()
            .map_err(|e| to_error("再生セッションの取得", e))?;

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
                        eprintln!("naui-windows: 再生状態の取得に失敗: {error}");
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
            .map_err(|e| to_error("再生状態の購読", e))?;

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
                        eprintln!("naui-windows: 再生位置の取得に失敗: {error}");
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
            .map_err(|e| to_error("再生位置の購読", e))?;

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
            .map_err(|e| to_error("再生終了の購読", e))?;

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
                    .unwrap_or_else(|error| format!("取得失敗: {error}"));
                let code = args
                    .ExtendedErrorCode()
                    .map(|code| format!("{code:?}"))
                    .unwrap_or_else(|error| format!("取得失敗: {error}"));
                let message = args
                    .ErrorMessage()
                    .map(|message| message.to_string_lossy())
                    .unwrap_or_default();
                eprintln!(
                    "naui-windows: MediaPlayer の再生に失敗: error={error}, extended_error={code}, message={message}"
                );
                Ok(())
            },
        );
        self.player
            .MediaFailed(&failed)
            .map_err(|e| to_error("再生失敗の購読", e))?;
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
                eprintln!("naui-windows: メディア URL の設定に失敗 ({source}): {error}");
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
                eprintln!("naui-windows: メディアファイルの設定に失敗 ({source}): {error}");
                // 相対パスなど、StorageFile として開けない入力は
                // file:// URI も試す。
                if let Err(fallback_error) = self.set_uri_source(&file_uri(source)) {
                    eprintln!(
                        "naui-windows: メディア URL のフォールバックにも失敗 ({source}): {fallback_error}"
                    );
                }
            }
        }
        self.emit(PlaybackState::Idle);
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

    /// 再生位置を通知する。
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

    /// 状態が変わっていれば記録して通知する。同じ状態の連続では呼ばない。
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

/// 動画と音声に共通の再生 API を生やす。
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
                let _ = self.0.player.Pause();
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
                self.0
                    .player
                    .PlaybackSession()
                    .and_then(|session| session.Position())
                    .map(to_seconds)
                    .unwrap_or(0.0)
            }

            /// メディアの長さ (秒)。**読み込みが終わるまでは `None`。**
            ///
            /// 長さが決まらない配信 (ライブなど) でも `None` を返す。
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

            /// 音量 (0.0..=1.0)。範囲外は丸める。
            pub fn set_volume(&self, volume: f64) {
                let _ = self.0.player.SetVolume(volume.clamp(0.0, 1.0));
            }

            pub fn volume(&self) -> f64 {
                self.0.player.Volume().unwrap_or(0.0)
            }

            /// 消音する。音量の値は保ったまま音だけ止まる。
            pub fn set_muted(&self, muted: bool) {
                let _ = self.0.player.SetIsMuted(muted);
            }

            pub fn is_muted(&self) -> bool {
                self.0.player.IsMuted().unwrap_or(false)
            }

            /// 最後まで再生したら先頭へ戻って繰り返す。
            pub fn set_loop(&self, looping: bool) {
                self.0.looping.set(looping);
                let _ = self.0.player.SetIsLoopingEnabled(looping);
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
                let _ = self.0.player.SetAutoPlay(autoplay);
                if autoplay
                    && self.0.state.get() == PlaybackState::Idle
                    && !self.0.source.borrow().is_empty()
                {
                    self.0.play();
                }
            }

            /// WinUI 標準の再生バーを使うかどうか。
            ///
            /// Windows App SDK 2.2.4 では標準バーを有効にすると
            /// `MediaPlayerElement` の visual tree 追加時にプロセスが
            /// `0xc000027b` で終了するため、このバックエンドでは常に
            /// 無効にする。アプリ側の再生 UI と組み合わせて使う。
            pub fn set_controls(&self, controls: bool) {
                if controls {
                    eprintln!("naui-windows: WinUI 標準の再生バーは安全性のため無効です");
                }
                let _ = self.0.native.SetAreTransportControlsEnabled(false);
            }

            /// 再生状態が変わったときに呼ばれる。設定し直すと以前のものは外れる。
            ///
            /// アプリから [`play`](Self::play) を呼んだときだけでなく、
            /// **再生バーをユーザーが操作したときにも届く**。
            pub fn on_state_change(&self, f: impl FnMut(PlaybackState) + 'static) {
                *self.0.callback.borrow_mut() = Some(Box::new(f));
            }

            /// 再生位置が進むたびに、その位置 (秒) で呼ばれる。
            ///
            /// シークバーの表示を再生に追従させるためのもの。間隔はネイティブ側が
            /// 決めるが、およそ 4 回/秒で、シークの直後にも届く。
            /// **再生していない間は呼ばれない。**
            pub fn on_position_change(&self, f: impl FnMut(f64) + 'static) {
                *self.0.position_callback.borrow_mut() = Some(Box::new(f));
            }
        }
    };
}

// ------------------------------------------------------------------ Video

/// 動画 (MediaPlayerElement)。
#[derive(Clone)]
pub struct Video(Rc<MediaInner>);
impl_widget!(Video, native);
impl_playback!(Video);

impl Video {
    pub(crate) fn new(source: &str) -> Result<Self> {
        let this = Self(MediaInner::new()?);
        this.set_fit(Fit::default());
        this.set_source(source);
        Ok(this)
    }

    /// 映像の収め方 (MediaPlayerElement の `Stretch`)。
    pub fn set_fit(&self, fit: Fit) {
        let _ = self.0.native.SetStretch(stretch(fit));
    }
}

// ------------------------------------------------------------------ Audio

/// 音声 (MediaPlayerElement)。映像トラックが無いので、再生バーだけが見える。
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

    /// core の `file_url` が作る URL も、Windows で読める形に直る。
    #[test]
    fn source_uri_unescapes_utf8_in_file_urls() {
        let url = naui_core::media::file_url("C:/Users/太郎/写真 #1.png");
        assert_eq!(source_uri(&url), "file:///C:/Users/太郎/写真%20%231.png");
        assert_eq!(source_uri("FILE:///C:/%E5%86%99.png"), "FILE:///C:/写.png");
    }

    /// ASCII の encode と、UTF-8 として読めない並びには手を付けない。
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

    /// `Windows.Foundation.Uri` に通したあとも、日本語のパスが同じパスとして残る。
    ///
    /// UTF-8 で percent-encode したものを渡すと、ここで `å†™` のような
    /// 別のパスに化ける。
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
