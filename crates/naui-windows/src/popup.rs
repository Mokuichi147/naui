//! ポップアップ (コンテキスト) メニュー (WinUI 3 のネイティブ `MenuFlyout`)。
//!
//! | naui | WinUI 3 |
//! | --- | --- |
//! | `PopupMenu` | `MenuFlyout` |
//! | 項目 | `MenuFlyoutItem` |
//! | 区切り線 | `MenuFlyoutSeparator` |
//!
//! 出す位置・影・角丸・ライトディスミス・キーボード操作 (矢印と Esc)・
//! 画面端での回り込みは、すべて `MenuFlyout` が持つ。naui が組み立てる
//! ものは無い。
//!
//! 右クリックで出すのは `UIElement.ContextFlyout` に預けるだけでよい。
//! WinUI が右クリック・長押し・コンテキストキーのどれでも出してくれる。

use std::cell::RefCell;
use std::rc::{Rc, Weak};
use std::sync::Arc;

use naui_core::{PopupItem, Result};
use naui_winui3::Microsoft::UI::Xaml::Controls::{MenuFlyout, MenuFlyoutItem, MenuFlyoutSeparator};
use naui_winui3::Microsoft::UI::Xaml::RoutedEventHandler;
use windows::Foundation::Point;
use windows_core::HSTRING;

use crate::navigation::SelectHandler;
use crate::to_error;
use crate::ui_thread::UiThreadCell;
use crate::widgets::Widget;

struct PopupMenuInner {
    flyout: MenuFlyout,
    /// 項目ごとの `MenuFlyoutItem`。区切り線の位置は `None`。
    items: RefCell<Vec<Option<MenuFlyoutItem>>>,
    /// 取り付けたウィジェットのハンドル。コールバックごと生かしておく。
    attached: RefCell<Vec<Box<dyn Widget>>>,
    handler: SelectHandler,
}

/// ポップアップ (コンテキスト) メニュー。
///
/// 画面に並ぶウィジェットではないので [`Widget`] ではない。
/// [`crate::Ui`] が生成したメニューを保持するため、戻り値を捨てても
/// 取り付け先から消えることはない。
#[derive(Clone)]
pub struct PopupMenu(Rc<PopupMenuInner>);

impl PopupMenu {
    pub(crate) fn new() -> Result<Self> {
        let flyout = MenuFlyout::new().map_err(|e| to_error("MenuFlyout の生成", e))?;
        Ok(Self(Rc::new(PopupMenuInner {
            flyout,
            items: RefCell::new(Vec::new()),
            attached: RefCell::new(Vec::new()),
            handler: SelectHandler::new(),
        })))
    }

    /// 項目を作り直す。以前の項目は取り除かれる。
    ///
    /// インデックスは区切り線を含めた並びの位置。
    pub fn set_items(&self, items: &[PopupItem]) {
        let Ok(children) = self.0.flyout.Items() else {
            return;
        };
        let _ = children.Clear();
        self.0.items.borrow_mut().clear();

        let mut built = Vec::with_capacity(items.len());
        for (index, item) in items.iter().enumerate() {
            if item.is_separator() {
                if let Ok(separator) = MenuFlyoutSeparator::new() {
                    let _ = children.Append(&separator);
                }
                built.push(None);
                continue;
            }
            match self.build_item(&item.label, item.enabled, index) {
                Ok(entry) => {
                    let _ = children.Append(&entry);
                    built.push(Some(entry));
                }
                Err(_) => built.push(None),
            }
        }
        *self.0.items.borrow_mut() = built;
    }

    fn build_item(&self, label: &str, enabled: bool, index: usize) -> Result<MenuFlyoutItem> {
        let entry = MenuFlyoutItem::new().map_err(|e| to_error("メニュー項目の生成", e))?;
        entry
            .SetText(&HSTRING::from(label))
            .map_err(|e| to_error("メニュー項目の文字設定", e))?;
        let _ = entry.SetIsEnabled(enabled);

        let weak = weak_cell(&self.0);
        let handler = RoutedEventHandler::new(move |_sender, _args| {
            if let Some(inner) = weak.try_with_mut(|weak| weak.upgrade()).flatten() {
                // 閉じるのは `MenuFlyout` が自分で行う。
                PopupMenu(inner).0.handler.emit(index);
            }
            Ok(())
        });
        entry
            .Click(&handler)
            .map_err(|e| to_error("メニュー項目の購読", e))?;
        Ok(entry)
    }

    /// 区切り線を含めた項目数。
    pub fn len(&self) -> usize {
        self.0.items.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// ウィジェットの右クリックでこのメニューを出すようにする。
    ///
    /// `ContextFlyout` へ預けるので、右クリック・長押し・コンテキストキーの
    /// どれで呼び出すかは WinUI が決める。同じメニューを複数のウィジェットへ
    /// 取り付けてよい (出るのは呼び出したウィジェットの上)。
    pub fn attach(&self, widget: &dyn Widget) {
        let element = widget.native_element();
        if element.SetContextFlyout(&self.0.flyout).is_ok() {
            self.0.attached.borrow_mut().push(widget.boxed_clone());
        }
    }

    /// プログラムからメニューを出す。位置は `widget` の**左上から**の
    /// 論理ピクセル (y は下向き)。
    pub fn open_at(&self, widget: &dyn Widget, x: f64, y: f64) {
        let element = widget.native_element();
        let point = Point {
            X: x as f32,
            Y: y as f32,
        };
        let _ = self.0.flyout.ShowAt2(&element, point);
    }

    /// 出ているメニューを閉じる。出ていなければ何もしない。
    pub fn close(&self) {
        let _ = self.0.flyout.Hide();
    }

    /// ユーザーが選んだのと同じ経路で項目を選ぶ (テストや自動操作用)。
    ///
    /// 区切り線と、選べない項目は無視する。
    pub fn select(&self, index: usize) {
        let entry = self.0.items.borrow().get(index).cloned().flatten();
        let Some(entry) = entry else {
            return;
        };
        if !entry.IsEnabled().unwrap_or(false) {
            return;
        }
        self.close();
        self.0.handler.emit(index);
    }

    /// 項目が選ばれたときに、そのインデックスで呼ばれる。
    pub fn on_select(&self, f: impl FnMut(usize) + 'static) {
        self.0.handler.set(f);
    }

    /// メニューそのもの。バックエンド固有の脱出口として公開している。
    pub fn native_flyout(&self) -> MenuFlyout {
        self.0.flyout.clone()
    }
}

/// WinRT のデリゲートは `Send` を要求するので、UI スレッド限定のセルに包む。
fn weak_cell(inner: &Rc<PopupMenuInner>) -> Arc<UiThreadCell<Weak<PopupMenuInner>>> {
    Arc::new(UiThreadCell::new(Rc::downgrade(inner)))
}
