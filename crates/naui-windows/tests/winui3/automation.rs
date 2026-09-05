//! UI オートメーションの、テストが使う分だけの手書き投影。
//!
//! ボタンを押す・チェックを反転する・項目を選ぶといった「利用者と同じ操作」は
//! WinUI の `AutomationPeer` からしか起こせない (WinUI 3 の `Button` には
//! プログラムから押す API が無い)。
//!
//! ただし `AutomationPeer` を [`naui_winui3`] の投影 (`bindings.rs`) へ足すと、
//! それを返す `IUIElementOverrides::OnCreateAutomationPeer` が投影可能になり、
//! 公開トレイト `IUIElementOverrides_Impl` に必須メソッドが増える。投影を
//! 使っている下流のクレートが、それだけでコンパイルできなくなる。テストの
//! 都合で公開 API を壊さないよう、要る数個だけをここで手書きする。
//!
//! IID と vtable の並びは `.winmd` から生成した投影を写したもの。使わない
//! スロットは `usize` の詰め物にしてある (`windows-bindgen` が投影できない
//! メンバーへ入れるのと同じ形)。**並びを間違えると別のメソッドを呼ぶ**ので、
//! 足すときは `tools/winui3-bindgen` を一時的に流して突き合わせること。
//! 受け取ったポインタを呼ぶだけで自分では vtable を作らないため、使う
//! スロットより後ろは持たなくてよい。

// WinRT の名前は元の綴りのまま出す。
#![allow(non_snake_case)]

use std::ffi::c_void;

use naui_winui3::Microsoft::UI::Xaml::UIElement;
use windows_core::{IInspectable, Interface, Result, Type, HRESULT, HSTRING};

/// 取りたいパターンの番号 (`Microsoft.UI.Xaml.Automation.Peers.PatternInterface`)。
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
    /// このコントロールが応じるパターン。応じないものを頼むと空が返る。
    fn pattern(&self, which: Pattern) -> Result<IInspectable> {
        unsafe {
            let mut result = std::mem::zeroed();
            (Interface::vtable(self).GetPattern)(Interface::as_raw(self), which as i32, &mut result)
                .and_then(|| Type::from_abi(result))
        }
    }

    /// 支援技術へ渡る読み上げ名。
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

/// 活性化ファクトリを引くための名前。クラス自体は投影しない。
struct FrameworkElementAutomationPeer;

impl windows_core::RuntimeName for FrameworkElementAutomationPeer {
    const NAME: &'static str = "Microsoft.UI.Xaml.Automation.Peers.FrameworkElementAutomationPeer";
}

/// ネイティブの UI オートメーションから見たこの要素。
///
/// WinUI がコントロールごとに用意する peer が返る (`Button` なら
/// `ButtonAutomationPeer`)。画面読み上げソフトや自動操作ツールが触るのと
/// 同じ口。
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

/// 実際に押したのと同じ経路 (Invoke パターン) でクリックする。
pub(crate) fn invoke(element: &UIElement) {
    let provider: IInvokeProvider = provider(element, Pattern::Invoke);
    unsafe { (Interface::vtable(&provider).Invoke)(Interface::as_raw(&provider)) }
        .ok()
        .expect("Invoke に失敗しました");
}

/// 実際に押したのと同じ経路 (Toggle パターン) で入り切りを反転させる。
pub(crate) fn toggle(element: &UIElement) {
    let provider: IToggleProvider = provider(element, Pattern::Toggle);
    unsafe { (Interface::vtable(&provider).Toggle)(Interface::as_raw(&provider)) }
        .ok()
        .expect("Toggle に失敗しました");
}

/// 実際に選んだのと同じ経路 (SelectionItem パターン) で項目を選ぶ。
pub(crate) fn select(element: &UIElement) {
    let provider: ISelectionItemProvider = provider(element, Pattern::SelectionItem);
    unsafe { (Interface::vtable(&provider).Select)(Interface::as_raw(&provider)) }
        .ok()
        .expect("Select に失敗しました");
}

/// 支援技術へ渡る読み上げ名。
pub(crate) fn accessible_name(element: &UIElement) -> String {
    peer(element)
        .expect("AutomationPeer を作れませんでした")
        .name()
        .map(|name| name.to_string())
        .unwrap_or_default()
}

/// peer からパターンを取り出す。応じないコントロールならテストを落とす。
fn provider<I: Interface>(element: &UIElement, which: Pattern) -> I {
    peer(element)
        .expect("AutomationPeer を作れませんでした")
        .pattern(which)
        .expect("パターンを取れませんでした")
        .cast::<I>()
        .expect("期待したプロバイダーではありません")
}
