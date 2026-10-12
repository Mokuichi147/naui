//! どのウィジェットにもある操作 (表示・非表示 / フォーカス / ツールチップ)。
//!
//! 対象は親へ置いている要素 ([`Widget::native_element`](crate::Widget::native_element))。
//! 中身を別の要素で包んでいるウィジェットでも、フォーカスは中の入力欄へ
//! 届くようにビジュアルツリーをたどる。

use naui_winui3::Microsoft::UI::Xaml::Controls::ToolTipService;
use naui_winui3::Microsoft::UI::Xaml::Media::VisualTreeHelper;
use naui_winui3::Microsoft::UI::Xaml::{DependencyObject, FocusState, UIElement, Visibility};
use windows::Foundation::PropertyValue;
use windows_core::{IInspectable, Interface, HSTRING};

/// 表示するかどうか。`Collapsed` にした要素はレイアウトで場所を取らない。
pub(crate) fn set_visible(element: &UIElement, visible: bool) {
    let _ = element.SetVisibility(if visible {
        Visibility::Visible
    } else {
        Visibility::Collapsed
    });
}

pub(crate) fn is_visible(element: &UIElement) -> bool {
    element
        .Visibility()
        .map_or(true, |v| v == Visibility::Visible)
}

/// キーボードフォーカスを移す。移せたら `true`。
///
/// 自分が受け取れなければ、子孫のうち最初に受け取れるものへ移す。
/// 画面に出る前・隠れている・無効なときは `UIElement.Focus` が断る。
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

/// ポインターを重ねたときに出す説明。`None` で外す。
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

/// 読み上げソフトに伝える名前 (`AutomationProperties.Name`)。`None` で外す。
pub(crate) fn set_accessible_label(element: &UIElement, text: Option<&str>) {
    use naui_winui3::Microsoft::UI::Xaml::Automation::AutomationProperties;
    use naui_winui3::Microsoft::UI::Xaml::DependencyObject;
    use windows_core::{Interface, HSTRING};
    let Ok(object) = element.cast::<DependencyObject>() else {
        return;
    };
    let _ = AutomationProperties::SetName(&object, &HSTRING::from(text.unwrap_or("")));
}
