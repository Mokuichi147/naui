//! ブラウザには OS のウィンドウが無いため、`<body>` 直下のブロック要素を
//! ウィンドウとして扱う。タイトルは `document.title` に反映する。

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use naui_core::{Error, Result, StateHandler, Theme, WindowState};
use wasm_bindgen::JsCast;
use web_sys::{Document, Element, HtmlElement};

use crate::apply_theme;
use crate::menu_bar::MenuBar;
use crate::sidebar::Sidebar;
use crate::to_error;
use crate::toolbar::Toolbar;
use crate::widgets::{create, Listener, Widget};

/// ウィンドウ要素に付ける印。
const WINDOW_ATTRIBUTE: &str = "data-naui-window";

/// ウィンドウ要素を `closest` で探すためのセレクタ。
pub(crate) const WINDOW_SELECTOR: &str = "[data-naui-window]";

struct WindowInner {
    element: HtmlElement,
    document: Document,
    title: RefCell<String>,
    child: RefCell<Option<Box<dyn Widget>>>,
    /// 上端に差し込んだツールバー。通知先ごと生かしておく。
    toolbar: RefCell<Option<Toolbar>>,
    /// 上端に差し込んだメニューバー。通知先ごと生かしておく。
    menu_bar: RefCell<Option<MenuBar>>,
    /// 取り付けたサイドバー。通知先ごと生かしておく。
    sidebar: RefCell<Option<Sidebar>>,
    /// ツールバーの行。サイドバーの開閉ボタンとツールバーを横に並べる。
    toolbar_row: HtmlElement,
    key_down: crate::keys::KeyDown,
    on_resize: Rc<crate::widgets::ValueHandler<(f64, f64)>>,
    /// 大きさの変化の購読。落とすと購読も外れる。
    resize_observer: RefCell<Option<web_sys::ResizeObserver>>,
    resize_callback: RefCell<Option<ObserverCallback>>,
    /// `ui.window` か `set_size` で頼まれた中身の大きさ。
    size: Cell<(f64, f64)>,
    /// `set_min_size` で頼まれた中身の下限。
    min_size: Cell<Option<(f64, f64)>>,
    /// メニューバーとツールバーの行の高さの変化の購読。行の高さが変わったら
    /// ウィンドウ要素の高さの指定を足し直す。
    bars_observer: RefCell<Option<web_sys::ResizeObserver>>,
    bars_callback: RefCell<Option<ObserverCallback>>,
    on_state_change: StateHandler,
    /// 文書の `fullscreenchange` の購読。全画面への出入りを知らせる。
    fullscreen_change: RefCell<Option<Listener>>,
    /// 最後に `set_state` で頼まれた向き (全画面にするなら `true`)。まだ
    /// 当たっていない間だけ `Some` で、当たったら (か、断られたら) `None`。
    wanted: Cell<Option<bool>>,
    /// いま待っている `requestFullscreen` (`true`) / `exitFullscreen`
    /// (`false`) の向き。待っていなければ `None`。
    in_flight: Cell<Option<bool>>,
    /// 待っていた Promise が済んだ (成功でも拒否でも) ときの受け口。受けないと
    /// 拒否がコンソールに「処理されなかった拒否」として出る。使い回すので、
    /// ここで生かしておく。
    settled: RefCell<Option<ObserverCallback>>,
    /// `set_resizable` で頼まれた値。Web では効かず、覚えるだけ。
    resizable: Cell<bool>,
}

/// `ResizeObserver` へ渡す通知先。
type ObserverCallback = wasm_bindgen::closure::Closure<dyn FnMut(wasm_bindgen::JsValue)>;

impl Drop for WindowInner {
    fn drop(&mut self) {
        if let Some(observer) = self.resize_observer.borrow_mut().take() {
            observer.disconnect();
        }
        if let Some(observer) = self.bars_observer.borrow_mut().take() {
            observer.disconnect();
        }
    }
}

/// ページ上のウィンドウ相当。
#[derive(Clone)]
pub struct Window(Rc<WindowInner>);

