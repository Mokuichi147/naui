//! 検索の入力欄 (WinUI 3 のネイティブ `AutoSuggestBox`)。
//!
//! Fluent 2 で検索の欄にあたるのは `AutoSuggestBox` で、虫めがねの印は
//! `QueryIcon`、確定は `QuerySubmitted` が受け持つ。公開 WinRT インター
//! フェイスの必要な部分をこのモジュールで定義している。`AutoSuggestBox` は
//! [`naui_winui3`] にも入ったので、この手書きの投影は将来そちらへ
//! 寄せられる。コントロール自体は `XamlReader` から生成される本物の
//! `AutoSuggestBox`。
//!
//! 候補の一覧 (`ItemsSource`) は渡さないので、打っても候補は出ない。
//! naui の検索欄は「打鍵の通知」と「確定の通知」だけを持つ、4 環境で
//! そろう部分に合わせてある。

use std::ffi::c_void;
use std::rc::Rc;
use std::sync::Arc;

use naui_core::Result;
use naui_winui3::Microsoft::UI::Xaml::Controls::Control;
use naui_winui3::Microsoft::UI::Xaml::Markup::XamlReader;
use naui_winui3::Microsoft::UI::Xaml::UIElement;
use windows::Foundation::TypedEventHandler;
use windows_core::{Interface, Param, HSTRING};

use crate::to_error;
use crate::ui_thread::{HandlerCell, UiThreadCell};
use crate::widgets::{impl_widget, Widget};

// 虫めがねの印は WinUI の標準アイコン (Segoe Fluent Icons の Find) を使う。
// XAML で渡すのは、`QueryIcon` へ入れる `IconElement` を自前で投影せずに
// 済ませるため。
const AUTO_SUGGEST_BOX_XAML: &str = r#"<AutoSuggestBox
    xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
    QueryIcon="Find"/>"#;

/// `AutoSuggestBoxTextChangedEventArgs.Reason` の値。
const REASON_PROGRAMMATIC_CHANGE: i32 = 1;

/// 文字列を 1 つ受け取る通知先。
///
/// WinRT のデリゲートは `Send + Sync` を要求するので [`UiThreadCell`] に
/// 載せる。呼び出しの間だけクロージャを取り出すため、通知の中から同じ欄を
/// 操作しても二重借用にならない。
#[derive(Clone)]
struct TextHandler(HandlerCell<dyn FnMut(&str)>);

impl TextHandler {
    fn new() -> Self {
        Self(Arc::new(UiThreadCell::new(None)))
    }

    fn set(&self, f: impl FnMut(&str) + 'static) {
        self.0.with_mut(|slot| *slot = Some(Box::new(f)));
    }

    fn emit(&self, text: &str) {
        let Some(Some(mut f)) = self.0.try_with_mut(|slot| slot.take()) else {
            return;
        };
        f(text);
        let _ = self.0.try_with_mut(|slot| {
            if slot.is_none() {
                *slot = Some(f);
            }
        });
    }
}

struct SearchInputInner {
    native: NativeAutoSuggestBox,
    on_change: TextHandler,
    on_search: TextHandler,
}

/// 検索の入力欄 (`AutoSuggestBox`)。
///
/// 虫めがねの印と、打ち始めると出る取り消しボタン (✕) は WinUI が出す。
#[derive(Clone)]
pub struct SearchInput(Rc<SearchInputInner>);
impl_widget!(SearchInput, native);

impl SearchInput {
    pub(crate) fn new() -> Result<Self> {
        let native = load_auto_suggest_box()?;
        let this = Self(Rc::new(SearchInputInner {
            native,
            on_change: TextHandler::new(),
            on_search: TextHandler::new(),
        }));
        this.connect()?;
        Ok(this)
    }

