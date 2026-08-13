#!/usr/bin/env bash
# ギャラリーを wasm へビルドして pkg/ に配置する。
#
#   ./build.sh
#
# ブラウザ向けバックエンドは DOM の標準コントロールを使うので、
# フォントの埋め込みなどは不要。
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/../../.." && pwd)"

die() { echo "エラー: $*" >&2; exit 1; }

if ! rustup target list --installed 2>/dev/null | grep -qx wasm32-unknown-unknown; then
  echo "wasm32-unknown-unknown ターゲットを追加します..."
  rustup target add wasm32-unknown-unknown
fi

want="$(awk '/^name = "wasm-bindgen"$/{getline; gsub(/[",]/,""); print $3; exit}' "$root/Cargo.lock")"
[ -n "$want" ] || die "Cargo.lock から wasm-bindgen のバージョンを取得できませんでした。"

command -v wasm-bindgen >/dev/null 2>&1 || die "wasm-bindgen が見つかりません。次を実行してください:
  cargo install wasm-bindgen-cli --version $want"

have="$(wasm-bindgen --version | awk '{print $2}')"
[ "$have" = "$want" ] || die "wasm-bindgen CLI のバージョンが一致しません (CLI: $have / クレート: $want)。
次を実行してください:
  cargo install wasm-bindgen-cli --version $want"

cd "$root"
cargo build -p gallery --lib --release --target wasm32-unknown-unknown

wasm-bindgen --target web --no-typescript \
  --out-dir "$here/pkg" \
  "$root/target/wasm32-unknown-unknown/release/gallery.wasm"

echo
echo "完了。次のように配信してください:"
echo "  cd \"$here\" && python3 -m http.server 8080"
echo "  → http://localhost:8080/"
