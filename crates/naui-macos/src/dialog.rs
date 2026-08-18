//! 汎用ダイアログ (AppKit)。
//!
//! AppKit の「見出し・本文・ボタン・任意のビュー」を持つモーダルは
//! **`NSAlert`** で、配置も既定ボタンの見た目も Esc / Return の割り当ても
//! AppKit が行う。naui はここへ設定を写し、押されたボタンを役割へ戻すだけ。
//!
//! 任意のウィジェットは `NSAlert` の `accessoryView` に載せる。
//!
//! ## 他の環境との違い
//!
//! `NSAlert` の [`runModal`](NSAlert::runModal) は**アプリモーダル**で、
//! 閉じられるまで戻らない。そのため [`Dialog::open`] は閉じるまで戻らず、
//! `on_response` は `open()` の中で呼ばれる (Web / Windows では `open()` は
//! すぐ戻り、あとから通知が届く)。`FilePicker` と同じ性質。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use naui_core::{DialogButtons, DialogResponse};
use objc2::rc::Retained;
use objc2::MainThreadMarker;
use objc2_app_kit::{
    NSAlert, NSAlertFirstButtonReturn, NSApplication, NSModalResponse, NSModalResponseAbort, NSView,
};
use objc2_foundation::{NSSize, NSString};

use crate::widgets::Widget;

/// ボタンを `NSAlert` へ足す順。
///
/// `NSAlert` は**先に足したものほど右**へ置き、先頭を既定のボタンにする。
/// この順で足すと、左から「副操作・取り消し・主操作」という
/// macOS の並びになる。
const BUTTON_ORDER: [DialogResponse; 3] = [
    DialogResponse::Primary,
    DialogResponse::Cancel,
    DialogResponse::Secondary,
];

/// Esc キー。取り消しのボタンに割り当てる。
const ESCAPE_KEY: &str = "\u{1b}";

/// 閉じたときの通知先。差し替え可能な 1 本のクロージャを共有で持つ。
///
/// 通知の中からもう一度ダイアログを開くような使い方をしても二重借用に
/// ならないよう、呼び出しの間だけクロージャを取り出す
/// (`FilePicker` の通知と同じ作り)。
#[derive(Clone, Default)]
struct ResponseHandler(Rc<RefCell<Option<Box<dyn FnMut(DialogResponse)>>>>);

impl ResponseHandler {
    fn set(&self, f: impl FnMut(DialogResponse) + 'static) {
        *self.0.borrow_mut() = Some(Box::new(f));
    }

    fn emit(&self, response: DialogResponse) {
        let Some(mut f) = self.0.borrow_mut().take() else {
            return;
        };
        f(response);
        let mut slot = self.0.borrow_mut();
        if slot.is_none() {
            *slot = Some(f);
        }
    }
}

struct DialogInner {
    mtm: MainThreadMarker,
    title: RefCell<String>,
    message: RefCell<String>,
    /// 中身のウィジェット。トランポリンごと生かしておく。
    child: RefCell<Option<Box<dyn Widget>>>,
    buttons: RefCell<DialogButtons>,
    on_response: ResponseHandler,
    open: Cell<bool>,
    /// [`Dialog::close`] で閉じたか。閉じた理由を通知しないための目印。
    closed_by_app: Cell<bool>,
}

/// モーダルダイアログ (NSAlert)。
#[derive(Clone)]
pub struct Dialog(Rc<DialogInner>);

impl Dialog {
    pub(crate) fn new(mtm: MainThreadMarker, title: &str) -> Self {
        Self(Rc::new(DialogInner {
            mtm,
            title: RefCell::new(title.to_string()),
            message: RefCell::new(String::new()),
            child: RefCell::new(None),
            buttons: RefCell::new(DialogButtons::new()),
            on_response: ResponseHandler::default(),
            open: Cell::new(false),
            closed_by_app: Cell::new(false),
        }))
    }

    pub fn set_title(&self, title: &str) {
        *self.0.title.borrow_mut() = title.to_string();
    }

    pub fn title(&self) -> String {
        self.0.title.borrow().clone()
    }

    /// 見出しの下に出る本文。空にすると出ない。
    pub fn set_message(&self, message: &str) {
        *self.0.message.borrow_mut() = message.to_string();
    }

    pub fn message(&self) -> String {
        self.0.message.borrow().clone()
    }

    /// 本文とボタンの間に置くウィジェット。呼ぶたびに置き換わる。
    pub fn set_child(&self, child: &dyn Widget) {
        *self.0.child.borrow_mut() = Some(child.boxed_clone());
    }

    /// 出すボタン。既定ではボタンを持たず、そのときは「OK」だけが出る。
    pub fn set_buttons(&self, buttons: DialogButtons) {
        *self.0.buttons.borrow_mut() = buttons;
    }

    pub fn buttons(&self) -> DialogButtons {
        self.0.buttons.borrow().clone()
    }

    /// 閉じたときに、閉じた理由で呼ばれる。設定し直すと以前のものは外れる。
    ///
    /// [`Dialog::close`] で閉じたときは呼ばれない。
    pub fn on_response(&self, f: impl FnMut(DialogResponse) + 'static) {
        self.0.on_response.set(f);
    }

    /// ダイアログを出す。
    ///
    /// **`NSAlert` はアプリモーダルなので、閉じられるまで戻らない。**
    /// `on_response` はこの呼び出しの中で呼ばれる。
    /// すでに出ているときは何もしない。
    pub fn open(&self) {
        if self.0.open.get() {
            return;
        }
        let (alert, roles) = self.build();
        self.0.open.set(true);
        self.0.closed_by_app.set(false);
        let code = alert.runModal();
        self.0.open.set(false);
        if self.0.closed_by_app.replace(false) {
            return; // アプリ側から閉じた。通知はしない。
        }
        let Some(response) = response_for(&roles, code) else {
            return;
        };
        self.0.on_response.emit(response);
    }

