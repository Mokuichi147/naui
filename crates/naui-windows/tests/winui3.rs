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
    #[cfg(all(target_os = "windows", feature = "self-contained"))]
    place_bundle_next_to_test_exe();
    #[cfg(target_os = "windows")]
    cases::run();
    #[cfg(not(target_os = "windows"))]
    println!("naui-windows の統合テストは Windows でだけ走ります");
}

/// Places the bundle next to this test exe if it is not there yet.
///
/// From Cargo 1.100 each test exe has its own directory
/// (`<profile>/build/<pkg>/<hash>/out/`), which naui-winui3's build.rs cannot know, so
/// the bundle it placed in `<profile>/naui-winappsdk/` is copied from there. Up to
/// 1.99 build.rs places it in `<profile>/deps/` itself and this does nothing. The same
/// code is in naui-winui3's `src/self_contained.rs`.
#[cfg(all(target_os = "windows", feature = "self-contained"))]
fn place_bundle_next_to_test_exe() {
    use std::fs;
    use std::path::Path;

    fn link_or_copy(source: &Path, target: &Path) {
        if target.exists() {
            fs::remove_file(target).expect("古い同梱物を消せない");
        }
        if fs::hard_link(source, target).is_err() {
            fs::copy(source, target).expect("同梱物を写せない");
        }
    }

    fn place(from: &Path, to: &Path, manifest: &str) {
        for entry in fs::read_dir(from).expect("同梱物を読めない") {
            let entry = entry.expect("同梱物を読めない");
            let (source, target) = (entry.path(), to.join(entry.file_name()));
            if source.is_dir() {
                fs::create_dir_all(&target).expect("同梱物の置き場を作れない");
                place(&source, &target, "");
            } else if entry.file_name() != manifest {
                link_or_copy(&source, &target);
            }
        }
    }

    let manifest = naui_winui3::self_contained::SelfContained::MANIFEST_FILE;
    let exe = std::env::current_exe().expect("テストの exe の場所が分からない");
    let directory = exe.parent().expect("テストの exe の親が無い");
    let bundle = directory
        .ancestors()
        .skip(1)
        .map(|ancestor| ancestor.join("naui-winappsdk"))
        .find(|bundle| bundle.join(manifest).is_file())
        .expect("build.rs が並べた naui-winappsdk/ が上の階層に無い");
    let placed = fs::read(directory.join(manifest)).ok();
    if placed.is_some() && placed == fs::read(bundle.join(manifest)).ok() {
        return;
    }
    place(&bundle, directory, manifest);
    // naui はマニフェストの有無で同梱物があるかを決めるので、最後に置く。
    link_or_copy(&bundle.join(manifest), &directory.join(manifest));
}
