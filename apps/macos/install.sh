#!/usr/bin/env bash
# Copies the built app to /Applications, replacing any running copy.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
app="$here/build/Spent Day.app"
dest="/Applications/Spent Day.app"

if [ ! -d "$app" ]; then
  echo "No build found. Run apps/macos/build.sh first." >&2
  exit 1
fi
if [ ! -x "$app/Contents/MacOS/SpentDay" ]; then
  echo "$app has no executable. Run apps/macos/build.sh again." >&2
  exit 1
fi
if ! codesign --verify --strict "$app"; then
  echo "$app fails signature verification. Run apps/macos/build.sh again." >&2
  exit 1
fi
if codesign -dv "$app" 2>&1 | grep -q "Signature=adhoc"; then
  echo "warning: this build is signed ad hoc, so permissions reset on every reinstall. See make-dev-cert.sh." >&2
fi

# Copied and checked beside the installed app first, so a failed copy leaves it in place.
staged="$(mktemp -d /Applications/.SpentDay.XXXXXX)"
trap 'rm -rf "$staged"' EXIT
ditto "$app" "$staged/Spent Day.app"
codesign --verify --strict "$staged/Spent Day.app"

if pgrep -xq SpentDay; then
  osascript -e 'tell application id "day.spent.SpentDay" to quit' || true
  for _ in 1 2 3 4 5 6 7 8 9 10; do pgrep -xq SpentDay || break; sleep 0.5; done
fi

"$here/publish.sh" "$staged/Spent Day.app" "$dest"
echo "Installed $dest"
open "$dest"
