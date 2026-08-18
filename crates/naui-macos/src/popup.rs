//! ポップアップ (コンテキスト) メニュー (AppKit)。
//!
//! 実体は `NSMenu` で、項目は `NSMenuItem`。区切り線は
//! `NSMenuItem::separatorItem` をそのまま使う。
//!
//! ウィジェットへ [`PopupMenu::attach`] すると、そのビューの `menu` に
//! なる。**右クリック (副ボタン) で出すのは AppKit 自身**で、naui は
//! 位置決めも表示も行わない。プログラムから出したいときだけ
//! [`PopupMenu::open_at`] が `popUpMenuPositioningItem:atLocation:inView:`
//! を呼ぶ。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use naui_core::PopupItem;
use objc2::rc::Retained;
use objc2::{sel, MainThreadMarker};
use objc2_app_kit::{NSMenu, NSMenuItem};
use objc2_foundation::{NSPoint, NSString};

use crate::trampoline::{ActionTarget, SelectHandler};
use crate::widgets::Widget;

struct PopupMenuInner {
    native: Retained<NSMenu>,
    /// 項目ごとのトランポリン。`NSMenuItem` の target は weak なので保持する。
    targets: RefCell<Vec<Retained<ActionTarget>>>,
    handler: SelectHandler,
    /// 区切り線を含めた項目数。
    count: Cell<usize>,
    /// 取り付けたウィジェットのハンドル。ビューごと生かしておく。
    attached: RefCell<Vec<Box<dyn Widget>>>,
}

/// ポップアップ (コンテキスト) メニュー (NSMenu)。
///
/// 画面に並ぶウィジェットではないので [`Widget`] ではない。
/// [`crate::Ui`] が生成したメニューを保持するため、戻り値を捨てても
/// 取り付け先のメニューが消えることはない。
#[derive(Clone)]
pub struct PopupMenu(Rc<PopupMenuInner>);

impl PopupMenu {
    pub(crate) fn new(mtm: MainThreadMarker) -> Self {
        let native = NSMenu::new(mtm);
        // 既定では AppKit が「その操作ができるか」を自分で判断して項目を
        // 出し入れしてしまい、`setEnabled(false)` が無視される。
        native.setAutoenablesItems(false);
        Self(Rc::new(PopupMenuInner {
            native,
            targets: RefCell::new(Vec::new()),
            handler: SelectHandler::default(),
            count: Cell::new(0),
            attached: RefCell::new(Vec::new()),
        }))
    }

    /// 項目を作り直す。以前の項目は取り除かれる。
    ///
    /// インデックスは区切り線を含めた並びの位置。
    pub fn set_items(&self, items: &[PopupItem]) {
        let mtm = MainThreadMarker::from(&*self.0.native);
        self.0.native.removeAllItems();
        self.0.targets.borrow_mut().clear();

        let mut targets = Vec::with_capacity(items.len());
        for (index, item) in items.iter().enumerate() {
            if item.is_separator() {
                self.0.native.addItem(&NSMenuItem::separatorItem(mtm));
                continue;
            }
            let native_item = NSMenuItem::new(mtm);
            native_item.setTitle(&NSString::from_str(&item.label));
            native_item.setEnabled(item.enabled);
            // ハンドルを強く持つとトランポリンとの間で循環するため、弱参照にする。
            let target = ActionTarget::new(mtm, {
                let weak = Rc::downgrade(&self.0);
                move || {
                    let Some(inner) = weak.upgrade() else {
                        return;
                    };
                    inner.handler.emit(index);
                }
            });
            unsafe {
                native_item.setTarget(Some(&target));
                native_item.setAction(Some(sel!(invoke:)));
            }
            self.0.native.addItem(&native_item);
            targets.push(target);
        }
        *self.0.targets.borrow_mut() = targets;
        self.0.count.set(items.len());
    }

    /// 区切り線を含めた項目数。
    pub fn len(&self) -> usize {
        self.0.count.get()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// ウィジェットの右クリックでこのメニューを出すようにする。
    ///
    /// 同じメニューを複数のウィジェットへ取り付けられる。
    pub fn attach(&self, widget: &dyn Widget) {
        let view = widget.native_view();
        unsafe { view.setMenu(Some(&self.0.native)) };
        self.0.attached.borrow_mut().push(widget.boxed_clone());
    }

    /// プログラムからメニューを出す。位置は `widget` の**左上から**の
    /// 論理ピクセル (y は下向き)。
    ///
    /// 出ている間 AppKit がイベントを取り回すため、この呼び出しは
    /// メニューが閉じるまで戻らない。
    pub fn open_at(&self, widget: &dyn Widget, x: f64, y: f64) {
        let view = widget.native_view();
        // AppKit のビュー座標は既定で左下原点。naui は左上原点でそろえる。
        let point = if view.isFlipped() {
            NSPoint::new(x, y)
        } else {
            NSPoint::new(x, view.bounds().size.height - y)
        };
        self.0
            .native
            .popUpMenuPositioningItem_atLocation_inView(None, point, Some(&view));
    }

    /// 出ているメニューを閉じる。出ていなければ何もしない。
    pub fn close(&self) {
        self.0.native.cancelTracking();
    }

    /// ユーザーが選んだのと同じ経路で項目を選ぶ (テストや自動操作用)。
    ///
    /// 区切り線と、選べない項目は無視する。
    pub fn select(&self, index: usize) {
        if index >= self.len() {
            return;
        }
        let Some(item) = self.0.native.itemAtIndex(index as isize) else {
            return;
        };
        if item.isSeparatorItem() || !item.isEnabled() {
            return;
        }
        self.0.native.performActionForItemAtIndex(index as isize);
    }

    /// 項目が選ばれたときに、そのインデックスで呼ばれる。
    pub fn on_select(&self, f: impl FnMut(usize) + 'static) {
        self.0.handler.set(f);
    }

    /// AppKit の実メニュー。バックエンド固有の脱出口として公開している。
    pub fn native_menu(&self) -> Retained<NSMenu> {
        self.0.native.clone()
    }
}
