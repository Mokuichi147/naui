//! A hand-written projection of just the UI Automation parts the tests use.
//!
//! "The same actions as a user" — clicking buttons, toggling checks, choosing items
//! — can only be triggered through WinUI's `AutomationPeer` (WinUI 3's `Button` has
//! no API for clicking it from code).
//!
//! However, adding `AutomationPeer` to the [`naui_winui3`] projection
//! (`bindings.rs`) makes `IUIElementOverrides::OnCreateAutomationPeer`, which returns
//! it, projectable, adding a required method to the public trait
//! `IUIElementOverrides_Impl`. Downstream crates using the projection would stop
//! compiling just from that. So as not to break the public API for the tests' sake,
//! only the few pieces needed are hand-written here.
//!
//! The IIDs and vtable order are copied from the projection generated from the
//! `.winmd`. Unused slots are filled with `usize` padding (the same form
//! `windows-bindgen` uses for members it cannot project). **Getting the order wrong
//! calls a different method**, so when adding, temporarily run
//! `tools/winui3-bindgen` and compare. It only calls through received pointers and
//! never builds vtables itself, so slots after the last one used need not be held.

// WinRT の名前は元の綴りのまま出す。
#![allow(non_snake_case)]

use std::ffi::c_void;

use naui_winui3::Microsoft::UI::Xaml::UIElement;
use windows_core::{IInspectable, Interface, Result, Type, HRESULT, HSTRING};

/// The number of the pattern to get
/// (`Microsoft.UI.Xaml.Automation.Peers.PatternInterface`).
#[derive(Clone, Copy)]
pub(crate) enum Pattern {
    Invoke = 0,
    SelectionItem = 11,
    Toggle = 15,
}

// ------------------------------------------------------------ AutomationPeer

windows_core::imp::define_interface!(
    IAutomationPeer,
    IAutomationPeer_Vtbl,
    0xe51d3e4e_34f0_568c_999f_6277e2afe6d7
);
windows_core::imp::interface_hierarchy!(IAutomationPeer, windows_core::IUnknown, IInspectable);

#[repr(C)]
#[doc(hidden)]
pub struct IAutomationPeer_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    EventsSource: usize,
    SetEventsSource: usize,
    pub GetPattern: unsafe extern "system" fn(*mut c_void, i32, *mut *mut c_void) -> HRESULT,
    RaiseAutomationEvent: usize,
    RaisePropertyChangedEvent: usize,
    GetAcceleratorKey: usize,
    GetAccessKey: usize,
    GetAutomationControlType: usize,
    GetAutomationId: usize,
    GetBoundingRectangle: usize,
    GetChildren: usize,
    Navigate: usize,
    GetClassName: usize,
    GetClickablePoint: usize,
    GetHelpText: usize,
    GetItemStatus: usize,
    GetItemType: usize,
    GetLabeledBy: usize,
    GetLocalizedControlType: usize,
    pub GetName: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> HRESULT,
}

impl IAutomationPeer {
    /// The pattern this control responds to. Asking for one it does not respond to
    /// returns empty.
    fn pattern(&self, which: Pattern) -> Result<IInspectable> {
        unsafe {
            let mut result = std::mem::zeroed();
            (Interface::vtable(self).GetPattern)(Interface::as_raw(self), which as i32, &mut result)
                .and_then(|| Type::from_abi(result))
        }
    }

    /// The name passed to assistive technology.
    fn name(&self) -> Result<HSTRING> {
        unsafe {
            let mut result = std::mem::zeroed();
            // HSTRING はポインタそのものなので、生成物と同じく transmute で
            // 受け取る (`Type::from_abi` は HSTRING には使えない)。
            (Interface::vtable(self).GetName)(Interface::as_raw(self), &mut result)
                .map(|| std::mem::transmute::<*mut c_void, HSTRING>(result))
        }
    }
}

// -------------------------------------------- FrameworkElementAutomationPeer

windows_core::imp::define_interface!(
    IFrameworkElementAutomationPeerStatics,
    IFrameworkElementAutomationPeerStatics_Vtbl,
    0x081f6fbe_6500_528a_a506_f5a4d41ddf6c
);

#[repr(C)]
#[doc(hidden)]
pub struct IFrameworkElementAutomationPeerStatics_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    FromElement: usize,
    pub CreatePeerForElement:
        unsafe extern "system" fn(*mut c_void, *mut c_void, *mut *mut c_void) -> HRESULT,
}

