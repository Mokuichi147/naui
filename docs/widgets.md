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

## 環境ごとの組み立て

上の表で 🟡 / 🔴 のものや、標準コントロールに手を加えているものについて、
どう組み立てているかと、その理由です。使う側から見た違いは
[既知の制限](known-limitations.md)にまとめています。

### Windows の組み立て

- **`MenuBar`**: WinUI 3 の `MenuBar` ではなく、地色を消した `Button` に
  `MenuFlyout` を預けたものを横に並べています。`MenuBar` / `MenuBarItem` /
  `KeyboardAccelerator` は `naui-winui3` の投影に入れていないためです (投影は
  コミットしてある生成物で、型を増やすと全体を作り直すことになります)。
  そのぶんショートカットは、ウィンドウの根まで上ってきた `KeyDown` を naui が
  見て呼び出しています。
- **`Toolbar`**: `CommandBar` で、項目は `AppBarButton`、区切りは
  `AppBarSeparator` です。**タイトルの右**に置きます (macOS の `NSToolbar`、
  Linux の `AdwHeaderBar` と同じ位置)。`SetTitleBar` へ要素を渡す形だと、その
  中の操作できるコントロールの上でもウィンドウのドラッグが始まってしまうため、
  ドラッグ領域は `AppWindowTitleBar::SetDragRectangles` で**タイトル文字の側と
  ツールバーより右の空きの 2 つ**に分けています。ボタンの上ではドラッグが
  始まらず、それ以外のタイトルバーはどこでもつかめます。矩形は物理ピクセル
  なので、ウィンドウの幅・項目数・表示倍率が変わるたびに計算し直します。
  アイコンは Segoe Fluent Icons を `FontIcon` で出し、印 1 つの幅は 40 です
  (`AppBarButton` の既定 68 は、印の下へラベルを出す配置のための幅です)。
  ラベルは `DefaultLabelPosition` を `Collapsed` にして隠し、ほかの 3 環境と
  同じく印だけを並べます。隠したラベルは `AutomationProperties.Name` と
  `ToolTipService.ToolTip` へ回すので、読み上げとツールチップには出ます。
  幅が足りなくなると `CommandBar` が自分で項目をオーバーフローメニューへ
  送るため、切り詰められて押せなくなることはありません。
- **`SplitView`**: 動かせる仕切りを持つコントロールが Windows App SDK に無いため
  (`Microsoft.UI.Xaml.Controls.SplitView` は開閉するナビゲーションのペインで、
  `GridSplitter` は Community Toolkit の側)、3 つの列 (行) を持つ `Grid` の
  真ん中に仕切りを置いています。仕切りの地色は `ControlStrokeColorDefaultBrush`
  から引くのでテーマに追従します。塗るのは境目の 1 px だけ (`BorderThickness`) で、
  残りは `Background="Transparent"` のつかみ代です (`Transparent` は `null` と
  違って当たり判定が残ります)。仕切りに合わせたカーソル (⇔) を出せないのは、
  カーソルの形を変える `UIElement.ProtectedCursor` が派生クラスからしか
  触れないためです。区画の大きさが変わったことは `LayoutUpdated` で拾っています。
- **`Label`**: `TextBlock` は `TextTrimming="CharacterEllipsis"` を持つ XAML から
  生成しています (折り返しの切り替えは `TextWrapping` で行います)。
- **`Toggle`**: ラベルは `OnContent` と `OffContent` の両方へ同じ文字を入れる
  ので、入り切りで読みは変わりません (WinUI の既定は「オン」「オフ」と
  切り替わる文字です)。
