//! 汎用ダイアログ (DOM)。
//!
//! ブラウザのモーダルは **`<dialog>`** そのもので、前面へ出すのも、
//! 背後の操作を止めるのも、Esc で閉じるのも、`showModal()` を呼んだあとは
//! ブラウザが行う (`::backdrop` の暗幕も含む)。
//!
//! ただし `<dialog>` は中身を持たない箱なので、見出し・本文・ボタンは
//! naui が `<h2>` / `<p>` / `<button>` を並べて組み立てる。CSS は
//! Flexbox の並べ方 (方向・間隔・右寄せ) にしか使わない。
//!
//! ## 他の環境との違い
//!
//! [`Dialog::open`] はすぐ戻り、閉じたときの通知はあとから届く
//! (macOS の `NSAlert` は閉じるまで戻らない)。
//!
//! 閉じたことは `<dialog>` の `close` イベントではなく、**押されたボタン**と
//! **`cancel` イベント (Esc)** から直接わかるようにしている。`close` は
//! `close()` を呼んだ側 (アプリ自身の [`Dialog::close`] を含む) と
//! ユーザー操作を区別できないうえ、埋め込みブラウザによっては届かない。

use std::cell::RefCell;
use std::rc::Rc;

use naui_core::{DialogButtons, DialogResponse, Result};
use wasm_bindgen::JsCast;
use web_sys::{Document, Element, HtmlDialogElement, HtmlElement};

use crate::to_error;
use crate::widgets::{create, Listener, Widget};

/// ボタンを左から並べる順。macOS の並びに合わせ、主となる操作を右端に置く。
const BUTTON_ORDER: [DialogResponse; 3] = [
    DialogResponse::Secondary,
    DialogResponse::Cancel,
    DialogResponse::Primary,
];

struct DialogInner {
    element: HtmlDialogElement,
    title: HtmlElement,
    message: HtmlElement,
    /// 中身のウィジェットを入れる箱。
    content: HtmlElement,
    /// ボタンを並べる箱。
    actions: HtmlElement,
    child: RefCell<Option<Box<dyn Widget>>>,
    buttons: RefCell<DialogButtons>,
    /// ボタンのクリックを受けるもの。組み立て直すたびに置き換わる。
    listeners: RefCell<Vec<Listener>>,
    /// Esc で閉じられたことを受けるもの。
    cancel: RefCell<Option<Listener>>,
    on_response: RefCell<Option<Box<dyn FnMut(DialogResponse)>>>,
}

/// モーダルダイアログ (`<dialog>`)。
#[derive(Clone)]
pub struct Dialog(Rc<DialogInner>);

impl Dialog {
    pub(crate) fn new(doc: &Document, title: &str) -> Result<Self> {
        let element: HtmlDialogElement = create(doc, "dialog")?.unchecked_into();
        let body: HtmlElement = create(doc, "div")?.unchecked_into();
        let style = body.style();
        let _ = style.set_property("display", "flex");
        let _ = style.set_property("flex-direction", "column");
        let _ = style.set_property("gap", "12px");

        let title_element: HtmlElement = create(doc, "h2")?.unchecked_into();
        // 見出しの既定の余白は箱の間隔 (gap) と二重になるので外す。
        let _ = title_element.style().set_property("margin", "0");
        let message: HtmlElement = create(doc, "p")?.unchecked_into();
        let _ = message.style().set_property("margin", "0");
        message.set_hidden(true);
        let content: HtmlElement = create(doc, "div")?.unchecked_into();
        content.set_hidden(true);
        let actions: HtmlElement = create(doc, "div")?.unchecked_into();
        let actions_style = actions.style();
        let _ = actions_style.set_property("display", "flex");
        let _ = actions_style.set_property("gap", "8px");
        let _ = actions_style.set_property("justify-content", "flex-end");

        for part in [&title_element, &message, &content, &actions] {
            body.append_child(part)
                .map_err(|e| to_error("ダイアログの組み立て", e))?;
        }
        element
            .append_child(&body)
            .map_err(|e| to_error("ダイアログの組み立て", e))?;
        // `showModal()` は文書に入っている要素にしか効かないため、
        // 作った時点で `<body>` へ入れておく。
        doc.body()
            .ok_or_else(|| naui_core::Error::new("body の取得", "body がありません"))?
            .append_child(&element)
            .map_err(|e| to_error("ダイアログの追加", e))?;

        let this = Self(Rc::new(DialogInner {
            element,
            title: title_element,
            message,
            content,
            actions,
            child: RefCell::new(None),
            buttons: RefCell::new(DialogButtons::new()),
            listeners: RefCell::new(Vec::new()),
            cancel: RefCell::new(None),
            on_response: RefCell::new(None),
        }));
        this.set_title(title);
        this.rebuild_buttons()?;
        this.install_cancel_handler()?;
        Ok(this)
    }

