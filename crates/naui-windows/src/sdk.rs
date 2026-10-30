//! Attaching the Windows App SDK runtime.
//!
//! WinUI 3 controls live in the framework package of the **Windows App SDK**, not
//! the Windows SDK. An unpackaged app attaches it to its own process as a "dynamic
//! dependency" before use.
//!
//! The target package differs per version
//! (`Microsoft.WindowsAppRuntime.<version>_8wekyb3d8bbwe`), so instead of pinning
//! one, naui **tries from the newest and uses the first one it can attach**. Even if
//! the user's environment has no 2.x, it works as is if 1.x is installed.
//!
//! The minimum for versioned candidates is 1.3 because `MicaBackdrop` and
//! `Window.SystemBackdrop`, used for the window background, were introduced in 1.3.
//! With older versions, even building a `Window` fails.
//!
//! The unversioned `Microsoft.WindowsAppRuntime.CBS` is the line shipped with the
//! OS, and its generation cannot be told from its name. It is the last resort when
//! nothing else is found. If it is too old to create Mica, only the background is
//! given up ([`crate::window`]).
//!
//! Building with the `self-contained` feature lets the runtime be bundled next to
//! the exe. In that case only the bundled files are used, and no framework package
//! is looked for ([`naui_winui3::self_contained`]). It is an error if the bundled
//! files are not next to the exe. Silently switching to an installed runtime would
//! make it work on the build machine, and forgetting to ship the bundled files would
//! only be noticed on the target machines.

use naui_core::{Error, Result};
use naui_winui3::bootstrap::{PackageDependency, Version};
#[cfg(feature = "self-contained")]
use naui_winui3::self_contained::SelfContained;

/// The attached Windows App SDK runtime.
///
/// Dropping it also detaches it, so it is held until the app ends.
#[derive(Debug)]
pub(crate) enum Runtime {
    /// A framework package installed on the OS.
    #[cfg_attr(feature = "self-contained", allow(dead_code))]
    Framework(#[allow(dead_code)] PackageDependency),
    /// Files bundled next to the exe.
    #[cfg(feature = "self-contained")]
    SelfContained(#[allow(dead_code)] SelfContained),
}

/// The versions to try attaching, newest first.
///
/// The order is exactly the one [`naui_winui3`] holds. It is the same list used when
/// looking for the bootstrap DLL, so one cannot grow without the other.
///
/// The `Cbs` (shipped with the OS) line is a separate line for environments without
/// the regular package of the corresponding version. Versioned ones such as `CBS.2`
/// come right after the same version, and the unversioned `CBS`, whose generation is
/// unknown, comes last.
pub(crate) const CANDIDATES: &[Version] = Version::ALL;

/// Attaches one usable Windows App SDK runtime.
///
/// When built with `self-contained` it uses the bundled files; otherwise it tries
/// framework packages, newest first. Dropping the return value also detaches it, so
/// it is held until the app ends.
pub(crate) fn initialize() -> Result<Runtime> {
    #[cfg(feature = "self-contained")]
    {
        initialize_self_contained()
    }
    #[cfg(not(feature = "self-contained"))]
    {
        initialize_framework()
    }
}

/// Attaches the files bundled next to the exe.
#[cfg(feature = "self-contained")]
fn initialize_self_contained() -> Result<Runtime> {
    const CONTEXT: &str = "initialize the bundled Windows App SDK runtime";
    match SelfContained::activate() {
        Ok(Some(bundle)) => Ok(Runtime::SelfContained(bundle)),
        Ok(None) => Err(Error::new(
            CONTEXT,
            format!(
                "the exe was built self-contained, but the bundle ({}) is not next to it \
                 (naui does not fall back to the installed runtime). Place what the build \
                 put in target/<profile>/naui-winappsdk/ next to the exe. \
                 If Cargo's build.build-dir is set, rebuild with the environment variable \
                 NAUI_WINAPPSDK_TARGET_DIR set to the target-dir as an absolute path",
                SelfContained::MANIFEST_FILE,
            ),
        )),
        Err(e) => Err(Error::new(CONTEXT, e.message())),
    }
}

/// Attaches an installed framework package, trying from the newest version.
#[cfg_attr(feature = "self-contained", allow(dead_code))]
fn initialize_framework() -> Result<Runtime> {
    let mut last = None;
    for version in CANDIDATES {
        match PackageDependency::initialize(*version) {
            Ok(dependency) => return Ok(Runtime::Framework(dependency)),
            Err(e) => last = Some(e),
        }
    }
    let detail = match last {
        Some(e) => e.message(),
        // CANDIDATES が空にならない限りここへは来ない。
        None => String::new(),
    };
    Err(Error::new(
        "initialize the Windows App SDK runtime",
        format!("{detail} (versions tried: {})", version_list()),
    ))
}

/// The list of versions tried, included in errors.
#[cfg_attr(feature = "self-contained", allow(dead_code))]
fn version_list() -> String {
    CANDIDATES
        .iter()
        .map(|version| format!("{version:?}"))
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Versions are tried from the newest. If older ones matched first, an environment
    /// with 2.x would needlessly grab 1.x. The unversioned `CBS`, whose generation is
    /// unknown, goes at the very end.
    #[test]
    fn candidates_are_newest_first() {
        let order: Vec<String> = CANDIDATES.iter().map(|v| format!("{v:?}")).collect();
        assert_eq!(order.first().map(String::as_str), Some("V2"));
        assert_eq!(order.last().map(String::as_str), Some("Cbs"));
        let versioned = order.iter().position(|v| v == "V1_3").expect("V1_3 が無い");
        let plain_cbs = order.iter().position(|v| v == "Cbs").expect("Cbs が無い");
        assert!(versioned < plain_cbs, "版の付く候補を先に試す");
    }

    /// Trying the same version twice is pointless.
    #[test]
    fn candidates_have_no_duplicates() {
        let mut seen: Vec<String> = CANDIDATES.iter().map(|v| format!("{v:?}")).collect();
        let before = seen.len();
        seen.sort();
        seen.dedup();
        assert_eq!(seen.len(), before, "重複した版がある");
    }

    /// The error message lists every version tried.
    #[test]
    fn version_list_names_every_candidate() {
        let list = version_list();
        for version in CANDIDATES {
            assert!(list.contains(&format!("{version:?}")), "{version:?} が無い");
        }
    }
}
