//! トースト (DOM)。
//!
//! ブラウザに「しばらく出してから自分で消える通知」の標準要素は無い
//! (`Notification` API はページの外へ出るもので、別物)。そのため
//! [`PopupMenu`](crate::PopupMenu) と同じく、`<div role="status">` と
//! `<button>` で組み立てる。
//!
//! 色は CSS のシステムカラー (`Canvas` / `CanvasText`) を使うので、
//! [`crate::Ui::set_theme`] が `<html>` に設定する `color-scheme` へ
//! そのまま追従する。CSS で決めているのは位置 (下端の中央)、余白、
//! 角の丸みだけ。
//!
//! 読み上げは `role="status"` (`aria-live="polite"`) に任せる。文字が
//! 入った時点で、読み上げ中の内容を遮らずに読まれる。
//!
//! 消えるまでの時間は `setTimeout` が数える。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use naui_core::{Error, Result, ToastSpec};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use web_sys::{Document, Element, HtmlElement};

use crate::to_error;
use crate::widgets::{create, Listener};

thread_local! {
    /// いま出ているトースト。同時に出るのは 1 つで、新しいものが置き換える。
    static CURRENT: RefCell<Option<Toast>> = const { RefCell::new(None) };
}

fn style(element: &HtmlElement, property: &str, value: &str) {
    let _ = element.style().set_property(property, value);
}

struct ToastInner {
    /// トースト本体。`<body>` 直下に置き、出ていない間は `display: none`。
    element: HtmlElement,
    /// 文字を出す `<span>`。
    label: HtmlElement,
    /// 操作ボタン。置いていない間は `hidden`。
    button: HtmlElement,
    spec: RefCell<ToastSpec>,
    /// 操作ボタンのクリック購読。
    _click: RefCell<Option<Listener>>,
    /// 時間切れをブラウザから受けるクロージャ。**トーストと同じだけ
    /// 生かしておく。**
    ///
    /// 出すたびに作り直さないのは、この中から消したときに
    /// **実行中のクロージャそのものを解放してしまわない**ようにするため。
    tick: RefCell<Option<Closure<dyn FnMut()>>>,
    /// `setTimeout` の予約。消えない指定なら持たない。
    timeout: Cell<Option<i32>>,
    on_action: RefCell<Option<Box<dyn FnMut()>>>,
    on_dismiss: RefCell<Option<Box<dyn FnMut()>>>,
    visible: Cell<bool>,
}

/// 一時的な通知 (`<div role="status">`)。
///
/// 画面に並ぶウィジェットではないので [`Widget`](crate::Widget) ではない。
#[derive(Clone)]
pub struct Toast(Rc<ToastInner>);

impl Toast {
    pub(crate) fn new(doc: &Document, message: &str) -> Result<Self> {
        let element: HtmlElement = create(doc, "div")?.unchecked_into();
        element
            .set_attribute("role", "status")
            .map_err(|e| to_error("トーストの組み立て", e))?;
        // 位置と重なりだけを CSS で作る。色はシステムカラーに任せる。
        style(&element, "position", "fixed");
        style(&element, "display", "none");
        style(&element, "left", "50%");
        style(&element, "bottom", "24px");
        style(&element, "transform", "translateX(-50%)");
        style(&element, "z-index", "1000");
        style(&element, "align-items", "center");
        style(&element, "gap", "12px");
        style(&element, "padding", "10px 16px");
        style(&element, "max-width", "calc(100% - 48px)");
        style(&element, "box-sizing", "border-box");
        style(&element, "background", "Canvas");
        style(&element, "color", "CanvasText");
        style(&element, "border", "1px solid CanvasText");
        style(&element, "border-radius", "10px");
        style(&element, "box-shadow", "0 4px 12px rgba(0, 0, 0, 0.25)");

        let label: HtmlElement = create(doc, "span")?.unchecked_into();
        label.set_text_content(Some(message));
        let button: HtmlElement = create(doc, "button")?.unchecked_into();
        button.set_hidden(true);
        for part in [&label, &button] {
            element
                .append_child(part)
                .map_err(|e| to_error("トーストの組み立て", e))?;
        }

        doc.body()
            .ok_or_else(|| Error::new("トーストの配置", "body 要素がありません"))?
            .append_child(&element)
            .map_err(|e| to_error("トーストの配置", e))?;

        let this = Self(Rc::new(ToastInner {
            element,
            label,
            button,
            spec: RefCell::new(ToastSpec::new(message)),
            _click: RefCell::new(None),
            tick: RefCell::new(None),
            timeout: Cell::new(None),
            on_action: RefCell::new(None),
            on_dismiss: RefCell::new(None),
            visible: Cell::new(false),
        }));
        this.install_action_handler()?;
        this.install_tick();
        Ok(this)
    }

    /// 時間切れを受けるクロージャを作る。以後はこれを使い回す。
    fn install_tick(&self) {
        let weak = Rc::downgrade(&self.0);
        let tick = Closure::<dyn FnMut()>::new(move || {
            let Some(inner) = weak.upgrade() else {
                return;
            };
            Toast(inner).finish(false);
        });
        *self.0.tick.borrow_mut() = Some(tick);
    }

    /// 操作ボタンが押されたら、通知して消えるようにする。
    fn install_action_handler(&self) -> Result<()> {
        let weak = Rc::downgrade(&self.0);
        let listener = Listener::attach(self.0.button.as_ref(), "click", move || {
            let Some(inner) = weak.upgrade() else {
                return;
            };
            Toast(inner).finish(true);
        })?;
        *self.0._click.borrow_mut() = Some(listener);
        Ok(())
    }

