//! Navigation handles (AppKit).
//!
//! Tabs, nav bars, docks, menus, breadcrumbs, pagination and links all share the
//! same structure: "a sequence of items + the one currently selected". So their
//! public APIs have the same shape, and on the AppKit side they map as follows:
//!
//! | naui | AppKit |
//! | --- | --- |
//! | `Tabs` | `NSTabView` + `NSTabViewItem` |
//! | `Navbar` | `NSTextField` (heading) + `NSSegmentedControl` |
//! | `Dock` | `NSSegmentedControl` (equal widths) |
//! | `Menu` | A vertical stack of `NSButton`s (AccessoryBar) |
//! | `Breadcrumbs` | `NSPathControl` + `NSPathControlItem` |
//! | `Pagination` | `NSButton` + `NSSegmentedControl` |
//! | `Link` | `NSButton` (borderless, link color) + `NSWorkspace` |

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use naui_core::NavItem;
use objc2::rc::Retained;
use objc2::runtime::{NSObjectProtocol, ProtocolObject};
use objc2::{define_class, msg_send, sel, MainThreadMarker, MainThreadOnly, Message};
use objc2_app_kit::{
    NSApplication, NSBezelStyle, NSButton, NSButtonType, NSColor, NSControlStateValueOff,
    NSControlStateValueOn, NSLayoutAttribute, NSLayoutConstraintOrientation, NSPathControl,
    NSPathControlItem, NSPathStyle, NSSegmentDistribution, NSSegmentStyle, NSSegmentSwitchTracking,
    NSSegmentedControl, NSStackView, NSStackViewDistribution, NSTabView, NSTabViewItem,
    NSTextField, NSUserInterfaceLayoutOrientation, NSView, NSWorkspace,
};
use objc2_foundation::{NSArray, NSEdgeInsets, NSString, NSURL};

use crate::trampoline::{ActionTarget, SelectHandler, TabObserver};
use crate::widgets::{impl_sizing, impl_widget, Widget};

/// Creates a horizontal stack. Used to compose navigation widgets.
fn row(mtm: MainThreadMarker, spacing: f64) -> Retained<NSStackView> {
    let stack = NSStackView::new(mtm);
    stack.setOrientation(NSUserInterfaceLayoutOrientation::Horizontal);
    stack.setDistribution(NSStackViewDistribution::GravityAreas);
    stack.setAlignment(NSLayoutAttribute::CenterY);
    stack.setSpacing(spacing);
    stack
}

/// Hands no leftover space to the Auto children of the inner row, letting it
/// escape to the end.
fn finish_row(stack: &NSStackView, mtm: MainThreadMarker) {
    let tail = NSView::new(mtm);
    crate::layout::prepare_child(&tail);
    for orientation in [
        NSLayoutConstraintOrientation::Horizontal,
        NSLayoutConstraintOrientation::Vertical,
    ] {
        tail.setContentHuggingPriority_forOrientation(2.0, orientation);
        tail.setContentCompressionResistancePriority_forOrientation(1.0, orientation);
    }

    let arranged = stack.arrangedSubviews();
    if !arranged.is_empty() {
        for index in 0..arranged.len() {
            let child = arranged.objectAtIndex(index);
            if !crate::layout::wants_fill(&child, true) {
                crate::layout::keep_auto_size(&child, true);
            }
        }
        let last = arranged.objectAtIndex(arranged.len() - 1);
        stack.setCustomSpacing_afterView(0.0, &last);
    }
    stack.addArrangedSubview(&tail);
}

// --------------------------------------------------------------- Segments

/// An internal handle wrapping `NSSegmentedControl`.
///
/// Shared by nav bars, docks and pagination. It is an `Rc`, so it can be cloned into
/// button closures.
#[derive(Clone)]
struct Segments(Rc<SegmentsInner>);

struct SegmentsInner {
    native: Retained<NSSegmentedControl>,
    handler: SelectHandler,
    count: Cell<usize>,
    /// The trampoline that receives clicks. AppKit targets are weak, so it is held.
    _target: Retained<ActionTarget>,
}

