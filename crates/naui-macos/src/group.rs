//! Dividing and grouping controls (`Separator` and `Group`, both `NSBox`).
//!
//! AppKit uses `NSBox` for both: `NSBoxSeparator` draws the theme's separator line,
//! and `NSBoxPrimary` draws the rounded frame with a title that System Settings and
//! Interface Builder use to group controls.
//!
//! A separator box decides its direction from its frame (wider than tall is
//! horizontal), so its thickness is held at 1 pt by a constraint, below required so
//! that a size the app specifies along that axis still wins. Constraints act on the
//! alignment rect, which for a separator box is the line itself; the frame is a
//! little larger.
//!
//! A group's content is placed in the box's `contentView` with constraints. The
//! box's own margins are 0 and the padding is the constants of those constraints,
//! because `contentViewMargins` can only be the same on both sides. The padding
//! insets a layout guide by its physical sides (`left` / `right`), the same as on
//! Windows and the Web, and the content is tied to the guide's leading and trailing
//! edges, so that in right-to-left text it still starts on the leading side.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use objc2::rc::Retained;
use objc2::{MainThreadMarker, Message};
use objc2_app_kit::{
    NSBox, NSBoxType, NSLayoutConstraint, NSLayoutGuide, NSLayoutPriority, NSTitlePosition, NSView,
};
use objc2_foundation::{NSArray, NSSize, NSString};

use naui_core::{Orientation, Padding, Sizing};

use crate::widgets::{impl_widget, Widget};

/// How strongly a separator keeps its 1 pt thickness. Above the default compression
/// resistance (750), below required, so that a size the app specifies wins.
const THICKNESS_PRIORITY: NSLayoutPriority = 999.0;

// -------------------------------------------------------------- Separator

struct SeparatorInner {
    native: Retained<NSBox>,
}

/// A thin line that divides controls (`NSBox` with `NSBoxSeparator`).
#[derive(Clone)]
pub struct Separator(Rc<SeparatorInner>);
impl_widget!(Separator);

impl Separator {
    pub(crate) fn new(mtm: MainThreadMarker, orientation: Orientation) -> Self {
        let native = NSBox::new(mtm);
        native.setBoxType(NSBoxType::Separator);
        let vertical = orientation.is_vertical();
        let thickness = if vertical {
            native.widthAnchor()
        } else {
            native.heightAnchor()
        }
        .constraintEqualToConstant(1.0);
        thickness.setPriority(THICKNESS_PRIORITY);
        thickness.setActive(true);

        let this = Self(Rc::new(SeparatorInner { native }));
        // 線は引いた向きいっぱいに伸ばす。
        this.set_sizing(if vertical {
            Sizing::fill_height()
        } else {
            Sizing::fill_width()
        });
        this
    }
}

// ------------------------------------------------------------------ Group

struct GroupInner {
    native: Retained<NSBox>,
    /// The inside of the box less the padding, on the box's `contentView`.
    inset: Retained<NSLayoutGuide>,
    /// The content's handle. Held to keep it alive along with its trampolines.
    child: RefCell<Option<Box<dyn Widget>>>,
    /// The constraints placing the guide and the content in the box. Recreated when
    /// the content is replaced or the padding changes.
    constraints: RefCell<Vec<Retained<NSLayoutConstraint>>>,
    padding: Cell<Padding>,
}

impl Drop for GroupInner {
    fn drop(&mut self) {
        let view: &NSView = self.native.as_ref();
        crate::layout::unregister_container(view);
    }
}

/// A frame with a heading around related controls (`NSBox` with `NSBoxPrimary`).
#[derive(Clone)]
pub struct Group(Rc<GroupInner>);
impl_widget!(Group);

impl Group {
    pub(crate) fn new(mtm: MainThreadMarker, text: &str) -> Self {
        let native = NSBox::new(mtm);
        native.setBoxType(NSBoxType::Primary);
        native.setContentViewMargins(NSSize::new(0.0, 0.0));
        let inset = NSLayoutGuide::new();
        if let Some(content) = native.contentView() {
            content.addLayoutGuide(&inset);
        }
        let this = Self(Rc::new(GroupInner {
            native,
            inset,
            child: RefCell::new(None),
            constraints: RefCell::new(Vec::new()),
            padding: Cell::new(naui_core::DEFAULT_GROUP_PADDING),
        }));
        this.set_text(text);
        this.0.place_child();

        // 中身の指定があとから変わったときに張り直す。最大の大きさは
        // 制約の強さに関わるので、中身の制約を作り直す。
        let view: &NSView = this.0.native.as_ref();
        let weak = Rc::downgrade(&this.0);
        crate::layout::register_container(view, move |_child| {
            if let Some(inner) = weak.upgrade() {
                inner.place_child();
            }
        });
        this
    }

