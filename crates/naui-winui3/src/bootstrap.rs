//! Attaches the Windows App SDK runtime to the process (dynamic dependency).
//!
//! An unpackaged app cannot activate WinRT classes unless it **itself** adds the
//! framework package containing WinUI 3 to the process's package graph. The API for
//! that is the "dynamic dependency", which lives in one of:
//!
//! - Windows 11 and later: `TryCreatePackageDependency` and friends in
//!   `kernelbase.dll`
//! - otherwise: the functions of the same names with `Mdd` in
//!   `Microsoft.WindowsAppRuntime.dll`, installed by the Windows App SDK
//!
//! The `windows` crate has only declarations for both and cannot link them, so they
//! are looked up with `GetProcAddress`.

use std::fmt;
use std::sync::OnceLock;

use windows::core::{w, Error, Result, HRESULT, HSTRING, PCSTR, PCWSTR, PWSTR};
use windows::Win32::Foundation::{
    FreeLibrary, ERROR_INSUFFICIENT_BUFFER, ERROR_MOD_NOT_FOUND, ERROR_SUCCESS, FARPROC, HMODULE,
};
use windows::Win32::Security::PSID;
use windows::Win32::Storage::Packaging::Appx::{
    AddPackageDependencyOptions, AddPackageDependencyOptions_None, CreatePackageDependencyOptions,
    CreatePackageDependencyOptions_None, FindPackagesByPackageFamily, GetPackagePathByFullName,
    PackageDependencyLifetimeKind, PackageDependencyLifetimeKind_Process,
    PackageDependencyProcessorArchitectures, PackageDependencyProcessorArchitectures_None,
    PACKAGEDEPENDENCY_CONTEXT, PACKAGE_FILTER_DIRECT, PACKAGE_FILTER_HEAD, PACKAGE_VERSION,
    PACKAGE_VERSION_0,
};
use windows::Win32::System::LibraryLoader::{
    GetModuleFileNameW, GetModuleHandleExW, GetProcAddress, LoadLibraryExW,
    LOAD_WITH_ALTERED_SEARCH_PATH,
};
use windows::Win32::System::Memory::{GetProcessHeap, HeapFree, HEAP_FLAGS};

/// A Windows App SDK version. It becomes part of the package family name.
///
/// Those with `Cbs` are the line shipped with the OS, with family names different
/// from the regular framework packages.
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
    /// The unversioned line shipped with the OS. Early Windows 11 builds use this name;
    /// newer builds add versions, as in `CBS.1.6` or `CBS.2`.
    Cbs,
    Cbs1_6,
    Cbs1_8,
    Cbs2,
}

