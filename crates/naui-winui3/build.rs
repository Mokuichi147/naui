//! Only with the `self-contained` feature, downloads the Windows App SDK runtime
//! from nuget.org and places it in the exe's output directory.
//!
//! Without the feature it does nothing. naui then, as before, looks for and uses the
//! framework package installed on the OS at run time.
//!
//! The master copy of the assembled bundle is kept in `OUT_DIR`, and from there it
//! is placed where exes go:
//!
//! - `<target-dir>/[<triple>/]<profile>/` and `examples/` below it (where cargo run
//!   and sample exes go)
//! - `<build-dir>/[<triple>/]<profile>/deps/` (where test exes go)
//! - `<profile>/naui-winappsdk/`, collecting what goes next to the exe for
//!   distribution
//!
//! Cargo does not tell build scripts the target-dir. `OUT_DIR` is inside the
//! build-dir, and by default the two are the same place, so it is derived from
//! there. When the build-dir is separate, the target-dir is derived from environment
//! variables ([`artifact_profile`]).
//!
//! What is placed can disappear, for example when CI restores a cache
//! (Swatinem/rust-cache deletes the top of the profile directory, keeping only the
//! dependency crates' build results). The placed manifests are made a condition for
//! rerunning, and if they are gone, they are placed again from the master copy in
//! `OUT_DIR`.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=build");

    #[cfg(feature = "self-contained")]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        if let Err(message) = bundle::run() {
            eprintln!("error: Windows App SDK ランタイムを同梱できません: {message}");
            std::process::exit(1);
        }
    }
}

#[cfg(feature = "self-contained")]
#[allow(dead_code)]
#[path = "build/packages.rs"]
mod packages;

#[cfg(feature = "self-contained")]
#[path = "build/manifest.rs"]
mod manifest;

#[cfg(feature = "self-contained")]
mod bundle {
    use std::env;
    use std::fs;
    use std::io::{Cursor, Read};
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use sha2::{Digest, Sha512};

    use crate::packages::{
        Component, COMPONENTS, EXCLUDED_FILES, LICENSE_FILE, MANIFEST_FILE, NOTICE_FILE,
        RESOURCES_FILE, RESOURCES_SOURCE, WINDOWS_APP_SDK,
    };

    /// The location of `.nupkg`s to use instead of downloading. For builds that cannot
    /// reach the network. Place them named `<id>.<version>.nupkg` (lowercase).
    const PACKAGES_ENV: &str = "NAUI_WINAPPSDK_PACKAGES";

    /// The environment variable that states the exe output directory (target-dir)
    /// explicitly. For when Cargo's build-dir is separate and the target-dir cannot be
    /// derived from environment variables.
    const TARGET_DIR_ENV: &str = "NAUI_WINAPPSDK_TARGET_DIR";

    /// The name of the directory collecting files for distribution (under
    /// `<profile>/`).
    const BUNDLE_DIR: &str = "naui-winappsdk";

    /// The modification time given to the created manifests (2000-01-01, in UNIX
    /// seconds).
    const PLACED_MTIME: u64 = 946_684_800;

