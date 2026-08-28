# naui-core

[naui](https://crates.io/crates/naui) の共通値型を置くクレートです。

バックエンド (AppKit / WinUI 3 / GTK4 / DOM) に依存せず、描画もレイアウト計算も
持ちません。ウィジェットそのものは各バックエンドが OS のネイティブコントロール
として実装します。

含まれるもの:

- `Error` / `Result`、`Settings`、`Theme`、`Orientation`、`Align`、`Padding`
- レイアウトの指定 (`Sizing`、`Length`、`Track`、`GridCell`、`ScrollPolicy`)
- ウィジェットへ渡す値 (`Color`、`DateTime`、`Time`、`ListItem`、`TreeItem`、
  `TableColumn`、`TableRow`、`FileFilter`、`ToolbarItem`、`ToastSpec` など)
- バックエンドに依らない受け渡し (`Sender`、`Tasks`、`Task`、`MainThread`)

`#![forbid(unsafe_code)]` です。

アプリから直接依存する必要はありません。これらの型は `naui` が再エクスポート
しています。

## ライセンス

MIT OR Apache-2.0