    /// WinUI の `TextChanged` / `QuerySubmitted` を Rust のクロージャへつなぐ。
    ///
    /// `TextChanged` は `Text` を書き換えたときにも飛ぶので、`Reason` が
    /// プログラムからの変更なら黙る (macOS / GTK / Web と同じく、通知は
    /// 利用者の操作のときだけ)。
    fn connect(&self) -> Result<()> {
        let target = Arc::new(UiThreadCell::new(Rc::downgrade(&self.0)));
        let changed = TypedEventHandler::<
            NativeAutoSuggestBox,
            NativeAutoSuggestBoxTextChangedEventArgs,
        >::new(move |_sender, args| {
            let _ = target.try_with_mut(|weak| {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                let reason = args
                    .as_ref()
                    .and_then(|args| args.reason().ok())
                    .unwrap_or(REASON_PROGRAMMATIC_CHANGE);
                if reason == REASON_PROGRAMMATIC_CHANGE {
                    return;
                }
                let text = inner.native.text().unwrap_or_default().to_string();
                inner.on_change.emit(&text);
            });
            Ok(())
        });
        self.0
            .native
            .text_changed(&changed)
            .map_err(|e| to_error("AutoSuggestBox の変更購読", e))?;

        let target = Arc::new(UiThreadCell::new(Rc::downgrade(&self.0)));
        let submitted = TypedEventHandler::<
            NativeAutoSuggestBox,
            NativeAutoSuggestBoxQuerySubmittedEventArgs,
        >::new(move |_sender, args| {
            let _ = target.try_with_mut(|weak| {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                // 確定した文字列は引数で来るが、読めなければ欄から読む。
                let text = args
                    .as_ref()
                    .and_then(|args| args.query_text().ok())
                    .or_else(|| inner.native.text().ok())
                    .unwrap_or_default()
                    .to_string();
                inner.on_search.emit(&text);
            });
            Ok(())
        });
        self.0
            .native
            .query_submitted(&submitted)
            .map_err(|e| to_error("AutoSuggestBox の確定購読", e))?;
        Ok(())
    }

    /// いま入力されている文字列。
    pub fn text(&self) -> String {
        self.0
            .native
            .text()
            .map(|s| s.to_string())
            .unwrap_or_default()
    }

    /// 文字列を置き換える。`on_change` は呼ばれない。
    pub fn set_text(&self, text: &str) {
        let _ = self.0.native.set_text(&HSTRING::from(text));
    }

    pub fn set_placeholder(&self, text: &str) {
        let _ = self.0.native.set_placeholder_text(&HSTRING::from(text));
    }

    pub fn set_enabled(&self, enabled: bool) {
        if let Ok(control) = self.0.native.cast::<Control>() {
            let _ = control.SetIsEnabled(enabled);
        }
    }

    /// 1 文字入力するたびに、その時点の文字列で呼ばれる。
    pub fn on_change(&self, f: impl FnMut(&str) + 'static) {
        self.0.on_change.set(f);
    }

    /// Enter か虫めがねの印で確定したときに、その時点の文字列で呼ばれる。
    pub fn on_search(&self, f: impl FnMut(&str) + 'static) {
        self.0.on_search.set(f);
    }
}

fn load_auto_suggest_box() -> Result<NativeAutoSuggestBox> {
    XamlReader::Load(&HSTRING::from(AUTO_SUGGEST_BOX_XAML))
        .and_then(|element| element.cast::<NativeAutoSuggestBox>())
        .map_err(|e| to_error("AutoSuggestBox の生成", e))
}

// -------------------------------------------------------------------------
// Microsoft.UI.Xaml.Controls.AutoSuggestBox の最小 WinRT 投影
//
// IID と vtable の並びは cppwinrt の生成ヘッダー
// (winrt/impl/Microsoft.UI.Xaml.Controls.0.h) の guid_v / abi<IAutoSuggestBox>
// に合わせている。プロパティは get / put の対で並ぶので、使わないものは
// `usize` で場所だけ空けてある。