impl Segments {
    fn new(mtm: MainThreadMarker, distribution: NSSegmentDistribution) -> Self {
        let native = NSSegmentedControl::new(mtm);
        native.setSegmentStyle(NSSegmentStyle::Automatic);
        native.setTrackingMode(NSSegmentSwitchTracking::SelectOne);
        native.setSegmentDistribution(distribution);
        native.setSegmentCount(0);

        let handler = SelectHandler::default();
        let target = ActionTarget::new(mtm, {
            let native = native.clone();
            let handler = handler.clone();
            move || {
                let index = native.selectedSegment();
                if index >= 0 {
                    handler.emit(index as usize);
                }
            }
        });
        unsafe {
            native.setTarget(Some(&target));
            native.setAction(Some(sel!(invoke:)));
        }

        Self(Rc::new(SegmentsInner {
            native,
            handler,
            count: Cell::new(0),
            _target: target,
        }))
    }

    fn view(&self) -> Retained<NSView> {
        let view: &NSView = self.0.native.as_ref();
        view.retain()
    }

    fn set_items(&self, items: &[NavItem]) {
        let native = &self.0.native;
        native.setSegmentCount(items.len() as isize);
        for (i, item) in items.iter().enumerate() {
            native.setLabel_forSegment(&NSString::from_str(&item.label), i as isize);
            native.setEnabled_forSegment(item.enabled, i as isize);
        }
        self.0.count.set(items.len());
        // 項目が変わればインデックスの意味も変わるので、選択は必ず外す。
        native.setSelectedSegment(-1);
    }

    fn len(&self) -> usize {
        self.0.count.get()
    }

    fn selected(&self) -> Option<usize> {
        let index = self.0.native.selectedSegment();
        (index >= 0).then_some(index as usize)
    }

    /// Changes the selection without notifying.
    fn set_selected(&self, index: usize) {
        if index < self.len() {
            self.0.native.setSelectedSegment(index as isize);
        }
    }

    /// Changes the selection through the same path as a user's choice (notifies).
    fn select(&self, index: usize) {
        if index < self.len() {
            self.0.native.setSelectedSegment(index as isize);
            self.0.handler.emit(index);
        }
    }

    fn on_select(&self, f: impl FnMut(usize) + 'static) {
        self.0.handler.set(f);
    }
}

// ------------------------------------------------------------------- Tabs

struct TabsInner {
    native: Retained<NSTabView>,
    /// Holds the children's handles, keeping them alive along with their trampolines.
    children: RefCell<Vec<Box<dyn Widget>>>,
    handler: SelectHandler,
    /// Stops notifications only during `set_selected`, since AppKit calls the delegate
    /// even for selection from code.
    silent: Rc<Cell<bool>>,
    /// The delegate is a weak reference, so it is held.
    _observer: Retained<TabObserver>,
}

/// Tabs (NSTabView). Holds each tab's content widget.
#[derive(Clone)]
pub struct Tabs(Rc<TabsInner>);
impl_widget!(Tabs);

impl Tabs {
    pub(crate) fn new(mtm: MainThreadMarker) -> Self {
        let native = NSTabView::new(mtm);
        let handler = SelectHandler::default();
        let silent = Rc::new(Cell::new(false));

        // デリゲートから来た選択を、通知を止めている間は捨てる。
        let bridge = SelectHandler::default();
        bridge.set({
            let handler = handler.clone();
            let silent = silent.clone();
            move |index| {
                if !silent.get() {
                    handler.emit(index);
                }
            }
        });
        let observer = TabObserver::new(mtm, bridge);
        native.setDelegate(Some(ProtocolObject::from_ref(&*observer)));

        Self(Rc::new(TabsInner {
            native,
            children: RefCell::new(Vec::new()),
            handler,
            silent,
            _observer: observer,
        }))
    }

    /// Adds one tab. `child` becomes that tab's content.
    pub fn add_tab(&self, label: &str, child: &dyn Widget) {
        let item = NSTabViewItem::new();
        item.setLabel(&NSString::from_str(label));
        item.setView(Some(&child.native_view()));
        self.0.native.addTabViewItem(&item);
        self.0.children.borrow_mut().push(child.boxed_clone());
    }

