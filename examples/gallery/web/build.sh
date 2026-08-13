#!/usr/bin/env bash
# ギャラリーを wasm へビルドし、`pkg/` に配置する。
#
#   ./build.sh <埋め込むフォントのパス>
#
# ブラウザには OS のフォントファイルを読む手段が無いため、
# 文字を表示するには TTF/OTF/TTC を wasm に埋め込む必要がある。
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/../../.." && pwd)"

die() {
  echo "エラー: $*" >&2
  exit 1
}

# --- 1. フォントの確認 -------------------------------------------------------
if [ $# -lt 1 ]; then
  cat >&2 <<'USAGE'
エラー: 埋め込むフォントを指定してください。

  ./build.sh <フォントのパス>

例:
  # macOS (日本語)
  ./build.sh "/System/Library/Fonts/ヒラギノ角ゴシック W3.ttc"
  # Windows
  ./build.sh "C:/Windows/Fonts/YuGothM.ttc"
  # Linux
  ./build.sh /usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc

ブラウザからは OS のフォントファイルを読めないため、
フォントを wasm に埋め込む必要があります。
USAGE
  exit 1
fi

font="$1"
[ -f "$font" ] || die "フォントが見つかりません: $font"
export MIUI_WEB_FONT="$font"
echo "埋め込むフォント: $font ($(du -h "$font" | cut -f1))"

# --- 2. ツールチェインの確認 -------------------------------------------------
if ! rustup target list --installed 2>/dev/null | grep -qx wasm32-unknown-unknown; then
  echo "wasm32-unknown-unknown ターゲットを追加します..."
  rustup target add wasm32-unknown-unknown
fi

# wasm-bindgen CLI は wasm-bindgen クレートと同じバージョンでなければならない。
want="$(awk '/^name = "wasm-bindgen"$/{getline; gsub(/[",]/,""); print $3; exit}' "$root/Cargo.lock")"
if [ -z "$want" ]; then
  die "Cargo.lock から wasm-bindgen のバージョンを取得できませんでした。"
fi

if ! command -v wasm-bindgen >/dev/null 2>&1; then
  die "wasm-bindgen が見つかりません。次を実行してください:
  cargo install wasm-bindgen-cli --version $want"
fi

have="$(wasm-bindgen --version | awk '{print $2}')"
if [ "$have" != "$want" ]; then
  die "wasm-bindgen CLI のバージョンが一致しません (CLI: $have / クレート: $want)。
次を実行してください:
  cargo install wasm-bindgen-cli --version $want"
fi

# --- 3. ビルド ---------------------------------------------------------------
cd "$root"
cargo build -p gallery --lib --release --target wasm32-unknown-unknown

wasm-bindgen \
  --target web \
  --no-typescript \
  --out-dir "$here/pkg" \
  "$root/target/wasm32-unknown-unknown/release/gallery.wasm"

echo
echo "完了。次のように配信してください:"
echo "  cd \"$here\" && python3 -m http.server 8080"
echo "  → http://localhost:8080/"
