//! 選択肢から 1 件を選ぶコンボボックス (AppKit)。
//!
//! macOS では編集できる `NSComboBox` ではなく、選択肢だけを持つ
//! `NSPopUpButton` を使う。項目を作り直した直後も未選択に戻すため、
//! 他のバックエンドの `<select>` / ComboBox と同じ状態遷移になる。

use std::rc::Rc;

use objc2::rc::Retained;
use objc2::{sel, MainThreadMarker, MainThreadOnly, Message};
use objc2_app_kit::{NSPopUpButton, NSView};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};

use crate::trampoline::{ActionTarget, SelectHandler};
use crate::widgets::{impl_widget, Widget};

struct ComboBoxInner {
    native: Retained<NSPopUpButton>,
    handler: SelectHandler,
    /// AppKit の target は weak なので、ハンドル側で生かしておく。
    _target: Retained<ActionTarget>,
}

/// 選択肢から 1 件を選ぶドロップダウン (`NSPopUpButton`)。
///
/// 作った直後と [`set_items`](Self::set_items) の直後は、何も選ばれていない。
#[derive(Clone)]
pub struct ComboBox(Rc<ComboBoxInner>);
impl_widget!(ComboBox);

impl ComboBox {
    pub(crate) fn new(mtm: MainThreadMarker) -> Self {
        let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(0.0, 0.0));
        let native =
            NSPopUpButton::initWithFrame_pullsDown(NSPopUpButton::alloc(mtm), frame, false);
        let handler = SelectHandler::default();
        let target = ActionTarget::new(mtm, {
            let native = native.clone();
            let handler = handler.clone();
            move || {
                let index = native.indexOfSelectedItem();
                if index >= 0 && index < native.numberOfItems() {
                    handler.emit(index as usize);
                }
            }
        });
        unsafe {
            native.setTarget(Some(&target));
            native.setAction(Some(sel!(invoke:)));
        }

        Self(Rc::new(ComboBoxInner {
            native,
            handler,
            _target: target,
        }))
    }

    /// 項目を作り直し、選択を外す。選択通知は発生しない。
    pub fn set_items<S: AsRef<str>>(&self, items: &[S]) {
        self.0.native.removeAllItems();
        for item in items {
            self.0
                .native
                .addItemWithTitle(&NSString::from_str(item.as_ref()));
        }
        // NSPopUpButton は最初の項目を追加すると自動選択するので、明示的に外す。
        self.0.native.selectItem(None);
    }

    /// 項目数。
    pub fn len(&self) -> usize {
        self.0.native.numberOfItems() as usize
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// 現在選ばれている項目。未選択なら `None`。
    pub fn selected(&self) -> Option<usize> {
        let index = self.0.native.indexOfSelectedItem();
        (index >= 0 && index < self.0.native.numberOfItems()).then_some(index as usize)
    }

    /// 範囲内の項目を通知せずに選ぶ。範囲外なら何もしない。
    pub fn set_selected(&self, index: usize) {
        if index < self.len() {
            self.0.native.selectItemAtIndex(index as isize);
        }
    }

    /// 選択を通知せずに外す。
    pub fn clear_selection(&self) {
        self.0.native.selectItem(None);
    }

    /// ユーザーが選んだのと同じように、範囲内の項目を選んで通知する。
    pub fn select(&self, index: usize) {
        if index < self.len() {
            self.0.native.selectItemAtIndex(index as isize);
            self.0.handler.emit(index);
        }
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.0.native.setEnabled(enabled);
    }

    /// 項目が選ばれたときに、そのインデックスで呼ばれる。
    /// 設定し直すと以前のコールバックは外れる。
    pub fn on_select(&self, f: impl FnMut(usize) + 'static) {
        self.0.handler.set(f);
    }

    /// AppKit の実コントロール。バックエンド固有の脱出口として公開している。
    pub fn native_combo_box(&self) -> Retained<NSPopUpButton> {
        self.0.native.clone()
    }
}