- **`NumberInput`**: `NumberBox` です。増減ボタンは既定では出ないので
  `SpinButtonPlacementMode` を `Inline` にして欄の右へ並べ、範囲・刻み・小数桁は
  `Minimum` / `Maximum` / `SmallChange` / `NumberFormatter` にも書いて、
  上下キーやホイールの動きと表示をそろえています。端に来ると増減ボタンが
  自分で無効になり、PageUp / PageDown は刻みの 10 倍動きます。
  - 1 文字ごとの通知は、テンプレートにある入力欄 (`InputBox`) の `TextChanged`
    から拾っています。`NumberBox` が値を決めるのは確定したとき (Enter・欄を
    離れたとき・増減ボタン・上下キー・ホイール) だけだからです。
  - そのため**打っている間の値を持っているのは naui だけ**で、`NumberBox` の値は
    確定するまで古いままです。読めない文字列を戻す先も増減の基準も `NumberBox` が
    持っている値なので、**表示が読めなくなった時点で受け取り済みの値を渡して**
    います。これをしないと、`12` まで打ってから `12x` にしたときに、巻き戻しも
    増減も古い値から始まってしまいます (`12x` で上矢印を押すと 13 ではなく 2 に
    なる)。値を渡すと `NumberBox` が表示を作り直してしまうので、打っている途中の
    表示 (`-` だけ、`12x`) と選択位置は書き戻しています。読める表示なら
    `NumberBox` が確定と増減のどちらでも先に表示を読むので、渡す必要はありません。
  - 打っている途中の表示は `NumberBox` が確定に使うのと同じ `NumberFormatter`
    (`INumberParser` でもある) で読みます。小数点や桁区切りは地域設定で変わるので、
    打鍵中と確定とで読み手が違うと通知の出かたがずれるためです。
  - 有効数字が 10 桁までなのは、`NumberBox` が内部に `SignificantDigits(10)` の
    丸め器を持っていて、`NumberFormatter` を差し替えても外せないためです。
    確定すると `NumberBox` はその表示を読み直すので値も 10 桁へそろい、値と表示が
    ずれたままにはなりません。
  - 数字を右へそろえるのは、macOS の `NSTextField` と 0.3.0 までの Windows が
    右寄せだったためです (`NumberBox` の既定は左寄せです)。`NumberBox` 自身の
    `TextAlignment` は投影元の Windows App SDK にまだ無いため、テンプレートの
    入力欄へ直に書いています。
- **`SearchInput`**: `AutoSuggestBox` です。虫めがねは `QueryIcon="Find"` を
  XAML で渡して作らせ (`SymbolIcon` は投影に入れていないため)、確定
  (`on_search`) は `QuerySubmitted` です。候補の一覧 (`ItemsSource`) は渡しません。
  `TextChanged` は `Text` を書き換えたときにも飛ぶため、`Reason` がプログラムからの
  変更なら黙ります (`set_text` は通知しません)。
- **`EditableComboBox`**: `ComboBox` (`IsEditable`) です。1 文字ごとの通知は、
  テンプレートにある入力欄 (`EditableText`) の `TextChanged` から拾っています。
  `ComboBox` 自身は文字の変化を表に出さないためです。`IsTextSearchEnabled` は
  一致する候補へ選択を移すだけで、一覧は絞り込みません。打った文字で候補を出す
  `AutoSuggestBox` は `SearchInput` が使っていますが、あちらは候補を「開いて
  全部見る」ことができません。`EditableComboBox` は一覧を開いて選べることを
  軸にしているので、Fluent の作法どおり `ComboBox` のままにしています。
- **`DatePicker`**: `DatePickerMode::DateTime` では `DatePicker` と `TimePicker` を
  `StackPanel` で横に並べます。`set_range` は WinUI 側へは年の範囲
  (`MinYear` / `MaxYear`) として渡し、月日と時刻の境界は naui 側で端へ寄せます。
  標準テンプレートは英語の長い月名に合わせて月の列だけを広く左寄せにするため、
  3 列を等幅・中央揃えへ直し、最小幅も詰めています。
- **`TimePicker`**: `DatePicker` と同じ WinUI の `TimePicker` です。下限・上限が
  無いので、`set_range` の範囲は naui 側で端へ寄せます。
- **`ColorPicker`**: WinUI 3 の `ColorPicker` はスペクトラムとスライダーを縦に
  並べた大きな面なので、`Button` の `Flyout` へ入れ、ボタンには選んだ色の見本
  (`Border` + `SolidColorBrush`) を出します。組み立てだけは XAML に書いて
  `XamlReader` へ渡します (`Flyout` は投影に入れていないため)。
- **`List`**: `ListView` です。行の見た目 (角丸・淡い塗り・左端のアクセント色の
  インジケーター) は標準テンプレートが持つので、naui は枠の `Border` だけを
  足します (`Table` と `Tree` と同じ色・角丸)。スクロールは `ListView` が自分で
  持ちます。
