//! WinUI 3 の Window ハンドル。

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use naui_core::{Result, Theme};
use windows::Foundation::TypedEventHandler;
use windows_core::{Interface, HSTRING};
use winui3::Microsoft::UI::Composition::ICompositionSupportsSystemBackdrop;
use winui3::Microsoft::UI::Composition::SystemBackdrops::{
    MicaController, SystemBackdropConfiguration, SystemBackdropTheme,
};
use winui3::Microsoft::UI::Xaml::Controls::TextBlock;
use winui3::Microsoft::UI::Xaml::Markup::XamlReader;
use winui3::Microsoft::UI::Xaml::Media::MicaBackdrop;
use winui3::Microsoft::UI::Xaml::{
    Application, ApplicationTheme, Controls::Grid, FrameworkElement, UIElement,
    Window as XamlWindow,
};

use crate::to_error;
use crate::toolbar::Toolbar;
use crate::ui_thread::UiThreadCell;
use crate::widgets::Widget;
use crate::Ui;

enum Backdrop {
    Controller {
        _controller: MicaController,
        configuration: SystemBackdropConfiguration,
    },
    BuiltIn {
        _mica: MicaBackdrop,
    },
}

struct WindowInner {
    native: XamlWindow,
    backdrop: Backdrop,
    child: RefCell<Option<Box<dyn Widget>>>,
    theme_root: RefCell<Option<UIElement>>,
    title_label: RefCell<Option<TextBlock>>,
    /// タイトルバーと中身の間にあるツールバーの入れ物。
    toolbar_host: RefCell<Option<Grid>>,
    /// 取り付けたツールバー。通知先ごと生かしておく。
    toolbar: RefCell<Option<Toolbar>>,
    visible: RefCell<bool>,
    theme: Cell<Theme>,
    width: i32,
    height: i32,
    wheel_subclass_installed: Cell<bool>,
    closing_token: Cell<Option<i64>>,
}

/// トップレベルウィンドウ。
#[derive(Clone)]
pub struct Window(Rc<WindowInner>);

