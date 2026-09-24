//! NSWindow のハンドル。

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use naui_core::{Result, Theme};
use objc2::rc::Retained;
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSAppearance, NSAppearanceCustomization, NSAppearanceNameAqua, NSAppearanceNameDarkAqua,
    NSApplication, NSBackingStoreType, NSSplitViewController, NSView, NSWindow, NSWindowStyleMask,
    NSWindowTitleVisibility,
};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};

use crate::menu_bar::MenuBar;
use crate::sidebar::Sidebar;
use crate::toolbar::Toolbar;
use crate::widgets::Widget;

thread_local! {
    /// naui が作ったウィンドウ。作った順に並ぶ。
    ///
    /// `NSApplication` の `windows` には AppKit が内部で作る、画面に出ない
    /// ウィンドウも混ざる。[`Toast`](crate::Toast) の出し先を選ぶときに
    /// そちらを掴まないよう、naui のぶんだけ覚えておく。
    /// `Ui` がウィンドウを持ち続けるので、ここで保持しても寿命は変わらない。
    static WINDOWS: RefCell<Vec<Retained<NSWindow>>> = const { RefCell::new(Vec::new()) };
}

struct WindowInner {
    native: Retained<NSWindow>,
    /// ルートの子を保持し、トランポリンごと生かしておく。
    child: RefCell<Option<Box<dyn Widget>>>,
    /// 取り付けたツールバー。`NSWindow` の toolbar は強参照だが、
    /// naui 側のハンドル (トランポリンと通知先) もここで生かしておく。
    toolbar: RefCell<Option<Toolbar>>,
    /// 取り付けたメニューバー。`NSApplication` が `mainMenu` を強参照するが、
    /// naui 側のハンドル (トランポリンと通知先) もここで生かしておく。
    menu_bar: RefCell<Option<MenuBar>>,
    /// 取り付けたサイドバー。`contentViewController` として強参照されるが、
    /// naui 側のハンドル (データソースと通知先) もここで生かしておく。
    sidebar: RefCell<Option<Sidebar>>,
}

