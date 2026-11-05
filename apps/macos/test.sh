#!/usr/bin/env bash
# Builds and runs the collector's checks against the real core, then the build scripts' checks.
# Exits nonzero on any failure.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
work=""
trap 'rm -rf "$work"' EXIT
mkdir -p "$here/build"
work="$(mktemp -d "$here/build/tests.XXXXXX")"

source "$here/core.sh"
build_core "$work/generated"

# Swift identifies private declarations by basename, so tests need distinct build filenames.
cp "$here/Tests/Treemap.swift" "$work/TreemapTests.swift"
cp "$here/Tests/TileColors.swift" "$work/TileColorsTests.swift"
tests=("$here/Tests/main.swift" "$here/Tests/Core.swift" "$here/Tests/Browsers.swift" "$work/TreemapTests.swift" "$work/TileColorsTests.swift")
xcrun swiftc -swift-version 6 -target arm64-apple-macos14.0 \
  "$here"/Sources/{AppModel,BrowserPages,Collector,DemoStore,Presence,Recorder,Store,Treemap,TileColors}.swift "${tests[@]}" "${core_flags[@]}" \
  -o "$work/spentday-tests"
"$work/spentday-tests"
"$here/Tests/publish.sh"
"$here/Tests/verify-cleanup.sh"
"$here/Tests/make-dev-cert.sh"
