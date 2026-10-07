//! トップレベルウィンドウ (`AdwApplicationWindow`)。
//!
//! `AdwApplicationWindow` は `GtkApplicationWindow` と違い、**既定の
//! タイトルバーを持たない**。最小化・最大化・閉じるのボタンは
//! `AdwHeaderBar` が出すので、中身をそのまま入れるのではなく
//! `AdwToolbarView` の上段にヘッダーバーを、下段にアプリの中身を置く。
//!
//! 下段はさらに `AdwToastOverlay` で包む。[`Toast`](crate::Toast) はここへ
//! 足され、ヘッダーバーより下・アプリの中身の上へ重なる (GNOME の作法)。
//!
//! [`Sidebar`] を付けると、ウィンドウの中身は `GtkPaned` になり、上の
//! `AdwToolbarView` はその右の区画へ移る。左の区画はサイドバー自身の
//! ヘッダーバーと一覧を持つ。

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use adw::prelude::*;
use gtk::glib;
use naui_core::{CloseHandler, CloseResponse, Result, Theme};

use crate::callback::Notifier;

use crate::menu_bar::MenuBar;
use crate::sidebar::Sidebar;
use crate::toolbar::Toolbar;
use crate::widgets::Widget;

pub(crate) struct WindowInner {
    native: adw::ApplicationWindow,
    /// アプリの中身と、そこへ重なるトーストの入れ物。
    overlay: adw::ToastOverlay,
    /// 上段 (ヘッダーバーとメニューバー) と下段 (アプリの中身) の入れ物。
    view: adw::ToolbarView,
    /// タイトルと、最小化・最大化・閉じるのボタン。
    header: adw::HeaderBar,
    child: RefCell<Option<Box<dyn Widget>>>,
    /// ヘッダーバーへ差し込んだツールバー。通知先ごと生かしておく。
    toolbar: RefCell<Option<Toolbar>>,
    /// ヘッダーバーの下へ差し込んだメニューバー。通知先ごと生かしておく。
    menu_bar: RefCell<Option<MenuBar>>,
    /// 取り付けたサイドバー。通知先ごと生かしておく。
    sidebar: RefCell<Option<Sidebar>>,
    on_close_request: CloseHandler,
    on_resize: Notifier<(f64, f64)>,
    /// naui の [`Window::close`] の最中。GTK4 はプログラムから閉じても
    /// `close-request` を出すので、そのときは確認しない。
    closing: Cell<bool>,
}

/// トップレベルウィンドウ。
///
/// `run` に渡したコールバックの中で作る。フレームワーク
/// (`GtkApplication`) が参照を保持するので、戻り値を捨てても閉じられない。
#[derive(Clone)]
pub struct Window(Rc<WindowInner>);

/// ウィンドウを強く保持せずにイベントハンドラから参照するための弱参照。
#[derive(Clone)]
pub struct WeakWindow(Weak<WindowInner>);

impl WeakWindow {
    pub fn upgrade(&self) -> Option<Window> {
        self.0.upgrade().map(Window)
    }
}

impl Window {
    pub(crate) fn new(app: &adw::Application, title: &str, width: f64, height: f64) -> Self {
        let native = adw::ApplicationWindow::builder()
            .application(app)
            .title(title)
            .default_width(to_px(width))
            .default_height(to_px(height))
            .build();

        // ヘッダーバーは自分でタイトルを描かず、ウィンドウの `title` を映す。
        let header = adw::HeaderBar::new();
        let view = adw::ToolbarView::new();
        view.add_top_bar(&header);
        // GTK4 は既定でははみ出した中身を切り取らない。窓より中身が大きいとき
        // (縮めすぎたとき) に、ウィンドウの外へ描かれてしまうのを止める。
        view.set_overflow(gtk::Overflow::Hidden);
        // アプリの中身は、トーストを重ねられる入れ物ごしに置く。
        let overlay = adw::ToastOverlay::new();
        view.set_content(Some(&overlay));
        native.set_content(Some(&view));
        // `on_key_down` の受け口。通知先が無いあいだは素通りする。
        crate::keys::install(&native);

        let this = Self(Rc::new(WindowInner {
            native,
            overlay,
            view,
            header,
            child: RefCell::new(None),
            toolbar: RefCell::new(None),
            menu_bar: RefCell::new(None),
            sidebar: RefCell::new(None),
            on_close_request: CloseHandler::default(),
            on_resize: Notifier::default(),
            closing: Cell::new(false),
        }));
        this.observe();
        this
    }

    fn observe(&self) {
        let weak = Rc::downgrade(&self.0);
        self.0.native.connect_close_request(move |_| {
            let Some(inner) = weak.upgrade() else {
                return glib::Propagation::Proceed;
            };
            if inner.closing.get() || inner.on_close_request.ask() == CloseResponse::Close {
                glib::Propagation::Proceed
            } else {
                glib::Propagation::Stop
            }
        });
        // GTK4 は利用者が大きさを変えると `default-width` / `default-height` を
        // 追従させる。中身の大きさが配られるのはその後のレイアウトなので、
        // ループを 1 周させてから読む。
        for property in ["default-width", "default-height"] {
            let weak = Rc::downgrade(&self.0);
            self.0
                .native
                .connect_notify_local(Some(property), move |_, _| {
                    let weak = weak.clone();
                    glib::idle_add_local_once(move || {
                        if let Some(inner) = weak.upgrade() {
                            let size = Window(inner.clone()).size();
                            inner.on_resize.emit(size);
                        }
                    });
                });
        }
    }

