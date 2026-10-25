#!/bin/sh
# Builds Spent Day and installs it for the current user under ~/.local. Safe to run again.
#
#   ./install.sh              autostart entry (~/.config/autostart), the default
#   ./install.sh --systemd    systemd --user service instead
#   ./install.sh --no-start   neither
#
# On GNOME it also installs the focus extension, on KDE the KWin script. Both are needed on
# Wayland, where those desktops do not tell apps which window is focused.
set -eu
cd "$(dirname "$0")"
mode="${1:---autostart}"
prefix="${PREFIX:-$HOME/.local}"
config="${XDG_CONFIG_HOME:-$HOME/.config}"
data="${XDG_DATA_HOME:-$HOME/.local/share}"
target="${CARGO_TARGET_DIR:-target}"
bin="$prefix/bin/spentday"
# This script runs from apps/linux, so a relative PREFIX would install into the checkout.
case "$prefix" in
  /*) ;;
  *)
    printf 'install.sh cannot install to %s\n' "$prefix" >&2
    printf 'PREFIX must be an absolute path, such as %s/.local.\n' "$HOME" >&2
    exit 2 ;;
esac
# systemd will not run a binary whose path has a quote or a backslash, GIO will not load a
# desktop entry with % in it, and $ would expand in a unit and in a compositor's exec line.
# A tab, a line break or any other control character would break a line in either file.
case "$bin" in
  *[%\$\"\'\\\`]* | *[[:cntrl:]]*)
    printf 'install.sh cannot install to %s\n' "$bin" >&2
    printf 'Desktop entries and systemd units do not work with %%, $, quotes, backslashes, backticks or control characters such as tabs and line breaks in the path. Set PREFIX to another folder.\n' >&2
    exit 2 ;;
esac
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

# Copies desktop entry $1 to $2 with spentday in its Exec key replaced by the installed binary.
install_desktop() {
  EXEC="\"$bin\"" awk 'sub(/^Exec=spentday/, "") { print "Exec=" ENVIRON["EXEC"] $0; next } 1' "$1" > "$tmp/entry"
  install -Dm644 "$tmp/entry" "$2"
}

# The exact line that puts session variables $1 into the systemd user manager, for this desktop.
import_line() {
  case "${XDG_CURRENT_DESKTOP:-}" in
    *sway*) echo "add to your Sway config:  exec systemctl --user import-environment $1" ;;
    *Hyprland*) echo "add to hyprland.conf:  exec-once = dbus-update-activation-environment --systemd $1" ;;
    *) echo "run at login, from your session's startup:  dbus-update-activation-environment --systemd $1" ;;
  esac
}

# spentday.service sees the user manager's environment, not this shell's. GNOME and KDE import
# their variables, Sway, Hyprland and most other compositors only do when configured to.
check_service_environment() {
  manager_env="$(systemctl --user show-environment 2>/dev/null || true)"
  missing=""
  for var in WAYLAND_DISPLAY DISPLAY SWAYSOCK HYPRLAND_INSTANCE_SIGNATURE XDG_CURRENT_DESKTOP; do
    eval "value=\${$var:-}"
    if [ -n "$value" ] && ! printf '%s\n' "$manager_env" | grep -q "^$var="; then
      missing="${missing:+$missing }$var"
    fi
  done
  if [ -n "$missing" ]; then
    echo "Warning: the systemd user manager does not have $missing, so spentday.service cannot see this session." >&2
    echo "To fix it, $(import_line "$missing"), then log in again." >&2
  fi
  if ! systemctl --user is-active -q graphical-session.target; then
    echo "Warning: graphical-session.target is not active, so spentday.service will not start at login." >&2
    echo "Start it from your compositor, or use the autostart line in the README instead." >&2
  fi
}

cargo build --release --locked
install -Dm755 "$target/release/spentday" "$bin"
install_desktop data/day.spent.SpentDay.desktop "$prefix/share/applications/day.spent.SpentDay.desktop"
install -Dm644 data/icons/day.spent.SpentDay.svg "$prefix/share/icons/hicolor/scalable/apps/day.spent.SpentDay.svg"
gtk-update-icon-cache -q -t "$prefix/share/icons/hicolor" 2>/dev/null || true

case "$mode" in
  --autostart)
    install_desktop data/day.spent.SpentDay-autostart.desktop "$config/autostart/day.spent.SpentDay.desktop"
    case "${XDG_CURRENT_DESKTOP:-}" in
      *sway*) printf '%s\n' "Sway does not run autostart entries. Add to your Sway config:  exec '$bin' --background" ;;
      *Hyprland*) printf '%s\n' "Hyprland does not run autostart entries. Add to hyprland.conf:  exec-once = '$bin' --background" ;;
      *) echo "Spent Day starts in the background at login. Run 'spentday --background' to start it now." ;;
    esac
    ;;
  --systemd)
    EXEC="\"$bin\"" awk '/^ExecStart=/ { print "ExecStart=" ENVIRON["EXEC"] " --background"; next } 1' \
      data/spentday.service > "$tmp/spentday.service"
    install -Dm644 "$tmp/spentday.service" "$config/systemd/user/spentday.service"
    systemctl --user daemon-reload
    systemctl --user enable spentday.service
    # Restart rather than start, so a rerun runs the binary just installed.
    systemctl --user restart spentday.service
    echo "Spent Day runs as the systemd user service spentday.service."
    check_service_environment
    ;;
  --no-start) ;;
  *) echo "unknown option $mode" >&2; exit 2 ;;
esac

case "${XDG_CURRENT_DESKTOP:-}" in
  *GNOME*)
    dest="$data/gnome-shell/extensions/spentday@spent.day"
    install -Dm644 gnome-extension/metadata.json "$dest/metadata.json"
    install -Dm644 gnome-extension/extension.js "$dest/extension.js"
    gnome-extensions enable spentday@spent.day 2>/dev/null \
      || echo "Log out and back in, then run: gnome-extensions enable spentday@spent.day"
    ;;
  *KDE*)
    kpackagetool6 --type=KWin/Script --install kwin-script 2>/dev/null \
      || kpackagetool6 --type=KWin/Script --upgrade kwin-script
    kwriteconfig6 --file kwinrc --group Plugins --key spentday-focusEnabled true
    # start() loads every enabled script that is not loaded yet, so an upgrade unloads first.
    qdbus6 org.kde.KWin /Scripting org.kde.kwin.Scripting.unloadScript spentday-focus >/dev/null 2>&1 || true
    qdbus6 org.kde.KWin /Scripting org.kde.kwin.Scripting.start 2>/dev/null \
      || echo "Log out and back in to start the KWin script."
    ;;
esac
printf 'Installed %s\n' "$bin"
