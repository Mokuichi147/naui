//! The contents of the bundled Windows App SDK runtime.
//!
//! Read both from `build.rs` and from `self_contained`, which looks for the bundled
//! files at run time. It is written only here, so the two never disagree on where
//! things go.

/// The Windows App SDK version the components come from. The component versions
/// match those the `Microsoft.WindowsAppSDK` NuGet package of this version depends
/// on.
pub(crate) const WINDOWS_APP_SDK: &str = "2.5.1";

/// A component's NuGet package.
pub(crate) struct Component {
    /// The NuGet package ID (lowercase).
    pub(crate) id: &'static str,
    pub(crate) version: &'static str,
    /// The SHA-512 of the `.nupkg` (hex). The same as the `packageHash` listed in
    /// nuget.org's catalog. The downloaded contents are verified against it.
    pub(crate) sha512: &'static str,
    /// Whether it has a `package.appxfragment`, the source of the reg-free WinRT
    /// manifest. DWriteCore has no WinRT classes, so it has none.
    pub(crate) has_fragment: bool,
}

/// The bundled components.
///
/// WinUI must be **2.3.6 or later**. The WinUI packages from 2.0.12 through 2.3.2
/// mistakenly contained a preview license that forbids redistribution
/// (microsoft/WindowsAppSDK#6654, fixed in 2.3.6).
pub(crate) const COMPONENTS: &[Component] = &[
    Component {
        id: "microsoft.windowsappsdk.foundation",
        version: "2.3.12",
        sha512: "057c7d56ee994e7d85e8f740c7589a3dad9f7421723de80f219fe6e3ef03329d94af7a2bbd454003f7f64aff6607291fc953c3f5d8763502a3d091584bfe30d6",
        has_fragment: true,
    },
    Component {
        id: "microsoft.windowsappsdk.interactiveexperiences",
        version: "2.1.9",
        sha512: "dd6429dedd2dc6183e447c358f850436d52f881ab99fe6c39c2aa0634c0386846096fa112b53ba62cbd638c5e72f97b6231c0856ec1917409786f0f9a4a51e07",
        has_fragment: true,
    },
    Component {
        id: "microsoft.windowsappsdk.winui",
        version: "2.3.9",
        sha512: "b4e75550f15f9197b9f812711859c9a6563ed5ed42168834c6acfc94442f9227359cc12f874055cd4ff3f3a18166d0ef2f18c2360e544d7983b4e06ad270a055",
        has_fragment: true,
    },
    Component {
        id: "microsoft.windowsappsdk.dwrite",
        version: "2.1.0",
        sha512: "966d6624c85003a1d5ef4d8ea26dd88e6edee40d1951f53a262d0914386061250fef502505ede140c30d61cd680aea8e77f6fb51d1e5e9a2705c6ce43fedb41f",
        has_fragment: false,
    },
];

/// The reg-free WinRT manifest placed next to the exe.
///
/// It is not named `<exe name>.exe.manifest`. The OS reads that name at startup, but
/// ignores it when the exe has an embedded manifest, and naui cannot decide the exe's
/// name anyway. naui loads it at run time as an activation context. **The presence
/// of this file is the marker that the runtime is bundled.**
pub(crate) const MANIFEST_FILE: &str = "naui-winappsdk.manifest";

/// The resources from which WinUI controls take their default styles. Placed next to
/// the exe as `resources.pri`. Without it, `Microsoft.UI.Xaml.dll` crashes with an
/// exception during startup.
pub(crate) const RESOURCES_SOURCE: &str = "Microsoft.UI.Xaml.Controls.pri";
pub(crate) const RESOURCES_FILE: &str = "resources.pri";

/// The name under which the components' license terms (each `.nupkg`'s
/// `license.txt`) are placed.
///
/// Shipping them together with the bundled files makes it easier to meet the
/// redistribution condition (having users agree to equivalent or stronger terms). If
/// they differ per component, all of them are listed.
pub(crate) const LICENSE_FILE: &str = "WindowsAppSDK-LICENSE.txt";

/// The name under which the open source notices contained in the components (the
/// `NOTICE.txt` at the top of each `.nupkg`) are placed.
pub(crate) const NOTICE_FILE: &str = "WindowsAppSDK-NOTICE.txt";

/// The DLL loaded first at startup. On old Windows 10, it handles reg-free WinRT in
/// place of the OS.
pub(crate) const RUNTIME_DLL: &str = "Microsoft.WindowsAppRuntime.dll";

/// Files under `runtimes-framework/win-<arch>/native/` that are not bundled.
///
/// Executables that MSBuild's self-contained deployment
/// (`Microsoft.WindowsAppSDK.SelfContained.targets`) does not copy to the output.
/// They are for installing the runtime on the OS and are not used when bundled.
pub(crate) const EXCLUDED_FILES: &[&str] = &["DeploymentAgent.exe"];
