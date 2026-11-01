//! A list and its detail, side by side or one at a time (`NSView` + `NSBox`).
//!
//! AppKit has no container that shows two panes side by side when wide and one at
//! a time when narrow: `NSSplitViewController` collapses a sidebar but never puts
//! the detail in its place, and the navigation stacks that do (`UISplitViewController`)
//! are UIKit's. So naui decides, with naui-core's [`PanesArrangement`]: it watches
//! the container's frame (`NSViewFrameDidChangeNotification`) and, when the width
//! crosses the breakpoint, switches on the next iteration of the main queue rather
//! than in the middle of the layout pass that moved the frame (the same as
//! `Stack::set_breakpoint`).
//!
//! The parts are plain AppKit views laid out by Auto Layout:
//!
//! - Side by side, the list pane has the fixed width, a separator (`NSBox`, the
//!   same line as a thin `NSSplitView` divider) follows it, and the detail pane
//!   takes the rest.
//! - One at a time, both panes span the whole width and the one not shown is
//!   hidden. Keeping the hidden pane laid out at the same width keeps the
//!   container's minimum width the same whichever pane is shown, and keeps its
//!   content (wrapped text, scroll positions) as it was.
//! - While only the detail is shown, a back button (`chevron.backward`, the symbol
//!   of the back buttons in System Settings' toolbar) sits above it.
//!
//! The panes are views naui owns, so hiding them does not touch the `hidden` the app
//! sets on its widgets with `set_visible`.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use dispatch2::{DispatchQueue, MainThreadBound};
use naui_core::{ui_strings, Pane, PanesArrangement, DEFAULT_LIST_PANE_WIDTH};
use objc2::rc::Retained;
use objc2::{sel, MainThreadMarker, Message};
use objc2_app_kit::{
    NSBox, NSBoxType, NSButton, NSCellImagePosition, NSImage, NSLayoutConstraint, NSLayoutPriority,
    NSView, NSViewFrameDidChangeNotification,
};
use objc2_foundation::{NSArray, NSNotificationCenter, NSString};

use crate::trampoline::{ActionTarget, ValueHandler};
use crate::widgets::{impl_widget, Widget};

/// The margin around the back button, and between it and the detail.
const BACK_MARGIN: f64 = 8.0;

/// The back button's symbol (SF Symbols).
const BACK_SYMBOL: &str = "chevron.backward";

/// The priority of the list pane's width while both panes are shown. Just below
/// required, so a list whose content needs more width than that widens the pane
/// instead of breaking the constraints.
const LIST_WIDTH_PRIORITY: NSLayoutPriority = 999.0;

/// One pane: a view naui owns, with the app's widget pinned to its edges.
struct PaneSlot {
    view: Retained<NSView>,
    /// The widget's handle. Held to keep it alive along with its trampolines.
    child: RefCell<Option<Box<dyn Widget>>>,
    constraints: RefCell<Vec<Retained<NSLayoutConstraint>>>,
}

impl PaneSlot {
    fn new(mtm: MainThreadMarker) -> Self {
        let view = NSView::new(mtm);
        crate::layout::prepare_child(&view);
        Self {
            view,
            child: RefCell::new(None),
            constraints: RefCell::new(Vec::new()),
        }
    }

    /// Replaces the widget, pinning the new one to all four edges.
    fn set(&self, child: &dyn Widget) {
        let previous = self.child.borrow_mut().take();
        let constraints = std::mem::take(&mut *self.constraints.borrow_mut());
        if !constraints.is_empty() {
            NSLayoutConstraint::deactivateConstraints(&NSArray::from_retained_slice(&constraints));
        }
        if let Some(previous) = previous {
            previous.native_view().removeFromSuperview();
        }

        let view = child.native_view();
        crate::layout::prepare_child(&view);
        self.view.addSubview(&view);
        let pane = &self.view;
        let constraints = [
            view.leadingAnchor()
                .constraintEqualToAnchor(&pane.leadingAnchor()),
            view.trailingAnchor()
                .constraintEqualToAnchor(&pane.trailingAnchor()),
            view.topAnchor().constraintEqualToAnchor(&pane.topAnchor()),
            view.bottomAnchor()
                .constraintEqualToAnchor(&pane.bottomAnchor()),
        ];
        NSLayoutConstraint::activateConstraints(&NSArray::from_retained_slice(&constraints));
        *self.constraints.borrow_mut() = constraints.into();
        *self.child.borrow_mut() = Some(child.boxed_clone());
    }

