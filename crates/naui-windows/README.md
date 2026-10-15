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
Windows App SDK 1.3 以降のフレームワークランタイムが必要です。起動時に
フレームワークパッケージの依存関係を追加しますが、版は決め打ちにせず 2.x から
1.3 まで新しい順に試し、最初に取り付けられたものを使います。OS 同梱の系統
(`Microsoft.WindowsAppRuntime.CBS*`) も候補に入れます。動作確認は 2.x で
行っています。

`self-contained` フィーチャーを有効にすると、ランタイムを exe と一緒に配れます。
ビルド時に部品を nuget.org から取ってきて exe の出力先へ並べ、実行時はそれを
使います。詳しくは
[リポジトリの README](https://github.com/mokuichi147/naui#windows-app-sdk-ランタイムを同梱する)
を参照してください。

また、コントロールは `Application::Start` の後でしか生成できません。naui の
公開 API が「コールバックの中で UI を組み立てる」形なのはこの制約のためです。

## ライセンス

MIT OR Apache-2.0
