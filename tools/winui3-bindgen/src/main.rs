//! `crates/naui-winui3/src/bindings.rs` を作り直す。
//!
//! ```text
//! cargo run --manifest-path tools/winui3-bindgen/Cargo.toml -- <winmd を置いた場所>
//! ```
//!
//! `.winmd` の取り方は隣の `README.md` を参照。出力先は既定で
//! `crates/naui-winui3/src/bindings.rs`。

use std::path::{Path, PathBuf};

/// `windows-collections` 0.3 に無く、`windows` クレート側にある型。
///
/// `windows-bindgen` は `Windows.Foundation.Collections` を丸ごと
/// `windows_collections` へ向けるので、そこに無いものだけ向け直す。
const COLLECTION_FALLBACKS: &[&str] = &[
    "IObservableVector",
    "IObservableMap",
    "IVectorChangedEventArgs",
    "IMapChangedEventArgs",
    "VectorChangedEventHandler",
    "MapChangedEventHandler",
    "CollectionChange",
];

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(winmd) = args.next() else {
        eprintln!("winmd を置いた場所を渡してください (README.md 参照)");
        std::process::exit(2);
    };
    let root = repo_root();
    let out = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("crates/naui-winui3/src/bindings.rs"));

    let filter_path = root.join("tools/winui3-bindgen/filter.txt");
    let filter = std::fs::read_to_string(&filter_path)
        .unwrap_or_else(|e| panic!("{} を読めません: {e}", filter_path.display()));

    let mut args: Vec<String> = vec![
        "--in".into(),
        // 既定のメタデータ (Windows.*) と、渡された Windows App SDK の分。
        "default".into(),
        winmd,
        "--out".into(),
        out.display().to_string(),
        // `IApplicationOverrides` のように Rust 側で実装するインターフェースの
        // ための `*_Impl` トレイトを出す。これが無いと `Application` を
        // 継承できない。
        "--implement".into(),
    ];
    // `Windows.*` は `windows` 系のクレートへ向ける。同じ型を二重に持つと
    // 別の Rust 型になってしまい、naui 側で受け渡しできなくなる。
    for reference in [
        "windows-collections,flat,Windows.Foundation.Collections.IIterable",
        "windows-collections,flat,Windows.Foundation.Collections.IIterator",
        "windows-collections,flat,Windows.Foundation.Collections.IKeyValuePair",
        "windows-collections,flat,Windows.Foundation.Collections.IMap",
        "windows-collections,flat,Windows.Foundation.Collections.IMapView",
        "windows-collections,flat,Windows.Foundation.Collections.IVector",
        "windows-collections,flat,Windows.Foundation.Collections.IVectorView",
        "windows,skip-root,Windows",
    ] {
        args.push("--reference".into());
        args.push(reference.into());
    }
    args.push("--filter".into());
    for line in filter.lines() {
        let line = line.trim();
        if !line.is_empty() && !line.starts_with('#') {
            args.push(line.into());
        }
    }

    let warnings = windows_bindgen::bindgen(&args);
    // 投影できなかったメンバーの一覧は長いので、件数だけ出す。naui が使う
    // メンバーが落ちていれば naui-windows のビルドが失敗して分かる。
    let skipped = warnings.to_string().lines().filter(|l| l.starts_with("skipping")).count();
    println!("{} を書き出しました (投影できなかったメンバー: {skipped} 件)", out.display());

    fix_collection_paths(&out);
}

/// `windows_collections` に無い型への参照を `windows` 側へ向け直す。
fn fix_collection_paths(out: &Path) {
    let mut source = std::fs::read_to_string(out).expect("生成した bindings.rs を読めません");
    for name in COLLECTION_FALLBACKS {
        source = source.replace(
            &format!("windows_collections::{name}"),
            &format!("windows::Foundation::Collections::{name}"),
        );
    }
    std::fs::write(out, source).expect("bindings.rs を書き戻せません");
}

/// このツールから見たリポジトリのルート。
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("tools/winui3-bindgen の 2 つ上")
        .to_path_buf()
}
