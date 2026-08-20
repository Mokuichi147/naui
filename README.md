# naui

各 OS のネイティブ UI を、1 つの Rust API から扱う軽量 GUI ツールキットです。

naui はウィジェットを自前で描画しません。ボタン、入力欄、レイアウト、IME、
アクセシビリティ、テーマへの追従は、それぞれのプラットフォームが提供する
ツールキットへ任せます。

| 対象 | バックエンド | ボタンの実体 |
| --- | --- | --- |
| Windows | WinUI 3 (Windows App SDK) | `Microsoft.UI.Xaml.Controls.Button` |
| macOS | AppKit | `NSButton` |
| Linux | GTK4 / libadwaita | `GtkButton` |
| Web (wasm) | DOM | `<button>` |

## 特長

- 4 つのバックエンドを同じ API で利用可能
- OS 標準の描画、入力、アクセシビリティ、テーマを活用
- Rust 側でレイアウト計算やメディアのデコードを行わない軽量な設計
- 共通 API で足りない場合はネイティブオブジェクトへアクセス可能
- 最小サンプルと、種別ごとに全ウィジェットの特徴を確認できる Gallery を同梱

## 対応状況

| 環境 | 状態 | 確認内容 |
| --- | --- | --- |
| macOS | ✅ 動作確認済み | AppKit の実コントロールを使った統合テストと Gallery の実行 |
| Linux | ✅ 動作確認済み | Ubuntu 24.04、GTK 4.14、libadwaita 1.5、Wayland で Gallery と統合テストを実行 |
| Web | ✅ 動作確認済み | ブラウザ上で DOM の描画、入力、ナビゲーション、ファイル選択、メディア、ダイアログを操作 |
| Windows | ✅ 動作確認済み | Windows App SDK 2.3.1 の x64 実機で全ウィジェットとナビゲーションを操作 |

未確認の範囲もあります。

- Web: `PopupMenu` のブラウザ実行と、埋め込みブラウザで配送されなかった
  `Dialog` の Esc 操作

これらは実装済みで、Web ターゲット向けの `cargo check` は通ります。

## クイックスタート

必要な Rust の最小バージョンは 1.82 です。

```sh
git clone https://github.com/mokuichi147/naui.git
cd naui
cargo run -p counter
```

全ウィジェットは、基本、入力、一覧、ナビゲーション、レイアウト、ファイル、
メディア、ダイアログの種別にまとめた Gallery で確認できます。

```sh
cargo run -p gallery
```

Linux では、先に GTK4 と libadwaita の開発用ライブラリを導入してください。
Ubuntu 24.04 の例:

```sh
sudo apt install libgtk-4-dev libadwaita-1-dev build-essential pkg-config
```

Windows での実行には Windows App SDK 2.x のフレームワークランタイムが必要です。
このリポジトリでは 2.3.1 で動作を確認しています。

## 基本的な使い方

UI は `run` に渡すコールバックの中で組み立てます。ウィジェットのハンドルを
clone しても、参照するネイティブオブジェクトは同じです。

```rust
use std::cell::Cell;
use std::rc::Rc;

use naui::{Orientation, Padding, Settings};

fn main() -> naui::Result<()> {
    naui::run(Settings::new("counter"), |ui| {
        let window = ui.window("counter", 320.0, 200.0)?;
        let stack = ui.stack(Orientation::Vertical)?;
        stack.set_spacing(12.0);
        stack.set_padding(Padding::all(24.0));

        let label = ui.label("0")?;
        let button = ui.button("増やす")?;
        let count = Rc::new(Cell::new(0));

        button.on_click({
            let label = label.clone();
            move || {
                count.set(count.get() + 1);
                label.set_text(&count.get().to_string());
            }
        });

        stack.append(&label);
        stack.append(&button);
        window.set_child(&stack);
        window.show();
        Ok(())
    })
}
```

より小さな実例は [`examples/counter`](examples/counter/src/main.rs)、種別ごとの
全機能の使用例は [`examples/gallery`](examples/gallery/src/lib.rs) を参照してください。

### テーマ

既定の `Theme::System` は OS またはブラウザの設定へ追従します。起動時に固定する
場合は `Settings::theme`、実行中に切り替える場合は `Ui::set_theme` を使います。

```rust
use naui::{Settings, Theme};

let settings = Settings::new("counter").theme(Theme::Dark);
naui::run(settings, |ui| {
    ui.set_theme(Theme::System)?;
    Ok(())
})?;
```

### 配置とサイズ

レイアウトには次の 4 種類を使います。

