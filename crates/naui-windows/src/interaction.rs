//! Operations every widget has (show/hide / focus / tooltip).
//!
//! The target is the element placed in the parent
//! ([`Widget::native_element`](crate::Widget::native_element)). Even for widgets that
//! wrap their content in another element, the visual tree is walked so that focus
//! reaches the inner field.

use naui_winui3::Microsoft::UI::Xaml::Controls::ToolTipService;
use naui_winui3::Microsoft::UI::Xaml::Media::VisualTreeHelper;
use naui_winui3::Microsoft::UI::Xaml::{
    DependencyObject, FocusState, FrameworkElement, UIElement, Visibility,
};
use windows::Foundation::PropertyValue;
use windows_core::{IInspectable, Interface, HSTRING};

/// Whether it is shown. An element set to `Collapsed` takes no space in layout.
pub(crate) fn set_visible(element: &UIElement, visible: bool) {
    let _ = element.SetVisibility(if visible {
        Visibility::Visible
    } else {
        Visibility::Collapsed
    });
    // Stack は隠れた子に行 (列) を持たせないので、並べ直す。
    if let Ok(framework) = element.cast::<FrameworkElement>() {
        crate::layout::rearrange_parent_stack(&framework);
    }
}

pub(crate) fn is_visible(element: &UIElement) -> bool {
    element
        .Visibility()
        .map_or(true, |v| v == Visibility::Visible)
}

/// Moves keyboard focus here. `true` if it moved.
///
/// If the widget itself cannot take focus, focus moves to the first descendant that
/// can. `UIElement.Focus` refuses before it is on screen, while hidden, or while
/// disabled.
pub(crate) fn request_focus(element: &UIElement) -> bool {
    // ビジュアルツリーに載っていない要素は `XamlRoot` を持たない。
    if element.XamlRoot().is_err() {
        return false;
    }
    element
        .cast::<DependencyObject>()
        .is_ok_and(|object| focus_first(&object))
}

fn focus_first(object: &DependencyObject) -> bool {
    if let Ok(element) = object.cast::<UIElement>() {
        if !is_visible(&element) {
            return false;
        }
        if element.Focus(FocusState::Programmatic).unwrap_or(false) {
            return true;
        }
    }
    let count = VisualTreeHelper::GetChildrenCount(object).unwrap_or(0);
    (0..count)
        .filter_map(|index| VisualTreeHelper::GetChild(object, index).ok())
        .any(|child| focus_first(&child))
}

/// The description shown when the pointer hovers over it. `None` removes it.
pub(crate) fn set_tooltip(element: &UIElement, text: Option<&str>) {
    let Ok(object) = element.cast::<DependencyObject>() else {
        return;
    };
    match text.map(|text| PropertyValue::CreateString(&HSTRING::from(text))) {
        Some(Ok(tip)) => {
            let _ = ToolTipService::SetToolTip(&object, &tip);
        }
        Some(Err(_)) => {}
        None => {
            let _ = ToolTipService::SetToolTip(&object, None::<&IInspectable>);
        }
    }
}

/// The name reported to screen readers (`AutomationProperties.Name`). `None` removes
/// it.
pub(crate) fn set_accessible_label(element: &UIElement, text: Option<&str>) {
    use naui_winui3::Microsoft::UI::Xaml::Automation::AutomationProperties;
    use naui_winui3::Microsoft::UI::Xaml::DependencyObject;
    use windows_core::{Interface, HSTRING};
    let Ok(object) = element.cast::<DependencyObject>() else {
        return;
    };
    let _ = AutomationProperties::SetName(&object, &HSTRING::from(text.unwrap_or("")));
}