/// トップレベルウィンドウ (NSWindow)。
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

    pub(crate) fn new(mtm: MainThreadMarker, title: &str, width: f64, height: f64) -> Self {
        let native = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(width, height)),
                NSWindowStyleMask::Titled
                    | NSWindowStyleMask::Closable
                    | NSWindowStyleMask::Miniaturizable
                    | NSWindowStyleMask::Resizable,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        native.setTitle(&NSString::from_str(title));
        // Rust 側が Retained を持つので、閉じたときの自動解放は切る
        // (切らないと二重解放になる)。
        unsafe { native.setReleasedWhenClosed(false) };
        native.center();

        WINDOWS.with(|slot| slot.borrow_mut().push(native.clone()));

        Self(Rc::new(WindowInner {
            native,
            child: RefCell::new(None),
            toolbar: RefCell::new(None),
            menu_bar: RefCell::new(None),
            sidebar: RefCell::new(None),
        }))
    }

    pub fn set_title(&self, title: &str) {
        self.0.native.setTitle(&NSString::from_str(title));
    }

    pub fn title(&self) -> String {
        self.0.native.title().to_string()
    }

    pub fn set_size(&self, width: f64, height: f64) {
        self.0.native.setContentSize(NSSize::new(width, height));
        self.0.native.center();
    }

    /// ルートに置くウィジェット。呼ぶたびに置き換わる。
    ///
    /// サイドバーを付けているときは、その右の区画に置かれる。
    pub fn set_child(&self, child: &dyn Widget) {
        let view = child.native_view();
        let sidebar = self.0.sidebar.borrow().clone();
        match sidebar {
            Some(sidebar) => sidebar.set_content(Some(view)),
            None => self.0.native.setContentView(Some(&view)),
        }
        *self.0.child.borrow_mut() = Some(child.boxed_clone());
    }

    /// ウィンドウの左に付けるサイドバー。呼ぶたびに置き換わる。
    ///
    /// ウィンドウの `contentViewController` を `NSSplitViewController` へ
    /// 差し替え、[`set_child`](Self::set_child) の子はその右の区画へ移す。
    /// サイドバーをタイトルバーの下まで伸ばすため (「システム設定」と同じ形)、
    /// 付けている間はウィンドウに `fullSizeContentView` を足す。子の上端は
    /// タイトルバーを避けるので、中身の見え方は変わらない。
    pub fn set_sidebar(&self, sidebar: &Sidebar) {
        self.clear_sidebar();
        let native = &self.0.native;
        // `contentViewController` を差し替えると、ウィンドウがその大きさへ
        // 合わせて縮むことがある。置く前の枠へ戻す。
        let frame = native.frame();
        let child = self
            .0
            .child
            .borrow()
            .as_ref()
            .map(|child| child.native_view());
        if let Some(view) = &child {
            view.removeFromSuperview();
        }
        native.setStyleMask(native.styleMask() | NSWindowStyleMask::FullSizeContentView);
        native.setContentViewController(Some(&sidebar.native_split_view_controller()));
        native.setFrame_display(frame, true);
        sidebar.set_content(child);
        sidebar.apply_width();
        *self.0.sidebar.borrow_mut() = Some(sidebar.clone());
        self.apply_toolbar();
    }

    /// 取り付けたサイドバーを外す。付いていなければ何もしない。
    ///
    /// 右の区画にあった子は、ウィンドウの中身へ戻る。
    pub fn clear_sidebar(&self) {
        let Some(sidebar) = self.0.sidebar.borrow_mut().take() else {
            return;
        };
        let native = &self.0.native;
        let frame = native.frame();
        let child = sidebar.take_content();
        native.setContentViewController(None);
        native.setStyleMask(native.styleMask() & !NSWindowStyleMask::FullSizeContentView);
        let view = child.unwrap_or_else(|| NSView::new(MainThreadMarker::from(&**native)));
        // 右の区画では制約で置いていた。ウィンドウの中身は枠で置かれる。
        view.setTranslatesAutoresizingMaskIntoConstraints(true);
        native.setContentView(Some(&view));
        // ボタンだけのツールバーを外すとウィンドウの高さが変わるので、
        // ツールバーを決め直してから元の枠へ戻す。
        self.apply_toolbar();
        native.setFrame_display(frame, true);
    }

    /// ウィンドウの上端に付けるツールバー。呼ぶたびに置き換わる。
    ///
    /// AppKit ではタイトルバーと一体で表示され、項目が入りきらないときは
    /// AppKit が送り出しのメニューを出す。
    ///
    /// **タイトル文字は隠れる。** ツールバーのあるウィンドウでタイトルを
    /// 出さないのが macOS の作法で、出したままだとタイトルが先頭を占め、
    /// 項目が右端へ押しやられてしまう。[`set_title`](Self::set_title) で
    /// 設定した文字はウィンドウのタイトルとして残り (ウィンドウメニューや
    /// Mission Control には出る)、[`title`](Self::title) も返し続ける。
    ///
    /// サイドバーを付けているときは、先頭に AppKit 標準のサイドバーボタンが
    /// 入る (naui の項目のインデックスには数えない)。
    pub fn set_toolbar(&self, toolbar: &Toolbar) {
        let old = self.0.toolbar.borrow_mut().replace(toolbar.clone());
        self.apply_toolbar();
        // 前のツールバーは、ウィンドウから外れてから元の並びへ戻す。
        if let Some(old) = old.filter(|old| !old.is_same(toolbar)) {
            old.set_sidebar_controls(false);
        }
    }

    /// 取り付けたツールバーを外す。付いていなければ何もしない。
    ///
    /// 隠していたタイトル文字も出し直す。
    ///
    /// サイドバーを付けているときは、サイドバーボタンだけのツールバーが残る。
    pub fn clear_toolbar(&self) {
        let old = self.0.toolbar.borrow_mut().take();
        self.apply_toolbar();
        if let Some(old) = old {
            old.set_sidebar_controls(false);
        }
    }

    /// ツールバーとサイドバーの組み合わせから、ウィンドウのツールバーを決める。
    ///
    /// | アプリのツールバー | サイドバー | ウィンドウに付くもの |
    /// | --- | --- | --- |
    /// | あり | あり | アプリのツールバー (先頭にサイドバーボタン) |
    /// | あり | なし | アプリのツールバー |
    /// | なし | あり | サイドバーボタンだけのツールバー (タイトルは出したまま) |
    /// | なし | なし | 無し |
    ///
    /// サイドバー用の項目を外すのは、ツールバーをウィンドウから外している
    /// 間に行い、入れるのはウィンドウに付けてから行う
    /// (`NSToolbarSidebarTrackingSeparatorItemIdentifier` は、付いていない
    /// ツールバーには入らない)。
    fn apply_toolbar(&self) {
        let toolbar = self.0.toolbar.borrow().clone();
        let sidebar = self.0.sidebar.borrow().clone();
        self.0.native.setToolbar(None);
        if let Some(toolbar) = &toolbar {
            toolbar.set_sidebar_controls(sidebar.is_some());
        }
        let (chosen, visibility) = match (&toolbar, &sidebar) {
            (Some(toolbar), _) => (Some(toolbar.clone()), NSWindowTitleVisibility::Hidden),
            (None, Some(sidebar)) => (
                Some(sidebar.controls_toolbar()),
                NSWindowTitleVisibility::Visible,
            ),
            (None, None) => (None, NSWindowTitleVisibility::Visible),
        };
        self.0
            .native
            .setToolbar(chosen.as_ref().map(|t| t.native_toolbar()).as_deref());
        self.0.native.setTitleVisibility(visibility);
        // ウィンドウに付いてからでないとサイドバー用の区切りが入らないので、
        // 付けたあとでもう一度そろえる。
        if let Some(chosen) = &chosen {
            chosen.set_sidebar_controls(sidebar.is_some());
        }
    }

    /// 画面上端に出すメニューバー。呼ぶたびに置き換わる。
    ///
    /// **macOS のメニューバーはアプリに 1 つ**なので、
    /// `NSApplication.mainMenu` を差し替える。どのウィンドウから呼んでも
    /// アプリ全体に効き、ウィンドウが前面かどうかでは変わらない
    /// (ほかの 3 環境に合わせてウィンドウの API にしてある)。
    pub fn set_menu_bar(&self, menu_bar: &MenuBar) {
        menu_bar.install();
        *self.0.menu_bar.borrow_mut() = Some(menu_bar.clone());
    }

    /// 取り付けたメニューバーを外す。付いていなければ何もしない。
    ///
    /// メニューが 1 つも無いと ⌘C / ⌘V が配送されなくなるため、naui の
    /// 既定のメニュー (アプリメニューと編集メニュー) へ戻す。
    pub fn clear_menu_bar(&self) {
        if let Some(old) = self.0.menu_bar.borrow_mut().take() {
            old.uninstall();
        }
    }

    /// 画面に出して前面へ持ってくる。
    pub fn show(&self) {
        self.0.native.makeKeyAndOrderFront(None);
    }

    pub fn close(&self) {
        self.0.native.close();
    }

    pub fn is_visible(&self) -> bool {
        self.0.native.isVisible()
    }

    /// このウィンドウの配色テーマを切り替える。
    pub fn set_theme(&self, theme: Theme) -> Result<()> {
        let appearance = match theme {
            Theme::System => None,
            Theme::Light => unsafe { NSAppearance::appearanceNamed(NSAppearanceNameAqua) },
            Theme::Dark => unsafe { NSAppearance::appearanceNamed(NSAppearanceNameDarkAqua) },
        };
        self.0.native.setAppearance(appearance.as_deref());
        Ok(())
    }

    /// AppKit の実ウィンドウ。バックエンド固有の脱出口。
    pub fn native_window(&self) -> Retained<NSWindow> {
        self.0.native.clone()
    }
}

