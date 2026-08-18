# naui

**各 OS のネイティブ UI を、1 つの API から扱う軽量 GUI ツールキット (Rust)。**

naui は自前で描画しません。`ui.button("押す")` が返すのは **本物の OS のボタン**であり、
描画・レイアウト・IME・アクセシビリティ・OS のテーマ追従は、すべて
プラットフォームのツールキットが行います。

| ビルド対象 | 使うツールキット | 例: ボタンの実体 |
| --- | --- | --- |
| Windows | **WinUI 3** (Windows App SDK) | `Microsoft.UI.Xaml.Controls.Button` |
| macOS | **AppKit** | `NSButton` |
| Linux | GTK4 / libadwaita | `GtkButton` (**未実装**) |
| Web (wasm) | **DOM** | `<button>` |

---

## 実装状況 (最初に読んでください)

| 環境 | 状態 | 根拠 |
| --- | --- | --- |
| **macOS** | ✅ 動作 | アプリを実行して確認。AppKit の実コントロールに対する自動テスト 51 件 (ポップアップメニューの `NSMenu` への写しと選択の通知を含む)。メディアは実ファイルの再生 (状態変化・長さ・再生位置・繰り返し) まで自動テストで確認 |
| **Web (wasm)** | ✅ 動作 | ブラウザで実行し、全ウィジェットを DOM イベントで操作して確認 (ナビゲーション系も、ナビバー・タブ・メニュー・ページ送り・ドックのクリックがコールバックまで届くことを確認)。グリッド・スクロール・スペーサーは実際の描画位置を測って確認。`FilePicker` はボタンから `<input>` への転送と、選択 (単数 / 複数 / フォルダー) がコールバックへ届くところまで確認。`Image` / `Video` / `Audio` の表示・再生もブラウザで確認済み。**`PopupMenu` はブラウザでの実行確認をしていません** (合成した `<div role="menu">` のコードはビルドが通るところまで)。`List` (`<select size>`) も、行のクリック・複数選択・プログラムからの選択・選べない行のクリックがすべて期待どおりに動くことをブラウザで確認済み |
| **Windows** | ✅ 動作 | Windows App SDK 2.3.1 の実機で `cargo run -p gallery` を実行し、基本ウィジェット・ナビゲーション系 7 種・レイアウト (`Grid` / `Scroll` / `Spacer` / `set_sizing`、`Scroll` のマウスホイール対応を含む)・`FilePicker` のファイル / フォルダー選択・`Image` / `Video` / `Audio` の読み込みと再生・動画表示のリサイズを確認済み。**`PopupMenu` は実機で未確認**で、`x86_64-pc-windows-msvc` 向けの `cargo check` が通るところまでです |
| **Linux** | ❌ 未実装 | API の形だけ定義した骨組み。呼ぶとエラーを返します |

Linux が未実装なのは、GTK4 バックエンドがまだ骨組みの段階だからです。
Windows は Windows App SDK 2.3.1 ランタイムを備えた x64 環境で、
`Tabs` / `Navbar` / `Dock` / `Menu` / `Breadcrumbs` / `Pagination` / `Link` を含む
`gallery` の起動を確認済みです。
詳細は [`crates/naui-gtk`](crates/naui-gtk/src/lib.rs) のドキュメントを参照してください。

---

## 使い方

UI は `run` に渡すコールバックの中で組み立てます。
WinUI 3 が `Application::Start` より前のコントロール生成を許さないため、
4 バックエンドで同じ形にそろえてあります。

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

### テーマ

テーマは `Theme::System` (OS / ブラウザの設定に追従) が標準です。起動時に固定する場合は
`Settings::theme` を使い、実行中は `Ui::set_theme` で切り替えられます。

```rust
use naui::{Settings, Theme};

let settings = Settings::new("counter").theme(Theme::System);
naui::run(settings, |ui| {
    // 設定画面などのイベントからも呼べます。
    ui.set_theme(Theme::Dark)?;
    // ui.set_theme(Theme::Light)?;
    // ui.set_theme(Theme::System)?;
    Ok(())
})?;
```

`System` は macOS / Windows ではネイティブ UI のシステムテーマ、Web では
`prefers-color-scheme` に対応するブラウザの配色を使います。

ウィジェットのハンドルは `Rc` なので clone しても実体は 1 つです。
コンテナに `append` した子はコンテナが保持するため、ハンドルを手放しても
コールバックは生き続けます。

### ネイティブへの脱出口

共通 API で足りない部分は、各バックエンドのネイティブオブジェクトを直接取れます。

```rust
// macOS
let ns_button: objc2::rc::Retained<NSView> = button.native_view();
// Web
let element: web_sys::Element = button.native_element();
```

---

## ウィジェット

凡例:

| 記号 | 意味 |
| --- | --- |
| ✅ | **完全ネイティブ** — そのプラットフォームの標準コントロールを 1 つそのまま使用 |
| 🟡 | **ネイティブ + 合成** — ネイティブコントロールは使うが、単体では足りない部分を組み立てている |
| 🔴 | **再現** — 相当するネイティブの概念が無いため、別の要素で代用している |
| ❌ | 未実装 |

| naui | Windows (WinUI 3) | macOS (AppKit) | Web (DOM) | Linux (GTK4) |
| --- | --- | --- | --- | --- |
| `Window` | ✅ `Microsoft.UI.Xaml.Window` | ✅ `NSWindow` | 🔴 `<div>` + `document.title` | ❌ |
| `Stack` | ✅ `StackPanel` | ✅ `NSStackView` | 🟡 `<div>` + CSS Flexbox | ❌ |
| `Grid` | ✅ `Grid` (`RowDefinition` / `ColumnDefinition`) | ✅ `NSGridView` | 🟡 `<div>` + CSS Grid | ❌ |
| `Scroll` | ✅ `ScrollViewer` | ✅ `NSScrollView` | 🟡 `<div>` + `overflow` | ❌ |
| `Spacer` | 🔴 中身の無い `Grid` (`StackPanel` では効かない) | 🟡 中身の無い `NSView` (hugging priority 最小) | 🟡 `<div>` + `flex-grow` | ❌ |
| `Label` | ✅ `TextBlock` | ✅ `NSTextField` (`labelWithString:`) | ✅ `<span>` | ❌ |
| `Button` | ✅ `Button` | ✅ `NSButton` (`buttonWithTitle:`) | ✅ `<button>` | ❌ |
| `Checkbox` | ✅ `CheckBox` | ✅ `NSButton` (`checkboxWithTitle:`) | 🟡 `<input type=checkbox>` + `<label>` | ❌ |
| `TextInput` | ✅ `TextBox` | ✅ `NSTextField` (`textFieldWithString:`) | ✅ `<input type=text>` | ❌ |
| `Slider` | ✅ `Slider` | ✅ `NSSlider` | ✅ `<input type=range>` | ❌ |
| `ProgressBar` | 🟡 `Grid` + `Border` (WinUI XAML) | ✅ `NSProgressIndicator` (Bar) | ✅ `<progress>` | ❌ |
| `List` | ✅ `ListBox` + `ListBoxItem` | ✅ `NSTableView` (1 列) + `NSScrollView` | ✅ `<select size>` (文字だけ) / 🟡 `<ul role=listbox>` (`detail` あり) | ❌ |
| `FilePicker` | 🟡 `Button` + `IFileOpenDialog` (共通ダイアログ) | 🟡 `NSButton` + `NSOpenPanel` | 🟡 `<button>` + 隠した `<input type=file>` | ❌ |
| `Image` | 🟡 `Image` (バインディングが無いため `XamlReader` 経由) | ✅ `NSImageView` | ✅ `<img>` | ❌ |
| `Video` | ✅ `MediaPlayerElement` + `MediaPlayer` | ✅ `AVPlayerView` (AVKit) | ✅ `<video>` | ❌ |
| `Audio` | ✅ `MediaPlayerElement` (映像トラック無し) | 🟡 `AVPlayerView` (映像面を持たない) | ✅ `<audio>` | ❌ |

