# 既知の制限

制限事項は環境ごとにまとめています。利用するターゲットの項目を確認してください。

## 共通

- 対応するのは [README](../README.md#ウィジェット) の表にある 46 コンポーネントです。
- `Toolbar` はウィンドウに取り付けるもので、レイアウトの好きな位置には置けません
  (`NSToolbar` が `NSWindow` に付くものであるため)。アイコンは `ToolbarIcon` の
  20 種類からしか選べず、任意の画像は置けません。項目をインデックスで識別する
  都合上、macOS の「ツールバーをカスタマイズ」(利用者による並べ替え) は
  切ってあります。
- `MenuBar` も `Toolbar` と同じくウィンドウに取り付けるもので、レイアウトの
  好きな位置には置けません。**入れ子のメニュー (サブメニューのさらに下) と、
  チェックの付く項目はありません。** 見出しと項目の 2 段までです。
  ショートカット (`MenuShortcut`) は**主修飾キー + 英数字 1 文字**の
  組み合わせだけで、主修飾キーは macOS が ⌘、ほかの 3 環境が Ctrl です。
  ファンクションキーや記号のキーは指定できません。
- 絶対配置はありません。`Stack`、`Grid`、`Spacer` で配置します。
- `Canvas` の語彙は塗り・線・1 行の文字だけで、画像の貼り付け・クリップ・
  変形 (回転や拡大) ・グラデーション・文字の幅の計量はありません。中身から
  大きさは決まらないので `set_sizing` で指定します。ポインターの通知は
  ボタンを見分けますが、修飾キー (⇧・Ctrl など) は見ません。トラックパッドの
  ピンチ (拡大縮小のジェスチャー) は拡大縮小としては届きません (Web の
  Chromium 系と Firefox は、Ctrl 付きのホイールとして `on_wheel` へ届きます)。ホイールの 1 刻みの量は
  `WHEEL_STEP` にそろえますが、Web でブラウザがピクセルで渡すときはブラウザの
  量のままです。
- `SplitView` の区画は 2 つだけで、仕切りも 1 本です。3 つ以上に分けるときは
  入れ子にします。区画をたたむ (幅 0 にして隠す) 指定はありません。
- `List` は 1 列です。単純な行は `ListItem` の `label` / `detail`、複合行は
  `ListRow` に包んだ任意の `Widget` で作れます。行そのものへのクリックは
  `ListRow::on_activate` の 1 つだけで、右クリックや二度押しは受け取れません。
  列が必要なデータ一覧には `Table` を使います。
- `Table` のセルには、文字列 (`TableRow`) のほかに任意の `Widget`
  (`TableCells` + `set_row_builder`) を置けます。見出しを押しての並べ替えは
  「どの列を、どちら向きに」を通知するところまでで、**行を並べ替えるのは
  アプリの仕事**です。列の幅をドラッグで変えられるのは macOS だけです。
- 行数が 200 を超える `Table` は、画面に出ている範囲だけを組み立てます
  (`set_rows` / `set_row_builder` のどちらでも)。そのため**行の高さはどれも
  同じ**とみなし、必要なら `set_row_height` で決めます。組み立てる行では、
  まだ作っていない行が選べるかどうかを naui は知らないので、既定では
  「選べる」とみなします (`set_row_selectable` で渡せます)。また、絞っている
  表でセルにウィジェットを置く列は、幅を指定しないと**見出しの幅と余りの
  分け合い**だけで決まります (行ごとに中身の幅が違っても列が動かないように
  するためです)。 Web では、行数 × 行の高さがブラウザの扱える要素の高さ
  (Chromium で 3,300 万 px ほど) を超えると、スクロールバーの位置と行の
  対応がずれます (30px の行なら 100 万行あたり)。インデックスでの選択や
  `set_rows` は、それより多くても動きます。
- `Tree` は単一選択で、項目に置けるのは `List` と同じ文字列だけです。
  項目はパス (`&[usize]`) で指し、ドラッグでの並べ替えはできません。
- `Dialog` は同時に 1 つだけで、ボタンは Primary、Secondary、Cancel の最大 3 個です。
- `Toast` も同時に 1 つだけで、新しく出したものが前のものを置き換えます
  (`AdwToastOverlay` の順番待ちも、この形にそろえています)。操作ボタンは 1 個で、
  出る位置は下端の中央に固定です。
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

## Windows

- `StackPanel` は主軸の余りを子へ配らないため、`Stack` 内の主軸方向では
  `Fill` と `Spacer` が効きません。代わりに `Grid` の `Track::Fill` を使います。
- `MenuBar` は WinUI 3 の `MenuBar` ではなく、地色を消した `Button` に
  `MenuFlyout` を預けたものを横に並べて組み立てています。`MenuBar` /
  `MenuBarItem` / `KeyboardAccelerator` は `naui-winui3` の投影に入れていない
  ためです (投影はコミットしてある生成物で、型を増やすと全体を作り直すことに
  なります)。そのぶんショートカットは、ウィンドウの根まで上ってきた `KeyDown`
  を naui が見て呼び出しています。
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
- `Canvas` は `Grid` の中に XAML の `Canvas` を置き、`on_draw` の命令を
  `Path` (パスの記法 `M` / `L` / `C` / `Z`) と `TextBlock` にして `XamlReader`
  で組み立てます。描き直しのたびに面を丸ごと差し替えるので、命令が数千を
  超える図では他の 3 環境より時間がかかります。破線は XAML の決まりどおり
  **線の太さを 1 とする単位**へ直して渡しています。中央・右寄せの文字は、
  読み込んだ `TextBlock` を測って `Canvas.Left` を書き直します。
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

## macOS

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
- `MenuBar` はウィンドウではなくアプリに付きます (`NSApplication.mainMenu`)。
  **naui はアプリ名のメニューと「編集」メニューを必ず残す**ので、アプリが渡した
  見出しはその 2 つに挟まれて並びます。⌘C / ⌘V がメインメニューのキー等価として
  配送される以上、これを外すとテキスト入力で貼り付けができなくなるためです。
  並びを完全に決めたいときは `MenuBar::native_menu` から `NSMenu` を取り出します。
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

## Linux

- `Grid` の `Track::Fill` は重みの違いを反映しません。
- `Fit::None` は GTK4 の `SCALE_DOWN` に対応するため、「原寸」ではなく
  「拡大しない」動作になります。
- テーマはウィンドウ単位ではなくアプリ全体へ適用されます。
- `MenuBar` は `GtkPopoverMenuBar` で、ヘッダーバーの**下**に入ります。GNOME は
  メニューバーよりハンバーガーメニューを勧めますが、naui は 4 環境で同じ API を
  出すため、GTK4 が持つメニューバーをそのまま使います。項目は `GMenu` のモデルと
  `GSimpleAction` の組で表し、ショートカットは `GtkApplication` のアクセラレータ
  へ登録します (**メニューを開かなくても効きます**)。アクセラレータの表は
  アプリ全体で 1 つなので、登録するのはウィンドウへ取り付けている間だけで、
  `clear_menu_bar` で外すと登録も外します。操作の名前空間は
  メニューバーごとに分けてあるので、複数のウィンドウで同じショートカットを
  使っても互いに上書きせず、フォーカスのあるウィンドウの項目だけが呼ばれます。
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
- `Label::set_style` は libadwaita のスタイルクラス (`.title-1` など) を当てる
  だけで、級数と太さを決めるのはテーマです。**libadwaita は `.title-2` と
  `.title-3` に同じ級数を与え、太さだけを変えます**。そのため
  `TextStyle::Title` と `TextStyle::Subtitle` は太さで見分ける段階になります
  (`Heading` と `Body` も同じ関係です)。ここで naui が級数を決めることはしません。
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

## Web

- `Window` は OS のウィンドウではなく、`<body>` 直下の要素と
  `document.title` で表現されます。
- ブラウザには OS のアプリケーションメニューが無いため、`MenuBar` は
  WAI-ARIA の役割 (`role="menubar"` / `"menu"` / `"menuitem"`) を付けた要素で
  組み立てています。ショートカットも `document` の `keydown` を naui が見張って
  突き合わせ、押されたときは `preventDefault()` でブラウザの既定 (⌘S など) を
  止めます。**主修飾キーは Ctrl と ⌘ のどちらでも通します** (macOS の
  ブラウザでは ⌘ が主修飾キーになるため)。項目の右端の `Ctrl+S` という表示も
  naui が添えています。`document` への購読はウィンドウへ取り付けている間だけ
  張り、閉じた (隠した) ウィンドウのメニューバーはキーに反応しません。ページに
  ウィンドウが複数あるときは、キーが上がってきたウィンドウのメニューバーだけが
  受け取ります。
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
- ブラウザに見出しの段階 (type ramp) が無いため、`Label::set_style` の大きさ
  だけは naui が CSS の相対値 (`em`) で他の 3 環境へそろえています。相対値
  なので、ブラウザとユーザーが決めた基準の文字サイズには追従します。
- `Label::set_color` は CSS のシステム色を使いますが、成功・注意・危険に
  あたるシステム色がブラウザに無いため、その 3 つだけ naui が `light-dark()`
  で色を決めています。また **Chromium 系は `AccentColor` に OS のアクセント
  カラーを出さず灰色を返す**ので、`TextColor::Accent` は Firefox と Safari で
  しか OS の色になりません (naui が別の色を当てて代えることはしません)。
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
- `Dialog` と `PopupMenu` の Esc は、`<dialog>` の `cancel` イベントではなく
  **`keydown` の既定動作を止めてから naui が閉じます**。Safari はウィンドウが
  全画面のとき、ページが既定動作を止めない限り Esc をまず全画面の解除に使い、
  ダイアログを閉じるのに 2 回押すことになるためです。変換中 (`isComposing`) の
  Esc は入力を取り消すためのものなので、そのまま IME へ渡します。
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
