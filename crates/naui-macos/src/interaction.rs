//! Operations every widget has (show/hide / focus / tooltip).
//!
//! The target is the view placed in the parent
//! ([`Widget::native_view`](crate::Widget::native_view)). Even for widgets that
//! wrap their content in another view (the `NSScrollView` of `TextArea`, for
//! example), descendants are searched so that focus reaches the inner field.

use std::cell::RefCell;
use std::thread::LocalKey;

use objc2::rc::{Retained, Weak};
use objc2::Message;
use objc2_app_kit::NSView;
use objc2_foundation::NSString;

type HiddenViews = RefCell<Vec<Weak<NSView>>>;

thread_local! {
    /// The views the app hid with `set_visible(false)`.
    static APP_HIDDEN: HiddenViews = const { RefCell::new(Vec::new()) };
    /// The views a container is hiding (the content of a collapsed `Expander`).
    ///
    /// Both use `hidden`, so remembering only one would let the other erase it when it
    /// shows the view again. Both are remembered, and the view is hidden if either
    /// hides it.
    static CONTAINER_HIDDEN: HiddenViews = const { RefCell::new(Vec::new()) };
}

fn contains(list: &'static LocalKey<HiddenViews>, view: &NSView) -> bool {
    list.with(|hidden| {
        hidden
            .borrow()
            .iter()
            .filter_map(Weak::load)
            .any(|other| std::ptr::eq(&*other, view))
    })
}

fn mark(list: &'static LocalKey<HiddenViews>, view: &NSView, hidden: bool) {
    list.with(|list| {
        let mut list = list.borrow_mut();
        // 消えたビューの分もここで片づける。
        list.retain(|weak| {
            weak.load()
                .is_some_and(|other| !std::ptr::eq(&*other, view))
        });
        if hidden {
            list.push(Weak::from(view));
        }
    });
}

/// Decides `hidden` again from the two settings.
fn sync(view: &NSView) {
    let hidden = contains(&APP_HIDDEN, view) || contains(&CONTAINER_HIDDEN, view);
    if view.isHidden() == hidden {
        return;
    }
    view.setHidden(hidden);
    // 大きさが変わるので、親の連なりへ伝える (Grid の Auto 行など)。
    crate::layout::invalidate_ancestors(view);
}

/// Whether it is shown. When hidden, it is removed from layout inside an
/// `NSStackView`.
pub(crate) fn set_visible(view: &NSView, visible: bool) {
    mark(&APP_HIDDEN, view, !visible);
    sync(view);
}

pub(crate) fn is_visible(view: &NSView) -> bool {
    !contains(&APP_HIDDEN, view)
}

/// Hides / shows it for a container's reasons. Remembered separately from the
/// app's [`set_visible`].
pub(crate) fn set_hidden_by_container(view: &NSView, hidden: bool) {
    mark(&CONTAINER_HIDDEN, view, hidden);
    sync(view);
}

/// Moves keyboard focus here. `true` if it moved.
///
/// If the widget itself cannot take focus, focus moves to the first descendant that
/// can. It cannot move while not in a window, hidden, or disabled.
pub(crate) fn request_focus(view: &NSView) -> bool {
    let Some(window) = view.window() else {
        return false;
    };
    if view.isHiddenOrHasHiddenAncestor() {
        return false;
    }
    let Some(target) = focus_target(view) else {
        return false;
    };
    window.makeFirstResponder(Some(&target))
}

fn focus_target(view: &NSView) -> Option<Retained<NSView>> {
    if view.isHidden() {
        return None;
    }
    if view.acceptsFirstResponder() {
        return Some(view.retain());
    }
    view.subviews()
        .iter()
        .find_map(|child| focus_target(&child))
}

/// The description shown when the pointer hovers over it. `None` removes it.
pub(crate) fn set_tooltip(view: &NSView, text: Option<&str>) {
    let text = text.map(NSString::from_str);
    view.setToolTip(text.as_deref());
}

/// The name reported to screen readers (`accessibilityLabel`). `None` removes it.
pub(crate) fn set_accessible_label(view: &NSView, text: Option<&str>) {
    use objc2_app_kit::NSAccessibility;
    let text = text.map(NSString::from_str);
    view.setAccessibilityLabel(text.as_deref());
}
