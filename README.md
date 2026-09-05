# naui

[![CI](https://github.com/mokuichi147/naui/actions/workflows/ci.yml/badge.svg)](https://github.com/mokuichi147/naui/actions/workflows/ci.yml)

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
  (プラットフォームを補う必要があるところは「既知の制限」に明記しています)
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
- Windows: Windows App SDK 1.3 以降のフレームワークランタイム

Ubuntu 24.04 では、次のコマンドで Linux 向けの依存パッケージを導入できます。

```sh
sudo apt install libgtk-4-dev libadwaita-1-dev build-essential pkg-config
```

naui は 2.x から 1.3 まで新しい順に探し、最初に見つかったランタイムを使います。
OS 同梱の系統 (`Microsoft.WindowsAppRuntime.CBS*`) も候補に入れます。実機での
動作確認は 2.x で行っています。

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
| macOS | ✅ 動作確認済み | AppKit の実コントロールを使った統合テストと Gallery の実行 (色ピッカー・時刻ピッカー・テーブル・検索入力・自由入力コンボボックス・分割ビュー・別スレッドからの受け渡しと `spawn` を含む) |
| Linux | ✅ 動作確認済み | Ubuntu 24.04、GTK 4.14、libadwaita 1.5、Wayland で Gallery と統合テストを実行 (スイッチ・色ピッカー・時刻ピッカー・テーブル・検索入力・自由入力コンボボックス・分割ビュー・ラベルの折り返し・別スレッドからの受け渡しと `spawn` を含む) |
| Web | ✅ 動作確認済み | ブラウザ上で DOM の描画、入力、検索入力、自由入力コンボボックス、ナビゲーション、ファイル選択、メディア、ダイアログ、トースト、折りたたみ、スイッチ、時刻ピッカー、色ピッカー、テーブル、分割ビュー、ラベルの折り返し、非同期処理の実行と中断を操作 |
| Windows | ✅ 動作確認済み | Windows App SDK 2.3.1 の x64 実機で全ウィジェットとナビゲーションを操作 (スイッチ・色ピッカー・時刻ピッカー・テーブル・検索入力・自由入力コンボボックス・分割ビュー・ラベルの折り返し・別スレッドからの受け渡しと `spawn` を含む) |

実装済みで `cargo check` は通るものの、実機で未確認の範囲があります。

<details>
<summary>未確認の範囲を表示</summary>

- Web: `PopupMenu` のブラウザ実行と、埋め込みブラウザで配送されなかった
  `Dialog` の Esc 操作

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

### 後からウィジェットを作る

`Ui` はウィジェットのハンドルと同じく clone できます。clone しても参照が増える
だけで中身は同じなので、コールバックへ持ち込めば `build` が終わったあとでも
ウィジェットを作れます。一覧へ行を足す、入力欄を増やすといった、数が実行時に
決まる画面はこの形で書きます。

```rust
use std::cell::RefCell;
use std::rc::Rc;

use naui::{ListRow, Orientation};

let list = ui.list()?;
// 行は積み上げていくので、並びはアプリ側で持つ。
let rows: Rc<RefCell<Vec<ListRow>>> = Rc::new(RefCell::new(Vec::new()));

let add = ui.button("行を足す")?;
add.on_click({
    let ui = ui.clone(); // コールバックの中で使う Ui
    let list = list.clone();
    let rows = rows.clone();
    move || {
        // ウィジェットを作る API は Result を返すので、ここで受ける。
        let (Ok(content), Ok(label)) =
            (ui.stack(Orientation::Horizontal), ui.label("新しい行"))
        else {
            return;
        };
        content.append(&label);
        rows.borrow_mut().push(ListRow::new(&content));
        list.set_rows(&rows.borrow());
    }
});
```

`Ui` は UI スレッド専用で `Send` ではないため、別スレッドへは渡せません。
作れるのは `build` と同じ UI スレッドの上だけで、ウィジェットの通知と
`Tasks` の受け口・`spawn` した future の中がそれにあたります
([別スレッドと非同期](#別スレッドと非同期)を参照)。

### 後からウィジェットを外す

置いたものを減らす側も、コンテナごとに手段があります。

| 変えたいもの | 使う API |
| --- | --- |
| `Stack` の子 | `insert` / `remove` / `clear` |
| `Grid` のマス | `replace` / `remove` / `clear` |
| `Tabs` のタブ | `remove_tab` / `clear` |
| 一覧・表・ツリー・ナビゲーション | `set_items` / `set_rows` で丸ごと置き換える |
| 中身が 1 つのもの (`Scroll`、`Expander`、`SplitView`、`Window`) | `set_child` などで差し替える |

```rust
use naui::{GridCell, Orientation};

let stack = ui.stack(Orientation::Vertical)?;
stack.append(&ui.label("1 行目")?);
stack.insert(0, &ui.label("見出し")?); // 先頭へ差し込む
stack.remove(1);                       // 位置を指して外す
stack.clear();                         // 全部外す

let form = ui.grid()?;
form.attach(&ui.label("名前")?, GridCell::new(0, 0));
form.remove(GridCell::new(0, 0));      // マスを指して外す
```

`Stack` と `Tabs` は**位置 (インデックス) で指し**、`Grid` は**マスで指します**
(span は見ません)。範囲外の位置や空のマスを指したときは何もしません。
`Tabs::remove_tab` で選択中のタブを外したときは、同じ位置のタブ (無ければ最後の
タブ) が選ばれ、**この移動は通知しません** (`set_selected` と同じ決まりです)。

`Grid::replace` は**そのマスだけ**を差し替えます。ほかのマスの子は残ります。

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

レイアウトには次の 6 種類を使います。

| API | 用途 |
| --- | --- |
| `Stack` | 子を縦または横へ並べる |
| `Grid` | 行と列を指定して配置する |
| `Scroll` | はみ出した内容をスクロールする |
| `Spacer` | 親の余った空間を受け取る |
| `Expander` | 見出しを押したときだけ中身を見せる |
| `SplitView` | 2 つの区画を、動かせる仕切りで分ける |

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

`List` の `Auto` 高さは全行に追従します。行数が多く、限られた領域で
スクロールさせたいときは `Fixed` または `Fill` を指定します。
`Tree`、`Scroll`、`SplitView`、`TextArea` は内容から高さを決めないため、
通常は `set_sizing` で高さを指定します。

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

#### ラベルの折り返し

`Label` は**既定では折り返しません**。1 行に収まらない分は、末尾を省略記号 (…)
で切ります。長い文章を出すときは `set_wrap(true)` を指定します。

```rust
let note = ui.label("狭いところでは折り返してほしい長い説明")?;
note.set_wrap(true);
note.set_sizing(Sizing::fill_width()); // 折り返す幅は親が決める
```

**折り返す幅を決めるのは親**なので、`Stack` の中では `set_sizing` で幅も
与えます。幅が決まらないと、どの環境でも 1 行ぶんの幅を要求したままになります。

| 環境 | 折り返さないとき | 折り返すとき |
| --- | --- | --- |
| Windows | `TextWrapping="NoWrap"` + `TextTrimming="CharacterEllipsis"` | `TextWrapping="Wrap"` |
| macOS | `usesSingleLineMode` + `ByTruncatingTail` | `ByWordWrapping` + `preferredMaxLayoutWidth` |
| Linux | `PangoEllipsizeMode::End` | `gtk_label_set_wrap` |
| Web | `white-space: nowrap` + `text-overflow: ellipsis` | `white-space: normal` |

**`<span>` の既定は折り返す**ので、Web だけは naui が CSS で他の 3 環境へ
そろえています (`Table` や `Tree` のセルと同じ扱いです)。

#### 区画の分割

画面を 2 つに分け、その境目を利用者に動かさせたいときは `SplitView` を使います。
区画は start (左または上) と end (右または下) の 2 つで、**仕切りの位置は
start 側の大きさ**を論理ピクセルで表します。

```rust
let split = ui.split_view(Orientation::Horizontal)?; // 区画が横に並ぶ
split.set_start(&sidebar);
split.set_end(&body);
split.set_position(220.0);         // 通知せずに仕切りを置く
split.set_min_sizes(120.0, 200.0); // これより狭くはできない
split.on_resize(|position| println!("サイドバーの幅は {position}"));
split.set_sizing(Sizing::fill());  // 中身の高さでは決まらない
```

**余った場所は end 側が受け取ります。** ウィンドウを広げても start 側は指定した
大きさのままなので、サイドバーと本文のような組み合わせにそのまま使えます。逆に
したいときは、狭いほうを end 側へ置いてください。

作った直後の位置は `DEFAULT_SPLIT_POSITION` (200 px) で、4 環境とも同じです
(環境ごとの既定には任せていません)。`set_position` は通知せず、`drag_to` は
利用者が動かしたのと同じく `on_resize` を呼びます (`ComboBox` の `set_selected`
と `select` と同じ決まりです)。

`set_min_sizes` の既定はどちらも 0 ですが、**中身のコントロール自身がそれ以上
縮まないことがあります**。そのときはその環境が決める最小のほうが勝ちます。
画面がせまくて両方の最小を満たせないときは start 側が優先され、広がればまた
指定した位置へ戻ります。

`Scroll` と同じく**中身の大きさから高さは決まらない**ので、大きさは
`set_sizing` で指定します。区画は 2 つなので、3 つ以上に分けたいときは
`SplitView` を入れ子にします。

**仕切りが「その環境の標準コントロール」なのは macOS と Linux だけ**です
(`NSSplitView` と `GtkPaned`)。Windows と Web には対応するコントロールが無いため、
naui が 3 つの区画 (start・仕切り・end) を並べて組み立てます。組み立てるのは
位置と当たり判定だけで、仕切りの色はテーマリソース (Windows) と CSS のシステム
カラー (Web)、カーソルの形は CSS の `col-resize` / `row-resize` に任せています。

この 2 つでは、**見えるのは境目に引く 1 px の線だけ**で、残りは透明なつかみ代に
なっています (つかめる幅は 6 px)。塗りつぶしの帯にすると、区切りというより
1 つの部品のように見えて周りから浮くためです。

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

#### 検索の入力

絞り込みや検索の欄には `SearchInput` を使います。`TextInput` に**確定の通知**を
足したもので、`on_change` は打つたび、`on_search` は **Enter で確定したとき**に
呼ばれます (Windows は虫めがねの印を押したときも確定になります)。

```rust
let search = ui.search_input()?;
search.set_placeholder("検索");
search.on_change(|text| println!("絞り込み: {text}")); // 打ちながら絞り込む
search.on_search(|text| println!("{text} を探す")); // 押されたときだけ探す
```

虫めがねの印と、打ち始めると出る**取り消しボタン (✕) はその環境が出します**
(naui は見た目を作りません)。**取り消しボタンは確定ではありません**。空になった
ことは `on_change` で伝わり、`on_search` は呼ばれません。ブラウザによっては
取り消しボタンが出ません (Firefox にはありません)。

`set_text` は通知しません (`TextInput` と同じです)。**候補の一覧は持ちません**。
Windows の `AutoSuggestBox` には候補を出す仕組みがありますが、残る 3 環境の
検索欄に無いためです。

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

候補にない値も受け付けたいときは `EditableComboBox` を使います。こちらは
打ち込める入力欄で、候補は入力の補助にすぎません。**値はインデックスではなく
文字列**で、`on_change` は打鍵でも候補の選択でも呼ばれます。

```rust
let city = ui.editable_combo_box()?;
city.set_items(&["東京", "大阪", "札幌"]);
city.set_placeholder("都市名");
city.set_text("京都"); // 候補にない値も入ります (通知はしません)
city.on_change(|text| println!("{text} と入力されました"));
```

`selected` は「今の文字列とそのまま一致する候補」を返すので、候補にない値が
入っているときは `None` になります。`set_text` と `set_selected` は通知せず、
`select` は利用者が選んだときと同じく通知します。

**打った文字で候補の一覧が絞り込まれるかどうかは環境によって違います。**
naui は一覧を自分で組み立てず、それぞれのコントロールに任せているためです。

| 環境 | 打った文字と候補の一覧 |
| --- | --- |
| Windows | 絞り込みません (一致する候補へ選択が移るだけです) |
| macOS | 絞り込みません (代わりに入力欄の側が補完されます) |
| Linux | 絞り込みません (矢印を押すと候補が全部出ます) |
| Web | ブラウザが絞り込みます (前方一致か部分一致かはブラウザ次第で、切れません) |

どの環境でも絞り込みたい場合は、`on_change` の中でアプリが `set_items` を
呼び直して候補そのものを入れ替えてください (Web ではさらにブラウザ側の
絞り込みが重なります)。

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

#### リスト

縦に並ぶ一覧は `List` です。行の作り方は 2 通りあります。

| 行の作り方 | 使う型 | 中身 |
| --- | --- | --- |
| `set_items` | `ListItem` | 文字だけの行。`detail` を付けると補助の文字が 2 行目に添えられます |
| `set_rows` | `ListRow` | 任意のウィジェットを 1 行として並べます |

どちらも**行の識別はインデックス**で、選択は `SelectionMode` で単一と複数を
選べます。高さを指定しなければ全行に追従します
([配置とサイズ](#配置とサイズ)を参照)。

設定画面のように、先頭のチェックボックス・複数のラベル・末尾のボタンを持つ行は、
`Grid` や `Stack` で組み立てて `ListRow` に包み、`set_rows` で表示します。

```rust
use naui::{ListRow, Orientation, Sizing};

let content = ui.stack(Orientation::Horizontal)?;
let check = ui.checkbox("")?;
content.append(&check);
content.append(&ui.label("Wi-Fi")?);
content.append(&ui.button("設定…")?);
content.set_sizing(Sizing::fill_width());

// 行全体は選ばせず、押されたらチェックだけを切り替える。
let row = ListRow::new(&content).selectable(false);
row.on_activate(move || check.set_checked(!check.is_checked()));

let list = ui.list()?;
list.set_rows(&[row]);
```

`on_activate` は**行のラベルや余白が押されたときだけ**呼ばれます。中のボタンや
チェックボックスを直接押したときは、それぞれのコールバックだけが呼ばれるので、
同じ操作が二重に起きません。押せるようにするかどうかは行を組み立てるときに
決まるので、`set_rows` へ渡す前に指定します。

行の数や中身が実行時に決まるときは、clone した `Ui` をコールバックへ持ち込んで、
そこで行を組み立てます ([後からウィジェットを作る](#後からウィジェットを作る)を
参照)。

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

行数が多いときは、`set_sizing` で表示領域の高さを決めておきます
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

#### 別スレッドと非同期

時間のかかる処理を別のスレッドでやって画面を書き換えたいときは `Ui::tasks`
を使います。ウィジェットのハンドルは `Rc` を持つので**別スレッドへは送れません**。
そこで、**受け取るクロージャを UI スレッド側に据え、送る側だけをスレッドへ渡す**
形になります。

```rust
let label = ui.label("待機中")?;
let sender = ui.tasks().channel({
    let label = label.clone();
    move |text: String| label.set_text(&text) // UI スレッドで呼ばれる
});
std::thread::spawn(move || {
    let _ = sender.send(heavy_work());        // 別スレッドから
});
```

`Sender` は `Send + Sync + Clone` なので、複数のスレッドから同じ受け口へ送れます。
同じチャネルへ送った値は**送った順に**届きます。`send` はその場でクロージャを
呼ばず、UI スレッドの手が空いてから届きます。

ボタンのクロージャの中から非同期処理を始めたいときは `Tasks::spawn` を使います。
**future は `Send` でなくてよい**ので、ウィジェットのハンドルをそのまま
持ち込めます。返る `Task` を捨ててもタスクは止まりません。押すたびに前の処理を
打ち切りたいときは、取っ手を持っておいて次のときに `cancel` を呼びます。

```rust
let tasks = ui.tasks();
let label = ui.label("待機中")?;
let button = ui.button("読み込む")?;
button.on_click(move || {
    let label = label.clone();
    tasks.spawn(async move { label.set_text(&fetch().await); });
});
```

**naui は async ランタイムを持ち込みません。** `spawn` は UI スレッドの
イベントループの上で future を進めるだけの小さな実行器で、tokio などは
要りません。逆に言うと、**future の中でブロッキング処理をすると画面が止まります**。
重い処理は `std::thread::spawn` と `channel` へ出してください。

また `spawn` は**汎用ランタイムの代わりにはなりません**。naui が引き受けるのは
「future を UI スレッドで poll し、その `Waker` で再び poll する」ところまでで、
特定のランタイムの実行器やリアクターを必要とする future (tokio の I/O など) が
動くことは保証しません。それらは**そのランタイムで動かし、結果だけを `Sender`
で画面へ戻します**。この形なら、好きなランタイムから naui へ戻せます。

```rust,ignore
let sender = ui.tasks().channel(move |result: String| label.set_text(&result));
tokio::spawn(async move {
    let result = tokio_specific_operation().await;
    let _ = sender.send(result);
});
```

投函先は環境ごとに違いますが、**その場では実行せず必ず後回しにする**ところは
同じです。これにより、通知の中から `send` や `spawn` を呼んでも再入になりません。

| naui | Windows | macOS | Linux | Web |
| --- | --- | --- | --- | --- |
| 投函先 | `DispatcherQueue.TryEnqueue` | main queue | `g_idle_add` | microtask キュー |

</details>

## ウィジェット

| 分類 | API |
| --- | --- |
| ウィンドウ・レイアウト | `Window`、`Stack`、`Grid`、`Scroll`、`Spacer`、`Expander`、`SplitView`、`Toolbar` |
| 基本・入力 | `Label`、`Button`、`Checkbox`、`Toggle`、`TextInput`、`TextArea`、`PasswordInput`、`SearchInput`、`NumberInput`、`Slider`、`ProgressBar` |
| データ選択 | `ComboBox`、`EditableComboBox`、`RadioGroup`、`DatePicker`、`TimePicker`、`ColorPicker`、`List`、`Table`、`Tree` |
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
| `SplitView` | 🔴 `Grid` + 仕切りの `Grid` | ✅ `NSSplitView` | ✅ `GtkPaned` | 🔴 `<div>` + `<div role="separator">` |
| `Toolbar` | ✅ `CommandBar` + `AppBarButton` | ✅ `NSToolbar` + `NSToolbarItem` | 🟡 `AdwHeaderBar` + `GtkButton` | 🟡 `<div role="toolbar">` + `<button>` |

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
| `SearchInput` | ✅ `AutoSuggestBox` | ✅ `NSSearchField` | ✅ `GtkSearchEntry` | ✅ `<input type="search">` |
| `NumberInput` | ✅ `NumberBox` | 🟡 `NSTextField` + `NSStepper` | ✅ `GtkSpinButton` | ✅ `<input type="number">` |
| `Slider` | ✅ `Slider` | ✅ `NSSlider` | ✅ `GtkScale` | ✅ `<input type="range">` |
| `ProgressBar` | 🟡 `Grid` + `Border` | ✅ `NSProgressIndicator` | ✅ `GtkProgressBar` | ✅ `<progress>` |

</details>

<details>
<summary><strong>データ選択</strong></summary>

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

</details>

<details>
<summary><strong>ファイル・メディア</strong></summary>

| naui | Windows (WinUI 3) | macOS (AppKit) | Linux (GTK4) | Web (DOM) |
| --- | --- | --- | --- | --- |
| `FilePicker` | 🟡 `Button` + `IFileOpenDialog` | 🟡 `NSButton` + `NSOpenPanel` | 🟡 `GtkButton` + `GtkFileDialog` | 🟡 `<button>` + `<input type="file">` |
| `FileSaver` | 🟡 `Button` + `IFileSaveDialog` | 🟡 `NSButton` + `NSSavePanel` | 🟡 `GtkButton` + `GtkFileDialog` (save) | 🔴 `<button>` + `showSaveFilePicker` / `<a download>` |
| `Image` | ✅ `Image` | ✅ `NSImageView` | ✅ `GtkPicture` | ✅ `<img>` |
| `Video` | ✅ `MediaPlayerElement` | ✅ `AVPlayerView` | 🟡 `GtkPicture` + `GtkMediaControls` | ✅ `<video>` |
| `Audio` | ✅ `MediaPlayerElement` | 🟡 `AVPlayerView` | 🟡 `GtkMediaControls` + `GtkMediaFile` | ✅ `<audio>` |

</details>

<details>
<summary><strong>オーバーレイ</strong></summary>

| naui | Windows (WinUI 3) | macOS (AppKit) | Linux (GTK4) | Web (DOM) |
| --- | --- | --- | --- | --- |
| `PopupMenu` | ✅ `MenuFlyout` | ✅ `NSMenu` | ✅ `GtkPopoverMenu` + `GMenu` | 🟡 `<div role="menu">` |
| `Dialog` | ✅ `ContentDialog` | 🟡 `NSAlert` + `accessoryView` | ✅ `AdwAlertDialog` | 🟡 `<dialog>` + `showModal()` |
| `Toast` | 🟡 `InfoBar` を重ねたもの | 🔴 `NSVisualEffectView` を重ねたもの | ✅ `AdwToast` + `AdwToastOverlay` | 🔴 `<div role="status">` |

</details>

<details>
<summary><strong>ナビゲーション</strong></summary>

| naui | Windows (WinUI 3) | macOS (AppKit) | Linux (GTK4) | Web (DOM) |
| --- | --- | --- | --- | --- |
| `Tabs` | 🟡 `Grid` + `ToggleButton` | ✅ `NSTabView` | ✅ `GtkNotebook` | 🟡 `role="tablist"` + `<button>` |
| `Navbar` | 🟡 `TextBlock` + `ToggleButton` | 🟡 `NSTextField` + `NSSegmentedControl` | 🟡 `GtkLabel` + `GtkToggleButton` | 🟡 `<nav>` + `<strong>` + `<button>` |
| `Dock` | 🟡 `ToggleButton` の横並び | ✅ `NSSegmentedControl` | 🟡 `GtkToggleButton` の横並び | 🟡 `<nav>` + `<button>` |
| `Menu` | 🟡 `NavigationViewItem` の縦並び | 🟡 `NSButton` の縦並び | 🟡 `GtkToggleButton` の縦並び | 🟡 `<nav><ul><li><button>` |
| `Breadcrumbs` | 🟡 `HyperlinkButton` + 区切り | ✅ `NSPathControl` | 🔴 `GtkToggleButton` + 区切り | 🟡 `<nav><ol><li><a>` |
| `Pagination` | 🟡 `Button` + `ToggleButton` | 🟡 `NSButton` + `NSSegmentedControl` | 🟡 `GtkButton` + `GtkToggleButton` | 🟡 `<nav>` + `<button>` |
| `Link` | ✅ `HyperlinkButton` | 🟡 `NSButton` + `NSWorkspace` | ✅ `GtkLinkButton` | ✅ `<a>` |

</details>

### 主なデータ型

- `Sizing` / `Length` / `Track` / `GridCell`: 配置とサイズ
- `NavItem`: ナビゲーション項目
- `ListItem` / `ListRow` / `SelectionMode`: 単純なリスト項目、任意内容の行、単一・複数選択
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
  naui-core      共通の値型・チャネル・タスク
  naui-macos     AppKit バックエンド
  naui-web       DOM バックエンド
  naui-windows   WinUI 3 バックエンド
  naui-winui3    WinUI 3 の WinRT 投影 (naui-windows だけが使う)
  naui-gtk       GTK4 / libadwaita バックエンド
  naui           対象に応じてバックエンドを選ぶファサード
examples/
  counter        最小サンプル
  gallery        種別ごとの全ウィジェットデモ
tools/
  winui3-bindgen naui-winui3 の投影を .winmd から作り直すツール
```

バックエンド固有の依存はターゲット別に宣言されています。たとえば macOS の
ビルドで GTK4 や WinUI 3 の依存が引き込まれることはありません。

WinUI 3 は Windows App SDK に入っていて `windows` クレートに投影がないため、
`naui-winui3` が Microsoft の配る `.winmd` から作った投影を持っています。
生成物 (`crates/naui-winui3/src/bindings.rs`) はコミットしてあるので、
ふだんのビルドに `.winmd` は要りません。型を足したいときの手順は
`tools/winui3-bindgen/README.md` にあります。

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

Web バックエンドのテストは**実ブラウザの上**で走ります (`crates/naui-web/tests/dom.rs`)。
`Cargo.lock` と同じ版の wasm-bindgen CLI と、ブラウザのドライバが必要です。

```sh
cargo install wasm-bindgen-cli --version "$(awk '/^name = "wasm-bindgen"$/{getline; gsub(/[",]/,""); print $3; exit}' Cargo.lock)"
CHROMEDRIVER="$(command -v chromedriver)" \
  cargo test --target wasm32-unknown-unknown -p naui-web
```

`GECKODRIVER` を渡せば Firefox でも同じテストが走ります。ドライバが手元に無い
ときは `NO_HEADLESS=1` を付けると、ランナーが待ち受ける URL を表示するので、
任意のブラウザで開けば結果がそのページに出ます。

```sh
NO_HEADLESS=1 cargo test --target wasm32-unknown-unknown -p naui-web
```

別ターゲットの API 互換性は `cargo check` で確認できます。

```sh
cargo check --target wasm32-unknown-unknown -p naui
cargo check --target x86_64-pc-windows-msvc -p naui
cargo check --target x86_64-unknown-linux-gnu -p naui
```

`crates/naui/src/lib.rs` の `__api_contract` が公開 API を一通り型検査し、
バックエンド間のシグネチャのずれを検出します。

### CI

`.github/workflows/ci.yml` が push と pull request で 4 環境を並行に検査します。

| ジョブ | ランナー | 内容 |
| --- | --- | --- |
| 整形と lint | ubuntu-latest | `cargo fmt --all --check` と `naui-core` の clippy |
| macOS | macos-latest | ビルドと AppKit の統合テスト |
| Windows | windows-latest | ビルド、clippy、単体 / ドキュメンテーションテスト |
| Linux | ubuntu-latest | GTK4 を導入し、Xvfb 上で統合テスト |
| Web | ubuntu-latest | wasm ターゲットの型検査、Chrome / Firefox 上の DOM テスト、Gallery の wasm ビルド |

`cargo test --workspace` は使いません。`crates/naui-gtk/tests/gtk4.rs` に cfg が
無いため、Linux 以外では壊れます。各ジョブは自分のプラットフォーム向けの
パッケージだけを指定します。

テストには実行環境の前提がいくつかあります。

- Linux: 日本語を持つフォント (CI では `fonts-noto-cjk`) と、デスクトップと
  同じ大きさのフォント設定。チェックボックスの印の位置は日本語の行の
  ascent と行の高さをもとに測るため、欧文へ代替されたり、GTK 組み込みの
  既定 (`Sans 10`) のまま行が低かったりすると成り立ちません。CI は
  `~/.config/gtk-4.0/settings.ini` に `gtk-font-name=Sans 11` を書いています。
- Web: 実ブラウザとそのドライバ。ラベルを押すとチェックが入る、同じ `name` の
  ラジオが排他になる、Flexbox が子を並べるといった挙動はブラウザのもので、
  naui は実装を持ちません。ドキュメントへ載っていない要素はレイアウトされず、
  `click()` を呼んでも `change` が飛ばないため、通知や位置を見るテストは
  `<body>` へ載せてから測ります。

### リリース

`v` で始まるタグを push すると `.github/workflows/release.yml` が動き、4 環境ぶんの
Gallery と counter をビルドして GitHub Release へ添付します。

```sh
git tag v0.3.0
git push origin v0.3.0
```

タグ名は `Cargo.toml` の `[workspace.package]` の `version` と一致している必要が
あり、ずれていればワークフローが止まります。`v0.3.0-rc1` のようにハイフンを
含むタグはプレリリースとして公開されます。

配布物は次の 4 つです。

- `naui-gallery-<タグ>-macos-universal.tar.gz` (Intel / Apple Silicon 共通)
- `naui-gallery-<タグ>-linux-x86_64.tar.gz` (Ubuntu 24.04 でビルド)
- `naui-gallery-<タグ>-windows-x86_64.zip`
- `naui-gallery-<タグ>-web-wasm.tar.gz` (HTTP で配信して開く)

タグを打たずにビルドだけ確かめたいときは、Actions から `Release` を
`workflow_dispatch` で実行してください。Release は作られません。

## 既知の制限

制限事項は環境ごとにまとめています。利用するターゲットの項目を確認してください。

<details>
<summary><strong>共通</strong></summary>

- 対応するのは上記の 43 コンポーネントです。
- `Toolbar` はウィンドウに取り付けるもので、レイアウトの好きな位置には置けません
  (`NSToolbar` が `NSWindow` に付くものであるため)。アイコンは `ToolbarIcon` の
  20 種類からしか選べず、任意の画像は置けません。項目をインデックスで識別する
  都合上、macOS の「ツールバーをカスタマイズ」(利用者による並べ替え) は
  切ってあります。
- 絶対配置はありません。`Stack`、`Grid`、`Spacer` で配置します。
- `SplitView` の区画は 2 つだけで、仕切りも 1 本です。3 つ以上に分けるときは
  入れ子にします。区画をたたむ (幅 0 にして隠す) 指定はありません。
- `List` は 1 列です。単純な行は `ListItem` の `label` / `detail`、複合行は
  `ListRow` に包んだ任意の `Widget` で作れます。行そのものへのクリックは
  `ListRow::on_activate` の 1 つだけで、右クリックや二度押しは受け取れません。
  列が必要なデータ一覧には `Table` を使います。
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
- 別スレッドからの受け渡しで順序が保たれるのは**同じチャネルの中だけ**です。
  チャネルが複数あるとき、チャネルをまたいだ配送の順序は決まっていません。
- チャネルに上限はありません。短い間に多くの値を送っても**値は 1 つも捨てません**が、
  複数の通知が次の描画より先にまとめて処理されるため、**途中の状態は画面に見えず
  最後の状態だけが描かれることがあります**。高頻度の進捗やテレメトリは、
  アプリ側で間引いてください。
- `Sender::send` が返す `Ok` は「値を受け取り、UI への配送が予約された」の意味で、
  配送されたことの保証ではありません。予約の直後に画面が閉じれば実行されません。
- `Tasks::spawn` に渡せるのは値を返さない future だけです。結果は future の中で
  直接ウィジェットへ書きます。返る `Task` を落としてもタスクは止まりません
  (止めるには `Task::cancel` を呼びます)。
- 受信クロージャや future の中で panic しても、その場で止めるだけでアプリは
  巻き戻しません。WinRT / GLib / libdispatch の境界を越えて巻き戻せないため、
  4 環境そろえてこの形にしてあります (panic の内容はいつもどおり標準エラー出力へ
  出ます)。`panic = "abort"` の設定では捕まえられません。

</details>

<details>
<summary><strong>Windows</strong></summary>

- `StackPanel` は主軸の余りを子へ配らないため、`Stack` 内の主軸方向では
  `Fill` と `Spacer` が効きません。代わりに `Grid` の `Track::Fill` を使います。
- 一部の Windows App SDK 環境で異常終了を避けるため、`Tabs` は `TabView` を使わず、
  `Video` / `Audio` の標準再生バーは無効にしています。
- `EditableComboBox` の 1 文字ごとの通知は、`ComboBox` のテンプレートにある
  入力欄 (`EditableText`) の `TextChanged` から拾っています。`ComboBox` 自身は
  文字の変化を表に出さないためです。標準のテンプレートを差し替えて入力欄が
  見つからない場合は、候補の選択と Enter での確定だけが通知されます。
  **候補の一覧は打った文字で絞り込まれません** (`IsTextSearchEnabled` は
  一致する候補へ選択を移すだけです)。打った文字で候補を出す `AutoSuggestBox` は
  `SearchInput` が使っていますが、あちらは候補を「開いて全部見る」
  ことができません。`EditableComboBox` は一覧を開いて選べることを軸にしている
  ので、Fluent の作法どおり `ComboBox` のままにしています。
- `PopupMenu` は `MenuFlyout` です。出す位置・影・角丸・ライトディスミス・
  キーボード操作 (矢印と Esc)・画面端での回り込みはすべて WinUI が持ちます。
  右クリックは `UIElement.ContextFlyout` へ預けるので、長押しとコンテキスト
  キーでも同じメニューが出ます。なお 0.3.0 までの脱出口だった
  `native_element()` は無くなり、**`native_flyout()` に変わりました**
  (受け皿を組み立てなくなったため)。
- `Dialog` は `window.show()` より前には開けません。
- naui の UI 実行環境が終わった後の `Sender::send` は失敗します
  (`DispatcherQueue` が受け付けないため)。
- `List` は `ListView` です。行の見た目 (角丸・淡い塗り・左端のアクセント色の
  インジケーター) は標準テンプレートが持つので、naui は枠の `Border` だけを
  足します (`Table` と `Tree` と同じ色・角丸)。スクロールは `ListView` が自分で
  持ちます。なお 0.3.0 までの脱出口だった `native_list_box()` は無くなり、
  **`native_list_view()` に変わりました**。
- WinUI 3 に `DataGrid` は無い (Community Toolkit のもの) ため、`Table` は
  `ListView` の行を `Grid` にして組み立て、見出しは同じ列定義を持つ別の `Grid` に
  置いて幅をそろえています。行の余白だけは `ListView` の既定ではなく見出しと
  同じ値を書いて、列の位置をそろえています。並べ替えできる見出しは、地色と枠を
  消した `Button` です (WinUI に列見出し用のコントロールが無いため)。
- `Tree` は `TreeView` です。項目は `TreeViewNode` へ写して木のまま渡すので、
  段付け・開閉の山形・キーボード操作・選択の見た目はすべて WinUI が持ちます。
  枠だけは `List` と同じ色と角丸を持たせた `Border` で囲みます。
  `TreeItem::enabled` に当たるものは WinUI に無い (`TreeViewItem` を無効にすると、
  枝では開閉までできなくなる) ため、文字を薄くしたうえで行の中身に押下を
  受け止める覆いをかぶせています。覆いがかかるのは開閉の山形より右だけなので、
  **選べない枝も開閉はできます**。スクロールは `TreeView` が自分で持ちます
  (中の一覧は与えられた高さぶんしか並べないので、`List` のように外側の
  `ScrollViewer` へ預けると伸びずに切れてしまいます)。
- `NumberInput` は `NumberBox` です。増減ボタンは既定では出ないので
  `SpinButtonPlacementMode` を `Inline` にして欄の右へ並べ、範囲・刻み・小数桁は
  `Minimum` / `Maximum` / `SmallChange` / `NumberFormatter` にも書いて、
  上下キーやホイールの動きと表示をそろえています。端に来ると増減ボタンが
  自分で無効になり、PageUp / PageDown は刻みの 10 倍動きます。
  `NumberBox` の 1 文字ごとの通知は、`EditableComboBox` と同じくテンプレートに
  ある入力欄 (`InputBox`) の `TextChanged` から拾っています。`NumberBox` が値を
  決めるのは確定したとき (Enter・欄を離れたとき・増減ボタン・上下キー・
  ホイール) だけだからです。標準のテンプレートを差し替えて入力欄が見つからない
  場合は、確定したときだけ通知されます。
  そのため**打っている間の値を持っているのは naui だけ**で、`NumberBox` の値は
  確定するまで古いままです。読めない文字列を戻す先も増減の基準も `NumberBox` が
  持っている値なので、**表示が読めなくなった時点で受け取り済みの値を渡して**
  います。これをしないと、`12` まで打ってから `12x` にしたときに、巻き戻しも
  増減も古い値から始まってしまいます (`12x` で上矢印を押すと 13 ではなく 2 に
  なる)。値を渡すと `NumberBox` が表示を作り直してしまうので、打っている途中の
  表示 (`-` だけ、`12x`) と選択位置は書き戻しています。読める表示なら
  `NumberBox` が確定と増減のどちらでも先に表示を読むので、渡す必要はありません。
  打っている途中の表示は `NumberBox` が確定に使うのと同じ `NumberFormatter`
  (`INumberParser` でもある) で読みます。小数点や桁区切りは地域設定で変わるので、
  打鍵中と確定とで読み手が違うと通知の出かたがずれるためです。
  **有効数字は 10 桁までです。**`NumberBox` は表示を作る前に値を有効数字 10 桁へ
  丸めます (`SignificantDigits(10)` の丸め器を内部に持っており、`NumberFormatter`
  を差し替えても外せません)。確定すると `NumberBox` はその表示を読み直すので値も
  10 桁へそろい、値と表示がずれたままにはなりませんが、`set_decimals` に 10 桁を
  超える有効数字を求める桁数を渡しても Windows では切れます。
  数字は**右へそろえて**います。`NumberBox` の既定は左寄せですが、macOS の
  `NSTextField` と 0.3.0 までの Windows が右寄せだったので、そちらへ合わせて
  います (`GtkSpinButton` と `<input type="number">` は環境の既定のまま左寄せ
  です)。`NumberBox` 自身の `TextAlignment` は投影元の Windows App SDK にまだ
  無いため、テンプレートの入力欄へ直に書いています。
  `TextInput` と同じく中身に合わせた幅を持たないので幅は `set_sizing` で
  指定しますが、増減ボタンと消去ボタンが並ぶぶん**1 行入力より広めに**取って
  ください (naui のギャラリーは 200 px にしています)。
  なお 0.3.0 までの脱出口だった `native_text_box()` と `native_spin_buttons()`
  は無くなり、**`native_number_box()` に変わりました** (組み立てをやめたため)。
- `Toolbar` は `CommandBar` で、項目は `AppBarButton`、区切りは
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
  なお 0.3.0 までの脱出口だった `native_panel()` は無くなり、
  **`native_command_bar()` に変わりました** (組み立てをやめたため)。
- `Toast` の見た目は `InfoBar` です。ただし `InfoBar` は本来ページの中へ
  並べて使う帯なので、下端の中央へ寄せて中身の層に重ねています。操作ボタンは
  `ActionButton` に置くので、文字の右か下かの並べ方は WinUI が決めます。
  閉じるボタン (×) は出しません (`IsClosable` を切っています)。消えるのは
  時間切れ・操作ボタン・`dismiss` の 3 つで、ほかの 3 環境にも × が無いため
  です。`Dialog` と同じく、`window.show()` より前には出せません。
- `Label` の `TextBlock` は `TextTrimming="CharacterEllipsis"` を持つ XAML から
  生成しています (折り返しの切り替えは `TextWrapping` で行います)。
- 動かせる仕切りを持つコントロールが Windows App SDK に無いため
  (`Microsoft.UI.Xaml.Controls.SplitView` は開閉するナビゲーションのペインで、
  `GridSplitter` は Community Toolkit の側)、`SplitView` は 3 つの列 (行) を
  持つ `Grid` の真ん中に仕切りを置いて組み立てています。仕切りの地色は
  `ControlStrokeColorDefaultBrush` から引くのでテーマに追従します。塗るのは
  境目の 1 px だけ (`BorderThickness`) で、残りは `Background="Transparent"` の
  つかみ代です (`Transparent` は `null` と違って当たり判定が残ります)。ただし
  **仕切りに合わせたカーソル (⇔) は出ません**。カーソルの形を変える
  `UIElement.ProtectedCursor` は派生クラスからしか触れないためです。
  区画の大きさが変わったことは `LayoutUpdated` で拾っています。
- `Toggle` のラベルは `OnContent` と `OffContent` の両方へ同じ文字を入れる
  ので、入り切りで読みは変わりません (WinUI の既定は「オン」「オフ」と
  切り替わる文字です)。
- WinUI 3 の `ColorPicker` はスペクトラムとスライダーを縦に並べた大きな面
  なので、`Button` の `Flyout` へ入れ、ボタンには選んだ色の見本
  (`Border` + `SolidColorBrush`) を出します。組み立てだけは XAML に書いて
  `XamlReader` へ渡します (`Flyout` は投影に入れていないため)。
- `DatePicker` は `DatePickerMode::DateTime` では 2 つを
  `StackPanel` で横に並べます。年 / 月 / 日 の並び順と表記はシステムのロケールに
  従い、暦はグレゴリオ暦に固定しています。`set_range` は WinUI 側へは年の範囲
  (`MinYear` / `MaxYear`) として渡し、月日と時刻の境界は naui 側で端へ寄せます。
  標準テンプレートは英語の長い月名に合わせて月の列だけを広く左寄せにするため、
  3 列を等幅・中央揃えへ直し、最小幅も詰めています。
- `TimePicker` ウィジェットは、`DatePicker` と同じ WinUI の `TimePicker` です。
  WinUI 3 の `TimePicker` に下限・上限は無いので、`set_range` の範囲は naui 側で
  端へ寄せます。
- `SearchInput` は `AutoSuggestBox` です。虫めがねは `QueryIcon="Find"` を
  XAML で渡して作らせ (`SymbolIcon` は投影に入れていないため)、確定
  (`on_search`) は `QuerySubmitted` です。候補の一覧 (`ItemsSource`) は渡さないので、打っても
  候補は出ません。`TextChanged` は `Text` を書き換えたときにも飛ぶため、
  `Reason` がプログラムからの変更なら黙ります (`set_text` は通知しません)。

</details>

<details>
<summary><strong>macOS</strong></summary>

- `Grid` の `Track::Fill` は重みの違いを反映しません。
- 交差軸の `Fill` と Grid セル内の配置は、コンテナへ追加する前に指定してください。
- `Grid` で**結合したマス (span を持つ子) を外しても、結合そのものは残ります**。
  `NSGridView` に結合を解く API が無く、結合をまたぐ行・列も外せません
  (試すと AppKit が例外を投げます)。naui は結合した範囲を**まとめて 1 つのマスと
  して扱う**ので、跡へ 1 マスぶんの子を置いても位置と大きさはふつうのマスと
  変わりません。違いが出るのは、`Fill` の子が結合した範囲まで広がることと、
  範囲内の別の位置へ置くと同じマスなので前の子が外れることの 2 つです。
- `Grid` の**同じマスへ子を重ねて置けません**。後から置いたものが前のものを
  外します (`NSGridCell` は中身を 1 つしか持てないため)。他の 3 環境では
  重ねて置けます。
- `Image` のリモート URL は同期的に読み込むため、ローカルファイルの利用を推奨します。
- `Dialog::open` と `PopupMenu::open_at` は閉じるまで戻りません。
- `DatePicker` は `NSDatePicker` そのものです。欄をクリックするとカレンダーが
  重なって開き (`presentsCalendarOverlay`)、キーボードとステッパーでも編集
  できます。見た目・操作とも AppKit の既定 (SwiftUI の `DatePicker` と同じ
  `textFieldAndStepper`) と変わりません。
- 数値専用のコントロールが AppKit に無いため、`NumberInput` は `NSTextField` と
  `NSStepper` を横に並べて組み立てています (システム設定の数値欄と同じ形)。
- `EditableComboBox` は `NSComboBox` です。打ちかけの文字を候補で補う
  `completes` を入れてあるので、候補の先頭に一致する文字を打つと残りが
  補完されます (そのまま打ち切れば候補にない文字列も残せます)。**候補の一覧
  そのものは絞り込まれません。** AppKit に一覧を絞る仕組みが無いためです。
- `Table` の行の高さは、システムフォントから決めた一定の値です。AppKit に
  求めさせる指定 (`usesAutomaticRowHeights`) は使っていません。それを使うと、
  列を足し引きしたときに AppKit が行へ張る制約が、前の列で外れたセルを指した
  まま有効になり、表が壊れるためです。
- `Table` で幅を指定しなかった列への割り当ては naui が計算します。AppKit の
  列の自動調整は、幅を固定した列があると余りを配りきれず、表の右側が空いた
  ままになるためです。
- `Grid` の `Auto` 行の高さと、`Fill` の子が受け取る取り分も naui が計算します
  (「Rust 側でレイアウト計算を行わない」の**例外**です)。`NSGridView` は行と列の
  大きさを中身から決めますが、**余りをどこへ渡すかは決まっておらず**、hugging
  priority だけでは `Auto` の側へ余白が入ります。そこで `Auto` 行にはセル内容の
  `fittingSize` を行の高さとして渡し、`Fill` の子には「グリッドの大きさ − ほかの
  列が要る幅」まで伸びたいという弱い希望を張ります (固定幅の列はその指定幅、
  それ以外はセル内容の `fittingSize` で見積もります)。**位置と大きさそのものを
  決めるのは Auto Layout** で、naui が frame を置くわけではありません。
- `Toolbar` の区切りは、macOS の作法にならって `NSToolbarSpaceItem`
  (一定幅の空き) になります。区切り線は引かれません。
- `Toolbar` を付けるとウィンドウのタイトル文字は隠れます (macOS の作法)。
  出したままだとタイトルが先頭を占め、項目が右端へ寄ってしまうためです。
  `set_title` の値は残り、`clear_toolbar` で外すと表示も戻ります。
- トーストにあたるコントロールが AppKit に無いため、`Toast` は
  `NSVisualEffectView` (`NSPopover` と同じ材質) をウィンドウの中身へ重ねて
  組み立てています。出す先はいちばん手前のウィンドウで、まだ焦点が
  決まっていないときは最後に作ったウィンドウです。
- `Label::set_wrap(true)` にすると、naui は `NSViewFrameDidChangeNotification`
  を見て `preferredMaxLayoutWidth` を frame の幅へ追従させます。
  `NSTextField` は折り返す幅が決まって初めて高さを返せるためです。
- `SplitView` は `NSSplitView` そのものです。ただし**区画の配り方だけは
  naui が置き換えています** (`splitView:resizeSubviewsWithOldSize:`)。
  `NSSplitView` の既定は大きさが変わったときに**割合**で配るため、そのままでは
  ウィンドウを広げるとサイドバーまで広がってしまうためです。区画の frame を
  naui が直接置く都合上、区画のビューだけは
  `translatesAutoresizingMaskIntoConstraints` を切っていません (中身の配置は
  その frame の中で Auto Layout が行います)。
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
- `List` は `GtkListBox` を `GtkScrolledWindow` へ載せたものです。行のクリック
  (`ListRow::on_activate`) は `GtkListBox` の `row-activated` で受けます
  (`GtkListBoxRow` の `activate` はキーボードの Enter / Space だけの経路です)。
  `SelectionMode::Multiple` では、クリックに付いた Ctrl / Shift を GTK4 に
  読ませるため単発クリックでの確定を切るので、**行のクリックは 2 回押しで
  届きます**。
- `Table` は `GtkColumnView` (`GtkListItemFactory` と `GListModel` を要求する)
  ではなく、`GtkListBox` の行を横並びにして組み立て、列の幅は列ごとの
  `GtkSizeGroup` でそろえています。並べ替えできる見出しは `flat` な
  `GtkButton` で、向きは見出しの文字に付く矢印で表します。
- `Tree` は `GtkTreeExpander` (`GtkListView` 専用) ではなく、`GtkListBox` の
  行と開閉ボタンで組み立てています。
- `Label` の既定 (折り返さない) では `PangoEllipsizeMode::End` を入れています。
  **省略記号を付けると `GtkLabel` の最小幅も下がる**ので、狭いコンテナへ
  入れてもコンテナごと押し広げてしまうことがなくなります。
- **折り返す中身は「幅が決まってから高さが決まる」**ので、naui が中身を包む
  入れ物は `GtkSizeRequestMode` を中身から引き継ぎます。`GtkWidget` の既定は
  `GTK_SIZE_REQUEST_CONSTANT_SIZE` で、そのままだと親が幅を渡さずに高さを
  尋ね、折り返す前の 1 行ぶんしか場所を配りません。
- `SplitView` は `GtkPaned` です。区画の最小の大きさは、アプリが `Sizing` で
  指定する `size_request` とぶつからないよう、区画ごとにかぶせた入れ物のほうに
  持たせています。`GtkPaned` は子の最小より仕切りを寄せられないので、中身が
  大きな最小を申告するときはそちらが勝ちます。
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
- 打ち込めるコンボボックスが GTK4 に無いため、`EditableComboBox` は
  `GtkEntry` と、候補を出す `GtkMenuButton` を `.linked` の `GtkBox` へ
  並べて組み立てています。入力欄を持つ `GtkComboBoxText` と
  `GtkEntryCompletion` は GTK 4.10 で非推奨になり、置き換え先が用意されて
  いないためです。**打った文字での候補の絞り込みはありません** (矢印を押すと
  候補が全部出ます)。
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
- **`<span>` の既定は折り返す**ため、`Label` には naui が
  `white-space: nowrap` と `text-overflow: ellipsis` を入れて、他の 3 環境の
  既定 (1 行 + 省略記号) へそろえています。折り返したいときは
  `Label::set_wrap(true)` を使います。
- `ListItem::detail` か `ListRow` (任意内容の行) を使うと、`List` は
  `<select>` から `<ul role="listbox">` を使った実装へ切り替わります。
  文字だけの行しか無ければ `<select size>` のままです。`<option>` の内容モデルは
  テキストだけで、要素も改行も置けないためです。**`<select>` を離れたぶん、
  選択・フォーカス・キー操作 (矢印・Home / End・Space) は `Table` と同じく naui が
  足しています**。指している行は `aria-activedescendant` で示し、行そのものを
  押したときはリスト (`<ul tabindex="0">`) へフォーカスを移してキーボードで
  続けられるようにします。行の中に置いたボタンや入力欄はそれ自身のものなので、
  そこへ届いたクリックとキーはリスト操作にしません (チェックボックスの Space が
  行の選択に化けないようにするためです)。枠と選択の色はブラウザのシステム色
  (`Field` / `SelectedItem` / `Highlight` / `GrayText`) のままです。
- 任意内容の行は `role="option"` の中にボタンや入力欄が入るため、**ARIA の
  「option の中に操作できるものを置かない」規定からは外れます**。Tab では行の中の
  コントロールへ届き、それぞれの名前と役割もそのまま読まれますが、読み上げソフトの
  ブラウズモードでは行が 1 つの項目として読まれ、中のコントロールが出ないことが
  あります。`<option>` に要素を置けない以上、任意内容の行を出すにはこの形しか
  ないため、行そのものの操作 (`ListRow::on_activate`) と行内のコントロールの
  両方を残し、どちらの経路でも操作できるようにしています。
- `Tree` の `TreeItem::detail` は、行の高さをそろえるため
  `ラベル — 補助` の形で 1 行に収まります。
- `EditableComboBox` は `<input list>` と `<datalist>` です。**4 環境のうち、
  打った文字で候補が絞り込まれるのは Web だけ**ですが、これはブラウザが行う
  ことで、切ることはできません。絞り込み方 (前方一致か部分一致か)・矢印を
  出すかどうか・一覧の開き方もブラウザごとに異なります。naui は見た目も
  絞り込みも作りません。
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
- 動かせる仕切りで区画を分ける要素が HTML に無いため (`resize` は要素の隅に
  つまみを出すだけです)、`SplitView` は `Toast` と同じく naui が組み立てます。
  仕切りは `<div role="separator">` で、カーソルは `col-resize` / `row-resize`
  に任せています (つまみの絵を naui の CSS で描くことはしません)。見えるのは
  境目の 1 px の線だけで、残りは透明なつかみ代です。線の色はシステムカラーの
  `GrayText` を使います。`ButtonBorder` と `CanvasText` はブラウザによって
  **地色の正反対** (ダークなら白、ライトなら黒) になり、区切り線としては
  強すぎるのに対し、`GrayText` はどちらの配色でも地色と文字色の中間に来る
  ためです。`tabindex="0"` を付けてあるので**キーボードでも動かせます**
  (矢印キーで 10 px ずつ)。
- ページに一時的な通知の標準要素が無いため、`Toast` は `<div role="status">` を
  `position: fixed` で下端の中央へ出します (`Notification` API はページの外へ
  出るもので別物です)。
- タイトルバーが無いため、`Toolbar` はウィンドウ要素の先頭に置かれます。
- ブラウザに標準のアイコンセットが無いため、`Toolbar` のアイコンだけは naui が
  SVG を持ちます (ここだけは OS のものを使いません)。
- ブラウザの制限により、メディアの自動再生が拒否される場合があります。
- **wasm にはスレッドがありません。** `std::thread::spawn` は使えないので、
  `Sender` へ送る側は `Tasks::spawn` で回す future の中に置きます。
  Web Worker は別の JavaScript 実行環境なので、`Sender` が Rust の型として
  `Send` であっても、そこへ渡すことはできません (`postMessage` で橋を架け、
  受けた側で `send` を呼ぶ形になります)。`Tasks` の API 自体は 4 環境で同じなので
  `cfg` の書き分けは要りませんが、**`std::thread::spawn` を含むコードは
  そのままでは Web で動きません**。
- `Tasks::spawn` は microtask キューの上で進みます。譲らない future や、
  自分を起こし続ける future を回すと描画に順番が回りません。
- ブラウザにはアプリの終了が無いため、`Sender::send` が閉塞を理由に失敗することは
  ありません。

</details>

## ライセンス

[MIT](LICENSE-MIT) OR [Apache-2.0](LICENSE-APACHE)
