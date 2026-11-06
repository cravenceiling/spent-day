#!/usr/bin/env bash
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"

if [ "${1:-}" = --linux ]; then
  [ "$#" -eq 1 ] || { echo 'Usage: snapshot.sh [--linux]' >&2; exit 2; }
  repo="$(cd "$here/../.." && pwd)"
  exec docker run --rm \
    -v "$repo:/src" \
    -v spentday-desktop-target:/target \
    -v spentday-desktop-cargo-registry:/usr/local/cargo/registry \
    -v spentday-desktop-cargo-git:/usr/local/cargo/git \
    -e CARGO_TARGET_DIR=/target/unit6 \
    -w /src/apps/desktop rust:1-trixie bash -c '
      apt-get update -qq &&
      apt-get install -y -qq --no-install-recommends libxkbcommon-dev libwayland-dev pkg-config mesa-vulkan-drivers libvulkan1 &&
      ./snapshot.sh'
fi
[ "$#" -eq 0 ] || { echo 'Usage: snapshot.sh [--linux]' >&2; exit 2; }
case "$(uname -s)" in
  Darwin) os=macos ;;
  Linux) os=linux ;;
  *) echo 'Snapshots support macOS and Linux only.' >&2; exit 1 ;;
esac
cd "$here"
cargo build --locked
spentday="${CARGO_TARGET_DIR:-$here/target}/debug/spentday"
shots="$here/screenshots/$os"
mkdir -p "$shots"
for window in dash panel fatal; do
  scenes="$("$spentday" --demo --window "$window" --list-scenes)"
  while IFS= read -r scene; do
    case "$scene" in
      panel) name=menu-demo ;;
      panel-syncing) name=menu-syncing-demo ;;
      settings-*|db-error|track-error) name="$scene-demo" ;;
      *) name="dashboard-$scene-demo" ;;
    esac
    "$spentday" --demo --window "$window" --scene "$scene" --theme dark --snap "$shots/$name.png"
  done <<< "$scenes"
done
"$spentday" --demo --scene day --theme light --snap "$shots/dashboard-light-demo.png"
"$spentday" --demo --scene day --theme dark --size 720x560 --snap "$shots/dashboard-small-demo.png"
