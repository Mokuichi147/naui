# miui

**各 OS のネイティブ UI を、1 つの API から扱う軽量 GUI ツールキット (Rust)。**

miui は自前で描画しません。`ui.button("押す")` が返すのは **本物の OS のボタン**であり、
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
| **macOS** | ✅ 動作 | アプリを実行して確認。AppKit の実コントロールに対する自動テスト 21 件 |
| **Web (wasm)** | ✅ 動作 | ブラウザで実行し、全ウィジェットを DOM イベントで操作して確認 (ナビゲーション系も、ナビバー・タブ・メニュー・ページ送り・ドックのクリックがコールバックまで届くことを確認)。グリッド・スクロール・スペーサーは実際の描画位置を測って確認 |
| **Windows** | ✅ 動作 (レイアウトは未確認) | Windows App SDK 2.3.1 の実機で `cargo run -p gallery` を実行し、基本ウィジェットとナビゲーション系 7 種の起動を確認済み。`Grid` / `Scroll` / `Spacer` / `set_sizing` は**コンパイル確認のみ** |
| **Linux** | ❌ 未実装 | API の形だけ定義した骨組み。呼ぶとエラーを返します |

Linux が未実装なのは、GTK4 バックエンドがまだ骨組みの段階だからです。
Windows は Windows App SDK 2.3.1 ランタイムを備えた x64 環境で、
`Tabs` / `Navbar` / `Dock` / `Menu` / `Breadcrumbs` / `Pagination` / `Link` を含む
`gallery` の起動を確認済みです。
詳細は [`crates/miui-gtk`](crates/miui-gtk/src/lib.rs) のドキュメントを参照してください。

---

## 使い方

UI は `run` に渡すコールバックの中で組み立てます。
WinUI 3 が `Application::Start` より前のコントロール生成を許さないため、
4 バックエンドで同じ形にそろえてあります。

```rust
use std::cell::Cell;
use std::rc::Rc;
use miui::{Orientation, Padding, Settings};

fn main() -> miui::Result<()> {
    miui::run(Settings::new("counter"), |ui| {
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
use miui::{Settings, Theme};

let settings = Settings::new("counter").theme(Theme::System);
miui::run(settings, |ui| {
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

| miui | Windows (WinUI 3) | macOS (AppKit) | Web (DOM) | Linux (GTK4) |
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

### 配置とサイズ

どのウィジェットも `set_sizing` で大きさを指定できます。計算するのは
ネイティブのレイアウト機構 (Auto Layout / XAML のレイアウトパス / CSS) で、
miui は制約やプロパティを設定するだけです。

```rust
use miui::{GridCell, Length, ScrollPolicy, Sizing, Track};

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

#### Spacer

中身を持たず、余った空間だけを受け取るウィジェットです。縦スタックの途中に
置くと、後ろの要素が下端へ寄ります (`Dock` を画面下端に置く用途)。

```rust
root.append(&ui.spacer()?);
root.append(&dock);   // 下端に寄る
```

Windows の `StackPanel` は余りを配らないため、`Spacer` と主軸の `Fill` は
`Stack` の中では効きません。`Grid` の行を `Track::Fill` にしてください。

### ナビゲーション