/// ウィンドウを強く保持せずにイベントハンドラから参照するための弱参照。
#[derive(Clone)]
pub struct WeakWindow(Weak<WindowInner>);

impl WeakWindow {
    /// ウィンドウがまだ生きていれば強参照へ戻す。
    pub fn upgrade(&self) -> Option<Window> {
        self.0.upgrade().map(Window)
    }
}

impl Window {
    /// イベントハンドラなどへ渡しても所有権循環を作らない参照を返す。
    pub fn downgrade(&self) -> WeakWindow {
        WeakWindow(Rc::downgrade(&self.0))
    }

    pub(crate) fn new(document: &Document, title: &str, width: f64, height: f64) -> Result<Self> {
        let element: HtmlElement = create(document, "div")?.unchecked_into();
        // メニューバーが「キーがどのウィンドウから上がってきたか」を見分ける印。
        let _ = element.set_attribute(WINDOW_ATTRIBUTE, "");
        let style = element.style();
        // 幅と高さは `apply_size` が付ける。
        let _ = style.set_property("margin", "0 auto");
        let _ = style.set_property("box-sizing", "border-box");
        // 中身がウィンドウの高さいっぱいに広がれるようにする。
        let _ = style.set_property("display", "flex");
        let _ = style.set_property("flex-direction", "column");
        crate::layout::mark_parent(
            &element,
            crate::layout::ParentLayout::Flex(naui_core::Orientation::Vertical),
        );

        let body = document
            .body()
            .ok_or_else(|| Error::new("body の取得", "body がありません"))?;
        body.append_child(&element)
            .map_err(|e| to_error("ウィンドウの追加", e))?;

        // ツールバーの行。サイドバーの開閉ボタン (あれば) とツールバーを並べる。
        // macOS でサイドバーボタンがツールバーの先頭に入るのと同じ並び。
        let toolbar_row: HtmlElement = create(document, "div")?.unchecked_into();
        let row_style = toolbar_row.style();
        let _ = row_style.set_property("display", "flex");
        let _ = row_style.set_property("flex-direction", "row");
        let _ = row_style.set_property("align-items", "center");
        let _ = row_style.set_property("gap", "6px");
        let _ = row_style.set_property("flex-shrink", "0");

        let this = Self(Rc::new(WindowInner {
            element,
            document: document.clone(),
            title: RefCell::new(String::new()),
            child: RefCell::new(None),
            toolbar: RefCell::new(None),
            menu_bar: RefCell::new(None),
            sidebar: RefCell::new(None),
            toolbar_row,
            key_down: crate::keys::KeyDown::default(),
            on_resize: Rc::default(),
            resize_observer: RefCell::new(None),
            resize_callback: RefCell::new(None),
            size: Cell::new((width.max(0.0), height.max(0.0))),
            min_size: Cell::new(None),
            bars_observer: RefCell::new(None),
            bars_callback: RefCell::new(None),
            on_state_change: StateHandler::default(),
            fullscreen_change: RefCell::new(None),
            wanted: Cell::new(None),
            in_flight: Cell::new(None),
            settled: RefCell::new(None),
            resizable: Cell::new(true),
        }));
        this.observe_fullscreen();
        this.set_title(title);
        this.observe_bars();
        this.apply_size();
        Ok(this)
    }

    /// メニューバーとツールバーの行の高さの合計。付いていない行は数えない。
    fn bars_height(&self) -> f64 {
        let menu_bar = self.0.menu_bar.borrow().as_ref().map(|m| m.mount());
        std::iter::once(self.0.toolbar_row.clone())
            .chain(menu_bar)
            .filter(|bar| bar.parent_element().as_ref() == Some(self.0.element.as_ref()))
            .map(|bar| f64::from(bar.offset_height()))
            .sum()
    }

