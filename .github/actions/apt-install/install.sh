#!/usr/bin/env bash
# action.yml から呼ぶ。入れるパッケージは環境変数 PACKAGES で受け取る。
#
# 1. キャッシュにある一覧と .deb だけで入れる (ネットワークを使わない)。
# 2. 足りなければミラーから取る。Canonical のミラーの名前を引けないときは、
#    Canonical 以外のミラーだけから取る。
set -euo pipefail

dir="$HOME/.cache/naui-apt"
mkdir -p "$dir/archives/partial" "$dir/lists/partial"

set -f
# shellcheck disable=SC2206 # 空白と改行で分けたい
packages=($PACKAGES)
set +f

# 一覧と .deb の置き場をキャッシュの場所へ向ける。取ったものは消さずに残す。
apt=(sudo apt-get
  -o "Dir::State::Lists=$dir/lists"
  -o "Dir::Cache::Archives=$dir/archives"
  -o APT::Keep-Downloaded-Packages=true
  -o APT::Sandbox::User=root
  -o Acquire::Retries=3)
install=(install --no-install-recommends -y "${packages[@]}")

# apt は root で書くので、キャッシュへ保存できるよう持ち主を戻す。
trap 'sudo chown -R "$(id -u):$(id -g)" "$dir"' EXIT

if compgen -G "$dir/lists/*InRelease" >/dev/null; then
  echo "::group::キャッシュだけで導入する"
  if "${apt[@]}" --no-download "${install[@]}"; then
    echo "::endgroup::"
    echo "キャッシュだけで導入した (ミラーには行っていない)"
    exit 0
  fi
  echo "::endgroup::"
  echo "キャッシュだけでは足りないので、ミラーから取る"
fi

# Canonical 以外の Ubuntu のミラー。どれも Ubuntu の鍵で署名された一覧を
# そのまま配っているので、apt の署名の検証は Canonical のミラーと変わらない。
fallbacks=(
  https://mirrors.edge.kernel.org/ubuntu/
  https://mirrors.mit.edu/ubuntu/
)
resolves() { timeout 10 getent ahosts "$1" >/dev/null 2>&1; }

# GitHub のランナーは sources を mirror+file:/etc/apt/apt-mirrors.txt に
# 向けていて、apt はこのファイルの上から順に試す。
mirrors=/etc/apt/apt-mirrors.txt
if [ -f "$mirrors" ]; then
  if resolves azure.archive.ubuntu.com || resolves archive.ubuntu.com; then
    # ふだんは近い Azure のミラーを先に使い、予備は最後に足すだけ。
    n="$(grep -c . "$mirrors" || true)"
    for url in "${fallbacks[@]}"; do
      n=$((n + 1))
      printf '%s\tpriority:%d\n' "$url" "$n"
    done | sudo tee -a "$mirrors" >/dev/null
  else
    echo "::warning::Ubuntu のミラーの名前を引けないので、Canonical 以外のミラーから取る"
    n=0
    for url in "${fallbacks[@]}"; do
      n=$((n + 1))
      printf '%s\tpriority:%d\n' "$url" "$n"
    done | sudo tee "$mirrors" >/dev/null
  fi
  echo "::group::$mirrors"
  cat "$mirrors"
  echo "::endgroup::"
fi

# 一覧を取れなかったら、古い一覧のまま進まずにここで止める。
"${apt[@]}" update --error-on=any
"${apt[@]}" "${install[@]}"
# もう一覧に無い古い .deb を消し、キャッシュが膨らみ続けないようにする。
"${apt[@]}" autoclean
