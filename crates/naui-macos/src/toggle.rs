//! 入り切りのスイッチ (`NSSwitch` + ラベルの `NSTextField`)。
//!
//! `NSSwitch` は入り切りの部分だけを持ち、文字は持たない (システム設定の
//! スイッチと同じ)。ラベルはとなりへ `NSTextField` を並べて添える。
//!
//! 幅の余りはラベルが受け取り、スイッチは自分の大きさのままでいる
//! (`NumberInput` の上下ボタンと同じ扱い)。

use std::cell::RefCell;
use std::rc::Rc;

use objc2::rc::Retained;
use objc2::{sel, MainThreadMarker, Message};
use objc2_app_kit::{
    NSControlStateValueOff, NSControlStateValueOn, NSLayoutAttribute,
    NSLayoutConstraintOrientation, NSLayoutPriority, NSStackView, NSStackViewDistribution,
    NSSwitch, NSTextField, NSUserInterfaceLayoutOrientation, NSView,
};
use objc2_foundation::NSString;

use crate::trampoline::{ActionTarget, ValueHandler};
use crate::widgets::{impl_widget, Widget};

/// スイッチとラベルのすき間。
const SPACING: f64 = 8.0;

/// スイッチは自分の大きさのままでいて、幅の余りはラベルへ渡す。
const SWITCH_HUGGING: NSLayoutPriority = 1000.0;

struct ToggleInner {
    native: Retained<NSStackView>,
    switch: Retained<NSSwitch>,
    label: Retained<NSTextField>,
    handler: ValueHandler<bool>,
    /// AppKit の target は弱参照なので、ここで生かしておく。
    target: RefCell<Option<Retained<ActionTarget>>>,
}

/// 入り切りを切り替えるスイッチ (`NSSwitch`)。
#[derive(Clone)]
pub struct Toggle(Rc<ToggleInner>);
impl_widget!(Toggle);

impl Toggle {
    pub(crate) fn new(mtm: MainThreadMarker, label: &str) -> Self {
        let native = NSStackView::new(mtm);
        native.setOrientation(NSUserInterfaceLayoutOrientation::Horizontal);
        // 中身の hugging priority に従って余りを配る (`Stack` と同じ理由)。
        native.setDistribution(NSStackViewDistribution::GravityAreas);
        native.setAlignment(NSLayoutAttribute::CenterY);
        native.setSpacing(SPACING);

        let switch = NSSwitch::new(mtm);
        switch.setState(NSControlStateValueOff);
        switch.setContentHuggingPriority_forOrientation(
            SWITCH_HUGGING,
            NSLayoutConstraintOrientation::Horizontal,
        );

        let label = NSTextField::labelWithString(&NSString::from_str(label), mtm);

        let this = Self(Rc::new(ToggleInner {
            native,
            switch,
            label,
            handler: ValueHandler::default(),
            target: RefCell::new(None),
        }));
        this.0.native.addArrangedSubview(&this.0.switch);
        this.0.native.addArrangedSubview(&this.0.label);

        // 中継はハンドルと同じ寿命で持つ。作り直すと、通知の中から切り替えた
        // ときに実行中の中継そのものを解放してしまう。
        let target = ActionTarget::new(mtm, {
            let weak = Rc::downgrade(&this.0);
            move || {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                let on = inner.switch.state() != NSControlStateValueOff;
                inner.handler.emit(on);
            }
        });
        unsafe {
            this.0.switch.setTarget(Some(&target));
            this.0.switch.setAction(Some(sel!(invoke:)));
        }
        *this.0.target.borrow_mut() = Some(target);
        this
    }

    /// 入っているかどうか。
    pub fn is_on(&self) -> bool {
        self.0.switch.state() != NSControlStateValueOff
    }

    /// プログラムから切り替える。`on_toggle` は呼ばれない。
    pub fn set_on(&self, on: bool) {
        self.0.switch.setState(if on {
            NSControlStateValueOn
        } else {
            NSControlStateValueOff
        });
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.0.switch.setEnabled(enabled);
        // 文字も操作できないように見せる (`NSTextField` のラベルは
        // enabled を持つが、押しても何も起きないので見た目だけ)。
        self.0.label.setEnabled(enabled);
    }

    /// 利用者が切り替えるたびに、切り替えた後の状態で呼ばれる。
    pub fn on_toggle(&self, f: impl FnMut(bool) + 'static) {
        self.0.handler.set(f);
    }

    /// クリックを発生させる (テストや自動操作用)。
    pub fn click(&self) {
        unsafe { self.0.switch.performClick(None) };
    }

    /// スイッチ本体。バックエンド固有の脱出口として公開している。
    pub fn native_switch(&self) -> Retained<NSSwitch> {
        self.0.switch.clone()
    }
}