/// The name for looking up the activation factory. The class itself is not
/// projected.
struct FrameworkElementAutomationPeer;

impl windows_core::RuntimeName for FrameworkElementAutomationPeer {
    const NAME: &'static str = "Microsoft.UI.Xaml.Automation.Peers.FrameworkElementAutomationPeer";
}

/// This element as seen by native UI Automation.
///
/// The peer WinUI provides per control comes back (`ButtonAutomationPeer` for a
/// `Button`). It is the same door screen readers and automation tools touch.
fn peer(element: &UIElement) -> Result<IAutomationPeer> {
    static SHARED: windows_core::imp::FactoryCache<
        FrameworkElementAutomationPeer,
        IFrameworkElementAutomationPeerStatics,
    > = windows_core::imp::FactoryCache::new();
    SHARED.call(|statics| unsafe {
        let mut result = std::mem::zeroed();
        (Interface::vtable(statics).CreatePeerForElement)(
            Interface::as_raw(statics),
            Interface::as_raw(element),
            &mut result,
        )
        .and_then(|| Type::from_abi(result))
    })
}

// ----------------------------------------------------------- 各パターン

windows_core::imp::define_interface!(
    IInvokeProvider,
    IInvokeProvider_Vtbl,
    0x02481105_3378_544d_b4e1_a1b368afbc02
);

#[repr(C)]
#[doc(hidden)]
pub struct IInvokeProvider_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub Invoke: unsafe extern "system" fn(*mut c_void) -> HRESULT,
}

windows_core::imp::define_interface!(
    IToggleProvider,
    IToggleProvider_Vtbl,
    0x021080c2_30a9_52ef_bc32_2b79847b6ba7
);

#[repr(C)]
#[doc(hidden)]
pub struct IToggleProvider_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    ToggleState: usize,
    pub Toggle: unsafe extern "system" fn(*mut c_void) -> HRESULT,
}

windows_core::imp::define_interface!(
    ISelectionItemProvider,
    ISelectionItemProvider_Vtbl,
    0xc9dfdd81_d4ac_5d31_be7f_24fab16060e4
);

#[repr(C)]
#[doc(hidden)]
pub struct ISelectionItemProvider_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    IsSelected: usize,
    SelectionContainer: usize,
    AddToSelection: usize,
    RemoveFromSelection: usize,
    pub Select: unsafe extern "system" fn(*mut c_void) -> HRESULT,
}

// --------------------------------------------------------- テストが使う口

/// Clicks through the same path as an actual click (the Invoke pattern).
pub(crate) fn invoke(element: &UIElement) {
    let provider: IInvokeProvider = provider(element, Pattern::Invoke);
    unsafe { (Interface::vtable(&provider).Invoke)(Interface::as_raw(&provider)) }
        .ok()
        .expect("Invoke に失敗しました");
}

/// Toggles on/off through the same path as an actual click (the Toggle pattern).
pub(crate) fn toggle(element: &UIElement) {
    let provider: IToggleProvider = provider(element, Pattern::Toggle);
    unsafe { (Interface::vtable(&provider).Toggle)(Interface::as_raw(&provider)) }
        .ok()
        .expect("Toggle に失敗しました");
}

/// Selects an item through the same path as an actual selection (the SelectionItem
/// pattern).
pub(crate) fn select(element: &UIElement) {
    let provider: ISelectionItemProvider = provider(element, Pattern::SelectionItem);
    unsafe { (Interface::vtable(&provider).Select)(Interface::as_raw(&provider)) }
        .ok()
        .expect("Select に失敗しました");
}

/// The name passed to assistive technology.
pub(crate) fn accessible_name(element: &UIElement) -> String {
    peer(element)
        .expect("AutomationPeer を作れませんでした")
        .name()
        .map(|name| name.to_string())
        .unwrap_or_default()
}

/// Takes a pattern out of the peer. Fails the test for controls that do not respond
/// to it.
fn provider<I: Interface>(element: &UIElement, which: Pattern) -> I {
    peer(element)
        .expect("AutomationPeer を作れませんでした")
        .pattern(which)
        .expect("パターンを取れませんでした")
        .cast::<I>()
        .expect("期待したプロバイダーではありません")
}