windows_core::imp::define_interface!(
    IAutoSuggestBox,
    IAutoSuggestBox_Vtbl,
    0x3eea809e_b2db_521d_97db_e0648fb5d798
);
impl windows_core::RuntimeType for IAutoSuggestBox {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}

#[repr(C)]
pub struct IAutoSuggestBox_Vtbl {
    base__: windows_core::IInspectable_Vtbl,
    max_suggestion_list_height: usize,
    set_max_suggestion_list_height: usize,
    is_suggestion_list_open: usize,
    set_is_suggestion_list_open: usize,
    text_member_path: usize,
    set_text_member_path: usize,
    text: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> windows_core::HRESULT,
    set_text: unsafe extern "system" fn(*mut c_void, *mut c_void) -> windows_core::HRESULT,
    update_text_on_select: usize,
    set_update_text_on_select: usize,
    placeholder_text: usize,
    set_placeholder_text:
        unsafe extern "system" fn(*mut c_void, *mut c_void) -> windows_core::HRESULT,
    header: usize,
    set_header: usize,
    auto_maximize_suggestion_area: usize,
    set_auto_maximize_suggestion_area: usize,
    text_box_style: usize,
    set_text_box_style: usize,
    query_icon: usize,
    set_query_icon: usize,
    light_dismiss_overlay_mode: usize,
    set_light_dismiss_overlay_mode: usize,
    description: usize,
    set_description: usize,
    suggestion_chosen: usize,
    remove_suggestion_chosen: usize,
    text_changed:
        unsafe extern "system" fn(*mut c_void, *mut c_void, *mut i64) -> windows_core::HRESULT,
    remove_text_changed: unsafe extern "system" fn(*mut c_void, i64) -> windows_core::HRESULT,
    query_submitted:
        unsafe extern "system" fn(*mut c_void, *mut c_void, *mut i64) -> windows_core::HRESULT,
    remove_query_submitted: unsafe extern "system" fn(*mut c_void, i64) -> windows_core::HRESULT,
}

#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
struct NativeAutoSuggestBox(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    NativeAutoSuggestBox,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl windows_core::RuntimeType for NativeAutoSuggestBox {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IAutoSuggestBox>();
}
unsafe impl Interface for NativeAutoSuggestBox {
    type Vtable = IAutoSuggestBox_Vtbl;
    const IID: windows_core::GUID = IAutoSuggestBox::IID;
}
impl windows_core::RuntimeName for NativeAutoSuggestBox {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.AutoSuggestBox";
}

impl NativeAutoSuggestBox {
    fn text(&self) -> windows_core::Result<HSTRING> {
        unsafe {
            let mut result = core::mem::zeroed();
            (Interface::vtable(self).text)(Interface::as_raw(self), &mut result)
                .map(|| core::mem::transmute(result))
        }
    }

    fn set_text(&self, value: &HSTRING) -> windows_core::Result<()> {
        unsafe {
            (Interface::vtable(self).set_text)(
                Interface::as_raw(self),
                core::mem::transmute_copy(value),
            )
            .ok()
        }
    }

    fn set_placeholder_text(&self, value: &HSTRING) -> windows_core::Result<()> {
        unsafe {
            (Interface::vtable(self).set_placeholder_text)(
                Interface::as_raw(self),
                core::mem::transmute_copy(value),
            )
            .ok()
        }
    }

    fn text_changed<P>(&self, handler: P) -> windows_core::Result<i64>
    where
        P: Param<TypedEventHandler<NativeAutoSuggestBox, NativeAutoSuggestBoxTextChangedEventArgs>>,
    {
        unsafe {
            let mut token = 0;
            (Interface::vtable(self).text_changed)(
                Interface::as_raw(self),
                handler.param().abi(),
                &mut token,
            )
            .map(|| token)
        }
    }

    fn query_submitted<P>(&self, handler: P) -> windows_core::Result<i64>
    where
        P: Param<
            TypedEventHandler<NativeAutoSuggestBox, NativeAutoSuggestBoxQuerySubmittedEventArgs>,
        >,
    {
        unsafe {
            let mut token = 0;
            (Interface::vtable(self).query_submitted)(
                Interface::as_raw(self),
                handler.param().abi(),
                &mut token,
            )
            .map(|| token)
        }
    }
}

windows_core::imp::define_interface!(
    IAutoSuggestBoxTextChangedEventArgs,
    IAutoSuggestBoxTextChangedEventArgs_Vtbl,
    0xd7191d84_e886_547f_a3e2_12f0e05b20fa
);
impl windows_core::RuntimeType for IAutoSuggestBoxTextChangedEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}