| miui | Windows (WinUI 3) | macOS (AppKit) | Web (DOM) | Linux (GTK4) |
| --- | --- | --- | --- | --- |
| `Tabs` | 🟡 `StackPanel` + `ToggleButton` (TabViewの未パッケージ起動回避) | ✅ `NSTabView` + `NSTabViewItem` | 🟡 `role="tablist"` + `<button role=tab>` + `hidden` | ❌ |
| `Navbar` | 🟡 `TextBlock` + `ToggleButton` の横並び | 🟡 `NSTextField` + `NSSegmentedControl` | 🟡 `<nav>` + `<strong>` + `<button>` | ❌ |
| `Dock` | 🟡 `ToggleButton` の横並び | ✅ `NSSegmentedControl` (等幅) | 🟡 `<nav>` + `<button>` (等幅) | ❌ |
| `Menu` | 🟡 `ToggleButton` の縦並び | 🟡 `NSButton` (AccessoryBar) の縦並び | 🟡 `<nav><ul><li><button>` | ❌ |
| `Breadcrumbs` | 🟡 `HyperlinkButton` + 区切りの `TextBlock` | ✅ `NSPathControl` + `NSPathControlItem` | 🟡 `<nav><ol><li><a href>` | ❌ |
| `Pagination` | 🟡 `Button` + `ToggleButton` | 🟡 `NSButton` + `NSSegmentedControl` | 🟡 `<nav>` + `<button>` | ❌ |
| `Link` | ✅ `HyperlinkButton` | 🟡 `NSButton` (枠なし・リンク色) + `NSWorkspace` | ✅ `<a href>` | ❌ |

7 種類とも **同じ形の API** を持ちます。項目は `NavItem` の並びで渡し、
選ばれたものはインデックスで返ります。