| API | 用途 |
| --- | --- |
| `Stack` | 子を縦または横へ並べる |
| `Grid` | 行と列を指定して配置する |
| `Scroll` | はみ出した内容をスクロールする |
| `Spacer` | 親の余った空間を受け取る |

すべてのウィジェットは `Sizing` でサイズを指定できます。`Length` は中身に合わせる
`Auto`、固定値の `Fixed`、余った領域へ広がる `Fill` の 3 種類です。

```rust
use naui::{GridCell, Length, Sizing, Track};

let form = ui.grid()?;
form.set_spacing(12.0, 8.0);
form.set_column_track(0, Track::Fixed(96.0));
form.set_column_track(1, Track::FILL);

let field = ui.text_input("")?;
field.set_sizing(Sizing::new().width(Length::Fill));

form.attach(&ui.label("名前")?, GridCell::new(0, 0));
form.attach(&field, GridCell::new(1, 0));
```

`List`、`Scroll`、`TextArea` は内容から高さを決めないため、通常は
`set_sizing` で高さを指定します。

### 選択入力

省スペースな単一選択には `ComboBox` を使います。候補を入れ替えると選択は外れ、
`set_selected` は通知せず、`select` は利用者が選んだときと同じく通知します。

```rust
let language = ui.combo_box()?;
language.set_items(&["Rust", "Swift", "TypeScript"]);
language.set_selected(0);
language.on_select(|index| println!("{index} 番目が選ばれました"));
```

候補をすべて画面に出すなら `RadioGroup` を使います。API は `ComboBox` と同じで、
違うのは候補の見せ方と、並べる向きを選べることだけです。

```rust
let plan = ui.radio_group()?;
plan.set_items(&["無料", "標準", "上位"]);
plan.set_orientation(Orientation::Horizontal); // 既定は縦
plan.set_selected(0);
plan.on_select(|index| println!("{index} 番目が選ばれました"));
```

排他になるのは 1 つの `RadioGroup` の中だけです。同じ画面に複数置いても混ざりません。

## ウィジェット

| 分類 | API |
| --- | --- |
| 基本 | `Window`、`Label`、`Button`、`Checkbox`、`TextInput`、`TextArea`、`Slider`、`ProgressBar` |
| レイアウト | `Stack`、`Grid`、`Scroll`、`Spacer` |
| ナビゲーション | `Tabs`、`Navbar`、`Dock`、`Menu`、`Breadcrumbs`、`Pagination`、`Link` |
| データ選択 | `ComboBox`、`RadioGroup`、`List` |
| ファイル選択 | `FilePicker` |
| メディア | `Image`、`Video`、`Audio` |
| オーバーレイ | `PopupMenu`、`Dialog` |

### プラットフォーム別の実装

凡例:

| 記号 | 意味 |
| --- | --- |
| ✅ | プラットフォームの標準コントロールをそのまま使用 |
| 🟡 | 標準コントロールを組み合わせて実装 |
| 🔴 | 対応する概念がないため、別の要素で再現 |

#### ウィジェット対応表

