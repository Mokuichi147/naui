//! Running an app's UI tests on all four platforms.
//!
//! A test case is an `async fn` that receives a [`Ui`], builds widgets, operates
//! them the way a user does (`click`, `select`, `activate` and so on on the widgets)
//! and checks the result. The same case runs on macOS, Linux, Windows and the Web:
//!
//! ```ignore
//! // tests/ui.rs, with `harness = false` for this test target in Cargo.toml
//! use naui::{testing, Orientation, Result, Ui};
//!
//! naui::test_cases!(counts_clicks, sidebar_collapses_when_narrow);
//!
//! async fn counts_clicks(ui: Ui) -> Result<()> {
//!     let window = ui.window("counter", 320.0, 200.0)?;
//!     let (root, label, button) = my_app::build_counter(&ui)?;
//!     window.set_child(&root);
//!     window.show();
//!     button.click();
//!     assert_eq!(label.text(), "1");
//!     window.close();
//!     Ok(())
//! }
//!
//! async fn sidebar_collapses_when_narrow(ui: Ui) -> Result<()> {
//!     let window = my_app::main_window(&ui)?;
//!     window.show();
//!     testing::resize(&ui, &window, 500.0, 400.0).await?;
//!     testing::wait_until(&ui, || my_app::sidebar(&window).is_collapsed()).await?;
//!     window.close();
//!     Ok(())
//! }
//! ```
//!
//! [`test_cases!`](crate::test_cases) writes the entry point for each platform:
//! natively a `main` that starts the UI once, runs the cases in order on its event
//! loop and reports like `cargo test`; on the Web one `#[wasm_bindgen_test]` per
//! case. The waits in this module are futures driven by that event loop, so layout,
//! timers and deferred work progress while a case waits, on the Web as well, where
//! nothing can block.
//!
//! See [Testing apps](crate::guide#testing-apps) for setting up the test target and
//! for what differs between platforms.

use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use naui_core::{Error, Result};

use crate::{Ui, Window};

/// What a test case returns, boxed so that cases of different types fit in one list.
pub type CaseFuture = Pin<Box<dyn Future<Output = Result<()>>>>;

