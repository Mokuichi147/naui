# Development

## Verification

naui has been verified on all four platforms.

| Platform | Environment and method |
| --- | --- |
| macOS | Integration tests using real AppKit controls and the Gallery, run on macOS Golden Gate 27.0.1 |
| Linux | Integration tests and the Gallery, run on Ubuntu 26.04.1 LTS (Wayland) |
| Web | Each Gallery widget operated in a browser |
| Windows | The Gallery operated on a real x64 machine with Windows App SDK 2.3.1. WinUI 3 integration tests run in CI (also run with `self-contained`, and every Gallery page visited on Windows Server 2022 without the runtime installed) |

For platform-specific caveats, see [Known limitations](known-limitations.md).

### Features verified per platform

| Platform | Features verified |
| --- | --- |
| macOS | Run with automated tests on macOS Golden Gate 27.0.1 (156 tests, including the sidebar, combo box, editable combo box, radio group, date picker, time picker, number input, password input, search input, navigation, list, table, tree, toolbar, menu bar, file chooser, pop-up menu, multi-line input, dialog, toast, expander, switch, color picker, split view, drawing surface, label wrapping, label typography, handing values over from other threads, and `spawn`) |
| Web (wasm) | Verified in a browser: navigation; sidebar selection, resizing with the divider, and collapsing; both the `<select>` and `role="listbox"` forms of the list (including row activation and right-click); number input rounding, range and confirmation; password input; editable combo box typing, matching against options, and notifications; file chooser; media display and playback; dialog responses via buttons; toast display, action button, timeout and replacement; expander toggling and notifications; color picker value round trips and notifications; time picker value round trips, range and notifications; table column widths, text alignment, selection, keyboard operation, column replacement and sorting from headers; search input typing and confirmation with Enter (Enter during IME composition is not counted); split view divider dragging, keyboard operation and pushing back at the minimum size; drawing surface pixels and pointers; menu bar opening and closing of headings, shortcuts and attaching to a window; label wrapping and ellipses; label typography; running and cancelling asynchronous work. The switch's toggling and notifications were verified in a browser (it looks like a check box because Chromium 148 does not support the `switch` attribute) |
| Windows | Every widget and navigation operated on a real machine with Windows App SDK 2.3.1 (including the sidebar, toast, expander, switch, color picker, time picker, table, search input, editable combo box, drawing surface, handing values over from other threads, and `spawn`) |
| Linux | Every tab of `gallery` run on Ubuntu 26.04.1 LTS (Wayland) (including the sidebar, toast, expander, switch, color picker, time picker, table, search input, editable combo box, split view, drawing surface, handing values over from other threads, and `spawn`). 146 automated tests against real GTK4 controls (including the switch, color picker, time picker, table, search input, editable combo box, split view, drawing surface, menu bar and label wrapping). Media verified with a real file (H.264 + AAC) through playback, seeking and state changes |

## Crate layout

```text
crates/
  naui-core      Shared value types, channels and tasks
  naui-macos     AppKit backend
  naui-web       DOM backend
  naui-windows   WinUI 3 backend
  naui-winui3    WinRT projection of WinUI 3 (used only by naui-windows)
  naui-gtk       GTK4 / libadwaita backend
  naui           Facade that picks the backend for the target
examples/
  counter        Minimal sample
  i18n           Translating an app with Fluent
  gallery        Demo of every widget, by kind
tools/
  winui3-bindgen Tool that regenerates naui-winui3's projection from .winmd files
```

Backend-specific dependencies are declared per target. For example, a macOS build
never pulls in GTK4 or WinUI 3 dependencies.

WinUI 3 ships in the Windows App SDK and has no projection in the `windows` crate,
so `naui-winui3` carries a projection generated from the `.winmd` files Microsoft
distributes. The generated file (`crates/naui-winui3/src/bindings.rs`) is
committed, so regular builds do not need the `.winmd` files. The steps for adding
types are in `tools/winui3-bindgen/README.md`.

## Tests

Run them with `naui-core`, the backend for the current platform, and `naui`. For
example, on macOS:

```sh
cargo test -p naui-core -p naui-macos -p naui
```

Linux needs a display (Wayland or X11).

```sh
cargo test -p naui-core -p naui-gtk -p naui
```

Windows needs the Windows App SDK runtime and a desktop session
(`crates/naui-windows/tests/winui3.rs`). No WinUI 3 control can be created before
`Application::Start`, and `Application::Start` can be called only once per
process, so the app is started once and every case runs inside it. Operations are
triggered through UI Automation (Invoke / Toggle / SelectionItem), so they go
through the same path as assistive technologies and automation tools.

```sh
cargo test -p naui-core -p naui-winui3 -p naui-windows -p naui
```

With the `self-contained` feature, the same tests run with the runtime bundled
next to the exe. The components are fetched from nuget.org.

```sh
cargo test -p naui-winui3 -p naui-windows --features naui-windows/self-contained
```

The Web backend's tests run **in a real browser** (`crates/naui-web/tests/dom.rs`).
They need the wasm-bindgen CLI at the same version as in `Cargo.lock`, and a
browser driver.

```sh
cargo install wasm-bindgen-cli --version "$(awk '/^name = "wasm-bindgen"$/{getline; gsub(/[",]/,""); print $3; exit}' Cargo.lock)"
CHROMEDRIVER="$(command -v chromedriver)" \
  cargo test --target wasm32-unknown-unknown -p naui-web
```

