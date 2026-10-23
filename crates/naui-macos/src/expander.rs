//! A collapsible section (a heading `NSButton` + an `NSStackView` for the content).
//!
//! AppKit has no single control that "shows and hides its content when the heading
//! is clicked". The standard AppKit arrangement is to stack an `NSButton` with a
//! disclosure triangle above the content (the same shape as "Details" in System
//! Settings), so the heading and content are laid out in an `NSStackView`.
//!
//! When collapsed, the content view is hidden. `NSStackView` removes hidden
//! children from layout (the default of `detachesHiddenViews`), so collapsing
//! shrinks it to the heading's height. The app also uses the same `hidden` through
//! [`set_visible`](crate::Label::set_visible), so hiding goes through
//! `crate::interaction`, which combines both settings.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use objc2::rc::Retained;
use objc2::{sel, MainThreadMarker, Message};
use objc2_app_kit::{
    NSButton, NSButtonType, NSCellImagePosition, NSControlStateValueOff, NSControlStateValueOn,
    NSImage, NSLayoutAttribute, NSLayoutConstraint, NSStackView, NSStackViewDistribution,
    NSUserInterfaceLayoutOrientation, NSView,
};
use objc2_foundation::{NSArray, NSString};

use crate::trampoline::{ActionTarget, ValueHandler};
use crate::widgets::{impl_widget, Widget};

/// The gap between the heading and the content.
const SPACING: f64 = 6.0;

/// The triangles for expanded and collapsed (SF Symbols).
const SYMBOL_EXPANDED: &str = "chevron.down";
const SYMBOL_COLLAPSED: &str = "chevron.right";

struct ExpanderInner {
    native: Retained<NSStackView>,
    header: Retained<NSButton>,
    /// The content's handle. Held to keep it alive along with its trampolines.
    child: RefCell<Option<Box<dyn Widget>>>,
    /// The constraints tying the content to the expander's width. Recreated when it is
    /// replaced.
    fill_constraints: RefCell<Vec<Retained<NSLayoutConstraint>>>,
    expanded: Cell<bool>,
    handler: ValueHandler<bool>,
    /// AppKit targets are weak references, so they are kept alive here.
    target: RefCell<Option<Retained<ActionTarget>>>,
}

/// A container whose contents are shown and hidden by clicking its heading.
#[derive(Clone)]
pub struct Expander(Rc<ExpanderInner>);
impl_widget!(Expander);

impl Expander {
    pub(crate) fn new(mtm: MainThreadMarker, text: &str) -> Self {
        let native = NSStackView::new(mtm);
        native.setOrientation(NSUserInterfaceLayoutOrientation::Vertical);
        // 中身の hugging priority に従って余りを配る (`Stack` と同じ理由)。
        native.setDistribution(NSStackViewDistribution::GravityAreas);
        native.setAlignment(NSLayoutAttribute::Leading);
        native.setSpacing(SPACING);

        let header = unsafe {
            NSButton::buttonWithTitle_target_action(&NSString::from_str(text), None, None, mtm)
        };
        // 押すたびに入り切りが変わるボタン。枠は出さず、三角と文字だけを出す。
        header.setButtonType(NSButtonType::PushOnPushOff);
        header.setBordered(false);
        header.setImagePosition(NSCellImagePosition::ImageLeading);

        let this = Self(Rc::new(ExpanderInner {
            native,
            header,
            child: RefCell::new(None),
            fill_constraints: RefCell::new(Vec::new()),
            expanded: Cell::new(false),
            handler: ValueHandler::default(),
            target: RefCell::new(None),
        }));
        this.0.native.addArrangedSubview(&this.0.header);
        this.0.write_native(false);

        // 中継はハンドルと同じ寿命で持つ。作り直すと、通知の中から
        // 開閉したときに実行中の中継そのものを解放してしまう。
        let target = ActionTarget::new(mtm, {
            let weak = Rc::downgrade(&this.0);
            move || {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                let expanded = inner.header.state() != NSControlStateValueOff;
                inner.expanded.set(expanded);
                inner.write_native(expanded);
                inner.handler.emit(expanded);
            }
        });
        unsafe {
            this.0.header.setTarget(Some(&target));
            this.0.header.setAction(Some(sel!(invoke:)));
        }
        *this.0.target.borrow_mut() = Some(target);
        this
    }

