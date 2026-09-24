# gallery

naui の全ウィジェットを種別ごとに試せるデモです。画面は基本、入力、一覧、表、
ナビゲーション、レイアウト、描画、ファイル、メディア、ダイアログ、非同期に
分かれています。操作の結果は、画面の下端にトーストで出ます。

```sh
cargo run -p gallery
```

表示されるコントロールはすべて OS (またはブラウザ) の実ウィジェットです。
macOS なら `NSButton` / `NSTextField`、Web なら `<button>` / `<input>` が
出ています。

## Web で動かす

`web/build.sh` が wasm へビルドして `web/pkg/` へ配置します。wasm ターゲットと、
`Cargo.lock` と同じバージョンの `wasm-bindgen-cli` が要ります (不足していれば
スクリプトが必要なコマンドを案内します)。

```sh
cd examples/gallery/web
./build.sh
python3 -m http.server 8080
```

その後 <http://localhost:8080/> を開いてください。ネイティブと Web で共通の
入口は `naui::entry!` が作っています。
