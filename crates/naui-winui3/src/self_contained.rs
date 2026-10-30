//! Uses a Windows App SDK runtime bundled with the exe (the `self-contained`
//! feature).
//!
//! When using a framework package, the package tells the OS which WinRT class lives
//! in which DLL. When bundled, the reg-free WinRT manifest created at build time
//! (`naui-winappsdk.manifest` next to the exe) is instead enabled as an **activation
//! context**. Then `RoGetActivationFactory` looks up classes from the DLLs next to
//! the exe.
//!
//! Unlike embedding the manifest in the exe (MSBuild's self-contained deployment),
//! it requires no changes to the user's build steps.
//!
//! ## Differences between Windows versions
//!
//! Before Windows 11 24H2, as soon as `Microsoft.WindowsAppRuntime.dll` is loaded,
//! reg-free WinRT is replaced with the Windows App SDK's own implementation
//! (`dev/UndockedRegFreeWinRT/urfw.cpp`). That implementation **reads only manifests
//! embedded in the exe**, but passes classes it cannot find on to the OS's
//! implementation. OSes from Windows 10 1903 on can look them up from the activation
//! context, so it works (verified on CI's Windows Server 2022). On 1809, whose OS has
//! no reg-free WinRT, they cannot be looked up.
//!
//! So right after enabling it, whether the bundled classes can actually be looked up
//! is checked. If not, it returns an error urging you to embed the manifest in the
//! exe. Once embedded, they can be looked up from the Windows App SDK's own
//! implementation too.
//!
//! ## Threads
//!
//! Activation contexts are stacked per thread. Threads created from the enabling
//! thread with `std::thread::spawn` and the like inherit the one active at creation.
//! To newly create WinUI classes on other threads (a thread pool, for example),
//! embed the manifest in the exe.

use std::fmt;
use std::marker::PhantomData;
use std::path::Path;

use windows::core::{h, Error, IUnknown, Result, HSTRING, PCWSTR};
use windows::Win32::Foundation::{ERROR_FILENAME_EXCED_RANGE, HANDLE};
use windows::Win32::System::ApplicationInstallationAndServicing::{
    ActivateActCtx, CreateActCtxW, DeactivateActCtx, ReleaseActCtx, ACTCTXW,
};
use windows::Win32::System::Com::{CoDecrementMTAUsage, CoIncrementMTAUsage};
use windows::Win32::System::LibraryLoader::{LoadLibraryExW, LOAD_WITH_ALTERED_SEARCH_PATH};
use windows::Win32::System::WinRT::RoGetActivationFactory;

#[allow(dead_code)]
#[path = "../build/packages.rs"]
mod packages;

// build.rs がマニフェストを作るのに使うもの。テストから直に確かめる。
#[cfg(test)]
#[path = "../build/manifest.rs"]
mod manifest;

/// The flag for using `lpAssemblyDirectory` (winbase.h). In the `windows` crate it
/// is in `Win32_System_WindowsProgramming`, and a large feature is not pulled in just
/// for this one.
const ACTCTX_FLAG_ASSEMBLY_DIRECTORY_VALID: u32 = 0x4;

/// The WinRT class used to check that the bundle can be looked up from. It is not in
/// the OS, only in the bundled InteractiveExperiences.
const PROBE_CLASS: &HSTRING = h!("Microsoft.UI.Dispatching.DispatcherQueueController");

/// A Windows App SDK runtime assembled from the bundled files.
///
/// Dropping it also deactivates the activation context, so it is held until the app
/// ends. It must be deactivated on the thread that activated it, so it is not
/// `Send`.
pub struct SelfContained {
    context: HANDLE,
    cookie: usize,
    directory: String,
    _thread_bound: PhantomData<*const ()>,
}

impl SelfContained {
    /// The Windows App SDK version the bundled components come from.
    pub const WINDOWS_APP_SDK_VERSION: &'static str = packages::WINDOWS_APP_SDK;

    /// The name of the bundle's manifest placed next to the exe. Its presence is the
    /// marker that the runtime is bundled.
    pub const MANIFEST_FILE: &'static str = packages::MANIFEST_FILE;

