# Bundling the Windows App SDK runtime

Enabling the `self-contained` feature (off by default) lets you ship the runtime
together with the exe, so users do not need to install it separately.

```toml
[dependencies]
naui = { version = "0.11", features = ["self-contained"] }
```

At build time, the Windows App SDK 2.5.1 components are fetched from nuget.org,
verified with SHA-512, and placed in the exe's output directory. When
distributing, put everything in `target/<profile>/naui-winappsdk/` (about 60 MB,
including license terms and notices) next to the exe.

**If the bundled files are missing, naui fails with an error instead of falling
back to an installed runtime.** This prevents the situation where it works on your
machine and the missing files are first noticed on the user's machine.

To try it with the Gallery, run `cargo run -p gallery --features self-contained`.

## License

- **Enabling this feature means agreeing to the license of each package fetched.**
  naui's build fetches them without asking. The terms and redistribution conditions
  are in each `.nupkg`'s `license.txt` and on each package's page on nuget.org.
- WinUI 2.0.12 through 2.3.2 mistakenly included a license prohibiting
  redistribution
  ([microsoft/WindowsAppSDK#6654](https://github.com/microsoft/WindowsAppSDK/issues/6654)),
  so only the fixed 2.3.6 and later are bundled.

## Constraints

- The files placed are the same as MSBuild's self-contained deployment (excluding
  `DeploymentAgent.exe`).
- Some Windows App SDK features, such as push notifications, are not available when
  bundled (naui does not use them).
- Runtime bug and security fixes are delivered by redistributing the app.
- Keep the path of the folder holding the exe to roughly 200 characters or fewer.
  Launching fails if any file path exceeds 260 characters.
- A `resources.pri` is placed next to the exe, so it cannot coexist with the app's
  own `resources.pri`.
- For offline builds, set the environment variable `NAUI_WINAPPSDK_PACKAGES` to a
  folder containing the four `.nupkg` files. File names are lowercase, like
  `microsoft.windowsappsdk.winui.2.3.9.nupkg`.

## Embedding the manifest in the exe

naui loads the bundled manifest at startup. This works on Windows 10 1903 and later
(verified on Windows Server 2022 and 2025 in CI). On 1809, the Windows App SDK looks
**only at a manifest embedded in the exe**, so embedding is expected to be needed
(unverified). If the runtime cannot be found without embedding, `naui::run` returns
an error suggesting embedding.

A dependency crate cannot embed into the app's exe, so embed it in the app's
`build.rs`. This requires the MSVC linker.

```rust
// build.rs
fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc")
    {
        // The manifest naui's build places in target/<profile>/naui-winappsdk/.
        let out_dir = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
        let manifest = out_dir.ancestors().nth(3).unwrap()
            .join("naui-winappsdk").join("naui-winappsdk.manifest");
        println!("cargo:rustc-link-arg-bins=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg-bins=/MANIFESTINPUT:{}", manifest.display());
    }
}
```

An example of switching bundling with a feature is in
[`examples/gallery/build.rs`](../examples/gallery/build.rs). Embedding also makes it
possible to create WinUI classes from threads other than the one that started naui,
such as a thread pool.

## When build-dir is separate

Cargo does not tell build scripts the exe's output directory (target-dir), so naui
derives it from build-dir. By default they are the same place, so nothing needs to
be done.

If they are separated with `build.build-dir` (`CARGO_BUILD_BUILD_DIR`), set the
environment variable `NAUI_WINAPPSDK_TARGET_DIR` to the target-dir as an absolute
path (if `CARGO_TARGET_DIR` is an absolute path, it is used). If neither is set, the
build stops, but `build.build-dir` in a config file is not visible to build scripts,
so it cannot be stopped, and the app fails at startup with an error saying the
bundled files are missing.

Even if restoring a CI cache (such as `Swatinem/rust-cache`) removes the placed
files, the next build places them again.
