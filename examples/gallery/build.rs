//! `self-contained` フィーチャーのとき、同梱する Windows App SDK ランタイムの
//! マニフェストを exe に埋め込む。
//!
//! 埋め込まなくても Windows 10 1903 以降では動く。OS が reg-free WinRT を
//! 持たない 1809 では、Windows App SDK が exe に埋め込まれたマニフェストしか
//! 読まないので要る。naui を使うアプリが同じことをするときの見本も兼ねる
//! (README の「マニフェストを exe に埋め込む」)。

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    let windows_msvc = std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc");
    if !windows_msvc || std::env::var_os("CARGO_FEATURE_SELF_CONTAINED").is_none() {
        return;
    }

    // naui のビルドが target/<profile>/naui-winappsdk/ に置くマニフェスト。
    // OUT_DIR は target/[<triple>/]<profile>/build/gallery-<hash>/out。
    let out_dir = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let manifest = out_dir
        .ancestors()
        .nth(3)
        .unwrap()
        .join("naui-winappsdk")
        .join("naui-winappsdk.manifest");
    println!("cargo:rustc-link-arg-bins=/MANIFEST:EMBED");
    println!(
        "cargo:rustc-link-arg-bins=/MANIFESTINPUT:{}",
        manifest.display()
    );
}
