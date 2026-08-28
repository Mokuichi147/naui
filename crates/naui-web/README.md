# naui-web

[naui](https://crates.io/crates/naui) の Web (wasm) バックエンドです。

DOM の標準コントロール (`<button>` / `<input>` / `<progress>` …) をそのまま
生成します。ブラウザにおける「ネイティブ UI」はフォームコントロールそのものな
ので、見た目を作り込むことはせず、ブラウザ既定のスタイルに任せています。
レイアウトだけは Flexbox を使います。

アプリから直接依存する必要はありません。`wasm32` 向けにビルドすると `naui` が
このクレートを引き込みます。それ以外のターゲットでは中身が空になり、
wasm-bindgen などの依存も引き込みません。共通 API で足りないときのために、
各ウィジェットは `native_*` で `web_sys` の要素を返します。

## ビルド

wasm ターゲットと、`Cargo.lock` に記録されたものと同じバージョンの
`wasm-bindgen-cli` が必要です。

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.127
```

入口は `naui::entry!` に任せると、`#[wasm_bindgen(start)]` をマクロが作るので
アプリ側に wasm-bindgen への依存が要りません。

## 制限

wasm にはスレッドがないため、`std::thread::spawn` は使えません。別スレッドから
値を送る代わりに `Tasks::spawn` の future の中で `Sender::send` を呼びます。
その他の制限は
[リポジトリの README](https://github.com/mokuichi147/naui#既知の制限)
にまとめています。

## ライセンス

MIT OR Apache-2.0