### 配置とサイズ

どのウィジェットも `set_sizing` で大きさを指定できます。計算するのは
ネイティブのレイアウト機構 (Auto Layout / XAML のレイアウトパス / CSS) で、
naui は制約やプロパティを設定するだけです。

```rust
use naui::{GridCell, Length, ScrollPolicy, Sizing, Track};

// 幅は親いっぱい、高さは 160px、幅は 120px 以上。
widget.set_sizing(
    Sizing::new()
        .width(Length::Fill)
        .height(Length::Fixed(160.0))
        .min_width(120.0),
);
```

`Length` は 3 つです。

| 値 | 意味 |
| --- | --- |
| `Auto` | 中身に合わせる (既定) |
| `Fixed(f64)` | 論理ピクセルで固定する |
| `Fill` | 親の余りを受け取って広がる |

`Fill` の意味は軸によって変わります。親の並び方向 (主軸) では**余った空間を
受け取り**、それと直交する方向 (交差軸) では**親いっぱいに広がります**。

| | 主軸の `Fill` | 交差軸の `Fill` |
| --- | --- | --- |
| macOS | hugging priority を下げて NSStackView から余りを受け取る | 親の幅 / 高さに合わせる制約 |
| Web | `flex-grow: 1` | `align-self: stretch` |
| Windows | **`Stack` では効きません** (StackPanel は余りを配らない)。`Grid` の `Track::Fill` を使ってください | `HorizontalAlignment` / `VerticalAlignment` の `Stretch` |

#### Grid

行と列で位置を決めるコンテナです。列 / 行の幅は `Track` で決めます。

```rust
let form = ui.grid()?;
form.set_spacing(12.0, 8.0);          // 列間, 行間
form.set_column_track(0, Track::Fixed(96.0));
form.set_column_track(1, Track::FILL); // 残りいっぱい
form.attach(&ui.label("名前")?, GridCell::new(0, 0));
form.attach(&field, GridCell::new(1, 0));
form.attach(&submit, GridCell::new(0, 1).span(2, 1)); // 2 マス分
```

マスの中では、**縦は中央ぞろえ**です (`Fill` を指定した子だけマスいっぱいに
広がります)。ラベルと入力欄のように高さの違うものを同じ行に並べても、
上端でずれません。横は各環境の既定 (先頭ぞろえ) のままです。

`Track::Fill(weight)` の重みは、Web では `fr`、Windows では `Star` に対応します。
macOS の NSGridView には重みの概念が無いため、**重みの違いは反映されません**
(`Fill` 配置と hugging priority による近似です)。

#### Scroll

はみ出した分をスクロールさせるコンテナです。既定は横 `Never`・縦 `Auto`。

```rust
let scroll = ui.scroll()?;
scroll.set_policy(ScrollPolicy::Never, ScrollPolicy::Auto);
scroll.set_child(&long_list);
scroll.set_sizing(Sizing::new().width(Length::Fill).height(Length::Fixed(160.0)));
```

Windows では、WinUI 3 のホストウィンドウでマウスホイール入力を受け取り、
表示中の `ScrollViewer` のスクロール位置へ反映します。そのため、スクロール領域の
子要素上でホイールを操作した場合も縦にスクロールできます。`ScrollPolicy::Never` を
指定した軸は、スクロールバーとマウスホイールのどちらからもスクロールしません。

#### Spacer

中身を持たず、余った空間だけを受け取るウィジェットです。縦スタックの途中に
置くと、後ろの要素が下端へ寄ります (`Dock` を画面下端に置く用途)。

```rust
root.append(&ui.spacer()?);
root.append(&dock);   // 下端に寄る
```

Windows の `StackPanel` は余りを配らないため、`Spacer` と主軸の `Fill` は
`Stack` の中では効きません。`Grid` の行を `Track::Fill` にしてください。

### メディア

写真・動画・音声を表示します。**デコードも再生も naui は行いません。**
ファイルを開くのも再生バーを描くのも、その環境のツールキット
(AVFoundation / ブラウザ / Windows.Media.Playback) の仕事です。

```rust
use naui::{Fit, PlaybackState};

let photo = ui.image("/path/to/photo.jpg")?;   // パスでも URL でもよい
photo.set_fit(Fit::Cover);
photo.set_alt("桜の写真");                      // 読み上げ用の説明

let movie = ui.video("https://example.com/clip.mp4")?;
movie.set_sizing(Sizing::fixed(320.0, 180.0));
movie.play();
movie.set_volume(0.5);

let sound = ui.audio("/path/to/bgm.m4a")?;     // 音声を再生する
```

`Video` と `Audio` は**同じ形の再生 API** を持ちます。

| メソッド | 意味 |
| --- | --- |
| `set_source(&str)` / `source()` | 場所を指定する。再生は止まり `Idle` に戻る |
| `play()` / `pause()` | 再生・一時停止。最後まで再生した後の `play()` は先頭へ戻す |
| `seek(秒)` / `position()` | 再生位置。負の値は先頭として扱う |
| `duration()` | 長さ (秒)。**読み込みが終わるまで `None`**。ライブ配信も `None` |
| `set_volume(0.0..=1.0)` / `set_muted(bool)` | 音量と消音。範囲外は丸める |
| `set_loop(bool)` / `set_autoplay(bool)` | 繰り返しと自動再生 |
| `set_controls(bool)` | ネイティブの再生バーを出すか (Windows では安全性のため常に無効。Gallery は独自の操作欄を使用) |
| `on_state_change(f)` | 状態が変わったとき。`Idle` / `Buffering` / `Playing` / `Paused` / `Ended` |
| `on_position_change(f)` | 再生位置が進むたび (およそ 4 回/秒)。シークバーの追従に使う |