#[repr(C)]
pub struct IAutoSuggestBoxTextChangedEventArgs_Vtbl {
    base__: windows_core::IInspectable_Vtbl,
    reason: unsafe extern "system" fn(*mut c_void, *mut i32) -> windows_core::HRESULT,
    set_reason: usize,
    check_current: usize,
}

#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
struct NativeAutoSuggestBoxTextChangedEventArgs(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    NativeAutoSuggestBoxTextChangedEventArgs,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl windows_core::RuntimeType for NativeAutoSuggestBoxTextChangedEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IAutoSuggestBoxTextChangedEventArgs>();
}
unsafe impl Interface for NativeAutoSuggestBoxTextChangedEventArgs {
    type Vtable = IAutoSuggestBoxTextChangedEventArgs_Vtbl;
    const IID: windows_core::GUID = IAutoSuggestBoxTextChangedEventArgs::IID;
}
impl windows_core::RuntimeName for NativeAutoSuggestBoxTextChangedEventArgs {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.AutoSuggestBoxTextChangedEventArgs";
}

impl NativeAutoSuggestBoxTextChangedEventArgs {
    fn reason(&self) -> windows_core::Result<i32> {
        unsafe {
            let mut result = 0;
            (Interface::vtable(self).reason)(Interface::as_raw(self), &mut result).map(|| result)
        }
    }
}

windows_core::imp::define_interface!(
    IAutoSuggestBoxQuerySubmittedEventArgs,
    IAutoSuggestBoxQuerySubmittedEventArgs_Vtbl,
    0x26da5de4_57a6_57bf_acc9_aac599c0b22b
);
impl windows_core::RuntimeType for IAutoSuggestBoxQuerySubmittedEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}

#[repr(C)]
pub struct IAutoSuggestBoxQuerySubmittedEventArgs_Vtbl {
    base__: windows_core::IInspectable_Vtbl,
    query_text: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> windows_core::HRESULT,
    chosen_suggestion: usize,
}

#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
struct NativeAutoSuggestBoxQuerySubmittedEventArgs(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    NativeAutoSuggestBoxQuerySubmittedEventArgs,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl windows_core::RuntimeType for NativeAutoSuggestBoxQuerySubmittedEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IAutoSuggestBoxQuerySubmittedEventArgs>();
}
unsafe impl Interface for NativeAutoSuggestBoxQuerySubmittedEventArgs {
    type Vtable = IAutoSuggestBoxQuerySubmittedEventArgs_Vtbl;
    const IID: windows_core::GUID = IAutoSuggestBoxQuerySubmittedEventArgs::IID;
}
impl windows_core::RuntimeName for NativeAutoSuggestBoxQuerySubmittedEventArgs {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.AutoSuggestBoxQuerySubmittedEventArgs";
}

impl NativeAutoSuggestBoxQuerySubmittedEventArgs {
    fn query_text(&self) -> windows_core::Result<HSTRING> {
        unsafe {
            let mut result = core::mem::zeroed();
            (Interface::vtable(self).query_text)(Interface::as_raw(self), &mut result)
                .map(|| core::mem::transmute(result))
        }
    }
}