    /// 利用者がウィンドウを閉じようとしたときの確認。
    ///
    /// [`CloseResponse::KeepOpen`] を返すと閉じない (保存していない変更が
    /// あるときに確認のダイアログを出す、など)。プログラムからの
    /// [`close`](Self::close) では呼ばれない。
    pub fn on_close_request(&self, f: impl FnMut() -> CloseResponse + 'static) {
        self.0.on_close_request.set(f);
    }

    /// 中身の領域の大きさ (幅, 高さ)。単位は論理ピクセル。ヘッダーバーは含まない。
    ///
    /// 画面に出る前は、[`set_size`](Self::set_size) で指定した大きさ。
    pub fn size(&self) -> (f64, f64) {
        let (width, height) = (self.0.overlay.width(), self.0.overlay.height());
        if width > 0 && height > 0 {
            return (f64::from(width), f64::from(height));
        }
        let (width, height) = self.0.native.default_size();
        (f64::from(width.max(0)), f64::from(height.max(0)))
    }

    /// 大きさが変わったときの通知。変わった後の [`size`](Self::size) が届く。
    pub fn on_resize(&self, f: impl FnMut((f64, f64)) + 'static) {
        self.0.on_resize.set(f);
    }

    /// 対応する GTK4 のウィンドウ。バックエンド固有の脱出口として公開している。
    pub fn native_window(&self) -> adw::ApplicationWindow {
        self.0.native.clone()
    }

    /// タイトルと最小化・最大化・閉じるのボタンを持つヘッダーバー。
    ///
    /// バックエンド固有の脱出口として公開している。
    pub fn native_header_bar(&self) -> adw::HeaderBar {
        self.0.header.clone()
    }

    /// トーストが重なる `AdwToastOverlay`。
    ///
    /// バックエンド固有の脱出口として公開している。
    pub fn native_toast_overlay(&self) -> adw::ToastOverlay {
        self.0.overlay.clone()
    }

    pub fn downgrade(&self) -> WeakWindow {
        WeakWindow(Rc::downgrade(&self.0))
    }

    /// このウィンドウの中で押されたキーの通知。
    ///
    /// フォーカスのあるウィジェットの `on_key_down` が `Continue` を返した
    /// キーが届く。`Handled` を返すと、そのキーはウィジェットへ渡らない。
    /// IME で変換している間のキーは届かない。
    pub fn on_key_down(
        &self,
        f: impl FnMut(&naui_core::KeyEvent) -> naui_core::EventResponse + 'static,
    ) {
        crate::keys::set_window_handler(self.0.native.upcast_ref(), f);
    }

    pub fn set_title(&self, title: &str) {
        self.0.native.set_title(Some(title));
    }

    pub fn title(&self) -> String {
        self.0
            .native
            .title()
            .map(|t| t.to_string())
            .unwrap_or_default()
    }

    pub fn set_size(&self, width: f64, height: f64) {
        self.0.native.set_default_size(to_px(width), to_px(height));
    }

    /// ウィンドウの中身を差し替える。
    pub fn set_child(&self, child: &dyn Widget) {
        let bin = child.size_bin();
        // ウィンドウの中身は、他のバックエンドと同じく窓いっぱいに広がる。
        bin.fill_parent();
        // ヘッダーバーの下が、アプリの中身の置き場になる。
        // 直接ではなく、トーストを重ねる入れ物ごしに入れる。
        self.0.overlay.set_child(Some(&bin));
        *self.0.child.borrow_mut() = Some(child.boxed_clone());
    }

    /// ウィンドウの左に付けるサイドバー。呼ぶたびに置き換わる。
    ///
    /// ウィンドウの中身を `GtkPaned` へ差し替え、これまでの中身
    /// (ヘッダーバー・メニューバー・子) はその右の区画へ移す。
    pub fn set_sidebar(&self, sidebar: &Sidebar) {
        self.clear_sidebar();
        let paned = sidebar.native_paned();
        self.0.native.set_content(None::<&gtk::Widget>);
        paned.set_end_child(Some(&self.0.view));
        self.0.native.set_content(Some(&paned));
        *self.0.sidebar.borrow_mut() = Some(sidebar.clone());
        self.mount_header_start();
        sidebar.set_content_header(Some(&self.0.header));
    }