`on_state_change` と `on_position_change` は、アプリから `play()` を呼んだときだけで
なく、macOS / Web の再生UIや、Windowsでアプリ側に用意した操作欄をユーザーが操作した
ときにも届きます。

`Fit` は画像と動画の映像面の収め方です。

| 値 | 意味 |
| --- | --- |
| `Contain` | 縦横比を保って収める (既定) |
| `Cover` | 縦横比を保って埋める。はみ出しは切り取る |
| `Fill` | 縦横比を無視して引き伸ばす |
| `None` | 原寸のまま |

#### ファイル選択と組み合わせる

`FilePicker` が返す `FileEntry::source()` は、そのままメディアへ渡せます。
ネイティブでは絶対パス、Web ではブラウザが作る `blob:` URL になります。

```rust
let pick = ui.file_picker("動画を選ぶ")?;
pick.set_filters(&[FileFilter::new("動画", ["mp4", "mov"])]);
pick.on_select({
    let movie = movie.clone();
    move |entries| {
        if let Some(source) = entries.first().and_then(|e| e.source()) {
            movie.set_source(source);
        }
    }
});
```

Web の `blob:` URL は、**同じ `FilePicker` で次に選び直すまで**有効です
(選び直すと以前のものは `URL.revokeObjectURL` で破棄されます)。

どのウィジェットで表示するかは、**`FileFilter` で受け付ける拡張子を絞って
選ばせる**のが確実です。選ばれた時点で種類が決まるので、naui 側に種類を
推測する仕組みは持たせていません。

### ナビゲーション

| naui | Windows (WinUI 3) | macOS (AppKit) | Web (DOM) | Linux (GTK4) |
| --- | --- | --- | --- | --- |
| `Tabs` | 🟡 `Grid` + `ToggleButton` (TabViewの未パッケージ起動回避) | ✅ `NSTabView` + `NSTabViewItem` | 🟡 `role="tablist"` + `<button role=tab>` + `hidden` | ❌ |
| `Navbar` | 🟡 `TextBlock` + `ToggleButton` の横並び | 🟡 `NSTextField` + `NSSegmentedControl` | 🟡 `<nav>` + `<strong>` + `<button>` | ❌ |
| `Dock` | 🟡 `ToggleButton` の横並び | ✅ `NSSegmentedControl` (等幅) | 🟡 `<nav>` + `<button>` (等幅) | ❌ |
| `Menu` | 🟡 `ToggleButton` の縦並び | 🟡 `NSButton` (AccessoryBar) の縦並び | 🟡 `<nav><ul><li><button>` | ❌ |
| `Breadcrumbs` | 🟡 `HyperlinkButton` + 区切りの `TextBlock` | ✅ `NSPathControl` + `NSPathControlItem` | 🟡 `<nav><ol><li><a href>` | ❌ |
| `Pagination` | 🟡 `Button` + `ToggleButton` | 🟡 `NSButton` + `NSSegmentedControl` | 🟡 `<nav>` + `<button>` | ❌ |
| `Link` | ✅ `HyperlinkButton` | 🟡 `NSButton` (枠なし・リンク色) + `NSWorkspace` | ✅ `<a href>` | ❌ |

7 種類とも **同じ形の API** を持ちます。項目は `NavItem` の並びで渡し、
選ばれたものはインデックスで返ります。

```rust
let navbar = ui.navbar("naui")?;
navbar.set_items(&NavItem::list(["ホーム", "検索", "設定"]));
navbar.on_select(|index| println!("{index} 番目が選ばれた"));
navbar.set_selected(0);            // 通知せずに選択を変える
navbar.select(1);                  // ユーザー操作と同じ経路 (通知あり)
let _: Option<usize> = navbar.selected();
```

| メソッド | 意味 |
| --- | --- |
| `set_items(&[NavItem])` | 項目を作り直す。インデックスの意味が変わるため選択は外れる (`Breadcrumbs` だけは末尾が現在地になる) |
| `selected()` | いま選ばれている位置。未選択なら `None` |
| `set_selected(i)` | **通知せずに**選択を変える (アプリの状態を UI に反映する用) |
| `select(i)` | ユーザーが選んだのと同じ経路で選択を変える (通知あり。テストや自動操作にも使える) |
| `on_select(f)` | 選ばれたときに呼ばれる。設定し直すと以前のものは外れる |

`Tabs` だけは中身のウィジェットごと持つため `add_tab(label, &child)` で組み立て、
`Pagination` はページ番号を扱うため `set_page_count` / `page` / `set_page` /
`go_previous` / `go_next` / `on_change` という名前になっています。

### ポップアップ (コンテキスト) メニュー

`PopupMenu` は**右クリック (副ボタン) で出るメニュー**です。画面に並ばないため
`Widget` ではなく、`Stack` や `Grid` に入れることはできません。

| naui | Windows (WinUI 3) | macOS (AppKit) | Web (DOM) | Linux (GTK4) |
| --- | --- | --- | --- | --- |
| `PopupMenu` | 🟡 ルートに重ねる `Grid` + `Button` の縦並び | ✅ `NSMenu` + `NSMenuItem` | 🟡 `<div role="menu">` + `<button role="menuitem">` | ❌ |

```rust
let label = ui.label("右クリックしてください")?;

let popup = ui.popup_menu()?;
popup.set_items(&[
    PopupItem::new("コピー"),
    PopupItem::separator(),               // 選べないので通知も来ない
    PopupItem::new("削除").enabled(false),
]);
popup.on_select(|index| println!("{index} 番目が選ばれた"));
popup.attach(&label);                    // 右クリックで出るようにする
popup.open_at(&label, 0.0, 24.0);        // プログラムから出す (左上からの位置)
popup.close();                           // 出ているものを閉じる
```

| メソッド | 意味 |
| --- | --- |
| `set_items(&[PopupItem])` | 項目を作り直す。インデックスは**区切り線を含めた並びの位置** |
| `attach(&widget)` | そのウィジェットの右クリックでメニューを出す。いくつでも取り付けられる |
| `open_at(&widget, x, y)` | プログラムから出す。位置はウィジェットの左上からの論理ピクセル (y は下向き) |
| `close()` | 出ているメニューを閉じる |
| `select(i)` | ユーザーが選んだのと同じ経路で通知する (テストや自動操作用)。区切り線と選べない項目は無視する |
| `on_select(f)` | 選ばれたときに呼ばれる。設定し直すと以前のものは外れる |