/// ウィンドウの中身へ重ねるビュー (トーストの載せ先)。
///
/// naui のサイドバーを付けたウィンドウでは、`contentViewController` が
/// `NSSplitViewController` になっているので、その最後の区画 (中身の側) を
/// 返す。`contentView` そのもの (`NSSplitView`) へ足すと、区画の 1 つとして
/// 並べられてしまう。
pub(crate) fn overlay_host(window: &NSWindow) -> Option<Retained<NSView>> {
    let mtm = MainThreadMarker::from(window);
    let split = window
        .contentViewController()
        .and_then(|controller| controller.downcast::<NSSplitViewController>().ok());
    match split {
        Some(split) => split
            .splitViewItems()
            .lastObject()
            .map(|item| item.viewController(mtm).view()),
        None => window.contentView(),
    }
}

/// naui が作ったウィンドウのうち、いちばん手前のもの。1 つも無ければ `None`。
///
/// 焦点のあるウィンドウを使い、まだどれにも当たっていないとき (起動直後や
/// 自動テスト) は**最後に作ったもの**にする。`Dialog` が親を選ぶのと
/// 同じ考え方。
pub(crate) fn frontmost(mtm: MainThreadMarker) -> Option<Retained<NSWindow>> {
    let app = NSApplication::sharedApplication(mtm);
    WINDOWS.with(|slot| {
        let windows = slot.borrow();
        let ours = |window: &NSWindow| {
            windows
                .iter()
                .any(|ours| std::ptr::eq(&**ours as *const NSWindow, window as *const NSWindow))
        };
        app.keyWindow()
            .filter(|window| ours(window))
            .or_else(|| app.mainWindow().filter(|window| ours(window)))
            .or_else(|| windows.last().cloned())
    })
}
