//! トースト (`AdwToast`)。
//!
//! **4 環境で唯一、ネイティブのトーストがあるのがここ。** 文字・操作ボタン・
//! 消えるまでの時間という naui のトーストの形は、`AdwToast` とそのまま
//! 対応する。出す位置 (下端の中央)、重ね方、消えるときの動き、読み上げは
//! すべて `AdwToastOverlay` が行う。
//!
//! `AdwToast` は足したあと内容を変えても差し支えないが、**出すたびに
//! 作り直す** (`Dialog` が `AdwAlertDialog` を組み立て直すのと同じ)。
//! `Toast` のほうは「何を出すか」だけを持つ。
//!
//! ## 他の環境との違い
//!
//! `adw_toast_set_timeout` は**秒**しか受け取らないため、1 秒未満の指定は
//! 1 秒になる ([`ToastSpec::timeout_secs`])。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use naui_core::ToastSpec;

use crate::callback::Notifier;

thread_local! {
    /// いま出ているトースト。同時に出るのは 1 つで、新しいものが置き換える。
    static CURRENT: RefCell<Option<Toast>> = const { RefCell::new(None) };
}

struct ToastInner {
    app: adw::Application,
    spec: RefCell<ToastSpec>,
    /// 出している間だけ持つ実物。
    current: RefCell<Option<adw::Toast>>,
    on_action: Notifier<()>,
    on_dismiss: Notifier<()>,
    /// アプリ側から消したか。消えた理由を通知しないための目印。
    dismissed_by_app: Cell<bool>,
}

/// 一時的な通知 (`AdwToast`)。
///
/// ウィジェットではないので、コンテナへは入れない (`Dialog` と同じ)。
#[derive(Clone)]
pub struct Toast(Rc<ToastInner>);

impl Toast {
    pub(crate) fn new(app: &adw::Application, message: &str) -> Self {
        Self(Rc::new(ToastInner {
            app: app.clone(),
            spec: RefCell::new(ToastSpec::new(message)),
            current: RefCell::new(None),
            on_action: Notifier::default(),
            on_dismiss: Notifier::default(),
            dismissed_by_app: Cell::new(false),
        }))
    }

    /// 出す文字列。出している間に呼ぶと、その場で書き換わる。
    pub fn set_message(&self, message: &str) {
        self.0.spec.borrow_mut().set_message(message);
        if let Some(native) = self.0.current.borrow().as_ref() {
            native.set_title(message);
        }
    }

    pub fn message(&self) -> String {
        self.0.spec.borrow().message().to_string()
    }

    /// 操作ボタンの文字列。**空文字列を渡すとボタンを外す。**
    pub fn set_action(&self, label: &str) {
        self.0.spec.borrow_mut().set_action(label);
        if let Some(native) = self.0.current.borrow().as_ref() {
            native.set_button_label(self.0.spec.borrow().action());
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
    pub fn on_action(&self, mut f: impl FnMut() + 'static) {
        self.0.on_action.set(move |()| f());
    }

    /// 消えたときに呼ばれる。設定し直すと以前のものは外れる。
    ///
    /// 呼ばれるのは**時間で消えたとき**と**操作ボタンで消えたとき**。
    /// [`dismiss`](Self::dismiss) で消したときと、別のトーストに
    /// 置き換えられたときは呼ばれない (アプリ自身の操作は通知しない、
    /// という [`Dialog::close`](crate::Dialog::close) と同じ決まり)。
    pub fn on_dismiss(&self, mut f: impl FnMut() + 'static) {
        self.0.on_dismiss.set(move |()| f());
    }

    /// トーストを出す。
    ///
    /// **同時に出るのは 1 つ**で、ほかのトーストが出ていれば置き換える
    /// (置き換えられたほうの `on_dismiss` は呼ばれない)。
    /// 出せるウィンドウがまだ無いときは何もしない。
    ///
    /// `AdwToastOverlay` は足されたトーストを順番待ちさせる作りだが、
    /// naui は 4 環境で同じにそろえるため、前のものを消してから足す。
    pub fn show(&self) {
        let Some(overlay) = self.overlay() else {
            return;
        };
        if let Some(previous) = CURRENT.with(|slot| slot.borrow_mut().take()) {
            previous.take_down();
        }

        let spec = self.0.spec.borrow().clone();
        let native = adw::Toast::new(spec.message());
        native.set_timeout(spec.timeout_secs());
        native.set_button_label(spec.action());

        {
            let weak = Rc::downgrade(&self.0);
            native.connect_button_clicked(move |_| {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                inner.on_action.emit(());
            });
        }
        {
            let weak = Rc::downgrade(&self.0);
            native.connect_dismissed(move |_| {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                Toast(inner).finish();
            });
        }

        *self.0.current.borrow_mut() = Some(native.clone());
        self.0.dismissed_by_app.set(false);
        CURRENT.with(|slot| *slot.borrow_mut() = Some(self.clone()));
        overlay.add_toast(native);
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
        self.0.current.borrow().is_some()
    }

    /// いま出ている `AdwToast`。出していなければ `None`。
    ///
    /// バックエンド固有の脱出口として公開している。
    pub fn native_toast(&self) -> Option<adw::Toast> {
        self.0.current.borrow().clone()
    }

    /// `AdwToast` が消えたときの後始末と通知。
    fn finish(&self) {
        if self.0.current.borrow_mut().take().is_none() {
            return; // すでに片付けてある。
        }
        self.forget_current();
        if self.0.dismissed_by_app.replace(false) {
            return; // アプリ側から消した。通知はしない。
        }
        self.0.on_dismiss.emit(());
    }

    /// トーストを消す。通知はしない。
    fn take_down(&self) {
        let native = self.0.current.borrow().clone();
        let Some(native) = native else {
            return;
        };
        // 消せば `dismissed` が飛ぶので、後始末はそちらの経路で済む。
        self.0.dismissed_by_app.set(true);
        native.dismiss();
        // ウィンドウが閉じたあとなど、`dismissed` が来なかったときの受け皿。
        self.0.current.borrow_mut().take();
        self.0.dismissed_by_app.set(false);
    }

    /// トーストを載せる `AdwToastOverlay`。まだウィンドウが無ければ `None`。
    ///
    /// いちばん手前のウィンドウへ出す。まだどれにも焦点が当たっていないとき
    /// (起動直後など) は、アプリが持っているウィンドウの先頭を使う
    /// (`Dialog` が親を選ぶのと同じ考え方)。
    fn overlay(&self) -> Option<adw::ToastOverlay> {
        let window = self
            .0
            .app
            .active_window()
            .or_else(|| self.0.app.windows().into_iter().next())?;
        crate::window::toast_overlay(&window)
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