    /// 出しているダイアログを閉じる。`on_response` は呼ばれない。
    pub fn close(&self) {
        if !self.0.open.get() {
            return;
        }
        self.0.closed_by_app.set(true);
        NSApplication::sharedApplication(self.0.mtm).abortModal();
    }

    /// いま出ているか。
    pub fn is_open(&self) -> bool {
        self.0.open.get()
    }

    /// いまの設定を反映した `NSAlert` を組み立てて返す。**まだ表示しない。**
    ///
    /// バックエンド固有の脱出口。シートとして出したい
    /// (`beginSheetModalForWindow:completionHandler:`)、アイコンや
    /// `alertStyle` を変えたい、といった AppKit 固有の使い方はここから行う。
    pub fn native_alert(&self) -> Retained<NSAlert> {
        self.build().0
    }

    /// `NSAlert` と、足したボタンの役割の並びを作る。
    ///
    /// 応答コードはボタンを足した順で返るので、その順を覚えておく。
    fn build(&self) -> (Retained<NSAlert>, Vec<DialogResponse>) {
        let alert = NSAlert::new(self.0.mtm);
        alert.setMessageText(&NSString::from_str(&self.0.title.borrow()));
        alert.setInformativeText(&NSString::from_str(&self.0.message.borrow()));

        if let Some(child) = self.0.child.borrow().as_deref() {
            let view = child.native_view();
            // `NSAlert` は accessoryView の **frame** を見て場所を空ける。
            // 制約に任せる (translatesAutoresizingMaskIntoConstraints = false)
            // ままだと、frame を入れても潰れてしまうので戻しておく。
            view.setTranslatesAutoresizingMaskIntoConstraints(true);
            view.setFrameSize(accessory_size(&view));
            alert.setAccessoryView(Some(&view));
        }

        let buttons = self.0.buttons.borrow().resolved();
        let mut roles = Vec::new();
        for (role, label) in buttons.in_order(&BUTTON_ORDER) {
            let button = alert.addButtonWithTitle(&NSString::from_str(&label));
            if role == DialogResponse::Cancel {
                // 取り消しは Esc で閉じられるようにする。文字列が
                // "Cancel" のときは AppKit も同じことをするが、
                // 日本語のラベルでも同じにそろえる。
                button.setKeyEquivalent(&NSString::from_str(ESCAPE_KEY));
            }
            roles.push(role);
        }
        (alert, roles)
    }
}

/// 幅を自分で決められない中身に渡す幅。`NSAlert` の本文欄とほぼ同じ。
const ACCESSORY_WIDTH: f64 = 260.0;
/// 高さを自分で決められない中身に渡す高さ (1 行ぶん)。
const ACCESSORY_HEIGHT: f64 = 22.0;

/// accessoryView に入れる frame の大きさ。
///
/// 制約から出る大きさ (`fittingSize`) を使うが、幅の決め手を持たない
/// ウィジェット (`NSTextField` など) では 0 が返る。そのときだけ既定値を
/// 使う。`set_sizing` で大きさを指定してあれば、そちらが優先される。
fn accessory_size(view: &NSView) -> NSSize {
    let fitting = view.fittingSize();
    NSSize::new(
        if fitting.width > 0.0 {
            fitting.width
        } else {
            ACCESSORY_WIDTH
        },
        if fitting.height > 0.0 {
            fitting.height
        } else {
            ACCESSORY_HEIGHT
        },
    )
}

/// `runModal` の戻り値を、押されたボタンの役割へ戻す。
///
/// `NSAlert` は足した順に `NSAlertFirstButtonReturn` から番号を振る。
/// [`Dialog::close`] による中断 (`NSModalResponseAbort`) と、範囲外の値は
/// 「押されていない」として `None` になる。
fn response_for(roles: &[DialogResponse], code: NSModalResponse) -> Option<DialogResponse> {
    if code == NSModalResponseAbort {
        return None;
    }
    let index = code.checked_sub(NSAlertFirstButtonReturn)?;
    let index = usize::try_from(index).ok()?;
    roles.get(index).copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn response_follows_the_order_buttons_were_added() {
        let roles = [
            DialogResponse::Primary,
            DialogResponse::Cancel,
            DialogResponse::Secondary,
        ];
        assert_eq!(
            response_for(&roles, NSAlertFirstButtonReturn),
            Some(DialogResponse::Primary)
        );
        assert_eq!(
            response_for(&roles, NSAlertFirstButtonReturn + 1),
            Some(DialogResponse::Cancel)
        );
        assert_eq!(
            response_for(&roles, NSAlertFirstButtonReturn + 2),
            Some(DialogResponse::Secondary)
        );
    }

    #[test]
    fn aborting_and_unknown_codes_do_not_map_to_a_button() {
        let roles = [DialogResponse::Cancel];
        assert_eq!(response_for(&roles, NSModalResponseAbort), None);
        assert_eq!(response_for(&roles, NSAlertFirstButtonReturn + 1), None);
        assert_eq!(response_for(&roles, 0), None);
    }

    #[test]
    fn button_order_puts_the_default_button_first() {
        // 先に足したものが右端かつ既定のボタンになる。
        assert_eq!(BUTTON_ORDER[0], DialogResponse::Primary);
    }
}
