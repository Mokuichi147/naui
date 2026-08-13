//! WinUI 3 の Window ハンドル。

use std::cell::RefCell;
use std::rc::Rc;

use miui_core::Result;
use windows_core::HSTRING;
use winui3::Microsoft::UI::Xaml::Window as XamlWindow;

use crate::to_error;
use crate::widgets::Widget;

struct WindowInner {
    native: XamlWindow,
    child: RefCell<Option<Box<dyn Widget>>>,
    visible: RefCell<bool>,
}

/// トップレベルウィンドウ。
#[derive(Clone)]
pub struct Window(Rc<WindowInner>);

impl Window {
    pub(crate) fn new(title: &str, width: f64, height: f64) -> Result<Self> {
        let native = XamlWindow::new().map_err(|e| to_error("Window の生成", e))?;
        native
            .SetTitle(&HSTRING::from(title))
            .map_err(|e| to_error("Window のタイトル設定", e))?;

        let this = Self(Rc::new(WindowInner {
            native,
            child: RefCell::new(None),
            visible: RefCell::new(false),
        }));
        this.set_size(width, height);
        Ok(this)
    }

    pub fn set_title(&self, title: &str) {
        let _ = self.0.native.SetTitle(&HSTRING::from(title));
    }

    pub fn title(&self) -> String {
        self.0
            .native
            .Title()
            .map(|s| s.to_string())
            .unwrap_or_default()
    }

    pub fn set_size(&self, width: f64, height: f64) {
        use windows::Graphics::SizeInt32;
        if let Ok(app_window) = self.0.native.AppWindow() {
            let _ = app_window.Resize(SizeInt32 {
                Width: width as i32,
                Height: height as i32,
            });
        }
    }

    /// ルートに置くウィジェット。呼ぶたびに置き換わる。
    pub fn set_child(&self, child: &dyn Widget) {
        if self.0.native.SetContent(&child.native_element()).is_ok() {
            *self.0.child.borrow_mut() = Some(child.boxed_clone());
        }
    }

    /// 画面に出して前面へ持ってくる。
    pub fn show(&self) {
        if self.0.native.Activate().is_ok() {
            *self.0.visible.borrow_mut() = true;
        }
    }

    pub fn close(&self) {
        if self.0.native.Close().is_ok() {
            *self.0.visible.borrow_mut() = false;
        }
    }

    pub fn is_visible(&self) -> bool {
        self.0.native.Visible().unwrap_or(*self.0.visible.borrow())
    }

    /// WinUI 3 の実ウィンドウ。バックエンド固有の脱出口。
    pub fn native_window(&self) -> XamlWindow {
        self.0.native.clone()
    }
}