    /// Esc で閉じられたときに、取り消しとして通知する。
    ///
    /// ブラウザ既定の「閉じる」は止めて自分で閉じる。こうすると
    /// 通知の中から開き直しても、そのあとブラウザに閉じられない。
    fn install_cancel_handler(&self) -> Result<()> {
        let weak = Rc::downgrade(&self.0);
        let listener = Listener::attach_event(self.0.element.as_ref(), "cancel", move |event| {
            let Some(inner) = weak.upgrade() else {
                return;
            };
            event.prevent_default();
            inner.element.close();
            Dialog(inner).emit(DialogResponse::Cancel);
        })?;
        *self.0.cancel.borrow_mut() = Some(listener);
        Ok(())
    }

    /// 通知。通知の中から設定し直しても二重借用にならないよう、
    /// 呼び出しの間だけクロージャを取り出す。
    fn emit(&self, response: DialogResponse) {
        let Some(mut f) = self.0.on_response.borrow_mut().take() else {
            return;
        };
        f(response);
        let mut slot = self.0.on_response.borrow_mut();
        if slot.is_none() {
            *slot = Some(f);
        }
    }

    pub fn set_title(&self, title: &str) {
        self.0.title.set_text_content(Some(title));
        self.0.title.set_hidden(title.is_empty());
    }

    pub fn title(&self) -> String {
        self.0.title.text_content().unwrap_or_default()
    }

    /// 見出しの下に出る本文。空にすると出ない。
    pub fn set_message(&self, message: &str) {
        self.0.message.set_text_content(Some(message));
        self.0.message.set_hidden(message.is_empty());
    }

    pub fn message(&self) -> String {
        self.0.message.text_content().unwrap_or_default()
    }

    /// 本文とボタンの間に置くウィジェット。呼ぶたびに置き換わる。
    pub fn set_child(&self, child: &dyn Widget) {
        self.0.content.set_inner_html("");
        let element = child.native_element();
        if self.0.content.append_child(&element).is_ok() {
            self.0.content.set_hidden(false);
            *self.0.child.borrow_mut() = Some(child.boxed_clone());
        }
    }

    /// 出すボタン。既定ではボタンを持たず、そのときは「OK」だけが出る。
    pub fn set_buttons(&self, buttons: DialogButtons) {
        *self.0.buttons.borrow_mut() = buttons;
        let _ = self.rebuild_buttons();
    }

    pub fn buttons(&self) -> DialogButtons {
        self.0.buttons.borrow().clone()
    }

    /// `<button>` を並べ直す。押されたら `<dialog>` を閉じ、その役割で通知する。
    fn rebuild_buttons(&self) -> Result<()> {
        // 以前のボタンとその購読を外す。
        self.0.listeners.borrow_mut().clear();
        self.0.actions.set_inner_html("");

        let document = self
            .0
            .element
            .owner_document()
            .ok_or_else(|| naui_core::Error::new("document の取得", "文書がありません"))?;
        let mut listeners = Vec::new();
        for (role, label) in self.0.buttons.borrow().resolved().in_order(&BUTTON_ORDER) {
            let button: HtmlElement = create(&document, "button")?.unchecked_into();
            button.set_text_content(Some(&label));
            self.0
                .actions
                .append_child(&button)
                .map_err(|e| to_error("ダイアログのボタン追加", e))?;
            let weak = Rc::downgrade(&self.0);
            listeners.push(Listener::attach(button.as_ref(), "click", move || {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                inner.element.close();
                Dialog(inner).emit(role);
            })?);
        }
        *self.0.listeners.borrow_mut() = listeners;
        Ok(())
    }

    /// 閉じたときに、閉じた理由で呼ばれる。設定し直すと以前のものは外れる。
    ///
    /// [`Dialog::close`] で閉じたときは呼ばれない。
    pub fn on_response(&self, f: impl FnMut(DialogResponse) + 'static) {
        *self.0.on_response.borrow_mut() = Some(Box::new(f));
    }

    /// ダイアログを出す。**すぐ戻り、閉じたことは `on_response` で届く。**
    ///
    /// すでに出ているときは何もしない。
    pub fn open(&self) {
        if self.0.element.open() {
            return;
        }
        let _ = self.0.element.show_modal();
    }

    /// 出しているダイアログを閉じる。`on_response` は呼ばれない。
    pub fn close(&self) {
        if !self.0.element.open() {
            return;
        }
        self.0.element.close();
    }

    /// いま出ているか。
    pub fn is_open(&self) -> bool {
        self.0.element.open()
    }

    /// `<dialog>` 要素。バックエンド固有の脱出口。
    pub fn native_element(&self) -> Element {
        self.0.element.clone().unchecked_into()
    }
}
