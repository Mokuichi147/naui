//! トースト (WinUI 3)。
//!
//! WinUI 3 の `InfoBar` と `TeachingTip` は `winio-winui3` 0.4.5 の
//! バインディングに無く、`Windows.UI.Notifications` のトーストは**アプリの
//! 外** (通知センター) へ出るもので別物なので、`Grid` + `StackPanel` +
//! `TextBlock` + `Button` を組み立て、ウィンドウの中身へ重ねる。
//!
//! 重ねる先はタイトルバーとツールバーの下、アプリの中身と同じ層
//! ([`crate::window`] が作る 3 行目)。`Grid` は子を重ね順に置くので、
//! あとから足したトーストが中身の上に出る。
//!
//! 色と角丸は `{ThemeResource ...}` で引くので、ライト / ダークの
//! どちらにも追従する ([`crate::list`] の枠と同じ作り)。
//!
//! 消えるまでの時間は `DispatcherQueueTimer` が数える。

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use naui_core::{Result, ToastSpec};
use windows::Foundation::TimeSpan;
use windows_core::{Interface, HSTRING};
use winui3::Microsoft::UI::Dispatching::{DispatcherQueue, DispatcherQueueTimer};
use winui3::Microsoft::UI::Xaml::Controls::{
    Button, Grid, Orientation as XamlOrientation, StackPanel, TextBlock,
};
use winui3::Microsoft::UI::Xaml::Markup::XamlReader;
use winui3::Microsoft::UI::Xaml::{RoutedEventHandler, UIElement, Visibility};

use crate::to_error;
use crate::ui_thread::UiThreadCell;
use crate::window::owner_content_layer;

/// トーストの見た目。下端の中央へ置く。
const TOAST_XAML: &str = r##"<Grid
    xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
    HorizontalAlignment="Center" VerticalAlignment="Bottom" Margin="24"
    Background="{ThemeResource ControlFillColorDefaultBrush}"
    BorderBrush="{ThemeResource ControlStrokeColorDefaultBrush}"
    BorderThickness="1" CornerRadius="8" Padding="16,10">
    <StackPanel Orientation="Horizontal" Spacing="12" VerticalAlignment="Center">
        <TextBlock VerticalAlignment="Center" TextWrapping="WrapWholeWords"
            Foreground="{ThemeResource TextFillColorPrimaryBrush}"/>
        <Button Visibility="Collapsed">
            <TextBlock/>
        </Button>
    </StackPanel>
</Grid>"##;

/// `TimeSpan` の 1 ミリ秒 (100 ナノ秒きざみ)。
const TICKS_PER_MILLI: i64 = 10_000;

thread_local! {
    /// いま出ているトースト。同時に出るのは 1 つで、新しいものが置き換える。
    static CURRENT: RefCell<Option<Toast>> = const { RefCell::new(None) };
}

struct ToastInner {
    /// 重ねる本体。
    native: Grid,
    label: TextBlock,
    button: Button,
    /// 操作ボタンの文字。`Button` の中身は `TextBlock` にしてある
    /// (ほかのボタンと同じ作り)。
    button_label: TextBlock,
    spec: RefCell<ToastSpec>,
    /// 重ねた先。出している間だけ持つ。
    layer: RefCell<Option<Grid>>,
    /// 自動で消すためのタイマー。消えない指定なら持たない。
    timer: RefCell<Option<DispatcherQueueTimer>>,
    on_action: RefCell<Option<Box<dyn FnMut()>>>,
    on_dismiss: RefCell<Option<Box<dyn FnMut()>>>,
    visible: Cell<bool>,
}

/// 一時的な通知 (`Grid` を重ねたもの)。
///
/// ウィジェットではないので、コンテナへは入れない (`Dialog` と同じ)。
#[derive(Clone)]
pub struct Toast(Rc<ToastInner>);

impl Toast {
    pub(crate) fn new(message: &str) -> Result<Self> {
        let (native, label, button, button_label) = build_surface()?;
        label
            .SetText(&HSTRING::from(message))
            .map_err(|e| to_error("トーストの文字設定", e))?;

        let this = Self(Rc::new(ToastInner {
            native,
            label,
            button,
            button_label,
            spec: RefCell::new(ToastSpec::new(message)),
            layer: RefCell::new(None),
            timer: RefCell::new(None),
            on_action: RefCell::new(None),
            on_dismiss: RefCell::new(None),
            visible: Cell::new(false),
        }));
        this.install_action_handler()?;
        Ok(this)
    }

