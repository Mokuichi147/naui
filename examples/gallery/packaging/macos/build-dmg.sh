#!/usr/bin/env bash
# Gallery の .app を組み立て、.dmg に入れる。
#
#   ./build-dmg.sh <gallery の実行ファイル> <バージョン> <出力する .dmg>
#
# 実行ファイルはユニバーサルでも単一アーキテクチャでもよい。
# 署名は ad-hoc だけ (Developer ID での署名と公証はしない)。
set -euo pipefail

[ $# -eq 3 ] || { echo "使い方: $0 <実行ファイル> <バージョン> <出力する .dmg>" >&2; exit 2; }
bin="$1"
version="$2"
out="$3"

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
gallery="$(cd "$here/../.." && pwd)"
root="$(cd "$gallery/../.." && pwd)"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

# .dmg の中身。.app と、ドラッグで入れられるよう /Applications へのリンク。
stage="$work/stage"
app="$stage/naui UI gallery.app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$bin" "$app/Contents/MacOS/gallery"
# CFBundleVersion は数字と点だけなので、0.9.0-rc1 の -rc1 は落とす。
sed -e "s/@VERSION@/${version%%-*}/g" "$here/Info.plist" >"$app/Contents/Info.plist"
plutil -lint "$app/Contents/Info.plist" >/dev/null
printf 'APPL????' >"$app/Contents/PkgInfo"
cp -R "$gallery/assets" "$app/Contents/Resources/assets"
cp "$root/LICENSE-MIT" "$root/LICENSE-APACHE" "$app/Contents/Resources/"
ln -s /Applications "$stage/Applications"

# リンカーが実行ファイルだけに付けた署名を、Info.plist と Resources も
# 含めた .app 全体の ad-hoc 署名に付け直す。
codesign --force --sign - "$app"
codesign --verify --strict "$app"

rm -f "$out"
hdiutil create -volname "naui UI gallery" -srcfolder "$stage" \
  -fs HFS+ -format UDZO -ov "$out"