    /// 頼まれた中身の大きさと下限を、ウィンドウ要素の CSS へ写す。
    ///
    /// ウィンドウ要素はメニューバーとツールバーの行も含むので、高さには
    /// 行の高さを足す。幅は `max-width` (狭い画面では縮む)、高さは
    /// `min-height` (中身が多ければ伸びる) で、どちらも目安になる。
    fn apply_size(&self) {
        let (width, height) = self.0.size.get();
        let min = self.0.min_size.get();
        let bars = self.bars_height();
        let style = self.0.element.style();
        let _ = style.set_property("max-width", &format!("{width}px"));
        let min_height = height.max(min.map_or(0.0, |(_, height)| height)) + bars;
        let _ = style.set_property("min-height", &format!("{min_height}px"));
        match min {
            Some((min_width, _)) => {
                let _ = style.set_property("min-width", &format!("{min_width}px"));
            }
            None => {
                let _ = style.remove_property("min-width");
            }
        }
    }

    /// 全画面への出入り (利用者が Esc で出たときも) を `on_state_change` へ知らせる。
    fn observe_fullscreen(&self) {
        let weak = Rc::downgrade(&self.0);
        let listener = Listener::attach(self.0.document.as_ref(), "fullscreenchange", move || {
            if let Some(inner) = weak.upgrade() {
                let state = Window(inner.clone()).state();
                inner.on_state_change.update(state);
            }
        });
        *self.0.fullscreen_change.borrow_mut() = listener.ok();
        let weak = Rc::downgrade(&self.0);
        let settled = wasm_bindgen::closure::Closure::<dyn FnMut(wasm_bindgen::JsValue)>::new(
            move |_: wasm_bindgen::JsValue| {
                if let Some(inner) = weak.upgrade() {
                    Window(inner).finish_fullscreen_request();
                }
            },
        );
        *self.0.settled.borrow_mut() = Some(settled);
    }

    /// 行の高さが変わったら (付け外しや、フォントの読み込みで) 高さを足し直す。
    fn observe_bars(&self) {
        let weak = Rc::downgrade(&self.0);
        let callback = wasm_bindgen::closure::Closure::<dyn FnMut(wasm_bindgen::JsValue)>::new(
            move |_entries: wasm_bindgen::JsValue| {
                if let Some(inner) = weak.upgrade() {
                    Window(inner).apply_size();
                }
            },
        );
        let Ok(observer) = web_sys::ResizeObserver::new(callback.as_ref().unchecked_ref()) else {
            return;
        };
        observer.observe(self.0.toolbar_row.as_ref());
        *self.0.bars_observer.borrow_mut() = Some(observer);
        *self.0.bars_callback.borrow_mut() = Some(callback);
    }

    /// 利用者がウィンドウを閉じようとしたときの確認。
    ///
    /// **Web では呼ばれない。** ブラウザには利用者がページの中のウィンドウを
    /// 閉じる操作が無く、タブを閉じるときに独自の確認を出すこともできない
    /// (`beforeunload` はブラウザ自身の文言しか出せない) ため。4 環境で同じ
    /// コードを書けるよう、受け付けるだけは受け付ける。
    pub fn on_close_request(&self, f: impl FnMut() -> naui_core::CloseResponse + 'static) {
        drop(f);
    }

    /// 中身の領域の大きさ (幅, 高さ)。単位は CSS ピクセル。
    ///
    /// ほかの環境と同じく、メニューバーとツールバーの行は含まない
    /// (ウィンドウ要素の大きさから行の高さを引く)。
    pub fn size(&self) -> (f64, f64) {
        (
            f64::from(self.0.element.client_width()),
            (f64::from(self.0.element.client_height()) - self.bars_height()).max(0.0),
        )
    }

    /// 大きさが変わったときの通知。変わった後の [`size`](Self::size) が届く。
    ///
    /// `ResizeObserver` で拾うので、文書に載った直後にも 1 回届く。
    pub fn on_resize(&self, f: impl FnMut((f64, f64)) + 'static) {
        self.0.on_resize.set(f);
        if self.0.resize_observer.borrow().is_some() {
            return;
        }
        let weak = Rc::downgrade(&self.0);
        let callback = wasm_bindgen::closure::Closure::<dyn FnMut(wasm_bindgen::JsValue)>::new(
            move |_entries: wasm_bindgen::JsValue| {
                if let Some(inner) = weak.upgrade() {
                    let size = Window(inner.clone()).size();
                    inner.on_resize.emit(size);
                }
            },
        );
        let Ok(observer) = web_sys::ResizeObserver::new(callback.as_ref().unchecked_ref()) else {
            return;
        };
        observer.observe(self.0.element.as_ref());
        *self.0.resize_observer.borrow_mut() = Some(observer);
        *self.0.resize_callback.borrow_mut() = Some(callback);
    }

