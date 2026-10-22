# 開発

## 動作確認

4 環境とも動作確認済みです。

| 環境 | 確認した環境と方法 |
| --- | --- |
| macOS | macOS Golden Gate 27.0.1 で、AppKit の実コントロールを使った統合テストと Gallery を実行 |
| Linux | Ubuntu 26.04.1 LTS (Wayland) で統合テストと Gallery を実行 |
| Web | ブラウザ上で Gallery の各ウィジェットを操作 |
| Windows | Windows App SDK 2.3.1 の x64 実機で Gallery を操作。WinUI 3 の統合テストを CI で実行 (`self-contained` でも実行し、ランタイム未導入の Windows Server 2022 で Gallery の全ページを巡回) |

プラットフォーム固有の注意点は[既知の制限](known-limitations.md)を参照してください。

### 環境ごとに確認した機能

| 環境 | 確認した機能 |
| --- | --- |
| macOS | macOS Golden Gate 27.0.1 で実行・自動テストあり (サイドバー・コンボボックス・自由入力コンボボックス・ラジオグループ・日付ピッカー・時刻ピッカー・数値入力・パスワード入力・検索入力・ナビゲーション・リスト・テーブル・ツリー・ツールバー・メニューバー・ファイル選択・ポップアップメニュー・複数行入力・ダイアログ・トースト・折りたたみ・スイッチ・色ピッカー・分割ビュー・描画面・ラベルの折り返し・ラベルの文字づかい・別スレッドからの受け渡しと `spawn` を含む 156 件) |
| Web (wasm) | ブラウザで実行確認 (ナビゲーション、サイドバーの選択・仕切りでの幅の変更・開閉、リストの `<select>` と `role="listbox"` の両方 (行の起動と右クリックを含む)、数値入力の丸め・範囲・確定、パスワード入力、自由入力コンボボックスの打鍵・候補との一致・通知、ファイル選択、メディアの表示と再生、ダイアログのボタン経由の応答、トーストの表示・操作ボタン・時間切れ・置き換え、折りたたみの開閉と通知、色ピッカーの値の往復と通知、時刻ピッカーの値の往復・範囲・通知、テーブルの列幅・文字揃え・選択・キーボード操作・列の差し替え・見出しからの並べ替え、検索入力の打鍵と Enter での確定 (変換中の Enter は数えない)、分割ビューの仕切りのドラッグ・キーボード操作・最小の大きさでの押し戻し、描画面の画素とポインター、メニューバーの見出しの開閉・ショートカット・ウィンドウへの取り付け、ラベルの折り返しと省略記号、ラベルの文字づかい、非同期処理の実行と中断を確認。スイッチは切り替えと通知をブラウザで確認 (見た目は Chromium 148 で `switch` 属性が未対応のためチェックボックス)) |
| Windows | Windows App SDK 2.3.1 の実機で全ウィジェットとナビゲーションを操作して確認 (サイドバー・トースト・折りたたみ・スイッチ・色ピッカー・時刻ピッカー・テーブル・検索入力・自由入力コンボボックス・描画面・別スレッドからの受け渡しと `spawn` を含む) |
| Linux | Ubuntu 26.04.1 LTS (Wayland) で `gallery` の全タブ (サイドバー・トースト・折りたたみ・スイッチ・色ピッカー・時刻ピッカー・テーブル・検索入力・自由入力コンボボックス・分割ビュー・描画面・別スレッドからの受け渡しと `spawn` を含む) を実行確認。GTK4 の実コントロールに対する自動テスト 146 件 (スイッチ・色ピッカー・時刻ピッカー・テーブル・検索入力・自由入力コンボボックス・分割ビュー・描画面・メニューバー・ラベルの折り返しを含む)。メディアは実ファイル (H.264 + AAC) の再生・シーク・状態変化まで確認 |

## クレート構成

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

## テスト

`naui-core`、現在のプラットフォーム用バックエンド、`naui` を指定して実行します。
たとえば macOS では:

```sh
cargo test -p naui-core -p naui-macos -p naui
```

Linux ではディスプレイ (Wayland または X11) が必要です。

```sh
cargo test -p naui-core -p naui-gtk -p naui
```

Windows では Windows App SDK ランタイムとデスクトップセッションが必要です
(`crates/naui-windows/tests/winui3.rs`)。WinUI 3 のコントロールは
`Application::Start` より前には 1 つも作れず、`Application::Start` は 1 つの
プロセスで 1 回しか呼べないので、アプリを 1 度だけ起こして全ケースをその中で
走らせます。操作は UI オートメーション (Invoke / Toggle / SelectionItem) から
起こすため、支援技術や自動操作ツールと同じ経路を通ります。

```sh
cargo test -p naui-core -p naui-winui3 -p naui-windows -p naui
```

`self-contained` フィーチャーを付けると、同じテストを exe の隣へ同梱した
ランタイムで走らせます。部品は nuget.org から取ってきます。

