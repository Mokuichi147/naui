# naui-gtk

[naui](https://crates.io/crates/naui) の Linux バックエンドです。

GTK4 / libadwaita の実コントロール (`AdwApplicationWindow` / `GtkButton` /
`GtkEntry` / `GtkScale` / `GtkListBox` …) を生成し、GTK4 のシグナルを Rust の
クロージャへ中継します。描画・レイアウト・IME・アクセシビリティ・テーマ追従は
すべて GTK4 が行います。

アプリから直接依存する必要はありません。Linux 向けにビルドすると `naui` が
このクレートを引き込みます。共通 API で足りないときのために、各ウィジェットは
`native_*` で GTK4 / libadwaita のオブジェクトを返します。

`naui` と GTK4 の対応表は [API ドキュメント](https://docs.rs/naui-gtk) に
あります。

## ビルド要件

GTK4 と libadwaita の開発用ライブラリが必要です。Ubuntu 24.04 では次の通りです。

```sh
sudo apt install libgtk-4-dev libadwaita-1-dev build-essential pkg-config
```

対象外の OS では中身が空のクレートになり、これらの依存も引き込みません
(macOS から `cargo check --workspace` を通すため)。

## テスト

GTK4 はメインスレッドを要求するので、統合テストは標準ハーネスを使いません
(`harness = false`)。実行にはディスプレイが必要です。

```sh
cargo test -p naui-gtk
```

## ライセンス

MIT OR Apache-2.0