- **`Table`**: WinUI 3 に `DataGrid` は無い (Community Toolkit のもの) ため、
  `ListView` の行を `Grid` にして組み立て、見出しは同じ列定義を持つ別の `Grid` に
  置いて幅をそろえています。行の余白だけは `ListView` の既定ではなく見出しと
  同じ値を書いて、列の位置をそろえています。並べ替えできる見出しは、地色と枠を
  消した `Button` です (WinUI に列見出し用のコントロールが無いため)。
- **`Tree`**: `TreeView` です。項目は `TreeViewNode` へ写して木のまま渡すので、
  段付け・開閉の山形・キーボード操作・選択の見た目はすべて WinUI が持ちます。
  枠だけは `List` と同じ色と角丸を持たせた `Border` で囲みます。
  `TreeItem::enabled` に当たるものは WinUI に無い (`TreeViewItem` を無効にすると、
  枝では開閉までできなくなる) ため、文字を薄くしたうえで行の中身に押下を
  受け止める覆いをかぶせています。覆いがかかるのは開閉の山形より右だけなので、
  選べない枝も開閉はできます。スクロールは `TreeView` が自分で持ちます
  (中の一覧は与えられた高さぶんしか並べないので、`List` のように外側の
  `ScrollViewer` へ預けると伸びずに切れてしまいます)。
- **`PopupMenu`**: `MenuFlyout` です。出す位置・影・角丸・ライトディスミス・
  キーボード操作 (矢印と Esc)・画面端での回り込みはすべて WinUI が持ちます。
  右クリックは `UIElement.ContextFlyout` へ預けるので、長押しとコンテキスト
  キーでも同じメニューが出ます。
- **`Toast`**: 見た目は `InfoBar` です。ただし `InfoBar` は本来ページの中へ
  並べて使う帯なので、下端の中央へ寄せて中身の層に重ねています。操作ボタンは
  `ActionButton` に置くので、文字の右か下かの並べ方は WinUI が決めます。
  閉じるボタン (×) は出しません (`IsClosable` を切っています)。消えるのは
  時間切れ・操作ボタン・`dismiss` の 3 つで、ほかの 3 環境にも × が無いため
  です。
- **`Canvas`**: `Grid` の中に XAML の `Canvas` を置き、`on_draw` の命令を
  `Path` (パスの記法 `M` / `L` / `C` / `Z`) と `TextBlock` にして `XamlReader`
  で組み立てます。破線は XAML の決まりどおり**線の太さを 1 とする単位**へ
  直して渡しています。中央・右寄せの文字は、読み込んだ `TextBlock` を測って
  `Canvas.Left` を書き直します。
- **`Window` の位置と状態**: `position` は `AppWindow.Position` の物理ピクセルを
  このウィンドウの `RasterizationScale` で割った値です。最大化と最小化の間は
  `GetWindowPlacement` / `SetWindowPlacement` の `rcNormalPosition` (元に戻した
  ときの枠) を読み書きします。全画面は `AppWindow` のプレゼンターを
  `FullScreenPresenter` へ差し替えて作り、入る前の位置を覚えておいて返し、
  頼まれた位置は全画面を出たところで当てます。出るときは最初の
  `OverlappedPresenter` を付け直します (最小の大きさや `set_resizable` の設定を
  残すため)。

### macOS の組み立て

- **`Grid`**: `Auto` 行の高さと、`Fill` の子が受け取る取り分は naui が計算します。
  `NSGridView` は行と列の大きさを中身から決めますが、**余りをどこへ渡すかは
  決まっておらず**、hugging priority だけでは `Auto` の側へ余白が入ります。
  そこで `Auto` 行にはセル内容の `fittingSize` を行の高さとして渡し、`Fill` の子
  には「グリッドの大きさ − ほかの列が要る幅」まで伸びたいという弱い希望を張り
  ます (固定幅の列はその指定幅、それ以外はセル内容の `fittingSize` で見積もり
  ます)。**位置と大きさそのものを決めるのは Auto Layout** で、naui が frame を
  置くわけではありません。結合をまたぐ行・列を外せないのは、試すと AppKit が
  例外を投げるためです。
- **`SplitView`**: `NSSplitView` そのものです。ただし**区画の配り方だけは
  naui が置き換えています** (`splitView:resizeSubviewsWithOldSize:`)。
  `NSSplitView` の既定は大きさが変わったときに**割合**で配るため、そのままでは
  ウィンドウを広げるとサイドバーまで広がってしまうためです。区画の frame を
  naui が直接置く都合上、区画のビューだけは
  `translatesAutoresizingMaskIntoConstraints` を切っていません (中身の配置は
  その frame の中で Auto Layout が行います)。