    /// Removes one tab. Does nothing if out of range.
    ///
    /// When the selected tab is removed, the platform selects a nearby tab instead.
    /// Like [`set_selected`](Tabs::set_selected), this move is not notified.
    pub fn remove_tab(&self, index: usize) {
        if index >= self.len() {
            return;
        }
        let selected = self.selected();
        let item = self.0.native.tabViewItemAtIndex(index as isize);
        let previous = self.0.silent.replace(true);
        self.0.native.removeTabViewItem(&item);
        self.0.children.borrow_mut().remove(index);
        // 選択の寄せ先は環境任せにせず、4 バックエンドで同じ形にそろえる。
        let left = self.len();
        let selected = match selected {
            _ if left == 0 => None,
            Some(current) if current == index => Some(index.min(left - 1)),
            Some(current) if current > index => Some(current - 1),
            other => other,
        };
        if let Some(selected) = selected {
            self.0.native.selectTabViewItemAtIndex(selected as isize);
        }
        self.0.silent.set(previous);
    }

    /// Removes every tab.
    pub fn clear(&self) {
        let previous = self.0.silent.replace(true);
        while self.0.native.numberOfTabViewItems() > 0 {
            let item = self.0.native.tabViewItemAtIndex(0);
            self.0.native.removeTabViewItem(&item);
        }
        self.0.silent.set(previous);
        self.0.children.borrow_mut().clear();
    }

    pub fn len(&self) -> usize {
        self.0.children.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn selected(&self) -> Option<usize> {
        let item = self.0.native.selectedTabViewItem()?;
        let index = self.0.native.indexOfTabViewItem(&item);
        (index >= 0).then_some(index as usize)
    }

    /// Changes the selection without notifying.
    pub fn set_selected(&self, index: usize) {
        if index < self.len() {
            self.0.silent.set(true);
            self.0.native.selectTabViewItemAtIndex(index as isize);
            self.0.silent.set(false);
        }
    }

    /// Changes the selection through the same path as a user's choice (notifies).
    pub fn select(&self, index: usize) {
        if index < self.len() {
            // 同じタブを選び直したときは AppKit がデリゲートを呼ばないため、
            // デリゲート経由の通知は止めて、ここで 1 回だけ通知する。
            self.0.silent.set(true);
            self.0.native.selectTabViewItemAtIndex(index as isize);
            self.0.silent.set(false);
            self.0.handler.emit(index);
        }
    }

    /// Called with the index when the tab changes.
    pub fn on_select(&self, f: impl FnMut(usize) + 'static) {
        self.0.handler.set(f);
    }
}

// ----------------------------------------------------------------- Navbar

struct NavbarInner {
    native: Retained<NSStackView>,
    title: Retained<NSTextField>,
    segments: Segments,
}

/// Horizontal navigation placed at the top of the screen.
///
/// A heading (`NSTextField`) and items (`NSSegmentedControl`) are laid out in a
/// horizontal stack.
#[derive(Clone)]
pub struct Navbar(Rc<NavbarInner>);
impl_widget!(Navbar);

impl Navbar {
    pub(crate) fn new(mtm: MainThreadMarker, title: &str) -> Self {
        let native = row(mtm, 12.0);
        let title_field = NSTextField::labelWithString(&NSString::from_str(title), mtm);
        let segments = Segments::new(mtm, NSSegmentDistribution::Fit);
        native.addArrangedSubview(&title_field);
        native.addArrangedSubview(&segments.view());
        finish_row(&native, mtm);
        Self(Rc::new(NavbarInner {
            native,
            title: title_field,
            segments,
        }))
    }

    /// The heading at the left end.
    pub fn set_title(&self, title: &str) {
        self.0.title.setStringValue(&NSString::from_str(title));
    }

    pub fn title(&self) -> String {
        self.0.title.stringValue().to_string()
    }

    /// Rebuilds the items. Indices change meaning, so the selection is cleared.
    pub fn set_items(&self, items: &[NavItem]) {
        self.0.segments.set_items(items);
    }