    /// 出す文字列。出している間に呼ぶと、その場で書き換わる。
    pub fn set_message(&self, message: &str) {
        self.0.spec.borrow_mut().set_message(message);
        self.0.label.set_text_content(Some(message));
    }

    pub fn message(&self) -> String {
        self.0.spec.borrow().message().to_string()
    }

    /// 操作ボタンの文字列。**空文字列を渡すとボタンを外す。**
    pub fn set_action(&self, label: &str) {
        self.0.spec.borrow_mut().set_action(label);
        let action = self.0.spec.borrow().action().map(str::to_string);
        match action {
            Some(label) => {
                self.0.button.set_text_content(Some(&label));
                self.0.button.set_hidden(false);
            }
            None => {
                self.0.button.set_text_content(None);
                self.0.button.set_hidden(true);
            }
        }
    }

    /// 操作ボタンの文字列。置いていなければ空文字列。
    pub fn action(&self) -> String {
        self.0
            .spec
            .borrow()
            .action()
            .unwrap_or_default()
            .to_string()
    }

    /// 自動で消えるまでの秒数。**0 を渡すと自動では消えない。**
    ///
    /// 次に [`show`](Self::show) したときから効く。
    pub fn set_timeout(&self, seconds: f64) {
        self.0.spec.borrow_mut().set_timeout(seconds);
    }

    pub fn timeout(&self) -> f64 {
        self.0.spec.borrow().timeout()
    }

    /// いまの設定。
    pub fn spec(&self) -> ToastSpec {
        self.0.spec.borrow().clone()
    }

    /// 操作ボタンが押されたときに呼ばれる。設定し直すと以前のものは外れる。
    ///
    /// 押されるとトーストは消えるので、続けて `on_dismiss` も呼ばれる。
    pub fn on_action(&self, f: impl FnMut() + 'static) {
        *self.0.on_action.borrow_mut() = Some(Box::new(f));
    }

    /// 消えたときに呼ばれる。設定し直すと以前のものは外れる。
    ///
    /// 呼ばれるのは**時間で消えたとき**と**操作ボタンで消えたとき**。
    /// [`dismiss`](Self::dismiss) で消したときと、別のトーストに
    /// 置き換えられたときは呼ばれない (アプリ自身の操作は通知しない、
    /// という [`Dialog::close`](crate::Dialog::close) と同じ決まり)。
    pub fn on_dismiss(&self, f: impl FnMut() + 'static) {
        *self.0.on_dismiss.borrow_mut() = Some(Box::new(f));
    }

    /// トーストを出す。
    ///
    /// **同時に出るのは 1 つ**で、ほかのトーストが出ていれば置き換える
    /// (置き換えられたほうの `on_dismiss` は呼ばれない)。
    pub fn show(&self) {
        if let Some(previous) = CURRENT.with(|slot| slot.borrow_mut().take()) {
            previous.take_down();
        }
        style(&self.0.element, "display", "flex");
        self.0.visible.set(true);
        self.start_timeout();
        CURRENT.with(|slot| *slot.borrow_mut() = Some(self.clone()));
    }

    /// 出しているトーストを消す。`on_dismiss` は呼ばれない。
    pub fn dismiss(&self) {
        if !self.is_visible() {
            return;
        }
        self.forget_current();
        self.take_down();
    }

    /// いま出ているか。
    pub fn is_visible(&self) -> bool {
        self.0.visible.get()
    }

    /// トーストの要素。バックエンド固有の脱出口。
    pub fn native_element(&self) -> Element {
        self.0.element.clone().unchecked_into()
    }

    /// 消えたことをアプリへ知らせる。`action` は操作ボタンで消えたか。
    fn finish(&self, action: bool) {
        if !self.is_visible() {
            return;
        }
        self.forget_current();
        self.take_down();
        if action {
            emit(&self.0.on_action);
        }
        emit(&self.0.on_dismiss);
    }

    /// 時間を数え始める。消えない指定なら何もしない。
    fn start_timeout(&self) {
        let Some(millis) = self.0.spec.borrow().timeout_millis() else {
            return;
        };
        let Some(window) = web_sys::window() else {
            return;
        };
        let handle = self.0.tick.borrow().as_ref().and_then(|tick| {
            window
                .set_timeout_with_callback_and_timeout_and_arguments_0(
                    tick.as_ref().unchecked_ref(),
                    millis,
                )
                .ok()
        });
        self.0.timeout.set(handle);
    }

    /// 隠して、時間を数えるのをやめる。通知はしない。
    fn take_down(&self) {
        if let (Some(handle), Some(window)) = (self.0.timeout.take(), web_sys::window()) {
            window.clear_timeout_with_handle(handle);
        }
        style(&self.0.element, "display", "none");
        self.0.visible.set(false);
    }

    /// 「いま出ているトースト」が自分なら、その座を空ける。
    fn forget_current(&self) {
        CURRENT.with(|slot| {
            let mine = slot
                .borrow()
                .as_ref()
                .is_some_and(|current| Rc::ptr_eq(&current.0, &self.0));
            if mine {
                slot.borrow_mut().take();
            }
        });
    }
}

/// 通知。通知の中から設定し直しても二重借用にならないよう、
/// 呼び出しの間だけクロージャを取り出す。
fn emit(slot: &RefCell<Option<Box<dyn FnMut()>>>) {
    let Some(mut f) = slot.borrow_mut().take() else {
        return;
    };
    f();
    let mut slot = slot.borrow_mut();
    if slot.is_none() {
        *slot = Some(f);
    }
}
