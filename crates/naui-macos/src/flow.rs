//! `Flow`: a container that wraps its children onto new lines, laid out by naui.
//!
//! AppKit has no view that wraps arbitrary views: `NSCollectionView`'s flow layout
//! lays out items the collection view creates and recycles itself, and
//! `NSStackView` only drops views that do not fit. So the view here
//! (`NauiFlowView`) places its children itself with naui-core's [`FlowLayout`], in
//! AppKit's own layout pass, the way `NSTextField` wraps its text:
//!
//! - Its intrinsic height is the height of the lines at its current width. When a
//!   layout pass gives it another width, `layout` places the children for it and,
//!   if that changes the height, invalidates the intrinsic size, which has AppKit
//!   solve the constraints again in the same pass (as the `Grid`'s `Auto` rows do).
//! - Its intrinsic width is the width of all children on one line, held by a
//!   compression resistance below a window's (500), so it never widens the window
//!   and wraps instead. A constraint at the content's priority keeps it at least as
//!   wide as its widest child.
//! - The children keep their own size constraints (`Sizing`) and are measured with
//!   `fittingSize`; the flow sets their frames (with
//!   `translatesAutoresizingMaskIntoConstraints`), so their positions are not
//!   constraints and changing lines never re-enters the constraint solver.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use naui_core::{flow_line_tops, Align, FlowItem, FlowLayout, Padding};
use objc2::rc::Retained;
use objc2::runtime::NSObjectProtocol;
use objc2::{define_class, msg_send, DefinedClass, MainThreadMarker, MainThreadOnly, Message};
use objc2_app_kit::{
    NSLayoutConstraint, NSLayoutConstraintOrientation, NSLayoutPriority,
    NSUserInterfaceLayoutDirection, NSView, NSWindowOrderingMode,
};
use objc2_foundation::{NSPoint, NSRect, NSSize};

use crate::widgets::{impl_interaction, Widget};

/// The flow's compression resistance on the width while it fits its content.
///
/// Below a window keeping its size (`NSLayoutPriorityWindowSizeStayPut` = 500), so a
/// flow whose children would be wider on one line wraps instead of widening the
/// window.
const WRAP_COMPRESSION_RESISTANCE: NSLayoutPriority = 250.0;

/// The priority keeping the flow at least as wide as its widest child: the same as
/// a control's own compression resistance.
const MINIMUM_WIDTH_PRIORITY: NSLayoutPriority = 750.0;

/// What the flow view and its handle share.
struct FlowState {
    /// The children in order.
    children: RefCell<Vec<Retained<NSView>>>,
    column_spacing: Cell<f64>,
    row_spacing: Cell<f64>,
    padding: Cell<Padding>,
    align: Cell<Align>,
    /// The height last reported as the intrinsic height.
    reported_height: Cell<f64>,
    /// "At least as wide as the widest child". Its constant follows the children.
    minimum_width: RefCell<Option<Retained<NSLayoutConstraint>>>,
}

struct FlowViewIvars {
    state: Rc<FlowState>,
}

define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "NauiFlowView"]
    #[ivars = FlowViewIvars]
    struct FlowView;

    unsafe impl NSObjectProtocol for FlowView {}

    impl FlowView {
        /// Lines run from the top.
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(intrinsicContentSize))]
        fn intrinsic_content_size(&self) -> NSSize {
            // 高さは今の幅で折り返したときのもの。幅は 1 行に並べたときの幅。
            let arranged = self.arrange(Some(self.bounds().size.width));
            self.ivars().state.reported_height.set(arranged.height);
            NSSize::new(arranged.natural_width, arranged.height)
        }

        #[unsafe(method(layout))]
        fn layout(&self) {
            let _: () = unsafe { msg_send![super(self), layout] };
            let arranged = self.arrange(Some(self.bounds().size.width));
            for (view, frame) in &arranged.frames {
                view.setFrame(*frame);
            }
            let state = &self.ivars().state;
            if let Some(constraint) = state.minimum_width.borrow().as_ref() {
                if (constraint.constant() - arranged.minimum_width).abs() > 0.5 {
                    constraint.setConstant(arranged.minimum_width);
                }
            }
            // 幅が変わって行の数が変わったら、新しい高さで解き直してもらう
            // (NSTextField の折り返しと同じ)。
            if (state.reported_height.get() - arranged.height).abs() > 0.5 {
                state.reported_height.set(arranged.height);
                self.invalidateIntrinsicContentSize();
            }
        }
    }
);

/// The children placed for one width.
struct Arranged {
    frames: Vec<(Retained<NSView>, NSRect)>,
    height: f64,
    natural_width: f64,
    minimum_width: f64,
}

