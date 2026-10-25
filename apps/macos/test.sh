#!/usr/bin/env bash
# Builds and runs the collector's checks against the real core, then the build scripts' checks.
# Exits nonzero on any failure.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
work=""
trap 'rm -rf "$work"' EXIT
work="$(mktemp -d)"

source "$here/core.sh"
build_core "$work/generated"

xcrun swiftc -swift-version 6 -target arm64-apple-macos14.0 \
  "$here"/Sources/{BrowserPages,Collector,Presence,Ranges,Recorder,Store}.swift "$here"/Tests/*.swift "${core_flags[@]}" \
  -o "$work/spentday-tests"
"$work/spentday-tests"
"$here/Tests/publish.sh"
"$here/Tests/verify-cleanup.sh"
"$here/Tests/make-dev-cert.sh"
