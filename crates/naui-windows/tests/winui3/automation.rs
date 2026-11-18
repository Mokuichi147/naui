//! The UI Automation helpers the tests use.
//!
//! The projection itself lives in `naui_windows::automation`, which `click` on the
//! widgets also uses. These wrappers fail the test instead of returning errors.

use naui_windows::automation;
use naui_winui3::Microsoft::UI::Xaml::UIElement;

/// Clicks through the same path as an actual click (the Invoke pattern).
pub(crate) fn invoke(element: &UIElement) {
    automation::invoke(element).expect("Invoke に失敗しました");
}

/// Toggles on/off through the same path as an actual click (the Toggle pattern).
pub(crate) fn toggle(element: &UIElement) {
    automation::toggle(element).expect("Toggle に失敗しました");
}

/// Selects an item through the same path as an actual selection (the SelectionItem
/// pattern).
pub(crate) fn select(element: &UIElement) {
    automation::select(element).expect("Select に失敗しました");
}

/// The name passed to assistive technology.
pub(crate) fn accessible_name(element: &UIElement) -> String {
    automation::accessible_name(element).expect("AutomationPeer を作れませんでした")
}

/// `AutomationControlType.Group`.
pub(crate) const CONTROL_TYPE_GROUP: i32 = automation::CONTROL_TYPE_GROUP;

/// The kind of control passed to assistive technology. `None` when the element has
/// no peer and so does not appear to assistive technology at all.
pub(crate) fn control_type(element: &UIElement) -> Option<i32> {
    automation::control_type(element).map(|kind| kind.expect("GetAutomationControlType"))
}
