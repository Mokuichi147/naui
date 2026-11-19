//! Puts focus back when an overlay that holds it goes away (toasts).
//!
//! When the focused element leaves the visual tree (or is collapsed), WinUI moves
//! focus to the first element of the window that can take it. For an overlay that
//! focus only visits — Tab into a toast's action button and press Enter — that sends
//! the user to the top of the window (the toolbar's first button, say) instead of
//! where they came from.
//!
//! So while the overlay is shown, the element that last lost focus outside it is
//! remembered (`LostFocus` bubbles, so one subscription on the window root sees every
//! element). Entering the overlay makes the element left behind lose focus, so when
//! focus is inside the overlay, the one remembered is where it came from. When the
//! overlay goes away with focus inside, focus goes back there, in the same
//! [`FocusState`] (a keyboard visit gets its focus rectangle back). If that element
//! is gone or cannot take focus, it goes to the first element of the content that
//! can.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use naui_winui3::Microsoft::UI::Xaml::Controls::{Control, Grid};
use naui_winui3::Microsoft::UI::Xaml::Media::VisualTreeHelper;
use naui_winui3::Microsoft::UI::Xaml::{
    DependencyObject, FocusState, RoutedEventHandler, UIElement,
};
use windows_core::{Interface, Weak};

use crate::keys::same;
use crate::ui_thread::UiThreadCell;

/// Watches focus around one overlay while it is shown.
pub(crate) struct FocusReturn {
    overlay: UIElement,
    /// The root `LostFocus` is attached to, and its token.
    root: UIElement,
    token: i64,
    /// The element that last lost focus outside the overlay.
    left: Rc<RefCell<Option<Weak<UIElement>>>>,
}

/// Where focus goes once the overlay is gone. See [`FocusReturn::release`].
pub(crate) struct PendingReturn {
    to: Option<UIElement>,
    state: FocusState,
}

impl FocusReturn {
    /// Starts watching. `layer` is where `overlay` was added; the watch covers the
    /// whole window content above it (title bar and menu bar included).
    pub(crate) fn watch(overlay: &UIElement, layer: &Grid) -> Option<Self> {
        let root = layer
            .Parent()
            .ok()
            .and_then(|parent| parent.cast::<UIElement>().ok())
            .or_else(|| layer.cast::<UIElement>().ok())?;
        let left: Rc<RefCell<Option<Weak<UIElement>>>> = Rc::default();
        let cell = Arc::new(UiThreadCell::new((left.clone(), overlay.clone())));
        let handler = RoutedEventHandler::new(move |_, args| {
            let source = args
                .as_ref()
                .and_then(|args| args.OriginalSource().ok())
                .and_then(|source| source.cast::<UIElement>().ok());
            let Some(source) = source else {
                return Ok(());
            };
            cell.try_with_mut(|(left, overlay)| {
                if !is_inside(&source, overlay) {
                    *left.borrow_mut() = source.downgrade().ok();
                }
            });
            Ok(())
        });
        let token = root.LostFocus(&handler).ok()?;
        Some(Self {
            overlay: overlay.clone(),
            root,
            token,
            left,
        })
    }

    /// Stops watching. Called **before** the overlay is hidden or removed (both move
    /// focus away from it). If focus is inside the overlay, returns where it should go
    /// back to, for [`PendingReturn::restore`] once the overlay is gone.
    pub(crate) fn release(self) -> Option<PendingReturn> {
        let _ = self.root.RemoveLostFocus(self.token);
        let state = focus_state_within(&self.overlay.cast::<DependencyObject>().ok()?)?;
        let to = self
            .left
            .borrow_mut()
            .take()
            .and_then(|weak| weak.upgrade());
        Some(PendingReturn { to, state })
    }
}

impl PendingReturn {
    /// Moves focus back. `content` is the fallback when the remembered element is gone
    /// or refuses focus.
    pub(crate) fn restore(self, content: Option<&Grid>) {
        // ツリーから外れた要素は `XamlRoot` を持たず、Focus も通らない。
        if let Some(to) = &self.to {
            if to.XamlRoot().is_ok() && to.Focus(self.state).unwrap_or(false) {
                return;
            }
        }
        if let Some(content) = content.and_then(|c| c.cast::<DependencyObject>().ok()) {
            focus_first(&content, self.state);
        }
    }
}

/// Whether `element` is `overlay` or inside it.
fn is_inside(element: &UIElement, overlay: &UIElement) -> bool {
    let Ok(overlay) = overlay.cast::<DependencyObject>() else {
        return false;
    };
    let mut current = element.cast::<DependencyObject>().ok();
    while let Some(object) = current {
        if same(&object, &overlay) {
            return true;
        }
        current = VisualTreeHelper::GetParent(&object).ok();
    }
    false
}

/// The focus state of the control holding focus inside `object`, if one does.
fn focus_state_within(object: &DependencyObject) -> Option<FocusState> {
    let state = object
        .cast::<Control>()
        .and_then(|control| control.FocusState())
        .unwrap_or(FocusState::Unfocused);
    if state != FocusState::Unfocused {
        return Some(state);
    }
    let count = VisualTreeHelper::GetChildrenCount(object).unwrap_or(0);
    (0..count)
        .filter_map(|index| VisualTreeHelper::GetChild(object, index).ok())
        .find_map(|child| focus_state_within(&child))
}

/// Focuses the first element under `object` that takes it.
fn focus_first(object: &DependencyObject, state: FocusState) -> bool {
    if let Ok(element) = object.cast::<UIElement>() {
        if !crate::interaction::is_visible(&element) {
            return false;
        }
        if element.Focus(state).unwrap_or(false) {
            return true;
        }
    }
    let count = VisualTreeHelper::GetChildrenCount(object).unwrap_or(0);
    (0..count)
        .filter_map(|index| VisualTreeHelper::GetChild(object, index).ok())
        .any(|child| focus_first(&child, state))
}
