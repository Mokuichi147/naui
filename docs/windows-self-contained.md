# Windows App SDK ランタイムを同梱する

`self-contained` フィーチャー (既定は無効) を有効にすると、ランタイムを exe と
一緒に配れるので、利用者が別に導入する必要がなくなります。

```toml
[dependencies]
naui = { version = "0.9", features = ["self-contained"] }
```

ビルド時に Windows App SDK 2.5.1 の部品を nuget.org から取得し、SHA-512 で
照合してから exe の出力先へ並べます。配布するときは
`target/<profile>/naui-winappsdk/` の中身 (約 60MB、ライセンス条項と告知を含む)
をすべて exe の隣へ置いてください。

**同梱物が無いと、導入済みのランタイムへは切り替えずにエラーにします。**
手元では動いてしまい、配り忘れに配布先で初めて気づく、ということを防ぐためです。

Gallery で試すには `cargo run -p gallery --features self-contained` を実行します。

## ライセンス

- **このフィーチャーを有効にすると、取得する各パッケージのライセンスに同意した
  ことになります。** naui のビルドは確認なしに取得します。条文と再配布の条件は、
  各 `.nupkg` の `license.txt` と nuget.org の各パッケージのページにあります。
- WinUI 2.0.12〜2.3.2 には再配布を禁じるライセンスが誤って入っていたため
  ([microsoft/WindowsAppSDK#6654](https://github.com/microsoft/WindowsAppSDK/issues/6654))、
  修正後の 2.3.6 以上だけを同梱します。

## 制約

- 並べるファイルは MSBuild の self-contained 配置と同じです
  (`DeploymentAgent.exe` は除きます)。
- プッシュ通知など、同梱では使えない Windows App SDK の機能があります
  (naui は使っていません)。
- ランタイムの不具合やセキュリティの修正は、アプリの配り直しで届けます。
- exe を置くフォルダーのパスはおおむね 200 文字以内にしてください。260 文字を
  超えるファイルがあると起動に失敗します。
- exe の隣に `resources.pri` を置くため、アプリ自身の `resources.pri` とは
  両立しません。
- オフラインでは、4 つの `.nupkg` を置いたフォルダーを環境変数
  `NAUI_WINAPPSDK_PACKAGES` に指定します。ファイル名は
  `microsoft.windowsappsdk.winui.2.3.9.nupkg` のような小文字の形です。

## マニフェストを exe に埋め込む

naui は起動時に同梱物のマニフェストを読み込みます。Windows 10 1903 以降は
これで動きます (CI の Windows Server 2022・2025 で確認)。1809 では Windows App
SDK が **exe に埋め込まれたマニフェストだけ**を見るため、埋め込みが要る見込み
です (未確認)。埋め込まずにランタイムが見つからないときは、`naui::run` が
埋め込みを促すエラーを返します。

依存クレートの側からはアプリの exe へ埋め込めないので、アプリの `build.rs` で
埋め込みます。MSVC のリンカーが要ります。

```rust
// build.rs
fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc")
    {
        // naui のビルドが target/<profile>/naui-winappsdk/ に置くマニフェスト。
        let out_dir = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
        let manifest = out_dir.ancestors().nth(3).unwrap()
            .join("naui-winappsdk").join("naui-winappsdk.manifest");
        println!("cargo:rustc-link-arg-bins=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg-bins=/MANIFESTINPUT:{}", manifest.display());
    }
}
```

フィーチャーで同梱を切り替える例は [`examples/gallery/build.rs`](../examples/gallery/build.rs) にあります。
埋め込むと、スレッドプールなど naui を起動したのとは別のスレッドからも WinUI の
クラスを作れるようになります。

## build-dir を分けている場合

Cargo はビルドスクリプトに exe の出力先 (target-dir) を知らせないので、naui は
build-dir から求めます。既定では同じ場所なので何もしなくて構いません。

`build.build-dir` (`CARGO_BUILD_BUILD_DIR`) で分けている場合は、target-dir を
環境変数 `NAUI_WINAPPSDK_TARGET_DIR` に絶対パスで指定してください
(`CARGO_TARGET_DIR` が絶対パスならそれを使います)。どちらも無ければビルドを
止めますが、設定ファイルの `build.build-dir` はビルドスクリプトから見えないため
止められず、起動時に同梱物が無いというエラーになります。

CI のキャッシュ (`Swatinem/rust-cache` など) の復元で並べたファイルが消えても、
次のビルドで並べ直します。
