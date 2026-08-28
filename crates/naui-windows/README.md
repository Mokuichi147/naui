# naui-windows

[naui](https://crates.io/crates/naui) の Windows バックエンドです。

WinUI 3 (Fluent 2) の実コントロール (`Microsoft.UI.Xaml.Controls.Button` など) を
生成します。描画・レイアウト・IME・アクセシビリティ・テーマ追従はすべて
WinUI 3 が行います。

アプリから直接依存する必要はありません。Windows 向けにビルドすると `naui` が
このクレートを引き込みます。Windows 以外では中身が空になるので、依存も
引き込みません。共通 API で足りないときのために、各ウィジェットは `native_*` で
WinUI 3 のオブジェクトを返します。

## 実行要件

WinUI 3 は Windows SDK ではなく **Windows App SDK** に含まれるため、実行環境に
Windows App SDK 2.x のフレームワークランタイムが必要です (2.3.1 で動作確認)。
起動時に `V2` のフレームワークパッケージ依存関係を追加し、インストール済みの
最新 2.x ランタイムを使います。

また、コントロールは `Application::Start` の後でしか生成できません。naui の
公開 API が「コールバックの中で UI を組み立てる」形なのはこの制約のためです。

## ライセンス

MIT OR Apache-2.0