**持たないもの**: 階層 (サブメニュー)、チェック印、ショートカットの表示。
項目は「文字・選べるかどうか・区切り線」だけで、4 環境がそろって同じ形で
扱える範囲にそろえてあります。

ネイティブのメニューがあるのは macOS (`NSMenu`) だけで、**Windows と Web は
合成**です (WinUI 3 の `MenuFlyout` は `winio-winui3` のバインディングに無く、
ブラウザにはコンテキストメニューを差し替える API がありません)。合成のほうは
**矢印キーでの項目移動を持ちません**。Escape で閉じられるのは Web だけで、
Windows はキーイベントのバインディングが無いため閉じられません
(メニューの外側を押せば、どちらも閉じます)。

ブラウザ既定のコンテキストメニューを抑止するのは、**取り付けたウィジェットの
上だけ**です。それ以外の場所では今までどおり出ます。

`Menu` (縦に並ぶナビゲーション一覧) との違いは役割です。`Menu` は画面を
切り替えるもので画面に並び続け、`PopupMenu` は操作を選ばせるもので
押したときだけ出ます。

### リスト

`List` は**行が縦に並ぶ一覧**です。ナビゲーションの `Menu` と見た目は
似ていますが、役割が違います。`Menu` は画面を切り替えるもの、`List` は
データを選ぶもので、`List` だけが**複数選択**と**自前のスクロール**を持ちます。

```rust
let list = ui.list()?;
list.set_items(&ListItem::list(["札幌", "東京", "大阪"]));
list.set_selection_mode(SelectionMode::Multiple);  // 既定は Single
list.on_select(|indices| println!("{indices:?} が選ばれた"));

list.set_selection(&[0, 2]);       // 通知せずに選択を置き換える
list.select(1);                    // ユーザー操作と同じ経路 (通知あり)
let _: Vec<usize> = list.selection();
let _: Option<usize> = list.selected();  // 選ばれているうち、いちばん上の行

// スクロールと同じく高さを自分では決めないので、指定しておく。
list.set_sizing(Sizing::new().width(Length::Fill).height(Length::Fixed(180.0)));
```

行には**補助の文字**を付けられます。macOS / Windows では 2 行目に小さく出て、
その行だけ高さが増えます (高さを決めるのは AppKit の
`usesAutomaticRowHeights` と WinUI のレイアウトパスで、naui は制約を張るだけです)。

```rust
list.set_items(&[
    ListItem::new("札幌").detail("北海道"),
    ListItem::new("東京").detail("東京都"),
    ListItem::new("京都"),                    // detail 無しの行は 1 行のまま
]);
```

**Web は行の中身で作りが変わります。** `<option>` の内容モデルはテキストのみで、
要素も改行も置けません。そこで

| 行 | Web の作り |
| --- | --- |
| 文字だけ | `<select size>` + `<option>` — ブラウザ標準のリストボックスそのもの |
| `detail` あり | `<ul role="listbox">` + `<li role="option">` — 2 行にするための合成 |

と切り替えます。`<select>` は選択もキーボード操作もスクロールもブラウザが
面倒を見てくれる本物のコントロールなので、**その必要が無いときは使い続けます**。
合成のほうは naui がクリック・⌘ / Ctrl / Shift での複数選択・矢印キー・
Home / End・`aria-activedescendant` を受け持ちますが、枠と選択の配色は
ブラウザのシステム色 (`Field` / `SelectedItem` / `Highlight` / `GrayText`) に
任せていて、naui は色を決めません。

| メソッド | 意味 |
| --- | --- |
| `set_items(&[ListItem])` | 行を作り直す。インデックスの意味が変わるため選択は外れる |
| `set_selection_mode(mode)` | `Single` (既定) と `Multiple` の切り替え。切り替えると選択は外れる |
| `selection()` | 選ばれている行を**昇順**で返す。複数選択では 0 件にもなる |
| `selected()` | 選ばれているうち、いちばん上の行。無ければ `None` |
| `set_selected(i)` / `set_selection(&[i])` | **通知せずに**選択を置き換える |
| `clear_selection()` | **通知せずに**選択をすべて外す |
| `select(i)` / `select_many(&[i])` | ユーザーが選んだのと同じ経路で選択を変える (通知あり) |
| `on_select(f)` | 選択が変わったときに、選ばれている行 (昇順) で呼ばれる |

`set_selection` に渡した並びは、**範囲外・選べない行 (`ListItem::enabled` が
`false`)・重複が取り除かれ、昇順にそろえられます**。`Single` のときは
先頭の 1 件だけが残ります。この正規化は `naui-core` の
`SelectionMode::normalize` にあり、3 バックエンドとも同じものを通します。

`Multiple` はどの環境でも「⌘ / Ctrl や Shift を押しながら選ぶ」形です
(WinUI では `SelectionMode::Multiple` ではなく `Extended` に写しています。
`Multiple` はクリックのたびに反転する挙動で、macOS / Web と揃わないためです)。

実際の動きは `gallery` の**「リスト」タブ**で確認できます。単一 / 複数の切り替え、
選択の表示、通知あり (`select`) と通知なし (`clear_selection`) の違い、
選べない行 (末尾の「那覇 (準備中)」) がクリックで選ばれないことを試せます。

```sh
cargo run -p gallery          # ネイティブ (macOS / Windows)
```

### ファイルとフォルダーの選択

`FilePicker` は**ボタン 1 つ**です。押すと、その環境の標準のファイル選択が
開きます (macOS は `NSOpenPanel`、Windows はエクスプローラーと同じ
Common Item Dialog、Web はブラウザのファイル選択)。一覧・検索・アクセス権限の
扱いは、すべてその環境が行います。

```rust
use naui::{FileFilter, FilePickerMode};

let picker = ui.file_picker("画像を選ぶ")?;
picker.set_mode(FilePickerMode::File);   // File / Files / Folder
picker.set_filters(&[FileFilter::new("画像", ["png", "jpg"])]);
picker.on_select(|entries| {
    for entry in entries {
        match entry.path() {
            Some(path) => println!("{}", path.display()),  // ネイティブ
            None => println!("{}", entry.name()),          // Web
        }
    }
});
stack.append(&picker);
```

