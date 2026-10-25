#!/usr/bin/env bash
# Builds apps/macos/build/Spent Day.app from a clean checkout.
# `build.sh --dev` builds apps/macos/build/dev/Spent Day.app instead, which also reads
# SPENTDAY_DB and SPENTDAY_DEMO. Scripts use it to run the app on a scratch database.
# Its bundle id is day.spent.SpentDay.dev, so it has its own settings and Automation
# grants. install.sh only installs the normal build, which ignores both variables.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
out="$here/build"
app="$out/Spent Day.app"
dev_flags=()
bundle_id=""
case "${1:-}" in
  "") ;;
  --dev) app="$out/dev/Spent Day.app"; dev_flags=(-D SPENTDAY_DEV); bundle_id=day.spent.SpentDay.dev ;;
  *) echo "usage: build.sh [--dev]" >&2; exit 2 ;;
esac

source "$here/core.sh"
build_core "$out/generated"

# The bundle is assembled and signed next to the old one, which is replaced only
# once the new one verifies, so a failed build never leaves a half-built app.
mkdir -p "$out"
stage="$(mktemp -d "$out/.stage.XXXXXX")"
trap 'rm -rf "$stage"' EXIT
staged="$stage/Spent Day.app"
mkdir -p "$staged/Contents/MacOS" "$staged/Contents/Resources"
cp "$here/Info.plist" "$staged/Contents/Info.plist"
[ -z "$bundle_id" ] || /usr/libexec/PlistBuddy -c "Set :CFBundleIdentifier $bundle_id" "$staged/Contents/Info.plist"

xcrun swiftc \
  -O -parse-as-library -swift-version 6 \
  -target arm64-apple-macos14.0 \
  -module-name SpentDay ${dev_flags[@]+"${dev_flags[@]}"} \
  "$here"/Sources/*.swift "${core_flags[@]}" \
  -o "$staged/Contents/MacOS/SpentDay"

# A stable identity keeps the Automation grants across rebuilds. macOS ties them
# to the signature, and an ad-hoc one changes every build.
identity="${SPENTDAY_SIGN_IDENTITY:-}"
if [ -z "$identity" ] && security find-identity -v -p codesigning 2>/dev/null | grep -q '"Spent Day Dev"'; then
  identity="Spent Day Dev"
fi
if [ -z "$identity" ]; then
  echo "warning: signing ad hoc. macOS will ask for Automation again after each rebuild." >&2
  echo "warning: run apps/macos/make-dev-cert.sh once, or set SPENTDAY_SIGN_IDENTITY, to keep them." >&2
  identity="-"
fi
# The hardened runtime stops other code from being loaded into the app, for example with
# DYLD_INSERT_LIBRARIES, so it can't borrow the app's Automation grants. The only
# entitlement is sending Apple events, which the browser queries need.
codesign --force --sign "$identity" --timestamp=none --options runtime \
  --entitlements "$here/SpentDay.entitlements" "$staged"
codesign --verify --strict "$staged"
signature="$(codesign -d --verbose "$staged" 2>&1)"
runtime='flags=0x[0-9a-f]+\([^)]*runtime'
[[ "$signature" =~ $runtime ]] || { echo "error: the app isn't signed with the hardened runtime" >&2; exit 1; }

mkdir -p "$(dirname "$app")"
"$here/publish.sh" "$staged" "$app"
echo "Signed with: $identity"
echo "Built $app"