    fn child_view(&self) -> Option<Retained<NSView>> {
        self.child
            .borrow()
            .as_ref()
            .map(|child| child.native_view())
    }
}

/// The constraints of each arrangement. Only one set is active at a time.
struct Arrangements {
    /// Both panes side by side.
    wide: Vec<Retained<NSLayoutConstraint>>,
    /// Both panes spanning the whole width, one of them hidden.
    narrow: Vec<Retained<NSLayoutConstraint>>,
    /// The detail directly below the pane's top edge.
    without_back: Retained<NSLayoutConstraint>,
    /// The detail below the back button.
    with_back: Retained<NSLayoutConstraint>,
}

struct AdaptivePanesInner {
    native: Retained<NSView>,
    list: PaneSlot,
    /// The back button and, below it, the detail pane.
    detail_side: Retained<NSView>,
    detail: PaneSlot,
    separator: Retained<NSBox>,
    back: Retained<NSButton>,
    list_width: Retained<NSLayoutConstraint>,
    arrangements: Arrangements,
    /// The breakpoint, the last width, and the pane shown while only one is.
    arrangement: Cell<PanesArrangement>,
    /// Applying the collapsed state `arrangement` decided is queued on the main queue.
    switch_scheduled: Cell<bool>,
    on_back: ValueHandler<()>,
    /// AppKit targets are weak references, so they are kept alive here.
    back_target: RefCell<Option<Retained<ActionTarget>>>,
    frame_target: RefCell<Option<Retained<ActionTarget>>>,
}

impl Drop for AdaptivePanesInner {
    fn drop(&mut self) {
        if let Some(target) = self.frame_target.get_mut().take() {
            unsafe { NSNotificationCenter::defaultCenter().removeObserver(&target) };
        }
    }
}

/// A list and its detail, side by side when wide and one at a time when narrow.
#[derive(Clone)]
pub struct AdaptivePanes(Rc<AdaptivePanesInner>);
impl_widget!(AdaptivePanes);

