#!/bin/sh
set -eu
cd "$(dirname "$0")/../.."
repo="$(pwd)"
export CFLAGS="${CFLAGS:-} -fno-lto"
(cd apps/desktop && cargo build --release --locked)
target="${CARGO_TARGET_DIR:-$repo/apps/desktop/target}"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
appdir="$tmp/SpentDay.AppDir"
install -Dm755 "$target/release/spentday" "$appdir/usr/bin/spentday"
install -Dm644 apps/desktop/linux/data/day.spent.SpentDay.desktop "$appdir/day.spent.SpentDay.desktop"
install -Dm644 apps/desktop/linux/data/icons/day.spent.SpentDay.svg "$appdir/day.spent.SpentDay.svg"
install -Dm644 LICENSE "$appdir/usr/share/licenses/spentday/LICENSE"
install -m755 packaging/linux/AppRun "$appdir/AppRun"

# No host libraries are copied. In particular, Vulkan/Mesa, Wayland, xkbcommon and X11
# must stay with the host's driver stack, as in AppImage's pkg2appimage excludelist.
curl -fL --retry 3 https://github.com/AppImage/appimagetool/releases/download/1.9.1/appimagetool-x86_64.AppImage -o "$tmp/appimagetool"
echo "ed4ce84f0d9caff66f50bcca6ff6f35aae54ce8135408b3fa33abfc3cb384eb0  $tmp/appimagetool" | sha256sum -c -
curl -fL --retry 3 https://github.com/AppImage/type2-runtime/releases/download/20251108/runtime-x86_64 -o "$tmp/runtime"
echo "2fca8b443c92510f1483a883f60061ad09b46b978b2631c807cd873a47ec260d  $tmp/runtime" | sha256sum -c -
chmod +x "$tmp/appimagetool"
version="$(sed -n 's/^version = "\([^"]*\)"/\1/p' apps/desktop/Cargo.toml | head -1)"
mkdir -p dist
ARCH=x86_64 "$tmp/appimagetool" --appimage-extract-and-run --runtime-file "$tmp/runtime" \
    "$appdir" "$repo/dist/SpentDay-$version-x86_64.AppImage"
