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

#[cfg(target_os = "windows")]
#[path = "winui3/cases.rs"]
mod cases;

fn main() {
    #[cfg(target_os = "windows")]
    cases::run();
    #[cfg(not(target_os = "windows"))]
    println!("naui-windows の統合テストは Windows でだけ走ります");
}
