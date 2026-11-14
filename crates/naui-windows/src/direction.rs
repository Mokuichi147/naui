//! The app's layout direction (`FrameworkElement.FlowDirection`).
//!
//! XAML mirrors everything under an element whose `FlowDirection` is
//! `RightToLeft`: panels lay their children out from the right, `Thickness` and
//! `HorizontalAlignment` swap their left and right, and text starts at the right.
//! The mirroring is a transform on the rendering, so it also flips what an element
//! draws itself (an `Image`, a video, the surface of `Canvas`) and the pointer
//! positions it reports. Those are given `LeftToRight` back
//! ([`pin_left_to_right`]).
//!
//! `FlowDirection` is inherited through the visual tree, so naui sets it on each
//! window's content. A `ContentDialog` is shown in a separate layer and does not
//! inherit it, so it is given the direction where naui creates it. Flyouts and menu
//! flyouts take their target's direction by themselves; `Flyout` then reads its
//! `Left` and `Right` placements in that direction (see `popover.rs`).
//!
//! The system draws the caption buttons, and they stay at the right end in a
//! right-to-left layout, so the title bar keeps their reserve on the physical
//! right (see `window.rs`).

use naui_core::LayoutDirection;
use naui_winui3::Microsoft::UI::Xaml::{FlowDirection, FrameworkElement};
use windows_core::Interface;

/// Gives `element` the app's layout direction. Leaves it alone (WinUI's default,
/// left to right) when the app named none.
pub(crate) fn apply_app_direction(element: &impl Interface) {
    let Some(direction) = naui_core::app_layout_direction() else {
        return;
    };
    if let Ok(element) = element.cast::<FrameworkElement>() {
        let _ = element.SetFlowDirection(flow_direction(direction));
    }
}

/// Keeps `element` and everything inside it left to right, also in a right-to-left
/// layout: for pictures, drawings, the number in a `NumberBox` and the playback
/// bar, which must not be mirrored.
pub(crate) fn pin_left_to_right(element: &impl Interface) {
    if let Ok(element) = element.cast::<FrameworkElement>() {
        let _ = element.SetFlowDirection(FlowDirection::LeftToRight);
    }
}

/// Which way the app's layout runs: what naui gave the windows, or WinUI's default.
pub(crate) fn layout_direction() -> LayoutDirection {
    naui_core::app_layout_direction().unwrap_or(LayoutDirection::Ltr)
}

fn flow_direction(direction: LayoutDirection) -> FlowDirection {
    if direction.is_rtl() {
        FlowDirection::RightToLeft
    } else {
        FlowDirection::LeftToRight
    }
}