    pub fn len(&self) -> usize {
        self.0.segments.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn selected(&self) -> Option<usize> {
        self.0.segments.selected()
    }

    /// Changes the selection without notifying.
    pub fn set_selected(&self, index: usize) {
        self.0.segments.set_selected(index);
    }

    /// Changes the selection through the same path as a user's choice (notifies).
    pub fn select(&self, index: usize) {
        self.0.segments.select(index);
    }

    /// Called with the index when an item is chosen.
    pub fn on_select(&self, f: impl FnMut(usize) + 'static) {
        self.0.segments.on_select(f);
    }
}

// ------------------------------------------------------------------- Dock

struct DockInner {
    segments: Segments,
}

/// Horizontal navigation placed at the bottom of the screen (equal widths).
///
/// **Placement is the app's job.** Put it last in a vertical stack and give the
/// element before it [`naui_core::Length::Fill`] or a `Spacer` to push it to the
/// bottom.
#[derive(Clone)]
pub struct Dock(Rc<DockInner>);

impl Widget for Dock {
    fn native_view(&self) -> Retained<NSView> {
        self.0.segments.view()
    }
    fn boxed_clone(&self) -> Box<dyn Widget> {
        Box::new(self.clone())
    }
}

impl_sizing!(Dock);

impl Dock {
    pub(crate) fn new(mtm: MainThreadMarker) -> Self {
        let segments = Segments::new(mtm, NSSegmentDistribution::FillEqually);
        Self(Rc::new(DockInner { segments }))
    }

    /// Rebuilds the items. Indices change meaning, so the selection is cleared.
    pub fn set_items(&self, items: &[NavItem]) {
        self.0.segments.set_items(items);
    }

    pub fn len(&self) -> usize {
        self.0.segments.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn selected(&self) -> Option<usize> {
        self.0.segments.selected()
    }

    /// Changes the selection without notifying.
    pub fn set_selected(&self, index: usize) {
        self.0.segments.set_selected(index);
    }

    /// Changes the selection through the same path as a user's choice (notifies).
    pub fn select(&self, index: usize) {
        self.0.segments.select(index);
    }

    /// Called with the index when an item is chosen.
    pub fn on_select(&self, f: impl FnMut(usize) + 'static) {
        self.0.segments.on_select(f);
    }
}

// ------------------------------------------------------------------- Menu

struct MenuInner {
    native: Retained<NSStackView>,
    buttons: RefCell<Vec<Retained<NSButton>>>,
    targets: RefCell<Vec<Retained<ActionTarget>>>,
    handler: SelectHandler,
    selected: Cell<Option<usize>>,
}

/// A vertical list of navigation items.
///
/// As in macOS sidebars, borderless buttons with a selected state (`NSButton`
/// AccessoryBar) are stacked vertically in an `NSStackView`. Note that it is not a
/// popup `NSMenu`.
#[derive(Clone)]
pub struct Menu(Rc<MenuInner>);
impl_widget!(Menu);

impl Menu {
    pub(crate) fn new(mtm: MainThreadMarker) -> Self {
        let native = NSStackView::new(mtm);
        native.setOrientation(NSUserInterfaceLayoutOrientation::Vertical);
        native.setDistribution(NSStackViewDistribution::Fill);
        native.setAlignment(NSLayoutAttribute::Leading);
        native.setSpacing(2.0);
        Self(Rc::new(MenuInner {
            native,
            buttons: RefCell::new(Vec::new()),
            targets: RefCell::new(Vec::new()),
            handler: SelectHandler::default(),
            selected: Cell::new(None),
        }))
    }