    /// The heading text. Empty when there is none.
    pub fn text(&self) -> String {
        if self.0.native.titlePosition() == NSTitlePosition::NoTitle {
            return String::new();
        }
        self.0.native.title().to_string()
    }

    /// The heading text. An empty string removes the heading, leaving only the frame.
    pub fn set_text(&self, text: &str) {
        self.0.native.setTitle(&NSString::from_str(text));
        // 空の見出しでも `NSBox` は見出しの高さを取るので、見出しごと消す。
        self.0.native.setTitlePosition(if text.is_empty() {
            NSTitlePosition::NoTitle
        } else {
            NSTitlePosition::AtTop
        });
        crate::layout::invalidate_ancestors(&self.0.native);
    }

    /// The space between the frame and the content. The default is
    /// [`DEFAULT_GROUP_PADDING`](naui_core::DEFAULT_GROUP_PADDING).
    ///
    /// `left` and `right` are the physical sides, also in right-to-left text.
    pub fn set_padding(&self, padding: Padding) {
        self.0.padding.set(padding);
        self.0.place_child();
    }

    /// The content inside the frame. Each call replaces it.
    ///
    /// The content fills the inside of the frame.
    pub fn set_child(&self, child: &dyn Widget) {
        let Some(content) = self.0.native.contentView() else {
            return;
        };
        if let Some(previous) = self.0.child.borrow_mut().take() {
            previous.native_view().removeFromSuperview();
        }
        let view = child.native_view();
        crate::layout::prepare_child(&view);
        content.addSubview(&view);
        *self.0.child.borrow_mut() = Some(child.boxed_clone());
        self.0.place_child();
    }
}

impl GroupInner {
    /// Ties the content to the inside of the box, `padding` away from its edges.
    ///
    /// The padding is on the physical sides: `left` stays on the left in
    /// right-to-left text too. The content's leading and top edges are fixed. The
    /// trailing and bottom edges may not pass the inside and want to reach it just
    /// below required, so content with a maximum stops there instead of breaking the
    /// constraints.
    fn place_child(&self) {
        let mut constraints = self.constraints.borrow_mut();
        if !constraints.is_empty() {
            NSLayoutConstraint::deactivateConstraints(&NSArray::from_retained_slice(&constraints));
            constraints.clear();
        }
        let Some(content) = self.native.contentView() else {
            return;
        };
        let padding = self.padding.get();
        let inset = &self.inset;
        constraints.extend([
            inset
                .leftAnchor()
                .constraintEqualToAnchor_constant(&content.leftAnchor(), padding.left),
            inset
                .topAnchor()
                .constraintEqualToAnchor_constant(&content.topAnchor(), padding.top),
            inset
                .rightAnchor()
                .constraintEqualToAnchor_constant(&content.rightAnchor(), -padding.right),
            inset
                .bottomAnchor()
                .constraintEqualToAnchor_constant(&content.bottomAnchor(), -padding.bottom),
        ]);
        if let Some(child) = self
            .child
            .borrow()
            .as_ref()
            .map(|child| child.native_view())
        {
            let leading = child
                .leadingAnchor()
                .constraintEqualToAnchor(&inset.leadingAnchor());
            let top = child
                .topAnchor()
                .constraintEqualToAnchor(&inset.topAnchor());
            let trailing_limit = child
                .trailingAnchor()
                .constraintLessThanOrEqualToAnchor(&inset.trailingAnchor());
            let bottom_limit = child
                .bottomAnchor()
                .constraintLessThanOrEqualToAnchor(&inset.bottomAnchor());
            let trailing = child
                .trailingAnchor()
                .constraintEqualToAnchor(&inset.trailingAnchor());
            let bottom = child
                .bottomAnchor()
                .constraintEqualToAnchor(&inset.bottomAnchor());
            trailing.setPriority(crate::layout::CROSS_FILL_PRIORITY);
            bottom.setPriority(crate::layout::CROSS_FILL_PRIORITY);
            constraints.extend([leading, top, trailing_limit, bottom_limit, trailing, bottom]);
        }
        NSLayoutConstraint::activateConstraints(&NSArray::from_retained_slice(&constraints));
        drop(constraints);
        crate::layout::invalidate_ancestors(&self.native);
    }
}