    /// 取り付けたサイドバーを外す。付いていなければ何もしない。
    ///
    /// 右の区画にあった中身は、ウィンドウの中身へ戻る。
    pub fn clear_sidebar(&self) {
        let Some(old) = self.0.sidebar.borrow_mut().take() else {
            return;
        };
        let paned = old.native_paned();
        self.0.native.set_content(None::<&gtk::Widget>);
        paned.set_end_child(None::<&gtk::Widget>);
        self.0.native.set_content(Some(&self.0.view));
        let slot = old.content_slot();
        if slot.parent().is_some() {
            self.0.header.remove(&slot);
        }
        old.set_content_header(None);
    }

    /// ヘッダーバーの左側を、サイドバーボタンの置き場 → ツールバーの順に
    /// 並べ直す。
    ///
    /// 置き場にはサイドバーを閉じている間だけサイドバーボタンが入る。
    /// `pack_start` は後ろへ足していくので、先に付いていたほうを外してから
    /// 並べる (置き場はいつも左端)。
    fn mount_header_start(&self) {
        let sidebar = self.0.sidebar.borrow().clone();
        let toolbar = self.0.toolbar.borrow().clone();
        if let Some(sidebar) = &sidebar {
            let slot = sidebar.content_slot();
            if slot.parent().is_some() {
                self.0.header.remove(&slot);
            }
        }
        if let Some(toolbar) = &toolbar {
            let mount = toolbar.mount();
            if mount.parent().is_some() {
                self.0.header.remove(&mount);
            }
        }
        if let Some(sidebar) = &sidebar {
            self.0.header.pack_start(&sidebar.content_slot());
        }
        if let Some(toolbar) = &toolbar {
            self.0.header.pack_start(&toolbar.mount());
        }
    }

    /// ウィンドウの上端に付けるツールバー。呼ぶたびに置き換わる。
    ///
    /// GNOME の作法どおり、項目はヘッダーバーの左側へ並ぶ。
    pub fn set_toolbar(&self, toolbar: &Toolbar) {
        self.clear_toolbar();
        *self.0.toolbar.borrow_mut() = Some(toolbar.clone());
        self.mount_header_start();
    }

    /// 取り付けたツールバーを外す。付いていなければ何もしない。
    pub fn clear_toolbar(&self) {
        if let Some(old) = self.0.toolbar.borrow_mut().take() {
            self.0.header.remove(&old.mount());
        }
    }

    /// ウィンドウの上端に付けるメニューバー。呼ぶたびに置き換わる。
    ///
    /// `GtkPopoverMenuBar` はヘッダーバーの下へ入る。項目の操作は
    /// ウィンドウの操作の組として入れるので、`GtkApplication` へ登録した
    /// アクセラレータ (ショートカット) はメニューを開かなくても効く。
    pub fn set_menu_bar(&self, menu_bar: &MenuBar) {
        self.clear_menu_bar();
        self.0.view.add_top_bar(&menu_bar.mount());
        self.0
            .native
            .insert_action_group(&menu_bar.group(), Some(&menu_bar.action_group()));
        menu_bar.attach();
        *self.0.menu_bar.borrow_mut() = Some(menu_bar.clone());
    }

    /// 取り付けたメニューバーを外す。付いていなければ何もしない。
    pub fn clear_menu_bar(&self) {
        let old = self.0.menu_bar.borrow_mut().take();
        if let Some(old) = old {
            self.0.view.remove(&old.mount());
            self.0
                .native
                .insert_action_group(&old.group(), None::<&gtk::gio::SimpleActionGroup>);
            // アプリ全体のアクセラレータの表からも外す。
            old.detach();
        }
    }

    pub fn show(&self) {
        self.0.native.present();
    }

    pub fn close(&self) {
        self.0.closing.set(true);
        self.0.native.close();
        self.0.closing.set(false);
    }

    pub fn is_visible(&self) -> bool {
        WidgetExt::is_visible(&self.0.native)
    }

    /// このウィンドウに配色テーマを適用する。
    ///
    /// libadwaita のテーマはアプリ全体で 1 つなので、実際にはアプリ全体に効く。
    pub fn set_theme(&self, theme: Theme) -> Result<()> {
        crate::apply_theme(theme);
        Ok(())
    }
}

/// `window` に載っている `AdwToastOverlay`。naui が作ったウィンドウでなければ
/// `None`。
///
/// [`Toast`](crate::Toast) は「いちばん手前のウィンドウ」へ出すので、
/// `GtkApplication` からたどったウィンドウを、naui が組んだ構造
/// (`AdwApplicationWindow` → `AdwToolbarView` → `AdwToastOverlay`) に沿って
/// 下りる。サイドバーを付けていれば、間に `GtkPaned` の右の区画が挟まる。
pub(crate) fn toast_overlay(window: &gtk::Window) -> Option<adw::ToastOverlay> {
    let window = window.clone().downcast::<adw::ApplicationWindow>().ok()?;
    let mut content = window.content()?;
    if let Some(paned) = content.downcast_ref::<gtk::Paned>() {
        content = paned.end_child()?;
    }
    let view = content.downcast::<adw::ToolbarView>().ok()?;
    view.content()?.downcast::<adw::ToastOverlay>().ok()
}

fn to_px(value: f64) -> i32 {
    value.round().clamp(1.0, i32::MAX as f64) as i32
}