| naui | Windows (WinUI 3) | macOS (AppKit) | Linux (GTK4) | Web (DOM) |
| --- | --- | --- | --- | --- |
| `Window` | ✅ `Microsoft.UI.Xaml.Window` | ✅ `NSWindow` | ✅ `AdwApplicationWindow` | 🔴 `<div>` + `document.title` |
| `Stack` | ✅ `StackPanel` | ✅ `NSStackView` | ✅ `GtkBox` | 🟡 `<div>` + CSS Flexbox |
| `Grid` | ✅ `Grid` | ✅ `NSGridView` | 🟡 `GtkGrid` | 🟡 `<div>` + CSS Grid |
| `Scroll` | ✅ `ScrollViewer` | ✅ `NSScrollView` | ✅ `GtkScrolledWindow` | 🟡 `<div>` + `overflow` |
| `Spacer` | 🔴 中身のない `Grid` | 🟡 中身のない `NSView` | 🟡 中身のない `GtkBox` | 🟡 `<div>` + `flex-grow` |
| `Label` | ✅ `TextBlock` | ✅ `NSTextField` | ✅ `GtkLabel` | ✅ `<span>` |
| `Button` | ✅ `Button` | ✅ `NSButton` | ✅ `GtkButton` | ✅ `<button>` |
| `Checkbox` | ✅ `CheckBox` | ✅ `NSButton` | ✅ `GtkCheckButton` | 🟡 `<input type="checkbox">` + `<label>` |
| `ComboBox` | ✅ `ComboBox` | ✅ `NSPopUpButton` | ✅ `GtkDropDown` | ✅ `<select>` |
| `RadioGroup` | 🟡 `StackPanel` + `RadioButton` | 🟡 `NSStackView` + `NSButton` (ラジオ型) | 🟡 `GtkBox` + 組にした `GtkCheckButton` | 🟡 `<div role="radiogroup">` + `<input type="radio">` |
| `TextInput` | ✅ `TextBox` | ✅ `NSTextField` | ✅ `GtkEntry` | ✅ `<input type="text">` |
| `TextArea` | ✅ `TextBox` | 🟡 `NSTextView` + `NSScrollView` | 🟡 `GtkTextView` + `GtkScrolledWindow` | ✅ `<textarea>` |
| `Slider` | ✅ `Slider` | ✅ `NSSlider` | ✅ `GtkScale` | ✅ `<input type="range">` |
| `ProgressBar` | 🟡 `Grid` + `Border` | ✅ `NSProgressIndicator` | ✅ `GtkProgressBar` | ✅ `<progress>` |
| `List` | ✅ `ListBox` | ✅ `NSTableView` + `NSScrollView` | 🟡 `GtkListBox` + `GtkScrolledWindow` | ✅ `<select size>` / 🟡 `<ul role="listbox">` |
| `FilePicker` | 🟡 `Button` + `IFileOpenDialog` | 🟡 `NSButton` + `NSOpenPanel` | 🟡 `GtkButton` + `GtkFileDialog` | 🟡 `<button>` + `<input type="file">` |
| `Image` | 🟡 `Image` (`XamlReader` 経由) | ✅ `NSImageView` | ✅ `GtkPicture` | ✅ `<img>` |
| `Video` | ✅ `MediaPlayerElement` | ✅ `AVPlayerView` | 🟡 `GtkPicture` + `GtkMediaControls` | ✅ `<video>` |
| `Audio` | ✅ `MediaPlayerElement` | 🟡 `AVPlayerView` | 🟡 `GtkMediaControls` + `GtkMediaFile` | ✅ `<audio>` |
| `PopupMenu` | 🟡 `Grid` + `Button` | ✅ `NSMenu` | ✅ `GtkPopoverMenu` + `GMenu` | 🟡 `<div role="menu">` |
| `Dialog` | ✅ `ContentDialog` | 🟡 `NSAlert` + `accessoryView` | ✅ `AdwAlertDialog` | 🟡 `<dialog>` + `showModal()` |

#### ナビゲーション対応表

| naui | Windows (WinUI 3) | macOS (AppKit) | Linux (GTK4) | Web (DOM) |
| --- | --- | --- | --- | --- |
| `Tabs` | 🟡 `Grid` + `ToggleButton` | ✅ `NSTabView` | ✅ `GtkNotebook` | 🟡 `role="tablist"` + `<button>` |
| `Navbar` | 🟡 `TextBlock` + `ToggleButton` | 🟡 `NSTextField` + `NSSegmentedControl` | 🟡 `GtkLabel` + `GtkToggleButton` | 🟡 `<nav>` + `<strong>` + `<button>` |
| `Dock` | 🟡 `ToggleButton` の横並び | ✅ `NSSegmentedControl` | 🟡 `GtkToggleButton` の横並び | 🟡 `<nav>` + `<button>` |
| `Menu` | 🟡 `ToggleButton` の縦並び | 🟡 `NSButton` の縦並び | 🟡 `GtkToggleButton` の縦並び | 🟡 `<nav><ul><li><button>` |
| `Breadcrumbs` | 🟡 `HyperlinkButton` + 区切り | ✅ `NSPathControl` | 🔴 `GtkToggleButton` + 区切り | 🟡 `<nav><ol><li><a>` |
| `Pagination` | 🟡 `Button` + `ToggleButton` | 🟡 `NSButton` + `NSSegmentedControl` | 🟡 `GtkButton` + `GtkToggleButton` | 🟡 `<nav>` + `<button>` |
| `Link` | ✅ `HyperlinkButton` | 🟡 `NSButton` + `NSWorkspace` | ✅ `GtkLinkButton` | ✅ `<a>` |

### 主なデータ型

- `Sizing` / `Length` / `Track` / `GridCell`: 配置とサイズ
- `NavItem`: ナビゲーション項目
- `ListItem` / `SelectionMode`: リスト項目と単一・複数選択
- `FileFilter` / `FilePickerMode` / `FileEntry`: ファイル選択
- `Fit` / `PlaybackState`: メディア表示と再生状態
- `PopupItem`: ポップアップメニュー項目
- `DialogButtons` / `DialogResponse`: ダイアログのボタンと応答

API の詳しい説明は、リポジトリ内で次のコマンドを実行して確認できます。