| メソッド | 意味 |
| --- | --- |
| `set_mode(FilePickerMode)` | 何を選ばせるか。`File` (既定) / `Files` (複数) / `Folder` |
| `set_filters(&[FileFilter])` | 拡張子で絞り込む。`Folder` のときは無視される |
| `selection()` | 最後に選ばれたもの (`Vec<FileEntry>`)。未選択なら空 |
| `on_select(f)` | 選ばれたときに `&[FileEntry]` で呼ばれる。**取り消したときは呼ばれない** |
| `open()` | ボタンを押さずにダイアログを出す (Web は後述の制約あり) |

選ばれたものは `FileEntry` で、`name()` は表示名、`path()` は絶対パスです。

| 環境 | `path()` | 備考 |
| --- | --- | --- |
| macOS / Windows | `Some(絶対パス)` | |
| Web | **常に `None`** | ブラウザはパスを渡さない。中身が要るときは `native_element()` から `<input>` を取り出して `FileList` を読む |

**Web には 2 つ制約があります。**

- `open()` は**ユーザー操作のイベントの中でしか効きません**。ブラウザが
  ファイル選択の自動起動を禁じているためで、ボタンを押した経路 (既定の動き)
  なら問題ありません。
- フォルダーを選ぶと、ブラウザは**そのフォルダーの中のファイル一覧**を返します。
  他の環境はフォルダー 1 つを返すので、naui は `webkitRelativePath` の先頭から
  フォルダー名を取り出し、**1 件に畳んで**そろえています。

保存ダイアログ (名前を付けて保存) はありません。`<input type=file>` に相当が
無く、Web だけ形が変わってしまうためです。

### 🟡 / 🔴 の内訳

| 箇所 | 内容 |
| --- | --- |
| 🔴 Web の `Window` | ブラウザにはページ内ウィンドウの概念が無い。`<body>` 直下の `<div>` で代用し、タイトルは `document.title` に反映している。`show()` / `close()` は `display` の切り替え、`set_size()` は `max-width` / `min-height` の指定であり、**OS のウィンドウ操作ではない** |
| 🟡 Web の `Stack` | HTML に「スタック」というコントロールは存在しない。ただし CSS Flexbox はブラウザ自身のレイアウト機構なので、独自のレイアウト計算はしていない (`display:flex` + `flex-direction` + `gap` + `padding`) |
| 🟡 Web の `Checkbox` | `<input type=checkbox>` 自体はネイティブだが、ラベル文字列を持たないため `<label>` で `<input>` と `<span>` を包んでいる |
| 🟡 Web のナビゲーション全般 | ブラウザに「タブ」「ナビバー」というコントロールは無い。`<nav>` / `<ol>` / `<a>` / `<button>` と WAI-ARIA のロール (`tablist` / `tab` / `tabpanel` / `aria-current`) で意味づけし、隠すのは `hidden` 属性に任せている。CSS は Flexbox のレイアウトと、選択中を示す `font-weight: bold` だけ |
| 🟡 macOS の `Navbar` | `NSSegmentedControl` はネイティブだが、見出しを持てないため `NSTextField` と `NSStackView` で横に並べている |
| 🟡 macOS の `Menu` | AppKit の `NSMenu` はポップアップ用。サイドバー相当の縦一覧は `NSButton` (AccessoryBar・PushOnPushOff) を `NSStackView` に並べて作っている |
| macOS の `List` の行 | 文字だけの `NSTextField` を行にすると、AppKit が枠の上端に文字を描くため選択の帯とずれる。AppKit 標準の `NSTableCellView` に入れ、上下の余白まで制約でつないで、高さを `usesAutomaticRowHeights` に求めさせている |
| 🟡 Web の `List` (`detail` あり) | `<option>` はテキストしか持てず 2 行にできないため、`<ul role="listbox">` + `<li role="option">` を組み立てている。選択とキーボード操作は naui が受け持つ。**文字だけの行なら `<select size>` のまま**で、この合成は使わない |
| 🟡 macOS の `Link` | AppKit にリンク専用のコントロールは無い。枠なしの `NSButton` を `NSColor::linkColor` にし、`href` は `NSWorkspace` で開いている |
| 🟡 Windows の `Image` | `Microsoft.UI.Xaml.Controls.Image` と `BitmapImage` が `winio-winui3` 0.4.5 のバインディングに無く、Rust から `Source` を設定できない。`XamlReader` に `<Image>` の XAML を読ませ、ホストの `Grid` の中身を差し替えている (`ProgressBar` と同じ手口)。表示するのは WinUI 標準の `Image` そのもの |
| 🟡 Windows の `Video` / `Audio` | `MediaPlayerElement` と `MediaPlayer` を使う。Windows App SDK の一部環境で標準 `MediaTransportControls` を visual tree に追加すると `0xc000027b` で終了するため、標準の再生バーは無効にし、Gallery は独自の操作欄を使っている |
| 🟡 macOS の `Audio` | AppKit に音声専用のコントロールは無い。映像トラックの無いメディアを `AVPlayerView` に載せると再生バーだけが出るので、それを使っている |
| 🟡 すべての `Pagination` | ページ送りに相当するネイティブコントロールはどの環境にも無い。前へ / 次へのボタンとページ番号を、その環境のネイティブなボタンで並べている |
| 🟡 Windows の `Navbar` / `Dock` / `Menu` | `NavigationView` は `winio-winui3` 0.4.5 のバインディングに含まれていないため、WinUI 標準の `ToggleButton` を `StackPanel` に並べ、選択状態を `IsChecked` で表している |
| 🟡 Windows の `PopupMenu` | `MenuFlyout` は `winio-winui3` 0.4.5 のバインディングに含まれていない。ウィンドウのルート (いちばん外側の `Panel`) へ透明な `Grid` を重ね、その中に WinUI 標準の `Button` を縦に並べている。位置は `Margin`、色は `{ThemeResource ...}` なので Fluent のテーマには追従する |
| 🟡 Web の `PopupMenu` | ブラウザ既定のコンテキストメニューは差し替えられないため、`<body>` 直下に `position: fixed` の `<div role="menu">` を置いて合成している。色は CSS のシステムカラー (`Canvas` / `CanvasText`) なので `color-scheme` に追従する |
| 🟡 Windows の `Breadcrumbs` | `BreadcrumbBar` は `winio-winui3` 0.4.5 のバインディングに含まれていないため、標準の `HyperlinkButton` と区切り文字を `StackPanel` に並べている |

### 補足 (誤解しやすい箇所)

