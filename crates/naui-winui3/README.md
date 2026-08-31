# naui-winui3

naui が使う分だけの **WinUI 3 (Windows App SDK) の WinRT 投影**。

WinUI 3 のコントロールは Windows SDK ではなく Windows App SDK に入っていて、
`windows` クレートには投影がありません。このクレートは Microsoft が配る
`.winmd` から [`windows-bindgen`] で作った投影 (`src/bindings.rs`) と、
生成物だけでは足りない次の 3 つを持ちます。

- `bootstrap`: 未パッケージのアプリが Windows App SDK ランタイムを自分の
  プロセスへ取り付ける仕組み (動的依存)
- `compose`: `Application` のような composable なクラスを Rust 側で継承する
  ための土台
- `native`: XAML の `Window` から HWND を取り出す `IWindowNative`

`src/bindings.rs` を作り直す手順は `tools/winui3-bindgen/README.md` にあります。

**naui のための下ごしらえなので、単体で使うことは考えていません。** WinUI 3 を
Rust から一般的に使いたい場合は、必要な型を自分で選んで
`windows-bindgen` にかけるほうが小さく済みます。

[`windows-bindgen`]: https://crates.io/crates/windows-bindgen
