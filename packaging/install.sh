#!/bin/sh
main() {
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
ID='' ID_LIKE=''
# shellcheck source=/dev/null
if [ -r /etc/os-release ]; then . /etc/os-release; fi
family=" $ID $ID_LIKE "
format=AppImage
if [ "${SPENTDAY_INSTALL:-}" != appimage ]; then
    case "$family" in
        *' arch '*) format=pkg.tar.zst ;;
        *' debian '*|*' ubuntu '*)
            glibc=$(getconf GNU_LIBC_VERSION 2>/dev/null | awk '{print $2}')
            if printf '%s\n' "$glibc" | awk -F. '{exit !($1 > 2 || ($1 == 2 && $2 >= 39))}'; then
                format=deb
            fi ;;
    esac
fi
if [ "$format" != AppImage ] && ! command -v sudo >/dev/null 2>&1; then
    echo 'sudo is unavailable; installing the AppImage for the current user.'
    format=AppImage
fi
tmp=$(mktemp -d)
chmod 755 "$tmp"
trap 'rm -rf "$tmp"' 0
trap 'exit 1' HUP INT TERM
# SPENTDAY_RELEASE_URL points to an asset directory for local release tests.
base=${SPENTDAY_RELEASE_URL:-}
tag=${SPENTDAY_VERSION:-}
if [ -z "$base" ]; then
    releases=https://github.com/cravenceiling/spent-day/releases
    if [ -n "$tag" ]; then
        tag=v${tag#v}
    else
        curl -fsS -D "$tmp/headers" -o /dev/null "$releases/latest" </dev/null \
            || fail 'No published release found.'
        tag=$(tr -d '\r' < "$tmp/headers" | sed -n 's|^[Ll][Oo][Cc][Aa][Tt][Ii][Oo][Nn]: https://github.com/cravenceiling/spent-day/releases/tag/\([^ /]*\)$|\1|p')
        [ -n "$tag" ] || fail 'Could not resolve the latest release.'
    fi
    base="$releases/download/$tag"
fi
base=${base%/}
fetch() {
    status=$(curl -sSL -w '%{http_code}' "$base/$1" -o "$2" </dev/null) || fail "Could not download $1."
    case "$status" in
        200) ;;
        404) fail "No release ${tag:-at $base} found, or it has no $1 download." ;;
        *) fail "Could not download $1 (HTTP $status)." ;;
    esac
}
fetch SHA256SUMS "$tmp/SHA256SUMS"
download() {
    name=$1
    fetch "$name" "$tmp/$name"
    checksum=$(awk -v name="$name" '$2 == name { print; found=1 } END { if (!found) exit 1 }' "$tmp/SHA256SUMS") \
        || fail "No release checksum for $name."
    printf '%s\n' "$checksum" | (cd "$tmp" && sha256sum -c -) || fail "Checksum failed for $name."
}
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
            # SPENTDAY_DRM_ROOT allows hybrid-GPU release tests without host GPU passthrough.
            for vendor in "${SPENTDAY_DRM_ROOT:-/sys/class/drm}"/card*/device/vendor; do
                [ -r "$vendor" ] || continue
                case "$(cat "$vendor")" in
                    0x8086) package=vulkan-intel ;;
                    0x1002) package=vulkan-radeon ;;
                    0x10de) package=vulkan-nouveau ;;
                    *) continue ;;
                esac
                case " $driver " in
                    *" $package "*) ;;
                    *) driver="${driver:+$driver }$package" ;;
                esac
            done
            driver=${driver:-vulkan-swrast}
            echo "No Vulkan driver found. Install with: sudo pacman -S $driver" ;;
        *' debian '*|*' ubuntu '*)
            driver=mesa-vulkan-drivers
            echo 'No Vulkan driver found. Install with: sudo apt-get install mesa-vulkan-drivers' ;;
        *' fedora '*) echo 'No Vulkan driver found. Run: sudo dnf install mesa-vulkan-drivers' ;;
        *) echo "No Vulkan driver found. Install your GPU's Vulkan driver." ;;
    esac
