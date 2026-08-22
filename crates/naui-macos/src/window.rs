//! NSWindow のハンドル。

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use naui_core::{Result, Theme};
use objc2::rc::Retained;
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSAppearance, NSAppearanceCustomization, NSAppearanceNameAqua, NSAppearanceNameDarkAqua,
    NSBackingStoreType, NSWindow, NSWindowStyleMask, NSWindowTitleVisibility,
};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};

use crate::toolbar::Toolbar;
use crate::widgets::Widget;

struct WindowInner {
    native: Retained<NSWindow>,
    /// ルートの子を保持し、トランポリンごと生かしておく。
    child: RefCell<Option<Box<dyn Widget>>>,
    /// 取り付けたツールバー。`NSWindow` の toolbar は強参照だが、
    /// naui 側のハンドル (トランポリンと通知先) もここで生かしておく。
    toolbar: RefCell<Option<Toolbar>>,
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

        Self(Rc::new(WindowInner {
            native,
            child: RefCell::new(None),
            toolbar: RefCell::new(None),
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
    pub fn set_child(&self, child: &dyn Widget) {
        let view = child.native_view();
        self.0.native.setContentView(Some(&view));
        *self.0.child.borrow_mut() = Some(child.boxed_clone());
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
    pub fn set_toolbar(&self, toolbar: &Toolbar) {
        self.0.native.setToolbar(Some(&toolbar.native_toolbar()));
        self.0
            .native
            .setTitleVisibility(NSWindowTitleVisibility::Hidden);
        *self.0.toolbar.borrow_mut() = Some(toolbar.clone());
    }

    /// 取り付けたツールバーを外す。付いていなければ何もしない。
    ///
    /// 隠していたタイトル文字も出し直す。
    pub fn clear_toolbar(&self) {
        self.0.native.setToolbar(None);
        self.0
            .native
            .setTitleVisibility(NSWindowTitleVisibility::Visible);
        *self.0.toolbar.borrow_mut() = None;
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
