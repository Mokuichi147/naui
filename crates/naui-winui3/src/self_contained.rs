//! exe に同梱した Windows App SDK ランタイムを使う (`self-contained` フィーチャー)。
//!
//! フレームワークパッケージを使うときは、どの WinRT クラスがどの DLL に
//! あるかをパッケージが OS へ知らせる。同梱したときは代わりに、
//! ビルド時に作った reg-free WinRT のマニフェスト
//! (exe の隣の `naui-winappsdk.manifest`) を
//! **アクティベーションコンテキスト**として有効にする。すると
//! `RoGetActivationFactory` が、exe の隣の DLL からクラスを引くようになる。
//!
//! マニフェストを exe に埋め込む方式 (MSBuild の self-contained 配置) と
//! 違って、利用者のビルド手順に手を入れずに済む。
//!
//! ## Windows の版による違い
//!
//! Windows 11 24H2 より前では、`Microsoft.WindowsAppRuntime.dll` が読み込まれた
//! 時点で、reg-free WinRT を Windows App SDK 自前の実装へ差し替える
//! (`dev/UndockedRegFreeWinRT/urfw.cpp`)。その実装は **exe に埋め込まれた
//! マニフェストしか読まない**が、見つからないクラスは OS の実装へ回す。
//! Windows 10 1903 以降の OS はアクティベーションコンテキストから引けるので
//! 動く (CI の Windows Server 2022 で確かめている)。OS に reg-free WinRT の
//! 無い 1809 では引けない。
//!
//! そこで有効にした直後に、同梱物のクラスが実際に引けるかを確かめる。
//! 引けなければ、マニフェストを exe に埋め込むよう促すエラーを返す。
//! 埋め込めば、Windows App SDK 自前の実装からも引ける。
//!
//! ## スレッド
//!
//! アクティベーションコンテキストはスレッドごとに積まれる。有効にした
//! スレッドから `std::thread::spawn` などで作ったスレッドは、作られた時点の
//! ものを引き継ぐ。それ以外 (スレッドプールなど) のスレッドで WinUI の
//! クラスを新しく作るには、マニフェストを exe に埋め込む。

use std::fmt;
use std::marker::PhantomData;
use std::path::Path;

use windows::core::{h, Error, IUnknown, Result, HSTRING, PCWSTR};
use windows::Win32::Foundation::HANDLE;
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

/// `lpAssemblyDirectory` を使う印 (winbase.h)。`windows` クレートでは
/// `Win32_System_WindowsProgramming` にあり、この 1 つのために大きな機能を
/// 引き込まない。
const ACTCTX_FLAG_ASSEMBLY_DIRECTORY_VALID: u32 = 0x4;

/// 同梱物から引けることを確かめる WinRT クラス。OS には無く、同梱した
/// InteractiveExperiences にだけある。
const PROBE_CLASS: &HSTRING = h!("Microsoft.UI.Dispatching.DispatcherQueueController");

/// 同梱物から組み立てた Windows App SDK ランタイム。
///
/// 落とすとアクティベーションコンテキストも外れるので、アプリが終わるまで
/// 持ち続ける。有効にしたスレッドで外さなければならないため、`Send` ではない。
pub struct SelfContained {
    context: HANDLE,
    cookie: usize,
    directory: String,
    _thread_bound: PhantomData<*const ()>,
}

impl SelfContained {
    /// 同梱した部品の出どころになった Windows App SDK の版。
    pub const WINDOWS_APP_SDK_VERSION: &'static str = packages::WINDOWS_APP_SDK;

    /// exe の隣に同梱物があれば、このスレッドで有効にする。
    ///
    /// 同梱物が無ければ `Ok(None)` を返すので、呼び出し側はフレームワーク
    /// パッケージ ([`crate::bootstrap`]) へ回る。あるのに使えなければ
    /// エラーにする。同梱物が壊れているか、Windows 10 1809 のように OS が
    /// reg-free WinRT を持たないのに、マニフェストが exe に埋め込まれていない
    /// (モジュールの説明を参照)。
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
        let context = unsafe { CreateActCtxW(&request) }?;
        let mut cookie = 0;
        if let Err(e) = unsafe { ActivateActCtx(Some(context), &mut cookie) } {
            unsafe { ReleaseActCtx(context) };
            return Err(e);
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
        unsafe { LoadLibraryExW(&runtime, None, LOAD_WITH_ALTERED_SEARCH_PATH) }?;

        probe().map_err(|e| {
            Error::new(
                e.code(),
                format!(
                    "同梱した Windows App SDK ランタイムから {PROBE_CLASS} を引けません ({})。\
                     Windows 10 1809 では、{} を exe に埋め込む必要があります \
                     (naui の README「マニフェストを exe に埋め込む」を参照)",
                    e.message(),
                    packages::MANIFEST_FILE,
                ),
            )
        })?;

        Ok(Some(this))
    }

    /// 同梱物を置いた場所 (exe の置き場)。
    pub fn directory(&self) -> &str {
        &self.directory
    }
}

/// 同梱物のクラスが実際に引けるかを確かめる。
///
/// このスレッドのアパートメントは決めない (naui はこの後で STA に入る)。
/// MTA を生かしておけば、COM を初期化していないスレッドからも暗黙の MTA
/// として呼べる。
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

    /// テストの exe の隣 (`target/<profile>/deps/`) にも build.rs が同梱物を
    /// 並べるので、有効にすれば WinUI のクラスが引ける。フレームワーク
    /// パッケージは取り付けていないので、引けたなら同梱物から来ている。
    ///
    /// `std::thread::spawn` で作ったスレッドが、作った側のアクティベーション
    /// コンテキストを引き継ぐことも確かめる。naui の別スレッドの API は
    /// これに頼っている。
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

    /// WinUI は 2.3.6 以上を同梱する。それより前の 2.x のパッケージには、
    /// 再配布を禁じるライセンスが誤って入っていた。
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

    /// MSBuild と同じ形のマニフェストになる。クラスは DLL ごとにまとまり、
    /// 同梱では使えないプッシュ通知のプロキシスタブは載らない。
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
