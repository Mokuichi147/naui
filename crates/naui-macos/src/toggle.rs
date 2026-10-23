//! An on/off switch (`NSSwitch` + an `NSTextField` for the label).
//!
//! `NSSwitch` has only the on/off part and no text (like the switches in System
//! Settings). The label is added as an `NSTextField` placed next to it.
//!
//! The label takes the leftover width and the switch keeps its own size (the same
//! treatment as `NumberInput`'s up/down buttons).

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

/// The gap between the switch and the label.
const SPACING: f64 = 8.0;

/// The switch keeps its own size and hands leftover width to the label.
const SWITCH_HUGGING: NSLayoutPriority = 1000.0;

struct ToggleInner {
    native: Retained<NSStackView>,
    switch: Retained<NSSwitch>,
    label: Retained<NSTextField>,
    handler: ValueHandler<bool>,
    /// AppKit targets are weak references, so they are kept alive here.
    target: RefCell<Option<Retained<ActionTarget>>>,
}

/// An on/off switch (`NSSwitch`).
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

    /// Whether it is on.
    pub fn is_on(&self) -> bool {
        self.0.switch.state() != NSControlStateValueOff
    }

    /// Switches it from code. `on_toggle` is not called.
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

    /// Called with the new state every time the user switches it.
    pub fn on_toggle(&self, f: impl FnMut(bool) + 'static) {
        self.0.handler.set(f);
    }

    /// Triggers a click (for tests and automation).
    pub fn click(&self) {
        unsafe { self.0.switch.performClick(None) };
    }

    /// The switch itself. Public as a backend-specific escape hatch.
    pub fn native_switch(&self) -> Retained<NSSwitch> {
        self.0.switch.clone()
    }
}