impl Version {
    /// The known versions, newest first.
    ///
    /// Also used when looking for the bootstrap DLL. Whichever version's framework
    /// package it is in, it is the same as a dynamic dependency API.
    pub const ALL: &'static [Version] = &[
        Self::V2,
        Self::Cbs2,
        Self::V1_8,
        Self::Cbs1_8,
        Self::V1_7,
        Self::V1_6,
        Self::Cbs1_6,
        Self::V1_5,
        Self::V1_4,
        Self::V1_3,
        // 版が付かないぶん、どの世代か分からない。最後に回す。
        Self::Cbs,
    ];

    /// The string embedded in the package family name.
    const fn tag(self) -> &'static str {
        match self {
            Self::V1_3 => "1.3",
            Self::V1_4 => "1.4",
            Self::V1_5 => "1.5",
            Self::V1_6 => "1.6",
            Self::V1_7 => "1.7",
            Self::V1_8 => "1.8",
            Self::V2 => "2",
            Self::Cbs => "CBS",
            Self::Cbs1_6 => "CBS.1.6",
            Self::Cbs1_8 => "CBS.1.8",
            Self::Cbs2 => "CBS.2",
        }
    }

    /// The family name of this version's framework package.
    fn family_name(self) -> HSTRING {
        HSTRING::from(format!(
            "Microsoft.WindowsAppRuntime.{}_8wekyb3d8bbwe",
            self.tag()
        ))
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

/// Bundles the four dynamic dependency functions.
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

/// The bootstrap DLL. It has the four functions with `Mdd`.
const BOOTSTRAP_DLL: &str = "Microsoft.WindowsAppRuntime.dll";

/// Looks up the four names from the DLL. If even one is missing, it cannot be used.
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

/// Loads the DLL placed in the same location as the exe. This is what matches for
/// self-contained deployment.
///
/// Calling `LoadLibrary` by name alone could grab something else earlier on PATH, so
/// the exe's location is looked up first and the DLL is loaded by absolute path.
///
/// **DLLs inside framework packages are not found here.** The package's location is
/// not on the DLL search path; for those, [`load_from_framework_package`] resolves
/// the real path and loads it.
fn load_beside_executable() -> Option<Api> {
    let directory = executable_directory()?;
    let path = HSTRING::from(format!("{directory}\\{BOOTSTRAP_DLL}"));
    let library = unsafe { LoadLibraryExW(&path, None, LOAD_WITH_ALTERED_SEARCH_PATH) }.ok()?;
    match load(library, "Mdd") {
        Some(api) => Some(api),
        None => {
            unsafe {
                let _ = FreeLibrary(library);
            }
            None
        }
    }
}

/// Where the running exe is.
pub(crate) fn executable_directory() -> Option<String> {
    let mut buffer = [0u16; 32768];
    let length = unsafe { GetModuleFileNameW(None, &mut buffer) } as usize;
    // 収まらなかったときは端まで書いて切り捨てるので、満杯なら諦める。
    if length == 0 || length >= buffer.len() {
        return None;
    }
    let path = String::from_utf16_lossy(&buffer[..length]);
    let (directory, _) = path.rsplit_once('\\')?;
    Some(directory.to_string())
}

/// Finds an installed framework package and loads the DLL inside it by its real
/// path.
///
/// This is before the dynamic dependency is set up, so DLLs inside the package cannot
/// be loaded by name. As the official bootstrapper does, the package's location is
/// looked up first and the DLL is loaded by absolute path.
/// `LOAD_WITH_ALTERED_SEARCH_PATH` is given because that DLL depends on another DLL
/// in the same location.
fn load_from_framework_package() -> Option<Api> {
    for version in Version::ALL {
        for directory in package_paths(&version.family_name()) {
            let path = HSTRING::from(format!("{directory}\\{BOOTSTRAP_DLL}"));
            // 別のアーキテクチャのパッケージもここへ来る。読めなければ次へ。
            let Ok(library) =
                (unsafe { LoadLibraryExW(&path, None, LOAD_WITH_ALTERED_SEARCH_PATH) })
            else {
                continue;
            };
            if let Some(api) = load(library, "Mdd") {
                return Some(api);
            }
            unsafe {
                let _ = FreeLibrary(library);
            }
        }
    }
    None
}

/// The locations of the installed packages of that family.
fn package_paths(family: &HSTRING) -> Vec<String> {
    const FILTERS: u32 = PACKAGE_FILTER_HEAD | PACKAGE_FILTER_DIRECT;
    let mut count = 0;
    let mut buffer_length = 0;
    unsafe {
        // 1 回目は要る大きさを聞くだけ。入っていなければ 0 件で成功が返るので、
        // 「足りない」と言われたときだけ先へ進む。
        let asked = FindPackagesByPackageFamily(
            family,
            FILTERS,
            &mut count,
            None,
            &mut buffer_length,
            None,
            None,
        );
        if asked != ERROR_INSUFFICIENT_BUFFER || count == 0 {
            return Vec::new();
        }
        let mut names = vec![PWSTR::null(); count as usize];
        let mut buffer = vec![0u16; buffer_length as usize];
        let found = FindPackagesByPackageFamily(
            family,
            FILTERS,
            &mut count,
            Some(names.as_mut_ptr()),
            &mut buffer_length,
            Some(PWSTR(buffer.as_mut_ptr())),
            None,
        );
        if found != ERROR_SUCCESS {
            return Vec::new();
        }
        names
            .into_iter()
            .filter_map(|name| package_path(name))
            .collect()
    }
}

/// Looks up the location from a package's full name.
///
/// # Safety
///
/// `full_name` must be a live NUL-terminated string written by
/// [`FindPackagesByPackageFamily`].
unsafe fn package_path(full_name: PWSTR) -> Option<String> {
    let full_name = PCWSTR(full_name.0);
    let mut length = 0;
    unsafe {
        let asked = GetPackagePathByFullName(full_name, &mut length, None);
        if asked != ERROR_INSUFFICIENT_BUFFER || length == 0 {
            return None;
        }
        let mut path = vec![0u16; length as usize];
        let got = GetPackagePathByFullName(full_name, &mut length, Some(PWSTR(path.as_mut_ptr())));
        if got != ERROR_SUCCESS {
            return None;
        }
        // `length` は NUL を含んだ長さ。
        Some(String::from_utf16_lossy(&path[..length as usize - 1]))
    }
}

fn api() -> Result<&'static Api> {
    API.get_or_init(|| {
        // Windows 11 以降は OS が持っている。接頭辞なしの名前。
        let mut library = HMODULE::default();
        if unsafe { GetModuleHandleExW(0, w!("kernelbase.dll"), &mut library) }.is_ok() {
            if let Some(api) = load(library, "") {
                return Some(api);
            }
        }
        // それ以外 (Windows 10 など) は Windows App SDK が持ち込む DLL から引く。
        load_beside_executable().or_else(load_from_framework_package)
    })
    .as_ref()
    .ok_or_else(|| Error::from_hresult(HRESULT::from_win32(ERROR_MOD_NOT_FOUND.0)))
}