| 箇所 | 説明 |
| --- | --- |
| macOS の `Image` の読み込み | `NSImage` は**同期的に**読む。リモートの URL を渡すと読み終わるまで UI が止まるため、ローカルのファイルを渡すこと |
| `duration()` が `None` を返す間 | メディアの読み込みは 3 環境とも非同期。`set_source` の直後は長さが決まっていないので `None` になる。決まったかどうかは `on_state_change` / `on_position_change` を見る |
| `Fit::Cover` と macOS の `Image` | `NSImageView` に設定が無いため独自描画で縦横比を保って拡大し、表示領域外を切り取る (動画の `Video` は `AVLayerVideoGravity` があるので効く) |
| Web の自動再生 | ブラウザの自動再生制限で `play()` が拒否されることがある。拒否されると状態が変わらないので、`on_state_change` で見分けられる |
| Windows の再生通知のスレッド | `PlaybackStateChanged` などは UI スレッドではなく再生パイプラインのスレッドで起きる。`DispatcherQueue` で UI スレッドへ渡し直してから通知している |
| macOS の `Label` | AppKit に `NSLabel` は無く、`NSTextField` を非編集で使うのが標準。`labelWithString:` はそのためのファクトリなので完全ネイティブ |
| macOS のメニューバー | `run` が最小限のメインメニュー (アプリ・編集) を用意する。macOS では ⌘C / ⌘V / ⌘A が**メインメニューのキー等価として配送される**ため、メニューが無いと `TextInput` で貼り付けができない。項目のターゲットは nil で、コピーや貼り付けを行うのは AppKit 自身。アプリが自分でメニューを作っていれば、そちらを尊重して何もしない |
| macOS の `Checkbox` | `NSButton` の `Switch` タイプが AppKit のチェックボックスそのもの。別クラスではない |
| WinUI 3 の `Button` / `Checkbox` | ラベルを `TextBlock` にして `Content` に入れている。XAML の標準的なやり方で、コントロール自体はネイティブ |
| Web の `Slider` | `<input type=range>` の既定 `step` は 1 なので、連続値になるよう `(max-min)/1000` を設定している。値のクランプはブラウザ自身が行う |
| すべての `Slider` / `ProgressBar` | 値のクランプはネイティブ側でも行われる (`NSSlider` は範囲外を丸める)。naui 側の `clamp` は二重の保険 |
| `Menu` という名前 | naui の `Menu` は**縦に並ぶナビゲーション一覧** (サイドバー) であって、ポップアップメニューではない。右クリックで出るほうは `PopupMenu` |
| `PopupMenu` のインデックス | 区切り線も 1 項目として数える。`set_items` に渡した並びの位置がそのまま返るので、渡した `Vec` を `get(index)` で引ける。区切り線と選べない項目は通知されない |
| macOS の `PopupMenu` の `open_at` | `NSMenu` が出ている間は AppKit がイベントを取り回すため、**閉じるまで呼び出しが戻らない**。自動テストで呼ばないのはこのため (選択の確認は `select` で行う) |
| macOS の `autoenablesItems` | `NSMenu` は既定で AppKit 自身が項目の有効・無効を決めてしまい、`enabled(false)` が無視される。naui は生成時に切っている |
| `List` の高さ | 中身に合わせた高さを持たない (macOS は `NSScrollView` そのものなので `Scroll` と同じ)。`set_sizing` で高さを指定すること |
| `List` の通知 | `on_select` は**選択が変わるたび**に、選ばれている行の並びで呼ばれる。複数選択では**空の並び**で呼ばれることもある。`set_selected` / `set_selection` / `clear_selection` は通知しない |
| `List` の列 | 1 列だけ。複数列のテーブルは未実装で、必要なら `native_table()` / `native_list_box()` / `native_select()` から直接触る |
| `ListItem::detail` | macOS / Windows は 2 行目に小さく出る。**Web は `detail` があると `<select>` から `role="listbox"` の合成へ切り替わる** (`<option>` はテキストしか持てないため) |
| Web の `List` の切り替わり | 判定は `set_items` のたび。1 行でも `detail` があれば合成になる。切り替わっても外側の要素は変わらないので、`set_sizing` の指定と親への追加はそのまま生きる |
| Web の `List::native_select` | 合成に切り替わっているときは `None` を返す。どちらの場合も外枠は `native_element()` から取れる |
| `List` の行の高さ | 行ごとに変わる (`detail` のある行だけ高い)。決めるのは AppKit / WinUI で、naui は上下の余白を制約にするだけ |
| Web の `List` の行数 | `<select>` は `size` を指定しないとドロップダウンになるため、行数に応じて 2..=8 の値を入れている。`set_sizing` で高さを指定すると CSS が優先される |
| `Dock` の配置 | 下端への固定は行わない。**置く場所はアプリの責務**で、縦スタックの最後に置き、手前に `Spacer` か `Fill` を使うと下端に寄る |
| `Fill` と `Auto` | どちらもネイティブのレイアウト機構への指示。naui 自身は位置も大きさも計算しない |
| グリッドのマスの中 | 縦は中央ぞろえ (`NSGridCellPlacement::Center` / `VerticalAlignment::Center` / `align-items: center`)。`Fill` を指定した子だけマスいっぱいに広がる |
| Windows の `Fill` の目印 | `HorizontalAlignment` は指定しなくても `Stretch` なので、プロパティだけでは「`Fill` と言われた」のか「既定のまま」なのかを区別できない。グリッドのマスの中でだけこの違いが要るため、`FrameworkElement.Tag` に目印を残している |
| `set_sizing` を呼ぶ順番 | macOS の交差軸 `Fill` とグリッドのマス内配置は、`append` / `attach` の**前**に指定しておく (AppKit では追加時に制約とセルの配置を張るため)。Web と Windows は後から変えても追従する |
| `Link` の遷移 | `href` が空でなければ、押したときにその環境の標準的な方法で開く (macOS は `NSWorkspace`、Windows は `HyperlinkButton` の `NavigateUri`、Web は `target="_blank"`)。Web で同じタブに遷移すると wasm のアプリごと破棄されるため、別タブに揃えている |
| Windows の `Spacer` / 主軸の `Fill` | `StackPanel` は子へ余りを配らないため、`Stack` の中では効かない。`Grid` の `Track::Fill` (XAML の `Star`) が同じ役割を果たす |
| macOS の `Track::Fill` | NSGridView に重みの概念が無いため、`Fill` 配置と hugging priority による近似。重みの違いは反映されない |
| `FilePicker` の通知 | `on_select` は**選ばれたときだけ**呼ばれる。取り消しの通知は、Web の `cancel` イベントが新しく環境がそろわないため持たない |
| `FilePicker` の絞り込み | 拡張子は `png` の形に正規化される (`.png` や `*.PNG` と書いても同じ)。macOS は `NSSavePanel` の拡張子指定、Windows は種類欄 (`COMDLG_FILTERSPEC`)、Web は `accept` 属性になる |
| macOS の `NSOpenPanel` を直接使う | `FilePicker::native_panel()` が、設定済みで**未表示**のパネルを返す。シート表示 (`beginSheetModalForWindow:`) や開始ディレクトリの指定はここから行う |
| 🟡 すべての `FilePicker` | 「押すとファイル選択が開くコントロール」は macOS にも WinUI 3 にも無い。その環境のネイティブなボタンと、その環境の標準のファイル選択ダイアログを組み合わせている。Web の `<input type=file>` は単体でボタンだが、**ボタンの文字列がブラウザ所有で差し替えられない**ため、`<button>` を表に出して押しを転送している |
| 🟡 Windows の `FilePicker` | `Windows.Storage.Pickers` は `winio-winui3` 0.4.5 のバインディングに含まれていないため、Win32 側の `IFileOpenDialog` (Common Item Dialog) を使っている。エクスプローラーと同じダイアログで、未パッケージ実行でも開ける |
| Windows の `ProgressBar` | Windows App SDK 2.3.1 の未パッケージ実行では `ProgressBar` の既定テンプレート適用時にランタイムが終了するため、WinUI XAML の `Grid` と `Border` を組み合わせて同等の表示を構成している。表示幅は親に合わせて伸縮し、値の変更 API は維持している |