    /// ウィンドウの左上の位置。**Web では取れないので、いつも `None`。**
    ///
    /// ページの中のウィンドウはブラウザの窓の中に並ぶだけで、画面の上の
    /// 位置を持たない (ブラウザの窓の位置を動かす `moveTo` も、自分で開いた
    /// 窓にしか効かない)。
    pub fn position(&self) -> Option<(f64, f64)> {
        None
    }

    /// ウィンドウを動かす。**Web では何もしない** ([`position`](Self::position) を参照)。
    pub fn set_position(&self, x: f64, y: f64) {
        let _ = (x, y);
    }

    /// いまの表示の状態。Web では [`WindowState::Normal`] か
    /// [`WindowState::Fullscreen`] だけ。
    ///
    /// ページ全体 (`document.documentElement`) が全画面のときに `Fullscreen`。
    /// 動画など、ほかの要素だけが全画面のときは `Normal` のまま。
    pub fn state(&self) -> WindowState {
        let page: Option<Element> = self.0.document.document_element();
        if page.is_some() && self.0.document.fullscreen_element() == page {
            WindowState::Fullscreen
        } else {
            WindowState::Normal
        }
    }

    /// 表示の状態を変える。**Web は全画面だけ**で、Fullscreen API を使う。
    ///
    /// | 頼んだ状態 | Web |
    /// | --- | --- |
    /// | `Fullscreen` | `document.documentElement.requestFullscreen()` |
    /// | `Normal` | 全画面なら `document.exitFullscreen()` |
    /// | `Maximized` / `Minimized` | 全画面なら出るだけ (ページから窓を最大化・最小化する手段が無い) |
    ///
    /// ブラウザは**利用者の操作 (クリックやキー) の中からしか全画面に
    /// させない**ので、ボタンの `on_click` などから呼ぶ。起動時やタイマーから
    /// 呼んでも断られ、状態は変わらない。全画面になるのは非同期なので、
    /// 変わったことは [`on_state_change`](Self::on_state_change) で受け取る。
    ///
    /// ページ全体を全画面にするので、ウィンドウが複数あれば全部が全画面の
    /// 中に入る (トーストやダイアログも出る)。
    ///
    /// 全画面へ入る・出る途中にほかの状態を頼んだときは、いま待っている
    /// 切り替えが済んでから、最後に頼まれたほうへ切り替え直す (途中では
    /// `fullscreenElement` がまだ変わっていないので、その場では決められない)。
    pub fn set_state(&self, state: WindowState) {
        self.0.wanted.set(Some(state == WindowState::Fullscreen));
        self.apply_wanted();
    }

    /// 頼まれた向きといまの状態が違えば、切り替えを頼む。
    ///
    /// 切り替えを待っている間は頼まない (済んだときにまた呼ばれる)。
    fn apply_wanted(&self) {
        if self.0.in_flight.get().is_some() {
            return;
        }
        let Some(wanted) = self.0.wanted.get() else {
            return;
        };
        if wanted == (self.state() == WindowState::Fullscreen) {
            self.0.wanted.set(None);
            return;
        }
        let promise = if wanted {
            self.0
                .document
                .document_element()
                .and_then(|page| call_promise(page.as_ref(), "requestFullscreen"))
        } else {
            call_promise(self.0.document.as_ref(), "exitFullscreen")
        };
        let settled = self.0.settled.borrow();
        let (Some(promise), Some(settled)) = (promise, settled.as_ref()) else {
            // Fullscreen API の無いブラウザ。頼みは捨てる。
            self.0.wanted.set(None);
            return;
        };
        self.0.in_flight.set(Some(wanted));
        let _ = promise.then2(settled, settled);
    }