```rust
let navbar = ui.navbar("miui")?;
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

### 🟡 / 🔴 の内訳

| 箇所 | 内容 |
| --- | --- |
| 🔴 Web の `Window` | ブラウザにはページ内ウィンドウの概念が無い。`<body>` 直下の `<div>` で代用し、タイトルは `document.title` に反映している。`show()` / `close()` は `display` の切り替え、`set_size()` は `max-width` / `min-height` の指定であり、**OS のウィンドウ操作ではない** |
| 🟡 Web の `Stack` | HTML に「スタック」というコントロールは存在しない。ただし CSS Flexbox はブラウザ自身のレイアウト機構なので、独自のレイアウト計算はしていない (`display:flex` + `flex-direction` + `gap` + `padding`) |
| 🟡 Web の `Checkbox` | `<input type=checkbox>` 自体はネイティブだが、ラベル文字列を持たないため `<label>` で `<input>` と `<span>` を包んでいる |
| 🟡 Web のナビゲーション全般 | ブラウザに「タブ」「ナビバー」というコントロールは無い。`<nav>` / `<ol>` / `<a>` / `<button>` と WAI-ARIA のロール (`tablist` / `tab` / `tabpanel` / `aria-current`) で意味づけし、隠すのは `hidden` 属性に任せている。CSS は Flexbox のレイアウトと、選択中を示す `font-weight: bold` だけ |
| 🟡 macOS の `Navbar` | `NSSegmentedControl` はネイティブだが、見出しを持てないため `NSTextField` と `NSStackView` で横に並べている |
| 🟡 macOS の `Menu` | AppKit の `NSMenu` はポップアップ用。サイドバー相当の縦一覧は `NSButton` (AccessoryBar・PushOnPushOff) を `NSStackView` に並べて作っている |
| 🟡 macOS の `Link` | AppKit にリンク専用のコントロールは無い。枠なしの `NSButton` を `NSColor::linkColor` にし、`href` は `NSWorkspace` で開いている |
| 🟡 すべての `Pagination` | ページ送りに相当するネイティブコントロールはどの環境にも無い。前へ / 次へのボタンとページ番号を、その環境のネイティブなボタンで並べている |
| 🟡 Windows の `Navbar` / `Dock` / `Menu` | `NavigationView` は `winio-winui3` 0.4.5 のバインディングに含まれていないため、WinUI 標準の `ToggleButton` を `StackPanel` に並べ、選択状態を `IsChecked` で表している |
| 🟡 Windows の `Breadcrumbs` | `BreadcrumbBar` は `winio-winui3` 0.4.5 のバインディングに含まれていないため、標準の `HyperlinkButton` と区切り文字を `StackPanel` に並べている |

### 補足 (誤解しやすい箇所)

| 箇所 | 説明 |
| --- | --- |
| macOS の `Label` | AppKit に `NSLabel` は無く、`NSTextField` を非編集で使うのが標準。`labelWithString:` はそのためのファクトリなので完全ネイティブ |
| macOS の `Checkbox` | `NSButton` の `Switch` タイプが AppKit のチェックボックスそのもの。別クラスではない |
| WinUI 3 の `Button` / `Checkbox` | ラベルを `TextBlock` にして `Content` に入れている。XAML の標準的なやり方で、コントロール自体はネイティブ |
| Web の `Slider` | `<input type=range>` の既定 `step` は 1 なので、連続値になるよう `(max-min)/1000` を設定している。値のクランプはブラウザ自身が行う |
| すべての `Slider` / `ProgressBar` | 値のクランプはネイティブ側でも行われる (`NSSlider` は範囲外を丸める)。miui 側の `clamp` は二重の保険 |
| `Menu` という名前 | miui の `Menu` は**縦に並ぶナビゲーション一覧** (サイドバー) であって、ポップアップメニューではない。`NSMenu` / `MenuFlyout` に相当するものは未実装 |
| `Dock` の配置 | 下端への固定は行わない。**置く場所はアプリの責務**で、縦スタックの最後に置き、手前に `Spacer` か `Fill` を使うと下端に寄る |
| `Fill` と `Auto` | どちらもネイティブのレイアウト機構への指示。miui 自身は位置も大きさも計算しない |
| `set_sizing` を呼ぶ順番 | macOS の交差軸 `Fill` とグリッドのマス内配置は、`append` / `attach` の**前**に指定しておく (AppKit では追加時に制約とセルの配置を張るため)。Web と Windows は後から変えても追従する |
| `Link` の遷移 | `href` が空でなければ、押したときにその環境の標準的な方法で開く (macOS は `NSWorkspace`、Windows は `HyperlinkButton` の `NavigateUri`、Web は `target="_blank"`)。Web で同じタブに遷移すると wasm のアプリごと破棄されるため、別タブに揃えている |
| Windows の `Spacer` / 主軸の `Fill` | `StackPanel` は子へ余りを配らないため、`Stack` の中では効かない。`Grid` の `Track::Fill` (XAML の `Star`) が同じ役割を果たす |
| macOS の `Track::Fill` | NSGridView に重みの概念が無いため、`Fill` 配置と hugging priority による近似。重みの違いは反映されない |
| Windows の `ProgressBar` | Windows App SDK 2.3.1 の未パッケージ実行では `ProgressBar` の既定テンプレート適用時にランタイムが終了するため、WinUI XAML の `Grid` と `Border` を組み合わせて同等の表示を構成している。値の変更 API は維持している |

### 未対応のコンポーネント

ポップアップメニュー、ダイアログ、リスト / テーブル、ラジオボタン、コンボボックス、
複数行テキスト、ツールバー、ツリー、画像表示などはありません。
レイアウトはスタック・グリッド・スクロールで、絶対配置はありません。

> **注意:** Windows 列のうち、基本ウィジェットは Windows App SDK 2.3.1 の実機で
> `cargo run -p gallery` による起動確認済みです。ナビゲーション系 7 種も
> 実機での起動を確認しています。
> `ProgressBar` だけは上記の理由により、WinUI XAML 要素を組み合わせた実装です。
> **`Grid` / `Scroll` / `Spacer` と `set_sizing` の Windows 実装は、
> `cargo check --target x86_64-pc-windows-msvc` を通しただけで実機未確認です。**

---

## クレート構成

```
crates/
  miui-core     … 共通の値型 (Error / Settings / Orientation / Align / Padding)
  miui-macos    … AppKit バックエンド (objc2)
  miui-web      … DOM バックエンド (web-sys)
  miui-windows  … WinUI 3 バックエンド (winio-winui3)
  miui-gtk      … GTK4 バックエンドの骨組み (未実装)
  miui          … ターゲットに応じてバックエンドを選ぶファサード
