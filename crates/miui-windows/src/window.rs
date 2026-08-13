//! WinUI 3 の Window ハンドル。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use miui_core::{Result, Theme};
use windows_core::{Interface, HSTRING};
use winui3::Microsoft::UI::Xaml::{FrameworkElement, UIElement, Window as XamlWindow};

use crate::to_error;
use crate::widgets::Widget;

struct WindowInner {
    native: XamlWindow,
    child: RefCell<Option<Box<dyn Widget>>>,
    visible: RefCell<bool>,
    theme: Cell<Theme>,
    width: i32,
    height: i32,
}

/// トップレベルウィンドウ。
#[derive(Clone)]
pub struct Window(Rc<WindowInner>);

impl Window {
    pub(crate) fn new(title: &str, width: f64, height: f64, theme: Theme) -> Result<Self> {
        let native = XamlWindow::new().map_err(|e| to_error("Window の生成", e))?;
        native
            .SetTitle(&HSTRING::from(title))
            .map_err(|e| to_error("Window のタイトル設定", e))?;

        let this = Self(Rc::new(WindowInner {
            native,
            child: RefCell::new(None),
            visible: RefCell::new(false),
            theme: Cell::new(theme),
            width: width as i32,
            height: height as i32,
        }));
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
        let element = child.native_element();
        if self.0.native.SetContent(&element).is_ok() {
            let _ = set_theme_on_element(&element, self.0.theme.get());
            *self.0.child.borrow_mut() = Some(child.boxed_clone());
        }
    }

    /// このウィンドウの配色テーマを切り替える。
    pub fn set_theme(&self, theme: Theme) -> Result<()> {
        self.0.theme.set(theme);
        let child = self.0.child.borrow();
        if let Some(child) = child.as_deref() {
            set_theme_on_element(&child.native_element(), theme)?;
        }
        Ok(())
    }

    /// 画面に出して前面へ持ってくる。
    pub fn show(&self) {
        self.set_size(self.0.width as f64, self.0.height as f64);
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

/// `winio-winui3` 0.4.x は `FrameworkElement.RequestedTheme` の型を公開していない。
/// ただし生成された vtable には ABI スロットが存在するため、公開されている
/// `IsLoaded` の 4 スロット前を使って同じ WinRT プロパティを呼び出す。
#[repr(transparent)]
#[derive(Clone, Copy)]
struct ElementTheme(i32);

impl From<Theme> for ElementTheme {
    fn from(theme: Theme) -> Self {
        Self(match theme {
            Theme::System => 0,
            Theme::Light => 1,
            Theme::Dark => 2,
        })
    }
}

fn set_theme_on_element(element: &UIElement, theme: Theme) -> Result<()> {
    let element = element
        .cast::<FrameworkElement>()
        .map_err(|e| to_error("テーマ要素への変換", e))?;
    let set_requested_theme: unsafe extern "system" fn(
        *mut std::ffi::c_void,
        ElementTheme,
    ) -> windows_core::HRESULT = unsafe {
        let is_loaded = std::ptr::addr_of!(element.vtable().IsLoaded) as *const usize;
        std::mem::transmute(*is_loaded.sub(4))
    };
    unsafe { set_requested_theme(Interface::as_raw(&element), theme.into()).ok() }
        .map_err(|e| to_error("テーマの設定", e))
}