/// A string on the process heap, returned by the dynamic dependency API.
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

/// The attached Windows App SDK runtime.
///
/// Dropping it also detaches it, so it is held until the app ends.
pub struct PackageDependency {
    context: PACKAGEDEPENDENCY_CONTEXT,
    id: HeapString,
    full_name: HSTRING,
}

impl PackageDependency {
    /// Attaches the runtime of the given version.
    ///
    /// It fails if that version is not installed, so the caller tries them newest
    /// first.
    pub fn initialize(version: Version) -> Result<Self> {
        let api = api()?;
        let family = version.family_name();
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

    /// The full name of the attached package.
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

#[cfg(test)]
mod tests {
    use super::*;

    /// For a family that is not installed, it returns nothing and is done with it.
    ///
    /// `FindPackagesByPackageFamily` does not say "insufficient" even when there are 0
    /// results, so this checks that it stops there.
    #[test]
    fn unknown_family_has_no_packages() {
        let family = HSTRING::from("Naui.ThisFamilyIsNotInstalled_8wekyb3d8bbwe");
        assert!(package_paths(&family).is_empty());
    }

    /// In an environment with the Windows App SDK installed, the location can be looked
    /// up, the bootstrap DLL is there, and it can actually be loaded.
    ///
    /// This path is not used on Windows 11 (`kernelbase.dll` matches first), so the test
    /// calls it directly to check it. In environments without it installed it asserts
    /// nothing, so it also passes on CI without the runtime.
    #[test]
    fn installed_framework_package_carries_a_loadable_bootstrap_dll() {
        let mut installed = false;
        for version in Version::ALL {
            for directory in package_paths(&version.family_name()) {
                installed = true;
                let path = std::path::Path::new(&directory).join(BOOTSTRAP_DLL);
                assert!(path.is_file(), "{} が無い", path.display());
            }
        }
        if installed {
            assert!(
                load_from_framework_package().is_some(),
                "置き場は引けたのに DLL から動的依存の API を取れない"
            );
        }
    }
}
