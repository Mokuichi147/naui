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
//! insets a layout guide by its leading and trailing sides (`start` and `end`),
//! and the content is tied to the guide's leading edge.
//!
//! A group's box has no natural size of its own; its size comes from the content
//! through those constraints. On recent macOS, where `NSBox` is drawn by SwiftUI,
//! the box reports the heading's size as its intrinsic size. A `Stack` holds `Auto`
//! children to their intrinsic size at 999, which beat the content's compression
//! resistance and squashed the content into the heading's height. The heading's
//! width is kept instead by a separate minimum width, which only keeps the box from
//! becoming narrower than the heading.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObjectProtocol};
use objc2::{define_class, msg_send, MainThreadMarker, MainThreadOnly, Message};
use objc2_app_kit::{
    NSBox, NSBoxType, NSFontAttributeName, NSLayoutConstraint, NSLayoutGuide, NSLayoutPriority,
    NSStringDrawing, NSTitlePosition, NSView, NSViewNoIntrinsicMetric,
};
use objc2_foundation::{NSArray, NSDictionary, NSSize, NSString};

use naui_core::{Orientation, Padding, Sizing};

use crate::widgets::{impl_widget, Widget};

/// How strongly a separator keeps its 1 pt thickness. Above the default compression
/// resistance (750), below required, so that a size the app specifies wins.
const THICKNESS_PRIORITY: NSLayoutPriority = 999.0;

/// How strongly a group keeps the heading's width. Above the content's wish to fill
/// the inside ([`CROSS_FILL_PRIORITY`](crate::layout::CROSS_FILL_PRIORITY)), so that
/// content narrower than the heading stretches instead of the heading being cut
/// off. Below required, so that a fixed or maximum width the app specifies wins.
const TITLE_WIDTH_PRIORITY: NSLayoutPriority = crate::layout::CROSS_FILL_PRIORITY + 0.5;

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

// 大きさは中身との制約だけで決める `NSBox`。
define_class!(
    #[unsafe(super(NSBox))]
    #[thread_kind = MainThreadOnly]
    #[name = "NauiGroupBox"]
    struct NauiGroupBox;

    unsafe impl NSObjectProtocol for NauiGroupBox {}

    impl NauiGroupBox {
        // SwiftUI で描く `NSBox` は見出しの大きさを自然な大きさとして返す。
        // 中身を含まない値なので、Stack が Auto の子をこれへ縮めると中身がつぶれる。
        #[unsafe(method(intrinsicContentSize))]
        fn intrinsic_content_size(&self) -> NSSize {
            let none = unsafe { NSViewNoIntrinsicMetric };
            NSSize::new(none, none)
        }
    }
);

// `define_class!` は親の `NSBox` までしか `AsRef` を作らない。
impl AsRef<NSView> for NauiGroupBox {
    fn as_ref(&self) -> &NSView {
        let native: &NSBox = self;
        native.as_ref()
    }
}

impl NauiGroupBox {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        unsafe { msg_send![super(this), init] }
    }

    /// The width the heading needs, including its inset from the frame's edge. 0
    /// without a heading.
    fn title_width(&self) -> f64 {
        if self.titlePosition() == NSTitlePosition::NoTitle {
            return 0.0;
        }
        // SwiftUI で描く `NSBox` は見出しの幅を自然な大きさとして返す。
        let size: NSSize = unsafe { msg_send![super(self), intrinsicContentSize] };
        if size.width > 0.0 {
            return size.width;
        }
        // 返さない `NSBox` では、見出しの文字を測って左右に同じ字下げを足す。
        let font = self.titleFont();
        let font_object: &AnyObject = font.as_ref();
        let attributes =
            NSDictionary::from_slices(&[unsafe { NSFontAttributeName }], &[font_object]);
        let text = unsafe { self.title().sizeWithAttributes(Some(&attributes)) };
        text.width.ceil() + 2.0 * self.titleRect().origin.x.max(0.0)
    }
}

struct GroupInner {
    native: Retained<NauiGroupBox>,
    /// Keeps the box at least as wide as the heading. Its constant follows the text.
    title_width: Retained<NSLayoutConstraint>,
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
        let native = NauiGroupBox::new(mtm);
        native.setBoxType(NSBoxType::Primary);
        native.setContentViewMargins(NSSize::new(0.0, 0.0));
        let inset = NSLayoutGuide::new();
        if let Some(content) = native.contentView() {
            content.addLayoutGuide(&inset);
        }
        let title_width = native
            .widthAnchor()
            .constraintGreaterThanOrEqualToConstant(0.0);
        title_width.setPriority(TITLE_WIDTH_PRIORITY);
        title_width.setActive(true);
        let this = Self(Rc::new(GroupInner {
            native,
            title_width,
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
        self.0.title_width.setConstant(self.0.native.title_width());
        crate::layout::invalidate_ancestors(&self.0.native);
    }

    /// The space between the frame and the content. The default is
    /// [`DEFAULT_GROUP_PADDING`](naui_core::DEFAULT_GROUP_PADDING).
    ///
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
    /// `start` is on the leading side, which is the right in a right-to-left
    /// layout. The content's leading and top edges are fixed. The
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
                .leadingAnchor()
                .constraintEqualToAnchor_constant(&content.leadingAnchor(), padding.start),
            inset
                .topAnchor()
                .constraintEqualToAnchor_constant(&content.topAnchor(), padding.top),
            inset
                .trailingAnchor()
                .constraintEqualToAnchor_constant(&content.trailingAnchor(), -padding.end),
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