/// `Window` を強く保持せずにイベントハンドラから参照するための弱参照。
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

    pub(crate) fn new(title: &str, width: f64, height: f64, theme: Theme) -> Result<Self> {
        let native = XamlWindow::new().map_err(|e| to_error("Window の生成", e))?;
        native
            .SetTitle(&HSTRING::from(title))
            .map_err(|e| to_error("Window のタイトル設定", e))?;
        let backdrop = create_backdrop(&native, theme)?;

        let this = Self(Rc::new(WindowInner {
            native,
            backdrop,
            child: RefCell::new(None),
            theme_root: RefCell::new(None),
            toolbar_host: RefCell::new(None),
            toolbar: RefCell::new(None),
            title_label: RefCell::new(None),
            visible: RefCell::new(false),
            theme: Cell::new(theme),
            width: width as i32,
            height: height as i32,
            wheel_subclass_installed: Cell::new(false),
            closing_token: Cell::new(None),
        }));
        Ok(this)
    }

    pub fn set_title(&self, title: &str) {
        let _ = self.0.native.SetTitle(&HSTRING::from(title));
        if let Some(label) = self.0.title_label.borrow().as_ref() {
            let _ = label.SetText(&HSTRING::from(title));
        }
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
        let themed = themed_content_root(&element, &self.title());
        let (theme_root, title_bar, title_label, toolbar_host) = match themed {
            Ok(content) => (
                content.root,
                Some(content.title_bar),
                Some(content.title_label),
                Some(content.toolbar_host),
            ),
            Err(error) => {
                eprintln!("naui-windows: テーマ付きウィンドウルートの生成に失敗: {error}");
                (element.clone(), None, None, None)
            }
        };
        if self.0.native.SetContent(&theme_root).is_ok() {
            if let Some(title_bar) = title_bar {
                // Mica をタイトルバーまで連続させ、タイトルバー要素全体を
                // ウィンドウのドラッグ領域として扱う。
                let _ = self.0.native.SetExtendsContentIntoTitleBar(true);
                let _ = self.0.native.SetTitleBar(&title_bar);
            }
            let _ = set_theme_on_element(&theme_root, self.0.theme.get());
            if let Some(label) = title_label.as_ref() {
                let _ = set_title_foreground(label, &theme_root, self.0.theme.get());
            }
            *self.0.theme_root.borrow_mut() = Some(theme_root);
            *self.0.title_label.borrow_mut() = title_label;
            *self.0.toolbar_host.borrow_mut() = toolbar_host;
            *self.0.child.borrow_mut() = Some(child.boxed_clone());
            // ルートを作り直したので、取り付けてあったツールバーを載せ直す。
            self.mount_toolbar();
            if !self.0.wheel_subclass_installed.get() {
                self.0
                    .wheel_subclass_installed
                    .set(crate::layout::install_wheel_subclass(&self.0.native));
            }
        }
    }

    /// ウィンドウの上端に付けるツールバー。呼ぶたびに置き換わる。
    ///
    /// WinUI 3 の `CommandBar` は `winio-winui3` のバインディングに無いため、
    /// タイトルバーと中身の間の行へ `StackPanel` を置いて構成する。
    /// タイトルバーはウィンドウのドラッグ領域なので、そこには置けない。
    pub fn set_toolbar(&self, toolbar: &Toolbar) {
        self.clear_toolbar();
        *self.0.toolbar.borrow_mut() = Some(toolbar.clone());
        self.mount_toolbar();
    }

    /// 取り付けたツールバーを外す。付いていなければ何もしない。
    pub fn clear_toolbar(&self) {
        self.0.toolbar.borrow_mut().take();
        if let Some(host) = self.0.toolbar_host.borrow().as_ref() {
            let _ = host.Children().and_then(|children| children.Clear());
        }
    }

    /// ツールバーを置き場へ載せ直す。置き場か中身がまだ無ければ何もしない。
    fn mount_toolbar(&self) {
        let host = self.0.toolbar_host.borrow();
        let toolbar = self.0.toolbar.borrow();
        let (Some(host), Some(toolbar)) = (host.as_ref(), toolbar.as_ref()) else {
            return;
        };
        let Ok(children) = host.Children() else {
            return;
        };
        let _ = children.Clear();
        let _ = children.Append(&toolbar.mount());
    }

    /// このウィンドウの配色テーマを切り替える。
    pub fn set_theme(&self, theme: Theme) -> Result<()> {
        self.0.theme.set(theme);
        let theme_root = self.0.theme_root.borrow();
        if let Some(theme_root) = theme_root.as_ref() {
            set_theme_on_element(theme_root, theme)?;
            if let Some(label) = self.0.title_label.borrow().as_ref() {
                set_title_foreground(label, theme_root, theme)?;
            }
        } else {
            let child = self.0.child.borrow();
            if let Some(child) = child.as_deref() {
                set_theme_on_element(&child.native_element(), theme)?;
            }
        }
        if let Backdrop::Controller { configuration, .. } = &self.0.backdrop {
            configuration
                .SetTheme(backdrop_theme(theme))
                .map_err(|e| to_error("Micaテーマの設定", e))?;
        }
        Ok(())
    }

    /// 画面に出して前面へ持ってくる。
    pub fn show(&self) {
        self.set_size(self.0.width as f64, self.0.height as f64);
        if self.0.native.Activate().is_ok() {
            *self.0.visible.borrow_mut() = true;
            remember_owner(&self.0.native);
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

    pub(crate) fn clear_content_for_shutdown(&self) {
        let _ = self.0.native.SetContent(None);
        *self.0.child.borrow_mut() = None;
        *self.0.theme_root.borrow_mut() = None;
        *self.0.title_label.borrow_mut() = None;
    }

    /// WinUI の XAML ツリーが破棄される前に、Content と子ウィジェットを外す。
    /// `Destroying` では遅すぎるため、AppWindow の `Closing` で実行する。
    pub(crate) fn install_closing_handler(&self, state: &'static UiThreadCell<Option<Ui>>) {
        if self.0.closing_token.get().is_some() {
            return;
        }
        let Ok(app_window) = self.0.native.AppWindow() else {
            return;
        };
        let handler = TypedEventHandler::<
            winui3::Microsoft::UI::Windowing::AppWindow,
            winui3::Microsoft::UI::Windowing::AppWindowClosingEventArgs,
        >::new(move |_sender, _args| {
            state.with_mut(|slot| {
                if let Some(ui) = slot.take() {
                    // 画面が畳まれた後は、投函しても誰も取り出さない。
                    // 送信側へ失敗を返せるようにし、受信クロージャと future を解放する。
                    ui.tasks.shutdown();
                    ui.clear_windows_for_shutdown();
                }
            });
            Ok(())
        });
        if let Ok(token) = app_window.Closing(&handler) {
            self.0.closing_token.set(Some(token));
        }
    }
}

struct ThemedContent {
    root: UIElement,
    title_bar: UIElement,
    title_label: TextBlock,
    /// タイトルバーと中身の間に置くツールバーの入れ物。
    toolbar_host: Grid,
}

fn backdrop_theme(theme: Theme) -> SystemBackdropTheme {
    match theme {
        Theme::System => SystemBackdropTheme::Default,
        Theme::Light => SystemBackdropTheme::Light,
        Theme::Dark => SystemBackdropTheme::Dark,
    }
}

fn create_backdrop(native: &XamlWindow, theme: Theme) -> Result<Backdrop> {
    if MicaController::IsSupported().unwrap_or(false) {
        if let Ok(target) = native.cast::<ICompositionSupportsSystemBackdrop>() {
            if let Ok(controller) = MicaController::new() {
                if let Ok(configuration) = SystemBackdropConfiguration::new() {
                    let configured = configuration.SetIsInputActive(true).is_ok()
                        && configuration.SetTheme(backdrop_theme(theme)).is_ok()
                        && controller.AddSystemBackdropTarget(&target).unwrap_or(false)
                        && controller
                            .SetSystemBackdropConfiguration(&configuration)
                            .is_ok();
                    if configured {
                        return Ok(Backdrop::Controller {
                            _controller: controller,
                            configuration,
                        });
                    }
                }
            }
        }
    }

    let mica = MicaBackdrop::new().map_err(|e| to_error("Mica の生成", e))?;
    native
        .SetSystemBackdrop(&mica)
        .map_err(|e| to_error("Mica の適用", e))?;
    Ok(Backdrop::BuiltIn { _mica: mica })
}

fn set_title_foreground(label: &TextBlock, root: &UIElement, requested: Theme) -> Result<()> {
    let theme = effective_theme(root, requested);
    let color = match theme {
        Theme::Light => "#FF1A1A1A",
        Theme::Dark | Theme::System => "#FFFFFFFF",
    };
    let brush = XamlReader::Load(&HSTRING::from(format!(
        r##"<TextBlock xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
            Foreground="{color}"/>"##
    )))
    .map_err(|e| to_error("タイトル文字色ブラシの生成", e))?
    .cast::<TextBlock>()
    .map_err(|e| to_error("タイトル文字色要素への変換", e))?
    .Foreground()
    .map_err(|e| to_error("タイトル文字色ブラシの取得", e))?;
    label
        .SetForeground(&brush)
        .map_err(|e| to_error("タイトル文字色の設定", e))
}

fn effective_theme(root: &UIElement, requested: Theme) -> Theme {
    if requested != Theme::System {
        return requested;
    }
    Application::Current()
        .ok()
        .and_then(|app| app.RequestedTheme().ok())
        .map(|theme| {
            if theme == ApplicationTheme::Light {
                Theme::Light
            } else {
                Theme::Dark
            }
        })
        .or_else(|| actual_theme(root).ok())
        .unwrap_or(Theme::Dark)
}

fn themed_content_root(element: &UIElement, title: &str) -> Result<ThemedContent> {
    let root = XamlReader::Load(&HSTRING::from(
        r##"<Grid xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
            Background="Transparent">
            <Grid.RowDefinitions>
                <RowDefinition Height="48"/>
                <RowDefinition Height="Auto"/>
                <RowDefinition Height="*"/>
            </Grid.RowDefinitions>
            <Grid Grid.Row="0" Height="48" Background="Transparent"
                Padding="16,0,140,0">
                <TextBlock FontSize="14" VerticalAlignment="Center"/>
            </Grid>
            <Grid Grid.Row="1" Background="Transparent" Padding="12,0,12,8"/>
            <Grid Grid.Row="2" Background="Transparent"/>
        </Grid>"##,
    ))
    .map_err(|e| to_error("テーマ背景要素の生成", e))?
    .cast::<Grid>()
    .map_err(|e| to_error("テーマ背景要素への変換", e))?;
    let children = root
        .Children()
        .map_err(|e| to_error("テーマ背景要素の子取得", e))?;
    let title_bar = children
        .GetAt(0)
        .map_err(|e| to_error("タイトルバーの取得", e))?
        .cast::<Grid>()
        .map_err(|e| to_error("タイトルバーへの変換", e))?;
    let title_label = title_bar
        .Children()
        .map_err(|e| to_error("タイトルラベルの子取得", e))?
        .GetAt(0)
        .map_err(|e| to_error("タイトルラベルの取得", e))?
        .cast::<TextBlock>()
        .map_err(|e| to_error("タイトルラベルへの変換", e))?;
    title_label
        .SetText(&HSTRING::from(title))
        .map_err(|e| to_error("タイトルラベルの設定", e))?;

    let toolbar_host = children
        .GetAt(1)
        .map_err(|e| to_error("ツールバー置き場の取得", e))?
        .cast::<Grid>()
        .map_err(|e| to_error("ツールバー置き場への変換", e))?;
    let content = children
        .GetAt(2)
        .map_err(|e| to_error("コンテンツレイヤーの取得", e))?
        .cast::<Grid>()
        .map_err(|e| to_error("コンテンツレイヤーへの変換", e))?;
    content
        .Children()
        .map_err(|e| to_error("テーマ背景要素の子取得", e))?
        .Append(element)
        .map_err(|e| to_error("テーマ背景要素への配置", e))?;
    Ok(ThemedContent {
        root: root
            .cast::<UIElement>()
            .map_err(|e| to_error("テーマ背景要素への変換", e))?,
        title_bar: title_bar
            .cast::<UIElement>()
            .map_err(|e| to_error("タイトルバーへの変換", e))?,
        title_label,
        toolbar_host,
    })
}

/// `winio-winui3` 0.4.x は `FrameworkElement.RequestedTheme` の型を公開していない。
/// ただし生成された vtable には ABI スロットが存在する。`IsLoaded` の直前は
/// `SetRequestedTheme` なので、そのスロットを使って同じ WinRT プロパティを呼び出す。
#[repr(transparent)]
#[derive(Clone, Copy)]
struct ElementTheme(i32);

thread_local! {
    /// 最後に表示したウィンドウの HWND。モーダルダイアログの親に使う。
    ///
    /// XAML の要素から自分の載っているウィンドウをたどる API が
    /// `winio-winui3` のバインディングに無いため、表示のたびに覚えておく。
    static OWNER_HWND: Cell<isize> = const { Cell::new(0) };
    /// 最後に表示したウィンドウ。`ContentDialog` を出す `XamlRoot` に使う。
    static OWNER_WINDOW: RefCell<Option<XamlWindow>> = const { RefCell::new(None) };
}

/// モーダルダイアログの親にするウィンドウを覚える。
fn remember_owner(window: &XamlWindow) {
    OWNER_WINDOW.with(|slot| *slot.borrow_mut() = Some(window.clone()));
    let Ok(native) = window.cast::<winui3::IWindowNative>() else {
        return;
    };
    if let Ok(hwnd) = unsafe { native.WindowHandle() } {
        OWNER_HWND.with(|slot| slot.set(hwnd.0 as isize));
    }
}

/// `ContentDialog` を出す土台。まだウィンドウを表示していなければ `None`。
///
/// `XamlRoot` はウィンドウが表示されてから決まるので、覚えたウィンドウから
/// 呼ばれるたびに取り直す。
pub(crate) fn owner_xaml_root() -> Option<winui3::Microsoft::UI::Xaml::XamlRoot> {
    OWNER_WINDOW.with(|slot| {
        let window = slot.borrow();
        let content = window.as_ref()?.Content().ok()?;
        content.XamlRoot().ok()
    })
}

/// トーストを重ねる層。まだウィンドウを表示していなければ `None`。
///
/// [`themed_content_root`] が作る 3 行目 (アプリの中身の置き場) で、
/// `Grid` は子を重ね順に置くため、あとから足したトーストが中身の上に出る。
pub(crate) fn owner_content_layer() -> Option<Grid> {
    OWNER_WINDOW.with(|slot| {
        let window = slot.borrow();
        let root = window.as_ref()?.Content().ok()?.cast::<Grid>().ok()?;
        root.Children().ok()?.GetAt(2).ok()?.cast::<Grid>().ok()
    })
}

/// モーダルダイアログの親にするウィンドウ。まだ何も表示していなければ `None`。
pub(crate) fn owner_hwnd() -> Option<windows::Win32::Foundation::HWND> {
    let raw = OWNER_HWND.with(|slot| slot.get());
    if raw == 0 {
        return None;
    }
    Some(windows::Win32::Foundation::HWND(
        raw as *mut std::ffi::c_void,
    ))
}

impl From<Theme> for ElementTheme {
    fn from(theme: Theme) -> Self {
        Self(match theme {
            Theme::System => 0,
            Theme::Light => 1,
            Theme::Dark => 2,
        })
    }
}

pub(crate) fn set_theme_on_element(element: &UIElement, theme: Theme) -> Result<()> {
    let element = element
        .cast::<FrameworkElement>()
        .map_err(|e| to_error("テーマ要素への変換", e))?;
    let set_requested_theme: unsafe extern "system" fn(
        *mut std::ffi::c_void,
        ElementTheme,
    ) -> windows_core::HRESULT = unsafe {
        let is_loaded = std::ptr::addr_of!(element.vtable().IsLoaded) as *const usize;
        std::mem::transmute(*is_loaded.sub(1))
    };
    unsafe { set_requested_theme(Interface::as_raw(&element), theme.into()).ok() }
        .map_err(|e| to_error("テーマの設定", e))
}

fn actual_theme(element: &UIElement) -> Result<Theme> {
    let element = element
        .cast::<FrameworkElement>()
        .map_err(|e| to_error("実テーマ要素への変換", e))?;
    let is_loaded = std::ptr::addr_of!(element.vtable().IsLoaded) as *const usize;
    let get_actual_theme: unsafe extern "system" fn(
        *mut std::ffi::c_void,
        *mut ElementTheme,
    ) -> windows_core::HRESULT = unsafe { std::mem::transmute(*is_loaded.add(1)) };
    let mut actual = ElementTheme(0);
    unsafe { get_actual_theme(Interface::as_raw(&element), &mut actual).ok() }
        .map_err(|e| to_error("実テーマの取得", e))?;
    Ok(match actual.0 {
        1 => Theme::Light,
        2 => Theme::Dark,
        _ => Theme::Dark,
    })
}
