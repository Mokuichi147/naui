# ウィジェットの実装

各ウィジェットが、それぞれの環境で何のコントロールになるかをまとめています。

## プラットフォーム別の実装

凡例:

| 記号 | 意味 |
| --- | --- |
| ✅ | プラットフォームの標準コントロールをそのまま使用 |
| 🟡 | 標準コントロールを組み合わせて実装 |
| 🔴 | 対応する概念がないため、別の要素で再現 |

### ウィンドウ・レイアウト

| naui | Windows (WinUI 3) | macOS (AppKit) | Linux (GTK4) | Web (DOM) |
| --- | --- | --- | --- | --- |
| `Window` | ✅ `Microsoft.UI.Xaml.Window` | ✅ `NSWindow` | ✅ `AdwApplicationWindow` | 🔴 `<div>` + `document.title` |
| `Stack` | ✅ `StackPanel` | ✅ `NSStackView` | ✅ `GtkBox` | 🟡 `<div>` + CSS Flexbox |
| `Grid` | ✅ `Grid` | ✅ `NSGridView` | 🟡 `GtkGrid` | 🟡 `<div>` + CSS Grid |
| `Scroll` | ✅ `ScrollViewer` | ✅ `NSScrollView` | ✅ `GtkScrolledWindow` | 🟡 `<div>` + `overflow` |
| `Spacer` | 🔴 中身のない `Grid` | 🟡 中身のない `NSView` | 🟡 中身のない `GtkBox` | 🟡 `<div>` + `flex-grow` |
| `Expander` | ✅ `Expander` | 🟡 `NSButton` (入り切り) + `NSStackView` | ✅ `GtkExpander` | ✅ `<details>` + `<summary>` |
| `SplitView` | 🔴 `Grid` + 仕切りの `Grid` | ✅ `NSSplitView` | ✅ `GtkPaned` | 🔴 `<div>` + `<div role="separator">` |
| `Toolbar` | ✅ `CommandBar` + `AppBarButton` | ✅ `NSToolbar` + `NSToolbarItem` | 🟡 `AdwHeaderBar` + `GtkButton` | 🟡 `<div role="toolbar">` + `<button>` |
| `MenuBar` | 🟡 `Button` + `MenuFlyout` の横並び | ✅ `NSMenu` (`NSApplication.mainMenu`) | ✅ `GtkPopoverMenuBar` + `GMenu` | 🟡 `<div role="menubar">` + `<div role="menu">` |
| `Sidebar` | 🟡 `NavigationView` (Left) + 仕切りの `Grid` | ✅ `NSSplitViewController` (サイドバー項目) + `NSTableView` (ソースリスト) | 🟡 `GtkPaned` + `GtkListBox` (`.navigation-sidebar`) | 🔴 `<aside>` + `<nav>` + `<div role="separator">` |

### 基本・入力

| naui | Windows (WinUI 3) | macOS (AppKit) | Linux (GTK4) | Web (DOM) |
| --- | --- | --- | --- | --- |
| `Label` | ✅ `TextBlock` | ✅ `NSTextField` | ✅ `GtkLabel` | ✅ `<span>` |
| `Button` | ✅ `Button` | ✅ `NSButton` | ✅ `GtkButton` | ✅ `<button>` |
| `Checkbox` | ✅ `CheckBox` | ✅ `NSButton` | ✅ `GtkCheckButton` | 🟡 `<input type="checkbox">` + `<label>` |
| `Toggle` | ✅ `ToggleSwitch` | 🟡 `NSSwitch` + `NSTextField` | 🟡 `GtkSwitch` + `GtkLabel` | 🟡 `<input type="checkbox" switch>` + `<label>` |
| `TextInput` | ✅ `TextBox` | ✅ `NSTextField` | ✅ `GtkEntry` | ✅ `<input type="text">` |
| `TextArea` | ✅ `TextBox` | 🟡 `NSTextView` + `NSScrollView` | 🟡 `GtkTextView` + `GtkScrolledWindow` | ✅ `<textarea>` |
| `PasswordInput` | ✅ `PasswordBox` | ✅ `NSSecureTextField` | ✅ `GtkPasswordEntry` | ✅ `<input type="password">` |
| `SearchInput` | ✅ `AutoSuggestBox` | ✅ `NSSearchField` | ✅ `GtkSearchEntry` | ✅ `<input type="search">` |
| `NumberInput` | ✅ `NumberBox` | 🟡 `NSTextField` + `NSStepper` | ✅ `GtkSpinButton` | ✅ `<input type="number">` |
| `Slider` | ✅ `Slider` | ✅ `NSSlider` | ✅ `GtkScale` | ✅ `<input type="range">` |
| `ProgressBar` | 🟡 `Grid` + `Border` | ✅ `NSProgressIndicator` | ✅ `GtkProgressBar` | ✅ `<progress>` |

