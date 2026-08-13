//! Web ビルド用のフォント埋め込み。
//!
//! ブラウザではフォントファイルを OS から読めないため、wasm バイナリへ
//! 直接埋め込む必要がある。`MIUI_WEB_FONT` に TTF/OTF のパスを渡すと、
//! そのフォントを埋め込む。未指定ならフォント無し (空バイト列) になる。

use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-env-changed=MIUI_WEB_FONT");
    let out = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR")).join("webfont.ttf");

    match std::env::var("MIUI_WEB_FONT") {
        Ok(path) if !path.is_empty() => {
            println!("cargo:rerun-if-changed={path}");
            std::fs::copy(&path, &out).unwrap_or_else(|e| {
                let hint = "実在する TTF/OTF/TTC のパスを指定してください \
                            (例: web/build.sh \"/System/Library/Fonts/ヒラギノ角ゴシック W3.ttc\")。";
                panic!(
                    "MIUI_WEB_FONT に指定されたフォントを読めません。\n  パス: {path}\n  理由: {e}\n{hint}"
                )
            });
        }
        _ => {
            std::fs::write(&out, []).expect("プレースホルダを書き出せません");
        }
    }
}