/// A test case: its name and the function that starts it. [`test_cases!`] makes
/// these from `async fn` names.
///
/// [`test_cases!`]: crate::test_cases
pub type Case = (&'static str, fn(Ui) -> CaseFuture);

/// How long [`wait_until`] and [`resize`] wait before giving up.
pub const TIMEOUT: Duration = Duration::from_secs(5);

/// How long [`settle`] lets the event loop run.
pub const SETTLE: Duration = Duration::from_millis(250);

/// How far [`resize`] lets the window's size be off from the one asked for. Rounding
/// to physical pixels can leave it a pixel off (Windows).
const SIZE_SLACK: f64 = 2.0;

/// The interval at which [`wait_until`] checks its condition.
const POLL: Duration = Duration::from_millis(10);

/// Waits until `ready` returns `true`, checking it every few milliseconds while the
/// event loop runs. Fails if it does not hold within [`TIMEOUT`].
///
/// Use it for whatever the platforms deliver from the event loop rather than on
/// the spot: the layout after [`Window::set_size`] or [`Window::show`], adaptive
/// changes such as `Sidebar`'s automatic collapsing and `Stack::set_breakpoint`,
/// [`Popover::dismiss`](crate::Popover::dismiss)'s notification, timers and
/// spawned tasks.
pub async fn wait_until(ui: &Ui, mut ready: impl FnMut() -> bool) -> Result<()> {
    let tasks = ui.tasks();
    let mut waited = Duration::ZERO;
    while !ready() {
        if waited >= TIMEOUT {
            return Err(Error::new(
                "wait in a test",
                format!("the condition did not hold within {} s", TIMEOUT.as_secs()),
            ));
        }
        tasks.sleep(POLL).await;
        waited += POLL;
    }
    Ok(())
}

/// Lets the event loop run for [`SETTLE`], so that layout and the work naui defers
/// to the next iteration have run.
///
/// Use it before checking that something **did not** change (a `Stack` still
/// vertical just below its breakpoint, for example), where there is nothing to
/// [`wait_until`].
pub async fn settle(ui: &Ui) {
    ui.tasks().sleep(SETTLE).await;
}

/// Resizes `window`'s content area from code ([`Window::set_size`]), and waits until
/// the window has that size (give or take 2 pixels of rounding) and its layout has
/// [`settle`]d.
///
/// This is a resize from code, not the user dragging the window's edge. Where a
/// platform reacts to the drag itself (AppKit collapses a `Sidebar` only during such a
/// live resize), naui makes the same change for a size set from code, so the
/// adaptive layout ends up the same.
///
/// Show the window first: adaptive layouts (`Sidebar`'s automatic collapsing,
/// `Stack::set_breakpoint`, `AdaptivePanes`) follow the size the window is laid out
/// at, which a hidden window does not have. Fails if the window does not reach the
/// size within [`TIMEOUT`], for example below its
/// [minimum size](Window::set_min_size) or beyond the screen.
///
/// **On the Web only the width is compared.** A window there is an element whose
/// width [`Window::set_size`] caps (`max-width`) and whose height the page decides,
/// so it cannot be wider than the browser's viewport.
pub async fn resize(ui: &Ui, window: &Window, width: f64, height: f64) -> Result<()> {
    window.set_size(width, height);
    let reached = |(actual_width, actual_height): (f64, f64)| {
        // 論理ピクセルから物理ピクセルへの丸めで 1 ずれることがある (Windows)。
        let height_matches =
            cfg!(target_arch = "wasm32") || (actual_height - height).abs() <= SIZE_SLACK;
        (actual_width - width).abs() <= SIZE_SLACK && height_matches
    };
    let waited = wait_until(ui, || reached(window.size())).await;
    settle(ui).await;
    waited.map_err(|_| {
        let (actual_width, actual_height) = window.size();
        Error::new(
            "resize the window in a test",
            format!(
                "asked for {width} x {height}, but it stayed at {actual_width} x {actual_height}"
            ),
        )
    })
}

/// Runs `cases` in order and reports like `cargo test`. **Native only**; the Web
/// runs each case as its own `#[wasm_bindgen_test]`.
///
/// [`test_cases!`](crate::test_cases) calls it from the `main` it writes. The UI is
/// started once, and every case receives a clone of the same [`Ui`] (WinUI 3 can
/// start only once per process). The arguments are read as `cargo test` reads
/// them: the others filter the cases by name, `--skip <filter>` leaves cases out,
/// `--exact` makes both match whole names, and `--list` lists the cases selected
/// without starting the UI (`--format terse` leaves out the count, as tools such as
/// cargo-nextest expect). There are no ignored cases, so `--ignored` selects none.
/// Other options, and the values of the ones that take a value, are accepted and
/// do nothing.
///
/// A case fails when it returns an error or panics. A panic is reported, and the
/// remaining cases still run. Close the windows a case opens before it returns; a
/// failing case leaves its windows open.
#[cfg(not(target_arch = "wasm32"))]
pub fn run(cases: &[Case]) -> std::process::ExitCode {
    use std::cell::RefCell;
    use std::io::Write;
    use std::panic::{catch_unwind, AssertUnwindSafe};
    use std::rc::Rc;
    use std::task::Poll;

    let options = Options::parse(std::env::args().skip(1));
    let selected: Vec<Case> = cases
        .iter()
        .filter(|(name, _)| options.selects(name))
        .copied()
        .collect();
    let filtered_out = cases.len() - selected.len();

    if options.list {
        for (name, _) in &selected {
            println!("{name}: test");
        }
        if !options.terse {
            println!();
            println!("{}, 0 benchmarks", count(selected.len(), "test"));
        }
        return std::process::ExitCode::SUCCESS;
    }

    println!();
    println!("running {}", count(selected.len(), "test"));
    let failures: Rc<RefCell<Vec<String>>> = Rc::default();
    let passed = Rc::new(std::cell::Cell::new(0usize));
    let started = {
        let failures = failures.clone();
        let passed = passed.clone();
        crate::sys::run_until_done_for_test(move |ui| {
            let ui = Ui(ui.clone());
            async move {
                for (name, case) in selected {
                    print!("test {name} ... ");
                    let _ = std::io::stdout().flush();
                    // panic はここで止めて、残りのケースへ進む。naui の `Tasks` も
                    // poll の panic を止めるが、そうすると残りのケースごと捨てられる。
                    let mut future = case(ui.clone());
                    let outcome = std::future::poll_fn(|cx| {
                        match catch_unwind(AssertUnwindSafe(|| future.as_mut().poll(cx))) {
                            Ok(Poll::Pending) => Poll::Pending,
                            Ok(Poll::Ready(result)) => Poll::Ready(Ok(result)),
                            Err(_) => Poll::Ready(Err(())),
                        }
                    })
                    .await;
                    match outcome {
                        Ok(Ok(())) => {
                            println!("ok");
                            passed.set(passed.get() + 1);
                        }
                        Ok(Err(error)) => {
                            println!("FAILED");
                            failures.borrow_mut().push(format!("{name}: {error}"));
                        }
                        Err(()) => {
                            println!("FAILED");
                            failures.borrow_mut().push(format!("{name}: panicked"));
                        }
                    }
                }
            }
        })
    };

    if let Err(error) = started {
        eprintln!("could not start the UI for testing: {error}");
        return std::process::ExitCode::FAILURE;
    }
    let failures = failures.borrow();
    if !failures.is_empty() {
        println!();
        println!("failures:");
        for failure in failures.iter() {
            println!("    {failure}");
        }
    }
    println!();
    println!(
        "test result: {}. {} passed; {} failed; 0 ignored; 0 measured; {} filtered out",
        if failures.is_empty() { "ok" } else { "FAILED" },
        passed.get(),
        failures.len(),
        filtered_out,
    );
    println!();
    if failures.is_empty() {
        std::process::ExitCode::SUCCESS
    } else {
        std::process::ExitCode::FAILURE
    }
}

/// The arguments of [`run`], read as Rust's own test harness reads them.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug, Default, PartialEq)]
struct Options {
    filters: Vec<String>,
    skips: Vec<String>,
    exact: bool,
    list: bool,
    terse: bool,
    /// `--ignored`: only the ignored cases, of which there are none.
    ignored_only: bool,
}

#[cfg(not(target_arch = "wasm32"))]
impl Options {
    fn parse(args: impl IntoIterator<Item = String>) -> Self {
        // 値を取るオプション。値を名前の絞り込みと取り違えないよう読み飛ばす。
        const WITH_VALUE: &[&str] = &[
            "--color",
            "--format",
            "--logfile",
            "--shuffle-seed",
            "--skip",
            "--test-threads",
            "-Z",
        ];
        let mut options = Self::default();
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            let (flag, inline) = match arg.split_once('=') {
                Some((flag, value)) if flag.starts_with("--") => (flag, Some(value.to_string())),
                _ => (arg.as_str(), None),
            };
            if !flag.starts_with('-') {
                options.filters.push(arg);
                continue;
            }
            let value = if WITH_VALUE.contains(&flag) {
                inline.or_else(|| args.next())
            } else {
                None
            };
            match flag {
                "--exact" => options.exact = true,
                "--list" => options.list = true,
                "--ignored" => options.ignored_only = true,
                "--skip" => options.skips.extend(value),
                "--format" => options.terse = value.as_deref() == Some("terse"),
                _ => {}
            }
        }
        options
    }

    fn selects(&self, name: &str) -> bool {
        let matches = |pattern: &String| {
            if self.exact {
                name == pattern
            } else {
                name.contains(pattern.as_str())
            }
        };
        !self.ignored_only
            && (self.filters.is_empty() || self.filters.iter().any(matches))
            && !self.skips.iter().any(matches)
    }
}

