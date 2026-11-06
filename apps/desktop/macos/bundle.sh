#!/usr/bin/env bash
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
desktop="$(cd "$here/.." && pwd)"
out="$here/build"
app="$out/Spent Day.app"
bundle_id=""
[ "$#" -le 1 ] || { echo "usage: bundle.sh [--dev]" >&2; exit 2; }
case "${1:-}" in
  "") ;;
  --dev) app="$out/dev/Spent Day.app"; bundle_id=day.spent.SpentDay.dev ;;
  *) echo "usage: bundle.sh [--dev]" >&2; exit 2 ;;
esac

export MACOSX_DEPLOYMENT_TARGET=14.0
target=aarch64-apple-darwin
cargo_target="${CARGO_TARGET_DIR:-$desktop/target}"
case "$cargo_target" in /*) ;; *) cargo_target="$desktop/$cargo_target" ;; esac
(cd "$desktop" && cargo build --release --locked --target "$target")

# Publish only after signing and verification, so a failed build keeps the old app.
mkdir -p "$out"
stage="$(mktemp -d "$out/.stage.XXXXXX")"
trap 'rm -rf "$stage"' EXIT
staged="$stage/Spent Day.app"
mkdir -p "$staged/Contents/MacOS" "$staged/Contents/Resources"
cp "$here/Info.plist" "$staged/Contents/Info.plist"
[ -z "$bundle_id" ] || /usr/libexec/PlistBuddy -c "Set :CFBundleIdentifier $bundle_id" "$staged/Contents/Info.plist"
cp "$cargo_target/$target/release/spentday" "$staged/Contents/MacOS/SpentDay"

# A stable identity preserves Automation grants across rebuilds.
identity="${SPENTDAY_SIGN_IDENTITY:-}"
if [ -z "$identity" ] && security find-identity -v -p codesigning 2>/dev/null | grep -q '"Spent Day Dev"'; then
  identity="Spent Day Dev"
fi
if [ -z "$identity" ]; then
  echo "warning: signing ad hoc. macOS will ask for Automation again after each rebuild." >&2
  echo "warning: run apps/desktop/macos/make-dev-cert.sh once, or set SPENTDAY_SIGN_IDENTITY, to keep them." >&2
  identity="-"
fi
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