    /// 待っていた切り替えが済んだ (成功でも拒否でも)。
    ///
    /// 済んだのと同じ向きをまだ頼まれていれば、それで答えたことにする
    /// (断られたものを頼み直し続けない)。待っている間に逆の向きを頼まれて
    /// いれば、いまの状態と比べて切り替え直す。
    fn finish_fullscreen_request(&self) {
        let done = self.0.in_flight.take();
        if done.is_some() && self.0.wanted.get() == done {
            self.0.wanted.set(None);
        }
        self.apply_wanted();
    }

    /// 表示の状態が変わったときの通知。変わった後の [`state`](Self::state) が届く。
    ///
    /// 全画面への出入りで呼ばれる (`fullscreenchange`)。利用者が Esc で
    /// 全画面を出たときも届く。
    pub fn on_state_change(&self, f: impl FnMut(WindowState) + 'static) {
        self.0.on_state_change.set(f);
    }

    /// 利用者が大きさを変えられるか。[`set_resizable`](Self::set_resizable) で
    /// 頼んだ値を返す (既定は `true`)。
    pub fn is_resizable(&self) -> bool {
        self.0.resizable.get()
    }

    /// 利用者が大きさを変えられるかどうか。**Web では効かない** (値を覚えるだけ)。
    ///
    /// ページの中のウィンドウには利用者がつかむ縁が無く、ブラウザの窓の
    /// 大きさはページからは止められないため。4 環境で同じコードを書けるよう、
    /// 受け付けるだけは受け付ける。
    pub fn set_resizable(&self, resizable: bool) {
        self.0.resizable.set(resizable);
    }

    /// このウィンドウの中で押されたキーの通知。
    ///
    /// フォーカスのあるウィジェットの `on_key_down` が `Continue` を返した
    /// キーが届く。`Handled` を返すと、そのキーの既定動作を止める。
    /// IME で変換している間のキーは届かない。
    pub fn on_key_down(
        &self,
        f: impl FnMut(&naui_core::KeyEvent) -> naui_core::EventResponse + 'static,
    ) {
        let document: &web_sys::EventTarget = self.0.document.as_ref();
        let _ = self
            .0
            .key_down
            .set_on_window(document, self.0.element.as_ref(), f);
    }

    /// 利用者が縮められる下限 (中身の幅, 高さ)。単位は [`set_size`](Self::set_size) と同じ。
    ///
    /// 小さくしすぎてレイアウトが崩れるのを防ぐ。
    ///
    /// Web ではウィンドウの要素に `min-width` / `min-height` を付ける
    /// (ブラウザの窓そのものの大きさは決められないので、狭いときはスクロールになる)。
    /// 高さにはメニューバーとツールバーの行の高さを足す。
    pub fn set_min_size(&self, width: f64, height: f64) {
        self.0.min_size.set(Some((width.max(0.0), height.max(0.0))));
        self.apply_size();
    }

    pub fn set_title(&self, title: &str) {
        *self.0.title.borrow_mut() = title.to_string();
        self.0.document.set_title(title);
    }

    pub fn title(&self) -> String {
        self.0.title.borrow().clone()
    }

    /// 中身の領域の大きさを変える。単位と範囲は [`size`](Self::size) と同じで、
    /// メニューバーとツールバーの行は含まない (ほかの環境と同じ)。
    ///
    /// ブラウザの窓そのものの大きさは決められないので、ウィンドウ要素の
    /// `max-width` (幅の上限。狭い画面では縮む) と `min-height` (高さの下限。
    /// 中身が多ければ伸びる) に写す。高さには行の高さを足す。
    pub fn set_size(&self, width: f64, height: f64) {
        self.0.size.set((width.max(0.0), height.max(0.0)));
        self.apply_size();
    }