    pub(crate) fn run() -> Result<(), String> {
        for name in [
            PACKAGES_ENV,
            TARGET_DIR_ENV,
            "CARGO_BUILD_BUILD_DIR",
            "CARGO_TARGET_DIR",
            "CARGO_BUILD_TARGET_DIR",
        ] {
            println!("cargo:rerun-if-env-changed={name}");
        }

        let arch = match env::var("CARGO_CFG_TARGET_ARCH").as_deref() {
            Ok("x86_64") => "x64",
            Ok("aarch64") => "arm64",
            Ok("x86") => "x86",
            Ok("arm64ec") => "arm64ec",
            other => return Err(format!("対応していないアーキテクチャです: {other:?}")),
        };

        // OUT_DIR は <build-dir>/[<triple>/]<profile>/build/naui-winui3-<hash>/out。
        let out_dir = PathBuf::from(env::var("OUT_DIR").map_err(|e| e.to_string())?);
        let (Some(build_dir), Some(build_profile)) =
            (out_dir.ancestors().nth(2), out_dir.ancestors().nth(3))
        else {
            return Err(format!(
                "OUT_DIR から出力先を割り出せません: {}",
                out_dir.display()
            ));
        };
        let artifact_profile = artifact_profile(build_profile)?;

        // 正本と印は OUT_DIR に置く。依存クレートのビルド結果として扱われ、
        // CI のキャッシュの復元でも残る。
        let staging = out_dir.join(BUNDLE_DIR);
        let stamp_path = out_dir.join(format!("{BUNDLE_DIR}.stamp"));
        let stamp = stamp(arch);
        let fresh = fs::read_to_string(&stamp_path).ok().as_deref() != Some(stamp.as_str())
            || !staging.join(MANIFEST_FILE).is_file();
        if fresh {
            assemble(
                &staging,
                &build_dir.join(format!("{BUNDLE_DIR}-packages")),
                arch,
            )?;
            fs::write(&stamp_path, &stamp).map_err(|e| describe(&stamp_path, e))?;
        }

        // build-dir 側の naui-winappsdk/ は、アプリの build.rs が自分の OUT_DIR
        // からマニフェストを探して exe に埋め込めるようにするため
        // (docs/windows-self-contained.md の「マニフェストを exe に埋め込む」)。
        let mut destinations: Vec<PathBuf> = Vec::new();
        for destination in [
            artifact_profile.clone(),
            artifact_profile.join("examples"),
            artifact_profile.join(BUNDLE_DIR),
            build_profile.join("deps"),
            build_profile.join(BUNDLE_DIR),
        ] {
            // 既定では target-dir と build-dir が同じなので、重なる。
            if !destinations.contains(&destination) {
                destinations.push(destination);
            }
        }
        for destination in &destinations {
            place(&staging, destination, fresh)?;
        }

        // 並べたものが消えたら (CI のキャッシュの復元など)、走り直して並べ直す。
        // Cargo は、ここに挙げたファイルが無くなっても再実行する。
        for watched in [
            artifact_profile.join(MANIFEST_FILE),
            artifact_profile.join(BUNDLE_DIR).join(MANIFEST_FILE),
        ] {
            println!("cargo:rerun-if-changed={}", watched.display());
        }
        Ok(())
    }

    /// The profile directory where exes go (`<target-dir>/[<triple>/]<profile>`).
    ///
    /// Cargo does not tell build scripts the target-dir. By default it is the same as
    /// the build-dir, so `build_profile`, derived from `OUT_DIR`, is used as is. When
    /// the build-dir is separate (`CARGO_BUILD_BUILD_DIR`), the target-dir is derived
    /// from environment variables, and if it cannot be, the build stops. A config file's
    /// `build.build-dir` is invisible to build scripts, so in that case naui reports the
    /// missing bundle as an error at run time.
    fn artifact_profile(build_profile: &Path) -> Result<PathBuf, String> {
        // build-dir からの相対 ([<triple>/]<profile>)。--target を付けたときは
        // profile の上に対象のトリプルの階層が入る。
        let profile = build_profile
            .file_name()
            .ok_or_else(|| format!("profile が分かりません: {}", build_profile.display()))?;
        let mut relative = PathBuf::new();
        if let (Some(parent), Ok(target)) = (build_profile.parent(), env::var("TARGET")) {
            if parent.file_name() == Some(target.as_ref()) {
                relative.push(target);
            }
        }
        relative.push(profile);

        if let Some(root) = env::var_os(TARGET_DIR_ENV) {
            let root = PathBuf::from(root);
            if !root.is_absolute() {
                return Err(format!(
                    "{TARGET_DIR_ENV} は絶対パスで指定してください: {}",
                    root.display()
                ));
            }
            return Ok(root.join(relative));
        }
        if env::var_os("CARGO_BUILD_BUILD_DIR").is_some() {
            for name in ["CARGO_TARGET_DIR", "CARGO_BUILD_TARGET_DIR"] {
                if let Some(root) = env::var_os(name).map(PathBuf::from) {
                    if root.is_absolute() {
                        return Ok(root.join(relative));
                    }
                }
            }
            return Err(format!(
                "Cargo の build-dir (CARGO_BUILD_BUILD_DIR) を分けているため、exe の出力先が\
                 分かりません。target-dir を {TARGET_DIR_ENV} (または CARGO_TARGET_DIR) に\
                 絶対パスで指定してください"
            ));
        }
        Ok(build_profile.to_path_buf())
    }

