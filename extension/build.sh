#!/bin/sh
# Zips the extension for each store: dist/spentday-chrome.zip and dist/spentday-firefox.zip.
# Chrome only reads background.service_worker and Firefox only reads background.scripts,
# so each package keeps its own key and drops what the other store would warn about.
set -eu
cd "$(dirname "$0")"
rm -rf dist && mkdir -p dist
for target in chrome firefox; do
  stage="dist/$target"
  mkdir -p "$stage"
  cp -R background.js popup.html popup.css popup.js icons "$stage/"
  python3 - "$target" manifest.json "$stage/manifest.json" <<'PY'
import json, sys
target, src, dst = sys.argv[1:]
m = json.load(open(src))
if target == "chrome":
    del m["background"]["scripts"]
    del m["browser_specific_settings"]
else:
    del m["background"]["service_worker"]
json.dump(m, open(dst, "w"), indent=2)
PY
  (cd "$stage" && zip -qr "../spentday-$target.zip" .)
  rm -rf "$stage"
done
ls -l dist