    /// ルートに置くウィジェット。呼ぶたびに置き換わる。
    ///
    /// ルートはウィンドウいっぱいに広がる (AppKit の contentView と同じ)。
    /// サイドバーを付けているときは、その右の中身の側に置かれる。
    pub fn set_child(&self, child: &dyn Widget) {
        *self.0.child.borrow_mut() = Some(child.boxed_clone());
        self.mount_body();
    }

    /// ウィンドウの左に付けるサイドバー。呼ぶたびに置き換わる。
    ///
    /// ブラウザにはサイドバーのコントロールが無いため、ウィンドウ要素の中を
    /// 「サイドバー (`<aside>`) と中身」の横並びへ組み替え、
    /// [`set_child`](Self::set_child) の子は中身の側へ移す。
    pub fn set_sidebar(&self, sidebar: &Sidebar) {
        *self.0.sidebar.borrow_mut() = Some(sidebar.clone());
        self.mount_body();
    }

    /// 取り付けたサイドバーを外す。付いていなければ何もしない。
    ///
    /// 中身の側にあった子は、ウィンドウの中身へ戻る。
    pub fn clear_sidebar(&self) {
        let old = self.0.sidebar.borrow_mut().take();
        if let Some(old) = old {
            old.set_content(None);
            old.mount().remove();
            self.mount_body();
        }
    }

    /// ウィンドウ要素の中身を組み直す。
    ///
    /// 子 (とサイドバー) を置き、先頭へツールバーとメニューバーを差し込む。
    fn mount_body(&self) {
        self.0.element.set_inner_html("");
        let child = self
            .0
            .child
            .borrow()
            .as_ref()
            .map(|child| child.native_element());
        let sidebar = self.0.sidebar.borrow().clone();
        let body = match &sidebar {
            Some(sidebar) => {
                sidebar.set_content(child.as_ref());
                Some(sidebar.mount().unchecked_into::<Element>())
            }
            None => child,
        };
        if let Some(body) = body {
            if self.0.element.append_child(&body).is_ok() {
                crate::layout::fill_parent(&body);
                crate::layout::apply_child_layout(
                    &body,
                    crate::layout::ParentLayout::Flex(naui_core::Orientation::Vertical),
                );
            }
        }
        // 中身を入れ替えると差し込んだ要素も消えるので、付け直す。
        // メニューバーはツールバーより上に来る (OS のメニューと同じ順)。
        self.mount_toolbar();
        self.mount_menu_bar();
    }

    /// ウィンドウの上端に付けるツールバー。呼ぶたびに置き換わる。
    ///
    /// ブラウザにはタイトルバーが無いため、ウィンドウ要素の先頭に置く。
    pub fn set_toolbar(&self, toolbar: &Toolbar) {
        self.clear_toolbar();
        *self.0.toolbar.borrow_mut() = Some(toolbar.clone());
        self.mount_toolbar();
    }

    /// 取り付けたツールバーを外す。付いていなければ何もしない。
    pub fn clear_toolbar(&self) {
        if let Some(old) = self.0.toolbar.borrow_mut().take() {
            old.mount().remove();
        }
        self.mount_toolbar();
    }

    /// ツールバーの行を組み直して、ウィンドウの先頭へ置く。
    ///
    /// 並びは [サイドバーの開閉ボタン][ツールバー]。どちらも無ければ行ごと
    /// 外す。ツールバーが無くてもサイドバーがあれば、ボタンだけの行になる
    /// (macOS のボタンだけのツールバーと同じ)。
    fn mount_toolbar(&self) {
        let row = &self.0.toolbar_row;
        row.set_inner_html("");
        if let Some(sidebar) = self.0.sidebar.borrow().as_ref() {
            let _ = row.append_child(&sidebar.toggle());
        }
        if let Some(toolbar) = self.0.toolbar.borrow().as_ref() {
            let _ = row.append_child(&toolbar.mount());
        }
        if row.child_element_count() == 0 {
            row.remove();
            return;
        }
        // メニューバーが付いていれば、その直下 (OS のメニューと同じ順)。
        let menu_bar = self.0.menu_bar.borrow().as_ref().map(|m| m.mount());
        match menu_bar.filter(|m| m.parent_element().as_ref() == Some(self.0.element.as_ref())) {
            Some(menu_bar) => {
                let _ = self
                    .0
                    .element
                    .insert_before(row, menu_bar.next_sibling().as_ref());
            }
            None => self.mount_first(row),
        }
    }

