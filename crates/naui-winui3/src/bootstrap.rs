//! Windows App SDK ランタイムをプロセスへ取り付ける (動的依存)。
//!
//! 未パッケージのアプリは、WinUI 3 の入ったフレームワークパッケージを
//! **自分で**プロセスのパッケージグラフへ足さないと、WinRT のクラスを
//! アクティベートできない。そのための API が「動的依存」
//! (dynamic dependency) で、
//!
//! - Windows 11 以降: `kernelbase.dll` の `TryCreatePackageDependency` ほか
//! - それ以外: Windows App SDK が入れる `Microsoft.WindowsAppRuntime.dll` の
//!   `Mdd` 付きの同名関数
//!
//! のどちらかに居る。どちらも `windows` クレートには宣言だけあって
//! リンクできないので、`GetProcAddress` で引く。

use std::fmt;
use std::sync::OnceLock;

use windows::core::{w, Error, Result, HRESULT, HSTRING, PCSTR, PCWSTR, PWSTR};
use windows::Win32::Foundation::{FreeLibrary, ERROR_MOD_NOT_FOUND, FARPROC, HMODULE};
use windows::Win32::Security::PSID;
use windows::Win32::Storage::Packaging::Appx::{
    AddPackageDependencyOptions, AddPackageDependencyOptions_None, CreatePackageDependencyOptions,
    CreatePackageDependencyOptions_None, PackageDependencyLifetimeKind,
    PackageDependencyLifetimeKind_Process, PackageDependencyProcessorArchitectures,
    PackageDependencyProcessorArchitectures_None, PACKAGEDEPENDENCY_CONTEXT, PACKAGE_VERSION,
    PACKAGE_VERSION_0,
};
use windows::Win32::System::LibraryLoader::{GetModuleHandleExW, GetProcAddress, LoadLibraryW};
use windows::Win32::System::Memory::{GetProcessHeap, HeapFree, HEAP_FLAGS};

/// Windows App SDK の版。パッケージファミリー名の一部になる。
///
/// `Cbs` の付くものは OS へ同梱される系統で、通常のフレームワーク
/// パッケージとは別のファミリー名を持つ。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Version {
    V1_3,
    V1_4,
    V1_5,
    V1_6,
    V1_7,
    V1_8,
    V2,
    Cbs1_6,
    Cbs1_8,
    Cbs2,
}

impl Version {
    /// パッケージファミリー名へ埋める文字列。
    const fn tag(self) -> &'static str {
        match self {
            Self::V1_3 => "1.3",
            Self::V1_4 => "1.4",
            Self::V1_5 => "1.5",
            Self::V1_6 => "1.6",
            Self::V1_7 => "1.7",
            Self::V1_8 => "1.8",
            Self::V2 => "2",
            Self::Cbs1_6 => "CBS.1.6",
            Self::Cbs1_8 => "CBS.1.8",
            Self::Cbs2 => "CBS.2",
        }
    }
}

type TryCreateFn = unsafe extern "system" fn(
    user: PSID,
    family: PCWSTR,
    min_version: PACKAGE_VERSION,
    architectures: PackageDependencyProcessorArchitectures,
    lifetime: PackageDependencyLifetimeKind,
    artifact: PCWSTR,
    options: CreatePackageDependencyOptions,
    id: *mut PWSTR,
) -> HRESULT;

type AddFn = unsafe extern "system" fn(
    id: PCWSTR,
    rank: i32,
    options: AddPackageDependencyOptions,
    context: *mut PACKAGEDEPENDENCY_CONTEXT,
    full_name: *mut PWSTR,
) -> HRESULT;

type RemoveFn = unsafe extern "system" fn(context: PACKAGEDEPENDENCY_CONTEXT) -> HRESULT;

type DeleteFn = unsafe extern "system" fn(id: PCWSTR) -> HRESULT;

/// 動的依存の 4 関数をまとめたもの。
struct Api {
    library: HMODULE,
    try_create: TryCreateFn,
    add: AddFn,
    remove: RemoveFn,
    delete: DeleteFn,
}

// SAFETY: 中身は関数ポインタと DLL のハンドルだけで、スレッドに縛られない。
unsafe impl Send for Api {}
unsafe impl Sync for Api {}

impl Drop for Api {
    fn drop(&mut self) {
        unsafe {
            let _ = FreeLibrary(self.library);
        }
    }
}

static API: OnceLock<Option<Api>> = OnceLock::new();