impl AdaptivePanes {
    pub(crate) fn new(mtm: MainThreadMarker) -> Self {
        let native = NSView::new(mtm);
        let list = PaneSlot::new(mtm);
        let detail = PaneSlot::new(mtm);
        let detail_side = NSView::new(mtm);
        crate::layout::prepare_child(&detail_side);

        let separator = NSBox::new(mtm);
        separator.setBoxType(NSBoxType::Separator);
        crate::layout::prepare_child(&separator);

        let image = NSImage::imageWithSystemSymbolName_accessibilityDescription(
            &NSString::from_str(BACK_SYMBOL),
            None,
        );
        let back = unsafe {
            NSButton::buttonWithTitle_target_action(
                &NSString::from_str(ui_strings().back),
                None,
                None,
                mtm,
            )
        };
        back.setImage(image.as_deref());
        back.setImagePosition(NSCellImagePosition::ImageLeading);
        crate::layout::prepare_child(&back);

        native.addSubview(&list.view);
        native.addSubview(&separator);
        native.addSubview(&detail_side);
        detail_side.addSubview(&back);
        detail_side.addSubview(&detail.view);

        let list_width = list
            .view
            .widthAnchor()
            .constraintEqualToConstant(DEFAULT_LIST_PANE_WIDTH);
        list_width.setPriority(LIST_WIDTH_PRIORITY);

        // どの並びでも変わらないもの。
        let mut fixed = Vec::new();
        for view in [&*list.view, &*separator, &*detail_side] {
            fixed.push(
                view.topAnchor()
                    .constraintEqualToAnchor(&native.topAnchor()),
            );
            fixed.push(
                view.bottomAnchor()
                    .constraintEqualToAnchor(&native.bottomAnchor()),
            );
        }
        fixed.push(separator.widthAnchor().constraintEqualToConstant(1.0));
        fixed.push(
            separator
                .leadingAnchor()
                .constraintEqualToAnchor(&list.view.trailingAnchor()),
        );
        fixed.push(
            list.view
                .leadingAnchor()
                .constraintEqualToAnchor(&native.leadingAnchor()),
        );
        fixed.push(
            detail_side
                .trailingAnchor()
                .constraintEqualToAnchor(&native.trailingAnchor()),
        );
        fixed.push(
            back.leadingAnchor()
                .constraintEqualToAnchor_constant(&detail_side.leadingAnchor(), BACK_MARGIN),
        );
        fixed.push(
            back.topAnchor()
                .constraintEqualToAnchor_constant(&detail_side.topAnchor(), BACK_MARGIN),
        );
        fixed.push(
            detail
                .view
                .leadingAnchor()
                .constraintEqualToAnchor(&detail_side.leadingAnchor()),
        );
        fixed.push(
            detail
                .view
                .trailingAnchor()
                .constraintEqualToAnchor(&detail_side.trailingAnchor()),
        );
        fixed.push(
            detail
                .view
                .bottomAnchor()
                .constraintEqualToAnchor(&detail_side.bottomAnchor()),
        );
        NSLayoutConstraint::activateConstraints(&NSArray::from_retained_slice(&fixed));

        let arrangements = Arrangements {
            wide: vec![
                list_width.clone(),
                detail_side
                    .leadingAnchor()
                    .constraintEqualToAnchor(&separator.trailingAnchor()),
            ],
            narrow: vec![
                list.view
                    .trailingAnchor()
                    .constraintEqualToAnchor(&native.trailingAnchor()),
                detail_side
                    .leadingAnchor()
                    .constraintEqualToAnchor(&native.leadingAnchor()),
            ],
            without_back: detail
                .view
                .topAnchor()
                .constraintEqualToAnchor(&detail_side.topAnchor()),
            with_back: detail
                .view
                .topAnchor()
                .constraintEqualToAnchor_constant(&back.bottomAnchor(), BACK_MARGIN),
        };

        let this = Self(Rc::new(AdaptivePanesInner {
            native,
            list,
            detail_side,
            detail,
            separator,
            back,
            list_width,
            arrangements,
            arrangement: Cell::new(PanesArrangement::default()),
            switch_scheduled: Cell::new(false),
            on_back: ValueHandler::default(),
            back_target: RefCell::new(None),
            frame_target: RefCell::new(None),
        }));
        this.0.apply();

        // 中継はハンドルと同じ寿命で持つ (通知の中から戻っても、実行中の
        // 中継そのものを解放しないようにするため)。
        let target = ActionTarget::new(mtm, {
            let weak = Rc::downgrade(&this.0);
            move || {
                if let Some(inner) = weak.upgrade() {
                    AdaptivePanes(inner).go_back();
                }
            }
        });
        unsafe {
            this.0.back.setTarget(Some(&target));
            this.0.back.setAction(Some(sel!(invoke:)));
        }
        *this.0.back_target.borrow_mut() = Some(target);
        this.watch_frame(mtm);
        this
    }

    /// The list pane. Each call replaces it.
    pub fn set_list(&self, child: &dyn Widget) {
        self.0.list.set(child);
    }

    /// The detail pane. Each call replaces it.
    pub fn set_detail(&self, child: &dyn Widget) {
        self.0.detail.set(child);
    }

    /// The narrowest width at which both panes are shown.
    pub fn set_breakpoint(&self, width: f64) {
        let mut arrangement = self.0.arrangement.get();
        arrangement.resized(self.0.native.frame().size.width);
        arrangement.set_breakpoint(width);
        self.0.arrangement.set(arrangement);
        self.0.apply();
    }

    pub fn breakpoint(&self) -> f64 {
        self.0.arrangement.get().breakpoint()
    }

    /// The list pane's width while both panes are shown.
    pub fn set_list_width(&self, width: f64) {
        self.0.list_width.setConstant(width.max(0.0));
    }

    pub fn list_width(&self) -> f64 {
        self.0.list_width.constant()
    }

    /// Whether only one pane is shown.
    pub fn is_collapsed(&self) -> bool {
        self.0.arrangement.get().is_collapsed()
    }

    /// The pane shown while only one is.
    pub fn visible_pane(&self) -> Pane {
        self.0.arrangement.get().visible()
    }

    /// Chooses the pane shown while only one is. `on_back` is not called.
    pub fn show(&self, pane: Pane) {
        let mut arrangement = self.0.arrangement.get();
        if arrangement.show(pane) {
            self.0.arrangement.set(arrangement);
            self.0.apply();
        }
    }

    /// Goes back to the list as if the user had pressed the back button. Calls
    /// `on_back`, even when the list is already shown.
    pub fn go_back(&self) {
        self.show(Pane::List);
        self.0.on_back.emit(());
    }