/// "1 test" or "2 tests", as Rust's own test harness writes it.
#[cfg(not(target_arch = "wasm32"))]
fn count(n: usize, noun: &str) -> String {
    if n == 1 {
        format!("1 {noun}")
    } else {
        format!("{n} {noun}s")
    }
}

/// Runs one case in the browser. Called by the `#[wasm_bindgen_test]` functions
/// that [`test_cases!`](crate::test_cases) writes; not meant to be called directly.
#[cfg(target_arch = "wasm32")]
#[doc(hidden)]
pub async fn run_in_browser(name: &str, case: fn(Ui) -> CaseFuture) {
    let mut ui = None;
    crate::sys::run_for_test(|backend| {
        ui = Some(Ui(backend.clone()));
        Ok(())
    })
    .unwrap_or_else(|error| panic!("{name}: could not start the UI for testing: {error}"));
    let ui = ui.expect("run_for_test calls its closure");
    if let Err(error) = case(ui).await {
        panic!("{name}: {error}");
    }
}

/// Writes the entry point that runs the listed test cases on every platform.
///
/// Each name is an `async fn(naui::Ui) -> naui::Result<()>` in the same file. Put it
/// in a test target with `harness = false`:
///
/// ```toml
/// [[test]]
/// name = "ui"
/// harness = false
///
/// # Only for running the tests on the Web.
/// [target.'cfg(target_arch = "wasm32")'.dev-dependencies]
/// wasm-bindgen-test = "0.3"
/// ```
///
/// ```ignore
/// naui::test_cases!(counts_clicks, sidebar_collapses_when_narrow);
/// ```
///
/// Natively it writes a `main` that calls `naui::testing::run`.
/// macOS and GTK touch the UI only from the main thread, and WinUI 3 starts only
/// once per process, so the cases cannot run on the threads of Rust's own test
/// harness. On the Web it writes a `#[wasm_bindgen_test]` per case, run in a
/// browser (`wasm-bindgen-test-runner`), which is why the test target needs
/// `wasm-bindgen-test` there.
#[macro_export]
macro_rules! test_cases {
    ($($case:ident),+ $(,)?) => {
        #[cfg(not(target_arch = "wasm32"))]
        fn main() -> ::std::process::ExitCode {
            $crate::testing::run(&[$(
                (
                    ::core::stringify!($case),
                    (|ui| ::std::boxed::Box::pin($case(ui))) as fn($crate::Ui) -> $crate::testing::CaseFuture,
                ),
            )+])
        }

        #[cfg(target_arch = "wasm32")]
        fn main() {}

        #[cfg(target_arch = "wasm32")]
        mod __naui_test_cases {
            ::wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

            $(
                #[::wasm_bindgen_test::wasm_bindgen_test]
                async fn $case() {
                    $crate::testing::run_in_browser(
                        ::core::stringify!($case),
                        |ui| ::std::boxed::Box::pin(super::$case(ui)),
                    )
                    .await
                }
            )+
        }
    };
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Options {
        Options::parse(args.iter().map(|arg| arg.to_string()))
    }

    #[test]
    fn plain_arguments_filter_by_part_of_the_name() {
        let options = parse(&["side"]);
        assert!(options.selects("sidebar_collapses"));
        assert!(!options.selects("form_lines_up"));
        assert!(parse(&[]).selects("anything"));
    }

    #[test]
    fn exact_matches_whole_names() {
        let options = parse(&["--exact", "side"]);
        assert!(!options.selects("sidebar_collapses"));
        assert!(options.selects("side"));
    }

    #[test]
    fn skip_leaves_cases_out() {
        let options = parse(&["--skip", "form", "--skip=greets"]);
        assert!(options.selects("sidebar_collapses"));
        assert!(!options.selects("form_lines_up"));
        assert!(!options.selects("create_greets_by_name"));
    }

    #[test]
    fn values_of_options_are_not_filters() {
        let options = parse(&["--test-threads", "1", "--format", "pretty", "--color=never"]);
        assert!(options.filters.is_empty());
        assert!(options.selects("anything"));
    }

    #[test]
    fn list_and_terse_format_are_read() {
        let options = parse(&["--list", "--format", "terse"]);
        assert!(options.list);
        assert!(options.terse);
        assert!(!parse(&["--list", "--format=json"]).terse);
    }

    #[test]
    fn ignored_selects_nothing() {
        assert!(!parse(&["--ignored"]).selects("anything"));
        assert!(parse(&["--include-ignored"]).selects("anything"));
    }

    #[test]
    fn counts_are_singular_for_one() {
        assert_eq!(count(0, "test"), "0 tests");
        assert_eq!(count(1, "test"), "1 test");
        assert_eq!(count(6, "test"), "6 tests");
    }
}
