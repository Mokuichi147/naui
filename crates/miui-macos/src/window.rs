//! NSWindow のハンドル。

use std::cell::RefCell;
use std::rc::Rc;

use objc2::rc::Retained;
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSBackingStoreType, NSWindow, NSWindowStyleMask};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};

use crate::widgets::Widget;

struct WindowInner {
    native: Retained<NSWindow>,
    /// ルートの子を保持し、トランポリンごと生かしておく。
    child: RefCell<Option<Box<dyn Widget>>>,
}

/// トップレベルウィンドウ (NSWindow)。
#[derive(Clone)]
pub struct Window(Rc<WindowInner>);

impl Window {
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
        }))
    }

    pub fn set_title(&self, title: &str) {
        self.0.native.setTitle(&NSString::from_str(title));
    }

    pub fn title(&self) -> String {
        self.0.native.title().to_string()
    }

    pub fn set_size(&self, width: f64, height: f64) {
        self.0
                .native
                .setContentSize(NSSize::new(width, height));
        self.0.native.center();
    }

    /// ルートに置くウィジェット。呼ぶたびに置き換わる。
    pub fn set_child(&self, child: &dyn Widget) {
        let view = child.native_view();
        self.0.native.setContentView(Some(&view));
        *self.0.child.borrow_mut() = Some(child.boxed_clone());
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

    /// AppKit の実ウィンドウ。バックエンド固有の脱出口。
    pub fn native_window(&self) -> Retained<NSWindow> {
        self.0.native.clone()
    }
}