Passing `GECKODRIVER` runs the same tests in Firefox. Without a driver at hand,
add `NO_HEADLESS=1` and the runner prints the URL it is listening on; open it in
any browser and the results appear on that page.

```sh
NO_HEADLESS=1 cargo test --target wasm32-unknown-unknown -p naui-web
```

API compatibility with other targets can be checked with `cargo check`.

```sh
cargo check --target wasm32-unknown-unknown -p naui
cargo check --target x86_64-pc-windows-msvc -p naui
cargo check --target x86_64-unknown-linux-gnu -p naui
```

`naui`'s types (such as `naui::Button`) wrap the backend's types and forward each
method to the inner value. The same wrappers are compiled for every target, so if
a backend is missing a method or has a mismatched signature, `cargo check` for that
target fails.

## CI

`.github/workflows/ci.yml` checks the four platforms in parallel on pushes and
pull requests.

| Job | Runner | What it does |
| --- | --- | --- |
| Format and lint | ubuntu-latest | `cargo fmt --all --check` and clippy for `naui-core` |
| macOS | macos-latest | Build and AppKit integration tests |
| Windows | windows-latest | Build, clippy, WinUI 3 integration tests with the Windows App SDK runtime installed, and the same tests with the runtime bundled (`self-contained`) |
| Windows (minimum supported Rust) | windows-latest | Builds both the framework-dependent and runtime-bundled variants with `--locked`, using the Rust from `rust-version` in `Cargo.toml` |
| Windows Server 2022 | windows-2022 | Without installing the runtime, launches the bundled Gallery (with and without an embedded manifest) and visits every page (the path for versions before Windows 11 24H2; `.github/scripts/gallery-smoke.ps1`) |
| Linux | ubuntu-latest | Installs GTK4 and runs the integration tests on Xvfb |
| Web | ubuntu-latest | Type checks for the wasm target, DOM tests on Chrome / Firefox, and a wasm build of the Gallery |

`cargo test --workspace` is not used. `crates/naui-gtk/tests/gtk4.rs` has no cfg,
so it breaks on platforms other than Linux. Each job specifies only the packages
for its own platform.

The tests have a few assumptions about the environment.

- Linux: a font with Japanese glyphs (`fonts-noto-cjk` in CI) and font settings of
  the same size as on a desktop. The position of the check box mark is measured
  from the ascent and line height of a Japanese line, so it does not hold if the
  text falls back to a Latin font, or if lines are short because GTK's built-in
  default (`Sans 10`) is left as is. CI writes `gtk-font-name=Sans 11` to
  `~/.config/gtk-4.0/settings.ini`.
- Windows: the Windows App SDK runtime (CI installs
  `WindowsAppRuntimeInstall-x64.exe` with `--quiet`) and a desktop session that can
  create windows. WinUI 3 controls ship in the Windows App SDK, not the Windows
  SDK, so without the runtime not a single one can be created. Tests with the
  `self-contained` feature use the bundled runtime, so no installation is needed
  (network access is needed to fetch the components from nuget.org). Widgets are
  inspected without being shown on screen, so behavior that only appears once
  WinUI's default templates are applied (`TextBox`'s `TextChanged`, UI
  Automation's Value pattern) cannot be observed here. What cannot be verified is
  listed at the top of `crates/naui-windows/tests/winui3.rs`.
- Web: a real browser and its driver. Behavior such as a check box being checked
  when its label is pressed, radios with the same `name` being mutually exclusive,
  and Flexbox laying out children belongs to the browser; naui has no
  implementation of it. Elements not attached to the document are not laid out,
  and calling `click()` on them does not fire `change`, so tests that check
  notifications or positions attach elements to `<body>` before measuring.

## Releases

Pushing a tag starting with `v` runs `.github/workflows/release.yml`, which builds
the Gallery and counter for the four platforms and attaches them to a GitHub
Release.

```sh
git tag v0.3.0
git push origin v0.3.0
```

The tag name must match `version` in `[workspace.package]` of `Cargo.toml`; if
they differ, the workflow stops. Tags containing a hyphen, such as `v0.3.0-rc1`,
are published as pre-releases.

The release assets are as follows. The archives contain the Gallery and counter,
and the packages contain only the Gallery.

| Platform | Archive | Package |
| --- | --- | --- |
| macOS (Apple Silicon) | `naui-gallery-<tag>-macos-arm64.tar.gz` | `naui-gallery-<tag>-macos-arm64.dmg` (contains the `.app`) |
| Windows (Windows App SDK runtime bundled) | `naui-gallery-<tag>-windows-x86_64.zip` | `naui-gallery-<tag>-windows-x86_64.msi`, `naui-gallery-<tag>-windows-x86_64-setup.exe` |
| Linux (built on Ubuntu 24.04) | `naui-gallery-<tag>-linux-x86_64.tar.gz` | `naui-gallery-<tag>-linux-x86_64.deb` |
| Web (serve over HTTP and open) | `naui-gallery-<tag>-web-wasm.tar.gz` | |

How the packages are built is in `examples/gallery/packaging/`. The `.dmg` is
built by `build-dmg.sh`, the `.msi` (WiX v5) and `-setup.exe` (Inno Setup 6) by
`build-installers.ps1`, and the `.deb` from `[package.metadata.deb]` (cargo-deb) in
`examples/gallery/Cargo.toml`. None of them are code-signed (macOS is only ad-hoc
signed). Windows's `.msi` and `-setup.exe` use the same folder and shortcuts, so
install only one of them. If the other is installed, the installer stops.

To check only the build without tagging, run `Release` from Actions via
`workflow_dispatch`. No Release is created.
