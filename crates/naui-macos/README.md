# naui-macos

[naui](https://crates.io/crates/naui) の macOS バックエンドです。

AppKit の実コントロール (`NSWindow` / `NSButton` / `NSTextField` / `NSSlider` /
`NSStackView` …) を生成し、target/action とデリゲートを Rust のクロージャへ
中継します。描画・レイアウト・IME・アクセシビリティ・テーマ追従はすべて
AppKit が行います。

アプリから直接依存する必要はありません。macOS 向けにビルドすると `naui` が
このクレートを引き込みます。共通 API で足りないときのために、各ウィジェットは
`native_*` で AppKit のオブジェクトを返します。

Objective-C を呼ぶため `unsafe` を使います。

## テスト

AppKit はメインスレッドを要求するので、統合テストは標準ハーネスを使いません
(`harness = false`)。実行には macOS の GUI セッションが必要です。

```sh
cargo test -p naui-macos
```

## ライセンス

MIT OR Apache-2.0
