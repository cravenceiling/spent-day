#!/usr/bin/env bash
# Draws the dashboard and the menu bar panel from demo data into apps/macos/screenshots,
# or the directory given. It draws offscreen and never opens the app or a window.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
out="${1:-$here/screenshots}"
work=""
trap 'rm -rf "$work"' EXIT
work="$(mktemp -d)"

source "$here/core.sh"
build_core "$work/generated"

sources=()
for f in "$here"/Sources/*.swift; do
  [ "$(basename "$f")" = SpentDayApp.swift ] || sources+=("$f")
done
xcrun swiftc -swift-version 6 -target arm64-apple-macos14.0 -module-name SpentDay \
  "${sources[@]}" "$here/Snapshot/main.swift" "${core_flags[@]}" \
  -o "$work/spentday-snapshot"
mkdir -p "$out"
"$work/spentday-snapshot" "$out"