    /// A marker for telling whether the contents changed. Besides the component versions
    /// and the architecture, it is also rebuilt when the assembling code itself (how
    /// manifests are made and so on) changes.
    fn stamp(arch: &str) -> String {
        let mut stamp = format!("Windows App SDK {WINDOWS_APP_SDK} {arch}\n");
        for component in COMPONENTS {
            stamp.push_str(&format!("{} {}\n", component.id, component.version));
        }
        let code = Sha512::new()
            .chain_update(include_str!("build.rs"))
            .chain_update(include_str!("build/packages.rs"))
            .chain_update(include_str!("build/manifest.rs"))
            .finalize();
        stamp.push_str(&hex(&code));
        stamp.push('\n');
        stamp
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    /// Extracts the components, adds the manifest and resources, and collects them in
    /// `bundle`.
    fn assemble(bundle: &Path, cache: &Path, arch: &str) -> Result<(), String> {
        if bundle.exists() {
            fs::remove_dir_all(bundle).map_err(|e| {
                format!(
                    "{} を消せません。同梱物を使う exe が動いていれば終了してください: {e}",
                    bundle.display()
                )
            })?;
        }
        fs::create_dir_all(bundle).map_err(|e| describe(bundle, e))?;

        let native = format!("runtimes-framework/win-{arch}/native/");
        let mut fragments = Vec::new();
        let mut licenses = Vec::new();
        let mut notices = Vec::new();
        for component in COMPONENTS {
            let bytes = obtain(component, cache)?;
            let fragments_before = fragments.len();
            let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
                .map_err(|e| format!("{} を開けません: {e}", file_name(component)))?;
            for index in 0..archive.len() {
                let mut entry = archive.by_index(index).map_err(|e| e.to_string())?;
                let name = entry.name().to_string();
                if name == "runtimes-framework/package.appxfragment" {
                    fragments.push(read_text(&mut entry)?);
                    continue;
                }
                if name == "license.txt" {
                    collect(&mut licenses, component, read_text(&mut entry)?);
                    continue;
                }
                if name == "NOTICE.txt" {
                    collect(&mut notices, component, read_text(&mut entry)?);
                    continue;
                }
                let Some(relative) = name.strip_prefix(&native) else {
                    continue;
                };
                if relative.is_empty()
                    || relative.ends_with('/')
                    || EXCLUDED_FILES.contains(&relative)
                {
                    continue;
                }
                // 展開先から出ていく名前は受け付けない。
                if relative
                    .split('/')
                    .any(|part| part == ".." || part.is_empty())
                {
                    return Err(format!(
                        "{} に不正な名前があります: {name}",
                        file_name(component)
                    ));
                }
                let target = bundle.join(relative);
                if let Some(parent) = target.parent() {
                    fs::create_dir_all(parent).map_err(|e| describe(parent, e))?;
                }
                let mut file = fs::File::create(&target).map_err(|e| describe(&target, e))?;
                std::io::copy(&mut entry, &mut file).map_err(|e| describe(&target, e))?;
            }
            if component.has_fragment && fragments.len() != fragments_before + 1 {
                return Err(format!(
                    "{} に package.appxfragment がありません",
                    file_name(component)
                ));
            }
        }

        let fragments: Vec<&str> = fragments.iter().map(String::as_str).collect();
        let manifest = crate::manifest::generate(&fragments)?;
        let manifest_path = bundle.join(MANIFEST_FILE);
        fs::write(&manifest_path, manifest).map_err(|e| describe(&manifest_path, e))?;
        // 並べた先のマニフェストは再実行の条件 (rerun-if-changed) にしている。
        // Cargo はこのビルドスクリプトが走り始めた時刻より新しいと「変わった」と
        // みなすので、作った直後のままだと次のビルドでもう一度走り、naui-winui3
        // を作り直してしまう。中身は同じなので、時刻を過去へ戻しておく。
        set_modified(
            &manifest_path,
            UNIX_EPOCH + Duration::from_secs(PLACED_MTIME),
        )?;

        let resources = bundle.join(RESOURCES_FILE);
        fs::copy(bundle.join(RESOURCES_SOURCE), &resources).map_err(|e| describe(&resources, e))?;

        if licenses.is_empty() {
            return Err("部品のどれにも license.txt がありません".to_string());
        }
        for (file, texts) in [(LICENSE_FILE, &licenses), (NOTICE_FILE, &notices)] {
            if texts.is_empty() {
                continue;
            }
            let path = bundle.join(file);
            fs::write(&path, join_texts(texts)).map_err(|e| describe(&path, e))?;
        }
        Ok(())
    }

    /// Reads text inside a `.nupkg`. A leading BOM is dropped.
    fn read_text(entry: &mut impl Read) -> Result<String, String> {
        let mut text = String::new();
        entry.read_to_string(&mut text).map_err(|e| e.to_string())?;
        Ok(text.trim_start_matches('\u{feff}').to_string())
    }

    /// Collects the text per component. Identical contents are merged into one, with
    /// their sources appended.
    fn collect(texts: &mut Vec<(Vec<String>, String)>, component: &Component, text: String) {
        let source = format!("{} {}", component.id, component.version);
        match texts.iter_mut().find(|(_, existing)| *existing == text) {
            Some((sources, _)) => sources.push(source),
            None => texts.push((vec![source], text)),
        }
    }

    /// Turns the collected texts into one file, with headings saying which component
    /// each came from.
    fn join_texts(texts: &[(Vec<String>, String)]) -> String {
        texts
            .iter()
            .map(|(sources, text)| {
                let mut out = String::new();
                for source in sources {
                    out.push_str(&format!("# {source}\n"));
                }
                out.push('\n');
                out.push_str(text.trim_end());
                out.push('\n');
                out
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn file_name(component: &Component) -> String {
        format!("{}.{}.nupkg", component.id, component.version)
    }

    /// Obtains a `.nupkg`'s contents. It tries the specified location, the cache and
    /// nuget.org in that order, and whatever it gets is used only after verifying its
    /// SHA-512.
    fn obtain(component: &Component, cache: &Path) -> Result<Vec<u8>, String> {
        let name = file_name(component);

        if let Some(directory) = env::var_os(PACKAGES_ENV) {
            let path = Path::new(&directory).join(&name);
            let bytes = fs::read(&path).map_err(|e| describe(&path, e))?;
            verify(component, &bytes).map_err(|e| format!("{}: {e}", path.display()))?;
            return Ok(bytes);
        }

        let cached = cache.join(&name);
        if let Ok(bytes) = fs::read(&cached) {
            if verify(component, &bytes).is_ok() {
                return Ok(bytes);
            }
            let _ = fs::remove_file(&cached);
        }

        if env::var("CARGO_NET_OFFLINE").as_deref() == Ok("true") {
            return Err(format!(
                "オフラインのため {name} を取得できません。{PACKAGES_ENV} に .nupkg の置き場を指定してください"
            ));
        }

        fs::create_dir_all(cache).map_err(|e| describe(cache, e))?;
        let url = format!(
            "https://api.nuget.org/v3-flatcontainer/{id}/{version}/{name}",
            id = component.id,
            version = component.version,
        );
        let partial = cache.join(format!("{name}.partial"));
        let status = Command::new("curl")
            .args(["--fail", "--silent", "--show-error", "--location", "--retry", "3", "--output"])
            .arg(&partial)
            .arg(&url)
            .status()
            .map_err(|e| {
                format!("curl を起動できません ({e})。{PACKAGES_ENV} に .nupkg の置き場を指定すれば取得せずに済みます")
            })?;
        if !status.success() {
            let _ = fs::remove_file(&partial);
            return Err(format!("{url} を取得できません ({status})"));
        }
        let bytes = fs::read(&partial).map_err(|e| describe(&partial, e))?;
        if let Err(e) = verify(component, &bytes) {
            let _ = fs::remove_file(&partial);
            return Err(format!("{url}: {e}"));
        }
        fs::rename(&partial, &cached).map_err(|e| describe(&cached, e))?;
        Ok(bytes)
    }

    fn verify(component: &Component, bytes: &[u8]) -> Result<(), String> {
        let actual = hex(&Sha512::digest(bytes));
        if actual == component.sha512 {
            Ok(())
        } else {
            Err(format!(
                "SHA-512 が一致しません (期待 {}、実際 {actual})",
                component.sha512
            ))
        }
    }

    /// Places the contents of `bundle` into `destination`.
    ///
    /// It is the same volume, so hard links do; if they cannot be made, it copies. When
    /// the contents have not changed (`replace` is false), files of the same size that
    /// already exist are left alone, because DLLs loaded by a running exe cannot be
    /// replaced.
    fn place(bundle: &Path, destination: &Path, replace: bool) -> Result<(), String> {
        for source in files(bundle)? {
            let relative = source.strip_prefix(bundle).map_err(|e| e.to_string())?;
            let target = destination.join(relative);
            if let Ok(existing) = target.metadata() {
                let same_size = source.metadata().map(|m| m.len()).ok() == Some(existing.len());
                if !replace && same_size {
                    continue;
                }
                fs::remove_file(&target).map_err(|e| {
                    format!(
                        "{} を置き換えられません。同梱物を使う exe が動いていれば終了してください: {e}",
                        target.display()
                    )
                })?;
            }
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).map_err(|e| describe(parent, e))?;
            }
            if fs::hard_link(&source, &target).is_err() {
                fs::copy(&source, &target).map_err(|e| describe(&target, e))?;
                // コピーでは更新時刻が引き継がれないことがある。マニフェストは
                // 再実行の条件にしているので、正本と同じ時刻にそろえる。
                let modified = source
                    .metadata()
                    .and_then(|m| m.modified())
                    .map_err(|e| describe(&source, e))?;
                set_modified(&target, modified)?;
            }
        }
        Ok(())
    }

    fn set_modified(path: &Path, time: SystemTime) -> Result<(), String> {
        fs::File::options()
            .write(true)
            .open(path)
            .and_then(|file| file.set_modified(time))
            .map_err(|e| describe(path, e))
    }

    fn files(directory: &Path) -> Result<Vec<PathBuf>, String> {
        let mut found = Vec::new();
        let mut pending = vec![directory.to_path_buf()];
        while let Some(current) = pending.pop() {
            for entry in fs::read_dir(&current).map_err(|e| describe(&current, e))? {
                let path = entry.map_err(|e| e.to_string())?.path();
                if path.is_dir() {
                    pending.push(path);
                } else {
                    found.push(path);
                }
            }
        }
        Ok(found)
    }

    fn describe(path: &Path, error: std::io::Error) -> String {
        format!("{}: {error}", path.display())
    }
}