/// 4 つの名前を DLL から引く。1 つでも欠けたら使えない。
fn load(library: HMODULE, prefix: &str) -> Option<Api> {
    unsafe {
        let name = |suffix: &str| format!("{prefix}{suffix}\0");
        let proc =
            |suffix: &str| -> FARPROC { GetProcAddress(library, PCSTR(name(suffix).as_ptr())) };
        let try_create = std::mem::transmute::<FARPROC, Option<TryCreateFn>>(proc(
            "TryCreatePackageDependency",
        ))?;
        let add = std::mem::transmute::<FARPROC, Option<AddFn>>(proc("AddPackageDependency"))?;
        let remove =
            std::mem::transmute::<FARPROC, Option<RemoveFn>>(proc("RemovePackageDependency"))?;
        let delete =
            std::mem::transmute::<FARPROC, Option<DeleteFn>>(proc("DeletePackageDependency"))?;
        Some(Api {
            library,
            try_create,
            add,
            remove,
            delete,
        })
    }
}

fn api() -> Result<&'static Api> {
    API.get_or_init(|| unsafe {
        // Windows 11 以降は OS が持っている。接頭辞なしの名前。
        let mut library = HMODULE::default();
        if GetModuleHandleExW(0, w!("kernelbase.dll"), &mut library).is_ok() {
            if let Some(api) = load(library, "") {
                return Some(api);
            }
        }
        // それ以外は Windows App SDK が入れる DLL の `Mdd` 付きの名前。
        if let Ok(library) = LoadLibraryW(w!("Microsoft.WindowsAppRuntime.dll")) {
            return load(library, "Mdd");
        }
        None
    })
    .as_ref()
    .ok_or_else(|| Error::from_hresult(HRESULT::from_win32(ERROR_MOD_NOT_FOUND.0)))
}

/// 動的依存の API が返す、プロセスヒープ上の文字列。
struct HeapString(PWSTR);

impl Drop for HeapString {
    fn drop(&mut self) {
        unsafe {
            if let Ok(heap) = GetProcessHeap() {
                let _ = HeapFree(heap, HEAP_FLAGS(0), Some(self.0 .0.cast()));
            }
        }
    }
}

/// 取り付けた Windows App SDK ランタイム。
///
/// 落とすと取り付けも外れるので、アプリが終わるまで持ち続ける。
pub struct PackageDependency {
    context: PACKAGEDEPENDENCY_CONTEXT,
    id: HeapString,
    full_name: HSTRING,
}

impl PackageDependency {
    /// 指定した版のランタイムを取り付ける。
    ///
    /// 入っていなければ失敗するので、呼び出し側が新しい順に試す。
    pub fn initialize(version: Version) -> Result<Self> {
        let api = api()?;
        let family = HSTRING::from(format!(
            "Microsoft.WindowsAppRuntime.{}_8wekyb3d8bbwe",
            version.tag()
        ));
        // 下限は付けない。そのファミリーに入っているものを使う。
        let min_version = PACKAGE_VERSION {
            Anonymous: PACKAGE_VERSION_0 { Version: 0 },
        };

        let id = unsafe {
            let mut id = PWSTR::null();
            (api.try_create)(
                PSID::default(),
                PCWSTR(family.as_ptr()),
                min_version,
                PackageDependencyProcessorArchitectures_None,
                // プロセスが終われば消える。ディスクへは残さない。
                PackageDependencyLifetimeKind_Process,
                PCWSTR::null(),
                CreatePackageDependencyOptions_None,
                &mut id,
            )
            .ok()?;
            HeapString(id)
        };

        let mut context = PACKAGEDEPENDENCY_CONTEXT::default();
        let full_name = unsafe {
            let mut full_name = PWSTR::null();
            (api.add)(
                PCWSTR(id.0 .0),
                0,
                AddPackageDependencyOptions_None,
                &mut context,
                &mut full_name,
            )
            .ok()?;
            let full_name = HeapString(full_name);
            full_name.0.to_hstring()
        };

        Ok(Self {
            context,
            id,
            full_name,
        })
    }

    /// 取り付けたパッケージのフルネーム。
    pub fn package_full_name(&self) -> &HSTRING {
        &self.full_name
    }
}

impl fmt::Debug for PackageDependency {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PackageDependency")
            .field("package_full_name", &self.full_name)
            .finish_non_exhaustive()
    }
}

impl Drop for PackageDependency {
    fn drop(&mut self) {
        // 外せなくてもプロセスは終わるので、失敗は握りつぶす。
        if let Ok(api) = api() {
            unsafe {
                let _ = (api.remove)(self.context);
                let _ = (api.delete)(PCWSTR(self.id.0 .0));
            }
        }
    }
}