- **`Expander`**: 折りたたみにあたるコントロールが AppKit に無いため、入り切りの
  `NSButton` (山形は SF Symbols) と `NSStackView` で組み立てています。
- **`Toolbar`**: 項目をインデックスで識別する都合上、「ツールバーをカスタマイズ」
  (利用者による並べ替え) は切ってあります。区切りは macOS の作法にならって
  `NSToolbarSpaceItem` (一定幅の空き) です。タイトル文字を隠すのは、出したままだと
  タイトルが先頭を占め、項目が右端へ寄ってしまうためです。
- **`Label`**: `set_wrap(true)` にすると、naui は `NSViewFrameDidChangeNotification`
  を見て `preferredMaxLayoutWidth` を frame の幅へ追従させます。`NSTextField` は
  折り返す幅が決まって初めて高さを返せるためです。
- **`Toggle`**: `NSSwitch` は文字を持たないため、ラベルは横へ並べた
  `NSTextField` です。
- **`NumberInput`**: 数値専用のコントロールが AppKit に無いため、`NSTextField` と
  `NSStepper` を横に並べて組み立てています (システム設定の数値欄と同じ形)。
- **`EditableComboBox`**: `NSComboBox` です。打ちかけの文字を候補で補う
  `completes` を入れてあります (そのまま打ち切れば候補にない文字列も残せます)。
  一覧を絞り込まないのは、AppKit に一覧を絞る仕組みが無いためです。
- **`DatePicker`**: `NSDatePicker` そのものです。欄をクリックするとカレンダーが
  重なって開き (`presentsCalendarOverlay`)、キーボードとステッパーでも編集
  できます。見た目・操作とも AppKit の既定 (SwiftUI の `DatePicker` と同じ
  `textFieldAndStepper`) と変わりません。
- **`TimePicker`**: 時刻専用のコントロールが AppKit に無いため、`NSDatePicker` の
  表示項目を時分だけにしたものです (AppKit で時刻を入力させるときの作法)。
  日付の部分は画面に出ないので 1970-01-01 へ固定してあり、`set_range` の
  下限・上限は `NSDatePicker` 自身にも渡ります。
- **`ColorPicker`**: `NSColorWell` そのものです。押すとシステムのカラーパネルが
  開きます。パネルはカタログ色 (`systemBlue` など) も返すので、成分を読む前に
  sRGB へ変換しています。`set_enabled(false)` ではパネルとのつながりも切ります。
- **`Table`**: 行の高さを一定にしているのは、AppKit に求めさせる指定
  (`usesAutomaticRowHeights`) を使うと、列を足し引きしたときに AppKit が行へ張る
  制約が、前の列で外れたセルを指したまま有効になり、表が壊れるためです。
  幅を指定しなかった列への割り当てを naui が計算するのは、AppKit の列の
  自動調整が、幅を固定した列があると余りを配りきれず、表の右側が空いたままに
  なるためです。
- **`Toast`**: トーストにあたるコントロールが AppKit に無いため、
  `NSVisualEffectView` (`NSPopover` と同じ材質) をウィンドウの中身へ重ねて
  組み立てています。
- **`Window` の位置と状態**: 最小化の間は `frame` が元の枠のままなので、
  そのまま読み書きします。最大化は `windowShouldZoom:toFrame:` で zoom する前の
  枠を覚えておき、AppKit が元の枠へ戻し終えてから頼まれた位置へ動かします。
  全画面は入る前の位置を覚えておいて返し、出終わった後に、頼まれた位置か
  入る前の位置へ動かします (macOS 15 の AppKit は `set_position` で動かした分を
  忘れて、表示したときの枠へ戻すことがあるため)。

### Linux の組み立て

- **`SplitView`**: `GtkPaned` です。区画の最小の大きさは、アプリが `Sizing` で
  指定する `size_request` とぶつからないよう、区画ごとにかぶせた入れ物のほうに
  持たせています。