impl FlowView {
    fn new(mtm: MainThreadMarker, state: Rc<FlowState>) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(FlowViewIvars { state });
        unsafe { msg_send![super(this), init] }
    }

    /// Breaks the shown children into lines for `width` (the flow's own width,
    /// padding included), or for the width of one line when it is not laid out yet.
    fn arrange(&self, width: Option<f64>) -> Arranged {
        let state = &self.ivars().state;
        let padding = state.padding.get();
        let column_spacing = state.column_spacing.get();
        let views: Vec<Retained<NSView>> = state
            .children
            .borrow()
            .iter()
            .filter(|view| !view.isHidden() && crate::layout::is_placed_in(view, self))
            .cloned()
            .collect();
        let fitting: Vec<NSSize> = views.iter().map(|view| view.fittingSize()).collect();
        let items: Vec<FlowItem> = views
            .iter()
            .zip(&fitting)
            .map(|(view, size)| {
                let fill = crate::layout::wants_fill(view, true);
                FlowItem {
                    // `Fill` の子は行の幅まで縮められる。ほかは中身の幅を守る
                    // (縮めた幅での高さを AppKit から引けないため)。
                    minimum: if fill { 0.0 } else { size.width },
                    natural: size.width,
                    grow: fill,
                }
            })
            .collect();
        let horizontal_padding = padding.start + padding.end;
        let natural_width = naui_core::flow_natural_width(&items, column_spacing);
        let minimum_width = naui_core::flow_minimum_width(&items);
        let room = match width {
            Some(width) if width > 0.0 => (width - horizontal_padding).max(0.0),
            _ => natural_width,
        };
        let layout = FlowLayout::new(&items, room, column_spacing, state.align.get());

        // 幅を変えた子 (伸ばした・縮めた) は、その幅での高さを測り直す。
        let widths: Vec<f64> = layout
            .slots
            .iter()
            .zip(&views)
            .map(|(slot, view)| match crate::layout::max_size(view, true) {
                Some(max) if slot.width > max => max,
                _ => slot.width,
            })
            .collect();
        let heights: Vec<f64> = views
            .iter()
            .zip(&fitting)
            .zip(&widths)
            .map(|((view, size), width)| {
                if (size.width - width).abs() > 0.5 {
                    height_at(view, *width)
                } else {
                    size.height
                }
            })
            .collect();
        let line_heights = layout.line_heights(&heights);
        let (tops, lines_height) = flow_line_tops(&line_heights, state.row_spacing.get());

        let mirrored =
            self.userInterfaceLayoutDirection() == NSUserInterfaceLayoutDirection::RightToLeft;
        let frames = views
            .into_iter()
            .zip(layout.slots.iter())
            .zip(widths.iter().zip(&heights))
            .map(|((view, slot), (width, height))| {
                let line_height = line_heights[slot.line];
                let height = if crate::layout::wants_fill(&view, false) {
                    match crate::layout::max_size(&view, false) {
                        Some(max) => line_height.min(max),
                        None => line_height,
                    }
                } else {
                    *height
                };
                let x = if mirrored {
                    padding.end + room - slot.x - width
                } else {
                    padding.start + slot.x
                };
                let y = padding.top + tops[slot.line] + (line_height - height) / 2.0;
                let frame = NSRect::new(NSPoint::new(x, y), NSSize::new(*width, height));
                (view, frame)
            })
            .collect();
        Arranged {
            frames,
            height: lines_height + padding.top + padding.bottom,
            natural_width: natural_width + horizontal_padding,
            minimum_width: minimum_width + horizontal_padding,
        }
    }
}

/// The height `view` needs at `width`, measured with a temporary width constraint.
fn height_at(view: &NSView, width: f64) -> f64 {
    let constraint = view.widthAnchor().constraintEqualToConstant(width.max(0.0));
    constraint.setActive(true);
    let height = view.fittingSize().height;
    constraint.setActive(false);
    height
}

struct FlowInner {
    native: Retained<FlowView>,
    state: Rc<FlowState>,
    /// Holds the children's handles, keeping them alive along with their trampolines.
    children: RefCell<Vec<Box<dyn Widget>>>,
}

impl Drop for FlowInner {
    fn drop(&mut self) {
        crate::layout::unregister_container(&self.native);
    }
}

/// A container that wraps its children onto new lines (an `NSView` named
/// `NauiFlowView` that places them itself; see the module documentation).
#[derive(Clone)]
pub struct Flow(Rc<FlowInner>);

impl_interaction!(Flow);

impl Widget for Flow {
    fn native_view(&self) -> Retained<NSView> {
        let view: &NSView = self.0.native.as_ref();
        view.retain()
    }

    fn boxed_clone(&self) -> Box<dyn Widget> {
        Box::new(self.clone())
    }
}

impl Flow {
    pub(crate) fn new(mtm: MainThreadMarker) -> Self {
        let state = Rc::new(FlowState {
            children: RefCell::new(Vec::new()),
            column_spacing: Cell::new(0.0),
            row_spacing: Cell::new(0.0),
            padding: Cell::new(Padding::ZERO),
            align: Cell::new(Align::Start),
            reported_height: Cell::new(f64::NAN),
            minimum_width: RefCell::new(None),
        });
        let native = FlowView::new(mtm, state.clone());
        let minimum = native
            .widthAnchor()
            .constraintGreaterThanOrEqualToConstant(0.0);
        minimum.setPriority(MINIMUM_WIDTH_PRIORITY);
        minimum.setActive(true);
        *state.minimum_width.borrow_mut() = Some(minimum);
        let this = Self(Rc::new(FlowInner {
            native,
            state,
            children: RefCell::new(Vec::new()),
        }));
        this.relax_width();
        // 子の大きさの指定が変わったら、並べ直す。
        let weak = Rc::downgrade(&this.0);
        crate::layout::register_container(&this.0.native, move |_| {
            if let Some(inner) = weak.upgrade() {
                Flow(inner).invalidate();
            }
        });
        this
    }