    /// ウィンドウの上端に付けるメニューバー。呼ぶたびに置き換わる。
    ///
    /// ブラウザには OS のメニューバーが無いため、ウィンドウ要素の先頭
    /// (ツールバーより上) に置く。
    pub fn set_menu_bar(&self, menu_bar: &MenuBar) {
        self.clear_menu_bar();
        *self.0.menu_bar.borrow_mut() = Some(menu_bar.clone());
        self.mount_menu_bar();
        if let Some(observer) = self.0.bars_observer.borrow().as_ref() {
            observer.observe(menu_bar.mount().as_ref());
        }
        // ショートカットの購読は、取り付けている間だけ張る。
        menu_bar.attach(self.0.element.as_ref());
    }

    /// 取り付けたメニューバーを外す。付いていなければ何もしない。
    ///
    /// 外したメニューバーはショートカットにも反応しなくなる。
    pub fn clear_menu_bar(&self) {
        let old = self.0.menu_bar.borrow_mut().take();
        if let Some(old) = old {
            old.detach();
            if let Some(observer) = self.0.bars_observer.borrow().as_ref() {
                observer.unobserve(old.mount().as_ref());
            }
            old.mount().remove();
            self.apply_size();
        }
    }

    /// メニューバーをウィンドウの先頭へ置き直す。
    fn mount_menu_bar(&self) {
        let menu_bar = self.0.menu_bar.borrow();
        let Some(menu_bar) = menu_bar.as_ref() else {
            return;
        };
        self.mount_first(&menu_bar.mount());
    }

    /// ウィンドウ要素の先頭へ差し込む。
    fn mount_first(&self, mount: &HtmlElement) {
        let first = self.0.element.first_element_child();
        let _ = self
            .0
            .element
            .insert_before(mount, first.as_ref().map(|e| e.as_ref()));
    }

    /// 表示する。Web では最初から表示されているため、隠していた場合に戻す。
    pub fn show(&self) {
        // 中身を縦に積む flex コンテナへ戻す (`none` からの復帰)。
        let _ = self.0.element.style().set_property("display", "flex");
    }

    pub fn close(&self) {
        // 開いたままのメニューを残さない (隠れたウィンドウの Esc を拾わない)。
        let menu_bar = self.0.menu_bar.borrow().clone();
        if let Some(menu_bar) = menu_bar {
            menu_bar.close();
        }
        let _ = self.0.element.style().set_property("display", "none");
    }

    pub fn is_visible(&self) -> bool {
        self.0
            .element
            .style()
            .get_property_value("display")
            .map(|v| v != "none")
            .unwrap_or(true)
    }

    /// このウィンドウの配色テーマを切り替える。
    pub fn set_theme(&self, theme: Theme) -> Result<()> {
        apply_theme(&self.0.document, theme)
    }

    /// DOM 要素。バックエンド固有の脱出口。
    pub fn native_element(&self) -> Element {
        self.0.element.clone().unchecked_into()
    }
}

/// `target` のメソッド `name` を引数なしで呼び、返った Promise を返す。
///
/// web-sys の `request_fullscreen` / `exit_fullscreen` は Promise を返さない古い
/// 形なので、断られたことを受けられない。メソッドが無いブラウザ (iPhone の
/// Safari の `requestFullscreen` など) では `None`。
fn call_promise(target: &wasm_bindgen::JsValue, name: &str) -> Option<js_sys::Promise> {
    let method = js_sys::Reflect::get(target, &wasm_bindgen::JsValue::from_str(name))
        .ok()?
        .dyn_into::<js_sys::Function>()
        .ok()?;
    method
        .call0(target)
        .ok()?
        .dyn_into::<js_sys::Promise>()
        .ok()
}
