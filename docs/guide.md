# 使い方

[README](../README.md) のクイックスタートの続きです。全ウィジェットの使用例は
[`examples/gallery`](../examples/gallery/src/lib.rs) にあります。

## ネイティブと Web の入口を共通化する

ネイティブと Web の両方を提供するアプリでは `entry!` を使うと、
`#[wasm_bindgen(start)]` を含む入口を共通化できます。

```rust,ignore
naui::entry!(Settings::new("naui gallery"), build);
```

## 後からウィジェットを作る

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

## 後からウィジェットを外す

置いたものを減らす側も、コンテナごとに手段があります。

| 変えたいもの | 使う API |
| --- | --- |
| `Stack` の子 | `insert` / `remove` / `clear` |
| `Grid` のマス | `replace` / `remove` / `clear` |
| `Tabs` のタブ | `remove_tab` / `clear` |
| 一覧・表・ツリー・ナビゲーション | `set_items` / `set_rows` で丸ごと置き換える |
| 中身が 1 つのもの (`Scroll`、`Expander`、`SplitView`、`Window`) | `set_child` などで差し替える |
| ウィンドウに取り付けたもの (`Toolbar`、`MenuBar`、`Sidebar`) | `clear_toolbar` / `clear_menu_bar` / `clear_sidebar` |

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