    /// Sets the size. Each call discards the previous setting.
    ///
    /// With `Auto` on the width, the flow keeps a compression resistance below a
    /// window's, so it wraps rather than widening the window (see the module
    /// documentation).
    pub fn set_sizing(&self, sizing: naui_core::Sizing) {
        let view = <Self as Widget>::native_view(self);
        crate::layout::apply_sizing(&view, sizing);
        self.relax_width();
    }

    fn relax_width(&self) {
        let view: &NSView = self.0.native.as_ref();
        if !crate::layout::wants_fill(view, true) {
            view.setContentCompressionResistancePriority_forOrientation(
                WRAP_COMPRESSION_RESISTANCE,
                NSLayoutConstraintOrientation::Horizontal,
            );
        }
    }

    /// Has the flow measure and place its children again, and its parents follow.
    fn invalidate(&self) {
        crate::layout::invalidate_ancestors(&self.0.native);
    }

    pub fn set_spacing(&self, column: f64, row: f64) {
        self.0.state.column_spacing.set(column.max(0.0));
        self.0.state.row_spacing.set(row.max(0.0));
        self.invalidate();
    }

    /// The padding's `start` and `end` follow the layout direction, as the lines do.
    pub fn set_padding(&self, padding: Padding) {
        self.0.state.padding.set(padding);
        self.invalidate();
    }

    pub fn set_align(&self, align: Align) {
        if self.0.state.align.replace(align) != align {
            self.invalidate();
        }
    }

    /// Appends a child at the end.
    pub fn append(&self, child: &dyn Widget) {
        self.mount(usize::MAX, child);
    }

    /// Inserts a child at the given position. If `index` is at least the current
    /// count, it is appended at the end.
    ///
    /// The other children stay in the view hierarchy, so a field being typed into
    /// keeps its focus and IME composition.
    pub fn insert(&self, index: usize, child: &dyn Widget) {
        self.mount(index, child);
    }

    /// Removes the child at the given position. Does nothing if out of range.
    pub fn remove(&self, index: usize) {
        let child = {
            let mut children = self.0.children.borrow_mut();
            if index >= children.len() {
                return;
            }
            children.remove(index)
        };
        self.unmount(&*child);
        self.invalidate();
    }

    /// Removes every child.
    pub fn clear(&self) {
        let children = std::mem::take(&mut *self.0.children.borrow_mut());
        for child in &children {
            self.unmount(&**child);
        }
        self.invalidate();
    }

    fn mount(&self, index: usize, child: &dyn Widget) {
        let view = child.native_view();
        // 移っていった子が戻ってきたら (または同じ子をもう一度入れたら)、古い
        // 記録は捨てる。
        let mut index = index;
        let next = {
            let mut children = self.0.children.borrow_mut();
            if let Some(old) = children
                .iter()
                .position(|current| std::ptr::eq::<NSView>(&*current.native_view(), &*view))
            {
                if old < index {
                    index -= 1;
                }
                children.remove(old);
            }
            let index = index.min(children.len());
            children.insert(index, child.boxed_clone());
            let mut views = self.0.state.children.borrow_mut();
            views.retain(|current| !std::ptr::eq::<NSView>(&**current, &*view));
            views.insert(index, view.clone());
            // 描く順 (とキーボードで移る順) も並びに合わせる。後ろの子のうち、まだ
            // この中にいるものの手前へ入れる。
            views[index + 1..]
                .iter()
                .find(|next| crate::layout::is_placed_in(next, &self.0.native))
                .cloned()
        };
        // 位置は枠で決める (制約にしない)。大きさの指定 (幅の固定など) は子自身の
        // 制約として残る。
        view.setTranslatesAutoresizingMaskIntoConstraints(true);
        view.removeFromSuperview();
        match next {
            Some(next) => self.0.native.addSubview_positioned_relativeTo(
                &view,
                NSWindowOrderingMode::Below,
                Some(&next),
            ),
            None => self.0.native.addSubview(&view),
        }
        self.invalidate();
    }

    /// Takes a child out of the view hierarchy. A child that has moved to another
    /// parent is left where it is: only this flow's record of it goes.
    fn unmount(&self, child: &dyn Widget) {
        let view = child.native_view();
        self.0
            .state
            .children
            .borrow_mut()
            .retain(|current| !std::ptr::eq::<NSView>(&**current, &*view));
        if crate::layout::is_placed_in(&view, &self.0.native) {
            view.removeFromSuperview();
            crate::layout::prepare_child(&view);
        }
    }

    pub fn len(&self) -> usize {
        self.0.children.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
