# winui3-bindgen

`crates/naui-winui3/src/bindings.rs` を Windows App SDK の `.winmd` から
作り直すツール。生成物はリポジトリへコミットしてあるので、**WinUI 3 の型を
足したくなったときだけ**動かす。

## 使いかた

1. `filter.txt` へ、投影したい型を 1 行 1 つ書く (名前空間ではなく型名で
   書く。依存する型は `windows-bindgen` がたどって足す)。
2. `.winmd` を集める (下記)。
3. 生成する。

```sh
cargo run --manifest-path tools/winui3-bindgen/Cargo.toml -- <winmd を置いた場所>
```

4. `cargo check -p naui-windows` を通す。`no method named ...` が出たら、
   その戻り値の型が `filter.txt` に足りていない。足して 3 に戻る。

## `.winmd` の集めかた

Windows App SDK の NuGet パッケージに入っている。実行時のランタイムには
入っていないので、パッケージを取ってきて展開する。

```sh
for pkg in \
  "microsoft.windowsappsdk.winui/2.3.0" \
  "microsoft.windowsappsdk.interactiveexperiences/2.1.3" \
  "microsoft.windowsappsdk.foundation/2.3.5"
do
  name="${pkg%/*}"; ver="${pkg#*/}"
  curl -sSO "https://api.nuget.org/v3-flatcontainer/$name/$ver/$name.$ver.nupkg"
  unzip -o -j "$name.$ver.nupkg" '*.winmd' -d winmd
done
```

`Microsoft.UI.Xaml.winmd` は `Microsoft.Web.WebView2.Core` を参照している。
naui は WebView2 を使わないが、参照が解決できないと生成が止まるので
`Microsoft.Web.WebView2` パッケージの `.winmd` も同じ場所へ置く。

```sh
curl -sSO "https://api.nuget.org/v3-flatcontainer/microsoft.web.webview2/1.0.2151.40/microsoft.web.webview2.1.0.2151.40.nupkg"
unzip -o -j microsoft.web.webview2.1.0.2151.40.nupkg 'lib/*.winmd' -d winmd
```

`Microsoft.UI.winmd` は Windows のバージョンごとに複数入っている。新しい
ほう (`10.0.18362.0`) を使う。

## どの版から作るか

インターフェースの IID と vtable の並びは版をまたいで変わらないので、
新しい `.winmd` から作った投影は古いランタイムでもそのまま動く。ただし
**その版で初めて入ったメンバーは古いランタイムに無い**ので、呼ぶと
失敗する。naui が対応するランタイムの下限は
`crates/naui-windows/src/sdk.rs` に書いてある。