- **折り返す中身**: 「幅が決まってから高さが決まる」ので、naui が中身を包む
  入れ物は `GtkSizeRequestMode` を中身から引き継ぎます。`GtkWidget` の既定は
  `GTK_SIZE_REQUEST_CONSTANT_SIZE` で、そのままだと親が幅を渡さずに高さを
  尋ね、折り返す前の 1 行ぶんしか場所を配りません。
- **`Toolbar`**: 項目は、GNOME の作法にならってヘッダーバーの左側へ並びます。
- **`MenuBar`**: `GtkPopoverMenuBar` で、ヘッダーバーの下に入ります。GNOME は
  メニューバーよりハンバーガーメニューを勧めますが、naui は 4 環境で同じ API を
  出すため、GTK4 が持つメニューバーをそのまま使います。項目は `GMenu` のモデルと
  `GSimpleAction` の組で表し、ショートカットは `GtkApplication` のアクセラレータ
  へ登録します。アクセラレータの表はアプリ全体で 1 つなので、登録するのは
  ウィンドウへ取り付けている間だけで、`clear_menu_bar` で外すと登録も外します。
  操作の名前空間はメニューバーごとに分けてあるので、複数のウィンドウで同じ
  ショートカットを使っても互いに上書きしません。
- **`Label`**: `set_style` は libadwaita のスタイルクラス (`.title-1` など) を
  当てるだけで、級数と太さを決めるのはテーマです。既定 (折り返さない) では
  `PangoEllipsizeMode::End` を入れています。**省略記号を付けると `GtkLabel` の
  最小幅も下がる**ので、狭いコンテナへ入れてもコンテナごと押し広げてしまうことが
  なくなります。
- **`Toggle`**: `GtkSwitch` は文字を持たないため、ラベルは `GtkBox` へ横に並べた
  `GtkLabel` です。
- **`EditableComboBox`**: 打ち込めるコンボボックスが GTK4 に無いため、`GtkEntry`
  と、候補を出す `GtkMenuButton` を `.linked` の `GtkBox` へ並べて組み立てて
  います。入力欄を持つ `GtkComboBoxText` と `GtkEntryCompletion` は GTK 4.10 で
  非推奨になり、置き換え先が用意されていないためです。
- **`DatePicker`**: 日付を選ぶ 1 つのコントロールが GTK4 に無いため、
  `GtkMenuButton` のポップオーバーに載せた `GtkCalendar` と、時分の
  `GtkSpinButton` で組み立てています。
- **`TimePicker`**: 時刻を選ぶ 1 つのコントロールも GTK4 に無いため、時と分の
  `GtkSpinButton` を `:` で挟んで並べています (GNOME の時計アプリと同じ形)。
  スピンボタンに時刻としての範囲は無いので、`set_range` の範囲は naui 側で
  端へ寄せ、押し戻した結果を表示にも書き戻します。
- **`ColorPicker`**: `GtkColorDialogButton` です。透明度を扱わないので、開く
  `GtkColorDialog` の `with-alpha` は切ってあります。
- **`List`**: `GtkListBox` を `GtkScrolledWindow` へ載せたものです。行のクリック
  (`ListRow::on_activate`) は `GtkListBox` の `row-activated` で受けます
  (`GtkListBoxRow` の `activate` はキーボードの Enter / Space だけの経路です)。
  `SelectionMode::Multiple` では、クリックに付いた Ctrl / Shift を GTK4 に
  読ませるため単発クリックでの確定を切ります。
- **`Table`**: `GtkColumnView` (`GtkListItemFactory` と `GListModel` を要求する)
  ではなく、`GtkListBox` の行を横並びにして組み立て、列の幅は列ごとの
  `GtkSizeGroup` でそろえています。並べ替えできる見出しは `flat` な
  `GtkButton` で、向きは見出しの文字に付く矢印で表します。
- **`Tree`**: `GtkTreeExpander` (`GtkListView` 専用) ではなく、`GtkListBox` の
  行と開閉ボタンで組み立てています。
- **`Window` の状態**: X11 では、ウィンドウマネージャが `unminimize` だけでは
  戻さないことがあるので、最小化から戻すときは `present` で前面へ出すよう
  頼んでいます。表示する前に頼んだ最小化は、コンポジタから最初の状態が
  届いてから当てます。

### Web の組み立て

