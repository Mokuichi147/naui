//! Behavior checks against real WinUI 3 controls.
//!
//! Native-side operations (UI Automation's Invoke / Toggle / Value and so on) are
//! actually triggered to check that they reach Rust closures and that the WinUI-side
//! state changes.
//!
//! WinUI 3 cannot create a single control before `Application::Start`, and
//! `Application::Start` can be called only once per process. So `harness = false` is
//! set, the app is started once, and every case runs in order inside it. This is
//! where it differs from the macOS version, which recreates the app per case.
//!
//! Running it requires the Windows App SDK runtime
//! (`crates/naui-windows/src/sdk.rs`).
//!
//! ## What cannot be verified here
//!
//! Widgets are only created, not shown on screen, so WinUI's default templates are
//! not applied. Behavior that relies on templates cannot be seen here.
//!
//! - `TextBox`'s `TextChanged`. It does not fire when the text is rewritten
//!   (`on_change` is covered by the macOS / GTK / Web integration tests, and on
//!   Windows by running the Gallery).
//! - `TextBox`'s Value pattern. `GetPattern` returns empty, so typing is replaced by
//!   rewriting `TextBox.Text`.
//!
//! On the other hand, `Click`, `Checked`, `Toggled` and `SelectionChanged` fire even
//! without templates, so these are checked all the way to the notification reaching
//! the Rust closure.

#[cfg(target_os = "windows")]
#[path = "winui3/automation.rs"]
mod automation;
#[cfg(target_os = "windows")]
#[path = "winui3/cases.rs"]
mod cases;

fn main() {
    #[cfg(target_os = "windows")]
    cases::run();
    #[cfg(not(target_os = "windows"))]
    println!("naui-windows の統合テストは Windows でだけ走ります");
}
