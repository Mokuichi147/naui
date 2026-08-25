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

## 目次

- [クイックスタート](#クイックスタート)
- [対応状況](#対応状況)
- [基本的な使い方](#基本的な使い方)
- [ウィジェット](#ウィジェット)
- [Web 版の実行](#web-版の実行)
- [開発](#開発)
- [既知の制限](#既知の制限)

## クイックスタート

### 必要なもの

- Rust 1.82 以降
- Linux: GTK4 と libadwaita の開発用ライブラリ
- Windows: Windows App SDK 2.x のフレームワークランタイム

Ubuntu 24.04 では、次のコマンドで Linux 向けの依存パッケージを導入できます。

```sh
sudo apt install libgtk-4-dev libadwaita-1-dev build-essential pkg-config
```

Windows App SDK は 2.3.1 で動作を確認しています。

### サンプルを実行

最小構成のカウンターを起動します。

```sh
git clone https://github.com/mokuichi147/naui.git
cd naui
cargo run -p counter
```

全ウィジェットを確認するには Gallery を起動します。画面は基本、入力、一覧、
ナビゲーション、レイアウト、ファイル、メディア、ダイアログに分かれています。

```sh
cargo run -p gallery
```

## 対応状況

| 環境 | 状態 | 確認内容 |
| --- | --- | --- |
| macOS | ✅ 動作確認済み | AppKit の実コントロールを使った統合テストと Gallery の実行 (色ピッカー・時刻ピッカーを含む) |
| Linux | ✅ 動作確認済み | Ubuntu 24.04、GTK 4.14、libadwaita 1.5、Wayland で Gallery と統合テストを実行 (スイッチ・色ピッカー・時刻ピッカーを含む) |
| Web | ✅ 動作確認済み | ブラウザ上で DOM の描画、入力、ナビゲーション、ファイル選択、メディア、ダイアログ、トースト、折りたたみ、スイッチ、時刻ピッカー、色ピッカー、テーブルを操作 |
| Windows | ✅ 動作確認済み | Windows App SDK 2.3.1 の x64 実機で全ウィジェットとナビゲーションを操作 (スイッチ・色ピッカー・時刻ピッカーを含む) |

実装済みで `cargo check` は通るものの、実機で未確認の範囲があります。

<details>
<summary>未確認の範囲を表示</summary>

- Web: `PopupMenu` のブラウザ実行と、埋め込みブラウザで配送されなかった
  `Dialog` の Esc 操作
- `FileSaver`: macOS のみ実機と自動テストで確認しています。Windows・Linux・Web
  での実行は未確認です。
- Windows / Linux / Web: `Tree` の実機・ブラウザでの実行 (macOS では
  統合テストと Gallery で確認済み。Linux 向けの統合テストは用意してあります)
- Windows / Linux: `DatePicker` の実機での実行 (macOS は統合テストと Gallery、
  Web はブラウザで確認済み。Linux 向けの統合テストは用意してあります)
- Windows / Linux: `NumberInput` と `PasswordInput` の実機での実行 (macOS は
  統合テストと Gallery、Web はブラウザで値の丸め・範囲・通知まで確認済み。
  Linux 向けの統合テストは用意してあります)
- `Table`: Windows・Linux の実機での実行。macOS は統合テストと Gallery、
  Web はブラウザで列幅・文字揃え・選択・キーボード操作・列の差し替え・
  見出しからの並べ替えまで確認済みです。Linux 向けの統合テストは
  用意してあります

</details>

プラットフォーム固有の注意点は[既知の制限](#既知の制限)を参照してください。

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

レイアウトには次の 5 種類を使います。

| API | 用途 |
| --- | --- |
| `Stack` | 子を縦または横へ並べる |
| `Grid` | 行と列を指定して配置する |
| `Scroll` | はみ出した内容をスクロールする |
| `Spacer` | 親の余った空間を受け取る |
| `Expander` | 見出しを押したときだけ中身を見せる |

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

`List`、`Tree`、`Scroll`、`TextArea` は内容から高さを決めないため、通常は
`set_sizing` で高さを指定します。

### 機能別の補足

全ウィジェットの基本的な使用例は
[`examples/gallery`](examples/gallery/src/lib.rs) にあります。ここでは、共通 API
だけでは意図や動作が分かりにくい機能を補足します。

<details>
<summary><strong>機能別の補足を表示</strong></summary>

#### 折りたたみ

ふだんは隠しておき、見出しを押したときだけ見せたいものは `Expander` へ入れます。
中身は 1 つなので、複数並べたいときは `Stack` などのコンテナごと入れます。

```rust
let details = ui.expander("詳細設定")?;
let body = ui.stack(Orientation::Vertical)?;
body.append(&ui.checkbox("バックアップを作る")?);
details.set_child(&body);
details.set_expanded(true); // 通知せずに開く
details.on_toggle(|expanded| println!("開いている: {expanded}"));
```

既定は閉じた状態で、たたんでいる間、中身はレイアウトから外れます (場所を空けません)。
`set_expanded` はプログラムからの操作なので `on_toggle` を呼びません
(`Checkbox::set_checked` と同じ決まりです)。

#### 数値とパスワードの入力

数を入れさせるには `NumberInput` を使います。値は `f64` で、下限・上限・刻み・
小数桁を指定できます。**既定は整数**(刻み 1、小数桁 0、範囲の制限なし)なので、
小数を扱うときは刻みと小数桁の両方を指定します。

```rust
let count = ui.number_input(1.0)?;
count.set_range(Some(1.0), Some(99.0)); // 1〜99 の外へは出られない
count.on_change(|value| println!("{value} 個"));

let rate = ui.number_input(0.5)?;
rate.set_decimals(2); // 0.50 のように 2 桁で見せる
rate.set_step(0.05); // 上下のボタンで動く量
```

値は**小数桁へ丸めてから範囲へ収めます**。範囲の外の値を `set_value` へ渡すと、
通知せずに端へ寄ります。打っている最中は表示を書き換えず、読める値になった時点で
`on_change` が呼ばれます。表示を値へそろえ直すのは確定したとき (Enter、欄を離れた
とき、増減のボタン) で、数として読めない文字列は確定時に元の値へ戻ります。
中身に合わせた幅を持たないので、幅は `set_sizing` で指定します。

パスワードは `PasswordInput` を使います。API は `TextInput` と同じで、違うのは
打った文字が伏せ字になることだけです。

```rust
let password = ui.password_input()?;
password.set_placeholder("パスワード");
password.on_change(|text| println!("{} 文字", text.chars().count()));
```

**伏せ字を外す切り替えは持ちません**。`NSSecureTextField` に無く、4 環境の共通
部分にならないためです。入力された文字列は `text()` で読めるので、扱いはアプリの
責任になります。

#### 入り切りの切り替え

入っているか切れているかの 2 択は、`Checkbox` か `Toggle` で切り替えます。
API はどちらも同じで、違うのは見せ方だけです。**印を付けるチェックボックスは
「同意する」のようにまとめて決めるもの、つまみを動かすスイッチはその場で効く
設定**に向きます (どちらを使うかは、その環境の作法に合わせてください)。

```rust
let backup = ui.toggle("バックアップを作る")?;
backup.set_on(true); // 通知せずに入れる
backup.on_toggle(|on| println!("バックアップ: {on}"));
```

`set_checked` と `set_on` はプログラムからの操作なので、`on_toggle` を呼びません。

`NSSwitch` と `GtkSwitch` は文字を持たないため、macOS と Linux ではラベルを
横へ添えています。Windows は `OnContent` / `OffContent` へ同じ文字を入れて、
入り切りで読みが変わらないようにしています。**切り替えの当たり判定はスイッチの
部分**で、Web だけは `<label>` で組む都合上、文字を押しても切り替わります。
Web は `switch` 属性でブラウザへ「スイッチとして描いて」と頼むだけで、naui が
見た目を作ることはしません。**この属性に対応しているのは Safari 17.4 以降だけ**
なので、Chrome や Firefox ではチェックボックスの見た目で出ます (値の扱い・通知・
読み上げ (`role="switch"`) は同じです)。

#### 選択入力

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

#### 日付と時刻

日付や時刻を選ばせるには `DatePicker` を使います。何を選ばせるかは生成時の
`DatePickerMode` で決め、値は `DateTime` (年月日と時分) でやり取りします。

```rust
use naui::{DatePickerMode, DateTime};

let start = ui.date_picker(DatePickerMode::Date)?;
start.set_value(DateTime::date(2026, 8, 22)); // 通知せずに値を入れる
start.set_range(Some(DateTime::date(2026, 1, 1)), None); // 下限だけ決める
start.on_change(|value| println!("{value} が選ばれました"));

let alarm = ui.date_picker(DatePickerMode::Time)?; // 時刻だけ
alarm.set_value(DateTime::time(7, 30));
```

作った直後の値は**その環境の現在日時 (ローカル時刻)** で、空の状態は持ちません
(`NSDatePicker` に「未選択」が無いためです)。**秒は持ちません**。秒まで選ばせる
コントロールが 4 環境の共通部分に無いためです。

`DatePickerMode::Date` は時刻を、`DatePickerMode::Time` は日付を**選ばせない
だけで、捨てはしません**。`set_value` で入れた側の値は `value()` にそのまま
残ります。

`set_value` は通知せず、暦として成り立たない値 (11 月 31 日など) はその月の端へ
丸めます。`set_range` の外へは出られず、範囲の比較には**選ばせている部分だけ**を
使います (時刻だけの表示なら日付を見ません)。

時刻だけを選ばせたいなら、次の `TimePicker` のほうが扱いやすいです。

#### 時刻の選択

時刻だけを選ばせるには `TimePicker` を使います。値は `Time` (時分) で
やり取りし、日付は持ちません。

```rust
use naui::Time;

let alarm = ui.time_picker()?;
alarm.set_value(Time::new(7, 30)); // 通知せずに値を入れる
alarm.set_range(Some(Time::new(6, 0)), Some(Time::new(9, 0)));
alarm.on_change(|value| println!("{value} が選ばれました")); // 07:30
```

作った直後の値は**その環境の現在時刻 (ローカル時刻)** で、空の状態は持ちません
(`DatePicker` と同じです)。**秒は持ちません**。12 時間制と 24 時間制のどちらで
出るかは、その環境のロケールに従います。

`set_value` は通知せず、時計として成り立たない値 (25 時 70 分など) は端へ
丸めます。繰り上がりはしないので、`Time::new(25, 70)` は翌日の 2 時 10 分では
なく 23 時 59 分です。`set_range` の外へは出られません。**日付をまたぐ範囲**
(22:00〜翌 06:00 など) は指定できません。`Time` に日付が無く、下限が上限より
後ろのときは上限が勝つためです。

#### 色の選択

色を選ばせるには `ColorPicker` を使います。値は `Color` (sRGB の 8 bit) で
やり取りし、色を選ぶ UI はその環境のものがそのまま開きます。

```rust
use naui::Color;

let accent = ui.color_picker()?;
accent.set_value(Color::rgb(0x33, 0x66, 0xff)); // 通知せずに値を入れる
accent.on_change(|value| println!("{value} が選ばれました")); // #3366ff
```

作った直後の値は黒 (`Color::BLACK`) です。**透明度は扱いません**。
`<input type="color">` が不透明な色しか返さないため、4 環境でそろう範囲に
合わせています。

`set_value` は通知せず、`pick` は利用者が選んだのと同じく `on_change` を
呼びます (`ComboBox` の `set_selected` と `select` と同じ決まりです)。

Windows の WinUI 3 `ColorPicker` は、スペクトラムとスライダーを縦に並べた
**大きな面**で、他の 3 環境の「色の見本を押すと選択の UI が開く」という形とは
並び方が違います。そこで WinUI 3 の作法どおり `Button` の `Flyout` へ入れ、
ボタンには選んだ色の見本を出しています。

macOS のカラーパネルはカタログ色 (`systemBlue` など) も返すので、成分を読む
前に sRGB へ変換しています。

#### テーブル

列見出しを持つ表を出すには `Table` を使います。列は `TableColumn`、行は
`TableRow` で、**行の識別はリストと同じくインデックス**です。選択の通知も
行のインデックスで返ります。

```rust
use naui::{Align, TableColumn, TableRow};

let table = ui.table()?;
table.set_columns(&[
    TableColumn::new("都市").sortable(true),                 // 見出しを押して並べ替え
    TableColumn::new("人口").width(120.0).align(Align::End), // 幅を固定して右寄せ
]);
table.set_rows(&TableRow::list([
    ["東京", "13,960,000"],
    ["大阪", "8,838,000"],
]));
table.on_select(|indices| println!("{indices:?} が選ばれました"));
```

選択のふるまいは `List` と同じで、`SelectionMode` で単一と複数を選べます。
`set_selection`、`set_selected`、`clear_selection` は通知せず、`select`、
`select_many` は利用者の操作と同じく通知します。`TableRow::enabled(false)`
にした行は選べません。

`TableColumn::width` を指定しない列だけで、余った幅を分け合います。セルに
置けるのは文字だけで、任意のウィジェットは置けません。**列の幅をドラッグで
変えられるのは macOS だけ**で (`NSTableView` が持つ機能)、他の環境には対応する
標準コントロールがありません。

`TableColumn::sortable(true)` にした列は、見出しを押して並べ替えられます。
押すたびに向きが反転し、指標 (macOS は `NSTableView` の ▲▼、ほかは見出しに
付く矢印と `aria-sort`) は naui が出します。**並べ替えそのものは naui では
行いません。** セルは文字列なので、それが数値なのか日付なのかを知っているのは
アプリだけだからです。通知を受けたら、アプリが自分のデータを並べ替えて
`set_rows` で渡し直します。

```rust
use naui::SortOrder;

table.on_sort(move |column, order| {
    rows.sort_by(|a, b| {
        let ordering = a.cell(column).cmp(b.cell(column));
        match order {
            SortOrder::Ascending => ordering,
            SortOrder::Descending => ordering.reverse(),
        }
    });
    table.set_rows(&rows); // 選択は外れるので、必要なら選び直す
});
```

`set_sort` で通知せずに指標だけを動かせます (起動時の既定の並び順を見せる
とき)。`sort()` でいまの指定を読めます。

行数が多いときは、リストと同じく `set_sizing` で高さを決めておきます
(中身の高さでは決まりません)。

#### ツリー

入れ子の項目を開閉して 1 つ選ぶには `Tree` を使います。項目は**根からの子
インデックスの並び (パス)** で指し、`[0, 2]` は「1 番目の根の 3 番目の子」、
空のパスは「選択なし」を表します。

```rust
use naui::TreeItem;

let tree = ui.tree()?;
tree.set_items(&[
    TreeItem::new("src")
        .expanded(true) // 最初から開いた状態で出す
        .children([TreeItem::new("main.rs"), TreeItem::new("lib.rs")]),
    TreeItem::new("docs").child(TreeItem::new("guide.md").detail("12 KB")),
]);
tree.on_select(|path| println!("{path:?} が選ばれました"));
tree.on_expand(|path, expanded| println!("{path:?} は {expanded}"));
tree.select(&[0, 1]); // 閉じた枝の中でも、祖先ごと開いて選ばれる
```

`set_selected`、`clear_selection`、`set_expanded`、`expand_all`、`collapse_all`
は通知せず、`select`、`expand`、`collapse` は利用者の操作と同じく通知します。
`TreeItem::enabled(false)` にした枝は、その子孫もまとめて選べなくなります。

#### ファイルの保存

`FileSaver` は、押すと環境標準の保存ダイアログが開くボタンです。**保存先の
パスを返すのではなく、渡しておいた内容を書き出します**。ブラウザには保存先の
パスという概念が無く、パスを返す API では Web で何もできないためです。

```rust
let saver = ui.file_saver("保存")?;
saver.set_file_name("メモ"); // 拡張子は絞り込みから補われる
saver.set_filters(&[FileFilter::new("テキスト", ["txt"])]);
saver.set_contents("こんにちは".as_bytes());
saver.on_save(|entry| println!("{} へ保存しました", entry.name()));
saver.on_error(|error| eprintln!("{error}"));
```

`set_contents` のバイト列が、選ばれた場所へそのまま書かれます。書き出しに
成功すると `on_save` に書き出し先が届き、取り消したときは何も呼ばれません。
書き込みに失敗したときだけ `on_error` が呼ばれます。ボタンを押した時点の
内容を書き出すため、内容が変わるたびに `set_contents` を呼び直します。

#### トースト

済んだことを知らせるだけで、操作を止めたくないときは `Toast` を使います。
画面の下端に短く出て、**何秒かで自分から消えます**。`Dialog` と同じく
レイアウトには置かず、`show()` で出します。

```rust
let toast = ui.toast("保存しました")?;
toast.set_action("元に戻す"); // 任意。空文字列で外す
toast.set_timeout(3.0); // 秒。0 なら自分では消えない
toast.on_action(|| println!("元に戻す"));
toast.on_dismiss(|| println!("消えました"));
toast.show();
```

**同時に出るのは 1 つ**で、新しく出したものが前のものを置き換えます。
`dismiss()` で消したときと、置き換えられたときは `on_dismiss` を呼びません
(アプリ自身の操作は通知しない、という `Dialog::close` と同じ決まりです)。

ネイティブのトーストがあるのは Linux (`AdwToast`) だけで、残る 3 環境では
naui が同じ形に組み立てます。OS の通知センターへ出す通知は、アプリの外へ
出る別の仕組みなので扱いません。時間の刻みは Linux だけ秒なので、1 秒未満の
指定は 1 秒になります。

#### ツールバー

よく使う操作をウィンドウの上端に並べるには `Toolbar` を使います。ほかの
ウィジェットと違い**レイアウトには置かず**、`Window::set_toolbar` で
ウィンドウに取り付けます。macOS の `NSToolbar` がウィンドウに付くもので
あるためで、macOS と Linux ではタイトルバーと一体で表示されます。

ナビゲーションと違い選択状態を持たず、押されるたびにその場で実行します。
押された項目は区切りを含めた並びのインデックスで返ります。

項目はアイコンで並びます。アイコンの呼び名は環境ごとに違うため、naui は
`ToolbarIcon` で**操作の種類**だけを受け取り、その環境の標準アイコンへ
写します。`label` は読み上げ・ツールチップ・項目が入りきらないときの
メニューに使われます。

```rust
let toolbar = ui.toolbar()?;
toolbar.set_items(&[
    ToolbarItem::new(ToolbarIcon::New, "新規"),
    ToolbarItem::new(ToolbarIcon::Open, "開く"),
    ToolbarItem::separator(),
    ToolbarItem::new(ToolbarIcon::Save, "保存").enabled(false),
]);
toolbar.on_activate(|index| println!("{index} 番目が押されました"));
toolbar.set_item_enabled(3, true);
window.set_toolbar(&toolbar);
```

| 環境 | アイコンの出どころ |
| --- | --- |
| macOS | SF Symbols |
| Linux | アイコンテーマ (freedesktop の標準名) |
| Windows | Segoe Fluent Icons |
| Web | naui 同梱の SVG (ブラウザに標準のアイコンセットが無いため) |

用意しているのは `ToolbarIcon` に並ぶ 20 種類の操作だけで、任意の画像は
置けません。

</details>

## ウィジェット

| 分類 | API |
| --- | --- |
| ウィンドウ・レイアウト | `Window`、`Stack`、`Grid`、`Scroll`、`Spacer`、`Expander`、`Toolbar` |
| 基本・入力 | `Label`、`Button`、`Checkbox`、`Toggle`、`TextInput`、`TextArea`、`PasswordInput`、`NumberInput`、`Slider`、`ProgressBar` |
| データ選択 | `ComboBox`、`RadioGroup`、`DatePicker`、`TimePicker`、`ColorPicker`、`List`、`Table`、`Tree` |
| ファイル・メディア | `FilePicker`、`FileSaver`、`Image`、`Video`、`Audio` |
| オーバーレイ | `PopupMenu`、`Dialog`、`Toast` |
| ナビゲーション | `Tabs`、`Navbar`、`Dock`、`Menu`、`Breadcrumbs`、`Pagination`、`Link` |

### プラットフォーム別の実装

凡例:

| 記号 | 意味 |
| --- | --- |
| ✅ | プラットフォームの標準コントロールをそのまま使用 |
| 🟡 | 標準コントロールを組み合わせて実装 |
| 🔴 | 対応する概念がないため、別の要素で再現 |

<details>
<summary><strong>ウィンドウ・レイアウト</strong></summary>

| naui | Windows (WinUI 3) | macOS (AppKit) | Linux (GTK4) | Web (DOM) |
| --- | --- | --- | --- | --- |
| `Window` | ✅ `Microsoft.UI.Xaml.Window` | ✅ `NSWindow` | ✅ `AdwApplicationWindow` | 🔴 `<div>` + `document.title` |
| `Stack` | ✅ `StackPanel` | ✅ `NSStackView` | ✅ `GtkBox` | 🟡 `<div>` + CSS Flexbox |
| `Grid` | ✅ `Grid` | ✅ `NSGridView` | 🟡 `GtkGrid` | 🟡 `<div>` + CSS Grid |
| `Scroll` | ✅ `ScrollViewer` | ✅ `NSScrollView` | ✅ `GtkScrolledWindow` | 🟡 `<div>` + `overflow` |
| `Spacer` | 🔴 中身のない `Grid` | 🟡 中身のない `NSView` | 🟡 中身のない `GtkBox` | 🟡 `<div>` + `flex-grow` |
| `Expander` | ✅ `Expander` | 🟡 `NSButton` (入り切り) + `NSStackView` | ✅ `GtkExpander` | ✅ `<details>` + `<summary>` |
| `Toolbar` | 🟡 `StackPanel` + `Button` | ✅ `NSToolbar` + `NSToolbarItem` | 🟡 `AdwHeaderBar` + `GtkButton` | 🟡 `<div role="toolbar">` + `<button>` |

</details>

<details>
<summary><strong>基本・入力</strong></summary>

| naui | Windows (WinUI 3) | macOS (AppKit) | Linux (GTK4) | Web (DOM) |
| --- | --- | --- | --- | --- |
| `Label` | ✅ `TextBlock` | ✅ `NSTextField` | ✅ `GtkLabel` | ✅ `<span>` |
| `Button` | ✅ `Button` | ✅ `NSButton` | ✅ `GtkButton` | ✅ `<button>` |
| `Checkbox` | ✅ `CheckBox` | ✅ `NSButton` | ✅ `GtkCheckButton` | 🟡 `<input type="checkbox">` + `<label>` |
| `Toggle` | ✅ `ToggleSwitch` | 🟡 `NSSwitch` + `NSTextField` | 🟡 `GtkSwitch` + `GtkLabel` | 🟡 `<input type="checkbox" switch>` + `<label>` |
| `TextInput` | ✅ `TextBox` | ✅ `NSTextField` | ✅ `GtkEntry` | ✅ `<input type="text">` |
| `TextArea` | ✅ `TextBox` | 🟡 `NSTextView` + `NSScrollView` | 🟡 `GtkTextView` + `GtkScrolledWindow` | ✅ `<textarea>` |
| `PasswordInput` | ✅ `PasswordBox` | ✅ `NSSecureTextField` | ✅ `GtkPasswordEntry` | ✅ `<input type="password">` |
| `NumberInput` | 🟡 `TextBox` + 増減ボタン | 🟡 `NSTextField` + `NSStepper` | ✅ `GtkSpinButton` | ✅ `<input type="number">` |
| `Slider` | ✅ `Slider` | ✅ `NSSlider` | ✅ `GtkScale` | ✅ `<input type="range">` |
| `ProgressBar` | 🟡 `Grid` + `Border` | ✅ `NSProgressIndicator` | ✅ `GtkProgressBar` | ✅ `<progress>` |

</details>

<details>
<summary><strong>データ選択</strong></summary>

| naui | Windows (WinUI 3) | macOS (AppKit) | Linux (GTK4) | Web (DOM) |
| --- | --- | --- | --- | --- |
| `ComboBox` | ✅ `ComboBox` | ✅ `NSPopUpButton` | ✅ `GtkDropDown` | ✅ `<select>` |
| `RadioGroup` | 🟡 `StackPanel` + `RadioButton` | 🟡 `NSStackView` + `NSButton` (ラジオ型) | 🟡 `GtkBox` + 組にした `GtkCheckButton` | 🟡 `<div role="radiogroup">` + `<input type="radio">` |
| `DatePicker` | ✅ `DatePicker` / `TimePicker` | ✅ `NSDatePicker` | 🟡 `GtkMenuButton` + `GtkCalendar` + `GtkSpinButton` | ✅ `<input type="date">` / `"time"` / `"datetime-local"` |
| `TimePicker` | ✅ `TimePicker` | ✅ `NSDatePicker` (時分だけ) | 🟡 時と分の `GtkSpinButton` | ✅ `<input type="time">` |
| `ColorPicker` | 🟡 `Button` + `Flyout` + `ColorPicker` | ✅ `NSColorWell` | ✅ `GtkColorDialogButton` | ✅ `<input type="color">` |
| `List` | ✅ `ListBox` | ✅ `NSTableView` + `NSScrollView` | 🟡 `GtkListBox` + `GtkScrolledWindow` | ✅ `<select size>` / 🟡 `<ul role="listbox">` |
| `Table` | 🟡 `Grid` + `ListBox` (行は `Grid`) | ✅ `NSTableView` + `NSTableHeaderView` | 🟡 `GtkListBox` + `GtkSizeGroup` | 🟡 `<table role="grid">` |
| `Tree` | 🟡 `ListBox` + 開閉ボタン | ✅ `NSOutlineView` + `NSScrollView` | 🟡 `GtkListBox` + 開閉ボタン | 🟡 `<ul role="tree">` |

</details>

<details>
<summary><strong>ファイル・メディア</strong></summary>

| naui | Windows (WinUI 3) | macOS (AppKit) | Linux (GTK4) | Web (DOM) |
| --- | --- | --- | --- | --- |
| `FilePicker` | 🟡 `Button` + `IFileOpenDialog` | 🟡 `NSButton` + `NSOpenPanel` | 🟡 `GtkButton` + `GtkFileDialog` | 🟡 `<button>` + `<input type="file">` |
| `FileSaver` | 🟡 `Button` + `IFileSaveDialog` | 🟡 `NSButton` + `NSSavePanel` | 🟡 `GtkButton` + `GtkFileDialog` (save) | 🔴 `<button>` + `showSaveFilePicker` / `<a download>` |
| `Image` | 🟡 `Image` (`XamlReader` 経由) | ✅ `NSImageView` | ✅ `GtkPicture` | ✅ `<img>` |
| `Video` | ✅ `MediaPlayerElement` | ✅ `AVPlayerView` | 🟡 `GtkPicture` + `GtkMediaControls` | ✅ `<video>` |
| `Audio` | ✅ `MediaPlayerElement` | 🟡 `AVPlayerView` | 🟡 `GtkMediaControls` + `GtkMediaFile` | ✅ `<audio>` |

</details>

<details>
<summary><strong>オーバーレイ</strong></summary>

| naui | Windows (WinUI 3) | macOS (AppKit) | Linux (GTK4) | Web (DOM) |
| --- | --- | --- | --- | --- |
| `PopupMenu` | 🟡 `Grid` + `Button` | ✅ `NSMenu` | ✅ `GtkPopoverMenu` + `GMenu` | 🟡 `<div role="menu">` |
| `Dialog` | ✅ `ContentDialog` | 🟡 `NSAlert` + `accessoryView` | ✅ `AdwAlertDialog` | 🟡 `<dialog>` + `showModal()` |
| `Toast` | 🔴 `Grid` + `StackPanel` を重ねたもの | 🔴 `NSVisualEffectView` を重ねたもの | ✅ `AdwToast` + `AdwToastOverlay` | 🔴 `<div role="status">` |

</details>

<details>
<summary><strong>ナビゲーション</strong></summary>

| naui | Windows (WinUI 3) | macOS (AppKit) | Linux (GTK4) | Web (DOM) |
| --- | --- | --- | --- | --- |
| `Tabs` | 🟡 `Grid` + `ToggleButton` | ✅ `NSTabView` | ✅ `GtkNotebook` | 🟡 `role="tablist"` + `<button>` |
| `Navbar` | 🟡 `TextBlock` + `ToggleButton` | 🟡 `NSTextField` + `NSSegmentedControl` | 🟡 `GtkLabel` + `GtkToggleButton` | 🟡 `<nav>` + `<strong>` + `<button>` |
| `Dock` | 🟡 `ToggleButton` の横並び | ✅ `NSSegmentedControl` | 🟡 `GtkToggleButton` の横並び | 🟡 `<nav>` + `<button>` |
| `Menu` | 🟡 `ToggleButton` の縦並び | 🟡 `NSButton` の縦並び | 🟡 `GtkToggleButton` の縦並び | 🟡 `<nav><ul><li><button>` |
| `Breadcrumbs` | 🟡 `HyperlinkButton` + 区切り | ✅ `NSPathControl` | 🔴 `GtkToggleButton` + 区切り | 🟡 `<nav><ol><li><a>` |
| `Pagination` | 🟡 `Button` + `ToggleButton` | 🟡 `NSButton` + `NSSegmentedControl` | 🟡 `GtkButton` + `GtkToggleButton` | 🟡 `<nav>` + `<button>` |
| `Link` | ✅ `HyperlinkButton` | 🟡 `NSButton` + `NSWorkspace` | ✅ `GtkLinkButton` | ✅ `<a>` |

</details>

### 主なデータ型

- `Sizing` / `Length` / `Track` / `GridCell`: 配置とサイズ
- `NavItem`: ナビゲーション項目
- `ListItem` / `SelectionMode`: リスト項目と単一・複数選択
- `TreeItem`: ツリー項目 (入れ子・開閉・選べるかどうか)
- `DateTime` / `DatePickerMode`: 年月日と時分の値、日付選択で何を選ばせるか
- `Time`: 時分だけの値 (時刻の選択でやり取りする値)
- `NumberSpec`: 数値入力の下限・上限・刻み・小数桁
- `Color`: sRGB の 8 bit で表す色 (色の選択でやり取りする値)
- `FileFilter` / `FilePickerMode` / `FileEntry`: ファイルの選択と保存
- `Fit` / `PlaybackState`: メディア表示と再生状態
- `PopupItem`: ポップアップメニュー項目
- `ToolbarItem` / `ToolbarIcon`: ツールバー項目とアイコン
- `DialogButtons` / `DialogResponse`: ダイアログのボタンと応答
- `ToastSpec`: トーストの文字・操作ボタン・消えるまでの時間

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

制限事項は環境ごとにまとめています。利用するターゲットの項目を確認してください。

<details>
<summary><strong>共通</strong></summary>

- 対応するのは上記の 40 コンポーネントです。
- `Toolbar` はウィンドウに取り付けるもので、レイアウトの好きな位置には置けません
  (`NSToolbar` が `NSWindow` に付くものであるため)。アイコンは `ToolbarIcon` の
  20 種類からしか選べず、任意の画像は置けません。項目をインデックスで識別する
  都合上、macOS の「ツールバーをカスタマイズ」(利用者による並べ替え) は
  切ってあります。
- 絶対配置はありません。`Stack`、`Grid`、`Spacer` で配置します。
- `List` は 1 列で、行に置けるのは `label` と `detail` の文字列だけです。
- `Table` のセルに置けるのも文字列だけです。見出しを押しての並べ替えは
  「どの列を、どちら向きに」を通知するところまでで、**行を並べ替えるのは
  アプリの仕事**です。列の幅をドラッグで変えられるのは macOS だけです。
- `Tree` は単一選択で、項目に置けるのは `List` と同じ文字列だけです。
  項目はパス (`&[usize]`) で指し、ドラッグでの並べ替えはできません。
- `Dialog` は同時に 1 つだけで、ボタンは Primary、Secondary、Cancel の最大 3 個です。
- `Toast` も同時に 1 つだけで、新しく出したものが前のものを置き換えます
  (`AdwToastOverlay` の順番待ちも、この形にそろえています)。操作ボタンは 1 個で、
  出る位置は下端の中央に固定です。
- ウィンドウを閉じるイベントと、入力欄で Enter を押したときの共通
  `on_submit` はありません。
- メディアの対応形式は各 OS、ブラウザ、Linux の GStreamer 環境に依存します。

</details>

<details>
<summary><strong>Windows</strong></summary>

- `StackPanel` は主軸の余りを子へ配らないため、`Stack` 内の主軸方向では
  `Fill` と `Spacer` が効きません。代わりに `Grid` の `Track::Fill` を使います。
- 一部の Windows App SDK 環境で異常終了を避けるため、`Tabs` は `TabView` を使わず、
  `Video` / `Audio` の標準再生バーは無効にしています。
- `Dialog` は `window.show()` より前には開けません。
- `DataGrid` も `ListView` もバインディングに無いため、`Table` は `ListBox` の
  行を `Grid` にして組み立て、見出しは同じ列定義を持つ別の `Grid` に置いて
  幅をそろえています。並べ替えできる見出しは、地色と枠を消した `Button` です
  (WinUI に列見出し用のコントロールが無いため)。
- `TreeView` のバインディングが無いため、`Tree` は `ListBox` の行として
  組み立てています。選べない枝は行ごと無効になるので、その開閉ボタンも
  押せません (プログラムからの `expand` は効きます)。
- `NumberBox` のバインディングが無いため、`NumberInput` は `TextBox` と
  `-` / `+` のボタンを横に並べて組み立てています (`NumberBox` の既定と同じ並び)。
  値の確定は欄を離れたときです。
- `CommandBar` がバインディングに無いため、`Toolbar` は `Button` を横に並べて
  構成し、タイトルバー (ドラッグ領域) ではなくその下の行に置きます。アイコンは
  Segoe Fluent Icons を `FontIcon` で出します。
- `InfoBar` / `TeachingTip` がバインディングに無いため、`Toast` は `Grid` と
  `StackPanel` を中身の層へ重ねて組み立てています。`Dialog` と同じく、
  `window.show()` より前には出せません。
- `Expander` は `winio-winui3` の投影に含まれていませんが、WinUI の公開 WinRT
  インターフェイスを最小限投影し、`XamlReader` から本物の `Expander` を生成しています。
- `ToggleSwitch` も投影に含まれていないため、`Expander` と同じく公開 WinRT
  インターフェイスを最小限投影し、`XamlReader` から本物の `ToggleSwitch` を
  生成しています。`Toggle` のラベルは `OnContent` と `OffContent` の両方へ
  同じ文字を入れるので、入り切りで読みは変わりません (WinUI の既定は
  「オン」「オフ」と切り替わる文字です)。
- `ColorPicker` と `SolidColorBrush` も投影に含まれていないため、同じく公開
  WinRT インターフェイスを最小限投影しています。WinUI 3 の `ColorPicker` は
  スペクトラムとスライダーを縦に並べた大きな面なので、`Button` の `Flyout` へ
  入れ、ボタンには選んだ色の見本 (`Border` + `SolidColorBrush`) を出します。
- `DatePicker` / `TimePicker` も投影に含まれていないため、同じく公開 WinRT
  インターフェイスを最小限投影し、`XamlReader` から本物の `DatePicker` と
  `TimePicker` を生成しています。`DatePickerMode::DateTime` では 2 つを
  `StackPanel` で横に並べます。年 / 月 / 日 の並び順と表記はシステムのロケールに
  従い、暦はグレゴリオ暦に固定しています。`set_range` は WinUI 側へは年の範囲
  (`MinYear` / `MaxYear`) として渡し、月日と時刻の境界は naui 側で端へ寄せます。
  標準テンプレートは英語の長い月名に合わせて月の列だけを広く左寄せにするため、
  3 列を等幅・中央揃えへ直し、最小幅も詰めています。
- `TimePicker` ウィジェットは、この `TimePicker` の投影をそのまま共有しています。
  WinUI 3 の `TimePicker` に下限・上限は無いので、`set_range` の範囲は naui 側で
  端へ寄せます。

</details>

<details>
<summary><strong>macOS</strong></summary>

- `Grid` の `Track::Fill` は重みの違いを反映しません。
- 交差軸の `Fill` と Grid セル内の配置は、コンテナへ追加する前に指定してください。
- `Image` のリモート URL は同期的に読み込むため、ローカルファイルの利用を推奨します。
- `Dialog::open` と `PopupMenu::open_at` は閉じるまで戻りません。
- `DatePicker` は `NSDatePicker` そのものです。欄をクリックするとカレンダーが
  重なって開き (`presentsCalendarOverlay`)、キーボードとステッパーでも編集
  できます。見た目・操作とも AppKit の既定 (SwiftUI の `DatePicker` と同じ
  `textFieldAndStepper`) と変わりません。
- 数値専用のコントロールが AppKit に無いため、`NumberInput` は `NSTextField` と
  `NSStepper` を横に並べて組み立てています (システム設定の数値欄と同じ形)。
- `Toolbar` の区切りは、macOS の作法にならって `NSToolbarSpaceItem`
  (一定幅の空き) になります。区切り線は引かれません。
- `Toolbar` を付けるとウィンドウのタイトル文字は隠れます (macOS の作法)。
  出したままだとタイトルが先頭を占め、項目が右端へ寄ってしまうためです。
  `set_title` の値は残り、`clear_toolbar` で外すと表示も戻ります。
- トーストにあたるコントロールが AppKit に無いため、`Toast` は
  `NSVisualEffectView` (`NSPopover` と同じ材質) をウィンドウの中身へ重ねて
  組み立てています。出す先はいちばん手前のウィンドウで、まだ焦点が
  決まっていないときは最後に作ったウィンドウです。
- 折りたたみにあたるコントロールが AppKit に無いため、`Expander` は入り切りの
  `NSButton` (山形は SF Symbols) と `NSStackView` で組み立てています。
  開閉のアニメーションはありません。
- `NSSwitch` は文字を持たないため、`Toggle` のラベルは横へ並べた
  `NSTextField` です。**切り替わるのはスイッチを押したときだけ**で、
  文字を押しても切り替わりません。
- 時刻専用のコントロールが AppKit に無いため、`TimePicker` は `NSDatePicker` の
  表示項目を時分だけにしたものです (AppKit で時刻を入力させるときの作法)。
  日付の部分は画面に出ないので 1970-01-01 へ固定してあり、`set_range` の
  下限・上限は `NSDatePicker` 自身にも渡ります。
- `ColorPicker` は `NSColorWell` そのものです。押すとシステムのカラーパネルが
  開きます。パネルはカタログ色 (`systemBlue` など) も返すので、成分を読む前に
  sRGB へ変換しています。`set_enabled(false)` ではパネルとのつながりも切ります。

</details>

<details>
<summary><strong>Linux</strong></summary>

- `Grid` の `Track::Fill` は重みの違いを反映しません。
- `Fit::None` は GTK4 の `SCALE_DOWN` に対応するため、「原寸」ではなく
  「拡大しない」動作になります。
- テーマはウィンドウ単位ではなくアプリ全体へ適用されます。
- `Table` は `GtkColumnView` (`GtkListItemFactory` と `GListModel` を要求する)
  ではなく、`GtkListBox` の行を横並びにして組み立て、列の幅は列ごとの
  `GtkSizeGroup` でそろえています。並べ替えできる見出しは `flat` な
  `GtkButton` で、向きは見出しの文字に付く矢印で表します。
- `Tree` は `GtkTreeExpander` (`GtkListView` 専用) ではなく、`GtkListBox` の
  行と開閉ボタンで組み立てています。
- `Toolbar` の項目は、GNOME の作法にならってヘッダーバーの左側へ並びます。
- `Toast` の時間は `AdwToast` に合わせて**秒**単位です。1 秒未満を指定しても
  1 秒になります (0 に丸めると「消えない」指定に変わってしまうため)。
- `GtkSwitch` は文字を持たないため、`Toggle` のラベルは `GtkBox` へ横に並べた
  `GtkLabel` です。**切り替わるのはスイッチを押したときだけ**で、文字を押しても
  切り替わりません。
- `ColorPicker` は `GtkColorDialogButton` (GTK 4.10 以降) です。透明度を
  扱わないので、開く `GtkColorDialog` の `with-alpha` は切ってあります。
- 時刻を選ぶ 1 つのコントロールも GTK4 に無いため、`TimePicker` は時と分の
  `GtkSpinButton` を `:` で挟んで並べています (GNOME の時計アプリと同じ形)。
  スピンボタンに時刻としての範囲は無いので、`set_range` の範囲は naui 側で
  端へ寄せ、押し戻した結果を表示にも書き戻します。
- 日付を選ぶ 1 つのコントロールが GTK4 に無いため、`DatePicker` は
  `GtkMenuButton` のポップオーバーに載せた `GtkCalendar` と、時分の
  `GtkSpinButton` で組み立てています。`GtkCalendar` は「日を押した」と
  「月を送った」を区別しないので、**日を選んでもポップオーバーは閉じません**。
  ボタンに出る日付はロケールの書式に従います。

</details>

<details>
<summary><strong>Web</strong></summary>

- `Window` は OS のウィンドウではなく、`<body>` 直下の要素と
  `document.title` で表現されます。
- `ListItem::detail` を使うと、`List` は `<select>` から
  `<ul role="listbox">` を使った実装へ切り替わります。
- `Tree` の `TreeItem::detail` は、行の高さをそろえるため
  `ラベル — 補助` の形で 1 行に収まります。
- `Table` は `<table role="grid">` です。`<table>` に行を選ぶ仕組みは無いので、
  クリック (⌘ / Ctrl / Shift) とキー操作 (矢印・Home / End・Space) は naui が
  足しています。列の幅は `<colgroup>` の `<col>` と `table-layout: fixed` で
  決まります。並べ替えできる見出しは `<th>` の中の `<button>` で、向きは
  `aria-sort` と矢印で表します。
- `FilePicker::open` と `FileSaver::open` はユーザー操作のイベント内で
  呼ぶ必要があります。
- `FileSaver` は `showSaveFilePicker` があればそれを使い、無いブラウザ
  (Firefox / Safari) では `<a download>` のダウンロードになります。後者では
  保存先の確認が出ないことがあり、`FileEntry::path` はどちらでも `None` です。
- `DatePicker` の入力 UI (カレンダーなど) はブラウザが出すため、見た目と操作は
  ブラウザごとに違います。時間帯を持ち込まないよう `datetime-local` を使います。
- `<details>` に `disabled` が無いため、`Expander::set_enabled(false)` は
  見出しをマウスとタブ順から外して押せなくするだけです (`aria-disabled` は
  付きますが、ネイティブの無効なコントロールとは扱いが違います)。
- `TimePicker` の入力 UI (スピナーや時計) もブラウザが出すため、見た目と操作は
  ブラウザごとに違います。`<input type="time">` が秒まで返した場合、naui は秒を
  捨てて分までを扱います。
- `ColorPicker` の入力 UI (パレットやスポイト) はブラウザと OS が出すため、
  見た目と操作はブラウザごとに違います。値は `change` (確定) で受け取ります。
- `Toggle` の `switch` 属性に対応しているのは Safari 17.4 以降だけなので、
  Chrome や Firefox ではチェックボックスの見た目で出ます (値の扱い・通知・
  読み上げは同じです)。つまみの見た目を naui の CSS で作ることはしません。
- ページに一時的な通知の標準要素が無いため、`Toast` は `<div role="status">` を
  `position: fixed` で下端の中央へ出します (`Notification` API はページの外へ
  出るもので別物です)。
- タイトルバーが無いため、`Toolbar` はウィンドウ要素の先頭に置かれます。
- ブラウザに標準のアイコンセットが無いため、`Toolbar` のアイコンだけは naui が
  SVG を持ちます (ここだけは OS のものを使いません)。
- ブラウザの制限により、メディアの自動再生が拒否される場合があります。

</details>

## ライセンス

[MIT](LICENSE-MIT) OR [Apache-2.0](LICENSE-APACHE)