### 未対応のコンポーネント

汎用のダイアログ、保存ダイアログ、複数列のテーブル、
ラジオボタン、コンボボックス、複数行テキスト、ツールバー、ツリーなどはありません
(1 列のリストは `List`、ファイル / フォルダーの選択は `FilePicker`、
右クリックのメニューは `PopupMenu`、
画像・動画・音声は `Image` / `Video` / `Audio` があります)。
レイアウトはスタック・グリッド・スクロールで、絶対配置はありません。

> **注意:** Windows 列は、Windows App SDK 2.3.1 の実機で `cargo run -p gallery` を
> 実行し、基本ウィジェット・ナビゲーション系 7 種・`Grid` / `Scroll` / `Spacer` /
> `set_sizing`・`FilePicker` のファイル / フォルダー選択・`Image` / `Video` / `Audio` の
> 読み込みと再生・動画表示のリサイズまで確認済みです。
> `ProgressBar` だけは上記の理由により、WinUI XAML 要素を組み合わせた実装です。

---

## クレート構成

```
crates/
  naui-core     … 共通の値型 (Error / Settings / Orientation / Align / Padding)
  naui-macos    … AppKit バックエンド (objc2)
  naui-web      … DOM バックエンド (web-sys)
  naui-windows  … WinUI 3 バックエンド (winio-winui3)
  naui-gtk      … GTK4 バックエンドの骨組み (未実装)
  naui          … ターゲットに応じてバックエンドを選ぶファサード
examples/
  counter       … 最小サンプル
  gallery       … 全ウィジェットのデモ (ネイティブ / Web 共通コード)
```

バックエンドは別クレートなので、API のずれは型検査でしか捕まりません。
そのため `crates/naui/src/lib.rs` に **`__api_contract`** という関数を置き、
公開 API を一通り呼んでいます。どのターゲットでもコンパイルされるので、
バックエンド間でシグネチャが食い違うとビルドが壊れます
(実際、この仕組みが Windows 側の実装漏れを 1 件検出しました)。

### 依存

| バックエンド | 依存 |
| --- | --- |
| macOS | `objc2`, `objc2-app-kit`, `objc2-foundation`, `objc2-av-kit`, `objc2-av-foundation`, `objc2-core-media`, `block2` (メディア) |
| Web | `wasm-bindgen`, `web-sys` |
| Windows | `winio-winui3` (WinUI 3 バインディング), `windows`, `windows-core` |
| Linux | (未実装。実装時に `gtk4` / `libadwaita`) |

ターゲット別の依存として宣言してあるので、macOS ビルドで GTK4 や
Windows のバインディングが引き込まれることはありません。

---

## ビルドと実行

### ネイティブ

```sh
cargo run -p counter
```

```sh
cargo run -p gallery
```

### Web (wasm)

```sh
cargo install wasm-bindgen-cli --version 0.2.127   # Cargo.lock と同じ版
cd examples/gallery/web
./build.sh
python3 -m http.server 8080
# → http://localhost:8080/
```

DOM がテキストを描くため、フォントの埋め込みなどは不要です。

ブラウザから呼ばれる入口 (`#[wasm_bindgen(start)]`) は `naui::entry!` が作るので、
アプリ側には wasm-bindgen への依存も `cfg` も要りません。

```rust,ignore
naui::entry!(Settings::new("naui gallery"), build); // pub fn start() ができる
```

### Windows

Windows バックエンドは WinUI 3 / Windows App SDK 2.x を使用します。現在の安定版
である Windows App SDK 2.3.1 をインストールした Windows x64 環境で動作確認済みです。
実行時には Windows App SDK のフレームワークランタイムが必要です。`cargo run` は
インストール済みの Windows App SDK 2.x ランタイムを動的依存関係として追加します。