    /// Called when the user goes back from the detail to the list.
    pub fn on_back(&self, mut f: impl FnMut() + 'static) {
        self.0.on_back.set(move |()| f());
    }

    /// The back button. Public as a backend-specific escape hatch.
    pub fn native_back_button(&self) -> Retained<NSButton> {
        self.0.back.clone()
    }

    /// Starts telling `arrangement` the width after each change of the frame.
    fn watch_frame(&self, mtm: MainThreadMarker) {
        let target = ActionTarget::new(mtm, {
            let weak = Rc::downgrade(&self.0);
            move || {
                if let Some(inner) = weak.upgrade() {
                    AdaptivePanes(inner).frame_changed();
                }
            }
        });
        self.0.native.setPostsFrameChangedNotifications(true);
        unsafe {
            NSNotificationCenter::defaultCenter().addObserver_selector_name_object(
                &target,
                sel!(invoke:),
                Some(NSViewFrameDidChangeNotification),
                Some(&self.0.native),
            );
        }
        *self.0.frame_target.borrow_mut() = Some(target);
    }

    /// The frame changed, in the middle of a layout pass. If the width crossed the
    /// breakpoint, switches once the pass is over.
    fn frame_changed(&self) {
        let mut arrangement = self.0.arrangement.get();
        let changed = arrangement.resized(self.0.native.frame().size.width);
        self.0.arrangement.set(arrangement);
        if !changed || self.0.switch_scheduled.replace(true) {
            return;
        }
        let mtm = MainThreadMarker::from(&*self.0.native);
        let weak = MainThreadBound::new(Rc::downgrade(&self.0), mtm);
        DispatchQueue::main().exec_async(move || {
            // メインキューの仕事はメインスレッドで走る。
            let mtm = MainThreadMarker::new().expect("called outside the main queue");
            if let Some(inner) = weak.into_inner(mtm).upgrade() {
                inner.switch_scheduled.set(false);
                // 予約のあとに幅が戻っていれば、いまの並びのままになる。
                inner.apply();
            }
        });
    }
}

impl AdaptivePanesInner {
    /// Makes the views match `arrangement`.
    fn apply(&self) {
        let arrangement = self.arrangement.get();
        let collapsed = arrangement.is_collapsed();
        let a = &self.arrangements;
        let (off, on) = if collapsed {
            (&a.wide, &a.narrow)
        } else {
            (&a.narrow, &a.wide)
        };
        NSLayoutConstraint::deactivateConstraints(&NSArray::from_retained_slice(off));
        NSLayoutConstraint::activateConstraints(&NSArray::from_retained_slice(on));
        let back = arrangement.shows_back();
        if back {
            a.without_back.setActive(false);
            a.with_back.setActive(true);
        } else {
            a.with_back.setActive(false);
            a.without_back.setActive(true);
        }

        let shows_list = arrangement.shows(Pane::List);
        let shows_detail = arrangement.shows(Pane::Detail);
        // 隠す区画にフォーカスがあれば、出す区画へ移す (戻るボタンで戻ったときも、
        // ボタンごと隠れるので一覧へ移る)。
        let leaving = if !shows_list && !self.list.view.isHidden() {
            Some((&*self.list.view, &self.detail))
        } else if !shows_detail && !self.detail_side.isHidden() {
            Some((&*self.detail_side, &self.list))
        } else {
            None
        };
        let refocus = leaving.and_then(|(view, to)| holds_focus(view).then_some(to));

        self.list.view.setHidden(!shows_list);
        self.detail_side.setHidden(!shows_detail);
        self.separator.setHidden(collapsed);
        self.back.setHidden(!back);

        if let Some(to) = refocus {
            let moved = to
                .child_view()
                .is_some_and(|view| crate::interaction::request_focus(&view));
            if !moved {
                if let Some(window) = self.native.window() {
                    window.makeFirstResponder(None);
                }
            }
        }
        // 1 区画と 2 区画で最小の幅が変わるので、親の連なりへ伝える。
        crate::layout::invalidate_ancestors(&self.native);
    }
}

/// Whether the window's first responder is `view` or inside it.
fn holds_focus(view: &NSView) -> bool {
    let Some(window) = view.window() else {
        return false;
    };
    let Some(responder) = window.firstResponder() else {
        return false;
    };
    let Ok(focused) = responder.downcast::<NSView>() else {
        return false;
    };
    // 編集中の入力欄では、フィールドエディタが入力欄の子孫として入っている。
    focused.isDescendantOf(view)
}