- **`Label`**: **`<span>` の既定は折り返す**ため、naui が `white-space: nowrap` と
  `text-overflow: ellipsis` を入れて、ほかの 3 環境の既定 (1 行 + 省略記号) へ
  そろえています。折り返したいときは `set_wrap(true)` を使います。ブラウザに見出しの段階 (type ramp) が無いため、`set_style` の
  大きさだけは naui が CSS の相対値 (`em`) でほかの 3 環境へそろえています。
  相対値なので、ブラウザと利用者が決めた基準の文字サイズには追従します。
- **`SplitView`**: 動かせる仕切りで区画を分ける要素が HTML に無いため
  (`resize` は要素の隅につまみを出すだけです)、naui が組み立てます。
  仕切りは `<div role="separator">` で、カーソルは `col-resize` / `row-resize`
  に任せています (つまみの絵を naui の CSS で描くことはしません)。見えるのは
  境目の 1 px の線だけで、残りは透明なつかみ代です。線の色はシステムカラーの
  `GrayText` を使います。`ButtonBorder` と `CanvasText` はブラウザによって
  **地色の正反対** (ダークなら白、ライトなら黒) になり、区切り線としては
  強すぎるのに対し、`GrayText` はどちらの配色でも地色と文字色の中間に来る
  ためです。`tabindex="0"` を付けてあるので、キーボードでも動かせます
  (矢印キーで 10 px ずつ)。
- **`MenuBar`**: ブラウザには OS のアプリケーションメニューが無いため、
  WAI-ARIA の役割 (`role="menubar"` / `"menu"` / `"menuitem"`) を付けた要素で
  組み立てています。ショートカットは `document` の `keydown` を naui が見張って
  突き合わせ、項目の右端の `Ctrl+S` という表示も naui が添えています。
  `document` への購読はウィンドウへ取り付けている間だけ張ります。
- **`EditableComboBox`**: `<input list>` と `<datalist>` です。naui は見た目も
  絞り込みも作りません。
- **`DatePicker`**: 時間帯を持ち込まないよう `datetime-local` を使います。
- **`List`**: `ListItem::detail` か `ListRow` (任意内容の行) を使うと、
  `<select>` から `<ul role="listbox">` を使った実装へ切り替わります。文字だけの
  行しか無ければ `<select size>` のままです。`<option>` の内容モデルはテキスト
  だけで、要素も改行も置けないためです。**`<select>` を離れたぶん、選択・
  フォーカス・キー操作 (矢印・Home / End・Space) は `Table` と同じく naui が
  足しています**。指している行は `aria-activedescendant` で示し、行そのものを
  押したときはリスト (`<ul tabindex="0">`) へフォーカスを移してキーボードで
  続けられるようにします。行の中に置いたボタンや入力欄はそれ自身のものなので、
  そこへ届いたクリックとキーはリスト操作にしません (チェックボックスの Space が
  行の選択に化けないようにするためです)。枠と選択の色はブラウザのシステム色
  (`Field` / `SelectedItem` / `Highlight` / `GrayText`) のままです。任意内容の
  行を出すにはこの形しかないため、行そのものの操作 (`ListRow::on_activate`) と
  行内のコントロールの両方を残し、どちらの経路でも操作できるようにしています。
- **`Table`**: `<table role="grid">` です。`<table>` に行を選ぶ仕組みは無いので、
  クリック (⌘ / Ctrl / Shift) とキー操作 (矢印・Home / End・Space) は naui が
  足しています。列の幅は `<colgroup>` の `<col>` と `table-layout: fixed` で
  決まります。並べ替えできる見出しは `<th>` の中の `<button>` で、向きは
  `aria-sort` と矢印で表します。
- **`Dialog` と `PopupMenu`**: Esc は、`<dialog>` の `cancel` イベントではなく
  **`keydown` の既定動作を止めてから naui が閉じます**。Safari はウィンドウが
  全画面のとき、ページが既定動作を止めない限り Esc をまず全画面の解除に使い、
  ダイアログを閉じるのに 2 回押すことになるためです。変換中 (`isComposing`) の
  Esc は入力を取り消すためのものなので、そのまま IME へ渡します。
- **`Toast`**: ページに一時的な通知の標準要素が無いため、`<div role="status">` を
  `position: fixed` で下端の中央へ出します (`Notification` API はページの外へ
  出るもので別物です)。

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
`downgrade()` で作ります ([使い方](../crates/naui/src/guide.md#後からウィジェットを外す)を参照)。

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