    /// If a bundle is next to the exe, activates it on this thread.
    ///
    /// Returns `Ok(None)` if there is no bundle. Whether to fall back to a framework
    /// package ([`crate::bootstrap`]) then is up to the caller (naui does not, and errors
    /// instead). It is an error if the bundle is there but cannot be used: either the
    /// bundle is broken, or, as on Windows 10 1809, the OS has no reg-free WinRT and the
    /// manifest is not embedded in the exe (see the module docs).
    pub fn activate() -> Result<Option<Self>> {
        let Some(directory) = crate::bootstrap::executable_directory() else {
            return Ok(None);
        };
        let manifest = format!("{directory}\\{}", packages::MANIFEST_FILE);
        if !Path::new(&manifest).is_file() {
            return Ok(None);
        }

        let source = HSTRING::from(manifest);
        let assembly_directory = HSTRING::from(directory.as_str());
        let request = ACTCTXW {
            cbSize: std::mem::size_of::<ACTCTXW>() as u32,
            dwFlags: ACTCTX_FLAG_ASSEMBLY_DIRECTORY_VALID,
            lpSource: PCWSTR(source.as_ptr()),
            // マニフェストに書いた DLL の名前は、ここからの相対で解決される。
            lpAssemblyDirectory: PCWSTR(assembly_directory.as_ptr()),
            ..Default::default()
        };
        let context = unsafe { CreateActCtxW(&request) }
            .map_err(|e| explain(e, "load the bundled manifest", &directory))?;
        let mut cookie = 0;
        if let Err(e) = unsafe { ActivateActCtx(Some(context), &mut cookie) } {
            unsafe { ReleaseActCtx(context) };
            return Err(explain(e, "activate the bundled manifest", &directory));
        }
        let this = Self {
            context,
            cookie,
            directory,
            _thread_bound: PhantomData,
        };

        // MSBuild の self-contained 配置が起動時に必ず行うことに合わせる。
        // 24H2 より前の Windows では、この DLL が reg-free WinRT を受け持つ。
        // 読み込んだままにしておく (WinUI がプロセスの終わりまで使う)。
        let runtime = HSTRING::from(format!("{}\\{}", this.directory, packages::RUNTIME_DLL));
        unsafe { LoadLibraryExW(&runtime, None, LOAD_WITH_ALTERED_SEARCH_PATH) }.map_err(|e| {
            let step = format!("load {}", packages::RUNTIME_DLL);
            explain(e, &step, &this.directory)
        })?;

        probe().map_err(|e| {
            Error::new(
                e.code(),
                format!(
                    "cannot look up {PROBE_CLASS} in the bundled Windows App SDK runtime ({}). \
                     On Windows 10 1809, {} must be embedded in the exe \
                     (see \"Embedding the manifest in the exe\" in naui's \
                     docs/windows-self-contained.md)",
                    e.message(),
                    packages::MANIFEST_FILE,
                ),
            )
        })?;

        Ok(Some(this))
    }

    /// Where the bundle is placed (the exe's location).
    pub fn directory(&self) -> &str {
        &self.directory
    }
}

/// Adds which step it was to a failure in the steps of attaching the bundle.
///
/// When a bundled file's path exceeds 260 characters (MAX_PATH), loading fails with
/// `ERROR_FILENAME_EXCED_RANGE` (this happened loading
/// `Microsoft.WindowsAppRuntime.dll` when the exe's location was 224 characters).
/// Adding `\\?\` to the path does not avoid it, so it urges shortening the location.
fn explain(e: Error, step: &str, directory: &str) -> Error {
    let mut message = format!("failed to {step} ({})", e.message());
    if e.code() == ERROR_FILENAME_EXCED_RANGE.to_hresult() {
        message.push_str(&format!(
            ". The path of the folder containing the exe ({} characters) is too long. \
             Place it in a folder of about 200 characters or fewer, so the paths of the \
             bundled files fit within 260 characters",
            directory.encode_utf16().count(),
        ));
    }
    Error::new(e.code(), message)
}

/// Checks whether the bundled classes can actually be looked up.
///
/// It does not decide this thread's apartment (naui enters the STA afterwards).
/// Keeping an MTA alive lets threads that have not initialized COM call in as the
/// implicit MTA.
fn probe() -> Result<()> {
    let usage = unsafe { CoIncrementMTAUsage() }?;
    let resolved = unsafe { RoGetActivationFactory::<IUnknown>(PROBE_CLASS) }.map(drop);
    unsafe {
        let _ = CoDecrementMTAUsage(usage);
    }
    resolved
}

impl fmt::Debug for SelfContained {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SelfContained")
            .field("directory", &self.directory)
            .field("windows_app_sdk", &Self::WINDOWS_APP_SDK_VERSION)
            .finish_non_exhaustive()
    }
}

