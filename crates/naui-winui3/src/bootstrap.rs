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
    /// 版の付かない OS 同梱の系統。Windows 11 の初期のビルドはこの名前で、
    /// 新しいビルドでは `CBS.1.6` や `CBS.2` のように版が付く。
    Cbs,
    Cbs1_6,
    Cbs1_8,
    Cbs2,
}

impl Version {
    /// 知っている版を新しい順に並べたもの。
    ///
    /// ブートストラップの DLL を探すときにも使う。どの版のフレームワーク
    /// パッケージに入っているものでも、動的依存の API としては同じ。
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
            Self::Cbs => "CBS",
            Self::Cbs1_6 => "CBS.1.6",
            Self::Cbs1_8 => "CBS.1.8",
            Self::Cbs2 => "CBS.2",
        }
    }

    /// このバージョンのフレームワークパッケージのファミリー名。
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

/// ブートストラップの DLL。`Mdd` の付いた 4 関数を持つ。
const BOOTSTRAP_DLL: &str = "Microsoft.WindowsAppRuntime.dll";

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

/// exe と同じ場所に置かれた DLL を読む。自己完結配置のときに当たる。
///
/// 名前だけで `LoadLibrary` すると PATH の上にある別物を掴みかねないので、
/// exe の場所を引いてから絶対パスで読む。
///
/// **フレームワークパッケージの中の DLL はここでは見つからない。** パッケージの
/// 置き場は DLL の探索経路に入っていないためで、そちらは
/// [`load_from_framework_package`] が実パスを解決して読む。
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

/// いま動いている exe の置き場。
fn executable_directory() -> Option<String> {
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

/// 入っているフレームワークパッケージを探し、その中の DLL を実パスで読む。
///
/// 動的依存を張る前なので、パッケージの中の DLL は名前だけでは読めない。
/// 公式のブートストラップと同じく、パッケージの置き場を先に引いてから
/// 絶対パスで読む。`LOAD_WITH_ALTERED_SEARCH_PATH` を付けるのは、その DLL が
/// 同じ場所にある別の DLL へ依存しているため。
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

/// そのファミリーで入っているパッケージの置き場。
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

/// パッケージのフルネームから置き場を引く。
///
/// # Safety
///
/// `full_name` は [`FindPackagesByPackageFamily`] が書いた、生きている
/// NUL 終端の文字列であること。
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

#[cfg(test)]
mod tests {
    use super::*;

    /// 入っていないファミリーでは、何も返さずに済ませる。
    ///
    /// `FindPackagesByPackageFamily` は 0 件のときも「足りない」とは言わない
    /// ので、そこで打ち切れているかを見る。
    #[test]
    fn unknown_family_has_no_packages() {
        let family = HSTRING::from("Naui.ThisFamilyIsNotInstalled_8wekyb3d8bbwe");
        assert!(package_paths(&family).is_empty());
    }

    /// Windows App SDK が入っている環境では、置き場が引けて、そこに
    /// ブートストラップの DLL があり、実際に読める。
    ///
    /// この経路は Windows 11 では使われない (`kernelbase.dll` が先に当たる)
    /// ため、テストから直に呼んで確かめる。入っていない環境では何も主張
    /// しないので、ランタイム抜きの CI でも通る。
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
