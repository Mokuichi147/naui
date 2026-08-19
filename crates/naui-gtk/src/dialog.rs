//! モーダルダイアログ (`AdwAlertDialog`)。
//!
//! 見出し・本文・任意のウィジェット (`extra_child`)・役割つきのボタンという
//! naui の形が、`AdwAlertDialog` とそのまま対応する。
//!
//! ボタンは `AdwAlertDialog` に足したあと取り除けないため、**開くたびに
//! ダイアログを組み立てる**。`Dialog` のほうは見出しや本文といった
//! 「何を出すか」だけを持つ。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use naui_core::{DialogButtons, DialogResponse};

use crate::callback::Notifier;
use crate::widgets::Widget;

/// 役割と `AdwAlertDialog` の応答 ID の対応。
const PRIMARY: &str = "primary";
const SECONDARY: &str = "secondary";
const CANCEL: &str = "cancel";

/// GNOME のガイドラインに合わせた並び。取り消しが左、主となる操作が右。
const ORDER: [DialogResponse; 3] = [
    DialogResponse::Cancel,
    DialogResponse::Secondary,
    DialogResponse::Primary,
];

fn response_id(response: DialogResponse) -> &'static str {
    match response {
        DialogResponse::Primary => PRIMARY,
        DialogResponse::Secondary => SECONDARY,
        DialogResponse::Cancel => CANCEL,
    }
}

fn from_response_id(id: &str) -> DialogResponse {
    match id {
        PRIMARY => DialogResponse::Primary,
        SECONDARY => DialogResponse::Secondary,
        // Esc や外側の操作で閉じたときもここへ来る。
        _ => DialogResponse::Cancel,
    }
}

struct DialogInner {
    app: adw::Application,
    title: RefCell<String>,
    message: RefCell<String>,
    buttons: RefCell<DialogButtons>,
    child: RefCell<Option<Box<dyn Widget>>>,
    on_response: Notifier<DialogResponse>,
    /// 開いている間だけ持つ実物。
    current: RefCell<Option<adw::AlertDialog>>,
    open: Cell<bool>,
}

/// モーダルダイアログ。
///
/// ウィジェットではないので、コンテナへは入れない (`Window` と同じ)。
#[derive(Clone)]
pub struct Dialog(Rc<DialogInner>);

impl Dialog {
    pub(crate) fn new(app: &adw::Application, title: &str) -> Self {
        Self(Rc::new(DialogInner {
            app: app.clone(),
            title: RefCell::new(title.to_string()),
            message: RefCell::new(String::new()),
            buttons: RefCell::new(DialogButtons::new()),
            child: RefCell::new(None),
            on_response: Notifier::default(),
            current: RefCell::new(None),
            open: Cell::new(false),
        }))
    }

    pub fn set_title(&self, title: &str) {
        *self.0.title.borrow_mut() = title.to_string();
        if let Some(native) = self.0.current.borrow().as_ref() {
            native.set_heading(Some(title));
        }
    }

    pub fn title(&self) -> String {
        self.0.title.borrow().clone()
    }

    pub fn set_message(&self, message: &str) {
        *self.0.message.borrow_mut() = message.to_string();
        if let Some(native) = self.0.current.borrow().as_ref() {
            native.set_body(message);
        }
    }

    pub fn message(&self) -> String {
        self.0.message.borrow().clone()
    }

    /// 本文の下に置くウィジェット。
    pub fn set_child(&self, child: &dyn Widget) {
        *self.0.child.borrow_mut() = Some(child.boxed_clone());
    }

    /// 出すボタン。1 つも指定しなければ「OK」だけになる。
    pub fn set_buttons(&self, buttons: DialogButtons) {
        *self.0.buttons.borrow_mut() = buttons;
    }

    pub fn buttons(&self) -> DialogButtons {
        self.0.buttons.borrow().clone()
    }

    /// 閉じたときに、その理由で呼ばれる。
    pub fn on_response(&self, f: impl FnMut(DialogResponse) + 'static) {
        self.0.on_response.set(f);
    }

    /// 開く。すでに開いていれば何もしない。
    pub fn open(&self) {
        if self.0.open.get() {
            return;
        }
        let native =
            adw::AlertDialog::new(Some(&self.0.title.borrow()), Some(&self.0.message.borrow()));

        let buttons = self.0.buttons.borrow().resolved();
        for (role, label) in buttons.in_order(&ORDER) {
            let id = response_id(role);
            native.add_response(id, &label);
            if role == DialogResponse::Primary {
                native.set_response_appearance(id, adw::ResponseAppearance::Suggested);
                native.set_default_response(Some(id));
            }
        }
        // Esc や外側を押して閉じたときは、取り消し扱いにする。
        native.set_close_response(CANCEL);

        if let Some(child) = self.0.child.borrow().as_ref() {
            let bin = child.size_bin();
            bin.fill_parent();
            native.set_extra_child(Some(&bin));
        }

        {
            let weak = Rc::downgrade(&self.0);
            native.connect_response(None, move |_, id| {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                // 1 回開くごとに応答は 1 回だけ。閉じたあとに来たものは捨てる。
                if !inner.open.get() {
                    return;
                }
                Dialog(inner.clone()).finish();
                inner.on_response.emit(from_response_id(id));
            });
        }

        self.0.open.set(true);
        *self.0.current.borrow_mut() = Some(native.clone());
        native.present(self.parent().as_ref());
    }

    /// ダイアログを載せるウィンドウ。
    ///
    /// まだどのウィンドウにも焦点が当たっていないことがある (起動直後など) ので、
    /// そのときはアプリが持っているウィンドウの先頭を使う。
    fn parent(&self) -> Option<gtk::Window> {
        self.0
            .app
            .active_window()
            .or_else(|| self.0.app.windows().into_iter().next())
    }

    /// 閉じる。開いていなければ何もしない。
    pub fn close(&self) {
        let native = self.0.current.borrow().clone();
        let Some(native) = native else {
            return;
        };
        // 閉じられれば `AdwAlertDialog` が取り消しの応答を出すので、
        // 後始末はそちらの経路で済む。
        if !native.close() {
            native.force_close();
        }
        // ウィンドウが無いなど、ネイティブが閉じられなかったときの受け皿。
        // naui の `close` は「必ず閉じて取り消しを知らせる」ものとして扱う。
        if self.0.open.get() {
            self.finish();
            self.0.on_response.emit(DialogResponse::Cancel);
        }
    }

    pub fn is_open(&self) -> bool {
        self.0.open.get()
    }

    /// いま開いている `AdwAlertDialog`。開いていなければ `None`。
    ///
    /// バックエンド固有の脱出口として公開している。
    pub fn native_dialog(&self) -> Option<adw::AlertDialog> {
        self.0.current.borrow().clone()
    }

    /// 閉じたあとの後始末。中身のウィジェットは次に開くときも使えるようにする。
    fn finish(&self) {
        self.0.open.set(false);
        if let Some(native) = self.0.current.borrow_mut().take() {
            native.set_extra_child(None::<&gtk::Widget>);
        }
    }
}