### データ選択

| naui | Windows (WinUI 3) | macOS (AppKit) | Linux (GTK4) | Web (DOM) |
| --- | --- | --- | --- | --- |
| `ComboBox` | ✅ `ComboBox` | ✅ `NSPopUpButton` | ✅ `GtkDropDown` | ✅ `<select>` |
| `EditableComboBox` | ✅ `ComboBox` (`IsEditable`) | ✅ `NSComboBox` | 🟡 `GtkEntry` + `GtkMenuButton` + `GtkListBox` | ✅ `<input list>` + `<datalist>` |
| `RadioGroup` | 🟡 `StackPanel` + `RadioButton` | 🟡 `NSStackView` + `NSButton` (ラジオ型) | 🟡 `GtkBox` + 組にした `GtkCheckButton` | 🟡 `<div role="radiogroup">` + `<input type="radio">` |
| `DatePicker` | ✅ `DatePicker` / `TimePicker` | ✅ `NSDatePicker` | 🟡 `GtkMenuButton` + `GtkCalendar` + `GtkSpinButton` | ✅ `<input type="date">` / `"time"` / `"datetime-local"` |
| `TimePicker` | ✅ `TimePicker` | ✅ `NSDatePicker` (時分だけ) | 🟡 時と分の `GtkSpinButton` | ✅ `<input type="time">` |
| `ColorPicker` | 🟡 `Button` + `Flyout` + `ColorPicker` | ✅ `NSColorWell` | ✅ `GtkColorDialogButton` | ✅ `<input type="color">` |
| `List` | ✅ `ListView` | ✅ `NSTableView` + `NSScrollView` | 🟡 `GtkListBox` + `GtkScrolledWindow` | ✅ `<select size>` / 🟡 `<ul role="listbox">` |
| `Table` | 🟡 `Grid` + `ListView` (行は `Grid`) | ✅ `NSTableView` + `NSTableHeaderView` | 🟡 `GtkListBox` + `GtkSizeGroup` | 🟡 `<table role="grid">` |
| `Tree` | ✅ `TreeView` | ✅ `NSOutlineView` + `NSScrollView` | 🟡 `GtkListBox` + 開閉ボタン | 🟡 `<ul role="tree">` |

### ファイル・メディア

| naui | Windows (WinUI 3) | macOS (AppKit) | Linux (GTK4) | Web (DOM) |
| --- | --- | --- | --- | --- |
| `FilePicker` | 🟡 `Button` + `IFileOpenDialog` | 🟡 `NSButton` + `NSOpenPanel` | 🟡 `GtkButton` + `GtkFileDialog` | 🟡 `<button>` + `<input type="file">` |
| `FileSaver` | 🟡 `Button` + `IFileSaveDialog` | 🟡 `NSButton` + `NSSavePanel` | 🟡 `GtkButton` + `GtkFileDialog` (save) | 🔴 `<button>` + `showSaveFilePicker` / `<a download>` |
| `Image` | ✅ `Image` | ✅ `NSImageView` | ✅ `GtkPicture` | ✅ `<img>` |
| `Video` | ✅ `MediaPlayerElement` | ✅ `AVPlayerView` | 🟡 `GtkPicture` + `GtkMediaControls` | ✅ `<video>` |
| `Audio` | ✅ `MediaPlayerElement` | 🟡 `AVPlayerView` | 🟡 `GtkMediaControls` + `GtkMediaFile` | ✅ `<audio>` |

### オーバーレイ

| naui | Windows (WinUI 3) | macOS (AppKit) | Linux (GTK4) | Web (DOM) |
| --- | --- | --- | --- | --- |
| `PopupMenu` | ✅ `MenuFlyout` | ✅ `NSMenu` | ✅ `GtkPopoverMenu` + `GMenu` | 🟡 `<div role="menu">` |
| `Dialog` | ✅ `ContentDialog` | 🟡 `NSAlert` + `accessoryView` | ✅ `AdwAlertDialog` | 🟡 `<dialog>` + `showModal()` |
| `Toast` | 🟡 `InfoBar` を重ねたもの | 🔴 `NSVisualEffectView` を重ねたもの | ✅ `AdwToast` + `AdwToastOverlay` | 🔴 `<div role="status">` |

### 描画

