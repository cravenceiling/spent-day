#!/bin/sh
set -eu

fail() { printf '%s\n' "$*" >&2; exit 1; }
case "$(uname -s) $(uname -m)" in
    'Linux x86_64') ;;
    *) fail 'Spent Day supports Linux x86_64 only.' ;;
esac
[ "$(id -u)" -ne 0 ] || fail 'Run this installer as your desktop user, not root.'
for tool in curl sha256sum; do
    command -v "$tool" >/dev/null 2>&1 || fail "Install $tool, then run this installer again."
done
tmp=$(mktemp -d)
chmod 755 "$tmp"
trap 'rm -rf "$tmp"' 0
trap 'exit 1' HUP INT TERM
# SPENTDAY_RELEASE_URL points to an asset directory for local release tests.
base=${SPENTDAY_RELEASE_URL:-}
if [ -z "$base" ]; then
    api=https://api.github.com/repos/cravenceiling/spent-day/releases
    if [ -n "${SPENTDAY_VERSION:-}" ]; then
        api="$api/tags/$SPENTDAY_VERSION"
    else
        api="$api/latest"
    fi
    curl -fsSL "$api" > "$tmp/release.json"
    sums_url=$(sed -n 's/.*"browser_download_url": *"\([^"]*\/SHA256SUMS\)".*/\1/p' "$tmp/release.json")
    [ -n "$sums_url" ] || fail 'This release has no SHA256SUMS. Choose a newer release.'
    base=${sums_url%/*}
fi
base=${base%/}
curl -fsSL "$base/SHA256SUMS" > "$tmp/SHA256SUMS"
download() {
    name=$1
    curl -fsSL "$base/$name" -o "$tmp/$name"
    checksum=$(awk -v name="$name" '$2 == name { print; found=1 } END { if (!found) exit 1 }' "$tmp/SHA256SUMS") \
        || fail "No release checksum for $name."
    printf '%s\n' "$checksum" | (cd "$tmp" && sha256sum -c -) || fail "Checksum failed for $name."
}
ID='' ID_LIKE=''
# shellcheck source=/dev/null
if [ -r /etc/os-release ]; then . /etc/os-release; fi
family=" $ID $ID_LIKE "
format=AppImage
if [ "${SPENTDAY_INSTALL:-}" != appimage ]; then
    case "$family" in
        *' arch '*) format=pkg.tar.zst ;;
        *' debian '*|*' ubuntu '*) format=deb ;;
    esac
fi
asset=$(awk '{print $2}' "$tmp/SHA256SUMS" | grep -E "\.$format\$" | head -n 1)
[ -n "$asset" ] || fail "This release has no $format download."
download "$asset"
driver=
has_icd=false
for icd in /usr/share/vulkan/icd.d/*.json /etc/vulkan/icd.d/*.json /usr/local/share/vulkan/icd.d/*.json; do
    if [ -f "$icd" ]; then has_icd=true; break; fi
done
if [ "$has_icd" = false ]; then
    case "$family" in
        *' arch '*)
            for vendor in /sys/class/drm/card*/device/vendor; do
                [ -r "$vendor" ] || continue
                case "$(cat "$vendor")" in
                    0x8086) driver="$driver vulkan-intel" ;;
                    0x1002) driver="$driver vulkan-radeon" ;;
                    0x10de) driver="$driver nvidia-utils" ;;
                esac
            done
            driver=${driver:-vulkan-swrast}
            echo "No Vulkan driver found. Install with: sudo pacman -S $driver" ;;
        *' debian '*|*' ubuntu '*)
            driver=mesa-vulkan-drivers
            echo 'No Vulkan driver found. Install with: sudo apt install mesa-vulkan-drivers' ;;
        *' fedora '*) echo 'No Vulkan driver found. Run: sudo dnf install mesa-vulkan-drivers' ;;
        *) echo "No Vulkan driver found. Install your GPU's Vulkan driver." ;;
    esac
fi
bin=/usr/bin/spentday
case "$format" in
    pkg.tar.zst)
        if [ -n "$driver" ]; then
            # pacman -U accepts files only; stage the driver and dependencies for the same transaction.
            # shellcheck disable=SC2086
            sudo pacman -Sw --noconfirm --cachedir "$tmp" $driver
        fi
        sudo pacman -U --needed --noconfirm "$tmp"/*.pkg.tar.zst ;;
    deb)
        if [ -n "$driver" ]; then
            sudo apt install -y "$tmp/$asset" "$driver"
        else
            sudo apt install -y "$tmp/$asset"
        fi ;;
    AppImage)
        bin="$HOME/.local/bin/spentday"
        case "$bin" in
            *[%\$\"\'\\\`]*|*[[:cntrl:]]*) fail 'The install path cannot contain %, $, quotes, backslashes, backticks or control characters.' ;;
        esac
        data=${XDG_DATA_HOME:-$HOME/.local/share}
        mkdir -p "$HOME/.local/bin" "$data/applications" "$data/icons/hicolor/scalable/apps"
        download day.spent.SpentDay.svg
        install -m755 "$tmp/$asset" "$bin"
        install -m644 "$tmp/day.spent.SpentDay.svg" "$data/icons/hicolor/scalable/apps/day.spent.SpentDay.svg"
        cat > "$data/applications/day.spent.SpentDay.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=Spent Day
Comment=Screen time across your devices
Exec="$bin"
Icon=day.spent.SpentDay
Terminal=false
Categories=Utility;
StartupWMClass=day.spent.SpentDay
DESKTOP
        case ":$PATH:" in
            *":$HOME/.local/bin:"*) ;;
            *) echo 'Add ~/.local/bin to PATH to open the dashboard with spentday.' ;;
        esac ;;
esac
if [ "${SPENTDAY_NO_AUTOSTART:-}" != 1 ]; then
    "$bin" --enable-autostart || fail 'Launch at login could not be enabled. Open Settings > Launch at login after fixing the error above.'
fi
case "${XDG_CURRENT_DESKTOP:-}" in
    *GNOME*|*KDE*)
        if ! "$bin" --install-helpers; then
            echo 'Desktop helpers could not be installed. Follow the message above, then rerun spentday --install-helpers.'
        fi ;;
esac
if [ -n "${DISPLAY:-}${WAYLAND_DISPLAY:-}" ] && [ -n "${DBUS_SESSION_BUS_ADDRESS:-}" ]; then
    if ! command -v pgrep >/dev/null 2>&1 || ! pgrep -u "$(id -u)" -x spentday >/dev/null; then
        nohup "$bin" --background </dev/null >/dev/null 2>&1 &
    fi
else
    echo 'No desktop session found. Open Spent Day after logging in.'
fi
echo 'Installed Spent Day. Open the dashboard with spentday or the app launcher.'