    /// The heading text.
    pub fn text(&self) -> String {
        self.0.header.title().to_string()
    }

    pub fn set_text(&self, text: &str) {
        self.0.header.setTitle(&NSString::from_str(text));
    }

    /// Whether it is expanded.
    pub fn is_expanded(&self) -> bool {
        self.0.expanded.get()
    }

    /// Expands or collapses it from code. `on_toggle` is not called.
    pub fn set_expanded(&self, expanded: bool) {
        self.0.expanded.set(expanded);
        self.0.write_native(expanded);
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.0.header.setEnabled(enabled);
    }

    /// The contents to collapse. Each call replaces them.
    pub fn set_child(&self, child: &dyn Widget) {
        if let Some(previous) = self.0.child.borrow_mut().take() {
            let view = previous.native_view();
            self.0.native.removeArrangedSubview(&view);
            view.removeFromSuperview();
            // 外した中身は、たたんでいたことによる非表示を解く。
            crate::interaction::set_hidden_by_container(&view, false);
        }
        let mut constraints = self.0.fill_constraints.borrow_mut();
        if !constraints.is_empty() {
            NSLayoutConstraint::deactivateConstraints(&NSArray::from_retained_slice(&constraints));
            constraints.clear();
        }

        let view = child.native_view();
        crate::layout::prepare_child(&view);
        if !crate::layout::wants_fill(&view, false) {
            crate::layout::keep_auto_size(&view, false);
        }
        self.0.native.addArrangedSubview(&view);
        // 中身は開いた場所の幅いっぱいに置く (`Scroll::set_child` と同じ扱い)。
        // 中身の中で左右のどこへ寄せるかは、中身のコンテナが決める。
        let at_most = view
            .widthAnchor()
            .constraintLessThanOrEqualToAnchor(&self.0.native.widthAnchor());
        let equal = view
            .widthAnchor()
            .constraintEqualToAnchor(&self.0.native.widthAnchor());
        equal.setPriority(crate::layout::CROSS_FILL_PRIORITY);
        for constraint in [&at_most, &equal] {
            constraint.setActive(true);
        }
        constraints.push(at_most);
        constraints.push(equal);
        drop(constraints);

        crate::interaction::set_hidden_by_container(&view, !self.is_expanded());
        *self.0.child.borrow_mut() = Some(child.boxed_clone());
    }

    /// Called with the new state every time the user expands or collapses it.
    pub fn on_toggle(&self, f: impl FnMut(bool) + 'static) {
        self.0.handler.set(f);
    }

    /// Triggers a click on the heading (for tests and automation).
    pub fn click(&self) {
        unsafe { self.0.header.performClick(None) };
    }

    /// The heading button. Public as a backend-specific escape hatch.
    pub fn native_header(&self) -> Retained<NSButton> {
        self.0.header.clone()
    }
}

impl ExpanderInner {
    /// Writes the expanded state to the native side.
    fn write_native(&self, expanded: bool) {
        self.header.setState(if expanded {
            NSControlStateValueOn
        } else {
            NSControlStateValueOff
        });
        let symbol = NSString::from_str(if expanded {
            SYMBOL_EXPANDED
        } else {
            SYMBOL_COLLAPSED
        });
        let image = NSImage::imageWithSystemSymbolName_accessibilityDescription(&symbol, None);
        self.header.setImage(image.as_deref());
        if let Some(child) = self.child.borrow().as_ref() {
            crate::interaction::set_hidden_by_container(&child.native_view(), !expanded);
        }
        // 開閉で高さが変わる。親の連なりへ伝えないと、Grid の Auto 行が
        // たたんだときの高さのままになり、開いた中身が潰れる。
        crate::layout::invalidate_ancestors(&self.native);
    }
}
