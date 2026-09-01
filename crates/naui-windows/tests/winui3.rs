//! WinUI 3 の実コントロールに対する動作確認。
//!
//! ネイティブ側の操作 (UI オートメーションの Invoke / Toggle / Value など) を
//! 実際に起こし、Rust のクロージャへ届くこと・WinUI 側の状態が変わることを
//! 確かめる。
//!
//! WinUI 3 のコントロールは `Application::Start` より前には 1 つも作れず、
//! `Application::Start` は 1 プロセスで 1 回しか呼べない。そのため
//! `harness = false` にして、アプリを 1 度だけ起こし、その中で全ケースを
//! 順に走らせる。ケースごとにアプリを作り直す macOS 版とはそこが違う。
//!
//! 実行には Windows App SDK ランタイムが要る (`crates/naui-windows/src/sdk.rs`)。
//!
//! ## ここで確かめられないこと
//!
//! ウィジェットは作るだけで画面には出していないので、WinUI の既定テンプレート
//! が当たっていない。テンプレート頼みのふるまいはここでは見られない。
//!
//! - `TextBox` の `TextChanged`。書き換えても出ない (`on_change` は macOS /
//!   GTK / Web の統合テストと、Windows は Gallery の実行で見ている)。
//! - `TextBox` の Value パターン。`GetPattern` が空を返すので、打ち込みは
//!   `TextBox.Text` の書き換えで代えている。
//!
//! 一方 `Click` `Checked` `Toggled` `SelectionChanged` はテンプレート無しでも
//! 出るので、通知が Rust のクロージャへ届くところまで確かめている。
//!
//! アプリの畳みかたも実アプリとは違う。`Application::Exit` から先の XAML の
//! 後片づけが未パッケージ起動ではアクセス違反で落ちるため、結果を出し切った
//! 時点でプロセスを終えている。

#[cfg(target_os = "windows")]
#[path = "winui3/cases.rs"]
mod cases;

fn main() {
    #[cfg(target_os = "windows")]
    cases::run();
    #[cfg(not(target_os = "windows"))]
    println!("naui-windows の統合テストは Windows でだけ走ります");
}