    /// Rebuilds the items. The previous items are removed and the selection is cleared.
    pub fn set_items(&self, items: &[NavItem]) {
        let mtm = MainThreadMarker::from(&*self.0.native);
        for button in self.0.buttons.borrow_mut().drain(..) {
            let view: &NSView = button.as_ref();
            self.0.native.removeArrangedSubview(view);
            view.removeFromSuperview();
        }
        self.0.targets.borrow_mut().clear();

        let mut buttons = Vec::with_capacity(items.len());
        let mut targets = Vec::with_capacity(items.len());
        for (index, item) in items.iter().enumerate() {
            let button = unsafe {
                NSButton::buttonWithTitle_target_action(
                    &NSString::from_str(&item.label),
                    None,
                    None,
                    mtm,
                )
            };
            button.setBezelStyle(NSBezelStyle::AccessoryBar);
            button.setButtonType(NSButtonType::PushOnPushOff);
            button.setEnabled(item.enabled);
            // ハンドルを強く持つとトランポリンとの間で循環するため、弱参照にする。
            let target = ActionTarget::new(mtm, {
                let weak = Rc::downgrade(&self.0);
                move || {
                    let Some(inner) = weak.upgrade() else {
                        return;
                    };
                    let menu = Menu(inner);
                    menu.mark_selected(Some(index));
                    menu.0.handler.emit(index);
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

        // 項目を作り直したので、選択は一度外す。
        self.0.selected.set(None);
        self.mark_selected(None);
    }

    /// Matches the buttons' pressed state to the selection.
    fn mark_selected(&self, index: Option<usize>) {
        for (i, button) in self.0.buttons.borrow().iter().enumerate() {
            let state = if Some(i) == index {
                NSControlStateValueOn
            } else {
                NSControlStateValueOff
            };
            button.setState(state);
        }
        self.0.selected.set(index);
    }

    pub fn len(&self) -> usize {
        self.0.buttons.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn selected(&self) -> Option<usize> {
        self.0.selected.get()
    }

    /// Changes the selection without notifying.
    pub fn set_selected(&self, index: usize) {
        if index < self.len() {
            self.mark_selected(Some(index));
        }
    }

    /// Changes the selection through the same path as a user's choice (notifies).
    pub fn select(&self, index: usize) {
        if index < self.len() {
            self.mark_selected(Some(index));
            self.0.handler.emit(index);
        }
    }

    /// Called with the index when an item is chosen.
    pub fn on_select(&self, f: impl FnMut(usize) + 'static) {
        self.0.handler.set(f);
    }
}

// ------------------------------------------------------------ Breadcrumbs

/// The inset from the left edge of a standard-style `NSPathControl`'s frame to
/// where it starts drawing text.
///
/// This width is returned in `alignmentRectInsets`, so that Auto Layout aligns by
/// **the text's position** (the same mechanism by which `NSTextField` labels shift
/// by 2pt). Without it, it looks shifted right of the labels in the same column.
///
/// AppKit does not publish the value, so it was decided by capturing the Gallery
/// and counting to the text's left edge (an 8px = 4pt shift). If it drifts, measure
/// again the same way.
const TEXT_INSET: f64 = 4.0;

define_class!(
    #[unsafe(super(NSPathControl))]
    #[thread_kind = MainThreadOnly]
    #[name = "NauiBreadcrumbs"]
    #[ivars = ()]
    struct BreadcrumbsControl;

    unsafe impl NSObjectProtocol for BreadcrumbsControl {}

    impl BreadcrumbsControl {
        /// Alignment uses the text's left edge, not the frame ([`TEXT_INSET`]).
        #[unsafe(method(alignmentRectInsets))]
        fn alignment_rect_insets(&self) -> NSEdgeInsets {
            NSEdgeInsets {
                top: 0.0,
                left: TEXT_INSET,
                bottom: 0.0,
                right: 0.0,
            }
        }

        /// Does not take keyboard focus on click.
        ///
        /// `NSPathControl`'s `acceptsFirstResponder` is always true, so once clicked a blue
        /// focus ring stays and never goes away. Breadcrumbs are navigation that just
        /// changes the screen when clicked, so like `NSButton` they take focus **only when
        /// the keyboard navigation setting is on**.
        #[unsafe(method(acceptsFirstResponder))]
        fn accepts_first_responder(&self) -> bool {
            let mtm = MainThreadMarker::from(self);
            NSApplication::sharedApplication(mtm).isFullKeyboardAccessEnabled()
        }
    }
);

impl BreadcrumbsControl {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        unsafe { msg_send![super(this), init] }
    }
}

struct BreadcrumbsInner {
    native: Retained<BreadcrumbsControl>,
    /// Shared with the trampoline to find the position of the clicked item.
    items: Rc<RefCell<Vec<Retained<NSPathControlItem>>>>,
    handler: SelectHandler,
    selected: Cell<Option<usize>>,
    _target: Retained<ActionTarget>,
}

/// Breadcrumbs (NSPathControl).
///
/// Lays out the current position in a hierarchy from left to right. Clicking an
/// item calls [`Breadcrumbs::on_select`] with its index.
#[derive(Clone)]
pub struct Breadcrumbs(Rc<BreadcrumbsInner>);

// `NSPathControl` を継いだ自前のクラスなので、`NSView` までは自分で降りる。
impl Widget for Breadcrumbs {
    fn native_view(&self) -> Retained<NSView> {
        self.0.native.clone().into_super().into_super().into_super()
    }

    fn boxed_clone(&self) -> Box<dyn Widget> {
        Box::new(self.clone())
    }
}

impl_sizing!(Breadcrumbs);

impl Breadcrumbs {
    pub(crate) fn new(mtm: MainThreadMarker) -> Self {
        let native = BreadcrumbsControl::new(mtm);
        native.setPathStyle(NSPathStyle::Standard);
        native.setEditable(false);

        let handler = SelectHandler::default();
        let items: Rc<RefCell<Vec<Retained<NSPathControlItem>>>> =
            Rc::new(RefCell::new(Vec::new()));
        let target = ActionTarget::new(mtm, {
            let native = native.clone();
            let handler = handler.clone();
            let items = items.clone();
            move || {
                let Some(clicked) = native.clickedPathItem() else {
                    return;
                };
                let index = items
                    .borrow()
                    .iter()
                    .position(|item| std::ptr::eq(&**item, &*clicked));
                if let Some(index) = index {
                    handler.emit(index);
                }
            }
        });
        unsafe {
            native.setTarget(Some(&target));
            native.setAction(Some(sel!(invoke:)));
        }

        Self(Rc::new(BreadcrumbsInner {
            native,
            items,
            handler,
            selected: Cell::new(None),
            _target: target,
        }))
    }

    /// Rebuilds the hierarchy. The last item becomes the current position.
    pub fn set_items(&self, items: &[NavItem]) {
        let built: Vec<Retained<NSPathControlItem>> = items
            .iter()
            .map(|item| {
                let path_item = NSPathControlItem::new();
                path_item.setTitle(&NSString::from_str(&item.label));
                path_item
            })
            .collect();
        self.0
            .native
            .setPathItems(&NSArray::from_retained_slice(&built));
        *self.0.items.borrow_mut() = built;
        // 末尾がいまいる場所。
        self.0.selected.set(items.len().checked_sub(1));
    }

    pub fn len(&self) -> usize {
        self.0.items.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The current position. The last item by default.
    pub fn selected(&self) -> Option<usize> {
        self.0.selected.get()
    }

    /// Changes the current position without notifying.
    pub fn set_selected(&self, index: usize) {
        if index < self.len() {
            self.0.selected.set(Some(index));
        }
    }

    /// Changes the selection through the same path as a user's choice (notifies).
    pub fn select(&self, index: usize) {
        if index < self.len() {
            self.0.selected.set(Some(index));
            self.0.handler.emit(index);
        }
    }

    /// Called with the index when an item is chosen.
    pub fn on_select(&self, f: impl FnMut(usize) + 'static) {
        self.0.handler.set(f);
    }
}

// ------------------------------------------------------------- Pagination

struct PaginationInner {
    native: Retained<NSStackView>,
    segments: Segments,
    /// The trampolines for previous / next.
    _targets: Vec<Retained<ActionTarget>>,
}

/// Pagination.
///
/// There is no equivalent native control, so it is composed of previous / next
/// `NSButton`s and an `NSSegmentedControl` for the page numbers laid out in a row.
#[derive(Clone)]
pub struct Pagination(Rc<PaginationInner>);
impl_widget!(Pagination);

impl Pagination {
    pub(crate) fn new(mtm: MainThreadMarker, page_count: usize) -> Self {
        let native = row(mtm, 6.0);
        let segments = Segments::new(mtm, NSSegmentDistribution::Fit);

        let make_step = |title: &str, forward: bool| {
            let button = unsafe {
                NSButton::buttonWithTitle_target_action(&NSString::from_str(title), None, None, mtm)
            };
            let target = ActionTarget::new(mtm, {
                let segments = segments.clone();
                move || {
                    let current = segments.selected().unwrap_or(0);
                    let next = if forward {
                        current.saturating_add(1)
                    } else {
                        current.saturating_sub(1)
                    };
                    if next < segments.len() {
                        segments.select(next);
                    }
                }
            });
            unsafe {
                button.setTarget(Some(&target));
                button.setAction(Some(sel!(invoke:)));
            }
            (button, target)
        };

        let (prev, prev_target) = make_step("‹", false);
        let (next, next_target) = make_step("›", true);

        native.addArrangedSubview(&prev);
        native.addArrangedSubview(&segments.view());
        native.addArrangedSubview(&next);
        finish_row(&native, mtm);

        let this = Self(Rc::new(PaginationInner {
            native,
            segments,
            _targets: vec![prev_target, next_target],
        }));
        this.set_page_count(page_count);
        this
    }

    /// Changes the number of pages. The display starts at 1; the API starts at 0.
    pub fn set_page_count(&self, count: usize) {
        let items: Vec<NavItem> = (1..=count).map(|n| NavItem::new(n.to_string())).collect();
        self.0.segments.set_items(&items);
        if count > 0 {
            self.0.segments.set_selected(0);
        }
    }

    pub fn page_count(&self) -> usize {
        self.0.segments.len()
    }

    /// The current page (starting at 0).
    pub fn page(&self) -> usize {
        self.0.segments.selected().unwrap_or(0)
    }

    /// Changes the page without notifying.
    pub fn set_page(&self, page: usize) {
        self.0.segments.set_selected(page);
    }

    /// Changes the page through the same path as a user's choice (notifies).
    pub fn select(&self, page: usize) {
        self.0.segments.select(page);
    }

    /// Goes to the previous page. Does nothing on the first page.
    pub fn go_previous(&self) {
        let page = self.page();
        if page > 0 {
            self.select(page - 1);
        }
    }

    /// Goes to the next page. Does nothing on the last page.
    pub fn go_next(&self) {
        let page = self.page();
        if page + 1 < self.page_count() {
            self.select(page + 1);
        }
    }

    /// Called with the page number (starting at 0) when the page changes.
    pub fn on_change(&self, f: impl FnMut(usize) + 'static) {
        self.0.segments.on_select(f);
    }
}

// ------------------------------------------------------------------- Link

struct LinkInner {
    native: Retained<NSButton>,
    /// Shared with the trampoline, so that the result of `set_href` takes effect on
    /// click.
    href: Rc<RefCell<String>>,
    target: RefCell<Option<Retained<ActionTarget>>>,
}

/// A link.
///
/// AppKit has no dedicated link control, so it is a borderless `NSButton` in the
/// link color (`NSColor::linkColor`). Clicking it opens `href` with `NSWorkspace`,
/// and then [`Link::on_click`] is called.
#[derive(Clone)]
pub struct Link(Rc<LinkInner>);
impl_widget!(Link);

impl Link {
    pub(crate) fn new(mtm: MainThreadMarker, text: &str, href: &str) -> Self {
        let native = unsafe {
            NSButton::buttonWithTitle_target_action(&NSString::from_str(text), None, None, mtm)
        };
        native.setBordered(false);
        native.setContentTintColor(Some(&NSColor::linkColor()));

        let this = Self(Rc::new(LinkInner {
            native,
            href: Rc::new(RefCell::new(href.to_string())),
            target: RefCell::new(None),
        }));
        // href を開くだけのハンドラを既定で入れておく。
        this.on_click(|| {});
        this
    }

    pub fn text(&self) -> String {
        self.0.native.title().to_string()
    }

    pub fn set_text(&self, text: &str) {
        self.0.native.setTitle(&NSString::from_str(text));
    }

    pub fn href(&self) -> String {
        self.0.href.borrow().clone()
    }

    pub fn set_href(&self, href: &str) {
        *self.0.href.borrow_mut() = href.to_string();
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.0.native.setEnabled(enabled);
    }

    /// Called on click. Setting it again removes the previous one.
    ///
    /// If `href` is not empty, it is opened in the browser before the callback.
    pub fn on_click(&self, mut f: impl FnMut() + 'static) {
        let mtm = MainThreadMarker::from(&*self.0.native);
        let href = self.0.href.clone();
        let target = ActionTarget::new(mtm, move || {
            open_href(&href.borrow());
            f();
        });
        unsafe {
            self.0.native.setTarget(Some(&target));
            self.0.native.setAction(Some(sel!(invoke:)));
        }
        *self.0.target.borrow_mut() = Some(target);
    }

    /// Triggers a click (for tests and automation).
    pub fn click(&self) {
        unsafe { self.0.native.performClick(None) };
    }
}

fn open_href(href: &str) {
    if href.is_empty() {
        return;
    }
    if let Some(url) = NSURL::URLWithString(&NSString::from_str(href)) {
        NSWorkspace::sharedWorkspace().openURL(&url);
    }
}