fi
bin=/usr/bin/spentday
case "$format" in
    pkg.tar.zst)
        if [ -n "$driver" ]; then
            # pacman -U accepts files only; keep drivers separate so they remain dependencies.
            mkdir "$tmp/drivers"
            # shellcheck disable=SC2086
            sudo pacman -Sw --noconfirm --cachedir "$tmp/drivers" $driver </dev/null
            sudo pacman -U --asdeps --needed --noconfirm "$tmp/drivers"/*.pkg.tar.zst </dev/null
        fi
        sudo pacman -U --needed --noconfirm "$tmp/$asset" </dev/null ;;
    deb)
        sudo apt-get update </dev/null
        if [ -n "$driver" ]; then
            sudo apt-get install -y "$tmp/$asset" "$driver" </dev/null
        else
            sudo apt-get install -y "$tmp/$asset" </dev/null
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
desktop=$(printf '%s' "${XDG_CURRENT_DESKTOP:-}:${XDG_SESSION_DESKTOP:-}" | tr '[:upper:]' '[:lower:]')
if [ "${SPENTDAY_NO_AUTOSTART:-}" != 1 ]; then
    manual=false
    case "$desktop" in
        *hyprland*|*sway*|*i3*)
            # uwsm starts this target to run XDG entries; compositor-only sessions do not.
            if ! systemctl --user is-active --quiet xdg-desktop-autostart.target </dev/null 2>/dev/null; then
                manual=true
            fi ;;
    esac
    if [ "$manual" = true ]; then
        case "$desktop" in
            *hyprland*) echo 'To start at login, add to ~/.config/hypr/autostart.conf or hyprland.conf: exec-once = spentday --background' ;;
            *sway*) echo 'To start at login, add to ~/.config/sway/config: exec spentday --background' ;;
            *i3*) echo 'To start at login, add to ~/.config/i3/config: exec spentday --background' ;;
        esac
    else
        "$bin" --enable-autostart </dev/null || fail 'Launch at login could not be enabled. Open Settings > Launch at login after fixing the error above.'
    fi
fi
case "$desktop" in
    *gnome*|*kde*)
        if [ "$format" = AppImage ]; then
            if ! "$bin" --install-helpers </dev/null; then
                echo 'Desktop helpers could not be installed. Follow the message above, then rerun spentday --install-helpers.'
            fi
        else
            case "$desktop" in
                *gnome*)
                    gnome-extensions enable spentday@spent.day </dev/null \
                        || echo 'Log out and back in, then run: gnome-extensions enable spentday@spent.day' ;;
                *kde*)
                    kwriteconfig6 --file kwinrc --group Plugins --key spentday-focusEnabled true </dev/null \
                        || echo 'Could not enable the KWin script. Run: kwriteconfig6 --file kwinrc --group Plugins --key spentday-focusEnabled true'
                    qdbus6 org.kde.KWin /Scripting org.kde.kwin.Scripting.unloadScript spentday-focus </dev/null \
                        || echo 'Could not unload the previous KWin script. Log out and back in to reload it.'
                    qdbus6 org.kde.KWin /Scripting org.kde.kwin.Scripting.start </dev/null \
                        || echo 'Log out and back in to start the KWin script.' ;;
            esac
        fi ;;
esac
if [ -n "${DISPLAY:-}${WAYLAND_DISPLAY:-}" ] && [ -n "${DBUS_SESSION_BUS_ADDRESS:-}" ]; then
    if command -v pkill >/dev/null 2>&1; then
        pkill -x -u "$(id -u)" spentday || true
        for _ in 1 2 3 4 5; do
            if ! pgrep -x -u "$(id -u)" spentday >/dev/null; then break; fi
            sleep 1
        done
    fi
    nohup "$bin" --background </dev/null >/dev/null 2>&1 &
else
    echo 'No desktop session found. Open Spent Day after logging in.'
fi
echo 'Installed Spent Day. Open the dashboard with spentday or the app launcher.'
}
main "$@"