```sh
cargo doc --open -p naui
```

### ネイティブオブジェクトへのアクセス

共通 API で足りない場合は、バックエンド固有のオブジェクトを取得できます。

```rust,ignore
// macOS
let view = button.native_view();

// Web
let element = button.native_element();
```

このコードは対象プラットフォームに依存するため、必要に応じて `cfg` で分けてください。

## Web 版の実行

wasm ターゲットと、`Cargo.lock` に記録されたものと同じバージョンの
`wasm-bindgen-cli` が必要です。現在のロックファイルでは 0.2.127 です。

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.127
cd examples/gallery/web
./build.sh
python3 -m http.server 8080
```

その後、<http://localhost:8080/> を開いてください。

ネイティブと Web の両方を提供するアプリでは `entry!` を使うと、
`#[wasm_bindgen(start)]` を含む入口を共通化できます。

```rust,ignore
naui::entry!(Settings::new("naui gallery"), build);
```

## 開発

### クレート構成

```text
crates/
  naui-core      共通の値型
  naui-macos     AppKit バックエンド
  naui-web       DOM バックエンド
  naui-windows   WinUI 3 バックエンド
  naui-gtk       GTK4 / libadwaita バックエンド
  naui           対象に応じてバックエンドを選ぶファサード
examples/
  counter        最小サンプル
  gallery        種別ごとの全ウィジェットデモ
```

バックエンド固有の依存はターゲット別に宣言されています。たとえば macOS の
ビルドで GTK4 や WinUI 3 の依存が引き込まれることはありません。

### テスト

`naui-core`、現在のプラットフォーム用バックエンド、`naui` を指定して実行します。
たとえば macOS では:

```sh
cargo test -p naui-core -p naui-macos -p naui
```

Linux ではディスプレイ (Wayland または X11) が必要です。

```sh
cargo test -p naui-core -p naui-gtk -p naui
```

別ターゲットの API 互換性は `cargo check` で確認できます。

```sh
cargo check --target wasm32-unknown-unknown -p naui
cargo check --target x86_64-pc-windows-msvc -p naui
cargo check --target x86_64-unknown-linux-gnu -p naui
```

`crates/naui/src/lib.rs` の `__api_contract` が公開 API を一通り型検査し、
バックエンド間のシグネチャのずれを検出します。

## 既知の制限

### 共通

- 対応するのは上記の 27 コンポーネントです。保存ダイアログ、複数列テーブル、
  ラジオボタン、ツールバー、ツリーは未実装です。
- 絶対配置はありません。`Stack`、`Grid`、`Spacer` で配置します。
- `List` は 1 列で、行に置けるのは `label` と `detail` の文字列だけです。
- `Dialog` は同時に 1 つだけで、ボタンは Primary、Secondary、Cancel の最大 3 個です。
- ウィンドウを閉じるイベントと、入力欄で Enter を押したときの共通
  `on_submit` はありません。
- メディアの対応形式は各 OS、ブラウザ、Linux の GStreamer 環境に依存します。

### Windows

- `StackPanel` は主軸の余りを子へ配らないため、`Stack` 内の主軸方向では
  `Fill` と `Spacer` が効きません。代わりに `Grid` の `Track::Fill` を使います。
- 一部の Windows App SDK 環境で異常終了を避けるため、`Tabs` は `TabView` を使わず、
  `Video` / `Audio` の標準再生バーは無効にしています。
- `Dialog` は `window.show()` より前には開けません。

### macOS

- `Grid` の `Track::Fill` は重みの違いを反映しません。
- 交差軸の `Fill` と Grid セル内の配置は、コンテナへ追加する前に指定してください。
- `Image` のリモート URL は同期的に読み込むため、ローカルファイルの利用を推奨します。
- `Dialog::open` と `PopupMenu::open_at` は閉じるまで戻りません。

### Linux

- `Grid` の `Track::Fill` は重みの違いを反映しません。
- `Fit::None` は GTK4 の `SCALE_DOWN` に対応するため、「原寸」ではなく
  「拡大しない」動作になります。
- テーマはウィンドウ単位ではなくアプリ全体へ適用されます。

### Web

- `Window` は OS のウィンドウではなく、`<body>` 直下の要素と
  `document.title` で表現されます。
- `ListItem::detail` を使うと、`List` は `<select>` から
  `<ul role="listbox">` を使った実装へ切り替わります。
- `FilePicker::open` はユーザー操作のイベント内で呼ぶ必要があります。
- ブラウザの制限により、メディアの自動再生が拒否される場合があります。

## ライセンス

[MIT](LICENSE-MIT) OR [Apache-2.0](LICENSE-APACHE)
