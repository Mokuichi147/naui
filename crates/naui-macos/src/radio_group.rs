//! 選択肢を並べて 1 つだけ選ばせるラジオグループ (AppKit)。
//!
//! `NSButton` のラジオ型を `NSStackView` へ並べる。AppKit は「同じ superview に
//! いて同じ action を持つラジオボタン」を 1 つの組として扱うので、グループ専用の
//! スタックビューを持つことで、ほかのラジオグループと混ざらない。
//!
//! 排他の状態は naui 側でも明示的に書く。AppKit の自動グルーピングは利用者の
//! クリックにしか効かず、[`set_selected`](RadioGroup::set_selected) や
//! [`clear_selection`](RadioGroup::clear_selection) は naui が面倒を見るため。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use naui_core::Orientation;
use objc2::rc::Retained;
use objc2::{sel, MainThreadMarker, Message};
use objc2_app_kit::{
    NSButton, NSControlStateValueOff, NSControlStateValueOn, NSLayoutAttribute, NSStackView,
    NSStackViewDistribution, NSUserInterfaceLayoutOrientation, NSView,
};
use objc2_foundation::NSString;

use crate::trampoline::{ActionTarget, SelectHandler};
use crate::widgets::{impl_widget, Widget};

struct RadioGroupInner {
    native: Retained<NSStackView>,
    buttons: RefCell<Vec<Retained<NSButton>>>,
    /// ボタンごとの target。AppKit の target は weak なので、ここで生かしておく。
    targets: RefCell<Vec<Retained<ActionTarget>>>,
    handler: SelectHandler,
    /// [`set_enabled`](RadioGroup::set_enabled) の指定。`set_items` で作り直す
    /// ボタンにも同じ状態を引き継ぐために覚えておく。
    enabled: Cell<bool>,
}

/// 選択肢を並べて 1 つだけ選ばせるラジオグループ (`NSButton` のラジオ型)。
///
/// 作った直後と [`set_items`](Self::set_items) の直後は、何も選ばれていない。
#[derive(Clone)]
pub struct RadioGroup(Rc<RadioGroupInner>);
impl_widget!(RadioGroup);

impl RadioGroup {
    pub(crate) fn new(mtm: MainThreadMarker) -> Self {
        let native = NSStackView::new(mtm);
        native.setDistribution(NSStackViewDistribution::GravityAreas);
        apply_orientation(&native, Orientation::Vertical);
        Self(Rc::new(RadioGroupInner {
            native,
            buttons: RefCell::new(Vec::new()),
            targets: RefCell::new(Vec::new()),
            handler: SelectHandler::default(),
            enabled: Cell::new(true),
        }))
    }

    /// 選択肢を作り直し、選択を外す。選択通知は発生しない。
    pub fn set_items<S: AsRef<str>>(&self, items: &[S]) {
        let mtm = MainThreadMarker::from(&*self.0.native);
        for button in self.0.buttons.borrow_mut().drain(..) {
            self.0.native.removeArrangedSubview(&button);
            button.removeFromSuperview();
        }
        self.0.targets.borrow_mut().clear();

        let enabled = self.0.enabled.get();
        let mut buttons = Vec::with_capacity(items.len());
        let mut targets = Vec::with_capacity(items.len());
        for (index, item) in items.iter().enumerate() {
            let button = unsafe {
                NSButton::radioButtonWithTitle_target_action(
                    &NSString::from_str(item.as_ref()),
                    None,
                    None,
                    mtm,
                )
            };
            // ラジオボタンは作った直後にオンで来ることがあるため、明示的に外す。
            button.setState(NSControlStateValueOff);
            button.setEnabled(enabled);
            let target = ActionTarget::new(mtm, {
                let weak = Rc::downgrade(&self.0);
                move || {
                    let Some(inner) = weak.upgrade() else {
                        return;
                    };
                    let group = RadioGroup(inner);
                    // AppKit も排他にしてくれるが、naui 側の状態と必ず揃える。
                    group.write_selected(Some(index));
                    group.0.handler.emit(index);
                }
            });
            unsafe {
                button.setTarget(Some(&target));
                button.setAction(Some(sel!(invoke:)));
            }
            self.0.native.addArrangedSubview(&button);
            buttons.push(button);
            targets.push(target);
        }
        *self.0.buttons.borrow_mut() = buttons;
        *self.0.targets.borrow_mut() = targets;
    }

    /// 選択肢の数。
    pub fn len(&self) -> usize {
        self.0.buttons.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// 現在選ばれている選択肢。未選択なら `None`。
    pub fn selected(&self) -> Option<usize> {
        self.0
            .buttons
            .borrow()
            .iter()
            .position(|button| button.state() != NSControlStateValueOff)
    }

    /// 範囲内の選択肢を通知せずに選ぶ。範囲外なら何もしない。
    pub fn set_selected(&self, index: usize) {
        if index < self.len() {
            self.write_selected(Some(index));
        }
    }

    /// 選択を通知せずに外す。
    pub fn clear_selection(&self) {
        self.write_selected(None);
    }

    /// ユーザーが選んだのと同じように、範囲内の選択肢を選んで通知する。
    pub fn select(&self, index: usize) {
        if index < self.len() {
            self.write_selected(Some(index));
            self.0.handler.emit(index);
        }
    }

    /// 選択肢の並ぶ向き。既定は縦。
    pub fn set_orientation(&self, orientation: Orientation) {
        apply_orientation(&self.0.native, orientation);
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.0.enabled.set(enabled);
        for button in self.0.buttons.borrow().iter() {
            button.setEnabled(enabled);
        }
    }

    /// 選択肢が選ばれたときに、そのインデックスで呼ばれる。
    /// 設定し直すと以前のコールバックは外れる。
    pub fn on_select(&self, f: impl FnMut(usize) + 'static) {
        self.0.handler.set(f);
    }

    /// AppKit の実コントロール。バックエンド固有の脱出口として公開している。
    pub fn native_buttons(&self) -> Vec<Retained<NSButton>> {
        self.0.buttons.borrow().clone()
    }

    /// 排他の状態を書く。`None` なら全部オフ。
    fn write_selected(&self, index: Option<usize>) {
        for (position, button) in self.0.buttons.borrow().iter().enumerate() {
            button.setState(if Some(position) == index {
                NSControlStateValueOn
            } else {
                NSControlStateValueOff
            });
        }
    }
}

fn apply_orientation(native: &NSStackView, orientation: Orientation) {
    if orientation.is_vertical() {
        native.setOrientation(NSUserInterfaceLayoutOrientation::Vertical);
        // 縦に並べたラジオは、丸の位置を左端で揃える。
        native.setAlignment(NSLayoutAttribute::Leading);
    } else {
        native.setOrientation(NSUserInterfaceLayoutOrientation::Horizontal);
        native.setAlignment(NSLayoutAttribute::CenterY);
    }
}