| naui | Windows (WinUI 3) | macOS (AppKit) | Linux (GTK4) | Web (DOM) |
| --- | --- | --- | --- | --- |
| `Canvas` | 🔴 `Grid` + `Canvas` + `Path` / `TextBlock` | ✅ `NSView` + `drawRect:` | ✅ `GtkDrawingArea` | ✅ `<canvas>` |

### ナビゲーション

| naui | Windows (WinUI 3) | macOS (AppKit) | Linux (GTK4) | Web (DOM) |
| --- | --- | --- | --- | --- |
| `Tabs` | 🟡 `Grid` + `ToggleButton` | ✅ `NSTabView` | ✅ `GtkNotebook` | 🟡 `role="tablist"` + `<button>` |
| `Navbar` | 🟡 `TextBlock` + `ToggleButton` | 🟡 `NSTextField` + `NSSegmentedControl` | 🟡 `GtkLabel` + `GtkToggleButton` | 🟡 `<nav>` + `<strong>` + `<button>` |
| `Dock` | 🟡 `ToggleButton` の横並び | ✅ `NSSegmentedControl` | 🟡 `GtkToggleButton` の横並び | 🟡 `<nav>` + `<button>` |
| `Menu` | 🟡 `NavigationViewItem` の縦並び | 🟡 `NSButton` の縦並び | 🟡 `GtkToggleButton` の縦並び | 🟡 `<nav><ul><li><button>` |
| `Breadcrumbs` | 🟡 `HyperlinkButton` + 区切り | ✅ `NSPathControl` | 🟡 `GtkLabel` のリンク + 区切り | 🟡 `<nav><ol><li><a>` |
| `Pagination` | 🟡 `Button` + `ToggleButton` | 🟡 `NSButton` + `NSSegmentedControl` | 🟡 `GtkButton` + `GtkToggleButton` | 🟡 `<nav>` + `<button>` |
| `Link` | ✅ `HyperlinkButton` | 🟡 `NSButton` + `NSWorkspace` | ✅ `GtkLinkButton` | ✅ `<a>` |

## 主なデータ型

- `WindowState` / `CloseResponse`: ウィンドウの状態 (通常・最大化・最小化・全画面) と、閉じる操作への応答
- `Sizing` / `Length` / `Track` / `GridCell`: 配置とサイズ
- `Key` / `KeyEvent` / `Modifiers` / `EventResponse`: キー入力の通知と、そのキーを先へ渡すかどうか
- `NavItem`: ナビゲーション項目
- `ListItem` / `ListRow` / `SelectionMode`: 単純なリスト項目、任意内容の行、単一・複数選択
- `TableColumn` / `TableRow` / `SortOrder`: 表の列・行と並べ替えの向き
- `TreeItem`: ツリー項目 (入れ子・開閉・選べるかどうか)
- `DateTime` / `DatePickerMode`: 年月日と時分の値、日付選択で何を選ばせるか
- `Time`: 時分だけの値 (時刻の選択でやり取りする値)
- `NumberSpec`: 数値入力の下限・上限・刻み・小数桁
- `Color`: sRGB の 8 bit で表す色 (色の選択でやり取りする値)
- `FileFilter` / `FilePickerMode` / `FileEntry`: ファイルの選択と保存
- `Fit` / `PlaybackState`: メディア表示と再生状態
- `Painter` / `Path` / `Point` / `Rect` / `PointerEvent` / `PointerButton`: 描画面に描く命令と、その座標・ポインターとホイールの通知
- `PopupItem`: ポップアップメニュー項目
- `ToolbarItem` / `ToolbarIcon`: ツールバー項目とアイコン
- `MenuSpec` / `MenuItem` / `MenuShortcut`: メニューバーの見出し・項目・ショートカット
- `SidebarSection` / `SidebarItem`: サイドバーのまとまり (見出し) と項目 (文字とアイコン)
- `DialogButtons` / `DialogResponse`: ダイアログのボタンと応答
- `ToastSpec`: トーストの文字・操作ボタン・消えるまでの時間

通知の中でウィジェット自身を触るときに使う弱参照 `WeakWidget` は、各ウィジェットの
`downgrade()` で作ります ([使い方](guide.md#後からウィジェットを外す)を参照)。

API の詳しい説明は、リポジトリ内で次のコマンドを実行して確認できます。

```sh
cargo doc --open -p naui
```

## ネイティブオブジェクトへのアクセス

共通 API で足りない場合は、バックエンド固有のオブジェクトを取得できます。

```rust,ignore
// macOS
let view = button.native_view();

// Web
let element = button.native_element();
```

このコードは対象プラットフォームに依存するため、必要に応じて `cfg` で分けてください。