    /// 操作ボタンが押されたら、通知して消えるようにする。
    fn install_action_handler(&self) -> Result<()> {
        let weak = UiThreadCell::new(Rc::downgrade(&self.0));
        let handler = RoutedEventHandler::new(move |_sender, _args| {
            weak.try_with_mut(|weak: &mut Weak<ToastInner>| {
                if let Some(inner) = weak.upgrade() {
                    Toast(inner).finish(true);
                }
            });
            Ok(())
        });
        self.0
            .button
            .Click(&handler)
            .map_err(|e| to_error("トーストのボタンの購読", e))?;
        Ok(())
    }

    /// 出す文字列。出している間に呼ぶと、その場で書き換わる。
    pub fn set_message(&self, message: &str) {
        self.0.spec.borrow_mut().set_message(message);
        let _ = self.0.label.SetText(&HSTRING::from(message));
    }

    pub fn message(&self) -> String {
        self.0.spec.borrow().message().to_string()
    }

    /// 操作ボタンの文字列。**空文字列を渡すとボタンを外す。**
    pub fn set_action(&self, label: &str) {
        self.0.spec.borrow_mut().set_action(label);
        let action = self.0.spec.borrow().action().map(str::to_string);
        match action {
            Some(label) => {
                let _ = self.0.button_label.SetText(&HSTRING::from(label));
                let _ = self.0.button.SetVisibility(Visibility::Visible);
            }
            None => {
                let _ = self.0.button.SetVisibility(Visibility::Collapsed);
            }
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
    pub fn on_action(&self, f: impl FnMut() + 'static) {
        *self.0.on_action.borrow_mut() = Some(Box::new(f));
    }

    /// 消えたときに呼ばれる。設定し直すと以前のものは外れる。
    ///
    /// 呼ばれるのは**時間で消えたとき**と**操作ボタンで消えたとき**。
    /// [`dismiss`](Self::dismiss) で消したときと、別のトーストに
    /// 置き換えられたときは呼ばれない (アプリ自身の操作は通知しない、
    /// という [`Dialog::close`](crate::Dialog::close) と同じ決まり)。
    pub fn on_dismiss(&self, f: impl FnMut() + 'static) {
        *self.0.on_dismiss.borrow_mut() = Some(Box::new(f));
    }

    /// トーストを出す。
    ///
    /// **同時に出るのは 1 つ**で、ほかのトーストが出ていれば置き換える
    /// (置き換えられたほうの `on_dismiss` は呼ばれない)。
    /// まだウィンドウを表示していないときは何もしない
    /// (重ねる先が決まらないため。`Dialog` と同じ)。
    pub fn show(&self) {
        let Some(layer) = owner_content_layer() else {
            return;
        };
        if let Some(previous) = CURRENT.with(|slot| slot.borrow_mut().take()) {
            previous.take_down();
        }
        let element = match self.0.native.cast::<UIElement>() {
            Ok(element) => element,
            Err(_) => return,
        };
        if layer
            .Children()
            .and_then(|children| children.Append(&element))
            .is_err()
        {
            return;
        }
        *self.0.layer.borrow_mut() = Some(layer);
        self.0.visible.set(true);
        self.start_timer();
        CURRENT.with(|slot| *slot.borrow_mut() = Some(self.clone()));
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
        self.0.visible.get()
    }

    /// 重ねている要素。バックエンド固有の脱出口。
    pub fn native_element(&self) -> UIElement {
        self.0
            .native
            .cast::<UIElement>()
            .expect("Grid は UIElement")
    }

    /// 時間を数え始める。消えない指定なら何もしない。
    fn start_timer(&self) {
        let Some(millis) = self.0.spec.borrow().timeout_millis() else {
            return;
        };
        let Ok(queue) = DispatcherQueue::GetForCurrentThread() else {
            return;
        };
        let Ok(timer) = queue.CreateTimer() else {
            return;
        };
        let interval = TimeSpan {
            Duration: i64::from(millis) * TICKS_PER_MILLI,
        };
        if timer.SetInterval(interval).is_err() || timer.SetIsRepeating(false).is_err() {
            return;
        }
        let weak = UiThreadCell::new(Rc::downgrade(&self.0));
        let handler = windows::Foundation::TypedEventHandler::<
            DispatcherQueueTimer,
            windows_core::IInspectable,
        >::new(move |_sender, _args| {
            weak.try_with_mut(|weak: &mut Weak<ToastInner>| {
                if let Some(inner) = weak.upgrade() {
                    Toast(inner).finish(false);
                }
            });
            Ok(())
        });
        if timer.Tick(&handler).is_err() || timer.Start().is_err() {
            return;
        }
        *self.0.timer.borrow_mut() = Some(timer);
    }

    /// 消えたことをアプリへ知らせる。`action` は操作ボタンで消えたか。
    fn finish(&self, action: bool) {
        if !self.is_visible() {
            return;
        }
        self.forget_current();
        self.take_down();
        if action {
            emit(&self.0.on_action);
        }
        emit(&self.0.on_dismiss);
    }

    /// 重ねた要素を外し、タイマーを止める。通知はしない。
    fn take_down(&self) {
        if let Some(timer) = self.0.timer.borrow_mut().take() {
            let _ = timer.Stop();
        }
        self.0.visible.set(false);
        let Ok(element) = self.0.native.cast::<UIElement>() else {
            return;
        };
        // 重ねた先から外す。出したあとにウィンドウが変わっても、
        // 足した側の入れ物から確実に取れるように覚えておく。
        let Some(layer) = self.0.layer.borrow_mut().take() else {
            return;
        };
        let Ok(children) = layer.Children() else {
            return;
        };
        let mut index = 0;
        if children.IndexOf(&element, &mut index).unwrap_or(false) {
            let _ = children.RemoveAt(index);
        }
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

/// 通知。通知の中から設定し直しても二重借用にならないよう、
/// 呼び出しの間だけクロージャを取り出す。
fn emit(slot: &RefCell<Option<Box<dyn FnMut()>>>) {
    let Some(mut f) = slot.borrow_mut().take() else {
        return;
    };
    f();
    let mut slot = slot.borrow_mut();
    if slot.is_none() {
        *slot = Some(f);
    }
}

/// テーマ付きの見た目を読み込む。読めなければ素の `Grid` に戻す
/// ([`crate::list`] の枠と同じ受け皿)。
fn build_surface() -> Result<(Grid, TextBlock, Button, TextBlock)> {
    match load_surface() {
        Ok(surface) => Ok(surface),
        Err(error) => {
            eprintln!("naui-windows: トーストのテーマ付き見た目の生成に失敗: {error}");
            plain_surface()
        }
    }
}

fn load_surface() -> Result<(Grid, TextBlock, Button, TextBlock)> {
    let native = XamlReader::Load(&HSTRING::from(TOAST_XAML))
        .and_then(|element| element.cast::<Grid>())
        .map_err(|e| to_error("トーストの生成", e))?;
    let row = native
        .Children()
        .and_then(|children| children.GetAt(0))
        .and_then(|child| child.cast::<StackPanel>())
        .map_err(|e| to_error("トーストの中身の取得", e))?;
    let children = row
        .Children()
        .map_err(|e| to_error("トーストの中身の取得", e))?;
    let label = children
        .GetAt(0)
        .and_then(|child| child.cast::<TextBlock>())
        .map_err(|e| to_error("トーストの文字の取得", e))?;
    let button = children
        .GetAt(1)
        .and_then(|child| child.cast::<Button>())
        .map_err(|e| to_error("トーストのボタンの取得", e))?;
    let button_label = button
        .Content()
        .and_then(|content| content.cast::<TextBlock>())
        .map_err(|e| to_error("トーストのボタンの文字の取得", e))?;
    Ok((native, label, button, button_label))
}

fn plain_surface() -> Result<(Grid, TextBlock, Button, TextBlock)> {
    let native = Grid::new().map_err(|e| to_error("トーストの生成", e))?;
    let row = StackPanel::new().map_err(|e| to_error("トーストの中身の生成", e))?;
    row.SetOrientation(XamlOrientation::Horizontal)
        .map_err(|e| to_error("トーストの中身の向き設定", e))?;
    row.SetSpacing(12.0)
        .map_err(|e| to_error("トーストの中身の間隔設定", e))?;
    let label = TextBlock::new().map_err(|e| to_error("トーストの文字の生成", e))?;
    let button = Button::new().map_err(|e| to_error("トーストのボタンの生成", e))?;
    let button_label = TextBlock::new().map_err(|e| to_error("トーストのボタンの文字の生成", e))?;
    button
        .SetContent(&button_label)
        .map_err(|e| to_error("トーストのボタンの文字の配置", e))?;
    button
        .SetVisibility(Visibility::Collapsed)
        .map_err(|e| to_error("トーストのボタンの表示設定", e))?;
    let children = row
        .Children()
        .map_err(|e| to_error("トーストの中身の子取得", e))?;
    children
        .Append(&label)
        .map_err(|e| to_error("トーストの文字の配置", e))?;
    children
        .Append(&button)
        .map_err(|e| to_error("トーストのボタンの配置", e))?;
    native
        .Children()
        .map_err(|e| to_error("トーストの子取得", e))?
        .Append(&row)
        .map_err(|e| to_error("トーストの中身の配置", e))?;
    Ok((native, label, button, button_label))
}