impl Drop for SelfContained {
    fn drop(&mut self) {
        // 外せなくてもプロセスは終わるので、失敗は握りつぶす。
        unsafe {
            let _ = DeactivateActCtx(0, self.cookie);
            ReleaseActCtx(self.context);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};

    const CLASS: &HSTRING = PROBE_CLASS;

    fn factory_resolves() -> bool {
        unsafe {
            let _ = RoInitialize(RO_INIT_MULTITHREADED);
            RoGetActivationFactory::<IUnknown>(CLASS).is_ok()
        }
    }

    /// build.rs also places the bundle next to the test exe (`target/<profile>/deps/`),
    /// so once activated, WinUI classes can be looked up. No framework package is
    /// attached, so if they can be looked up they come from the bundle.
    ///
    /// It also checks that threads created with `std::thread::spawn` inherit the
    /// creator's activation context. naui's other-thread API relies on this.
    #[test]
    fn bundled_classes_resolve_here_and_on_spawned_threads() {
        assert!(
            !factory_resolves(),
            "有効にする前から {CLASS} が引ける。同梱物を確かめたことにならない"
        );
        let bundle = SelfContained::activate()
            .expect("同梱物を有効にできない")
            .expect("テストの exe の隣に同梱物が無い");
        assert!(
            factory_resolves(),
            "有効にしたスレッドで {CLASS} が引けない"
        );
        let spawned = std::thread::spawn(factory_resolves)
            .join()
            .expect("スレッドが落ちた");
        assert!(spawned, "作ったスレッドで {CLASS} が引けない");
        drop(bundle);
    }

    /// WinUI 2.3.6 or later is bundled. The 2.x packages before it mistakenly contained
    /// a license forbidding redistribution.
    #[test]
    fn bundled_winui_is_redistributable() {
        let winui = packages::COMPONENTS
            .iter()
            .find(|c| c.id == "microsoft.windowsappsdk.winui")
            .expect("WinUI が部品に無い");
        let version: Vec<u32> = winui
            .version
            .split('.')
            .map(|part| part.parse().expect("版が数字でない"))
            .collect();
        assert!(
            version >= vec![2, 3, 6],
            "WinUI {} は再配布できない",
            winui.version
        );
    }

    /// The manifest has the same shape as MSBuild's. Classes are grouped per DLL, and
    /// the push notification proxy stubs, which cannot be used when bundled, are not
    /// included.
    #[test]
    fn manifest_maps_classes_to_their_dlls() {
        let fragment = r#"<?xml version="1.0" encoding="utf-8"?>
<Fragment xmlns="http://schemas.microsoft.com/appx/manifest/foundation/windows10">
  <Extensions>
    <Extension Category="windows.activatableClass.inProcessServer">
      <InProcessServer>
        <Path>Microsoft.UI.Xaml.dll</Path>
        <ActivatableClass ActivatableClassId="Microsoft.UI.Xaml.Application" ThreadingModel="both" />
        <ActivatableClass ActivatableClassId="Microsoft.UI.Xaml.Window" ThreadingModel="both" />
      </InProcessServer>
    </Extension>
    <Extension Category="windows.activatableClass.proxyStub">
      <ProxyStub ClassId="11111111-2222-3333-4444-555555555555">
        <Path>Sample.ProxyStub.dll</Path>
        <Interface Name="ISample" InterfaceId="AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE" />
      </ProxyStub>
    </Extension>
    <Extension Category="windows.activatableClass.proxyStub">
      <ProxyStub ClassId="66666666-2222-3333-4444-555555555555">
        <Path>PushNotificationsLongRunningTask.ProxyStub.dll</Path>
        <Interface Name="IPush" InterfaceId="FFFFFFFF-BBBB-CCCC-DDDD-EEEEEEEEEEEE" />
      </ProxyStub>
    </Extension>
  </Extensions>
</Fragment>"#;
        let manifest = manifest::generate(&[fragment]).expect("マニフェストを作れない");
        let expected = "    <asmv3:file name='Microsoft.UI.Xaml.dll'>
        <winrtv1:activatableClass name='Microsoft.UI.Xaml.Application' threadingModel='both'/>
        <winrtv1:activatableClass name='Microsoft.UI.Xaml.Window' threadingModel='both'/>
    </asmv3:file>
    <asmv3:file name='Sample.ProxyStub.dll'>
        <asmv3:comClass clsid='{11111111-2222-3333-4444-555555555555}'/>
        <asmv3:comInterfaceProxyStub name='ISample' iid='{AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE}'/>
    </asmv3:file>
</assembly>
";
        assert!(manifest.ends_with(expected), "{manifest}");
        assert!(!manifest.contains("PushNotifications"), "{manifest}");
        // OS が読めるよう、XML として正しいこと。
        roxmltree::Document::parse(&manifest).expect("XML として読めない");
    }
}