Windows App SDK の[安定版リリース情報](https://github.com/microsoft/WindowsAppSDK/releases)
も参照してください。

```sh
cargo run -p gallery
```

---

## テスト

```sh
cargo test --workspace
```

バックエンドは別クレートなので、4 ターゲット分のコンパイルも確認します
(macOS からでも、ターゲットを追加すれば `cargo check` は通ります)。

```sh
cargo check --target x86_64-pc-windows-msvc -p naui
```

```sh
cargo check --target wasm32-unknown-unknown -p naui
```

```sh
cargo check --target x86_64-unknown-linux-gnu -p naui
```

- `naui-core`: 設定・エラー整形の単体テスト
- `naui-macos`: **AppKit の実コントロールに対する 51 件の統合テスト**
  - `performClick` でネイティブのクリックを発生させ、Rust のクロージャに届くこと
  - チェックボックスのネイティブ状態が反転し、変更後の値が通知されること
  - 日本語を含む文字列が NSTextField と往復すること
  - NSSlider が範囲でクランプすること (naui ではなく AppKit の挙動)
  - ハンドルを捨てた後もコンテナ経由でコールバックが生きていること
  - NSWindow を生成・設定・クローズしても二重解放しないこと
  - `NSSegmentedControl` の選択が往復し、`set_selected` は通知しないこと
  - `NSTabView` がタブの中身を保持し、切り替えを 1 回だけ通知すること
  - メニューの縦一覧で、押し込まれるボタンが常に 1 つだけであること
  - リストの選択が NSTableView と往復し、`set_selected` は通知しないこと
  - リストの複数選択が昇順にそろい、選択が 0 件にもなること
  - リストが選べない行を飛ばし、AppKit にも「選べない」と伝えていること
  - リストの行が NSTableView のビューとして作られ、日本語がそのまま出ること
  - NSTableView 側で選択を変えても、デリゲート経由でクロージャへ届くこと
  - 通知の中から行を差し替えても、AppKit の再入で壊れないこと
  - 行の文字が、選択の帯とずれないよう行の縦中央にそろうこと
  - `detail` を付けた行だけが 2 行になり、そのぶん高くなること
  - パンくずが末尾を現在地にし、階層を差し替えても追従すること
  - ページ送りが先頭・末尾で止まること
  - リンクのネイティブクリックがクロージャへ届くこと
  - 大きさの指定が NSLayoutConstraint になり、AppKit の計算結果に出ること
  - 指定し直しても制約が積み上がらず、AppKit 自身の制約を壊さないこと
  - 交差軸の `Fill` が、余白を除いた親の幅に追従すること
  - `Spacer` が余りを吸い、後続の子が下端へ寄ること
  - NSGridView が行と列を自分で増やし、固定幅の列が効くこと
  - NSScrollView が中身を保持し、コールバックが生き続けること
  - 画像が実ファイルから NSImage として読み込まれ、収め方が imageScaling になること
  - **実ファイルを最後まで再生し、`Playing` → `Ended` が届くこと**
  - 繰り返し再生では末尾で止まらないこと
  - 再生位置が定期的にクロージャへ届き、先頭へ戻らないこと
  - 音量と消音が AVPlayer と往復し、範囲外が丸められること
  - KVO と定期観測を張ったままハンドルを捨てても異常終了しないこと
  - ポップアップメニューの項目・区切り線・選べない項目が、そのまま `NSMenu` の中身になること
  - 選択が**区切り線を含めた位置**で届き、区切り線と選べない項目では届かないこと
  - 取り付けたウィジェットのビューが、そのメニューを持つようになること

AppKit はメインスレッドを要求しますが、Rust の標準テストハーネスは
各テストを別スレッドで走らせます (`--test-threads=1` でも同じ)。
そのため `harness = false` にして、自前のランナーをメインスレッドで回しています。

---

## 既知の制限

- **Linux が未実装。** 上記のとおり。
- **Windows App SDK の実行環境が必要。** Windows バックエンドは Windows App SDK 2.x の
  フレームワークランタイムを必要とし、現在は2.3.1で実機確認しています。
- **Windows の `Tabs` は `TabView` を使用しません。** Windows App SDK 2.3.1 の
  未パッケージ実行では `TabView` の既定テンプレートがランタイム終了を起こすため、
  `Grid` と `ToggleButton` で同じ選択 API を構成しています。
- **Enter で確定するコールバック (`on_submit`) がありません。**
  `winio-winui3` がキーボードイベント (`KeyDown` / `KeyEventHandler`) を
  バインドしていないため、Windows で実装できませんでした。
  「共通 API は全バックエンドの共通部分」という方針を優先して、
  macOS / Web からも外してあります。必要な場合はネイティブへの脱出口を使ってください。
- **コンポーネントは 24 種類のみ** (画面に並ぶウィジェット 23 種と、
  並ばない `PopupMenu` 1 種)**。** 基本 8 種 (`Window` / `Stack` / `Label` / `Button` /
  `Checkbox` / `TextInput` / `Slider` / `ProgressBar`)、レイアウト 3 種
  (`Grid` / `Scroll` / `Spacer`)、ナビゲーション 7 種
  (`Tabs` / `Navbar` / `Dock` / `Menu` / `Breadcrumbs` / `Pagination` / `Link`)、
  リスト 1 種 (`List`)、ポップアップメニュー 1 種 (`PopupMenu`)、
  ファイル選択 1 種 (`FilePicker`)、
  メディア 3 種 (`Image` / `Video` / `Audio`) です。
  汎用のダイアログ、保存ダイアログ、テーブル、
  複数行テキストなどは未実装です。
- **`List` は 1 列だけです。** 複数列のテーブルはありません。また `List` は
  中身に合わせた高さを持たないため、`set_sizing` で高さを指定してください。
- **`List` の行に置けるのは文字 (`label` と `detail`) だけです。** 任意の
  ウィジェットや画像のアイコンは置けません。macOS の `NSTableView` と WinUI の
  `ListBox` は行に任意のビューを入れられますが、Web で同じことをするには
  `role="listbox"` の合成でも足りず (ARIA の `option` は操作できる子要素を
  持てません)、3 環境で形がそろわないためです。絵文字や記号ならラベルに
  入れればどの環境でも出ます。
- **Web で `detail` を使うと、`List` は合成になります。** 文字だけの行なら
  `<select>` のままですが、`detail` があると `<ul role="listbox">` に切り替わり、
  複数選択のキーボード操作は naui の実装になります。
- **Windows の `Stack` では主軸の `Fill` と `Spacer` が効きません。** `StackPanel` が
  子へ余りを配らないためです。`Grid` の `Track::Fill` を使ってください。
- **macOS の `Track::Fill` は重みを無視します。** NSGridView に重みの概念が無く、
  `Fill` 配置と hugging priority による近似だからです。
- **メディアのデコードと再生は行いません。** 対応している形式は、その環境の
  ツールキット (AVFoundation / ブラウザ / Windows.Media.Playback) が決めます。
- **macOS の `Image` はリモート URL を同期的に読み込みます。** 読み終わるまで
  UI が止まるため、ローカルのファイルを渡してください。
- **Windows の標準再生バーは無効です。** Windows App SDK の一部環境では
  `MediaTransportControls` の表示時に `0xc000027b` で終了するため、`Video` / `Audio`
  の標準バーは使わず、アプリ側で `play()` / `pause()` などの操作欄を用意してください。
- **Web のメディアはブラウザ標準のデコーダーに依存します。** 自動再生が拒否された場合は、ユーザー操作から `play()` を呼び出してください。
- **絶対配置はありません。** 位置は `Grid` のマス目・`Align`・`Spacer` で決めます。
- **`set_sizing` はコンテナの中の子に効きます。** ウィンドウ直下のルートは
  ウィンドウいっぱいに広がるため、そこでの指定は意味を持ちません。
- **macOS では交差軸の `Fill` とグリッドのマス内配置を、コンテナへ入れる
  「前」に指定する必要があります。** AppKit では制約とセルの配置を追加時に
  張るためです (Web と Windows は後から変えても追従します)。
- **ウィンドウを閉じるイベントを購読できません。**

---

## ライセンス

MIT OR Apache-2.0
