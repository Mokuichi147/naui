//! 同梱する Windows App SDK ランタイムの中身。
//!
//! `build.rs` と、実行時に同梱物を探す `self_contained` の両方から読む。
//! 両者で置き場の名前がずれないよう、ここにしか書かない。

/// 部品の出どころになった Windows App SDK の版。部品の版はこの版の
/// `Microsoft.WindowsAppSDK` NuGet パッケージが依存するものにそろえる。
pub(crate) const WINDOWS_APP_SDK: &str = "2.5.1";

/// 部品の NuGet パッケージ。
pub(crate) struct Component {
    /// NuGet のパッケージ ID (小文字)。
    pub(crate) id: &'static str,
    pub(crate) version: &'static str,
    /// `.nupkg` の SHA-512 (16 進)。nuget.org のカタログが載せる
    /// `packageHash` と同じもの。取ってきた中身をこれで確かめる。
    pub(crate) sha512: &'static str,
    /// reg-free WinRT のマニフェストの元になる `package.appxfragment` を
    /// 持つか。DWriteCore は WinRT クラスを持たないので無い。
    pub(crate) has_fragment: bool,
}

/// 同梱する部品。
///
/// WinUI は **2.3.6 以上**にする。2.0.12 から 2.3.2 までの WinUI の
/// パッケージには、誤って再配布を禁じるプレビュー版のライセンスが
/// 入っていた (microsoft/WindowsAppSDK#6654、2.3.6 で修正)。
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

/// exe の隣に置く、reg-free WinRT のマニフェスト。
///
/// `<exe 名>.exe.manifest` にはしない。その名前は OS が起動時に読むが、
/// exe がマニフェストを埋め込んでいると無視されるうえ、naui からは exe の
/// 名前を決められない。naui が実行時にアクティベーションコンテキストとして
/// 読み込む。**このファイルがあることが、同梱されていることの印になる。**
pub(crate) const MANIFEST_FILE: &str = "naui-winappsdk.manifest";

/// WinUI のコントロールが既定のスタイルを引くリソース。exe の隣に
/// `resources.pri` の名前で置く。無いと `Microsoft.UI.Xaml.dll` が
/// 起動中に例外で落ちる。
pub(crate) const RESOURCES_SOURCE: &str = "Microsoft.UI.Xaml.Controls.pri";
pub(crate) const RESOURCES_FILE: &str = "resources.pri";

/// 起動時に先に読み込む DLL。古い Windows 10 では、これが OS の代わりに
/// reg-free WinRT を受け持つ。
pub(crate) const RUNTIME_DLL: &str = "Microsoft.WindowsAppRuntime.dll";

/// `runtimes-framework/win-<arch>/native/` のうち同梱しないもの。
///
/// MSBuild の self-contained 配置 (`Microsoft.WindowsAppSDK.SelfContained.targets`)
/// が出力へコピーしない実行ファイル。ランタイムを OS へ導入するためのもので、
/// 同梱した場合には使わない。
pub(crate) const EXCLUDED_FILES: &[&str] = &["DeploymentAgent.exe"];