外したウィジェットは、ハンドルをすべて手放したときに解放されます。ただし、
**ウィジェットの通知 (`on_change` や `on_focus_change` など) のクロージャが
そのウィジェット自身のハンドルを捕まえていると、循環して解放されません。**
通知の引数で足りるならそれを使い、足りなければ中身を `Rc` などで別に持ちます
([フォーカスの出入り](#フォーカスの出入り)の例を参照)。

## テーマ

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

## 配置とサイズ

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

## 機能別の補足

全ウィジェットの基本的な使用例は
[`examples/gallery`](../examples/gallery/src/lib.rs) にあります。ここでは、共通 API
だけでは意図や動作が分かりにくい機能を補足します。

### ウィンドウの位置と状態

最大化・最小化・全画面は `Window::set_state` で頼み、いまの状態は
`Window::state` で `WindowState` として読みます。位置は `set_position` /
`position`、利用者が縁をつかんで大きさを変えられるかどうかは `set_resizable` /
`is_resizable` です。

```rust
let window = ui.window("ビューア", 800.0, 600.0)?;
window.set_position(80.0, 60.0); // Linux と Web では何もしない
window.set_state(WindowState::Maximized); // 表示するときに最大化する
window.set_resizable(true);
window.on_state_change(|state| println!("いまは {state:?}"));
window.show();

if let Some((x, y)) = window.position() {
    println!("左上は ({x}, {y})"); // Linux と Web では None
}
```

位置は**主画面の左上を原点に、右と下へ数えた論理ピクセル**で、タイトルバーを
含むウィンドウの枠の左上です。最大化・最小化・全画面の間の `position` は
**通常の状態へ戻ったときの位置**を返し、`set_position` は状態をそのままにして
その戻る先を決めます (見えている枠は動きません)。閉じる前に `position` と `state`
を保存しておき、次に開くときに両方を当てれば、最大化していたウィンドウも元の
配置へ戻ります。画面に出す前の `set_state` は覚えておき、`show` で当てます。
状態の変化は非同期のことが多い (macOS の全画面はアニメーションの後、
Linux はコンポジタが応えた後) ので、変わったことは `on_state_change` で受け取ります。
利用者がタイトルバーのボタンや Esc で変えたときも呼ばれます。

できることは環境で違います。

| | Windows | macOS | Linux | Web |
| --- | --- | --- | --- | --- |
| 位置 | ○ | ○ | × (いつも `None`) | × (いつも `None`) |
| 最大化・最小化 | ○ | ○ | ○ (Wayland では最小化を知れない) | × |
| 全画面 | ○ | ○ | ○ | ○ (利用者の操作の中からだけ) |
| 大きさを変えられるか | ○ | ○ | ○ | × (値を覚えるだけ) |

Linux の位置は、GTK4 にウィンドウの位置を扱う API が無いためです (Wayland では
アプリが位置を知る仕組みそのものが無く、GTK4 は X11 でも外しました)。Web は
Fullscreen API だけを使い、ページ全体を全画面にします。ブラウザは利用者の操作の
中からしか全画面にさせないので、ボタンの `on_click` などから呼びます。詳しくは
[既知の制限](known-limitations.md) を参照してください。

### 折りたたみ

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

### ラベルの折り返し

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

### ラベルの文字づかい

`Label` の大きさと色は `set_style` と `set_color` で変えられます。渡すのは
**段階と役割**で、級数や色の値ではありません。naui はそれを、その環境が標準で
持っている見出しの段階と意味づけされた色へ写します (`ToolbarIcon` と同じ
考え方です)。

```rust
let title = ui.label("設定")?;
title.set_style(TextStyle::Title);

let note = ui.label("保存されていない変更があります")?;
note.set_style(TextStyle::Caption);
note.set_color(TextColor::Warning);
```

実際の級数と色を決めるのは OS なので、**文字を大きくする設定・アクセント
カラー・ライト / ダークの切り替えにそのまま追従します**。

| 段階 | Windows | macOS | Linux | Web |
| --- | --- | --- | --- | --- |
| `LargeTitle` | `TitleLargeTextBlockStyle` | `NSFontTextStyleLargeTitle` | `.title-1` | `2em` / 700 |
| `Title` | `TitleTextBlockStyle` | `NSFontTextStyleTitle1` | `.title-2` | `1.5em` / 700 |
| `Subtitle` | `SubtitleTextBlockStyle` | `NSFontTextStyleTitle3` | `.title-3` | `1.25em` / 600 |
| `Heading` | `BodyStrongTextBlockStyle` | `NSFontTextStyleHeadline` | `.heading` | `1em` / 700 |
| `Body` (既定) | `BodyTextBlockStyle` | `NSFontTextStyleBody` | (既定) | (既定) |
| `Caption` | `CaptionTextBlockStyle` | `NSFontTextStyleSubheadline` | `.caption` | `0.85em` |

| 役割 | Windows | macOS | Linux | Web |
| --- | --- | --- | --- | --- |
| `Default` (既定) | (既定) | `labelColor` | (既定) | (既定) |
| `Secondary` | `TextFillColorSecondaryBrush` | `secondaryLabelColor` | `.dim-label` | `GrayText` |
| `Accent` | `AccentTextFillColorPrimaryBrush` | `controlAccentColor` | `.accent` | `AccentColor` |
| `Success` | `SystemFillColorSuccessBrush` | `systemGreenColor` | `.success` | naui が決める |
| `Warning` | `SystemFillColorCautionBrush` | `systemOrangeColor` | `.warning` | naui が決める |
| `Danger` | `SystemFillColorCriticalBrush` | `systemRedColor` | `.error` | naui が決める |

Web だけは事情が違います。**ブラウザには見出しの段階そのものが無い**ので、
折り返しと同じく naui が CSS の相対値で他の 3 環境へそろえています。色も、
`Secondary` と `Accent` は CSS のシステム色を使いますが、成功・注意・危険に
あたるシステム色は無いため、そこだけ naui が `light-dark()` で決めています。
また **Chromium 系は `AccentColor` に OS のアクセントカラーを出さず、灰色を
返します** (Firefox と Safari は OS の色を返します)。

### 区画の分割

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

### 数値とパスワードの入力

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

### Enter での確定

`TextInput`、`PasswordInput`、`NumberInput` は、Enter で確定したときに
`on_submit` を呼びます。フォームの送信やログインのように、打ち終えたところで
動かしたい処理に使います。

```rust
let name = ui.text_input("")?;
name.on_change(|text| println!("入力中: {text}")); // 打つたび
name.on_submit(|text| println!("{text} で登録する")); // Enter で確定したとき
```

**IME で変換している間の Enter (変換の確定) では呼ばれません。** 読み取り専用の
`TextInput` と、`on_key_down` が Enter に `Handled` を返したときも呼ばれません。

`NumberInput` は、打ちかけの表示を値へ読み込んでから (読めなければ元の値へ
戻してから)、確定した値で呼びます。値が変わったなら `on_change` が先に呼ばれます。

`TextArea` の Enter は改行なので `on_submit` を持ちません。Enter で送信したい
ときは `on_key_down` で拾います。`SearchInput` では同じ通知を `on_search` と
呼びます。

### フォーカスの出入り

文字入力のウィジェット (`TextInput`、`TextArea`、`PasswordInput`、`SearchInput`、
`NumberInput`、`EditableComboBox`) は、フォーカスが入ったとき (`true`) と
出たとき (`false`) に `on_focus_change` を呼びます。欄を離れたときの検証や
保存に使います。

```rust
use std::cell::RefCell;
use std::rc::Rc;

let mail = ui.text_input("")?;
// 中身は on_change で受け取っておく (mail 自身は捕まえない)。
let text = Rc::new(RefCell::new(String::new()));
mail.on_change({
    let text = text.clone();
    move |value| *text.borrow_mut() = value.to_owned()
});
mail.on_focus_change(move |focused| {
    if !focused && !text.borrow().contains('@') {
        println!("メールアドレスを確かめてください");
    }
});
```

**通知のクロージャに、その欄自身のハンドルを捕まえないでください。**
欄が通知を持ち、通知が欄を持つ循環になるので、画面から外して手放しても
解放されません ([後からウィジェットを外す](#後からウィジェットを外す)を参照)。
`set_text` で書き換えたときは `on_change` が呼ばれないので、共有の中身も
一緒に書き換えます。

**ウィンドウの中でフォーカスが移ったときだけ**呼ばれます。別のアプリへ
切り替えたとき (Web ではタブやブラウザのウィンドウを切り替えたとき) は、
ウィンドウの中のフォーカスは変わらないので呼ばれません。数値入力の欄と
増減のボタンのように、ウィジェットの中で移っても呼ばれません。

通知先を置いた時点でフォーカスがすでにその中にあっても、入ったことは
知らせません (そこから出たときの `false` は届きます)。ウィンドウを出したときに
最初の入力欄へフォーカスを置くかは環境が決めます (macOS、Linux、Windows は
置くことがあり、Web は置きません)。

### 検索の入力

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

### 入り切りの切り替え

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

### 選択入力

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

### 日付と時刻

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

### 時刻の選択

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

### 色の選択

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

### リスト

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

### テーブル

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

`TableColumn::width` を指定しない列だけで、余った幅を分け合います。**列の幅を
ドラッグで変えられるのは macOS だけ**で (`NSTableView` が持つ機能)、他の環境には
対応する標準コントロールがありません。

#### セルにウィジェットを置く

`TableRow` は文字だけの表向けの簡便 API です。セルにボタンやチェックボックス、
アイコンを置くときは `set_row_builder` に「行数」と「その行を組み立てる関数」を
渡します。呼ばれるのは**画面に出ている行だけ**なので、行数が多くても開くのは
速いままです。

```rust
use naui::{TableCells, TableColumn};

table.set_columns(&TableColumn::list(["都市", "操作"]));

let ui = ui.clone(); // 行はコールバックの中で組み立てる
table.set_row_builder(cities.len(), move |index| {
    let open = ui.button("開く")?;
    open.on_click(move || println!("{index} 行目を開く"));
    // セルは列と同じ順に並べる。文字とウィジェットを混ぜてよい。
    let cells = TableCells::new().text(&cities[index]).cell(&open);
    // 行の中のボタンだけを使う行にする (行そのものは選ばせない)。
    Ok(cells.selectable(false))
});
```

行の中のウィジェットは、組み立てるたびに作り直してかまいません。行ごとの状態は
アプリのデータ側に持ち、関数の中でそこから作ります。中身が変わったら `refresh`
で組み立て直します。

**組み立てる関数の中でアプリのデータを借りる (`RefCell::borrow`) なら、`refresh` や
`set_row_builder` を呼ぶ前に借用を返しておきます。** これらは見えている行をその場で
組み立て直すので、`borrow_mut` を持ったまま呼ぶと二重借用で落ちます。並べ替えの
通知はこの形になりやすいところです (`examples/gallery` の「行が多い表」が例です)。

`TableCells::on_activate` は、行のセルや余白が押されたときだけ呼ばれます。
**ボタン・チェックボックス・入力欄を直接押したときは呼ばれない**ので、行の処理と
中のコントロールの処理が二重に起きません (`ListRow::on_activate` と同じ扱いです)。

#### 行が多い表

行数が 200 を超えると、どの環境でも**画面に出ている範囲 (と、その少し前後) だけ**
を組み立てます。`set_rows` に数十万行を渡しても、`set_row_builder` に大きな
`count` を渡しても、開くのにかかる時間と使うメモリは画面の大きさで決まります。
スクロールバーの長さと位置は全行分のままです (見えていない行の分は、上下の
詰め物が高さを持ちます)。

そのために、**行の高さはどれも同じ**とみなします。既定では組み立てた行から
測りますが、セルにボタンや画像を置いて高さが変わるときは `set_row_height` で
決めておくと、スクロールバーの長さが実際とずれません。

選択はインデックスで覚えているので、画面の外にある行も選べます
(`set_selection`) し、`selection` にも出てきます。絞っている表では、ネイティブの
コントロールへ届くのが組み立ててある行の選択だけなので、**画面の外の選択を残すか
どうかは押された修飾キーで決めます**。⌘ / Ctrl の足し引きは残し、ふつうのクリックと
Shift の範囲選びは選び直しとして落とします (macOS は `NSTableView` が全行の選択を
持つので、この判断そのものが要りません)。

`set_row_builder` で組み立てる行では、まだ作っていない行が選べるかどうかを naui は
知りません。既定では「選べる」とみなすので、画面の外の行に `selectable(false)` を
使うなら、同じ判断を `set_row_selectable` にも渡しておきます。

```rust
table.set_row_builder(locked.len(), move |index| {
    Ok(TableCells::new()
        .text(format!("行 {index}"))
        .selectable(!rows[index]))
});
// 画面の外の行についても set_selection が同じ判断をできるようにする。
table.set_row_selectable(move |index| !locked[index]);
```

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

`set_row_builder` で行を組み立てている表では、行数が変わらないので `refresh` を
呼びます (アプリのデータを並べ替えてから)。行を絞っていても並べ替えの扱いは同じで、
指標は見出しに出るのでスクロールで組み立て直しても消えません。

`set_sort` で通知せずに指標だけを動かせます (起動時の既定の並び順を見せる
とき)。`sort()` でいまの指定を読めます。

行数が多いときは、`set_sizing` で表示領域の高さを決めておきます
(中身の高さでは決まりません)。

### ツリー

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

### ファイルの保存

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

### 描画面

グラフ・図形・可視化のように**アプリが自分で描くもの**は `Canvas` に描きます。
描く内容は `on_draw` に渡すクロージャの中で `Painter` へ書き、描き直したいときは
`redraw` を呼びます。面の大きさが変わったときは naui が描き直します。

```rust
use naui::{Color, Point, Rect, Sizing};

let chart = ui.canvas()?;
chart.set_sizing(Sizing::fill()); // 中身から大きさは決まらない
chart.on_draw(|painter| {
    let bar = Rect::new(10.0, 10.0, 40.0, painter.height() - 20.0);
    painter.fill_rect(bar, Color::rgb(0x33, 0x66, 0xff));
    painter.text("売上", Point::new(10.0, painter.height() - 16.0), 12.0, Color::BLACK);
});
chart.on_pointer(|event| println!("{:?} at {:?}", event.phase, event.point));
chart.redraw();
```

`Painter` は描画命令を**記録するだけ**で、画素に落とすのはその環境の 2D API の
仕事です (naui はラスタライズもフォントの計量も持ちません)。座標は左上原点・
右と下が正の論理ピクセルで、高解像度の倍率は環境が掛けます。

| 環境 | 描く実体 |
| --- | --- |
| Windows | `Canvas` に置いた XAML の `Path` と `TextBlock` (`XamlReader` で組み直す) |
| macOS | `NSView` の `drawRect:` で `NSBezierPath` と `NSString` の描画 |
| Linux | `GtkDrawingArea` の cairo と Pango |
| Web | `<canvas>` の 2D コンテキスト |

語彙は 4 環境でそろう最小限にしぼっています。

| 命令 | 内容 |
| --- | --- |
| `fill_rect` / `stroke_rect` | 矩形の塗りと線 |
| `fill_circle` / `stroke_circle` | 円の塗りと線 |
| `line` / `polyline` / `fill_polygon` | 直線・折れ線・多角形 |
| `fill_path` / `stroke_path` | 任意の `Path` (直線と 3 次ベジェ。2 次ベジェ・楕円・弧はベジェへ直して積む) |
| `text` | 1 行の文字。`at` は**上端**で、横は寄せ方に従う |
| `set_opacity` / `set_dash` / `set_text_align` | 以後の命令に効く状態 (透け具合・破線・文字の寄せ方) |

文字の書体はその環境の標準の UI フォントで、`size` は文字の大きさ (論理
ピクセル) です。**文字の幅を測る API はありません** (計量を持たないため)。

`redraw` は**その場では描きません**。次の描画のとき (macOS / Linux はネイティブの
描画パス、Web は次のフレーム、Windows は `DispatcherQueue` の次の順番) に
`on_draw` が呼ばれます。続けて何度呼んでも描くのは 1 回で、`on_draw` の中から
呼んでも再入しません。

`on_pointer` にはポインター (マウス・タッチ・ペン) の押下・移動・解放が、面の
左上を原点にした位置で届きます (`PointerEvent`)。移動は押していない間 (ホバー) も
届き、押している間は面の外へ出ても届きます (捕捉します)。押下と解放には、どの
ボタンかが `PointerButton` で付きます (`Primary`・`Secondary`・`Middle`・`Back`・
`Forward`。タッチとペン先は `Primary`)。ボタンの番号は環境ごとに違う (GTK は右が
3、Web は中が 1) ので、意味でそろえてあります。修飾キーはまだ区別しません。

```rust
use naui::{EventResponse, PointerButton, PointerPhase, WHEEL_STEP};

chart.on_pointer(|event| {
    if event.phase == PointerPhase::Down && event.button == Some(PointerButton::Secondary) {
        println!("右クリック: {:?}", event.point); // PopupMenu::open_at などへ
    }
});

let mut scale = 1.0_f64;
chart.on_wheel(move |event| {
    scale *= 1.2_f64.powf(-event.wheel_y / WHEEL_STEP); // 1 刻みで 1.2 倍
    EventResponse::Handled // ページはスクロールさせない
});
```

ホイール (トラックパッドの 2 本指のスクロールも含む) は `on_wheel` に、
`PointerPhase::Wheel` の `PointerEvent` として届きます。量は `wheel_x` /
`wheel_y` で、**論理ピクセルにそろえてあります**。正は右・下へ読み進める向き
(ページのスクロールと同じ) で、OS の「ナチュラルなスクロール」の設定は反映した
あとの向きです。

| 環境 | ネイティブの量 | naui に届く量 |
| --- | --- | --- |
| Windows | `MouseWheelDelta` (1 刻みで 120、縦は奥へ回すと正) | 120 を `WHEEL_STEP` に直し、縦の符号を反転 |
| macOS | `scrollingDeltaX` / `Y` (トラックパッドはピクセル、ホイールは行。中身が動く向きで正) | ピクセルはそのまま、行は `WHEEL_STEP` 倍。符号を反転 |
| Linux | `GtkEventControllerScroll` (単位が `SURFACE` ならピクセル、`WHEEL` なら刻み) | ピクセルはそのまま、刻みは `WHEEL_STEP` 倍 |
| Web | `WheelEvent` (`deltaMode` がピクセル・行・ページ) | ピクセルはそのまま、行は `WHEEL_STEP` の 1/3 倍、ページは面の大きさ倍 |

刻みのあるホイールの 1 刻みは `WHEEL_STEP` (48 ピクセル。Windows の既定の
「1 刻みで 3 行」に 1 行 16 ピクセルを掛けた値) になります。ただし Web で
ブラウザがピクセルで渡すときはブラウザの量のままなので、1 刻みの量はブラウザに
よって違います。拡大縮小のように「1 刻みでどれだけ」を決めるときは、上の例の
ように量を `WHEEL_STEP` で割った値を指数に使うと、トラックパッドの細かい量でも
滑らかに効きます。

`on_wheel` は `EventResponse` を返します。`Handled` なら外側のスクロール
(`Scroll` やページ) へ回さず、`Continue` なら外側がいつもどおりスクロール
します。`on_wheel` を付けていない面は、ホイールを外側へ回します。

Web では、右クリックで出るブラウザのコンテキストメニューを面の上では出しません
(他の 3 環境の描画面にも無いため)。

Windows だけは**画素を描く面が Windows App SDK にありません** (Win2D は別
パッケージ、`SwapChainPanel` は DirectX を自前で回す口)。そこで XAML が持つ図形の
要素 `Path` と `TextBlock` を `Canvas` へ置く形にしていて、描き直しのたびに面を
丸ごと組み直します。命令が数千を超える図では、他の 3 環境より描き直しに時間が
かかります。

### トースト

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

### ツールバー

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

### メニューバー (OS のアプリケーションメニュー)

「ファイル」「編集」のような**見出しと項目の 2 段**のメニューは `MenuBar` を
使います。`Toolbar` と同じくレイアウトには置かず、`Window::set_menu_bar` で
ウィンドウに取り付けます。

見出し 1 つが `MenuSpec`、その中の 1 行が `MenuItem` です。押されると
**(見出しのインデックス, 区切り線を含めた項目のインデックス)** で通知されます。

```rust
let menu_bar = ui.menu_bar()?;
menu_bar.set_menus(&[
    MenuSpec::new("ファイル", [
        MenuItem::new("新規").shortcut(MenuShortcut::new('n')),
        MenuItem::new("開く").shortcut(MenuShortcut::new('o')),
        MenuItem::separator(),
        MenuItem::new("保存").shortcut(MenuShortcut::new('s')).enabled(false),
    ]),
    MenuSpec::new("表示", ["拡大", "縮小"]), // 文字列の並びでもよい
]);
menu_bar.on_activate(|menu, item| println!("{menu} 番目の {item} 番目"));
menu_bar.set_item_enabled(0, 3, true);
window.set_menu_bar(&menu_bar);
```

**macOS ではメニューバーはアプリに 1 つ**で、画面の上端に出ます
(`NSApplication.mainMenu`)。`Window::set_menu_bar` はどのウィンドウから呼んでも
アプリ全体に効きます。ほかの 3 環境ではウィンドウの上端 (タイトルバーの下) に
敷かれます。

ショートカットは `MenuShortcut` で指定します。主修飾キーは macOS だけ ⌘ で、
Windows・Linux・Web では Ctrl になります。⇧ と ⌥ / Alt は `shift` / `alt` で
足します。

| 環境 | 押されたキーを受け取るのは | 項目の右端の表示 |
| --- | --- | --- |
| macOS | AppKit (`keyEquivalent`) | AppKit |
| Linux | GTK4 (`GtkApplication` のアクセラレータ) | GTK4 |
| Windows | naui (根の `KeyDown`) | naui (`Ctrl+S` の形) |
| Web | naui (`document` の `keydown`) | naui (`Ctrl+S` の形) |

macOS では ⌘C / ⌘V などがメインメニューのキー等価として配送されるため、
メニューが無いと**テキスト入力で貼り付けができなくなります**。そのため naui は
アプリ名のメニュー (先頭) と「編集」メニュー (末尾) を必ず用意し、アプリが
渡した見出しをその間へ並べます。`Window::clear_menu_bar` で外すと、この 2 つ
だけの既定のメニューへ戻ります。

### サイドバー

「システム設定」や Finder の左側のような、ウィンドウの高さいっぱいの
サイドバーは `Sidebar` を使います。`Toolbar` と同じくレイアウトには置かず、
`Window::set_sidebar` でウィンドウに取り付けます。取り付けると、`set_child`
の子はサイドバーの右の区画に置かれます。

項目は `SidebarItem` (文字とアイコン)、まとまりは `SidebarSection` です。
選ばれた項目は**まとまりをまたいだ通し番号**で通知されます (見出しは数えません)。

```rust
let sidebar = ui.sidebar()?;
sidebar.set_sections(&[
    SidebarSection::untitled([
        SidebarItem::new("一般").icon(ToolbarIcon::Settings),
        SidebarItem::new("情報").icon(ToolbarIcon::Info),
    ]),
    SidebarSection::new("場所", ["書類", "ダウンロード"]), // 見出し付き
]);
sidebar.on_select(|index| println!("{index} 番目が選ばれました")); // 0〜3
sidebar.set_selected(0);     // 通知せずに選ぶ
sidebar.set_width(240.0);    // 既定は 220
window.set_sidebar(&sidebar);
sidebar.set_collapsed(true); // 閉じる (項目と選択は残る)
sidebar.on_collapse(|collapsed| println!("利用者が開閉しました: {collapsed}"));
sidebar.on_resize(|width| println!("利用者が幅を {width} にしました"));
```

| 環境 | 実体 | 位置 |
| --- | --- | --- |
| macOS | `NSSplitViewController` のサイドバー項目 + ソースリストの `NSTableView` | タイトルバーの下まで伸びる (macOS 26 では浮いたガラス) |
| Linux | `GtkPaned` + `.navigation-sidebar` の `GtkListBox` | サイドバーと中身がそれぞれヘッダーバーを持つ |
| Windows | `NavigationView` (`PaneDisplayMode` は Left) | タイトルバー (とメニューバー) の下から |
| Web | naui の `SplitView` の中に `<aside>` + `<nav>` + `<button>` | ウィンドウ要素の中、メニューバー・ツールバーの下 |

**4 環境とも、利用者は仕切りをドラッグして幅を変えられます** (下限は
`SIDEBAR_MIN_WIDTH` の 140)。変えると `on_resize` が呼ばれます (`set_width`
では呼ばれません)。macOS と Linux は標準の仕切り (`NSSplitView` / `GtkPaned`)、
Windows と Web は `SplitView` と同じ 6 px のつかみ代です。Linux の
`AdwOverlaySplitView` は幅を変えられないため、`GtkPaned` で分けています。

開閉は各環境の標準のサイドバーボタンで利用者も行え、そのときは
`on_collapse` が呼ばれます (`set_collapsed` では呼ばれません)。ボタンの
位置は 4 環境でそろえてあり、**ウィンドウの上端の左 (ツールバーと同じ高さ)** に
来ます。開閉してもボタンは動かず、中身も上下しません。

| 環境 | 開閉ボタン | 閉じたときの姿 |
| --- | --- | --- |
| macOS | ツールバー先頭のサイドバーボタン (ツールバーが無ければボタンだけのツールバーを付ける) | 区画ごと隠れる |
| Linux | ヘッダーバーの左端の `sidebar-show-symbolic` | 区画ごと隠れる |
| Windows | `NavigationView` のペインを畳むボタン | アイコンだけの細い帯 |
| Web | ツールバーの行の先頭の `<button aria-expanded>` (ツールバーが無ければボタンだけの行) | 区画ごと隠れる |

macOS では付けている間だけウィンドウに `fullSizeContentView` が付きますが、
子の上端はタイトルバーを避けるので中身の見え方は変わりません。アイコンは
`ToolbarIcon` と同じ種類を使います。

libadwaita 1.9 にはサイドバーの一覧まで持つ `AdwSidebar` がありますが、naui が
対象にしている 1.5 には無いため、Linux は 1.5 までの推奨どおり
`GtkListBox` で組んでいます。Web はブラウザにサイドバーのコントロールが
無いので、`Menu` と同じく標準要素とブラウザ既定のボタンで組み立てます。

### 別スレッドと非同期

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