```sh
cargo test -p naui-winui3 -p naui-windows --features naui-windows/self-contained
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

`naui` の型 (`naui::Button` など) はバックエンドの型を包み、各メソッドを
中身へ渡しています。どのターゲットでも同じ包みをコンパイルするので、
バックエンドにメソッドが欠けていたりシグネチャがずれていたりすると、
そのターゲットの `cargo check` が失敗します。

## CI

`.github/workflows/ci.yml` が push と pull request で 4 環境を並行に検査します。

| ジョブ | ランナー | 内容 |
| --- | --- | --- |
| 整形と lint | ubuntu-latest | `cargo fmt --all --check` と `naui-core` の clippy |
| macOS | macos-latest | ビルドと AppKit の統合テスト |
| Windows | windows-latest | ビルド、clippy、Windows App SDK ランタイムを導入して WinUI 3 の統合テスト、同じテストをランタイム同梱 (`self-contained`) でも実行 |
| Windows (最低対応 Rust) | windows-latest | `Cargo.toml` の `rust-version` の Rust で、フレームワーク依存とランタイム同梱の両方を `--locked` でビルド |
| Windows Server 2022 | windows-2022 | ランタイムを導入せず、同梱した Gallery (マニフェストの埋め込みあり・なし) を起動して全ページを巡回 (Windows 11 24H2 より前の経路。`.github/scripts/gallery-smoke.ps1`) |
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
- Windows: Windows App SDK ランタイム (CI は `WindowsAppRuntimeInstall-x64.exe`
  を `--quiet` で導入) と、ウィンドウを作れるデスクトップセッション。
  WinUI 3 のコントロールは Windows SDK ではなく Windows App SDK に入って
  いるため、ランタイムが無いと 1 つも生成できません。`self-contained`
  フィーチャーを付けたテストは同梱したランタイムを使うので、導入は要りません
  (部品を nuget.org から取ってくるためのネットワークは要ります)。ウィジェットは画面に
  出さずに調べるので、WinUI の既定テンプレートが当たっていないと出ない
  ふるまい (`TextBox` の `TextChanged` や UI オートメーションの Value
  パターン) はここでは見られません。何が確かめられないかは
  `crates/naui-windows/tests/winui3.rs` の冒頭にあります。
- Web: 実ブラウザとそのドライバ。ラベルを押すとチェックが入る、同じ `name` の
  ラジオが排他になる、Flexbox が子を並べるといった挙動はブラウザのもので、
  naui は実装を持ちません。ドキュメントへ載っていない要素はレイアウトされず、
  `click()` を呼んでも `change` が飛ばないため、通知や位置を見るテストは
  `<body>` へ載せてから測ります。

## リリース

`v` で始まるタグを push すると `.github/workflows/release.yml` が動き、4 環境ぶんの
Gallery と counter をビルドして GitHub Release へ添付します。

```sh
git tag v0.3.0
git push origin v0.3.0
```

タグ名は `Cargo.toml` の `[workspace.package]` の `version` と一致している必要が
あり、ずれていればワークフローが止まります。`v0.3.0-rc1` のようにハイフンを
含むタグはプレリリースとして公開されます。

配布物は次のとおりです。書庫には Gallery と counter を、パッケージには Gallery
だけを入れます。

| 環境 | 書庫 | パッケージ |
| --- | --- | --- |
| macOS (Apple Silicon) | `naui-gallery-<タグ>-macos-arm64.tar.gz` | `naui-gallery-<タグ>-macos-arm64.dmg` (`.app` 入り) |
| Windows (Windows App SDK ランタイムを同梱) | `naui-gallery-<タグ>-windows-x86_64.zip` | `naui-gallery-<タグ>-windows-x86_64.msi`、`naui-gallery-<タグ>-windows-x86_64-setup.exe` |
| Linux (Ubuntu 24.04 でビルド) | `naui-gallery-<タグ>-linux-x86_64.tar.gz` | `naui-gallery-<タグ>-linux-x86_64.deb` |
| Web (HTTP で配信して開く) | `naui-gallery-<タグ>-web-wasm.tar.gz` | |

パッケージの作り方は `examples/gallery/packaging/` にあります。`.dmg` は
`build-dmg.sh`、`.msi` (WiX v5) と `-setup.exe` (Inno Setup 6) は
`build-installers.ps1`、`.deb` は `examples/gallery/Cargo.toml` の
`[package.metadata.deb]` (cargo-deb) から作ります。どれもコード署名はしていません
(macOS は ad-hoc 署名だけ)。Windows の `.msi` と `-setup.exe` は同じフォルダーと
ショートカットを使うので、どちらか一方だけを入れます。もう一方が入っていると、
インストーラーは止まります。

タグを打たずにビルドだけ確かめたいときは、Actions から `Release` を
`workflow_dispatch` で実行してください。Release は作られません。