examples/
  counter       … 最小サンプル
  gallery       … 全ウィジェットのデモ (ネイティブ / Web 共通コード)
```

バックエンドは別クレートなので、API のずれは型検査でしか捕まりません。
そのため `crates/miui/src/lib.rs` に **`__api_contract`** という関数を置き、
公開 API を一通り呼んでいます。どのターゲットでもコンパイルされるので、
バックエンド間でシグネチャが食い違うとビルドが壊れます
(実際、この仕組みが Windows 側の実装漏れを 1 件検出しました)。

### 依存

| バックエンド | 依存 |
| --- | --- |
| macOS | `objc2`, `objc2-app-kit`, `objc2-foundation` |
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
cargo check --target x86_64-pc-windows-msvc -p miui
```

```sh
cargo check --target wasm32-unknown-unknown -p miui
```

```sh
cargo check --target x86_64-unknown-linux-gnu -p miui
```

- `miui-core`: 設定・エラー整形の単体テスト
- `miui-macos`: **AppKit の実コントロールに対する 21 件の統合テスト**
  - `performClick` でネイティブのクリックを発生させ、Rust のクロージャに届くこと
  - チェックボックスのネイティブ状態が反転し、変更後の値が通知されること
  - 日本語を含む文字列が NSTextField と往復すること
  - NSSlider が範囲でクランプすること (miui ではなく AppKit の挙動)
  - ハンドルを捨てた後もコンテナ経由でコールバックが生きていること
  - NSWindow を生成・設定・クローズしても二重解放しないこと
  - `NSSegmentedControl` の選択が往復し、`set_selected` は通知しないこと
  - `NSTabView` がタブの中身を保持し、切り替えを 1 回だけ通知すること
  - メニューの縦一覧で、押し込まれるボタンが常に 1 つだけであること
  - パンくずが末尾を現在地にし、階層を差し替えても追従すること
  - ページ送りが先頭・末尾で止まること
  - リンクのネイティブクリックがクロージャへ届くこと
  - 大きさの指定が NSLayoutConstraint になり、AppKit の計算結果に出ること
  - 指定し直しても制約が積み上がらず、AppKit 自身の制約を壊さないこと
  - 交差軸の `Fill` が、余白を除いた親の幅に追従すること
  - `Spacer` が余りを吸い、後続の子が下端へ寄ること
  - NSGridView が行と列を自分で増やし、固定幅の列が効くこと
  - NSScrollView が中身を保持し、コールバックが生き続けること

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
  `StackPanel` と `ToggleButton` で同じ選択 API を構成しています。
- **Enter で確定するコールバック (`on_submit`) がありません。**
  `winio-winui3` がキーボードイベント (`KeyDown` / `KeyEventHandler`) を
  バインドしていないため、Windows で実装できませんでした。
  「共通 API は全バックエンドの共通部分」という方針を優先して、
  macOS / Web からも外してあります。必要な場合はネイティブへの脱出口を使ってください。
- **ウィジェットは 18 種類のみ。** 基本 8 種 (`Window` / `Stack` / `Label` / `Button` /
  `Checkbox` / `TextInput` / `Slider` / `ProgressBar`)、レイアウト 3 種
  (`Grid` / `Scroll` / `Spacer`)、ナビゲーション 7 種
  (`Tabs` / `Navbar` / `Dock` / `Menu` / `Breadcrumbs` / `Pagination` / `Link`) です。
  ポップアップメニュー、ダイアログ、リスト、複数行テキストなどは未実装です。
- **Windows の `Stack` では主軸の `Fill` と `Spacer` が効きません。** `StackPanel` が
  子へ余りを配らないためです。`Grid` の `Track::Fill` を使ってください。
- **macOS の `Track::Fill` は重みを無視します。** NSGridView に重みの概念が無く、
  `Fill` 配置と hugging priority による近似だからです。
- **Windows の `Grid` / `Scroll` / `Spacer` は実機未確認です。** コンパイル確認のみ。
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
