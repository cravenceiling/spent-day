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
if [ "$format" != AppImage ]; then
    sudo_failed=
    if ! command -v sudo >/dev/null 2>&1; then
        sudo_failed='sudo is unavailable'
    elif ! sudo -v </dev/null; then
        sudo_failed='sudo authorization failed'
    fi
    if [ -n "$sudo_failed" ]; then
        # An AppImage beside the package would shadow it in PATH and the app launcher.
        [ ! -e /usr/bin/spentday ] || fail "$sudo_failed, and updating the installed Spent Day package needs it."
        echo "$sudo_failed; installing the AppImage for the current user."
        format=AppImage
    fi
fi
bin=/usr/bin/spentday
if [ "$format" = AppImage ]; then bin="$HOME/.local/bin/spentday"; fi
was_installed=false
if [ -e /usr/bin/spentday ] || [ -e "$HOME/.local/bin/spentday" ]; then was_installed=true; fi
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
            case "$driver" in
                '') driver=vulkan-swrast ;;
                vulkan-nouveau) driver="$driver vulkan-swrast" ;;
            esac
            driver_cmd="sudo pacman -S $driver" ;;
        *' debian '*|*' ubuntu '*)
            driver=mesa-vulkan-drivers
            driver_cmd="sudo apt-get install $driver" ;;
        *' fedora '*) driver_cmd='sudo dnf install mesa-vulkan-drivers' ;;
        *) driver_cmd="your GPU's Vulkan driver" ;;
    esac
    echo 'No Vulkan driver found. Spent Day tracks without one, but needs one to open its window.'
    answer=n
    if [ -n "$driver" ] && [ "$format" != AppImage ]; then
        if [ "${SPENTDAY_INSTALL_DRIVER:-}" = 1 ]; then
            answer=y
        elif (: </dev/tty) 2>/dev/null; then
            # curl | sh feeds this script on stdin, so the answer has to come from the terminal.
            printf 'Install %s now? [Y/n] ' "$driver" >/dev/tty
            read -r answer </dev/tty || answer=n
            answer=${answer:-y}
        fi
    fi
    case "$answer" in
        [Yy]*) ;;
        *) driver=
           echo "Install it later with: $driver_cmd" ;;
    esac
fi
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
        sudo apt-get update </dev/null \
            || echo 'apt-get update failed; continuing with cached package lists.'
        # The package and libvulkan1 recommend a Vulkan driver; only the answer above may add one.
        if [ -n "$driver" ]; then
            sudo apt-get install -y --no-install-recommends "$tmp/$asset" "$driver" </dev/null
        else
            sudo apt-get install -y --no-install-recommends "$tmp/$asset" </dev/null
        fi ;;
    AppImage)
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
if [ "$was_installed" = false ] && [ "${SPENTDAY_NO_AUTOSTART:-}" != 1 ]; then
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
            *hyprland*) printf "To start at login, add to ~/.config/hypr/autostart.conf or hyprland.conf: exec-once = '%s' --background\n" "$bin" ;;
            *sway*) printf "To start at login, add to ~/.config/sway/config: exec '%s' --background\n" "$bin" ;;
            *i3*) printf "To start at login, add to ~/.config/i3/config: exec '%s' --background\n" "$bin" ;;
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
                    gnome-extensions enable spentday@spent.day </dev/null >/dev/null 2>&1 \
                        || echo 'Log out and back in, then run: gnome-extensions enable spentday@spent.day' ;;
                *kde*)
                    kwriteconfig6 --file kwinrc --group Plugins --key spentday-focusEnabled true </dev/null >/dev/null 2>&1 \
                        || echo 'Could not enable the KWin script. Run: kwriteconfig6 --file kwinrc --group Plugins --key spentday-focusEnabled true'
                    qdbus6 org.kde.KWin /Scripting org.kde.kwin.Scripting.unloadScript spentday-focus </dev/null >/dev/null 2>&1 \
                        || echo 'Could not unload the previous KWin script. Log out and back in to reload it.'
                    qdbus6 org.kde.KWin /Scripting org.kde.kwin.Scripting.start </dev/null >/dev/null 2>&1 \
                        || echo 'Log out and back in to start the KWin script.' ;;
            esac
        fi ;;
esac
dbus_call() {
    if command -v busctl >/dev/null 2>&1; then
        busctl --user call org.freedesktop.DBus /org/freedesktop/DBus org.freedesktop.DBus "$1" s day.spent.SpentDay </dev/null
    else
        gdbus call --session --dest org.freedesktop.DBus --object-path /org/freedesktop/DBus \
            --method "org.freedesktop.DBus.$1" day.spent.SpentDay </dev/null
    fi
}
running() {
    dbus_call NameHasOwner 2>/dev/null | grep -q 'true'
}
if [ -n "${SSH_CONNECTION:-}" ]; then
    if running; then
        echo 'Restart Spent Day from your desktop session to use the new version.'
    else
        echo 'Open Spent Day from your desktop session.'
    fi
elif [ -n "${DISPLAY:-}${WAYLAND_DISPLAY:-}" ] && [ -n "${DBUS_SESSION_BUS_ADDRESS:-}" ]; then
    if systemctl --user is-active --quiet spentday.service </dev/null 2>/dev/null; then
        systemctl --user daemon-reload </dev/null || true
        unit_bin=$(systemctl --user show -p ExecStart --value spentday.service </dev/null | sed -n 's/.*path=\(.*\) ; argv\[\].*/\1/p')
        if [ "$unit_bin" != "$bin" ]; then
            echo "spentday.service runs $unit_bin, not $bin. Update its ExecStart, then run: systemctl --user restart spentday"
        else
            systemctl --user restart spentday.service </dev/null \
                || echo 'Could not restart spentday.service. Run: systemctl --user restart spentday'
        fi
    elif dbus_call NameHasOwner >/dev/null 2>&1; then
        if running; then
            pid=$(dbus_call GetConnectionUnixProcessID | sed -n 's/^u \([0-9][0-9]*\)$/\1/p; s/^(uint32 \([0-9][0-9]*\),)$/\1/p')
            [ -n "$pid" ] || fail 'Could not find the running Spent Day process. Restart it from your desktop session.'
            kill -TERM "$pid" 2>/dev/null || true
            attempts=0
            while running; do
                [ "$attempts" -lt 15 ] || fail 'Spent Day is still stopping. Restart it from your desktop session.'
                sleep 1
                attempts=$((attempts + 1))
            done
        fi
        nohup "$bin" --background </dev/null >/dev/null 2>&1 &
    else
        echo 'Could not query the desktop session bus. Open Spent Day from your desktop session.'
    fi
else
    echo 'No desktop session found. Open Spent Day after logging in.'
fi
echo 'Installed Spent Day. Open the dashboard with spentday or the app launcher.'
}
main "$@"
