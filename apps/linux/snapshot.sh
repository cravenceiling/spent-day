#!/bin/sh
set -eu
here="$(cd "$(dirname "$0")" && pwd)"
if [ -z "${DBUS_SESSION_BUS_ADDRESS:-}" ]; then
  exec dbus-run-session -- "$0" "$@"
fi
if [ "${1:-}" != --display-ready ]; then
  exec xvfb-run -a -s '-screen 0 1280x900x24' "$0" --display-ready
fi
export GSK_RENDERER=cairo GTK_A11Y=none NO_AT_BRIDGE=1 LANG=C.UTF-8
spentday="${CARGO_TARGET_DIR:-$here/target}/debug/spentday"
shots="$here/screenshots"
logs=$(mktemp -d)
app_pid=
openbox >/dev/null 2>&1 &
wm_pid=$!
cleanup() {
  if [ -n "$app_pid" ]; then kill "$app_pid" 2>/dev/null || true; wait "$app_pid" 2>/dev/null || true; fi
  kill "$wm_pid" 2>/dev/null || true
  cat "$logs/app.log" 2>/dev/null || true
  rm -rf "$logs"
}
trap cleanup EXIT INT TERM
mkdir -p "$shots"
start() {
  ADW_DEBUG_COLOR_SCHEME="$1" "$spentday" --demo >"$logs/app.log" 2>&1 &
  app_pid=$!
  window=$(timeout 15 xdotool search --sync --onlyvisible --all --pid "$app_pid" --name '^Spent Day' | head -1)
  [ -n "$window" ] || { cat "$logs/app.log"; exit 1; }
  xdotool windowsize --sync "$window" 1280 800
  xdotool windowactivate --sync "$window"
  xdotool mousemove --window "$window" 1200 60
  sleep 2
}
shot() {
  # GTK can place a popover outside its parent window.
  if [ "$1" = hover ]; then
    import -window root "$shots/dashboard-hover-demo.png"
    return
  fi
  eval "$(xdotool getwindowgeometry --shell "$window")"
  import -window root -crop "${WIDTH}x${HEIGHT}+${X}+${Y}" +repage "$shots/dashboard-$1-demo.png"
}
start prefer-dark
shot day
xdotool mousemove --window "$window" 150 300
sleep 1
shot hover
xdotool mousemove --window "$window" 100 175 click 1
xdotool mousemove --window "$window" 1200 60
sleep 0.5
shot device
xdotool mousemove --window "$window" 150 300 click 1
xdotool mousemove --window "$window" 1200 60
sleep 0.5
shot sessions
xdotool key Escape Escape
sleep 0.5
xdotool windowsize --sync "$window" 720 560
sleep 0.5
shot small
xdotool windowsize --sync "$window" 1280 800
xdotool key ctrl+Right
sleep 0.5
shot empty
kill "$app_pid"; wait "$app_pid" 2>/dev/null || true
app_pid=
start prefer-light
shot light
cat "$logs/app.log"
